# Phase 4D 规格：Plugin SDK、开发工具链与官方收录仓库

2026-11-25 对齐确认，作为 Phase 4 的收尾子阶段（紧接 Phase 4C 之后），聚焦**插件开发体验（DX）、Vite + TypeScript/JSX 工程化工具链、本地热重载、安全软链开发流**，并将 4 个官方离线工具插件迁出至独立官方收录仓库，以真实官方样板验证整套生态闭环。

---

## 1. 原则与架构边界

1. **双轨共存、产物单一**：
   - 宿主（Rust `rquickjs` + NSH 桌面端）始终只认最终单文件产物：`manifest.toml` + `index.js` + 可选 `ui.js`；
   - **纯 JS 免构建轨道**：零 Node/npm 依赖，手写目录即可运行，适合轻量脚本、OJ 数据源与简单导入/导出插件；
   - **Vite + TypeScript/JSX 工程化轨道**：支持多文件模块化、npm 纯 JS 库 Tree-shaking 打包、完整类型补全、IDE `manifest.toml` 校验与构建期产物护栏。
2. **显式函数式响应式（零黑盒）**：
   - 延续 NSH `signal` 本身即 getter 的心智模型，组件 props 统一支持直接传 `value={tab}` 或 `() => expr`；
   - **不做编译期 AST 魔法转换**，确保零黑盒、报错行号 1:1 准确，通过 `@xcpc/plugin-sdk/ui` 的 TypeScript 类型定义引导写对。
3. **主仓库保持零 Node 构建依赖**：
   - 主仓库内置插件收敛为 **2 个核心 OJ 插件**（`codeforces`、`atcoder`），保证训练看板开箱可用且 Rust Core 构建无需 Node.js；
   - 4 个工具插件（`graph-editor`、`math-bitwise`、`complexity-mem`、`string-lab`）全部迁出至独立官方收录仓库，以 `@xcpc/plugin-sdk` + Vite + TSX 重构并作为官方样板与生态狗粮；
   - 桌面端默认预置官方收录仓库订阅（可移除），并在「工具」空态直接展示官方推荐工具列表供一键安装。
4. **安全边界不妥协**：
   - `xcpc plugin link <dir>` 仅放行显式注册的顶层开发链接根目录，插件内部任何子路径经 `canonicalize` 后仍严禁逃逸出该目标根目录；
   - 第三方或含 `ui.js` 的插件仍遵循既有的信任模型与安装前显式提示（官方预置仓库中的插件也清晰标明来源与非沙箱 UI 边界）。

### 明确不做（Phase 4D 范围外）
- 不做编译期响应式语法糖 / Babel / 自定义 JSX 响应式编译器；
- 不做独立开发守护进程、WebSocket HMR 服务器或内置插件代码编辑器；
- 不做中心化账号/付费/评价系统式插件商店（沿用去中心化 `index.json` + SHA-256 仓库协议）；
- 不改变既有 5 类扩展点（`oj_source`、`tool`、`commands`、`importer`/`exporter`、`credentials`）的运行时协议。

---

## 2. 仓内单一 SDK 包：`packages/plugin-sdk`（`@xcpc/plugin-sdk`）

在主仓库根目录下新增 `packages/plugin-sdk/`（单包多入口导出，开发期支持 `file:` 本地引用，后续可直接发布 npm），零运行时重依赖：

| 子路径导出 | 定位与内容 |
| :--- | :--- |
| `@xcpc/plugin-sdk/host` | `index.js`（QuickJS 沙箱）完整类型定义与协议契约（`HostApi`、`OjSourcePlugin`、`ToolPlugin`、`ToolOutputBlock`、`CommandPlugin`、`ImporterPlugin`、`ExporterPlugin` 等），取代零散的 `xcpc-plugin.d.ts`。 |
| `@xcpc/plugin-sdk/ui` | `ui.js`（NSH 宿主）完整类型定义（`PluginUiSdk`、`MaybeAccessor<T>`、全部 `ui.*` 组件 Props、JSX 类型声明）与虚拟模块导出；经 Vite 插件编译时自动映射为宿主注入的 `sdk` 参数，零重复打包宿主运行时。 |
| `@xcpc/plugin-sdk/vite` | 零配置 Vite 插件 `xcpcPlugin(options?)`，封装双入口单文件构建、沙箱静态护栏与打包辅助。 |
| `manifest.schema.json` | 随包发布的 JSON Schema（同步导出路径 `@xcpc/plugin-sdk/manifest.schema.json`），供 Taplo / VS Code 对 `manifest.toml` 做实时补全与校验。 |

### `@xcpc/plugin-sdk/vite` 行为规格
1. **自动读取 `manifest.toml` 推导入口**：
   - 检测 `src/index.{ts,js}` → 输出 `dist/index.js`（CJS、`inlineDynamicImports: true`、`target: "es2022"`）；
   - 若存在 `src/ui.{tsx,ts,jsx,js}`（或 `manifest.toml` 声明 UI 入口）→ 输出 `dist/ui.js`（CJS、包裹为宿主 `render(sdk)` 契约、`jsxImportSource` / `jsxFactory: "sdk.h"` 自动配置，并将 `@xcpc/plugin-sdk/ui` 虚拟模块映射至注入的 `sdk`）；
2. **编译期护栏（针对 `index.js`）**：
   - 拦截 QuickJS 沙箱不可用的 Node 内置模块（`fs`、`path`、`child_process`、`http`、`node:*` 等）与浏览器 DOM 全局对象误用，构建阶段直接报错并给出替代的 `host.*` 提示；
3. **开发与构建联动**：
   - `vite build --watch` 每次增量构建后将产物写入 `dist/` 并自动同步 `manifest.toml`，直接触发桌面端 `notify` 热重载；
   - `vite build` 自动拷贝 `manifest.toml` 至 `dist/`，可选 `zip: true` 输出干净的可分发 `.zip`（或交给 `xcpc plugin pack` 统一校验打包）。

---

## 3. 宿主 Plugin UI SDK（`pluginSdk.tsx`）与可观测性升级

在现有 `apps/desktop/src/ui/tools/pluginSdk.tsx`（已有 `View`、`Text`、`Scroll`、`Canvas`、`Button`、`Icon`、`Segmented`、`Chip`、`Input`、`If`、`Output`）基础上补齐高频原语与生命周期：

### 3.1 新增 UI 原语与布局组件
- **列表与布局**：
  - `ui.Each`：带稳定 `key` 的响应式列表渲染（封装宿主 `<Each>`）；
  - `ui.Row` / `ui.Col`：常用 Flex 水平/垂直布局原语（默认 `gap`、`alignItems`、`flexWrap` 等快捷 props，遵循响应式 `MaybeAccessor`）；
  - `ui.Card`：遵循 `docs/design-system.md` Token、圆角与边框规范的容器卡片。
- **表单与展示组件**：
  - `ui.Switch`：布尔开关控件（复用宿主 `nsh/base` / `kit` 样式，无 1px 抖动）；
  - `ui.Select`：下拉选择器（选项列表 + 当前值 + `onChange`）；
  - `ui.Badge`：克制的中性/语义状态徽标（遵循设计规范，禁用品牌淡底色）。
- **画布辅助**：
  - `sdk.canvas.fitBounds(points, viewport, padding?)`：计算点集在当前视口内的自适应缩放与居中偏移，供图论/几何类插件直接复用。

### 3.2 高层状态、生命周期与持久存储
- **`sdk.useTool(toolId, inputsAccessor, options?)`**：
  - 封装调用自身 `index.js` 工具的常见样板：内置防抖（默认 `debounceMs: 120`）、请求序号竞态丢弃（仅采纳最新请求结果），返回 `{ data, loading, error, refresh }` 响应式句柄；
  - 组件卸载或热重载时自动取消未触发的防抖定时器并丢弃在途回调。
- **`sdk.onCleanup(fn)`**：
  - 注册清理函数，在插件切换卸载、禁用或热重载重建 `PluginUiHost` 前自动逆序执行（清理定时器、外部订阅等副作用），防止内存与定时器泄漏。
- **`sdk.storage`（同步 API，复用 Core `KvStore`）**：
  - 接口：`get<T = unknown>(key: string): T | undefined`、`set(key: string, value: unknown): void`、`delete(key: string): boolean`；
  - 底层经由桌面端 Native Bridge 直接读写 `crates/xcpc-plugin/src/kv.rs` 的 `KvStore`（`<dataDir>/plugin_kv/<plugin_id>.json`，原子持久化），与 `index.js` 的 `host.kv` 共享同一插件命名空间；
  - 同步返回便于插件在 `render(sdk)` 初始化 `signal(sdk.storage.get("...") ?? defaultVal)` 时零闪白恢复上次输入与偏好。

### 3.3 调试可观测性
- **Desktop `ui.js`**：在 `mountPluginUi` 构造执行函数时尾部注入 `//# sourceURL=xcpc-plugin://<pluginId>/ui.js`，使错误堆栈直接指向 `ui.js` 真实行号而非 `<anonymous>`；
- **Core `index.js`**：在 `rquickjs` 执行插件脚本时传入真实文件名（如 `xcpc-plugin://<pluginId>/index.js`），确保 QuickJS 异常堆栈携带准确文件名与行列号。

---

## 4. 本地热重载与 `xcpc plugin` CLI 工具链

### 4.1 软链开发流与安全模型（`link` / `unlink`）
- **注册机制**：
  - `xcpc plugin link <dir>` 校验 `<dir>/manifest.toml` 合法并读取 `id`（先验证 `validate_plugin_id`），在 `<dataDir>/plugins/<id>` 创建指向 `<dir>`（canonicalize 后的绝对路径）的 OS 符号链接，并在 `<dataDir>/plugin_links.json` 登记 `{ id, target }`；
  - `xcpc plugin unlink <id>` 移除符号链接与 `plugin_links.json` 中的登记项（绝不删除开发者源目录）；
  - 若目标 `<id>` 已存在非链接的本地安装目录，默认报错拒绝覆盖（提示先卸载或显式替换）。
- **路径沙箱守卫**：
  - `PluginManager::load_plugin` 加载 `<dataDir>/plugins/<id>` 时：
    - 若为普通目录：沿用现有规则，canonicalize 后必须位于 `<dataDir>/plugins/` 内；
    - 若为符号链接：**仅当 `<id>` 与其解析后的目标根目录精确匹配 `plugin_links.json` 登记项时放行**；任何未登记的符号链接一律拒绝；
    - 放行后，插件内部加载的 `manifest.toml`、`index.js`、`ui.js` 等所有子路径经 `canonicalize` 后仍严禁逃逸出该目标根目录。
- **桌面端标识**：
  - `PluginSummary` 增加 `is_linked: bool` 与 `link_target: Option<String>`；「设置 → 插件」与「工具」侧栏对开发链接插件显示简洁的「开发链接」标识（遵循设计系统克制风格）。

### 4.2 桌面端 `notify` 自动热重载
- 桌面端 Rust 侧（`apps/desktop/src/plugins.rs` / `lib.rs`）复用现有 `notify` 依赖，启动插件文件监听器：
  - 同时监听 `<dataDir>/plugins/` 以及 `plugin_links.json` 中每个已注册链接的真实目标目录（解决 Linux `inotify` 不自动追踪外部软链目录内文件变更的问题）；
  - 过滤 `manifest.toml`、`index.js`、`ui.js` 变更事件，做轻量防抖（如 `150ms`，覆盖 `vite build --watch` 的连续写入），自动重载 Core 插件缓存并向前端广播事件递增 `pluginsRevision`；
  - `PluginUiHost` 响应 `pluginsRevision` 变化：先触发旧实例的 `onCleanup` 回调，再无闪白原地重新挂载新版本 `ui.js`；
  - 新增/移除 `link` 或安装/卸载插件时自动同步刷新 watcher 监听列表。

### 4.3 `xcpc plugin` CLI 命令升级
遵循「做减法、不留同义重复命令」原则，升级现有 `init`、保留 `test`、新增 4 项开发命令：

| 命令 | 职责与行为 |
| :--- | :--- |
| `xcpc plugin init <dir> [--kind oj\|tool] [--vite]` | 升级现有命令：默认仍生成零构建纯 JS 模板（`manifest.toml` + `index.js`）；加 `--vite` 生成 Vite + TypeScript/JSX 工程模板（`package.json`、`vite.config.ts`、`tsconfig.json`、`manifest.toml`、`src/index.ts`、tool 类型附 `src/ui.tsx`）。 |
| `xcpc plugin link <dir>` / `xcpc plugin unlink <id>` | 挂载/卸载本地开发插件软链并维护 `plugin_links.json`，直接联动桌面端热重载。 |
| `xcpc plugin check <dir> [--json]` | 静态 + 运行时全面体检：① 校验 `manifest.toml` 合法性与字段约束；② 检查 `index.js` / `ui.js` 存在性与路径沙箱；③ 在 `rquickjs` 沙箱内 dry-run 加载 `index.js` 并校验声明扩展点对应的导出函数签名（如 `oj_source` 需导出 `syncAccount`/`normalizeRating`，`tool` 需导出 `runTool`）；④ 检查 `ui.js` 语法与导出 `render` 契约。失败返回非零退出码与结构化错误列表。 |
| `xcpc plugin pack <dir> [-o <out.zip>] [--json]` | 先自动运行 `check`（未通过拒绝打包），随后按白名单仅打包运行时必需文件（`manifest.toml`、`index.js`、可选 `ui.js`、`README.md` / `LICENSE` 等声明资产），严格排除 `node_modules/`、`src/`、`.git/`、`*.map` 等开发文件，输出 `.zip` 并打印 SHA-256 校验值（便于直接填入仓库 `index.json`）。 |
| `xcpc plugin test <dir> [action] [--args <json>] [--json]` | 保留既有命令，用于开发期单步调用指定导出函数并检查输出与耗时。 |

---

## 5. 官方插件边界与独立官方收录仓库

### 5.1 主仓库内置插件收敛
- `crates/xcpc-plugin/src/builtin.rs` 仅保留 **2 个核心 OJ 插件**：`codeforces`、`atcoder`；
- 移除主仓库内 `crates/xcpc-plugin/plugins/{graph-editor,math-bitwise,complexity-mem,string-lab}/` 及 `apps/desktop/src/ui/tools/builtinUi.ts` 中对应的硬编码映射；
- 宿主「工具」Tab 保持 100% 纯净插件容器，零内置业务工具代码。

### 5.2 默认预置官方仓库与「工具」空态体验
- **默认官方仓库订阅**：
  - `PluginManager` 定义默认官方仓库常量 `OFFICIAL_PLUGIN_REPO_URL`（指向官方收录仓库发布的 `index.json`），支持环境变量 `XCPC_OFFICIAL_PLUGIN_REPO` 覆写（便于本地调试与自动化验收）；
  - 首次初始化 `plugin_repos.json`（文件不存在）时默认预置官方仓库条目；用户可随时在「设置 → 插件」移除，移除后不再自动写回。
- **「工具」Tab 空态推荐安装**：
  - 当已启用的 `tool` 插件为空时，「工具」空态不显示干瘪空白页：
    - 异步拉取已订阅仓库（默认含官方仓库）中含 `tool` 扩展点的插件列表，直接展示官方推荐工具卡片（名称、简述、版本、一键「安装」按钮）；
    - 点击安装仍走 Core 的 SHA-256 校验与既有信任确认（含 `ui.js` 时明示非沙箱桌面界面边界），安装完成后原地切入刚装好的工具，无需手动跳转；
    - 离线或未订阅任何仓库时优雅回退：说明当前无已安装工具，提供前往「设置 → 插件」添加仓库或安装本地包的入口。

### 5.3 独立官方插件收录仓库结构与 CI
独立维护官方插件收录仓库（如 `xcpc-plugins`），承担双重角色：**官方工具插件源码托管** + **社区插件元数据收录**：
- **官方插件（源码托管）**：
  - `plugins/graph-editor/`、`plugins/math-bitwise/`、`plugins/complexity-mem/`、`plugins/string-lab/`：4 个官方工具全部基于 `@xcpc/plugin-sdk` + Vite + TSX 重构，由官方 CI 统一构建发布。
- **社区 PR 收录（只收 repo 链接与元数据，不收第三方源码）**：
  - `registry.toml`：社区作者通过 PR 登记 `{ id, repo, version/tag, zip_url, sha256 }`（类似 Obsidian `community-plugins.json`，锁版本与 SHA-256 防止上游静默篡改）；CI 自动拉取 `.zip` 并执行 `xcpc plugin check` 校验与哈希比对，通过后合入官方 `index.json`（并清晰标明第三方来源）；
  - `README.md` / `community-repos.md`：可选收录社区自建第三方源（`index.json` URL），供用户按需在「设置 → 插件」自行订阅，官方不背书。
- **自动化发布流水线（GitHub Actions）**：
  - 官方插件执行 `vite build` → `xcpc plugin check dist` → `xcpc plugin pack dist`；
  - 社区登记条目执行下载校验（SHA-256 + `xcpc plugin check`）；
  - 自动生成汇总 `index.json`（含插件 `id`、`name`、`version`、`description`、`capabilities`、`has_ui`、`download_url`、`sha256`、`official` 标识）并发布至 GitHub Releases / Pages。

---

## 6. 实施顺序与验收清单

按以下三步推进，每步独立可测可验收：

### Step 1：宿主 SDK 新原语、Core KV Bridge、软链安全模型与 `notify` 热重载
1. `crates/xcpc-plugin`：`plugin_links.json` 登记与顶层软链安全放行（子路径仍严控沙箱）、`rquickjs` 真实文件名堆栈、`OFFICIAL_PLUGIN_REPO_URL` + `XCPC_OFFICIAL_PLUGIN_REPO` 默认预置逻辑。
2. `apps/desktop`（Rust + TS）：
   - 暴露同步 KV Bridge 给 `pluginSdk.tsx` 实现 `sdk.storage`；
   - 补齐 `ui.{Each,Row,Col,Card,Switch,Select,Badge}`、`sdk.canvas.fitBounds`、`sdk.useTool`、`sdk.onCleanup` 与 `//# sourceURL=` 注入；
   - 接入 `notify` 监听 `<dataDir>/plugins/` 与已链接目标目录，实现热重载并触发旧实例 `onCleanup`；
   - 「工具」空态支持拉取并一键安装官方推荐工具，「设置 → 插件」标出开发链接标识。

### Step 2：`packages/plugin-sdk`（含 Vite 插件）与 `xcpc plugin` CLI 工具链
1. 落地 `packages/plugin-sdk`（`/host`、`/ui`、`/vite`、`manifest.schema.json`）及单元测试（含 QuickJS 禁用模块拦截测试与 TSX 编译测试）。
2. 落地 `xcpc plugin init --vite`、`link` / `unlink`、`check`、`pack` 及其集成测试。

### Step 3：迁出 4 个官方工具插件至独立官方收录仓库并用新 SDK 重构验收
1. 建立独立官方插件收录仓库结构、CI 自动构建与 `index.json` 生成脚本，用 `@xcpc/plugin-sdk` + Vite + TSX 重构 `graph-editor`、`math-bitwise`、`complexity-mem`、`string-lab` 并全部通过 `xcpc plugin check` 与 `pack`。
2. 主仓库移除 4 个内置工具（保留 `codeforces`、`atcoder`），通过 `XCPC_OFFICIAL_PLUGIN_REPO` 指向本地生成的 `index.json` 在 `linux-cua` 实机验收完整闭环：
   - 全新空配置启动 → 「工具」空态直接展示 4 个官方工具 → 一键安装并交互验收（画布拖拽、位运算、复杂度/内存、字符串）；
   - `xcpc plugin init --vite` → `xcpc plugin link` → `vite build --watch` 修改一行 TSX → 桌面端免重启原地热重载且 `sdk.storage` 保持状态。
