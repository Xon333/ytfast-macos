import AppKit

/// Exact non-blue tokens from oxocarbon.nvim, adapted to native AppKit colours.
/// MIT, Copyright (c) 2022 Riccardo Mazzarini; full notice is bundled.
/// https://github.com/nyoom-engineering/oxocarbon.nvim/blob/cd6523a0836d6e8ee823d343149fd06c7b71fdde/lua/oxocarbon/init.lua
/// The Neovim highlight engine cannot host AppKit controls. Reuse its base00,
/// base06 and base13 directly; black is the user's explicit OLED override.
/// Existing Mino-derived controls retain their input and hover implementation.
enum NativeTheme {
    static let base = NSColor.black                 // requested OLED override
    static let surface = NSColor(srgbRed: 22 / 255, green: 22 / 255, blue: 22 / 255, alpha: 1) // base00 #161616
    static let text = NSColor.white                 // base06 #ffffff
    static let accent = NSColor(srgbRed: 66 / 255, green: 190 / 255, blue: 101 / 255, alpha: 1) // base13 #42be65
    static let secondary = text.withAlphaComponent(0.7)
    static let border = text.withAlphaComponent(0.18)
    static let hover = text.withAlphaComponent(0.14)

    static func install(on panel: PlayerPanel) {
        panel.view.appearance = NSAppearance(named: .darkAqua)
        panel.titleLabel.textColor = text
        panel.artistLabel.textColor = secondary
        panel.sourceLabel.textColor = secondary
        panel.search.textColor = text
        panel.search.backgroundColor = surface
        panel.seek.trackFillColor = text
        panel.volume.trackFillColor = text
    }
}

/// Reuse the existing opaque backdrop AS the root. An OLED-black panel does
/// not need a second background or the visual-effect material it used to hide.
final class ThemeBackdrop: NSView {
    override var isOpaque: Bool { true }
    override func draw(_ dirtyRect: NSRect) { NativeTheme.base.setFill(); dirtyRect.fill() }
}
