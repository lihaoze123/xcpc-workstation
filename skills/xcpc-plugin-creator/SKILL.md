---
name: xcpc-plugin-creator
description: 在 XCPC Workstation 中端到端设计、开发、测试、联调与打包插件（OJ 数据源、无头表单工具与纯 Web `<WebView>` 交互工具）的标准流程，涵盖安全沙箱红线、`manifest.toml` 规范、`@xcpc/plugin-sdk`（QuickJS `host` API / WebView `window.xcpc` / Vite 单文件构建）及 `xcpc plugin` 全生命周期命令。
---

# XCPC 插件开发编排 Skill（OJ 数据源 / 无头工具 / 纯 Web `<WebView>` 工具）

本 Skill 指导 Agent 为 **XCPC Workstation** 端到端编写、验证、联调与打包插件。插件采用**双运行时隔离架构**：业务逻辑运行在受限的 **QuickJS 沙箱**（`entry = "index.js"`），自定义交互界面运行在严格 CSP 与无同源权限的 **纯 Web `<WebView>`**（`ui_entry = "ui.html"`），两者通过宿主能力桥接通信。

---

## 1. 安全与工程红线（不可违反的纪律）

开发与排错时，必须定位根因并遵循平台安全边界，**严禁以下任何绕过或偷懒行为**：

1. **禁止在 WebView 内直接发起网络请求**：宿主对插件 HTML 强制注入严格 CSP（`default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; base-uri 'none';`）。所有外部 HTTP 请求**必须**通过 `window.xcpc.runTool(...)` 委托给 QuickJS 沙箱中的 `host.http.fetch(...)`，并在 `manifest.toml` 的 `[permissions].domains` 声明精确域名或 `*.example.com`（**严禁 `*` 通配符**）。
2. **禁止假设 `allow-same-origin` 或绕过桥接**：插件 HTML 运行在 `<iframe id="xcpc-plugin-frame" sandbox="allow-scripts">` 中，不含 `allow-same-origin` 与 `allow-top-navigation`，且外层 shell 使用单次挂载的 `bridgeToken` 鉴权转发。严禁尝试读取 `window.nsh`、`window.parent` DOM 或发起顶层跳转，一切宿主能力只能经由 `window.xcpc` 调用。
3. **必须将 `<meta charset="UTF-8">` 放在 `<head>` 第一位**：无论零构建手写 `ui.html` 还是 Vite 模板 `index.html`，首行元标签必须声明 UTF-8，避免中文文案或数学符号在不同平台 WebView 中乱码。
4. **严格遵守 `manifest.schema.json`**：Schema 开启 `additionalProperties: false`，禁止写入未定义字段；`id` 必须匹配 `^[a-z0-9][a-z0-9_-]{0,63}$`；`entry` 与 `ui_entry` 必须是插件目录内的相对路径，严禁 `..`、绝对路径或指向目录外的符号链接；`ui_entry` 扩展名**仅允许 `.html` / `.htm`**。
5. **保持 `ui.html` 单文件自包含，禁止外部 CDN**：禁止引用任何外部 `<script src="https://...">`、`<link rel="stylesheet" href="...">`、Google Fonts 或远程图片；所有 JS/CSS/图标必须内联（图片仅限 `data:` / `blob:`，字体仅限 `data:`）。
6. **必须适配宿主主题与动态切换**：CSS 必须消费宿主注入的 `--xcpc-*` 变量并提供浅/深色 fallback，JS 必须读取 `window.xcpc.theme` 并订阅 `window.xcpc.onThemeChange(...)` 同步 `document.documentElement.setAttribute("data-theme", scheme)`，严禁写死纯白/纯黑背景导致深浅模式割裂。
7. **必须使用 `window.xcpc.storage`（或 `host.kv`）持久化用户状态**：由于 iframe 无 `allow-same-origin`，直接访问 `localStorage` / `sessionStorage` 在真机 WebView 中会抛出 `SecurityError` 或丢失数据。持久化一律优先走 `window.xcpc.storage.get/set/remove`（本地浏览器单独预览时才可将 `localStorage` 置于 `try/catch` 降级分支）。
8. **严禁在 QuickJS 沙箱代码中混入 Node.js 或 DOM API**：`index.js` 运行在 QuickJS（默认 30s 超时、64 MiB 内存），不存在 `fs` / `http` / `path` / `child_process` 等 Node 内置模块，也不存在 `window` / `document` / `XMLHttpRequest` / `localStorage`。`@xcpc/plugin-sdk/vite` 默认开启 `sandboxGuard` 在构建期拦截，禁止为图省事关闭它。
9. **禁止跳过 `xcpc plugin check` 与 `xcpc plugin test` 直接交付**：交付或打包前必须保证静态校验、沙箱导出干跑与核心动作调用全部通过。

---

## 2. 插件架构与模式选型

在动手写代码前，先根据需求选定插件模式（可组合 `commands`、`importer`、`exporter`、`credentials`）：

| 模式 | 适用场景 | 必需能力与入口 | 典型代表 |
| :--- | :--- | :--- | :--- |
| **`oj_source`**（OJ 数据源） | 同步外部 OJ 的账号概况、提交记录、Rating 曲线、比赛日历与题面链接解析 | `capabilities = ["oj_source"]`；仅需 `entry = "index.js"`（无 `ui_entry`）；通常需声明 `[permissions].domains` 与可选 `[[credentials]]` | Codeforces / AtCoder / 洛谷等 OJ 适配器 |
| **无头表单 `tool`** | 纯输入→输出计算或查询，无需自定义画布/复杂状态 | `capabilities = ["tool"]`；仅需 `entry = "index.js"`（不设 `ui_entry`）；在 `manifest.toml` 声明 `[[tools]]` 与 `[[tools.inputs]]`，宿主自动生成表单并渲染 `ToolOutputBlock` | 简单编码转换、快速校验器 |
| **纯 Web `<WebView>` `tool`** | 需要交互画布、实时多面板联动、快捷键、拖拽或复杂本地状态 | `capabilities = ["tool", "commands"]`；同时提供 `entry = "index.js"` 与 `ui_entry = "ui.html"`；UI 负责交互体验，`index.js` 同时保留 `runTool`（供无头调用 / CLI / `window.xcpc.runTool` 复用）与 `runCommand`（`open_tool` 快捷入口） | `math-bitwise`（数论与位运算工作台）、`graph-editor`（图论与几何画板） |

### 能力与必需导出函数映射（`xcpc plugin check` 强校验）

| `capabilities` 值 | `index.js` 必需导出 | 可选导出 |
| :--- | :--- | :--- |
| `"oj_source"` | `normalizeRating` + (`syncSummary` 或 `syncAccount`) | `verify`, `syncSubmissions`, `syncRatingHistory`, `listContests`, `parseProblemUrl` |
| `"tool"` | `runTool(toolId, inputs, ctx)` | — |
| `"commands"` | `runCommand(commandId, ctx)` | — |
| `"importer"` | `importPackage(files, importerId)` | — |
| `"exporter"` | `exportPackage(ctx, exporterId)` | — |
| `"credentials"` | （无额外导出，配合 `[[credentials]]` 声明） | — |

---

## 3. SDK 契约与 `manifest.toml` 速查

### 3.1 `manifest.toml` 模板与约束

顶部推荐添加 Schema 提示，遵循最小权限原则（不需要网络或读工作区时保持 `domains = []`、`workspace_read = false`，避免触发不必要的信任拦截）：

```toml
# yaml-language-server: $schema=./node_modules/@xcpc/plugin-sdk/manifest.schema.json
id = "my-tool"
name = "我的算法工具"
version = "0.1.0"
description = "一句话说明用途"
author = "Your Name"
entry = "index.js"
ui_entry = "ui.html"          # 纯 Web 工具填 "ui.html"；无头工具或 OJ 插件省略此行
capabilities = ["tool", "commands"]

[permissions]
domains = []                  # 仅填必需域名，如 ["api.example.com", "*.example.org"]，禁止 "*"
workspace_read = false        # 仅在需要读取当前工作区题目/比赛时设为 true

[[commands]]
id = "open"
title = "打开我的算法工具"
context = "global"            # "global" | "problem" | "contest"

[[tools]]
id = "calc"
name = "计算"
description = "无头调用与表单回退入口"

[[tools.inputs]]
name = "expr"
label = "表达式"
kind = "string"               # "string" | "text" | "multiline" | "number" | "boolean" | "select"
default = "998244353"
required = true
```

### 3.2 QuickJS 沙箱 `host` API（`@xcpc/plugin-sdk/host`）

- `host.http.fetch({ url, method, headers, body, credentialHeader, timeoutMs })`：同步发起受域名白名单保护的 HTTP 请求；凭据优先用 `credentialHeader` 注入，避免明文拼装泄露。
- `host.credentials.get(key)`：读取用户在设置中保存的加密凭据。
- `host.html.select(html, cssSelector)`：基于 CSS 选择器解析 HTML，返回 `{ text, html, attrs }[]`。
- `host.kv.get<T>(key)` / `set(key, value)` / `delete(key)`：插件专属持久化键值存储。
- `host.workspace.listProblems()` / `getProblem(id)`：只读工作区题目（需 `workspace_read = true`）。
- `host.render.svgToPng(svg, scale)` / `svgToRgba(svg, scale)`：将 SVG 光栅化为 Base64 PNG 或 RGBA。
- `host.highlight(code, lang)`：词法高亮（支持 `cpp`/`rust`/`python`/`js`/`ts`/`json`/`toml` 等）。
- `host.log(level, msg)`：输出诊断日志（`"debug" | "info" | "warn" | "error"`）。
- `ToolOutputBlock` 支持 6 种块类型：`text`、`markdown`、`table`、`graph`、`svg`、`image`。

### 3.3 WebView `window.xcpc` API 与主题 Token（`@xcpc/plugin-sdk/ui`）

- `window.xcpc.runTool<T>(toolId, inputs)`：异步调用同插件 QuickJS 中的 `runTool`。
- `window.xcpc.storage.get<T>(key)` / `set(key, value)` / `remove(key)`：异步持久化插件状态。
- `window.xcpc.workspace.listProblems()` / `getProblem(id)` / `listContests()` / `getContest(id)`：异步只读工作区（需 `workspace_read = true`）。
- `window.xcpc.clipboard.writeText(text)`：写入系统剪贴板。
- `window.xcpc.files.saveText(defaultName, content)` / `savePng(defaultName, pngBase64)` / `pickFile({ title, extensions })`：调用宿主原生文件对话框。
- `window.xcpc.toast(message, kind?)`：触发宿主右下角单槽 Toast（`"info" | "success" | "error"`）。
- `window.xcpc.openTool(pluginId, toolId?, payload?)`：跨插件/同插件打开工具并传递 `initialPayload`。
- `window.xcpc.theme`（`{ scheme: "light" | "dark", tokens? }`）与 `window.xcpc.onThemeChange(cb)`（返回取消订阅函数）。
- **宿主注入的 CSS 变量**（在 `:root` 中提供 fallback）：
  `--xcpc-bg`、`--xcpc-fg`、`--xcpc-card`、`--xcpc-card-fg`、`--xcpc-muted`、`--xcpc-muted-fg`、`--xcpc-secondary-fg`、`--xcpc-border`、`--xcpc-input`、`--xcpc-primary`、`--xcpc-primary-fg`、`--xcpc-accent`、`--xcpc-destructive`、`--xcpc-success`、`--xcpc-warning`、`--xcpc-font-sans`、`--xcpc-font-mono`。

---

## 4. 端到端开发、测试与打包工作流

按以下 6 步推进，每步确认无误后再进入下一步：

### Step 1：初始化脚手架

优先根据复杂度选择 **Vite + TypeScript**（推荐用于中大型 WebView 工具或需要类型检查的插件）或 **零构建纯文件**（适合极简工具或简单 OJ 适配器）：

```bash
# 方式 A：Vite + TypeScript（推荐，自动配置 @xcpc/plugin-sdk/vite）
xcpc plugin init ./my-plugin --kind tool --vite
# 或 OJ 数据源：
xcpc plugin init ./my-oj --kind oj --vite

# 方式 B：零构建（直接产出 manifest.toml + index.js [+ ui.html]）
xcpc plugin init ./my-plugin --kind tool
```

Vite 模式生成标准结构：
`package.json`、`vite.config.ts`、`tsconfig.json`、`manifest.toml`、`index.html`、`src/index.ts`、`src/main.ts`、`src/style.css`。

### Step 2：实现 `manifest.toml` 与 QuickJS 逻辑（`src/index.ts` / `index.js`）

1. 填写精确的 `id`、`name`、`version`、`capabilities` 与最小化 `[permissions]`。
2. 在 `src/index.ts` 中使用 `import type { ... } from "@xcpc/plugin-sdk/host"` 编写纯逻辑与导出函数（如 `export function runTool(...)`、`export function runCommand(...)`）。
3. 若插件提供 `ui.html`，仍应保持 `runTool` 可独立工作（例如 `math-bitwise` 与 `graph-editor` 同时提供无头计算/渲染能力，便于 CLI/自动化测试和跨工具复用），并把 `open` 命令映射到 `{ type: "open_tool", plugin_id: "<id>" }`。

### Step 3：实现纯 Web UI（仅含 `ui_entry` 的插件）

1. **`index.html`**：首行 `<!doctype html>`，`<head>` 第一项为 `<meta charset="UTF-8" />`，仅含 `<div id="app"></div>` 与 `<script type="module" src="/src/main.ts"></script>`（零构建 `ui.html` 则把内联 `<script>` 放在 `</body>` 之前）。
2. **`src/style.css`**：映射 `--xcpc-*` 到本地 CSS 变量，同时通过 `@media (prefers-color-scheme: dark)` 与 `:root[data-theme="dark"]` 保证独立预览与宿主内外观一致；数字区域启用 `font-variant-numeric: tabular-nums`。
3. **`src/main.ts`**：
   - 初始化时调用 `syncTheme()` 读取 `window.xcpc?.theme` 并订阅 `window.xcpc?.onThemeChange(...)`。
   - 用 `window.xcpc.storage` 读写用户偏好/草稿（捕获异常并提供默认值）。
   - 复制、导出文件、提示反馈一律走 `window.xcpc.clipboard`、`window.xcpc.files`、`window.xcpc.toast`。

### Step 4：构建、静态体检与沙箱单测

```bash
# 1. Vite 项目先安装依赖并构建到 dist/
npm install
npm run build
# vite.config.ts 只需 plugins: [xcpcPlugin()]：
# 自动将 CSS + IIFE JS 内联到 dist/ui.html，编译 src/index.ts -> dist/index.js（CJS + sandboxGuard），并复制 manifest.toml

# 2. 静态 + QuickJS 导出干跑校验（Vite 项目检查 dist/，零构建项目检查插件根目录）
xcpc plugin check dist

# 3. 直接在 QuickJS 沙箱中执行导出函数验证逻辑（无需安装）
xcpc plugin test dist runTool --args '["calc", {"expr": "998244353"}]'
# OJ 插件示例：
xcpc plugin test dist verify --args '{"handle": "tourist"}'
```

> 验收标准：`xcpc plugin check` 返回退出码 0（零 error），且 `xcpc plugin test` 对所有核心 `toolId` / `action` 返回符合契约的 JSON 且无异常日志。

### Step 5：在 XCPC Workstation 中实时联调

```bash
# 1. 将构建产物目录链接到 <dataDir>/plugins/<id>（开启桌面端文件监听热重载）
xcpc plugin link dist

# 2. 若插件声明了 domains / credentials / workspace_read，首次链接会处于 pending_trust 状态，需显式信任：
xcpc plugin trust <plugin-id>

# 3. （Vite 项目）启动监听构建，修改代码自动重建 dist/ 并触发桌面端重载
npx vite build --watch
```

在 XCPC Workstation 桌面端打开对应工具或设置页，逐项自检：
- 浅色与深色模式切换时配色自然、无对比度缺陷或闪白；
- 刷新/重开工具后通过 `window.xcpc.storage` 保存的状态能正确恢复；
- 剪贴板复制、文件保存/选取、Toast 提示与 `runTool` 调用全部正常；
- 无运行时未捕获异常（宿主会自动捕获 `error` / `unhandledrejection` 并展示错误面板）。

### Step 6：打包与冷安装验收

```bash
# 1. 校验并打包为干净的 .zip（默认输出到上级目录 <id>-<version>.zip，也可 -o 指定路径）
xcpc plugin pack dist -o ./release/<id>-<version>.zip

# 2. 解除开发链接，用打包后的 .zip 做一次冷安装验证
xcpc plugin unlink <plugin-id>
xcpc plugin install ./release/<id>-<version>.zip
# 若有敏感权限，再次验证 trust 流程：
xcpc plugin trust <plugin-id>
```

确认 `pack` 输出的 `files` 列表仅包含预期文件、`sha256` 已记录，且冷安装后在桌面端运行与开发态完全一致。

---

## 5. 常见踩坑排查清单

- [ ] **Vite `<script>` 执行时机与 `#app` 为空**：Vite 默认把模块脚本放在 `<head>`，而单文件内联后会转为普通 IIFE `<script>`。`@xcpc/plugin-sdk/vite` 的 `inlineHtmlBundle` 会自动移除 `<head>` 中的原 script 标签并将其注入到 `</body>` 之前；若自行编写构建脚本或手写零构建 `ui.html`，**务必把内联 `<script>` 放在 `</body>` 前**（或监听 `DOMContentLoaded`），同时对内联代码中的 `</script` 转义为 `<\/script`。
- [ ] **`bridgeToken` 与 `sandbox="allow-scripts"` 导致的能力失效**：插件 iframe 的 `origin` 为 `"null"`，且不持有 `bridgeToken`。任何直接操作 `localStorage`、调用 `fetch()`、访问 `window.nsh` 或 `window.top` 的代码都会静默失败或抛出安全异常——必须全部改用 `window.xcpc.*`（UI 侧）或 `host.*`（QuickJS 侧）。
- [ ] **`pending_trust` 静默拦截外部插件**：非内置插件只要声明了非空 `permissions.domains`、`credentials` 或 `workspace_read = true`，在 `install`、`link` 或后续修改 `manifest.toml` **扩大权限**重载时，都会被 Core 标记为 `pending_trust = true`。此时 `list_tools` 会隐藏该插件，`load_plugin_ui_html` 与 `run_tool` 均返回 `PLUGIN_UNTRUSTED`。调试时看到插件未出现或报未信任，先运行 `xcpc plugin list` 检查状态并执行 `xcpc plugin trust <id>`。
- [ ] **`ui_entry` 扩展名与路径校验失败**：宿主强制校验 `ui_entry` 必须以 `.html` 或 `.htm` 结尾（旧版 `.js` 会报 `UNSUPPORTED_UI_ENTRY`）；文件不能为空白（`EMPTY_UI_HTML`）；且 `check` / `load_plugin_ui_html` 会对路径做规范化校验，任何指向插件目录外的 symlink 都会触发 `SYMLINK_ESCAPE`。
- [ ] **对 Vite 源目录直接执行 `check` / `link` / `pack`**：Vite 项目的源码在 `src/`，根目录没有 `index.js` 和 `ui.html`。必须先 `npm run build`，再对 **`dist/`** 目录执行 `xcpc plugin check dist`、`xcpc plugin link dist`、`xcpc plugin pack dist`。
- [ ] **`xcpc plugin pack` 白名单丢文件**：`pack` 只打包 `manifest.toml`、`entry`、`ui_entry` 以及可选的 `README.md` / `LICENSE` / `LICENSE.md` / `LICENSE.txt`。额外的图片、WASM 或 JSON 数据不要作为散落文件放在 `dist/` 下——应通过 Vite（`assetsInlineLimit: Infinity`）或 Data URL 内联进 `ui.html` 或 `index.js`。
- [ ] **QuickJS 沙箱中 `check` 阶段网络禁用**：`xcpc plugin check` 在干跑加载 `index.js` 时会注入禁用网络的 Mock HTTP 客户端。切勿在模块顶层（top-level）发起 `host.http.fetch(...)`，所有网络请求必须放在导出的函数内部按需触发。
