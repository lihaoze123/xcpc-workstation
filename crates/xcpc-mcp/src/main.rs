//! `xcpc`: the headless entry point, a thin shell over the Core crates.
//!
//! - `xcpc init [dir]`: initialize a new workspace (never overwrites existing `AGENTS.md`);
//! - `xcpc build|check|render <problem|contest> <id>`: publish + preflight lint (`xcpc-lint`);
//! - `xcpc search [query] ...`: scan the workspace index and search problem metadata;
//! - `xcpc show <problem|contest> <id>`: inspect problem details or contest pages & stats;
//! - `xcpc contest compose|override|freeze|unfreeze`: compose contests, manage overrides & snapshots;
//! - `xcpc list`, `xcpc fonts`;
//! - `xcpc mcp`: the same Core over MCP (stdio), without unfreeze.
//!
//! With `--json`, stdout carries one JSON object. Exit codes:
//! - 0: success
//! - 1: document or lint error
//! - 2: usage, workspace, I/O, frozen, conflict, or unconfirmed error

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::json;
use xcpc_convert::publish::{self, Kind, Report, Target};
use xcpc_domain::{
    BatchItemStatus, BatchReport, ContestMetaPatch, DiffKind, GroupFilter, Language, LimitsPatch,
    OverridePatch, Patch, ProblemSource, RatingTier, SearchQuery, SyncedProblemStatus,
    TrainingLogEntry, TrainingLogStatus,
};
use xcpc_engine::{Engine, FontOptions};
use xcpc_storage::{CreateProblemOptions, Workspace, WorkspaceIndex};

mod mcp;
mod views;

#[derive(Parser)]
#[command(name = "xcpc", version, about = "XCPC Workstation command line")]
struct Cli {
    /// Print one machine-readable JSON object on stdout.
    #[arg(long, global = true)]
    json: bool,
    /// The workspace (default: $XCPC_WORKSPACE, else the one the current directory is in).
    #[arg(long, short, global = true)]
    workspace: Option<PathBuf>,
    /// Refuse every write to the workspace.
    #[arg(long, global = true)]
    read_only: bool,
    /// Also use the operating system's fonts (after XCPC_FONT_DIRS).
    #[arg(long, global = true)]
    system_fonts: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize a new XCPC workspace (creates workspace.toml, problems/, contests/, .gitignore, AGENTS.md).
    Init {
        /// Populate the workspace with the built-in example contest and problems.
        #[arg(long)]
        example: bool,
        /// Language for `--example` (`chinese`/`zh` or `english`/`en`; defaults to `chinese`).
        #[arg(long, alias = "language")]
        lang: Option<Lang>,
        /// Copy built-in problem authoring skills into `.agents/skills/xcpc-problem-creator/`.
        #[arg(long)]
        with_skills: bool,
        /// Target directory (defaults to current directory `.`).
        dir: Option<PathBuf>,
    },
    /// Typeset a problem, a contest, or an ad-hoc merged booklet of problems and write its PDF.
    Build {
        /// What to typeset: `problem`, `contest`, or `problems` (merged booklet).
        kind: BuildKind,
        /// Problem ID, contest ID, or one or more problem IDs (when kind is `problems`).
        #[arg(value_name = "ID", required = true)]
        ids: Vec<String>,
        /// The editorial book instead of the statements.
        #[arg(long)]
        editorial: bool,
        /// The language to set the problem(s) in (a contest has its own).
        #[arg(long, alias = "lang")]
        language: Option<Lang>,
        /// Booklet title (for `build problems`).
        #[arg(long)]
        title: Option<String>,
        /// Show cover page (`true` or `false`; for `build problems`).
        #[arg(long)]
        cover: Option<bool>,
        /// Write the PDF here (default: build/<title>.pdf in the workspace).
        #[arg(long = "output", alias = "out", short = 'o')]
        out: Option<PathBuf>,
        /// Also write the generated Typst source here.
        #[arg(long)]
        emit_typst: Option<PathBuf>,
    },
    /// Typeset, run preflight lint, and report diagnostics, pages and overflows; write nothing.
    Check {
        #[command(flatten)]
        what: What,
    },
    /// Typeset and render pages to PNG.
    Render {
        #[command(flatten)]
        what: What,
        /// Output directory (default: build/<id>/ in the workspace); pages are
        /// `page-<n>.png` (1-based).
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// Pixels per point (2.0 ≈ 144 dpi).
        #[arg(long, default_value_t = 2.0)]
        scale: f32,
        /// Only this page (1-based).
        #[arg(long)]
        page: Option<usize>,
    },
    /// Search problems in the workspace by text, tags, rating range, source platform, or group.
    Search {
        /// Free-text query matched against ID, title, tags, sources, statement, and editorial.
        #[arg(value_name = "QUERY")]
        query: Vec<String>,
        /// Require a tag (repeatable; problem must have all specified tags).
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Minimum rating (inclusive).
        #[arg(long)]
        rating_min: Option<u32>,
        /// Maximum rating (inclusive).
        #[arg(long)]
        rating_max: Option<u32>,
        /// Filter by external source platform (e.g. codeforces, atcoder, luogu).
        #[arg(long)]
        platform: Option<String>,
        /// Filter by persistent group name (conflicts with `--ungrouped`).
        #[arg(long, conflicts_with = "ungrouped")]
        group: Option<String>,
        /// Only show ungrouped problems (conflicts with `--group`).
        #[arg(long)]
        ungrouped: bool,
    },
    /// Manage persistent problem groups (list, create, rename, delete, reorder).
    Group {
        #[command(subcommand)]
        action: GroupCommand,
    },
    /// Show detailed metadata, content hashes, pages, and statistics for a problem or contest.
    Show { kind: TargetKind, id: String },
    /// Manage problems (duplicate, batch group/tag/rating updates, delete).
    Problem {
        #[command(subcommand)]
        action: ProblemCommand,
    },
    /// Compose contests, manage metadata overrides, freeze snapshots, unfreeze, and archive.
    Contest {
        #[command(subcommand)]
        action: ContestCommand,
    },
    /// Manage local Polygon problem packages (scaffold, build/verify, stress test).
    Package {
        #[command(subcommand)]
        action: PackageCommand,
    },
    /// Import problems or contests from external archives (e.g. Polygon ZIP).
    Import {
        #[command(subcommand)]
        action: ImportCommand,
    },
    /// Export verified problem(s) or contest(s) to OJ packages (DOMjudge or Hydro).
    Export {
        /// Target OJ format (`domjudge` or `hydro`).
        format: ExportFormat,
        /// Export a single `problem`, an entire `contest`, or a batch of `problems`.
        kind: ExportKind,
        /// Problem ID, contest ID, or one or more problem IDs (when kind is `problems`).
        #[arg(value_name = "ID", required = true)]
        ids: Vec<String>,
        /// Output `.zip` path (for problem) or directory (for contest / problems). Defaults under `build/`.
        #[arg(long = "output", alias = "out", short = 'o')]
        out: Option<PathBuf>,
        /// Bypass strict verification check (built testdata is still required).
        #[arg(long)]
        force: bool,
    },
    /// Inspect built-in and custom Typst templates and their declarative schemas.
    Template {
        #[command(subcommand)]
        action: TemplateCommand,
    },
    /// Manage plugins (list, install, enable/disable, export, init, test, repo).
    Plugin {
        #[command(subcommand)]
        action: PluginCommand,
    },
    /// Sync OJ accounts, submissions, and contests via enabled oj_source plugins, or manage handles.
    Sync {
        #[command(subcommand)]
        action: Option<SyncCommand>,
        /// Sync only this platform ID (e.g. codeforces, atcoder).
        #[arg(long)]
        platform: Option<String>,
        /// Ignore stored incremental cursor and run a full sync.
        #[arg(long)]
        full: bool,
    },
    /// Manage personal training status logs and derived status.
    Training {
        #[command(subcommand)]
        action: TrainingCommand,
    },
    /// List the workspace's problems and contests.
    List,
    /// List the font families the engine sees.
    Fonts,
    /// Serve MCP on stdin and stdout.
    Mcp,
}

#[derive(Subcommand)]
enum GroupCommand {
    /// List persistent problem groups and ungrouped count.
    List,
    /// Create a new declared group in `workspace.toml`.
    Create {
        /// Group name (1..=64 chars, no control characters).
        name: String,
    },
    /// Rename a group in `workspace.toml` and all member problems.
    Rename {
        /// Existing group name.
        old_name: String,
        /// New group name.
        new_name: String,
    },
    /// Delete a group (moves member problems to ungrouped without deleting problems).
    Delete {
        /// Group name to delete.
        name: String,
    },
    /// Reorder declared groups in `workspace.toml` (must be a strict permutation).
    Reorder {
        /// Complete ordered list of declared group names.
        #[arg(value_name = "GROUP")]
        order: Vec<String>,
    },
}

#[derive(Subcommand)]
enum ProblemCommand {
    /// Duplicate an existing problem directory (`problems/<source_id>/`) into `problems/<new_id>/`.
    Duplicate {
        /// Source problem ID.
        source_id: String,
        /// New problem ID (slug under problems/).
        new_id: String,
        /// Optional new title (defaults to source problem's title).
        #[arg(long)]
        title: Option<String>,
    },
    /// Batch-assign problems to a persistent group, or clear their group with `--clear`.
    Group {
        /// Clear the group assignment from the specified problems.
        #[arg(long)]
        clear: bool,
        /// Group name followed by problem IDs (or only problem IDs when `--clear` is set).
        #[arg(value_name = "ARGS", required = true)]
        args: Vec<String>,
    },
    /// Batch-add and/or remove tags across one or more problems.
    Tag {
        /// Tag(s) to add (repeatable or comma-separated).
        #[arg(long = "add", value_name = "TAG", value_delimiter = ',')]
        add: Vec<String>,
        /// Tag(s) to remove (repeatable or comma-separated).
        #[arg(long = "remove", value_name = "TAG", value_delimiter = ',')]
        remove: Vec<String>,
        /// Problem IDs.
        #[arg(value_name = "ID", required = true)]
        ids: Vec<String>,
    },
    /// Batch-set a difficulty rating across problems, or clear it with `--clear`.
    Rating {
        /// Clear the rating from the specified problems.
        #[arg(long)]
        clear: bool,
        /// Rating integer followed by problem IDs (or only problem IDs when `--clear` is set).
        #[arg(value_name = "ARGS", required = true)]
        args: Vec<String>,
    },
    /// Batch-delete one or more problems (skips problems referenced by unfrozen contests).
    Delete {
        /// Problem IDs to delete.
        #[arg(value_name = "ID", required = true)]
        ids: Vec<String>,
        /// Skip interactive confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Create a problem from an external OJ URL (deduplicating by `[[source]]`).
    FromUrl {
        /// External OJ problem URL (e.g. Codeforces or AtCoder problem URL).
        url: String,
        /// Override target problem ID (defaults to suggested slug such as `cf-1999a`).
        #[arg(long)]
        id: Option<String>,
        /// Override problem title (defaults to resolved title or `<platform> <problem_key>`).
        #[arg(long)]
        title: Option<String>,
    },
}

#[derive(Subcommand)]
enum PackageCommand {
    /// Inspect a local Polygon package (read-only: files, subtasks, freshness, latest report).
    Inspect {
        /// Problem ID (slug under problems/).
        id: String,
    },
    /// Scaffold a local Polygon package (validator, generator, std, validator self-tests) without overwriting existing files.
    Init {
        /// Problem ID (slug under problems/).
        id: String,
        /// Generate interactive problem scaffold (`interactor.cpp` and interactive `std.cpp`).
        #[arg(long)]
        interactive: bool,
        /// Multi-run communication pass count (`>= 2` writes `[package].runs = N`).
        #[arg(long)]
        runs: Option<u32>,
        /// Generate OI `[[subtask]]` scaffold in `problem.toml`.
        #[arg(long)]
        subtasks: bool,
    },
    /// Compile, generate tests, validate, and run solutions against the checker.
    Build {
        /// One or more problem IDs (slugs under problems/).
        #[arg(value_name = "ID", required = true)]
        ids: Vec<String>,
        /// Run full strict acceptance verification (all tests × all solutions, all hard gates).
        #[arg(long, conflicts_with = "sanitize")]
        strict: bool,
        /// Run isolated ASan/UBSan undefined-behavior check across generators, validator, checker, interactor, and expected-AC solutions.
        #[arg(long, conflicts_with = "strict")]
        sanitize: bool,
        /// Subset of tests to run (1-based index, name, or tag; comma-separated or repeatable; debug/sanitize mode).
        #[arg(long, value_delimiter = ',')]
        tests: Vec<String>,
        /// Subset of solution names to run (comma-separated or repeatable; debug/sanitize mode).
        #[arg(long, value_delimiter = ',')]
        solutions: Vec<String>,
        /// Parallel worker count (overrides both tool and solution concurrency).
        #[arg(long, short = 'j')]
        jobs: Option<usize>,
        /// Check under this contest's overridden time and memory limits.
        #[arg(long, value_name = "CONTEST", conflicts_with = "sanitize")]
        contest: Option<String>,
    },
    /// Run bounded differential stress testing against `std` and save the first counterexample to `tests/stress-<n>.in`.
    Stress {
        /// Problem ID (slug under problems/).
        id: String,
        /// Generator name under `generators/<name>.cpp`.
        #[arg(long = "gen")]
        generator: String,
        /// Generator arguments template (`{i}` or `{seed}` replaced by iteration number).
        #[arg(long, default_value = "", allow_hyphen_values = true)]
        args: String,
        /// Target solution names to compare against `std` (comma-separated or repeatable; defaults to all non-main solutions).
        #[arg(long, value_delimiter = ',')]
        solutions: Vec<String>,
        /// Maximum number of iterations.
        #[arg(long, default_value_t = 100)]
        max_iter: u32,
        /// Time budget in seconds.
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Evaluate one or more external `.cpp` files against a problem's built test data without modifying the workspace.
    VerifyExternal {
        /// Problem ID (slug under problems/).
        id: String,
        /// External `.cpp` solution file(s) to evaluate.
        #[arg(value_name = "FILE", required = true)]
        files: Vec<PathBuf>,
        /// Automatically build test data in debug mode if not yet built.
        #[arg(long)]
        auto_build: bool,
        /// Parallel worker count (used when `--auto-build` triggers a build).
        #[arg(long, short = 'j')]
        jobs: Option<usize>,
        /// Also export a Markdown report to this file path.
        #[arg(long)]
        markdown: Option<PathBuf>,
    },
    /// Atomically adopt an external `.cpp` solution into `problems/<id>/solutions/` and register it in `problem.toml`.
    Adopt {
        /// Problem ID (slug under problems/).
        id: String,
        /// External `.cpp` file to adopt.
        file: PathBuf,
        /// Target solution name under `solutions/<name>.cpp` (defaults to file stem).
        #[arg(long)]
        name: Option<String>,
        /// Expected verdict(s) (comma-separated or repeatable, e.g. `AC` or `WA,TLE`; defaults to `AC`).
        #[arg(long, value_delimiter = ',')]
        expect: Vec<String>,
        /// Optional test tag filter(s) (comma-separated or repeatable).
        #[arg(long = "tag", value_delimiter = ',')]
        tags: Vec<String>,
        /// Mark the adopted solution as `main = true`.
        #[arg(long)]
        main: bool,
    },
}

#[derive(Subcommand)]
enum ImportCommand {
    /// Import a Polygon problem or contest ZIP archive into the workspace.
    Polygon {
        /// Path to the Polygon `.zip` archive.
        zip: PathBuf,
        /// Override target problem slug or contest ID.
        #[arg(long)]
        id: Option<String>,
        /// Assign imported problem(s) to this persistent group (overrides auto-grouping).
        #[arg(long)]
        group: Option<String>,
        /// Disable automatic grouping by contest name for Polygon contest imports.
        #[arg(long)]
        no_auto_group: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ExportFormat {
    #[value(alias = "dj")]
    Domjudge,
    #[value(alias = "hydrooj")]
    Hydro,
}

impl ExportFormat {
    fn into_oj_format(self) -> xcpc_packages::OjFormat {
        match self {
            ExportFormat::Domjudge => xcpc_packages::OjFormat::Domjudge,
            ExportFormat::Hydro => xcpc_packages::OjFormat::Hydro,
        }
    }
}

#[derive(Subcommand)]
enum TemplateCommand {
    /// List workspace templates (`templates/*.typ`) and available preset seeds.
    List,
    /// Show the parsed declarative `#let schema` of one workspace template.
    Schema {
        /// Template ID under `templates/<name>.typ`.
        name: String,
    },
    /// Install a bundled preset seed (`olymp-classic`, `cnoi-classic`, `minimal`) into `templates/`.
    Add {
        /// Preset seed ID (`olymp-classic`, `cnoi-classic`, or `minimal`).
        preset: String,
        /// Target template ID under `templates/<name>.typ` (defaults to preset ID).
        name: Option<String>,
    },
    /// Fork an existing workspace template into `templates/<new_name>.typ`.
    Fork {
        /// Source workspace template ID.
        source: String,
        /// New template ID (slug under templates/).
        new_name: String,
    },
}

#[derive(Subcommand)]
enum PluginCommand {
    /// List all built-in and locally installed plugins.
    List,
    /// Install a plugin from a local directory or `.zip` archive.
    Install {
        /// Path to a plugin directory or `.zip` archive.
        path: PathBuf,
    },
    /// Enable a plugin by ID.
    Enable {
        /// Plugin ID.
        id: String,
    },
    /// Disable a plugin by ID.
    Disable {
        /// Plugin ID.
        id: String,
    },
    /// Export a built-in plugin to `<dataDir>/plugins/<id>/` for local customization.
    Export {
        /// Built-in plugin ID.
        id: String,
    },
    /// Scaffold a new plugin directory (`manifest.toml`, `index.js`, `xcpc-plugin.d.ts`).
    Init {
        /// Target directory to scaffold into.
        dir: PathBuf,
        /// Plugin template kind (`oj` or `tool`).
        #[arg(long, default_value = "oj")]
        kind: PluginScaffoldKind,
    },
    /// Run a local plugin directory directly without installing it.
    Test {
        /// Path to the local plugin directory.
        dir: PathBuf,
        /// Exported function name to invoke (defaults to `verify` for OJ plugins or `runTool` for tools).
        action: Option<String>,
        /// JSON arguments passed to the exported function (array of args, or single JSON value).
        #[arg(long)]
        args: Option<String>,
    },
    /// Manage decentralized plugin repository (`index.json`) subscriptions.
    Repo {
        #[command(subcommand)]
        action: PluginRepoCommand,
    },
}

#[derive(Subcommand)]
enum PluginRepoCommand {
    /// List configured plugin repository URLs.
    List,
    /// Add or update a plugin repository URL.
    Add {
        /// Repository `index.json` URL.
        url: String,
        /// Optional display name for the repository.
        #[arg(long)]
        name: Option<String>,
    },
    /// Remove a plugin repository URL.
    Remove {
        /// Repository URL to remove.
        url: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum PluginScaffoldKind {
    Oj,
    Tool,
}

impl PluginScaffoldKind {
    fn as_str(self) -> &'static str {
        match self {
            PluginScaffoldKind::Oj => "oj",
            PluginScaffoldKind::Tool => "tool",
        }
    }

    fn into_scaffold_kind(self) -> xcpc_plugin::ScaffoldKind {
        match self {
            PluginScaffoldKind::Oj => xcpc_plugin::ScaffoldKind::Oj,
            PluginScaffoldKind::Tool => xcpc_plugin::ScaffoldKind::Tool,
        }
    }
}

#[derive(Subcommand)]
enum SyncCommand {
    /// Get, set, or clear the configured handle for an OJ platform.
    Handle {
        /// Platform ID (e.g. codeforces, atcoder).
        platform: String,
        /// Handle to set (omit to show current handle).
        #[arg(conflicts_with = "clear")]
        handle: Option<String>,
        /// Clear the configured handle for this platform.
        #[arg(long)]
        clear: bool,
    },
}

#[derive(Subcommand)]
enum TrainingCommand {
    /// Record a manual training status change for a problem and show its derived status.
    Status {
        /// Problem ID (slug under problems/).
        problem_id: String,
        /// Status to record (`todo`, `attempted`, `ac`, `upsolved`, or `clear`).
        status: TrainingStatusArg,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum TrainingStatusArg {
    Todo,
    Attempted,
    Ac,
    Upsolved,
    Clear,
}

impl TrainingStatusArg {
    fn into_log_status(self) -> TrainingLogStatus {
        match self {
            TrainingStatusArg::Todo => TrainingLogStatus::Todo,
            TrainingStatusArg::Attempted => TrainingLogStatus::Attempted,
            TrainingStatusArg::Ac => TrainingLogStatus::Ac,
            TrainingStatusArg::Upsolved => TrainingLogStatus::Upsolved,
            TrainingStatusArg::Clear => TrainingLogStatus::Clear,
        }
    }
}

#[derive(Subcommand)]
enum ContestCommand {
    /// Create or update a contest's ordered problem list (labels A..Z) and metadata.
    Compose {
        /// Contest ID (slug under contests/).
        id: String,
        /// Ordered list of problem IDs (max 26).
        #[arg(value_name = "PROBLEM_ID")]
        problems: Vec<String>,
        /// Contest name.
        #[arg(long)]
        name: Option<String>,
        /// Contest location.
        #[arg(long)]
        location: Option<String>,
        /// Contest date.
        #[arg(long)]
        date: Option<String>,
        /// Cover author line.
        #[arg(long)]
        author: Option<String>,
        /// Contest language.
        #[arg(long)]
        language: Option<Lang>,
        /// Template ID (e.g. olymp-classic, cnoi-classic, or custom templates/<name>.typ).
        #[arg(long)]
        template: Option<String>,
        /// Show cover page (`true` or `false`).
        #[arg(long)]
        cover: Option<bool>,
        /// Start every problem on an odd page (`true` or `false`).
        #[arg(long)]
        blank_pages: Option<bool>,
        /// Show the limits table by default (`true` or `false`).
        #[arg(long)]
        show_metadata: Option<bool>,
    },
    /// Set or clear metadata overrides for one problem inside a contest.
    Override {
        /// Contest ID.
        contest: String,
        /// Problem label (e.g. A, B, C).
        label: String,
        /// Override problem title.
        #[arg(long, conflicts_with = "clear_title")]
        title: Option<String>,
        /// Clear the title override.
        #[arg(long)]
        clear_title: bool,
        /// Override time limit (e.g. "2s").
        #[arg(long, conflicts_with = "clear_time")]
        time: Option<String>,
        /// Clear the time limit override.
        #[arg(long)]
        clear_time: bool,
        /// Override memory limit (e.g. "512MB").
        #[arg(long, conflicts_with = "clear_memory")]
        memory: Option<String>,
        /// Clear the memory limit override.
        #[arg(long)]
        clear_memory: bool,
        /// Override input filename (empty string for standard input).
        #[arg(long, conflicts_with = "clear_input")]
        input: Option<String>,
        /// Clear the input filename override.
        #[arg(long)]
        clear_input: bool,
        /// Override output filename (empty string for standard output).
        #[arg(long, conflicts_with = "clear_output")]
        output: Option<String>,
        /// Clear the output filename override.
        #[arg(long)]
        clear_output: bool,
        /// Override whether to show the limits table (`true` or `false`).
        #[arg(long, conflicts_with = "clear_show_metadata")]
        show_metadata: Option<bool>,
        /// Clear the show-metadata override.
        #[arg(long)]
        clear_show_metadata: bool,
    },
    /// Validate, export snapshot PDFs, and freeze a contest into a read-only archive.
    Freeze {
        /// Contest ID.
        id: String,
    },
    /// List differences between snapshot and library, then unfreeze a contest.
    Unfreeze {
        /// Contest ID.
        id: String,
        /// Skip interactive contest ID confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Archive a contest (marks it read-only and moves it to the archived section).
    Archive {
        /// Contest ID.
        id: String,
    },
    /// Unarchive a contest, restoring its previous editable/frozen state.
    Unarchive {
        /// Contest ID.
        id: String,
    },
    /// Run a read-only contest readiness audit across all problems in the contest.
    Audit {
        /// Contest ID.
        id: String,
        /// Also export a Markdown audit report to this file path.
        #[arg(long)]
        markdown: Option<PathBuf>,
    },
    /// Evaluate a directory of external tester submissions (single-level or `<dir>/<tester>/<prob>.cpp`) against a contest without modifying the workspace.
    VerifyExternal {
        /// Contest ID.
        id: String,
        /// External submissions directory (single-level or 2-level multi-tester layout).
        path: PathBuf,
        /// Automatically build missing test data in debug mode before evaluating.
        #[arg(long)]
        auto_build: bool,
        /// Parallel worker count (used when `--auto-build` triggers a build).
        #[arg(long, short = 'j')]
        jobs: Option<usize>,
        /// Also export a Markdown report to this file path.
        #[arg(long)]
        markdown: Option<PathBuf>,
    },
}

#[derive(Args)]
struct What {
    kind: TargetKind,
    id: String,
    /// The editorial book instead of the statements.
    #[arg(long)]
    editorial: bool,
    /// The language to set a single problem in (a contest has its own).
    #[arg(long)]
    language: Option<Lang>,
}

#[derive(Clone, Copy, ValueEnum)]
enum TargetKind {
    Problem,
    Contest,
}

#[derive(Clone, Copy, ValueEnum)]
enum BuildKind {
    Problem,
    Contest,
    Problems,
}

#[derive(Clone, Copy, ValueEnum)]
enum ExportKind {
    Problem,
    Contest,
    Problems,
}

#[derive(Clone, Copy, ValueEnum)]
enum Lang {
    #[value(alias = "zh")]
    Chinese,
    #[value(alias = "en")]
    English,
}

impl Lang {
    fn into_language(self) -> Language {
        match self {
            Lang::Chinese => Language::Chinese,
            Lang::English => Language::English,
        }
    }
}

impl What {
    fn target(&self) -> Target {
        match self.kind {
            TargetKind::Problem => Target::Problem(self.id.clone()),
            TargetKind::Contest => Target::Contest(self.id.clone()),
        }
    }

    fn kind(&self) -> Kind {
        if self.editorial {
            Kind::Editorial
        } else {
            Kind::Statements
        }
    }

    fn language(&self) -> Option<Language> {
        self.language.map(Lang::into_language)
    }
}

/// An error with a machine-readable code, exit code, and optional Chinese human message.
#[derive(Debug)]
struct Fatal {
    code: &'static str,
    message: String,
    human: Option<String>,
    exit_code: u8,
}

impl From<xcpc_storage::Error> for Fatal {
    fn from(e: xcpc_storage::Error) -> Self {
        Fatal {
            code: e.code(),
            message: e.to_string(),
            human: Some(e.human_message()),
            exit_code: 2,
        }
    }
}

impl From<xcpc_judge::JudgeError> for Fatal {
    fn from(e: xcpc_judge::JudgeError) -> Self {
        Fatal {
            code: e.code(),
            message: e.to_string(),
            human: Some(e.human_message()),
            exit_code: 2,
        }
    }
}

impl From<xcpc_packages::PackageError> for Fatal {
    fn from(e: xcpc_packages::PackageError) -> Self {
        let exit_code = match &e {
            xcpc_packages::PackageError::Storage(_) | xcpc_packages::PackageError::Io { .. } => 2,
            _ => 1,
        };
        Fatal {
            code: e.code(),
            message: e.to_string(),
            human: Some(e.human_message()),
            exit_code,
        }
    }
}

impl From<xcpc_plugin::PluginError> for Fatal {
    fn from(e: xcpc_plugin::PluginError) -> Self {
        Fatal {
            code: e.code(),
            message: e.to_string(),
            human: None,
            exit_code: 2,
        }
    }
}

impl From<xcpc_sync::SyncError> for Fatal {
    fn from(e: xcpc_sync::SyncError) -> Self {
        match e {
            xcpc_sync::SyncError::Plugin(p) => p.into(),
            xcpc_sync::SyncError::PlatformNotFound(msg) => Fatal {
                code: "platform_not_found",
                message: format!("platform not found or disabled: {msg}"),
                human: Some(format!("找不到或未启用平台「{msg}」。")),
                exit_code: 2,
            },
            xcpc_sync::SyncError::InvalidDataDir(msg) => Fatal {
                code: "invalid_data_dir",
                message: format!("invalid data directory: {msg}"),
                human: Some(format!("无法定位全局数据目录：{msg}")),
                exit_code: 2,
            },
            xcpc_sync::SyncError::Io(err) => Fatal {
                code: "io",
                message: err.to_string(),
                human: None,
                exit_code: 2,
            },
            xcpc_sync::SyncError::Json(err) => Fatal {
                code: "json",
                message: err.to_string(),
                human: None,
                exit_code: 2,
            },
            xcpc_sync::SyncError::Sqlite(err) => Fatal {
                code: "sqlite",
                message: err.to_string(),
                human: None,
                exit_code: 2,
            },
        }
    }
}

impl From<anyhow::Error> for Fatal {
    fn from(e: anyhow::Error) -> Self {
        match e.downcast::<xcpc_storage::Error>() {
            Ok(e) => e.into(),
            Err(e) => Fatal {
                code: "failed",
                message: format!("{e:#}"),
                human: None,
                exit_code: 2,
            },
        }
    }
}

#[derive(Serialize)]
struct Output {
    #[serde(flatten)]
    report: Report,
    outputs: Vec<String>,
}

#[derive(Serialize)]
struct FreezeOutput {
    id: String,
    frozen: bool,
    #[serde(flatten)]
    report: Report,
    outputs: Vec<String>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => code,
        Err(e) => {
            if cli.json {
                println!(
                    "{}",
                    json!({ "ok": false, "error": { "code": e.code, "message": e.message } })
                );
            } else {
                eprintln!("error: {}", e.human.as_deref().unwrap_or(&e.message));
            }
            ExitCode::from(e.exit_code)
        }
    }
}

fn fonts(cli: &Cli) -> FontOptions {
    let mut fonts = FontOptions::from_env();
    fonts.system |= cli.system_fonts;
    fonts
}

fn open_workspace(cli: &Cli) -> Result<Workspace, Fatal> {
    let ws = match (&cli.workspace, std::env::var_os("XCPC_WORKSPACE")) {
        (Some(dir), _) => Workspace::open(dir, cli.read_only)?,
        (None, Some(dir)) => Workspace::open(dir, cli.read_only)?,
        (None, None) => Workspace::discover(".", cli.read_only)?,
    };
    Ok(ws)
}

fn flag_patch<T>(set: Option<T>, clear: bool) -> Patch<T> {
    if clear {
        Patch::Clear
    } else if let Some(v) = set {
        Patch::Set(v)
    } else {
        Patch::Keep
    }
}

fn dir_has_entries(dir: &std::path::Path) -> Result<bool, Fatal> {
    match std::fs::read_dir(dir) {
        Ok(mut entries) => Ok(entries.next().is_some()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(xcpc_storage::Error::Io {
            path: dir.display().to_string(),
            source: e,
        }
        .into()),
    }
}

fn run(cli: &Cli) -> Result<ExitCode, Fatal> {
    match &cli.command {
        Command::Init {
            example,
            lang,
            with_skills,
            dir,
        } => {
            if cli.read_only {
                return Err(xcpc_storage::Error::ReadOnly.into());
            }
            let target_dir = dir.clone().unwrap_or_else(|| PathBuf::from("."));
            if *example && dir_has_entries(&target_dir)? {
                return Err(Fatal {
                    code: "dir_not_empty",
                    message: format!(
                        "target directory {} is not empty; specify an empty directory for --example",
                        target_dir.display()
                    ),
                    human: Some(format!(
                        "目标目录「{}」非空，请指定一个空目录来初始化示例工作区。",
                        target_dir.display()
                    )),
                    exit_code: 2,
                });
            }
            let ws = if *example {
                let language = lang.map(Lang::into_language).unwrap_or(Language::Chinese);
                xcpc_storage::init_example_workspace_with_options(&target_dir, None, language)?
            } else {
                xcpc_storage::init_workspace(&target_dir)?
            };
            let installed_skills = if *with_skills {
                xcpc_storage::install_workspace_skills(ws.root())?
            } else {
                Vec::new()
            };
            let root = ws.root().display().to_string();
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "root": root,
                        "example": example,
                        "with_skills": with_skills,
                        "skills": installed_skills,
                    }))
                    .expect("serializable")
                );
            } else if *example {
                println!("initialized example workspace at {root}");
                if !installed_skills.is_empty() {
                    println!(
                        "installed {} skill file(s) into .agents/skills/xcpc-problem-creator/",
                        installed_skills.len()
                    );
                }
            } else {
                println!("initialized workspace at {root}");
                if !installed_skills.is_empty() {
                    println!(
                        "installed {} skill file(s) into .agents/skills/xcpc-problem-creator/",
                        installed_skills.len()
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Fonts => {
            let families = Engine::new(&fonts(cli), None).font_families();
            if cli.json {
                let list: Vec<_> = families
                    .iter()
                    .map(|(f, n)| json!({ "family": f, "faces": n }))
                    .collect();
                println!("{}", json!({ "ok": true, "fonts": list }));
            } else {
                for (family, faces) in families {
                    println!("{family}\t{faces}");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::List => {
            let ws = open_workspace(cli)?;
            let problems = ws.list_problems()?;
            let contests = views::list_contests(&ws)?;
            if cli.json {
                println!(
                    "{}",
                    json!({ "ok": true, "problems": problems, "contests": contests })
                );
            } else {
                for id in problems {
                    println!("problem\t{id}");
                }
                for c in contests {
                    if c.archived {
                        println!("contest\t{}\t[archived]", c.id);
                    } else {
                        println!("contest\t{}", c.id);
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Mcp => {
            let ws = open_workspace(cli)?;
            mcp::serve(ws, fonts(cli)).map_err(Fatal::from)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Build {
            kind,
            ids,
            editorial,
            language,
            title,
            cover,
            out,
            emit_typst,
        } => {
            let ws = open_workspace(cli)?;
            let doc_kind = if *editorial {
                Kind::Editorial
            } else {
                Kind::Statements
            };
            let doc_lang = language.map(Lang::into_language);
            let mut engine = publish::engine(&ws, &fonts(cli));
            match kind {
                BuildKind::Problem | BuildKind::Contest => {
                    if ids.len() != 1 {
                        return Err(Fatal {
                            code: "invalid_arguments",
                            message: "expected exactly one ID for `build problem` / `build contest`"
                                .to_owned(),
                            human: Some("`build problem` 或 `build contest` 只能指定一个 ID；批量合并请使用 `build problems`。".to_owned()),
                            exit_code: 2,
                        });
                    }
                    let id = &ids[0];
                    let target = match kind {
                        BuildKind::Problem => Target::Problem(id.clone()),
                        BuildKind::Contest => Target::Contest(id.clone()),
                        BuildKind::Problems => unreachable!(),
                    };
                    let (compiled, doc, report) =
                        publish::check(&ws, &mut engine, &target, doc_kind, doc_lang, None)?;
                    if let (Some(path), Some(doc)) = (emit_typst, doc.as_ref()) {
                        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                            std::fs::create_dir_all(dir)
                                .with_context(|| format!("creating {}", dir.display()))?;
                        }
                        xcpc_storage::write_atomic(path, doc.source.as_bytes())
                            .with_context(|| format!("writing {}", path.display()))?;
                    }
                    let mut outputs = Vec::new();
                    if let (Some(compiled), true) = (compiled, report.ok) {
                        let pdf = compiled
                            .pdf()
                            .map_err(|e| anyhow::anyhow!("PDF export failed: {}", e[0].message))?;
                        outputs.push(write_output(
                            &ws,
                            out.as_ref(),
                            &publish::default_output(&ws, &target, doc_kind),
                            &pdf,
                        )?);
                    }
                    Ok(finish(cli, Output { report, outputs }))
                }
                BuildKind::Problems => {
                    let opts = publish::MergedBookOptions {
                        title: title.clone(),
                        language: doc_lang,
                        kind: doc_kind,
                        template: None,
                        cover: *cover,
                    };
                    let unique_count = {
                        let mut seen = std::collections::HashSet::with_capacity(ids.len());
                        ids.iter().filter(|id| seen.insert(id.as_str())).count()
                    };
                    let default_rel = publish::default_merged_output(unique_count, &opts);
                    if out.is_none() {
                        ws.check_writable(&default_rel)?;
                    }
                    let doc = publish::document_problems(&ws, ids, &opts)?;
                    if let Some(path) = emit_typst {
                        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                            std::fs::create_dir_all(dir)
                                .with_context(|| format!("creating {}", dir.display()))?;
                        }
                        xcpc_storage::write_atomic(path, doc.source.as_bytes())
                            .with_context(|| format!("writing {}", path.display()))?;
                    }
                    engine.reset_files();
                    let (compiled, report) = publish::compile(&mut engine, &doc);
                    let mut outputs = Vec::new();
                    if let (Some(compiled), true) = (compiled, report.ok) {
                        let pdf = compiled
                            .pdf()
                            .map_err(|e| anyhow::anyhow!("PDF export failed: {}", e[0].message))?;
                        outputs.push(write_output(&ws, out.as_ref(), &default_rel, &pdf)?);
                    }
                    Ok(finish(cli, Output { report, outputs }))
                }
            }
        }
        Command::Check { what } => {
            let ws = open_workspace(cli)?;
            let target = what.target();
            let mut engine = publish::engine(&ws, &fonts(cli));
            let (_, _, report) = publish::check(
                &ws,
                &mut engine,
                &target,
                what.kind(),
                what.language(),
                None,
            )?;
            Ok(finish(
                cli,
                Output {
                    report,
                    outputs: Vec::new(),
                },
            ))
        }
        Command::Render {
            what,
            out,
            scale,
            page,
        } => {
            let ws = open_workspace(cli)?;
            let target = what.target();
            let mut engine = publish::engine(&ws, &fonts(cli));
            let (compiled, _, report) = publish::check(
                &ws,
                &mut engine,
                &target,
                what.kind(),
                what.language(),
                None,
            )?;
            let mut outputs = Vec::new();
            if let Some(compiled) = compiled {
                let pages: Vec<usize> = match page {
                    Some(p) => vec![p.checked_sub(1).context("pages are 1-based")?],
                    None => (0..compiled.page_count()).collect(),
                };
                let dir = match what.kind() {
                    Kind::Statements => format!("build/{}", target.id()),
                    Kind::Editorial => format!("build/{}-editorial", target.id()),
                };
                for index in pages {
                    let png = compiled
                        .render_png(index, *scale)
                        .with_context(|| format!("there is no page {}", index + 1))?;
                    let name = format!("page-{}.png", index + 1);
                    let path = out.as_ref().map(|d| d.join(&name));
                    outputs.push(write_output(
                        &ws,
                        path.as_ref(),
                        &format!("{dir}/{name}"),
                        &png,
                    )?);
                }
            }
            Ok(finish(cli, Output { report, outputs }))
        }
        Command::Search {
            query,
            tags,
            rating_min,
            rating_max,
            platform,
            group,
            ungrouped,
        } => {
            let ws = open_workspace(cli)?;
            let index = WorkspaceIndex::scan(&ws)?;
            let joined = query.join(" ");
            let text = (!joined.trim().is_empty()).then_some(joined);
            let group_filter = if *ungrouped {
                Some(GroupFilter::Ungrouped)
            } else {
                group.as_ref().map(|g| GroupFilter::Named(g.clone()))
            };
            let q = SearchQuery {
                text,
                tags: tags.clone(),
                rating_min: *rating_min,
                rating_max: *rating_max,
                platform: platform.clone(),
                group: group_filter,
            };
            let problems = index.search_problems(&q);
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "ok": true, "problems": problems }))
                        .expect("serializable")
                );
            } else {
                for p in &problems {
                    let rating = p.rating.map_or_else(|| "-".to_owned(), |r| r.to_string());
                    let tags_str = if p.tags.is_empty() {
                        "-".to_owned()
                    } else {
                        p.tags.join(",")
                    };
                    let sources_str = if p.sources.is_empty() {
                        "-".to_owned()
                    } else {
                        p.sources
                            .iter()
                            .map(|s| format!("{}:{}", s.platform, s.id))
                            .collect::<Vec<_>>()
                            .join(",")
                    };
                    let mut flags = Vec::new();
                    if p.has_editorial {
                        flags.push("editorial");
                    }
                    if p.has_solutions {
                        flags.push("solutions");
                    }
                    let flags_str = if flags.is_empty() {
                        "-".to_owned()
                    } else {
                        flags.join(",")
                    };
                    println!(
                        "{}\t{}\t{}\t{}\t{}\t{}",
                        p.id, p.title, rating, tags_str, sources_str, flags_str
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Show { kind, id } => {
            let ws = open_workspace(cli)?;
            match kind {
                TargetKind::Problem => {
                    let detail = views::problem_detail(&ws, id)?;
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&detail).expect("serializable")
                        );
                    } else {
                        print_problem_detail(&detail);
                    }
                }
                TargetKind::Contest => {
                    let mut engine = publish::engine(&ws, &fonts(cli));
                    let detail = views::contest_detail(&ws, &mut engine, id)?;
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&detail).expect("serializable")
                        );
                    } else {
                        print_contest_detail(&detail);
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Group { action } => run_group(cli, action),
        Command::Problem { action } => run_problem(cli, action),
        Command::Contest { action } => run_contest(cli, action),
        Command::Package { action } => run_package(cli, action),
        Command::Import { action } => run_import(cli, action),
        Command::Export {
            format,
            kind,
            ids,
            out,
            force,
        } => run_export(cli, *format, *kind, ids, out.as_ref(), *force),
        Command::Template { action } => run_template(cli, action),
        Command::Plugin { action } => run_plugin(cli, action),
        Command::Sync {
            action,
            platform,
            full,
        } => run_sync(cli, action.as_ref(), platform.as_deref(), *full),
        Command::Training { action } => run_training(cli, action),
    }
}

fn run_import(cli: &Cli, action: &ImportCommand) -> Result<ExitCode, Fatal> {
    let ws = open_workspace(cli)?;
    if ws.is_read_only() {
        return Err(xcpc_storage::Error::ReadOnly.into());
    }
    match action {
        ImportCommand::Polygon {
            zip,
            id,
            group,
            no_auto_group,
        } => {
            let opts = xcpc_packages::PolygonImportOptions {
                target_id: id.clone(),
                group: group.clone(),
                no_auto_group: *no_auto_group,
                ..Default::default()
            };
            let res = xcpc_packages::import_polygon(&ws, zip, &opts)?;
            if cli.json {
                let mut value = serde_json::to_value(&res).expect("serializable");
                value["ok"] = json!(true);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).expect("serializable")
                );
            } else {
                print_polygon_import_result(&res);
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn run_export(
    cli: &Cli,
    format: ExportFormat,
    kind: ExportKind,
    ids: &[String],
    out: Option<&PathBuf>,
    force: bool,
) -> Result<ExitCode, Fatal> {
    let ws = open_workspace(cli)?;
    if ws.is_read_only() {
        return Err(xcpc_storage::Error::ReadOnly.into());
    }
    let oj_format = format.into_oj_format();
    match kind {
        ExportKind::Problem | ExportKind::Contest => {
            if ids.len() != 1 {
                return Err(Fatal {
                    code: "invalid_arguments",
                    message: "expected exactly one ID for `export problem` / `export contest`"
                        .to_owned(),
                    human: Some(
                        "`export problem` 或 `export contest` 只能指定一个 ID；批量导出请使用 `export <format> problems <ID>...`。"
                            .to_owned(),
                    ),
                    exit_code: 2,
                });
            }
            let id = &ids[0];
            let target = match kind {
                ExportKind::Problem => xcpc_packages::ExportTarget::Problem(id.clone()),
                ExportKind::Contest => xcpc_packages::ExportTarget::Contest(id.clone()),
                ExportKind::Problems => unreachable!(),
            };
            let output_path = match out {
                Some(path) => path.clone(),
                None => match kind {
                    ExportKind::Problem => ws
                        .root()
                        .join(format!("build/{id}-{}.zip", oj_format.as_str())),
                    ExportKind::Contest => {
                        ws.root().join(format!("build/{id}-{}", oj_format.as_str()))
                    }
                    ExportKind::Problems => unreachable!(),
                },
            };
            let opts = xcpc_packages::ExportOjOptions {
                format: oj_format,
                output_path,
                force,
                language: None,
            };
            let res = xcpc_packages::export_oj_package(&ws, &target, &opts)?;
            if cli.json {
                let mut value = serde_json::to_value(&res).expect("serializable");
                value["ok"] = json!(true);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).expect("serializable")
                );
            } else {
                print_export_oj_result(&res);
            }
            Ok(ExitCode::SUCCESS)
        }
        ExportKind::Problems => {
            let output_path = out.cloned().unwrap_or_default();
            let opts = xcpc_packages::ExportOjOptions {
                format: oj_format,
                output_path,
                force,
                language: None,
            };
            let res = xcpc_packages::export_oj_packages_batch(&ws, ids, &opts, None, None)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&res).expect("serializable")
                );
            } else {
                print_batch_export_oj_result(&res);
            }
            if res.ok {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(1))
            }
        }
    }
}

fn print_polygon_import_result(res: &xcpc_packages::PolygonImportResult) {
    match res.kind {
        xcpc_packages::PolygonImportKind::Problem => {
            if let Some(p) = res.problems.first() {
                println!("已导入题目「{}」（{}）", p.id, p.title);
                let mut extras = Vec::new();
                if p.has_interactor {
                    extras.push("交互题".to_owned());
                }
                if let Some(r) = p.runs.filter(|&r| r > 1) {
                    extras.push(format!("多轮 ×{r}"));
                }
                if p.subtasks_count > 0 {
                    extras.push(format!("子任务 {} 个", p.subtasks_count));
                }
                let extra_str = if extras.is_empty() {
                    String::new()
                } else {
                    format!("，{}", extras.join("，"))
                };
                println!(
                    "  样例 {} 组，测试点 {} 组，生成器 {} 个，解法 {} 个，校验器：{}{extra_str}",
                    p.samples_count,
                    p.tests_count,
                    p.generators_count,
                    p.solutions_count,
                    p.checker
                );
            } else {
                println!("已导入题目（共 0 道）");
            }
        }
        xcpc_packages::PolygonImportKind::Contest => {
            let cid = res.contest_id.as_deref().unwrap_or("-");
            println!("已导入比赛「{cid}」（共 {} 道题）：", res.problems.len());
            for p in &res.problems {
                let label = p.label.as_deref().unwrap_or("-");
                println!(
                    "  {label}\t{}\t{}（样例 {}，测试点 {}）",
                    p.id, p.title, p.samples_count, p.tests_count
                );
            }
        }
    }
    if let Some(ref g) = res.group {
        println!("  分组：{g}");
    }
    print_package_warnings(&res.warnings);
}

fn print_batch_export_oj_result(res: &xcpc_packages::BatchExportOjResult) {
    let format_label = match res.format {
        xcpc_packages::OjFormat::Domjudge => "DOMjudge",
        xcpc_packages::OjFormat::Hydro => "Hydro",
    };
    println!(
        "批量导出 {format_label} 题目包（输出目录：{}）：已导出 {}，跳过 {}，失败 {}",
        res.output_dir, res.exported, res.skipped, res.failed
    );
    for item in &res.items {
        match item.status {
            xcpc_packages::BatchExportItemStatus::Exported => {
                let forced = if item.forced { " [强制导出]" } else { "" };
                println!(
                    "  [导出] {} -> {}（样例 {}，测试点 {}，{}）{forced}",
                    item.problem_id,
                    item.zip_path.as_deref().unwrap_or("-"),
                    item.sample_count,
                    item.secret_count,
                    format_bytes(item.size_bytes.unwrap_or(0)),
                );
            }
            xcpc_packages::BatchExportItemStatus::Skipped => {
                println!(
                    "  [跳过] {}：{}",
                    item.problem_id,
                    item.message
                        .as_deref()
                        .or(item.code.as_deref())
                        .unwrap_or("已跳过")
                );
            }
            xcpc_packages::BatchExportItemStatus::Failed => {
                println!(
                    "  [失败] {}：{}",
                    item.problem_id,
                    item.message
                        .as_deref()
                        .or(item.code.as_deref())
                        .unwrap_or("导出失败")
                );
            }
            xcpc_packages::BatchExportItemStatus::Cancelled => {
                println!("  [取消] {}", item.problem_id);
            }
        }
    }
    print_package_warnings(&res.warnings);
}

fn print_export_oj_result(res: &xcpc_packages::ExportOjResult) {
    let format_label = match res.format {
        xcpc_packages::OjFormat::Domjudge => "DOMjudge",
        xcpc_packages::OjFormat::Hydro => "Hydro",
    };
    println!(
        "已导出 {format_label} 题目包（共 {} 个）：",
        res.artifacts.len()
    );
    for a in &res.artifacts {
        let forced = if a.forced { " [强制导出]" } else { "" };
        if let Some(ref label) = a.label {
            println!(
                "  {label}\t{} -> {}（样例 {}，测试点 {}，{}）{forced}",
                a.problem_id,
                a.zip_path,
                a.sample_count,
                a.secret_count,
                format_bytes(a.size_bytes),
            );
        } else {
            println!(
                "  {} -> {}（样例 {}，测试点 {}，{}）{forced}",
                a.problem_id,
                a.zip_path,
                a.sample_count,
                a.secret_count,
                format_bytes(a.size_bytes),
            );
        }
    }
    print_package_warnings(&res.warnings);
}

fn print_package_warnings(warnings: &[xcpc_packages::PackageWarning]) {
    if warnings.is_empty() {
        return;
    }
    println!("警告（{} 条）：", warnings.len());
    for w in warnings {
        if let Some(ref path) = w.path {
            println!("  [{}] {}（{path}）", w.code, w.message);
        } else {
            println!("  [{}] {}", w.code, w.message);
        }
    }
}

fn run_package(cli: &Cli, action: &PackageCommand) -> Result<ExitCode, Fatal> {
    let ws = open_workspace(cli)?;
    match action {
        PackageCommand::Inspect { id } => {
            let insp = xcpc_judge::inspect_package(&ws, id)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&insp).expect("serializable")
                );
            } else {
                print_package_inspection(&insp);
            }
            Ok(ExitCode::SUCCESS)
        }
        PackageCommand::Init {
            id,
            interactive,
            runs,
            subtasks,
        } => {
            if ws.is_read_only() {
                return Err(xcpc_storage::Error::ReadOnly.into());
            }
            let opts = xcpc_judge::InitPackageOptions {
                interactive: *interactive,
                runs: *runs,
                subtasks: *subtasks,
            };
            let res = xcpc_judge::init_package_with_options(&ws, id, &opts)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "problem_id": res.problem_id,
                        "created_files": res.created_files,
                        "skipped_files": res.skipped_files,
                        "updated_problem_toml": res.updated_problem_toml,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("已初始化题目「{}」的题目包脚手架", res.problem_id);
                for f in &res.created_files {
                    println!("  新建：{f}");
                }
                for f in &res.skipped_files {
                    println!("  跳过（已存在）：{f}");
                }
                if res.updated_problem_toml {
                    println!("  已更新：problem.toml");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        PackageCommand::Build {
            ids,
            strict,
            sanitize,
            tests,
            solutions,
            jobs,
            contest,
        } => {
            if ws.is_read_only() {
                return Err(xcpc_storage::Error::ReadOnly.into());
            }
            if *sanitize {
                let opts = xcpc_judge::SanitizeOptions {
                    tests: tests.clone(),
                    solutions: solutions.clone(),
                    jobs: *jobs,
                    ..Default::default()
                };
                if ids.len() == 1 {
                    let report = xcpc_judge::sanitize_package(&ws, &ids[0], &opts, None, None)?;
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&report).expect("serializable")
                        );
                    } else {
                        print_sanitizer_report(&report);
                    }
                    if report.ok {
                        Ok(ExitCode::SUCCESS)
                    } else {
                        Ok(ExitCode::from(1))
                    }
                } else {
                    let report = xcpc_judge::sanitize_packages_batch(&ws, ids, &opts, None, None)?;
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&report).expect("serializable")
                        );
                    } else {
                        print_batch_build_report(&report);
                    }
                    if report.ok {
                        Ok(ExitCode::SUCCESS)
                    } else {
                        Ok(ExitCode::from(1))
                    }
                }
            } else {
                let opts = xcpc_judge::BuildOptions {
                    mode: if *strict {
                        xcpc_judge::BuildMode::Verify
                    } else {
                        xcpc_judge::BuildMode::Debug
                    },
                    tests: tests.clone(),
                    solutions: solutions.clone(),
                    jobs: *jobs,
                    solution_jobs: *jobs,
                    contest: contest.clone(),
                    ..Default::default()
                };
                if ids.len() == 1 {
                    let report = xcpc_judge::build_package(&ws, &ids[0], &opts, None, None)?;
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&report).expect("serializable")
                        );
                    } else {
                        print_build_report(&report);
                    }
                    let passed = if *strict {
                        report.ok && report.verified
                    } else {
                        report.ok
                    };
                    if passed {
                        Ok(ExitCode::SUCCESS)
                    } else {
                        Ok(ExitCode::from(1))
                    }
                } else {
                    let report = xcpc_judge::build_packages_batch(&ws, ids, &opts, None, None)?;
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&report).expect("serializable")
                        );
                    } else {
                        print_batch_build_report(&report);
                    }
                    if report.ok {
                        Ok(ExitCode::SUCCESS)
                    } else {
                        Ok(ExitCode::from(1))
                    }
                }
            }
        }
        PackageCommand::Stress {
            id,
            generator,
            args,
            solutions,
            max_iter,
            timeout,
        } => {
            if ws.is_read_only() {
                return Err(xcpc_storage::Error::ReadOnly.into());
            }
            let opts = xcpc_judge::StressOptions {
                generator: generator.clone(),
                args: args.clone(),
                solutions: solutions.clone(),
                max_iter: *max_iter,
                timeout_secs: *timeout,
                ..Default::default()
            };
            let res = xcpc_judge::stress_package(&ws, id, &opts, None)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&res).expect("serializable")
                );
            } else {
                print_stress_result(&res);
            }
            Ok(ExitCode::SUCCESS)
        }
        PackageCommand::VerifyExternal {
            id,
            files,
            auto_build,
            jobs,
            markdown,
        } => {
            if *auto_build && ws.is_read_only() {
                return Err(xcpc_storage::Error::ReadOnly.into());
            }
            let opts = xcpc_judge::ExternalRunOptions {
                auto_build: *auto_build,
                jobs: *jobs,
                ..Default::default()
            };
            let report =
                xcpc_judge::run_external_solutions_for_problem(&ws, id, files, &opts, None)?;
            let md_written = if let Some(path) = markdown {
                let md = xcpc_judge::render_external_problem_markdown(&report);
                Some(write_explicit_file(path, md.as_bytes())?)
            } else {
                None
            };
            if cli.json {
                let mut value = serde_json::to_value(&report).expect("serializable");
                value["ok"] = json!(true);
                if let Some(ref p) = md_written {
                    value["markdown"] = json!(p);
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).expect("serializable")
                );
            } else {
                print_external_problem_report(&report, md_written.as_deref());
            }
            Ok(ExitCode::SUCCESS)
        }
        PackageCommand::Adopt {
            id,
            file,
            name,
            expect,
            tags,
            main,
        } => {
            if ws.is_read_only() {
                return Err(xcpc_storage::Error::ReadOnly.into());
            }
            let opts = xcpc_judge::AdoptSolutionOptions {
                name: name.clone(),
                expect: expect.clone(),
                tags: tags.clone(),
                is_main: *main,
            };
            let res = xcpc_judge::adopt_external_solution(&ws, id, file, &opts)?;
            if cli.json {
                let mut value = serde_json::to_value(&res).expect("serializable");
                value["ok"] = json!(true);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).expect("serializable")
                );
            } else {
                println!(
                    "已将验题人代码「{}」加入解法 {}（题目「{}」{}）",
                    res.solution_name,
                    res.dest_rel,
                    res.problem_id,
                    if res.updated_problem_toml {
                        "，已更新 problem.toml"
                    } else {
                        ""
                    }
                );
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn format_score_value(v: f64) -> String {
    if (v - v.round()).abs() < 1e-6 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

fn print_transcript(tr: &xcpc_judge::InteractionTranscript, indent: &str, max_lines: usize) {
    let mut shown = 0usize;
    for entry in &tr.head {
        if shown >= max_lines {
            break;
        }
        let arrow = match entry.direction {
            xcpc_judge::TranscriptDirection::InteractorToSolution => "→",
            xcpc_judge::TranscriptDirection::SolutionToInteractor => "←",
        };
        let run_prefix = match entry.run {
            Some(r) => format!("[run #{r}] "),
            None => String::new(),
        };
        println!("{indent}{run_prefix}{arrow} {}", entry.text);
        shown += 1;
    }
    if tr.omitted_entries > 0 {
        println!("{indent}...（省略 {} 行交互记录）...", tr.omitted_entries);
    }
    for entry in &tr.tail {
        if shown >= max_lines {
            break;
        }
        let arrow = match entry.direction {
            xcpc_judge::TranscriptDirection::InteractorToSolution => "→",
            xcpc_judge::TranscriptDirection::SolutionToInteractor => "←",
        };
        let run_prefix = match entry.run {
            Some(r) => format!("[run #{r}] "),
            None => String::new(),
        };
        println!("{indent}{run_prefix}{arrow} {}", entry.text);
        shown += 1;
    }
    if !tr.interactor_message.trim().is_empty() {
        println!("{indent}交互器输出：{}", tr.interactor_message.trim());
    }
}

fn print_package_inspection(insp: &xcpc_judge::PackageInspection) {
    let freshness_str = match &insp.freshness {
        xcpc_judge::PackageFreshness::NotAPackage => "仅题面",
        xcpc_judge::PackageFreshness::VerifiedFresh { .. } => "已通过",
        xcpc_judge::PackageFreshness::Stale { .. } => "未检查（改动后还没重新检查）",
        xcpc_judge::PackageFreshness::Unverified { .. } => "未检查",
        xcpc_judge::PackageFreshness::Failed { .. } => "有问题",
    };
    let mut modes = Vec::new();
    if insp.interactive {
        modes.push("交互题".to_owned());
    }
    if insp.runs > 1 {
        modes.push(format!("多轮 ×{}", insp.runs));
    }
    if !insp.subtasks.is_empty() {
        if let Some(total) = insp.total_score {
            modes.push(format!("子任务（总分 {total}）"));
        } else {
            modes.push(format!("子任务（{} 组）", insp.subtasks.len()));
        }
    }
    let mode_suffix = if modes.is_empty() {
        String::new()
    } else {
        format!("，{}", modes.join("，"))
    };
    println!(
        "题目「{}」：{freshness_str}（评测器：{}{}）",
        insp.problem_id, insp.checker, mode_suffix
    );
    if let Some(ref inter) = insp.files.interactor {
        println!("  交互器：{inter}");
    }
    if let Some(ref val) = insp.files.validator {
        println!("  校验器：{val}");
    }
    if let Some(ref chk) = insp.files.custom_checker {
        println!("  自定义评测器：{chk}");
    }
    println!(
        "  测试点：{} 组，生成器：{} 个，解法：{} 个",
        insp.expanded_tests.len(),
        insp.files.generators.len(),
        insp.solutions.len()
    );

    if !insp.subtasks.is_empty() {
        println!("\n子任务：");
        for st in &insp.subtasks {
            let scoring = match st.scoring {
                xcpc_domain::SubtaskScoring::Min => "全对才得分",
                xcpc_domain::SubtaskScoring::Sum => "按通过比例得分",
            };
            let deps = if st.depends.is_empty() {
                String::new()
            } else {
                format!("，依赖：{}", st.depends.join("、"))
            };
            println!(
                "  #{}：{} 分（{scoring}，{} 组测试点{deps}）",
                st.id, st.score, st.test_count
            );
        }
    }

    if !insp.solutions.is_empty() {
        println!("\n解法配置：");
        for s in &insp.solutions {
            let role = if s.is_main { " [标程]" } else { "" };
            let mut extra = Vec::new();
            if let Some(ref sc) = s.score {
                match sc {
                    xcpc_domain::ScoreExpect::Exact(v) => extra.push(format!("应得分数 {v}")),
                    xcpc_domain::ScoreExpect::Range([lo, hi]) => {
                        extra.push(format!("应得分数 {lo}..={hi}"))
                    }
                }
            }
            if !s.subtasks.is_empty() {
                let parts: Vec<String> = s
                    .subtasks
                    .iter()
                    .map(|(k, v)| match v {
                        xcpc_domain::ExpectVerdict::Single(one) => format!("{k}={one}"),
                        xcpc_domain::ExpectVerdict::Multiple(many) => {
                            format!("{k}={}", many.join("|"))
                        }
                    })
                    .collect();
                extra.push(format!("子任务预期 {{{}}}", parts.join(", ")));
            }
            let extra_str = if extra.is_empty() {
                String::new()
            } else {
                format!("，{}", extra.join("，"))
            };
            println!(
                "  {}{role}（{}）：预期 [{}]{extra_str}",
                s.name,
                s.source_rel,
                s.expected.join("|")
            );
        }
    }

    if !insp.next_steps.is_empty() {
        println!("\n下一步建议：");
        for step in &insp.next_steps {
            println!("  - {step}");
        }
    }
}

fn print_build_report(report: &xcpc_judge::BuildReport) {
    let mode_str = match report.mode {
        xcpc_judge::BuildMode::Debug => "快速调试",
        xcpc_judge::BuildMode::Verify => "检查",
    };
    let status_str = if report.verified {
        "已通过"
    } else if report.ok {
        "通过"
    } else {
        "有问题"
    };
    let mut mode_tags = Vec::new();
    if report.interactive {
        mode_tags.push("交互题".to_owned());
    }
    if report.runs > 1 {
        mode_tags.push(format!("多轮 ×{}", report.runs));
    }
    if !report.subtasks.is_empty() {
        if let Some(total) = report.total_score {
            mode_tags.push(format!("子任务，总分 {total}"));
        } else {
            mode_tags.push(format!("子任务 {} 组", report.subtasks.len()));
        }
    }
    let extra_tags = if mode_tags.is_empty() {
        String::new()
    } else {
        format!("，{}", mode_tags.join("，"))
    };
    println!(
        "题目「{}」：{}（模式：{}{}，评测器：{}{}）",
        report.problem_id,
        status_str,
        mode_str,
        if report.subset { "，局部运行" } else { "" },
        report.checker,
        extra_tags,
    );
    if let Some(ref c) = report.compiler {
        println!("  编译器：{}（{}）", c.name, c.version);
    }

    // Safety margin summary
    let sm = &report.safety_margin;
    let std_sec = sm.std_max_time_ms as f64 / 1000.0;
    let lim_sec = sm.time_limit_ms as f64 / 1000.0;
    let pct = sm.std_time_ratio * 100.0;
    println!(
        "  安全边际：最大输入 {}，标程最慢 {:.3}s / {:.2}s（{:.1}%），峰值内存 {:.1} MiB / {:.0} MiB",
        format_bytes(sm.max_input_bytes),
        std_sec,
        lim_sec,
        pct,
        sm.std_max_memory_kib as f64 / 1024.0,
        sm.memory_limit_kib as f64 / 1024.0,
    );
    if let (Some(ac_sol), Some(ac_ms), Some(tle_sol), Some(tle_ms)) = (
        &sm.slowest_ac_solution,
        sm.slowest_ac_time_ms,
        &sm.fastest_tle_solution,
        sm.fastest_tle_time_ms,
    ) {
        println!(
            "  耗时余量：最慢 AC 解「{ac_sol}」（{:.3}s）对比最快 TLE 错解「{tle_sol}」（{:.3}s）",
            ac_ms as f64 / 1000.0,
            tle_ms as f64 / 1000.0
        );
    }

    if !report.compile_failures.is_empty() {
        println!("\n编译失败：");
        for cf in &report.compile_failures {
            println!("  {}（{}）：", cf.target, cf.source_path);
            for line in cf.stderr.lines().take(20) {
                println!("    {line}");
            }
        }
    }

    if !report.dirty_inputs.is_empty() {
        println!("\n脏输入检查：");
        for d in &report.dirty_inputs {
            let mut flags = Vec::new();
            if d.has_crlf {
                flags.push("CRLF 换行");
            }
            if d.has_bom {
                flags.push("UTF-8 BOM");
            }
            if d.missing_trailing_newline {
                flags.push("缺少末尾换行");
            }
            println!("  {}：{}", d.path, flags.join("、"));
        }
    }

    let failed_self_tests: Vec<_> = report.self_tests.iter().filter(|st| !st.passed).collect();
    if !failed_self_tests.is_empty() {
        println!("\n自测未通过：");
        for st in failed_self_tests {
            let detail = if st.message.trim().is_empty() {
                String::new()
            } else {
                format!(" — {}", st.message.trim())
            };
            println!(
                "  [{}] {}：预期 {}，实际 {}{detail}",
                st.kind, st.name, st.expected, st.actual
            );
        }
    }

    let validator_failures: Vec<_> = report
        .tests
        .iter()
        .filter(|t| t.validator_ok == Some(false))
        .collect();
    if !validator_failures.is_empty() {
        println!("\n校验器未通过：");
        for t in validator_failures {
            let msg = t
                .validator_message
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("校验失败（无详细输出）");
            println!("  #{} {}：{msg}", t.index, t.name);
        }
    }

    if !report.subtasks.is_empty() {
        println!("\n子任务分组：");
        for st in &report.subtasks {
            let scoring = match st.scoring {
                xcpc_domain::SubtaskScoring::Min => "min",
                xcpc_domain::SubtaskScoring::Sum => "sum",
            };
            let deps = if st.depends.is_empty() {
                String::new()
            } else {
                format!("，依赖 [{}]", st.depends.join(", "))
            };
            println!(
                "  #{}：{} 分（{scoring}，{} 组测试点{deps}）",
                st.id, st.score, st.test_count
            );
        }
    }

    if !report.solutions.is_empty() {
        println!("\n解法结果：");
        for s in &report.solutions {
            let mark = if s.matched {
                "符合预期"
            } else {
                "不符预期"
            };
            let role = if s.is_main { " [标程]" } else { "" };
            let score_part = match (s.total_score, s.max_score) {
                (Some(got), Some(max)) => format!("，得分 {}/{max}", format_score_value(got)),
                (Some(got), None) => format!("，得分 {}", format_score_value(got)),
                _ => String::new(),
            };
            println!(
                "  [{mark}] {}{role}：实际 {}，预期 [{}]{score_part}，通过 {}/{}，最慢 {}ms，峰值 {:.1} MiB",
                s.name,
                s.actual_verdict,
                s.expected.join("|"),
                s.passed_tests,
                s.total_tests,
                s.max_time_ms,
                s.max_memory_kib as f64 / 1024.0,
            );
            if !s.subtask_scores.is_empty() {
                let parts: Vec<String> = s
                    .subtask_scores
                    .iter()
                    .map(|ss| {
                        format!(
                            "#{}: {}/{} ({})",
                            ss.subtask_id,
                            format_score_value(ss.score),
                            ss.max_score,
                            ss.verdict
                        )
                    })
                    .collect();
                println!("    子任务得分：{}", parts.join(" | "));
            }
        }
    }

    let failing_cells: Vec<_> = report
        .matrix
        .iter()
        .filter(|c| c.verdict != xcpc_judge::Verdict::AC)
        .collect();
    if !failing_cells.is_empty() {
        println!("\n非 AC 详情（最多显示 20 项）：");
        for c in failing_cells.iter().take(20) {
            let ratio_str = match c.score_ratio {
                Some(r) if r > 0.0 && r < 1.0 => format!("，得分率 {:.0}%", r * 100.0),
                _ => String::new(),
            };
            let detail = if !c.checker_message.is_empty() {
                format!(" — {}", c.checker_message.trim())
            } else if !c.stderr.is_empty() {
                format!(" — {}", c.stderr.lines().next().unwrap_or("").trim())
            } else {
                String::new()
            };
            println!(
                "  #{} {} × {}：{}（{}ms{ratio_str}）{detail}",
                c.test_index, c.test_name, c.solution, c.verdict, c.cpu_time_ms
            );
            if let Some(ref tr) = c.transcript {
                print_transcript(tr, "    ", 12);
            }
        }
    }

    if !report.gates.is_empty() {
        println!("\n检查项：");
        for g in &report.gates {
            let tag = match g.status {
                xcpc_judge::GateStatus::Pass => "通过",
                xcpc_judge::GateStatus::Fail => "有问题",
                xcpc_judge::GateStatus::Warn => "提醒",
                xcpc_judge::GateStatus::Skip => "跳过",
            };
            println!("  [{tag}] {}：{}", g.title, g.message);
        }
    }

    if !report.repro_commands.is_empty() {
        println!("\n单步复现：");
        for cmd in &report.repro_commands {
            println!("  {cmd}");
        }
    }

    if !report.next_steps.is_empty() {
        println!("\n下一步建议：");
        for step in &report.next_steps {
            println!("  - {step}");
        }
    }
}

fn print_stress_result(res: &xcpc_judge::StressResult) {
    if res.found_counterexample {
        println!(
            "对拍「{}」：第 {} 轮发现反例（耗时 {} ms）",
            res.problem_id, res.iterations_run, res.elapsed_ms
        );
        if let Some(seed) = res.seed {
            println!("  种子：{seed}");
        }
        if let Some(ref cmd) = res.gen_cmd {
            println!("  生成命令：{cmd}");
        }
        if let Some(ref path) = res.saved_test_path {
            println!("  已保存反例：{path}");
        }
        if let (Some(sol), Some(v)) = (&res.failing_solution, res.verdict) {
            println!("  失败解法：{sol}（{v}）");
        }
        if let Some(ref msg) = res.checker_message {
            println!("  校验器输出：{}", msg.trim());
        }
        if let Some(ref inp) = res.input_preview {
            println!("--- 输入预览 ---\n{inp}");
        }
        if let Some(ref out) = res.std_output_preview {
            println!("--- 标程输出 ---\n{out}");
        }
        if let Some(ref out) = res.solution_output_preview {
            println!("--- 解法输出 ---\n{out}");
        }
        if let Some(ref tr) = res.transcript {
            println!("--- 交互日志 ---");
            print_transcript(tr, "  ", 20);
        }
    } else {
        println!(
            "对拍「{}」：通过（共 {} 轮，耗时 {} ms，未发现反例）",
            res.problem_id, res.iterations_run, res.elapsed_ms
        );
    }
}

fn print_batch_build_report(report: &xcpc_judge::BatchBuildReport) {
    println!(
        "批量检查完成（共 {} 题，耗时 {} ms）：已通过 {}，有问题 {}，跳过 {}",
        report.total, report.elapsed_ms, report.passed, report.failed, report.skipped
    );
    for item in &report.items {
        let tag = match item.status {
            xcpc_judge::BatchBuildItemStatus::Passed => {
                if item.verified {
                    "已通过"
                } else {
                    "通过"
                }
            }
            xcpc_judge::BatchBuildItemStatus::Failed => "有问题",
            xcpc_judge::BatchBuildItemStatus::Skipped => "跳过",
            xcpc_judge::BatchBuildItemStatus::Error => "有问题",
            xcpc_judge::BatchBuildItemStatus::Cancelled => "已取消",
        };
        if let Some(ref msg) = item.message {
            println!(
                "  [{tag}] {}（{}ms）：{msg}",
                item.problem_id, item.elapsed_ms
            );
        } else {
            println!("  [{tag}] {}（{}ms）", item.problem_id, item.elapsed_ms);
        }
    }
}

fn print_sanitizer_report(report: &xcpc_judge::SanitizerReport) {
    let status_str = match report.status {
        xcpc_judge::SanitizerStatus::Passed => "没有发现内存问题",
        xcpc_judge::SanitizerStatus::Failed => "发现内存问题",
        xcpc_judge::SanitizerStatus::Unsupported => "当前编译器不支持内存检查",
    };
    println!(
        "题目「{}」内存检查：{status_str}{}",
        report.problem_id,
        if report.subset {
            "（局部运行）"
        } else {
            ""
        }
    );
    if let Some(ref c) = report.compiler {
        println!("  编译器：{}（{}）", c.name, c.version);
    }
    if let Some(ref reason) = report.unsupported_reason {
        println!("  不支持原因：{reason}");
    }
    if !report.compile_failures.is_empty() {
        println!("\n编译失败：");
        for cf in &report.compile_failures {
            println!("  {}（{}）：", cf.target, cf.source_path);
            for line in cf.stderr.lines().take(20) {
                println!("    {line}");
            }
        }
    }
    if !report.targets.is_empty() {
        println!("\n检查目标：");
        for t in &report.targets {
            let tag = if t.passed { "通过" } else { "失败" };
            println!(
                "  [{tag}] {}（{}）：运行 {} 次，发现 {} 处异常",
                t.target, t.source_path, t.checked_runs, t.findings_count
            );
        }
    }
    if !report.findings.is_empty() {
        println!("\n发现异常（{} 处）：", report.findings.len());
        for f in &report.findings {
            let test_ctx = match (f.test_index, f.test_name.as_deref()) {
                (Some(idx), Some(name)) => format!("（测试 #{idx} {name}）"),
                (Some(idx), None) => format!("（测试 #{idx}）"),
                _ => String::new(),
            };
            println!("  {}{test_ctx}：{}", f.target, f.summary);
            for frame in f.stack_summary.iter().take(6) {
                println!("    {frame}");
            }
        }
    }
    if !report.diagnostics.is_empty() {
        println!("\n诊断信息：");
        for d in &report.diagnostics {
            println!("  - {d}");
        }
    }
    if !report.next_steps.is_empty() {
        println!("\n下一步建议：");
        for step in &report.next_steps {
            println!("  - {step}");
        }
    }
}

fn print_external_submission_line(s: &xcpc_judge::ExternalSubmissionResult) {
    let prefix = match (&s.tester, &s.problem_label) {
        (Some(t), Some(l)) => format!("{t}/{l}\t"),
        (Some(t), None) => format!("{t}\t"),
        (None, Some(l)) => format!("{l}\t"),
        (None, None) => String::new(),
    };
    let ratio = s
        .ratio_vs_std
        .map(|r| {
            if r >= 2.0 {
                format!("（比标程慢 {r:.1} 倍）")
            } else {
                "（与标程相近）".to_owned()
            }
        })
        .unwrap_or_default();
    let score = match (s.total_score, s.max_score) {
        (Some(got), Some(max)) => format!("，得分 {}/{max}", format_score_value(got)),
        (Some(got), None) => format!("，得分 {}", format_score_value(got)),
        _ => String::new(),
    };
    println!(
        "  {prefix}{}：{}，通过 {}/{}{score}，最慢 {}ms{ratio}",
        s.file_name, s.verdict, s.passed_tests, s.total_tests, s.max_time_ms
    );
    if let Some(ref ce) = s.compile_error {
        if let Some(first_line) = ce.lines().find(|l| !l.trim().is_empty()) {
            println!("    编译错误：{}", first_line.trim());
        }
    } else if let Some(ref ff) = s.first_failure {
        let msg = if ff.message.trim().is_empty() {
            String::new()
        } else {
            format!(" — {}", ff.message.trim())
        };
        println!(
            "    首个失败：#{} {}（{}）{msg}",
            ff.test_index, ff.test_name, ff.verdict
        );
    }
}

fn print_external_problem_report(
    report: &xcpc_judge::ExternalProblemReport,
    markdown_path: Option<&str>,
) {
    println!(
        "题目「{}」验题人代码试跑（共 {} 份）：",
        report.problem_id,
        report.submissions.len()
    );
    for s in &report.submissions {
        print_external_submission_line(s);
    }
    if let Some(path) = markdown_path {
        println!("已导出检查报告：{path}");
    }
}

fn print_external_contest_report(
    report: &xcpc_judge::ExternalContestReport,
    markdown_path: Option<&str>,
) {
    let mode = match report.mode {
        xcpc_judge::ExternalContestMode::SingleDirectory => "单目录",
        xcpc_judge::ExternalContestMode::MultiTester => "多位验题人",
    };
    println!(
        "比赛「{}」（{}）验题人代码试跑（{mode}，共 {} 份）：",
        report.contest_id,
        report.contest_title,
        report.submissions.len()
    );
    if !report.testers.is_empty() {
        println!("  验题人：{}", report.testers.join("、"));
    }
    for s in &report.submissions {
        print_external_submission_line(s);
    }
    if !report.unmatched.is_empty() {
        println!("\n未匹配文件（{} 份，已跳过）：", report.unmatched.len());
        for u in &report.unmatched {
            if let Some(ref t) = u.tester {
                println!("  [{t}] {}：{}", u.file_path, u.reason);
            } else {
                println!("  {}：{}", u.file_path, u.reason);
            }
        }
    }
    if let Some(path) = markdown_path {
        println!("已导出检查报告：{path}");
    }
}

fn print_contest_audit_report(
    report: &xcpc_judge::ContestAuditReport,
    markdown_path: Option<&str>,
) {
    let s = &report.summary;
    let unchecked_count = s.stale_count + s.unbuilt_count;
    let ready = if s.ready_for_freeze {
        "可以定稿".to_owned()
    } else if s.failed_count > 0 {
        format!("{} 题有问题", s.failed_count)
    } else if unchecked_count > 0 {
        format!("{unchecked_count} 题未检查")
    } else {
        "存在问题".to_owned()
    };
    println!(
        "比赛「{}」（{}）检查结果：{ready}（共 {} 题：已通过 {} / 未检查 {} / 有问题 {} / 仅题面 {}）",
        report.contest_id,
        report.contest_title,
        s.total_problems,
        s.fresh_count,
        unchecked_count,
        s.failed_count,
        s.statement_only_count
    );
    println!(
        "  检查汇总：问题 {} 项，提醒 {} 项；内存检查：通过 {} / 发现问题 {} / 未检查 {}",
        s.error_risk_count,
        s.warning_risk_count,
        s.sanitizer_passed_count,
        s.sanitizer_failed_count,
        s.sanitizer_not_run_count
    );
    if !report.problems.is_empty() {
        println!("\n逐题总览：");
        println!(
            "  题号\t题目 ID\t状态\t标程耗时占比\t单点最大体积\t独立AC/错解\t内存检查\t题面告警(E/W)"
        );
        for row in &report.problems {
            let ratio = match (row.std_max_time_ms, row.time_limit_ms, row.std_time_ratio) {
                (Some(std_ms), Some(tl_ms), Some(r)) => format!(
                    "{:.2}s/{:.2}s({:.0}%)",
                    std_ms as f64 / 1000.0,
                    tl_ms as f64 / 1000.0,
                    r * 100.0
                ),
                _ => "-".to_owned(),
            };
            let max_file = if row.max_test_file_bytes > 0 {
                format_bytes(row.max_test_file_bytes)
            } else {
                "-".to_owned()
            };
            println!(
                "  {}\t{}\t{}\t{}\t{}\t{}/{}\t{}\t{}/{}",
                row.label,
                row.problem_id,
                row.verification_status.label_zh(),
                ratio,
                max_file,
                row.independent_ac_count,
                row.wrong_solution_count,
                row.sanitizer_status.label_zh(),
                row.lint_error_count,
                row.lint_warning_count,
            );
        }
    }
    if !report.risk_flags.is_empty() {
        println!("\n问题与提醒（{} 项）：", report.risk_flags.len());
        for flag in &report.risk_flags {
            let tag = match flag.level {
                xcpc_judge::AuditRiskLevel::Error => "问题",
                xcpc_judge::AuditRiskLevel::Warning => "提醒",
            };
            println!("  [{tag}] {}", flag.message);
        }
    }
    if !report.next_steps.is_empty() {
        println!("\n下一步建议：");
        for step in &report.next_steps {
            println!("  - {step}");
        }
    }
    if let Some(path) = markdown_path {
        println!("已导出检查报告：{path}");
    }
}

fn run_group(cli: &Cli, action: &GroupCommand) -> Result<ExitCode, Fatal> {
    let ws = open_workspace(cli)?;
    match action {
        GroupCommand::List => {
            let (groups, ungrouped_count) = ws.list_groups()?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "groups": groups,
                        "ungrouped_count": ungrouped_count,
                    }))
                    .expect("serializable")
                );
            } else {
                for g in &groups {
                    if g.declared {
                        println!("{}\t{}", g.name, g.count);
                    } else {
                        println!("{}\t{}\t[undeclared]", g.name, g.count);
                    }
                }
                println!("(ungrouped)\t{ungrouped_count}");
            }
            Ok(ExitCode::SUCCESS)
        }
        GroupCommand::Create { name } => {
            let norm = ws.create_group(name)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "name": norm,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("created group {norm}");
            }
            Ok(ExitCode::SUCCESS)
        }
        GroupCommand::Rename { old_name, new_name } => {
            let report = ws.rename_group(old_name, new_name)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "old_name": report.old_name,
                        "new_name": report.new_name,
                        "updated_problems": report.updated_problems,
                    }))
                    .expect("serializable")
                );
            } else {
                println!(
                    "renamed group {} -> {} ({} problem(s) updated)",
                    report.old_name, report.new_name, report.updated_problems
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        GroupCommand::Delete { name } => {
            let report = ws.delete_group(name)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "name": report.name,
                        "ungrouped_problems": report.ungrouped_problems,
                    }))
                    .expect("serializable")
                );
            } else {
                println!(
                    "deleted group {} ({} problem(s) moved to ungrouped)",
                    report.name, report.ungrouped_problems
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        GroupCommand::Reorder { order } => {
            let groups = ws.reorder_groups(order)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "groups": groups,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("reordered groups: {}", groups.join(", "));
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn finish_batch(cli: &Cli, report: &BatchReport, verb: &str) -> ExitCode {
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(report).expect("serializable")
        );
    } else {
        println!(
            "{verb}: total={} succeeded={} skipped={} failed={}",
            report.total, report.succeeded, report.skipped, report.failed
        );
        for item in &report.items {
            let tag = match item.status {
                BatchItemStatus::Ok => "ok",
                BatchItemStatus::Skipped => "skipped",
                BatchItemStatus::Failed => "failed",
            };
            if let Some(ref msg) = item.message {
                println!("  [{tag}] {}: {msg}", item.id);
            } else if let Some(ref code) = item.code {
                println!("  [{tag}] {}: {code}", item.id);
            } else {
                println!("  [{tag}] {}", item.id);
            }
        }
    }
    if report.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn run_problem(cli: &Cli, action: &ProblemCommand) -> Result<ExitCode, Fatal> {
    let ws = open_workspace(cli)?;
    match action {
        ProblemCommand::Duplicate {
            source_id,
            new_id,
            title,
        } => {
            let problem = ws.duplicate_problem(source_id, new_id, title.as_deref())?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "source_id": source_id,
                        "id": problem.id,
                        "title": problem.meta.title,
                        "path": problem.dir,
                    }))
                    .expect("serializable")
                );
            } else {
                println!(
                    "duplicated problem {source_id} -> {} ({})",
                    problem.id, problem.dir
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        ProblemCommand::Group { clear, args } => {
            let (group, ids): (Option<&str>, &[String]) = if *clear {
                (None, args.as_slice())
            } else {
                if args.len() < 2 {
                    return Err(Fatal {
                        code: "invalid_arguments",
                        message: "expected `<GROUP> <ID>...` (or `--clear <ID>...`)".to_owned(),
                        human: Some(
                            "请提供分组名称和至少一个题目 ID（或使用 `--clear <ID>...` 清空分组）。"
                                .to_owned(),
                        ),
                        exit_code: 2,
                    });
                }
                (Some(args[0].as_str()), &args[1..])
            };
            let report = ws.batch_set_group(ids, group)?;
            Ok(finish_batch(cli, &report, "problem group"))
        }
        ProblemCommand::Tag { add, remove, ids } => {
            if add.is_empty() && remove.is_empty() {
                return Err(Fatal {
                    code: "invalid_arguments",
                    message: "specify at least one `--add <TAG>` or `--remove <TAG>`".to_owned(),
                    human: Some("请至少指定一个 `--add <TAG>` 或 `--remove <TAG>`。".to_owned()),
                    exit_code: 2,
                });
            }
            let report = ws.batch_edit_tags(ids, add, remove)?;
            Ok(finish_batch(cli, &report, "problem tag"))
        }
        ProblemCommand::Rating { clear, args } => {
            let (rating, ids): (Option<u32>, &[String]) = if *clear {
                (None, args.as_slice())
            } else {
                if args.len() < 2 {
                    return Err(Fatal {
                        code: "invalid_arguments",
                        message: "expected `<RATING> <ID>...` (or `--clear <ID>...`)".to_owned(),
                        human: Some(
                            "请提供难度数值和至少一个题目 ID（或使用 `--clear <ID>...` 清除难度）。"
                                .to_owned(),
                        ),
                        exit_code: 2,
                    });
                }
                let parsed: u32 = args[0].parse().map_err(|_| Fatal {
                    code: "invalid_arguments",
                    message: format!(
                        "invalid rating {:?}: expected a non-negative integer",
                        args[0]
                    ),
                    human: Some(format!("难度值「{}」无效，请输入非负整数。", args[0])),
                    exit_code: 2,
                })?;
                (Some(parsed), &args[1..])
            };
            let report = ws.batch_set_rating(ids, rating)?;
            Ok(finish_batch(cli, &report, "problem rating"))
        }
        ProblemCommand::Delete { ids, yes } => {
            if ws.is_read_only() {
                return Err(xcpc_storage::Error::ReadOnly.into());
            }
            if !*yes {
                eprint!(
                    "Delete {} problem(s) ({})? Type \"yes\" to confirm: ",
                    ids.len(),
                    ids.join(", ")
                );
                let _ = std::io::stderr().flush();
                let mut line = String::new();
                let n = std::io::stdin()
                    .read_line(&mut line)
                    .context("reading confirmation from stdin")?;
                if n == 0 || !matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                    return Err(Fatal {
                        code: "unconfirmed",
                        message: "delete aborted: confirmation not received".to_owned(),
                        human: Some(
                            "删除已取消：未收到确认。请使用 `--yes` 或输入 `yes` 确认删除。"
                                .to_owned(),
                        ),
                        exit_code: 2,
                    });
                }
            }
            let report = ws.batch_delete_problems(ids)?;
            Ok(finish_batch(cli, &report, "problem delete"))
        }
        ProblemCommand::FromUrl { url, id, title } => {
            if ws.is_read_only() {
                return Err(xcpc_storage::Error::ReadOnly.into());
            }
            let data_dir = resolve_data_dir()?;
            let engine = xcpc_sync::SyncEngine::default_for_data_dir(data_dir);
            let Some(resolved) = engine.resolve_problem_url(url)? else {
                return Err(Fatal {
                    code: "unsupported_url",
                    message: format!("no enabled oj_source plugin could parse URL: {url}"),
                    human: Some(format!(
                        "无法解析该链接「{url}」：当前已启用的 OJ 插件均不支持此 URL。"
                    )),
                    exit_code: 2,
                });
            };

            if let Some(existing_id) =
                ws.find_problem_by_source(&resolved.platform, &resolved.problem_key)?
            {
                let existing = ws.load_problem(&existing_id)?;
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "ok": true,
                            "already_exists": true,
                            "id": existing.id,
                            "title": existing.meta.title,
                            "path": existing.dir,
                            "platform": resolved.platform,
                            "problem_key": resolved.problem_key,
                            "url": resolved.url,
                            "rating": existing.meta.rating,
                            "tags": existing.meta.tags,
                            "sources": existing.meta.sources,
                        }))
                        .expect("serializable")
                    );
                } else {
                    println!(
                        "problem already exists: {} ({}) [{}:{}]",
                        existing.id, existing.dir, resolved.platform, resolved.problem_key
                    );
                }
                return Ok(ExitCode::SUCCESS);
            }

            let slug = id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(&resolved.suggested_slug);
            let final_title = title
                .as_deref()
                .or(resolved.title.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{} {}", resolved.platform, resolved.problem_key));
            let rating = resolved
                .normalized_rating
                .or(resolved.raw_rating)
                .and_then(|r| u32::try_from(r).ok());
            let opts = CreateProblemOptions {
                sources: vec![ProblemSource {
                    platform: resolved.platform.clone(),
                    id: resolved.problem_key.clone(),
                    url: Some(resolved.url.clone()),
                }],
                tags: resolved.tags.clone(),
                difficulty: None,
                rating,
            };
            let created = ws.create_problem_with_options(slug, &final_title, &opts)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "already_exists": false,
                        "id": created.id,
                        "title": created.meta.title,
                        "path": created.dir,
                        "platform": resolved.platform,
                        "problem_key": resolved.problem_key,
                        "url": resolved.url,
                        "rating": created.meta.rating,
                        "tags": created.meta.tags,
                        "sources": created.meta.sources,
                    }))
                    .expect("serializable")
                );
            } else {
                println!(
                    "created problem {} ({}) [{}:{}]",
                    created.id, created.dir, resolved.platform, resolved.problem_key
                );
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn resolve_data_dir() -> Result<PathBuf, Fatal> {
    xcpc_sync::default_data_dir().ok_or_else(|| Fatal {
        code: "invalid_data_dir",
        message: "cannot determine XCPC data directory; set XCPC_DATA_DIR".to_owned(),
        human: Some("无法定位全局数据目录，请设置环境变量 XCPC_DATA_DIR。".to_owned()),
        exit_code: 2,
    })
}

struct CliWorkspaceReader {
    ws: Workspace,
}

impl xcpc_plugin::WorkspaceReader for CliWorkspaceReader {
    fn list_problems(&self) -> xcpc_plugin::Result<Vec<xcpc_plugin::WorkspaceProblemSummary>> {
        let ids = self
            .ws
            .list_problems()
            .map_err(|e| xcpc_plugin::PluginError::InvalidPackage(e.to_string()))?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Ok(p) = self.ws.load_problem(&id) {
                out.push(xcpc_plugin::WorkspaceProblemSummary {
                    id: p.id,
                    title: p.meta.title,
                    tags: p.meta.tags,
                    difficulty: p.meta.rating.map(|r| r.to_string()),
                });
            }
        }
        Ok(out)
    }

    fn get_problem(
        &self,
        id: &str,
    ) -> xcpc_plugin::Result<Option<xcpc_plugin::WorkspaceProblemDetail>> {
        match self.ws.load_problem(id) {
            Ok(p) => {
                let statement_md = p
                    .statement(Language::Chinese)
                    .or_else(|| p.statements.first())
                    .map(|s| s.file.text.clone());
                let editorial_md = p.editorial.map(|e| e.text);
                Ok(Some(xcpc_plugin::WorkspaceProblemDetail {
                    id: p.id,
                    title: p.meta.title,
                    tags: p.meta.tags,
                    difficulty: p.meta.rating.map(|r| r.to_string()),
                    statement_md,
                    editorial_md,
                }))
            }
            Err(xcpc_storage::Error::NotFound(_)) => Ok(None),
            Err(e) => Err(xcpc_plugin::PluginError::InvalidPackage(e.to_string())),
        }
    }
}

fn plugin_summary_json(p: &xcpc_plugin::InstalledPlugin) -> serde_json::Value {
    json!({
        "id": p.manifest.id,
        "name": p.manifest.name,
        "version": p.manifest.version,
        "description": p.manifest.description,
        "origin": p.origin,
        "enabled": p.enabled,
        "capabilities": p.manifest.capabilities,
        "domains": p.manifest.permissions.domains,
        "has_unsandboxed_ui": p.manifest.ui_entry.is_some(),
        "last_error": p.last_error,
        "local_dir": p.local_dir.as_ref().map(|d| d.display().to_string()),
    })
}

fn format_plugin_origin(origin: xcpc_plugin::PluginOrigin) -> &'static str {
    match origin {
        xcpc_plugin::PluginOrigin::Builtin => "builtin",
        xcpc_plugin::PluginOrigin::LocalOverride => "local_override",
        xcpc_plugin::PluginOrigin::Local => "local",
    }
}

fn format_plugin_capability(cap: xcpc_plugin::PluginCapability) -> &'static str {
    match cap {
        xcpc_plugin::PluginCapability::OjSource => "oj_source",
        xcpc_plugin::PluginCapability::Tool => "tool",
        xcpc_plugin::PluginCapability::Commands => "commands",
        xcpc_plugin::PluginCapability::Importer => "importer",
        xcpc_plugin::PluginCapability::Exporter => "exporter",
        xcpc_plugin::PluginCapability::Credentials => "credentials",
    }
}

fn run_plugin(cli: &Cli, action: &PluginCommand) -> Result<ExitCode, Fatal> {
    match action {
        PluginCommand::List => {
            let pm = xcpc_plugin::PluginManager::new(resolve_data_dir()?);
            let plugins = pm.list_plugins()?;
            if cli.json {
                let items: Vec<_> = plugins.iter().map(plugin_summary_json).collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "plugins": items,
                    }))
                    .expect("serializable")
                );
            } else {
                for p in &plugins {
                    let state = if p.enabled { "enabled" } else { "disabled" };
                    let caps = if p.manifest.capabilities.is_empty() {
                        "-".to_owned()
                    } else {
                        p.manifest
                            .capabilities
                            .iter()
                            .map(|c| format_plugin_capability(*c))
                            .collect::<Vec<_>>()
                            .join(",")
                    };
                    let domains = if p.manifest.permissions.domains.is_empty() {
                        "-".to_owned()
                    } else {
                        p.manifest.permissions.domains.join(",")
                    };
                    let ui_flag = if p.manifest.ui_entry.is_some() {
                        "\t[unsandboxed-ui]"
                    } else {
                        ""
                    };
                    println!(
                        "{}\t{}\t{}\t{}\t{}\t{}\t{}{}",
                        p.manifest.id,
                        p.manifest.version,
                        format_plugin_origin(p.origin),
                        state,
                        caps,
                        domains,
                        p.manifest.name,
                        ui_flag,
                    );
                    if let Some(ref err) = p.last_error {
                        println!("  last_error: {err}");
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        PluginCommand::Install { path } => {
            let pm = xcpc_plugin::PluginManager::new(resolve_data_dir()?);
            let installed = if path.is_dir() {
                pm.install_from_dir(path)?
            } else if path.is_file() {
                pm.install_from_zip(path)?
            } else {
                return Err(xcpc_plugin::PluginError::InvalidPackage(format!(
                    "plugin path does not exist: {}",
                    path.display()
                ))
                .into());
            };
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "plugin": plugin_summary_json(&installed),
                    }))
                    .expect("serializable")
                );
            } else {
                println!(
                    "installed plugin {} ({}) [{}]",
                    installed.manifest.id,
                    installed.manifest.version,
                    format_plugin_origin(installed.origin)
                );
                if installed.manifest.ui_entry.is_some() {
                    println!(
                        "  warning: this plugin includes unsandboxed desktop UI code ({})",
                        installed.manifest.ui_entry.as_deref().unwrap_or("ui.js")
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        PluginCommand::Enable { id } => {
            let pm = xcpc_plugin::PluginManager::new(resolve_data_dir()?);
            pm.set_enabled(id, true)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "id": id,
                        "enabled": true,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("enabled plugin {id}");
            }
            Ok(ExitCode::SUCCESS)
        }
        PluginCommand::Disable { id } => {
            let pm = xcpc_plugin::PluginManager::new(resolve_data_dir()?);
            pm.set_enabled(id, false)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "id": id,
                        "enabled": false,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("disabled plugin {id}");
            }
            Ok(ExitCode::SUCCESS)
        }
        PluginCommand::Export { id } => {
            let pm = xcpc_plugin::PluginManager::new(resolve_data_dir()?);
            let dest = pm.export_builtin_copy(id)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "id": id,
                        "path": dest.display().to_string(),
                    }))
                    .expect("serializable")
                );
            } else {
                println!("exported plugin {id} to {}", dest.display());
            }
            Ok(ExitCode::SUCCESS)
        }
        PluginCommand::Init { dir, kind } => {
            xcpc_plugin::init_plugin_scaffold(dir, kind.into_scaffold_kind())?;
            let files = ["manifest.toml", "index.js", "xcpc-plugin.d.ts"];
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "dir": dir.display().to_string(),
                        "kind": kind.as_str(),
                        "files": files,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("scaffolded {} plugin in {}", kind.as_str(), dir.display());
                for f in files {
                    println!("  {f}");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        PluginCommand::Test { dir, action, args } => {
            let manifest = xcpc_plugin::PluginManifest::from_file(&dir.join("manifest.toml"))?;
            let (resolved_action, resolved_args) = match action {
                Some(a) => (a.clone(), args.clone().unwrap_or_default()),
                None => {
                    let is_tool_only = manifest.has_capability(xcpc_plugin::PluginCapability::Tool)
                        && !manifest.has_capability(xcpc_plugin::PluginCapability::OjSource);
                    if is_tool_only {
                        let first_tool = manifest
                            .tools
                            .first()
                            .map(|t| t.id.as_str())
                            .unwrap_or("hello");
                        let default_args =
                            serde_json::to_string(&json!([first_tool, {}])).expect("serializable");
                        ("runTool".to_owned(), args.clone().unwrap_or(default_args))
                    } else {
                        (
                            "verify".to_owned(),
                            args.clone()
                                .unwrap_or_else(|| r#"{"handle":"test"}"#.to_owned()),
                        )
                    }
                }
            };
            let ws_reader: Option<Arc<dyn xcpc_plugin::WorkspaceReader>> =
                open_workspace(cli).ok().map(|ws| {
                    Arc::new(CliWorkspaceReader { ws }) as Arc<dyn xcpc_plugin::WorkspaceReader>
                });
            let report = xcpc_plugin::test_plugin_dir(
                dir,
                &resolved_action,
                &resolved_args,
                Arc::new(xcpc_plugin::UreqHttpClient::new()),
                Arc::new(xcpc_plugin::SystemKeyringStore::new()),
                ws_reader,
            )?;
            if cli.json {
                let mut val = serde_json::to_value(&report).expect("serializable");
                val["ok"] = json!(true);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&val).expect("serializable")
                );
            } else {
                println!(
                    "plugin {} :: {} ({} ms)",
                    report.plugin_id, report.action, report.duration_ms
                );
                for log in &report.logs {
                    println!("  [{}] {}", log.level, log.message);
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report.result).expect("serializable")
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        PluginCommand::Repo { action } => {
            let pm = xcpc_plugin::PluginManager::new(resolve_data_dir()?);
            match action {
                PluginRepoCommand::List => {
                    let repos = pm.list_repos()?;
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json!({
                                "ok": true,
                                "repos": repos,
                            }))
                            .expect("serializable")
                        );
                    } else if repos.is_empty() {
                        println!("no plugin repositories configured");
                    } else {
                        for r in &repos {
                            if let Some(ref name) = r.name {
                                println!("{}\t{}", r.url, name);
                            } else {
                                println!("{}", r.url);
                            }
                        }
                    }
                    Ok(ExitCode::SUCCESS)
                }
                PluginRepoCommand::Add { url, name } => {
                    pm.add_repo(url, name.clone())?;
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json!({
                                "ok": true,
                                "url": url.trim(),
                                "name": name,
                            }))
                            .expect("serializable")
                        );
                    } else {
                        println!("added plugin repo {}", url.trim());
                    }
                    Ok(ExitCode::SUCCESS)
                }
                PluginRepoCommand::Remove { url } => {
                    let removed = pm.remove_repo(url)?;
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json!({
                                "ok": true,
                                "url": url,
                                "removed": removed,
                            }))
                            .expect("serializable")
                        );
                    } else if removed {
                        println!("removed plugin repo {url}");
                    } else {
                        println!("plugin repo not found: {url}");
                    }
                    Ok(ExitCode::SUCCESS)
                }
            }
        }
    }
}

fn run_sync(
    cli: &Cli,
    action: Option<&SyncCommand>,
    platform: Option<&str>,
    full: bool,
) -> Result<ExitCode, Fatal> {
    let data_dir = resolve_data_dir()?;
    match action {
        Some(SyncCommand::Handle {
            platform,
            handle,
            clear,
        }) => {
            let platform_norm = platform.trim().to_ascii_lowercase();
            if platform_norm.is_empty() {
                return Err(Fatal {
                    code: "invalid_arguments",
                    message: "platform ID cannot be empty".to_owned(),
                    human: Some("平台 ID 不能为空。".to_owned()),
                    exit_code: 2,
                });
            }
            let mut config = xcpc_sync::SyncConfig::load(&data_dir)?;
            if *clear {
                config.set_handle(&platform_norm, "");
                config.save(&data_dir)?;
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "ok": true,
                            "platform": platform_norm,
                            "handle": serde_json::Value::Null,
                        }))
                        .expect("serializable")
                    );
                } else {
                    println!("cleared handle for {platform_norm}");
                }
            } else if let Some(h) = handle {
                config.set_handle(&platform_norm, h);
                config.save(&data_dir)?;
                let saved = config.handle_for(&platform_norm).map(str::to_owned);
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "ok": true,
                            "platform": platform_norm,
                            "handle": saved,
                        }))
                        .expect("serializable")
                    );
                } else if let Some(ref val) = saved {
                    println!("set {platform_norm} handle to {val}");
                } else {
                    println!("cleared handle for {platform_norm}");
                }
            } else {
                let current = config.handle_for(&platform_norm).map(str::to_owned);
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "ok": true,
                            "platform": platform_norm,
                            "handle": current,
                        }))
                        .expect("serializable")
                    );
                } else if let Some(val) = current {
                    println!("{platform_norm}\t{val}");
                } else {
                    println!("{platform_norm}\t(not set)");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        None => {
            let engine = xcpc_sync::SyncEngine::default_for_data_dir(data_dir);
            let report = engine.sync(&xcpc_sync::SyncOptions {
                platform: platform.map(str::to_owned),
                full,
            })?;
            let all_ok = report.platforms.iter().all(|p| p.ok);
            if cli.json {
                let mut val = serde_json::to_value(&report).expect("serializable");
                val["ok"] = json!(all_ok);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&val).expect("serializable")
                );
            } else if report.platforms.is_empty() {
                println!(
                    "no enabled oj_source platforms with configured handles (synced_at: {})",
                    report.synced_at
                );
            } else {
                println!("synced at {}:", report.synced_at);
                for p in &report.platforms {
                    if p.ok {
                        println!(
                            "  [ok] {} ({}): submissions={} rating_history={} contests={} account={} summary={}",
                            p.platform,
                            p.handle,
                            p.submissions_synced,
                            p.rating_history_synced,
                            p.contests_synced,
                            p.account_synced,
                            p.summary_synced,
                        );
                    } else {
                        println!(
                            "  [error] {} ({}): {}",
                            p.platform,
                            p.handle,
                            p.error.as_deref().unwrap_or("sync failed")
                        );
                    }
                }
            }
            if all_ok {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(1))
            }
        }
    }
}

fn run_training(cli: &Cli, action: &TrainingCommand) -> Result<ExitCode, Fatal> {
    let ws = open_workspace(cli)?;
    match action {
        TrainingCommand::Status { problem_id, status } => {
            if ws.is_read_only() {
                return Err(xcpc_storage::Error::ReadOnly.into());
            }
            let problem = ws.load_problem(problem_id)?;
            let log_status = status.into_log_status();
            let entry = TrainingLogEntry {
                at: xcpc_sync::now_rfc3339_utc(),
                problem_id: Some(problem.id.clone()),
                source: problem.meta.sources.first().cloned(),
                status: log_status,
            };
            ws.append_training_log(&entry)?;

            let synced = match resolve_data_dir() {
                Ok(data_dir) if data_dir.join(xcpc_sync::DB_FILENAME).is_file() => {
                    xcpc_sync::SyncDb::open(&data_dir)?
                        .get_problem_sync_status(&problem.meta.sources)?
                }
                _ => SyncedProblemStatus::default(),
            };
            let derived =
                ws.derive_problem_training_status(&problem.id, &problem.meta.sources, synced)?;

            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "problem_id": problem.id,
                        "recorded": log_status,
                        "status": derived,
                        "synced": {
                            "ac": synced.ac,
                            "attempted": synced.attempted,
                        },
                        "at": entry.at,
                    }))
                    .expect("serializable")
                );
            } else {
                println!(
                    "{}\trecorded={}\tstatus={}",
                    problem.id, log_status, derived
                );
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn run_template(cli: &Cli, action: &TemplateCommand) -> Result<ExitCode, Fatal> {
    let ws = open_workspace(cli)?;
    match action {
        TemplateCommand::List => {
            let engine = publish::engine(&ws, &fonts(cli));
            let templates = publish::list_templates(&ws, &engine);
            let presets = xcpc_engine::list_presets();
            let default_tpl = ws
                .default_template()
                .unwrap_or_else(|| "olymp-classic".into());
            let workspace_options = ws.workspace_template_options();
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "default": default_tpl,
                        "workspace_options": workspace_options,
                        "templates": templates,
                        "presets": presets,
                    }))
                    .expect("serializable")
                );
            } else {
                for tpl in &templates {
                    let mark = if tpl.id == default_tpl { "*" } else { " " };
                    println!(
                        "{mark}\t{}\t{}\t{}\t{} field(s)",
                        tpl.id,
                        tpl.path,
                        tpl.name.resolve("zh-CN"),
                        tpl.schema.fields.len()
                    );
                }
                if !presets.is_empty() {
                    println!("\npresets:");
                    for p in &presets {
                        println!(
                            "  {}\t{}\t{}",
                            p.id,
                            p.name.resolve("zh-CN"),
                            p.description.resolve("zh-CN")
                        );
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        TemplateCommand::Schema { name } => {
            let engine = publish::engine(&ws, &fonts(cli));
            let Some(info) = publish::inspect_template(&ws, &engine, name) else {
                return Err(xcpc_storage::Error::NotFound(format!("template {name:?}")).into());
            };
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "template": info,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("{} ({})", info.name.resolve("zh-CN"), info.id);
                if let Some(desc) = &info.description {
                    println!("{}", desc.resolve("zh-CN"));
                }
                for f in &info.schema.fields {
                    println!(
                        "  {}\t{:?}\t{:?}\t{}",
                        f.key,
                        f.scope,
                        f.field_type,
                        f.label.resolve("zh-CN")
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        TemplateCommand::Add { preset, name } => {
            let rel = ws.install_preset_template(preset, name.as_deref())?;
            let id = name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(preset.as_str());
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "preset": preset,
                        "id": id,
                        "path": rel,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("installed preset {preset} -> {rel}");
            }
            Ok(ExitCode::SUCCESS)
        }
        TemplateCommand::Fork { source, new_name } => {
            let rel = publish::fork_template(&ws, source, new_name)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "source": source,
                        "id": new_name,
                        "path": rel,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("forked template {source} -> {rel}");
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn run_contest(cli: &Cli, action: &ContestCommand) -> Result<ExitCode, Fatal> {
    let ws = open_workspace(cli)?;
    match action {
        ContestCommand::Compose {
            id,
            problems,
            name,
            location,
            date,
            author,
            language,
            template,
            cover,
            blank_pages,
            show_metadata,
        } => {
            let patch = ContestMetaPatch {
                name: name.clone(),
                location: location.clone().map_or(Patch::Keep, Patch::Set),
                date: date.clone().map_or(Patch::Keep, Patch::Set),
                author: author.clone().map_or(Patch::Keep, Patch::Set),
                language: language.map(Lang::into_language),
                template: template.clone().map_or(Patch::Keep, Patch::Set),
                show_metadata: *show_metadata,
                cover: cover.map_or(Patch::Keep, Patch::Set),
                blank_pages: *blank_pages,
                ..Default::default()
            };
            let has_patch = patch != ContestMetaPatch::default();
            let contest = ws.compose_contest(id, problems, has_patch.then_some(&patch))?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "id": contest.id,
                        "dir": contest.dir,
                        "meta": contest.meta,
                    }))
                    .expect("serializable")
                );
            } else {
                println!(
                    "composed contest {} ({} problem(s))",
                    contest.id,
                    contest.meta.problems.len()
                );
                for item in &contest.meta.problems {
                    println!("  {}\t{}", item.label, item.id);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        ContestCommand::Override {
            contest,
            label,
            title,
            clear_title,
            time,
            clear_time,
            memory,
            clear_memory,
            input,
            clear_input,
            output,
            clear_output,
            show_metadata,
            clear_show_metadata,
        } => {
            let patch = OverridePatch {
                title: flag_patch(title.clone(), *clear_title),
                limits: LimitsPatch {
                    time: flag_patch(time.clone(), *clear_time),
                    memory: flag_patch(memory.clone(), *clear_memory),
                    input: flag_patch(input.clone(), *clear_input),
                    output: flag_patch(output.clone(), *clear_output),
                },
                show_metadata: flag_patch(*show_metadata, *clear_show_metadata),
                ..Default::default()
            };
            let updated = ws.set_override(contest, label, &patch)?;
            let item = updated.meta.problems.iter().find(|p| p.label == *label);
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "id": updated.id,
                        "label": label,
                        "item": item,
                        "meta": updated.meta,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("updated overrides for {} {}", updated.id, label);
            }
            Ok(ExitCode::SUCCESS)
        }
        ContestCommand::Freeze { id } => {
            let mut engine = publish::engine(&ws, &fonts(cli));
            let outcome = publish::freeze_contest(&ws, &mut engine, id)?;
            let mut report = outcome.report;
            report.ok = outcome.frozen;
            let out = FreezeOutput {
                id: id.clone(),
                frozen: outcome.frozen,
                report,
                outputs: outcome.outputs,
            };
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&out).expect("serializable")
                );
            } else {
                print_report(&out.report, &out.outputs, Some((&ws, id.as_str())));
                if out.frozen {
                    println!("已定稿比赛「{}」", out.id);
                } else {
                    eprintln!("比赛「{}」未能定稿，请先修复上述错误。", out.id);
                }
            }
            if out.frozen {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(1))
            }
        }
        ContestCommand::Unfreeze { id, yes } => {
            if ws.is_read_only() {
                return Err(xcpc_storage::Error::ReadOnly.into());
            }
            let diffs = ws.unfreeze_diff(id)?;
            if !cli.json {
                if diffs.is_empty() {
                    println!("no differences between snapshot and problem library");
                } else {
                    println!(
                        "differences between snapshot and problem library ({} problem(s)):",
                        diffs.len()
                    );
                    for diff in &diffs {
                        let kinds = diff
                            .kinds
                            .iter()
                            .map(|k| format_diff_kind(*k))
                            .collect::<Vec<_>>()
                            .join(", ");
                        if diff.changed_files.is_empty() {
                            println!("  {} ({}): {}", diff.label, diff.problem_id, kinds);
                        } else {
                            println!(
                                "  {} ({}): {} [{}]",
                                diff.label,
                                diff.problem_id,
                                kinds,
                                diff.changed_files.join(", ")
                            );
                        }
                    }
                }
                let _ = std::io::stdout().flush();
            }

            if !*yes {
                eprint!("Type contest ID {id:?} to confirm unfreeze: ");
                let _ = std::io::stderr().flush();
                let mut line = String::new();
                let n = std::io::stdin()
                    .read_line(&mut line)
                    .context("reading confirmation from stdin")?;
                if n == 0 || line.trim() != id {
                    return Err(Fatal {
                        code: "unconfirmed",
                        message: format!("unfreeze aborted: confirmation did not match {id:?}"),
                        human: Some(format!(
                            "重新编辑已取消：输入的确认标识与比赛「{id}」不一致。请重新执行并输入正确的比赛 ID。"
                        )),
                        exit_code: 2,
                    });
                }
            }

            let contest = ws.unfreeze_contest(id)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "id": contest.id,
                        "frozen": contest.meta.frozen,
                        "diffs": diffs,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("unfrozen contest {}", contest.id);
            }
            Ok(ExitCode::SUCCESS)
        }
        ContestCommand::Archive { id } => {
            let meta = ws.set_contest_archived(id, true)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "id": id,
                        "archived": meta.archived,
                        "frozen": meta.frozen,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("archived contest {id}");
            }
            Ok(ExitCode::SUCCESS)
        }
        ContestCommand::Unarchive { id } => {
            let meta = ws.set_contest_archived(id, false)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "id": id,
                        "archived": meta.archived,
                        "frozen": meta.frozen,
                    }))
                    .expect("serializable")
                );
            } else {
                println!("unarchived contest {id}");
            }
            Ok(ExitCode::SUCCESS)
        }
        ContestCommand::Audit { id, markdown } => {
            let report = views::audit_contest_with_lint(&ws, id)?;
            let md_written = if let Some(path) = markdown {
                let md = xcpc_judge::render_contest_audit_markdown(&report);
                Some(write_explicit_file(path, md.as_bytes())?)
            } else {
                None
            };
            if cli.json {
                let mut value = serde_json::to_value(&report).expect("serializable");
                value["ok"] = json!(true);
                if let Some(ref p) = md_written {
                    value["markdown"] = json!(p);
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).expect("serializable")
                );
            } else {
                print_contest_audit_report(&report, md_written.as_deref());
            }
            Ok(ExitCode::SUCCESS)
        }
        ContestCommand::VerifyExternal {
            id,
            path,
            auto_build,
            jobs,
            markdown,
        } => {
            if *auto_build && ws.is_read_only() {
                return Err(xcpc_storage::Error::ReadOnly.into());
            }
            let opts = xcpc_judge::ExternalRunOptions {
                auto_build: *auto_build,
                jobs: *jobs,
                ..Default::default()
            };
            let report =
                xcpc_judge::run_external_solutions_for_contest(&ws, id, path, &opts, None)?;
            let md_written = if let Some(md_path) = markdown {
                let md = xcpc_judge::render_external_contest_markdown(&report);
                Some(write_explicit_file(md_path, md.as_bytes())?)
            } else {
                None
            };
            if cli.json {
                let mut value = serde_json::to_value(&report).expect("serializable");
                value["ok"] = json!(true);
                if let Some(ref p) = md_written {
                    value["markdown"] = json!(p);
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).expect("serializable")
                );
            } else {
                print_external_contest_report(&report, md_written.as_deref());
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Atomically writes bytes to an explicit user-specified path, creating parent directories if needed.
fn write_explicit_file(path: &std::path::Path, bytes: &[u8]) -> Result<String, Fatal> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    xcpc_storage::write_atomic(path, bytes)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path.display().to_string())
}

/// Writes to `explicit` (a path the user gave, anywhere), else to `default` in the
/// workspace (atomic, sandboxed, refused when read-only).
fn write_output(
    ws: &Workspace,
    explicit: Option<&PathBuf>,
    default: &str,
    bytes: &[u8],
) -> Result<String, Fatal> {
    match explicit {
        Some(path) => {
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("creating {}", dir.display()))?;
            }
            xcpc_storage::write_atomic(path, bytes)
                .with_context(|| format!("writing {}", path.display()))?;
            Ok(path.display().to_string())
        }
        None => {
            ws.write(default, bytes)?;
            Ok(ws.root().join(default).display().to_string())
        }
    }
}

/// `contest`: the contest being checked, so a re-check hint carries `--contest` when that
/// contest overrides the problem's limits.
fn print_report(report: &Report, outputs: &[String], contest: Option<(&Workspace, &str)>) {
    for d in &report.diagnostics {
        eprintln!("{}", d.display());
        for hint in &d.hints {
            eprintln!("  提示：{hint}");
        }
        if matches!(
            d.code.as_deref(),
            Some("package_unverified" | "package_stale" | "package_failed")
        ) && let Some(slug) = d
            .file
            .as_deref()
            .and_then(|f| f.strip_prefix("problems/"))
            .and_then(|rest| rest.split('/').next())
            .filter(|s| !s.is_empty())
        {
            let in_contest = contest
                .filter(|(ws, cid)| {
                    matches!(
                        xcpc_judge::contest_limits_key(ws, slug, Some(cid)),
                        Ok(Some(_))
                    )
                })
                .map(|(_, cid)| format!(" --contest {cid}"))
                .unwrap_or_default();
            eprintln!("  提示：请先运行 `xcpc package build {slug} --strict{in_contest}`");
        }
    }
    if let Some(pages) = report.pages {
        eprintln!("{pages} page(s) in {} ms", report.compile_ms);
    }
    for p in &report.problems {
        eprintln!(
            "  {} {}: page {} ({} page(s))",
            p.label, p.id, p.first, p.count
        );
    }
    for path in outputs {
        eprintln!("wrote {path}");
    }
}

fn finish(cli: &Cli, out: Output) -> ExitCode {
    let ok = out.report.ok;
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&out).expect("serializable")
        );
    } else {
        print_report(&out.report, &out.outputs, None);
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn print_problem_detail(detail: &views::ProblemDetail) {
    println!("problem {}: {}", detail.id, detail.meta.title);
    println!("  dir: {}", detail.dir);
    println!("  meta_hash: {}", detail.meta_hash);
    if let Some(rating) = detail.meta.rating {
        println!("  rating: {rating}");
    }
    if !detail.meta.tags.is_empty() {
        println!("  tags: {}", detail.meta.tags.join(", "));
    }
    if let Some(author) = &detail.meta.author {
        println!("  author: {author}");
    }
    if detail.meta.limits.time.is_some()
        || detail.meta.limits.memory.is_some()
        || detail.meta.limits.input.is_some()
        || detail.meta.limits.output.is_some()
    {
        println!(
            "  limits: time={} memory={} input={} output={}",
            detail.meta.limits.time.as_deref().unwrap_or("-"),
            detail.meta.limits.memory.as_deref().unwrap_or("-"),
            detail.meta.limits.input.as_deref().unwrap_or("stdin"),
            detail.meta.limits.output.as_deref().unwrap_or("stdout"),
        );
    }
    if !detail.meta.sources.is_empty() {
        let srcs = detail
            .meta
            .sources
            .iter()
            .map(|s| format!("{}:{}", s.platform, s.id))
            .collect::<Vec<_>>()
            .join(", ");
        println!("  sources: {srcs}");
    }
    let sample_names: Vec<&str> = detail.samples.iter().map(|s| s.name.as_str()).collect();
    println!(
        "  samples: {}",
        if sample_names.is_empty() {
            "(none)".to_owned()
        } else {
            sample_names.join(", ")
        }
    );
    println!(
        "  solutions: {}",
        if detail.solutions.is_empty() {
            "(none)".to_owned()
        } else {
            detail.solutions.join(", ")
        }
    );
    for st in &detail.statements {
        println!("--- {} (sha256={}) ---", st.path, st.hash);
        print!("{}", st.text);
        if !st.text.ends_with('\n') {
            println!();
        }
    }
    if let Some(ed) = &detail.editorial {
        println!("--- {} (sha256={}) ---", ed.path, ed.hash);
        print!("{}", ed.text);
        if !ed.text.ends_with('\n') {
            println!();
        }
    }
}

fn print_contest_detail(detail: &views::ContestDetail) {
    let mut badges = vec![if detail.frozen { "frozen" } else { "unfrozen" }];
    if detail.archived {
        badges.push("archived");
    }
    println!(
        "contest {}: {} [{}]",
        detail.id,
        detail.name,
        badges.join(", ")
    );
    if let Some(loc) = &detail.location {
        println!("  location: {loc}");
    }
    if let Some(date) = &detail.date {
        println!("  date: {date}");
    }
    if let Some(author) = &detail.author {
        println!("  author: {author}");
    }
    println!("  language: {}", detail.language.name());
    if let Some(pages) = detail.pages {
        println!("  pages: {pages}");
    }
    for p in &detail.problems {
        let title = p.title.as_deref().unwrap_or("(missing)");
        let page_info = match (p.first, p.count) {
            (Some(first), Some(count)) => format!("p.{first} ({count}p)"),
            _ => "p.-".to_owned(),
        };
        let mut ov = Vec::new();
        if let Some(t) = &p.overrides.title {
            ov.push(format!("title={t:?}"));
        }
        if let Some(lim) = &p.overrides.limits {
            if let Some(t) = &lim.time {
                ov.push(format!("time={t}"));
            }
            if let Some(m) = &lim.memory {
                ov.push(format!("memory={m}"));
            }
            if let Some(i) = &lim.input {
                ov.push(format!("input={i:?}"));
            }
            if let Some(o) = &lim.output {
                ov.push(format!("output={o:?}"));
            }
        }
        if let Some(sm) = p.overrides.show_metadata {
            ov.push(format!("show-metadata={sm}"));
        }
        if ov.is_empty() {
            println!("  {}\t{}\t{}\t{}", p.label, p.id, title, page_info);
        } else {
            println!(
                "  {}\t{}\t{}\t{}\t[{}]",
                p.label,
                p.id,
                title,
                page_info,
                ov.join(", ")
            );
        }
    }
    let tiers: Vec<String> = RatingTier::ALL
        .into_iter()
        .filter_map(|tier| {
            let count = detail
                .stats
                .rating_distribution
                .get(&tier)
                .copied()
                .unwrap_or(0);
            (count > 0).then(|| format!("{}={count}", format_rating_tier(tier)))
        })
        .collect();
    if !tiers.is_empty() {
        println!("  ratings: {}", tiers.join(", "));
    }
    if !detail.stats.tag_counts.is_empty() {
        let tags: Vec<String> = detail
            .stats
            .tag_counts
            .iter()
            .map(|(t, c)| format!("{t}({c})"))
            .collect();
        println!("  tags: {}", tags.join(", "));
    }
}

fn format_rating_tier(tier: RatingTier) -> &'static str {
    match tier {
        RatingTier::Newbie => "newbie",
        RatingTier::Pupil => "pupil",
        RatingTier::Specialist => "specialist",
        RatingTier::Expert => "expert",
        RatingTier::CandidateMaster => "candidate_master",
        RatingTier::Master => "master",
        RatingTier::Grandmaster => "grandmaster",
        RatingTier::Unrated => "unrated",
    }
}

fn format_diff_kind(kind: DiffKind) -> &'static str {
    match kind {
        DiffKind::StatementChanged => "statement changed",
        DiffKind::SamplesChanged => "samples changed",
        DiffKind::EditorialChanged => "editorial changed",
        DiffKind::AssetsChanged => "assets changed",
        DiffKind::MetaChanged => "metadata changed",
        DiffKind::ProblemDeleted => "problem deleted",
    }
}
