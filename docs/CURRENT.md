# Current state

**Updated:** 9 October 2026 · **Runtime correction:** 0.5.1 · **Preserved UI evidence:** 0.5.0, compact OLED player

[Implementation and merge record](https://github.com/Xon333/ytfast-macos/pull/7) · [Architecture](MACOS.md) · [Measured audio correction](https://github.com/Xon333/ytfast-macos/pull/6)

## Native runtime correction

The 9 October user-supplied native inspection identified two unused built-in Lua scripts still loaded by the installed 0.5.0 package. The invocation now uses mpv's exact `--load-positioning=no` and `--load-context-menu=no` switches, and the existing production-transport test checks both through IPC. No memory or CPU saving is assigned to this cleanup without measurement. Flat EQ was already bypassed (`af=[]` in the inspection); output selection, audio format, normalization, source-quality selection, cache limits and dependencies are unchanged.

The existing memory probe now has an explicit [real-output comparison](MACOS.md#bounded-real-output-comparison). It compares the installed AVFoundation/CoreAudio backends and a CoreAudio interleaved-float arm with identical media and buffering, without copying or rebuilding an audio backend. The float arm follows [upstream macOS 27 initialization evidence](https://github.com/mpv-player/mpv/issues/18384#issuecomment-5597870245); it is not a validated default. The [upstream AVFoundation pause-loop patch](https://github.com/mpv-player/mpv/pull/18390) remains under review and describes a remaining device-notification race. Neither upstream report diagnoses this user's earlier incident. No dependency upgrade or unreviewed patch is adopted.

Formatting, menu-core Clippy, **48 active core tests** and **seven portable probe checks** passed locally. [Runtime-branch CI](https://github.com/Xon333/ytfast-macos/actions?query=branch%3Afix%2Fnative-audio-runtime-20261009) records native launch/IPC, packaging and regression checks. The transport fixture still uses null output; real-output measurements remain native work. The valid 0.5.0 UI evidence below is retained with its original scope.

## 9 October real-Mac evidence

User-reported measurements on an Apple M4 / macOS 27.0 (26A428), installed revision `d64a6093d0646fc043dc3738238fd1916486f4c5` with the executable hash recorded below:

| Measurement | YTfast | mpv 0.41.0 |
| --- | ---: | ---: |
| CPU, 60.79-second window, fraction of one core | 0.13% | 3.78% |
| Physical footprint, three snapshots | 28.22 / 28.28 / 28.34 MiB | 101.27 / 204.59 / 141.49 MiB |
| Reported process-lifetime footprint peak | 30.8 MiB | 432.14 MiB |

Playback used AVFoundation, 48 kHz stereo Opus, float output and approximately 5 MiB of demux cache. Two short stack samples predominantly showed normal waits; the earlier full-core incident did not recur. Footprint fell as playback continued, so the record establishes variable allocation, not an unbounded leak or its cause. These are separate process footprints, not RSS or a deduplicated application total. Real output A/B, pause/resume, device changes and acoustic acceptance were not performed. The authoring session has no Mac/OMV execution route; these checks remain native work. No claim of sustained sub-100 MB or 50 MB playback is established.

## Applied result

The 360-point player opens at **196 points high** in the connected fixture. Search, Playlists, Liked and Albums expand on demand and collapse on dismissal. The current song/collection remains in the player; codec/bitrate metadata is in More. The Play/Pause fill is a true circle. Visible Shuffle On/Off governs current and future queues, preserves the current song, and changes collection Play into Shuffle. Explicit song selections stay exact; existing queue shuffle/unshuffle is reused.

The root is opaque black. Exact non-blue Oxocarbon tokens supply neutral surfaces, pink accents and green active shuffle; Mino-derived controls and MacControlCenterUI volume symbols remain. The bundled notices were checked. No runtime framework or package was added. Exact reuse → adaptation → rebuild last remains repository-wide guidance in AGENTS.md.

Only the expanded page crosses the Rust/Swift bridge. Closing releases its native page/row copies; the bounded Rust cache remains. There is no eager hidden-library request or hidden visual-effect material. App-local text assistance is disabled where supported, without changing host settings. Queue fetching begins before old-player cleanup, redundant Stop is avoided, and cold successor resolution waits for current load acceptance. Source-quality selection, account isolation and audio-buffer bounds are retained.

## Preserved 0.5.0 package and verification

[CI run 37814352878](https://github.com/Xon333/ytfast-macos/actions/runs/37814352878) passed on macOS 15 Apple Silicon and Ubuntu for source **`0ec7844adc7cc8a5b610804f6912c410d4b152ea`**. The Mac job was retried after a runner-less cancellation. The final README/CURRENT update changes no production code or tests; this valid build evidence is reused.

[Historical CI package and native captures](https://github.com/Xon333/ytfast-macos/actions/runs/37814352878/artifacts/11568240145) document the generated `dist/ytfast-macos-arm64.zip` artifact; current installation uses the [source build instructions](../README.md#build-from-source). Version **0.5.0**, packaged revision **`d64a6093d0646fc043dc3738238fd1916486f4c5`**: its tree equals the tested source tree. This PR build revision differs from the later documentation/merge commit.

| Identity | SHA-256 |
| --- | --- |
| Actions archive | `8b196360333270d645aed8cfb0da428fe6faa252f5696a395e51f5a25671d1fd` |
| Installable ZIP | `3272ec5eff62a1104d81391216857ab43352f12e75e7e6386c3c830eead3d899` |
| Executable | `7145db634f81f55de320fd7d0f7c2d147307fb92432d244ad1783b2060a8b7bb` |

Downloaded archive integrity, hashes, executable permission, version, both revision fields and licenses were checked. AppKit typecheck, formatting/Clippy, core and retained-desktop tests, production mpv transport, offline format selection, renderer-free dependency checks, packaging/signature and isolated launch/Show/Quit/no idle audio children passed. The native result contains **53 regression checks plus 10 compact-flow checks**. All **15 actual AppKit fixture captures** were inspected, including dark appearance under a light host. No clipping or control overlap was observed in those captures.

## Current synthetic measurements

| Native fixture | Result |
| --- | --- |
| Collapsed / four-row expanded height | 196 / 418 pt |
| Warm-open handler, 30 cycles | 0.259 ms mean; zero table reloads |
| 1,000-row apply/layout | 25.787 ms; five instantiated rows |
| Signed-out idle process | 12.74 MiB physical footprint / 58.09 MiB RSS |
| Expanded test-process snapshot | 71.66 MiB footprint / 116.64 MiB RSS |
| Collapsed test-process snapshot | 72.03 MiB footprint / 117.02 MiB RSS |

These are account-free CI observations, not live click-to-audio or a matched previous-build memory comparison. The expanded/collapsed snapshots are from the UI test process, not the separate idle app. **Native reference release passed, but an immediate process-memory decrease on collapse was not observed.** No sustained sub-100 MB or 50 MB listening-session claim is established.

## Reused measured audio result

[Probe 37806011920](https://github.com/Xon333/ytfast-macos/actions/runs/37806011920) used the same macOS 15 runner, generated 48 kHz stereo 256 kbps Opus, unchanged cache bounds and a queued successor, with null audio output. Nine short and two three-minute trials passed. The exact built-in mpv `libmpv` profile was tried before adapting through existing script-disable options; this implementation remains unchanged.

| Helper-only fixture | Baseline | Applied policy |
| --- | ---: | ---: |
| Short playback footprint, median of 3 | 21.9M | 17.3M |
| Three-minute footprint, one trial each | 25.7M | 20.0M |
| Three-minute RSS | 135.09 MiB | 142.23 MiB |
| Short load to playback-restart, median of 3 | 22.00 ms | 3.99 ms |

`M` preserves vmmap output. RSS did not consistently improve. These are not acoustic onset, real YouTube latency or total application memory. [Raw probe data](https://github.com/Xon333/ytfast-macos/actions/runs/37806011920/artifacts/11562374079). The preceding callback-pool/metadata experiment did not improve memory and remains discarded; the new shell page release exists specifically to satisfy collapsed-state ownership, not to resurrect that experiment.

## Real-Mac limits and predecessor

The earlier native report measured app footprint 34.0M idle / 43.0M during playback-state observation, plus 116.7M for its mpv child. These are previous-build, separate-process readings. The later screenshots show 107.5–133 MB without memory-column headings; their metric is not inferred.

No matched before/after same-Mac memory comparison, acoustic onset/dropout measurement, real IME-composition acceptance or long-term guarantee is established. Removal of the OS AutoFill helper is not established. No real account was accessed or modified for CI. Raw account reports/captures stay outside Git/CI. Distribution remains ad-hoc signed and not notarized.

[PR #5](https://github.com/Xon333/ytfast-macos/pull/5) retains the earlier native report and dismissal evidence. [Predecessor CI](https://github.com/Xon333/ytfast-macos/actions/runs/37808615571) and [0.4.0 package](https://github.com/Xon333/ytfast-macos/actions/runs/37808615571/artifacts/11564426394) preserve source `71c982f`, package revision `0a56c40`, and its measured mpv correction.
