// Adapted from Mino's MenuActionButton (MIT, Copyright (c) 2024 NAD).
// https://github.com/nad-bit/Mino/blob/0ee56b782b60c47ae1313d3f2c5c07bfb7497d0e/SwiftApp/Sources/RepoMenuItemView.swift#L21-L88
// License: native/ThirdParty/Mino-LICENSE.txt (also included in the app bundle).
// YTfast adaptations: draw without a per-button layer, track the visible area of
// a key window, and reset hover when disabled, hidden, or detached. SymbolButton
// supplies product-specific sizes, selected/pressed drawing and accessibility.
import AppKit

class MenuActionButton: NSButton {
    private var trackingArea: NSTrackingArea?
    private(set) var isHovered = false

    var baseColor: NSColor = .secondaryLabelColor {
        didSet { if !isHovered { contentTintColor = baseColor } }
    }
    var hoverColor: NSColor = .labelColor {
        didSet { if isHovered { contentTintColor = hoverColor } }
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let trackingArea { removeTrackingArea(trackingArea) }
        let options: NSTrackingArea.Options = [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect]
        let area = NSTrackingArea(rect: .zero, options: options, owner: self, userInfo: nil)
        trackingArea = area; addTrackingArea(area)
        reconcileHoverState()
    }

    private func reconcileHoverState() {
        // Reused rows may move while the pointer stays still. Reconcile both
        // entry and exit, rather than retaining a ghost hover from the old row.
        if isEnabled, !isHiddenOrHasHiddenAncestor, let window, window.isKeyWindow,
           visibleRect.contains(convert(window.mouseLocationOutsideOfEventStream, from: nil)) {
            isHovered = true; contentTintColor = hoverColor; needsDisplay = true
        } else { resetHoverState() }
    }

    override func mouseEntered(with event: NSEvent) {
        super.mouseEntered(with: event)
        guard isEnabled, !isHiddenOrHasHiddenAncestor else { return }
        isHovered = true
        contentTintColor = hoverColor
        needsDisplay = true
    }

    override func mouseExited(with event: NSEvent) {
        super.mouseExited(with: event)
        resetHoverState()
    }

    func resetHoverState() {
        guard isHovered else { return }
        isHovered = false
        contentTintColor = baseColor
        needsDisplay = true
    }

    override var isEnabled: Bool { didSet { if oldValue != isEnabled { reconcileHoverState() } } }
    override var isHidden: Bool { didSet { if oldValue != isHidden { reconcileHoverState() } } }
    override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); reconcileHoverState() }
}
