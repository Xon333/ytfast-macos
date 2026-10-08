// CI-only harness, compiled beside the unmodified production Swift sources.
// It substitutes ONLY the entry point and account/network responses. It is not
// linked into the distributed app. Quiet callbacks use the real delivery path.
import AppKit
import CoreFoundation

private final class MemoryFixtureAPI: PlayerAPI {
    var payload = Data()
    private let decoder = JSONDecoder()
    func send(_ action: [String: Any]) -> State? {
        try! decoder.decode(State.self, from: payload)
    }
}

private final class MemoryFixture {
    let api = MemoryFixtureAPI()
    var controller: MenuApp!
    var frames: [Data] = []
    var records: [[String: Any]] = []
    var index = 0
    var started = CFAbsoluteTimeGetCurrent()
    let destination: URL

    init(destination: URL) {
        self.destination = destination
        var state = State()
        state.signed_in = true; state.account_checking = false
        state.account = "Synthetic fixture"; state.profile = "fixture"
        state.track = Song(id: "abcdefghijk", title: "Fixture track", artist: "Fixture artist")
        state.playing = true; state.duration = 600; state.format = "Opus 256 kbps"
        let target = encodeTarget("Watch", ["video_id": "abcdefghijk", "playlist_id": NSNull(), "params": NSNull()])
        let rows = (0..<1000).map { Row(title: "Synthetic song \($0)", subtitle: "Synthetic artist", play: target, video: "abcdefghijk") }
        state.pages = (0..<8).map { index in
            let id = index == 0 ? playlistID : "fixture-\(index)"
            return Page(key: "browse:\(id):", target: browseTarget(id), title: "Fixture collection", rows: rows, loading: false, more: false)
        }
        api.payload = try! JSONEncoder().encode(state)
        controller = MenuApp(api: api, testing: false)
        controller.setup(); controller.refresh(); controller.showPopover()
        controller.panel.view.layoutSubtreeIfNeeded()
        controller.popover.performClose(nil)
        state.pages = nil
        frames = (0..<128).map { index in
            state.position = Double(index) * 0.25
            return try! JSONEncoder().encode(state)
        }
        // Consume queued setup/browse work before the measured quiet sequence.
        onMainRunLoop { [self] in onMainRunLoop { [self] in
            started = CFAbsoluteTimeGetCurrent(); record("warmed_closed"); tick()
        } }
    }

    func record(_ phase: String) {
        var sample = memorySample()
        sample["phase"] = phase; sample["callbacks"] = index
        sample["elapsed_ms"] = (CFAbsoluteTimeGetCurrent() - started) * 1000
        sample["table_reloads"] = controller.panel.reloadCount
        records.append(sample)
        precondition(sample["task_info_ok"] as? Bool == true, "memory sample unavailable")
    }

    func tick() {
        onMainRunLoop { [self] in
            index += 1
            api.payload = frames[index % frames.count]
            controller.refresh()
            switch index {
            case 1000: record("1000_closed_callbacks")
            case 5000:
                record("5000_closed_callbacks")
                controller.showPopover()
                controller.panel.view.layoutSubtreeIfNeeded()
            case 6000: record("1000_visible_callbacks")
            case 10000:
                record("5000_visible_callbacks")
                for _ in 0..<20 { controller.popover.performClose(nil); controller.showPopover() }
                controller.popover.performClose(nil)
                onMainRunLoop { [self] in finish() }
                return
            default: break
            }
            if index % 1000 == 0, let bytes = memorySample()["physical_footprint_bytes"] as? UInt64, bytes > 700 * 1024 * 1024 {
                record("safety_cap"); finish(); return
            }
            tick()
        }
    }

    func finish() {
        record("after_20_open_close_cycles")
        let data = try! JSONSerialization.data(withJSONObject: [
            "kind": "synthetic-production-UI-and-JSON-decoding",
            "rows": 8000, "callbacks": index, "samples": records
        ], options: [.prettyPrinted, .sortedKeys])
        try! data.write(to: destination)
        controller.dismissal.stop()
        NSStatusBar.system.removeStatusItem(controller.status)
        exit(0)
    }
}

guard ProcessInfo.processInfo.environment["GITHUB_ACTIONS"] == "true", CommandLine.arguments.count == 2 else {
    fatalError("CI-only memory fixture; no live account access")
}
let memoryApp = NSApplication.shared
memoryApp.setActivationPolicy(.accessory)
private let memoryFixture = autoreleasepool { MemoryFixture(destination: URL(fileURLWithPath: CommandLine.arguments[1])) }
memoryApp.run()
