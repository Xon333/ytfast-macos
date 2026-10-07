# ytfast for macOS

A lean macOS port of [MayberryDT/ytfast](https://github.com/MayberryDT/ytfast): a native, audio-only YouTube Music player in Rust + egui. No Electron, WebView or hosted backend. The upstream player and interface are retained; platform integration is adapted rather than rewritten.

![Upstream Now Playing interface, captured on Linux](docs/screenshots/lyrics.png)

## Build on your Mac

Requires macOS 13+, Xcode Command Line Tools, Rust 1.98+ and Homebrew.

```sh
brew install cmake mpv yt-dlp deno
git clone https://github.com/Xon333/ytfast-macos.git
cd ytfast-macos
scripts/build-macos.sh
open dist/YTfast.app
```

The builder produces a native `YTfast.app` and an architecture-labelled ZIP under `dist/`, with two compiler jobs by default. The app is ad-hoc signed, not notarized, and uses your installed Homebrew playback tools rather than bundling their dependency tree. Finder launches find both standard Homebrew prefixes.

## Use

Sign in to YouTube Music in **Chrome, Brave or Chromium**, then open YTfast. Allow access to the browser's Safe Storage item when Keychain asks. Settings → Reconnect refreshes the session; profile selection does not silently fall back to a different account. Safari/Firefox sign-in is not supported.

Home, Explore, Library, search, queue editing, lyrics, equalizer, sleep timer, audition and smooth mixes use the existing upstream implementation. The screenshot above is upstream's Linux screenshot, not evidence of a Mac runtime test.

Closing a window with a queue keeps playback alive. Click the menu-bar note or Dock icon to reopen; right-click the note for playback controls and Quit. **Command+Q** quits, **Command+W** closes and **Command+M** minimizes. **Control+M** or the mini-player button opens the floating mini player. The existing CLI is retained.

The Mac interface uses the existing neutral **dark palette**. Native Control Center/Now Playing, global hardware media keys, song notifications, automatic system appearance switching and URL-scheme registration are not implemented in this first port. Linux integrations stay available behind platform-specific compilation.

## Verification and development

[Native checks](https://github.com/Xon333/ytfast-macos/actions) builds/checks macOS and Linux, runs synthetic tests and produces an Apple Silicon app archive. Consult the run for the exact commit; successful compilation does not establish real-account playback or Keychain behaviour on your Mac.

[macOS notes](docs/MACOS.md) document the platform boundary, paths, source references and remaining interactive acceptance checks. [AGENTS.md](AGENTS.md) governs changes. The inherited [product specification](docs/SPEC.md) and [integration facts](docs/integration.md) describe upstream Linux behaviour; the macOS supplement overrides its earlier Mac exclusion.

## Upstream and credits

Forked from Tyler Mayberry's **ytfast**, starting at `8c10cc2c9afe47d4e35922b566f1af8d4431bb49`. See the [upstream feature guide](https://github.com/MayberryDT/ytfast/blob/8c10cc2c9afe47d4e35922b566f1af8d4431bb49/README.md) for the full interface and Linux instructions.

Built on Carmine Paolino's [fastframe](https://github.com/crmne/fastframe) and pinned egui/winit forks; [egui](https://github.com/emilk/egui) by Emil Ernerfeldt and contributors; [mpv](https://mpv.io); [yt-dlp](https://github.com/yt-dlp/yt-dlp); [Lucide](https://lucide.dev) icons. All fastframe dependencies stay on upstream's v0.2.2 tag, including the native tray implementation.

Unofficial and unaffiliated with YouTube/Google. YouTube's private API can change. The inherited client connects to YouTube/Google and LRCLIB for lyrics, with no ytfast-operated service or telemetry.

[MIT license](LICENSE). Original copyright and icon licensing are retained.
