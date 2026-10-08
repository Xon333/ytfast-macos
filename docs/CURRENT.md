# Current state

**Updated:** 8 October 2026 · **Build:** 0.4.0, native controls and playback preparation

[Implementation](https://github.com/Xon333/ytfast-macos/pull/3) · [Architecture and bounds](MACOS.md)

Each Mac package identifies its exact source commit in `YTfast.app/Contents/Resources/source-revision.txt` and the `YTfastSourceRevision` Info.plist field.

## Applied changes

The native dropdown groups metadata, transport, seek and volume in one compact player card. Library tabs fill the navigation row; a collection replaces that row with Back/title/Play instead of adding another row. Reusable music cells expose direct Play/Add actions on hover or selection and through context menus. Add captures the chosen song, and stale row actions cannot cross an account boundary. Connection screens distinguish browser permission, missing profiles, Keychain and unverified sessions, with complete instructions and a stable layout.

Selecting a known song starts its stream lookup and mpv startup alongside the authoritative queue request. Preparation survives the queue handoff and shares existing next-song lookups; obsolete work is cancelled. The queue still determines the song actually played, and cold-player adoption reapplies the latest volume. Eight mpv property observations now use one ordered IPC write with individually checked replies, removing seven sequential reply dependencies. Stage timings include the original selection and queue wait.

Audio selection follows yt-dlp's language and source-quality ranking, including Premium Opus/AAC, suffixed format IDs and non-DRC alternatives. The selected User-Agent is now forwarded correctly, and quality labels use reported source metadata. Only unusable DASH fragment extraction is skipped; direct HTTPS/HLS audio and upstream Premium discovery remain available. More exposes the existing saved, attenuation-only Volume normalization setting.

Valid account-scoped stream URLs now survive native cleanup and can be reused across launches. Temporary cookie exports, including owned crash leftovers, are still removed. No dependencies or runtime helpers were added; the 32-entry stream cache and existing audio-buffer limits remain. The native panel also releases the duplicate catalogue snapshot after adopting its pages.

## Verified evidence

[Actions run 37744004866](https://github.com/Xon333/ytfast-macos/actions/runs/37744004866) passed on macOS 15 Apple Silicon and Ubuntu for implementation source `be5e7d11ba90e930fdd8264c66d9c2204b4e562e`, checked out and packaged as PR merge revision `9896ef65ded25fec85b9cdd006d5ee2c2a8b3291`.

The [0.4.0 native artifact](https://github.com/Xon333/ytfast-macos/actions/runs/37744004866/artifacts/11534358550) contains `dist/ytfast-macos-arm64.zip`, nine production-view captures with synthetic data, UI results and the isolated memory sample. The downloaded archive's SHA-256 was independently verified: `938574c6b0c0a9ebb9e38d6dea1fdce60f9ba5a4231a66caf0719bd468b65f36`. Its version and embedded revision were also checked. Only this evidence page and the README screenshot were finalized after that run.

| Check | Result |
| --- | --- |
| Renderer-free Rust release suite | 43 passed on each platform; two integration checks run separately on macOS |
| Retained Linux desktop release suite | 31 passed |
| Production mpv IPC transport | Passed |
| Real yt-dlp offline selection contract | Eight synthetic format cases passed |
| Native UI checks | 43 passed |
| Four-row player/library and equivalent collection | Both 360 × 443 pt; collection navigation adds no row |
| Warm reopen handler, mean of 30 fixture cycles | 0.320 ms; zero table reloads |
| 100 playback-position updates | Zero table reloads |
| Apply/layout of 1,000 supplied rows | 24.45 ms; six row views instantiated |
| Signed-out idle physical footprint after 15 seconds | 12.5 MiB |
| Signed-out idle RSS at the same sample | 56.0 MiB |
| Idle audio/resolver children | None |
| Local PCM/null-output mpv startup | 566.09 ms |
| Local PCM load to mpv playback-restart event | 64.46 ms |

AppKit typechecking, formatting, strict Clippy, renderer-free dependencies, packaging/ad-hoc signature, isolated launch and Show/Quit passed. Both visual reviews inspected all nine captures; the README now shows the verified native player.

The preceding main-build capture was 360 × 460 pt. Its separate [CI sample](https://github.com/Xon333/ytfast-macos/actions/runs/37678830140) reported 12.5 MiB physical footprint and 56.3 MiB RSS. These are isolated, signed-out samples on shared runners, not a controlled performance comparison or a total listening-session memory guarantee. UI timings measure fixture handling; the PCM timings exclude YouTube, account access, network resolution and physical audio output. No live click-to-audio speedup percentage is established.

## Real-Mac observations and limits

Before this pass, the user reported working playback, browser authentication after Full Disk Access, library/playlists, Premium Opus and approximately 33–50 MB RAM. Those observations describe preceding builds, not new measurements of 0.4.0.

CI has no access to that browser session or physical audio devices. Live Premium availability, YouTube start/transition latency, account writes, audible output and listening-session RAM were not remeasured. Distribution remains ad-hoc signed without notarization. The native interface omits artwork, Home/Lyrics and playlist editing beyond adding a song.

[Earlier UI refinement](https://github.com/Xon333/ytfast-macos/pull/2), [initial native work](https://github.com/Xon333/ytfast-macos/pull/1) and [historical evidence](evidence/macos-menubar-20261007.json) retain their earlier results.
