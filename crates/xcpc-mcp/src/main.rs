//! `xcpc`: the headless entry point, a shell over the Core crates.
//!
//! - `xcpc build|check|render <problem|contest> <id>`: publish from the workspace;
//! - `xcpc list`, `xcpc fonts`;
//! - `xcpc mcp`: the same over MCP (stdio).
//!
//! With `--json`, stdout carries one JSON object. Exit codes: 0 success, 1 the
//! document has errors, 2 usage, workspace or I/O errors.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::json;
use xcpc_convert::publish::{self, Kind, Report, Target};
use xcpc_domain::Language;
use xcpc_engine::{Engine, FontOptions};
use xcpc_storage::Workspace;

mod mcp;

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
    /// Typeset a problem or a contest and write its PDF.
    Build {
        #[command(flatten)]
        what: What,
        /// Write the PDF here (default: build/<id>.pdf in the workspace).
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// Also write the generated Typst source here.
        #[arg(long)]
        emit_typst: Option<PathBuf>,
    },
    /// Typeset and report diagnostics, pages and overflows; write nothing.
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
    /// List the workspace's problems and contests.
    List,
    /// List the font families the engine sees.
    Fonts,
    /// Serve MCP on stdin and stdout.
    Mcp,
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
        self.language.map(|l| match l {
            Lang::Chinese => Language::Chinese,
            Lang::English => Language::English,
        })
    }
}

/// An error with a machine-readable code.
#[derive(Debug)]
struct Fatal {
    code: &'static str,
    message: String,
}

impl From<xcpc_storage::Error> for Fatal {
    fn from(e: xcpc_storage::Error) -> Self {
        Fatal {
            code: e.code(),
            message: e.to_string(),
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
                eprintln!("error: {}", e.message);
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

fn run(cli: &Cli) -> Result<ExitCode, Fatal> {
    match &cli.command {
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
            let doc = publish::document(&ws, &target, what.kind(), what.language())?;
            if let Some(path) = emit_typst {
                xcpc_storage::write_atomic(path, doc.source.as_bytes())
                    .with_context(|| format!("writing {}", path.display()))?;
            }
            let mut engine = publish::engine(&ws, &fonts(cli));
            let (compiled, report) = publish::compile(&mut engine, &doc);
            let mut outputs = Vec::new();
            if let (Some(compiled), true) = (compiled, report.ok) {
                let pdf = compiled
                    .pdf()
                    .map_err(|e| anyhow::anyhow!("PDF export failed: {}", e[0].message))?;
                outputs.push(write_output(
                    &ws,
                    out.as_ref(),
                    &publish::default_output(&target, what.kind()),
                    &pdf,
                )?);
            }
            Ok(finish(cli, Output { report, outputs }))
        }
        Command::Check { what } => {
            let ws = open_workspace(cli)?;
            let doc = publish::document(&ws, &what.target(), what.kind(), what.language())?;
            let mut engine = publish::engine(&ws, &fonts(cli));
            let (_, report) = publish::compile(&mut engine, &doc);
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
            let doc = publish::document(&ws, &target, what.kind(), what.language())?;
            let mut engine = publish::engine(&ws, &fonts(cli));
            let (compiled, report) = publish::compile(&mut engine, &doc);
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

fn finish(cli: &Cli, out: Output) -> ExitCode {
    let ok = out.report.ok;
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&out).expect("serializable")
        );
    } else {
        for d in &out.report.diagnostics {
            eprintln!("{}", d.display());
            for hint in &d.hints {
                eprintln!("  hint: {hint}");
            }
        }
        if let Some(pages) = out.report.pages {
            eprintln!("{pages} page(s) in {} ms", out.report.compile_ms);
        }
        for p in &out.report.problems {
            eprintln!(
                "  {} {}: page {} ({} page(s))",
                p.label, p.id, p.first, p.count
            );
        }
        for path in &out.outputs {
            eprintln!("wrote {path}");
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
