# 插件开发指南（Plugin Development Guide）

本文档是 **XCPC Workstation 插件开发者的唯一事实源**（对应当前版本 `v0.4.0-beta.1` 与 `@xcpc/plugin-sdk` `v0.2.0`）。

> **防漂移约定**：本文档只阐述架构模型、安全边界、开发工作流与最小可运行示例，**不复制**详尽的字段表与方法签名。所有权威 API 契约与清单字段以仓内类型定义和 Schema 为准：
> - QuickJS 宿主 API 与插件导出契约：[packages/plugin-sdk/host.d.ts](../packages/plugin-sdk/host.d.ts)
> - WebView `window.xcpc` 能力桥接：[packages/plugin-sdk/ui.d.ts](../packages/plugin-sdk/ui.d.ts)
> - Vite 单文件构建插件选项：[packages/plugin-sdk/vite.d.ts](../packages/plugin-sdk/vite.d.ts)
> - `manifest.toml` JSON Schema：[packages/plugin-sdk/manifest.schema.json](../packages/plugin-sdk/manifest.schema.json)

---

## 1. 插件架构与三种开发模式

无论使用免构建手写文件还是 Vite 工程化构建，宿主始终只识别单一、自包含的插件目录结构：

- `manifest.toml`（必需）：插件元数据、能力声明、权限与工具/命令定义。
- `index.js`（默认入口，可选省略但建议提供）：运行在 Core **QuickJS 沙箱** 中的无头逻辑，桌面端、CLI 与 MCP 共用。
- `ui.html`（可选，由 `ui_entry = "ui.html"` 声明）：运行在桌面端 **纯 Web `<WebView>` 沙箱** 中的自定义界面，必须是单文件自包含 HTML（JS/CSS/资源全部内联）。

基于上述产物，插件主要有三种形态：

### 模式 A：OJ 数据源插件（`oj_source`）

- 在 `manifest.toml` 中声明 `capabilities = ["oj_source"]` 与 `[permissions].domains`。
- 运行在 Core QuickJS 沙箱内，通过 `host.http.fetch`、`host.html.select`、`host.credentials.get`、`host.kv` 等宿主能力同步账号、提交记录、Rating 历史与比赛列表。
- 导出契约见 [`OjSourcePlugin`](../packages/plugin-sdk/host.d.ts)。主仓库内置的 `codeforces` 与 `atcoder` 即为此模式。

### 模式 B：无头表单工具（Headless `tool`）

- 声明 `capabilities = ["tool"]` 与 `[[tools]]`（含 `[[tools.inputs]]`），**不声明** `ui_entry`。
- 仅提供 `index.js` 并导出 `runTool(toolId, inputs, ctx)`，返回结构化 `ToolExecutionResult`（文本、Markdown、表格、图、SVG、PNG 等块）。
- 桌面端自动使用原生 `HeadlessToolRunner` 根据 `inputs` 渲染表单并展示结果；同时天然支持 `xcpc plugin test`、CLI 与 MCP 调用。官方插件 `complexity-mem`、`string-lab` 即为此模式。

### 模式 C：纯 Web 自定义界面工具（`<WebView>` `tool`）

- 在模式 B 基础上声明 `ui_entry = "ui.html"`（可同时保留 `index.js` 作为 QuickJS 计算/CLI/MCP 后端，并可搭配 `commands` 在命令面板中一键打开）。
- 桌面端将 `ui.html` 加载进隔离的 `<WebView>`，并注入类型化的 `window.xcpc` 能力桥接与可选的宿主主题 CSS 变量（`--xcpc-*` 与 `data-theme="light|dark"`）。
- 宿主只管能力不管样式：你可以使用原生 HTML/TS/CSS、Canvas2D、SVG，或通过 Vite 打包 Preact/React 等任意标准前端框架。官方插件 `graph-editor`、`math-bitwise` 即为此模式。

> 除上述三种主形态外，`manifest.toml` 还支持 `commands`、`importer`、`exporter`、`credentials` 扩展点，详见 [`manifest.schema.json`](../packages/plugin-sdk/manifest.schema.json) 与 [`host.d.ts`](../packages/plugin-sdk/host.d.ts)。

---

## 2. 安全边界与沙箱机制

所有安全边界均在 Core 与桌面宿主层强制执行，插件无法绕过：

### 2.1 Core 强制信任门（`pending_trust`）

- 任何外部来源（本地目录、`.zip`、订阅仓库、CLI、`xcpc plugin link`）安装的插件，只要在 `manifest.toml` 中声明了以下任一敏感权限，安装后一律进入 `pending_trust = true` 状态，调用会被 Core 拒绝（`PLUGIN_UNTRUSTED`）：
  - `[permissions].domains` 非空（网络请求白名单，禁止通配符 `"*"`）
  - `[permissions].workspace_read = true`（只读访问当前工作区题目/比赛）
  - `[[credentials]]` 非空（读取凭据）
- 用户必须在桌面端确认弹窗中放行，或执行 `xcpc plugin trust <id>` 后方可执行。
- Core 记录每个已安装插件的 `source_repo`：发生跨仓库覆写或升级时权限范围扩大，都会自动重置为 `pending_trust = true`。

### 2.2 三层 WebView 隔离

1. **原生能力封锁**：桌面端 [`apps/desktop/nsh.json`](../apps/desktop/nsh.json) 声明 `capabilities.webview.hosts = []`，在 WebKitGTK / WebView2 策略层硬性禁止 WebView 发起任何直接网络请求或外部导航。
2. **受控外层壳 + Sandboxed Iframe**：宿主生成受控外层 HTML 壳，在字节 0 注入严格 CSP：
   ```text
   default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; base-uri 'none';
   ```
   插件的 `ui.html` 被放入 `<iframe sandbox="allow-scripts" srcdoc="...">`（无 `allow-same-origin`、无 `allow-top-navigation`），彻底杜绝外链资源、Cookie/Storage 越权或通过注释/前置脚本绕过 CSP。
3. **每次挂载独立 `bridgeToken`**：外层壳持有每次挂载随机生成的 `bridgeToken`（不暴露给 iframe），仅中继满足 `ev.source === frame.contentWindow && ev.origin === "null"` 的消息给宿主校验。宿主不向未授权插件暴露 `clipboard.readText` 或 `openExternal`。

### 2.3 QuickJS 沙箱、仓库协议与软链防护

- **QuickJS 沙箱**：`index.js` 环境中没有 Node 内置模块（`fs`/`http`/`child_process` 等），也没有浏览器 DOM（`window`/`document`/`localStorage`/`XMLHttpRequest`）；使用 `@xcpc/plugin-sdk/vite` 时会在编译期拦截上述禁用导入与全局引用。
- **仓库源与下载校验**：插件仓库 URL 强制 **HTTPS**（仅豁免本地回环地址 `127.0.0.1` / `localhost` / `::1`），且重定向逐跳校验 HTTPS；安装包下载强制校验 SHA-256 与防 Zip Slip 路径穿越。
- **开发软链隔离**：`xcpc plugin link <dir>` 仅放行登记在 `plugin_links.json` 中的顶层插件目录，目录内部任何试图逃逸出插件根的符号链接均会被拒绝（`SYMLINK_ESCAPE`）。

---

## 3. 开发工作流

### 3.1 安装 `@xcpc/plugin-sdk`

`@xcpc/plugin-sdk` 随官方插件仓库 `xcpc-plugins` 的 `official-latest` Release 以 tarball 形式发布，无需依赖公共 npm 仓库：

```bash
npm install -D https://github.com/lihaoze123/xcpc-plugins/releases/download/official-latest/xcpc-plugin-sdk.tgz
```

使用 `xcpc plugin init --vite` 创建项目时，上述依赖会自动写入 `package.json`。

### 3.2 初始化脚手架（`xcpc plugin init`）

CLI 同时支持 **免构建（零 Node 依赖）** 与 **Vite + TypeScript 工程化** 两条轨道：

```bash
# 1. 免构建轨道（默认 --kind tool，生成 manifest.toml + index.js + ui.html）
xcpc plugin init ./my-tool

# 免构建 OJ 插件（生成 manifest.toml + index.js）
xcpc plugin init ./my-oj --kind oj

# 2. Vite + TypeScript 轨道（自动配置 @xcpc/plugin-sdk/vite 单文件打包）
xcpc plugin init ./my-tool --kind tool --vite
cd ./my-tool
npm install
npm run build
```

> 在编辑器中为 `manifest.toml` 首行保留 `# yaml-language-server: $schema=./node_modules/@xcpc/plugin-sdk/manifest.schema.json`（或配合 Taplo 配置），即可获得自动补全与静态校验。

### 3.3 本地联调与按插件热重载（`link` / `test` / `watch`）

桌面端内置按插件粒度的文件监听，仅重载发生变更的插件，编辑或重建不会重置其他已打开的工具页面。

- **免构建插件**：直接链接源码目录，保存 `manifest.toml` / `index.js` / `ui.html` 即刻生效：
  ```bash
  xcpc plugin link ./my-tool
  ```
- **Vite 工程化插件**：先构建一次，链接产物目录 `dist/`，再开启增量监听构建：
  ```bash
  npm run build
  xcpc plugin link ./dist
  npm run dev   # 等价于 vite build --watch，每次保存自动更新 dist/ 并触发桌面端热重载
  ```
- **信任敏感权限**（若声明了 `domains` / `credentials` / `workspace_read`）：
  ```bash
  xcpc plugin trust my-tool
  ```
- **无头快速单测**（无需安装，直接在 QuickJS 沙箱中执行导出函数）：
  ```bash
  # 工具插件默认调用 runTool，传入 [toolId, inputs]
  xcpc plugin test ./dist runTool --args '["hello", {"text":"XCPC"}]'

  # OJ 插件默认调用 verify
  xcpc plugin test ./my-oj verify --args '{"handle":"tourist"}'
  ```
- **开发完成后取消链接**（不删除源码目录）：
  ```bash
  xcpc plugin unlink my-tool
  ```

### 3.4 校验与打包（`check` / `pack`）

```bash
# 校验 manifest、路径沙箱、QuickJS 导出与 ui_entry（支持 --json）
# 注意：Vite 项目应校验产物目录 dist/，而不是源码根目录
xcpc plugin check ./dist

# 校验并打包为干净的分发 .zip（默认输出到父目录 <id>-<version>.zip，避免触发热重载），同时打印 SHA-256
xcpc plugin pack ./dist
# 或指定输出路径
xcpc plugin pack ./dist -o ./release/my-tool-0.1.0.zip
```

### 3.5 发布与社区收录

将 `xcpc plugin pack` 生成的 `.zip` 上传至你的 GitHub Release 后：
- 用户可直接通过本地路径或 `xcpc plugin install <path-or-id>` 安装；
- 若希望上架到默认内置的官方索引，向 [`lihaoze123/xcpc-plugins`](https://github.com/lihaoze123/xcpc-plugins) 提交 PR，在 `registry.toml` 中登记元数据、`download_url`（HTTPS）与 `sha256`，CI 会自动下载校验并合入 `index.json`。

---

## 4. 最小可运行示例

### 4.1 无头表单工具（免构建）

目录结构：`my-calc/{manifest.toml, index.js}`。

`manifest.toml`：

```toml
id = "my-calc"
name = "模运算小工具"
version = "0.1.0"
description = "快速计算 (a^b) mod m"
entry = "index.js"
capabilities = ["tool"]

[permissions]
domains = []
workspace_read = false

[[tools]]
id = "powmod"
name = "快速幂"

[[tools.inputs]]
name = "a"
label = "底数 a"
kind = "number"
default = 2
required = true

[[tools.inputs]]
name = "b"
label = "指数 b"
kind = "number"
default = 10
required = true

[[tools.inputs]]
name = "m"
label = "模数 m"
kind = "number"
default = 998244353
required = true
```

`index.js`：

```js
export function runTool(toolId, inputs) {
  if (toolId !== "powmod") {
    throw new Error(`Unknown tool: ${toolId}`);
  }
  let base = BigInt(inputs.a ?? 0);
  let exp = BigInt(inputs.b ?? 0);
  const mod = BigInt(inputs.m ?? 1);
  if (mod <= 0n || exp < 0n) {
    throw new Error("要求 m > 0 且 b >= 0");
  }
  let ans = 1n % mod;
  base = ((base % mod) + mod) % mod;
  while (exp > 0n) {
    if (exp & 1n) ans = (ans * base) % mod;
    base = (base * base) % mod;
    exp >>= 1n;
  }
  return {
    summary: `结果 = ${ans}`,
    blocks: [
      { type: "text", title: "快速幂结果", content: ans.toString(), monospace: true },
    ],
  };
}
```

### 4.2 纯 Web `<WebView>` 工具（Vite + `@xcpc/plugin-sdk`）

演示 `window.xcpc`（通过 `getXcpc()` 获取）的 `storage`、主题同步、`toast` 与文件导出，以及 `@xcpc/plugin-sdk/vite` 单文件打包配置。

`vite.config.ts`：

```ts
import { defineConfig } from "vite";
import { xcpcPlugin } from "@xcpc/plugin-sdk/vite";

export default defineConfig({
  plugins: [xcpcPlugin()],
});
```

`manifest.toml`（关键字段）：

```toml
id = "scratchpad"
name = "草稿本"
version = "0.1.0"
entry = "index.js"
ui_entry = "ui.html"
capabilities = ["tool"]

[permissions]
domains = []
workspace_read = false

[[tools]]
id = "stats"
name = "字数统计"
```

`index.html`：

```html
<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>草稿本</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

`src/main.ts`：

```ts
import { getXcpc } from "@xcpc/plugin-sdk/ui";

const xcpc = getXcpc();
const root = document.getElementById("app")!;

root.innerHTML = `
  <main style="padding:16px;font-family:var(--xcpc-font-sans);color:var(--xcpc-fg);background:var(--xcpc-bg);min-height:100vh;">
    <textarea id="note" rows="8" style="width:100%;background:var(--xcpc-card);color:var(--xcpc-card-fg);border:1px solid var(--xcpc-border);border-radius:8px;padding:8px;"></textarea>
    <div style="margin-top:8px;display:flex;gap:8px;align-items:center;">
      <button id="export">导出文本</button>
      <span id="theme-badge"></span>
    </div>
  </main>
`;

const textarea = document.getElementById("note") as HTMLTextAreaElement;
const badge = document.getElementById("theme-badge")!;

// 1. 读取持久化草稿（Core 独立作用域 KvStore）
void xcpc.storage.get<string>("draft").then((saved) => {
  if (saved) textarea.value = saved;
});
textarea.addEventListener("input", () => {
  void xcpc.storage.set("draft", textarea.value);
});

// 2. 响应宿主浅色/深色主题切换（--xcpc-* CSS 变量也会自动同步）
const renderTheme = (scheme: string) => {
  badge.textContent = `当前主题：${scheme}`;
};
renderTheme(xcpc.theme.scheme);
xcpc.onThemeChange((next) => renderTheme(next.scheme));

// 3. 通过宿主能力导出文件并给出反馈
document.getElementById("export")!.addEventListener("click", async () => {
  const savedPath = await xcpc.files.saveText("note.txt", textarea.value);
  if (savedPath) {
    xcpc.toast("已导出草稿", "success");
  }
});
```

> 若不需要 Vite 构建，也可以直接手写单文件 `ui.html`，在 `<script>` 中直接访问全局 `window.xcpc`。

---

## 5. 权威 API 参考

- QuickJS 宿主全局对象 `host` 与各能力模块导出契约：[`packages/plugin-sdk/host.d.ts`](../packages/plugin-sdk/host.d.ts)
- WebView 桥接对象 `window.xcpc` 与 `getXcpc()`：[`packages/plugin-sdk/ui.d.ts`](../packages/plugin-sdk/ui.d.ts)
- Vite 单文件打包插件 `xcpcPlugin(options?)`：[`packages/plugin-sdk/vite.d.ts`](../packages/plugin-sdk/vite.d.ts)
- 插件清单完整字段与约束：[`packages/plugin-sdk/manifest.schema.json`](../packages/plugin-sdk/manifest.schema.json)
- Phase 4D 架构规格与设计背景：[`docs/specs/phase-4d-plugin-sdk-and-ecosystem.md`](./specs/phase-4d-plugin-sdk-and-ecosystem.md)
