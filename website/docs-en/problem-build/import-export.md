---
title: Package Import & Export
---

# Package Import & Export

XCPC Workstation natively imports standard **Codeforces Polygon** packages (single-problem or full-contest ZIP archives) into the local open-directory workspace, and exports verified problems or full contests as ready-to-upload minimal ZIP packages for **DOMjudge** and **Hydro OJ**.

## Importing Polygon Packages

### Single-Problem and Full-Contest Import

`xcpc import polygon` automatically detects `problem.xml` (single problem) or `contest.xml` (full contest) inside the archive, completes all validation and conversion in memory, and writes atomically to the workspace (leaving the workspace untouched on parse failure, with built-in Zip Slip path-traversal protection and archive size/entry-count limits):

```bash
# Import a single Polygon problem ZIP (defaults to problem.xml short-name as slug)
xcpc import polygon './a-plus-b-12$linux.zip'

# Explicitly specify target problem slug and group
xcpc import polygon './a-plus-b-12$linux.zip' --id my-a-plus-b --group "2026 Training"

# Import a full Polygon contest ZIP (creates all problems and contests/<id>/contest.toml)
xcpc import polygon ./regional-contest.zip

# Full-contest import creates and assigns a group named after the contest by default; override or disable it
xcpc import polygon ./regional-contest.zip --group "ICPC 2026 Regional"
xcpc import polygon ./regional-contest.zip --no-auto-group
```

| Flag | Description |
| :--- | :--- |
| `--id <ID>` | Override the imported problem slug (single problem) or contest ID (full contest) |
| `--group <NAME>` | Assign imported problems to a persistent group (auto-registered in `workspace.toml` if new) |
| `--no-auto-group` | Full-contest import only: disable automatic grouping by contest name |

### Restored Assets

The importer maps Polygon package assets into the workspace structure as faithfully as possible:

- **Statement conversion**: Parses `statement-sections/<lang>/` or `statements/<lang>/problem.tex` (preferring Chinese and English), converts LaTeX sections, math, lists, and tables into workspace Markdown (`statement.zh.md` / `statement.en.md`), and preserves original samples under `samples/`.
- **Metadata and limits**: Restores title, time/memory limits, built-in checker name or custom `checker.cpp`, `validator.cpp`, all generators, and solutions with their expected verdicts (mapping Polygon tags to `main = true` and `AC`, `WA`, `TLE`, `MLE`, `PE`, etc.).
- **Interactive, multi-run, and subtasks**: Automatically detects and restores `interactor.cpp`, `[package].runs` multi-run configuration, and Polygon test groups (`<groups>`) with dependencies into `[[subtask]]`.
- **Conflict protection**: If the target problem slug or contest ID already exists in the workspace, the importer rejects the operation without overwriting existing data.

## Exporting OJ Packages (DOMjudge / Hydro)

### Static/Dynamic Separation and Verification Hash Gating

`xcpc export` strictly enforces static/dynamic separation: **export never compiles or executes any C++ code**. Instead, it packages the already-built test data from `build/problems/<slug>/tests/` and compiles the statement PDF on the fly using the Typst engine.

By default, the exporter performs a static freshness check (comparing `package_hash` against `verified_report.json`):

- Export is allowed only when the problem is **`verified_fresh`** (latest strict verification passed and no package files have changed since).
- Exporting a full contest (`export <format> contest <id>`) pre-checks **every problem** in the contest; if any problem fails the check, the entire export aborts cleanly without leaving partial files on disk.
- Batch-exporting multiple problems (`export <format> problems <id1> <id2> ...`) skips unverified problems or those missing test data and reports them in the summary without interrupting the rest.
- To bypass strict verification gating (e.g., for temporary test uploads during development), pass `--force`. Even with `--force`, built test data must already exist in `build/problems/<slug>/tests/`.

### CLI Usage

Supported built-in formats are `domjudge` (alias `dj`) and `hydro` (alias `hydrooj`):

```bash
# Export a single problem as a DOMjudge ZIP (default output: build/<slug>-domjudge.zip)
xcpc export domjudge problem a-plus-b -o ./dist/a-plus-b-dj.zip

# Export a single problem as a Hydro ZIP
xcpc export hydro problem tree-dp -o ./dist/tree-dp-hydro.zip

# Export a full contest (outputs a directory containing <Label>-<slug>.zip files)
xcpc export domjudge contest icpc-2026 -o ./dist/icpc-2026-dj/

# Batch export multiple problems (defaults to build/batch-<format>-<YYYYMMDD-HHMMSS>/)
xcpc export hydro problems a-plus-b tree-dp guess-number

# Force export of an unverified problem (still requires built test data)
xcpc export domjudge problem a-plus-b --force
```

!!! tip "Custom Plugin Exporters"
    `xcpc export` and `xcpc import` also support custom formats provided by enabled plugins (`<plugin>:<exporter>` / `<plugin>:<importer>`). Pass `--with-pdf` to supply the compiled statement PDF as Base64 to a plugin exporter. See [Plugins & Extensions](../plugins/index.md).

## Exported ZIP Layout Reference

=== "DOMjudge (aligned with Polygon2DOMjudge)"

    ```text
    <problem>.zip
    ├── domjudge-problem.ini          # short-name, externalid, timelimit (LF endings, no color)
    ├── problem.yaml                  # name, limits.memory (MiB), validation, validator_flags
    ├── problem.pdf                   # Statement PDF compiled from the active Typst template
    ├── data/
    │   ├── sample/                   # Adaptive zero-padding (3 digits when >= 100 tests), lexical order
    │   │   ├── 01.in
    │   │   └── 01.ans
    │   └── secret/
    │       ├── 01.in
    │       └── 01.ans
    └── output_validators/            # Generated only for interactive problems or custom checker.cpp
        ├── interactor/               # or checker/
        │   ├── interactor.cpp        # (or checker.cpp)
        │   ├── testlib.h             # Effective testlib.h
        │   ├── build                 # 0755 executable build script
        │   └── run                   # 0755 executable run wrapper (maps testlib exit codes to 42/43)
    ```

    - **Built-in checker mapping**: `wcmp`/`ncmp`/`lcmp` → `default` (`case_sensitive`); `fcmp` → `space_change_sensitive case_sensitive`; `rcmp4`/`rcmp6`/`rcmp9` → `float_tolerance 1e-4/1e-6/1e-9`; `yesno`/`nyesno` → `default` (case-insensitive).
    - **Platform differences**: Because DOMjudge uses ICPC scoring, exporting a problem with `[[subtask]]` flattens all secret tests into `data/secret/` and emits a warning. Interactive problems package only `interactor.cpp` (without a separate checker).

=== "Hydro OJ"

    ```text
    <problem>.zip
    ├── problem.yaml                  # title and tag list
    ├── problem_zh.md                 # or problem_en.md, embedding @[pdf](file://problem.pdf?noDisposition=1)
    ├── additional_file/
    │   └── problem.pdf               # Statement PDF compiled from the active Typst template
    └── testdata/
        ├── config.yaml               # type, time, memory, checker/interactor, and subtasks config
        ├── checker.cpp               # Included for non-default built-in or custom checker.cpp
        ├── interactor.cpp            # Included for interactive problems (with testlib.h)
        ├── 01.in
        └── 01.out
    ```

    - **Full OI subtask preservation**: When `[[subtask]]` is declared, the exporter writes the complete `subtasks` list into `testdata/config.yaml`, including `score`, scoring type (`min`/`sum`), prerequisite indices (`if`), and test case mappings (`cases`).
    - **Built-in checker mapping**: `wcmp`/`ncmp`/`lcmp` map to `checker_type: default`; all other built-in checkers (`fcmp`/`rcmp*`/`yesno`/`nyesno`) and custom `checker.cpp` are packaged as `checker_type: testlib` with their corresponding source file.
