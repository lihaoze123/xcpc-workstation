---
title: Traditional Problems & Strict Verification
---

# Traditional Problems & Strict Verification

Traditional problems read input from `stdin`, write output to `stdout`, and compare contestant output against the reference answer using a checker. This page covers the directory convention, `problem.toml` declarations, built-in and custom checkers, validator self-tests, strict verification gates, bounded stress testing, ASan/UBSan checks, and external tester trial runs.

## Directory Layout and `problem.toml`

A complete traditional problem package lives under `problems/<slug>/`:

```text
problems/a-plus-b/
├── problem.toml              # Problem metadata, limits, [package], [[test]], [solutions.*]
├── statement.zh.md           # Problem statement
├── samples/                  # Automatically included as leading tests (tag = "sample")
│   ├── 01.in
│   └── 01.ans                # or 01.out
├── validator.cpp             # Input format validator (testlib.h)
├── checker.cpp               # Optional: custom checker (defaults to wcmp if omitted)
├── generators/
│   └── gen_rand.cpp          # Test data generator
├── solutions/
│   ├── std.cpp               # Main reference solution (main = true)
│   ├── alt_ac.cpp            # Independent AC solution (cross-check)
│   └── brute_tle.cpp         # Expected failing solution (test strength check)
└── tests/
    ├── hand-edge.in          # Hand-written edge case
    ├── validator/
    │   ├── valid/            # Positive validator self-tests (must pass)
    │   └── invalid/          # Negative validator self-tests (must reject)
    └── checker/              # Self-tests for custom checker.cpp (one subdir per case)
        ├── ok-01/            # Contains in, out, ans
        └── wa-01/            # Contains in, out, ans
```

Declare package configuration, test cases, and solution expectations in `problem.toml`:

```toml
title = "A + B Problem"

[limits]
time = "1s"
memory = "256MB"

[package]
checker = "ncmp"              # Built-in checker name, or custom path such as "checker.cpp"

# 1. Hand-written test case
[[test]]
file = "tests/hand-edge.in"
name = "min-bound"
tag = "edge"

# 2. Generator test batch ({i} expands to 1..=repeat)
[[test]]
gen = "gen_rand"
args = "--n 100000 --seed {i}"
repeat = 5
tag = "max"

# 3. Generated test written back into samples/
[[test]]
gen = "gen_rand"
args = "--n 5 --seed 1"
sample = true

# Solution declarations (undeclared solutions/*.cpp default to expect = "AC")
[solutions.std]
main = true

[solutions.alt_ac]
expect = "AC"

[solutions.brute_tle]
expect = ["TLE", "WA"]        # Single string or array; "ANY" is also supported
tags = ["max"]                # Optional: run only on tests matching these tags
```

!!! note "Test Expansion Order and Automatic Sample Inclusion"
    Non-interactive problems automatically include `samples/*.in` (sorted by filename) as leading test cases with `tag = "sample"`, followed by `[[test]]` declarations in the order they appear in `problem.toml`. If `repeat > 1` and `args` does not contain `{i}`, the engine automatically appends the index `1..=repeat` to the argument list.

## Built-in Checkers and Custom `checker.cpp`

When `[package].checker` is omitted: if `checker.cpp` (or `checker.{17,20,23}.cpp`) exists in the problem directory, it is used automatically; otherwise the default is `wcmp`.

XCPC Workstation embeds 9 standard `testlib.h` checkers that can be referenced by name in `[package].checker` without copying source files:

| Checker Name | Comparison Semantics | Typical Use Case |
| :--- | :--- | :--- |
| `wcmp` | Strict token-by-token (whitespace-delimited string) comparison (default) | Most string and mixed output problems |
| `ncmp` | 64-bit signed integer token comparison | Pure integer output problems |
| `fcmp` | Exact line-by-line comparison (sensitive to spaces and line endings) | Strict formatting or ASCII art output |
| `lcmp` | Line-by-line comparison, matching tokens within each line (ignores extra spaces) | Multi-token output structured by line |
| `rcmp4` | Floating-point relative or absolute error $\le 10^{-4}$ | 4-digit real number output |
| `rcmp6` | Floating-point relative or absolute error $\le 10^{-6}$ | 6-digit real number output |
| `rcmp9` | Floating-point relative or absolute error $\le 10^{-9}$ | 9-digit real number output |
| `yesno` | Single `YES` / `NO` verdict (case-insensitive) | Single-case decision problems |
| `nyesno` | Multiple `YES` / `NO` verdicts (case-insensitive) | Multi-test decision problems |

For problems requiring a custom Special Judge, implement `checker.cpp` using the standard `registerTestlibCmd(argc, argv)` interface, and add self-test subdirectories under `tests/checker/` (each containing `in`, `out`, and `ans`):

- Prefix `ok-*`: Expected checker verdict `AC` (exit code `0`)
- Prefix `wa-*`: Expected checker verdict `WA` (exit code `1` or `8`)
- Prefix `pe-*`: Expected checker verdict `PE` (exit code `2`)

Strict verification requires a custom `checker.cpp` to have at least **1 `ok-*` and 1 `wa-*`** self-test, all passing.

## Validator (`validator.cpp`) and Self-Tests

`validator.cpp` uses `registerValidation(argc, argv)` to verify that every test input (samples, hand-written tests, and generated tests) strictly conforms to the statement constraints and formatting rules (no stray whitespace, no invalid characters, all values within bounds).

To guard against bugs in the validator itself (such as missed upper bounds or rejecting valid inputs), maintain bidirectional self-tests under `tests/validator/`:

- `tests/validator/valid/`: Valid inputs that the validator must accept (exit code `0`).
- `tests/validator/invalid/`: Invalid inputs (out of range, missing lines, illegal characters, etc.) that the validator must reject (non-zero exit code).

Strict verification requires `validator.cpp` to exist, every test case to pass validation, and both `valid/` and `invalid/` to contain at least 1 passing self-test.

## Strict Verification Gates

When running `xcpc package build <id> --strict`, the engine evaluates the following gates in sequence:

| Gate ID | Check | Debug Mode | Strict Verification (`--strict`) |
| :--- | :--- | :--- | :--- |
| `compile` | All tools and solutions compile cleanly | Blocking | Blocking |
| `validator` | `validator.cpp` exists with ≥ 1 `valid` and ≥ 1 `invalid` self-test, all passing | Missing/insufficient: warning; failed self-test: blocking | **Blocking** |
| `checker_self_tests` | Custom `checker.cpp` has ≥ 1 `ok` and ≥ 1 `wa` self-test, all passing | Insufficient: warning; failed self-test: blocking | **Blocking** |
| `subtasks` | OI `[[subtask]]` DAG is acyclic, non-empty, and covers all non-sample tests | Errors blocking; total score ≠ 100 is warning | Errors blocking; total score ≠ 100 is warning |
| `dirty_inputs` | Hand-written inputs have no CRLF, UTF-8 BOM, or missing trailing newline | Warning | Warning |
| `validator_tests` | All test cases pass `validator.cpp` | Blocking | **Blocking** |
| `solutions_expect` | All solution verdicts, scores, and subtask assertions match `problem.toml` | Blocking | **Blocking** |
| `std_time_margin` | Main solution slowest test time $\le 50\%$ of time limit | Warning | **Blocking** |
| `solution_diversity` | At least 1 independent AC solution and 1 expected non-AC solution | Warning | Warning |

Once all blocking gates pass, the engine writes the verification report to `build/problems/<slug>/verified_report.json` and marks the problem freshness as `verified_fresh`.

## Bounded Stress Testing (`xcpc package stress`)

Use the built-in bounded stress tester to find counterexamples for wrong solutions or cross-check brute-force and optimal solutions:

```bash
xcpc package stress a-plus-b \
  --gen gen_rand \
  --args "--n 20 --seed {i}" \
  --solutions brute_wa,alt_ac \
  --max-iter 200 \
  --timeout 30
```

- `--gen <name>`: Generator file `generators/<name>.cpp`.
- `--args <template>`: Generator argument template; `{i}` or `{seed}` is replaced with the current iteration index.
- `--solutions <names>`: Target solutions to compare against main `std` (comma-separated or repeated; defaults to all non-main solutions if omitted).
- `--max-iter <N>` (default `100`) and `--timeout <secs>` (default `30`): Dual bounds to prevent unbounded runs.

As soon as a target solution disagrees with the main solution on a generated input, the engine stops, atomically saves the first counterexample to `tests/stress-<n>.in`, and prints the reproducible generator command.

## ASan / UBSan Undefined Behavior Checks (`--sanitize`)

Undefined behavior in C++—such as out-of-bounds array access, null pointer dereference, or signed integer overflow—can silently pass under `-O2`. Run an isolated sanitizer check with `--sanitize`:

```bash
# Full sanitizer check (generators, validator, custom checker, interactor, and all expected-AC solutions)
xcpc package build a-plus-b --sanitize

# Restrict to specific test cases and solutions
xcpc package build a-plus-b --sanitize --tests 1,2,max --solutions std,alt_ac
```

Isolation and limit-relaxation behavior in sanitizer mode:

- **Compiler flags**: `-O1 -std=c++<ver> -fsanitize=address,undefined -fno-omit-frame-pointer -g -fno-sanitize-recover=undefined`.
- **Cache and report isolation**: Executables are cached in `~/.cache/xcpc/sanitize/bin/` and reports are written to `build/problems/<slug>/sanitizer-report.json`. Normal `report.json`, `verified_report.json`, and `samples/` are **never** overwritten.
- **Relaxed limits**: Time limit is relaxed to $\max(5 \times \text{TL}, 5000\text{ms})$, and virtual memory (`RLIMIT_AS`) and RSS MLE enforcement are disabled to avoid false positives from ASan shadow memory and instrumentation overhead.
- **Structured diagnostics**: Automatically extracts `AddressSanitizer` / `UndefinedBehaviorSanitizer` summaries and key stack frames from stderr.

## External Tester Trial Runs and Adoption

During cross-testing, problem setters frequently need to test external `.cpp` submissions from testers without modifying the workspace's `solutions/` directory or `problem.toml`.

### Zero-Pollution Trial Runs (`verify-external`)

```bash
# Compile and judge external solutions in a temporary directory without modifying workspace files
xcpc package verify-external a-plus-b /tmp/tester_alice.cpp /tmp/tester_bob.cpp

# Automatically run a Debug build first if test data has not been generated yet
xcpc package verify-external a-plus-b /tmp/tester_alice.cpp --auto-build

# Export results as a concise Markdown verification report
xcpc package verify-external a-plus-b /tmp/tester_alice.cpp --markdown /tmp/report.md
```

Each submission report includes overall verdict, passed/total test count, maximum time and peak memory, runtime ratio relative to the main solution (`ratio_vs_std`), and details of the first failing test case. In the desktop app, you can also select an entire contest tester folder (flat `A.cpp` / `<slug>.cpp` or nested `<dir>/<tester_name>/<prob>.cpp`) via **Try Tester Code…** for batch trial runs.

### Explicit Atomic Adoption (`adopt`)

Once an external submission is worth keeping as an official reference or wrong solution, use `adopt` to copy it atomically into `problems/<slug>/solutions/` and register it in `problem.toml`:

```bash
xcpc package adopt a-plus-b /tmp/tester_alice.cpp \
  --name alice_ac \
  --expect AC

# Adopt as an expected failing solution restricted to a specific test tag
xcpc package adopt a-plus-b /tmp/tester_greedy.cpp \
  --name greedy_wa \
  --expect WA,TLE \
  --tag max
```

Pass `--main` if the adopted file should be registered as the main solution (`main = true`).
