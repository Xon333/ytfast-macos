# Current state

**Updated:** 8 October 2026 · **Build:** 0.4.0, measured audio-memory reduction

[Implementation](https://github.com/Xon333/ytfast-macos/pull/6) · [Architecture and bounds](MACOS.md)

## Applied result

The native player invokes mpv's **actual built-in `libmpv` profile**, then disables its unused console, overlays, script loaders and adjacent-file discovery. No playback engine, UI framework or runtime package is added. mpv **0.41+** is required. The existing 4 MiB forward / 1 MiB back buffers, 60-second read-ahead, gapless playback, queue prefetch, source selection and account isolation are retained.

`AGENTS.md` now makes exact implementation reuse → adaptation of that source → rebuild only after both are unsuitable mandatory for **all repository work**. Native Swift, theme colours, search, navigation, dismissal and account setup are unchanged from main `ca3400f`.

## Measured outcome, not a memory-target claim

[Same-runner probe 37806011920](https://github.com/Xon333/ytfast-macos/actions/runs/37806011920) passed all nine short trials and both three-minute trials on macOS 15 Apple Silicon. Baseline and candidate used the same generated 48 kHz stereo 256 kbps Opus file, forced cache, queued successor and **null audio output**. Physical footprint preserves `vmmap`'s rounded `M` output; RSS is separately converted from KiB to MiB.

| Paired workload | Baseline | Applied policy |
| --- | ---: | ---: |
| Short-playback footprint, median of 3 trials | 21.9M | 17.3M |
| Three-minute playback footprint, one trial each | 25.7M | 20.0M |
| Three-minute playback RSS, same trials | 135.09 MiB | 142.23 MiB |
| Short load to playback-restart, median of 3 trials | 22.00 ms | 3.99 ms |

The three-minute footprint fell by 5.7M; **RSS did not consistently decrease**. These are audio-helper measurements, not total YTfast memory, real output-device cost, YouTube latency or a guarantee of 50–100 MB on the user's Mac. [Raw data, options and vmmap samples](https://github.com/Xon333/ytfast-macos/actions/runs/37806011920/artifacts/11562374079) retain all trials and the unchanged decoded audio parameters.

A separate [production-Swift experiment](https://github.com/Xon333/ytfast-macos/actions/runs/37806897479) tested callback autorelease pools, shell snapshot removal and metadata deduplication. Three paired runs used 8,000 synthetic catalogue rows, 10,000 accelerated callbacks and 20 immediate popover cycles. Final median footprint was **42.86 → 44.35 MiB**, not an improvement. **Those UI changes and their extra harness were discarded**; the final Swift files match `ca3400f` byte-for-byte. The experiment does not establish a live-app leak or regression.

## Exact verified package

[Final macOS/Ubuntu CI](https://github.com/Xon333/ytfast-macos/actions/runs/37808615571) passed for source **`71c982fa134949fbf687e1f819b080818856d616`**. The subsequent owning-document update changes no tested production code.

[App and CI evidence](https://github.com/Xon333/ytfast-macos/actions/runs/37808615571/artifacts/11564426394) contains `dist/ytfast-macos-arm64.zip`, version **0.4.0**, packaged revision **`0a56c40c9649388b7efa9dc49ece8235bb64097a`**. Its tree equals the tested source tree; it is a PR build revision, not the later merge commit.

| Identity | SHA-256 |
| --- | --- |
| Actions archive | `9d94e9f242a33dfa1c450a71d14c38d08cc7e5d9e3c81b455c0cd2b62504eebf` |
| Installable ZIP | `29cc0cdc07ea52e5b66de573e42cf4b49c3bc172bbba73eb80ecf2081219dac8` |
| Executable | `b47f3a17784815704f53d5d9c2616ce8dfbb4ffaa185c2659dea82dd70591886` |

Archive integrity, executable mode, both revision fields and bundled license notices were checked. AppKit typechecking, formatting/Clippy, 44 core release tests per platform, 31 retained desktop tests, 53 native fixture checks, focused dismissal checks, actual mpv IPC/options/queue progression, the offline format contract, renderer-free dependencies, packaging/signature and isolated launch/Show/Quit/no idle audio children passed. The final run verifies mpv's disabled facilities and unchanged gapless/buffer options explicitly.

## Reused acceptance and limits

[PR #5](https://github.com/Xon333/ytfast-macos/pull/5) retains the preceding dark UI, dismissal correction and bounded native report. That report measured app footprint **34.0M idle / 43.0M during playback-state observation**, with **116.7M for its mpv child**. Those are prior-build, separate-process observations, not results for this build and not the screenshot's unidentified columns.

No current real-Mac same-metric comparison, acoustic onset/dropout measurement or long-term memory guarantee is established. The screenshot alone does not identify its memory columns. AutoFill was not killed or disabled speculatively, and account data, permissions and saved settings were not changed. The replacement-theme reference was not supplied; no new palette was selected. Distribution remains ad-hoc signed and not notarized. Raw account reports/captures remain outside Git and CI.
