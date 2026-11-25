//! Integration tests for Phase 5 CLI (`snippet`, `bundle`, `trd`) and MCP tools.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};
use xcpc_storage::init_example_workspace;

fn xcpc_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xcpc"))
}

fn run_json(ws: &Path, args: &[&str]) -> (i32, Value) {
    let out = xcpc_bin()
        .arg("-w")
        .arg(ws)
        .arg("--json")
        .args(args)
        .output()
        .expect("run xcpc");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let val: Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!("invalid JSON (exit {code}): {e}\nstdout:\n{stdout}\nstderr:\n{stderr}")
    });
    (code, val)
}

struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl McpClient {
    fn spawn(ws: &Path, read_only: bool) -> Self {
        let mut cmd = xcpc_bin();
        cmd.arg("-w").arg(ws);
        if read_only {
            cmd.arg("--read-only");
        }
        cmd.arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().expect("spawn xcpc mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut client = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        let init = client.request(
            "initialize",
            json!({ "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }),
        );
        assert!(init.get("result").is_some(), "init failed: {init}");
        client
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).expect("valid jsonrpc reply")
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Value {
        self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn cli_snippet_bundle_and_trd_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    init_example_workspace(dir.path()).unwrap();
    let ws = dir.path();

    // 1. snippet list & show
    let (code, list) = run_json(ws, &["snippet", "list"]);
    assert_eq!(code, 0);
    let slugs: Vec<&str> = list["snippets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["slug"].as_str().unwrap())
        .collect();
    assert!(slugs.contains(&"modint"));
    assert!(slugs.contains(&"ntt"));
    assert!(slugs.contains(&"number-theory-facts"));

    let (code, show) = run_json(ws, &["snippet", "show", "ntt"]);
    assert_eq!(code, 0);
    assert_eq!(show["snippet"]["slug"], "ntt");
    assert_eq!(show["snippet"]["reverse_refs"]["deps"], json!(["modint"]));

    // 2. snippet new & delete
    let (code, created) = run_json(
        ws,
        &[
            "snippet",
            "new",
            "my-card",
            "--title",
            "My Card",
            "--tag",
            "math",
            "--formula-only",
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(created["snippet"]["slug"], "my-card");
    assert!(created["snippet"]["code"].is_null());

    let (code, del) = run_json(ws, &["snippet", "delete", "my-card", "--yes"]);
    assert_eq!(code, 0);
    assert_eq!(del["deleted"], "my-card");

    // 3. snippet verify (self-test on modint)
    let (code, ver) = run_json(ws, &["snippet", "verify", "modint"]);
    assert_eq!(code, 0, "verify modint failed: {ver}");
    assert_eq!(ver["reports"][0]["status"], "passed");

    // 4. bundle --snippet ntt (with --compact and -o)
    let out_cpp = dir.path().join("bundled_ntt.cpp");
    let (code, bundled) = run_json(
        ws,
        &[
            "bundle",
            "--snippet",
            "ntt",
            "--compact",
            "-o",
            out_cpp.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "bundle --snippet failed: {bundled}");
    assert!(out_cpp.is_file());
    let bundled_code = bundled["code"].as_str().unwrap();
    assert!(bundled_code.contains("struct ModInt"));
    assert!(bundled_code.contains("void ntt("));

    // 5. bundle local file outside workspace with -I local header dir
    let ext_dir = tempfile::tempdir().unwrap();
    let inc_dir = ext_dir.path().join("myinc");
    fs::create_dir_all(&inc_dir).unwrap();
    fs::write(
        inc_dir.join("helper.hpp"),
        "#pragma once\nint helper_fn() { return 7; }\n",
    )
    .unwrap();
    let main_cpp = ext_dir.path().join("sol.cpp");
    fs::write(
        &main_cpp,
        "#include \"helper.hpp\"\n#include \"modint/code.hpp\"\nint main() { return helper_fn(); }\n",
    )
    .unwrap();
    let (code, local_bundle) = run_json(
        ws,
        &[
            "bundle",
            main_cpp.to_str().unwrap(),
            "-I",
            inc_dir.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "bundle local file failed: {local_bundle}");
    let local_code = local_bundle["code"].as_str().unwrap();
    assert!(local_code.contains("int helper_fn()"));
    assert!(local_code.contains("struct ModInt"));

    // 6. trd compose, check, build (--emit-typst), render
    let (code, composed) = run_json(
        ws,
        &[
            "trd",
            "compose",
            "my-trd",
            "--name",
            "Test TRD",
            "--template",
            "trd-compact",
            "--chapter",
            "Math:modint,number-theory-facts",
        ],
    );
    assert_eq!(code, 0, "trd compose failed: {composed}");

    let (code, checked) = run_json(ws, &["trd", "check", "my-trd"]);
    assert_eq!(code, 0, "trd check failed: {checked}");
    assert_eq!(checked["ok"], true);

    let emit_typ = dir.path().join("out.typ");
    let out_pdf = dir.path().join("out.pdf");
    let (code, built) = run_json(
        ws,
        &[
            "trd",
            "build",
            "my-trd",
            "-o",
            out_pdf.to_str().unwrap(),
            "--emit-typst",
            emit_typ.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "trd build failed: {built}");
    assert!(out_pdf.is_file());
    assert!(emit_typ.is_file());

    let render_dir = dir.path().join("rendered_trd");
    let (code, rendered) = run_json(
        ws,
        &[
            "trd",
            "render",
            "my-trd",
            "--page",
            "1",
            "-o",
            render_dir.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "trd render failed: {rendered}");
    assert!(render_dir.join("page-1.png").is_file());

    // 7. trd freeze --strict fails on unverified `dsu`, passes on `my-trd` (modint passed + formula card)
    let (code_strict_fail, _) = run_json(ws, &["trd", "freeze", "icpc-regional", "--strict"]);
    assert_ne!(
        code_strict_fail, 0,
        "strict freeze must block unverified snippets"
    );

    let (code_strict_ok, frozen) = run_json(ws, &["trd", "freeze", "my-trd", "--strict"]);
    assert_eq!(
        code_strict_ok, 0,
        "strict freeze on verified my-trd failed: {frozen}"
    );
    assert_eq!(frozen["frozen"], true);

    // 8. trd unfreeze --yes
    let (code_unfreeze, unfrozen) = run_json(ws, &["trd", "unfreeze", "my-trd", "--yes"]);
    assert_eq!(code_unfreeze, 0, "trd unfreeze failed: {unfrozen}");
    assert_eq!(unfrozen["trd"]["meta"]["frozen"], false);
}

#[test]
fn mcp_phase5_tools_sandbox_read_only_and_no_unfreeze() {
    let dir = tempfile::tempdir().unwrap();
    init_example_workspace(dir.path()).unwrap();

    // 1. Read-write MCP client
    let mut rw = McpClient::spawn(dir.path(), false);

    // Verify no unfreeze tool is exposed
    let listed = rw.request("tools/list", json!({}));
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(!names.iter().any(|n| n.contains("unfreeze")));
    let unfreeze_call = rw.call_tool("unfreeze_trd", json!({ "id": "icpc-regional" }));
    assert_eq!(unfreeze_call["error"]["code"], -32602);

    // search_snippets & get_snippet
    let searched = rw.call_tool(
        "search_snippets",
        json!({ "query": "ntt", "tags": ["ntt"] }),
    );
    assert_ne!(searched["result"]["isError"], true);
    assert_eq!(searched["result"]["structuredContent"]["total"], 1);
    assert_eq!(
        searched["result"]["structuredContent"]["snippets"][0]["slug"],
        "ntt"
    );

    let got = rw.call_tool("get_snippet", json!({ "slug": "ntt" }));
    assert_ne!(got["result"]["isError"], true);
    assert_eq!(
        got["result"]["structuredContent"]["snippet"]["reverse_refs"]["deps"],
        json!(["modint"])
    );

    // bundle_code: slug succeeds, host path outside workspace MUST fail!
    let bundled = rw.call_tool("bundle_code", json!({ "slug": "ntt", "compact": true }));
    assert_ne!(bundled["result"]["isError"], true);
    assert!(
        bundled["result"]["structuredContent"]["result"]["code"]
            .as_str()
            .unwrap()
            .contains("struct ModInt")
    );

    let escaped = rw.call_tool("bundle_code", json!({ "path": "/etc/passwd" }));
    assert_eq!(escaped["result"]["isError"], true);
    let escaped_rel = rw.call_tool("bundle_code", json!({ "path": "../outside.cpp" }));
    assert_eq!(escaped_rel["result"]["isError"], true);

    // verify_snippet on modint
    let verified = rw.call_tool("verify_snippet", json!({ "slug": "modint" }));
    assert_ne!(verified["result"]["isError"], true);
    assert_eq!(
        verified["result"]["structuredContent"]["reports"][0]["status"],
        "passed"
    );

    // write_snippet + compose_trd + compile_and_diagnose + render_page_png + export_pdf + freeze_trd
    let written = rw.call_tool(
        "write_snippet",
        json!({
            "slug": "gcd-note",
            "title": "GCD Note",
            "tags": ["math"],
            "files": { "notes.md": "$\\gcd(a, b) = \\gcd(b, a \\bmod b)$\n" }
        }),
    );
    assert_ne!(
        written["result"]["isError"], true,
        "write_snippet failed: {written}"
    );

    let composed = rw.call_tool(
        "compose_trd",
        json!({
            "id": "mcp-trd",
            "name": "MCP TRD",
            "template": "trd-book",
            "chapters": [{ "title": "Math", "items": ["modint", "gcd-note"] }]
        }),
    );
    assert_ne!(
        composed["result"]["isError"], true,
        "compose_trd failed: {composed}"
    );

    let diag = rw.call_tool(
        "compile_and_diagnose",
        json!({ "target": "trd", "id": "mcp-trd" }),
    );
    assert_ne!(
        diag["result"]["isError"], true,
        "compile_and_diagnose trd failed: {diag}"
    );
    assert_eq!(diag["result"]["structuredContent"]["ok"], true);

    let rendered = rw.call_tool(
        "render_page_png",
        json!({ "target": "trd", "id": "mcp-trd", "page": 1 }),
    );
    assert_ne!(
        rendered["result"]["isError"], true,
        "render_page_png trd failed: {rendered}"
    );
    assert_eq!(rendered["result"]["content"][0]["mimeType"], "image/png");

    let exported = rw.call_tool("export_pdf", json!({ "target": "trd", "id": "mcp-trd" }));
    assert_ne!(
        exported["result"]["isError"], true,
        "export_pdf trd failed: {exported}"
    );
    assert!(dir.path().join("build/MCP TRD.pdf").is_file());

    let frozen = rw.call_tool("freeze_trd", json!({ "id": "mcp-trd", "strict": true }));
    assert_ne!(
        frozen["result"]["isError"], true,
        "freeze_trd failed: {frozen}"
    );
    assert_eq!(frozen["result"]["structuredContent"]["frozen"], true);

    // 2. Read-only MCP client must refuse write_snippet, compose_trd, freeze_trd, export_pdf(trd)
    let mut ro = McpClient::spawn(dir.path(), true);
    assert_eq!(
        ro.call_tool("write_snippet", json!({ "slug": "ro-test", "title": "RO" }))["result"]["isError"],
        true
    );
    assert_eq!(
        ro.call_tool("compose_trd", json!({ "id": "ro-trd", "chapters": [] }))["result"]["isError"],
        true
    );
    assert_eq!(
        ro.call_tool("freeze_trd", json!({ "id": "icpc-regional" }))["result"]["isError"],
        true
    );
    assert_eq!(
        ro.call_tool(
            "export_pdf",
            json!({ "target": "trd", "id": "icpc-regional" })
        )["result"]["isError"],
        true
    );
}
