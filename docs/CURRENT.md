# Current state

**Updated:** 8 October 2026 · **Candidate:** 0.5.0, compact OLED player

[Current pass](https://github.com/Xon333/ytfast-macos/pull/7) · [Architecture](MACOS.md) · [Measured audio correction](https://github.com/Xon333/ytfast-macos/pull/6)

## Candidate — verification pending

Branch `feat/compact-oled-20261008` includes the measured audio implementation from `71c982f` and reconciles the subsequent main documentation. The memory branch is preserved.

The player opens compact, with current song/collection and Search, Playlists, Liked and Albums launchers. Only the selected destination expands; closing resets expansion. Play/Pause paints a true circle. Shuffle On/Off is global, preserves the current song, and changes collection Play into Shuffle; explicit song selections remain exact. Audio-format metadata moves to More.

The OLED root is black. Exact non-blue Oxocarbon literals supply neutral surfaces, pink accents and green active shuffle; existing Mino controls and native navigation/queue code are reused. Licenses are bundled; no runtime framework or package is added. The repo-wide reuse-first rule remains in AGENTS.

Only the expanded page crosses the Rust/Swift bridge; closing releases native page/row copies while the bounded Rust cache remains. Queue fetching starts before old-player cleanup, redundant Stop is avoided, and a cold successor resolver waits for current load acceptance. Audio quality, account isolation and buffers remain unchanged. The non-improving callback autorelease experiment from PR #6 is not reintroduced. The app-shell catalogue copy is released here specifically so the new collapsed-state contract actually drops the last native page owner.

Native compilation, revised interaction fixtures and captures for this candidate are pending. The package below is the accepted predecessor, not this candidate.

## Reused measured audio result

[Probe 37806011920](https://github.com/Xon333/ytfast-macos/actions/runs/37806011920): same macOS 15 runner, generated 48 kHz stereo 256 kbps Opus, unchanged cache bounds and queued successor, null audio output. Nine short trials and two three-minute trials passed. The exact built-in `libmpv` profile was tried before adapting through existing script-disable options; that implementation is retained unchanged here.

| Helper-only fixture | Baseline | Applied policy |
| --- | ---: | ---: |
| Short playback footprint, median of 3 | 21.9M | 17.3M |
| Three-minute footprint, one trial each | 25.7M | 20.0M |
| Three-minute RSS | 135.09 MiB | 142.23 MiB |
| Short load to playback-restart, median of 3 | 22.00 ms | 3.99 ms |

`M` preserves vmmap output. RSS did not consistently improve. These are not acoustic onset, real YouTube latency, total application memory or a guarantee of 50–100 MB on the user's Mac. [Raw probe data](https://github.com/Xon333/ytfast-macos/actions/runs/37806011920/artifacts/11562374079).

The preceding [Swift experiment](https://github.com/Xon333/ytfast-macos/actions/runs/37806897479) measured final median footprint 42.86 → 44.35 MiB, not a gain; its callback-pool/metadata experiment and extra harness were discarded. New compact-view measurements must be identified separately.

## Accepted predecessor

[CI 37808615571](https://github.com/Xon333/ytfast-macos/actions/runs/37808615571) passed for source `71c982fa134949fbf687e1f819b080818856d616`. [Preceding 0.4.0 package](https://github.com/Xon333/ytfast-macos/actions/runs/37808615571/artifacts/11564426394), embedded revision `0a56c40c9649388b7efa9dc49ece8235bb64097a`, contains the same tested tree. Its installable ZIP SHA-256 is `29cc0cdc07ea52e5b66de573e42cf4b49c3bc172bbba73eb80ecf2081219dac8`.

It passed 44 core tests per platform, 31 retained desktop tests, 53 native fixtures, focused dismissal tests, mpv IPC/options/queue progression, offline format contract, renderer-free graph, packaging/signature and isolated smoke. [PR #5](https://github.com/Xon333/ytfast-macos/pull/5) preserves earlier native validation and dismissal evidence.

## Real-Mac limits

Earlier validation measured app footprint 34.0M idle / 43.0M during playback-state observation, plus 116.7M for its mpv child. These are previous-build, separate-process readings. The current screenshots show 107.5–133 MB without memory-column headings; that metric is not inferred.

No current same-Mac memory comparison, acoustic click-to-audio measurement or long-term target guarantee is established. Search assistance is scoped to the app's text controls; removal of the OS AutoFill helper is not established. Account data, permissions and saved preferences are not modified by setup. Raw account reports/captures stay outside Git/CI. Distribution remains ad-hoc signed and not notarized.
