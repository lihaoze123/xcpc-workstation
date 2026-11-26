---
title: Team Reference Document (TRD)
---

# Team Reference Document (TRD)

Team Reference Documents (TRDs) live under `trds/<id>/` in the workspace and assemble algorithm snippets and formula cards from `snippets/` via **live references**: as you maintain and verify snippets during training, the handbook updates automatically; before printing for a contest, run pre-checks and finalize to lock an immutable version.

## Chapter Composition (`trd.toml`)

Each handbook is defined by `trds/<id>/trd.toml`, specifying metadata, template options, and chapter ordering:

```toml
name = "2026 ICPC Regional TRD"
author = "Team Alpha"
date = "November 2026"
template = "trd-compact"      # Options: trd-compact (default), trd-book, or custom templates/<name>.typ
frozen = false
archived = false

[template-options.trd-compact]
columns = "3"                 # Column count (trd-compact: "2"/"3"/"4"; trd-book: "1"/"2")
font-size = "7.5pt"           # Body font size
hash-interval = 10            # Cumulative checksum interval (lines); 0 prints only the final hash
strip-comments = false        # Strip C/C++ comments when printing

[[chapter]]
title = "Math & Polynomials"
items = ["modint", "ntt", "number-theory-facts"]

[[chapter]]
title = "Data Structures"
items = ["dsu"]
```

You can also create or update handbook composition from the CLI with `xcpc trd compose`:

```bash
# Compose chapters (--chapter is repeatable, format "Chapter Title:slug1,slug2")
xcpc trd compose icpc-regional \
  --name "2026 ICPC Regional TRD" \
  --author "Team Alpha" \
  --date "November 2026" \
  --template trd-compact \
  --chapter "Math & Polynomials:modint,ntt,number-theory-facts" \
  --chapter "Data Structures:dsu"

# List all handbooks or inspect chapters and pre-check status for one handbook
xcpc trd list
xcpc trd show icpc-regional
```

## Built-in Typesetting Templates

XCPC Workstation ships with two print-ready grayscale Typst templates tailored to different workflows. Both support declarative tuning via `[template-options.<template>]` and can be forked into custom workspace templates using `xcpc template fork`:

| Template ID | Paper & Layout | Default Columns / Size | Typical Use Case |
| :--- | :--- | :--- | :--- |
| `trd-compact` (default) | Landscape A4, compact multi-column, grayscale print | 3 columns / `7.5pt` (2/3/4 columns) | 25-page printed contest reference handbook |
| `trd-book` | Portrait A4, serif prose, grayscale print | 1 column / `10pt` (1/2 columns) | Pre-contest study notes, derivations, and mixed formula/code booklets |

Both templates natively provide:

- **Automatic table of contents and headers/footers**: Generates chapter navigation and page numbers for fast lookup during a contest.
- **KACTL++ cumulative checksum margin**: Prints line numbers and 6-character cumulative MD5 digests at every `hash-interval`, along with the terminal verification command below the table of contents when `hash-interval > 0`. When `strip-comments = true`, comments are stripped and blank lines collapsed **before** computing checksums so the printed code and printed digests match exactly.
- **Dependency badges**: Automatically strips `#include "<slug>/code.hpp"` from the code body and displays required upstream snippets cleanly in the item header.

## Pre-Print Checks (`xcpc trd check`)

Before exporting or printing, run `xcpc trd check` to execute both **structure/verification pre-checks** and **Typst compilation validation** without writing any files:

```bash
xcpc trd check icpc-regional
```

Pre-checks detect five categories of structural and quality issues:

| Severity | Check | Description |
| :--- | :--- | :--- |
| Error | `missing_snippet` | A chapter references a snippet slug that does not exist in the workspace |
| Error | `missing_dependency` | A snippet depends on an upstream snippet (e.g., `ntt` depends on `modint`) that is not included in the handbook |
| Error | `dependency_cycle` | Referenced snippets contain a cyclic `#include` dependency |
| Warning | `stale_snippet` | An included code snippet or its upstream dependency was modified after its last verification (formula cards excluded) |
| Warning | `failed_snippet` | An included code snippet failed its latest verification (formula cards excluded) |

!!! note "Unverified Snippets"
    Code snippets with status `unverified` do not emit a pre-check warning in `xcpc trd check` by default. To require every code snippet to have a fresh passing verification before locking the handbook, use `xcpc trd freeze <id> --strict`.

## Building PDFs and Rendering Pages

```bash
# Compile and export PDF (defaults to build/<name>.pdf in the workspace)
xcpc trd build icpc-regional
xcpc trd build icpc-regional -o ./dist/icpc-2026-trd.pdf

# Also emit generated Typst source for layout inspection
xcpc trd build icpc-regional --emit-typst /tmp/trd.typ

# Render pages to PNG previews (defaults to build/<id>/page-<n>.png, scale 2.0 ≈ 144 dpi)
xcpc trd render icpc-regional
xcpc trd render icpc-regional --page 1 --scale 2.5 -o ./preview/
```

## Reversible Finalization and Read-Only Snapshots (`freeze` / `unfreeze`)

When a Team Reference Document is ready for printing, **finalize** it to archive all referenced snippet files, layout settings, and the compiled PDF into `trds/<id>/snapshot/`:

```bash
# Standard finalization: allows warnings (blocking errors such as missing snippets, missing deps, or cycles still abort)
xcpc trd freeze icpc-regional

# Strict finalization: refuses to finalize if any code snippet is unverified, stale, or failed
xcpc trd freeze icpc-regional --strict
```

Post-finalization guarantees match contest finalization (see [Contest Assembly & Finalization](../publishing/contests.md)):

- **Strict read-only enforcement**: A handbook with `frozen = true` in `trd.toml` rejects all mutations at the Core layer. Subsequent previews, checks, and exports read directly from `trds/<id>/snapshot/` and are **completely unaffected** by later edits to `snippets/`.
- **Reversible reopening with diff summary**: To revise a finalized handbook against the latest snippet library, reopen it from the desktop UI (**Reopen**) or CLI. Before unfreezing, the system compares `snapshot/` against current `snippets/` (code changes, note changes, metadata changes, or deleted snippets) and requires confirmation (pass `--yes` in CLI to skip interactive prompt):

```bash
xcpc trd unfreeze icpc-regional
```

!!! warning "Security Boundary"
    Unfreezing (`unfreeze`) is available only in the desktop UI and local CLI. It is **not exposed via MCP tools**, preventing external AI agents from lifting read-only protection on finalized handbooks.
