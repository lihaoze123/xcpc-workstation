//! End-to-end integration tests for all 10 MCP tools over real stdio JSON-RPC (`xcpc mcp`).

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};
use xcpc_storage::content_hash;

/// Minimal valid 1x1 transparent PNG bytes.
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpClient {
    fn spawn(workspace: &Path, read_only: bool) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_xcpc"));
        cmd.arg("-w").arg(workspace);
        if read_only {
            cmd.arg("--read-only");
        }
        cmd.arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = cmd.spawn().expect("spawn xcpc mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        let n = self.stdout.read_line(&mut line).unwrap();
        assert!(n > 0, "xcpc mcp closed stdout unexpectedly");
        let reply: Value = serde_json::from_str(&line).expect("valid JSON-RPC reply");
        assert_eq!(reply["id"], id);
        reply
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Value {
        let reply = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        assert!(
            reply.get("error").is_none(),
            "unexpected JSON-RPC error for {name}: {reply}"
        );
        reply["result"].clone()
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn err_code(result: &Value) -> String {
    assert_eq!(result["isError"], true, "expected isError=true: {result}");
    let text = result["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).expect("error content JSON");
    parsed["code"].as_str().unwrap().to_owned()
}

fn setup_workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    xcpc_storage::init_workspace(dir.path()).unwrap();

    // Problem 1: alpha (with editorial + solution + tags + codeforces source + unique body token)
    let p1 = dir.path().join("problems/alpha");
    fs::create_dir_all(p1.join("samples")).unwrap();
    fs::create_dir_all(p1.join("solutions")).unwrap();
    fs::write(
        p1.join("problem.toml"),
        r#"title = "Alpha Sum"
rating = 1200
tags = ["greedy", "math"]
author = "Alice"

[limits]
time = "1s"
memory = "256MB"

[[source]]
platform = "codeforces"
id = "1999A"
url = "https://codeforces.com/problemset/problem/1999/A"
"#,
    )
    .unwrap();
    fs::write(
        p1.join("statement.md"),
        "SECRET_BODY_TOKEN_ALPHA 给定两个整数 $a, b$。\n\n## 输入格式\n\n一行两个整数。\n\n## 输出格式\n\n输出它们的和。\n",
    )
    .unwrap();
    fs::write(p1.join("editorial.md"), "## 做法\n\n直接输出 $a + b$。\n").unwrap();
    fs::write(p1.join("samples/01.in"), "1 2\n").unwrap();
    fs::write(p1.join("samples/01.ans"), "3\n").unwrap();
    fs::write(p1.join("solutions/std.cpp"), "int main() { return 0; }\n").unwrap();

    // Problem 2: beta (higher rating, atcoder source, no editorial, no solutions)
    let p2 = dir.path().join("problems/beta");
    fs::create_dir_all(p2.join("samples")).unwrap();
    fs::write(
        p2.join("problem.toml"),
        r#"title = "Beta Tree"
rating = 2100
tags = ["dp", "trees"]

[limits]
time = "2s"
memory = "512MB"

[[source]]
platform = "atcoder"
id = "abc300_g"
"#,
    )
    .unwrap();
    fs::write(
        p2.join("statement.md"),
        "给定一棵树。\n\n## 输入格式\n\n树边。\n\n## 输出格式\n\n答案。\n",
    )
    .unwrap();
    fs::write(p2.join("samples/01.in"), "2\n1 2\n").unwrap();
    fs::write(p2.join("samples/01.ans"), "1\n").unwrap();

    dir
}

#[test]
fn tools_list_has_all_ten_tools_and_no_unfreeze() {
    let dir = setup_workspace();
    let mut client = McpClient::spawn(dir.path(), false);

    let init = client.request("initialize", json!({ "protocolVersion": "2025-11-25" }));
    assert_eq!(init["result"]["protocolVersion"], "2025-11-25");

    let list = client.request("tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 13);

    let mut read_only_map = std::collections::BTreeMap::new();
    for tool in tools {
        let name = tool["name"].as_str().unwrap().to_owned();
        let ro = tool["annotations"]["readOnlyHint"].as_bool().unwrap();
        assert!(
            !name.to_lowercase().contains("unfreeze"),
            "MCP must never expose unfreeze tool: {name}"
        );
        read_only_map.insert(name, ro);
    }

    for read_tool in [
        "compile_and_diagnose",
        "render_page_png",
        "list_templates",
        "search_problems",
        "get_problem",
        "get_contest",
    ] {
        assert_eq!(
            read_only_map.get(read_tool),
            Some(&true),
            "{read_tool} must be readOnlyHint=true"
        );
    }
    for write_tool in [
        "export_pdf",
        "add_template",
        "write_problem",
        "compose_contest",
        "set_override",
        "set_template_options",
        "freeze_contest",
    ] {
        assert_eq!(
            read_only_map.get(write_tool),
            Some(&false),
            "{write_tool} must be readOnlyHint=false"
        );
    }

    // Calling unfreeze_contest directly must fail as JSON-RPC unknown tool (-32602).
    let unknown = client.request(
        "tools/call",
        json!({ "name": "unfreeze_contest", "arguments": { "id": "c1" } }),
    );
    assert_eq!(unknown["error"]["code"], -32602);
}

#[test]
fn search_get_and_write_problem_with_hash_conflict_and_whitelist() {
    let dir = setup_workspace();
    let mut client = McpClient::spawn(dir.path(), false);

    // 1. search_problems by body text (verifying statement body is NOT leaked in metadata result).
    let res = client.call_tool(
        "search_problems",
        json!({ "query": "SECRET_BODY_TOKEN_ALPHA" }),
    );
    assert_ne!(res["isError"], true);
    let body_text = res["content"][0]["text"].as_str().unwrap();
    assert!(
        !body_text.contains("SECRET_BODY_TOKEN_ALPHA"),
        "search_problems must not return statement body"
    );
    let probs = res["structuredContent"]["problems"].as_array().unwrap();
    assert_eq!(probs.len(), 1);
    assert_eq!(probs[0]["id"], "alpha");
    assert_eq!(probs[0]["has_editorial"], true);
    assert_eq!(probs[0]["has_solutions"], true);

    // 2. search_problems by tags, rating range, platform, and `text` alias.
    let res_tag = client.call_tool(
        "search_problems",
        json!({ "tags": ["dp", "trees"], "rating_min": 2000, "rating_max": 2200, "platform": "atcoder" }),
    );
    let probs_tag = res_tag["structuredContent"]["problems"].as_array().unwrap();
    assert_eq!(probs_tag.len(), 1);
    assert_eq!(probs_tag[0]["id"], "beta");
    assert_eq!(probs_tag[0]["has_editorial"], false);
    assert_eq!(probs_tag[0]["has_solutions"], false);

    let res_alias = client.call_tool("search_problems", json!({ "text": "Beta" }));
    assert_eq!(
        res_alias["structuredContent"]["problems"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // 3. get_problem returns metadata, statement/editorial text + SHA-256, samples, solutions.
    let gp = client.call_tool("get_problem", json!({ "id": "alpha" }));
    assert_ne!(gp["isError"], true);
    let sc = &gp["structuredContent"];
    assert_eq!(sc["meta"]["title"], "Alpha Sum");
    assert_eq!(sc["solutions"], json!(["std.cpp"]));
    assert_eq!(sc["samples"].as_array().unwrap().len(), 1);
    let stmt_text = sc["statements"][0]["text"].as_str().unwrap();
    let stmt_hash = sc["statements"][0]["hash"].as_str().unwrap();
    assert_eq!(stmt_hash, content_hash(stmt_text.as_bytes()));
    let ed_text = sc["editorial"]["text"].as_str().unwrap();
    let ed_hash = sc["editorial"]["hash"].as_str().unwrap();
    assert_eq!(ed_hash, content_hash(ed_text.as_bytes()));

    // 4. write_problem: create a new problem with samples & solution.
    let created = client.call_tool(
        "write_problem",
        json!({
            "id": "gamma",
            "files": {
                "problem.toml": "title = \"Gamma\"\nrating = 1600\ntags = [\"graph\"]\n",
                "statement.md": "图论题。\n\n## 输入格式\n\n$n, m$。\n\n## 输出格式\n\n答案。\n",
                "samples/01.in": "3 2\n",
                "samples/01.ans": "2\n",
                "solutions/main.cpp": "int main() {}\n"
            }
        }),
    );
    assert_ne!(created["isError"], true);
    let gamma_stmt_hash = created["structuredContent"]["hashes"]["statement.md"]
        .as_str()
        .unwrap()
        .to_owned();

    // 5. write_problem: update with matching expected_hash succeeds.
    let updated = client.call_tool(
        "write_problem",
        json!({
            "id": "gamma",
            "expected_hash": gamma_stmt_hash,
            "files": {
                "statement.md": "更新后的图论题。\n\n## 输入格式\n\n$n, m$。\n\n## 输出格式\n\n答案。\n"
            }
        }),
    );
    assert_ne!(updated["isError"], true);

    // 6. write_problem: stale expected_hash fails with conflict and does not overwrite disk.
    let conflict = client.call_tool(
        "write_problem",
        json!({
            "id": "gamma",
            "expected_hash": gamma_stmt_hash,
            "files": {
                "statement.md": "SHOULD NOT BE WRITTEN"
            }
        }),
    );
    assert_eq!(err_code(&conflict), "conflict");
    let on_disk = fs::read_to_string(dir.path().join("problems/gamma/statement.md")).unwrap();
    assert!(on_disk.contains("更新后的图论题"));

    // 7. write_problem: whitelist & path traversal blocked.
    for bad_path in [
        "../alpha/statement.md",
        "/etc/passwd",
        "assets/image.png",
        "samples/nested/01.in",
        "notes.txt",
    ] {
        let rejected = client.call_tool(
            "write_problem",
            json!({
                "id": "gamma",
                "files": { bad_path: "nope" }
            }),
        );
        assert_eq!(
            err_code(&rejected),
            "disallowed_path",
            "expected disallowed_path for {bad_path}"
        );
    }

    // 8. write_problem: invalid problem.toml rejected without corrupting file.
    let bad_toml = client.call_tool(
        "write_problem",
        json!({
            "id": "gamma",
            "files": { "problem.toml": "this is = [not valid toml" }
        }),
    );
    assert_eq!(err_code(&bad_toml), "invalid_file");

    // 9. write_problem: invalid ID & creating new problem without problem.toml rejected.
    let bad_id = client.call_tool(
        "write_problem",
        json!({
            "id": "../escape",
            "files": { "problem.toml": "title = \"Evil\"\n" }
        }),
    );
    assert_eq!(err_code(&bad_id), "invalid_id");

    let missing_toml = client.call_tool(
        "write_problem",
        json!({
            "id": "delta",
            "files": { "statement.md": "no meta" }
        }),
    );
    assert_eq!(err_code(&missing_toml), "invalid_file");

    // 10. Bilingual statements in get_problem both carry exact SHA-256 hashes.
    let bi = client.call_tool(
        "write_problem",
        json!({
            "id": "bilingual",
            "files": {
                "problem.toml": "title = \"Bi\"\n",
                "statement.zh.md": "中文。\n\n## 输入格式\n\nx\n\n## 输出格式\n\ny\n",
                "statement.en.md": "English.\n\n## Input\n\nx\n\n## Output\n\ny\n"
            }
        }),
    );
    assert_ne!(bi["isError"], true);
    let gp_bi = client.call_tool("get_problem", json!({ "id": "bilingual" }));
    assert_ne!(gp_bi["isError"], true);
    let stmts = gp_bi["structuredContent"]["statements"].as_array().unwrap();
    assert_eq!(stmts.len(), 2);
    for st in stmts {
        let text = st["text"].as_str().unwrap();
        assert_eq!(st["hash"], content_hash(text.as_bytes()));
    }
}

#[test]
fn contest_compose_override_pages_lint_freeze_and_frozen_guard() {
    let dir = setup_workspace();
    let mut client = McpClient::spawn(dir.path(), false);

    // 1. get_contest without id lists empty contests initially.
    let empty_list = client.call_tool("get_contest", json!({}));
    assert_ne!(empty_list["isError"], true);
    assert_eq!(
        empty_list["structuredContent"]["contests"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // 2. compose_contest creates a contest with metadata and A, B labels.
    let composed = client.call_tool(
        "compose_contest",
        json!({
            "id": "regional-2026",
            "problems": ["alpha", "beta"],
            "name": "Regional 2026",
            "location": "Shanghai",
            "date": "2026-11-20",
            "author": "Setters",
            "language": "chinese",
            "cover": true,
            "blank-pages": false,
            "show-metadata": true
        }),
    );
    assert_ne!(composed["isError"], true);
    assert_eq!(
        composed["structuredContent"]["meta"]["problem"][0]["label"],
        "A"
    );
    assert_eq!(
        composed["structuredContent"]["meta"]["problem"][1]["label"],
        "B"
    );

    // 3. compose_contest refuses > 26 problems.
    let too_many: Vec<String> = (0..27).map(|i| format!("p{i}")).collect();
    let over_limit = client.call_tool(
        "compose_contest",
        json!({ "id": "regional-2026", "problems": too_many }),
    );
    assert_eq!(err_code(&over_limit), "too_many_problems");

    // 4. set_override sets title, limits, show-metadata, then clears / auto-cleans on revert.
    let ov1 = client.call_tool(
        "set_override",
        json!({
            "contest": "regional-2026",
            "label": "B",
            "fields": {
                "title": "Beta Custom",
                "show-metadata": false,
                "limits": { "time": "3s", "memory": "1024MB" }
            }
        }),
    );
    assert_ne!(ov1["isError"], true);
    assert_eq!(ov1["structuredContent"]["item"]["title"], "Beta Custom");
    assert_eq!(ov1["structuredContent"]["item"]["limits"]["time"], "3s");
    assert_eq!(ov1["structuredContent"]["item"]["show-metadata"], false);

    // Reverting time back to library value ("2s") and clearing memory via null removes limits override.
    let ov2 = client.call_tool(
        "set_override",
        json!({
            "contest": "regional-2026",
            "label": "B",
            "fields": {
                "limits": { "time": "2s", "memory": null }
            }
        }),
    );
    assert_ne!(ov2["isError"], true);
    assert!(ov2["structuredContent"]["item"].get("limits").is_none());

    // Reject non-metadata override keys (e.g. statement) and support `limits: null`.
    let bad_ov = client.call_tool(
        "set_override",
        json!({
            "contest": "regional-2026",
            "label": "B",
            "fields": { "statement": "cannot override body" }
        }),
    );
    assert_eq!(err_code(&bad_ov), "invalid_arguments");

    let set_lim = client.call_tool(
        "set_override",
        json!({
            "contest": "regional-2026",
            "label": "B",
            "fields": { "limits": { "input": "b.in", "output": "b.out" } }
        }),
    );
    assert_ne!(set_lim["isError"], true);
    assert_eq!(
        set_lim["structuredContent"]["item"]["limits"]["input"],
        "b.in"
    );

    let clear_all_lim = client.call_tool(
        "set_override",
        json!({
            "contest": "regional-2026",
            "label": "B",
            "fields": { "limits": null }
        }),
    );
    assert_ne!(clear_all_lim["isError"], true);
    assert!(
        clear_all_lim["structuredContent"]["item"]
            .get("limits")
            .is_none()
    );

    // Reordering keeps B's remaining overrides when it moves to A!
    let reordered = client.call_tool(
        "compose_contest",
        json!({
            "id": "regional-2026",
            "problems": ["beta", "alpha"]
        }),
    );
    assert_ne!(reordered["isError"], true);
    assert_eq!(
        reordered["structuredContent"]["meta"]["problem"][0]["id"],
        "beta"
    );
    assert_eq!(
        reordered["structuredContent"]["meta"]["problem"][0]["label"],
        "A"
    );
    assert_eq!(
        reordered["structuredContent"]["meta"]["problem"][0]["title"],
        "Beta Custom"
    );

    // 5. get_contest with id returns real compiled page ranges, overrides, and stats.
    let gc = client.call_tool("get_contest", json!({ "id": "regional-2026" }));
    assert_ne!(gc["isError"], true);
    let detail = &gc["structuredContent"];
    assert_eq!(detail["frozen"], false);
    assert!(detail["pages"].as_u64().unwrap() >= 3); // cover + 2 problems
    let items = detail["problems"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["label"], "A");
    assert_eq!(items[0]["title"], "Beta Custom");
    assert_eq!(items[0]["overrides"]["title"], "Beta Custom");
    assert_eq!(items[0]["first"], 2);
    assert!(items[0]["count"].as_u64().unwrap() >= 1);
    assert!(items[1]["first"].as_u64().unwrap() > items[0]["first"].as_u64().unwrap());
    assert_eq!(detail["stats"]["problem_count"], 2);
    assert_eq!(detail["stats"]["rating_distribution"]["pupil"], 1);
    assert_eq!(detail["stats"]["rating_distribution"]["master"], 1);

    // 6. get_contest without id lists the contest.
    let listed = client.call_tool("get_contest", json!({}));
    assert_eq!(
        listed["structuredContent"]["contests"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // 7. Missing image blocks compile_and_diagnose, export_pdf, AND freeze_contest!
    let break_img = client.call_tool(
        "write_problem",
        json!({
            "id": "alpha",
            "files": {
                "statement.md": "看图 ![图](assets/fig.png)。\n\n## 输入格式\n\n输入。\n\n## 输出格式\n\n输出。\n"
            }
        }),
    );
    assert_ne!(break_img["isError"], true);

    let diag_err = client.call_tool(
        "compile_and_diagnose",
        json!({ "target": "contest", "id": "regional-2026" }),
    );
    assert_eq!(diag_err["isError"], true);
    let diags = diag_err["structuredContent"]["diagnostics"]
        .as_array()
        .unwrap();
    assert!(
        diags
            .iter()
            .any(|d| d["code"] == "missing_image" && d["severity"] == "error"),
        "expected missing_image error in diagnostics: {diags:?}"
    );

    let bad_export = client.call_tool(
        "export_pdf",
        json!({ "target": "contest", "id": "regional-2026" }),
    );
    assert_eq!(bad_export["isError"], true);
    assert!(!dir.path().join("build/regional-2026.pdf").exists());

    let blocked_freeze = client.call_tool("freeze_contest", json!({ "id": "regional-2026" }));
    assert_eq!(blocked_freeze["isError"], true);
    assert_eq!(blocked_freeze["structuredContent"]["frozen"], false);
    assert!(!dir.path().join("contests/regional-2026/snapshot").exists());

    // 8. Fix the missing image -> freeze_contest succeeds even though beta lacks editorial (warning only)!
    fs::create_dir_all(dir.path().join("problems/alpha/assets")).unwrap();
    fs::write(dir.path().join("problems/alpha/assets/fig.png"), TINY_PNG).unwrap();

    let frozen = client.call_tool("freeze_contest", json!({ "id": "regional-2026" }));
    assert_ne!(frozen["isError"], true, "freeze failed: {frozen}");
    assert_eq!(frozen["structuredContent"]["frozen"], true);
    let outputs = frozen["structuredContent"]["outputs"].as_array().unwrap();
    assert_eq!(outputs.len(), 2); // statements PDF + editorial PDF (alpha has editorial)
    assert!(
        dir.path()
            .join("contests/regional-2026/snapshot/regional-2026.pdf")
            .is_file()
    );
    assert!(
        dir.path()
            .join("contests/regional-2026/snapshot/regional-2026-editorial.pdf")
            .is_file()
    );
    // Beta has no editorial, so a non-blocking warning should be reported.
    let freeze_diags = frozen["structuredContent"]["diagnostics"]
        .as_array()
        .unwrap();
    assert!(
        freeze_diags
            .iter()
            .any(|d| d["code"] == "missing_editorial" && d["severity"] == "warning"),
        "expected missing_editorial warning: {freeze_diags:?}"
    );

    // 9. Frozen contest refuses all mutations (compose, override, export into contest dir, re-freeze).
    let frozen_compose = client.call_tool(
        "compose_contest",
        json!({ "id": "regional-2026", "problems": ["alpha"] }),
    );
    assert_eq!(err_code(&frozen_compose), "contest_frozen");

    let frozen_override = client.call_tool(
        "set_override",
        json!({ "contest": "regional-2026", "label": "A", "fields": { "title": "Nope" } }),
    );
    assert_eq!(err_code(&frozen_override), "contest_frozen");

    let frozen_export = client.call_tool(
        "export_pdf",
        json!({
            "target": "contest",
            "id": "regional-2026",
            "output": "contests/regional-2026/snapshot/tamper.pdf"
        }),
    );
    assert_eq!(err_code(&frozen_export), "contest_frozen");

    let refreeze = client.call_tool("freeze_contest", json!({ "id": "regional-2026" }));
    assert_eq!(err_code(&refreeze), "already_frozen");

    // 10. Read tools still work on frozen contest (render_page_png + get_contest), and
    //     invalid page / path traversal on export_pdf are properly rejected.
    let png = client.call_tool(
        "render_page_png",
        json!({ "target": "contest", "id": "regional-2026", "page": 1 }),
    );
    assert_ne!(png["isError"], true);
    assert_eq!(png["content"][0]["mimeType"], "image/png");

    let bad_page = client.call_tool(
        "render_page_png",
        json!({ "target": "contest", "id": "regional-2026", "page": 999 }),
    );
    assert_eq!(err_code(&bad_page), "no_such_page");

    let escape_pdf = client.call_tool(
        "export_pdf",
        json!({ "target": "problem", "id": "alpha", "output": "../escape.pdf" }),
    );
    assert_eq!(err_code(&escape_pdf), "path_escape");

    let gc_frozen = client.call_tool("get_contest", json!({ "id": "regional-2026" }));
    assert_eq!(gc_frozen["structuredContent"]["frozen"], true);

    // 11. Editorial check reports non-blocking missing_editorial warning (`isError: false`).
    let ed_diag = client.call_tool(
        "compile_and_diagnose",
        json!({ "target": "problem", "id": "beta", "book": "editorial" }),
    );
    assert_ne!(ed_diag["isError"], true);
    assert!(
        ed_diag["structuredContent"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "missing_editorial" && d["severity"] == "warning")
    );
}

#[test]
fn read_only_mode_allows_all_read_tools_and_blocks_all_five_write_tools() {
    let dir = setup_workspace();
    // Create a contest before launching read-only MCP.
    {
        let ws = xcpc_storage::Workspace::open(dir.path(), false).unwrap();
        ws.compose_contest("c1", &["alpha".into()], None).unwrap();
    }

    let mut client = McpClient::spawn(dir.path(), true);

    // All 5 read tools succeed.
    assert_ne!(
        client.call_tool("search_problems", json!({}))["isError"],
        true
    );
    assert_ne!(
        client.call_tool("get_problem", json!({ "id": "alpha" }))["isError"],
        true
    );
    assert_ne!(
        client.call_tool("get_contest", json!({ "id": "c1" }))["isError"],
        true
    );
    assert_ne!(
        client.call_tool(
            "compile_and_diagnose",
            json!({ "target": "problem", "id": "alpha" })
        )["isError"],
        true
    );
    assert_ne!(
        client.call_tool(
            "render_page_png",
            json!({ "target": "problem", "id": "alpha", "page": 1 })
        )["isError"],
        true
    );

    // All 5 write tools are refused with code "read_only".
    let w1 = client.call_tool("export_pdf", json!({ "target": "problem", "id": "alpha" }));
    assert_eq!(err_code(&w1), "read_only");

    let w2 = client.call_tool(
        "write_problem",
        json!({ "id": "alpha", "files": { "statement.md": "hack" } }),
    );
    assert_eq!(err_code(&w2), "read_only");

    let w3 = client.call_tool(
        "compose_contest",
        json!({ "id": "c1", "problems": ["alpha", "beta"] }),
    );
    assert_eq!(err_code(&w3), "read_only");

    let w4 = client.call_tool(
        "set_override",
        json!({ "contest": "c1", "label": "A", "fields": { "title": "Hack" } }),
    );
    assert_eq!(err_code(&w4), "read_only");

    let w5 = client.call_tool("freeze_contest", json!({ "id": "c1" }));
    assert_eq!(err_code(&w5), "read_only");

    let w6 = client.call_tool("add_template", json!({ "preset": "minimal" }));
    assert_eq!(err_code(&w6), "read_only");
}

#[test]
fn template_tools_list_presets_install_and_set_local_options() {
    let dir = setup_workspace();
    let mut client = McpClient::spawn(dir.path(), false);
    client.request("initialize", json!({ "protocolVersion": "2025-11-25" }));

    let listed = client.call_tool("list_templates", json!({}));
    assert_ne!(listed["isError"], true);
    assert_eq!(
        listed["structuredContent"]["presets"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    let added = client.call_tool(
        "add_template",
        json!({ "preset": "minimal", "name": "custom-min" }),
    );
    assert_ne!(added["isError"], true);
    assert_eq!(
        added["structuredContent"]["path"],
        "templates/custom-min.typ"
    );
    assert!(dir.path().join("templates/custom-min.typ").is_file());

    // Duplicate add_template returns conflict.
    let dup = client.call_tool(
        "add_template",
        json!({ "preset": "minimal", "name": "custom-min" }),
    );
    assert_eq!(err_code(&dup), "conflict");

    // set_template_options supports writing to `"local"` namespace on a contest.
    let _ = client.call_tool(
        "compose_contest",
        json!({ "id": "c-local", "problems": ["alpha"], "name": "Local Round" }),
    );
    let set_local = client.call_tool(
        "set_template_options",
        json!({
            "scope": "contest",
            "id": "c-local",
            "options": { "local": { "cover": "on" } }
        }),
    );
    assert_ne!(set_local["isError"], true);
    assert_eq!(
        set_local["structuredContent"]["meta"]["template-options"]["local"]["cover"],
        "on"
    );
}
