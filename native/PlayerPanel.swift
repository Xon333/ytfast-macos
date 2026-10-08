import AppKit

final class PlayerPanel: NSViewController, NSTableViewDataSource, NSTableViewDelegate, NSSearchFieldDelegate {
    static let width: CGFloat = 360
    static let searchDelay: TimeInterval = 0.18
    let send: ([String: Any]) -> Void
    var sizeChanged: ((NSSize) -> Void)?
    var closeRequested: (() -> Void)?
    private(set) var state = State()
    private(set) var pages: [String: Page] = [:]
    private(set) var location = Location.library(0)
    private(set) var rows: [Row] = []
    private(set) var reloadCount = 0
    private(set) var showingAccount = false
    private var history: [Location] = []
    private var libraryIndex = 0
    private var visible = false
    private var browserDirty = true
    private var renderedLocation = ""
    private var searchWork: DispatchWorkItem?
    private var searchPending = false
    private var noticeWork: DispatchWorkItem?
    private var detailsVisible = false
    private var pendingSong: Song?
    private var seekSongID: String?
    private var lastAudibleVolume = 70.0
    private var lastSentVolume: Double?
    private var keyboardMonitor: Any?
    private var accountGeneration = 0
    private var browserError: String?

    let titleLabel = NSTextField(labelWithString: "YTfast")
    let artistLabel = NSTextField(labelWithString: "Choose a song")
    let playButton = SymbolButton("play.fill", "Play", size: 38, pointSize: 16, primary: true)
    let previousButton = SymbolButton("backward.end.fill", "Previous", size: 28, pointSize: 13)
    let nextButton = SymbolButton("forward.end.fill", "Next", size: 28, pointSize: 13)
    let shuffleButton = SymbolButton("shuffle", "Shuffle", size: 28)
    let addButton = SymbolButton("plus", "Add to playlist", size: 28)
    let muteButton = SymbolButton("speaker.wave.2", "Mute", size: 24, pointSize: 12)
    let seek = ValueSlider(value: 0, maximum: 1)
    let volume = ValueSlider(value: 70, maximum: 100)
    private let elapsed = NSTextField(labelWithString: "0:00")
    private let remaining = NSTextField(labelWithString: "−0:00")
    private let playbackStatus = NSTextField(labelWithString: "")
    private let playbackSpinner = NSProgressIndicator()
    let search = NSSearchField()
    let sections = NSSegmentedControl(labels: ["Playlists", "Liked", "Albums"], trackingMode: .selectOne, target: nil, action: nil)
    let backButton = SymbolButton("chevron.left", "Back", size: 28)
    private let pageTitle = NSTextField(labelWithString: "")
    let pagePlay = SymbolButton("play.fill", "Play this collection", size: 28)
    let refreshButton = SymbolButton("arrow.clockwise", "Refresh", size: 28)
    private let pageSpinner = NSProgressIndicator()
    let table = MusicTable()
    let scroll = NSScrollView()
    private let emptyLabel = NSTextField(wrappingLabelWithString: "")
    private let emptyIcon = NSImageView()
    let moreButton = NSButton(title: "Load more", target: nil, action: nil)
    private let messageLabel = NSTextField(wrappingLabelWithString: "")
    private let dismissButton = SymbolButton("xmark", "Dismiss", size: 22, pointSize: 10)
    private let retryButton = NSButton(title: "Retry", target: nil, action: nil)
    let accountButton = SymbolButton("person.crop.circle", "Account", size: 26)
    private let overflowButton = SymbolButton("ellipsis", "More", size: 26)
    private let appLabel = NSTextField(labelWithString: "YTfast")

    let profilePicker = NSPopUpButton(frame: .zero, pullsDown: false)
    private let profileLabel = NSTextField(labelWithString: "")
    let reconnectButton = ConnectButton()
    let browserButton = NSButton(title: "Sign in", target: nil, action: nil)
    private let accountTitle = NSTextField(labelWithString: "Connect YouTube Music")
    private let accountStatus = NSTextField(wrappingLabelWithString: "")
    private let accountSpinner = NSProgressIndicator()
    private let accountIcon = NSImageView()
    private let accessButton = NSButton(title: "Allow Full Disk Access…", target: nil, action: nil)
    private let detailsButton = NSButton(title: "Details", target: nil, action: nil)
    private let accountDetails = NSTextField(wrappingLabelWithString: "")

    private let content = NSStackView()
    private let player = ControlCard()
    private let transport = NSStackView()
    private let timeRow = NSStackView()
    private let utilityRow = NSStackView()
    private let tabs = NSStackView()
    private let navigationContent = NSView()
    private let context = NSStackView()
    private let browser = NSView()
    private let accountPane = ControlCard()
    private let messageRow = NSStackView()
    private let footerSeparator = NSBox()
    private let footer = NSStackView()
    private var browserHeight: NSLayoutConstraint!
    private var playerHeight: NSLayoutConstraint!
    private var permissionRequired: Bool {
        let reason = state.account.lowercased()
        return !state.signed_in && !state.account_checking &&
            ["permission", "not permitted", "access denied", "full disk", "readonly database"].contains { reason.contains($0) }
    }

    init(send: @escaping ([String: Any]) -> Void) {
        self.send = send
        super.init(nibName: nil, bundle: nil)
        preferredContentSize = NSSize(width: Self.width, height: 400)
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }
    deinit {
        searchWork?.cancel(); noticeWork?.cancel()
        if let keyboardMonitor { NSEvent.removeMonitor(keyboardMonitor) }
    }

    private func fixed(_ child: NSView, _ height: CGFloat) {
        child.translatesAutoresizingMaskIntoConstraints = false
        child.heightAnchor.constraint(equalToConstant: height).isActive = true
    }
    private func horizontal(_ stack: NSStackView, _ children: [NSView], spacing: CGFloat = 8) {
        stack.orientation = .horizontal; stack.alignment = .centerY; stack.spacing = spacing
        for child in children { stack.addArrangedSubview(child) }
    }
    private func gap() -> NSView {
        let view = NSView()
        view.setContentHuggingPriority(.defaultLow, for: .horizontal)
        return view
    }
    private func spinner(_ indicator: NSProgressIndicator) {
        indicator.style = .spinning; indicator.controlSize = .small
        indicator.isDisplayedWhenStopped = false
        indicator.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([indicator.widthAnchor.constraint(equalToConstant: 12), indicator.heightAnchor.constraint(equalToConstant: 12)])
    }
    private func button(_ button: NSButton, _ action: Selector) {
        button.target = self; button.action = action
        if !(button is SymbolButton) && !(button is ConnectButton) { button.bezelStyle = .rounded; button.controlSize = .regular; button.font = .systemFont(ofSize: 12) }
    }
    private func add(_ child: NSView) {
        child.translatesAutoresizingMaskIntoConstraints = false
        content.addArrangedSubview(child)
        child.widthAnchor.constraint(equalTo: content.widthAnchor).isActive = true
    }

    override func loadView() {
        let surface = NSVisualEffectView(frame: NSRect(origin: .zero, size: preferredContentSize))
        surface.material = .popover; surface.blendingMode = .withinWindow; surface.state = .active
        view = surface
        content.orientation = .vertical; content.alignment = .leading; content.spacing = 8
        content.detachesHiddenViews = true; content.translatesAutoresizingMaskIntoConstraints = false
        surface.addSubview(content)
        NSLayoutConstraint.activate([
            content.leadingAnchor.constraint(equalTo: surface.leadingAnchor, constant: 12),
            content.trailingAnchor.constraint(equalTo: surface.trailingAnchor, constant: -12),
            content.topAnchor.constraint(equalTo: surface.topAnchor, constant: 12),
            content.bottomAnchor.constraint(equalTo: surface.bottomAnchor, constant: -12)
        ])
        player.orientation = .vertical; player.alignment = .leading; player.spacing = 5
        player.edgeInsets = NSEdgeInsets(top: 10, left: 10, bottom: 10, right: 10)
        titleLabel.font = .systemFont(ofSize: 14, weight: .semibold)
        artistLabel.font = .systemFont(ofSize: 12); artistLabel.textColor = .secondaryLabelColor
        let metadata = NSStackView(views: [titleLabel, artistLabel])
        metadata.orientation = .vertical; metadata.alignment = .leading; metadata.spacing = 3
        metadata.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        for label in [titleLabel, artistLabel] {
            label.lineBreakMode = .byTruncatingTail
            label.widthAnchor.constraint(equalTo: metadata.widthAnchor).isActive = true
            label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        }
        let heading = NSStackView()
        horizontal(transport, [previousButton, playButton, nextButton], spacing: 4)
        horizontal(heading, [metadata, transport], spacing: 12)
        metadata.widthAnchor.constraint(greaterThanOrEqualToConstant: 100).isActive = true
        fixed(heading, 40)
        button(playButton, #selector(toggle(_:))); button(previousButton, #selector(previous(_:)))
        button(nextButton, #selector(next(_:))); button(shuffleButton, #selector(shuffle(_:)))
        button(addButton, #selector(addSong(_:))); button(muteButton, #selector(mute(_:)))
        shuffleButton.setAccessibilityRole(.checkBox)
        seek.target = self; seek.action = #selector(seekChanged(_:)); seek.setAccessibilityLabel("Playback position")
        seek.beganEditing = { [weak self] in self?.seekSongID = self?.state.track?.id }
        seek.endedEditing = { [weak self] in self?.commitSeek() }
        volume.target = self; volume.action = #selector(volumeChanged(_:)); volume.setAccessibilityLabel("Volume")
        volume.beganEditing = { [weak self] in self?.lastSentVolume = nil }
        volume.endedEditing = { [weak self] in guard let self else { return }; self.volumeChanged(self.volume) }
        volume.widthAnchor.constraint(equalToConstant: 72).isActive = true
        for label in [elapsed, remaining, playbackStatus] {
            label.font = .monospacedDigitSystemFont(ofSize: 10, weight: .regular)
            label.textColor = .secondaryLabelColor
        }
        playbackStatus.font = .systemFont(ofSize: 10, weight: .medium)
        playbackStatus.lineBreakMode = .byTruncatingTail
        playbackStatus.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        elapsed.alignment = .left; remaining.alignment = .right
        elapsed.widthAnchor.constraint(equalToConstant: 31).isActive = true
        remaining.widthAnchor.constraint(equalToConstant: 35).isActive = true
        spinner(playbackSpinner)
        horizontal(timeRow, [elapsed, seek, remaining], spacing: 4)
        seek.setContentHuggingPriority(.defaultLow, for: .horizontal)
        fixed(timeRow, 20)
        let quality = NSStackView()
        horizontal(quality, [playbackSpinner, playbackStatus], spacing: 4)
        quality.detachesHiddenViews = true
        quality.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        horizontal(utilityRow, [shuffleButton, addButton, quality, gap(), muteButton, volume], spacing: 4)
        fixed(utilityRow, 28)
        for child in [heading, timeRow, utilityRow] {
            player.addArrangedSubview(child)
            child.widthAnchor.constraint(equalTo: player.widthAnchor, constant: -20).isActive = true
        }
        add(player); playerHeight = player.heightAnchor.constraint(equalToConstant: 118); playerHeight.isActive = true
        footerSeparator.boxType = .separator; fixed(footerSeparator, 1)
        search.placeholderString = "Search music"
        search.font = .systemFont(ofSize: 12); search.focusRingType = .none
        search.delegate = self; search.target = self; search.action = #selector(searchNow(_:))
        search.sendsWholeSearchString = true; search.setAccessibilityLabel("Search YouTube Music")
        fixed(search, 28); add(search)
        sections.selectedSegment = 0; sections.segmentDistribution = .fillEqually; sections.controlSize = .regular
        sections.font = .systemFont(ofSize: 12, weight: .medium)
        sections.target = self; sections.action = #selector(changeSection(_:))
        sections.setContentHuggingPriority(.defaultLow, for: .horizontal)
        button(refreshButton, #selector(refreshPage(_:))); spinner(pageSpinner)
        button(backButton, #selector(back(_:))); button(pagePlay, #selector(playPage(_:)))
        pageTitle.font = .systemFont(ofSize: 12, weight: .semibold)
        pageTitle.lineBreakMode = .byTruncatingTail
        pageTitle.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        horizontal(context, [backButton, pageTitle, gap()], spacing: 4)
        // Library sections and collection navigation occupy the same row.
        // The refresh indicator replaces its button without shifting content.
        for child in [sections, context] {
            child.translatesAutoresizingMaskIntoConstraints = false; navigationContent.addSubview(child)
            NSLayoutConstraint.activate([
                child.leadingAnchor.constraint(equalTo: navigationContent.leadingAnchor),
                child.trailingAnchor.constraint(equalTo: navigationContent.trailingAnchor),
                child.centerYAnchor.constraint(equalTo: navigationContent.centerYAnchor)
            ])
        }
        let refreshSlot = NSView()
        for child in [refreshButton, pageSpinner] {
            child.translatesAutoresizingMaskIntoConstraints = false; refreshSlot.addSubview(child)
            NSLayoutConstraint.activate([
                child.centerXAnchor.constraint(equalTo: refreshSlot.centerXAnchor),
                child.centerYAnchor.constraint(equalTo: refreshSlot.centerYAnchor)
            ])
        }
        refreshSlot.widthAnchor.constraint(equalToConstant: 28).isActive = true
        refreshSlot.heightAnchor.constraint(equalToConstant: 28).isActive = true
        horizontal(tabs, [navigationContent, pagePlay, refreshSlot], spacing: 4)
        tabs.detachesHiddenViews = true
        navigationContent.setContentHuggingPriority(.defaultLow, for: .horizontal)
        navigationContent.heightAnchor.constraint(equalToConstant: 30).isActive = true
        fixed(tabs, 30); add(tabs)
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("music"))
        table.addTableColumn(column); table.headerView = nil; table.rowHeight = 44
        table.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        table.intercellSpacing = .zero; table.backgroundColor = .clear; table.style = .plain
        table.dataSource = self; table.delegate = self
        table.activate = { [weak self] in self?.activateSelectedRow() }
        table.togglePlayback = { [weak self] in self?.toggle(nil) }
        table.goBack = { [weak self] in self?.escape() }
        table.rowMenu = { [weak self] index in self?.makeRowMenu(index) }
        table.setAccessibilityLabel("Music")
        scroll.documentView = table; scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true
        scroll.drawsBackground = false; scroll.borderType = .noBorder
        emptyLabel.font = .systemFont(ofSize: 12); emptyLabel.textColor = .secondaryLabelColor
        emptyLabel.alignment = .center; emptyLabel.maximumNumberOfLines = 2
        emptyIcon.contentTintColor = .tertiaryLabelColor
        for child in [scroll, emptyIcon, emptyLabel, accountPane] { child.translatesAutoresizingMaskIntoConstraints = false; browser.addSubview(child) }
        NSLayoutConstraint.activate([
            scroll.leadingAnchor.constraint(equalTo: browser.leadingAnchor), scroll.trailingAnchor.constraint(equalTo: browser.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: browser.topAnchor), scroll.bottomAnchor.constraint(equalTo: browser.bottomAnchor),
            emptyLabel.leadingAnchor.constraint(equalTo: browser.leadingAnchor, constant: 20), emptyLabel.trailingAnchor.constraint(equalTo: browser.trailingAnchor, constant: -20),
            emptyLabel.topAnchor.constraint(equalTo: emptyIcon.bottomAnchor, constant: 10),
            emptyIcon.centerXAnchor.constraint(equalTo: browser.centerXAnchor), emptyIcon.centerYAnchor.constraint(equalTo: browser.centerYAnchor, constant: -15),
            emptyIcon.widthAnchor.constraint(equalToConstant: 28), emptyIcon.heightAnchor.constraint(equalToConstant: 28),
            accountPane.leadingAnchor.constraint(equalTo: browser.leadingAnchor), accountPane.trailingAnchor.constraint(equalTo: browser.trailingAnchor),
            accountPane.centerYAnchor.constraint(equalTo: browser.centerYAnchor)
        ])
        makeAccountPane()
        add(browser); browserHeight = browser.heightAnchor.constraint(equalToConstant: 176); browserHeight.isActive = true
        button(moreButton, #selector(loadMore(_:))); fixed(moreButton, 26); add(moreButton)
        messageLabel.font = .systemFont(ofSize: 11); messageLabel.maximumNumberOfLines = 2
        messageLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        button(dismissButton, #selector(dismissMessage(_:))); button(retryButton, #selector(refreshPage(_:)))
        horizontal(messageRow, [messageLabel, retryButton, dismissButton], spacing: 5)
        fixed(messageRow, 34); add(messageRow)
        add(footerSeparator)
        button(accountButton, #selector(showAccount(_:)))
        button(overflowButton, #selector(showMore(_:)))
        appLabel.font = .systemFont(ofSize: 11, weight: .medium); appLabel.textColor = .secondaryLabelColor
        horizontal(footer, [appLabel, gap(), accountButton, overflowButton], spacing: 5)
        fixed(footer, 26); add(footer)
        browserDirty = true
        render()
    }

    private func makeAccountPane() {
        accountPane.orientation = .vertical; accountPane.alignment = .centerX; accountPane.spacing = 10
        accountPane.edgeInsets = NSEdgeInsets(top: 14, left: 12, bottom: 12, right: 12)
        accountPane.detachesHiddenViews = true
        accountTitle.font = .systemFont(ofSize: 15, weight: .semibold)
        accountTitle.lineBreakMode = .byTruncatingTail
        accountStatus.font = .systemFont(ofSize: 11); accountStatus.textColor = .secondaryLabelColor
        accountStatus.alignment = .left; accountStatus.maximumNumberOfLines = 2
        accountStatus.preferredMaxLayoutWidth = 248
        let heading = NSStackView(); spinner(accountSpinner)
        accountIcon.contentTintColor = .controlAccentColor
        accountIcon.translatesAutoresizingMaskIntoConstraints = false
        accountIcon.widthAnchor.constraint(equalToConstant: 32).isActive = true
        accountIcon.heightAnchor.constraint(equalToConstant: 32).isActive = true
        let copy = NSStackView(views: [accountTitle, accountStatus])
        copy.orientation = .vertical; copy.alignment = .leading; copy.spacing = 4
        for label in [accountTitle, accountStatus] {
            label.widthAnchor.constraint(equalTo: copy.widthAnchor).isActive = true
            label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        }
        horizontal(heading, [accountIcon, copy, accountSpinner], spacing: 10)
        heading.detachesHiddenViews = true
        accountPane.addArrangedSubview(heading)
        heading.widthAnchor.constraint(equalTo: accountPane.widthAnchor, constant: -24).isActive = true
        fixed(heading, 50)
        profilePicker.font = .systemFont(ofSize: 12); profilePicker.controlSize = .regular
        profilePicker.target = self; profilePicker.action = #selector(selectProfile(_:))
        profilePicker.setAccessibilityLabel("Browser profile")
        accountPane.addArrangedSubview(profilePicker)
        profilePicker.widthAnchor.constraint(equalTo: accountPane.widthAnchor, constant: -24).isActive = true
        profileLabel.font = .systemFont(ofSize: 11, weight: .medium)
        profileLabel.alignment = .center; profileLabel.lineBreakMode = .byTruncatingMiddle
        accountPane.addArrangedSubview(profileLabel)
        profileLabel.widthAnchor.constraint(equalTo: accountPane.widthAnchor, constant: -24).isActive = true
        button(reconnectButton, #selector(reconnect(_:))); button(browserButton, #selector(openSignIn(_:)))
        reconnectButton.bezelColor = .controlAccentColor
        let actions = NSStackView(); horizontal(actions, [browserButton, reconnectButton], spacing: 10)
        accountPane.addArrangedSubview(actions)
        button(accessButton, #selector(openDiskAccess(_:))); accessButton.isBordered = false
        button(detailsButton, #selector(toggleDetails(_:))); detailsButton.isBordered = false
        let help = NSStackView(), helpLeft = gap(), helpRight = gap()
        horizontal(help, [helpLeft, accessButton, detailsButton, helpRight], spacing: 8)
        help.detachesHiddenViews = true
        helpLeft.widthAnchor.constraint(equalTo: helpRight.widthAnchor).isActive = true
        accountPane.addArrangedSubview(help)
        help.widthAnchor.constraint(equalTo: accountPane.widthAnchor, constant: -24).isActive = true
        accountDetails.font = .systemFont(ofSize: 10); accountDetails.textColor = .secondaryLabelColor
        accountDetails.maximumNumberOfLines = 4; accountDetails.isSelectable = true
        accountPane.addArrangedSubview(accountDetails)
        accountDetails.widthAnchor.constraint(equalTo: accountPane.widthAnchor, constant: -24).isActive = true
        fixed(accountDetails, 54)
    }

    func opened() {
        _ = view; visible = true
        if keyboardMonitor == nil {
            keyboardMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
                guard let self, self.visible, event.window === self.view.window, event.keyCode == 53,
                      event.modifierFlags.intersection([.command, .control, .option]).isEmpty else { return event }
                // Escape cancels text composition before navigating away.
                if let editor = self.view.window?.firstResponder as? NSTextView, editor.hasMarkedText() { return event }
                self.escape(); return nil
            }
        }
        browserDirty = true
        render()
        if searchPending { submitSearch() } else { loadCurrent() }
        onMainRunLoop { [weak self] in
            guard let self, self.visible, let window = self.view.window,
                  !(window.firstResponder is NSTextView) else { return }
            window.makeFirstResponder(self.state.signed_in && !self.showingAccount ? self.table : self.reconnectButton)
        }
    }
    func closed() {
        visible = false
        location.scroll = scroll.contentView.bounds.origin
        searchWork?.cancel(); searchWork = nil
        if let keyboardMonitor { NSEvent.removeMonitor(keyboardMonitor); self.keyboardMonitor = nil }
        for indicator in [playbackSpinner, pageSpinner, accountSpinner] { indicator.stopAnimation(nil) }
    }
    func apply(_ next: State) {
        let connected = !state.signed_in && next.signed_in
        let boundary = state.profile != next.profile || (state.signed_in && !next.signed_in)
        let chromeChanged = state.account != next.account || state.account_checking != next.account_checking || state.account_unverified != next.account_unverified ||
            state.signed_in != next.signed_in || state.profiles != next.profiles ||
            state.error != next.error || state.notice != next.notice || state.adding != next.adding
        let trackChanged = state.track?.id != next.track?.id
        let transportChanged = trackChanged || state.playing != next.playing || state.loading != next.loading
        let hasAudioChanged = (state.track != nil || state.loading) != (next.track != nil || next.loading)
        if trackChanged || (state.loading && !next.loading) || next.error != nil { pendingSong = nil }
        if boundary {
            accountGeneration += 1; browserError = nil
            searchWork?.cancel(); searchWork = nil; searchPending = false
            pages.removeAll(); history.removeAll(); location = .library(libraryIndex)
            renderedLocation = ""; pendingSong = nil; detailsVisible = false
        }
        if !next.signed_in { pages.removeAll() }
        if connected { showingAccount = false; browserError = nil }
        if next.volume > 0 { lastAudibleVolume = next.volume }
        if next.notice != state.notice {
            noticeWork?.cancel()
            if let notice = next.notice, !next.adding, next.error == nil {
                let work = DispatchWorkItem { [weak self] in
                    guard let self, self.state.notice == notice, self.state.error == nil else { return }
                    self.send(["op": "dismiss"])
                }
                noticeWork = work; DispatchQueue.main.asyncAfter(deadline: .now() + 3, execute: work)
            }
        }
        state = next
        // Pages have one native owner; transport snapshots need not retain a
        // second copy of the complete catalogue.
        state.pages = nil
        if let updates = next.pages {
            pages = next.signed_in ? Dictionary(uniqueKeysWithValues: updates.map { ($0.key, $0) }) : [:]
            browserDirty = true
        }
        browserDirty = browserDirty || boundary || chromeChanged || trackChanged || hasAudioChanged
        if visible {
            render()
            if transportChanged { updateVisibleRows() }
        }
        if connected {
            // Warm only the small playlist index, including its disk snapshot.
            // No stream resolver or audio process is started by this request.
            onMainRunLoop { [weak self] in
                guard let self, self.state.signed_in else { return }
                self.send(["op": "browse", "target": browseTarget(playlistID), "force": false])
            }
        }
    }
    private func busy(_ indicator: NSProgressIndicator, _ active: Bool) {
        indicator.isHidden = !active
        if active && visible { indicator.startAnimation(nil) } else { indicator.stopAnimation(nil) }
    }
    private func render() {
        let song = state.loading ? (pendingSong ?? state.track) : state.track
        setText(titleLabel, song?.title ?? (state.loading ? "Starting playback…" : "YTfast"))
        setText(artistLabel, song?.artist ?? "Choose a song")
        titleLabel.toolTip = song?.title; artistLabel.toolTip = song?.artist
        playButton.setSymbol(state.loading ? "stop.fill" : (state.playing ? "pause.fill" : "play.fill"), state.loading ? "Cancel loading" : (state.playing ? "Pause" : "Play"))
        playButton.isEnabled = state.track != nil || state.loading
        previousButton.isEnabled = state.track != nil && !state.account_checking
        nextButton.isEnabled = state.track != nil && !state.account_checking
        shuffleButton.isOn = state.shuffle; shuffleButton.setAccessibilityValue(state.shuffle ? 1 : 0)
        shuffleButton.isEnabled = state.track != nil && !state.account_checking
        shuffleButton.setSymbol("shuffle", state.shuffle ? "Shuffle on" : "Shuffle off")
        addButton.isEnabled = state.signed_in && state.track != nil && !state.loading && !state.adding
        addButton.setSymbol("plus", state.adding ? "Adding…" : "Add to playlist")
        seek.isEnabled = state.track != nil && state.duration > 0 && !state.loading
        if !seek.editing { seek.maxValue = max(1, state.duration); seek.doubleValue = state.position }
        if !volume.editing { volume.doubleValue = state.volume }
        if !seek.editing { renderTimes(state.position) }
        setText(playbackStatus, state.loading ? "Loading…" : (state.format?.components(separatedBy: " (").first ?? ""))
        playbackStatus.toolTip = state.format
        busy(playbackSpinner, state.loading)
        muteButton.setSymbol(state.volume == 0 ? "speaker.slash" : "speaker.wave.2", state.volume == 0 ? "Unmute" : "Mute")
        volume.toolTip = "Volume \(Int(volume.doubleValue.rounded()))%"
        if browserDirty || renderedLocation != location.identity { renderBrowser() }
    }
    private func renderTimes(_ position: Double) {
        setText(elapsed, timeLabel(position)); setText(remaining, "−" + timeLabel(max(0, state.duration - position)))
    }
    private func renderBrowser() {
        browserDirty = false
        let account = showingAccount || !state.signed_in
        let hasAudio = state.track != nil || state.loading
        player.isHidden = !hasAudio
        search.isHidden = account || location.song != nil; tabs.isHidden = account
        search.isEnabled = state.signed_in; sections.isEnabled = state.signed_in
        let page = pages[location.key]
        let nested = location.libraryIndex == nil || location.song != nil || !history.isEmpty
        context.isHidden = !nested; sections.isHidden = nested
        backButton.isHidden = false
        scroll.isHidden = account; accountPane.isHidden = !account
        accountButton.isHidden = !state.signed_in
        accountButton.isOn = showingAccount
        accountButton.setSymbol(showingAccount ? "music.note.list" : "person.crop.circle", showingAccount ? "Back to library" : "Account · \(state.account)")
        let newRows = state.signed_in ? (page?.rows ?? []).filter { location.song == nil || $0.editable != nil } : []
        let moved = renderedLocation != location.identity
        if moved || newRows != rows {
            let selected = table.selectedRow >= 0 && table.selectedRow < rows.count ? rows[table.selectedRow].identity : nil
            let origin = moved ? location.scroll : scroll.contentView.bounds.origin
            rows = newRows; table.reloadData(); reloadCount += 1
            if !moved, let selected, let index = rows.firstIndex(where: { $0.identity == selected }) {
                table.selectRowIndexes(IndexSet(integer: index), byExtendingSelection: false)
            } else { table.deselectAll(nil) }
            table.layoutSubtreeIfNeeded()
            scroll.contentView.scroll(to: origin); scroll.reflectScrolledClipView(scroll.contentView)
            renderedLocation = location.identity
        }
        let loading = searchPending || page == nil || page?.loading == true
        busy(pageSpinner, !account && loading)
        refreshButton.isEnabled = !loading; refreshButton.isHidden = loading
        pagePlay.isHidden = account || location.song != nil || page?.play == nil
        pagePlay.isEnabled = state.signed_in && !state.account_checking
        pagePlay.setSymbol("play.fill", "Play \(page?.title ?? location.title)")
        setText(pageTitle, location.song.map { "Add “\($0.title)”" } ?? (page?.title.isEmpty == false ? page!.title : location.title))
        pageTitle.toolTip = pageTitle.stringValue
        sections.selectedSegment = nested ? -1 : libraryIndex
        if search.stringValue != location.query { search.stringValue = location.query }
        moreButton.isHidden = account || page?.more != true
        moreButton.isEnabled = !loading; moreButton.title = loading ? "Loading…" : "Load more"
        emptyLabel.isHidden = account || !rows.isEmpty
        emptyIcon.isHidden = emptyLabel.isHidden
        emptyIcon.image = NSImage(systemSymbolName: loading ? "ellipsis" : (page?.message != nil ? "wifi.exclamationmark" : (location.query.isEmpty ? "music.note.list" : "magnifyingglass")), accessibilityDescription: nil)
        setText(emptyLabel, loading ? (location.query.isEmpty ? "Loading…" : "Searching…") : (page?.message != nil ? "Couldn't load music" : (location.song != nil ? "No editable playlists" : (location.query.isEmpty ? "No music here yet" : "No results"))))
        let message = state.error ?? state.notice ?? (account ? nil : page?.message)
        messageRow.isHidden = message == nil
        setText(messageLabel, message ?? ""); messageLabel.toolTip = message
        retryButton.isHidden = account || page?.message == nil
        dismissButton.isHidden = state.error == nil && state.notice == nil
        if account { renderAccount() } else { busy(accountSpinner, false) }
        browserHeight.constant = account ? (detailsVisible ? 258 : 196) : (rows.isEmpty ? 104 : CGFloat(min(6, rows.count)) * 44)
        let heights: [(NSView, CGFloat)] = [(player, playerHeight.constant), (search, 28), (tabs, 30), (browser, browserHeight.constant), (moreButton, 26), (messageRow, 34), (footerSeparator, 1), (footer, 26)]
        let shown = heights.filter { !$0.0.isHidden }
        let size = NSSize(width: Self.width, height: shown.reduce(24) { $0 + $1.1 } + CGFloat(max(0, shown.count - 1)) * 8)
        if preferredContentSize != size { preferredContentSize = size; sizeChanged?(size) }
    }
    private func renderAccount() {
        let selected = state.profiles.first { $0.id == state.profile }
        let missing = state.profile != nil && selected == nil
        let browserName = selected?.label.components(separatedBy: " · ").first ?? "your browser"
        let name = state.account.components(separatedBy: " · ").first ?? state.account
        let heading: String
        let status: String
        let symbol: String
        if browserError != nil {
            heading = "Couldn't open \(browserName)"; status = "Open the selected browser and sign in to YouTube Music."; symbol = "exclamationmark.circle"
        } else if state.account_checking {
            heading = "Connecting…"; status = "Checking your selected browser session."; symbol = "person.crop.circle"
        } else if state.signed_in {
            heading = name; status = "Connected to YouTube Music"; symbol = "checkmark.circle.fill"
        } else if state.account_unverified {
            heading = "Connection not verified"; status = "Reconnect to check this browser session."; symbol = "wifi.exclamationmark"
        } else if permissionRequired {
            heading = "Allow browser access"; status = "Enable YTfast in Full Disk Access, then reopen."; symbol = "lock.shield"
        } else if missing {
            heading = "Profile unavailable"; status = "Choose a listed profile or restore the selected one."; symbol = "person.crop.circle.badge.exclamationmark"
        } else if state.account.localizedCaseInsensitiveContains("keychain") {
            heading = "Unlock the browser session"; status = "Reconnect and allow the browser’s Safe Storage prompt."; symbol = "key"
        } else {
            heading = "Connect YouTube Music"; status = "Sign in in \(browserName), then connect."; symbol = "music.note"
        }
        setText(accountTitle, heading); setText(accountStatus, status)
        accountTitle.toolTip = heading
        accountIcon.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)?
            .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: 26, weight: .regular))
        accountIcon.contentTintColor = state.signed_in ? .systemGreen : .controlAccentColor
        busy(accountSpinner, state.account_checking)
        let choices = (missing ? [Profile(id: state.profile!, label: "Selected profile unavailable")] : []) + state.profiles
        if profilePicker.itemArray.compactMap({ $0.representedObject as? String }) != choices.map(\.id) ||
           profilePicker.itemTitles != choices.map(\.label) {
            profilePicker.removeAllItems()
            for profile in choices {
                profilePicker.addItem(withTitle: profile.label)
                profilePicker.lastItem?.representedObject = profile.id
                profilePicker.lastItem?.isEnabled = !missing || profile.id != state.profile
            }
        }
        if let selected = state.profile, let item = profilePicker.itemArray.first(where: { $0.representedObject as? String == selected }) { profilePicker.select(item) }
        profilePicker.isHidden = choices.count <= 1
        profilePicker.isEnabled = !state.account_checking && choices.count > 1
        profileLabel.isHidden = !profilePicker.isHidden
        setText(profileLabel, selected?.label ?? (missing ? "Selected profile unavailable" : (state.profiles.first?.label ?? "Chrome, Brave or Chromium")))
        profilePicker.menu?.autoenablesItems = false
        reconnectButton.title = state.account_checking ? "Connecting…" : (state.signed_in || state.profile != nil ? "Reconnect" : "Connect")
        reconnectButton.isEnabled = !state.account_checking
        browserButton.title = state.signed_in || state.account_unverified ? "Open browser" : "Sign in"
        accessButton.isHidden = !permissionRequired
        detailsButton.isHidden = browserError == nil && (state.signed_in || state.account_checking)
        detailsButton.title = detailsVisible ? "Hide details" : "Details"
        accountDetails.isHidden = !detailsVisible
        setText(accountDetails, browserError ?? state.account); accountDetails.toolTip = browserError ?? state.account
    }

    private func updateVisibleRows() {
        table.enumerateAvailableRowViews { [weak self] rowView, index in
            guard let self, self.rows.indices.contains(index), let cell = rowView.view(atColumn: 0) as? MusicCell else { return }
            self.configure(cell, index)
        }
    }
    private func configure(_ cell: MusicCell, _ index: Int) {
        let row = rows[index]
        cell.configure(row, adding: location.song != nil, current: row.video != nil && row.video == state.track?.id,
            playing: state.playing, loading: state.loading, enabled: state.signed_in && !state.account_checking && !state.adding)
        let generation = accountGeneration
        cell.playAction = { [weak self] in self?.performRowAction(row, operation: "play", generation: generation) }
        cell.addAction = { [weak self] in self?.performRowAction(row, operation: "add", generation: generation) }
    }
    private func loadCurrent(force: Bool = false) {
        guard visible, state.signed_in, !showingAccount else { return }
        send(["op": "browse", "target": location.target, "force": force])
    }
    func navigate(_ next: Location, remember: Bool = true, load: Bool = true) {
        searchWork?.cancel(); searchWork = nil; searchPending = false
        if remember {
            location.scroll = scroll.contentView.bounds.origin; history.append(location)
            if history.count > 16 { history.removeFirst() }
        }
        showingAccount = false; location = next; browserDirty = true
        if let index = next.libraryIndex { libraryIndex = index }
        if visible { render() }
        if load { loadCurrent() }
    }
    @objc func changeSection(_ sender: NSSegmentedControl) {
        guard sender.selectedSegment >= 0 else { return }
        libraryIndex = sender.selectedSegment; history.removeAll()
        navigate(.library(libraryIndex), remember: false)
    }
    @objc func back(_ sender: Any?) {
        if showingAccount { showingAccount = false; browserDirty = true; render(); loadCurrent(); return }
        guard let previous = history.popLast() else {
            if location.libraryIndex == nil || location.song != nil { navigate(.library(libraryIndex), remember: false) }
            return
        }
        navigate(previous, remember: false)
    }
    func escape() {
        if showingAccount || !history.isEmpty { back(nil) } else { closeRequested?() }
    }
    @objc func refreshPage(_ sender: Any?) { loadCurrent(force: true) }
    @objc func loadMore(_ sender: Any?) { send(["op": "more", "key": location.key]) }
    @objc func playPage(_ sender: Any?) { if let target = pages[location.key]?.play { send(["op": "play", "target": target]) } }
    @objc func toggle(_ sender: Any?) { send(["op": "transport", "action": "toggle"]) }
    @objc func previous(_ sender: Any?) { send(["op": "transport", "action": "previous"]) }
    @objc func next(_ sender: Any?) { send(["op": "transport", "action": "next"]) }
    @objc func shuffle(_ sender: Any?) { send(["op": "shuffle"]) }
    @objc func mute(_ sender: Any?) {
        let value: Double = state.volume == 0 ? lastAudibleVolume : 0
        send(["op": "volume", "value": value])
    }
    @objc func seekChanged(_ sender: NSSlider) {
        renderTimes(sender.doubleValue)
        if !seek.editing {
            let value = sender.doubleValue
            send(["op": "seek", "value": value])
            sender.doubleValue = value; renderTimes(value)
        }
    }
    private func commitSeek() {
        if seekSongID == state.track?.id, state.track != nil, !state.loading { send(["op": "seek", "value": seek.doubleValue]) }
        else { seek.maxValue = max(1, state.duration); seek.doubleValue = state.position; renderTimes(state.position) }
        seekSongID = nil
    }
    @objc func volumeChanged(_ sender: NSSlider) {
        let value = sender.doubleValue.rounded()
        volume.toolTip = "Volume \(Int(value))%"
        guard !volume.editing || lastSentVolume != value else { return }
        lastSentVolume = value; send(["op": "volume", "value": value])
    }
    @objc func dismissMessage(_ sender: Any?) { send(["op": "dismiss"]) }
    @objc func reconnect(_ sender: Any?) { guard !state.account_checking else { return }; browserError = nil; send(["op": "reconnect"]) }
    @objc func quit(_ sender: Any?) { send(["op": "quit"]) }
    @objc func addSong(_ sender: Any?) {
        guard let song = state.track, state.signed_in, !state.adding, !state.loading else { return }
        beginAdding(song)
    }
    private func beginAdding(_ song: Song) {
        var destination = Location.library(0); destination.song = song
        navigate(destination)
    }
    func activateSelectedRow() {
        let index = table.selectedRow
        guard rows.indices.contains(index), state.signed_in, !state.account_checking else { return }
        let row = rows[index]
        if let song = location.song, let playlist = row.editable {
            send(["op": "add", "playlist": playlist, "video": song.id]); back(nil)
        } else if let target = row.browse, let destination = Location.from(target, title: row.title) {
            navigate(destination)
        } else if let target = row.play {
            play(row, target: target)
        }
    }
    private func play(_ row: Row, target: String) {
        if row.video != nil && row.video == state.track?.id { toggle(nil) }
        else {
            if let id = row.video { pendingSong = Song(id: id, title: row.title, artist: row.subtitle) }
            send(["op": "play", "target": target])
        }
    }
    private func performRowAction(_ row: Row, operation: String, generation: Int) {
        guard generation == accountGeneration, state.signed_in, !state.account_checking, !state.adding else { return }
        switch operation {
        case "play": if let target = row.play { play(row, target: target) }
        case "add": if let id = row.video { beginAdding(Song(id: id, title: row.title, artist: row.subtitle)) }
        case "open": if let target = row.browse, let destination = Location.from(target, title: row.title) { navigate(destination) }
        default: break
        }
    }
    func makeRowMenu(_ index: Int) -> NSMenu? {
        guard rows.indices.contains(index), location.song == nil else { return nil }
        let row = rows[index], menu = NSMenu()
        menu.autoenablesItems = false
        var choices: [(String, String)] = []
        if row.play != nil { choices.append((row.video != nil && row.video == state.track?.id ? (state.loading ? "Cancel loading" : (state.playing ? "Pause" : "Play")) : "Play", "play")) }
        if row.browse != nil { choices.append(("Open", "open")) }
        if row.video != nil { choices.append(("Add to playlist…", "add")) }
        for (title, operation) in choices {
            let item = NSMenuItem(title: title, action: #selector(rowMenuAction(_:)), keyEquivalent: "")
            item.target = self; item.representedObject = RowActionBox(row, operation: operation, generation: accountGeneration)
            item.isEnabled = state.signed_in && !state.account_checking && !state.adding
            menu.addItem(item)
        }
        return menu.items.isEmpty ? nil : menu
    }
    @objc func rowMenuAction(_ sender: NSMenuItem) {
        guard let action = sender.representedObject as? RowActionBox else { return }
        performRowAction(action.row, operation: action.operation, generation: action.generation)
    }
    func numberOfRows(in tableView: NSTableView) -> Int { rows.count }
    func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? { MusicRowView() }
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row index: Int) -> NSView? {
        guard rows.indices.contains(index) else { return nil }
        let identifier = NSUserInterfaceItemIdentifier("music-cell")
        let cell = tableView.makeView(withIdentifier: identifier, owner: self) as? MusicCell ?? MusicCell()
        cell.identifier = identifier; configure(cell, index); return cell
    }

    func controlTextDidChange(_ notification: Notification) {
        guard notification.object as AnyObject? === search else { return }
        enterSearch(immediate: false)
    }
    private func enterSearch(immediate: Bool) {
        let query = String(search.stringValue.trimmingCharacters(in: .whitespacesAndNewlines).prefix(300))
        searchWork?.cancel(); searchWork = nil
        guard !query.isEmpty else {
            searchPending = false
            if !location.query.isEmpty { back(nil) }
            return
        }
        guard state.signed_in else { return }
        let target = encodeTarget("Search", ["query": query, "params": NSNull()])
        guard let destination = Location.from(target, title: "Search") else { return }
        let changed = location.key != destination.key
        if changed { navigate(destination, remember: location.query.isEmpty, load: false) }
        if !changed && !searchPending { return }
        searchPending = true; browserDirty = true
        if visible { render() }
        if immediate { submitSearch() }
        else {
            let work = DispatchWorkItem { [weak self] in self?.submitSearch() }
            searchWork = work; DispatchQueue.main.asyncAfter(deadline: .now() + Self.searchDelay, execute: work)
        }
    }
    private func submitSearch() {
        searchWork?.cancel(); searchWork = nil
        guard searchPending, !location.query.isEmpty else { return }
        searchPending = false; browserDirty = true
        if visible { render() }
        loadCurrent()
    }
    @objc func searchNow(_ sender: Any?) { enterSearch(immediate: true) }
    func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
        guard control === search else { return false }
        if commandSelector == NSSelectorFromString("moveDown:") || commandSelector == NSSelectorFromString("insertNewline:") {
            searchNow(nil)
            if !rows.isEmpty { table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false); view.window?.makeFirstResponder(table) }
            return true
        }
        if commandSelector == NSSelectorFromString("insertTab:"), !rows.isEmpty {
            if table.selectedRow < 0 { table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false) }
            view.window?.makeFirstResponder(table); return true
        }
        if commandSelector == NSSelectorFromString("cancelOperation:") { escape(); return true }
        return false
    }

    func focusSearch() {
        guard state.signed_in else { return }
        showingAccount = false; browserDirty = true
        if visible { render(); view.window?.makeFirstResponder(search); search.selectText(nil) }
    }
    @objc func showAccount(_ sender: Any?) {
        showingAccount.toggle(); detailsVisible = false; browserDirty = true
        if showingAccount { searchWork?.cancel(); searchWork = nil }
        if visible { render() }
        if !showingAccount { if searchPending { submitSearch() } else { loadCurrent() } }
    }
    @objc private func selectProfile(_ sender: NSPopUpButton) {
        guard let id = sender.selectedItem?.representedObject as? String, id != state.profile else { return }
        send(["op": "profile", "id": id])
    }
    @objc private func toggleDetails(_ sender: Any?) { detailsVisible.toggle(); browserDirty = true; if visible { render() } }
    func makeMoreMenu() -> NSMenu {
        let menu = NSMenu(); menu.autoenablesItems = false
        let normalization = NSMenuItem(title: "Volume normalization", action: #selector(toggleNormalization(_:)), keyEquivalent: "")
        normalization.target = self; normalization.state = state.normalize ? .on : .off
        normalization.toolTip = "Lower unusually loud tracks without adding gain."
        menu.addItem(normalization); menu.addItem(.separator())
        for (title, action, key) in [("Open YouTube Music", #selector(openSignIn(_:)), ""), ("Full Disk Access…", #selector(openDiskAccess(_:)), ""), ("Quit YTfast", #selector(quit(_:)), "q")] {
            if !key.isEmpty { menu.addItem(.separator()) }
            let item = NSMenuItem(title: title, action: action, keyEquivalent: key); item.target = self; menu.addItem(item)
        }
        return menu
    }
    @objc func toggleNormalization(_ sender: Any?) { send(["op": "normalize", "enabled": !state.normalize]) }
    @objc private func showMore(_ sender: Any?) {
        makeMoreMenu().popUp(positioning: nil, at: NSPoint(x: 0, y: overflowButton.bounds.maxY + 4), in: overflowButton)
    }
    @objc func openSignIn(_ sender: Any?) {
        browserError = nil; detailsVisible = false; browserDirty = true
        if visible { render() }
        let workspace = NSWorkspace.shared
        let supported = ["com.google.Chrome", "com.brave.Browser", "org.chromium.Chromium"]
        let selected = state.profile?.lowercased() ?? ""
        let preferred = selected.contains("brave") ? supported[1] : (selected.contains("chromium") ? supported[2] : supported[0])
        let url = URL(string: "https://music.youtube.com/")!
        let bundle = workspace.urlForApplication(withBundleIdentifier: preferred)
            ?? (state.profile == nil ? supported.compactMap { workspace.urlForApplication(withBundleIdentifier: $0) }.first : nil)
        guard let bundle else {
            showBrowserError("The selected browser is unavailable. Install it or select another listed profile.")
            return
        }
        let generation = accountGeneration
        workspace.open([url], withApplicationAt: bundle, configuration: NSWorkspace.OpenConfiguration()) { [weak self] _, error in
            guard let error else { return }
            let message = error.localizedDescription
            onMainRunLoop {
                guard let self, self.accountGeneration == generation else { return }
                self.showBrowserError(message)
            }
        }
    }
    private func showBrowserError(_ message: String) {
        browserError = message; detailsVisible = true; showingAccount = true; browserDirty = true
        if visible { render() }
    }
    @objc private func openDiskAccess(_ sender: Any?) {
        NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")!)
    }
}
