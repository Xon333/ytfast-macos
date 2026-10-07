# YTfast for macOS

A small native YouTube Music menu-bar player. Rust handles music; AppKit displays
one menu. No browser engine, full desktop window, artwork cache or rendering loop.

**Play/pause · Previous/Next · Shuffle · Volume · Library · Add song to playlist**

Library contains Playlists, Liked Music and Albums. Playlists open into their songs;
only playlists the account can edit appear in Add song to playlist. That action
adds the song named when the menu opened, even if the next song starts meanwhile.

## Install

Install runtime tools once: `brew install mpv yt-dlp deno`.
Unzip the Apple Silicon build, replace `/Applications/YTfast.app`, then open it.
Quit the old YTfast first. There is no Dock window; click the music note in the menu bar.
The bundle identity and existing settings/queue paths are retained.

Sign in to YouTube Music in Chrome, Brave or Chromium. Account → Reconnect reads
that local session. Allow the browser's Safe Storage item when Keychain asks.
Safari/Firefox sessions are not supported. The app is ad-hoc signed, not notarized.

## Build

On a Mac with Rust 1.98+ and Xcode Command Line Tools:

```sh
scripts/build-macos.sh
open dist/YTfast.app
```

The script defaults to two compiler jobs and emits an architecture-labelled ZIP.
The binary contains both the native interface and Rust core, not two application
processes. `mpv` is the audio helper; `yt-dlp`/Deno run only for stream resolution.
The macOS Now Playing icon is system-owned, not a second YTfast menu/process.

[Mac design, source references and verification](docs/MACOS.md).
The previous desktop interface remains available with `cargo run --release`
(default `desktop-ui` feature); it is not inside the Mac menu-bar release.

## Credits and license

Fork of [MayberryDT/ytfast](https://github.com/MayberryDT/ytfast), by Tyler Mayberry.
The original Rust player, YouTube API client and account logic are reused.
The retained desktop UI uses [fastframe](https://github.com/crmne/fastframe),
egui and Lucide; runtime playback uses mpv and yt-dlp. Sonora informed the separation
of UI/player and bounded work; no Sonora code or GPUI dependency is included.

MIT; retain [LICENSE](LICENSE). Unofficial, not affiliated with YouTube or Google.
