import AppKit
import CoreFoundation

final class TestAPI: PlayerAPI {
    var state = State()
    var sent: [[String: Any]] = []
    func send(_ action: [String: Any]) -> State? { sent.append(action); return state }
    func last(_ operation: String) -> [String: Any]? { sent.last { $0["op"] as? String == operation } }
}

/// Production AppKit views with synthetic data. No browser or live account is
/// loaded. Capture the views themselves; Screen Recording access is unnecessary.
func selfTest() {
    let app = NSApplication.shared; app.setActivationPolicy(.accessory)
    let api = TestAPI()
    api.state.signed_in = true; api.state.account_checking = false
    api.state.account = "Demo · Chrome"; api.state.profile = "chrome:Default"
    api.state.profiles = [Profile(id: "chrome:Default", label: "Chrome · Default")]
    api.state.track = Song(id: "abcdefghijk", title: "Night Drive", artist: "YTfast demo")
    api.state.duration = 240; api.state.position = 62; api.state.playing = true; api.state.format = "Opus · 160 kbps"
    let fixture = Page(key: playlistKey, target: browseTarget(playlistID), title: "Playlists",
        rows: [Row(title: "Late nights", subtitle: "Playlist · 32 songs", browse: browseTarget("VLPLowned"), editable: "PLowned"),
               Row(title: "Focus", subtitle: "Playlist · 85 songs", browse: browseTarget("VLPLfocus"), editable: "PLfocus"),
               Row(title: "New discoveries", subtitle: "Playlist · 48 songs", browse: browseTarget("VLPLshared")),
               Row(title: "Weekend rides", subtitle: "Playlist · 64 songs", browse: browseTarget("VLPLrides"), editable: "PLrides")],
        loading: false, more: false)
    api.state.pages = [fixture]
    let controller = MenuApp(api: api, testing: true)
    controller.refresh()
    precondition(api.sent.isEmpty, "prelaunch wake must not access uncreated views")
    controller.setup(); let status = controller.status
    controller.setup(); precondition(controller.status === status, "exactly one status item")
    controller.refresh()
    let panel = controller.panel!
    let window = NSWindow(contentRect: NSRect(x: 100, y: 100, width: 380, height: 560), styleMask: [.borderless], backing: .buffered, defer: false)
    window.contentView = panel.view; window.appearance = NSAppearance(named: .darkAqua)
    window.makeKeyAndOrderFront(nil); panel.opened(); panel.view.layoutSubtreeIfNeeded()
    precondition(controller.status.menu == nil && controller.popover.contentViewController === panel)
    panel.shuffleButton.performClick(nil)
    precondition(api.last("shuffle") != nil)
    panel.seek.doubleValue = 90; panel.seekChanged(panel.seek)
    precondition(api.last("seek")?["value"] as? Double == 90)

    panel.addSong(nil)
    precondition(panel.rows.count == 3 && !panel.rows.contains { $0.title == "New discoveries" })
    api.state.track = Song(id: "lmnopqrstuv", title: "Next song", artist: "Fixture")
    controller.refresh()
    panel.table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
    panel.activateSelectedRow()
    precondition(api.last("add")?["video"] as? String == "abcdefghijk", "add must use the captured song")
    precondition(api.last("add")?["playlist"] as? String == "PLowned")
    precondition(panel.location.song == nil)

    api.state.pages![0].rows = (0..<1000).map { Row(title: "Playlist \($0)", subtitle: "Fixture", browse: browseTarget("VLPL\($0)"), editable: "PL\($0)") }
    let began = CFAbsoluteTimeGetCurrent()
    controller.refresh(); panel.view.layoutSubtreeIfNeeded()
    let renderMilliseconds = (CFAbsoluteTimeGetCurrent() - began) * 1000
    precondition(panel.table.numberOfRows == 1000)
    var liveRows = 0
    panel.table.enumerateAvailableRowViews { _, _ in liveRows += 1 }
    precondition(liveRows < 40, "AppKit should instantiate visible cells only")
    panel.table.selectRowIndexes(IndexSet(integer: 500), byExtendingSelection: false)
    api.state.pages![0].loading = true; controller.refresh()
    precondition(panel.table.numberOfRows == 1000 && panel.table.selectedRow == 500, "refresh retains rows and selection")
    api.state.pages![0].loading = false; api.state.pages![0].message = "Offline. Showing saved music."
    controller.refresh(); precondition(panel.table.numberOfRows == 1000)
    api.state.loading = true; api.state.playing = false; controller.refresh()
    precondition(panel.playButton.isEnabled && panel.playButton.toolTip == "Pause")
    panel.playButton.performClick(nil)
    precondition(api.last("transport")?["action"] as? String == "toggle")
    api.state.loading = false

    panel.search.stringValue = "test song"; panel.searchNow(nil)
    precondition(panel.location.key == "search:test song:")
    let requests = api.sent.count
    panel.searchNow(nil); precondition(api.sent.count == requests, "unchanged search must not refetch")
    api.state.pages = [fixture]; controller.refresh()
    precondition(panel.location.key == "search:test song:", "late page replies must not navigate")
    panel.back(nil); precondition(panel.location.key == playlistKey)

    api.state.signed_in = false; api.state.account = "Sign in to YouTube Music in your browser"
    api.state.pages = []; controller.refresh()
    precondition(panel.pages.isEmpty && panel.rows.isEmpty && !panel.search.isEnabled && !panel.addButton.isEnabled)
    let account = panel.makeAccountMenu()
    precondition(account.items.contains { $0.title == "Open YouTube Music to sign in" && $0.isEnabled })
    precondition(panel.reconnectButton.isEnabled)
    api.state.account_checking = true; controller.refresh()
    precondition(!panel.reconnectButton.isEnabled && panel.reconnectButton.title == "Connecting…")

    // Artifact screenshots are deliberately synthetic and only written on CI.
    if ProcessInfo.processInfo.environment["GITHUB_ACTIONS"] == "true",
       let directory = ProcessInfo.processInfo.environment["YTFAST_UI_CAPTURE_DIR"] {
        func capture(_ name: String, appearance: NSAppearance.Name) {
            window.appearance = NSAppearance(named: appearance)
            panel.view.layoutSubtreeIfNeeded(); window.displayIfNeeded()
            RunLoop.current.run(until: Date().addingTimeInterval(0.03))
            guard let bitmap = panel.view.bitmapImageRepForCachingDisplay(in: panel.view.bounds) else { preconditionFailure("native capture unavailable") }
            panel.view.cacheDisplay(in: panel.view.bounds, to: bitmap)
            guard let data = bitmap.representation(using: .png, properties: [:]) else { preconditionFailure("native PNG unavailable") }
            try! data.write(to: URL(fileURLWithPath: directory).appendingPathComponent(name))
        }
        api.state.account_checking = false; controller.refresh()
        capture("signed-out-dark.png", appearance: .darkAqua)
        api.state.signed_in = true; api.state.account = "Demo · Chrome"; api.state.pages = [fixture]
        api.state.track = Song(id: "abcdefghijk", title: "Night Drive", artist: "YTfast demo")
        api.state.playing = true; controller.refresh()
        capture("player-dark.png", appearance: .darkAqua)
        capture("player-light.png", appearance: .aqua)
    }
    panel.closed(); window.orderOut(nil)
    let result: [String: Any] = ["result": "pass", "checks": ["prelaunch_wake", "one_status_item", "native_popover", "shuffle", "seek", "editable_only", "captured_song", "1000_scrollable_rows", "visible_cell_reuse", "refresh_preserves_content", "loading_can_pause", "search_dedup", "stale_result_navigation", "signed_out_gating", "actionable_signin", "reconnect_progress"], "render_1000_rows_ms": renderMilliseconds, "instantiated_rows": liveRows]
    print(String(decoding: try! JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys]), as: UTF8.self))
    NSStatusBar.system.removeStatusItem(controller.status)
}
