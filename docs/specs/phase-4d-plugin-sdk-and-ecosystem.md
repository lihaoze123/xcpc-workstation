# Phase 4D 规格：Plugin SDK、纯 Web `<WebView>` 沙箱与官方收录仓库

2026-11-25 最终落地版，作为 Phase 4 的收尾子阶段，确立了**纯 Web `<WebView>` 沙箱插件界面（`ui_entry = "ui.html"`）**、**`@xcpc/plugin-sdk` + Vite 单文件 IIFE 工程化工具链**、**Core 强制信任门（`pending_trust`）与来源隔离（`source_repo`）**、**按插件粒度本地热重载**，并将 4 个官方离线工具插件迁出至独立官方收录仓库 `xcpc-plugins`。

---

## 1. 原则与架构边界

1. **双轨共存、产物单一**：
   - 宿主始终只认单文件产物：`manifest.toml` + `index.js`（可选，供 QuickJS 无头后端 / CLI / MCP 执行）+ `ui.html`（可选，供桌面端 `<WebView>` 沙箱渲染）；
   - **免构建轨道**：零 Node/npm 依赖，手写 `index.js` / `ui.html` 目录即可运行；
   - **Vite 工程化轨道**：使用 `@xcpc/plugin-sdk/vite` 将任意标准前端代码（原生 TS/CSS、Preact/React、Canvas2D、SVG 等）打包为单个自包含 IIFE `dist/ui.html`（并同步构建 `dist/index.js` 与拷贝 `dist/manifest.toml`）。
2. **纯 Web 化、只管能力不管样式**：
   - 宿主不维护自定义前端组件库，只通过 `window.xcpc` 提供受控宿主能力，并可选注入主题 CSS 变量（`--xcpc-*` + `data-theme="light|dark"`）；插件完全自主决定样式与交互框架。
   - 无自定义界面的表单工具（如 `complexity-mem`、`string-lab`）继续由桌面端原生 `HeadlessToolRunner` 渲染。
3. **主仓库零 Node 构建依赖**：
   - 主仓库仅内置 **2 个核心 OJ 插件**（`codeforces`、`atcoder`）；
   - 4 个官方离线工具（`graph-editor`、`math-bitwise`、`complexity-mem`、`string-lab`）托管于独立官方仓库 [`xcpc-plugins`](https://github.com/lihaoze123/xcpc-plugins)，桌面端默认预置订阅并在「工具」空态与仓库浏览器提供一键安装。
4. **纵深防御安全边界（Core + OS WebView + Sandboxed Iframe）**：
   - **Core 层强制 `pending_trust`**：声明了 `domains`、`credentials` 或 `workspace_read` 的外部插件（ZIP / 目录 / 仓库 / CLI / 开发链接）安装后一律默认待信任并拒绝运行（`PLUGIN_UNTRUSTED`），需经 GUI 确认弹窗或 `xcpc plugin trust <id>` 放行；记录 `source_repo`，跨仓库覆写或权限扩大自动重置信任。
   - **三层 WebView 隔离**：
     1. `apps/desktop/nsh.json` 声明 `capabilities.webview.hosts = []`，在原生 WebKitGTK / WebView2 策略层硬性拦截所有外部导航；
     2. 宿主生成受控外层 HTML 壳（字节 0 即生效严格 CSP：`default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; base-uri 'none';`），将插件 HTML 放入 `<iframe sandbox="allow-scripts" srcdoc>`（无 `allow-top-navigation`、无 `allow-same-origin`），从根源杜绝注释吞 `<head>` 或 `<head>` 前脚本绕过 CSP；
     3. 外层壳持有每次挂载随机生成的 `bridgeToken`（不暴露给 iframe），仅中继 `ev.source === frame.contentWindow && ev.origin === "null"` 的消息，宿主校验令牌并拦截一切外链跳转；不向未授权插件开放 `clipboard.readText` 与 `openExternal`。
   - **仓库与软链防护**：仓库源强制 HTTPS（仅豁免本地回环地址），`fetch_repo_catalog` 与 `get_bytes` 逐跳校验重定向 HTTPS；下载校验 SHA-256 与 Zip Slip；`xcpc plugin link` 仅放行 `plugin_links.json` 登记的顶层根，子路径严禁软链逃逸（`SYMLINK_ESCAPE`）。

### 明确不做（Phase 4D 范围外）
- 不在宿主维护自定义 JSX 组件库或编译期响应式 AST 编译器；
- 不做独立开发守护进程、WebSocket HMR 服务器或内置插件代码编辑器；
- 不做中心化账号/付费/评价系统式插件商店（沿用去中心化 `index.json` + SHA-256 仓库协议）。

---

## 2. 仓内单一 SDK 包：`packages/plugin-sdk`（`@xcpc/plugin-sdk` v0.2.0）

| 子路径导出 | 定位与内容 |
| :--- | :--- |
| `@xcpc/plugin-sdk/host` | `index.js`（Core QuickJS 沙箱）完整类型定义与协议契约（`HostApi`、`OjSourcePlugin`、`ToolPlugin`、`ToolOutputBlock`、`CommandPlugin`、`ImporterPlugin`、`ExporterPlugin` 等）。 |
| `@xcpc/plugin-sdk/ui` | `window.xcpc` WebView 能力桥接类型定义（`XcpcBridge`）与 `getXcpc()` 辅助函数。 |
| `@xcpc/plugin-sdk/vite` | 零配置 Vite 插件 `xcpcPlugin(options?)`：单文件 IIFE `dist/ui.html` 内联打包（`cssCodeSplit: false`、`assetsInlineLimit: Infinity`、`inlineDynamicImports: true`、脚本自动置于 `</body>` 前并转义 `</script`）、`src/index.ts` → `dist/index.js` 构建、QuickJS 禁用模块编译期护栏、`manifest.toml` 自动拷贝。 |
| `@xcpc/plugin-sdk/manifest.schema.json` | `manifest.toml` JSON Schema，供 Taplo / VS Code 补全与校验。 |

SDK tarball `xcpc-plugin-sdk.tgz` 随 `xcpc-plugins` 的 `official-latest` Release 原子发布，供 `xcpc plugin init --vite` 直接依赖安装。

---

## 3. `window.xcpc` 能力桥接（`apps/desktop/src/ui/tools/PluginWebView.tsx`）

- `xcpc.runTool(action, input)`：调用同插件 Core QuickJS `index.js` 工具逻辑；
- `xcpc.storage.{get,set,remove}`：读写插件独立作用域的 Core `KvStore`（`<dataDir>/plugin_kv/<id>.json`）；
- `xcpc.workspace.{getProblem,listProblems,getContest,listContests}`：由 Core 强制校验 `enabled && !pending_trust && workspace_read` 的只读工作区查询；
- `xcpc.toast(text, tone?)`、`xcpc.clipboard.writeText(text)`、`xcpc.files.{saveText,savePng,pickFile}`、`xcpc.openTool(pluginId, payload?)`；
- `xcpc.theme` / `xcpc.onThemeChange(cb)` / `xcpc.initialPayload`：实时同步浅色/深色主题与 `--xcpc-*` CSS 变量，无需重载 WebView；
- 内置 `error` / `unhandledrejection` 捕获与 `ERR_NSH_WEBVIEW_TERMINATED` 恢复界面（提供「查看详情」与「重新加载」）。

---

## 4. 本地热重载与 `xcpc plugin` CLI 工具链

- **按插件粒度热重载**：`PluginFileWatcher` 仅响应 `Create`、`Modify(Data|Name|Any)`、`Remove`（忽略 `Access`/`Metadata`，兼容 Linux 与 Windows），提取 `changedPluginIds` 仅重载发生变更的插件，安装或编辑无关插件不会重置当前打开的 `<PluginWebView>`。
- **CLI 命令**：
  - `xcpc plugin init <dir> [--kind tool|oj] [--vite]`（默认 `--kind tool`，`--vite` 自动输出后续构建与 link 指引）；
  - `xcpc plugin install <path-or-id>`（自动识别本地目录/ZIP 或已订阅仓库 ID）、`uninstall <id>`、`trust <id>`；
  - `xcpc plugin link <dir>` / `unlink <id>`；
  - `xcpc plugin check <dir> [--json]`（校验 manifest、路径沙箱、QuickJS `index.js` 导出、`.html` 非空 `ui_entry`，检测到 Vite 源码根目录自动提示先构建并检查 `dist/`）；
  - `xcpc plugin pack <dir> [-o <out.zip>] [--json]`（先 `check`，默认输出至父目录避免触发热重载，打印 SHA-256）；
  - `xcpc plugin repo list|add|remove|catalog [query]`；
  - `xcpc plugin test <dir> [action] [--args <json>] [--json]`。

---

## 5. 官方插件仓库（`xcpc-plugins`）与 CI 发布

- 默认官方源：`https://github.com/lihaoze123/xcpc-plugins/releases/download/official-latest/index.json`（自动迁移旧版 `raw.githubusercontent.com/.../main/index.json` 配置，支持 `XCPC_OFFICIAL_PLUGIN_REPO` 覆写与 GUI「恢复官方仓库」）。
- GitHub Actions（`.github/workflows/release.yml`）使用 `XCPC_SKIP_EMBED_FONTS=1` 构建 `xcpc` CLI，执行 `npm run typecheck`、`scripts/build-index.mjs`（构建官方 4 插件 + 拉取校验 `registry.toml` 社区条目）与 `npm pack` 生成 `xcpc-plugin-sdk.tgz`，最终通过 `gh release upload official-latest ... --clobber` 零停机原子更新。
