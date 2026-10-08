import AppKit

/// Focused shell/appearance regressions. Uses the production popover and fake
/// backend, not a browser account. Event-routing tests are not hardware trials.
func finishSelfTest() {
    let app = NSApplication.shared
    app.setActivationPolicy(.accessory)
    let api = TestAPI()
    api.state.signed_in = true; api.state.account_checking = false
    api.state.account = "Fixture"; api.state.profile = "fixture"
    api.state.track = Song(id: "abcdefghijk", title: "Fixture track", artist: "Fixture artist")
    api.state.playing = true
    api.state.pages = [Page(key: playlistKey, target: browseTarget(playlistID), title: "Playlists", rows: [], loading: false, more: false)]
    let controller = MenuApp(api: api, testing: true)
    controller.setup(); controller.refresh()
    let panel = controller.panel!
    precondition(controller.popover.behavior == .applicationDefined, "status toggle must have one dismissal owner")
    precondition(controller.dismissal.activeObserverCount == 0, "closed panel must have no dismissal monitors")
    controller.showPopover()
    precondition(controller.popover.isShown, "native popover did not open")
    let button = controller.status.button!
    let frame = button.window!.convertToScreen(button.convert(button.bounds, to: nil))
    let anchor = NSPoint(x: frame.midX, y: frame.midY)
    let before = api.sent.count
    for _ in 0..<20 {
        controller.dismissal.start()
        precondition(controller.dismissal.activeObserverCount == 4, "duplicate or missing dismissal observers")
        // Replay D1's ordering: outside-click processing before the anchor's
        // action. Both local and global routes must leave this click to it.
        controller.dismissal.mouseDown(at: anchor, in: button.window)
        controller.dismissal.mouseDown(at: anchor, in: nil)
        precondition(controller.popover.isShown, "anchor mouse-down must not auto-close before its toggle")
        button.performClick(nil)
        precondition(!controller.popover.isShown && controller.dismissal.activeObserverCount == 0, "anchor action must close once and remove observers")
        button.performClick(nil)
        precondition(controller.popover.isShown, "next activation must reopen")
    }
    precondition(api.sent.dropFirst(before).allSatisfy { $0["op"] as? String == "collapse" }, "panel toggles must not dispatch playback or account writes")
    let window = panel.view.window!
    controller.dismissal.mouseDown(at: NSPoint(x: window.frame.midX, y: window.frame.midY), in: window)
    precondition(controller.popover.isShown, "inside click must preserve the panel")
    let child = NSWindow(contentRect: .zero, styleMask: [.borderless], backing: .buffered, defer: false)
    window.addChildWindow(child, ordered: .above)
    controller.dismissal.mouseDown(at: NSPoint(x: window.frame.midX, y: window.frame.midY), in: child)
    precondition(controller.popover.isShown, "attached child controls must not dismiss their owner")
    window.removeChildWindow(child); child.orderOut(nil)
    controller.dismissal.applicationActivated(processIdentifier: ProcessInfo.processInfo.processIdentifier)
    precondition(controller.popover.isShown, "activating this app must not dismiss its panel")
    let outside = NSPoint(x: frame.minX - 100, y: frame.minY - 100)
    controller.dismissal.mouseDown(at: outside, in: nil)
    precondition(!controller.popover.isShown && controller.dismissal.activeObserverCount == 0, "outside click must close and clean up")
    controller.showPopover()
    controller.dismissal.applicationActivated(processIdentifier: -1)
    precondition(!controller.popover.isShown, "switching to another app must close the panel")
    controller.showPopover()
    NSWorkspace.shared.notificationCenter.post(name: NSWorkspace.activeSpaceDidChangeNotification, object: nil)
    precondition(!controller.popover.isShown, "Space change must close the panel")
    controller.showPopover(); panel.escape()
    precondition(!controller.popover.isShown && controller.dismissal.activeObserverCount == 0, "Escape must close and clean up")
    let lightHost = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 360, height: 400), styleMask: [.borderless], backing: .buffered, defer: false)
    lightHost.appearance = NSAppearance(named: .aqua)
    lightHost.contentView = panel.view
    precondition(panel.view.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua, "product must stay dark under a light host")
    precondition(panel.seek.trackFillColor == NativeTheme.accent && panel.volume.trackFillColor == NativeTheme.accent)
    precondition(panel.playButton.baseColor == NativeTheme.base, "primary glyph must contrast with the Oxocarbon accent")
    lightHost.contentView = nil; lightHost.orderOut(nil)
    controller.dismissal.stop()
    NSStatusBar.system.removeStatusItem(controller.status)
    fputs("Native finish checks: PASS (20 anchor-order cycles; inside/child/outside/activation/Space/Escape; observer cleanup; dark appearance)\n", stderr)
}
