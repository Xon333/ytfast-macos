# macOS port

This fork starts from MayberryDT/ytfast `8c10cc2c9afe47d4e35922b566f1af8d4431bb49`.
The port is additive: the upstream player, API client, queues, account operations,
lyrics, equalizer, animations and `app.rs` state machine are retained. This file
supersedes the macOS exclusion in the upstream SPEC and its Linux-specific paths.

## Build and launch

Requires macOS 13+, Xcode Command Line Tools, Rust 1.98+ and Homebrew.
Install the command-line tools with `xcode-select --install` when absent.

```sh
brew install cmake mpv yt-dlp deno
git clone https://github.com/Xon333/ytfast-macos.git
cd ytfast-macos
scripts/build-macos.sh
open dist/YTfast.app
```

The builder makes a native `dist/YTfast.app` and an architecture-labelled ZIP.
It defaults to two compiler jobs; change `CARGO_BUILD_JOBS` explicitly to override.
An Apple Silicon Mac builds ARM64; an Intel Mac builds x86_64. CI builds the
Apple Silicon package. `CARGO_BUILD_TARGET` can select another installed Mac target.

This is a locally/ad-hoc-signed app, not a notarized distribution. It uses the
installed `mpv`, `yt-dlp` and `deno`; these are not inside the ZIP. Finder launches
get both standard Homebrew prefixes without loading or editing shell profiles.
Missing runtime tools produce an actionable alert rather than a silent launch.
The bundle is called **YTfast** to avoid colliding with Apple's Music.app.

## Sign in

Sign in to YouTube Music in Google Chrome, Brave or Chromium, then launch YTfast.
Allow the browser's Safe Storage item when macOS Keychain asks. Settings →
Reconnect reads the current browser session again. No headers or cookie exports
need to be supplied manually. Safari and Firefox sessions are not supported.

The browser roots are under `~/Library/Application Support`:
`Google/Chrome`, `BraveSoftware/Brave-Browser`, and `Chromium`.
Both `Network/Cookies` and legacy `Cookies` profiles are discovered. The browser
is never restarted. SQLite is opened read-only and read in a transaction, including
its WAL, instead of copying a changing database and journal separately.

Profile listing never unlocks Keychain. Only loading the chosen/default session
requests its key. A missing selected profile or denied key does not silently
switch accounts; select an available profile in Settings, or Reconnect after
allowing access. The account-only API request, not cookie presence, decides
whether the UI says signed in.

macOS uses `security find-generic-password -w -s '<Browser> Safe Storage'`, privately
captured, and PBKDF2-SHA1 with `saltysalt`, 1003 rounds, a 16-byte AES key and an IV
of 16 spaces. Only v10 is accepted on macOS. Schema 24+ must have the matching
host SHA-256 prefix. Linux keeps its separate v10/v11 derivation. No macOS
fallback to the Linux `peanuts` password exists.

Source references checked for this port:
- [yt-dlp's Mac cookie decryption and browser mappings](https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/cookies.py).
- [fastframe-tray v0.2.2](https://github.com/crmne/fastframe/blob/v0.2.2/crates/fastframe-tray/README.md): main-thread attach, menu events, Dock reopen and headless AppKit pump.
- [fastframe-shell v0.2.2](https://github.com/crmne/fastframe/blob/v0.2.2/crates/fastframe-shell/README.md): resident state across window close/recreation.

## Desktop behaviour

Closing the window with a queue keeps the session alive; closing with no queue
quits, as upstream does. Click the menu-bar note or Dock icon to reopen. Right-click
the note for Play/Pause, Next, Previous, Show and Quit. The item stays available
while the main window is open too. No new playback implementation is involved.

Command+Q quits, Command+W closes, Command+M minimizes. Use Control+M or the existing
mini-player button to switch to the always-on-top mini player. Command+K and
Command+, use the existing command/search and Settings handlers. The upstream
shortcut overlay still labels those shared chords as Ctrl. CLI commands work via
`dist/YTfast.app/Contents/MacOS/ytfast show|toggle|next|previous|quit`.

The existing neutral **dark** palette is the macOS default. Automatic system
light/dark following, macOS Control Center/Now Playing and global hardware media
keys, native song notifications, URL-scheme registration and a bespoke native
application menu are outside this first port. The notification setting is disabled
rather than falsely reporting support. Linux MPRIS, notifications, tray and Omarchy
remain platform-gated and available on Linux.

## Files and privacy

| Data | macOS location |
| --- | --- |
| Settings and theme directory | `~/Library/Application Support/ytfast` |
| Pages, covers, session and logs | `~/Library/Caches/ytfast` |
| Runtime IPC and temporary session exports | `/tmp/ytfast-<uid>` |

The short runtime path avoids macOS Unix-socket path limits. Directories must be
real directories owned by this user and are restricted to 0700; exported cookies
and atomic writes are 0600. Schema/cookie failures contain no cookie or key values.
Normal exit removes session exports; startup removes leftovers from an interrupted
run. Persistent settings and library cache are not removed. An OS-held instance
lock protects concurrent launches and is released on a crash.

## Verification

The `Native checks` workflow is the build/test evidence for each commit; a source
change or green compilation is not proof of live playback. It checks formatting,
strict Clippy including the E2E feature, Rust tests, CLI startup and Mac packaging.
Synthetic tests cover independent cookie/KDF vectors, wrong-host/corrupted data,
domain boundaries, strict profile selection, private exports and filesystem paths.
No CI job accesses a real browser account.

On the target Mac, the remaining interactive acceptance pass is: launch from
Finder; allow/deny Keychain access; confirm the selected account; play a queue across
three track changes and seek; close/reopen via the menu bar and Dock; switch the
mini player; quit and confirm playback stops. Use an ordinary listening session;
no destructive playlist/like test is required. Keep account screenshots/logs under
the ignored `artifacts/` directory, never in a commit.
