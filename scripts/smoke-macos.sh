#!/bin/bash
# Isolated native-menu smoke and idle-memory sample. Never a real browser account.
set -euo pipefail
[[ "$(uname -s)" == Darwin ]] || exit 2
[[ "${GITHUB_ACTIONS:-}" == true ]] || { echo 'Use the CI runner for this isolated smoke test.' >&2; exit 2; }
repo="$(cd "$(dirname "$0")/.." && pwd)"
binary="$repo/dist/YTfast.app/Contents/MacOS/ytfast"
real_home="$HOME"
home="$(mktemp -d)"
pid=''
cleanup() {
  result=$?
  if [[ "$result" != 0 ]]; then
    for log in stdout stderr; do
      [[ ! -f "$home/$log" ]] || cp "$home/$log" "$repo/artifacts/native/startup-$log.txt"
    done
    # The CI runner has no real account. Capture only this app's crash report.
    sleep 3
    for report in "$real_home/Library/Logs/DiagnosticReports"/ytfast*.ips "$home/Library/Logs/DiagnosticReports"/ytfast*.ips; do
      [[ ! -f "$report" ]] || cp "$report" "$repo/artifacts/native/"
    done
  fi
  [[ -z "$pid" ]] || kill "$pid" 2>/dev/null || true
  rm -rf "$home"
}
trap cleanup EXIT
mkdir -p "$repo/artifacts/native"
YTFAST_UI_CAPTURE_DIR="$repo/artifacts/native" "$binary" --self-test > "$repo/artifacts/native/menu-tests.json"
export HOME="$home" PATH=/usr/bin:/bin:/usr/sbin:/sbin
export YTFAST_PROFILE_FILE="$repo/artifacts/native/idle-memory.json"
# Show starts the app when none exists; do not invoke it as a presence probe.
if [[ -S "/tmp/ytfast-$(id -u)/ytfast.sock" ]]; then
  echo 'An instance socket already exists; refusing to disturb it.' >&2; exit 2
fi
"$binary" > "$home/stdout" 2> "$home/stderr" &
pid=$!
for _ in {1..50}; do
  kill -0 "$pid" 2>/dev/null || { cat "$home/stderr"; exit 1; }
  [[ -s "$YTFAST_PROFILE_FILE" ]] && break
  sleep 0.5
done
[[ -s "$YTFAST_PROFILE_FILE" ]] || { cat "$home/stderr"; echo 'Memory sample absent' >&2; exit 1; }
ps -p "$pid" -o pid=,rss=,%cpu=,comm= > "$repo/artifacts/native/idle-process.txt"
# The signed-out, empty-session idle app must not launch mpv/yt-dlp/deno.
if pgrep -P "$pid" -x 'mpv|yt-dlp|deno' >/dev/null; then
  echo 'Unexpected idle playback/resolver child' >&2; exit 1
fi
"$binary" show
# Let AppKit enter menu tracking, instead of coalescing Show and Quit in one tick.
sleep 1
"$binary" quit
for _ in {1..40}; do
  kill -0 "$pid" 2>/dev/null || break
  sleep 0.25
done
if kill -0 "$pid" 2>/dev/null; then echo 'Quit did not finish' >&2; exit 1; fi
wait "$pid"
pid=''
python3 - "$repo/artifacts/native" <<'PY'
import json,sys
from pathlib import Path
p=Path(sys.argv[1]); data=json.loads(p.joinpath('idle-memory.json').read_text())
assert data['task_info_ok']
assert data['rss_bytes']>0 and data['physical_footprint_bytes']>0
print(json.dumps(data,indent=2))
print('Native menu: launch, Show, Quit, no idle audio children: PASS')
PY
