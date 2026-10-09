#!/usr/bin/env python3
"""Account-free comparison of upstream mpv options on one Mac.

No application, account, browser, cache or host settings are changed. Each
variant is a fresh process reading the same generated 256 kbps Opus file.
The historical default/--endurance modes require CI and use null output.
Explicit --real-audio compares built-in outputs on a Mac; it plays a test tone.
Neither mode measures live YouTube, acoustic onset, dropouts or sound quality.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import socket
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
    return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT, timeout=15)


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
        self.events = []

    def message(self):
        line = self.reader.readline()
        if not line:
            raise RuntimeError("mpv closed IPC")
        return json.loads(line)

    def command(self, *args):
        self.seq += 1
        self.sock.sendall((json.dumps({"command": args, "request_id": self.seq}) + "\n").encode())
        deadline = time.monotonic() + 8
        while True:
            self.sock.settimeout(max(0.001, deadline - time.monotonic()))
            message = self.message()
            if message.get("request_id") == self.seq:
                if message.get("error") != "success":
                    raise RuntimeError(f"mpv {args[0]} failed: {message.get('error')}")
                return message.get("data")
            if "event" in message:
                self.events.append(message)
            if time.monotonic() >= deadline:
                raise RuntimeError("mpv IPC command timed out")

    def wait_event(self, name):
        deadline = time.monotonic() + 8
        while True:
            for index, message in enumerate(self.events):
                if message.get("event") == name:
                    return self.events.pop(index)
                if message.get("event") == "end-file" and message.get("reason") == "error":
                    raise RuntimeError("mpv could not play the generated fixture")
            self.sock.settimeout(max(0.001, deadline - time.monotonic()))
            self.events.append(self.message())
            if time.monotonic() >= deadline:
                raise RuntimeError(f"no {name} within 8 seconds")

    def take_events(self):
        events, self.events = self.events, []
        return events

    def close(self):
        self.reader.close()
        self.sock.close()


def sample(pid, destination=None):
    vm = output("vmmap", "-summary", str(pid))
    if destination is not None:
        destination.write_text(vm)
    rss = int(output("ps", "-p", str(pid), "-o", "rss=").strip())
    footprint = re.search(r"Physical footprint:\s*(.+)", vm)
    peak = re.search(r"Physical footprint \(peak\):\s*(.+)", vm)
    return {"rss_kib": rss, "physical_footprint_raw": footprint.group(1).strip() if footprint else None,
            "peak_footprint_raw": peak.group(1).strip() if peak else None}


def generate_fixture(media, seconds):
    subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i",
                    f"sine=frequency=440:sample_rate=48000:duration={seconds}", "-ac", "2",
                    "-c:a", "libopus", "-b:a", "256k", str(media)], check=True, timeout=60)


def ci_probe(endurance):
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.uname().sysname != "Darwin":
        raise SystemExit("Run only on the account-free macOS CI runner")
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
        generate_fixture(media, 600)
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
                        ipc.wait_event("playback-restart")
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


def native_options(source):
    """Reuse the production policy; fail if it stops being a literal option list."""
    match = re.search(r"const NATIVE_OPTIONS: &\[&str\] = &\[(.*?)\];", source, re.S)
    if not match:
        raise RuntimeError("cannot read production NATIVE_OPTIONS")
    options = []
    for line in match.group(1).splitlines():
        line = line.split("//", 1)[0].strip()
        if not line:
            continue
        if not re.fullmatch(r'"--[^"\\]+",', line):
            raise RuntimeError("NATIVE_OPTIONS is no longer a literal option list")
        options.append(json.loads(line[:-1]))
    if not options:
        raise RuntimeError("production NATIVE_OPTIONS is empty")
    return options


def real_options(native, ao, floating):
    # The built-in AO and (third arm only) sample format are the experiment.
    # Reuse production embedding options and the historical probe's buffer setup.
    policy = [arg for arg in native if not arg.startswith(("--ao=", "--audio-format="))]
    return [*BASE, *policy, f"--ao={ao}", *(["--audio-format=float"] if floating else []),
            "--audio-device=auto", "--audio-buffer=0.2", "--volume=70", "--af="]


def cpu_time(value):
    days, separator, clock = value.strip().rpartition("-")
    seconds = float(days) * 86400 if separator else 0
    fields = clock.split(":")
    if len(fields) not in (2, 3):
        raise ValueError("unrecognized cumulative ps CPU time")
    for index, field in enumerate(reversed(fields)):
        seconds += float(field) * 60 ** index
    return seconds


def footprint_mib(value):
    match = re.fullmatch(r"([\d.]+)\s*([KMGT]?)(?:B)?", value or "", re.I)
    if not match:
        raise RuntimeError("vmmap did not report a recognized physical footprint")
    return float(match.group(1)) * 1024 ** (" KMGT".index(match.group(2).upper() or " ") - 2)


def real_sample(pid):
    # Keep only these numbers. Raw vmmap output, process arguments and logs are
    # not saved by the real-output mode.
    measured = sample(pid)
    return {"rss_mib": measured["rss_kib"] / 1024,
            "physical_footprint_mib": footprint_mib(measured["physical_footprint_raw"]),
            "process_lifetime_peak_mib": footprint_mib(measured["peak_footprint_raw"])}


def wait_property(ipc, name, expected):
    deadline = time.monotonic() + 3
    while ipc.command("get_property", name) != expected:
        if time.monotonic() >= deadline:
            raise RuntimeError(f"{name} did not settle within 3 seconds")
        time.sleep(0.05)


def read_real_state(ipc, active):
    state = {name: ipc.command("get_property", name) for name in
             ("pause", "idle-active", "paused-for-cache", "time-pos") if active or name != "time-pos"}
    if not active:
        return state
    state.update({name: ipc.command("get_property", name) for name in
                  ("current-ao", "volume", "speed", "audio-buffer", "playlist-count", "af")})
    for name in ("audio-params", "audio-out-params"):
        params = ipc.command("get_property", name)
        if not params:
            raise RuntimeError("audio parameters temporarily unavailable")
        state[name] = {key: params.get(key) for key in ("format", "samplerate", "channel-count")}
    cache = ipc.command("get_property", "demuxer-cache-state")
    state["demuxer-cache-state"] = {key: cache.get(key) for key in ("fw-bytes", "total-bytes")}
    state["options"] = {name: ipc.command("get_property", f"options/{name}") for name in
                        ("audio-device", "demuxer-max-bytes", "demuxer-max-back-bytes",
                         "demuxer-readahead-secs", "cache-secs")}
    if not state["current-ao"]:
        raise RuntimeError("audio parameters temporarily unavailable")
    return state


def real_state(ipc, active):
    # A snapshot can land between natural EOF and its successor's audio reconfig.
    deadline = time.monotonic() + 3
    while True:
        try:
            return read_real_state(ipc, active)
        except RuntimeError as error:
            if str(error) not in ("mpv get_property failed: property unavailable",
                                  "audio parameters temporarily unavailable") or time.monotonic() >= deadline:
                raise
            time.sleep(0.05)


def verify_state(state, ao, floating, active):
    if not active:
        if not state["idle-active"]:
            raise RuntimeError("player did not become idle after stop")
        return
    if state["current-ao"] != ao:
        raise RuntimeError(f"requested {ao}, selected {state['current-ao']}; fallback is forbidden")
    for name in ("audio-params", "audio-out-params"):
        params = state[name]
        if params["samplerate"] != 48000 or params["channel-count"] != 2:
            raise RuntimeError(f"{name} no longer matches the 48 kHz stereo fixture")
    if floating and state["audio-out-params"]["format"] != "float":
        raise RuntimeError("the float candidate did not produce float output")
    if state["af"] != [] or state["volume"] != 70 or state["speed"] != 1 or state["audio-buffer"] != 0.2:
        raise RuntimeError("fixed filter, volume, speed or audio-buffer settings changed")
    if state["options"] != {"audio-device": "auto", "demuxer-max-bytes": 4 * 1024 ** 2,
                            "demuxer-max-back-bytes": 1024 ** 2, "demuxer-readahead-secs": 60,
                            "cache-secs": 60}:
        raise RuntimeError("fixed output selection or cache bounds changed")


def observe(ipc, child, media, ao, floating, seconds, phase, record):
    active = phase != "idle"
    expected_pause = phase == "paused"
    started = time.monotonic()
    cpu_start = cpu_time(output("ps", "-p", str(child.pid), "-o", "time="))
    record.update({"phase": phase, "requested_seconds": seconds, "natural_eof_events": 0,
                   "successor_start_events": 0, "samples": []})
    next_sample = started
    while True:
        now = time.monotonic()
        # Commands retain unsolicited events so a quick natural transition is
        # not lost while sampling properties. Keep current + next, as in YTfast.
        paused = ipc.command("get_property", "pause")
        if active and paused != expected_pause:
            raise RuntimeError("pause state changed unexpectedly")
        for event in ipc.take_events():
            if event.get("event") == "end-file":
                if event.get("reason") == "error":
                    raise RuntimeError("fixture playback failed during observation")
                if event.get("reason") == "eof":
                    record["natural_eof_events"] += 1
                    wait_property(ipc, "playlist-pos", 1)
                    ipc.command("playlist-remove", 0)
                    ipc.command("loadfile", str(media), "append")
            elif event.get("event") == "start-file":
                record["successor_start_events"] += 1
        if now >= next_sample or now - started >= seconds:
            state = real_state(ipc, active)
            record["samples"].append({"elapsed_seconds": time.monotonic() - started,
                                      **real_sample(child.pid), "state": state})
            verify_state(state, ao, floating, active)
            next_sample = time.monotonic() + 30
        if now - started >= seconds:
            break
        time.sleep(min(0.5, max(0, started + seconds - time.monotonic())))
    cpu_end = cpu_time(output("ps", "-p", str(child.pid), "-o", "time="))
    record["elapsed_seconds"] = time.monotonic() - started
    record["cpu_seconds"] = cpu_end - cpu_start
    record["cpu_percent_of_one_core"] = 100 * record["cpu_seconds"] / record["elapsed_seconds"]
    return record


def real_probe(seconds, root):
    if os.uname().sysname != "Darwin":
        raise SystemExit("--real-audio requires a Mac with a real audio output")
    if not shutil.which("mpv") or not shutil.which("ffmpeg"):
        raise SystemExit("Existing mpv and ffmpeg are required; no dependencies will be installed")
    mpv = str(Path(shutil.which("mpv")).resolve())
    source = Path(__file__).resolve().parents[1] / "src/mpv.rs"
    native = native_options(source.read_text())
    result = {"mode": "explicit real audio output; generated local fixture", "os": output("sw_vers"),
              "mpv": output(mpv, "--no-config", "--version"),
              "mpv_binary_sha256": hashlib.sha256(Path(mpv).read_bytes()).hexdigest(),
              "source_revision": output("git", "-C", str(source.parent), "rev-parse", "HEAD").strip(),
              "native_options": native,
              "native_options_sha256": hashlib.sha256("\n".join(native).encode()).hexdigest(),
              "workload": "30s synthetic 440Hz 48kHz stereo 256kbps Opus; current + next; fresh process per arm",
              "device": "auto; keep the same physical output and system volume for every arm",
              "limits": ["Not live YouTube, app memory or a long-lived-session reproduction.",
                         "CPU uses cumulative process time; sampling and IPC add overhead.",
                         "RSS, physical footprint and process-lifetime peak are separate metrics.",
                         "Playback events do not establish acoustic onset, dropouts or quality.",
                         "Device changes, normalization and native media controls are not tested.",
                         "One trial per arm in fixed order; repeat before inferring an improvement."],
              "trials": []}
    root.mkdir(parents=True, exist_ok=True)
    destination = root / "mpv-real-audio.json"
    print("Real-output test plays a 440 Hz tone at mpv volume 70. Keep the output device fixed.", flush=True)
    with tempfile.TemporaryDirectory(prefix="ytfast-ao-") as directory:
        work = Path(directory)
        media = work / "fixture.webm"
        generate_fixture(media, 30)
        for name, ao, floating in (("avfoundation", "avfoundation", False),
                                   ("coreaudio", "coreaudio", False),
                                   ("coreaudio-float", "coreaudio", True)):
            sock = work / "p.sock"
            sock.unlink(missing_ok=True)
            options = real_options(native, ao, floating)
            record = {"arm": name, "options": options, "phases": []}
            child, ipc = None, None
            print(f"Starting {name}", flush=True)
            try:
                child = subprocess.Popen([mpv, *options, f"--input-ipc-server={sock}"],
                                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                         stderr=subprocess.DEVNULL)
                ipc = IPC(sock, child)
                ipc.command("loadfile", str(media), "replace")
                ipc.wait_event("playback-restart")
                ipc.command("loadfile", str(media), "append")
                record["initial_audio_state"] = real_state(ipc, True)
                verify_state(record["initial_audio_state"], ao, floating, True)
                ipc.take_events()  # Exclude the explicitly loaded first file.
                for phase, duration in (("playing", seconds), ("paused", 15), ("resumed", 10), ("idle", 15)):
                    if phase in ("paused", "resumed"):
                        ipc.command("set_property", "pause", phase == "paused")
                    elif phase == "idle":
                        ipc.command("stop")
                        wait_property(ipc, "idle-active", True)
                        ipc.take_events()
                    observed = {}
                    record["phases"].append(observed)
                    observe(ipc, child, media, ao, floating, duration, phase, observed)
                    print(f"{name} {phase}: {observed['cpu_percent_of_one_core']:.2f}% of one core", flush=True)
                if record["phases"][0]["successor_start_events"] < 2:
                    raise RuntimeError("fewer than two natural successor starts were observed")
                paused_samples = record["phases"][1]["samples"]
                if abs(paused_samples[-1]["state"]["time-pos"] - paused_samples[0]["state"]["time-pos"]) > 0.2:
                    raise RuntimeError("playback position advanced while paused")
                resumed = record["phases"][2]
                resumed_samples = resumed["samples"]
                if resumed["natural_eof_events"] == 0 and (
                        resumed_samples[-1]["state"]["time-pos"] - resumed_samples[0]["state"]["time-pos"] < 1):
                    raise RuntimeError("playback position did not advance after resume")
            except Exception as error:
                record["error"] = str(error)
            finally:
                if ipc:
                    try:
                        ipc.command("quit")
                    except Exception:
                        pass
                    ipc.close()
                if child:
                    try:
                        child.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait()
                result["trials"].append(record)
                destination.write_text(json.dumps(result, indent=2) + "\n")
    avf, _, floating = result["trials"]
    result["float_output_matches_avfoundation"] = (
        avf.get("initial_audio_state", {}).get("audio-out-params") ==
        floating.get("initial_audio_state", {}).get("audio-out-params")
    ) if "initial_audio_state" in avf and "initial_audio_state" in floating else None
    destination.write_text(json.dumps(result, indent=2) + "\n")
    print(f"Whitelisted results: {destination}", flush=True)
    if any("error" in trial for trial in result["trials"]):
        raise SystemExit("One or more real-output arms failed; no output fallback was used")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--endurance", action="store_true", help="historical CI-only 180s null-output comparison")
    mode.add_argument("--real-audio", action="store_true", help="explicit Mac AVFoundation/CoreAudio/float comparison")
    parser.add_argument("--seconds", type=int, help="real-output playback seconds per arm, 90–600 (default 180)")
    parser.add_argument("--output", type=Path, help="real-output result directory (default artifacts/memory-probe/real-audio)")
    args = parser.parse_args()
    if not args.real_audio:
        if args.seconds is not None or args.output is not None:
            parser.error("--seconds and --output require --real-audio")
        ci_probe(args.endurance)
    else:
        seconds = args.seconds if args.seconds is not None else 180
        if not 90 <= seconds <= 600:
            parser.error("--seconds must be between 90 and 600")
        real_probe(seconds, args.output or Path("artifacts/memory-probe/real-audio"))


if __name__ == "__main__":
    main()
