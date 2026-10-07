#!/bin/bash
# Compare the archived original port with the new native app on the same clean runner.
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true && "$(uname -s)" == Darwin ]] || exit 2
old="$1"
repo="$(cd "$(dirname "$0")/.." && pwd)"
home="$(mktemp -d)"
pid=''
trap '[[ -z "$pid" ]] || kill "$pid" 2>/dev/null || true; rm -rf "$home"' EXIT
HOME="$home" PATH=/usr/bin:/bin:/usr/sbin:/sbin "$old" > "$home/out" 2> "$home/err" &
pid=$!
sleep 15
kill -0 "$pid"
ps -p "$pid" -o pid=,rss=,%cpu=,comm= > "$repo/artifacts/native/baseline-idle-process.txt"
HOME="$home" "$old" quit
wait "$pid"
pid=''
python3 - "$repo/artifacts/native" <<'PY'
import json,sys
from pathlib import Path
p=Path(sys.argv[1])
old=p.joinpath('baseline-idle-process.txt').read_text().split()
new=p.joinpath('idle-process.txt').read_text().split()
a,b=int(old[1])*1024,int(new[1])*1024
out={'comparison':'same macOS runner, isolated empty homes, signed out, 15s settled; old visible window vs new menu only','baseline_commit':'837ad95184f279d0dcf722735f53fe72596a1f1a','baseline_app_rss_bytes':a,'native_app_rss_bytes':b,'rss_reduction_percent':round(100*(a-b)/a,1),'excludes':'shared OS helpers and temporary subprocesses; not the user live 421 MB session'}
p.joinpath('comparison.json').write_text(json.dumps(out,indent=2)+'\n')
print(json.dumps(out,indent=2))
PY
