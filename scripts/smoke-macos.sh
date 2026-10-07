#!/bin/bash
# CI-only native window smoke test; no browser account or injected playback.
set -euo pipefail
[[ "$(uname -s)" == Darwin && "${GITHUB_ACTIONS:-}" == true ]] || {
  echo 'Run this smoke test only on a fresh macOS GitHub runner.' >&2; exit 2;
}
repo="$(cd "$(dirname "$0")/.." && pwd)"
binary="$repo/dist/YTfast.app/Contents/MacOS/ytfast"
[[ -x "$binary" ]] || { echo 'Build the app first.' >&2; exit 1; }
[[ ! -S "/tmp/ytfast-$(id -u)/ytfast.sock" ]] || {
  echo 'A ytfast socket already exists; refusing to touch another session.' >&2; exit 1;
}
test_home="$(mktemp -d)"
pid=''
cleanup() {
  if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
  fi
  rm -rf "$test_home"
}
trap cleanup EXIT
# The empty home contains no browser profiles. A stripped PATH exercises the
# same dependency lookup required for Finder, without sourcing shell profiles.
export HOME="$test_home"
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
"$binary" > "$test_home/stdout.log" 2> "$test_home/stderr.log" &
pid=$!
log="$test_home/Library/Caches/ytfast/ytfast.log"
for _ in {1..60}; do
  if [[ -f "$log" ]] && grep -q 'first frame' "$log"; then break; fi
  kill -0 "$pid" 2>/dev/null || {
    cat "$test_home/stderr.log" >&2; echo 'App exited before its first frame.' >&2; exit 1;
  }
  sleep 1
done
[[ -f "$log" ]] && grep -q 'first frame' "$log" || {
  cat "$test_home/stderr.log" >&2; echo 'No native frame appeared.' >&2; exit 1;
}
"$binary" show
"$binary" quit
for _ in {1..20}; do
  kill -0 "$pid" 2>/dev/null || break
  sleep 1
done
if kill -0 "$pid" 2>/dev/null; then
  echo 'The app did not stop after the CLI Quit request.' >&2; exit 1
fi
wait "$pid"
pid=''
printf 'PASS: native ARM64 window drew a frame with an empty home and Finder-style PATH; Show and Quit returned cleanly.\n'
