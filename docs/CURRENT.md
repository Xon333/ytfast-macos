# Current state

**Updated:** 8 October 2026 · **Candidate:** 0.5.0, compact OLED player and bounded native memory

[Implementation and merge record](https://github.com/Xon333/ytfast-macos/pull/5) · [Architecture and bounds](MACOS.md)

## Compact OLED / memory candidate

Branch `perf/native-memory-20261008` continues from main `ca3400f`. The candidate replaces the blue theme with exact non-blue Oxocarbon literals on an OLED-black root, uses a true circular Play/Pause, exposes Shuffle On / Off globally, and opens only the selected library destination. Closing returns to a compact player showing its saved collection context. Audio format metadata moves to More.

The native bridge sends only one expanded page and releases native catalogue copies on close. An opaque root replaces redundant material compositing. The audio process reuses mpv's own embedding profile plus measured disabling of unused script interfaces. Current-start work no longer competes with a fresh successor resolver; queue HTTP starts before old-player cleanup, with no duplicate stop on handoff. Quality selection and buffer bounds are unchanged. The repo-wide exact-source → adaptation → replacement order is explicit in AGENTS.

### Reused, controlled audio-process probe

[Run 37803401851](https://github.com/Xon333/ytfast-macos/actions/runs/37803401851), source `c8d8973`, compared three fresh processes per policy on the same macOS 15 runner. Workload: generated 48 kHz stereo 256 kbps Opus, current plus queued file, unchanged 4/1 MiB cache bounds and null audio output. These are offline decoder/cache observations, not YouTube or sound measurements.

| Median of three trials | Previous options | Embedding profile + unused script UIs off |
| --- | ---: | ---: |
| Load → mpv playback-restart | 14.76 ms | 5.67 ms |
| Playing physical footprint, vmmap M | 19.7 | 18.4 |
| IPC ready | 159.19 ms | 190.85 ms |

This establishes a smaller scripted-player footprint and lower local load-to-restart time in that fixture; it does **not** establish faster process startup. It does not prove 50 MB or consistently sub-100 MB on the user's Mac. RSS, app footprint, OS AutoFill and audio/resolver children must be reported separately. Current user screenshots show a 107.5–133 MB app-row value without column headers; its metric is not inferred.

Candidate compilation, native captures and focused verification are pending. The package/evidence below belongs to the preceding accepted implementation, not this candidate.

## Exact verified package

[CI run 37795329364](https://github.com/Xon333/ytfast-macos/actions/runs/37795329364) passed on macOS 15 Apple Silicon and Ubuntu for source **`8070a06197d1d9e3e8331b92399166c7b8d1f386`**.

[App and synthetic evidence](https://github.com/Xon333/ytfast-macos/actions/runs/37795329364/artifacts/11558552619) contains `dist/ytfast-macos-arm64.zip`. It is version 0.4.0, packaged as GitHub PR revision **`b62ff2a0ac77bbeee698ea531cd85b5eff7342f4`**, whose tree equals the tested source tree. The final owning-document update is documentation only and reuses this build; the package revision is not the subsequent merge commit.

| Identity | SHA-256 |
| --- | --- |
| Actions archive | `0229023ef9d13dd5479f6517ed1532c412362a5af27e4e39895a0cab5e3dd8a6` |
| Inner installable ZIP | `b1676c35e2d72b0a4c051d53a6daa3239df359fc90d1789916d2fc01a4116e76` |
| App executable | `4868728ea0ce1727b2cfdeb7e8061bc133ef360f6203d765e122ca5e7e7c7889` |

The downloaded ZIP, executable permissions, both embedded revision fields and bundled license notices were checked. `Contents/Resources/source-revision.txt` and `YTfastSourceRevision` identify the package exactly.

## Verification and acceptance scope

The final run passed AppKit typechecking, formatting/strict Clippy, 44 renderer-free core release tests on each platform, the 31-test retained desktop suite, production mpv IPC, the offline yt-dlp format contract, packaging/ad-hoc signature and isolated launch/Show/Quit with no idle audio children.

All 53 existing native fixture checks passed. Added focused checks exercised the production popover through 20 anchor-event-order close/reopen cycles, inside/attached-child/outside clicks, own/other-app activation, Space changes, Escape, observer cleanup and dark appearance under a light host. All 13 synthetic production-view captures were inspected. The finish checks exercise AppKit and event routing; they are not a post-fix hardware-pointer trial on the user's macOS 27 installation.

| Final CI fixture | Observation |
| --- | --- |
| Four-row root / equivalent collection | Both 360 × 443 pt |
| Warm reopen, 30 cycles | 0.418 ms mean handler time; zero table reloads |
| Apply/layout of 1,000 rows | 8.424 ms; six instantiated rows |
| Signed-out idle | 13.80 MiB physical footprint / 58.23 MiB RSS |

These isolated shared-runner samples are not a same-Mac performance comparison. The prior paired 8,000-row serialization benchmark measured 12.88 → 2.43 ms; that code remains unchanged. No new live click-to-audio or total-memory improvement is claimed.

## Reused real-Mac evidence and limits

The supplied report inspected candidate source `613c23c`, package `d29b40e`, before the finish-only dismissal and colour changes. Its bounded passes are retained: multiword/trailing-space search, middle edits/caret/paste, interrupted searches, Account/Add exits, collection navigation, observed scroll restoration, and playback/pause/seek/cancellation UI state. Outside-pointer dismissal passed three trials; 20 AXPress panel cycles passed. The pointer-specific D1 failures are not cancelled by those passes.

On that Mac, the report recorded app footprint 34.0M signed-in idle and 43.0M during the playback-state phase; the mpv child was 116.7M at the latter sample. These are separate processes and distinct from RSS. They establish neither a regression nor a leak, and are not measurements of the final dark build.

Actual marked-text composition, audible output/onset/dropouts, settled empty-search visuals, full-account totals and account writes were not accepted. A list-count discrepancy and broad page-corruption report remained unconfirmed; no speculative fixes were made. UI progress/format labels do not prove sound or Premium entitlement. No matched real-Mac baseline or long-term test was available. Distribution remains ad-hoc signed and not notarized.

Raw real-account reports/captures remain outside Git and CI. Earlier evidence is retained in [PR #3](https://github.com/Xon333/ytfast-macos/pull/3), the [first corrective CI run](https://github.com/Xon333/ytfast-macos/actions/runs/37775793148), [PR #2](https://github.com/Xon333/ytfast-macos/pull/2) and [historical evidence](evidence/macos-menubar-20261007.json).
