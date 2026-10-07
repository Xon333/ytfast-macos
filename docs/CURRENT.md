# Current state

**Updated:** 7 October 2026

> **Release evidence pending:** Replace this marker with the verified source commit, Actions run, Mac artifact and current native test/memory results before final integration.

This page distinguishes the applied refinement from evidence about the preceding menu build. The product and exact resource bounds are defined in [MACOS](MACOS.md).

## Applied refinement

- **Native popover:** persistent playback controls, seek/volume, direct search and library sections, inline navigation, reusable table rows and explicit loading/error states.
- **Responsive library:** current lists survive refresh; validated account-scoped snapshots load alongside HTTPS; a late snapshot cannot replace fresh data.
- **Playback preparation:** current/next stream lookups and player startup overlap; metadata is off the readiness path; cancellation, account boundaries and stale mpv events are handled explicitly.
- **Account UX:** immediate Connecting state, usable sign-in/Reconnect/help actions, remembered browser selection and latest-attempt-only session publication.
- **Audio and resources:** Premium-capable selection is retained; normalization applies attenuation only; page/URL caches and audio buffers remain bounded.

The Mac executable is AppKit linked to the Rust player core. Its `menubar` target excludes the desktop renderer and browser engines. A restored paused queue does not start playback/resolver helpers.

## Verification

The local renderer-free release library suite passed: **29 tests passed, one production mpv integration test intentionally ignored in that run**. Native CI runs that transport test explicitly with mpv.

Rust coverage includes frozen account credentials, profile failure without account fallback, obsolete connection results, snapshot privacy/expiry/bounds, account-scoped stream reuse, cancellation while loading, stale playback events and retained refresh rows.

The native release workflow also checks AppKit typechecking, renderer-free dependencies, packaging/ad-hoc signature, synthetic UI behavior including captured Add-song identity, isolated launch, Show/Quit and idle child processes. Results for this candidate belong in the release evidence above.

## Real-account evidence and limits

On the preceding build, the user confirmed that Full Disk Access resolved Chromium authentication, library/playlists worked, and Premium playback used Premium Opus. Those are user-reported observations about that build, not newly repeated acceptance of this refinement.

This execution environment has no access to the user's Mac browser session or audio devices. Synthetic and CI checks do not establish the new build's live playlist writes, audible transitions, Bluetooth routing or live-session memory.

The native product currently omits artwork, Home/Lyrics and playlist editing beyond adding a song. Distribution remains ad-hoc signed without notarization.

## Historical evidence

[macos-menubar-20261007.json](evidence/macos-menubar-20261007.json) retains the earlier signed-out comparison between the visible-window port and initial native menu at `b8e6d0cb…`. It measures those historical builds and is not a memory result for this popover.

Use current source and the release's verified evidence to establish present behavior. Older desktop specifications and earlier implementation decisions are reference material.
