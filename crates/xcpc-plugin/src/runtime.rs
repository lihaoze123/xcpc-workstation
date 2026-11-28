use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rquickjs::context::EvalOptions;
use rquickjs::{CatchResultExt, CaughtError, Context, Ctx, Exception, Function, Runtime};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::archive::{CreateZipEntry, create_zip, list_zip, read_zip, sha256_encoded};
use crate::credentials::CredentialStore;
use crate::error::{PluginError, Result};
use crate::http::{HttpClient, HttpRequest};
use crate::kv::KvStore;
use crate::manifest::PluginManifest;
use crate::render::{
    RenderMathOptions, RenderTypstOptions, highlight_code, render_math_snippet,
    render_typst_snippet, svg_to_png_base64, svg_to_rgba,
};
use crate::workspace::{
    CreateProblemInput, UpdateProblemPatch, UpsertSnippetInput, WorkspaceReader, WorkspaceSample,
    WorkspaceWriter,
};

pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
pub const DEFAULT_MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct SandboxLimits {
    pub timeout: Duration,
    pub memory_limit_bytes: usize,
}

impl Default for SandboxLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
            memory_limit_bytes: DEFAULT_MEMORY_LIMIT_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginLogEntry {
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallResult {
    pub value: serde_json::Value,
    pub logs: Vec<PluginLogEntry>,
}

pub struct HostContext {
    pub manifest: PluginManifest,
    pub http_client: Arc<dyn HttpClient>,
    pub credentials: Arc<dyn CredentialStore>,
    pub kv: Arc<KvStore>,
    pub workspace: Option<Arc<dyn WorkspaceReader>>,
    pub workspace_writer: Option<Arc<dyn WorkspaceWriter>>,
    pub limits: SandboxLimits,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CredentialHeaderSpec {
    Structured {
        header: String,
        key: String,
        #[serde(default)]
        prefix: Option<String>,
    },
    Map(BTreeMap<String, String>),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsFetchRequest {
    url: String,
    #[serde(default = "default_get")]
    method: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    credential_header: Option<CredentialHeaderSpec>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    response_type: crate::http::HttpResponseType,
}

fn default_get() -> String {
    "GET".to_string()
}

#[derive(Debug, Serialize)]
struct HtmlElementMatch {
    text: String,
    html: String,
    attrs: BTreeMap<String, String>,
}

const HOST_BOOTSTRAP_JS: &str = r#"
(function() {
  const _httpFetch = globalThis.__xcpc_http_fetch;
  const _credGet = globalThis.__xcpc_cred_get;
  const _htmlSelect = globalThis.__xcpc_html_select;
  const _kvGet = globalThis.__xcpc_kv_get;
  const _kvSet = globalThis.__xcpc_kv_set;
  const _kvDelete = globalThis.__xcpc_kv_delete;
  const _wsRead = globalThis.__xcpc_ws_read;
  const _wsWrite = globalThis.__xcpc_ws_write;
  const _svgToPng = globalThis.__xcpc_render_svg_to_png;
  const _svgToRgba = globalThis.__xcpc_render_svg_to_rgba;
  const _renderTypst = globalThis.__xcpc_render_typst;
  const _renderMath = globalThis.__xcpc_render_math;
  const _archiveListZip = globalThis.__xcpc_archive_list_zip;
  const _archiveReadZip = globalThis.__xcpc_archive_read_zip;
  const _archiveCreateZip = globalThis.__xcpc_archive_create_zip;
  const _cryptoSha256 = globalThis.__xcpc_crypto_sha256;
  const _highlight = globalThis.__xcpc_highlight;
  const _log = globalThis.__xcpc_log;

  delete globalThis.__xcpc_http_fetch;
  delete globalThis.__xcpc_cred_get;
  delete globalThis.__xcpc_html_select;
  delete globalThis.__xcpc_kv_get;
  delete globalThis.__xcpc_kv_set;
  delete globalThis.__xcpc_kv_delete;
  delete globalThis.__xcpc_ws_read;
  delete globalThis.__xcpc_ws_write;
  delete globalThis.__xcpc_render_svg_to_png;
  delete globalThis.__xcpc_render_svg_to_rgba;
  delete globalThis.__xcpc_render_typst;
  delete globalThis.__xcpc_render_math;
  delete globalThis.__xcpc_archive_list_zip;
  delete globalThis.__xcpc_archive_read_zip;
  delete globalThis.__xcpc_archive_create_zip;
  delete globalThis.__xcpc_crypto_sha256;
  delete globalThis.__xcpc_highlight;
  delete globalThis.__xcpc_log;

  globalThis.host = Object.freeze({
    http: Object.freeze({
      fetch(req) {
        const raw = _httpFetch(JSON.stringify(req ?? {}));
        return JSON.parse(raw);
      },
    }),
    credentials: Object.freeze({
      get(key) {
        const val = _credGet(String(key));
        return val === undefined ? null : val;
      },
    }),
    html: Object.freeze({
      select(html, cssSelector) {
        return JSON.parse(_htmlSelect(String(html ?? ""), String(cssSelector ?? "")));
      },
    }),
    kv: Object.freeze({
      get(key) {
        const raw = _kvGet(String(key));
        return raw == null ? null : JSON.parse(raw);
      },
      set(key, val) {
        _kvSet(String(key), JSON.stringify(val === undefined ? null : val));
      },
      delete(key) {
        return Boolean(_kvDelete(String(key)));
      },
    }),
    workspace: Object.freeze({
      listProblems() {
        return JSON.parse(_wsRead("listProblems", "{}"));
      },
      getProblem(id) {
        return JSON.parse(_wsRead("getProblem", JSON.stringify({ id: String(id ?? "") })));
      },
      listContests() {
        return JSON.parse(_wsRead("listContests", "{}"));
      },
      getContest(id) {
        return JSON.parse(_wsRead("getContest", JSON.stringify({ id: String(id ?? "") })));
      },
      listSnippets() {
        return JSON.parse(_wsRead("listSnippets", "{}"));
      },
      getSnippet(id) {
        return JSON.parse(_wsRead("getSnippet", JSON.stringify({ id: String(id ?? "") })));
      },
      listTrds() {
        return JSON.parse(_wsRead("listTrds", "{}"));
      },
      getTrd(id) {
        return JSON.parse(_wsRead("getTrd", JSON.stringify({ id: String(id ?? "") })));
      },
      createProblem(draft) {
        return JSON.parse(_wsWrite("createProblem", JSON.stringify(draft ?? {})));
      },
      updateProblem(id, patch) {
        return JSON.parse(_wsWrite("updateProblem", JSON.stringify({ id: String(id ?? ""), patch: patch ?? {} })));
      },
      setSamples(problemId, samples) {
        return JSON.parse(_wsWrite("setSamples", JSON.stringify({ problemId: String(problemId ?? ""), samples: samples ?? [] })));
      },
      upsertSnippet(input) {
        return JSON.parse(_wsWrite("upsertSnippet", JSON.stringify(input ?? {})));
      },
    }),
    render: Object.freeze({
      svgToPng(svg, scale) {
        return _svgToPng(String(svg ?? ""), scale == null ? 1.0 : Number(scale));
      },
      svgToRgba(svg, scale) {
        return JSON.parse(_svgToRgba(String(svg ?? ""), scale == null ? 1.0 : Number(scale)));
      },
      renderTypst(source, opts) {
        return JSON.parse(_renderTypst(String(source ?? ""), JSON.stringify(opts ?? {})));
      },
      renderMath(latex, opts) {
        return JSON.parse(_renderMath(String(latex ?? ""), JSON.stringify(opts ?? {})));
      },
    }),
    archive: Object.freeze({
      listZip(zipBase64) {
        return JSON.parse(_archiveListZip(String(zipBase64 ?? "")));
      },
      readZip(zipBase64, opts) {
        return JSON.parse(_archiveReadZip(String(zipBase64 ?? ""), JSON.stringify(opts ?? {})));
      },
      createZip(entries) {
        return _archiveCreateZip(JSON.stringify(entries ?? []));
      },
    }),
    crypto: Object.freeze({
      sha256(input, encoding) {
        return _cryptoSha256(String(input ?? ""), encoding == null ? "utf8" : String(encoding));
      },
    }),
    highlight(code, lang) {
      return JSON.parse(_highlight(String(code ?? ""), String(lang ?? "text")));
    },
    log(level, msg) {
      _log(
        String(level ?? "info"),
        typeof msg === "string" ? msg : JSON.stringify(msg)
      );
    },
  });
})();
"#;

pub struct PluginRuntime;

impl PluginRuntime {
    /// Executes an exported function `fn_name` from `source` inside an isolated QuickJS sandbox.
    pub fn call_function(
        source: &str,
        fn_name: &str,
        args: &[serde_json::Value],
        host_ctx: &HostContext,
    ) -> Result<CallResult> {
        let plugin_id = host_ctx.manifest.id.clone();
        let limits = host_ctx.limits;

        let rt = Runtime::new().map_err(|e| PluginError::JsError(e.to_string()))?;
        rt.set_memory_limit(limits.memory_limit_bytes);
        // Cap stack size to 1MB to guard against infinite recursion.
        rt.set_max_stack_size(1024 * 1024);

        let deadline = Instant::now() + limits.timeout;
        let timed_out = Arc::new(AtomicBool::new(false));
        let timed_out_flag = Arc::clone(&timed_out);
        rt.set_interrupt_handler(Some(Box::new(move || {
            if Instant::now() >= deadline {
                timed_out_flag.store(true, Ordering::SeqCst);
                true
            } else {
                false
            }
        })));

        let logs: Arc<Mutex<Vec<PluginLogEntry>>> = Arc::new(Mutex::new(Vec::new()));
        let last_host_error: Arc<Mutex<Option<PluginError>>> = Arc::new(Mutex::new(None));

        let ctx = Context::full(&rt).map_err(|e| PluginError::JsError(e.to_string()))?;

        let eval_outcome: Result<Option<String>> = ctx.with(|ctx| {
            register_native_bridges(&ctx, host_ctx, &logs, &last_host_error)?;

            // Build frozen `host` global and hide raw native bridges.
            eval_checked(&ctx, HOST_BOOTSTRAP_JS)?;

            // Load plugin module in CommonJS wrapper (keep prefix on line 1 so stack line numbers match source).
            let entry_filename = format!(
                "xcpc-plugin://{}/{}",
                host_ctx.manifest.id, host_ctx.manifest.entry
            );
            let wrapped = format!(
                "globalThis.__xcpc_exports = (function() {{ const exports = {{}}; const module = {{ exports }}; (function(module, exports) {{{source}\n}})(module, exports); return module.exports; }})();"
            );
            eval_checked_with_filename(&ctx, &wrapped, &entry_filename)?;

            // Invoke target function (sync or Promise-returning).
            let fn_name_json = serde_json::to_string(fn_name)?;
            let args_json = serde_json::to_string(args)?;
            let invoke_js = format!(
                r#"
                (function() {{
                  const mod = globalThis.__xcpc_exports || {{}};
                  const fn = typeof mod[{fn_name_json}] === "function"
                    ? mod[{fn_name_json}]
                    : (typeof globalThis[{fn_name_json}] === "function" ? globalThis[{fn_name_json}] : null);
                  if (!fn) {{
                    throw new Error("FUNCTION_NOT_FOUND: plugin does not export '" + {fn_name_json} + "'");
                  }}
                  const args = {args_json};
                  const out = fn.apply(mod, args);
                  if (out && typeof out.then === "function") {{
                    globalThis.__xcpc_settled = false;
                    globalThis.__xcpc_result = undefined;
                    globalThis.__xcpc_error = undefined;
                    out.then(
                      (v) => {{ globalThis.__xcpc_result = v; globalThis.__xcpc_settled = true; }},
                      (e) => {{ globalThis.__xcpc_error = e; globalThis.__xcpc_settled = true; }}
                    );
                    return null;
                  }}
                  return JSON.stringify(out === undefined ? null : out);
                }})()
                "#
            );

            eval_checked_opt_string(&ctx, &invoke_js)
        });

        // Drive pending jobs if the function returned a Promise.
        let result_json = match eval_outcome {
            Ok(Some(json_str)) => json_str,
            Ok(None) => {
                while rt.is_job_pending() {
                    if timed_out.load(Ordering::SeqCst) {
                        return Err(PluginError::Timeout {
                            plugin_id,
                            timeout_ms: limits.timeout.as_millis() as u64,
                        });
                    }
                    match rt.execute_pending_job() {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(e) => {
                            return Err(classify_js_error(
                                &plugin_id,
                                limits,
                                &timed_out,
                                &last_host_error,
                                e.to_string(),
                            ));
                        }
                    }
                }
                ctx.with(|ctx| {
                    let check_js = r#"
                    (function() {
                      if (!globalThis.__xcpc_settled) {
                        throw new Error("PROMISE_UNSETTLED: async plugin function did not settle");
                      }
                      if (globalThis.__xcpc_error !== undefined) {
                        throw globalThis.__xcpc_error;
                      }
                      return JSON.stringify(globalThis.__xcpc_result === undefined ? null : globalThis.__xcpc_result);
                    })()
                    "#;
                    eval_checked_string(&ctx, check_js)
                })
                .map_err(|e| {
                    classify_js_error(
                        &plugin_id,
                        limits,
                        &timed_out,
                        &last_host_error,
                        e.to_string(),
                    )
                })?
            }
            Err(e) => {
                return Err(classify_js_error(
                    &plugin_id,
                    limits,
                    &timed_out,
                    &last_host_error,
                    e.to_string(),
                ));
            }
        };

        let value: serde_json::Value = serde_json::from_str(&result_json)?;
        let collected_logs = logs.lock().unwrap().clone();
        Ok(CallResult {
            value,
            logs: collected_logs,
        })
    }

    /// Returns whether a plugin script exports the given function name.
    pub fn has_export(source: &str, fn_name: &str, host_ctx: &HostContext) -> Result<bool> {
        let exports = Self::list_exports(source, host_ctx)?;
        Ok(exports.iter().any(|name| name == fn_name))
    }

    /// Dry-run loads `source` inside the sandbox and returns the list of exported function names.
    pub fn list_exports(source: &str, host_ctx: &HostContext) -> Result<Vec<String>> {
        let rt = Runtime::new().map_err(|e| PluginError::JsError(e.to_string()))?;
        rt.set_memory_limit(host_ctx.limits.memory_limit_bytes);
        rt.set_max_stack_size(1024 * 1024);
        let deadline = Instant::now() + host_ctx.limits.timeout;
        let timed_out = Arc::new(AtomicBool::new(false));
        let timed_out_flag = Arc::clone(&timed_out);
        rt.set_interrupt_handler(Some(Box::new(move || {
            if Instant::now() >= deadline {
                timed_out_flag.store(true, Ordering::SeqCst);
                true
            } else {
                false
            }
        })));
        let ctx = Context::full(&rt).map_err(|e| PluginError::JsError(e.to_string()))?;
        let logs: Arc<Mutex<Vec<PluginLogEntry>>> = Arc::new(Mutex::new(Vec::new()));
        let last_err: Arc<Mutex<Option<PluginError>>> = Arc::new(Mutex::new(None));
        let entry_filename = format!(
            "xcpc-plugin://{}/{}",
            host_ctx.manifest.id, host_ctx.manifest.entry
        );
        let json = ctx
            .with(|ctx| {
                register_native_bridges(&ctx, host_ctx, &logs, &last_err)?;
                eval_checked(&ctx, HOST_BOOTSTRAP_JS)?;
                let wrapped = format!(
                    "globalThis.__xcpc_exports = (function() {{ const exports = {{}}; const module = {{ exports }}; (function(module, exports) {{{source}\n}})(module, exports); return module.exports; }})();"
                );
                eval_checked_with_filename(&ctx, &wrapped, &entry_filename)?;
                let collect = r#"
                (function() {
                  const mod = globalThis.__xcpc_exports || {};
                  const names = new Set();
                  for (const k of Object.keys(mod)) {
                    if (typeof mod[k] === "function") names.add(k);
                  }
                  return JSON.stringify(Array.from(names));
                })()
                "#;
                eval_checked_string(&ctx, collect)
            })
            .map_err(|e| {
                classify_js_error(
                    &host_ctx.manifest.id,
                    host_ctx.limits,
                    &timed_out,
                    &last_err,
                    e.to_string(),
                )
            })?;
        let names: Vec<String> = serde_json::from_str(&json)?;
        Ok(names)
    }
}

fn classify_js_error(
    plugin_id: &str,
    limits: SandboxLimits,
    timed_out: &AtomicBool,
    last_host_error: &Mutex<Option<PluginError>>,
    msg: String,
) -> PluginError {
    if timed_out.load(Ordering::SeqCst) {
        return PluginError::Timeout {
            plugin_id: plugin_id.to_string(),
            timeout_ms: limits.timeout.as_millis() as u64,
        };
    }
    if let Some(host_err) = last_host_error.lock().unwrap().take() {
        return host_err;
    }
    let lower = msg.to_ascii_lowercase();
    if lower.contains("out of memory") || lower.contains("allocation failed") {
        return PluginError::MemoryLimit {
            plugin_id: plugin_id.to_string(),
            limit_bytes: limits.memory_limit_bytes,
        };
    }
    PluginError::JsError(msg)
}

fn format_caught_error<'js>(err: CaughtError<'js>) -> String {
    match err {
        CaughtError::Exception(exc) => {
            let msg = exc
                .message()
                .unwrap_or_else(|| "JavaScript exception".to_string());
            if let Some(stack) = exc.stack() {
                let stack_trimmed = stack.trim();
                if !stack_trimmed.is_empty() && !msg.contains(stack_trimmed) {
                    return format!("{msg}\n{stack_trimmed}");
                }
            }
            msg
        }
        other => format!("{other}"),
    }
}

fn eval_checked_with_filename<'js>(ctx: &Ctx<'js>, code: &str, filename: &str) -> Result<()> {
    let mut opts = EvalOptions::default();
    opts.global = true;
    opts.filename = Some(filename.to_string());
    ctx.eval_with_options::<(), _>(code, opts)
        .catch(ctx)
        .map_err(|e| PluginError::JsError(format_caught_error(e)))
}

fn eval_checked<'js>(ctx: &Ctx<'js>, code: &str) -> Result<()> {
    ctx.eval::<(), _>(code)
        .catch(ctx)
        .map_err(|e| PluginError::JsError(format_caught_error(e)))
}

fn eval_checked_string<'js>(ctx: &Ctx<'js>, code: &str) -> Result<String> {
    ctx.eval::<String, _>(code)
        .catch(ctx)
        .map_err(|e| PluginError::JsError(format_caught_error(e)))
}

fn eval_checked_opt_string<'js>(ctx: &Ctx<'js>, code: &str) -> Result<Option<String>> {
    ctx.eval::<Option<String>, _>(code)
        .catch(ctx)
        .map_err(|e| PluginError::JsError(format_caught_error(e)))
}

fn register_native_bridges<'js>(
    ctx: &Ctx<'js>,
    host_ctx: &HostContext,
    logs: &Arc<Mutex<Vec<PluginLogEntry>>>,
    last_host_error: &Arc<Mutex<Option<PluginError>>>,
) -> Result<()> {
    let globals = ctx.globals();

    // 1. host.http.fetch
    {
        let manifest = host_ctx.manifest.clone();
        let http = Arc::clone(&host_ctx.http_client);
        let creds = Arc::clone(&host_ctx.credentials);
        let last_err = Arc::clone(last_host_error);
        let func = Function::new(ctx.clone(), move |ctx: Ctx<'js>, req_json: String| {
            let parsed: JsFetchRequest = match serde_json::from_str(&req_json) {
                Ok(r) => r,
                Err(e) => {
                    return Err(Exception::throw_message(
                        &ctx,
                        &format!("INVALID_REQUEST: {e}"),
                    ));
                }
            };
            let url = match Url::parse(&parsed.url) {
                Ok(u) => u,
                Err(e) => {
                    return Err(Exception::throw_message(&ctx, &format!("INVALID_URL: {e}")));
                }
            };
            if url.scheme() != "http" && url.scheme() != "https" {
                let err = PluginError::DomainNotAllowed {
                    plugin_id: manifest.id.clone(),
                    host: format!("scheme:{}", url.scheme()),
                };
                let msg = err.to_string();
                *last_err.lock().unwrap() = Some(err);
                return Err(Exception::throw_message(&ctx, &msg));
            }
            let host_str = url.host_str().unwrap_or("");
            if !manifest.is_domain_allowed(host_str) {
                let err = PluginError::DomainNotAllowed {
                    plugin_id: manifest.id.clone(),
                    host: host_str.to_string(),
                };
                let msg = err.to_string();
                *last_err.lock().unwrap() = Some(err);
                return Err(Exception::throw_message(&ctx, &msg));
            }

            // Domain is verified whitelisted; only now may we attach credentials.
            let mut headers = parsed.headers;
            let mut credential_header_names: Vec<String> = Vec::new();
            if let Some(cred_spec) = parsed.credential_header {
                match cred_spec {
                    CredentialHeaderSpec::Structured {
                        header,
                        key,
                        prefix,
                    } => match creds.get(&manifest.id, &key) {
                        Ok(Some(secret)) => {
                            let val = match prefix {
                                Some(p) => format!("{p}{secret}"),
                                None => secret,
                            };
                            credential_header_names.push(header.to_ascii_lowercase());
                            headers.insert(header, val);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            let msg = e.to_string();
                            *last_err.lock().unwrap() = Some(e);
                            return Err(Exception::throw_message(&ctx, &msg));
                        }
                    },
                    CredentialHeaderSpec::Map(map) => {
                        for (header, key) in map {
                            match creds.get(&manifest.id, &key) {
                                Ok(Some(secret)) => {
                                    credential_header_names.push(header.to_ascii_lowercase());
                                    headers.insert(header, secret);
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    let msg = e.to_string();
                                    *last_err.lock().unwrap() = Some(e);
                                    return Err(Exception::throw_message(&ctx, &msg));
                                }
                            }
                        }
                    }
                }
            }

            const MAX_REDIRECTS: usize = 5;
            let mut current_url = url;
            let mut current_method = parsed.method;
            let mut current_body = parsed.body;
            let mut redirect_count = 0usize;

            let resp = loop {
                let http_req = HttpRequest {
                    url: current_url.to_string(),
                    method: current_method.clone(),
                    headers: headers.clone(),
                    body: current_body.clone(),
                    timeout_ms: parsed.timeout_ms,
                    response_type: parsed.response_type,
                };
                let resp = match http.send(&http_req) {
                    Ok(r) => r,
                    Err(e) => {
                        let msg = e.to_string();
                        *last_err.lock().unwrap() = Some(e);
                        return Err(Exception::throw_message(&ctx, &msg));
                    }
                };

                let is_redirect = matches!(resp.status, 301 | 302 | 303 | 307 | 308);
                let location = if is_redirect {
                    resp.headers
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case("location"))
                        .map(|(_, v)| v.trim().to_string())
                        .filter(|v| !v.is_empty())
                } else {
                    None
                };

                let Some(loc) = location else {
                    break resp;
                };

                redirect_count += 1;
                if redirect_count > MAX_REDIRECTS {
                    let err = PluginError::Http("too many redirects".into());
                    let msg = err.to_string();
                    *last_err.lock().unwrap() = Some(err);
                    return Err(Exception::throw_message(&ctx, &msg));
                }

                let next_url = match current_url.join(&loc) {
                    Ok(u) => u,
                    Err(e) => {
                        let err =
                            PluginError::Http(format!("invalid redirect location '{loc}': {e}"));
                        let msg = err.to_string();
                        *last_err.lock().unwrap() = Some(err);
                        return Err(Exception::throw_message(&ctx, &msg));
                    }
                };

                if next_url.scheme() != "http" && next_url.scheme() != "https" {
                    let err = PluginError::DomainNotAllowed {
                        plugin_id: manifest.id.clone(),
                        host: format!("scheme:{}", next_url.scheme()),
                    };
                    let msg = err.to_string();
                    *last_err.lock().unwrap() = Some(err);
                    return Err(Exception::throw_message(&ctx, &msg));
                }
                let next_host = next_url.host_str().unwrap_or("");
                if !manifest.is_domain_allowed(next_host) {
                    let err = PluginError::DomainNotAllowed {
                        plugin_id: manifest.id.clone(),
                        host: next_host.to_string(),
                    };
                    let msg = err.to_string();
                    *last_err.lock().unwrap() = Some(err);
                    return Err(Exception::throw_message(&ctx, &msg));
                }

                // Strip credential and sensitive auth headers on cross-host / cross-origin redirects.
                let same_origin = next_url.scheme().eq_ignore_ascii_case(current_url.scheme())
                    && next_url
                        .host_str()
                        .zip(current_url.host_str())
                        .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
                    && next_url.port_or_known_default() == current_url.port_or_known_default();
                if !same_origin {
                    headers.retain(|k, _| {
                        let lower = k.to_ascii_lowercase();
                        !matches!(
                            lower.as_str(),
                            "authorization" | "proxy-authorization" | "cookie"
                        ) && !credential_header_names.iter().any(|c| c == &lower)
                    });
                }

                if resp.status == 303
                    || ((resp.status == 301 || resp.status == 302)
                        && current_method.eq_ignore_ascii_case("POST"))
                {
                    current_method = "GET".to_string();
                    current_body = None;
                }
                current_url = next_url;
            };

            serde_json::to_string(&resp).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
        })
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_http_fetch", func)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }

    // 2. host.credentials.get
    {
        let plugin_id = host_ctx.manifest.id.clone();
        let creds = Arc::clone(&host_ctx.credentials);
        let last_err = Arc::clone(last_host_error);
        let func = Function::new(ctx.clone(), move |ctx: Ctx<'js>, key: String| {
            match creds.get(&plugin_id, &key) {
                Ok(v) => Ok(v),
                Err(e) => {
                    let msg = e.to_string();
                    *last_err.lock().unwrap() = Some(e);
                    Err(Exception::throw_message(&ctx, &msg))
                }
            }
        })
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_cred_get", func)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }

    // 3. host.html.select
    {
        let func = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, html: String, css_selector: String| {
                let selector = match Selector::parse(&css_selector) {
                    Ok(s) => s,
                    Err(e) => {
                        return Err(Exception::throw_message(
                            &ctx,
                            &format!("INVALID_CSS_SELECTOR: {e:?}"),
                        ));
                    }
                };
                let doc = Html::parse_document(&html);
                let mut matches = Vec::new();
                for el in doc.select(&selector) {
                    let text: String = el.text().collect();
                    let inner_html = el.inner_html();
                    let mut attrs = BTreeMap::new();
                    for (k, v) in el.value().attrs() {
                        attrs.insert(k.to_string(), v.to_string());
                    }
                    matches.push(HtmlElementMatch {
                        text,
                        html: inner_html,
                        attrs,
                    });
                }
                serde_json::to_string(&matches)
                    .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
            },
        )
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_html_select", func)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }

    // 4. host.kv.{get, set, delete}
    {
        let kv = Arc::clone(&host_ctx.kv);
        let func_get = Function::new(ctx.clone(), move |ctx: Ctx<'js>, key: String| {
            match kv.get(&key) {
                Ok(Some(v)) => serde_json::to_string(&v)
                    .map(Some)
                    .map_err(|e| Exception::throw_message(&ctx, &e.to_string())),
                Ok(None) => Ok(None),
                Err(e) => Err(Exception::throw_message(&ctx, &e.to_string())),
            }
        })
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_kv_get", func_get)
            .map_err(|e| PluginError::JsError(e.to_string()))?;

        let kv = Arc::clone(&host_ctx.kv);
        let func_set = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, key: String, val_json: String| {
                let val: serde_json::Value = serde_json::from_str(&val_json)
                    .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;
                kv.set(&key, val)
                    .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
            },
        )
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_kv_set", func_set)
            .map_err(|e| PluginError::JsError(e.to_string()))?;

        let kv = Arc::clone(&host_ctx.kv);
        let func_del = Function::new(ctx.clone(), move |ctx: Ctx<'js>, key: String| {
            kv.delete(&key)
                .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
        })
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_kv_delete", func_del)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }

    // 5. host.workspace (unified read + controlled write)
    {
        let plugin_id = host_ctx.manifest.id.clone();
        let allowed_read = host_ctx.manifest.permissions.workspace_read;
        let ws = host_ctx.workspace.clone();
        let last_err = Arc::clone(last_host_error);
        let func_read = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, method: String, args_json: String| {
                if !allowed_read {
                    let err = PluginError::PermissionDenied {
                        plugin_id: plugin_id.clone(),
                        permission: "workspace_read".into(),
                    };
                    let msg = err.to_string();
                    *last_err.lock().unwrap() = Some(err);
                    return Err(Exception::throw_message(&ctx, &msg));
                }
                let Some(ref reader) = ws else {
                    let err = PluginError::PermissionDenied {
                        plugin_id: plugin_id.clone(),
                        permission: "workspace_read (no workspace attached)".into(),
                    };
                    let msg = err.to_string();
                    *last_err.lock().unwrap() = Some(err);
                    return Err(Exception::throw_message(&ctx, &msg));
                };
                let args: serde_json::Value =
                    serde_json::from_str(&args_json).unwrap_or(serde_json::Value::Null);
                let id_arg = || {
                    args.get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned()
                };
                let serialized: Result<String> = match method.as_str() {
                    "listProblems" => reader
                        .list_problems()
                        .and_then(|v| Ok(serde_json::to_string(&v)?)),
                    "getProblem" => reader
                        .get_problem(&id_arg())
                        .and_then(|v| Ok(serde_json::to_string(&v)?)),
                    "listContests" => reader
                        .list_contests()
                        .and_then(|v| Ok(serde_json::to_string(&v)?)),
                    "getContest" => reader
                        .get_contest(&id_arg())
                        .and_then(|v| Ok(serde_json::to_string(&v)?)),
                    "listSnippets" => reader
                        .list_snippets()
                        .and_then(|v| Ok(serde_json::to_string(&v)?)),
                    "getSnippet" => reader
                        .get_snippet(&id_arg())
                        .and_then(|v| Ok(serde_json::to_string(&v)?)),
                    "listTrds" => reader
                        .list_trds()
                        .and_then(|v| Ok(serde_json::to_string(&v)?)),
                    "getTrd" => reader
                        .get_trd(&id_arg())
                        .and_then(|v| Ok(serde_json::to_string(&v)?)),
                    other => Err(PluginError::InvalidArgument(format!(
                        "unknown workspace read method '{other}'"
                    ))),
                };
                match serialized {
                    Ok(json) => Ok(json),
                    Err(e) => {
                        let msg = e.to_string();
                        *last_err.lock().unwrap() = Some(e);
                        Err(Exception::throw_message(&ctx, &msg))
                    }
                }
            },
        )
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_ws_read", func_read)
            .map_err(|e| PluginError::JsError(e.to_string()))?;

        let plugin_id = host_ctx.manifest.id.clone();
        let allowed_write = host_ctx.manifest.permissions.workspace_write;
        let writer = host_ctx.workspace_writer.clone();
        let last_err = Arc::clone(last_host_error);
        let func_write = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, method: String, args_json: String| {
                if !allowed_write {
                    let err = PluginError::PermissionDenied {
                        plugin_id: plugin_id.clone(),
                        permission: "workspace_write".into(),
                    };
                    let msg = err.to_string();
                    *last_err.lock().unwrap() = Some(err);
                    return Err(Exception::throw_message(&ctx, &msg));
                }
                let Some(ref writer) = writer else {
                    let err =
                        PluginError::WorkspaceReadOnly("no writable workspace is attached".into());
                    let msg = err.to_string();
                    *last_err.lock().unwrap() = Some(err);
                    return Err(Exception::throw_message(&ctx, &msg));
                };
                let serialized: Result<String> = (|| match method.as_str() {
                    "createProblem" => {
                        let input: CreateProblemInput = serde_json::from_str(&args_json)
                            .map_err(|e| PluginError::InvalidArgument(e.to_string()))?;
                        writer
                            .create_problem(input)
                            .and_then(|v| Ok(serde_json::to_string(&v)?))
                    }
                    "updateProblem" => {
                        #[derive(Deserialize)]
                        struct Args {
                            id: String,
                            patch: UpdateProblemPatch,
                        }
                        let args: Args = serde_json::from_str(&args_json)
                            .map_err(|e| PluginError::InvalidArgument(e.to_string()))?;
                        writer
                            .update_problem(&args.id, args.patch)
                            .and_then(|v| Ok(serde_json::to_string(&v)?))
                    }
                    "setSamples" => {
                        #[derive(Deserialize)]
                        #[serde(rename_all = "camelCase")]
                        struct Args {
                            problem_id: String,
                            samples: Vec<WorkspaceSample>,
                        }
                        let args: Args = serde_json::from_str(&args_json)
                            .map_err(|e| PluginError::InvalidArgument(e.to_string()))?;
                        writer
                            .set_samples(&args.problem_id, args.samples)
                            .and_then(|v| Ok(serde_json::to_string(&v)?))
                    }
                    "upsertSnippet" => {
                        let input: UpsertSnippetInput = serde_json::from_str(&args_json)
                            .map_err(|e| PluginError::InvalidArgument(e.to_string()))?;
                        writer
                            .upsert_snippet(input)
                            .and_then(|v| Ok(serde_json::to_string(&v)?))
                    }
                    other => Err(PluginError::InvalidArgument(format!(
                        "unknown workspace write method '{other}'"
                    ))),
                })();
                match serialized {
                    Ok(json) => Ok(json),
                    Err(e) => {
                        let msg = e.to_string();
                        *last_err.lock().unwrap() = Some(e);
                        Err(Exception::throw_message(&ctx, &msg))
                    }
                }
            },
        )
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_ws_write", func_write)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }

    // 6. host.render.{svgToPng, svgToRgba, renderTypst, renderMath}, host.archive.*, host.crypto.sha256 & host.highlight
    {
        let last_err = Arc::clone(last_host_error);
        let func = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, svg: String, scale: f64| match svg_to_png_base64(
                &svg,
                Some(scale as f32),
            ) {
                Ok(b64) => Ok(b64),
                Err(e) => {
                    let msg = e.to_string();
                    *last_err.lock().unwrap() = Some(e);
                    Err(Exception::throw_message(&ctx, &msg))
                }
            },
        )
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_render_svg_to_png", func)
            .map_err(|e| PluginError::JsError(e.to_string()))?;

        let last_err = Arc::clone(last_host_error);
        let func_rgba = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, svg: String, scale: f64| match svg_to_rgba(&svg, scale as f32) {
                Ok(rendered) => serde_json::to_string(&rendered)
                    .map_err(|e| Exception::throw_message(&ctx, &e.to_string())),
                Err(e) => {
                    let msg = e.to_string();
                    *last_err.lock().unwrap() = Some(e);
                    Err(Exception::throw_message(&ctx, &msg))
                }
            },
        )
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_render_svg_to_rgba", func_rgba)
            .map_err(|e| PluginError::JsError(e.to_string()))?;

        let last_err = Arc::clone(last_host_error);
        let func_typst = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, source: String, opts_json: String| {
                let opts: RenderTypstOptions = serde_json::from_str(&opts_json).unwrap_or_default();
                match render_typst_snippet(&source, &opts) {
                    Ok(rendered) => serde_json::to_string(&rendered)
                        .map_err(|e| Exception::throw_message(&ctx, &e.to_string())),
                    Err(e) => {
                        let msg = e.to_string();
                        *last_err.lock().unwrap() = Some(e);
                        Err(Exception::throw_message(&ctx, &msg))
                    }
                }
            },
        )
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_render_typst", func_typst)
            .map_err(|e| PluginError::JsError(e.to_string()))?;

        let last_err = Arc::clone(last_host_error);
        let func_math = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, latex: String, opts_json: String| {
                let opts: RenderMathOptions = serde_json::from_str(&opts_json).unwrap_or_default();
                match render_math_snippet(&latex, &opts) {
                    Ok(rendered) => serde_json::to_string(&rendered)
                        .map_err(|e| Exception::throw_message(&ctx, &e.to_string())),
                    Err(e) => {
                        let msg = e.to_string();
                        *last_err.lock().unwrap() = Some(e);
                        Err(Exception::throw_message(&ctx, &msg))
                    }
                }
            },
        )
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_render_math", func_math)
            .map_err(|e| PluginError::JsError(e.to_string()))?;

        let last_err = Arc::clone(last_host_error);
        let func_list_zip =
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'js>, b64: String| match list_zip(&b64) {
                    Ok(entries) => serde_json::to_string(&entries)
                        .map_err(|e| Exception::throw_message(&ctx, &e.to_string())),
                    Err(e) => {
                        let msg = e.to_string();
                        *last_err.lock().unwrap() = Some(e);
                        Err(Exception::throw_message(&ctx, &msg))
                    }
                },
            )
            .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_archive_list_zip", func_list_zip)
            .map_err(|e| PluginError::JsError(e.to_string()))?;

        let last_err = Arc::clone(last_host_error);
        let func_read_zip = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, b64: String, opts_json: String| {
                #[derive(Default, Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct ReadZipOpts {
                    #[serde(default)]
                    paths: Option<Vec<String>>,
                    #[serde(default)]
                    as_text: bool,
                }
                let opts: ReadZipOpts = serde_json::from_str(&opts_json).unwrap_or_default();
                match read_zip(&b64, opts.paths.as_deref(), opts.as_text) {
                    Ok(entries) => serde_json::to_string(&entries)
                        .map_err(|e| Exception::throw_message(&ctx, &e.to_string())),
                    Err(e) => {
                        let msg = e.to_string();
                        *last_err.lock().unwrap() = Some(e);
                        Err(Exception::throw_message(&ctx, &msg))
                    }
                }
            },
        )
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_archive_read_zip", func_read_zip)
            .map_err(|e| PluginError::JsError(e.to_string()))?;

        let last_err = Arc::clone(last_host_error);
        let func_create_zip =
            Function::new(ctx.clone(), move |ctx: Ctx<'js>, entries_json: String| {
                let entries: Vec<CreateZipEntry> = match serde_json::from_str(&entries_json) {
                    Ok(v) => v,
                    Err(e) => {
                        let err = PluginError::InvalidArgument(format!("invalid entries: {e}"));
                        let msg = err.to_string();
                        *last_err.lock().unwrap() = Some(err);
                        return Err(Exception::throw_message(&ctx, &msg));
                    }
                };
                match create_zip(&entries) {
                    Ok(b64) => Ok(b64),
                    Err(e) => {
                        let msg = e.to_string();
                        *last_err.lock().unwrap() = Some(e);
                        Err(Exception::throw_message(&ctx, &msg))
                    }
                }
            })
            .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_archive_create_zip", func_create_zip)
            .map_err(|e| PluginError::JsError(e.to_string()))?;

        let last_err = Arc::clone(last_host_error);
        let func_sha256 = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, input: String, encoding: String| match sha256_encoded(
                &input,
                Some(&encoding),
            ) {
                Ok(hex) => Ok(hex),
                Err(e) => {
                    let msg = e.to_string();
                    *last_err.lock().unwrap() = Some(e);
                    Err(Exception::throw_message(&ctx, &msg))
                }
            },
        )
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_crypto_sha256", func_sha256)
            .map_err(|e| PluginError::JsError(e.to_string()))?;

        let func_hl = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, code: String, lang: String| {
                let highlighted = highlight_code(&code, &lang);
                serde_json::to_string(&highlighted)
                    .map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
            },
        )
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_highlight", func_hl)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }

    // 7. host.log
    {
        let logs_ref = Arc::clone(logs);
        let func = Function::new(ctx.clone(), move |level: String, message: String| {
            logs_ref
                .lock()
                .unwrap()
                .push(PluginLogEntry { level, message });
        })
        .map_err(|e| PluginError::JsError(e.to_string()))?;
        globals
            .set("__xcpc_log", func)
            .map_err(|e| PluginError::JsError(e.to_string()))?;
    }

    Ok(())
}
