---
title: CLI Reference
---

# CLI Reference

`xcpc` is the headless command-line interface for XCPC Workstation. It shares the same Rust Core as the desktop application and `xcpc mcp`. Every workflow — problem library management, Typst typesetting, Problem Authoring & Export, Algorithm Snippets verification, Team Reference Document (TRD) composition, plugin management, and training analytics — can be executed in a terminal or CI pipeline.

---

## Global Options & Exit Codes

### Global Options

The following flags may be placed before or after any subcommand:

| Option | Short | Description |
| :--- | :--- | :--- |
| `--workspace <DIR>` | `-w <DIR>` | Specify the workspace root directory. When omitted, reads `XCPC_WORKSPACE` if set, otherwise walks upward from the current directory to locate a workspace |
| `--json` | — | Print a single machine-readable JSON object to `stdout` for scripting and automation |
| `--read-only` | — | Open the workspace in read-only mode, rejecting all write operations (also applies to `xcpc mcp`) |
| `--system-fonts` | — | Load operating-system fonts in addition to `XCPC_FONT_DIRS` and embedded fonts |

### Exit Codes

| Exit Code | Meaning | Typical Causes |
| :---: | :--- | :--- |
| `0` | Success | Command completed normally; all compilation, validation, and gate checks passed |
| `1` | Document, check, or judge failure | Typst compile error, `xcpc-lint` error, `package build` / `snippet verify` verification failure, freeze gate blocked, etc. |
| `2` | Argument, workspace, I/O, or permission error | Invalid arguments, workspace not found, missing target file, `--read-only` write rejection, frozen contest/TRD, conflict, or unconfirmed destructive action |

In `--json` mode, fatal errors that exit with code `2` emit a uniform JSON error object on `stdout`:

```json
{
  "ok": false,
  "error": {
    "code": "read_only",
    "message": "the workspace is open read-only"
  }
}
```

---

## Subcommand Reference (by Domain)

### 1. Workspace & Environment

| Command | Purpose & Key Options |
| :--- | :--- |
| `xcpc init [dir]` | Initialize a new workspace (creates `workspace.toml`, `problems/`, `contests/`, `.gitignore`, and `AGENTS.md` without overwriting an existing `AGENTS.md`). Supports `--example` (populates example problems and contests; requires an empty directory), `--lang <chinese\|zh\|english\|en>` (default `chinese`), and `--with-skills` (installs the built-in authoring Skill to `.agents/skills/xcpc-problem-creator/`) |
| `xcpc list` | List all problems and contests in the current workspace (including archived markers) |
| `xcpc show <problem\|contest> <id>` | Inspect single-problem metadata, content hash, and sample summary, or contest page ranges and difficulty/tag statistics |
| `xcpc fonts` | List all font families and weight counts currently visible to the typesetting engine |

### 2. Library Search, Groups & Problem Management

| Command | Purpose & Key Options |
| :--- | :--- |
| `xcpc search [QUERY...]` | Search the problem library. Supports `--tag <TAG>` (repeatable, AND semantics), `--rating-min <N>`, `--rating-max <N>`, `--platform <NAME>`, `--group <NAME>`, and `--ungrouped` |
| `xcpc group list` | List persistent groups declared in `workspace.toml` and the ungrouped problem count |
| `xcpc group create <name>` | Create a new group (1–64 characters) |
| `xcpc group rename <old> <new>` | Rename a group and update `problem.toml` across all member problems |
| `xcpc group delete <name>` | Delete a group (member problems become ungrouped; no problem files are deleted) |
| `xcpc group reorder <GROUP...>` | Reorder groups according to the given complete permutation |
| `xcpc problem duplicate <src> <new>` | Duplicate a problem directory to a new slug; optional `--title <TITLE>` |
| `xcpc problem group <GROUP> <ID...>` | Batch-assign problems to a group; pass `--clear <ID...>` to ungroup |
| `xcpc problem tag <ID...>` | Batch add/remove tags: `--add <T1,T2>`, `--remove <T1,T2>` |
| `xcpc problem rating <RATING> <ID...>` | Batch-set difficulty rating; pass `--clear <ID...>` to clear rating |
| `xcpc problem delete <ID...>` | Batch-delete problems (automatically skips problems referenced by unfinalized contests); `--yes` skips confirmation |
| `xcpc problem from-url <url>` | Create a problem from an external OJ URL (deduplicated by `[[source]]`); optional `--id <SLUG>`, `--title <TITLE>` |

### 3. Publishing, Templates & Contest Composition

| Command | Purpose & Key Options |
| :--- | :--- |
| `xcpc build <problem\|contest\|problems> <ID...>` | Compile and export PDF (writes to `build/` by default). Supports `--editorial` (editorial booklet), `--language <zh\|en>`, `-o/--output <PATH>`, `--emit-typst <FILE>`; `build problems` also supports `--title <TITLE>` and `--cover <true\|false>` |
| `xcpc check <problem\|contest> <id>` | Run Typst typesetting and `xcpc-lint` preflight, reporting diagnostics, page count, and overflow page numbers without writing files. Supports `--editorial`, `--language <zh\|en>` |
| `xcpc render <problem\|contest> <id>` | Render pages to PNG (defaults to `build/<id>/page-<n>.png`). Supports `--editorial`, `--language <zh\|en>`, `-o/--out <DIR>`, `--scale <F32>` (default `2.0`), `--page <N>` (1-based) |
| `xcpc template list` | List workspace templates (`templates/*.typ`) and built-in preset seeds |
| `xcpc template schema <name>` | Inspect the parsed declarative `#let schema` of a workspace template |
| `xcpc template add <preset> [name]` | Install a built-in preset (`olymp-classic`, `cnoi-classic`, `minimal`) into `templates/<name>.typ` |
| `xcpc template fork <source> <new_name>` | Duplicate an existing workspace template under a new name |
| `xcpc contest compose <id> [PROBLEM_ID...]` | Create or update a contest's ordered problem list (`A..Z`, up to 26 problems) and metadata: `--name`, `--location`, `--date`, `--author`, `--language`, `--template`, `--cover`, `--blank-pages`, `--show-metadata` |
| `xcpc contest override <contest> <label>` | Set or clear per-problem metadata overrides: `--title` / `--clear-title`, `--time` / `--clear-time`, `--memory` / `--clear-memory`, `--input` / `--clear-input`, `--output` / `--clear-output`, `--show-metadata` / `--clear-show-metadata` |
| `xcpc contest freeze <id>` | Run preflight checks, export statement/editorial PDFs, and finalize the contest into a read-only snapshot at `contests/<id>/snapshot/` |
| `xcpc contest unfreeze <id>` | Show diffs between the finalized snapshot and the current library, then reopen the contest (requires confirmation or `--yes`) |
| `xcpc contest archive <id>` | Archive a contest (marks it read-only and moves it to the archived section; orthogonal to frozen status) |
| `xcpc contest unarchive <id>` | Unarchive a contest, restoring its previous editable/finalized state |
| `xcpc contest audit <id>` | Run a read-only readiness audit across the entire contest (verification freshness, time-limit margin, data volume, solution coverage, and `xcpc-lint`); supports `--markdown <PATH>` to export a report |
| `xcpc contest verify-external <id> <path>` | Trial-run an external tester submission directory without modifying the workspace (flat or `<dir>/<tester>/<prob>.cpp`). Supports `--auto-build`, `-j/--jobs <N>`, `--markdown <PATH>` |

### 4. Problem Authoring, Import & Export

| Command | Purpose & Key Options |
| :--- | :--- |
| `xcpc package inspect <id>` | Read-only inspection of a problem package: file inventory, subtask configuration, expanded test cases, and latest verification freshness |
| `xcpc package init <id>` | Generate problem authoring scaffolding without overwriting existing files. Supports `--interactive` (stdio `interactor.cpp`), `--runs <N>` (multi-run communication), `--subtasks` (OI subtasks) |
| `xcpc package build <ID...>` | Compile components, generate and validate tests, and run the solution matrix. Supports `--strict` (full strict verification), `--sanitize` (isolated ASan/UBSan check), `--tests <LIST>`, `--solutions <LIST>`, `-j/--jobs <N>`, `--contest <CONTEST>` (apply contest time/memory overrides) |
| `xcpc package stress <id> --gen <name>` | Bounded randomized stress testing; saves the first counterexample to `tests/stress-<n>.in`. Supports `--args <TEMPLATE>` (`{i}` / `{seed}`), `--solutions <LIST>`, `--max-iter <N>` (default `100`), `--timeout <S>` (default `30`) |
| `xcpc package verify-external <id> <FILE...>` | Trial-run external `.cpp` solutions in a temporary directory without modifying the workspace. Supports `--auto-build`, `-j/--jobs <N>`, `--markdown <PATH>` |
| `xcpc package adopt <id> <file>` | Atomically adopt an external `.cpp` into `solutions/` and register it in `problem.toml`. Supports `--name`, `--expect <V1,V2>`, `--tag <T1,T2>`, `--main` |
| `xcpc import <target> <FILE...>` | Import external archives: `polygon` or `<plugin>:<importer>`. Polygon import supports `--id <SLUG>`, `--group <NAME>`, `--no-auto-group` |
| `xcpc export <format> <problem\|contest\|problems> <ID...>` | Export judge packages: `domjudge` (alias `dj`), `hydro` (alias `hydrooj`), or `<plugin>:<exporter>`. Supports `-o/--output <PATH>`, `--force` (bypass strict verification gate while still requiring built test data), `--with-pdf` (attach Base64 PDF for plugin exporters) |

### 5. Algorithm Snippets & Team Reference Document (TRD)

| Command | Purpose & Key Options |
| :--- | :--- |
| `xcpc snippet list` | List Algorithm Snippets in `snippets/` with verification status. Supports `--tag <TAG>`, `--status <unverified\|passed\|failed\|stale>`, `--formula-only` |
| `xcpc snippet show <slug>` | Show snippet details, dependencies, reverse references, cumulative short hash, and latest verification report |
| `xcpc snippet new <slug>` | Create a new Algorithm Snippet. Supports `--title`, `--tag`, `--time`, `--space`, `--description`, `--formula-only` |
| `xcpc snippet delete <slug>` | Delete a snippet directory (`--yes` skips confirmation) |
| `xcpc snippet verify [slug]` | Verify a single snippet, downstream dependents (`--cascade`), or the entire library (`--all`). Supports `-j/--jobs <N>`, `--force` (ignore cache and re-run) |
| `xcpc bundle [file]` | Expand a local C++ file or `--snippet <slug>` into a single self-contained C++ file. Supports `-I/--include-dir <DIR>`, `--compact` (strip comments and blank lines), `-o/--output <PATH>`, `--copy` (copy to clipboard) |
| `xcpc trd list` | List all Team Reference Documents under `trds/` |
| `xcpc trd show <id>` | Inspect TRD chapter layout, finalized status, and preflight diagnostics |
| `xcpc trd compose <id> [SLUG...]` | Compose TRD chapters (`--chapter "Chapter Name:slug1,slug2"`, repeatable) and metadata (`--name`, `--author`, `--date`, `--template <trd-compact\|trd-book\|...>`) |
| `xcpc trd check <id>` | Run TRD dependency preflight and Typst compilation checks |
| `xcpc trd build <id>` | Compile and export a TRD PDF; supports `-o/--output <PATH>`, `--emit-typst <FILE>` |
| `xcpc trd render <id>` | Render TRD pages to PNG; supports `-o/--out <DIR>`, `--scale <F32>`, `--page <N>` |
| `xcpc trd freeze <id>` | Validate, export PDF, and finalize a TRD snapshot; `--strict` requires all code snippets to have passing verification |
| `xcpc trd unfreeze <id>` | Display snapshot diffs and reopen a finalized TRD (requires confirmation or `--yes`) |

### 6. Training Sync, Plugins, Tools & MCP

| Command | Purpose & Key Options |
| :--- | :--- |
| `xcpc sync` | Incrementally sync configured OJ platforms and contest schedules. Supports `--platform <ID>`, `--full` |
| `xcpc sync handle <platform> [handle]` | View, set, or clear (`--clear`) the account handle for an OJ platform |
| `xcpc training <status\|dashboard\|contests\|submissions\|log\|editorials>` | Training dashboard queries and manual logging; see [Dashboard Views & Manual Logging](../training/dashboard.md) for full options |
| `xcpc plugin <list\|install\|uninstall\|enable\|disable\|trust\|export>` | Full plugin lifecycle management; see [Plugin Ecosystem & Development](../plugins/index.md) |
| `xcpc plugin <init\|link\|unlink\|check\|pack\|test>` | Plugin scaffolding, hot-reload linking, static/sandbox checks, and packaging; see [Plugin Development Guide](../plugins/development.md) |
| `xcpc plugin repo <list\|catalog\|add\|remove>` | Manage decentralized plugin repository (`index.json`) subscriptions and browse catalogs |
| `xcpc tool list` / `xcpc tool run <plugin>:<tool>` | List or run offline tools provided by enabled plugins headlessly; supports `--input key=value` and `--input-json <JSON>` |
| `xcpc mcp` | Start the MCP server over `stdin`/`stdout` (typically with `-w <DIR>` and optional `--read-only`); see [MCP Server Integration](./mcp.md) |

---

## Common Pipeline Examples

### 1. CI Pre-Contest Verification & Export Pipeline

```bash
# 1. Run strict verification and memory checks on all contest problems
xcpc -w ./contest-ws package build prob-a prob-b prob-c --strict
xcpc -w ./contest-ws package build prob-a prob-b prob-c --sanitize

# 2. Verify statement and editorial typesetting with zero errors
xcpc -w ./contest-ws check contest icpc-2026 --json
xcpc -w ./contest-ws check contest icpc-2026 --editorial --json

# 3. Export contest readiness audit report and DOMjudge package
xcpc -w ./contest-ws contest audit icpc-2026 --markdown ./build/audit.md
xcpc -w ./contest-ws export domjudge contest icpc-2026 -o ./build/domjudge
```

### 2. Algorithm Snippet Refactoring, Cascade Verification & TRD Finalization

```bash
# 1. After editing a base snippet, cascade-verify all downstream dependents
xcpc snippet verify modint --cascade

# 2. Bundle into a compact single-file C++ snippet and copy to clipboard
xcpc bundle --snippet ntt --compact --copy

# 3. Run strict pre-contest TRD finalization (all code snippets must be passed)
xcpc trd check regional-2026
xcpc trd freeze regional-2026 --strict
```

### 3. Daily Training Sync & Upsolving Triage

```bash
# Sync recent submissions and list unsolved Codeforces problems rated 1800–2200
xcpc sync --platform codeforces
xcpc training submissions --by-problem --unsolved --platform codeforces --min-rating 1800 --max-rating 2200
```
