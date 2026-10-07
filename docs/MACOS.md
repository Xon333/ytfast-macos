# macOS product and architecture

YTfast is a lean native YouTube Music menu-bar player. The current implementation uses one AppKit popover and one Rust player owner. These choices describe the applied design; current user direction and evidence can justify replacing them.

## Interface

The popover keeps the title, artist, previous/play-pause/next, shuffle, seek, volume and audio/loading status visible above the browser.

Search and **Playlists / Liked Music / Albums** are directly accessible. Search waits 280 ms after typing, or runs immediately on Return. Playlists, albums and search destinations open inline; Back restores the preceding location. A reusable-cell `NSTableView` displays the list, with Load more for continuations.

Refresh retains visible rows and reports progress. Account-scoped disk snapshots can supply the initial list while HTTPS runs. A failed refresh keeps the existing list and displays the error.

**Add song to playlist** captures the song when Add is opened and offers only server-confirmed editable destinations. Playback advancing does not change that captured target.

The Account control exposes connection status, Reconnect, supported browser/profile selection, browser sign-in, Full Disk Access and connection help. Connecting state updates immediately. Native keyboard and accessibility controls remain available; closing the popover does not stop playback.

## Ownership

| Component | Responsibility |
| --- | --- |
| AppKit popover and macOS media controls | Display state and dispatch actions |
| Rust backend | Queue, transport, account state and ordered writes |
| InnerTube client | YouTube Music API with shared HTTPS connections |
| Resolver: yt-dlp + Deno | Select and resolve audio streams |
| mpv | Audio output, buffering and queued transitions |

The Swift executable links the Rust core statically. The `menubar` dependency graph excludes egui, eframe, winit, wgpu and the desktop image/font stack. There is no WebView, browser player or UI polling/repaint timer; backend events wake the native run loop.

The app is an `LSUIElement` with one status item and no normal Dock window. The separate macOS global Now Playing item is OS-owned. mpv's own media-key handling is disabled so native controls retain one owner.

The optional `desktop-ui` target is separate from the Mac package. Retaining its old architecture is not a constraint on future Mac refinement.

## Playback path

On selection, the current stream lookup, next-track lookup and mpv startup can proceed concurrently. Loudness/history metadata is fetched separately and does not gate stream readiness. A quick skip shares an existing next-track lookup.

Pause cancels a pending start while preserving its selected track and seek position. Playback, queue and account generations reject obsolete completions; actual mpv events determine loading, seeking and playing state. Account verification cancels pending starts and queued successors and blocks fresh playback until the connection is accepted. Already-loaded audio keeps its pause, seek and volume controls.

The native path prepares the current and next track. It does not resolve visible library rows or fetch unused watch-next metadata. A saved queue is restored paused without starting mpv, yt-dlp or Deno.

Premium-capable format selection is retained. The resolver scopes cached URLs to the selected browser session and discards them ten minutes before expiry. A matching session can reuse valid URLs across launches. mpv receives the selected stream directly and prepares the queued transition.

When enabled, loudness normalization uses YouTube's metadata with attenuation only; it never applies positive gain without peak-headroom evidence. This change does not alter codec or bitrate selection.

## Resource bounds

| Resource | Current bound |
| --- | --- |
| Library pages held in memory | 8 |
| Rows exposed per loaded page | 1,000 |
| Navigation history | 16 locations |
| Persisted library snapshots | 32 files, 8 MiB total, 1 MiB per file, 7 days |
| Resolved stream URLs | 32 entries; expire before their signed URLs |
| mpv forward / back buffers | 4 MiB / 1 MiB |
| mpv read-ahead | 60 seconds |

Pages load on demand. No artwork, Home feed or lyrics are fetched for the native interface. Bounds limit individual caches and buffers; they are not a total-process memory guarantee.

## Authentication and account isolation

Supported browser profiles are Chrome, Brave and Chromium under `~/Library/Application Support`. Both `Network/Cookies` and `Cookies` layouts are read through SQLite transactions including the WAL. Discovery and session loading reuse one set of snapshots; discovery does not unlock Keychain.

The selected browser's Safe Storage secret is read locally to decrypt applicable cookies. Profile/API verification is staged, and only the newest connection attempt can publish its result. The initial discovered source is remembered. A missing profile, denied Keychain request or failed cookie read never selects a different account.

Full Disk Access may be required to read the protected cookie store. The Account actions open the supported browser or relevant System Settings page; they do not change browser preferences. The browser can open its last-used profile, so sign-in must occur in the profile selected in YTfast.

Each connection owns a private immutable Netscape export, removed when replaced or released. Resolver runs use their own temporary copy. HTTPS operations freeze their session, and account writes remain ordered. Stale replies cannot update a newer account's interface.

Library snapshots and signed stream URLs use an opaque fingerprint of the profile and authentication cookies. Raw cookie values do not enter cache names or metadata. Legacy unscoped page/stream caches are not used for authenticated results. An unreachable API is reported as unverified rather than as proof of browser sign-out.

## Files and privacy

| Data | macOS location |
| --- | --- |
| Settings | `~/Library/Application Support/ytfast` |
| Library snapshots, queue and session | `~/Library/Caches/ytfast` |
| Runtime sockets, signed URL cache, temporary cookie exports | `/tmp/ytfast-<uid>` |

Owned private directories use mode 0700; cookie exports and written cache files use 0600. The short runtime path accommodates Unix-domain sockets.

There is no YTfast backend or telemetry. Authenticated traffic goes directly to YouTube/Google. Cookies, Safe Storage secrets, real-account captures and private logs must not enter Git or CI artifacts.

## Design provenance

[Sonora](https://github.com/sonorahq/sonora) was inspected across its YouTube provider, playback/preload, snapshot, auth/session, state and native-menu architecture. The applied ideas are retained snapshots during refresh, bounded preparation, explicit asynchronous state and account isolation. YTfast keeps its own InnerTube/yt-dlp/mpv implementation. No GPL source was copied into this MIT project.

## Build and evidence

Build on macOS 13+ with Rust 1.98+, Xcode Command Line Tools and Homebrew `mpv`, `yt-dlp` and `deno`:

```sh
scripts/build-macos.sh
```

Outputs are `dist/YTfast.app` and `dist/ytfast-macos-<arch>.zip`. The app is ad-hoc signed and is not notarized.

See [CURRENT](CURRENT.md) for dated evidence and material limits, and [AGENTS](../AGENTS.md#verification) for focused checks. Current code and verified behavior take precedence over inherited [SPEC](SPEC.md) and [integration research](integration.md).
