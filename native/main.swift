// A single native menu, backed by the existing Rust player in this process.
// No SwiftUI/WebKit/egui window, artwork cache, animation loop or helper UI.
import AppKit
import MediaPlayer
import Darwin
import CoreFoundation

struct Song: Codable, Equatable { var id: String; var title: String; var artist: String }
struct Profile: Codable, Equatable { var id: String; var label: String }
struct Row: Codable, Equatable {
    var title: String; var subtitle: String
    var play: String?; var browse: String?; var video: String?; var editable: String?
}
struct Page: Codable, Equatable {
    var key: String; var target: String; var title: String; var rows: [Row]
    var play: String?; var loading: Bool; var more: Bool; var message: String?
}
struct State: Codable {
    var track: Song?; var playing = false; var loading = false
    var position = 0.0; var duration = 0.0; var volume = 70.0; var shuffle = false
    var format: String?; var signed_in = false; var account = "Connecting…"
    var profiles: [Profile] = []; var profile: String?; var pages: [Page]?
    var notice: String?; var error: String?; var adding = false; var show = false; var quit = false
}

protocol PlayerAPI { func send(_ action: [String: Any]) -> State? }

/// Owned C strings never outlive the conversion; Rust never hands out borrowed data.
func consume(_ pointer: UnsafeMutablePointer<CChar>?) -> Data {
    guard let pointer = pointer else { return Data() }
    defer { ytfast_free(pointer) }
    return Data(String(cString: pointer).utf8)
}

final class CoreAPI: PlayerAPI {
    func send(_ action: [String: Any]) -> State? {
        guard let bytes = try? JSONSerialization.data(withJSONObject: action),
              let text = String(data: bytes, encoding: .utf8) else { return nil }
        let response = text.withCString { consume(ytfast_call($0)) }
        if let state = try? JSONDecoder().decode(State.self, from: response) { return state }
        if let error = (try? JSONSerialization.jsonObject(with: response)) as? [String: Any] {
            fputs("YTfast: \(error["fatal"] as? String ?? "Invalid bridge response")\n", stderr)
        }
        return nil
    }
}

// NSMenu runs a nested event-tracking loop, which does not drain GCD's main
// queue. Deliver backend/media work in common modes so an open menu stays live.
func onMainRunLoop(_ action: @escaping () -> Void) {
    let loop = CFRunLoopGetMain()
    CFRunLoopPerformBlock(loop, CFRunLoopMode.commonModes.rawValue, action)
    CFRunLoopWakeUp(loop)
}

// Worker callbacks are coalesced. Nothing polls or repaints while the app is idle.
let wakeLock = NSLock()
var wakeQueued = false
var applicationDelegate: MenuApp?

@_cdecl("ytfast_native_wake")
func nativeWake() {
    wakeLock.lock()
    if wakeQueued { wakeLock.unlock(); return }
    wakeQueued = true
    wakeLock.unlock()
    onMainRunLoop {
        wakeLock.lock(); wakeQueued = false; wakeLock.unlock()
        applicationDelegate?.refresh()
    }
}

final class ActionBox: NSObject {
    let value: [String: Any]
    init(_ value: [String: Any]) { self.value = value }
}

enum MenuRole {
    case page(target: String, key: String, offset: Int)
    case add(offset: Int)
    case settings
    case volume
}

final class MenuRef {
    weak var menu: NSMenu?
    let role: MenuRole
    init(_ menu: NSMenu, _ role: MenuRole) { self.menu = menu; self.role = role }
}

func browseTarget(_ id: String) -> String {
    let data = try! JSONSerialization.data(withJSONObject: ["Browse": ["id": id, "params": NSNull()]])
    return String(decoding: data, as: UTF8.self)
}
let playlistID = "FEmusic_liked_playlists"
let playlistKey = "browse:\(playlistID):"

/// Main-thread menu objects are retained only while they are reachable from the
/// status item's menu. Render at most 40 entries per submenu, even in large libraries.
final class MenuApp: NSObject, NSApplicationDelegate, NSMenuDelegate {
    let api: PlayerAPI
    let testing: Bool
    private(set) var state = State()
    private(set) var pages: [String: Page] = [:]
    private(set) var status: NSStatusItem!
    let menu = NSMenu()
    private var dynamic: [MenuRef] = []
    private var signals: [DispatchSourceSignal] = []
    private var capturedSong: Song?
    private var addOpen = false
    private var mediaStamp: (Song?, Bool, Double, Date)?
    private var shuttingDown = false
    private var refreshBusy = false
    let titleItem = NSMenuItem(title: "YTfast", action: nil, keyEquivalent: "")
    let artistItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
    let playItem = NSMenuItem(title: "Play", action: nil, keyEquivalent: "")
    let shuffleItem = NSMenuItem(title: "Shuffle", action: nil, keyEquivalent: "")
    let addItem = NSMenuItem(title: "Add song to playlist", action: nil, keyEquivalent: "")
    let libraryItem = NSMenuItem(title: "Library", action: nil, keyEquivalent: "")
    let messageItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
    private var previousItem: NSMenuItem!
    private var nextItem: NSMenuItem!

    init(api: PlayerAPI, testing: Bool = false) { self.api = api; self.testing = testing }

    func applicationDidFinishLaunching(_ notification: Notification) {
        setup()
        refresh()
        if !testing { installMediaKeys(); installSignals(); scheduleMemorySample() }
    }

    func setup() {
        menu.autoenablesItems = false
        status = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        let image = NSImage(systemSymbolName: "music.note", accessibilityDescription: "YTfast")
        image?.isTemplate = true
        status.button?.image = image
        status.button?.toolTip = "YTfast"
        status.menu = menu
        menu.addItem(titleItem); menu.addItem(artistItem)
        titleItem.isEnabled = false; artistItem.isEnabled = false
        menu.addItem(.separator())
        previousItem = item("Previous", ["op":"transport", "action":"previous"])
        menu.addItem(previousItem)
        configure(playItem, ["op":"transport", "action":"toggle"])
        menu.addItem(playItem)
        nextItem = item("Next", ["op":"transport", "action":"next"])
        menu.addItem(nextItem)
        configure(shuffleItem, ["op":"shuffle"])
        menu.addItem(shuffleItem)
        menu.addItem(submenuItem("Volume", role: .volume))
        menu.addItem(.separator())
        let library = NSMenu(); library.autoenablesItems = false
        library.addItem(submenuItem("Playlists", role: .page(target:browseTarget(playlistID), key:playlistKey, offset:0)))
        library.addItem(submenuItem("Liked Music", role: .page(target:browseTarget("FEmusic_liked_videos"), key:"browse:FEmusic_liked_videos:", offset:0)))
        library.addItem(submenuItem("Albums", role: .page(target:browseTarget("FEmusic_liked_albums"), key:"browse:FEmusic_liked_albums:", offset:0)))
        libraryItem.submenu = library; menu.addItem(libraryItem)
        addItem.submenu = newMenu(.add(offset: 0)); menu.addItem(addItem)
        menu.addItem(messageItem); messageItem.isHidden = true
        messageItem.target = self; messageItem.action = #selector(copyMessage(_:))
        menu.addItem(.separator())
        menu.addItem(submenuItem("Account", role: .settings))
        menu.addItem(item("Quit YTfast", ["op":"quit"], key:"q"))
    }

    private func configure(_ item: NSMenuItem, _ command: [String: Any]) {
        item.target = self; item.action = #selector(choose(_:)); item.representedObject = ActionBox(command)
    }

    private func item(_ title: String, _ command: [String: Any]? = nil, key: String = "") -> NSMenuItem {
        let result = NSMenuItem(title: short(title), action: nil, keyEquivalent: key)
        result.toolTip = title
        if let command = command { configure(result, command) } else { result.isEnabled = false }
        return result
    }

    private func newMenu(_ role: MenuRole) -> NSMenu {
        let result = NSMenu(); result.autoenablesItems = false; result.delegate = self
        dynamic.removeAll { $0.menu == nil }
        dynamic.append(MenuRef(result, role))
        return result
    }

    private func submenuItem(_ title: String, role: MenuRole) -> NSMenuItem {
        let result = NSMenuItem(title: short(title), action: nil, keyEquivalent: "")
        result.submenu = newMenu(role); return result
    }

    private func short(_ string: String) -> String {
        string.count > 65 ? String(string.prefix(62)) + "…" : string
    }

    @objc func choose(_ sender: NSMenuItem) {
        guard let action = (sender.representedObject as? ActionBox)?.value else { return }
        if let next = api.send(action) { apply(next) }
    }

    @objc private func copyMessage(_ sender: NSMenuItem) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(state.error ?? state.notice ?? "", forType: .string)
    }

    func refresh() {
        // Common-mode wakes can arrive during AppKit startup, before the
        // launch delegate has created its status item. The launch snapshot
        // drains all accumulated events once setup is complete.
        guard status != nil, !shuttingDown, !refreshBusy else { return }
        refreshBusy = true
        defer { refreshBusy = false }
        if let next = api.send(["op":"poll"]) { apply(next) }
    }

    func apply(_ next: State) {
        state = next
        titleItem.title = short(next.track?.title ?? "Nothing playing")
        artistItem.title = short(next.track?.artist ?? "Choose music from Library")
        status.button?.toolTip = next.track.map {"\($0.title) · \($0.artist)"} ?? "YTfast"
        playItem.title = next.loading ? "Loading…" : (next.playing ? "Pause" : "Play")
        playItem.isEnabled = next.track != nil && !next.loading
        previousItem?.isEnabled = next.track != nil
        nextItem?.isEnabled = next.track != nil
        shuffleItem.state = next.shuffle ? .on : .off
        libraryItem.isEnabled = next.signed_in
        addItem.isEnabled = next.track != nil && next.signed_in && !next.adding
        addItem.title = next.adding ? "Adding song…" : "Add song to playlist"
        let message = next.error ?? next.notice
        messageItem.isHidden = message == nil
        messageItem.title = short(message ?? "")
        messageItem.toolTip = message.map { $0 + " (click to copy)" }
        let old = pages
        if !next.signed_in { pages.removeAll() }
        if let updates = next.pages {
            pages = next.signed_in ? Dictionary(uniqueKeysWithValues: updates.map {($0.key,$0)}) : [:]
            // Iterate a copy: rebuilding a menu can install nested submenu delegates.
            let refs = dynamic
            for ref in refs {
                guard let nativeMenu = ref.menu else {continue}
                switch ref.role {
                case let .page(_, key, _) where old[key] != pages[key]: build(nativeMenu, ref.role)
                case .add(_) where old[playlistKey] != pages[playlistKey]: build(nativeMenu, ref.role)
                default: break
                }
            }
        }
        if !testing { updateNowPlaying() }
        if next.quit {
            if !testing { menu.cancelTracking(); NSApplication.shared.terminate(nil) }
        } else if next.show && !testing {
            onMainRunLoop { [weak self] in self?.status.button?.performClick(nil) }
        }
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        guard let role = dynamic.first(where: {$0.menu === menu})?.role else {return}
        build(menu, role)
    }

    func menuWillOpen(_ menu: NSMenu) {
        guard let role = dynamic.first(where: {$0.menu === menu})?.role else {return}
        switch role {
        case let .page(target, _, offset) where offset == 0:
            if let next = api.send(["op":"browse", "target":target]) { apply(next) }
        case .add(0):
            addOpen = true; capturedSong = state.track
            if let next = api.send(["op":"browse", "target":browseTarget(playlistID)]) { apply(next) }
        default: break
        }
        build(menu, role)
    }

    func menuDidClose(_ menu: NSMenu) {
        if case .add(0)? = dynamic.first(where: {$0.menu === menu})?.role { addOpen = false }
    }

    private func build(_ menu: NSMenu, _ role: MenuRole) {
        menu.removeAllItems()
        switch role {
        case let .page(target, key, offset):
            guard let page = pages[key] else {menu.addItem(item("Loading…"));return}
            if offset == 0, let play = page.play {
                menu.addItem(item("Play", ["op":"play", "target":play])); menu.addItem(.separator())
            }
            let rows = page.rows.dropFirst(offset).prefix(40)
            for row in rows {
                if let browse = row.browse, let object = try? JSONSerialization.jsonObject(with: Data(browse.utf8)) as? [String:Any],
                   let browseBody = object["Browse"] as? [String:Any], let id = browseBody["id"] as? String {
                    let child = submenuItem(row.title, role:.page(target:browse,key:"browse:\(id):\(browseBody["params"] as? String ?? "")",offset:0))
                    // The submenu's own page supplies Play and its songs; selecting a
                    // playlist never overwrites the queue merely by hovering over it.
                    menu.addItem(child)
                } else if let play = row.play {
                    let label = row.subtitle.isEmpty ? row.title : "\(row.title) — \(row.subtitle)"
                    menu.addItem(item(label, ["op":"play", "target":play]))
                } else {menu.addItem(item(row.title))}
            }
            if page.rows.count > offset + 40 {
                menu.addItem(submenuItem("More…",role:.page(target:target,key:key,offset:offset+40)))
            } else if page.more {
                menu.addItem(item(page.loading ? "Loading…" : "Load more…", page.loading ? nil : ["op":"more", "key":key]))
            }
            if let message = page.message {menu.addItem(item(message))}
            if rows.isEmpty && !page.more && page.message == nil {menu.addItem(item(page.loading ? "Loading…" : "No music here"))}
            if offset == 0 {
                menu.addItem(.separator());menu.addItem(item("Refresh",["op":"browse","target":target,"force":true]))
            }
        case let .add(offset):
            let song = addOpen ? capturedSong : state.track
            guard let song = song else {menu.addItem(item("Nothing playing"));return}
            menu.addItem(item("Add “\(song.title)” to:"))
            guard let page = pages[playlistKey] else {menu.addItem(item("Loading playlists…"));return}
            let owned = page.rows.filter {$0.editable != nil}
            for row in owned.dropFirst(offset).prefix(40) {
                menu.addItem(item(row.title,["op":"add","playlist":row.editable!,"video":song.id]))
            }
            if owned.isEmpty {menu.addItem(item(page.loading ? "Loading playlists…" : "No editable playlists loaded"))}
            if owned.count > offset + 40 {
                menu.addItem(submenuItem("More…", role: .add(offset: offset + 40)))
            } else if page.more {
                menu.addItem(item(page.loading ? "Loading…" : "Load more playlists…", page.loading ? nil : ["op":"more","key":playlistKey]))
            }
            if let message=page.message {menu.addItem(item(message))}
            menu.addItem(.separator());menu.addItem(item("Refresh playlists",["op":"browse","target":browseTarget(playlistID),"force":true]))
        case .settings:
            menu.addItem(item(state.account))
            menu.addItem(item("Reconnect",["op":"reconnect"]))
            if !state.profiles.isEmpty { menu.addItem(.separator()) }
            for profile in state.profiles {
                let choice = item(profile.label,["op":"profile","id":profile.id])
                choice.state = state.profile == profile.id ? .on : .off
                menu.addItem(choice)
            }
            if let format = state.format {menu.addItem(.separator());menu.addItem(item(format))}
        case .volume:
            for value in stride(from:0, through:100, by:10) {
                let choice = item("\(value)%",["op":"volume","value":value])
                choice.state = abs(state.volume-Double(value)) < 5 ? .on : .off
                menu.addItem(choice)
            }
        }
    }

    private func installMediaKeys() {
        let center = MPRemoteCommandCenter.shared()
        for (command, action) in [(center.playCommand,"play"),(center.pauseCommand,"pause"),
            (center.togglePlayPauseCommand,"toggle"),(center.nextTrackCommand,"next"),
            (center.previousTrackCommand,"previous")] {
            command.addTarget { [weak self] _ in
                onMainRunLoop {
                    guard let self = self, let next = self.api.send(["op":"transport","action":action]) else {return}
                    self.apply(next)
                }
                return .success
            }
        }
        center.changeShuffleModeCommand.addTarget { [weak self] event in
            guard let event = event as? MPChangeShuffleModeCommandEvent else {return .commandFailed}
            let wanted = event.shuffleType != .off
            onMainRunLoop {
                guard let self = self, self.state.shuffle != wanted, let next = self.api.send(["op":"shuffle"]) else {return}
                self.apply(next)
            }
            return .success
        }
        updateNowPlaying()
    }

    private func updateNowPlaying() {
        let center=MPRemoteCommandCenter.shared()
        let enabled=state.track != nil
        for command in [center.playCommand,center.pauseCommand,center.togglePlayPauseCommand,center.nextTrackCommand,center.previousTrackCommand,center.changeShuffleModeCommand] { command.isEnabled=enabled }
        center.changeShuffleModeCommand.currentShuffleType = state.shuffle ? .items : .off
        let info=MPNowPlayingInfoCenter.default()
        guard let song=state.track else {
            if mediaStamp != nil {info.nowPlayingInfo=nil;info.playbackState = .stopped;mediaStamp=nil}
            return
        }
        if let (lastSong, playing, position, date)=mediaStamp {
            let expected=position+(playing ? Date().timeIntervalSince(date) : 0)
            if lastSong == song && playing == state.playing && abs(expected-state.position)<2 {return}
        }
        info.nowPlayingInfo = [MPMediaItemPropertyTitle:song.title,MPMediaItemPropertyArtist:song.artist,
            MPMediaItemPropertyPlaybackDuration:state.duration,MPNowPlayingInfoPropertyElapsedPlaybackTime:state.position,
            MPNowPlayingInfoPropertyPlaybackRate:state.playing ? 1.0 : 0.0,
            MPNowPlayingInfoPropertyMediaType:MPNowPlayingInfoMediaType.audio.rawValue]
        info.playbackState = state.playing ? .playing : .paused
        mediaStamp=(song,state.playing,state.position,Date())
    }

    private func installSignals() {
        for number in [SIGTERM,SIGINT] {
            signal(number,SIG_IGN)
            let source=DispatchSource.makeSignalSource(signal:number,queue:.global(qos: .utility))
            source.setEventHandler {onMainRunLoop {
                applicationDelegate?.menu.cancelTracking()
                NSApplication.shared.terminate(nil)
            }}
            source.resume();signals.append(source)
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        shuttingDown=true
        if !testing {
            MPNowPlayingInfoCenter.default().nowPlayingInfo=nil
            MPNowPlayingInfoCenter.default().playbackState = .stopped
            ytfast_stop()
        }
    }

    /// CI can inspect memory without collecting account/library data. No sample
    /// file is produced in ordinary launches, and it contains only process metrics.
    private func scheduleMemorySample() {
        guard ProcessInfo.processInfo.environment["GITHUB_ACTIONS"] == "true",
              let path=ProcessInfo.processInfo.environment["YTFAST_PROFILE_FILE"] else {return}
        DispatchQueue.main.asyncAfter(deadline:.now()+15) {
            if let data=try? JSONSerialization.data(withJSONObject:memorySample(),options:.prettyPrinted) {
                try? data.write(to:URL(fileURLWithPath:path),options:.atomic)
            }
        }
    }
}

func memorySample() -> [String:Any] {
    var info=task_vm_info_data_t()
    var count=mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size/MemoryLayout<integer_t>.size)
    let result=withUnsafeMutablePointer(to:&info) {pointer in
        pointer.withMemoryRebound(to:integer_t.self,capacity:Int(count)) {
            task_info(mach_task_self_,task_flavor_t(TASK_VM_INFO),$0,&count)
        }
    }
    return ["pid":getpid(),"task_info_ok":result == KERN_SUCCESS,
            "rss_bytes":info.resident_size,"physical_footprint_bytes":info.phys_footprint]
}

/// Synthetic menu actions exercise the real AppKit menu builder, not an HTML mock.
final class TestAPI: PlayerAPI {
    var state=State()
    var sent:[[String:Any]]=[]
    func send(_ action:[String:Any])->State? {sent.append(action);return state}
}

func selfTest() {
    let app=NSApplication.shared;app.setActivationPolicy(.accessory)
    let api=TestAPI()
    api.state.signed_in=true
    api.state.track=Song(id:"abcdefghijk",title:"Fixture song",artist:"Fixture artist")
    api.state.pages=[Page(key:playlistKey,target:browseTarget(playlistID),title:"Playlists",
        rows:[Row(title:"Owned",subtitle:"",play:nil,browse:nil,video:nil,editable:"PLowned"),
              Row(title:"Subscribed",subtitle:"",play:nil,browse:nil,video:nil,editable:nil)],
        play:nil,loading:false,more:false,message:nil)]
    let controller=MenuApp(api:api,testing:true)
    controller.refresh() // A backend wake may precede applicationDidFinishLaunching.
    precondition(api.sent.isEmpty)
    controller.setup();controller.refresh()
    precondition(controller.status.menu === controller.menu)
    precondition(controller.shuffleItem.state == .off)
    controller.choose(controller.shuffleItem)
    precondition(api.sent.last?["op"] as? String == "shuffle")
    let add=controller.addItem.submenu!
    controller.menuWillOpen(add)
    precondition(!add.items.contains {$0.title == "Subscribed"})
    let owned=add.items.first {$0.title == "Owned"}!
    api.state.track=Song(id:"lmnopqrstuv",title:"Next song",artist:"Fixture")
    controller.refresh()
    controller.choose(owned)
    precondition(api.sent.last?["video"] as? String == "abcdefghijk", "capture the displayed song")
    precondition(api.sent.last?["playlist"] as? String == "PLowned")
    controller.menuDidClose(add)
    api.state.pages![0].rows = (0..<85).map { Row(title:"Playlist \($0)",subtitle:"",play:nil,browse:nil,video:nil,editable:"PL\($0)") }
    controller.refresh(); controller.menuWillOpen(add)
    precondition(add.items.filter {($0.representedObject as? ActionBox)?.value["op"] as? String == "add"}.count == 40)
    let more = add.items.first {$0.title == "More…"}!.submenu!
    controller.menuWillOpen(more)
    precondition(more.items.contains {$0.title == "Playlist 40"})
    precondition(!more.items.contains {$0.title == "Playlist 0"})
    controller.choose(more.items.first {$0.title == "Playlist 40"}!)
    precondition(api.sent.last?["playlist"] as? String == "PL40")
    precondition(api.sent.last?["video"] as? String == "lmnopqrstuv")
    precondition(controller.libraryItem.isEnabled)
    api.state.signed_in=false;api.state.pages=[];controller.refresh()
    precondition(!controller.libraryItem.isEnabled && !controller.addItem.isEnabled)
    precondition(controller.pages.isEmpty)
    precondition(!add.items.contains {$0.title == "Playlist 0"})
    let payload:[String:Any]=["result":"pass","checks":["prelaunch_wake","one_status_item","shuffle_dispatch","editable_only","captured_song","bounded_playlist_menus","signed_out_gating"],"memory":memorySample()]
    print(String(decoding:try! JSONSerialization.data(withJSONObject:payload,options:[.prettyPrinted,.sortedKeys]),as:UTF8.self))
    NSStatusBar.system.removeStatusItem(controller.status)
}

if CommandLine.arguments.contains("--self-test") {
    selfTest()
} else {
    let data=consume(ytfast_start(nativeWake))
    let result=(try? JSONSerialization.jsonObject(with:data)) as? [String:Any] ?? [:]
    if let code=result["exit"] as? Int {
        if let message=result["message"] as? String {print(message)}
        exit(Int32(code))
    }
    if let error=result["fatal"] as? String {
        fputs("YTfast: \(error)\n",stderr)
        let alert=NSAlert();alert.messageText="YTfast could not start";alert.informativeText=error;alert.runModal()
        exit(1)
    }
    let app=NSApplication.shared
    app.setActivationPolicy(.accessory)
    applicationDelegate=MenuApp(api:CoreAPI())
    app.delegate=applicationDelegate
    app.run()
}
