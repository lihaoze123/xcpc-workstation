//! Phase 5 desktop bridge: algorithm snippets, C++ single-file bundler, background
//! snippet verification (self / problem / Library Checker), and Living TRD publishing.
//!
//! All domain, storage, judge, and conversion rules live in `crates/`; this module is a
//! thin glue layer over `State`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use xcpc_convert::publish;
use xcpc_domain::{
    BundleOptions, LibraryCheckerConfig, SnippetMeta, SnippetMetaPatch, SnippetVerifyReport,
    TemplateKind, TrdChapter, TrdMetaPatch,
};
use xcpc_judge::{
    JudgeError, SnippetVerifyOptions, all_snippet_verify_statuses, load_snippet_verify_report,
    snippet_verify_status, verify_all_snippets, verify_snippet, verify_snippet_cascade,
};

use crate::state::State;
use crate::types::{ActionResult, JsonReportResultJs, PdfFreshnessJs, WorkspaceFile};

/// Background snippet verification runner state polled/observed by the UI.
#[derive(Default)]
pub(crate) struct SnippetVerifySlot {
    pub(crate) seq: u32,
    pub(crate) running: bool,
    pub(crate) mode: String,
    pub(crate) target: Option<String>,
    pub(crate) completed: u32,
    pub(crate) total: u32,
    pub(crate) reports: Vec<SnippetVerifyReport>,
    pub(crate) error: Option<String>,
    pub(crate) error_code: Option<String>,
}

fn format_judge_verify_error(err: JudgeError) -> (Option<String>, String) {
    match err {
        JudgeError::NoCompiler => (
            Some("no_compiler".to_owned()),
            "未找到可用的 C++ 编译器，请先在设置中配置工具链".to_owned(),
        ),
        other => (None, other.to_string()),
    }
}

fn ok_json(val: &serde_json::Value) -> JsonReportResultJs {
    JsonReportResultJs {
        ok: true,
        report_json: Some(val.to_string()),
        markdown: None,
        error: None,
        code: None,
    }
}

fn err_json(code: impl Into<String>, message: impl Into<String>) -> JsonReportResultJs {
    JsonReportResultJs {
        ok: false,
        report_json: None,
        markdown: None,
        error: Some(message.into()),
        code: Some(code.into()),
    }
}

fn err_json_storage(err: xcpc_storage::Error) -> JsonReportResultJs {
    JsonReportResultJs {
        ok: false,
        report_json: None,
        markdown: None,
        error: Some(err.human_message()),
        code: Some(err.code().to_owned()),
    }
}

impl State {
    pub(crate) fn snippet_verify_options(&self, force: bool) -> SnippetVerifyOptions {
        let (cxx_override, mirror_url, local_repo_path) = {
            let core = self.core.lock().unwrap();
            (
                core.prefs.app.cxx_override.clone().map(PathBuf::from),
                core.prefs.app.library_checker_mirror_url.clone(),
                core.prefs.app.library_checker_local_repo_path.clone(),
            )
        };
        SnippetVerifyOptions {
            cxx_override,
            cache_dir: None,
            library_checker: LibraryCheckerConfig {
                mirror_url,
                local_repo_path,
            },
            jobs: None,
            force,
        }
    }

    pub(crate) fn set_library_checker_config(
        &self,
        mirror_url: Option<String>,
        local_repo_path: Option<String>,
    ) -> ActionResult {
        let mut core = self.core.lock().unwrap();
        core.prefs
            .set_library_checker_config(mirror_url.as_deref(), local_repo_path.as_deref());
        core.prefs.persist(self.data_dir.as_deref());
        ActionResult::ok()
    }

    // ---- Snippets ----------------------------------------------------------------------

    pub(crate) fn list_snippets_json(&self) -> JsonReportResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return err_json("no_workspace", "no workspace open");
        };
        let summaries = match ws.list_snippet_summaries() {
            Ok(list) => list,
            Err(e) => return err_json_storage(e),
        };
        let statuses = all_snippet_verify_statuses(&ws).unwrap_or_default();
        let reverse_refs = ws.all_snippet_reverse_refs().unwrap_or_default();

        let mut tags = std::collections::BTreeSet::new();
        let mut items = Vec::with_capacity(summaries.len());
        for s in summaries {
            for tag in &s.tags {
                tags.insert(tag.clone());
            }
            let status = statuses.get(&s.slug).copied().unwrap_or_default();
            let refs = reverse_refs.get(&s.slug).cloned().unwrap_or_default();
            let mut val = serde_json::to_value(&s).unwrap_or_else(|_| serde_json::json!({}));
            if let Some(obj) = val.as_object_mut() {
                obj.insert("status".into(), serde_json::json!(status.as_str()));
                obj.insert("reverse_deps".into(), serde_json::json!(refs.reverse_deps));
                obj.insert(
                    "related_problems".into(),
                    serde_json::json!(refs.related_problems),
                );
            }
            items.push(val);
        }

        let tags_vec: Vec<String> = tags.into_iter().collect();
        ok_json(&serde_json::json!({
            "snippets": items,
            "tags": tags_vec,
        }))
    }

    pub(crate) fn load_snippet_json(&self, slug: &str) -> JsonReportResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return err_json("no_workspace", "no workspace open");
        };
        let detail = match ws.load_snippet(slug) {
            Ok(d) => d,
            Err(e) => return err_json_storage(e),
        };
        let status = snippet_verify_status(&ws, slug);
        let report = load_snippet_verify_report(&ws, slug);
        ok_json(&serde_json::json!({
            "detail": detail,
            "status": status.as_str(),
            "report": report,
        }))
    }

    pub(crate) fn create_snippet(
        &self,
        slug: &str,
        meta_json: &str,
        code: Option<&str>,
        notes: Option<&str>,
    ) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        let meta: SnippetMeta = match serde_json::from_str(meta_json) {
            Ok(m) => m,
            Err(e) => {
                return ActionResult::err("invalid", format!("invalid snippet metadata: {e}"));
            }
        };
        match ws.create_snippet(slug, &meta, code, notes) {
            Ok(_) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn update_snippet_meta(&self, slug: &str, patch_json: &str) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        let patch: SnippetMetaPatch = match serde_json::from_str(patch_json) {
            Ok(p) => p,
            Err(e) => return ActionResult::err("invalid", format!("invalid snippet patch: {e}")),
        };
        match ws.update_snippet_meta(slug, &patch) {
            Ok(_) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn write_snippet_file(
        &self,
        slug: &str,
        file_name: &str,
        content: &str,
    ) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        let mut files = BTreeMap::new();
        files.insert(file_name.to_owned(), content.to_owned());
        match ws.write_snippet_files(slug, &files) {
            Ok(_) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn delete_snippet(&self, slug: &str) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.delete_snippet(slug) {
            Ok(()) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn start_snippet_verify(
        &self,
        mode: &str,
        slug: Option<String>,
        force: bool,
    ) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        let opts = self.snippet_verify_options(force);
        let slot = Arc::clone(&self.snippet_verify);
        let mode_str = mode.to_owned();

        let seq = {
            let mut guard = slot.lock().unwrap();
            if guard.running {
                return ActionResult::err("busy", "verification already running");
            }
            guard.seq = guard.seq.wrapping_add(1);
            guard.running = true;
            guard.mode = mode_str.clone();
            guard.target = slug.clone();
            guard.completed = 0;
            guard.total = 0;
            guard.reports.clear();
            guard.error = None;
            guard.error_code = None;
            guard.seq
        };

        std::thread::spawn(move || {
            let res: Result<Vec<SnippetVerifyReport>, (Option<String>, String)> =
                match mode_str.as_str() {
                    "single" => match slug.as_deref() {
                        Some(s) => verify_snippet(&ws, s, &opts)
                            .map(|r| vec![r])
                            .map_err(format_judge_verify_error),
                        None => Err((None, "缺少模板标识名".into())),
                    },
                    "cascade" => match slug.as_deref() {
                        Some(s) => {
                            verify_snippet_cascade(&ws, s, &opts).map_err(format_judge_verify_error)
                        }
                        None => Err((None, "缺少模板标识名".into())),
                    },
                    _ => verify_all_snippets(&ws, &opts).map_err(format_judge_verify_error),
                };

            let mut guard = slot.lock().unwrap();
            if guard.seq != seq {
                return;
            }
            guard.running = false;
            match res {
                Ok(reports) => {
                    guard.completed = reports.len() as u32;
                    guard.total = reports.len() as u32;
                    guard.reports = reports;
                }
                Err((code, err)) => {
                    guard.error_code = code;
                    guard.error = Some(err);
                }
            }
        });

        ActionResult::ok()
    }

    pub(crate) fn snippet_verify_status_json(&self) -> JsonReportResultJs {
        let guard = self.snippet_verify.lock().unwrap();
        ok_json(&serde_json::json!({
            "seq": guard.seq,
            "running": guard.running,
            "mode": guard.mode,
            "target": guard.target,
            "completed": guard.completed,
            "total": guard.total,
            "reports": guard.reports,
            "error": guard.error,
            "errorCode": guard.error_code,
        }))
    }

    // ---- Bundler -----------------------------------------------------------------------

    pub(crate) fn bundle_snippet_json(&self, slug: &str, compact: bool) -> JsonReportResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return err_json("no_workspace", "no workspace open");
        };
        let opts = BundleOptions {
            include_dirs: Vec::new(),
            compact,
            include_boundary_comments: !compact,
        };
        match ws.bundle_snippet(slug, &opts) {
            Ok(res) => ok_json(&serde_json::to_value(&res).unwrap_or_default()),
            Err(e) => err_json_storage(e),
        }
    }

    pub(crate) fn bundle_local_file_json(
        &self,
        path: &str,
        include_dirs: Vec<String>,
        compact: bool,
    ) -> JsonReportResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return err_json("no_workspace", "no workspace open");
        };
        let opts = BundleOptions {
            include_dirs,
            compact,
            include_boundary_comments: !compact,
        };
        match ws.bundle_local_file(std::path::Path::new(path), &opts) {
            Ok(res) => ok_json(&serde_json::to_value(&res).unwrap_or_default()),
            Err(e) => err_json_storage(e),
        }
    }

    pub(crate) fn bundle_local_source_json(
        &self,
        source: &str,
        base_dir: Option<String>,
        include_dirs: Vec<String>,
        compact: bool,
    ) -> JsonReportResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return err_json("no_workspace", "no workspace open");
        };
        let opts = BundleOptions {
            include_dirs,
            compact,
            include_boundary_comments: !compact,
        };
        let base_path = base_dir
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from);
        match ws.bundle_local_source(source, base_path.as_deref(), &opts) {
            Ok(res) => ok_json(&serde_json::to_value(&res).unwrap_or_default()),
            Err(e) => err_json_storage(e),
        }
    }

    // ---- TRDs --------------------------------------------------------------------------

    pub(crate) fn list_trds_json(&self) -> JsonReportResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return err_json("no_workspace", "no workspace open");
        };
        match ws.list_trd_summaries() {
            Ok(list) => ok_json(&serde_json::json!({ "trds": list })),
            Err(e) => err_json_storage(e),
        }
    }

    pub(crate) fn load_trd_json(&self, id: &str) -> JsonReportResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return err_json("no_workspace", "no workspace open");
        };
        let detail = match ws.load_trd(id) {
            Ok(d) => d,
            Err(e) => return err_json_storage(e),
        };
        let preflight = if detail.meta.frozen {
            Vec::new()
        } else {
            let statuses = all_snippet_verify_statuses(&ws).unwrap_or_default();
            ws.preflight_trd(id, &statuses).unwrap_or_default()
        };
        let local_template = if detail.has_local_template {
            let rel = format!("trds/{id}/template.typ");
            self.inspect_local_template_cached(&ws, &rel, |engine| {
                publish::inspect_trd_template(&ws, engine, id)
            })
        } else {
            None
        };
        ok_json(&serde_json::json!({
            "detail": detail,
            "preflight": preflight,
            "localTemplate": local_template,
        }))
    }

    pub(crate) fn compose_trd(
        &self,
        id: &str,
        chapters_json: &str,
        meta_patch_json: Option<&str>,
    ) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        let chapters: Vec<TrdChapter> = match serde_json::from_str(chapters_json) {
            Ok(c) => c,
            Err(e) => return ActionResult::err("invalid", format!("invalid chapters: {e}")),
        };
        let patch: Option<TrdMetaPatch> = match meta_patch_json {
            Some(raw) if !raw.trim().is_empty() => match serde_json::from_str(raw) {
                Ok(p) => Some(p),
                Err(e) => return ActionResult::err("invalid", format!("invalid trd patch: {e}")),
            },
            _ => None,
        };
        match ws.compose_trd(id, &chapters, patch.as_ref()) {
            Ok(_) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn update_trd_meta(&self, id: &str, patch_json: &str) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        let patch: TrdMetaPatch = match serde_json::from_str(patch_json) {
            Ok(p) => p,
            Err(e) => return ActionResult::err("invalid", format!("invalid trd patch: {e}")),
        };
        match ws.update_trd_meta(id, &patch) {
            Ok(_) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn delete_trd(&self, id: &str) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.delete_trd(id) {
            Ok(()) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn set_trd_archived(&self, id: &str, archived: bool) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.set_trd_archived(id, archived) {
            Ok(_) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn preflight_trd_json(&self, id: &str) -> JsonReportResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return err_json("no_workspace", "no workspace open");
        };
        let statuses = all_snippet_verify_statuses(&ws).unwrap_or_default();
        match ws.preflight_trd(id, &statuses) {
            Ok(issues) => ok_json(&serde_json::json!({ "issues": issues })),
            Err(e) => err_json_storage(e),
        }
    }

    pub(crate) fn export_trd_pdf(&self, id: &str) -> WorkspaceFile {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return WorkspaceFile {
                error: Some("no workspace open".into()),
                ..Default::default()
            };
        };
        let mut compiler = self.compiler.lock().unwrap();
        compiler.sync_root(&ws);
        match publish::export_trd_pdf(&ws, &mut compiler.engine, id, None) {
            Ok((Some(rel_path), _report)) => WorkspaceFile {
                path: rel_path,
                ..Default::default()
            },
            Ok((None, report)) => {
                let err = report
                    .diagnostics
                    .iter()
                    .find(|d| d.is_error())
                    .map(|d| d.message.clone())
                    .unwrap_or_else(|| "导出失败".into());
                WorkspaceFile {
                    path: publish::default_trd_output(&ws, id),
                    error: Some(err),
                    ..Default::default()
                }
            }
            Err(e) => WorkspaceFile {
                path: publish::default_trd_output(&ws, id),
                error: Some(e.to_string()),
                ..Default::default()
            },
        }
    }

    pub(crate) fn trd_export_status(&self, id: &str) -> PdfFreshnessJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return PdfFreshnessJs::default();
        };
        let rel_path = publish::default_trd_output(&ws, id);
        let abs = ws.root().join(&rel_path);
        let exists = abs.is_file();
        PdfFreshnessJs {
            rel_path,
            abs_path: abs.display().to_string(),
            exists,
            fresh: exists,
        }
    }

    pub(crate) fn freeze_trd(&self, id: &str, strict: bool) -> JsonReportResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return err_json("no_workspace", "no workspace open");
        };
        let statuses = all_snippet_verify_statuses(&ws).unwrap_or_default();
        let mut compiler = self.compiler.lock().unwrap();
        compiler.sync_root(&ws);
        match publish::freeze_trd_with_pdf(&ws, &mut compiler.engine, id, strict, &statuses) {
            Ok((Some(res), _report)) => {
                self.invalidate_cache();
                ok_json(&serde_json::to_value(&res).unwrap_or_default())
            }
            Ok((None, report)) => {
                let err = report
                    .diagnostics
                    .iter()
                    .find(|d| d.is_error())
                    .map(|d| d.message.clone())
                    .unwrap_or_else(|| "定稿失败".into());
                err_json("compile_error", err)
            }
            Err(e) => err_json_storage(e),
        }
    }

    pub(crate) fn diff_trd_for_unfreeze_json(&self, id: &str) -> JsonReportResultJs {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return err_json("no_workspace", "no workspace open");
        };
        match ws.diff_trd_for_unfreeze(id) {
            Ok(diff) => ok_json(&serde_json::to_value(&diff).unwrap_or_default()),
            Err(e) => err_json_storage(e),
        }
    }

    pub(crate) fn unfreeze_trd(&self, id: &str) -> ActionResult {
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return ActionResult::err("no_workspace", "no workspace open");
        };
        match ws.unfreeze_trd(id) {
            Ok(_) => {
                self.invalidate_cache();
                ActionResult::ok()
            }
            Err(e) => ActionResult::from_storage_err(e),
        }
    }

    pub(crate) fn list_trd_templates_json(&self) -> String {
        let presets = xcpc_engine::list_presets_by_kind(TemplateKind::Trd);
        let ws = self.core.lock().unwrap().workspace.clone();
        let Some(ws) = ws else {
            return serde_json::to_string(&serde_json::json!({
                "default": "trd-compact",
                "templates": [],
                "presets": presets,
                "fonts": [],
            }))
            .unwrap_or_else(|_| "{}".into());
        };
        let root = ws.root().to_path_buf();
        let fingerprint = crate::state::template_files_fingerprint(&root);
        let cached = {
            let guard = self.trd_templates_cache.lock().unwrap();
            guard
                .as_ref()
                .filter(|c| c.root == root && c.fingerprint == fingerprint)
                .map(|c| (c.templates.clone(), c.fonts.clone()))
        };
        let (templates, fonts) = match cached {
            Some(hit) => hit,
            None => {
                let (templates, fonts) = {
                    let mut engine = self.math_engine.lock().unwrap();
                    engine.set_root(Some(root.clone()));
                    engine.reset_files();
                    let templates =
                        publish::list_templates_by_kind(&ws, &engine, TemplateKind::Trd);
                    let mut fonts: Vec<String> = engine
                        .font_families()
                        .into_iter()
                        .map(|(name, _)| name)
                        .collect();
                    fonts.sort();
                    fonts.dedup();
                    (templates, fonts)
                };
                *self.trd_templates_cache.lock().unwrap() = Some(crate::state::TemplatesCache {
                    root,
                    fingerprint,
                    templates: templates.clone(),
                    fonts: fonts.clone(),
                });
                (templates, fonts)
            }
        };
        serde_json::to_string(&serde_json::json!({
            "default": "trd-compact",
            "templates": templates,
            "presets": presets,
            "fonts": fonts,
        }))
        .unwrap_or_else(|_| "{}".into())
    }
}
