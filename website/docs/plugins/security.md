---
title: 安全边界与信任门控
---

# 安全边界与信任门控

XCPC Workstation 的所有插件安全边界均在 Rust Core（`xcpc-plugin`）与桌面宿主层强制执行，插件代码无法在运行时绕过。

---

## 1. Core 强制信任门（`pending_trust`）

任何来自外部来源（本地目录、`.zip` 归档、订阅仓库、CLI 或 `xcpc plugin link` 开发链接）安装的插件，只要在 `manifest.toml` 中声明了以下任一敏感权限，安装后一律进入 `pending_trust = true` 状态：

- `[permissions].domains` 非空（外网请求域名白名单，**禁止**通配符 `"*"`）
- `[permissions].workspace_read = true`（只读访问当前工作区的题目与比赛元数据）
- `[[credentials]]` 非空（通过系统 `keyring` 读取托管凭据）

在用户显式放行之前，Core 会拒绝该插件的所有调用并返回错误码 `PLUGIN_UNTRUSTED`。放行方式有两种：

1. 在桌面端安装后自动弹出（或前往「设置 → 插件」与触发调用时弹出）的权限确认框中核对并点击信任；
2. 在终端执行：
   ```bash
   xcpc plugin trust <plugin-id>
   ```

### 权限扩大与跨仓库覆写自动重置

Core 会持久化记录每个已安装插件的来源仓库（`source_repo`）与已授权权限快照。升级或重新安装时，只要出现以下任一情况，信任状态都会自动重置为 `pending_trust = true`，需要用户重新确认：

- 新版本请求的权限范围扩大（新增域名、新增凭据或新开启 `workspace_read`）；
- 来自不同仓库源的跨仓库覆写。

---

## 2. QuickJS 沙箱隔离

插件的 `index.js` 运行在 Core 内嵌的 `rquickjs` 沙箱中，与宿主机文件系统和网络栈隔离：

- **资源配额**：
  - 内存硬上限：**64 MiB**（超限终止并返回 `MEMORY_LIMIT`）；
  - 执行时间上限：Core 默认中断上限 **30 秒**（可按调用场景收紧，如无头工具与单测场景下超限终止并返回 `TIMEOUT`）；
  - HTTP 请求约束：`host.http.fetch` 仅放行 `[permissions].domains` 白名单中的域名，且**对重定向逐跳校验域名白名单**（最多 5 跳，跨源重定向自动剥离认证头）；默认单次请求超时 15 秒（最大 60 秒），支持 `responseType: "text" | "base64"`（其中 `"base64"` 二进制响应体上限 **5 MiB**）。
- **禁用 API**：
  - 不提供任何 Node.js 内置模块（`fs`、`path`、`http`、`https`、`child_process`、`net`、`os`、`process` 等）；
  - 不提供浏览器 DOM 或网络全局对象（`window`、`document`、`localStorage`、`XMLHttpRequest` 等）；
  - 使用 `@xcpc/plugin-sdk/vite` 打包时，上述禁用导入与全局引用会在编译期直接报错拦截。
- **托管能力边界**：
  - 凭据通过系统 `keyring` 按插件 ID 隔离托管，插件仅可按 key 读取自身作用域的凭据，无法访问或枚举其他插件的凭据；
  - 持久化状态通过插件独立作用域的 `host.kv` 存储，不同插件间完全隔离；
  - 工作区访问严格只读（且需声明并信任 `workspace_read = true`），不可修改工作区文件。

---

## 3. 三层纯 Web `<WebView>` 隔离

声明了 `ui_entry = "ui.html"` 的工具插件运行在桌面端独立 `<WebView>` 中，宿主通过三层纵深防御隔绝外联泄露与越权调用：

### 第一层：原生能力封锁

桌面端配置文件 [`apps/desktop/nsh.json`](https://github.com/lihaoze123/xcpc-workstation/blob/main/apps/desktop/nsh.json) 显式声明：

```json
"capabilities": {
  "webview": {
    "hosts": []
  }
}
```

在底层 WebKitGTK / WebView2 策略层硬性禁止 WebView 发起任何直接网络请求或外部导航（仅放行宿主受控的本地壳文档 `about:blank` / WebView2 `data:text/html`，不授予任何 `http:` / `https:` 域名权限）。

### 第二层：受控外层壳 + Sandboxed Iframe + 严格 CSP

宿主为每个插件视图动态生成受控外层 HTML 壳，并在字节 0 注入严格 Content-Security-Policy：

```text
default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; base-uri 'none';
```

插件的 `ui.html` 被放入 `<iframe sandbox="allow-scripts" srcdoc="...">`（**不含** `allow-same-origin`、**不含** `allow-top-navigation`），杜绝外链脚本/样式加载、Cookie/Storage 越权以及通过注释或前置脚本绕过 CSP。

### 第三层：每次挂载独立 `bridgeToken` 与只写剪贴板

- 外层壳在每次挂载时生成随机 `bridgeToken`（不暴露给 iframe），仅中继同时满足 `ev.source === frame.contentWindow && ev.origin === "null"` 的消息至宿主校验；
- 宿主通过 `window.xcpc` 暴露最小化受控桥接（`runTool`、`storage`、`workspace`、`oj.listAccounts`、`toast`、`clipboard.{writeText,writeImage}`、`files`、`openTool` 与主题变量 `--xcpc-*`）；
- **明确不暴露**剪贴板读取（`clipboard.readText` / `clipboard.readImage`）与外部链接直跳（`openExternal`）；`oj.listAccounts()` 仅返回已绑定账号的平台、Handle 与缓存 Rating/Rank，不暴露凭据与提交明细。

---

## 4. 仓库协议、完整性校验与防路径逃逸

- **强制 HTTPS 与逐跳校验**：插件仓库 `index.json` 与插件包下载地址强制要求 **HTTPS**（仅豁免本地回环地址 `127.0.0.1`、`localhost`、`::1` 用于本地调试），自动跟随 HTTP 重定向时逐跳校验 HTTPS。
- **SHA-256 完整性校验**：从仓库下载 `.zip` 安装包时，Core 会严格比对索引声明的 SHA-256 校验和，不匹配即中止安装。
- **防 Zip Slip 与压缩包软链拒绝**：解压插件 `.zip` 时校验每个条目的规范化相对路径，拒绝任何 `..` 路径穿越或绝对路径，同时拒绝压缩包内的符号链接条目（Unix mode `0o120000`）。
- **开发软链防逃逸（`SYMLINK_ESCAPE`）**：执行 `xcpc plugin link <dir>` 时，Core 仅放行登记在 `plugin_links.json` 中的顶层插件目录；插件目录内部任何试图指向插件根目录之外的符号链接均会被拒绝并报错 `SYMLINK_ESCAPE`。
