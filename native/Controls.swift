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
}

final class MusicRowView: NSTableRowView {
    private var hovered = false
    private var tracking: NSTrackingArea?
    override func updateTrackingAreas() {
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(area); tracking = area
        super.updateTrackingAreas()
    }
    override func mouseEntered(with event: NSEvent) { hovered = true; needsDisplay = true }
    override func mouseExited(with event: NSEvent) { hovered = false; needsDisplay = true }
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

final class MusicCell: NSTableCellView {
    let title = NSTextField(labelWithString: "")
    let subtitle = NSTextField(labelWithString: "")
    let icon = NSImageView()
    let accessory = NSImageView()
    override init(frame: NSRect) {
        super.init(frame: frame)
        title.font = .systemFont(ofSize: 13, weight: .medium)
        subtitle.font = .systemFont(ofSize: 11)
        subtitle.textColor = .secondaryLabelColor
        for label in [title, subtitle] { label.lineBreakMode = .byTruncatingTail }
        for child in [title, subtitle, icon, accessory] {
            child.translatesAutoresizingMaskIntoConstraints = false; addSubview(child)
        }
        icon.contentTintColor = .secondaryLabelColor
        accessory.contentTintColor = .tertiaryLabelColor
        NSLayoutConstraint.activate([
            icon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            icon.centerYAnchor.constraint(equalTo: centerYAnchor),
            icon.widthAnchor.constraint(equalToConstant: 20), icon.heightAnchor.constraint(equalToConstant: 20),
            title.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: 10),
            title.topAnchor.constraint(equalTo: topAnchor, constant: 6),
            title.trailingAnchor.constraint(equalTo: accessory.leadingAnchor, constant: -8),
            subtitle.leadingAnchor.constraint(equalTo: title.leadingAnchor),
            subtitle.topAnchor.constraint(equalTo: title.bottomAnchor, constant: 2),
            subtitle.trailingAnchor.constraint(equalTo: title.trailingAnchor),
            accessory.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -10),
            accessory.centerYAnchor.constraint(equalTo: centerYAnchor),
            accessory.widthAnchor.constraint(equalToConstant: 12), accessory.heightAnchor.constraint(equalToConstant: 12)
        ])
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }
    func configure(_ row: Row, adding: Bool, current: Bool, playing: Bool) {
        var detail = row.subtitle
        for prefix in ["Playlist · ", "Album · "] where detail.hasPrefix(prefix) { detail.removeFirst(prefix.count) }
        setText(title, row.title); setText(subtitle, detail)
        title.textColor = current ? .controlAccentColor : .labelColor
        icon.image = NSImage(systemSymbolName: row.browse != nil ? "square.stack" : "music.note", accessibilityDescription: nil)
        let symbol = adding ? "plus" : (row.browse != nil ? "chevron.right" : (current && playing ? "speaker.wave.2.fill" : "play.fill"))
        accessory.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
        accessory.contentTintColor = current ? .controlAccentColor : .tertiaryLabelColor
        toolTip = row.title + (row.subtitle.isEmpty ? "" : "\n" + row.subtitle)
        setAccessibilityLabel(row.title + (row.subtitle.isEmpty ? "" : ", " + row.subtitle))
    }
}
