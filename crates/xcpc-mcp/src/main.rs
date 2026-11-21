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

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::json;
use xcpc_convert::publish::{self, Kind, Report, Target};
use xcpc_domain::{
    ContestMetaPatch, DiffKind, Language, LimitsPatch, OverridePatch, Patch, RatingTier,
    SearchQuery,
};
use xcpc_engine::{Engine, FontOptions};
use xcpc_storage::{Workspace, WorkspaceIndex};

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
        /// Target directory (defaults to current directory `.`).
        dir: Option<PathBuf>,
    },
    /// Typeset a problem or a contest and write its PDF.
    Build {
        #[command(flatten)]
        what: What,
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
    /// Search problems in the workspace by text, tags, rating range, or source platform.
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
    },
    /// Show detailed metadata, content hashes, pages, and statistics for a problem or contest.
    Show { kind: TargetKind, id: String },
    /// Compose contests, manage metadata overrides, freeze snapshots, and unfreeze.
    Contest {
        #[command(subcommand)]
        action: ContestCommand,
    },
    /// List the workspace's problems and contests.
    List,
    /// List the font families the engine sees.
    Fonts,
    /// Serve MCP on stdin and stdout.
    Mcp,
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
enum Lang {
    Chinese,
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

/// An error with a machine-readable code and optional Chinese human message.
#[derive(Debug)]
struct Fatal {
    code: &'static str,
    message: String,
    human: Option<String>,
}

impl From<xcpc_storage::Error> for Fatal {
    fn from(e: xcpc_storage::Error) -> Self {
        Fatal {
            code: e.code(),
            message: e.to_string(),
            human: Some(e.human_message()),
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
            ExitCode::from(2)
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
        Command::Init { example, dir } => {
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
                });
            }
            let ws = if *example {
                xcpc_storage::init_example_workspace(&target_dir)?
            } else {
                xcpc_storage::init_workspace(&target_dir)?
            };
            let root = ws.root().display().to_string();
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &json!({ "ok": true, "root": root, "example": example })
                    )
                    .expect("serializable")
                );
            } else if *example {
                println!("initialized example workspace at {root}");
            } else {
                println!("initialized workspace at {root}");
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
            let contests = ws.list_contests()?;
            if cli.json {
                println!(
                    "{}",
                    json!({ "ok": true, "problems": problems, "contests": contests })
                );
            } else {
                for id in problems {
                    println!("problem\t{id}");
                }
                for id in contests {
                    println!("contest\t{id}");
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
            what,
            out,
            emit_typst,
        } => {
            let ws = open_workspace(cli)?;
            let target = what.target();
            let mut engine = publish::engine(&ws, &fonts(cli));
            let (compiled, doc, report) = publish::check(
                &ws,
                &mut engine,
                &target,
                what.kind(),
                what.language(),
                None,
            )?;
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
                    &publish::default_output(&ws, &target, what.kind()),
                    &pdf,
                )?);
            }
            Ok(finish(cli, Output { report, outputs }))
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
        } => {
            let ws = open_workspace(cli)?;
            let index = WorkspaceIndex::scan(&ws)?;
            let joined = query.join(" ");
            let text = (!joined.trim().is_empty()).then_some(joined);
            let q = SearchQuery {
                text,
                tags: tags.clone(),
                rating_min: *rating_min,
                rating_max: *rating_max,
                platform: platform.clone(),
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
        Command::Contest { action } => run_contest(cli, action),
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
                show_metadata: *show_metadata,
                cover: cover.map_or(Patch::Keep, Patch::Set),
                blank_pages: *blank_pages,
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
                print_report(&out.report, &out.outputs);
                if out.frozen {
                    println!("frozen contest {}", out.id);
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
    }
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

fn print_report(report: &Report, outputs: &[String]) {
    for d in &report.diagnostics {
        eprintln!("{}", d.display());
        for hint in &d.hints {
            eprintln!("  hint: {hint}");
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
        print_report(&out.report, &out.outputs);
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
    let status = if detail.frozen { "frozen" } else { "unfrozen" };
    println!("contest {}: {} [{}]", detail.id, detail.name, status);
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
