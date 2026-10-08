import AppKit

/// Catppuccin Mocha palette, reused verbatim (MIT, Copyright (c) 2021 Catppuccin).
/// https://github.com/catppuccin/palette/blob/07d02aa110ef9eb7e7427afca5c73ba9cf7f8ebd/palette.json
/// Only these tokens are needed; no theme package, renderer or runtime parsing.
/// Full notice: ThirdParty/Catppuccin-LICENSE.txt, also bundled with the app.
enum NativeTheme {
    private static func rgb(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat) -> NSColor {
        NSColor(srgbRed: r / 255, green: g / 255, blue: b / 255, alpha: 1)
    }
    static let base = rgb(30, 30, 46)       // #1e1e2e
    static let surface = rgb(49, 50, 68)    // #313244
    static let border = rgb(69, 71, 90)     // #45475a
    static let text = rgb(205, 214, 244)    // #cdd6f4
    static let secondary = rgb(166, 173, 200) // #a6adc8
    static let accent = rgb(180, 190, 254)  // #b4befe, Lavender

    static func install(on panel: PlayerPanel) {
        let root = panel.view
        root.appearance = NSAppearance(named: .darkAqua)
        let backdrop = ThemeBackdrop(frame: root.bounds)
        backdrop.autoresizingMask = [.width, .height]
        backdrop.setAccessibilityElement(false)
        root.addSubview(backdrop, positioned: .below, relativeTo: nil)
        panel.titleLabel.textColor = text
        panel.artistLabel.textColor = secondary
        panel.search.textColor = text
        panel.search.backgroundColor = surface
        panel.sections.selectedSegmentBezelColor = accent.withAlphaComponent(0.22)
        panel.seek.trackFillColor = accent
        panel.volume.trackFillColor = accent
    }
}

/// One static, layer-free surface behind the existing native controls. It
/// cannot receive input or enter the accessibility tree; geometry is unchanged.
private final class ThemeBackdrop: NSView {
    override var isOpaque: Bool { true }
    override func draw(_ dirtyRect: NSRect) { NativeTheme.base.setFill(); dirtyRect.fill() }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}
