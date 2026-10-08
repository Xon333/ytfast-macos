# YTfast for macOS

**A small YouTube Music player in your menu bar.** Native AppKit + Rust, without a WebView, browser player or Dock window.

[**Download 0.5.0 · Apple Silicon**](https://github.com/Xon333/ytfast-macos/actions/runs/37814352878/artifacts/11568240145) · [Verified state](docs/CURRENT.md) · [CI builds](https://github.com/Xon333/ytfast-macos/actions/workflows/ci.yml)

## Use

The player opens compact: your song, the playing collection, transport and four library launchers. **Search / Playlists / Liked / Albums** expand only when selected. Click the selected launcher again to collapse; closing the popover always returns the next open to the compact player.

**Shuffle On / Off** controls the current queue and future collections. It preserves the current song; a collection's Play action becomes Shuffle when enabled and starts at a random loaded song. Explicitly selecting a song still starts that song. Turn shuffle off to restore the collection's order. The mode is available before choosing music.

OLED-black surfaces use [Oxocarbon](https://github.com/nyoom-engineering/oxocarbon.nvim)'s neutral, pink and green tokens, with no blue accents. Play/Pause stays circular. Codec, bitrate and available Premium metadata are in **More (⋯)**, not the player card. The download includes native screenshots with synthetic music data.

**Shortcuts:** `⌘F` search · `⌘R` refresh the expanded view · `Space` play/pause · `Return` activate a selected row · `Esc` back/collapse/close · `⌘Q` quit.

## Install

**Apple Silicon · macOS 13+**

1. Install the audio tools, skipping tools already present (mpv **0.41+**):

   ```sh
   brew install mpv yt-dlp deno
   ```

2. Download the build above. GitHub Actions wraps it in an outer ZIP: extract that, then `dist/ytfast-macos-arm64.zip` inside it.
3. Quit YTfast, move the new `YTfast.app` to **Applications**, replacing the old app, and open it.

The app is ad-hoc signed, not notarized. If macOS blocks it, use **System Settings → Privacy & Security → Open Anyway**.

## Connect YouTube Music

1. Sign in to YouTube Music in **Chrome, Brave or Chromium**, using the profile shown in YTfast's **Account** view.
2. Select **Connect / Reconnect** and approve the browser **Safe Storage** Keychain prompt when requested.
3. If browser access is blocked, select **Allow Full Disk Access…**, enable YTfast under **Privacy & Security → Full Disk Access**, then quit, reopen and reconnect.

Existing working accounts do not need to be reconfigured for these UI changes. Safari and Firefox sessions are not supported. Credentials stay local; authenticated traffic goes directly to YouTube/Google. No hosted sign-in service or telemetry.

## Playback and footprint

Direct mpv playback preserves yt-dlp's source-quality and Premium-aware stream selection. No transcoding or artificial enhancement. Queue requests, stream resolution and player preparation overlap. Unused mpv script interfaces and duplicate media controls are disabled through its own embedding profile; cold next-track resolution waits for the current load to be accepted.

Only the expanded library page crosses the native bridge; closing drops its Swift catalogue copy. The Rust cache remains bounded for quick reopening. No artwork, Home feed, lyrics, idle resolver, theme framework or UI polling loop.

**Memory numbers need a named metric.** App RSS, physical footprint, OS AutoFill helpers and the mpv/resolver processes are not interchangeable. A 50 MB target and consistent sub-100 MB real-Mac use are goals, not established guarantees. [Measurements and limits](docs/CURRENT.md).

<details>
<summary>Build from source</summary>

Requires Rust 1.98+, Xcode Command Line Tools and the audio tools above.

```sh
scripts/build-macos.sh
open dist/YTfast.app
```

Output: `dist/YTfast.app` and `dist/ytfast-macos-<arch>.zip`. The app embeds its exact source revision. See [architecture](docs/MACOS.md) and [contributor guidance](AGENTS.md).

</details>

---

Based on [MayberryDT/ytfast](https://github.com/MayberryDT/ytfast) (MIT). Reuses adapted [Mino](https://github.com/nad-bit/Mino) button source, [MacControlCenterUI](https://github.com/orchetect/MacControlCenterUI) volume symbols and [Oxocarbon](https://github.com/nyoom-engineering/oxocarbon.nvim) colour tokens; MIT notices are bundled. [Radio](https://github.com/pom11/Radio) was inspected for native UI reuse; [Sonora](https://github.com/sonorahq/sonora) remains a playback reference. [License](LICENSE).

*Unofficial. Not affiliated with YouTube or Google.*
