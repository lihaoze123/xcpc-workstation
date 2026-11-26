#!/usr/bin/env python3
"""Analyze a Chrome Trace / Perfetto JSON exported by XCPC Workstation.

Reports UI/UX metrics (missed refreshes, 60Hz jank, stall time, per-interaction
INP) and rendering pipeline timings (Rust Core spans).

INP spans (`ux.inp.*`) run from the input to the start of the frame after the
update was applied, so they include NSH's commit and layout. Frame events only
exist for frames that missed a refresh; totals come from `ui.frame_summary`.
To see *what* a long interaction spent its time on (event JS vs. NSH commit),
record the same steps with `scripts/nsh-perf.sh` and summarize them with
`scripts/nsh-perf-summary.py`.

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


def union_overlap_ms(window_start: float, window_end: float, intervals: list[tuple[float, float]]) -> float:
    clipped: list[tuple[float, float]] = []
    for s, e in intervals:
        if e <= window_start or s >= window_end:
            continue
        clipped.append((max(s, window_start), min(e, window_end)))
    if not clipped:
        return 0.0
    clipped.sort()
    total = 0.0
    cur_s, cur_e = clipped[0]
    for s, e in clipped[1:]:
        if s <= cur_e:
            if e > cur_e:
                cur_e = e
        else:
            total += cur_e - cur_s
            cur_s, cur_e = s, e
    total += cur_e - cur_s
    return total


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
    inp_by_name: dict[str, list[float]] = defaultdict(list)
    missed: list[float] = []
    long_frames: list[tuple[float, float]] = []
    work_intervals: list[tuple[float, float]] = []
    ipc_spans: list[tuple[float, str]] = []
    summary: dict = {}

    for ev in events:
        cat = str(ev.get("cat", ""))
        name = str(ev.get("name", ""))
        if cat == "frame.summary":
            summary = ev.get("args", {}) or {}
            continue
        if ev.get("ph") != "X":
            continue
        ts_ms = float(ev.get("ts", 0.0)) / 1000.0
        dur_ms = float(ev.get("dur", 0.0)) / 1000.0
        if cat == "frame":
            missed.append(dur_ms)
            if name == "ui.long_frame" and dur_ms > 16.6:
                long_frames.append((ts_ms, ts_ms + dur_ms))
        elif cat == "ux.inp":
            inp_by_name[name.removeprefix("ux.inp.")].append(dur_ms)
        else:
            by_key[f"{cat}.{name}"].append(dur_ms)
            if cat != "ux.content" and dur_ms > 0.0:
                work_intervals.append((ts_ms, ts_ms + dur_ms))
            if cat == "ipc":
                ipc_spans.append((dur_ms, name))

    failed = False
    meta = data.get("metadata", {})
    print(f"Trace: {args.trace_file}")
    print(
        f"Events: rust={meta.get('rustEvents', '?')}  js={meta.get('jsEvents', '?')}  "
        f"dropped={meta.get('droppedEvents', 0)}"
    )

    total = int(summary.get("totalFrames", 0))
    if total:
        vsync = float(summary.get("vsyncMs", 16.67))
        jank60 = int(summary.get("jank60", 0)) * 100.0 / total
        missed_rate = int(summary.get("missed", len(missed))) * 100.0 / total
        stall = float(summary.get("stallMs", 0.0))
        duration_s = total * vsync / 1000.0
        print("\n[UI Fluidity]")
        print(
            f"  frames={total}  vsync={vsync:.2f}ms  missed={missed_rate:.2f}%  jank60={jank60:.2f}%  "
            f"worst={float(summary.get('worstMs', 0)):.1f}ms  stall={stall:.0f}ms (~{stall / max(duration_s, 1e-9):.1f}ms/s)"
        )
        if missed:
            longest = sorted(missed, reverse=True)[:5]
            print("  longest frames: " + ", ".join(f"{d:.1f}ms" for d in longest))
        print("  (rates include idle frames while recording; prefer INP and stall time)")
        if args.gate and jank60 > args.max_jank_60:
            print(f"  FAIL: jank60 {jank60:.2f}% > {args.max_jank_60:.2f}%")
            failed = True

    if inp_by_name:
        all_inp = [v for vals in inp_by_name.values() for v in vals]
        p95_all = percentile(all_inp, 0.95)
        print("\n[Interaction to Next Paint]")
        print(f"  all  n={len(all_inp)}  p50={percentile(all_inp, 0.5):.1f}ms  p95={p95_all:.1f}ms  max={max(all_inp):.1f}ms")
        for name in sorted(inp_by_name, key=lambda n: -max(inp_by_name[n])):
            vals = inp_by_name[name]
            print(
                f"  {name:<28s} n={len(vals):<4d} p50={percentile(vals, 0.5):6.1f}ms  "
                f"p95={percentile(vals, 0.95):6.1f}ms  max={max(vals):6.1f}ms"
            )
        if args.gate and p95_all > args.max_inp_p95:
            print(f"  FAIL: INP p95 {p95_all:.1f}ms > {args.max_inp_p95:.1f}ms")
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

    if long_frames:
        total_long_ms = 0.0
        total_attributed_ms = 0.0
        unattributed_per_frame: list[tuple[float, float]] = []
        for s, e in long_frames:
            dur = e - s
            attr = union_overlap_ms(s, e, work_intervals)
            unattr = max(0.0, dur - attr)
            total_long_ms += dur
            total_attributed_ms += attr
            unattributed_per_frame.append((unattr, dur))
        total_unattributed_ms = max(0.0, total_long_ms - total_attributed_ms)
        unattr_pct = (total_unattributed_ms * 100.0 / total_long_ms) if total_long_ms > 0 else 0.0
        print("\n[Unattributed Stall Time (ui.long_frame > 16.6ms)]")
        print(
            f"  long_frames={len(long_frames)}  total={total_long_ms:.1f}ms  "
            f"attributed={total_attributed_ms:.1f}ms  unattributed={total_unattributed_ms:.1f}ms ({unattr_pct:.1f}%)"
        )
        worst_unattr = sorted(unattributed_per_frame, key=lambda x: -x[0])[:5]
        print(
            "  worst unattributed frames: "
            + ", ".join(f"{u:.1f}ms/{d:.1f}ms" for u, d in worst_unattr)
        )

    if ipc_spans:
        print("\n[Top-5 Slowest IPC Spans]")
        for rank, (dur_ms, name) in enumerate(sorted(ipc_spans, key=lambda x: -x[0])[:5], start=1):
            print(f"  {rank}. ipc.{name:<26s}  {dur_ms:7.2f}ms")

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
