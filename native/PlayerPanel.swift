import AppKit

final class MusicTable: NSTableView {
    var activate: (() -> Void)?
    var togglePlayback: (() -> Void)?
    override func keyDown(with event: NSEvent) {
        if event.keyCode == 36 || event.keyCode == 76 { activate?() }
        else if event.keyCode == 49 { togglePlayback?() }
        else { super.keyDown(with: event) }
    }
}

/// AppKit reuses only the visible cells. A thousand-song playlist is still one
/// scrollable list, not a chain of menus or a thousand retained row views.
final class MusicCell: NSTableCellView {
    let title = NSTextField(labelWithString: "")
    let subtitle = NSTextField(labelWithString: "")
    let accessory = NSImageView()
    override init(frame: NSRect) {
        super.init(frame: frame)
        title.font = .systemFont(ofSize: 12, weight: .medium)
        subtitle.font = .systemFont(ofSize: 11)
        subtitle.textColor = .secondaryLabelColor
        for label in [title, subtitle] {
            label.lineBreakMode = .byTruncatingTail
            label.translatesAutoresizingMaskIntoConstraints = false
            addSubview(label)
        }
        accessory.translatesAutoresizingMaskIntoConstraints = false
        accessory.contentTintColor = .secondaryLabelColor
        addSubview(accessory)
        NSLayoutConstraint.activate([
            title.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            title.topAnchor.constraint(equalTo: topAnchor, constant: 6),
            title.trailingAnchor.constraint(equalTo: accessory.leadingAnchor, constant: -8),
            subtitle.leadingAnchor.constraint(equalTo: title.leadingAnchor),
            subtitle.topAnchor.constraint(equalTo: title.bottomAnchor, constant: 2),
            subtitle.trailingAnchor.constraint(equalTo: title.trailingAnchor),
            accessory.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            accessory.centerYAnchor.constraint(equalTo: centerYAnchor),
            accessory.widthAnchor.constraint(equalToConstant: 14),
            accessory.heightAnchor.constraint(equalToConstant: 14)
        ])
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }
}

final class PlayerPanel: NSViewController, NSTableViewDataSource, NSTableViewDelegate, NSSearchFieldDelegate {
    let send: ([String: Any]) -> Void
    private(set) var state = State()
    private(set) var pages: [String: Page] = [:]
    private(set) var location = Location.library(0)
    private(set) var rows: [Row] = []
    private var history: [Location] = []
    private var searchWork: DispatchWorkItem?
    private var visible = false
    private var renderedLocation = ""
    private var browserDirty = true
    private var accountMenu: NSMenu?
    private var reconnectItem: NSMenuItem?
    private var accountInfo: NSMenuItem?

    let titleLabel = NSTextField(labelWithString: "Nothing playing")
    let artistLabel = NSTextField(labelWithString: "Choose a song below")
    let playButton = NSButton()
    let previousButton = NSButton()
    let nextButton = NSButton()
    let shuffleButton = NSButton()
    let addButton = NSButton()
    let seek = NSSlider(value: 0, minValue: 0, maxValue: 1, target: nil, action: nil)
    let volume = NSSlider(value: 70, minValue: 0, maxValue: 100, target: nil, action: nil)
    private let elapsed = NSTextField(labelWithString: "0:00")
    private let remaining = NSTextField(labelWithString: "−0:00")
    private let playbackStatus = NSTextField(labelWithString: "")
    private let playbackSpinner = NSProgressIndicator()
    let search = NSSearchField()
    let sections = NSSegmentedControl(labels: ["Playlists", "Liked Music", "Albums"], trackingMode: .selectOne, target: nil, action: nil)
    let backButton = NSButton()
    private let pageTitle = NSTextField(labelWithString: "Playlists")
    let pagePlay = NSButton(title: "Play", target: nil, action: nil)
    let refreshButton = NSButton()
    private let pageSpinner = NSProgressIndicator()
    let table = MusicTable()
    let scroll = NSScrollView()
    private let emptyLabel = NSTextField(wrappingLabelWithString: "")
    private let emptyActions = NSStackView()
    let reconnectButton = NSButton(title: "Reconnect", target: nil, action: nil)
    private let empty = NSStackView()
    let moreButton = NSButton(title: "Load more", target: nil, action: nil)
    private let messageRow = NSStackView()
    private let messageLabel = NSTextField(wrappingLabelWithString: "")
    private let dismissButton = NSButton()
    let accountButton = NSButton()
    private let quitButton = NSButton()

    init(send: @escaping ([String: Any]) -> Void) {
        self.send = send
        super.init(nibName: nil, bundle: nil)
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }

    private func symbol(_ button: NSButton, _ name: String, _ label: String, _ action: Selector, size: CGFloat = 28) {
        button.title = ""
        button.image = NSImage(systemSymbolName: name, accessibilityDescription: label)
        button.imagePosition = .imageOnly
        button.isBordered = false
        button.bezelStyle = .regularSquare
        button.target = self; button.action = action
        button.toolTip = label; button.setAccessibilityLabel(label)
        button.translatesAutoresizingMaskIntoConstraints = false
        button.widthAnchor.constraint(equalToConstant: size).isActive = true
        button.heightAnchor.constraint(equalToConstant: size).isActive = true
    }
    private func horizontal(_ views: [NSView], spacing: CGFloat = 8) -> NSStackView {
        let result = NSStackView(views: views)
        result.orientation = .horizontal; result.alignment = .centerY; result.spacing = spacing
        return result
    }
    private func spacer() -> NSView {
        let view = NSView()
        view.setContentHuggingPriority(.defaultLow, for: .horizontal)
        return view
    }
    private func spinner(_ view: NSProgressIndicator) {
        view.style = .spinning; view.controlSize = .small; view.isDisplayedWhenStopped = false
        view.widthAnchor.constraint(equalToConstant: 14).isActive = true
        view.heightAnchor.constraint(equalToConstant: 14).isActive = true
    }
    private func separator() -> NSBox {
        let line = NSBox(); line.boxType = .separator; return line
    }

    override func loadView() {
        let surface = NSVisualEffectView(frame: NSRect(x: 0, y: 0, width: 380, height: 560))
        surface.material = .popover; surface.blendingMode = .withinWindow; surface.state = .active
        view = surface
        let content = NSStackView()
        content.orientation = .vertical; content.alignment = .leading; content.spacing = 8
        content.translatesAutoresizingMaskIntoConstraints = false
        surface.addSubview(content)
        NSLayoutConstraint.activate([
            content.leadingAnchor.constraint(equalTo: surface.leadingAnchor, constant: 14),
            content.trailingAnchor.constraint(equalTo: surface.trailingAnchor, constant: -14),
            content.topAnchor.constraint(equalTo: surface.topAnchor, constant: 14),
            content.bottomAnchor.constraint(equalTo: surface.bottomAnchor, constant: -12)
        ])
        func add(_ child: NSView) {
            child.translatesAutoresizingMaskIntoConstraints = false
            content.addArrangedSubview(child)
            child.widthAnchor.constraint(equalTo: content.widthAnchor).isActive = true
        }
        titleLabel.font = .systemFont(ofSize: 15, weight: .semibold)
        titleLabel.lineBreakMode = .byTruncatingTail
        artistLabel.font = .systemFont(ofSize: 12); artistLabel.textColor = .secondaryLabelColor
        artistLabel.lineBreakMode = .byTruncatingTail
        let heading = NSStackView(views: [titleLabel, artistLabel])
        heading.orientation = .vertical; heading.alignment = .leading; heading.spacing = 3
        titleLabel.widthAnchor.constraint(equalTo: heading.widthAnchor).isActive = true
        artistLabel.widthAnchor.constraint(equalTo: heading.widthAnchor).isActive = true
        add(heading)
        symbol(shuffleButton, "shuffle", "Shuffle", #selector(shuffle(_:)))
        symbol(previousButton, "backward.end.fill", "Previous", #selector(previous(_:)), size: 32)
        symbol(playButton, "play.fill", "Play", #selector(toggle(_:)), size: 38)
        playButton.imageScaling = .scaleProportionallyUpOrDown
        symbol(nextButton, "forward.end.fill", "Next", #selector(next(_:)), size: 32)
        symbol(addButton, "text.badge.plus", "Add song to playlist", #selector(addSong(_:)))
        let leftSpace = spacer(), rightSpace = spacer()
        add(horizontal([shuffleButton, leftSpace, previousButton, playButton, nextButton, rightSpace, addButton]))
        leftSpace.widthAnchor.constraint(equalTo: rightSpace.widthAnchor).isActive = true
        seek.target = self; seek.action = #selector(seekChanged(_:)); seek.isContinuous = false
        seek.controlSize = .small; seek.setAccessibilityLabel("Playback position")
        volume.target = self; volume.action = #selector(volumeChanged(_:)); volume.isContinuous = true
        volume.controlSize = .small; volume.setAccessibilityLabel("Volume")
        for label in [elapsed, remaining] {
            label.font = .monospacedDigitSystemFont(ofSize: 10, weight: .regular)
            label.textColor = .secondaryLabelColor
            label.widthAnchor.constraint(equalToConstant: 38).isActive = true
        }
        remaining.alignment = .right
        add(horizontal([elapsed, seek, remaining], spacing: 4))
        let speaker = NSImageView(image: NSImage(systemSymbolName: "speaker.wave.2", accessibilityDescription: "Volume")!)
        speaker.contentTintColor = .secondaryLabelColor
        volume.widthAnchor.constraint(equalToConstant: 110).isActive = true
        playbackStatus.font = .systemFont(ofSize: 10); playbackStatus.textColor = .secondaryLabelColor
        playbackStatus.lineBreakMode = .byTruncatingTail; playbackStatus.alignment = .right
        playbackStatus.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        spinner(playbackSpinner)
        add(horizontal([speaker, volume, spacer(), playbackSpinner, playbackStatus], spacing: 6))
        add(separator())
        search.placeholderString = "Search YouTube Music"
        search.delegate = self; search.target = self; search.action = #selector(searchNow(_:))
        search.sendsWholeSearchString = true
        search.setAccessibilityLabel("Search YouTube Music")
        add(search)
        sections.selectedSegment = 0; sections.segmentDistribution = .fillEqually
        sections.target = self; sections.action = #selector(changeSection(_:)); sections.controlSize = .small
        add(sections)
        symbol(backButton, "chevron.left", "Back", #selector(back(_:)), size: 22)
        symbol(refreshButton, "arrow.clockwise", "Refresh", #selector(refreshPage(_:)), size: 22)
        pageTitle.font = .systemFont(ofSize: 12, weight: .semibold); pageTitle.lineBreakMode = .byTruncatingTail
        pageTitle.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        pagePlay.target = self; pagePlay.action = #selector(playPage(_:)); pagePlay.controlSize = .small; pagePlay.bezelStyle = .rounded
        spinner(pageSpinner)
        add(horizontal([backButton, pageTitle, spacer(), pageSpinner, pagePlay, refreshButton], spacing: 6))
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("music"))
        table.addTableColumn(column); table.headerView = nil; table.rowHeight = 44
        table.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        table.intercellSpacing = NSSize(width: 0, height: 1)
        table.backgroundColor = .clear; table.style = .plain
        table.dataSource = self; table.delegate = self; table.target = self; table.action = #selector(activateRow(_:))
        table.activate = { [weak self] in self?.activateSelectedRow() }
        table.togglePlayback = { [weak self] in self?.toggle(nil) }
        table.setAccessibilityLabel("Music")
        scroll.documentView = table; scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true
        scroll.drawsBackground = false; scroll.borderType = .noBorder
        let list = NSView(); list.addSubview(scroll); list.addSubview(empty)
        for child in [scroll, empty] { child.translatesAutoresizingMaskIntoConstraints = false }
        NSLayoutConstraint.activate([
            scroll.leadingAnchor.constraint(equalTo: list.leadingAnchor), scroll.trailingAnchor.constraint(equalTo: list.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: list.topAnchor), scroll.bottomAnchor.constraint(equalTo: list.bottomAnchor),
            empty.leadingAnchor.constraint(equalTo: list.leadingAnchor, constant: 14), empty.trailingAnchor.constraint(equalTo: list.trailingAnchor, constant: -14),
            empty.centerYAnchor.constraint(equalTo: list.centerYAnchor), list.heightAnchor.constraint(greaterThanOrEqualToConstant: 110)
        ])
        empty.orientation = .vertical; empty.alignment = .centerX; empty.spacing = 12
        emptyLabel.font = .systemFont(ofSize: 12); emptyLabel.textColor = .secondaryLabelColor
        emptyLabel.alignment = .center; emptyLabel.preferredMaxLayoutWidth = 320
        empty.addArrangedSubview(emptyLabel)
        emptyLabel.widthAnchor.constraint(equalTo: empty.widthAnchor).isActive = true
        let signIn = NSButton(title: "Open YouTube Music", target: self, action: #selector(openSignIn(_:)))
        signIn.bezelStyle = .rounded; signIn.controlSize = .small
        reconnectButton.bezelStyle = .rounded; reconnectButton.controlSize = .small
        reconnectButton.target = self; reconnectButton.action = #selector(reconnect(_:))
        emptyActions.orientation = .horizontal; emptyActions.spacing = 8
        emptyActions.addArrangedSubview(signIn); emptyActions.addArrangedSubview(reconnectButton)
        empty.addArrangedSubview(emptyActions)
        add(list)
        moreButton.target = self; moreButton.action = #selector(loadMore(_:)); moreButton.bezelStyle = .rounded; moreButton.controlSize = .small
        add(moreButton)
        messageLabel.font = .systemFont(ofSize: 11); messageLabel.maximumNumberOfLines = 2
        messageLabel.isSelectable = true; messageLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        symbol(dismissButton, "xmark", "Dismiss message", #selector(dismissMessage(_:)), size: 20)
        messageRow.orientation = .horizontal; messageRow.alignment = .centerY; messageRow.spacing = 6
        messageRow.addArrangedSubview(messageLabel); messageRow.addArrangedSubview(dismissButton)
        add(messageRow)
        add(separator())
        accountButton.title = "Account"; accountButton.isBordered = false; accountButton.font = .systemFont(ofSize: 11)
        accountButton.alignment = .left; accountButton.imagePosition = .imageLeft
        accountButton.image = NSImage(systemSymbolName: "person.crop.circle", accessibilityDescription: nil)
        accountButton.cell?.lineBreakMode = .byTruncatingTail
        accountButton.target = self; accountButton.action = #selector(showAccount(_:))
        accountButton.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        symbol(quitButton, "power", "Quit YTfast", #selector(quit(_:)), size: 22)
        quitButton.keyEquivalent = "q"; quitButton.keyEquivalentModifierMask = .command
        add(horizontal([accountButton, spacer(), quitButton]))
    }

    func opened() {
        _ = view
        visible = true
        // Closing a popover cancels the debounce, not the user's query.
        if !search.stringValue.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { searchNow(nil) }
        render(force: true)
        loadCurrent()
    }
    func closed() {
        visible = false
        searchWork?.cancel(); searchWork = nil
        playbackSpinner.stopAnimation(nil); pageSpinner.stopAnimation(nil)
    }
    func apply(_ next: State) {
        let connected = !state.signed_in && next.signed_in
        let boundary = state.profile != next.profile || (state.signed_in && !next.signed_in)
        if boundary || !next.signed_in {
            pages.removeAll()
            if boundary {
                searchWork?.cancel(); searchWork = nil
                history.removeAll(); location = .library(max(0, sections.selectedSegment))
                search.stringValue = ""; renderedLocation = ""
            }
        }
        state = next
        if let updates = next.pages {
            pages = next.signed_in ? Dictionary(uniqueKeysWithValues: updates.map { ($0.key, $0) }) : [:]
            browserDirty = true
        }
        if boundary { browserDirty = true }
        accountInfo?.title = next.account
        reconnectItem?.title = next.account_checking ? "Connecting…" : "Reconnect"
        reconnectItem?.isEnabled = !next.account_checking
        if visible {
            render()
            if connected { onMainRunLoop { [weak self] in self?.loadCurrent() } }
        }
    }
    private func busy(_ spinner: NSProgressIndicator, _ active: Bool) {
        if active && visible { spinner.startAnimation(nil) } else { spinner.stopAnimation(nil) }
    }
    private func render(force: Bool = false) {
        setText(titleLabel, state.track?.title ?? (state.loading ? "Starting playback…" : "Nothing playing"))
        setText(artistLabel, state.track?.artist ?? "Choose a song below")
        titleLabel.toolTip = state.track?.title; artistLabel.toolTip = state.track?.artist
        let active = state.playing || state.loading
        let playLabel = active ? "Pause" : "Play"
        if playButton.toolTip != playLabel {
            playButton.image = NSImage(systemSymbolName: active ? "pause.fill" : "play.fill", accessibilityDescription: playLabel)
            playButton.toolTip = playLabel; playButton.setAccessibilityLabel(playLabel)
        }
        playButton.isEnabled = state.track != nil || state.loading
        previousButton.isEnabled = state.track != nil; nextButton.isEnabled = state.track != nil
        shuffleButton.contentTintColor = state.shuffle ? .controlAccentColor : .labelColor
        shuffleButton.setAccessibilityValue(state.shuffle ? "On" : "Off")
        addButton.isEnabled = state.signed_in && state.track != nil && !state.adding
        addButton.toolTip = state.adding ? "Adding song…" : "Add song to playlist"
        seek.isEnabled = state.duration > 0 && state.track != nil && !state.loading
        seek.maxValue = max(1, state.duration)
        if !(seek.cell?.isHighlighted ?? false) { seek.doubleValue = state.position }
        if !(volume.cell?.isHighlighted ?? false) { volume.doubleValue = state.volume }
        volume.toolTip = "Volume \(Int(state.volume))%"
        setText(elapsed, timeLabel(state.position)); setText(remaining, "−" + timeLabel(max(0, state.duration - state.position)))
        setText(playbackStatus, state.loading ? "Loading audio…" : (state.format ?? (state.track == nil ? "" : "Paused")))
        playbackStatus.toolTip = state.format
        busy(playbackSpinner, state.loading)
        search.isEnabled = state.signed_in; sections.isEnabled = state.signed_in
        accountButton.title = state.account_checking ? "Connecting…" : (state.signed_in ? state.account : "Connect account")
        accountButton.toolTip = state.account; accountButton.setAccessibilityLabel("Account. " + state.account)
        reconnectButton.isEnabled = !state.account_checking
        reconnectButton.title = state.account_checking ? "Connecting…" : "Reconnect"
        let page = pages[location.key]
        setText(pageTitle, location.song.map { "Add “\($0.title)”" } ?? (page?.title.isEmpty == false ? page!.title : location.title))
        pageTitle.toolTip = pageTitle.stringValue
        backButton.isHidden = history.isEmpty
        pagePlay.isHidden = location.song != nil || page?.play == nil
        pagePlay.isEnabled = state.signed_in
        refreshButton.isEnabled = state.signed_in && page?.loading != true
        busy(pageSpinner, state.signed_in && (page == nil || page?.loading == true))
        if force || browserDirty || renderedLocation != location.identity {
            let newRows = state.signed_in ? (page?.rows ?? []).filter { location.song == nil || $0.editable != nil } : []
            if force || renderedLocation != location.identity || newRows != rows {
            let moved = renderedLocation != location.identity
            let selection = table.selectedRow >= 0 && table.selectedRow < rows.count ? rows[table.selectedRow].identity : nil
            let origin = moved ? location.scroll : scroll.contentView.bounds.origin
            rows = newRows; table.reloadData()
            if !moved, let selection, let index = rows.firstIndex(where: { $0.identity == selection }) {
                table.selectRowIndexes(IndexSet(integer: index), byExtendingSelection: false)
            } else { table.deselectAll(nil) }
            scroll.contentView.scroll(to: origin); scroll.reflectScrolledClipView(scroll.contentView)
            renderedLocation = location.identity
            }
            browserDirty = false
        }
        empty.isHidden = !rows.isEmpty
        emptyActions.isHidden = state.signed_in
        if !state.signed_in {
            setText(emptyLabel, state.account_checking ? "Connecting to YouTube Music…" : "Sign in to YouTube Music in your browser, then reconnect.")
        } else if page == nil || page?.loading == true {
            setText(emptyLabel, location.song == nil ? "Loading \(location.title.lowercased())…" : "Loading your playlists…")
        } else {
            setText(emptyLabel, page?.message ?? (location.song == nil ? "No music here" : "No editable playlists loaded"))
        }
        moreButton.isHidden = page?.more != true
        moreButton.isEnabled = page?.loading != true
        moreButton.title = page?.loading == true ? "Loading…" : "Load more"
        let message = state.error ?? state.notice ?? (rows.isEmpty ? nil : page?.message)
        messageRow.isHidden = message == nil
        setText(messageLabel, message ?? "")
        messageLabel.textColor = state.error != nil || page?.message != nil ? .secondaryLabelColor : .labelColor
        messageLabel.toolTip = message
        dismissButton.isHidden = state.error == nil && state.notice == nil
    }

    private func loadCurrent(force: Bool = false) {
        guard visible && state.signed_in else { return }
        send(["op": "browse", "target": location.target, "force": force])
    }
    func navigate(_ next: Location, remember: Bool = true) {
        searchWork?.cancel(); searchWork = nil
        if remember {
            location.scroll = scroll.contentView.bounds.origin
            history.append(location)
            if history.count > 16 { history.removeFirst() }
        }
        location = next; render(force: true); loadCurrent()
    }
    @objc func changeSection(_ sender: NSSegmentedControl) {
        searchWork?.cancel(); search.stringValue = ""; history.removeAll()
        navigate(.library(sender.selectedSegment), remember: false)
    }
    @objc func back(_ sender: Any?) {
        guard let previous = history.popLast() else { return }
        if !previous.key.hasPrefix("search:") { search.stringValue = "" }
        navigate(previous, remember: false)
    }
    @objc func refreshPage(_ sender: Any?) { loadCurrent(force: true) }
    @objc func loadMore(_ sender: Any?) { send(["op": "more", "key": location.key]) }
    @objc func playPage(_ sender: Any?) {
        if let target = pages[location.key]?.play { send(["op": "play", "target": target]) }
    }
    @objc func toggle(_ sender: Any?) { send(["op": "transport", "action": "toggle"]) }
    @objc func previous(_ sender: Any?) { send(["op": "transport", "action": "previous"]) }
    @objc func next(_ sender: Any?) { send(["op": "transport", "action": "next"]) }
    @objc func shuffle(_ sender: Any?) { send(["op": "shuffle"]) }
    @objc func seekChanged(_ sender: NSSlider) { send(["op": "seek", "value": sender.doubleValue]) }
    @objc func volumeChanged(_ sender: NSSlider) { send(["op": "volume", "value": sender.doubleValue]) }
    @objc func dismissMessage(_ sender: Any?) { send(["op": "dismiss"]) }
    @objc func reconnect(_ sender: Any?) { send(["op": "reconnect"]) }
    @objc func quit(_ sender: Any?) { send(["op": "quit"]) }
    @objc func addSong(_ sender: Any?) {
        guard let song = state.track, state.signed_in, !state.adding else { return }
        var destination = Location.library(0); destination.song = song
        navigate(destination)
    }
    @objc func activateRow(_ sender: Any?) { activateSelectedRow() }
    func activateSelectedRow() {
        let index = table.selectedRow
        guard rows.indices.contains(index), state.signed_in else { return }
        let row = rows[index]
        if let song = location.song, let playlist = row.editable {
            send(["op": "add", "playlist": playlist, "video": song.id])
            back(nil)
        } else if let target = row.browse, let destination = Location.from(target, title: row.title) {
            navigate(destination)
        } else if let target = row.play { send(["op": "play", "target": target]) }
    }
    func numberOfRows(in tableView: NSTableView) -> Int { rows.count }
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row index: Int) -> NSView? {
        guard rows.indices.contains(index) else { return nil }
        let identifier = NSUserInterfaceItemIdentifier("music-cell")
        let cell = tableView.makeView(withIdentifier: identifier, owner: self) as? MusicCell ?? MusicCell()
        cell.identifier = identifier
        let row = rows[index]
        setText(cell.title, row.title); setText(cell.subtitle, row.subtitle)
        cell.toolTip = row.subtitle.isEmpty ? row.title : row.title + "\n" + row.subtitle
        let icon = location.song != nil ? "plus" : (row.browse != nil ? "chevron.right" : "play.fill")
        cell.accessory.image = NSImage(systemSymbolName: icon, accessibilityDescription: nil)
        cell.setAccessibilityLabel(row.title + (row.subtitle.isEmpty ? "" : ", " + row.subtitle))
        return cell
    }

    func controlTextDidChange(_ notification: Notification) {
        guard notification.object as AnyObject? === search else { return }
        searchWork?.cancel()
        if search.stringValue.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            history.removeAll(); navigate(.library(max(0, sections.selectedSegment)), remember: false)
        } else {
            let work = DispatchWorkItem { [weak self] in self?.searchNow(nil) }
            searchWork = work; DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(280), execute: work)
        }
    }
    func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
        guard control === search else { return false }
        if commandSelector == NSSelectorFromString("moveDown:") || commandSelector == NSSelectorFromString("insertNewline:") {
            searchNow(nil)
            if !rows.isEmpty {
                if table.selectedRow < 0 { table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false) }
                view.window?.makeFirstResponder(table)
            }
            return true
        }
        if commandSelector == NSSelectorFromString("cancelOperation:") && !search.stringValue.isEmpty {
            search.stringValue = ""; history.removeAll()
            navigate(.library(max(0, sections.selectedSegment)), remember: false)
            return true
        }
        return false
    }
    @objc func searchNow(_ sender: Any?) {
        searchWork?.cancel(); searchWork = nil
        let query = search.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty, state.signed_in else { return }
        let target = encodeTarget("Search", ["query": String(query.prefix(300)), "params": NSNull()])
        guard let destination = Location.from(target, title: "Search results"), destination.key != location.key else { return }
        history = [.library(max(0, sections.selectedSegment))]
        navigate(destination, remember: false)
    }

    func makeAccountMenu() -> NSMenu {
        let menu = NSMenu(); menu.autoenablesItems = false
        @discardableResult
        func item(_ title: String, _ action: Selector? = nil) -> NSMenuItem {
            let entry = NSMenuItem(title: title, action: action, keyEquivalent: "")
            entry.target = self; entry.isEnabled = action != nil; menu.addItem(entry); return entry
        }
        accountInfo = item(state.account)
        accountInfo?.toolTip = state.account
        item("Open YouTube Music to sign in", #selector(openSignIn(_:)))
        reconnectItem = item(state.account_checking ? "Connecting…" : "Reconnect", #selector(reconnect(_:)))
        reconnectItem?.isEnabled = !state.account_checking
        if !state.profiles.isEmpty {
            menu.addItem(.separator())
            for profile in state.profiles {
                let entry = item(profile.label, #selector(selectProfile(_:)))
                entry.representedObject = profile.id; entry.state = state.profile == profile.id ? .on : .off
                entry.isEnabled = !state.account_checking
            }
        }
        menu.addItem(.separator())
        item("Full Disk Access…", #selector(openDiskAccess(_:)))
        item("Connection help…", #selector(connectionHelp(_:)))
        return menu
    }
    @objc func showAccount(_ sender: Any?) {
        accountMenu = makeAccountMenu()
        accountMenu?.popUp(positioning: nil, at: NSPoint(x: 0, y: accountButton.bounds.maxY + 4), in: accountButton)
        accountMenu = nil; accountInfo = nil; reconnectItem = nil
    }
    @objc private func selectProfile(_ sender: NSMenuItem) {
        guard let id = sender.representedObject as? String else { return }
        send(["op": "profile", "id": id])
    }
    /// Open the selected browser. Never select another browser's account or
    /// modify browser preferences; connecting still requires an explicit action.
    @objc func openSignIn(_ sender: Any?) {
        let workspace = NSWorkspace.shared
        let supported = ["com.google.Chrome", "com.brave.Browser", "org.chromium.Chromium"]
        let selected = state.profile?.lowercased() ?? ""
        let preferred = selected.contains("brave") ? supported[1] : (selected.contains("chromium") ? supported[2] : supported[0])
        let url = URL(string: "https://music.youtube.com/")!
        let bundle = workspace.urlForApplication(withBundleIdentifier: preferred)
            ?? (state.profile == nil ? supported.compactMap { workspace.urlForApplication(withBundleIdentifier: $0) }.first : nil)
        if let bundle {
            workspace.open([url], withApplicationAt: bundle, configuration: NSWorkspace.OpenConfiguration()) { _, error in
                if error != nil { onMainRunLoop { [weak self] in self?.connectionHelp(nil) } }
            }
        } else { connectionHelp(nil) }
    }
    @objc private func openDiskAccess(_ sender: Any?) {
        NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")!)
    }
    @objc private func connectionHelp(_ sender: Any?) {
        let alert = NSAlert()
        alert.messageText = "Connect YouTube Music"
        let profile = state.profiles.first { $0.id == state.profile }?.label
        let instruction = profile.map { "Sign in to YouTube Music in \($0). The browser may open its last-used profile; switch to this profile there." }
            ?? "Sign in to YouTube Music in Chrome, Brave or Chromium."
        alert.informativeText = instruction + " Give YTfast Full Disk Access in System Settings, then choose Reconnect. Allow the browser’s Safe Storage Keychain prompt if macOS asks.\n\nYTfast reads the selected browser’s session locally. Safari and Firefox sessions are not supported.\n\n" + state.account
        alert.addButton(withTitle: "Done")
        if let window = view.window { alert.beginSheetModal(for: window) } else { alert.runModal() }
    }
}
