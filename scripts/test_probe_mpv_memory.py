"""Focused portable checks for the one-off, real-output measurement helper."""
import importlib.util
import io
import json
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch


spec = importlib.util.spec_from_file_location("mpv_probe", Path(__file__).with_name("probe-mpv-memory.py"))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class Clock:
    def __init__(self):
        self.now = 0

    def monotonic(self):
        return self.now

    def sleep(self, seconds):
        self.now += seconds


class ProbeTests(unittest.TestCase):
    def test_ipc_keeps_events_before_command_reply(self):
        ipc = probe.IPC.__new__(probe.IPC)
        ipc.seq, ipc.events, ipc.sock = 0, [], Mock()
        messages = [{"event": "end-file", "reason": "eof"},
                    {"event": "start-file", "playlist_entry_id": 2},
                    {"event": "playback-restart"},
                    {"request_id": 1, "error": "success", "data": 1}]
        ipc.reader = io.BytesIO(b"".join((json.dumps(value) + "\n").encode() for value in messages))
        self.assertEqual(ipc.command("get_property", "playlist-pos"), 1)
        self.assertEqual(ipc.wait_event("playback-restart"), {"event": "playback-restart"})
        self.assertEqual([event["event"] for event in ipc.take_events()], ["end-file", "start-file"])
        self.assertEqual(ipc.take_events(), [])

    def test_production_options_and_only_explicit_output_overrides(self):
        source = (Path(__file__).resolve().parents[1] / "src/mpv.rs").read_text()
        native = probe.native_options(source)
        self.assertIn("--load-positioning=no", native)
        self.assertIn("--load-context-menu=no", native)
        # A future production driver default must not contaminate the A/B arms.
        native += ["--ao=other", "--audio-format=s16"]
        a = probe.real_options(native, "avfoundation", False)
        b = probe.real_options(native, "coreaudio", False)
        c = probe.real_options(native, "coreaudio", True)
        unchanged = lambda args: [arg for arg in args if not arg.startswith(("--ao=", "--audio-format="))]
        self.assertEqual(unchanged(a), unchanged(b))
        self.assertEqual(unchanged(b), unchanged(c))
        self.assertEqual([arg for arg in b if arg.startswith("--ao=")], ["--ao=coreaudio"])
        self.assertEqual([arg for arg in c if arg.startswith("--audio-format=")], ["--audio-format=float"])
        self.assertFalse(any("--ao=null" in arm for arm in (a, b, c)))
        with self.assertRaises(RuntimeError):
            probe.native_options("const NATIVE_OPTIONS: &[&str] = &[dynamic_option()];")

    def test_native_metrics_are_not_rss_or_smoothed_cpu(self):
        self.assertAlmostEqual(probe.cpu_time("01:23.45"), 83.45)
        self.assertAlmostEqual(probe.cpu_time("1-02:03:04.56"), 93784.56)
        self.assertAlmostEqual(probe.footprint_mib("101.3M"), 101.3)
        self.assertEqual(probe.footprint_mib("2048K"), 2)
        self.assertEqual(probe.footprint_mib("1.5G"), 1536)
        with self.assertRaises(RuntimeError):
            probe.footprint_mib(None)

    def test_real_output_fallback_and_changed_bounds_fail(self):
        state = {"current-ao": "coreaudio", "af": [], "volume": 70, "speed": 1,
                 "audio-buffer": 0.2,
                 "audio-params": {"format": "floatp", "samplerate": 48000, "channel-count": 2},
                 "audio-out-params": {"format": "float", "samplerate": 48000, "channel-count": 2},
                 "options": {"audio-device": "auto", "demuxer-max-bytes": 4194304,
                             "demuxer-max-back-bytes": 1048576, "demuxer-readahead-secs": 60,
                             "cache-secs": 60}}
        probe.verify_state(state, "coreaudio", True, True)
        with self.assertRaisesRegex(RuntimeError, "fallback is forbidden"):
            probe.verify_state({**state, "current-ao": "null"}, "coreaudio", True, True)
        with self.assertRaisesRegex(RuntimeError, "float output"):
            probe.verify_state({**state, "audio-out-params": {**state["audio-out-params"], "format": "s16"}},
                               "coreaudio", True, True)
        with self.assertRaisesRegex(RuntimeError, "cache bounds"):
            probe.verify_state({**state, "options": {**state["options"], "cache-secs": 120}},
                               "coreaudio", True, True)

    def test_property_and_audio_reconfig_settle_with_a_bound(self):
        clock, ipc = Clock(), Mock()
        ipc.command.side_effect = [-1, 0, 1]
        with patch.object(probe, "time", clock):
            probe.wait_property(ipc, "playlist-pos", 1)
            self.assertAlmostEqual(clock.now, 0.1)
            ipc.command.side_effect = None
            ipc.command.return_value = False
            with self.assertRaisesRegex(RuntimeError, "within 3 seconds"):
                probe.wait_property(ipc, "idle-active", True)
            with patch.object(probe, "read_real_state", side_effect=[
                    RuntimeError("mpv get_property failed: property unavailable"), {"current-ao": "coreaudio"}]):
                self.assertEqual(probe.real_state(ipc, True)["current-ao"], "coreaudio")

    def test_observation_replenishes_two_entries_and_uses_cpu_deltas(self):
        clock, ipc, record = Clock(), Mock(), {}
        ipc.command.side_effect = lambda *args: 1 if args == ("get_property", "playlist-pos") else False
        ipc.take_events.side_effect = [[], [{"event": "end-file", "reason": "eof"}, {"event": "start-file"}], [], [], []]
        with patch.object(probe, "time", clock), patch.object(probe, "output", side_effect=["00:01.00", "00:01.20"]), \
                patch.object(probe, "real_state", return_value={}), patch.object(probe, "verify_state"), \
                patch.object(probe, "real_sample", return_value={"physical_footprint_mib": 20, "rss_mib": 100}):
            probe.observe(ipc, SimpleNamespace(pid=123), Path("fixture.webm"), "coreaudio", True, 2, "playing", record)
        self.assertEqual(record["natural_eof_events"], 1)
        self.assertEqual(record["successor_start_events"], 1)
        self.assertAlmostEqual(record["cpu_percent_of_one_core"], 10)
        ipc.command.assert_any_call("playlist-remove", 0)
        ipc.command.assert_any_call("loadfile", "fixture.webm", "append")

    def test_non_mac_real_mode_stops_before_running_any_process(self):
        with patch.object(probe.os, "uname", return_value=SimpleNamespace(sysname="Linux")), \
                patch.object(probe.subprocess, "Popen") as start:
            with self.assertRaisesRegex(SystemExit, "requires a Mac"):
                probe.real_probe(180, Path("unused"))
            start.assert_not_called()


if __name__ == "__main__":
    unittest.main()
