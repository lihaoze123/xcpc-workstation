# Contest Composition & Finalization

In XCPC Workstation, a contest (`contests/<id>/contest.toml`) is an ordered collection of problems from the library. While in **Draft** state, contests use **live references** so statement fixes propagate automatically. Before printing, **Finalizing** (freezing) a contest creates a read-only snapshot of all source files, active templates, and PDFs for exact historical reproducibility.

## Live-Reference Composition & Metadata Overrides

### `contest.toml` Example

```toml
name = "ICPC 2026 Regional"
location = "Shanghai"
date = "November 20, 2026"
author = "Problem Committee"
language = "english"              # chinese | english
template = "olymp-classic"
frozen = false
archived = false

[template-options.olymp-classic]
cover = "auto"
blank-pages = true

[[problem]]
id = "lantern-street"
label = "A"

[[problem]]
id = "matrix-walk"
label = "B"
title = "Matrix Walk"             # Overrides title in this contest only

[problem.limits]
time = "2s"                       # Overrides time limit in this contest's statement only

[problem.options.olymp-classic]
sample-layout = "wide"            # Per-problem template option override
```

### Live-Reference Rules & Override Boundaries

- **Automatic Sync**: Fixing a typo or updating a sample in the problem library immediately updates every **unfinalized** contest that references that problem.
- **Automatic Label Reordering**: Reordering problems by dragging in the desktop app or via the CLI automatically reassigns labels `A`, `B`, `C`, … up to a maximum of 26 problems (`A`–`Z`).
- **Allowed Overrides**:
  - Metadata: `title`, `limits` (`time` / `memory` / `input` / `output`)
  - Per-problem template options: `[problem.options.<template>]` (e.g., `show-metadata`, `sample-layout`, `prob-type`)
- **Disallowed Overrides**: **Statement prose, samples, and images cannot be overridden at the contest layer.** If a contest needs a modified statement or sample variant, duplicate the problem in the library first (**Duplicate Problem…** in the desktop app or `xcpc problem duplicate <src> <new>`), then reference the new problem in the contest.
- **Scope of Limit Overrides**: `limits` overrides in `contest.toml` affect statement publishing only. Problem authoring verification uses `problem.toml` limits by default (pass `--contest <id>` when building if you want to verify against contest-overridden limits).

### CLI Composition & Override Commands

```bash
# Create or update a contest's problem list and metadata
xcpc contest compose spring-2026 lantern-street matrix-walk stamp-collecting \
  --name "Spring Contest 2026" \
  --date "April 18, 2026" \
  --language english \
  --template olymp-classic \
  --blank-pages true

# Set or clear metadata overrides for a problem in this contest
xcpc contest override spring-2026 B --title "Matrix Walk" --time "2s" --show-metadata true
xcpc contest override spring-2026 B --clear-title --clear-time

# Inspect contest problem list, per-problem page ranges, and summary stats
xcpc show contest spring-2026
```

## PDF Publishing & Blank Pages for Duplex Printing

XCPC Workstation supports three publishing formats. The desktop app (**Export PDF** in the studio top bar), CLI (`xcpc build`), and MCP produce identical PDFs (written to `build/` by default with automatic title-based filenames and embedded PDF metadata such as Title and Author):

1. **Contest Statement Booklet**: Includes cover page and problem list page (enabled by default when there is more than 1 problem), per-problem page breaks, and consistent headers/footers.
2. **A4 Editorial Booklet (`--editorial`)**: Uses the same paper size, margins, and typography as the statement booklet, opening with a title page and clickable table of contents, per-problem page breaks, PDF bookmarks, problem metadata, and monochrome-highlighted code blocks. Problems without `editorial.md` are skipped with a warning.
3. **Single-Problem PDF / Ad-hoc Booklet**: `xcpc build problem <id>` exports a standalone problem without a cover page or letter prefix; `xcpc build problems <id1> <id2> ...` combines arbitrary problems into an ad-hoc booklet.

### Duplex Blank Pages (`blank-pages`)

When `blank-pages = true` is enabled:

- Every problem starts on an **odd-numbered (right-hand) page**.
- If the previous problem ends on an odd page, the engine automatically inserts a blank even page centered with `This page is intentionally left blank` (or `此页故意留白` in Chinese mode).
- If the final problem ends on an odd page, a trailing blank page is appended so the booklet has an even page count ready for double-sided printing and binding.

## Finalization Snapshot (Freeze) & Read-Only Protection

Once pre-contest review is complete, **Finalize** (freeze) the contest to lock its historical state:

```bash
xcpc contest freeze spring-2026
```

The Core executes finalization atomically in three steps:

1. **Integrity Preflight**: Compiles both statement and editorial booklets in full and runs `xcpc-lint`. Any `error`-level diagnostic blocks finalization (warnings do not block). For problems with authoring packages, it also verifies that the latest strict verification report's content hash is still valid (without silently recompiling or running tests).
2. **Read-Only Snapshot Creation**: Writes `frozen = true` and saves the following into `contests/<id>/snapshot/`:
   - Resolved source copies for each problem with contest overrides merged (`problem.toml`, `statement*.md`, `editorial.md`, `samples/`, `assets/`, excluding package source code and test data)
   - Active `template.typ`, `templates/preamble.typ` (if present), and resolved template options
   - Exported statement booklet PDF and editorial booklet PDF (omitted if no editorials exist)
3. **Read-Only Guard**: Once finalized, the Core enforces strict read-only protection on the entire contest (including `snapshot/`), rejecting any edits to metadata, problem ordering, or overrides. Subsequent previews and exports read directly from `snapshot/`, unaffected by later edits in the library or workspace templates.

!!! tip "Commit `snapshot/` to Git"
    `contests/<id>/snapshot/` is a formal contest archive and should be committed to Git alongside `contest.toml` so the exact booklet can be viewed or reprinted years later.

## Reopening a Finalized Contest (Unfreeze)

If errata must be applied after finalization, a human operator must explicitly **Reopen** (unfreeze) the contest from the desktop app or CLI (**for safety, the MCP server does not expose an unfreeze tool**):

```bash
xcpc contest unfreeze spring-2026
```

Before reopening, the Core performs a file-by-file byte comparison between `snapshot/` and the current problem library, listing every problem whose statement, samples, or assets have changed or been deleted, and requires typing the contest ID as confirmation (scripts may pass `--yes` explicitly to skip interactive confirmation). Upon confirmation, `snapshot/` is deleted and the contest resumes live references and editing.

## Archiving Contests

The application does not provide a permanent "Delete Contest" button. Instead, finished contests are **Archived** (`archived = true` in `contest.toml`) and folded into the archived section:

```bash
# Archive a contest (fold and enforce read-only)
xcpc contest archive spring-2026

# Unarchive a contest (restore previous draft/finalized state)
xcpc contest unarchive spring-2026
```

- `archived` and `frozen` are completely orthogonal: both draft and finalized contests can be archived.
- While archived, the Core rejects all write operations (returning `contest_archived`); use **Unarchive** (`xcpc contest unarchive`) to restore access.
- Deleting a problem that is still referenced by an **unfinalized archived contest** is still blocked to protect historical contest records.
