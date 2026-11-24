#!/usr/bin/env python3
"""Analyze a Chrome Trace / Perfetto JSON exported by XCPC Workstation.

Reports both rendering pipeline timings (Rust Core) and UI/UX metrics
(frame fluidity, jank rate, worst frame, INP interaction latency).

Usage:
  python3 scripts/trace-analyze.py path/to/xcpc-trace.json
  python3 scripts/trace-analyze.py --gate path/to/xcpc-trace.json
"""

import argparse
import json
import sys
from collections import defaultdict


def percentile(values: list[float], p: float) -> float:
    if not values:
        return 0.0
    s = sorted(values)
    idx = min(len(s) - 1, max(0, round((len(s) - 1) * p)))
    return s[idx]


def main() -> int:
    parser = argparse.ArgumentParser(description="Analyze XCPC Workstation Chrome Trace JSON")
    parser.add_argument("trace_file", help="Path to exported Chrome Trace JSON")
    parser.add_argument("--gate", action="store_true", help="Fail if any UI/UX or render budget is exceeded")
    parser.add_argument("--max-jank-60", type=float, default=2.0, help="Max 60Hz jank rate %% (default: 2.0)")
    parser.add_argument("--max-inp-p95", type=float, default=16.6, help="Max INP P95 ms (default: 16.6)")
    parser.add_argument("--max-compile-p95", type=float, default=120.0, help="Max compile P95 ms (default: 120.0)")
    args = parser.parse_args()

    with open(args.trace_file, "r", encoding="utf-8") as fh:
        data = json.load(fh)

    events = data.get("traceEvents", [])
    by_key: dict[str, list[float]] = defaultdict(list)
    frame_dts: list[float] = []
    inp_ms: list[float] = []

    for ev in events:
        if ev.get("ph") != "X":
            continue
        dur_ms = float(ev.get("dur", 0.0)) / 1000.0
        cat = str(ev.get("cat", ""))
        name = str(ev.get("name", ""))
        if cat == "frame":
            frame_dts.append(dur_ms)
        elif cat == "ux.inp":
            inp_ms.append(dur_ms)
        else:
            by_key[f"{cat}.{name}"].append(dur_ms)

    failed = False
    meta = data.get("metadata", {})
    print(f"Trace: {args.trace_file}")
    print(
        f"Events: rust={meta.get('rustEvents', '?')}  js={meta.get('jsEvents', '?')}  "
        f"dropped={meta.get('droppedEvents', 0)}"
    )

    if frame_dts:
        total = len(frame_dts)
        jank60 = sum(1 for d in frame_dts if d > 16.67) * 100.0 / total
        jank120 = sum(1 for d in frame_dts if d > 8.33) * 100.0 / total
        p50 = percentile(frame_dts, 0.50)
        p95 = percentile(frame_dts, 0.95)
        p99 = percentile(frame_dts, 0.99)
        worst = max(frame_dts)
        print("\n[UI Fluidity / Frame Stability]")
        print(
            f"  frames={total}  p50={p50:.2f}ms  p95={p95:.2f}ms  p99={p99:.2f}ms  "
            f"worst={worst:.2f}ms  jank60={jank60:.2f}%  jank120={jank120:.2f}%"
        )
        if args.gate and jank60 > args.max_jank_60:
            print(f"  FAIL: jank60 {jank60:.2f}% > {args.max_jank_60:.2f}%")
            failed = True

    if inp_ms:
        p50 = percentile(inp_ms, 0.50)
        p95 = percentile(inp_ms, 0.95)
        p99 = percentile(inp_ms, 0.99)
        print("\n[UI Responsiveness / INP]")
        print(f"  samples={len(inp_ms)}  p50={p50:.2f}ms  p95={p95:.2f}ms  p99={p99:.2f}ms")
        if args.gate and p95 > args.max_inp_p95:
            print(f"  FAIL: INP p95 {p95:.2f}ms > {args.max_inp_p95:.2f}ms")
            failed = True

    if by_key:
        print("\n[Pipeline Spans]")
        for key in sorted(by_key):
            vals = by_key[key]
            p50 = percentile(vals, 0.50)
            p95 = percentile(vals, 0.95)
            mx = max(vals)
            print(f"  {key:<30s}  n={len(vals):<5d}  p50={p50:6.2f}ms  p95={p95:6.2f}ms  max={mx:6.2f}ms")
            if args.gate and key == "engine.compile" and p95 > args.max_compile_p95:
                print(f"  FAIL: {key} p95 {p95:.2f}ms > {args.max_compile_p95:.2f}ms")
                failed = True

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
