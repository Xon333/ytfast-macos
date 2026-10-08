import AppKit

/// Small native controls with consistent hit areas and explicit hover/on states.
/// No animation clock or per-row backing layers are required.
final class SymbolButton: NSButton {
    private var hovered = false
    private var tracking: NSTrackingArea?
    private var symbolName = ""
    let primary: Bool
    let symbolSize: CGFloat
    var isOn = false {
        didSet {
            if oldValue != isOn {
                contentTintColor = primary ? .white : (isOn ? .controlAccentColor : .labelColor)
                needsDisplay = true
            }
        }
    }

    init(_ symbol: String, _ label: String, size: CGFloat = 30, pointSize: CGFloat = 13, primary: Bool = false) {
        self.primary = primary
        self.symbolSize = pointSize
        super.init(frame: .zero)
        title = ""
        isBordered = false
        bezelStyle = .regularSquare
        imagePosition = .imageOnly
        imageScaling = .scaleNone
        focusRingType = .exterior
        setButtonType(.momentaryChange)
        translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            widthAnchor.constraint(equalToConstant: size),
            heightAnchor.constraint(equalToConstant: size)
        ])
        contentTintColor = primary ? .white : .labelColor
        setSymbol(symbol, label)
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }

    func setSymbol(_ symbol: String, _ label: String) {
        if symbolName != symbol {
            symbolName = symbol
            image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)?
                .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: symbolSize, weight: .medium))
        }
        if toolTip != label { toolTip = label; setAccessibilityLabel(label) }
    }
    override func updateTrackingAreas() {
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(area); tracking = area
        super.updateTrackingAreas()
        // Reused rows can move underneath a stationary pointer. Recompute the
        // hover state instead of waiting for a mouse-exit event that never comes.
        hovered = window.map { $0.isKeyWindow && visibleRect.contains(convert($0.mouseLocationOutsideOfEventStream, from: nil)) } ?? false
        needsDisplay = true
    }
    override func mouseEntered(with event: NSEvent) { hovered = true; needsDisplay = true }
    override func mouseExited(with event: NSEvent) { hovered = false; needsDisplay = true }
    override func draw(_ dirtyRect: NSRect) {
        let pressed = cell?.isHighlighted == true
        if primary || isOn || (isEnabled && (hovered || pressed)) {
            let fill: NSColor = primary ? .controlAccentColor : (isOn ? .controlAccentColor : .labelColor)
            fill.withAlphaComponent(primary ? (isEnabled ? (pressed ? 0.75 : 1) : 0.3) : (pressed ? 0.24 : (isOn ? 0.15 : 0.07))).setFill()
            let radius: CGFloat = primary ? bounds.height / 2 : 7
            NSBezierPath(roundedRect: bounds.insetBy(dx: 1, dy: 1), xRadius: radius, yRadius: radius).fill()
        }
        super.draw(dirtyRect)
    }
    override func drawFocusRingMask() {
        let radius: CGFloat = primary ? bounds.height / 2 : 7
        NSBezierPath(roundedRect: bounds.insetBy(dx: 1, dy: 1), xRadius: radius, yRadius: radius).fill()
    }
    override var focusRingMaskBounds: NSRect { bounds }
}

/// One lightweight surface groups related controls. AppKit draws the material
/// behind it; there is no hosted SwiftUI view, image, or animation task.
final class ControlCard: NSStackView {
    override func draw(_ dirtyRect: NSRect) {
        let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 12, yRadius: 12)
        NSColor.labelColor.withAlphaComponent(0.045).setFill(); shape.fill()
        NSColor.separatorColor.withAlphaComponent(0.3).setStroke(); shape.lineWidth = 1; shape.stroke()
        super.draw(dirtyRect)
    }
}

/// A clear primary action in the connection view, with native button input
/// and accessibility. Drawing the fill avoids platform-dependent bezel tinting.
final class ConnectButton: NSButton {
    init() {
        super.init(frame: .zero)
        title = "Connect"; isBordered = false
        setButtonType(.momentaryPushIn)
        font = .systemFont(ofSize: 12, weight: .semibold)
        focusRingType = .exterior
        translatesAutoresizingMaskIntoConstraints = false
        heightAnchor.constraint(equalToConstant: 28).isActive = true
        widthAnchor.constraint(greaterThanOrEqualToConstant: 100).isActive = true
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }
    override var intrinsicContentSize: NSSize {
        NSSize(width: max(100, super.intrinsicContentSize.width + 20), height: 28)
    }
    override func draw(_ dirtyRect: NSRect) {
        let pressed = cell?.isHighlighted == true
        let fill = isEnabled ? NSColor.controlAccentColor : NSColor.quaternaryLabelColor
        fill.withAlphaComponent(pressed ? 0.75 : 1).setFill()
        NSBezierPath(roundedRect: bounds.insetBy(dx: 1, dy: 1), xRadius: 6, yRadius: 6).fill()
        let text = NSAttributedString(string: title, attributes: [
            .font: font ?? NSFont.systemFont(ofSize: 12, weight: .semibold),
            .foregroundColor: isEnabled ? NSColor.white : NSColor.secondaryLabelColor
        ])
        let size = text.size()
        text.draw(at: NSPoint(x: (bounds.width - size.width) / 2, y: (bounds.height - size.height) / 2))
    }
    override func drawFocusRingMask() {
        NSBezierPath(roundedRect: bounds.insetBy(dx: 1, dy: 1), xRadius: 6, yRadius: 6).fill()
    }
    override var focusRingMaskBounds: NSRect { bounds }
}

/// mpv can deliver updates inside AppKit's mouse-tracking loop. Keep the thumb
/// under the user's control until tracking ends, then commit the final value.
final class ValueSlider: NSSlider {
    private(set) var editing = false
    var beganEditing: (() -> Void)?
    var endedEditing: (() -> Void)?
    init(value: Double, maximum: Double) {
        super.init(frame: .zero)
        minValue = 0; maxValue = maximum; doubleValue = value
        isContinuous = true; controlSize = .mini
        translatesAutoresizingMaskIntoConstraints = false
        heightAnchor.constraint(equalToConstant: 20).isActive = true
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }
    func beginEditing() { editing = true; beganEditing?() }
    func endEditing() { endedEditing?(); editing = false }
    override func mouseDown(with event: NSEvent) {
        guard isEnabled else { return }
        beginEditing()
        super.mouseDown(with: event)
        endEditing()
    }
}

final class MusicTable: NSTableView {
    var activate: (() -> Void)?
    var togglePlayback: (() -> Void)?
    var goBack: (() -> Void)?
    var rowMenu: ((Int) -> NSMenu?)?
    override func keyDown(with event: NSEvent) {
        guard event.modifierFlags.intersection([.command, .control, .option]).isEmpty else {
            super.keyDown(with: event); return
        }
        switch event.keyCode {
        case 36, 76: activate?()
        case 49: togglePlayback?()
        case 53: goBack?()
        default: super.keyDown(with: event)
        }
    }
    override func mouseDown(with event: NSEvent) {
        let clicked = row(at: convert(event.locationInWindow, from: nil))
        super.mouseDown(with: event)
        let ended = window.map { row(at: convert($0.mouseLocationOutsideOfEventStream, from: nil)) } ?? -1
        // Selection (including arrow keys) is not playback. A double-click's
        // second event must not immediately pause what its first event started.
        if clicked >= 0, selectedRow == clicked, ended == clicked, event.clickCount == 1,
           event.modifierFlags.intersection([.command, .control, .option, .shift]).isEmpty { activate?() }
    }
    override func menu(for event: NSEvent) -> NSMenu? {
        let index = row(at: convert(event.locationInWindow, from: nil))
        guard index >= 0 else { return nil }
        selectRowIndexes(IndexSet(integer: index), byExtendingSelection: false)
        return rowMenu?(index)
    }
}

final class MusicRowView: NSTableRowView {
    private var hovered = false
    private var tracking: NSTrackingArea?
    override func updateTrackingAreas() {
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(area); tracking = area
        super.updateTrackingAreas()
        hovered = window.map { $0.isKeyWindow && visibleRect.contains(convert($0.mouseLocationOutsideOfEventStream, from: nil)) } ?? false
        updateActions()
    }
    override var isSelected: Bool { didSet { updateActions() } }
    override func didAddSubview(_ subview: NSView) { super.didAddSubview(subview); updateActions() }
    override func mouseEntered(with event: NSEvent) { hovered = true; updateActions() }
    override func mouseExited(with event: NSEvent) { hovered = false; updateActions() }
    private func updateActions() {
        for cell in subviews.compactMap({ $0 as? MusicCell }) { cell.showActions(hovered || isSelected) }
        needsDisplay = true
    }
    override func drawBackground(in dirtyRect: NSRect) {
        if hovered && !isSelected {
            NSColor.labelColor.withAlphaComponent(0.05).setFill()
            NSBezierPath(roundedRect: bounds.insetBy(dx: 2, dy: 1), xRadius: 6, yRadius: 6).fill()
        }
    }
    override func drawSelection(in dirtyRect: NSRect) {
        NSColor.controlAccentColor.withAlphaComponent(isEmphasized ? 0.18 : 0.1).setFill()
        NSBezierPath(roundedRect: bounds.insetBy(dx: 2, dy: 1), xRadius: 6, yRadius: 6).fill()
    }
}

final class MusicGlyph: NSView {
    private let image = NSImageView()
    private var symbol = ""
    var active = false { didSet { image.contentTintColor = active ? .controlAccentColor : .secondaryLabelColor; needsDisplay = true } }
    override init(frame: NSRect) {
        super.init(frame: frame)
        image.translatesAutoresizingMaskIntoConstraints = false; addSubview(image)
        NSLayoutConstraint.activate([
            image.centerXAnchor.constraint(equalTo: centerXAnchor), image.centerYAnchor.constraint(equalTo: centerYAnchor),
            image.widthAnchor.constraint(equalToConstant: 16), image.heightAnchor.constraint(equalToConstant: 16)
        ])
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }
    func setSymbol(_ name: String) {
        guard name != symbol else { return }; symbol = name
        image.image = NSImage(systemSymbolName: name, accessibilityDescription: nil)?
            .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: 13, weight: .regular))
    }
    override func draw(_ dirtyRect: NSRect) {
        (active ? NSColor.controlAccentColor : NSColor.labelColor).withAlphaComponent(active ? 0.12 : 0.045).setFill()
        NSBezierPath(roundedRect: bounds, xRadius: 7, yRadius: 7).fill()
        super.draw(dirtyRect)
    }
}

final class MusicCell: NSTableCellView {
    let title = NSTextField(labelWithString: "")
    let subtitle = NSTextField(labelWithString: "")
    let icon = MusicGlyph()
    let accessory = NSImageView()
    let playActionButton = SymbolButton("play.fill", "Play", size: 26, pointSize: 11)
    let addActionButton = SymbolButton("plus", "Add to playlist", size: 26, pointSize: 11)
    var playAction: (() -> Void)?
    var addAction: (() -> Void)?
    private var showingActions = false
    private var canPlay = false
    private var canAdd = false
    private var isCollection = false
    private var isAdding = false
    private var labelsTrailing: NSLayoutConstraint!
    override init(frame: NSRect) {
        super.init(frame: frame)
        title.font = .systemFont(ofSize: 13, weight: .medium)
        subtitle.font = .systemFont(ofSize: 11)
        subtitle.textColor = .secondaryLabelColor
        for label in [title, subtitle] { label.lineBreakMode = .byTruncatingTail }
        for child in [title, subtitle, icon, accessory, playActionButton, addActionButton] {
            child.translatesAutoresizingMaskIntoConstraints = false; addSubview(child)
        }
        playActionButton.target = self; playActionButton.action = #selector(play(_:))
        addActionButton.target = self; addActionButton.action = #selector(add(_:))
        accessory.contentTintColor = .tertiaryLabelColor
        labelsTrailing = title.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -28)
        NSLayoutConstraint.activate([
            icon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            icon.centerYAnchor.constraint(equalTo: centerYAnchor),
            icon.widthAnchor.constraint(equalToConstant: 30), icon.heightAnchor.constraint(equalToConstant: 30),
            title.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: 10),
            title.topAnchor.constraint(equalTo: topAnchor, constant: 6),
            labelsTrailing,
            subtitle.leadingAnchor.constraint(equalTo: title.leadingAnchor),
            subtitle.topAnchor.constraint(equalTo: title.bottomAnchor, constant: 2),
            subtitle.trailingAnchor.constraint(equalTo: title.trailingAnchor),
            accessory.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -10),
            accessory.centerYAnchor.constraint(equalTo: centerYAnchor),
            accessory.widthAnchor.constraint(equalToConstant: 12), accessory.heightAnchor.constraint(equalToConstant: 12),
            addActionButton.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -6),
            addActionButton.centerYAnchor.constraint(equalTo: centerYAnchor),
            playActionButton.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -34),
            playActionButton.centerYAnchor.constraint(equalTo: centerYAnchor)
        ])
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }
    func configure(_ row: Row, adding: Bool, current: Bool, playing: Bool, loading: Bool = false, enabled: Bool = true) {
        var detail = row.subtitle
        for prefix in ["Playlist · ", "Album · "] where detail.hasPrefix(prefix) { detail.removeFirst(prefix.count) }
        setText(title, row.title); setText(subtitle, detail)
        title.textColor = current ? .controlAccentColor : .labelColor
        isCollection = row.browse != nil; isAdding = adding
        icon.setSymbol(current ? (loading ? "ellipsis" : (playing ? "waveform" : "pause.fill")) : (isCollection ? "square.stack" : "music.note"))
        icon.active = current
        canPlay = !adding && row.play != nil; canAdd = !adding && row.video != nil
        playActionButton.setSymbol(current && loading ? "stop.fill" : (current && playing ? "pause.fill" : "play.fill"), current && loading ? "Cancel loading" : (current && playing ? "Pause" : "Play \(row.title)"))
        addActionButton.setSymbol("plus", "Add \(row.title) to playlist")
        playActionButton.isEnabled = enabled; addActionButton.isEnabled = enabled
        let symbol = adding ? "plus" : (isCollection ? "chevron.right" : (current && loading ? "ellipsis" : (current && playing ? "speaker.wave.2.fill" : "play.fill")))
        accessory.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
        accessory.contentTintColor = current ? .controlAccentColor : .tertiaryLabelColor
        toolTip = row.title + (row.subtitle.isEmpty ? "" : "\n" + row.subtitle)
        setAccessibilityLabel(row.title + (row.subtitle.isEmpty ? "" : ", " + row.subtitle))
        showActions(showingActions)
    }
    func showActions(_ visible: Bool) {
        showingActions = visible
        playActionButton.isHidden = !visible || !canPlay
        addActionButton.isHidden = !visible || !canAdd
        accessory.isHidden = visible && (canPlay || canAdd) && !isCollection && !isAdding
        labelsTrailing.constant = visible && (canPlay || canAdd) ? -66 : -28
    }
    @objc private func play(_ sender: Any?) { playAction?() }
    @objc private func add(_ sender: Any?) { addAction?() }
}
