---
title: Security Boundaries & Trust Gate
---

# Security Boundaries & Trust Gate

All plugin security boundaries in XCPC Workstation are enforced in the Rust Core (`xcpc-plugin`) and the desktop host layer. Plugin code cannot bypass these controls at runtime.

---

## 1. Core-Enforced Trust Gate (`pending_trust`)

Any plugin installed from an external source (local directory, `.zip` archive, subscribed repository, CLI, or `xcpc plugin link` development link) enters `pending_trust = true` if its `manifest.toml` declares any of the following sensitive permissions:

- Non-empty `[permissions].domains` (outbound HTTP domain allowlist; wildcard `"*"` is **forbidden**)
- `[permissions].workspace_read = true` (read-only projections for problems, contests, Algorithm Snippets, and Team Reference Documents (TRDs) without exposing host paths)
- `[permissions].workspace_write = true` (scoped domain writes: only `createProblem`, `updateProblem`, `setSamples`, and `upsertSnippet`; never permits deleting files or unfreezing contests, and strictly enforces `--read-only`, atomic writes, and frozen protection)
- Non-empty `[[credentials]]` (access to OS `keyring`-managed credentials)

Until explicitly approved by the user, Core rejects every invocation of the plugin with error code `PLUGIN_UNTRUSTED`. Trust can be granted in two ways:

1. Review and confirm the permission prompt shown automatically after installation (or in **Settings → Plugins**, or when the plugin is first invoked);
2. Run in the terminal:
   ```bash
   xcpc plugin trust <plugin-id>
   ```

### Automatic Reset on Permission Escalation or Cross-Repo Override

Core persists each installed plugin's origin repository (`source_repo`) and a snapshot of its approved permissions. During an upgrade or reinstallation, trust is automatically reset to `pending_trust = true` (requiring re-confirmation) if either of the following occurs:

- The new version expands its requested permissions (adds a domain, adds a credential, or newly enables `workspace_read` / `workspace_write`);
- The plugin is overwritten by a package from a different repository source.

---

## 2. QuickJS Sandbox Isolation

A plugin's `index.js` executes inside an embedded `rquickjs` sandbox in Core, isolated from the host filesystem and network stack:

- **Resource quotas**:
  - Hard memory cap: **64 MiB** (exceeding terminates execution and returns `MEMORY_LIMIT`);
  - Execution time cap: Core default interrupt limit of **30 seconds** (may be tightened per invocation context, such as headless tools or unit tests; exceeding terminates execution and returns `TIMEOUT`);
  - HTTP request constraints: `host.http.fetch` only permits domains listed in `[permissions].domains`, and **validates every redirect hop against the domain allowlist** (up to 5 redirects; authorization headers are stripped on cross-origin redirects). Default per-request timeout is 15 seconds (maximum 60 seconds), supporting `responseType: "text" | "base64"` (with a **5 MiB** binary payload limit for `"base64"`).
- **Disabled APIs**:
  - No Node.js built-in modules (`fs`, `path`, `http`, `https`, `child_process`, `net`, `os`, `process`, etc.);
  - No browser DOM or network globals (`window`, `document`, `localStorage`, `XMLHttpRequest`, etc.);
  - When bundling with `@xcpc-workstation/plugin-sdk/vite`, imports or references to these forbidden APIs fail at build time.
- **Scoped host capabilities**:
  - Credentials are stored in the OS `keyring` isolated by plugin ID; a plugin can only read keys within its own scope and cannot access or enumerate credentials belonging to other plugins;
  - Persistent state is stored via `host.kv`, strictly isolated per plugin;
  - Workspace access is gated by explicit permissions: `workspace_read = true` exposes read-only domain projections only, while `workspace_write = true` permits scoped creation/update of problems, samples, and Algorithm Snippets (never deleting files or unfreezing contests, and always honoring `--read-only`);
  - In-memory Typst / LaTeX math rendering (`renderTypst` / `renderMath`) runs in an isolated diskless world (64 KiB source limit, 2000 ms timeout, 4096×4096 pixel cap); in-memory ZIP helpers (`host.archive.*`) reject Zip Slip paths and symlinks while enforcing 20 MiB input, 50 MiB unpacked, and 1000-entry anti-ZIP-bomb limits.

---

## 3. Three-Layer Pure-Web `<WebView>` Isolation

Tool plugins declaring `ui_entry = "ui.html"` run inside a dedicated desktop `<WebView>`. The host enforces three defense-in-depth layers to prevent data exfiltration and unauthorized host calls:

### Layer 1: Native Capability Lockdown

The desktop host configuration [`apps/desktop/nsh.json`](https://github.com/lihaoze123/xcpc-workstation/blob/main/apps/desktop/nsh.json) explicitly declares:

```json
"capabilities": {
  "webview": {
    "hosts": []
  }
}
```

This blocks all direct network requests and external navigation at the WebKitGTK / WebView2 policy layer (allowing only the host-controlled local shell document `about:blank` / WebView2 `data:text/html`, with no `http:` or `https:` origin privileges).

### Layer 2: Controlled Outer Shell + Sandboxed Iframe + Strict CSP

The host dynamically generates a controlled outer HTML shell for each plugin view and injects a strict Content-Security-Policy at byte 0:

```text
default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; base-uri 'none';
```

The plugin's `ui.html` is mounted inside `<iframe sandbox="allow-scripts" srcdoc="...">` (**without** `allow-same-origin` and **without** `allow-top-navigation`), preventing external script/stylesheet loads, cookie or storage access, and CSP bypass via comments or prepended scripts.

### Layer 3: Per-Mount `bridgeToken` & Write-Only Clipboard

- On every mount, the outer shell generates a random `bridgeToken` (never exposed to the iframe) and only relays messages to the host when `ev.source === frame.contentWindow && ev.origin === "null"`;
- The host exposes a minimal controlled bridge on `window.xcpc` (`runTool`, `storage`, `workspace`, `oj.listAccounts`, `toast`, `clipboard.{writeText,writeImage}`, `files`, `openTool`, and `--xcpc-*` theme variables);
- Clipboard reads (`clipboard.readText` / `clipboard.readImage`) and external URL navigation (`openExternal`) are **explicitly omitted**; `oj.listAccounts()` returns only the platform, handle, and cached rating/rank of linked accounts—never credentials or submission details.

---

## 4. Repository Protocol, Integrity Verification & Path-Escape Protection

- **HTTPS enforcement with per-hop checks**: Plugin repository `index.json` endpoints and package download URLs must use **HTTPS** (loopback addresses `127.0.0.1`, `localhost`, and `::1` are exempted for local development). Every HTTP redirect hop is validated for HTTPS.
- **SHA-256 integrity verification**: When downloading a `.zip` package from a repository, Core verifies the archive against the SHA-256 checksum declared in the index and aborts installation on mismatch.
- **Zip Slip & archive symlink rejection**: When extracting plugin `.zip` archives, Core validates the normalized relative path of every entry, rejecting `..` traversal, absolute paths, and symlink entries (Unix mode `0o120000`).
- **Development link escape protection (`SYMLINK_ESCAPE`)**: When linking a local directory with `xcpc plugin link <dir>`, Core only permits the top-level plugin directory registered in `plugin_links.json`. Any symlink inside the plugin directory pointing outside the plugin root is rejected with `SYMLINK_ESCAPE`.
