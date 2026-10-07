# YTfast contributor contract

## Current product

The macOS target is a **minimal native menu-bar YouTube Music player**.

Build it with:

```sh
--no-default-features --features menubar
```

The Mac package must remain renderer-free: no egui/eframe/winit/GPU UI stack, no WebView and no second playback engine.

The retained `desktop-ui` feature is the upstream/Linux desktop product and development reference. Do not let its requirements silently expand the Mac release.

## Read order

For ordinary macOS work:

1. [docs/MACOS.md](docs/MACOS.md) — current product/architecture contract.
2. [docs/CURRENT.md](docs/CURRENT.md) — current tested state, measurements and known limits.
3. Relevant source/tests.

Read [docs/SPEC.md](docs/SPEC.md) or [docs/integration.md](docs/integration.md) only for a specific inherited desktop/Linux requirement or technical fact. They are not the current Mac scope.

## Non-negotiable boundaries

### Secrets and account data

- Never log, print, commit or upload cookie values or browser Safe Storage secrets.
- Real-account screenshots, private API captures and logs do not enter Git/CI.
- Derived cookie files remain short-lived and private.
- Never restart or modify the user's browser to obtain auth.
- A failed selected profile/Keychain access must not silently switch accounts.

### One player

AppKit, macOS media controls and CLI must drive the same Rust player state.

Do not add:

- another playback engine;
- a WebView/browser player;
- a polling UI helper process;
- a second menu application.

The macOS global Now Playing item is OS-owned and separate by design.

### Resource direction

Preserve the menu build's lazy/bounded behavior:

- current + next track preparation;
- no menu artwork/Home/Lyrics loading;
- bounded stream/page caches;
- bounded native submenus;
- no render/repaint loop.

A feature that requires restoring the full renderer needs explicit product justification rather than being treated as a routine implementation detail.

### Platform separation

Keep Linux desktop behavior behind `desktop-ui`; keep Mac behavior behind `menubar` / native files.

Do not remove retained upstream functionality merely because it is not shipped in the Mac package.

## Dependency discipline

- Keep `Cargo.lock` committed.
- Do not upgrade unrelated dependencies during a focused fix.
- Keep the inherited fastframe/egui/winit pins aligned for the retained desktop build.
- Sonora is architecture reference only; do not copy GPL source into this MIT fork.

## Verification

Use the smallest meaningful affected checks.

### Documentation only

Check:

- current-state claims against code/CI evidence;
- relative links;
- no accidental change to historical evidence semantics.

Do not start services or access a real account just to validate prose.

### Renderer-free Rust/menu core

```sh
cargo fmt --all --check
cargo clippy --locked --no-default-features --features menubar --lib -- -D warnings
cargo test --locked --release --no-default-features --features menubar --lib
```

### Retained desktop UI

On Linux/appropriate CI:

```sh
cargo clippy --locked --all-targets --features e2e -- -D warnings
cargo test --locked --release --features e2e --lib
```

### Native Mac

The accepted CI additionally verifies:

- renderer-free dependency graph;
- production mpv IPC audio transport;
- `scripts/build-macos.sh`;
- `scripts/smoke-macos.sh`;
- AppKit self-tests, launch, Show/Quit and idle child-process behavior.

Do not claim real YouTube sign-in, playlist mutation, audible quality or live-session memory from compilation/synthetic tests alone.

## Documentation discipline

Keep current guidance small and layered:

- `README.md`: user/front door.
- `docs/MACOS.md`: current product + architecture.
- `docs/CURRENT.md`: dated tested state.
- `docs/SPEC.md` / `docs/integration.md`: inherited historical/reference material.

When a current rule changes, replace its owning statement. Do not accumulate another exception paragraph in multiple files.

## UI copy

Use short YouTube Music-familiar labels. The current Mac menu needs no desktop-style explanatory prose or visual hierarchy.

Current menu vocabulary includes:

- Previous
- Play / Pause
- Next
- Shuffle
- Volume
- Library
- Playlists
- Liked Music
- Albums
- Add song to playlist
- Account
- Reconnect
- Quit YTfast
