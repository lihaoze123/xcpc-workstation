---
title: MCP Server Integration
---

# MCP Server Integration

`xcpc mcp` provides a native [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server over standard input/output (`stdio`, newline-delimited JSON-RPC 2.0), supporting protocol versions `2025-11-25`, `2025-06-18`, `2025-03-26`, and `2024-11-05`.

Once connected, MCP-compatible AI clients (such as Claude Desktop, Cursor, or Codex) can search your problem library, compose contests, compile and diagnose Typst statements, inspect rendered page images, run Problem Authoring & Export verification, maintain Algorithm Snippets and Team Reference Documents (TRDs), and query training analytics through structured tool calls.

---

## Client Configuration

Before connecting, verify that the `xcpc` binary is on your `PATH` (or specify its absolute path in the configuration), and replace `/path/to/your/workspace` with your actual workspace directory.

=== "Claude Desktop"

    Edit the Claude Desktop configuration file (macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`; Windows: `%APPDATA%\Claude\claude_desktop_config.json`; Linux: `~/.config/Claude/claude_desktop_config.json`):

    ```json
    {
      "mcpServers": {
        "xcpc": {
          "command": "xcpc",
          "args": ["-w", "/path/to/your/workspace", "mcp"]
        }
      }
    }
    ```

=== "Cursor"

    Create `.cursor/mcp.json` in your project root (or configure it in Cursor's global MCP settings):

    ```json
    {
      "mcpServers": {
        "xcpc": {
          "command": "xcpc",
          "args": ["-w", "/path/to/your/workspace", "mcp"]
        }
      }
    }
    ```

=== "Codex"

    Add the following to `~/.codex/config.toml`:

    ```toml
    [mcp_servers.xcpc]
    command = "xcpc"
    args = ["-w", "/path/to/your/workspace", "mcp"]
    ```

!!! tip "Read-Only Mode (`--read-only`)"
    If you only want an AI assistant to read the library, review statements, inspect rendered pages, audit a contest, or analyze training metrics without modifying any files, add `"--read-only"` to `args`:

    ```json
    "args": ["-w", "/path/to/your/workspace", "--read-only", "mcp"]
    ```

    In `--read-only` mode, all write and build-execution tools (such as `write_problem`, `export_pdf`, `build_package`, `write_snippet`, and `set_training_status`) are blocked at the Core layer with error code `read_only`. `manage_groups` permits only `action: "list"`, and `run_external_solution` permits only `auto_build: false`.

---

## Security Boundaries

All security boundaries are enforced inside the Rust Core rather than relying on client-side prompt instructions:

1. **No Unfreeze via MCP**:
   - `unfreeze_contest` and `unfreeze_trd` are **never exposed** as MCP tools (invoking them returns JSON-RPC `-32602` unknown tool error).
   - Once finalized, a contest or TRD is strictly read-only. Reopening requires explicit human confirmation in the desktop UI or via `xcpc contest unfreeze` / `xcpc trd unfreeze` in a terminal.
2. **Workspace Path Sandbox & Write Allowlist**:
   - All file writes use temporary files followed by atomic rename to prevent partial-write corruption.
   - Path traversal (`..` or symlinks escaping the workspace root) is rejected. `write_problem` and `write_snippet` accept only allowlisted relative paths (such as `problem.toml`, `statement*.md`, `editorial.md`, `samples/*`, `solutions/*`, `generators/*`, `tests/*`, `checker.cpp`, `validator.cpp`, `interactor.cpp`, etc.).
   - `bundle_code` is strictly sandboxed to paths inside the workspace or snippet slugs and cannot read arbitrary host files.
   - `import_polygon` enforces `.zip` extensions, Zip Slip checks, and uncompressed size limits, and refuses to overwrite an existing problem slug.
3. **Concurrent Write Conflict Detection & Reference Protection**:
   - `write_problem` accepts an optional `expected_hash` (SHA-256) and returns `conflict` instead of overwriting if the file was modified externally on disk.
   - `batch_delete_problems` automatically skips problems referenced by unfinalized contests and reports `problem_referenced`.

---

## Complete MCP Tool Reference (41 Tools)

The tables below list all 41 tools exposed by `xcpc mcp`, grouped by functional domain. The **Read-Only** column reflects each tool's `readOnlyHint` annotation and `--read-only` policy.

### 1. Typesetting, Preview & Templates (6 Tools)

| Tool Name | Read-Only | Description & Core Parameters |
| :--- | :---: | :--- |
| `compile_and_diagnose` | Yes | Compile a problem, contest, or TRD (`target: "problem" \| "contest" \| "trd"`, `id`, optional `book: "statements" \| "editorial"`, `language`); returns compile errors, `xcpc-lint` diagnostics, page count, and overflow page numbers |
| `render_page_png` | Yes | Render a specified page as Base64 PNG for visual inspection (`target`, `id`, optional `book`, `language`, `page` default `1`, `scale` default `1.5`) |
| `export_pdf` | No | Compile and export a problem, contest, or TRD PDF into the workspace (`target`, `id`, optional `book`, `language`, `output`); does not write if compile or lint errors exist |
| `list_templates` | Yes | List workspace Typst templates (`templates/*.typ`), parsed declarative `#let schema` fields, and installable built-in preset seeds |
| `add_template` | No | Install a built-in preset (`preset: "olymp-classic" \| "cnoi-classic" \| "minimal" \| "trd-compact" \| "trd-book"`, optional `name`) into `templates/` without overwriting existing files |
| `set_template_options` | No | Update template selection and namespaced options (`scope: "workspace" \| "contest" \| "problem"`, optional `id`, `template`, `options`) |

### 2. Problem Library Search & Management (7 Tools)

| Tool Name | Read-Only | Description & Core Parameters |
| :--- | :---: | :--- |
| `search_problems` | Yes | Search problem metadata summaries by keyword, tags, rating range, platform, or group (`query`/`text`, `tags`, `rating_min`, `rating_max`, `platform`, `group`, `ungrouped`) |
| `get_problem` | Yes | Read problem metadata, full `statement*.md` / `editorial.md` text with SHA-256 hashes, samples, and `solutions/` file inventory (`id`) |
| `write_problem` | No | Atomically create or update allowlisted problem files (`id`, `files`, optional `expected_hash` for optimistic concurrency) |
| `duplicate_problem` | No | Duplicate a problem directory to a new slug (`source_id`, `new_id`, optional `title`), preserving `problem.toml` comments and unknown fields |
| `manage_groups` | Varies | Manage persistent groups (`action: "list" \| "create" \| "rename" \| "delete" \| "reorder"`, with `name`/`new_name`/`order` as needed); only `list` is allowed under `--read-only` |
| `batch_update_problems` | No | Batch-update group, tags, and rating across multiple problems (`ids`, optional `group`, `tags_add`, `tags_remove`, `rating`) |
| `batch_delete_problems` | No | Batch-delete problems (`ids`), automatically skipping problems referenced by unfinalized contests |

### 3. Contest Composition, Overrides & Finalization (6 Tools)

| Tool Name | Read-Only | Description & Core Parameters |
| :--- | :---: | :--- |
| `get_contest` | Yes | Lists all contests when `id` is omitted; when `id` is provided, returns contest metadata, finalized status, per-problem overrides, compiled page ranges, and difficulty/tag statistics |
| `compose_contest` | No | Create or update a contest's ordered problem list (`A..Z`, up to 26 problems) and metadata (`id`, `problems`, optional `name`, `location`, `date`, `author`, `language`, `template`, `show-metadata`, `cover`, `blank-pages`) |
| `set_override` | No | Set or clear per-problem metadata overrides within a contest (`contest`, `label`, `fields: { title, limits, show-metadata, options }`) |
| `freeze_contest` | No | Validate, export snapshot PDFs, and finalize a contest into a read-only archive (`id`); rejects if compile or lint errors exist |
| `archive_contest` | No | Mark a contest as archived (read-only, orthogonal to frozen status; `id`) |
| `unarchive_contest` | No | Remove the archived flag and restore the contest's previous state (`id`) |

### 4. Problem Authoring, Verification & Package Exchange (9 Tools)

| Tool Name | Read-Only | Description & Core Parameters |
| :--- | :---: | :--- |
| `inspect_package` | Yes | Read-only package overview: file inventory, interactive/multi-run/OI subtask config, expanded tests, verification freshness, gate status, and next-step guidance (`id`) |
| `inspect_test_case` | Yes | Safely inspect a test case's generator command, byte size, line count, and bounded head/tail preview (`id`, `test`, optional `head_lines`, `tail_lines`) without flooding context |
| `init_package` | No | Generate problem authoring scaffolding without overwriting existing files (`id`, optional `interactive`, `runs`, `subtasks`) |
| `build_package` | No | Compile components and run the test matrix (`id`, `mode: "debug" \| "verify" \| "sanitize"`, optional `tests`, `solutions`, `jobs`, `contest`) |
| `stress_package` | No | Bounded randomized stress testing (supports standard, interactive, and multi-run problems); writes the first counterexample to `tests/stress-<n>.in` (`id`, `gen`, optional `args`, `solutions`, `max_iter`, `timeout`) |
| `audit_contest` | Yes | Read-only contest readiness audit aggregating verification freshness, time-limit margins, data volume, solution coverage, and `xcpc-lint` risk flags (`id`) |
| `run_external_solution` | Yes | Trial-run external `.cpp` files or a tester directory in a temporary folder without polluting the workspace (`target: "problem" \| "contest"`, `id`, `path`/`paths`, optional `auto_build`, `jobs`); `auto_build: true` is rejected under `--read-only` |
| `import_polygon` | No | Import a Polygon problem or contest `.zip` archive (`zip_path`, optional `id`, `group`, `no_auto_group`) |
| `export_oj_package` | No | Export a minimal DOMjudge / Hydro `.zip` package (`format: "domjudge" \| "hydro"`, `target: "problem" \| "contest" \| "problems"`, `id`/`ids`, optional `output`, `force`) |

### 5. Training Dashboard & Personal Records (5 Tools)

| Tool Name | Read-Only | Description & Core Parameters |
| :--- | :---: | :--- |
| `get_training_dashboard` | Yes | Read aggregated training analytics combining `oj_sync.db` and the current workspace (`window: "30d" \| "90d" \| "year" \| "all"`, optional `heatmap_year`, `tag_min_rating`) |
| `set_training_status` | No | Append a manual status update for a workspace problem to `training/logs.jsonl` and return the derived status (`problem_id`, `status`, optional `platform` + `source_id`) |
| `ingest_problem` | No | Add an external OJ problem to the workspace library by URL or `(platform, problem_key)`, deduplicating automatically and filling metadata (optional `id`, `title`, `tags`, `rating`, `initial_status`) |
| `log_training_record` | No | Append a manual training entry (linked to a workspace problem or standalone external/offline record; optional `create_in_library: true`) |
| `list_training_editorials` | Yes | Search workspace problems that contain a non-empty `editorial.md` along with excerpts (optional `search`, `tag`, `min_rating`, `max_rating`) |

### 6. Algorithm Snippets & Team Reference Document (TRD) (8 Tools)

| Tool Name | Read-Only | Description & Core Parameters |
| :--- | :---: | :--- |
| `search_snippets` | Yes | Search Algorithm Snippets by keyword, tag, verification status (`unverified`/`passed`/`failed`/`stale`), or formula-only flag (`formula_only`) |
| `get_snippet` | Yes | Read full snippet metadata, `code.hpp`, `notes.md`, test files, dependency DAG, reverse references, cumulative short hash, and verification report (`slug`) |
| `write_snippet` | No | Atomically create or update snippet metadata and allowlisted files (`slug`, metadata fields, and `files`) |
| `verify_snippet` | Yes | Compile and run three-source snippet verification (`self`/`problem`/`library_checker`) for a single snippet, downstream dependents (`cascade: true`), or the entire library (`all: true`) without modifying source files |
| `bundle_code` | Yes | Expand a snippet `slug`, workspace-relative `path`, or inline `source` into a single self-contained C++ file (optional `include_dirs`, `compact`), strictly sandboxed within the workspace |
| `get_trd` | Yes | Lists all TRDs when `id` is omitted; when `id` is provided, returns chapter layout, finalized status, and preflight diagnostics |
| `compose_trd` | No | Create or update TRD chapters and metadata (`id`, `chapters`, optional `name`, `author`, `date`, `template`); rejects edits to finalized TRDs |
| `freeze_trd` | No | Validate, export PDF, archive source and template snapshots, and finalize a TRD (`id`, optional `strict: true` requiring all code snippets to pass verification) |

---

## Next Steps

- Pair MCP with the official Skills so AI agents follow standardized engineering workflows: [Official AI Skills Guide](./skills.md)
- Reference headless CLI commands and exit codes: [CLI Reference](./cli.md)
