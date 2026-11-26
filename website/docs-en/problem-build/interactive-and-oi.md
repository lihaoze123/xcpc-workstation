---
title: Interactive, Multi-Run & OI Subtasks
---

# Interactive, Multi-Run & OI Subtasks

Beyond traditional batch problems, XCPC Workstation natively supports stdio interactive problems, multi-run communication problems (`runs = N`), and OI subtask scoring with dependency DAGs and partial scores. All of these run directly in the local native process runner out of the box.

## stdio Interactive Problems

### Automatic Detection and Interactor Contract

When `interactor.cpp` (or `interactor.{17,20,23}.cpp`) is present in the problem directory, the build engine automatically treats the problem as **interactive**:

- The interactor initializes via `testlib.h`'s `registerInteraction(argc, argv)` and is launched by the engine as `interactor <input_path> <tout_path>`.
- Interactive problems do **not** automatically add `samples/*.in` to the test case list (since statement samples typically illustrate an example dialogue rather than hidden input); all test cases must be declared explicitly via `[[test]]`.
- **Single-stage judging (default)**: The final verdict (`_ok`, `_wa`, `_pe`, or `quitp` partial score) is determined directly from `interactor.cpp`'s exit code and stderr.
- **Two-stage judging**: If `[package].checker` is explicitly configured or `checker.cpp` exists in the problem directory, after the interactor finishes, the engine passes the `<tout_path>` written by the interactor to the checker (`checker <in> <tout> <ans>`) for secondary verification.

```bash
# Generate an interactive problem scaffold (interactor.cpp and interactive solutions/std.cpp)
xcpc package init guess-number --interactive
```

### Dual-Pipe Proxy, ILE Deadlock Detection, and Transcript

Instead of connecting the interactor and solution processes directly via raw OS pipes, the engine places **two managed proxy pipes** between them (`Interactor → Solution` and `Solution → Interactor`):

1. **Accurate separation of TLE and ILE**:
   - The runner independently tracks the solution process's CPU time and the last I/O activity timestamp across both pipes.
   - The idle timeout threshold is set to $\min(\text{TL} + 1500\text{ms}, \text{wall\_limit})$, avoiding false timeouts during legitimate heavy computation while immediately catching deadlocks (such as forgetting `fflush(stdout)` / `std::endl`), terminating both processes with **`ILE` (Idleness Limit Exceeded)**.
   - Exceeding the CPU time limit is accurately reported as **`TLE`**.
2. **Bidirectional Interaction Transcript**:
   - The proxy threads record exchanged lines in real time with direction (`→` / `←`) and run index, using a bounded head/tail window (keeping up to 30 leading and 30 trailing lines, capped at 512 characters per line) while tracking omitted line counts and total bytes in each direction so heavy interactions never exhaust memory.
   - On `WA`, `ILE`, or `RE`, you can inspect the **Interaction Transcript** directly in the desktop **Diagnostics** panel or CLI JSON report to pinpoint protocol mismatches or deadlocks.

## Multi-Run Communication Problems (`runs = N`)

For encoder/decoder problems or communication tasks requiring multiple independent invocations of the same executable, declare `runs` ($\ge 2$) in `problem.toml`:

```toml
[package]
runs = 2
```

You can also scaffold one directly:

```bash
xcpc package init encode-decode --interactive --runs 2
```

Execution and accounting rules for multi-run problems:

- **Pass chaining**:
  - Interactive mode (`interactor.cpp` present): `<tout_path>` written by the interactor in run $k$ automatically becomes `<input_path>` for the interactor in run $k+1$. A fresh solution process is spawned for each run, preventing cross-run memory leaks.
  - Non-interactive mode (no `interactor.cpp`): `stdout` from run $k$ is redirected into `stdin` of run $k+1$.
- **Cumulative CPU time and per-run memory limit**:
  - CPU time accumulates across all runs. Each run starts with a remaining budget of `time_limit_ms - total_cpu_ms`; once cumulative CPU time exceeds `time_limit_ms`, the engine immediately returns `TLE` and aborts subsequent runs.
  - Memory limit is enforced independently per run, and the report records the peak RSS across all runs.
- **Short-circuiting and score aggregation**: Any run returning a non-`AC` verdict immediately aborts remaining runs and identifies the failing pass in diagnostics (e.g., `Run 2` of 2). When all runs pass, the overall `score_ratio` is the minimum across all runs.

!!! warning "External OJ Export Note"
    Neither DOMjudge nor Hydro natively supports Polygon-style multi-run communication (`runs >= 2`). Exporting such problems emits a structured warning; manual adaptation to the target OJ's rules is required.

## OI Subtasks and Partial Scoring

### `[[subtask]]` Declarations and Dependency DAG

Declare subtasks with `[[subtask]]` in `problem.toml` and assign each non-sample `[[test]]` to a single subtask via `subtask = "<id>"`:

```toml
[[subtask]]
id = "1"
score = 20
scoring = "min"               # Bundled all-or-nothing scoring (default)

[[subtask]]
id = "2"
score = 30
depends = ["1"]               # Prerequisite: must pass subtask 1 first
scoring = "min"

[[subtask]]
id = "3"
score = 50
depends = ["2"]
scoring = "sum"               # Equal-share per-test scoring

[[test]]
gen = "gen_tree"
args = "--n 100 --seed {i}"
repeat = 3
subtask = "1"

[[test]]
gen = "gen_tree"
args = "--n 5000 --seed {i}"
repeat = 4
subtask = "2"

[[test]]
gen = "gen_tree"
args = "--n 100000 --seed {i}"
repeat = 5
subtask = "3"
```

Before running tests, the build engine statically validates the subtask graph (`subtasks` gate):

- Every subtask `id` must be non-empty and unique; all `depends` references must point to declared subtasks without forming cycles (enforced as a DAG).
- Every declared subtask must contain at least 1 test case (empty subtasks are rejected).
- When `[[subtask]]` is declared, every non-sample test case must belong to a valid subtask (single-assignment rule; test reuse is expressed via `depends`).
- If the sum of subtask scores is not `100`, a non-blocking warning is emitted.

### Scoring Policies and `quitp` Partial Scores

A checker or interactor can award per-test partial credit using `testlib.h`'s `quitp(ratio, ...)` (exit code `7`, stderr format `points <ratio> [message]`, with `ratio` clamped to $[0.0, 1.0]$; `ratio > 0` is recorded as `AC` with its ratio preserved, while `ratio == 0` is recorded as `WA`).

The two subtask scoring policies work as follows:

| Policy (`scoring`) | Semantics | Dependency Requirement | Subtask Score Formula |
| :--- | :--- | :--- | :--- |
| `min` (default) | Bundled | All tests in this subtask and all transitive dependencies must pass | $\text{score} \times \min(\text{ratio across self and transitive dependencies})$; `0` if any test is non-`AC` or `ratio == 0` |
| `sum` | Per-test average | All transitive dependencies must achieve full score (`AC` with `ratio == 1.0`) | $\frac{\text{score}}{k} \sum_{i=1}^{k} \text{ratio}_i$ ($k$ = tests in this subtask) if dependencies have full score; otherwise `0` |

### Subtask Constraint Validation via `validator --group`

When a test belongs to `subtask = "<id>"`, the engine automatically passes `--group <id>` to the validator (compatible with `testlib.h`'s `validator.group()`):

```cpp
#include "testlib.h"
#include <string>

int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    std::string grp = validator.group();
    int max_n = (grp == "1") ? 100 : (grp == "2" ? 5000 : 100000);
    int n = inf.readInt(1, max_n, "n");
    inf.readEoln();
    inf.readEof();
}
```

### Solution Score and Per-Subtask Assertions

For OI problems, in addition to overall `expect`, each solution can declare an expected total score (exact value or closed range `[min, max]`) and per-subtask verdict expectations:

```toml
[solutions.std]
main = true                    # Main solution must achieve full score on all subtasks

[solutions.brute_n2]
score = 50                     # Exact total score assertion
subtasks = { "1" = "AC", "2" = "AC", "3" = "TLE" }

[solutions.greedy_partial]
score = [20, 40]               # Closed-interval total score assertion
subtasks = { "1" = "AC", "2" = ["WA", "TLE"] }
```

!!! tip "Default `expect` Behavior with Subtask Assertions"
    For non-main solutions, whenever `score` or `subtasks` is configured and `expect` is omitted, the engine automatically defaults the overall `expect` to `"ANY"` so partial-score solutions do not conflict with an implicit `"AC"` expectation.
