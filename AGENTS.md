# YTfast

Lean native YouTube Music menu-bar player. Native T3 owns workflow, delegation
and selected settings. Current user direction and source take precedence over
older designs. [MACOS](docs/MACOS.md) owns architecture/bounds;
[CURRENT](docs/CURRENT.md) records verified state and limits.

## Product contracts

- AppKit, macOS media controls and CLI share one Rust playback engine and one
  YTfast status item. The OS owns its Now Playing item. Keep the Mac package
  renderer-free and dependencies proportionate; no duplicate player or polling
  helper. Build the Mac core with `--no-default-features --features menubar`.
- Keep controls usable during refresh, cancel obsolete work and bound caches,
  library views and audio buffers. The current native interface fetches no
  artwork, Home or Lyrics. [MACOS resource bounds](docs/MACOS.md#resource-bounds)
  owns exact limits; architecture may change when the requested outcome warrants it.
- Account selection scopes caches and async replies/writes. Failure must not
  silently select another account. Never log/upload browser cookies, Safe
  Storage secrets or real-account captures; keep cookie exports private and
  short-lived. Do not restart or alter the browser to obtain authentication.
- Reuse an existing implementation or dependency feature when it fits. Research
  alternatives only for a concrete gap; routine edits need no ritual of trying
  and rejecting two sources. Record source/revision/license for adopted code.
  Sonora is architectural evidence; do not copy GPL source into this MIT project.
- Keep `Cargo.lock` and compatible target pins. Avoid unrelated upgrades;
  retained optional desktop code remains buildable, not a second product mandate.

## Verification

Use checks for the changed risk and the existing release gates:

```sh
cargo fmt --all --check
cargo clippy --locked --no-default-features --features menubar --lib -- -D warnings
cargo test --locked --release --no-default-features --features menubar --lib
```

`scripts/build-macos.sh` and `scripts/smoke-macos.sh` own native packaging and
smoke checks. The existing CI owns optional desktop coverage. Compilation and
fixtures do not prove browser/Keychain access, audible transitions, real playlist
mutations or live memory usage. Reuse valid evidence; prose-only edits need
meaning/link checks, no account access or service start.

Keep installation/use in `README.md`, current architecture in `docs/MACOS.md`,
and observed results in `docs/CURRENT.md`. Update the owning document instead
of appending exception paragraphs or creating another progress report.
