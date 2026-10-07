# macOS contract

This is the current product and architecture contract for the YTfast Mac build.

The goal is **a very small native YouTube Music menu-bar player**, not a reduced version of the full desktop UI.

## Product boundary

The Mac release must provide:

- reliable audio playback;
- previous / play-pause / next;
- shuffle;
- volume;
- library browsing for Playlists, Liked Music and Albums;
- selecting music to play;
- adding the currently selected/playing song to an editable playlist;
- account profile selection / Reconnect;
- native macOS media controls;
- persistence compatible with the existing Rust core.

It should not carry a rendering framework merely to reproduce a desktop music application.

The inherited desktop UI remains a separate `desktop-ui` build and is not the Mac product target.

## Architecture

```text
AppKit NSStatusItem + MPRemoteCommandCenter
                  │
                  ▼
          Rust menubar bridge
                  │
        ┌─────────┴─────────┐
        ▼                   ▼
 YouTube Music API        Player state
 / account writes          / queue
        │                   │
        └─────────┬─────────┘
                  ▼
              resolver
          yt-dlp + Deno
                  │
                  ▼
                mpv
             audio only
```

### One player owner

AppKit is a thin view/controller. It does not implement a separate playback engine.

The native menu, macOS media commands and CLI all dispatch into the same Rust state.

This is the key architectural rule: **one source of playback truth**.

### Renderer-free Mac package

The `menubar` Cargo feature excludes:

- egui / eframe;
- winit / GPU UI backends;
- desktop image/font/theme stack;
- desktop window state.

The Swift shell uses AppKit directly and links the Rust core statically.

No WebView or browser engine is used.

## Resource behavior

### Lazy interface work

The app does not pre-load a full music UI.

- Library pages load when their menu opens.
- Cached library data expires and can be explicitly refreshed.
- No menu artwork is fetched.
- No Home feed or lyrics are loaded.
- Backend state changes wake the native run loop; there is no UI polling/repaint timer.

### Bounded menus and caches

- 40 rows per native submenu page;
- continuations exposed as Load more;
- parsed page ceiling 1,000 rows;
- up to 8 cached library pages;
- 32 resolved stream URLs.

These limits keep large libraries from becoming one huge native menu or unbounded in-memory history.

### Playback preparation

The Mac path prepares the **current and next track**.

Desktop-only speculation such as visible-page/hover/two-ahead resolution is not part of the native menu workflow. A saved crossfade preference is not rewritten merely because the Mac menu build does not run the desktop crossfade path.

### mpv limits

The native path uses bounded audio buffering:

- 4 MiB forward cache;
- 1 MiB back cache;
- 60 s read-ahead.

These settings do not reduce the selected stream codec/bitrate.

## Menu behavior

Top level:

- track title / artist;
- Previous;
- Play or Pause;
- Next;
- Shuffle;
- Volume;
- Library;
- Add song to playlist;
- Account;
- Quit YTfast.

### Library

Library exposes:

- Playlists;
- Liked Music;
- Albums.

Playlist browsing is hierarchical. Opening a playlist does not change playback by hover alone; choosing Play or a song does.

### Add song to playlist

Only server-confirmed editable playlists are offered.

The Add menu captures the song identity when the menu is opened. If playback advances before the click, the action still applies to the song shown by that Add menu.

The native build does not currently create, delete or broadly edit playlists.

## macOS media controls

YTfast registers with `MPRemoteCommandCenter` / `MPNowPlayingInfoCenter` for the same Rust player.

The mpv helper runs with its own media-key ownership disabled so there is not a competing player-control owner.

macOS may independently show its global circular **Now Playing** item. That is system UI and cannot be merged into YTfast's status item or application process.

## Authentication

Supported browsers:

- Google Chrome;
- Brave;
- Chromium.

Browser roots are under `~/Library/Application Support`. Both modern `Network/Cookies` and legacy `Cookies` locations are handled.

Profile discovery does not need to unlock Keychain. Loading the selected/default session requests that browser's Safe Storage secret.

### Local Keychain boundary

The app invokes macOS Keychain locally for the browser's Safe Storage secret and uses it locally to decrypt browser cookies.

The Safe Storage secret and cookie values must never be printed, logged, committed or included in CI artifacts.

Authenticated cookies are then used for direct YouTube/Google requests and a short-lived local Netscape export for yt-dlp.

A missing profile or denied Keychain access must not silently select another account.

## Files and privacy

| Data | Location |
| --- | --- |
| Settings | `~/Library/Application Support/ytfast` |
| Cache / queue / session data | `~/Library/Caches/ytfast` |
| Runtime socket / temporary cookie export | `/tmp/ytfast-<uid>` |

Private runtime directories are mode 0700 and cookie exports/owned files are mode 0600.

The runtime path is intentionally short enough for Unix-domain socket limits.

The app has:

- no telemetry;
- no YTfast-hosted backend;
- no remote auth service;
- no browser engine.

## Sonora: reference, not base code

Sonora was reviewed as an architecture reference.

What was adopted conceptually:

1. keep provider/playback logic independent of the UI;
2. keep one shared player owner;
3. preload only relevant upcoming media;
4. make resource ceilings explicit.

What was **not** adopted:

- GPUI;
- Sonora's decoder/stream stack;
- its disk-spool design;
- WebView login;
- its multi-service provider layer.

No Sonora source was copied. Sonora is GPL-3.0-or-later; this fork remains MIT because the work used architectural ideas only.

The existing YTfast core remains the better base for this product because it already owns the YouTube Music-specific auth, InnerTube parsing, queue/write semantics and yt-dlp/mpv path.

## Build

Requirements:

- macOS 13+;
- Xcode Command Line Tools;
- Rust 1.98+;
- Homebrew runtime tools: `mpv`, `yt-dlp`, `deno`.

Build:

```sh
scripts/build-macos.sh
```

Outputs:

- `dist/YTfast.app`;
- `dist/ytfast-macos-<arch>.zip`.

The bundle is an `LSUIElement`, so the normal Mac product is menu-bar only.

## Verification

Current automated evidence is summarized in [CURRENT](CURRENT.md).

For source changes, keep verification proportional to impact:

- documentation-only: content/link/current-state checks;
- Rust menu/backend: renderer-free fmt + strict Clippy + affected tests;
- native AppKit changes: build + native self-test/smoke;
- mpv behavior: native audio transport test;
- auth/security changes: synthetic known-answer/regression tests plus real-account acceptance before claiming live success.

Never put a real account's cookies, Safe Storage secret, captured private API responses or screenshots into Git/CI fixtures.

## Current source hierarchy

For macOS work:

1. this file;
2. [CURRENT](CURRENT.md);
3. [AGENTS](../AGENTS.md);
4. current source/tests.

The inherited [SPEC](SPEC.md) and [integration](integration.md) documents are retained desktop/Linux research. Use them for the specific technical fact you need, not as current Mac scope.

If an inherited requirement conflicts with this contract for macOS, this contract wins.
