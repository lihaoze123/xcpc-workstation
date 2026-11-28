---
title: 插件开发指南
---

# 插件开发指南

本指南介绍如何基于 `@xcpc-workstation/plugin-sdk`（当前版本 `v0.3.0`）与 `xcpc plugin` CLI 开发、本地联调、校验打包并发布 XCPC Workstation 插件。

> **权威 API 契约**（以仓库内 TypeScript 类型定义与 JSON Schema 为唯一事实源）：
> - QuickJS 宿主全局对象 `host` 与插件导出契约：[`packages/plugin-sdk/host.d.ts`](https://github.com/lihaoze123/xcpc-workstation/blob/main/packages/plugin-sdk/host.d.ts)
> - WebView 能力桥接 `window.xcpc` 与 `getXcpc()`：[`packages/plugin-sdk/ui.d.ts`](https://github.com/lihaoze123/xcpc-workstation/blob/main/packages/plugin-sdk/ui.d.ts)
> - Vite 单文件打包插件 `xcpcPlugin(options?)`：[`packages/plugin-sdk/vite.d.ts`](https://github.com/lihaoze123/xcpc-workstation/blob/main/packages/plugin-sdk/vite.d.ts)
> - `manifest.toml` JSON Schema：[`packages/plugin-sdk/manifest.schema.json`](https://github.com/lihaoze123/xcpc-workstation/blob/main/packages/plugin-sdk/manifest.schema.json)

---

## 1. 安装 `@xcpc-workstation/plugin-sdk`

`@xcpc-workstation/plugin-sdk` 已发布至 npm，直接安装为开发依赖即可：

```bash
npm install -D @xcpc-workstation/plugin-sdk
```

使用 `xcpc plugin init <dir> --vite` 创建项目时，该依赖会自动写入 `package.json`。

`@xcpc-workstation/plugin-sdk` 提供四个子路径导出：

| 子路径 | 用途 |
| :--- | :--- |
| `@xcpc-workstation/plugin-sdk/host` | QuickJS 沙箱 `host` 全局类型与各能力模块契约（`OjSourcePlugin`、`ToolPlugin`、`CommandPlugin`、`ImporterPlugin`、`ExporterPlugin` 等）。 |
| `@xcpc-workstation/plugin-sdk/ui` | 类型化的 `window.xcpc` 桥接接口（`XcpcBridge`）与 `getXcpc()` 辅助函数。 |
| `@xcpc-workstation/plugin-sdk/vite` | 零配置 `xcpcPlugin(options?)` Vite 插件：将 WebView 界面打包为单文件自包含 IIFE `dist/ui.html`，将 `src/index.ts` 编译为 `dist/index.js` 并启用 QuickJS 禁用 API 编译期护栏，同时复制 `manifest.toml`。 |
| `@xcpc-workstation/plugin-sdk/manifest.schema.json` | 供编辑器（如 Taplo / YAML Language Server）提供 `manifest.toml` 自动补全与静态校验。 |

---

## 2. 双轨道初始化脚手架（`xcpc plugin init`）

`xcpc plugin init` 同时支持**免构建（零 Node 依赖）**与 **Vite + TypeScript 工程化**两条轨道：

```bash
# 轨道一：免构建工具插件（默认 --kind tool，生成 manifest.toml + index.js + ui.html）
xcpc plugin init ./my-tool

# 免构建 OJ 数据源插件（生成 manifest.toml + index.js）
xcpc plugin init ./my-oj --kind oj

# 免构建排版模板插件（生成 manifest.toml + templates/sample.typ + assets/badge.svg）
xcpc plugin init ./my-template --kind template

# 轨道二：Vite + TypeScript 工程化工具插件（自动配置 @xcpc-workstation/plugin-sdk/vite）
xcpc plugin init ./my-tool --kind tool --vite
cd ./my-tool
npm install
npm run build
```

> 在编辑器中为 `manifest.toml` 首行保留 `# yaml-language-server: $schema=./node_modules/@xcpc-workstation/plugin-sdk/manifest.schema.json`（或配合 Taplo 配置），即可获得字段补全与静态检查。

---

## 3. 本地联调、按插件热重载与单测

桌面端内置**按插件 ID 粒度**的文件监听：编辑或重建某个插件时，仅重载该插件本身，不会重置其他已打开的工具页面。

### 免构建插件

直接链接源码目录，保存 `manifest.toml`、`index.js` 或 `ui.html` 即刻生效：

```bash
xcpc plugin link ./my-tool
```

### Vite 工程化插件

先完成首次构建，链接产物目录 `dist/`，再启动增量监听构建：

```bash
npm run build
xcpc plugin link ./dist
npm run dev   # 等价于 vite build --watch，保存即更新 dist/ 并触发桌面端热重载
```

### 放行敏感权限

若插件声明了 `domains`、`credentials`、`workspace_read = true` 或 `workspace_write = true`，链接后需执行一次信任确认（纯 `template` 排版模板插件免信任弹窗；详见 [安全边界与信任门控](./security.md)）：

```bash
xcpc plugin trust my-tool
```

### 无头快速单测（`xcpc plugin test`）

无需安装或启动桌面端，即可直接在 QuickJS 沙箱中调用插件导出函数：

```bash
# 工具插件默认调用 runTool，参数为 [toolId, inputs]
xcpc plugin test ./dist runTool --args '["hello", {"text":"XCPC"}]'

# OJ 插件默认调用 verify
xcpc plugin test ./my-oj verify --args '{"handle":"tourist"}'
```

### 取消开发链接

开发完成后可取消链接（不会删除本地源码目录）：

```bash
xcpc plugin unlink my-tool
```

---

## 4. 校验与打包（`check` / `pack`）

分发前使用 `xcpc plugin check` 与 `xcpc plugin pack` 完成完整性与安全检查：

```bash
# 校验 manifest.toml、路径沙箱、QuickJS 导出契约与 ui_entry（支持 --json）
# 注意：Vite 项目应校验产物目录 dist/，而非源码根目录
xcpc plugin check ./dist

# 校验并打包为干净的分发 .zip（默认输出到父目录 <id>-<version>.zip，避免触发热重载），同时打印 SHA-256
xcpc plugin pack ./dist

# 或手动指定输出路径
xcpc plugin pack ./dist -o ./release/my-tool-0.1.0.zip
```

---

## 5. 最小可运行示例

### 示例 A：无头表单工具（免构建）

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

`index.js`（免构建插件直接由 QuickJS 沙箱以 CommonJS 方式加载，使用 `module.exports` 导出）：

```js
function runTool(toolId, inputs) {
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

module.exports = { runTool };
```

运行验证：

```bash
xcpc plugin check ./my-calc
xcpc plugin test ./my-calc runTool --args '["powmod", {"a":2,"b":10,"m":998244353}]'
```

### 示例 B：纯 Web `<WebView>` 工具（Vite + `@xcpc-workstation/plugin-sdk`）

以下示例展示如何通过 `getXcpc()` 使用插件作用域持久化存储（`storage`）、宿主主题自动同步（`--xcpc-*` 与 `onThemeChange`）、文件导出（`files.saveText`）与右下角提示（`toast`）。

`vite.config.ts`：

```ts
import { defineConfig } from "vite";
import { xcpcPlugin } from "@xcpc-workstation/plugin-sdk/vite";

export default defineConfig({
  plugins: [xcpcPlugin()],
});
```

`manifest.toml`：

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

`src/index.ts`（声明 `capabilities = ["tool"]` 时必需，由 `@xcpc-workstation/plugin-sdk/vite` 编译为 `dist/index.js`）：

```ts
import type { ToolExecutionResult } from "@xcpc-workstation/plugin-sdk/host";

export function runTool(
  toolId: string,
  inputs: Record<string, unknown>,
): ToolExecutionResult {
  if (toolId !== "stats") {
    throw new Error(`Unknown tool: ${toolId}`);
  }
  const text = String(inputs?.text ?? "");
  return {
    summary: `共 ${text.length} 个字符`,
    blocks: [{ type: "text", content: `字符数：${text.length}` }],
  };
}
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
import { getXcpc } from "@xcpc-workstation/plugin-sdk/ui";

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

// 1. 读取与保存插件作用域持久化草稿
void xcpc.storage.get<string>("draft").then((saved) => {
  if (saved) textarea.value = saved;
});
textarea.addEventListener("input", () => {
  void xcpc.storage.set("draft", textarea.value);
});

// 2. 响应宿主浅色/深色主题切换（--xcpc-* CSS 变量自动同步）
const renderTheme = (scheme: string) => {
  badge.textContent = `当前主题：${scheme}`;
};
renderTheme(xcpc.theme.scheme);
xcpc.onThemeChange((next) => renderTheme(next.scheme));

// 3. 通过宿主文件对话框导出文本并弹出提示
document.getElementById("export")!.addEventListener("click", async () => {
  const savedPath = await xcpc.files.saveText("note.txt", textarea.value);
  if (savedPath) {
    xcpc.toast("已导出草稿", "success");
  }
});
```

> 若无需 Vite 构建，可直接手写单文件 `ui.html`，在 `<script>` 中访问全局 `window.xcpc`。

### 纯 Web `<WebView>` 宿主环境约定

运行 `ui_entry = "ui.html"` 工具时，桌面宿主会自动提供以下环境保障，确保插件在 Linux（WebKitGTK）与 Windows（WebView2）上表现一致：

- **内嵌字体与主题变量**：自动注入 `--xcpc-*` 主题变量（`data-theme="light|dark"`）以及 `Inter` 与 `JetBrains Mono` 的内联 `@font-face`（`font-src data:`）。若插件在 Canvas2D / SVG 中使用宿主字体，建议在 `document.fonts.ready` 后触发一次重绘。
- **低优先级基础样式（`:where(...)`）**：宿主以零优先级（`:where(...)`）注入 `6px` 细滚动条、适配浅色/深色主题的 `<select>` 下拉箭头，以及按钮的字体继承与 `white-space: nowrap`，避免 Windows WebView2 原生控件突兀，同时插件任意自定义 CSS 选择器均可无阻覆盖。
- **全高画布布局**：纯 Web 插件上方不叠放重复的宿主顶栏，插件自带工具栏直接作为主顶栏展示；用户可通过左侧栏底部按钮折叠插件列表以获得更宽画布。
- **跨平台剪贴板与空闲回收**：`xcpc.clipboard.writeImage(pngBase64)` 在 Windows 上自动写入 `CF_DIB` 位图与 `"PNG"` 双格式，确保直接粘贴到常见编辑器、聊天软件与绘图工具均生效。为控制常驻内存，用户离开「工具」空间约 15 秒后宿主会自动卸载空闲 `<WebView>` 实例，因此请始终使用 `xcpc.storage` 持久化需要保留的编辑状态。

---

## 6. 向 `xcpc-plugins` 社区上架

1. 执行 `xcpc plugin pack ./dist`，记录生成的 `.zip` 文件与终端打印的 SHA-256 校验和；
2. 将 `.zip` 上传至你的 GitHub Release（或任何支持 HTTPS 的静态托管地址）；
3. 用户即可通过本地文件或 `xcpc plugin install <path-or-id>` 安装；
4. 如希望上架至默认内置的官方索引，向 [lihaoze123/xcpc-plugins](https://github.com/lihaoze123/xcpc-plugins) 提交 PR，在 `registry.toml` 中登记插件元数据、HTTPS `download_url` 与 `sha256`。仓库 CI 会自动拉取安装包、校验 SHA-256 与安全规则，合并后自动更新 `index.json`。
