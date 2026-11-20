//! `xcpc mcp`: the Model Context Protocol over stdio (newline-delimited JSON-RPC 2.0),
//! the publishing tools of Phase 1 (docs/architecture.md §7.2):
//!
//! - `compile_and_diagnose`: diagnostics in workspace files, pages per problem,
//!   overflows;
//! - `render_page_png`: one page as an image, written nowhere;
//! - `export_pdf`: the PDF, into the workspace (sandboxed; refused with `--read-only`).
//!
//! There is no tool to unfreeze a contest, nor any other write.

use std::io::{BufRead, Write};

use anyhow::Result;
use base64::Engine as _;
use serde_json::{Value, json};
use xcpc_convert::publish::{self, Kind, Target};
use xcpc_domain::Language;
use xcpc_engine::{Engine, FontOptions};
use xcpc_storage::Workspace;

/// The protocol revisions this server speaks, newest first.
const VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

pub fn serve(ws: Workspace, fonts: FontOptions) -> Result<()> {
    let mut server = Server {
        engine: publish::engine(&ws, &fonts),
        ws,
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => server.handle(&message),
            Err(e) => Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(reply) = reply {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

struct Server {
    ws: Workspace,
    engine: Engine,
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

impl Server {
    /// The reply to one message; none for a notification.
    fn handle(&mut self, message: &Value) -> Option<Value> {
        let id = message.get("id").cloned()?;
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str);
                let version = asked
                    .filter(|v| VERSIONS.contains(v))
                    .unwrap_or(VERSIONS[0]);
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "xcpc", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": "Typesets the problems and contests of an XCPC workspace in olymp-classic. \
                        Problems live in problems/<id>/ (problem.toml, statement.md, samples/, editorial.md); \
                        contests in contests/<id>/contest.toml. Diagnostics point into those files.",
                })
            }
            "ping" => json!({}),
            "tools/list" => json!({ "tools": tools() }),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match self.call(name, &args) {
                    Ok(result) => result,
                    Err(Some((code, message))) => json!({
                        "content": [{ "type": "text", "text": json!({ "code": code, "message": message }).to_string() }],
                        "isError": true,
                    }),
                    Err(None) => return Some(error(id, -32602, &format!("unknown tool {name:?}"))),
                }
            }
            _ => return Some(error(id, -32601, &format!("method {method:?} not found"))),
        };
        Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    /// A tool's result; `Err(None)` for a tool that does not exist.
    fn call(&mut self, name: &str, args: &Value) -> Result<Value, Option<(String, String)>> {
        let fail = |code: &str, message: String| Some((code.to_owned(), message));
        let what = What::parse(args).map_err(|m| fail("invalid_arguments", m))?;
        let doc = publish::document(&self.ws, &what.target, what.kind, what.language)
            .map_err(|e| fail(e.code(), e.to_string()))?;
        match name {
            "compile_and_diagnose" => {
                let (_, report) = publish::compile(&mut self.engine, &doc);
                Ok(structured(
                    serde_json::to_value(&report).expect("serializable"),
                ))
            }
            "render_page_png" => {
                let page = args.get("page").and_then(Value::as_u64).unwrap_or(1) as usize;
                let scale = args
                    .get("scale")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.5)
                    .clamp(0.5, 4.0) as f32;
                let (compiled, report) = publish::compile(&mut self.engine, &doc);
                let Some(compiled) = compiled else {
                    return Ok(structured(
                        serde_json::to_value(&report).expect("serializable"),
                    ));
                };
                let png = page
                    .checked_sub(1)
                    .and_then(|p| compiled.render_png(p, scale))
                    .ok_or_else(|| {
                        fail(
                            "no_such_page",
                            format!("page {page} of {}", compiled.page_count()),
                        )
                    })?;
                Ok(json!({
                    "content": [
                        { "type": "image", "mimeType": "image/png", "data": base64::engine::general_purpose::STANDARD.encode(png) },
                        { "type": "text", "text": format!("page {page} of {}", compiled.page_count()) },
                    ],
                }))
            }
            "export_pdf" => {
                let output = match args.get("output").and_then(Value::as_str) {
                    Some(path) => path.to_owned(),
                    None => publish::default_output(&what.target, what.kind),
                };
                self.ws
                    .check_writable(&output)
                    .map_err(|e| fail(e.code(), e.to_string()))?;
                let (compiled, report) = publish::compile(&mut self.engine, &doc);
                let mut value = serde_json::to_value(&report).expect("serializable");
                if let (Some(compiled), true) = (compiled, report.ok) {
                    let pdf = compiled.pdf().map_err(|e| {
                        fail(
                            "pdf_failed",
                            e.first().map(|d| d.message.clone()).unwrap_or_default(),
                        )
                    })?;
                    self.ws
                        .write(&output, &pdf)
                        .map_err(|e| fail(e.code(), e.to_string()))?;
                    value["output"] = json!(output);
                }
                Ok(structured(value))
            }
            _ => Err(None),
        }
    }
}

/// A result carrying `value` both as structured content and as text.
fn structured(value: Value) -> Value {
    let failed = value.get("ok") == Some(&json!(false));
    json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value).expect("serializable") }],
        "structuredContent": value,
        "isError": failed,
    })
}

struct What {
    target: Target,
    kind: Kind,
    language: Option<Language>,
}

impl What {
    fn parse(args: &Value) -> Result<Self, String> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or("`id` is required")?
            .to_owned();
        let target = match args.get("target").and_then(Value::as_str) {
            Some("problem") => Target::Problem(id),
            Some("contest") => Target::Contest(id),
            _ => return Err("`target` must be \"problem\" or \"contest\"".into()),
        };
        let kind = match args.get("book").and_then(Value::as_str) {
            None | Some("statements") => Kind::Statements,
            Some("editorial") => Kind::Editorial,
            Some(other) => return Err(format!("unknown book {other:?}")),
        };
        let language = match args.get("language").and_then(Value::as_str) {
            None => None,
            Some(l) => Some(Language::parse(l).ok_or_else(|| format!("unknown language {l:?}"))?),
        };
        Ok(Self {
            target,
            kind,
            language,
        })
    }
}

fn tools() -> Value {
    let what = json!({
        "target": { "type": "string", "enum": ["problem", "contest"], "description": "What to typeset." },
        "id": { "type": "string", "description": "The problem's or contest's ID: its directory name under problems/ or contests/." },
        "book": { "type": "string", "enum": ["statements", "editorial"], "description": "The statements (default) or the A4 editorial book." },
        "language": { "type": "string", "enum": ["chinese", "english"], "description": "For a single problem: the language to set it in. A contest has its own." },
    });
    let with = |extra: Value| {
        let mut props = what.clone();
        props
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        json!({ "type": "object", "properties": props, "required": ["target", "id"], "additionalProperties": false })
    };
    json!([
        {
            "name": "compile_and_diagnose",
            "title": "Compile and diagnose",
            "description": "Typeset a problem or contest and report errors and warnings with their place in the workspace files \
                (statement.md line and column), the total page count, the pages of each problem, and content that runs past \
                the text area. Writes nothing.",
            "inputSchema": with(json!({})),
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "render_page_png",
            "title": "Render a page",
            "description": "Typeset a problem or contest and return one page as a PNG image, to check how it looks. Writes nothing.",
            "inputSchema": with(json!({
                "page": { "type": "integer", "minimum": 1, "description": "The page, 1-based (default 1). A contest's cover is page 1." },
                "scale": { "type": "number", "minimum": 0.5, "maximum": 4, "description": "Pixels per point (default 1.5)." },
            })),
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "export_pdf",
            "title": "Export PDF",
            "description": "Typeset a problem (single statement), a contest (the whole book with its cover) or either's editorial \
                book, and write the PDF into the workspace. Nothing is written when there are errors.",
            "inputSchema": with(json!({
                "output": { "type": "string", "description": "Where to write it, relative to the workspace (default build/<id>.pdf or build/<id>-editorial.pdf)." },
            })),
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true },
        },
    ])
}
