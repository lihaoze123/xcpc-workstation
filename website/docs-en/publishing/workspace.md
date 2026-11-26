# Workspace & Open Directory

XCPC Workstation uses an **open directory** as its Single Source of Truth. A workspace is an ordinary local folder where all problems, contests, templates, and training logs are stored as human-readable Markdown and TOML files, with no proprietary binary database.

## Directory Layout

A typical workspace root (identified by the presence of `workspace.toml`) has the following layout:

```text
my-xcpc/
├── workspace.toml            # Workspace config (default template, template options, groups, feature toggles)
├── .gitignore                # Automatically ignores .xcpc/ and build/ on init
├── AGENTS.md                 # Workspace-level AI Agent collaboration guide (written on init, user-maintained)
├── templates/                # Workspace Typst templates (<name>.typ, directly editable and Git-tracked)
│   ├── olymp-classic.typ
│   └── preamble.typ          # Optional: shared custom macros and styles injected across templates
├── assets/                   # Workspace-level shared static assets (e.g., institution logos, cover badges)
├── problems/                 # Problem library: one subfolder per problem, folder slug is the problem ID
│   └── lantern-street/
│       ├── problem.toml      # Problem metadata, time/memory limits, [[source]], package declarations
│       ├── statement.md      # Statement body (or statement.zh.md / statement.en.md for bilingual)
│       ├── editorial.md      # Editorial body (typeset into A4 editorial booklet)
│       ├── samples/          # Sample I/O directory (01.in, 01.ans, 02.in, 02.ans ...)
│       ├── assets/           # Problem-specific images
│       ├── validator.cpp     # Optional: authoring files (validator / checker / interactor)
│       ├── checker.cpp
│       ├── generators/       # Optional: test data generators
│       ├── solutions/        # Optional: reference and test solutions
│       └── tests/            # Optional: hand-written tests and self-tests
├── contests/                 # Contests directory
│   └── welcome-round/
│       ├── contest.toml      # Contest metadata, live problem references, metadata overrides, template options
│       ├── template.typ      # Optional: contest-specific template override
│       ├── assets/           # Optional: contest-specific images or cover logos
│       └── snapshot/         # Present only when finalized (frozen): source copies + template + PDFs (Git-tracked)
├── snippets/                 # Optional: algorithm snippets library (see Algorithm Snippets & TRD)
├── trds/                     # Optional: Team Reference Documents (TRD) directory
├── training/
│   └── logs.jsonl            # Append-only local training log (Git-tracked)
├── build/                    # Exported PDFs, PNGs, OJ ZIPs, and generated test data (not in Git)
└── .xcpc/                    # Local derived workspace data (always in .gitignore, never in Git)
```

## `workspace.toml` Configuration

`workspace.toml` may be empty or declare workspace-level defaults as needed:

```toml
# Workspace default publishing template (maps to templates/<name>.typ)
template = "olymp-classic"

# Declared persistent problem groups (ordered)
groups = ["Regional Candidates", "Daily Practice", "Fundamentals"]

# Optional: set to false in collaborative contest repos to hide the Training navigation item
training = true

# Optional: set to false to hide the Snippets navigation item
snippets = true

# Workspace-level template default options (namespaced by template ID)
[template-options.olymp-classic]
cover = "auto"
show-metadata = true
blank-pages = false
```

All TOML writes performed by the desktop app or CLI use a format-preserving editor (`toml_edit`), retaining your hand-written comments, key ordering, and unknown fields.

## Single Source of Truth & Cache Isolation

To keep workspaces clean across Git collaboration and multi-device syncing, XCPC Workstation strictly separates **project content** from **personal or derived data**:

| Data Category | Location | Git-Tracked | Notes |
| :--- | :--- | :---: | :--- |
| Problems, contests, templates, snippets, TRDs, training logs | Inside workspace | **Yes** | Single source of truth; directly editable in external editors with live reload in the desktop app. |
| Finalized contest snapshots (`contests/<id>/snapshot/`) | Inside workspace | **Yes** | Locks all source files, active template, and PDFs at finalization for permanent reproducibility. |
| Build artifacts (`build/`) | Inside workspace | **No** | Exported temporary PDFs, PNGs, OJ package ZIPs, and generated test data. |
| Local workspace derived directory (`.xcpc/`) | Inside workspace | **No** | Added to `.gitignore` by `xcpc init`; never committed to the repository. |
| Global appearance, OJ sync DB, plugins, build cache | User global data/cache dir (`<dataDir>` / `~/.cache/xcpc/`) | **No** | Includes `<dataDir>/theme.toml`, `<dataDir>/oj_sync.db`, `<dataDir>/plugins/`, OS keychain credentials, and the three-tier build cache shared across workspaces. |

!!! note "Training Status Never Pollutes Library Files"
    Per-problem training status (`ac` / `upsolved` / `attempted` / `todo`) is **never written into `problem.toml`**. It is derived at runtime by merging `training/logs.jsonl` with the global `<dataDir>/oj_sync.db`. Sharing a problem library across a team will never cause Git merge conflicts over personal practice history.

## Concurrent Write Protection & External Editors

- **Atomic Writes**: Every file write inside the workspace performed by the Core uses a temporary file followed by an atomic `rename`, preventing partial writes or corruption on interruption.
- **Conflict Detection**: Before saving, the desktop app and MCP server compare the file's content hash against its last-read state. If an external tool modified the file on disk, the Core reports a conflict instead of silently overwriting changes.
- **Live File Watching**: While a workspace is open, the desktop app recursively watches workspace files (ignoring `build/`, `.xcpc/`, `.git/`, `.jj/`, and atomic temporary files), covering `problems/`, `contests/`, `snippets/`, `trds/`, `workspace.toml`, and `templates/*.typ`. Whether you edit statements in VS Code / Vim or let an AI Agent update files via `xcpc mcp`, previews and template options refresh automatically without flash.

## Team Collaboration with Git

XCPC Workstation does not use cloud accounts or an online collaboration server. For team problem setting, create a dedicated Git repository per contest or problem-setting committee:

1. **Initialize the Contest Repo**: The head setter runs `xcpc init my-contest-2026`, sets `training = false` in `workspace.toml` to hide personal training views, and installs the required template via `xcpc template add`.
2. **Assign Independent Directories**: Each problem author works in their own `problems/<slug>/` directory. Because a problem's folder slug is fixed upon creation, directory-level merge conflicts are naturally avoided.
3. **Standardize Fonts & CI Gates**: Agree on a shared `XCPC_FONT_DIRS` setup and run `xcpc check contest <id>` and `xcpc package build <slug> --strict` in CI so every Pull Request passes typesetting and test-data verification before merging.
4. **Finalize & Archive**: Once final proofreading is complete, run `xcpc contest freeze <id>` (or click **Finalize** in the desktop app) on the main branch, commit `contests/<id>/snapshot/` along with the final PDFs, and tag the release in Git.
