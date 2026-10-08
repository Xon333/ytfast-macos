# YTfast for macOS

A lightweight, native **YouTube Music menu-bar player**. Playback, search and your library in one compact dropdown — without a WebView or browser player.

![YTfast native macOS player](docs/screenshots/menu-bar-current.png)

*Native app capture with example music data.*

## Install

Install the audio tools:

```sh
brew install mpv yt-dlp deno
```

Quit YTfast, unzip the Apple Silicon build, replace `/Applications/YTfast.app`, and open it. Click the **music-note** item in the menu bar.

Requires macOS 13+. The app is ad-hoc signed and not notarized.

### Connect

1. Choose **Sign in** to open YouTube Music in Chrome, Brave or Chromium. Use the profile shown in YTfast.
2. Sign in, return to YTfast and choose **Connect** or **Reconnect**. Allow the browser's Safe Storage Keychain prompt if macOS asks.
3. If browser access is blocked, choose **Allow Full Disk Access…**, enable YTfast in System Settings, then quit and reopen the app. This shortcut is also in the **More (…)** menu.

**Account** opens the connection view and browser-profile picker. A single profile is shown as a label. An unavailable selection never silently switches to another account. Safari and Firefox sessions are not supported.

Browser credentials stay local. Authenticated requests go directly to YouTube/Google; YTfast has no sign-in server or telemetry.

## Controls

- **Play/Pause, Previous and Next** stay together with seek and volume in the player card. The stop icon cancels a pending start. Shuffle has a visible on/off state; the speaker button mutes and restores volume.
- **Search, Playlists, Liked and Albums** open in the same dropdown. Back restores the previous page and scroll position. Reopening leaves you where you were; refreshing keeps the current list visible.
- **Plus (+)** adds the playing song to an editable playlist. Music rows also expose Play and Add actions on hover or selection, with a right-click menu. The chosen song remains the add target if playback advances.
- **Volume normalization** in **More (…)** switches between the source level and attenuation-only leveling. Your choice is saved.

Click a music row to play or open it. Use arrow keys to select, Return to activate, Space for playback, and Escape to go back. **⌘F** focuses search; **⌘R** refreshes. Quit is in **More (…)** or **⌘Q**. Closing the dropdown leaves playback running.

## Lightweight playback

The Rust core owns the queue, account operations and audio state. `yt-dlp` with Deno resolves streams; `mpv` plays them. Native media keys and macOS Now Playing control the same player.

Selecting a song starts its stream lookup and player startup alongside queue loading. A shared lookup survives a quick skip; obsolete selections are cancelled. Signed URLs and library snapshots have explicit size and account boundaries.

Audio selection uses yt-dlp's language and quality metadata, including Premium Opus/AAC and suffixed format IDs. The quality label shows the selected source's reported codec and bitrate; its tooltip includes sample rate and format details. No transcoding or artificial quality boost is applied.

The saved queue returns paused. No audio/resolver helpers start until playback is requested. The UI uses reusable AppKit rows and event-driven updates, without artwork fetching or a repaint timer.

[Architecture and resource bounds](docs/MACOS.md) · [Verification and measurements](docs/CURRENT.md)

## Build

With Rust 1.98+ and Xcode Command Line Tools on macOS:

```sh
scripts/build-macos.sh
open dist/YTfast.app
```

Outputs: `dist/YTfast.app` and `dist/ytfast-macos-<arch>.zip`. Each package embeds its source revision. The optional Linux `desktop-ui` target is excluded from the Mac package.

[Contributor guidance](AGENTS.md)

## Credits

Fork of [MayberryDT/ytfast](https://github.com/MayberryDT/ytfast), by Tyler Mayberry. MIT licensed.

[Radio](https://github.com/pom11/Radio), [MacControlCenterUI](https://github.com/orchetect/MacControlCenterUI) and [Mino](https://github.com/nad-bit/Mino) informed the compact player card, control affordances and reusable rows with inline actions. [Sonora](https://github.com/sonorahq/sonora) informed playback and session architecture; [MonitorControl](https://github.com/MonitorControl/MonitorControl) and [Maccy](https://github.com/p0deje/Maccy) informed native interaction patterns. These patterns are implemented with AppKit; no upstream source or UI-framework dependencies were added.

Unofficial and not affiliated with YouTube or Google.
