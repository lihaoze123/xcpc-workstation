---
title: Configuration File Reference
---

# Configuration File Reference

XCPC Workstation uses an **open directory as the single source of truth**. All workspace settings, problem metadata, contest definitions, Algorithm Snippets, and Team Reference Document (TRD) handbooks are stored as standard TOML files suited for Git version control and manual review.

> **Preserving unknown fields and comments**: All Core writes to `workspace.toml`, `problem.toml`, `contest.toml`, `snippet.toml`, and `trd.toml` perform semantic in-place edits with `toml_edit` and atomic persistence (temporary file + rename). Hand-written comments and custom unknown fields are preserved when files are edited through the desktop app, CLI, or MCP.
>
> Exception: `[[source]]` entries in `problem.toml` enforce `deny_unknown_fields` and accept only `platform`, `id`, and `url`.

---

## 1. `workspace.toml` (Workspace Root Configuration)

Located at the workspace root. It marks the workspace boundary and stores workspace-wide shared settings (personal preferences and UI themes are stored separately in the user's global `<dataDir>/prefs.json` and `<dataDir>/theme.toml`).

### Fields

| Field | Type | Required | Default | Description |
| :--- | :--- | :--- | :--- | :--- |
| `template` | `string` | No | — | Default statement template name for the workspace (e.g. `"olymp-classic"`, `"cnoi-classic"`, or a custom `templates/<name>.typ`). |
| `groups` | `string[]` | No | `[]` | Declared persistent problem groups (displayed in declared order). |
| `training` | `bool` | No | `true` | Whether to enable the **Training** space in the desktop app. |
| `snippets` | `bool` | No | `true` | Whether to enable the **Snippets** space (Algorithm Snippets and Team Reference Document (TRD)) in the desktop app. |
| `[template-options.<tpl>]` | `table` | No | `{}` | Workspace-level default template options, namespaced by template ID (overridden by matching contest or problem keys). |

### Minimal Example

```toml
template = "olymp-classic"
groups = ["Regional Selection", "Warmup"]
training = true
snippets = true

[template-options.olymp-classic]
body-size = "11pt"
```

---

## 2. `problem.toml` (Problem Metadata & Build Specification)

Located at `problems/<slug>/problem.toml`. The directory name `<slug>` is the permanent problem ID and is never renamed after creation. Training status is never written into this file; it is derived at runtime by merging `training/logs.jsonl` with the global `<dataDir>/oj_sync.db`.

### Core Fields

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `title` | `string` | Yes | Problem title. |
| `group` | `string` | No | Name of the persistent problem group. |
| `rating` | `integer` | No | Problem difficulty rating (non-negative integer). |
| `tags` | `string[]` | No | List of algorithm or topic tags. |
| `author` | `string` | No | Author attribution. |
| `[limits]` | `table` | No | Time, memory, and file I/O limits: `time` (e.g. `"1s"`), `memory` (e.g. `"256MB"`), `input`, `output` (omit or leave empty for standard I/O). |
| `[[source]]` | `table[]` | No | External OJ identifiers: `platform` (required), `id` (required), `url` (optional). Strictly validated to allow only these three fields. |
| `[template-options.<tpl>]` | `table` | No | Per-problem template option overrides, namespaced by template ID. |

### Problem Authoring & Export Fields (Optional)

| Field | Type | Description |
| :--- | :--- | :--- |
| `[package]` | `table` | `checker` (`wcmp`/`ncmp`/`fcmp`/`lcmp`/`rcmp4`/`rcmp6`/`rcmp9`/`yesno`/`nyesno`, or `"checker.cpp"`; if omitted, uses `checker.cpp` when present, otherwise `wcmp`); `runs` (multi-run communication pass count; enabled when `>= 2`). |
| `[[subtask]]` | `table[]` | OI subtasks: `id` (required), `score` (non-negative integer, required), `depends` (list of dependency subtask IDs), `scoring` (`"min"` for all-or-nothing or `"sum"` for proportional scoring; defaults to `"min"`). |
| `[[test]]` | `table[]` | Test declarations: hand-written `file` (e.g. `"tests/hand-01.in"`) or generator `gen` (mapping to `generators/<gen>.cpp`) + `args` (supports `{i}` expansion over `1..=repeat`), `repeat`, `tag`, `name`, `sample` (writes back to `samples/` when `true`), `subtask`. |
| `[solutions.<name>]` | `table` | Solution declaration (mapping to `solutions/<name>.cpp`): `main` (whether this is the main reference solution), `expect` (expected verdict, e.g. `"AC"` or `["WA", "TLE"]`), `tags` (run only on tests matching these tags), `score` (expected total score, integer or `[min, max]`), `subtasks` (map of subtask ID to expected verdict). |

### Minimal Example

```toml
title = "Tree Beacons"
group = "Regional Selection"
rating = 1900
tags = ["tree", "dp"]
author = "Problem Committee"

[limits]
time = "1s"
memory = "256MB"

[[source]]
platform = "codeforces"
id = "1999A"
url = "https://codeforces.com/problemset/problem/1999/A"

[package]
checker = "wcmp"

[[test]]
gen = "gen_random"
args = "--n 100000 --seed {i}"
repeat = 5
tag = "max"

[solutions.std]
main = true
expect = "AC"

[solutions.brute]
expect = ["TLE", "WA"]
```

---

## 3. `contest.toml` (Contest Composition & Overrides)

Located at `contests/<id>/contest.toml`. Contests reference problems from the library via **live references**; `[[problem]]` overrides may only change metadata and per-problem template options, never statement prose, samples, or assets.

### Fields

| Field | Type | Required | Default | Description |
| :--- | :--- | :--- | :--- | :--- |
| `name` | `string` | Yes | — | Contest name. |
| `location` | `string` | No | — | Contest venue or location. |
| `date` | `string` | No | — | Contest date text. |
| `author` | `string` | No | — | Author or organizing credits. |
| `language` | `string` | No | `"chinese"` | Publication language: `"chinese"` or `"english"`. |
| `show-metadata` | `bool` | No | `true` | Whether to display the problem limits header table (overridable per problem). |
| `cover` | `bool` | No | `true` when `>= 2` problems | Whether to generate a cover page with the problem list. |
| `blank-pages` | `bool` | No | `false` | Whether to force each problem to start on an odd page (inserting blank pages for duplex printing). |
| `template` | `string` | No | Inherits workspace default | Template name for this contest (if `contests/<id>/template.typ` exists, the contest-specific template takes precedence). |
| `[template-options.<tpl>]` | `table` | No | `{}` | Contest-level template option overrides. |
| `frozen` | `bool` | No | `false` | Whether the contest is finalized; when `true`, it is read-only and rendered from `snapshot/`. |
| `archived` | `bool` | No | `false` | Whether the contest is archived (orthogonal to `frozen`; archived contests are read-only and folded under the sidebar's Archived section). |
| `[[problem]]` | `table[]` | No | `[]` | Ordered list of referenced problems: `id` (problem slug, required), `label` (problem letter such as `"A"`, required), plus optional overrides `title`, `limits`, `show-metadata`, and `options.<tpl>`. |

### Minimal Example

```toml
name = "2026 Autumn Selection Contest"
location = "Beijing"
date = "November 2026"
language = "english"
show-metadata = true
blank-pages = false
template = "olymp-classic"
frozen = false

[[problem]]
id = "tree-beacon"
label = "A"

[[problem]]
id = "crc-check"
label = "B"
title = "Cyclic Redundancy Check (Adapted)"
limits = { time = "2s", memory = "512MB" }
```

---

## 4. `snippet.toml` (Algorithm Snippet Metadata & Verification)

Located at `snippets/<slug>/snippet.toml`. The same directory holds `code.hpp` (omitted for formula-only reference cards), `notes.md`, and verification sources. Inter-snippet dependencies are not written into this file; Core automatically resolves the dependency graph from `#include "<slug>/code.hpp"` directives (with optional `./` or `../` prefixes) in `code.hpp`. Verification state (`passed` / `stale` / `failed` / `unverified`) is cached in `.xcpc/snippet-verify/<slug>.json` without modifying this file.

### Fields

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `title` | `string` | Yes | Algorithm Snippet title. |
| `tags` | `string[]` | No | List of category tags. |
| `time_complexity` | `string` | No | Time complexity note (also accepts alias `time-complexity`). |
| `space_complexity` | `string` | No | Space complexity note (also accepts alias `space-complexity`). |
| `description` | `string` | No | Short summary of purpose or usage. |
| `source_problem` | `string` | No | Workspace problem ID from which the snippet was extracted (also accepts alias `source-problem`). |
| `[[verify]]` | `table[]` | No | Verification sources, classified by `kind`:<br>- `kind = "self"`: `file` (path to self-test source inside the snippet directory; exit code 0 passes);<br>- `kind = "problem"`: `problem` (workspace problem ID) + `adapter` (adapter source path);<br>- `kind = "library_checker"`: `problem` (Library Checker problem ID) + `adapter` (adapter source path). |

### Minimal Example

```toml
title = "Number Theoretic Transform (NTT)"
tags = ["poly", "math"]
time_complexity = "O(n log n)"
space_complexity = "O(n)"
description = "Polynomial multiplication modulo 998244353"

[[verify]]
kind = "self"
file = "test.cpp"

[[verify]]
kind = "library_checker"
problem = "convolution_mod"
adapter = "verify_conv.cpp"
```

---

## 5. `trd.toml` (Team Reference Document Composition)

Located at `trds/<id>/trd.toml`. Organizes Algorithm Snippets into chapters via live references. When finalized, the source files, dependency closure, and exported PDF are frozen into `trds/<id>/snapshot/` under read-only protection.

### Fields

| Field | Type | Required | Default | Description |
| :--- | :--- | :--- | :--- | :--- |
| `name` | `string` | Yes | — | Handbook title. |
| `author` | `string` | No | — | Team or author name. |
| `date` | `string` | No | — | Date text. |
| `template` | `string` | No | `"trd-compact"` | Typesetting template name (built-in preset `trd-compact` / `trd-book`, or a custom `templates/<name>.typ`). |
| `frozen` | `bool` | No | `false` | Whether the handbook is finalized; when `true`, it is read-only and rendered from `snapshot/`. |
| `archived` | `bool` | No | `false` | Whether the handbook is archived (orthogonal to `frozen`). |
| `[template-options.<tpl>]` | `table` | No | `{}` | Handbook-level template option overrides (such as `columns`, `hash-interval`, `strip-comments`). |
| `[[chapter]]` | `table[]` | No | `[]` | Ordered list of chapters: `title` (chapter title, required) and `items` (list of Algorithm Snippet slugs). |

### Minimal Example

```toml
name = "2026 ICPC Regional Team Reference Document"
author = "Team Name"
date = "November 2026"
template = "trd-compact"
frozen = false
archived = false

[template-options.trd-compact]
columns = 3
hash-interval = 10
strip-comments = false

[[chapter]]
title = "Math & Polynomials"
items = ["modint", "ntt"]

[[chapter]]
title = "Data Structures"
items = ["dsu", "segtree"]
```
