//! `xcpc mcp`: the Model Context Protocol over stdio (newline-delimited JSON-RPC 2.0),
//! implementing all 10 publishing, library, and contest composition tools of Phase 1–2
//! (docs/architecture.md §7.2):
//!
//! Read-only tools:
//! - `compile_and_diagnose`: typeset + preflight lint (`xcpc-lint`), page map & overflows;
//! - `render_page_png`: render one page to PNG in memory, writing nothing;
//! - `search_problems`: scan workspace index and filter/search problem metadata (no body);
//! - `get_problem`: parsed metadata, `statement*.md` / `editorial.md` text & SHA-256 hashes,
//!   samples, and `solutions/` filenames;
//! - `get_contest`: list all contests, or return one contest's metadata, frozen state,
//!   per-problem overrides + compiled page ranges, and difficulty/tag statistics.
//!
//! Write tools (refused under `--read-only`; frozen contests refused by Core):
//! - `export_pdf`: export statements or editorial PDF into the workspace;
//! - `write_problem`: atomically create/update whitelisted problem files with optional hash check;
//! - `compose_contest`: create/update a contest's ordered problem list (`A..Z`, max 26) & metadata;
//! - `set_override`: set or clear per-problem metadata overrides (`title`, `limits`, `show-metadata`);
//! - `freeze_contest`: validate, export PDFs, and atomically snapshot-freeze a contest.
//!
//! Security boundary: unfreezing a contest is intentionally NOT exposed over MCP; it is only
//! available to a human via the GUI or `xcpc contest unfreeze` with explicit confirmation.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};

use anyhow::Result;
use base64::Engine as _;
use serde_json::{Map, Value, json};
use xcpc_convert::publish::{self, Kind, Target};
use xcpc_domain::{
    ContestMetaPatch, Language, LimitsPatch, OverridePatch, Patch, ProblemMetaPatch, SearchQuery,
    TemplateOptionValue, TemplateOptionsPatch,
};
use xcpc_engine::{Engine, FontOptions};
use xcpc_storage::{Workspace, WorkspaceIndex};

use crate::views;

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

type ToolErr = Option<(String, String)>;

fn fail(code: &str, message: impl Into<String>) -> ToolErr {
    Some((code.to_owned(), message.into()))
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
                    "instructions": "Manages and typesets problems and contests of an XCPC workspace in olymp-classic. \
                        Problems live in problems/<id>/ (problem.toml, statement.md, samples/, editorial.md); \
                        contests live in contests/<id>/contest.toml. Diagnostics include preflight lint findings \
                        and point into workspace files. Frozen contests are strictly read-only; unfreezing is \
                        restricted to human confirmation in the GUI or CLI and is never available over MCP.",
                })
            }
            "ping" => json!({}),
            "tools/list" => json!({ "tools": tools() }),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = match params.get("arguments") {
                    None | Some(Value::Null) => json!({}),
                    Some(v) => v.clone(),
                };
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
    fn call(&mut self, name: &str, args: &Value) -> Result<Value, ToolErr> {
        let Some(obj) = args.as_object() else {
            return if is_known_tool(name) {
                Err(fail("invalid_arguments", "`arguments` must be an object"))
            } else {
                Err(None)
            };
        };

        match name {
            "compile_and_diagnose" => self.tool_compile_and_diagnose(obj),
            "render_page_png" => self.tool_render_page_png(obj),
            "export_pdf" => self.tool_export_pdf(obj),
            "list_templates" => self.tool_list_templates(obj),
            "add_template" => self.tool_add_template(obj),
            "search_problems" => self.tool_search_problems(obj),
            "get_problem" => self.tool_get_problem(obj),
            "get_contest" => self.tool_get_contest(obj),
            "write_problem" => self.tool_write_problem(obj),
            "duplicate_problem" => self.tool_duplicate_problem(obj),
            "compose_contest" => self.tool_compose_contest(obj),
            "set_override" => self.tool_set_override(obj),
            "set_template_options" => self.tool_set_template_options(obj),
            "freeze_contest" => self.tool_freeze_contest(obj),
            "archive_contest" => self.tool_set_contest_archived(obj, true),
            "unarchive_contest" => self.tool_set_contest_archived(obj, false),
            _ => Err(None),
        }
    }

    fn tool_compile_and_diagnose(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        let what = What::parse(args).map_err(|m| fail("invalid_arguments", m))?;
        let (_, _, report) = publish::check(
            &self.ws,
            &mut self.engine,
            &what.target,
            what.kind,
            what.language,
            None,
        )
        .map_err(|e| fail(e.code(), e.to_string()))?;
        Ok(structured(
            serde_json::to_value(&report).expect("serializable"),
        ))
    }

    fn tool_render_page_png(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        let what = What::parse(args).map_err(|m| fail("invalid_arguments", m))?;
        let page = match args.get("page") {
            None | Some(Value::Null) => 1usize,
            Some(v) => v
                .as_u64()
                .ok_or_else(|| fail("invalid_arguments", "`page` must be a positive integer"))?
                as usize,
        };
        let scale = match args.get("scale") {
            None | Some(Value::Null) => 1.5_f32,
            Some(v) => {
                let s = v
                    .as_f64()
                    .filter(|n| n.is_finite())
                    .ok_or_else(|| fail("invalid_arguments", "`scale` must be a finite number"))?;
                s.clamp(0.5, 4.0) as f32
            }
        };
        let (compiled, _, report) = publish::check(
            &self.ws,
            &mut self.engine,
            &what.target,
            what.kind,
            what.language,
            None,
        )
        .map_err(|e| fail(e.code(), e.to_string()))?;
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

    fn tool_export_pdf(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        let what = What::parse(args).map_err(|m| fail("invalid_arguments", m))?;
        let output = match opt_str(args, "output").map_err(|m| fail("invalid_arguments", m))? {
            Some(path) => path,
            None => publish::default_output(&self.ws, &what.target, what.kind),
        };
        self.ws
            .check_writable(&output)
            .map_err(|e| fail(e.code(), e.to_string()))?;
        let (compiled, _, report) = publish::check(
            &self.ws,
            &mut self.engine,
            &what.target,
            what.kind,
            what.language,
            None,
        )
        .map_err(|e| fail(e.code(), e.to_string()))?;
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

    fn tool_search_problems(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        let query = opt_str(args, "query").map_err(|m| fail("invalid_arguments", m))?;
        let text = opt_str(args, "text").map_err(|m| fail("invalid_arguments", m))?;
        let tags = opt_str_vec(args, "tags").map_err(|m| fail("invalid_arguments", m))?;
        let rating_min = opt_u32(args, "rating_min").map_err(|m| fail("invalid_arguments", m))?;
        let rating_max = opt_u32(args, "rating_max").map_err(|m| fail("invalid_arguments", m))?;
        let platform = opt_str(args, "platform").map_err(|m| fail("invalid_arguments", m))?;

        let index = WorkspaceIndex::scan(&self.ws).map_err(|e| fail(e.code(), e.to_string()))?;
        let search = SearchQuery {
            text: query.or(text),
            tags,
            rating_min,
            rating_max,
            platform,
        };
        let problems = index.search_problems(&search);
        Ok(structured(json!({
            "ok": true,
            "problems": problems,
        })))
    }

    fn tool_get_problem(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        let id = req_str(args, "id").map_err(|m| fail("invalid_arguments", m))?;
        let detail =
            views::problem_detail(&self.ws, &id).map_err(|e| fail(e.code(), e.to_string()))?;
        Ok(structured(
            serde_json::to_value(&detail).expect("serializable"),
        ))
    }

    fn tool_get_contest(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        let id = opt_str(args, "id").map_err(|m| fail("invalid_arguments", m))?;
        match id {
            None => {
                let contests =
                    views::list_contests(&self.ws).map_err(|e| fail(e.code(), e.to_string()))?;
                Ok(structured(json!({
                    "ok": true,
                    "contests": contests,
                })))
            }
            Some(id) => {
                let detail = views::contest_detail(&self.ws, &mut self.engine, &id)
                    .map_err(|e| fail(e.code(), e.to_string()))?;
                Ok(structured(
                    serde_json::to_value(&detail).expect("serializable"),
                ))
            }
        }
    }

    fn tool_write_problem(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        let id = req_str(args, "id").map_err(|m| fail("invalid_arguments", m))?;
        let files_obj = args
            .get("files")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                fail(
                    "invalid_arguments",
                    "`files` is required and must be an object mapping relative paths to strings",
                )
            })?;
        let mut files = BTreeMap::new();
        for (k, v) in files_obj {
            let content = v.as_str().ok_or_else(|| {
                fail(
                    "invalid_arguments",
                    format!("`files[{k:?}]` must be a string"),
                )
            })?;
            files.insert(k.clone(), content.to_owned());
        }
        let expected_hash =
            opt_str(args, "expected_hash").map_err(|m| fail("invalid_arguments", m))?;

        let hashes = self
            .ws
            .write_problem_files(&id, &files, expected_hash.as_deref())
            .map_err(|e| fail(e.code(), e.to_string()))?;
        Ok(structured(json!({
            "ok": true,
            "id": id,
            "hashes": hashes,
        })))
    }

    fn tool_compose_contest(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        const ALLOWED_KEYS: &[&str] = &[
            "id",
            "problems",
            "name",
            "location",
            "date",
            "author",
            "language",
            "template",
            "show-metadata",
            "show_metadata",
            "cover",
            "blank-pages",
            "blank_pages",
        ];
        reject_unknown_keys(args, ALLOWED_KEYS, "compose_contest")
            .map_err(|m| fail("invalid_arguments", m))?;

        let id = req_str(args, "id").map_err(|m| fail("invalid_arguments", m))?;
        let problems = req_str_vec(args, "problems").map_err(|m| fail("invalid_arguments", m))?;

        let name = opt_str(args, "name").map_err(|m| fail("invalid_arguments", m))?;
        let location = patch_str(args, "location").map_err(|m| fail("invalid_arguments", m))?;
        let date = patch_str(args, "date").map_err(|m| fail("invalid_arguments", m))?;
        let author = patch_str(args, "author").map_err(|m| fail("invalid_arguments", m))?;
        let language = match opt_str(args, "language").map_err(|m| fail("invalid_arguments", m))? {
            None => None,
            Some(l) => Some(
                Language::parse(&l)
                    .ok_or_else(|| fail("invalid_arguments", format!("unknown language {l:?}")))?,
            ),
        };
        let template = patch_str(args, "template").map_err(|m| fail("invalid_arguments", m))?;
        let show_metadata = opt_bool_alias(args, "show-metadata", "show_metadata")
            .map_err(|m| fail("invalid_arguments", m))?;
        let cover = patch_bool(args, "cover").map_err(|m| fail("invalid_arguments", m))?;
        let blank_pages = opt_bool_alias(args, "blank-pages", "blank_pages")
            .map_err(|m| fail("invalid_arguments", m))?;

        let patch = ContestMetaPatch {
            name,
            location,
            date,
            author,
            language,
            template,
            show_metadata,
            cover,
            blank_pages,
            ..Default::default()
        };
        let has_patch = patch != ContestMetaPatch::default();
        let contest = self
            .ws
            .compose_contest(&id, &problems, has_patch.then_some(&patch))
            .map_err(|e| fail(e.code(), e.to_string()))?;

        Ok(structured(json!({
            "ok": true,
            "id": contest.id,
            "dir": contest.dir,
            "meta": contest.meta,
        })))
    }

    fn tool_set_override(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        let contest_id = req_str(args, "contest").map_err(|m| fail("invalid_arguments", m))?;
        let label = req_str(args, "label").map_err(|m| fail("invalid_arguments", m))?;
        let fields = args
            .get("fields")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                fail(
                    "invalid_arguments",
                    "`fields` is required and must be an object",
                )
            })?;
        reject_unknown_keys(
            fields,
            &[
                "title",
                "limits",
                "show-metadata",
                "show_metadata",
                "options",
            ],
            "fields",
        )
        .map_err(|m| fail("invalid_arguments", m))?;

        let title = patch_str(fields, "title").map_err(|m| fail("invalid_arguments", m))?;
        let show_metadata = patch_bool_alias(fields, "show-metadata", "show_metadata")
            .map_err(|m| fail("invalid_arguments", m))?;
        let limits = match fields.get("limits") {
            None => LimitsPatch::default(),
            Some(Value::Null) => LimitsPatch::clear_all(),
            Some(Value::Object(lim_obj)) => {
                reject_unknown_keys(lim_obj, &["time", "memory", "input", "output"], "limits")
                    .map_err(|m| fail("invalid_arguments", m))?;
                LimitsPatch {
                    time: patch_str(lim_obj, "time").map_err(|m| fail("invalid_arguments", m))?,
                    memory: patch_str(lim_obj, "memory")
                        .map_err(|m| fail("invalid_arguments", m))?,
                    input: patch_str(lim_obj, "input").map_err(|m| fail("invalid_arguments", m))?,
                    output: patch_str(lim_obj, "output")
                        .map_err(|m| fail("invalid_arguments", m))?,
                }
            }
            Some(_) => {
                return Err(fail(
                    "invalid_arguments",
                    "`fields.limits` must be an object or null",
                ));
            }
        };
        let options = match fields.get("options") {
            None | Some(Value::Null) => BTreeMap::new(),
            Some(Value::Object(outer)) => {
                parse_namespaced_options_patch(outer).map_err(|m| fail("invalid_arguments", m))?
            }
            Some(_) => {
                return Err(fail(
                    "invalid_arguments",
                    "`fields.options` must be an object mapping template IDs to option patches",
                ));
            }
        };

        let patch = OverridePatch {
            title,
            limits,
            show_metadata,
            options,
        };
        let contest = self
            .ws
            .set_override(&contest_id, &label, &patch)
            .map_err(|e| fail(e.code(), e.to_string()))?;
        let item = contest.meta.problems.iter().find(|p| p.label == label);

        Ok(structured(json!({
            "ok": true,
            "contest": contest.id,
            "label": label,
            "item": item,
            "meta": contest.meta,
        })))
    }

    fn tool_list_templates(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        reject_unknown_keys(args, &[], "list_templates")
            .map_err(|m| fail("invalid_arguments", m))?;
        self.engine.reset_files();
        let templates = publish::list_templates(&self.ws, &self.engine);
        let presets = xcpc_engine::list_presets();
        let default_tpl = self
            .ws
            .default_template()
            .unwrap_or_else(|| "olymp-classic".into());
        let workspace_options = self.ws.workspace_template_options();
        Ok(structured(json!({
            "ok": true,
            "default": default_tpl,
            "workspace_options": workspace_options,
            "templates": templates,
            "presets": presets,
        })))
    }

    fn tool_add_template(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        reject_unknown_keys(args, &["preset", "name"], "add_template")
            .map_err(|m| fail("invalid_arguments", m))?;
        let preset = req_str(args, "preset").map_err(|m| fail("invalid_arguments", m))?;
        let name = opt_str(args, "name").map_err(|m| fail("invalid_arguments", m))?;
        let rel = self
            .ws
            .install_preset_template(&preset, name.as_deref())
            .map_err(|e| fail(e.code(), e.to_string()))?;
        let id = name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(preset.as_str())
            .to_owned();
        self.engine.reset_files();
        Ok(structured(json!({
            "ok": true,
            "preset": preset,
            "id": id,
            "path": rel,
        })))
    }

    fn tool_set_template_options(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        reject_unknown_keys(
            args,
            &["scope", "id", "template", "options"],
            "set_template_options",
        )
        .map_err(|m| fail("invalid_arguments", m))?;
        let scope = req_str(args, "scope").map_err(|m| fail("invalid_arguments", m))?;
        let template_patch =
            patch_str(args, "template").map_err(|m| fail("invalid_arguments", m))?;
        let options_patch = match args.get("options") {
            None | Some(Value::Null) => BTreeMap::new(),
            Some(Value::Object(outer)) => {
                parse_namespaced_options_patch(outer).map_err(|m| fail("invalid_arguments", m))?
            }
            Some(_) => {
                return Err(fail(
                    "invalid_arguments",
                    "`options` must be an object mapping template IDs to option patches",
                ));
            }
        };

        match scope.as_str() {
            "workspace" => {
                self.ws
                    .update_workspace_template(&template_patch, &options_patch)
                    .map_err(|e| fail(e.code(), e.to_string()))?;
                Ok(structured(json!({
                    "ok": true,
                    "scope": "workspace",
                    "default": self.ws.default_template().unwrap_or_else(|| "olymp-classic".into()),
                    "workspace_options": self.ws.workspace_template_options(),
                })))
            }
            "contest" => {
                let id = req_str(args, "id").map_err(|m| fail("invalid_arguments", m))?;
                let patch = ContestMetaPatch {
                    template: template_patch,
                    template_options: options_patch,
                    ..Default::default()
                };
                let meta = self
                    .ws
                    .update_contest_meta(&id, &patch)
                    .map_err(|e| fail(e.code(), e.to_string()))?;
                Ok(structured(json!({
                    "ok": true,
                    "scope": "contest",
                    "id": id,
                    "meta": meta,
                })))
            }
            "problem" => {
                let id = req_str(args, "id").map_err(|m| fail("invalid_arguments", m))?;
                let patch = ProblemMetaPatch {
                    template_options: options_patch,
                    ..Default::default()
                };
                let meta = self
                    .ws
                    .update_problem_meta(&id, &patch)
                    .map_err(|e| fail(e.code(), e.to_string()))?;
                Ok(structured(json!({
                    "ok": true,
                    "scope": "problem",
                    "id": id,
                    "meta": meta,
                })))
            }
            other => Err(fail(
                "invalid_arguments",
                format!(
                    "unknown scope {other:?}: expected \"workspace\", \"contest\", or \"problem\""
                ),
            )),
        }
    }

    fn tool_duplicate_problem(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        reject_unknown_keys(args, &["source_id", "new_id", "title"], "duplicate_problem")
            .map_err(|m| fail("invalid_arguments", m))?;
        let source_id = req_str(args, "source_id").map_err(|m| fail("invalid_arguments", m))?;
        let new_id = req_str(args, "new_id").map_err(|m| fail("invalid_arguments", m))?;
        let title = opt_str(args, "title").map_err(|m| fail("invalid_arguments", m))?;
        let problem = self
            .ws
            .duplicate_problem(&source_id, &new_id, title.as_deref())
            .map_err(|e| fail(e.code(), e.to_string()))?;
        let statements: Vec<String> = problem
            .statements
            .iter()
            .map(|s| s.file.path.clone())
            .collect();
        Ok(structured(json!({
            "ok": true,
            "source_id": source_id,
            "id": problem.id,
            "title": problem.meta.title,
            "path": problem.dir,
            "statements": statements,
            "samples": problem.samples.len(),
            "has_editorial": problem.editorial.is_some(),
        })))
    }

    fn tool_set_contest_archived(
        &mut self,
        args: &Map<String, Value>,
        archived: bool,
    ) -> Result<Value, ToolErr> {
        let tool_name = if archived {
            "archive_contest"
        } else {
            "unarchive_contest"
        };
        reject_unknown_keys(args, &["id"], tool_name).map_err(|m| fail("invalid_arguments", m))?;
        let id = req_str(args, "id").map_err(|m| fail("invalid_arguments", m))?;
        let meta = self
            .ws
            .set_contest_archived(&id, archived)
            .map_err(|e| fail(e.code(), e.to_string()))?;
        Ok(structured(json!({
            "ok": true,
            "id": id,
            "archived": meta.archived,
            "frozen": meta.frozen,
        })))
    }

    fn tool_freeze_contest(&mut self, args: &Map<String, Value>) -> Result<Value, ToolErr> {
        let id = req_str(args, "id").map_err(|m| fail("invalid_arguments", m))?;
        let outcome = publish::freeze_contest(&self.ws, &mut self.engine, &id)
            .map_err(|e| fail(e.code(), e.to_string()))?;
        let mut value = serde_json::to_value(&outcome.report).expect("serializable");
        value["id"] = json!(id);
        value["frozen"] = json!(outcome.frozen);
        value["outputs"] = json!(outcome.outputs);
        value["ok"] = json!(outcome.frozen);
        Ok(structured(value))
    }
}

fn is_known_tool(name: &str) -> bool {
    matches!(
        name,
        "compile_and_diagnose"
            | "render_page_png"
            | "export_pdf"
            | "list_templates"
            | "add_template"
            | "search_problems"
            | "get_problem"
            | "get_contest"
            | "write_problem"
            | "duplicate_problem"
            | "compose_contest"
            | "set_override"
            | "set_template_options"
            | "freeze_contest"
            | "archive_contest"
            | "unarchive_contest"
    )
}

fn parse_option_value(v: &Value) -> Result<TemplateOptionValue, String> {
    match v {
        Value::Bool(b) => Ok(TemplateOptionValue::Bool(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(TemplateOptionValue::Int(i))
            } else if let Some(f) = n.as_f64() {
                Ok(TemplateOptionValue::Float(f))
            } else {
                Err("invalid numeric template option value".into())
            }
        }
        Value::String(s) => Ok(TemplateOptionValue::String(s.clone())),
        Value::Array(arr) => {
            let mut lines = Vec::with_capacity(arr.len());
            for item in arr {
                let s = item.as_str().ok_or_else(|| {
                    "array template option values must contain strings".to_owned()
                })?;
                lines.push(s.to_owned());
            }
            Ok(TemplateOptionValue::Lines(lines))
        }
        _ => Err(
            "template option value must be a boolean, number, string, array of strings, or null"
                .into(),
        ),
    }
}

fn parse_namespaced_options_patch(
    outer: &Map<String, Value>,
) -> Result<BTreeMap<String, TemplateOptionsPatch>, String> {
    let mut out = BTreeMap::new();
    for (tpl, val) in outer {
        let Some(inner) = val.as_object() else {
            return Err(format!("`options[{tpl:?}]` must be an object"));
        };
        let mut fields = TemplateOptionsPatch::new();
        for (key, fv) in inner {
            if fv.is_null() {
                fields.insert(key.clone(), Patch::Clear);
            } else {
                fields.insert(key.clone(), Patch::Set(parse_option_value(fv)?));
            }
        }
        out.insert(tpl.clone(), fields);
    }
    Ok(out)
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

fn reject_unknown_keys(
    obj: &Map<String, Value>,
    allowed: &[&str],
    context: &str,
) -> Result<(), String> {
    for key in obj.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("unknown field {key:?} in {context}"));
        }
    }
    Ok(())
}

fn req_str(obj: &Map<String, Value>, key: &str) -> Result<String, String> {
    obj.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("`{key}` is required and must be a string"))
}

fn opt_str(obj: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("`{key}` must be a string")),
    }
}

fn opt_u32(obj: &Map<String, Value>, key: &str) -> Result<Option<u32>, String> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let n = v
                .as_u64()
                .ok_or_else(|| format!("`{key}` must be a non-negative integer"))?;
            u32::try_from(n)
                .map(Some)
                .map_err(|_| format!("`{key}` is out of range"))
        }
    }
}

fn opt_bool_alias(
    obj: &Map<String, Value>,
    primary: &str,
    alias: &str,
) -> Result<Option<bool>, String> {
    let p = obj.get(primary);
    let a = obj.get(alias);
    if p.is_some() && a.is_some() {
        return Err(format!("specify only one of `{primary}` or `{alias}`"));
    }
    match p.or(a) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("`{primary}` must be a boolean")),
    }
}

fn patch_str(obj: &Map<String, Value>, key: &str) -> Result<Patch<String>, String> {
    match obj.get(key) {
        None => Ok(Patch::Keep),
        Some(Value::Null) => Ok(Patch::Clear),
        Some(Value::String(s)) => Ok(Patch::Set(s.clone())),
        Some(_) => Err(format!("`{key}` must be a string or null")),
    }
}

fn patch_bool(obj: &Map<String, Value>, key: &str) -> Result<Patch<bool>, String> {
    match obj.get(key) {
        None => Ok(Patch::Keep),
        Some(Value::Null) => Ok(Patch::Clear),
        Some(Value::Bool(b)) => Ok(Patch::Set(*b)),
        Some(_) => Err(format!("`{key}` must be a boolean or null")),
    }
}

fn patch_bool_alias(
    obj: &Map<String, Value>,
    primary: &str,
    alias: &str,
) -> Result<Patch<bool>, String> {
    let p = obj.get(primary);
    let a = obj.get(alias);
    if p.is_some() && a.is_some() {
        return Err(format!("specify only one of `{primary}` or `{alias}`"));
    }
    match p.or(a) {
        None => Ok(Patch::Keep),
        Some(Value::Null) => Ok(Patch::Clear),
        Some(Value::Bool(b)) => Ok(Patch::Set(*b)),
        Some(_) => Err(format!("`{primary}` must be a boolean or null")),
    }
}

fn opt_str_vec(obj: &Map<String, Value>, key: &str) -> Result<Vec<String>, String> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(arr)) => arr
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("`{key}` must be an array of strings"))
            })
            .collect(),
        Some(_) => Err(format!("`{key}` must be an array of strings")),
    }
}

fn req_str_vec(obj: &Map<String, Value>, key: &str) -> Result<Vec<String>, String> {
    let arr = obj
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("`{key}` is required and must be an array of strings"))?;
    arr.iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("`{key}` must be an array of strings"))
        })
        .collect()
}

struct What {
    target: Target,
    kind: Kind,
    language: Option<Language>,
}

impl What {
    fn parse(args: &Map<String, Value>) -> Result<Self, String> {
        let id = req_str(args, "id")?;
        let target = match args.get("target").and_then(Value::as_str) {
            Some("problem") => Target::Problem(id),
            Some("contest") => Target::Contest(id),
            _ => return Err("`target` must be \"problem\" or \"contest\"".into()),
        };
        let kind = match opt_str(args, "book")?.as_deref() {
            None | Some("statements") => Kind::Statements,
            Some("editorial") => Kind::Editorial,
            Some(other) => return Err(format!("unknown book {other:?}")),
        };
        let language = match opt_str(args, "language")?.as_deref() {
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
            "description": "Typeset a problem or contest and report compile errors, preflight lint findings \
                (missing problem/image errors; sample, I/O section, editorial, and overflow warnings) with \
                workspace file locations, total page count, and pages per problem. Writes nothing.",
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
                book, and write the PDF into the workspace. Nothing is written when there are compile or lint errors.",
            "inputSchema": with(json!({
                "output": { "type": "string", "description": "Where to write it, relative to the workspace (default build/<title>.pdf, build/<contest>-试题册.pdf, or build/<title>-题解.pdf)." },
            })),
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true },
        },
        {
            "name": "list_templates",
            "title": "List templates and schemas",
            "description": "List all workspace Typst templates (`templates/*.typ`) together with their declarative `#let schema` fields, groups, defaults, current workspace defaults, and installable preset seeds.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "add_template",
            "title": "Install preset template",
            "description": "Install a bundled preset seed (`olymp-classic`, `cnoi-classic`, or `minimal`) as an editable template file under `templates/<name>.typ`. Refuses to overwrite an existing file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "preset": { "type": "string", "description": "Preset seed ID (`olymp-classic`, `cnoi-classic`, or `minimal`)." },
                    "name": { "type": "string", "description": "Optional target template slug under `templates/<name>.typ` (defaults to `preset`)." },
                },
                "required": ["preset"],
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false },
        },
        {
            "name": "search_problems",
            "title": "Search problems",
            "description": "Search the workspace problem library by optional text, tags, rating range, or source platform. \
                Returns metadata summaries only (ID, title, rating, tags, sources, editorial/solution presence; no statement body).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Free-text query matched against ID, title, author, tags, sources, statements, and editorial." },
                    "text": { "type": "string", "description": "Alias for `query`." },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Required tags (a problem must have all listed tags)." },
                    "rating_min": { "type": "integer", "minimum": 0, "description": "Minimum rating (inclusive)." },
                    "rating_max": { "type": "integer", "minimum": 0, "description": "Maximum rating (inclusive)." },
                    "platform": { "type": "string", "description": "Filter by external source platform (e.g. codeforces, atcoder, luogu)." },
                },
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "get_problem",
            "title": "Get problem",
            "description": "Read a problem's parsed metadata, full text and SHA-256 content hashes of statement*.md and editorial.md, \
                samples, and solutions/ filenames.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Problem ID (directory name under problems/)." },
                },
                "required": ["id"],
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "get_contest",
            "title": "Get contest",
            "description": "List all contests when `id` is omitted, or return one contest's metadata, frozen state, \
                problem list (label, ID, effective title, overrides, compiled page start/count), and difficulty/tag statistics.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Contest ID (directory name under contests/). Omit to list all contests." },
                },
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "write_problem",
            "title": "Write problem files",
            "description": "Atomically create or update whitelisted files inside problems/<id>/ (problem.toml, statement*.md, \
                editorial.md, samples/*, solutions/*, generators/*, tests/*, checker.cpp, validator.cpp, interactor.cpp). \
                Validates problem.toml syntax and optionally checks expected_hash (SHA-256) to detect concurrent edits.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Problem ID (slug under problems/)." },
                    "files": {
                        "type": "object",
                        "additionalProperties": { "type": "string" },
                        "description": "Map of problem-relative file paths to full UTF-8 contents.",
                    },
                    "expected_hash": { "type": "string", "description": "Optional expected SHA-256 hex digest of the primary target file on disk (empty string if missing)." },
                },
                "required": ["id", "files"],
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true },
        },
        {
            "name": "duplicate_problem",
            "title": "Duplicate problem",
            "description": "Recursively copy an existing problem directory (`problems/<source_id>/`) into a new problem slug (`problems/<new_id>/`), optionally updating its title while preserving `problem.toml` comments and unknown fields.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source_id": { "type": "string", "description": "Existing source problem ID." },
                    "new_id": { "type": "string", "description": "New problem ID (lowercase slug under problems/)." },
                    "title": { "type": "string", "description": "Optional new problem title (defaults to source title)." },
                },
                "required": ["source_id", "new_id"],
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false },
        },
        {
            "name": "compose_contest",
            "title": "Compose contest",
            "description": "Create or update a contest's ordered problem list (max 26, labelled A..Z) and optional metadata. \
                Preserves existing overrides on kept problems and removes dropped problems. Refuses frozen contests.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Contest ID (slug under contests/). Created if absent." },
                    "problems": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Ordered problem IDs (max 26).",
                    },
                    "name": { "type": "string", "description": "Contest display name." },
                    "location": { "type": ["string", "null"], "description": "Contest location (null to clear)." },
                    "date": { "type": ["string", "null"], "description": "Contest date (null to clear)." },
                    "author": { "type": ["string", "null"], "description": "Cover author line (null to clear)." },
                    "language": { "type": "string", "enum": ["chinese", "english"], "description": "Contest language." },
                    "template": { "type": ["string", "null"], "description": "Template ID (e.g. olymp-classic, cnoi-classic, or custom; null to inherit workspace default)." },
                    "show-metadata": { "type": "boolean", "description": "Show limits table by default." },
                    "cover": { "type": ["boolean", "null"], "description": "Show cover page (null for default: >= 2 problems)." },
                    "blank-pages": { "type": "boolean", "description": "Start every problem on an odd page." },
                },
                "required": ["id", "problems"],
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true },
        },
        {
            "name": "set_override",
            "title": "Set contest problem override",
            "description": "Set or clear metadata overrides (title, limits, show-metadata) for one problem in a contest. \
                Pass null for a field to delete its override; matching the library's original value also clears the override.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "contest": { "type": "string", "description": "Contest ID." },
                    "label": { "type": "string", "description": "Problem label in the contest (e.g. A, B, C)." },
                    "fields": {
                        "type": "object",
                        "description": "Override fields to update; null removes the override.",
                        "properties": {
                            "title": { "type": ["string", "null"], "description": "Override title, or null to clear." },
                            "show-metadata": { "type": ["boolean", "null"], "description": "Override limits-table visibility, or null to clear." },
                            "limits": {
                                "type": ["object", "null"],
                                "description": "Override limit fields, or null to clear all limit overrides.",
                                "properties": {
                                    "time": { "type": ["string", "null"] },
                                    "memory": { "type": ["string", "null"] },
                                    "input": { "type": ["string", "null"] },
                                    "output": { "type": ["string", "null"] },
                                },
                                "additionalProperties": false,
                            },
                            "options": {
                                "type": ["object", "null"],
                                "description": "Problem-scoped template option overrides, namespaced by template ID ({ \"<template>\": { \"<key>\": value | null } }).",
                            },
                        },
                        "additionalProperties": false,
                    },
                },
                "required": ["contest", "label", "fields"],
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true },
        },
        {
            "name": "set_template_options",
            "title": "Set template and template options",
            "description": "Update template selection and namespaced template schema options at workspace, contest, or problem scope. Use namespace \"local\" when a contest has its own `contests/<id>/template.typ`. Pass null for an option key to clear it and inherit from the parent scope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "scope": { "type": "string", "enum": ["workspace", "contest", "problem"], "description": "Target scope to update." },
                    "id": { "type": "string", "description": "Contest ID or problem ID (required when scope is \"contest\" or \"problem\")." },
                    "template": { "type": ["string", "null"], "description": "Template ID to select (workspace or contest scope; null clears to default)." },
                    "options": {
                        "type": ["object", "null"],
                        "description": "Namespaced option patch: { \"<template-id>\": { \"<key>\": value | null } }.",
                    },
                },
                "required": ["scope"],
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true },
        },
        {
            "name": "freeze_contest",
            "title": "Freeze contest",
            "description": "Validate, compile, export snapshot PDFs, and freeze a contest into a read-only archive. \
                Refuses to freeze if any compile or lint error exists; warnings do not block freezing.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Contest ID to freeze." },
                },
                "required": ["id"],
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false },
        },
        {
            "name": "archive_contest",
            "title": "Archive contest",
            "description": "Mark a contest as archived (read-only until unarchived; orthogonal to frozen status). Idempotent.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Contest ID to archive." },
                },
                "required": ["id"],
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true },
        },
        {
            "name": "unarchive_contest",
            "title": "Unarchive contest",
            "description": "Remove the archived flag from a contest, restoring its previous editable or frozen state. Idempotent.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Contest ID to unarchive." },
                },
                "required": ["id"],
                "additionalProperties": false,
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true },
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sample_workspace(read_only: bool) -> (tempfile::TempDir, Server) {
        let dir = tempfile::tempdir().unwrap();
        xcpc_storage::init_workspace(dir.path()).unwrap();
        let prob = dir.path().join("problems/a-plus-b");
        fs::create_dir_all(prob.join("samples")).unwrap();
        fs::write(
            prob.join("problem.toml"),
            "title = \"A + B\"\nrating = 800\ntags = [\"math\"]\n[limits]\ntime = \"1s\"\nmemory = \"256MB\"\n",
        )
        .unwrap();
        fs::write(
            prob.join("statement.md"),
            "Compute $a + b$.\n\n## Input\n\nTwo integers.\n\n## Output\n\nTheir sum.\n",
        )
        .unwrap();
        fs::write(prob.join("samples/01.in"), "1 2\n").unwrap();
        fs::write(prob.join("samples/01.ans"), "3\n").unwrap();
        let ws = Workspace::open(dir.path(), read_only).unwrap();
        let fonts = FontOptions::from_env();
        let engine = publish::engine(&ws, &fonts);
        (dir, Server { ws, engine })
    }

    #[test]
    fn initialize_and_list_all_ten_tools_without_unfreeze() {
        let (_dir, mut server) = sample_workspace(false);
        let init = server
            .handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-11-25" } }))
            .unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-11-25");

        let list = server
            .handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
            .unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec![
                "compile_and_diagnose",
                "render_page_png",
                "export_pdf",
                "list_templates",
                "add_template",
                "search_problems",
                "get_problem",
                "get_contest",
                "write_problem",
                "duplicate_problem",
                "compose_contest",
                "set_override",
                "set_template_options",
                "freeze_contest",
                "archive_contest",
                "unarchive_contest",
            ]
        );
        assert!(
            !names.iter().any(|n| n.contains("unfreeze")),
            "MCP must never expose any unfreeze tool: {names:?}"
        );

        // Calling an unfreeze tool name directly must return JSON-RPC unknown tool (-32602).
        let unfreeze = server
            .handle(&json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": { "name": "unfreeze_contest", "arguments": { "id": "any" } }
            }))
            .unwrap();
        assert_eq!(unfreeze["error"]["code"], -32602);
    }

    #[test]
    fn compile_render_and_export_respect_sandbox_and_read_only() {
        let (dir, mut server) = sample_workspace(false);
        let diag = server
            .handle(&json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "compile_and_diagnose", "arguments": { "target": "problem", "id": "a-plus-b" } }
            }))
            .unwrap();
        assert_ne!(diag["result"]["isError"], true);
        assert_eq!(diag["result"]["structuredContent"]["pages"], 1);

        let png = server
            .handle(&json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": { "name": "render_page_png", "arguments": { "target": "problem", "id": "a-plus-b", "page": 1 } }
            }))
            .unwrap();
        assert_ne!(png["result"]["isError"], true);
        assert_eq!(png["result"]["content"][0]["mimeType"], "image/png");

        let exported = server
            .handle(&json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": { "name": "export_pdf", "arguments": { "target": "problem", "id": "a-plus-b" } }
            }))
            .unwrap();
        assert_ne!(exported["result"]["isError"], true);
        assert!(dir.path().join("build/A + B.pdf").is_file());

        // Path traversal must be refused.
        let escape = server
            .handle(&json!({
                "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                "params": { "name": "export_pdf", "arguments": { "target": "problem", "id": "a-plus-b", "output": "../escape.pdf" } }
            }))
            .unwrap();
        assert_eq!(escape["result"]["isError"], true);

        // Read-only workspace must refuse export_pdf.
        let (_ro_dir, mut ro_server) = sample_workspace(true);
        let ro_export = ro_server
            .handle(&json!({
                "jsonrpc": "2.0", "id": 5, "method": "tools/call",
                "params": { "name": "export_pdf", "arguments": { "target": "problem", "id": "a-plus-b" } }
            }))
            .unwrap();
        assert_eq!(ro_export["result"]["isError"], true);
    }
}
