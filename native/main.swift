// One native dropdown, one Rust player. All I/O stays on the backend runtime.
import AppKit
import MediaPlayer
import Darwin
import CoreFoundation

// Common-mode delivery keeps commands live during menu and slider tracking.
func onMainRunLoop(_ action: @escaping () -> Void) {
    let loop = CFRunLoopGetMain()
    CFRunLoopPerformBlock(loop, CFRunLoopMode.commonModes.rawValue, action)
    CFRunLoopWakeUp(loop)
}
let wakeLock = NSLock()
var wakeQueued = false
var applicationDelegate: MenuApp?

@_cdecl("ytfast_native_wake")
func nativeWake() {
    wakeLock.lock()
    if wakeQueued { wakeLock.unlock(); return }
    wakeQueued = true; wakeLock.unlock()
    onMainRunLoop {
        wakeLock.lock(); wakeQueued = false; wakeLock.unlock()
        applicationDelegate?.refresh()
    }
}

private struct MediaStamp {
    var song: Song; var playing: Bool; var position: Double; var duration: Double; var date: Date
}

final class MenuApp: NSObject, NSApplicationDelegate, NSPopoverDelegate {
    let api: PlayerAPI
    let testing: Bool
    private(set) var state = State()
    private(set) var status: NSStatusItem!
    private(set) var panel: PlayerPanel!
    private(set) var dismissal: PopoverDismissal!
    let popover = NSPopover()
    private var signals: [DispatchSourceSignal] = []
    private var mediaStamp: MediaStamp?
    private var shuttingDown = false
    private var refreshBusy = false
    init(api: PlayerAPI, testing: Bool = false) { self.api = api; self.testing = testing }
    func applicationDidFinishLaunching(_ notification: Notification) {
        setup(); refresh()
        if !testing { installMediaKeys(); installSignals(); scheduleMemorySample() }
    }
    func setup() {
        guard status == nil else { return }
        status = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        let image = NSImage(systemSymbolName: "music.note", accessibilityDescription: "YTfast")
        image?.isTemplate = true
        status.button?.image = image; status.button?.toolTip = "YTfast"
        status.button?.target = self; status.button?.action = #selector(togglePopover(_:))
        panel = PlayerPanel { [weak self] action in self?.send(action) }
        panel.sizeChanged = { [weak self] size in self?.popover.contentSize = size }
        panel.closeRequested = { [weak self] in self?.popover.performClose(nil) }
        NativeTheme.install(on: panel)
        popover.contentViewController = panel; popover.contentSize = panel.preferredContentSize
        popover.appearance = NSAppearance(named: .darkAqua)
        popover.behavior = .applicationDefined; popover.animates = false; popover.delegate = self
        dismissal = PopoverDismissal(popover: popover, button: status.button!)
        if !testing { installApplicationMenu() }
    }
    func send(_ action: [String: Any]) { if let next = api.send(action) { apply(next) } }
    func refresh() {
        guard status != nil, !shuttingDown, !refreshBusy else { return }
        refreshBusy = true; defer { refreshBusy = false }
        send(["op": "poll"])
    }
    func apply(_ next: State) {
        state = next; panel.apply(next)
        status.button?.toolTip = next.track.map { "\($0.title) · \($0.artist)" } ?? "YTfast"
        if !testing { updateNowPlaying() }
        if next.quit && !testing { popover.close(); NSApplication.shared.terminate(nil) }
        else if next.show && !testing { onMainRunLoop { [weak self] in self?.showPopover() } }
    }
    @objc func togglePopover(_ sender: Any?) {
        if popover.isShown { popover.performClose(sender) } else { showPopover() }
    }
    func showPopover() {
        guard let button = status?.button, !popover.isShown else { return }
        NSApplication.shared.activate(ignoringOtherApps: true)
        popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        popover.contentViewController?.view.window?.makeKey()
    }
    func popoverWillShow(_ notification: Notification) {
        status.button?.highlight(true); panel.opened(); dismissal.start()
    }
    func popoverDidClose(_ notification: Notification) {
        dismissal.stop(); status.button?.highlight(false); panel.closed()
    }
    @objc private func focusSearch(_ sender: Any?) { showPopover(); panel.focusSearch() }
    @objc private func refreshLibrary(_ sender: Any?) { panel.refreshPage(sender) }
    @objc private func quit(_ sender: Any?) { send(["op": "quit"]) }

    private func installApplicationMenu() {
        // Accessory apps still need Edit commands for text-field shortcuts.
        let root = NSMenu()
        let application = NSMenuItem(); let appMenu = NSMenu()
        let quit = NSMenuItem(title: "Quit YTfast", action: #selector(quit(_:)), keyEquivalent: "q")
        quit.target = self; appMenu.addItem(quit); application.submenu = appMenu; root.addItem(application)
        let editItem = NSMenuItem(title: "Edit", action: nil, keyEquivalent: ""); let edit = NSMenu(title: "Edit")
        for (title, selector, key) in [("Cut", "cut:", "x"), ("Copy", "copy:", "c"), ("Paste", "paste:", "v"), ("Select All", "selectAll:", "a")] {
            edit.addItem(NSMenuItem(title: title, action: NSSelectorFromString(selector), keyEquivalent: key))
        }
        editItem.submenu = edit; root.addItem(editItem)
        let viewItem = NSMenuItem(title: "Music", action: nil, keyEquivalent: ""); let music = NSMenu(title: "Music")
        let search = NSMenuItem(title: "Search", action: #selector(focusSearch(_:)), keyEquivalent: "f")
        search.target = self; music.addItem(search)
        let refresh = NSMenuItem(title: "Refresh", action: #selector(refreshLibrary(_:)), keyEquivalent: "r")
        refresh.target = self; music.addItem(refresh); viewItem.submenu = music; root.addItem(viewItem)
        NSApplication.shared.mainMenu = root
    }
    private func installMediaKeys() {
        let center = MPRemoteCommandCenter.shared()
        for (command, action) in [(center.playCommand, "play"), (center.pauseCommand, "pause"),
            (center.togglePlayPauseCommand, "toggle"), (center.nextTrackCommand, "next"), (center.previousTrackCommand, "previous")] {
            command.addTarget { [weak self] _ in
                onMainRunLoop { self?.send(["op": "transport", "action": action]) }
                return .success
            }
        }
        center.changePlaybackPositionCommand.addTarget { [weak self] event in
            guard let event = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
            let position = event.positionTime
            onMainRunLoop { self?.send(["op": "seek", "value": position]) }
            return .success
        }
        center.changeShuffleModeCommand.addTarget { [weak self] event in
            guard let event = event as? MPChangeShuffleModeCommandEvent else { return .commandFailed }
            let wanted = event.shuffleType != .off
            onMainRunLoop {
                guard let self, self.state.shuffle != wanted else { return }
                self.send(["op": "shuffle"])
            }
            return .success
        }
        updateNowPlaying()
    }
    private func updateNowPlaying() {
        let center = MPRemoteCommandCenter.shared()
        let enabled = state.track != nil || state.loading
        for command in [center.playCommand, center.pauseCommand, center.togglePlayPauseCommand, center.nextTrackCommand, center.previousTrackCommand, center.changeShuffleModeCommand] { command.isEnabled = enabled }
        center.changePlaybackPositionCommand.isEnabled = state.track != nil && state.duration > 0 && !state.loading
        center.changeShuffleModeCommand.currentShuffleType = state.shuffle ? .items : .off
        let info = MPNowPlayingInfoCenter.default()
        guard let song = state.track else {
            if mediaStamp != nil { info.nowPlayingInfo = nil; info.playbackState = .stopped; mediaStamp = nil }
            return
        }
        let playing = state.playing && !state.loading
        if let last = mediaStamp {
            let expected = last.position + (last.playing ? Date().timeIntervalSince(last.date) : 0)
            if last.song == song && last.playing == playing && last.duration == state.duration && abs(expected - state.position) < 2 { return }
        }
        info.nowPlayingInfo = [MPMediaItemPropertyTitle: song.title, MPMediaItemPropertyArtist: song.artist,
            MPMediaItemPropertyPlaybackDuration: state.duration, MPNowPlayingInfoPropertyElapsedPlaybackTime: state.position,
            MPNowPlayingInfoPropertyPlaybackRate: playing ? 1.0 : 0.0,
            MPNowPlayingInfoPropertyMediaType: MPNowPlayingInfoMediaType.audio.rawValue]
        info.playbackState = playing ? .playing : .paused
        mediaStamp = MediaStamp(song: song, playing: playing, position: state.position, duration: state.duration, date: Date())
    }
    private func installSignals() {
        for number in [SIGTERM, SIGINT] {
            signal(number, SIG_IGN)
            let source = DispatchSource.makeSignalSource(signal: number, queue: .global(qos: .utility))
            source.setEventHandler { onMainRunLoop { NSApplication.shared.terminate(nil) } }
            source.resume(); signals.append(source)
        }
    }
    func applicationWillTerminate(_ notification: Notification) {
        shuttingDown = true; dismissal?.stop(); panel?.closed()
        if !testing {
            MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
            MPNowPlayingInfoCenter.default().playbackState = .stopped
            ytfast_stop()
        }
    }
    private func scheduleMemorySample() {
        guard ProcessInfo.processInfo.environment["GITHUB_ACTIONS"] == "true",
              let path = ProcessInfo.processInfo.environment["YTFAST_PROFILE_FILE"] else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 15) {
            if let data = try? JSONSerialization.data(withJSONObject: memorySample(), options: .prettyPrinted) {
                try? data.write(to: URL(fileURLWithPath: path), options: .atomic)
            }
        }
    }
}

func memorySample() -> [String: Any] {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
    let result = withUnsafeMutablePointer(to: &info) { pointer in
        pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
            task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
        }
    }
    return ["pid": getpid(), "task_info_ok": result == KERN_SUCCESS,
            "rss_bytes": info.resident_size, "physical_footprint_bytes": info.phys_footprint]
}

if CommandLine.arguments.contains("--self-test") {
    finishSelfTest()
    selfTest()
} else {
    let data = consume(ytfast_start(nativeWake))
    let result = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
    if let code = result["exit"] as? Int {
        if let message = result["message"] as? String { print(message) }
        exit(Int32(code))
    }
    if let error = result["fatal"] as? String {
        fputs("YTfast: \(error)\n", stderr)
        let alert = NSAlert(); alert.messageText = "YTfast could not start"; alert.informativeText = error; alert.runModal()
        exit(1)
    }
    let app = NSApplication.shared; app.setActivationPolicy(.accessory)
    applicationDelegate = MenuApp(api: CoreAPI()); app.delegate = applicationDelegate; app.run()
}
