# Native menu-bar build

## Accepted direction

2026-10-07: the user reported 421 MB for YTfast (plus a separately shown 13.1 MB
AutoFill helper) and chose a basic menu over the full UI. That screenshot is a
user observation, not an allocation profile or a benchmark of all helper processes.
The requested features are transport, shuffle, playlist/library selection and adding
the current song to a playlist. This supersedes the original desktop-UI Mac scope.

## Implementation

The Mac package is a single Swift/AppKit executable linked with the existing Rust
backend as a static library. Cargo's `menubar` build excludes egui, eframe, winit,
GPU backends, image decoders, fonts and the desktop theme/tray stack. Those remain
behind `desktop-ui` for Linux and the optional legacy interface. Authentication,
playlist parsing, queue, seek, gapless mpv playback, quality selection, normalization
and session persistence are reused instead of introducing a second music engine.

There is one NSStatusItem with native menus. No artwork is fetched, no Home feed or
lyrics are loaded, and no window renders in the background. Backend notifications
wake the main queue; they are coalesced rather than serviced by a polling timer.
Library pages are requested on opening their menus, refreshed after five minutes,
and retained in an eight-page cache. Native submenus show 40 entries at a time.
API continuations expose Load more; a 1,000-row page ceiling is explicitly labelled.
Only server-confirmed editable playlists can accept Add. The action captures the
song ID at menu-open time; it never substitutes a newly playing song. Pending/success
and refusal states are distinct. It does not delete or create playlists.

Only the current/next tracks are resolved. No screen/hover or two-ahead speculation,
startup resolution of paused sessions, extra audition deck, or crossfade deck runs
in this build. A saved crossfade preference is not overwritten. The native mpv
forward buffer is capped at 4 MiB, back buffer at 1 MiB, and read-ahead at 60 seconds;
this does not lower the codec/bitrate. Buffer sizes are not total process memory.
Resolved URLs are retained in a 32-entry cache rather than the whole listening history.

Native MPRemoteCommandCenter/Now Playing controls drive the same Rust state as the
menu. The mpv helper has `input-media-keys=no`, avoiding separate ownership by the
helper. macOS may still show its global circular Now Playing menu alongside the
app's music-note menu. These cannot be merged into one OS process or status item;
this app does not change the user's global menu-bar settings.

## Sonora review

Reviewed `sonorahq/sonora` at `9f6567874582749b675a59ee705a8447191da6b6`:

- `CLAUDE.md` and `crates/music/src/engine.rs`: provider/playback code independent
  of the GPUI interface, one shared player owner. Applied by extracting shared
  account/desktop types and building the existing engine without desktop dependencies.
- `crates/state/src/playback.rs` (`preload_next`, `preload_upcoming`): prepare the
  relevant next track rather than UI-wide speculative work. The existing yt-dlp
  resolver is kept because it already implements this fork's signed-in quality path.
- `crates/music/src/stream.rs`: bounded buffering and explicit resource ceilings.
  Adopted the principle using mpv's supported audio buffer controls; did not import
  Sonora's disk spool, decoder stack, GPUI, WebView login or other service providers.

These are architecture references, not copied source. Sonora is GPL-3.0-or-later;
no GPL code was incorporated and the fork remains MIT. Primary source:
https://github.com/sonorahq/sonora/tree/9f6567874582749b675a59ee705a8447191da6b6
mpv option semantics: https://mpv.io/manual/stable/#demuxer
mpv media-key owner: https://github.com/mpv-player/mpv/blob/master/osdep/mac/remote_command_center.swift

## Install and paths

Install `mpv`, `yt-dlp`, `deno` through Homebrew. Quit the previous app, replace
`/Applications/YTfast.app`, and reopen. There is no ordinary window or Dock icon.
Bundle ID stays `io.github.xon333.ytfast`; settings and queue remain under
`~/Library/Application Support/ytfast` and `~/Library/Caches/ytfast`. The runtime
socket/export directory stays `/tmp/ytfast-<uid>`, private mode 0700, exports 0600.
No migration clears the user's queue, settings or old cache. Old cover files are
left on disk but are not loaded. No new Keychain storage or sign-in server exists.

The local browser Safe Storage secret decrypts cookies locally; authenticated
cookies go to YouTube/Google. Keys/cookies must never enter logs, artifacts or Git.
Cached account pages from the old unscoped disk cache are not shown by the native
menu; fresh authenticated requests determine library contents and write access.

`Account` contains Reconnect and the existing browser-profile selector. Closing a
menu never quits audio; Quit YTfast/Command+Q or CLI `ytfast quit` saves and stops it.
The native executable also accepts existing transport and Show CLI commands.

## Verification boundaries

CI checks the renderer-free dependency graph, strict Rust Clippy, unit tests,
Swift compilation, architecture, ad-hoc signature and package. Native AppKit tests
check one status item, shuffle dispatch, editable-only destinations, captured song
IDs and signed-out gating. An isolated signed-out launch checks CLI Show/Quit and
absence of idle playback/resolver children. In-process task_info records RSS and
physical footprint without any account data. A synthetic local-WAV test runs the
production mpv IPC/options, checks pause/seek/resume, queue transitions and exit;
null audio output is enabled only in Rust test builds.

These checks do not prove live YouTube authentication, real playlist writes,
audible gapless quality, Bluetooth routing or your machine's memory use. The final
live check is normal listening: select a playlist, toggle shuffle, add a song to
an owned playlist and confirm it in YouTube Music. No destructive account tests
are run automatically. The GitHub Actions result is the evidence per commit; this
document alone is not a claim that every test has run.
