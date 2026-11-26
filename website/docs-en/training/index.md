---
title: Training Data Sync & Storage Isolation
---

# Training Data Sync & Storage Isolation

The training module in XCPC Workstation is designed for long-term personal practice and contest preparation. It follows a **shared cross-workspace external OJ cache + workspace-local log derivation** model: submissions and rating history from platforms such as Codeforces and AtCoder are aggregated automatically, while personal training status is never written into problem content files (such as `problem.toml`), keeping your problem library clean for version control and collaboration.

---

## Multi-Platform Incremental Sync

Training data synchronization is powered by built-in and third-party `oj_source` capability plugins. Two official OJ data source plugins are built in:

- **`codeforces`**: Syncs account profile, contest rating history, submissions, and contest schedule.
- **`atcoder`**: Syncs account profile, contest rating history, submissions, and contest schedule.

To connect additional platforms, install a community `oj_source` plugin or build your own (see [Plugin Ecosystem & Development](../plugins/index.md)).

### Managing Account Handles

Each platform maintains one primary account handle, stored in the global configuration file `<dataDir>/sync_config.json`. You can configure handles from the **Training** onboarding view, under **Settings → Plugins**, or via the CLI:

```bash
# View the currently configured handle
xcpc sync handle codeforces

# Set Codeforces and AtCoder handles
xcpc sync handle codeforces tourist
xcpc sync handle atcoder tourist

# Clear the handle for a platform
xcpc sync handle codeforces --clear
```

!!! info "Automatic Cleanup When Switching or Clearing Handles"
    When you change or clear a platform handle in the desktop app, the sync engine immediately clears that platform's personal cached records in `<dataDir>/oj_sync.db` (account profile, rating history, submissions, and summary cache) and resets its incremental cursor `sync_cursor`, while preserving public contest schedules (`oj_contests`) and problem metadata (`oj_problem_meta`) so records from different accounts never mix. If you switch to a new handle via CLI (`xcpc sync handle`) or by editing `sync_config.json` directly, the next `xcpc sync` run detects the account change and performs the same cleanup automatically.

### Incremental Sync & Full Re-sync

Once handles are configured, run incremental sync:

```bash
# Incrementally sync all enabled OJ platforms with configured handles (and fetch schedules)
xcpc sync

# Sync a specific platform only
xcpc sync --platform codeforces

# Ignore the incremental cursor and force a full re-sync
xcpc sync --full
xcpc sync --platform atcoder --full
```

- **Cursor-based incremental fetch**: After each sync, the plugin persists its incremental cursor (`sync_cursor`) in plugin-scoped KV storage so subsequent runs fetch only new submissions, minimizing network overhead.
- **Account-free contest schedules**: Even if no handle is configured for a platform, `xcpc sync` still makes a best-effort attempt to fetch upcoming contests for every enabled `oj_source` plugin.
- **Startup auto-sync & per-platform isolation**: `"auto_sync_on_startup": true` is enabled by default in `sync_config.json`. Errors are isolated per platform during multi-platform sync, so a timeout or rate limit on one platform does not interrupt others.

---

## Storage Architecture & Status Derivation

The training subsystem strictly separates **cross-workspace personal cache**, **workspace-local logs**, and **problem library content files**:

| Storage Location | Scope | Format | Contents & Role | Tracked in Library Git |
| :--- | :--- | :--- | :--- | :---: |
| `<dataDir>/sync_config.json` | Global (shared across workspaces) | JSON | Per-platform handles, `auto_sync_on_startup`, `last_synced_at` timestamp | No |
| `<dataDir>/oj_sync.db` | Global (shared across workspaces) | SQLite (WAL mode) | Rebuildable external OJ cache: accounts, rating history, submissions, problem metadata, contest schedules | No |
| `<workspace>/training/logs.jsonl` | Current workspace | JSON Lines (append-only) | Manual status updates, standalone offline/external practice entries, and notes | Optional |
| `<workspace>/problems/<id>/problem.toml` | Current workspace | TOML | Problem metadata and external identities (`[[source]]`); **never stores training status** | Yes |

### Global Data Directory (`<dataDir>`) Resolution

Both the desktop app and the `xcpc` CLI share the same `<dataDir>` (application ID `dev.xcpc.workstation`) so sync results are immediately available across both interfaces. Resolution order:

1. If the environment variable `XCPC_DATA_DIR` is set and non-empty, use that path.
2. Otherwise, use the operating system's standard application data directory:
   - **Linux / other Unix**: `$XDG_DATA_HOME/dev.xcpc.workstation` (falls back to `$HOME/.local/share/dev.xcpc.workstation` if unset)
   - **Windows**: `%APPDATA%\dev.xcpc.workstation`
   - **macOS**: `$HOME/Library/Application Support/dev.xcpc.workstation`

!!! note "Why is `oj_sync.db` stored globally instead of inside a workspace?"
    A competitor typically maintains multiple workspaces (such as personal libraries for different seasons, team authoring workspaces, or shared problem lists). Placing `oj_sync.db` in the user-level `<dataDir>` allows any workspace to immediately display your solve status whenever a problem's `[[source]]` matches an external submission, without placing large binary cache files inside workspace directories.

### Runtime Status Derivation Rules

A problem's effective training status (`none` / `todo` / `attempted` / `ac` / `upsolved`) is **not persisted in `problem.toml`**. Instead, it is derived dynamically at runtime by merging two sources:

1. **External OJ sync status**: Matches the problem's `[[source]]` (`platform` and `id`, case-insensitive) against submissions in `oj_sync.db` to determine whether an external `AC` or `attempted` record exists.
2. **Workspace-local log**: Scans `training/logs.jsonl` chronologically for the latest manual entry matching the current `problem_id` or `[[source]]` (`clear` removes any prior manual override).

The merge precedence is **`ac` / `upsolved` > `attempted` > `todo` > `none`**:

| External OJ Sync Status | Latest Local Manual Entry | Effective Derived Status |
| :--- | :--- | :--- |
| External `AC` exists | `upsolved` | `upsolved` |
| External `AC` exists | Any other value or none | `ac` |
| External `attempted` only (no AC) | `ac` or `upsolved` | Manual value (`ac` / `upsolved`) |
| External `attempted` only (no AC) | `none` / `todo` / `attempted` | `attempted` |
| No external record | Any value | Manual value (`none` if absent) |

---

## Disabling the Training Module per Workspace

In dedicated authoring workspaces, team collaboration repositories, or workspaces where training tracking is not needed, you can disable the training module in the workspace root `workspace.toml`:

```toml
# workspace.toml
training = false
```

- When `training` is omitted or set to `true`, the training module is enabled by default.
- When set to `training = false`, the desktop sidebar hides the **Training** space and the problem library hides personal training status badges. This setting applies only to the current workspace and does not affect the global personal cache in `<dataDir>/oj_sync.db`.

---

## Next Steps

- Explore the four dashboard views, interactive filters, and manual logging: [Dashboard Views & Manual Logging](./dashboard.md)
- Reference all `xcpc sync` and `xcpc training` commands: [CLI Reference](../cli-mcp/cli.md)
