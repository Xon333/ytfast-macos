#!/usr/bin/env python3
"""CI-only, account-free comparison of upstream mpv options on one Mac.

No application, account, browser, cache or host settings are changed. Each
variant is a fresh process reading the same generated 256 kbps Opus file.
These are decoder/cache measurements, never live YouTube or audible timing.
"""
import json
import os
from pathlib import Path
import re
import shutil
import socket
import sys
import subprocess
import tempfile
import time

BASE = [
    "--idle=yes", "--no-video", "--no-terminal", "--no-config", "--ytdl=no",
    "--gapless-audio=yes", "--prefetch-playlist=yes", "--cache=yes",
    "--demuxer-max-bytes=4MiB", "--audio-display=no", "--audio-client-name=ytfast",
    "--replaygain=no", "--input-media-keys=no", "--demuxer-max-back-bytes=1MiB",
    "--cache-secs=60", "--demuxer-readahead-secs=60",
]
# First reuse the actual built-in profile, not a reimplementation of it.
# Source: mpv-player/mpv v0.41.0 etc/builtin.conf, [libmpv].
EMBEDDED = ["--profile=libmpv"]
UNUSED = ["--load-scripts=no", "--load-stats-overlay=no", "--load-console=no",
          "--load-osd-console=no", "--load-commands=no", "--load-auto-profiles=no",
          "--load-select=no", "--osd-level=0", "--autoload-files=no"]


def output(*args):
    return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT)


class IPC:
    def __init__(self, path, child):
        self.sock = socket.socket(socket.AF_UNIX)
        deadline = time.monotonic() + 8
        while True:
            try:
                self.sock.connect(str(path))
                break
            except (FileNotFoundError, ConnectionRefusedError):
                if child.poll() is not None or time.monotonic() >= deadline:
                    raise RuntimeError("mpv did not open its IPC socket")
                time.sleep(0.01)
        self.sock.settimeout(8)
        self.reader = self.sock.makefile("rb")
        self.seq = 0

    def message(self):
        line = self.reader.readline()
        if not line:
            raise RuntimeError("mpv closed IPC")
        return json.loads(line)

    def command(self, *args):
        self.seq += 1
        self.sock.sendall((json.dumps({"command": args, "request_id": self.seq}) + "\n").encode())
        while True:
            message = self.message()
            if message.get("request_id") == self.seq:
                if message.get("error") != "success":
                    raise RuntimeError(str(message))
                return message.get("data")

    def close(self):
        self.reader.close()
        self.sock.close()


def sample(pid, destination):
    vm = output("vmmap", "-summary", str(pid))
    destination.write_text(vm)
    rss = int(output("ps", "-p", str(pid), "-o", "rss=").strip())
    footprint = re.search(r"Physical footprint:\s*(.+)", vm)
    peak = re.search(r"Physical footprint \(peak\):\s*(.+)", vm)
    return {"rss_kib": rss, "physical_footprint_raw": footprint.group(1).strip() if footprint else None,
            "peak_footprint_raw": peak.group(1).strip() if peak else None}


def main():
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.uname().sysname != "Darwin":
        raise SystemExit("Run only on the account-free macOS CI runner")
    endurance = "--endurance" in sys.argv
    root = Path("artifacts/memory-probe") / ("endurance" if endurance else "short")
    root.mkdir(parents=True, exist_ok=True)
    mpv = shutil.which("mpv")
    options = output(mpv, "--no-config", "--list-options")
    (root / "mpv-options.txt").write_text(options)
    (root / "upstream-libmpv-profile.txt").write_text(output(mpv, "--no-config", "--show-profile=libmpv"))
    available = {match.group(1) for match in re.finditer(r"^\s*--([a-z0-9-]+)\s", options, re.M)}
    unused = [arg for arg in UNUSED if arg[2:].split("=", 1)[0] in available]
    policies = [("base", []), ("upstream-libmpv", EMBEDDED), ("embedding-no-unused-scripts", EMBEDDED + unused)]
    if endurance:
        policies = [policies[0], policies[-1]]
    result = {"hold_seconds": 180 if endurance else 3, "base_revision": "ca3400f1983e307bd893ce326b7d41612253b5c4", "os": output("sw_vers"),
              "mpv": output(mpv, "--version"), "workload": "600s synthetic 48kHz stereo 256kbps Opus; forced cache; null audio output", "trials": []}
    (root / "available-unused-options.json").write_text(json.dumps(unused, indent=2))
    with tempfile.TemporaryDirectory(prefix="ytfast-mem-") as directory:
        work = Path(directory)
        media = work / "fixture.webm"
        subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i",
                        "sine=frequency=440:sample_rate=48000:duration=600", "-ac", "2", "-c:a",
                        "libopus", "-b:a", "256k", str(media)], check=True)
        for trial in range(1 if endurance else 3):
            ordered = policies if trial % 2 == 0 else list(reversed(policies))
            for name, policy in ordered:
                sock = work / "p.sock"
                sock.unlink(missing_ok=True)
                log = root / f"{name}-{trial}.log"
                with log.open("wb") as errors:
                    start = time.perf_counter()
                    child = subprocess.Popen([mpv, *policy, *BASE, "--ao=null", "--volume=70",
                                              f"--input-ipc-server={sock}"], stdin=subprocess.DEVNULL,
                                             stdout=subprocess.DEVNULL, stderr=errors)
                    ipc = None
                    record = {"policy": name, "trial": trial + 1, "extra_options": policy}
                    try:
                        ipc = IPC(sock, child)
                        record["ipc_ready_ms"] = (time.perf_counter() - start) * 1000
                        time.sleep(0.3)
                        record["idle"] = sample(child.pid, root / f"{name}-{trial}-idle.txt")
                        start = time.perf_counter()
                        ipc.command("loadfile", str(media), "replace")
                        deadline = time.monotonic() + 8
                        while True:
                            message = ipc.message()
                            if message.get("event") == "playback-restart":
                                break
                            if time.monotonic() >= deadline:
                                raise RuntimeError("no playback-restart")
                        record["load_to_restart_ms"] = (time.perf_counter() - start) * 1000
                        ipc.command("loadfile", str(media), "append")
                        time.sleep(180 if endurance else 3)
                        record["playing"] = sample(child.pid, root / f"{name}-{trial}-playing.txt")
                        record["position"] = ipc.command("get_property", "time-pos")
                        record["audio_params"] = ipc.command("get_property", "audio-params")
                        record["cache"] = ipc.command("get_property", "demuxer-cache-state")
                        ipc.command("set_property", "pause", True)
                        time.sleep(0.3)
                        record["paused"] = sample(child.pid, root / f"{name}-{trial}-paused.txt")
                    except Exception as error:
                        record["error"] = str(error)
                    finally:
                        if ipc:
                            try:
                                ipc.command("quit")
                            except Exception:
                                pass
                            ipc.close()
                        try:
                            child.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            child.kill(); child.wait()
                        result["trials"].append(record)
                        (root / "mpv-memory.json").write_text(json.dumps(result, indent=2))
                        print(json.dumps(record), flush=True)
    if any("error" in trial for trial in result["trials"]):
        raise SystemExit("A probe variant failed; inspect evidence before adopting it")


if __name__ == "__main__":
    main()
