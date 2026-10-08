#!/bin/bash
# Same-runner comparison using exact baseline/candidate Swift implementation.
# Only the test entry point/API are substituted. No real account or browser.
set -euo pipefail
[[ "$(uname -s)" == Darwin && "${GITHUB_ACTIONS:-}" == true ]] || exit 2
repo="$(cd "$(dirname "$0")/.." && pwd)"
base=ca3400f1983e307bd893ce326b7d41612253b5c4
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$repo"
mkdir -p artifacts/native/memory-comparison "$work/base" "$work/candidate"
git fetch --no-tags --depth=1 origin "$base"
git archive "$base" native | tar -x -C "$work/base"
cp -R native "$work/candidate/"
python3 - "$work" "$repo/scripts/native-memory-fixture.swift" <<'PY'
from pathlib import Path
import sys
work=Path(sys.argv[1]); harness=Path(sys.argv[2]).read_text()
for kind in ('base', 'candidate'):
    main=work/kind/'native/main.swift'
    source=main.read_text()
    boundary='if CommandLine.arguments.contains("--self-test") {'
    assert source.count(boundary)==1
    main.write_text(source.split(boundary)[0]+harness)
PY
target="$(rustc -vV | sed -n 's/^host: //p')"
arch="$(uname -m)"
lib="${CARGO_TARGET_DIR:-$repo/target}/$target/release/libytfast.a"
# Reuse the production build flags and already-built static library. TestAPI
# never invokes the library; both variants have the identical linked core.
for kind in base candidate; do
  swiftc -swift-version 5 -O -whole-module-optimization \
    -target "$arch-apple-macosx13.0" -import-objc-header "$work/$kind/native/Bridge.h" \
    "$work/$kind/native/"*.swift "$lib" -o "$work/$kind/fixture" \
    -framework AppKit -framework MediaPlayer -framework Security \
    -framework SystemConfiguration -framework CoreFoundation -lc++ -lresolv -liconv \
    -Xlinker -dead_strip
done
for trial in 1 2 3; do
  order='base candidate'; [[ "$trial" != 2 ]] || order='candidate base'
  for kind in $order; do
    "$work/$kind/fixture" "$repo/artifacts/native/memory-comparison/$kind-$trial.json"
  done
done
python3 - "$repo/artifacts/native/memory-comparison" <<'PY'
import json,sys
from pathlib import Path
p=Path(sys.argv[1])
for name in sorted(p.glob('*.json')):
    r=json.loads(name.read_text())
    print(name.name, [(s['phase'], round(s['physical_footprint_bytes']/1048576,2), round(s['rss_bytes']/1048576,2)) for s in r['samples']])
PY
