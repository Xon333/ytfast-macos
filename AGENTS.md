# ytfast agent guide

ytfast is a native YouTube Music client: Rust + egui on [fastframe](https://github.com/crmne/fastframe), modelled on ZapFast and Spotifast. It has no browser engine, no telemetry and no server of its own. Shown in the window as "Music"; the Mac bundle is "YTfast".

## Start here

1. [docs/MACOS.md](docs/MACOS.md): this fork's macOS scope, build, platform boundaries and verification limits. It overrides the original SPEC's macOS exclusion and Linux-only path/desktop assumptions. macOS work is explicitly authorized; Windows and feature rewrites are not.
2. [docs/SPEC.md](docs/SPEC.md): the inherited product journeys. Retain the player/UI instead of redefining the product from the code.
3. [docs/integration.md](docs/integration.md): upstream's dated Linux/YouTube integration facts. Read it before touching sign-in, playback or the build; do not present its Linux observations as Mac validation.
4. If a `notes/` directory exists, it is private and gitignored. Read `notes/AGENTS.md` before using it.

## Rules

- Cookie values and browser keys are secrets. Never log, print or commit them. Derived cookie files are 0600 and short-lived; private directories are 0700. Never restart or modify the browser for sign-in.
- Nothing from a real account goes into the repository: no captured responses, screenshots or logs. E2E artifacts stay in ignored `artifacts/`; app packages stay in ignored `dist/`.
- Reuse upstream player/API/UI code. Platform integrations must be conditional. Retain Linux functionality. Reuse fastframe's native tray and headless loop rather than a WebView, polling subprocess UI or a separate player.
- Colours come from the shared palette. Linux follows Omarchy; macOS currently uses the existing neutral dark fallback. No scattered UI colour constants or claims of automatic system appearance support.
- Do not vendor or patch upstream crates here. Keep all fastframe tags and the egui/winit fork pins aligned; do not upgrade unrelated dependencies during a port fix.
- Prefer real-app E2E for runtime claims. For platform boundaries, write synthetic known-answer/regression tests before changing the code. Never use a real account to seed tests or run destructive account tests automatically.
- `cargo fmt --all --check`, `cargo clippy --locked --all-targets --features e2e -- -D warnings` and `cargo test --locked --features e2e` must pass on both platforms. Limit compiler parallelism to two jobs by default. Do not claim a Mac runtime or playback result from source inspection or compilation alone.
- Keep the resolved Cargo.lock committed. Normal CI checks are read-only and must not silently rewrite source or dependency versions.

## UI copy

Use YouTube Music's familiar labels: Home, Explore, Library, Up next, Lyrics, Related, Quick picks and Listen again. Plain sentence case; say what happened and the next action, without exclamations. Put technical detail behind Copy or in Settings. Never report signed in before an account request succeeds, and never silently switch a selected account after a Keychain/session failure.
