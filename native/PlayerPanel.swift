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

    let titleLabel = NSTextField(labelWithString: "YTfast")
    let artistLabel = NSTextField(labelWithString: "Choose a song")
    let playButton = SymbolButton("play.fill", "Play", size: 42, pointSize: 18, primary: true)
    let previousButton = SymbolButton("backward.end.fill", "Previous", size: 32, pointSize: 15)
    let nextButton = SymbolButton("forward.end.fill", "Next", size: 32, pointSize: 15)
    let shuffleButton = SymbolButton("shuffle", "Shuffle")
    let addButton = SymbolButton("plus", "Add to playlist")
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
    let pagePlay = NSButton(title: "Play", target: nil, action: nil)
    let refreshButton = SymbolButton("arrow.clockwise", "Refresh", size: 28)
    private let pageSpinner = NSProgressIndicator()
    let table = MusicTable()
    let scroll = NSScrollView()
    private let emptyLabel = NSTextField(wrappingLabelWithString: "")
    let moreButton = NSButton(title: "Load more", target: nil, action: nil)
    private let messageLabel = NSTextField(wrappingLabelWithString: "")
    private let dismissButton = SymbolButton("xmark", "Dismiss", size: 22, pointSize: 10)
    private let retryButton = NSButton(title: "Retry", target: nil, action: nil)
    let accountButton = NSButton(title: "Account", target: nil, action: nil)
    private let overflowButton = SymbolButton("ellipsis", "More", size: 26)
    private let appLabel = NSTextField(labelWithString: "YTfast")

    let profilePicker = NSPopUpButton(frame: .zero, pullsDown: false)
    private let profileLabel = NSTextField(labelWithString: "")
    let reconnectButton = NSButton(title: "Connect", target: nil, action: nil)
    let browserButton = NSButton(title: "Sign in in browser", target: nil, action: nil)
    private let accountTitle = NSTextField(labelWithString: "Connect YouTube Music")
    private let accountStatus = NSTextField(wrappingLabelWithString: "")
    private let accountSpinner = NSProgressIndicator()
    private let accessButton = NSButton(title: "Allow Full Disk Access…", target: nil, action: nil)
    private let detailsButton = NSButton(title: "Details", target: nil, action: nil)
    private let accountDetails = NSTextField(wrappingLabelWithString: "")

    private let content = NSStackView()
    private let player = NSStackView()
    private let playerSeparator = NSBox()
    private let transport = NSStackView()
    private let timeRow = NSStackView()
    private let tabs = NSStackView()
    private let context = NSStackView()
    private let browser = NSView()
    private let accountPane = NSStackView()
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
    deinit { searchWork?.cancel(); noticeWork?.cancel() }

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
        if !(button is SymbolButton) { button.bezelStyle = .rounded; button.controlSize = .small; button.font = .systemFont(ofSize: 11) }
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
        player.orientation = .vertical; player.alignment = .leading; player.spacing = 4
        titleLabel.font = .systemFont(ofSize: 15, weight: .semibold)
        artistLabel.font = .systemFont(ofSize: 12); artistLabel.textColor = .secondaryLabelColor
        let heading = NSStackView(views: [titleLabel, artistLabel])
        heading.orientation = .vertical; heading.alignment = .leading; heading.spacing = 3
        for label in [titleLabel, artistLabel] {
            label.lineBreakMode = .byTruncatingTail
            label.widthAnchor.constraint(equalTo: heading.widthAnchor).isActive = true
        }
        fixed(heading, 36)
        let left = gap(), right = gap()
        horizontal(transport, [shuffleButton, left, previousButton, playButton, nextButton, right, addButton], spacing: 8)
        left.widthAnchor.constraint(equalTo: right.widthAnchor).isActive = true
        fixed(transport, 42)
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
        volume.widthAnchor.constraint(equalToConstant: 92).isActive = true
        for label in [elapsed, remaining, playbackStatus] {
            label.font = .monospacedDigitSystemFont(ofSize: 10, weight: .regular)
            label.textColor = .secondaryLabelColor
        }
        playbackStatus.font = .systemFont(ofSize: 10)
        playbackStatus.lineBreakMode = .byTruncatingTail
        spinner(playbackSpinner)
        let timeLeft = gap(), timeRight = gap()
        horizontal(timeRow, [elapsed, timeLeft, playbackSpinner, playbackStatus, timeRight, remaining], spacing: 3)
        timeLeft.widthAnchor.constraint(equalTo: timeRight.widthAnchor).isActive = true
        fixed(timeRow, 12)
        for child in [heading, transport, seek, timeRow] {
            player.addArrangedSubview(child)
            child.widthAnchor.constraint(equalTo: player.widthAnchor).isActive = true
        }
        add(player); playerHeight = player.heightAnchor.constraint(equalToConstant: 120); playerHeight.isActive = true
        for line in [playerSeparator, footerSeparator] { line.boxType = .separator; fixed(line, 1) }
        add(playerSeparator)
        search.placeholderString = "Search music"
        search.font = .systemFont(ofSize: 12); search.focusRingType = .none
        search.delegate = self; search.target = self; search.action = #selector(searchNow(_:))
        search.sendsWholeSearchString = true; search.setAccessibilityLabel("Search YouTube Music")
        fixed(search, 28); add(search)
        sections.selectedSegment = 0; sections.segmentDistribution = .fillEqually; sections.controlSize = .small
        sections.target = self; sections.action = #selector(changeSection(_:))
        sections.setContentHuggingPriority(.defaultLow, for: .horizontal)
        button(refreshButton, #selector(refreshPage(_:))); spinner(pageSpinner)
        horizontal(tabs, [sections, pageSpinner, refreshButton], spacing: 6)
        fixed(tabs, 30); add(tabs)
        button(backButton, #selector(back(_:))); button(pagePlay, #selector(playPage(_:)))
        pageTitle.font = .systemFont(ofSize: 12, weight: .semibold)
        pageTitle.lineBreakMode = .byTruncatingTail
        pageTitle.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        horizontal(context, [backButton, pageTitle, gap(), pagePlay], spacing: 4)
        fixed(context, 30); add(context)
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("music"))
        table.addTableColumn(column); table.headerView = nil; table.rowHeight = 44
        table.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        table.intercellSpacing = .zero; table.backgroundColor = .clear; table.style = .plain
        table.dataSource = self; table.delegate = self
        table.activate = { [weak self] in self?.activateSelectedRow() }
        table.togglePlayback = { [weak self] in self?.toggle(nil) }
        table.goBack = { [weak self] in self?.escape() }
        table.setAccessibilityLabel("Music")
        scroll.documentView = table; scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true
        scroll.drawsBackground = false; scroll.borderType = .noBorder
        emptyLabel.font = .systemFont(ofSize: 12); emptyLabel.textColor = .secondaryLabelColor
        emptyLabel.alignment = .center; emptyLabel.maximumNumberOfLines = 2
        for child in [scroll, emptyLabel, accountPane] { child.translatesAutoresizingMaskIntoConstraints = false; browser.addSubview(child) }
        NSLayoutConstraint.activate([
            scroll.leadingAnchor.constraint(equalTo: browser.leadingAnchor), scroll.trailingAnchor.constraint(equalTo: browser.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: browser.topAnchor), scroll.bottomAnchor.constraint(equalTo: browser.bottomAnchor),
            emptyLabel.leadingAnchor.constraint(equalTo: browser.leadingAnchor, constant: 20), emptyLabel.trailingAnchor.constraint(equalTo: browser.trailingAnchor, constant: -20),
            emptyLabel.centerYAnchor.constraint(equalTo: browser.centerYAnchor),
            accountPane.leadingAnchor.constraint(equalTo: browser.leadingAnchor, constant: 12), accountPane.trailingAnchor.constraint(equalTo: browser.trailingAnchor, constant: -12),
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
        button(accountButton, #selector(showAccount(_:))); accountButton.isBordered = false
        accountButton.image = NSImage(systemSymbolName: "person.crop.circle", accessibilityDescription: nil)
        accountButton.imagePosition = .imageLeft; accountButton.setAccessibilityLabel("Account")
        button(overflowButton, #selector(showMore(_:)))
        appLabel.font = .systemFont(ofSize: 11, weight: .medium); appLabel.textColor = .secondaryLabelColor
        horizontal(footer, [appLabel, muteButton, volume, gap(), accountButton, overflowButton], spacing: 5)
        fixed(footer, 30); add(footer)
        browserDirty = true
    }

    private func makeAccountPane() {
        accountPane.orientation = .vertical; accountPane.alignment = .centerX; accountPane.spacing = 10
        accountTitle.font = .systemFont(ofSize: 15, weight: .semibold)
        accountStatus.font = .systemFont(ofSize: 11); accountStatus.textColor = .secondaryLabelColor
        accountStatus.alignment = .center; accountStatus.maximumNumberOfLines = 2
        accountStatus.preferredMaxLayoutWidth = 300
        let heading = NSStackView(); spinner(accountSpinner)
        horizontal(heading, [accountSpinner, accountTitle], spacing: 6)
        accountPane.addArrangedSubview(heading)
        accountPane.addArrangedSubview(accountStatus)
        accountStatus.widthAnchor.constraint(equalTo: accountPane.widthAnchor).isActive = true
        profilePicker.font = .systemFont(ofSize: 12); profilePicker.controlSize = .small
        profilePicker.target = self; profilePicker.action = #selector(selectProfile(_:))
        profilePicker.setAccessibilityLabel("Browser profile")
        accountPane.addArrangedSubview(profilePicker)
        profilePicker.widthAnchor.constraint(equalTo: accountPane.widthAnchor).isActive = true
        profileLabel.font = .systemFont(ofSize: 11, weight: .medium)
        profileLabel.alignment = .center; profileLabel.lineBreakMode = .byTruncatingMiddle
        accountPane.addArrangedSubview(profileLabel)
        profileLabel.widthAnchor.constraint(equalTo: accountPane.widthAnchor).isActive = true
        button(reconnectButton, #selector(reconnect(_:))); button(browserButton, #selector(openSignIn(_:)))
        reconnectButton.bezelColor = .controlAccentColor
        let actions = NSStackView(); horizontal(actions, [browserButton, reconnectButton], spacing: 10)
        accountPane.addArrangedSubview(actions)
        button(accessButton, #selector(openDiskAccess(_:))); accessButton.isBordered = false
        button(detailsButton, #selector(toggleDetails(_:))); detailsButton.isBordered = false
        let help = NSStackView(); horizontal(help, [accessButton, detailsButton], spacing: 8)
        accountPane.addArrangedSubview(help)
        accountDetails.font = .systemFont(ofSize: 10); accountDetails.textColor = .secondaryLabelColor
        accountDetails.maximumNumberOfLines = 4; accountDetails.isSelectable = true
        accountPane.addArrangedSubview(accountDetails)
        accountDetails.widthAnchor.constraint(equalTo: accountPane.widthAnchor).isActive = true
        fixed(accountDetails, 54)
    }

    func opened() {
        _ = view; visible = true
        render()
        if searchPending { submitSearch() } else { loadCurrent() }
    }
    func closed() {
        visible = false
        location.scroll = scroll.contentView.bounds.origin
        searchWork?.cancel(); searchWork = nil
        for indicator in [playbackSpinner, pageSpinner, accountSpinner] { indicator.stopAnimation(nil) }
    }
    func apply(_ next: State) {
        let connected = !state.signed_in && next.signed_in
        let boundary = state.profile != next.profile || (state.signed_in && !next.signed_in)
        let chromeChanged = state.account != next.account || state.account_checking != next.account_checking ||
            state.signed_in != next.signed_in || state.profiles != next.profiles ||
            state.error != next.error || state.notice != next.notice || state.adding != next.adding
        let trackChanged = state.track?.id != next.track?.id
        let transportChanged = trackChanged || state.playing != next.playing || state.loading != next.loading
        if trackChanged || (state.loading && !next.loading) || next.error != nil { pendingSong = nil }
        if boundary {
            searchWork?.cancel(); searchWork = nil; searchPending = false
            pages.removeAll(); history.removeAll(); location = .library(libraryIndex)
            renderedLocation = ""; pendingSong = nil; detailsVisible = false
        }
        if !next.signed_in { pages.removeAll() }
        if connected { showingAccount = false }
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
        browserDirty = browserDirty || boundary || chromeChanged || trackChanged
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
        shuffleButton.setSymbol("shuffle", state.shuffle ? "Shuffle on" : "Shuffle off")
        addButton.isEnabled = state.signed_in && state.track != nil && !state.loading && !state.adding
        addButton.setSymbol(state.adding ? "checkmark" : "plus", state.adding ? "Adding…" : "Add to playlist")
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
        player.isHidden = account && !hasAudio; playerSeparator.isHidden = player.isHidden
        playerHeight.constant = hasAudio ? 122 : 36
        transport.isHidden = !hasAudio; seek.isHidden = !hasAudio; timeRow.isHidden = !hasAudio
        search.isHidden = account; tabs.isHidden = account
        search.isEnabled = state.signed_in; sections.isEnabled = state.signed_in
        context.isHidden = account || (history.isEmpty && location.song == nil)
        scroll.isHidden = account; accountPane.isHidden = !account
        accountButton.title = showingAccount && state.signed_in ? "Library" : "Account"
        accountButton.toolTip = state.account
        accountButton.setAccessibilityLabel(showingAccount && state.signed_in ? "Back to library" : "Account")
        muteButton.isHidden = !hasAudio; volume.isHidden = !hasAudio; appLabel.isHidden = hasAudio
        let page = pages[location.key]
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
        refreshButton.isEnabled = !loading
        pagePlay.isHidden = location.song != nil || page?.play == nil
        pagePlay.isEnabled = state.signed_in && !state.account_checking
        setText(pageTitle, location.song.map { "Add “\($0.title)”" } ?? (page?.title.isEmpty == false ? page!.title : location.title))
        pageTitle.toolTip = pageTitle.stringValue
        sections.selectedSegment = location.query.isEmpty ? libraryIndex : -1
        if search.stringValue != location.query { search.stringValue = location.query }
        moreButton.isHidden = account || page?.more != true
        moreButton.isEnabled = !loading; moreButton.title = loading ? "Loading…" : "Load more"
        emptyLabel.isHidden = account || !rows.isEmpty
        setText(emptyLabel, loading ? (location.query.isEmpty ? "Loading…" : "Searching…") : (page?.message != nil ? "Couldn't load music" : (location.song != nil ? "No editable playlists" : (location.query.isEmpty ? "No music here yet" : "No results"))))
        let message = state.error ?? state.notice ?? (account ? nil : page?.message)
        messageRow.isHidden = message == nil
        setText(messageLabel, message ?? ""); messageLabel.toolTip = message
        retryButton.isHidden = account || page?.message == nil
        dismissButton.isHidden = state.error == nil && state.notice == nil
        if account { renderAccount() } else { busy(accountSpinner, false) }
        browserHeight.constant = account ? (detailsVisible ? 234 : 180) : (rows.isEmpty ? 112 : CGFloat(min(6, rows.count)) * 44)
        let heights: [(NSView, CGFloat)] = [(player, playerHeight.constant), (playerSeparator, 1), (search, 28), (tabs, 30), (context, 30), (browser, browserHeight.constant), (moreButton, 26), (messageRow, 34), (footerSeparator, 1), (footer, 30)]
        let shown = heights.filter { !$0.0.isHidden }
        let size = NSSize(width: Self.width, height: shown.reduce(24) { $0 + $1.1 } + CGFloat(max(0, shown.count - 1)) * 8)
        if preferredContentSize != size { preferredContentSize = size; sizeChanged?(size) }
    }
    private func renderAccount() {
        setText(accountTitle, state.account_checking ? "Connecting…" : (state.signed_in ? "YouTube Music" : "Connect YouTube Music"))
        setText(accountStatus, state.signed_in ? state.account : (permissionRequired ? "Allow browser access, then reconnect." : (state.account_checking ? "Checking your browser session" : "Sign in in your browser, then connect.")))
        busy(accountSpinner, state.account_checking)
        if profilePicker.itemArray.compactMap({ $0.representedObject as? String }) != state.profiles.map(\.id) || profilePicker.numberOfItems == 0 {
            profilePicker.removeAllItems()
            if state.profiles.isEmpty { profilePicker.addItem(withTitle: "Chrome, Brave or Chromium") }
            for profile in state.profiles { profilePicker.addItem(withTitle: profile.label); profilePicker.lastItem?.representedObject = profile.id }
        }
        if let selected = state.profile, let item = profilePicker.itemArray.first(where: { $0.representedObject as? String == selected }) { profilePicker.select(item) }
        profilePicker.isHidden = state.profiles.count <= 1
        profilePicker.isEnabled = !state.account_checking && state.profiles.count > 1
        profileLabel.isHidden = !profilePicker.isHidden
        setText(profileLabel, state.profiles.first?.label ?? "Chrome, Brave or Chromium")
        reconnectButton.title = state.account_checking ? "Connecting…" : (state.signed_in || state.profile != nil ? "Reconnect" : "Connect")
        reconnectButton.isEnabled = !state.account_checking
        accessButton.isHidden = !permissionRequired
        detailsButton.isHidden = state.signed_in || state.account_checking
        detailsButton.title = detailsVisible ? "Hide details" : "Details"
        accountDetails.isHidden = !detailsVisible
        setText(accountDetails, state.account); accountDetails.toolTip = state.account
    }

    private func updateVisibleRows() {
        table.enumerateAvailableRowViews { [weak self] rowView, index in
            guard let self, self.rows.indices.contains(index), let cell = rowView.view(atColumn: 0) as? MusicCell else { return }
            self.configure(cell, index)
        }
    }
    private func configure(_ cell: MusicCell, _ index: Int) {
        let row = rows[index]
        cell.configure(row, adding: location.song != nil, current: row.video != nil && row.video == state.track?.id, playing: state.playing)
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
        guard let previous = history.popLast() else { return }
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
    @objc func mute(_ sender: Any?) { send(["op": "volume", "value": state.volume == 0 ? lastAudibleVolume : 0]) }
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
    @objc func reconnect(_ sender: Any?) { guard !state.account_checking else { return }; send(["op": "reconnect"]) }
    @objc func quit(_ sender: Any?) { send(["op": "quit"]) }
    @objc func addSong(_ sender: Any?) {
        guard let song = state.track, state.signed_in, !state.adding, !state.loading else { return }
        var destination = Location.library(0); destination.song = song
        navigate(destination)
    }
    func activateSelectedRow() {
        let index = table.selectedRow
        guard rows.indices.contains(index), state.signed_in else { return }
        let row = rows[index]
        if let song = location.song, let playlist = row.editable {
            send(["op": "add", "playlist": playlist, "video": song.id]); back(nil)
        } else if let target = row.browse, let destination = Location.from(target, title: row.title) {
            navigate(destination)
        } else if let target = row.play {
            if row.video != nil && row.video == state.track?.id { toggle(nil) }
            else {
                if let id = row.video { pendingSong = Song(id: id, title: row.title, artist: row.subtitle) }
                send(["op": "play", "target": target])
            }
        }
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
        if commandSelector == NSSelectorFromString("cancelOperation:") { escape(); return true }
        return false
    }

    func focusSearch() {
        guard state.signed_in else { return }
        showingAccount = false; browserDirty = true
        if visible { render(); view.window?.makeFirstResponder(search) }
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
        for (title, action, key) in [("Open YouTube Music", #selector(openSignIn(_:)), ""), ("Full Disk Access…", #selector(openDiskAccess(_:)), ""), ("Quit YTfast", #selector(quit(_:)), "q")] {
            if !key.isEmpty { menu.addItem(.separator()) }
            let item = NSMenuItem(title: title, action: action, keyEquivalent: key); item.target = self; menu.addItem(item)
        }
        return menu
    }
    @objc private func showMore(_ sender: Any?) {
        makeMoreMenu().popUp(positioning: nil, at: NSPoint(x: 0, y: overflowButton.bounds.maxY + 4), in: overflowButton)
    }
    @objc func openSignIn(_ sender: Any?) {
        let workspace = NSWorkspace.shared
        let supported = ["com.google.Chrome", "com.brave.Browser", "org.chromium.Chromium"]
        let selected = state.profile?.lowercased() ?? ""
        let preferred = selected.contains("brave") ? supported[1] : (selected.contains("chromium") ? supported[2] : supported[0])
        let url = URL(string: "https://music.youtube.com/")!
        let bundle = workspace.urlForApplication(withBundleIdentifier: preferred)
            ?? (state.profile == nil ? supported.compactMap { workspace.urlForApplication(withBundleIdentifier: $0) }.first : nil)
        guard let bundle else { detailsVisible = true; showingAccount = true; browserDirty = true; if visible { render() }; return }
        workspace.open([url], withApplicationAt: bundle, configuration: NSWorkspace.OpenConfiguration()) { [weak self] _, error in
            guard error != nil else { return }
            onMainRunLoop { self?.detailsVisible = true; self?.showingAccount = true; self?.browserDirty = true; if self?.visible == true { self?.render() } }
        }
    }
    @objc private func openDiskAccess(_ sender: Any?) {
        NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")!)
    }
}
