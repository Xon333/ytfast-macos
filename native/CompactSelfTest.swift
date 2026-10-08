import AppKit

/// Bounded checks of the changed product flows, using the same production views.
func compactSelfTest() {
    let api = TestAPI()
    api.state.signed_in = true; api.state.account_checking = false
    api.state.source = "Late nights"
    let target = encodeTarget("Watch", ["video_id": NSNull(), "playlist_id": "PLfixture", "params": NSNull()])
    let serverShuffle = encodeTarget("Watch", ["video_id": "abcdefghijk", "playlist_id": "PLfixture", "params": "server-shuffle"])
    let location = Location.from(browseTarget("VLPLfixture"), title: "Late nights")!
    let page = Page(key: location.key, target: location.target, title: "Late nights", rows: [], play: target, shuffle: serverShuffle, loading: false, more: false)
    api.state.pages = [page]
    let owner = MenuApp(api: api, testing: true)
    owner.setup(); owner.refresh(); owner.showPopover()
    let panel = owner.panel!
    let height = panel.preferredContentSize.height
    precondition(!panel.browserExpanded && panel.search.isHidden && panel.rows.isEmpty)
    precondition(panel.view is ThemeBackdrop && !(panel.view is NSVisualEffectView))
    precondition(height <= 240, "compact player should not reserve catalogue height")
    precondition(panel.shuffleButton.isEnabled, "shuffle must be available before choosing a song")
    precondition(panel.shuffleButton.title == "Shuffle off")
    panel.shuffleButton.performClick(nil)
    precondition(api.last("shuffle") != nil)
    api.state.shuffle = true; owner.refresh()
    precondition(panel.shuffleButton.title == "Shuffle on" && panel.shuffleButton.isOn)
    precondition(panel.sourceLabel.stringValue == "Late nights")
    panel.navigate(location)
    panel.shufflePage(nil)
    precondition(api.last("play")?["target"] as? String == serverShuffle, "reuse the server's shuffle target")
    precondition(api.last("play")?["shuffle"] as? Bool == true && api.last("play")?["source"] as? String == "Late nights")
    panel.playPage(nil)
    precondition(api.last("play")?["target"] as? String == target && api.last("play")?["shuffle"] as? Bool == false)
    panel.collapseBrowser(); owner.popover.performClose(nil)
    let browseCount = api.sent.filter { $0["op"] as? String == "browse" }.count
    for _ in 0..<10 {
        owner.showPopover()
        precondition(!panel.browserExpanded && panel.search.isHidden && panel.preferredContentSize.height == height)
        owner.popover.performClose(nil)
    }
    precondition(api.sent.filter { $0["op"] as? String == "browse" }.count == browseCount, "ordinary opens must not request a catalogue")
    owner.showPopover()
    for button in panel.libraryButtons {
        button.performClick(nil); precondition(panel.browserExpanded && button.isOn)
        button.performClick(nil); precondition(!panel.browserExpanded && !button.isOn)
    }
    panel.searchButton.performClick(nil); precondition(panel.searchExpanded && !panel.search.isHidden)
    panel.searchButton.performClick(nil); precondition(!panel.browserExpanded && panel.search.isHidden)
    api.state.format = "Opus · 256 kbps · Premium"; owner.refresh()
    precondition(panel.makeMoreMenu().items.contains { $0.title == api.state.format })
    panel.view.layoutSubtreeIfNeeded()
    precondition(abs(panel.playButton.frame.width - panel.playButton.frame.height) < 0.01)
    precondition(abs(panel.playButton.shapeBounds.width - panel.playButton.shapeBounds.height) < 0.01)
    let original = panel.playButton.bounds
    panel.playButton.bounds = NSRect(x: 0, y: 0, width: 36, height: 42)
    precondition(panel.playButton.shapeBounds.width == panel.playButton.shapeBounds.height, "even a stretched host must draw a circle")
    panel.playButton.bounds = original
    owner.popover.performClose(nil); owner.dismissal.stop()
    NSStatusBar.system.removeStatusItem(owner.status)
    fputs("Compact checks: PASS (collapsed startup/reopen, no eager browse, destinations, ordering, source, More audio, round control)\n", stderr)
}
