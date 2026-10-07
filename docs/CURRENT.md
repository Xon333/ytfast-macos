# Current state

**Evidence cut:** 7 October 2026  
**Accepted source:** `b8e6d0cb2ba66f6f4c8dd9397a6c6dad0b308525`

This page is the current-state front door. Historical implementation discussion stays in Git history and the inherited docs; it should not be used to infer current Mac behavior.

## Product state

The macOS product is a **menu-bar-only YouTube Music player**.

Current visible controls:

- Previous
- Play / Pause
- Next
- Shuffle
- Volume
- Library
  - Playlists
  - Liked Music
  - Albums
- Add song to playlist
- Account / browser profile / Reconnect
- Quit

There is no normal app window or Dock icon in the Mac release.

## Current architecture

The Mac package is one native Swift/AppKit executable linked to the Rust YTfast core as a static library.

The Mac dependency graph intentionally excludes the desktop renderer stack. CI verifies that the `menubar` feature does not pull in egui, eframe, wgpu, winit or fastframe-fonts.

Runtime helpers:

| Process/tool | When used |
| --- | --- |
| YTfast | menu, state, YouTube API, queue |
| `mpv` | active audio playback |
| `yt-dlp` | stream resolution |
| Deno | YouTube JS challenge support used by yt-dlp |

A signed-out idle app must not launch mpv, yt-dlp or Deno.

## Resource evidence

Controlled comparison from GitHub Actions run `37575953091`, on the same macOS ARM64 runner with isolated empty homes:

| Build | Idle RSS |
| --- | ---: |
| Previous visible-window Mac port, commit `837ad951…` | 173,785,088 B (~165.7 MiB) |
| Native menu build, commit `b8e6d0cb…` | 32,653,312 B (~31.1 MiB) |
| Reduction | **81.2%** |

The standalone native idle sample reported:

- RSS: 32,686,080 B
- physical footprint: 10,749,888 B

These numbers are **not** a prediction of memory during real playback. They exclude shared OS helpers and temporary resolver processes and were measured signed out. A user-observed previous live session reached 421 MB; that observation motivated the rewrite but was not itself an allocation profile.

## Verification at this cut

The accepted workflow passed on macOS and Linux:

- `cargo fmt --all --check`
- strict Clippy for the renderer-free menu core;
- renderer-free unit tests;
- retained Linux desktop UI checks;
- native audio transport test using production mpv IPC/options;
- Swift/AppKit compile and app packaging;
- architecture + ad-hoc signature checks;
- native status-item self-tests;
- signed-out launch;
- CLI Show / Quit;
- no unexpected idle mpv / yt-dlp / Deno child.

Native menu regression coverage includes:

- pre-launch wake handling;
- exactly one YTfast status item;
- shuffle dispatch;
- editable-playlist-only Add destinations;
- captured song identity while an Add menu is open;
- bounded playlist menus;
- signed-out gating.

## Not established by CI

The automated checks do **not** prove:

- your real browser/Keychain authentication;
- real playlist writes against your account;
- audible gapless playback on your device;
- Bluetooth/audio-device routing;
- live-session memory on your Mac;
- every YouTube Music account edge case.

The real acceptance pass is ordinary use: reconnect, choose a playlist, play several tracks, toggle shuffle, add one song to an owned playlist, and verify it in YouTube Music.

## Resource policy

The current Mac build keeps these explicit bounds:

- prepare current + next track rather than UI-wide speculative resolution;
- mpv forward buffer: 4 MiB;
- mpv back buffer: 1 MiB;
- read-ahead: 60 seconds;
- resolved stream cache: 32 entries;
- library page cache: 8 pages;
- native menu page: 40 entries before a More submenu;
- parsed page ceiling: 1,000 rows.

These are engineering bounds, not total-memory caps.

## Known limits / deliberate omissions

Not currently implemented:

- full desktop browsing UI on macOS;
- artwork in the native menu;
- Home / Explore / Lyrics surfaces in the Mac package;
- playlist create/delete/edit beyond Add song;
- automatic system light/dark UI;
- native song notifications;
- notarized distribution.

The inherited desktop UI remains available for Linux/development.

## Evidence hierarchy

When documents disagree:

1. current code + passing workflow at the referenced cut;
2. this page and [MACOS](MACOS.md);
3. [AGENTS](../AGENTS.md);
4. inherited [SPEC](SPEC.md) / [integration research](integration.md) for desktop/history context only.

Do not reinterpret an older desktop requirement as current macOS scope.
