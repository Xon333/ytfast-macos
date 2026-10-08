# YTfast contributor guidance

## Product and authority

The macOS product is a lean native YouTube Music menu-bar player. Its current AppKit popover provides persistent playback controls, search and inline library browsing.

Build the Mac core with:

```sh
--no-default-features --features menubar
```

Current user direction and source/observed behavior take precedence over older design documents. Architecture, menu structure and inherited subsystems are implementation choices. Change or remove them when evidence supports a smaller, more coherent product; legacy desktop compatibility is not an independent product requirement.

Read [docs/MACOS.md](docs/MACOS.md), [docs/CURRENT.md](docs/CURRENT.md), then the relevant source/tests. Use [docs/SPEC.md](docs/SPEC.md) and [docs/integration.md](docs/integration.md) for specific historical facts.

## Boundaries

### Accounts and privacy

- Never log, print, commit or upload cookies or browser Safe Storage secrets.
- Keep real-account screenshots, private API captures and logs out of Git/CI.
- Keep derived cookie exports private and short-lived.
- Do not restart or modify the user's browser to obtain authentication.
- A selected profile failure must never fall back to another account.
- Scope caches and asynchronous operations to their account. Late replies and queued writes must not adopt a newly selected identity.
- Preserve unrelated user data.

### One player owner

AppKit, macOS media controls and CLI dispatch to the same Rust playback state. Keep one playback engine and one YTfast status item. The global macOS Now Playing item is OS-owned.

The Mac package currently excludes the desktop renderer and browser engines. Keep dependencies proportionate to the native product; do not add a polling helper or duplicate playback state.

### Responsiveness and bounds

Every common action must complete promptly or immediately show useful state. Keep existing rows usable during refresh, cancel obsolete work, and fix the dependency causing a delay.

Current preparation covers the current and next track. Library views, account-scoped caches and audio buffers have explicit bounds in [MACOS](docs/MACOS.md#resource-bounds). No artwork, Home or Lyrics fetches run for the native interface.

The optional desktop target stays separate from the Mac package. Keep remaining code buildable; remove obsolete code deliberately rather than adding compatibility layers.

## Reuse-first requirement — all repository work

For every task, first try exact source or an existing implementation unchanged;
then adapt that exact source to YTfast. Rebuild only when both options have been
investigated and cannot satisfy the task or user intent. This applies to UI,
backend, performance, tooling and documentation—not only visual inspiration.
Keep a concise source/revision/license attribution and the concrete reason for
any adaptation or rejection. Prefer the existing dependency's built-in feature
over adding another framework. Licensing, privacy and correctness still apply.

## Dependencies and licensing

- Keep `Cargo.lock` committed.
- Avoid unrelated dependency upgrades.
- Keep dependency pins consistent for targets that remain present.
- Sonora is evidence and architectural inspiration. Do not copy GPL source into this MIT project.

## Verification

Use focused checks for the changed risk and satisfy the release gates. Reuse valid evidence rather than repeating broad checks.

### Rust/menu core

```sh
cargo fmt --all --check
cargo clippy --locked --no-default-features --features menubar --lib -- -D warnings
cargo test --locked --release --no-default-features --features menubar --lib
```

The current workflow also checks the optional Linux desktop target:

```sh
cargo clippy --locked --all-targets --features e2e -- -D warnings
cargo test --locked --release --features e2e --lib
```

### Native Mac

Native CI checks AppKit typechecking, the renderer-free dependency graph, production mpv IPC transport, packaging/signature, native UI self-tests, isolated launch, Show/Quit and idle child processes. Build and smoke entry points are `scripts/build-macos.sh` and `scripts/smoke-macos.sh`.

Compilation and synthetic fixtures do not establish real browser/Keychain access, playlist mutations, audible transitions or live-session memory. Label observed, synthetic and user-reported evidence accurately.

### Documentation only

Check claims against source/evidence, relative links and historical evidence semantics. Do not start services or access a real account to verify prose.

## Documentation and UI copy

Keep owning documents small:

- `README.md`: installation and use.
- `docs/MACOS.md`: current product, architecture and bounds.
- `docs/CURRENT.md`: dated verified state and material limits.
- `docs/SPEC.md` / `docs/integration.md`: inherited reference material.

Replace stale statements in their owning document. Do not accumulate exception paragraphs, duplicate trackers or a new report.

Use short, familiar labels and native controls. Account status, loading, refresh and errors must remain readable and actionable.
