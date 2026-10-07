#!/bin/bash
# Fresh-runner native menu smoke test. No real account is accessed.
set -euo pipefail
[[ "$(uname -s)" == Darwin && "${GITHUB_ACTIONS:-}" == true ]] || exit 2
repo="$(cd "$(dirname "$0")/.." && pwd)"
binary="$repo/dist/YTfast.app/Contents/MacOS/ytfast"
mkdir -p "$repo/artifacts/native"
"$binary" --self-test > "$repo/artifacts/native/menu-tests.json"
[[ ! -S "/tmp/ytfast-$(id -u)/ytfast.sock" ]] || { echo 'Another instance is active.' >&2; exit 1; }
home="$(mktemp -d)"
pid=''
cleanup() {
  [[ -z "$pid" ]] || kill "$pid" 2>/dev/null || true
  rm -rf "$home"
}
trap cleanup EXIT
export HOME="$home"
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
export YTFAST_PROFILE_FILE="$repo/artifacts/native/idle-memory.json"
"$binary" > "$home/stdout" 2> "$home/stderr" &
pid=$!
for _ in {1..45}; do
  [[ -f "$YTFAST_PROFILE_FILE" ]] && break
  kill -0 "$pid" 2>/dev/null || { cat "$home/stderr"; exit 1; }
  sleep 1
done
[[ -f "$YTFAST_PROFILE_FILE" ]] || { cat "$home/stderr"; echo 'No native main-loop sample.'; exit 1; }
ps -p "$pid" -o pid=,rss=,%cpu=,comm= > "$repo/artifacts/native/idle-process.txt"
# A restored, paused native session does not spawn playback/resolution helpers.
if pgrep -P "$pid" 'mpv|yt-dlp|deno' > /dev/null; then echo 'Unexpected idle playback helper'; exit 1; fi
"$binary" show
sleep 1
"$binary" quit
for _ in {1..20}; do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
if kill -0 "$pid" 2>/dev/null; then echo 'Quit did not stop the app'; exit 1; fi
wait "$pid"
pid=''
printf 'PASS: native menu, synthetic actions, isolated startup, no idle playback helpers, CLI Show/Quit.\n'
