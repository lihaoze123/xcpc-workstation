#!/usr/bin/env python3
"""Summarize an NSH `--trace` log (`nsh run --trace`, see scripts/nsh-perf.sh).

NSH runs the app's JS and its core (component render, text layout, diff) on one
engine thread. With `--trace` it logs, per input event, the handler's JS time
("event js") and then the commit that applies what the handler changed
("commit": ops, µs, list js, mount items). This script pairs them up so a long
interaction splits into "our handler" vs. "NSH commit" (and the list-binding JS
inside the commit), and lists the most expensive engine turns overall.

Usage:
  python3 scripts/nsh-perf-summary.py <nsh-trace.log> [--top N] [--min-ms 8]
"""

import argparse
import re
import sys
from dataclasses import dataclass, field

EVENT_RE = re.compile(r"event (\w+) on view (\d+)")
EVENT_JS_RE = re.compile(r"event js: (\d+)µs")
COMMIT_RE = re.compile(r"commit: (\d+) ops, (\d+)µs \(list js (\d+)µs\), (\d+) mount items, (\d+) nodes")
TURN_RE = re.compile(r"turn: (\d+)µs \(timers and tasks (\d+)µs\)")

# Pointer noise that never starts an interaction on its own.
PASSIVE = {"MouseMove", "MouseEnter", "MouseLeave", "MouseUp", "Touch", "Load", "PressDown", "Focus", "Blur"}


@dataclass
class Interaction:
    line: int
    kind: str
    view: int
    handler_us: int = 0
    commit_us: int = 0
    list_js_us: int = 0
    ops: int = 0
    items: int = 0
    nodes: int = 0
    commits: int = 0

    @property
    def total_ms(self) -> float:
        return (self.handler_us + self.commit_us) / 1000.0


@dataclass
class Turn:
    line: int
    total_us: int
    tasks_us: int
    commit: tuple[int, int, int, int] = field(default=(0, 0, 0, 0))  # ops, us, list js, items


def main() -> int:
    parser = argparse.ArgumentParser(description="Summarize an NSH --trace log")
    parser.add_argument("log", help="Output of `nsh run --trace`")
    parser.add_argument("--top", type=int, default=15, help="How many rows to show (default 15)")
    parser.add_argument("--min-ms", type=float, default=8.3, help="Hide rows cheaper than this (default 8.3)")
    args = parser.parse_args()

    interactions: list[Interaction] = []
    turns: list[Turn] = []
    current: Interaction | None = None
    last_commit: tuple[int, int, int, int] | None = None

    with open(args.log, encoding="utf-8", errors="replace") as fh:
        for lineno, line in enumerate(fh, 1):
            if m := EVENT_RE.search(line):
                kind = m.group(1)
                # A new active event closes the previous interaction.
                if kind not in PASSIVE:
                    current = Interaction(lineno, kind, int(m.group(2)))
                    interactions.append(current)
                elif current is not None and current.commits > 0:
                    current = None
                continue
            if m := EVENT_JS_RE.search(line):
                if current is not None and current.commits == 0:
                    current.handler_us += int(m.group(1))
                continue
            if m := COMMIT_RE.search(line):
                ops, us, list_js, items, nodes = (int(g) for g in m.groups())
                last_commit = (ops, us, list_js, items)
                if current is not None and current.handler_us > 0 and current.commits == 0:
                    current.commit_us += us
                    current.list_js_us += list_js
                    current.ops += ops
                    current.items += items
                    current.nodes = nodes
                    current.commits += 1
                continue
            if m := TURN_RE.search(line):
                turns.append(Turn(lineno, int(m.group(1)), int(m.group(2)), last_commit or (0, 0, 0, 0)))
                last_commit = None

    worst = sorted((i for i in interactions if i.total_ms >= args.min_ms), key=lambda i: -i.total_ms)
    print(f"[Interactions >= {args.min_ms}ms]  ({len(interactions)} input events in log)")
    print(f"  {'line':>6} {'event':<10} {'total':>8} {'handler':>8} {'commit':>8} {'listjs':>7} {'ops':>5} {'items':>6} {'nodes':>6}")
    for i in worst[: args.top]:
        print(
            f"  {i.line:>6} {i.kind:<10} {i.total_ms:7.1f}ms {i.handler_us / 1000:7.1f}ms "
            f"{i.commit_us / 1000:7.1f}ms {i.list_js_us / 1000:6.1f}ms {i.ops:>5} {i.items:>6} {i.nodes:>6}"
        )

    slow_turns = sorted((t for t in turns if t.total_us / 1000 >= args.min_ms), key=lambda t: -t.total_us)
    print(f"\n[Engine turns >= {args.min_ms}ms]  (timers, IPC results, animations; {len(turns)} turns in log)")
    print(f"  {'line':>6} {'total':>8} {'tasks':>8} {'commit':>8} {'listjs':>7} {'ops':>5} {'items':>6}")
    for t in slow_turns[: args.top]:
        ops, us, list_js, items = t.commit
        print(
            f"  {t.line:>6} {t.total_us / 1000:7.1f}ms {t.tasks_us / 1000:7.1f}ms "
            f"{us / 1000:7.1f}ms {list_js / 1000:6.1f}ms {ops:>5} {items:>6}"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
