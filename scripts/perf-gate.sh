#!/usr/bin/env bash
# Automated performance benchmark and tightened budget gate.
# Runs `preview_bench` under `--profile bench-fast` for both single-set (copies=1)
# and long-contest (copies=5) workloads, checks tight P95/P50 budgets (~3-4x real
# baseline so regressions are caught immediately), and optionally writes a Chrome
# Trace JSON when `XCPC_TRACE=1 XCPC_TRACE_OUT=<path>` is set.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Tightened regression budgets (ms), calibrated to ~3-4x measured baseline.
BUDGET_SHORT_FIRST_MS="${BUDGET_SHORT_FIRST_MS:-150}"
BUDGET_SHORT_EDIT_P95_MS="${BUDGET_SHORT_EDIT_P95_MS:-25}"
BUDGET_LONG_EDIT_P95_MS="${BUDGET_LONG_EDIT_P95_MS:-50}"
BUDGET_RASTER_2X_P95_MS="${BUDGET_RASTER_2X_P95_MS:-10}"
BUDGET_FINGERPRINTS_P50_MS="${BUDGET_FINGERPRINTS_P50_MS:-3}"

echo "[perf-gate] Building preview_bench (--profile bench-fast)..."
cargo build --profile bench-fast -p xcpc-engine --example preview_bench

BIN="$ROOT/target/bench-fast/examples/preview_bench"

echo "[perf-gate] Running short workload (copies=1)..."
SHORT_JSON="$(XCPC_BENCH_COPIES=1 "$BIN" --json)"
echo "$SHORT_JSON"

echo "[perf-gate] Running long contest workload (copies=5)..."
LONG_JSON="$(XCPC_BENCH_COPIES=5 "$BIN" --json)"
echo "$LONG_JSON"

python3 - "$SHORT_JSON" "$LONG_JSON" \
  "$BUDGET_SHORT_FIRST_MS" \
  "$BUDGET_SHORT_EDIT_P95_MS" \
  "$BUDGET_LONG_EDIT_P95_MS" \
  "$BUDGET_RASTER_2X_P95_MS" \
  "$BUDGET_FINGERPRINTS_P50_MS" <<'PY'
import json
import sys

short_data = json.loads(sys.argv[1])
long_data = json.loads(sys.argv[2])
budget_first = float(sys.argv[3])
budget_short = float(sys.argv[4])
budget_long = float(sys.argv[5])
budget_raster = float(sys.argv[6])
budget_fp = float(sys.argv[7])

checks = [
    ("short.firstCompileMs", short_data["firstCompileMs"], budget_first),
    ("short.editEarly.compileP95Ms", short_data["edits"]["editEarly"]["compileP95Ms"], budget_short),
    ("short.editLate.compileP95Ms", short_data["edits"]["editLate"]["compileP95Ms"], budget_short),
    ("short.editEarly.fingerprintsP50Ms", short_data["edits"]["editEarly"]["fingerprintsP50Ms"], budget_fp),
    ("short.raster@2.0.p95Ms", short_data["scales"]["2.0"]["rasterP95Ms"], budget_raster),
    ("long(5x).editEarly.compileP95Ms", long_data["edits"]["editEarly"]["compileP95Ms"], budget_long),
    ("long(5x).editLate.compileP95Ms", long_data["edits"]["editLate"]["compileP95Ms"], budget_long),
]

failed = False
print("\n[perf-gate] Summary:")
for name, actual, limit in checks:
    status = "PASS" if actual <= limit else "FAIL"
    if status == "FAIL":
        failed = True
    print(f"  {status:4s}  {name:<36s}  {actual:6.2f} ms  (budget <= {limit:.1f} ms)")

if failed:
    sys.exit(1)
PY
