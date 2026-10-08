import AppKit

/// Adapted from nyoom-engineering/oxocarbon.nvim (MIT).
/// cd6523a0836d6e8ee823d343149fd06c7b71fdde/lua/oxocarbon/init.lua:9-12.
/// Reuse its exact base00/base06/base10/base12/base13 literals. The requested
/// OLED background is the only base override; blue/cyan tokens are not used.
/// Full notice: ThirdParty/Oxocarbon-LICENSE.txt, bundled with the app.
enum NativeTheme {
    private static func rgb(_ value: UInt32) -> NSColor {
        NSColor(srgbRed: CGFloat((value >> 16) & 255) / 255,
                green: CGFloat((value >> 8) & 255) / 255,
                blue: CGFloat(value & 255) / 255, alpha: 1)
    }
    static let base = NSColor.black              // user-requested OLED override
    static let surface = rgb(0x161616)           // Oxocarbon base00
    static let text = rgb(0xffffff)              // Oxocarbon base06
    static let accent = rgb(0xff7eb6)            // Oxocarbon base12
    static let success = rgb(0x42be65)           // Oxocarbon base13
    static let error = rgb(0xee5396)             // Oxocarbon base10
    static let secondary = text.withAlphaComponent(0.68)
    static let border = text.withAlphaComponent(0.18)
    static let hover = text.withAlphaComponent(0.12)

    static func install(on panel: PlayerPanel) {
        panel.view.appearance = NSAppearance(named: .darkAqua)
        panel.titleLabel.textColor = text
        panel.artistLabel.textColor = secondary
        panel.search.textColor = text
        panel.search.backgroundColor = surface
        panel.sections.selectedSegmentBezelColor = accent.withAlphaComponent(0.22)
        panel.seek.trackFillColor = accent
        panel.volume.trackFillColor = accent
    }
}

/// The existing native root becomes opaque instead of compositing an unused
/// visual-effect material behind another opaque view. No extra layer or view.
final class ThemeSurface: NSView {
    override var isOpaque: Bool { true }
    override func draw(_ dirtyRect: NSRect) { NativeTheme.base.setFill(); dirtyRect.fill() }
}
