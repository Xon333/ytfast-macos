# macOS product and architecture

YTfast is a lean native YouTube Music menu-bar player with one AppKit popover and one Rust playback owner.

## Interface

The 360-point AppKit popover starts and reopens as a compact player with Search, Playlists, Liked and Albums launchers. Only the selected destination expands, showing up to five music rows with scrolling; clicking its launcher again collapses it. Account has its own connection view. A single player card groups transport, seek and volume above the list. The circular Play/Pause control becomes Cancel while a start is pending; Shuffle On / Off explicitly names its saved global state and is available before choosing a song. Seek previews the position during dragging and commits on release. Volume includes mute/restore. Quit and secondary actions live in More (…), including the saved Volume normalization toggle.

The dropdown stays dark independently of system appearance. It uses exact Oxocarbon base00/base06/base10/base12/base13 literals for neutral surfaces, white text, pink highlights and green state; the root background is explicitly overridden to OLED black. Blue/cyan tokens are not used. Existing Mino-derived buttons, AppKit sliders, segmented controls and reusable rows are adapted rather than replaced. One opaque native root replaces the unused visual-effect material plus opaque overlay. Play/Pause draws a true centred circle inside its hit area, independent of AppKit alignment insets.

**Search / Playlists / Liked / Albums** are direct destinations. Search displays local loading state immediately, waits 180 ms before a network request, and runs on Return. Its editor preserves spaces and IME composition; only committed queries are normalized for requests. Search retains a stable viewport through typing, loading and empty results, without leaving preceding results actionable. Leaving Account resumes an interrupted query. Selecting the current song uses transport rather than fetching its queue again. Library pages load only on expansion; connecting or reopening the compact player does not fetch the playlist index or start audio/resolver processes.

Playlists, albums and search destinations open inline. One navigation row switches between library tabs and a collection's Back/title/Play actions. Back restores query and scroll position; closing preserves the destination as history but resets expansion and releases the native page copy; reopening is compact. Rows expose Play and Add on hover or selection, plus a context menu, without confusing selection with activation. Position ticks and unchanged warm opens do not rebuild the table. `NSTableView` instantiates reusable visible cells, not a view per song. Refresh retains the visible list while the account-scoped snapshot and HTTPS path run.

Arrow keys select without playing. Return activates, Space controls playback, Escape goes back, Command-F focuses search (also leaving the Add chooser without submitting it) and Command-R refreshes. Controls retain native accessibility labels, SF Symbol sizes and hover/selected/disabled states. Volume's speaker symbol follows the displayed level, including during dragging. No animation clock or per-row backing layers are added.

The player displays the accepted collection/search context, saved with its queue across launches. More (⋯) contains codec/bitrate/source metadata. Shuffle changes upcoming queue order without restarting the current song. Collection Play actions reflect the global mode and choose a random loaded first song when shuffled; explicit song selections are preserved. The existing reversible queue shuffle owns order for playlists, albums, liked music and search-derived collections.

**Add to playlist** captures the playing song and shows only server-confirmed editable destinations. Playback advancing does not change the add target. A seek begun on one song cannot affect its successor; backend updates do not move a slider being dragged.

**Account** opens an inline connection view. Sign in opens the selected supported browser; Connect/Reconnect rechecks its session. A profile picker appears only when there is a choice. An unavailable selected profile is identified rather than displayed as a different account. Browser-access errors show a Full Disk Access action, and detailed errors are available on demand. Successful action notices clear after a one-shot delay; errors stay actionable.

## Ownership

| Component | Responsibility |
| --- | --- |
| AppKit popover and macOS media controls | Display state and dispatch actions |
| Rust backend | Queue, transport, account state and ordered writes |
| InnerTube client | YouTube Music API with shared HTTPS connections |
| Resolver: yt-dlp + Deno | Select and resolve audio streams |
| mpv | Audio output, buffering and queued transitions |

The Swift executable links the Rust core statically. The `menubar` dependency graph excludes egui, eframe, winit, wgpu and the desktop image/font stack. There is no WebView, browser player or UI polling/repaint timer; backend events wake the native run loop. Catalogue updates serialize only the expanded page, borrowed from the bounded Rust cache, directly into the bridge buffer; Swift consumes bytes directly into `Data`. Collapsed state sends no catalogue rows. Neither the app shell nor the panel state retains a duplicate catalogue. Closing clears native page/row copies. The native search field disables unneeded completion, writing, spelling and data-detection services without changing system preferences; this does not establish removal of the OS AutoFill helper.

The app is an `LSUIElement` with one status item and no normal Dock window. The separate macOS global Now Playing item is OS-owned. mpv's own media-key handling is disabled so native controls retain one owner.

`PopoverDismissal` owns closing while AppKit retains popover placement and anchoring. Application-defined behavior prevents transient mouse-down dismissal from racing the status item's mouse-up toggle. Local/global mouse monitors ignore the status anchor and attached controls; outside clicks, other-app activation, Space changes and Escape close the panel. Monitors and workspace observers are removed on close/termination and are absent while hidden. Mouse-only monitoring requires no new keyboard-monitoring permission.

The optional `desktop-ui` target is separate from the Mac package.

## Playback path

On selection of a known song, its stream lookup, authoritative queue request and mpv startup proceed concurrently. A preparation object owns the shared resolver request and player-start task across the queue handoff. The returned queue still selects the actual current song: a mismatched suggested video ID is discarded rather than played under different metadata. Playlist-only targets start mpv while the queue identifies their first song.

The authoritative queue request starts before old-player cleanup waits. A handoff does not repeat a stop command already sent for that queue. The next cold resolver waits until the current load is accepted, avoiding competing yt-dlp/Deno work during a first start. Loudness/history metadata is fetched separately and does not gate stream readiness. A quick skip shares an existing next-track lookup. Cancelling, replacing or rejecting a queue drops its pending preparation; each cold player attempt has its own socket. Player adoption reapplies the latest volume, including adjustments made during loading.

The native mpv invocation reuses the installed player's `[libmpv]` profile and disables its unused built-in scripts (stats, console, OSD console, commands, select and auto-profiles), OSD and adjacent-file autoload. It keeps the same decoder, output, buffering and prefetch behavior. This requires mpv 0.41+; no mpv GPL source is copied into YTfast.

mpv's eight property observations are submitted in one ordered IPC write, with every request ID/reply checked and all pending replies released on cancellation. Selection-to-playback timing includes queue latency; diagnostic stage timings distinguish queue readiness, stream/player readiness and accepted load. Actual mpv playback events determine completion. Buffer bounds and the reliable audio-output defaults are retained.

Pause cancels a pending start while preserving its selected track and seek position. Playback, queue and account generations reject obsolete completions; actual mpv events determine loading, seeking and playing state. Account verification cancels pending starts and queued successors and blocks fresh playback until the connection is accepted. Already-loaded audio keeps its pause, seek and volume controls.

The native path prepares the current and next track. It does not resolve visible library rows or fetch unused watch-next metadata. A saved queue is restored paused without starting mpv, yt-dlp or Deno.

The resolver uses yt-dlp's best audio selection with language, quality, source, codec and bitrate ordering. It accepts suffixed Premium formats and future audio format IDs, preserves original/default-language preference and avoids DRC variants when a better equivalent exists. Upstream Premium-aware client selection and account discovery remain intact. Only unusable DASH fragment-manifest extraction is skipped; direct HTTPS and HLS audio remain available.

The displayed codec/bitrate comes from the selected source's metadata rather than a fixed bitrate inferred from its itag. Sample rate, non-stereo channel count and format ID are available in the tooltip. Existing valid URL caches remain readable; entries without source metadata display only known codec/Premium information. The resolver scopes cached URLs and metadata to the selected browser session and only reuses URLs with more than ten minutes remaining. A matching session can reuse valid URLs across launches. mpv receives the selected stream directly and prepares the queued transition.

When enabled, loudness normalization uses YouTube's metadata with attenuation only; it never applies positive gain without peak-headroom evidence. The native More menu now exposes this existing setting. It does not alter the selected codec or bitrate, transcode audio or impose additional compression.

## Resource bounds

Native catalogue transport is at most one expanded page; the collapsed panel retains no page/row snapshot. These are not total-process memory guarantees.

| Resource | Current bound |
| --- | --- |
| Library pages held in memory | 8 |
| Rows exposed per loaded page | 1,000 |
| Navigation history | 16 locations |
| Persisted library snapshots | 32 files, 8 MiB total, 1 MiB per file, 7 days |
| Resolved stream URLs | 32 entries; expire before their signed URLs |
| mpv forward / back buffers | 4 MiB / 1 MiB |
| mpv read-ahead | 60 seconds |

All native library pages load on demand, including the playlist index. No artwork, Home feed or lyrics are fetched for the native interface. Bounds limit individual caches and buffers; they are not a total-process memory guarantee.

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

There is no hosted YTfast server or telemetry. Authenticated traffic goes directly to YouTube/Google. Cookies, Safe Storage secrets, real-account captures and private logs must not enter Git or CI artifacts.

## Design provenance

The action controls **reuse and adapt Mino's AppKit `MenuActionButton` source**, including its color/tracking/reset lifecycle: [revision `0ee56b7`](https://github.com/nad-bit/Mino/blob/0ee56b782b60c47ae1313d3f2c5c07bfb7497d0e/SwiftApp/Sources/RepoMenuItemView.swift). `native/MenuActionButton.swift` retains native button behavior and uses YTfast's existing layer-free drawing, larger hit targets and accessibility. `native/VolumeSymbol.swift` adapts **MacControlCenterUI's five volume levels and SF Symbols**: [revision `0869227`](https://github.com/orchetect/MacControlCenterUI/blob/086922750e4286477431be75032218350441db3c/Sources/MacControlCenterUI/Controls/MenuSlider/MenuSliderImage/VolumeMenuSliderImage.swift). `native/NativeTheme.swift` adapts exact non-blue **Oxocarbon palette literals** from [revision `cd6523a`](https://github.com/nyoom-engineering/oxocarbon.nvim/blob/cd6523a0836d6e8ee823d343149fd06c7b71fdde/lua/oxocarbon/init.lua#L9-L12), with the explicit OLED-black root override. Its Neovim/Lua highlighting code cannot run as an AppKit component, so only compatible tokens are adapted. Existing Mino button lifecycle and native navigation/queue implementations are reused for the compact flow and named shuffle state. The complete MIT notices are kept in `native/ThirdParty` and the app bundle.

[Radio's player card at `f50fa2d`](https://github.com/pom11/Radio/blob/f50fa2d9685be17bfbdee0534ed9408dace8e2c1/Sources/PlayerControlCard.swift) was inspected for reuse. It is coupled to Radio's SwiftUI player/store/output types and macOS 14 target. MacControlCenterUI's larger controls similarly require a SwiftUI host and its MenuBarExtraAccess dependency. Those integrations are unsuitable for this bounded AppKit correction; the compatible source is adapted directly. Mino's search handler lacks composition protection, so it is not transplanted. Existing table reuse, navigation and slider ownership are retained.

[Sonora](https://github.com/sonorahq/sonora) informed the existing playback preparation, snapshot and account-isolation design. YTfast keeps its own InnerTube/yt-dlp/mpv implementation.

For native interaction, [MonitorControl's slider handler](https://github.com/MonitorControl/MonitorControl/blob/84ac2d72bfb53b653536e484946f6ed027e4229c/MonitorControl/Support/SliderHandler.swift) informed explicit mouse-tracking ownership and compact volume controls. [Maccy's popup model](https://github.com/p0deje/Maccy/blob/a92c11ae3e7a86a57dc6359bad59e330e4625ede/Maccy/Observables/Popup.swift) informed compact sizing and the separation of navigation from activation. These patterns are independently implemented with AppKit; no external UI framework or GPL source was incorporated.

## Build and evidence

Build on macOS 13+ with Rust 1.98+, Xcode Command Line Tools and Homebrew `mpv` 0.41+, `yt-dlp` and `deno`:

```sh
scripts/build-macos.sh
```

Outputs are `dist/YTfast.app` and `dist/ytfast-macos-<arch>.zip`. The app is ad-hoc signed and is not notarized.

See [CURRENT](CURRENT.md) for dated evidence and material limits, and [AGENTS](../AGENTS.md#verification) for focused checks. [SPEC](SPEC.md) and [integration research](integration.md) retain the inherited desktop reference material.
