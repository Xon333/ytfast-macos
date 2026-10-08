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
    verify(!panel.browserExpanded && panel.rows.isEmpty && panel.search.isHidden, "first open must be compact")
    verify(api.sent.allSatisfy { $0["op"] as? String != "browse" }, "startup must not warm hidden catalogues")
    panel.selectLibrary(0); panel.view.layoutSubtreeIfNeeded()
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
    verify(collectionHeight == playerLibraryHeight + 38, "only the collection breadcrumb/actions should add a row")
    verify(!panel.destinations.isHidden && !panel.pagePlay.isHidden && !panel.pageShuffle.isHidden, "collection navigation at \(panel.location.key): playHidden=\(panel.pagePlay.isHidden), pageHasPlay=\(panel.pages[panel.location.key]?.play != nil)")
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
    panel.collapseBrowser()
    let warmReloads = panel.reloadCount
    let warmBegan = CFAbsoluteTimeGetCurrent()
    for _ in 0..<30 { panel.closed(); panel.opened() }
    let warmOpenMilliseconds = (CFAbsoluteTimeGetCurrent() - warmBegan) * 1000 / 30
    let warmReloadDelta = panel.reloadCount - warmReloads
    verify(warmReloadDelta == 0, "warm open must not rebuild unchanged rows")
    api.state.pages = nil
    for _ in 0..<100 { api.state.position += 0.25; controller.refresh() }
    verify(panel.reloadCount == warmReloads, "position ticks must not rebuild the library")

    panel.selectLibrary(0)
    panel.search.stringValue = "find album"; panel.searchNow(nil)
    let searchLocation = panel.location
    let album = Location.from(browseTarget("MPREfixture"), title: "An album")!
    panel.navigate(album)
    panel.closed(); panel.opened()
    verify(panel.location.key == album.key, "reopening a search destination must not rerun the old query")
    verify(!panel.browserExpanded && panel.libraryButtons.allSatisfy { !$0.isOn }, "reopening a collection stays collapsed")
    panel.back(nil)
    verify(panel.location.key == searchLocation.key && panel.search.stringValue == "find album", "Back restores the query")
    verify(panel.searchButton.isOn && panel.libraryButtons.allSatisfy { !$0.isOn }, "search results are not a library section")
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
    panel.focusSearch(); panel.search.stringValue = "fixture"; panel.searchNow(nil)
    let tabLocation = panel.location
    api.state.pages = [Page(key: tabLocation.key, target: tabLocation.target, title: "Search", rows: fixture.rows, loading: false, more: false)]
    controller.refresh()
    let editor = NSTextView()
    verify(!panel.search.isHidden && !panel.rows.isEmpty)
    verify(panel.control(panel.search, textView: editor, doCommandBy: NSSelectorFromString("insertTab:")))
    verify(window.firstResponder === panel.table, "Tab from search must enter the music list")
    panel.showAccount(nil); verify(panel.showingAccount && panel.search.isHidden)
    panel.focusSearch(); verify(!panel.showingAccount && !panel.search.isHidden)
    verify(PlayerPanel.searchDelay == 0.18)
    api.state.track = nil; api.state.pages = nil; controller.refresh()
    let idleHeight = panel.preferredContentSize.height
    api.state.loading = true; controller.refresh()
    verify(!panel.playButton.isHidden && panel.playButton.isEnabled && panel.preferredContentSize.height == idleHeight, "a pending start must expose Cancel without resizing")
    api.state.loading = false; controller.refresh()

    // Drive text changes synchronously; no sleeps or assumptions about the
    // debounce clock. Return explicitly submits the final committed draft.
    func browseCount() -> Int { api.sent.filter { $0["op"] as? String == "browse" }.count }
    func typeSearch(_ value: String) {
        panel.search.stringValue = value
        panel.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: panel.search))
    }
    func libraryRoot() {
        window.makeFirstResponder(panel.table)
        api.state.pages = [fixture]; controller.refresh()
        panel.selectLibrary(0)
    }
    func searchPage(_ location: Location, rows: [Row] = [], loading: Bool = false) -> Page {
        Page(key: location.key, target: location.target, title: "Search", rows: rows, loading: loading, more: false)
    }
    api.state.track = Song(id: "abcdefghijk", title: "Night Drive", artist: "YTfast demo")
    api.state.playing = true; api.state.duration = 240; api.state.position = 62
    libraryRoot()
    panel.focusSearch()
    // Entering Search is one deliberate expansion. Loading/results then stay stable.
    typeSearch("D")
    let searchHeight = panel.preferredContentSize.height
    let beforeTyping = browseCount()
    var draft = ""
    for character in "Daft Punk " {
        draft.append(character); typeSearch(draft)
        verify(panel.search.stringValue == draft && panel.location.searchText == draft, "typing must retain spaces in the raw search draft")
        verify(panel.preferredContentSize.height == searchHeight && panel.rows.isEmpty, "pending search must keep its viewport without actionable stale rows")
    }
    verify(browseCount() == beforeTyping, "typing must wait for debounce or explicit submission")
    panel.searchNow(nil)
    verify(panel.location.query == "Daft Punk" && browseCount() == beforeTyping + 1, "Return submits the normalized final query once")
    let rawSearchLocation = panel.location
    let resultRows = (0..<7).map { Row(title: "Result \($0 + 1)", subtitle: "Daft Punk", play: songTarget, video: "fixture\($0)") }
    for page in [searchPage(rawSearchLocation, loading: true), searchPage(rawSearchLocation, rows: resultRows), searchPage(rawSearchLocation)] {
        api.state.pages = [fixture, page]; controller.refresh()
        verify(panel.preferredContentSize.height == searchHeight, "search loading, results and empty states must retain the same viewport")
        verify(panel.search.stringValue == draft, "page snapshots must not normalize the field editor")
    }
    verify(panel.refreshButton.isEnabled && !panel.refreshButton.isHidden, "settled empty results must stop loading")
    panel.navigate(album); panel.back(nil)
    verify(panel.search.stringValue == "Daft Punk ", "Back must restore the exact search draft, including spaces")

    // Exercise AppKit's marked-text path without a particular keyboard layout
    // or input method. Composing text and its editor must survive a snapshot.
    libraryRoot(); panel.focusSearch()
    guard let compositionEditor = panel.search.currentEditor() as? NSTextView else { preconditionFailure("search field editor unavailable") }
    let beforeComposition = browseCount(), composingLocation = panel.location.key
    compositionEditor.setMarkedText("とう", selectedRange: NSRange(location: 2, length: 0), replacementRange: NSRange(location: 0, length: compositionEditor.string.utf16.count))
    verify(compositionEditor.hasMarkedText(), "synthetic composition must establish marked text")
    let composingText = compositionEditor.string
    panel.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: panel.search))
    controller.refresh()
    verify(compositionEditor.hasMarkedText() && compositionEditor.string == composingText, "backend snapshots must preserve active composition")
    verify(panel.location.key == composingLocation && browseCount() == beforeComposition, "marked text must not navigate or submit a partial search")
    verify(!panel.control(panel.search, textView: compositionEditor, doCommandBy: NSSelectorFromString("insertNewline:")), "Return belongs to the input method while text is marked")
    compositionEditor.unmarkText(); typeSearch("東京"); panel.searchNow(nil)
    verify(panel.location.query == "東京" && browseCount() == beforeComposition + 1, "committed composition must submit once")

    // Account hides/cancels the debounce, but both ways back must resume it.
    for focus in [false, true] {
        libraryRoot(); typeSearch(focus ? "resume from focus" : "resume from back")
        let pendingLocation = panel.location
        panel.showAccount(nil)
        verify(panel.showingAccount, "interrupted search fixture must show Account")
        let beforeResume = browseCount()
        if focus { panel.focusSearch() } else { panel.back(nil) }
        verify(!panel.showingAccount && panel.location.key == pendingLocation.key && browseCount() == beforeResume + 1, "leaving Account must resume the pending query exactly once")
        api.state.pages = [fixture, searchPage(pendingLocation, rows: [resultRows[0]])]; controller.refresh()
        verify(panel.refreshButton.isEnabled && !panel.refreshButton.isHidden, "resumed search must clear its loading state when results arrive")
    }

    // Command-F leaves Add without a write; availability changes update the
    // existing visible buttons even when the song and playback stay unchanged.
    libraryRoot()
    let likedLocation = Location.library(1)
    let likedPage = Page(key: likedLocation.key, target: likedLocation.target, title: "Liked Music", rows: [Row(title: "Night Drive", subtitle: "YTfast demo", play: songTarget, video: "abcdefghijk")], loading: false, more: false)
    api.state.pages = [fixture, likedPage]; controller.refresh()
    panel.selectLibrary(1)
    let writesBeforeSearch = api.sent.filter { $0["op"] as? String == "add" }.count
    panel.addSong(nil); verify(panel.location.song != nil && panel.search.isHidden)
    panel.focusSearch()
    verify(panel.location.key == likedLocation.key && panel.location.song == nil && !panel.search.isHidden && panel.search.currentEditor() != nil, "Command-F must restore browsing and focus visible search from Add")
    verify(api.sent.filter { $0["op"] as? String == "add" }.count == writesBeforeSearch, "leaving Add for search must not write account data")
    panel.selectLibrary(1); panel.view.layoutSubtreeIfNeeded()
    let availableCell = panel.table.view(atColumn: 0, row: 0, makeIfNecessary: true) as! MusicCell
    availableCell.showActions(true)
    let availabilityReloads = panel.reloadCount
    let hoverEvent = NSEvent.enterExitEvent(with: .mouseEntered, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber, context: nil, eventNumber: 0, trackingNumber: 0, userData: nil)!
    availableCell.playActionButton.mouseEntered(with: hoverEvent)
    verify(availableCell.playActionButton.isHovered, "reused native button must expose its hover state")
    api.state.pages = nil; api.state.adding = true; controller.refresh()
    verify(!availableCell.playActionButton.isEnabled && !availableCell.addActionButton.isEnabled && !availableCell.playActionButton.isHovered, "pending Add must disable visible actions and clear hover")
    api.state.adding = false; controller.refresh()
    verify(availableCell.playActionButton.isEnabled && availableCell.addActionButton.isEnabled && panel.reloadCount == availabilityReloads, "Add completion must re-enable the same row without a table reload")
    verify(panel.table.view(atColumn: 0, row: 0, makeIfNecessary: false) === availableCell, "availability changes must retain the existing cell")
    availableCell.playActionButton.mouseEntered(with: hoverEvent); availableCell.showActions(false)
    verify(!availableCell.playActionButton.isHovered, "hiding a reused button must clear hover")
    libraryRoot()

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
        api.state.format = "Opus 256 kbps · Premium (itag 774)"; api.state.source = "Late nights"; controller.refresh()
        panel.collapseBrowser()
        capture("player-dark.png", appearance: .darkAqua)
        capture("player-light.png", appearance: .aqua)
        verify(panel.destinations.frame.width >= 320, "destination buttons must fill the compact row")
        panel.selectLibrary(0); panel.view.layoutSubtreeIfNeeded()
        capture("library-dark.png", appearance: .darkAqua)
        panel.table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
        capture("player-actions-dark.png", appearance: .darkAqua)
        panel.table.deselectAll(nil)
        api.state.profiles.append(Profile(id: "brave:Default", label: "Brave · Default")); controller.refresh()
        panel.showAccount(nil); capture("account-dark.png", appearance: .darkAqua)
        panel.showAccount(nil)
        api.state.pages = [fixture, Page(key: "browse:VLPLowned:", target: browseTarget("VLPLowned"), title: "Late nights", rows: (0..<8).map { Row(title: "Song \($0 + 1)", subtitle: "Artist name", play: songTarget, video: "fixture\($0)") }, play: songTarget, loading: false, more: false)]
        controller.refresh(); panel.navigate(Location.from(browseTarget("VLPLowned"), title: "Late nights")!)
        capture("playlist-dark.png", appearance: .darkAqua)
        verify(!panel.destinations.isHidden && !panel.pagePlay.isHidden && !panel.pageShuffle.isHidden, "nested navigation must keep collection playback accessible")
        verify(panel.reconnectButton.frame.height == 28 && panel.reconnectButton.frame.width >= 100, "Connect needs a clear native hit area")
        for control in [panel.playButton, panel.previousButton, panel.nextButton, panel.shuffleButton, panel.addButton] {
            let frame = control.convert(control.bounds, to: panel.view)
            verify(panel.view.bounds.contains(frame) && frame.width >= 28 && frame.height >= 28, "transport hit areas must be visible and usable")
        }
        let visualSearch = Location.from(encodeTarget("Search", ["query": "Daft Punk", "params": NSNull()]), title: "Search")!
        api.state.pages = [fixture, searchPage(visualSearch, loading: true)]; controller.refresh()
        panel.navigate(visualSearch)
        capture("search-loading-dark.png", appearance: .darkAqua)
        let visualRows = ["One More Time", "Digital Love", "Something About Us", "Around the World"].enumerated().map { index, title in
            Row(title: title, subtitle: "Daft Punk", play: songTarget, video: "fixture\(index)")
        } + [Row(title: "Discovery", subtitle: "Album · Daft Punk · 2001", browse: browseTarget("MPREfixture")), Row(title: "Daft Punk essentials", subtitle: "Playlist · 24 songs", browse: browseTarget("VLPLfixture"))]
        api.state.pages = [fixture, searchPage(visualSearch, rows: visualRows)]; controller.refresh()
        capture("search-dark.png", appearance: .darkAqua)
        panel.table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
        let hoveredCell = panel.table.view(atColumn: 0, row: 0, makeIfNecessary: true) as! MusicCell
        hoveredCell.showActions(true); hoveredCell.playActionButton.mouseEntered(with: hoverEvent)
        capture("search-actions-dark.png", appearance: .darkAqua)
        api.state.pages = [fixture, searchPage(visualSearch)]; controller.refresh()
        capture("search-empty-dark.png", appearance: .darkAqua)
    }
    panel.closed(); window.orderOut(nil)
    let result: [String: Any] = ["result": "pass", "checks": ["prelaunch_wake", "one_status_item", "native_popover", "shuffle", "seek", "editable_only", "captured_song", "1000_scrollable_rows", "visible_cell_reuse", "refresh_preserves_content", "loading_can_pause", "search_dedup", "stale_result_navigation", "signed_out_gating", "actionable_signin", "reconnect_progress", "connecting_transport_gating", "single_profile_label", "missing_profile_not_substituted", "first_load_exposes_cancel", "warm_open_no_reload", "position_ticks_no_reload", "reopen_collapsed_preserves_internal_destination", "back_restores_search", "search_section_state", "selection_is_not_playback", "seek_tracking_stability", "seek_accounting_for_track_change", "volume_tracking_stability", "mute_restores_volume", "current_song_uses_transport", "inline_account", "search_focus_from_account", "control_hit_areas", "search_descendant_section_state", "compact_destination_sizing", "connect_hit_area", "inline_collection_play", "collection_breadcrumb_and_actions", "normalization_state_dispatch", "row_add_captures_song", "stale_row_action_account_boundary", "search_tab_navigation", "search_preserves_typed_spaces", "search_loading_viewport_stable", "search_history_preserves_draft", "marked_text_not_submitted", "marked_text_survives_snapshot", "search_back_resumes_debounce", "search_focus_resumes_debounce", "search_exits_add_without_write", "row_availability_without_reload", "reused_button_hover_reset"], "warm_open_ms": warmOpenMilliseconds, "warm_open_reloads": warmReloadDelta, "render_1000_rows_ms": renderMilliseconds, "instantiated_rows": liveRows, "player_library_height_pt": playerLibraryHeight, "collection_height_pt": collectionHeight]
    print(String(decoding: try! JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys]), as: UTF8.self))
    NSStatusBar.system.removeStatusItem(controller.status)
}
