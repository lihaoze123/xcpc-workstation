#!/usr/bin/env bash
# Runs the desktop app with NSH's per-commit trace and summarizes it on exit.
#
#   scripts/nsh-perf.sh [--workspace DIR] [--out LOG] [-- extra `nsh run` args]
#
# Reproduce the interaction you care about, close the app, and read which input
# events were slow and whether the time went to our handler or NSH's commit
# (component render, text layout, diff). Combine with an exported XCPC trace
# (`scripts/trace-analyze.py`) for INP and the Rust pipeline.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${TMPDIR:-/tmp}/xcpc-nsh-perf.log"
WORKSPACE="${XCPC_WORKSPACE:-}"
EXTRA=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workspace) WORKSPACE="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --) shift; EXTRA=("$@"); break ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

cd "$ROOT/apps/desktop"
echo "[nsh-perf] logging to $OUT (close the app to see the summary)"
XCPC_WORKSPACE="$WORKSPACE" nsh run --trace "${EXTRA[@]}" 2>&1 | tee "$OUT" | grep -v "nsh::perf\|^perf " || true
python3 "$ROOT/scripts/nsh-perf-summary.py" "$OUT"
