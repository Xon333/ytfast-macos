# Current state

**Updated:** 7 October 2026

**Version:** 0.3.0 · [Refinement and verification](https://github.com/Xon333/ytfast-macos/pull/1)

This page distinguishes measured native behavior from real-account observations about the preceding menu build. The product and exact resource bounds are defined in [MACOS](MACOS.md). Every Mac package embeds its exact checked-out source revision in `YTfast.app/Contents/Resources/source-revision.txt` and the `YTfastSourceRevision` Info.plist field.

## Applied refinement

- **Native popover:** persistent playback controls, seek/volume, direct search and library sections, inline navigation, reusable table rows and explicit loading/error states.
- **Responsive library:** current lists survive refresh; validated account-scoped snapshots load alongside HTTPS; a late snapshot cannot replace fresh data.
- **Playback preparation:** current/next stream lookups and player startup overlap; metadata is off the readiness path; cancellation, account boundaries and stale mpv events are handled explicitly.
- **Account UX:** immediate Connecting state, usable sign-in/Reconnect/help actions, remembered browser selection and latest-attempt-only session publication. Verification blocks fresh playback and retires queued preparation while preserving controls for already-loaded audio.
- **Audio and resources:** Premium-capable selection is retained; normalization applies attenuation only; page/URL caches and audio buffers remain bounded.

The Mac executable is AppKit linked to the Rust player core. Its `menubar` target excludes the desktop renderer and browser engines. A restored paused queue does not start playback/resolver helpers.

## Measured native snapshot

The macOS 15 Apple Silicon job in [Actions run 37650616566](https://github.com/Xon333/ytfast-macos/actions/runs/37650616566) passed against PR source [`124c7da`](https://github.com/Xon333/ytfast-macos/commit/124c7da0a32f22903da1e574088cf6eadbd7dca5), checked out as the PR merge revision `f18681bcf71a8399c31549b8adbd9842d38458a6`. Its [native artifact and captures](https://github.com/Xon333/ytfast-macos/actions/runs/37650616566/artifacts/11496038973) contain the measurements below. These are a dated snapshot; later verification of the account-transition barrier is recorded with the source in PR #1.

| Check | Observed result |
| --- | --- |
| Renderer-free Rust release tests | 31 passed; one transport test excluded from this invocation |
| Explicit production mpv transport test | Passed |
| Native AppKit behavior checks | 16 passed |
| Refresh/layout of a 1,000-row fixture | 6.93 ms; six row views instantiated |
| Idle physical footprint after 15 seconds | 12,470,400 bytes (11.9 MiB) |
| Idle RSS at the same sample | 55,951,360 bytes (53.4 MiB) |
| Idle mpv, yt-dlp or Deno children | None |

AppKit typechecking, Rust formatting/strict Clippy, renderer-free dependencies, packaging/ad-hoc signature, isolated launch and Show/Quit also passed. The README image is an actual native capture using synthetic music data, with both dark and light captures in the artifact.

The 1,000-row timing measures applying a supplied fixture and laying out the production AppKit table. It is not network latency or playback startup. The idle memory sample is from an isolated signed-out app on CI, not a real listening session or a controlled before/after comparison.

Rust coverage includes frozen account credentials, profile failure without account fallback, obsolete connection results, snapshot privacy/expiry/bounds, account-scoped stream reuse, cancellation while loading, stale playback events and retained refresh rows. Native fixtures cover loading cancellation, captured Add-song identity, search deduplication, stable navigation, sign-in actions and connection progress.

## Real-account evidence and limits

On the preceding build, the user confirmed that Full Disk Access resolved Chromium authentication, library/playlists worked, and Premium playback used Premium Opus. Those are user-reported observations about that build, not newly repeated acceptance of this refinement.

This execution environment has no access to the user's Mac browser session or audio devices. Synthetic and CI checks do not establish the new build's live playlist writes, audible transitions, Bluetooth routing or live-session memory.

The native product currently omits artwork, Home/Lyrics and playlist editing beyond adding a song. Distribution remains ad-hoc signed without notarization.

## Historical evidence

[macos-menubar-20261007.json](evidence/macos-menubar-20261007.json) retains the earlier signed-out comparison between the visible-window port and initial native menu at `b8e6d0cb…`. It measures those historical builds and is not a memory result for this popover.

Use current source and the release's verified evidence to establish present behavior. Older desktop specifications and earlier implementation decisions are reference material.
