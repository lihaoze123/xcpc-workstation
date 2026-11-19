//! `xcpc`: the headless entry point (CLI now, `xcpc mcp` later).
//!
//! Exit codes: 0 success, 1 compilation errors, 2 usage or I/O errors.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use xcpc_engine::{Compiled, Diagnostic, Engine, FontOptions};

#[derive(Parser)]
#[command(name = "xcpc", version, about = "XCPC Workstation command line")]
struct Cli {
    /// Print machine-readable JSON on stdout.
    #[arg(long, global = true)]
    json: bool,
    /// Also use the operating system's fonts (after XCPC_FONT_DIRS).
    #[arg(long, global = true)]
    system_fonts: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compile a Typst file and report diagnostics; optionally write a PDF.
    Compile {
        input: PathBuf,
        /// Write the PDF here.
        #[arg(long)]
        pdf: Option<PathBuf>,
    },
    /// Compile a Typst file and render pages to PNG.
    Render {
        input: PathBuf,
        /// Output directory; pages are written as `page-<n>.png` (1-based).
        #[arg(long, short)]
        out: PathBuf,
        /// Pixels per point (2.0 ≈ 144 dpi).
        #[arg(long, default_value_t = 2.0)]
        scale: f32,
        /// Only this page (1-based).
        #[arg(long)]
        page: Option<usize>,
    },
    /// List the font families the engine sees.
    Fonts,
}

#[derive(Serialize)]
struct Report {
    ok: bool,
    pages: Option<usize>,
    compile_ms: Option<u128>,
    errors: Vec<JsonDiagnostic>,
    warnings: Vec<JsonDiagnostic>,
    outputs: Vec<PathBuf>,
}

#[derive(Serialize)]
struct JsonDiagnostic {
    message: String,
    file: Option<String>,
    line: Option<usize>,
    column: Option<usize>,
    hints: Vec<String>,
}

impl From<&Diagnostic> for JsonDiagnostic {
    fn from(d: &Diagnostic) -> Self {
        Self {
            message: d.message.clone(),
            file: d.file.clone(),
            line: d.line,
            column: d.column,
            hints: d.hints.clone(),
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(report) => {
            let ok = report.ok;
            print_report(&cli, &report);
            if ok { ExitCode::SUCCESS } else { ExitCode::from(1) }
        }
        Err(err) => {
            if cli.json {
                println!("{}", serde_json::json!({ "ok": false, "fatal": format!("{err:#}") }));
            } else {
                eprintln!("error: {err:#}");
            }
            ExitCode::from(2)
        }
    }
}

fn run(cli: &Cli) -> Result<Report> {
    let input = match &cli.command {
        Command::Compile { input, .. } | Command::Render { input, .. } => input,
        Command::Fonts => {
            let mut fonts = FontOptions::from_env();
            fonts.system |= cli.system_fonts;
            for (family, faces) in Engine::new(&fonts, None).font_families() {
                println!("{family}\t{faces}");
            }
            std::process::exit(0);
        }
    };
    let compiled = compile(input, cli.system_fonts)?;
    let mut compiled = match compiled {
        Ok(c) => c,
        Err(report) => return Ok(report),
    };
    match &cli.command {
        Command::Compile { pdf, .. } => {
            if let Some(path) = pdf {
                let bytes = compiled.document.pdf().map_err(|errors| {
                    anyhow::anyhow!("PDF export failed: {}", errors[0].message)
                })?;
                write_atomic(path, &bytes)?;
                compiled.report.outputs.push(path.clone());
            }
        }
        Command::Fonts => unreachable!("handled above"),
        Command::Render { out, scale, page, .. } => {
            std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
            let pages: Vec<usize> = match page {
                Some(p) => vec![p.checked_sub(1).context("pages are 1-based")?],
                None => (0..compiled.document.page_count()).collect(),
            };
            for index in pages {
                let png = compiled
                    .document
                    .render_png(index, *scale)
                    .with_context(|| format!("no page {}", index + 1))?;
                let path = out.join(format!("page-{}.png", index + 1));
                write_atomic(&path, &png)?;
                compiled.report.outputs.push(path);
            }
        }
    }
    Ok(compiled.report)
}

struct Success {
    document: Compiled,
    report: Report,
}

fn compile(input: &Path, system_fonts: bool) -> Result<Result<Success, Report>> {
    let text = std::fs::read_to_string(input)
        .with_context(|| format!("reading {}", input.display()))?;
    let root = input
        .canonicalize()
        .with_context(|| format!("resolving {}", input.display()))?
        .parent()
        .map(Path::to_path_buf);
    let mut fonts = FontOptions::from_env();
    fonts.system |= system_fonts;
    let mut engine = Engine::new(&fonts, root);
    engine.set_main(&text);
    Ok(match engine.compile() {
        Ok(document) => {
            let report = Report {
                ok: true,
                pages: Some(document.page_count()),
                compile_ms: Some(document.duration.as_millis()),
                errors: vec![],
                warnings: document.warnings.iter().map(Into::into).collect(),
                outputs: vec![],
            };
            Ok(Success { document, report })
        }
        Err(err) => Err(Report {
            ok: false,
            pages: None,
            compile_ms: None,
            errors: err.errors.iter().map(Into::into).collect(),
            warnings: err.warnings.iter().map(Into::into).collect(),
            outputs: vec![],
        }),
    })
}

fn print_report(cli: &Cli, report: &Report) {
    if cli.json {
        println!("{}", serde_json::to_string_pretty(report).expect("serializable report"));
        return;
    }
    for (kind, list) in [("error", &report.errors), ("warning", &report.warnings)] {
        for d in list {
            let at = match (&d.file, d.line, d.column) {
                (Some(f), Some(l), Some(c)) => format!("{f}:{l}:{c}: "),
                (Some(f), _, _) => format!("{f}: "),
                _ => String::new(),
            };
            eprintln!("{kind}: {at}{}", d.message);
            for hint in &d.hints {
                eprintln!("  hint: {hint}");
            }
        }
    }
    if let (Some(pages), Some(ms)) = (report.pages, report.compile_ms) {
        eprintln!("{pages} page(s) in {ms} ms");
    }
    for path in &report.outputs {
        eprintln!("wrote {}", path.display());
    }
}

/// Writes through a temporary file in the same directory, then renames over `path`.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().context("output path has no file name")?;
    let tmp = dir.join(format!(".{}.tmp-{}", name.to_string_lossy(), std::process::id()));
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("renaming to {}", path.display()))?;
    Ok(())
}
