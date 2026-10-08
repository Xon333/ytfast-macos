import AppKit
import CoreFoundation
import Darwin

final class TestAPI: PlayerAPI {
    var state = State()
    var sent: [[String: Any]] = []
    func send(_ action: [String: Any]) -> State? { sent.append(action); return state }
    func last(_ operation: String) -> [String: Any]? { sent.last { $0["op"] as? String == operation } }
}

/// Keep assertion failures readable in optimized native builds.
private func verify(_ condition: @autoclosure () -> Bool, _ message: String = "Assertion failed", line: UInt = #line) {
    guard condition() else {
        fputs("Native check failed at SelfTest.swift:\(line): \(message)\n", stderr)
        exit(1)
    }
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
    let playlistStart = encodeTarget("Watch", ["video_id": NSNull(), "playlist_id": "PLowned", "params": NSNull()])
    let fixture = Page(key: playlistKey, target: browseTarget(playlistID), title: "Playlists",
        rows: [Row(title: "Late nights", subtitle: "Playlist · 32 songs", play: playlistStart, browse: browseTarget("VLPLowned"), editable: "PLowned"),
               Row(title: "Focus", subtitle: "Playlist · 85 songs", browse: browseTarget("VLPLfocus"), editable: "PLfocus"),
               Row(title: "New discoveries", subtitle: "Playlist · 48 songs", browse: browseTarget("VLPLshared")),
               Row(title: "Weekend rides", subtitle: "Playlist · 64 songs", browse: browseTarget("VLPLrides"), editable: "PLrides")],
        loading: false, more: false)
    api.state.pages = [fixture]
    let controller = MenuApp(api: api, testing: true)
    controller.refresh()
    verify(api.sent.isEmpty, "prelaunch wake must not access uncreated views")
    controller.setup(); let status = controller.status
    controller.setup(); verify(controller.status === status, "exactly one status item")
    controller.refresh()
    let panel = controller.panel!
    let window = NSWindow(contentRect: NSRect(x: 100, y: 100, width: PlayerPanel.width, height: 400), styleMask: [.borderless], backing: .buffered, defer: false)
    panel.sizeChanged = { [weak window] size in window?.setContentSize(size) }
    window.contentView = panel.view; window.appearance = NSAppearance(named: .darkAqua)
    window.makeKeyAndOrderFront(nil); panel.opened(); panel.view.layoutSubtreeIfNeeded()
    // NSButton.performClick runs AppKit's event loop. Drain the connection's
    // already-queued playlist warm-up before measuring this particular action.
    // The barrier stops this one run-loop turn; it does not sleep or poll.
    var startupSettled = false
    onMainRunLoop { startupSettled = true; CFRunLoopStop(CFRunLoopGetMain()) }
    _ = RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(1))
    verify(startupSettled, "initial native callbacks did not reach the run-loop barrier")
    verify(controller.status.menu == nil && controller.popover.contentViewController === panel)
    let playerLibraryHeight = panel.preferredContentSize.height
    let collectionCell = panel.table.view(atColumn: 0, row: 0, makeIfNecessary: true) as! MusicCell
    collectionCell.showActions(true)
    let collectionBrowses = api.sent.filter { $0["op"] as? String == "browse" }.count
    collectionCell.playActionButton.performClick(nil)
    verify(api.last("play")?["target"] as? String == playlistStart && panel.location.key == playlistKey, "inline playlist Play must not open the collection first")
    let afterPlayBrowses = api.sent.filter { $0["op"] as? String == "browse" }
    verify(afterPlayBrowses.count == collectionBrowses, "inline Play dispatched unexpected browsing: \(afterPlayBrowses.dropFirst(collectionBrowses))")
    let nestedFixture = Page(key: "browse:VLPLowned:", target: browseTarget("VLPLowned"), title: "Late nights", rows: fixture.rows, play: playlistStart, loading: false, more: false)
    api.state.pages = [fixture, nestedFixture]; controller.refresh()
    panel.navigate(Location.from(nestedFixture.target, title: nestedFixture.title)!)
    let collectionHeight = panel.preferredContentSize.height
    verify(collectionHeight == playerLibraryHeight, "collection navigation must replace the sections row instead of adding another row")
    verify(panel.sections.isHidden && !panel.pagePlay.isHidden, "collection navigation at \(panel.location.key): sectionsHidden=\(panel.sections.isHidden), playHidden=\(panel.pagePlay.isHidden), pageHasPlay=\(panel.pages[panel.location.key]?.play != nil)")
    panel.back(nil); api.state.pages = [fixture]; controller.refresh()
    verify(panel.makeMoreMenu().items.first?.state == .on)
    panel.toggleNormalization(nil)
    verify(api.last("normalize")?["enabled"] as? Bool == false)
    api.state.normalize = false; controller.refresh()
    verify(panel.makeMoreMenu().items.first?.state == .off)
    api.state.normalize = true; controller.refresh()
    panel.shuffleButton.performClick(nil)
    verify(api.last("shuffle") != nil)
    panel.seek.doubleValue = 90; panel.seekChanged(panel.seek)
    verify(api.last("seek")?["value"] as? Double == 90)

    panel.addSong(nil)
    verify(panel.rows.count == 3 && !panel.rows.contains { $0.title == "New discoveries" })
    api.state.track = Song(id: "lmnopqrstuv", title: "Next song", artist: "Fixture")
    controller.refresh()
    panel.table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
    panel.activateSelectedRow()
    verify(api.last("add")?["video"] as? String == "abcdefghijk", "add must use the captured song")
    verify(api.last("add")?["playlist"] as? String == "PLowned")
    verify(panel.location.song == nil)

    api.state.pages![0].rows = (0..<1000).map { Row(title: "Playlist \($0)", subtitle: "Fixture", browse: browseTarget("VLPL\($0)"), editable: "PL\($0)") }
    let began = CFAbsoluteTimeGetCurrent()
    controller.refresh(); panel.view.layoutSubtreeIfNeeded()
    let renderMilliseconds = (CFAbsoluteTimeGetCurrent() - began) * 1000
    verify(panel.table.numberOfRows == 1000)
    var liveRows = 0
    panel.table.enumerateAvailableRowViews { _, _ in liveRows += 1 }
    verify(liveRows > 0 && liveRows < 40, "AppKit instantiated \(liveRows) rows; expected 1–39 visible cells")
    panel.table.selectRowIndexes(IndexSet(integer: 500), byExtendingSelection: false)
    api.state.pages![0].loading = true; controller.refresh()
    verify(panel.table.numberOfRows == 1000 && panel.table.selectedRow == 500, "refresh retains rows and selection")
    api.state.pages![0].loading = false; api.state.pages![0].message = "Offline. Showing saved music."
    controller.refresh(); verify(panel.table.numberOfRows == 1000)
    api.state.loading = true; api.state.playing = false; controller.refresh()
    verify(panel.playButton.isEnabled && panel.playButton.toolTip == "Cancel loading")
    panel.playButton.performClick(nil)
    verify(api.last("transport")?["action"] as? String == "toggle")
    api.state.loading = false

    panel.search.stringValue = "test song"; panel.searchNow(nil)
    verify(panel.location.key == "search:test song:")
    let requests = api.sent.count
    panel.searchNow(nil); verify(api.sent.count == requests, "unchanged search must not refetch")
    api.state.pages = [fixture]; controller.refresh()
    verify(panel.location.key == "search:test song:", "late page replies must not navigate")
    panel.back(nil); verify(panel.location.key == playlistKey)

    api.state.signed_in = false; api.state.account = "Sign in to YouTube Music in your browser"
    api.state.pages = []; controller.refresh()
    verify(panel.pages.isEmpty && panel.rows.isEmpty && !panel.search.isEnabled && !panel.addButton.isEnabled)
    let account = panel.makeMoreMenu()
    verify(account.items.contains { $0.title == "Open YouTube Music" && $0.isEnabled })
    verify(panel.reconnectButton.isEnabled)
    api.state.account_checking = true; controller.refresh()
    verify(!panel.reconnectButton.isEnabled && panel.state.account_checking && panel.reconnectButton.title == "Connecting…")
    verify(!panel.nextButton.isEnabled && !panel.previousButton.isEnabled)

    verify(panel.profilePicker.isHidden, "a single profile is a label, not a disabled picker")
    api.state.account_checking = false; api.state.profile = "chrome:Missing"; controller.refresh()
    verify(!panel.profilePicker.isHidden && panel.profilePicker.selectedItem?.isEnabled == false, "an unavailable selection must not look like another account")
    api.state.profile = "chrome:Default"; controller.refresh()

    // Stable navigation and selection must survive ordinary position updates
    // and closing/reopening. Only changed rows cause a table reload.
    api.state.signed_in = true; api.state.account_checking = false
    api.state.account = "Demo · Chrome"; api.state.pages = [fixture]
    controller.refresh(); panel.navigate(.library(0), remember: false)
    let warmReloads = panel.reloadCount
    let warmBegan = CFAbsoluteTimeGetCurrent()
    for _ in 0..<30 { panel.closed(); panel.opened() }
    let warmOpenMilliseconds = (CFAbsoluteTimeGetCurrent() - warmBegan) * 1000 / 30
    let warmReloadDelta = panel.reloadCount - warmReloads
    verify(warmReloadDelta == 0, "warm open must not rebuild unchanged rows")
    api.state.pages = nil
    for _ in 0..<100 { api.state.position += 0.25; controller.refresh() }
    verify(panel.reloadCount == warmReloads, "position ticks must not rebuild the library")

    panel.search.stringValue = "find album"; panel.searchNow(nil)
    let searchLocation = panel.location
    let album = Location.from(browseTarget("MPREfixture"), title: "An album")!
    panel.navigate(album)
    panel.closed(); panel.opened()
    verify(panel.location.key == album.key, "reopening a search destination must not rerun the old query")
    verify(panel.sections.selectedSegment == -1, "search descendants must not claim to be a library section")
    panel.back(nil)
    verify(panel.location.key == searchLocation.key && panel.search.stringValue == "find album", "Back restores the query")
    verify(panel.sections.selectedSegment == -1, "search results are not a library section")
    panel.back(nil)
    verify(panel.location.key == playlistKey)
    verify(panel.table.action == nil, "selection and arrow keys must never dispatch playback")

    // Seek and volume must resist stale backend echoes during AppKit tracking.
    api.state.loading = false; api.state.track = Song(id: "abcdefghijk", title: "Track one", artist: "Demo")
    controller.refresh()
    panel.seek.beginEditing(); panel.seek.doubleValue = 100
    api.state.position = 3; controller.refresh()
    verify(panel.seek.doubleValue == 100)
    let seeksBeforeChange = api.sent.filter { $0["op"] as? String == "seek" }.count
    api.state.track = Song(id: "lmnopqrstuv", title: "Track two", artist: "Demo"); controller.refresh()
    panel.seek.endEditing()
    verify(api.sent.filter { $0["op"] as? String == "seek" }.count == seeksBeforeChange, "a drag begun on the previous song must not seek its successor")
    panel.volume.beginEditing(); panel.volume.doubleValue = 32; panel.volumeChanged(panel.volume)
    api.state.volume = 70; controller.refresh()
    verify(panel.volume.doubleValue == 32)
    panel.volume.endEditing()
    api.state.volume = 32; controller.refresh()
    panel.mute(nil); verify(api.last("volume")?["value"] as? Double == 0)
    api.state.volume = 0; controller.refresh()
    panel.mute(nil); verify(api.last("volume")?["value"] as? Double == 32)

    // Arrow navigation selects; Return activates, and an already-playing song
    // uses transport instead of another YouTube queue request.
    let songTarget = encodeTarget("Watch", ["video_id": "lmnopqrstuv", "playlist_id": NSNull(), "params": NSNull()])
    api.state.pages = [Page(key: playlistKey, target: browseTarget(playlistID), title: "Playlists", rows: [Row(title: "Track two", subtitle: "Demo", play: songTarget, video: "lmnopqrstuv")], loading: false, more: false)]
    controller.refresh(); panel.table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
    let playsBefore = api.sent.filter { $0["op"] as? String == "play" }.count
    panel.activateSelectedRow()
    verify(api.sent.filter { $0["op"] as? String == "play" }.count == playsBefore)
    verify(api.last("transport")?["action"] as? String == "toggle")
    let trackCell = panel.table.view(atColumn: 0, row: 0, makeIfNecessary: true) as! MusicCell
    trackCell.showActions(true)
    verify(!trackCell.playActionButton.isHidden && !trackCell.addActionButton.isHidden)
    let staleRowAction = panel.makeRowMenu(0)!.items.first!
    trackCell.addActionButton.performClick(nil)
    verify(panel.location.song?.id == "lmnopqrstuv", "row Add must capture that song")
    api.state.track = Song(id: "newcurrent01", title: "Playback moved on", artist: "Demo"); controller.refresh()
    verify(panel.location.song?.id == "lmnopqrstuv", "the captured row song must survive playback advancing")
    panel.back(nil)
    api.state.signed_in = false; api.state.pages = []; controller.refresh()
    api.state.signed_in = true; api.state.pages = [fixture]; controller.refresh()
    let requestsBeforeStaleAction = api.sent.count
    panel.rowMenuAction(staleRowAction)
    verify(api.sent.count == requestsBeforeStaleAction, "a context-menu action must not survive an account boundary")
    let editor = NSTextView()
    verify(panel.control(panel.search, textView: editor, doCommandBy: NSSelectorFromString("insertTab:")))
    verify(window.firstResponder === panel.table, "Tab from search must enter the music list")
    panel.showAccount(nil); verify(panel.showingAccount && panel.search.isHidden)
    panel.focusSearch(); verify(!panel.showingAccount && !panel.search.isHidden)
    verify(PlayerPanel.searchDelay == 0.18)
    api.state.track = nil; api.state.pages = nil; controller.refresh()
    let idleHeight = panel.preferredContentSize.height
    api.state.loading = true; controller.refresh()
    verify(!panel.playButton.isHidden && panel.preferredContentSize.height > idleHeight, "a first-ever pending start must expose Cancel")
    api.state.loading = false; controller.refresh()

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
        api.state.account_checking = false; api.state.signed_in = false
        api.state.track = nil; api.state.playing = false; api.state.loading = false
        api.state.account = "Sign in to YouTube Music in Chrome"; controller.refresh()
        capture("signed-out-dark.png", appearance: .darkAqua)
        api.state.account_checking = true; controller.refresh()
        capture("connecting-dark.png", appearance: .darkAqua)
        api.state.account_checking = false; api.state.account = "Operation not permitted. Full Disk Access is required."; controller.refresh()
        capture("permission-dark.png", appearance: .darkAqua)
        api.state.account_unverified = true; api.state.account = "Can't reach YouTube Music (connection timed out)"; controller.refresh()
        capture("unverified-dark.png", appearance: .darkAqua)
        api.state.account_unverified = false
        api.state.signed_in = true; api.state.account = "Demo · Chrome"; api.state.pages = [fixture]
        api.state.track = Song(id: "abcdefghijk", title: "Night Drive", artist: "YTfast demo")
        api.state.playing = true; api.state.position = 62; api.state.volume = 70
        api.state.format = "Opus 256 kbps · Premium (itag 774)"; controller.refresh()
        capture("player-dark.png", appearance: .darkAqua)
        capture("player-light.png", appearance: .aqua)
        verify(panel.sections.frame.width >= 280, "root library sections must fill the available row")
        panel.table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
        capture("player-actions-dark.png", appearance: .darkAqua)
        panel.table.deselectAll(nil)
        api.state.profiles.append(Profile(id: "brave:Default", label: "Brave · Default")); controller.refresh()
        panel.showAccount(nil); capture("account-dark.png", appearance: .darkAqua)
        panel.showAccount(nil)
        api.state.pages = [fixture, Page(key: "browse:VLPLowned:", target: browseTarget("VLPLowned"), title: "Late nights", rows: (0..<8).map { Row(title: "Song \($0 + 1)", subtitle: "Artist name", play: songTarget, video: "fixture\($0)") }, play: songTarget, loading: false, more: false)]
        controller.refresh(); panel.navigate(Location.from(browseTarget("VLPLowned"), title: "Late nights")!)
        capture("playlist-dark.png", appearance: .darkAqua)
        verify(panel.sections.isHidden && !panel.pagePlay.isHidden, "nested navigation must keep collection playback accessible")
        verify(panel.reconnectButton.frame.height == 28 && panel.reconnectButton.frame.width >= 100, "Connect needs a clear native hit area")
        for control in [panel.playButton, panel.previousButton, panel.nextButton, panel.shuffleButton, panel.addButton] {
            let frame = control.convert(control.bounds, to: panel.view)
            verify(panel.view.bounds.contains(frame) && frame.width >= 28 && frame.height >= 28, "transport hit areas must be visible and usable")
        }
    }
    panel.closed(); window.orderOut(nil)
    let result: [String: Any] = ["result": "pass", "checks": ["prelaunch_wake", "one_status_item", "native_popover", "shuffle", "seek", "editable_only", "captured_song", "1000_scrollable_rows", "visible_cell_reuse", "refresh_preserves_content", "loading_can_pause", "search_dedup", "stale_result_navigation", "signed_out_gating", "actionable_signin", "reconnect_progress", "connecting_transport_gating", "single_profile_label", "missing_profile_not_substituted", "first_load_exposes_cancel", "warm_open_no_reload", "position_ticks_no_reload", "reopen_preserves_destination", "back_restores_search", "search_section_state", "selection_is_not_playback", "seek_tracking_stability", "seek_accounting_for_track_change", "volume_tracking_stability", "mute_restores_volume", "current_song_uses_transport", "inline_account", "search_focus_from_account", "control_hit_areas", "search_descendant_section_state", "library_section_sizing", "connect_hit_area", "inline_collection_play", "single_collection_navigation_row", "normalization_state_dispatch", "row_add_captures_song", "stale_row_action_account_boundary", "search_tab_navigation"], "warm_open_ms": warmOpenMilliseconds, "warm_open_reloads": warmReloadDelta, "render_1000_rows_ms": renderMilliseconds, "instantiated_rows": liveRows, "player_library_height_pt": playerLibraryHeight, "collection_height_pt": collectionHeight]
    print(String(decoding: try! JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys]), as: UTF8.self))
    NSStatusBar.system.removeStatusItem(controller.status)
}
