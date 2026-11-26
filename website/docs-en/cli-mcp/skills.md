---
title: Official AI Skills Guide
---

# Official AI Skills Guide

In addition to the underlying [MCP Server](./mcp.md) and [CLI](./cli.md), XCPC Workstation provides three official AI Skills that codify engineering workflows for Problem Authoring & Export, Algorithm Snippets & Team Reference Document (TRD) curation, and plugin development into reusable agent instructions.

---

## Installation & Activation

### 1. Install the Built-in Authoring Skill via `xcpc init --with-skills`

Passing `--with-skills` when initializing a workspace atomically writes the embedded **`xcpc-problem-creator`** Skill suite into `.agents/skills/xcpc-problem-creator/`:

```bash
# Initialize an empty workspace and install the built-in authoring Skill
xcpc init ./my-contest --with-skills

# Or include example problems and contests as well
xcpc init ./my-contest --example --with-skills
```

This command writes the following 5 files:

```text
.agents/skills/xcpc-problem-creator/
├── SKILL.md                    # End-to-end authoring workflow and strict quality guardrails
├── mistake-taxonomy.md         # Taxonomy of common XCPC / OI wrong solutions and adversarial tests
├── judge-validator/SKILL.md    # Validator, Checker, Interactor, and self-test guidelines
├── solutions-tutorial/SKILL.md # Reference solutions, brute/partial solutions, stress testing, and editorials
└── wrong-gen/SKILL.md          # testlib.h generators, subtasks, and wrong-solution matrix rules
```

!!! note "Safe to Re-run"
    `xcpc init --with-skills` never overwrites an existing manually edited `AGENTS.md`, so it can safely be re-run in an existing workspace to install or refresh the built-in authoring Skill files.

### 2. Install Additional Official Skills (`xcpc-trd-curator` & `xcpc-plugin-creator`)

The repository's `skills/` directory maintains two additional official Skills:

- `skills/xcpc-trd-curator/SKILL.md`
- `skills/xcpc-plugin-creator/SKILL.md`

To use them in a workspace or globally, copy the corresponding directories into your workspace's `.agents/skills/` folder (or your AI client's global Skills directory). When paired with `xcpc mcp`, the agent automatically loads the relevant Skill upon recognizing a matching task.

---

## Overview of the Three Official Skills

### 1. `xcpc-problem-creator` — Problem Authoring, Verification & Publishing

- **Scope**: End-to-end creation or refactoring of competitive programming problems, covering **standard problems**, **stdio interactive problems (`interactor.cpp`)**, **multi-run communication problems (`runs = N`)**, and **OI subtasks / partial scoring (`[[subtask]]`)**.
- **Core Workflow (7-Step Loop)**:
  1. **Specification & Scaffolding**: Confirm problem type, intended algorithm, constraints, time/memory limits, and judging mode; call `inspect_package` / `init_package` to initialize the package structure.
  2. **Statement & Samples**: Use `write_problem` to author `problem.toml`, `statement.md` (sectioned with `##` headings, adding `## Interaction` for interactive problems and `## Subtasks` for OI problems), and `samples/01..NN.{in,ans}`.
  3. **Validator, Checker, Interactor & Self-Tests**: Implement `validator.cpp` using `testlib.h` (with `--group` subtask validation where applicable), write `interactor.cpp` or a custom `checker.cpp` as needed, and populate valid/invalid self-test directories.
  4. **Reference Solution, Brute Force & Stress Testing**: Write `std.cpp` and an independent `brute.cpp` (or partial solution), then run `stress_package` across diverse test shapes; counterexamples are saved automatically to `tests/stress-<n>.in`.
  5. **Adversarial Generators & Wrong-Solution Matrix**: Guided by `mistake-taxonomy.md`, build structured generators and targeted wrong solutions (WA / TLE / ILE / partial-score solutions), verifying with `build_package(mode: "debug")` that every wrong solution fails as expected.
  6. **Strict Verification & Editorial**: Run full `build_package(mode: "verify")` to satisfy all hard gates (including slowest `std` runtime `≤ 50%` of time limit and acyclic subtask dependencies), then write `editorial.md` matching `std.cpp`.
  7. **Typesetting & Visual Inspection**: Run `compile_and_diagnose` to ensure zero statement/editorial errors or page overflows, then inspect formulas, sample tables, and pagination with `render_page_png`.
- **Built-in Guardrails**: Strictly forbids silently relaxing `[limits]`, deleting failing tests or loosening `expect`/`score` to turn checks green, writing stub validators, silently overwriting hand-crafted sample outputs, or substituting `debug` mode for full `verify` acceptance.

### 2. `xcpc-trd-curator` — Algorithm Snippets & Team Reference Document (TRD)

- **Scope**: Curating and refactoring verifiable Algorithm Snippets (`snippets/`), maintaining cross-snippet dependencies and cascade verification, bundling single-file C++ for OJ submission, and composing printable Team Reference Documents (`trds/`).
- **Core Workflow**:
  1. **Add & Verify Snippets**: Check for duplicates via `search_snippets`, then use `write_snippet` to author valid C++ in `code.hpp` (starting with `#pragma once`, declaring dependencies directly via `#include "<slug>/code.hpp"` without maintaining a separate dependency list) and `notes.md` (also supporting code-free formula cards). Pair with `self` / `problem` / `library_checker` verification cases and confirm `passed` via `verify_snippet`.
  2. **Refactor & Cascade Regression**: Before refactoring a foundational snippet, inspect `reverse_deps` and its cumulative short hash with `get_snippet`. After editing, always run `verify_snippet(slug, cascade: true)` to regress all downstream dependents; never delete failing `[[verify]]` entries to hide regressions.
  3. **Single-File Bundling**: Expand modular snippets recursively into a single self-contained C++ file via `bundle_code` (or `xcpc bundle`), with optional compact comment-stripping mode.
  4. **Compose & Finalize**: Arrange chapters and choose a template (`trd-compact` landscape multi-column / `trd-book` portrait handbook) with `compose_trd`, run `compile_and_diagnose(target: "trd")` to catch missing transitive or cyclic dependencies, spot-check the table of contents and cumulative hash margin with `render_page_png`, and lock the snapshot with `freeze_trd(strict: true)`.

### 3. `xcpc-plugin-creator` — Hybrid Sandbox Plugin Development

- **Scope**: Designing, implementing, testing, hot-reload debugging, and packaging XCPC plugins across **OJ Data Sources (`oj_source`)**, **Headless Form Tools (`tool`)**, and **Web `<WebView>` Interactive Tools (`tool` + `commands`)**.
- **Core Workflow (6-Step Loop)**:
  1. **Scaffold**: Choose `xcpc plugin init <dir> --kind <tool|oj> --vite` (recommended, using `@xcpc-workstation/plugin-sdk` and Vite single-file inline bundling) or zero-build plain files depending on complexity.
  2. **Implement `manifest.toml` & QuickJS Logic**: Follow `manifest.schema.json` and the principle of least privilege (declare only required `domains`, `credentials`, and `workspace_read`), implementing exported handlers in `src/index.ts` / `index.js` using `host.*` APIs.
  3. **Implement Web UI (Optional)**: Build a single-file self-contained `ui.html` consuming host `--xcpc-*` theme variables and subscribing to `window.xcpc.onThemeChange`, delegating all external network requests to `window.xcpc.runTool` and persisting state through `window.xcpc.storage`.
  4. **Build & Sandbox Validation**: Run `npm run build`, then `xcpc plugin check dist` (static validation and QuickJS export dry run) and `xcpc plugin test dist <action> --args ...`.
  5. **Desktop Hot-Reload Debugging**: Link with `xcpc plugin link dist` (run `xcpc plugin trust <id>` if sensitive permissions are declared) alongside `npx vite build --watch`, verifying light/dark theme switching, state restoration, and host bridge calls in the desktop app.
  6. **Package & Cold-Install Acceptance**: Build a SHA-256-verified archive with `xcpc plugin pack dist`, unlink the development copy, and perform a clean install check with `xcpc plugin install <.zip>`.
- **Built-in Security Guardrails**: Forbids direct network requests or external CDNs inside WebViews, forbids assuming `allow-same-origin` or accessing `localStorage` / `window.nsh`, forbids Node.js or DOM APIs inside QuickJS, and forbids delivering a plugin without passing `xcpc plugin check` and `xcpc plugin test`.

---

## Recommended Combinations

| Goal | Recommended Stack |
| :--- | :--- |
| End-to-end problem authoring & verification | `xcpc mcp` + `xcpc-problem-creator` Skill + [Problem Authoring & Export](../problem-build/index.md) |
| Algorithm Snippet curation & TRD publishing | `xcpc mcp` + `xcpc-trd-curator` Skill + [Algorithm Snippets & TRD](../templates-trd/index.md) |
| Building OJ data sources or interactive tool plugins | `xcpc plugin` CLI + `xcpc-plugin-creator` Skill + [Plugin Development Guide](../plugins/development.md) |
