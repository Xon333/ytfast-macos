# Current state

**Updated:** 7 October 2026 · **Build:** 0.3.0, native UI polish

[UI refinement](https://github.com/Xon333/ytfast-macos/pull/2) · [Architecture and bounds](MACOS.md)

Each Mac package identifies its exact source commit in `YTfast.app/Contents/Resources/source-revision.txt` and the `YTfastSourceRevision` Info.plist field. Use that revision to distinguish builds sharing version 0.3.0.

## Applied changes

The 360-point native popover now has consistent control sizes, a prominent Play/Pause/Cancel button, visible shuffle state, stable seek and volume dragging, mute/restore, compact library navigation and an inline account screen. Secondary actions are in More (…). The connection screen uses short status text and exposes detailed errors only on request.

Warm reopening preserves the destination, query, scroll and rows. Ordinary playback-position updates do not reload the catalogue. Arrow keys select without playing; Return activates. Activating the current song uses transport instead of fetching its queue again. A seek begun on one song cannot seek its successor.

Search feedback appears immediately, with a 180 ms request debounce instead of 280 ms. The small playlist index warms once after connection. Browsing does not start mpv, yt-dlp or Deno. The Rust/audio implementation, dependency set, Premium-capable stream selection, account boundaries and cache limits are unchanged by this polish.

## Verified evidence

[Actions run 37675046391](https://github.com/Xon333/ytfast-macos/actions/runs/37675046391) passed on macOS 15 Apple Silicon and Ubuntu for source `98d8e1566f7cfdb263d5d5a9e245201adf99ae31`, checked out as PR merge revision `cd4439325214326c28fce639aa660be378d08a40`.

The [native artifact](https://github.com/Xon333/ytfast-macos/actions/runs/37675046391/artifacts/11507077375) contains the packaged app, UI fixtures, dark/light and connection-screen captures, and the measurements below. Its archive SHA-256 was independently checked: `7922b8f52345117b3358dcd2233f019777db3a67ae354a7c7bd9d499fc25a278`.

| Check | Result |
| --- | --- |
| Renderer-free Rust release suite | 34 passed; transport test run separately |
| Production mpv IPC transport test | Passed |
| Native UI checks | 37 passed |
| Warm reopen handler, mean of 30 fixture cycles | 0.069 ms; zero table reloads |
| 100 playback-position updates | Zero table reloads |
| Apply/layout of 1,000 supplied rows | 11.44 ms; six row views instantiated |
| Signed-out idle physical footprint after 15 seconds | 12.2 MiB |
| Signed-out idle RSS at the same sample | 56.3 MiB |
| Idle audio/resolver children | None |

AppKit typechecking, formatting, strict Clippy, renderer-free dependencies, packaging/ad-hoc signature, isolated launch and Show/Quit passed. The production-view captures were inspected in light and dark appearances. The README uses a compressed native capture with synthetic data, not a mockup.

The timings measure local fixture handling, not the complete click-to-visible animation or YouTube network latency. The memory sample is from isolated, signed-out CI; it is not comparable directly with a real listening session or a controlled before/after test. Final integration and its exact-source build are recorded in PR #2 and the repository's Actions history.

## Real-Mac observations and limits

Before this UI polish, the user confirmed that the app worked and reported approximately 33–50 MB RAM usage. Earlier reports confirmed working browser authentication after Full Disk Access, working library/playlists and Premium Opus playback. These are real-Mac observations about the preceding build, not new measurements of the polished build.

CI has no access to that browser session or physical audio devices. Live YouTube start/transition latency, account writes and audible output were not remeasured here. Distribution remains ad-hoc signed without notarization. The native interface omits artwork, Home/Lyrics and playlist editing beyond adding a song.

## Earlier evidence

[Initial native refinement](https://github.com/Xon333/ytfast-macos/pull/1) and [macos-menubar-20261007.json](evidence/macos-menubar-20261007.json) retain their historical results. Their memory figures do not describe this UI build.
