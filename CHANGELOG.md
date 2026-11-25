# 更新日志

所有值得注意的变更都会记录在本文件中。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [0.4.0-beta.4] - 2026-11-25

### 新增

- **QuickJS `host.http.fetch` 二进制 Base64 响应**：新增可选 `responseType: "text" | "base64"` 参数与响应字段 `body_base64`（受域名白名单、逐跳重定向校验与 5 MiB 体积上限约束），支持插件在沙箱内安全拉取远程头像等图片资源。
- **WebView 已绑定 OJ 账号只读桥接**：`window.xcpc.oj` 新增 `listAccounts()`，供纯 Web 插件一键读取工作站已绑定的 OJ 账号（平台、Handle 与缓存 Rating/Rank），不暴露凭据与提交记录；同步更新 `@xcpc/plugin-sdk/{host,ui}` 类型定义与 `docs/plugin-development.md`。

## [0.4.0-beta.3] - 2026-11-25

### 新增

- **WebView 插件图片剪贴板桥接**：`window.xcpc.clipboard` 新增 `writeImage(pngBase64)`（同步更新 `@xcpc/plugin-sdk/ui` 类型与 `docs/plugin-development.md`），支持纯 PNG Base64 与 `data:image/...;base64,...` 数据 URL，严格拦截非图片协议且继续禁止读取剪贴板。

### 修复

- 修复在「工具」空间切换两个不同的纯 Web `<WebView>` 插件（如 `CodeSnap`、`图论与几何画板`、`数论与位运算工作台`）时，因缺少按 `plugin_id` 键控重挂载导致 WebView 停留在前一个插件页面的问题。
- 修复 `<PluginWebView>` 内部未直接订阅 `pluginRevisionFor(pluginId)` 导致单个 WebView 插件热重载不触发刷新的问题。

## [0.4.0-beta.2] - 2026-11-25

### 修复

- 恢复 `.cargo/config.toml` 中 `[profile.dev.package."*"]` 的 `opt-level = 2`（保留 `debug = 0`），修复 `nsh dev` 模式下因 `nsh-*`、`taffy`、`ropey`、`rustybuzz`、`pulldown-cmark`、`kurbo` 等依赖退化为无优化构建导致的编辑器输入卡顿与 Typst/CeTZ 预览缓慢问题。

### 文档

- 补齐中英双语 README 的 Phase 4 功能介绍与「训练 / 工具」界面截图，新增完整插件开发指南（`docs/plugin-development.md`）、SDK 文档与防漂移校验脚本（`scripts/check-plugin-docs.py`），以及面向 AI Agent 的插件开发 Skill（`skills/xcpc-plugin-creator/SKILL.md`）。

## [0.4.0-beta.1] - 2026-11-25

Phase 4（4A–4D）首个预览版：在题库、出版与本地 Polygon 之外补齐**混合沙箱插件引擎、个人训练看板、纯净 WebView 插件化工具箱与插件开发生态**。

### 新增

- **混合沙箱插件底座与安全模型**（`xcpc-plugin`）：
  - Rust Core 内嵌 `rquickjs` 沙箱（64 MiB 内存上限、2 秒单次超时、域名白名单逐跳重定向校验、系统 `keyring` 凭据托管、插件作用域 KV 存储），支持 `oj_source`、`tool`、`commands`、`importer`、`exporter` 与 `credentials` 扩展点。
  - **Core 层强制信任门（`pending_trust`）**：声明了外网域名、凭据或 `workspace_read` 权限的外部插件在所有安装路径（目录/ZIP、仓库、CLI、开发链接）安装后默认处于「待信任」并拒绝执行，需经 GUI 确认弹窗或 `xcpc plugin trust <id>` 放行；持久化来源仓库（`source_repo`），跨仓库覆写或权限扩大自动重置信任。
  - **纯 Web `<WebView>` 沙箱插件界面（`ui_entry = "ui.html"`）**：采用宿主受控外层壳 + `<iframe sandbox="allow-scripts" srcdoc>` + 每次挂载随机桥接令牌 + 严格 CSP（`default-src 'none'; connect-src 'none'; form-action 'none'; base-uri 'none'`）+ NSH 原生 `capabilities.webview.hosts = []` 禁导航，通过 `window.xcpc` 提供 `runTool`、`storage`、`workspace`、`toast`、`clipboard.writeText`、`files`、`openTool` 与实时主题 CSS 变量（`--xcpc-*`），彻底隔绝外联泄露与主线程卡顿。
  - 仓库订阅强制 HTTPS（仅豁免本地回环地址），下载校验 SHA-256 与 Zip Slip / 软链越界防护，自动跟随 GitHub Release 302 重定向并逐跳校验 HTTPS。
- **个人训练看板与 OJ 同步**（`xcpc-sync`）：
  - 内置 **Codeforces** 与 **AtCoder**（Algorithm Rating）官方 OJ 插件，全局 `<dataDir>/oj_sync.db` 跨工作区共享，工作区 `training/logs.jsonl` 仅记录状态变更（不污染 `problem.toml`）；支持 `workspace.toml` 配置 `training = false`。
  - 「训练」空间四视图：**总览**（多平台 Rating、双模式年度热力图、段位色带 Rating 曲线、有效难度 P75/P90 突破趋势、难度分桶与高难标签短板）、**比赛日历**、**提交**（按提交/按题去重 + 一键收入题库 + 居中防跳动「手动记一题」弹窗）与**题解**（聚合工作区全部非空 `editorial.md` 与实时排版预览）。
  - 新增 `get_training_dashboard`、`set_training_status`、`ingest_problem` MCP 工具与对应 CLI 命令。
- **纯净「工具」Tab 与独立官方插件仓库（`xcpc-plugins`）**：
  - 主仓库仅内置 2 个核心 OJ 插件（构建零 Node 依赖）；4 个官方离线工具（图论与几何画板、数论与位运算工作台、复杂度与内存估算、字符串算法调试器）迁出至独立官方仓库 [`xcpc-plugins`](https://github.com/lihaoze123/xcpc-plugins)，默认预置订阅并在「工具」空态与仓库浏览器中一键安装。
  - `math-bitwise` 与 `graph-editor` 升级为纯 Web 单文件 `ui.html`（毫秒级位格翻转、原生 60fps Canvas2D、改边表保留既有节点坐标）。
- **Plugin SDK 与开发工具链**（`@xcpc/plugin-sdk` v0.2.0）：
  - 新增 `packages/plugin-sdk`（导出 `/host`、`/ui`、`/vite` 与 `manifest.schema.json`），`@xcpc/plugin-sdk/vite` 支持单文件 IIFE `dist/ui.html` + `dist/index.js` 双入口打包与 QuickJS 禁用 API 编译期护栏。
  - 升级 `xcpc plugin` CLI：`init [--kind tool|oj] [--vite]`、`install <path-or-id>`、`uninstall`、`trust`、`link` / `unlink`、`check`、`pack`、`test` 与 `repo list|add|remove|catalog`；桌面端按插件 ID 粒度监听文件变更实现免重启热重载（兼容 Linux 与 Windows）。

### 变更

- 左侧导航升级为 5 项：`题库`、`比赛`、`训练`、`工具`、`设置`；「设置 → 插件」统一管理 OJ 账号、凭据、插件安装/卸载/恢复内置版本与仓库源。
- `xcpc-engine` 支持 `XCPC_SKIP_EMBED_FONTS=1` 便于独立 CI 构建 CLI；`xcpc-plugins` CI 自动构建并原子发布 `official-latest` Release（含全部插件 zip、`index.json` 与 `xcpc-plugin-sdk.tgz`）。

### 修复

- 修复插件文件监听器误响应 `Access` / `Metadata` 事件导致的自激热重载循环与输入回弹。
- 修复仓库拉取失败时的中文 locale / JSON 解析错误提示与重复渲染，以及无头测试对系统 locale / 全局 `<dataDir>` 的依赖。

## [0.3.0-beta.1] - 2026-11-24

Phase 3「本地 Polygon」首个预览版：在出版工作台之上补齐出题、验题与 OJ 数据包流转，并完成一轮以 release 实测为准的流畅性优化。

### 新增

- **本地题目包与检查引擎**（`xcpc-judge`）：
  - 原生题目包：`solutions/`、`generators/`、`tests/`、`validator.cpp`、`checker.cpp`、`interactor.cpp` 与 `problem.toml` 声明式 `[[test]]`（生成器 + 参数、`repeat`、标签）和 `[solutions.*]`（标程与预期结论）。
  - 一键检查：编译 → 校验器自测 → 生成数据 → 校验 → 标程出答案 → 全解法评测，结论按预期核对（AC / WA / TLE / MLE / RE），附安全边际摘要与单步复现命令；支持逐格流式进度、按测试点 / 解法子集局部运行与严格验收（`--strict`）。
  - 交互题与多轮通信、OI 子任务计分与子任务自测；对拍（`xcpc package stress`）自动保存反例；ASan/UBSan 未定义行为体检（`--sanitize`）。
  - 整场就绪审计（`xcpc contest audit`）与外部验题者提交试跑（`verify-external`，不改动工作区），以及按比赛覆写的时限检查（`--contest`）。
- **Polygon 导入与 OJ 数据包导出**（`xcpc-packages`）：
  - Polygon ZIP 单题与整场导入（`contest.xml` 展开子题并自动归组），题面一次性转为 Markdown，样例、配图、校验器、检查器、生成器与解法全量导入；含 Zip Slip 防护与原子暂存。
  - DOMjudge / Hydro 单题与整场批量导出，严格剔除解法、生成器与题解，并以最新严格检查通过作为导出门禁。
- **桌面端出题工作室**：
  - 「题面 / 出题」并列模式：左侧对象树（解法、生成器、测试数据、校验与评测），右侧测试点 × 解法结果矩阵，可用键盘浏览用例并查看输入、答案与实际输出；支持轻量单文件快编与数据增删。
  - 比赛工作室内就地出题与检查，大纲同步显示每题状态；统一状态标记，出错时原位说明，并提前告知缺少编译器。
- **题库升级**：持久化分组、多选与批量操作（归组、标签、难度、加入比赛、删除、构建、导出），表格重构与 `Ctrl+K` 命令面板配额。
- **CLI 与 MCP**：新增 `xcpc package`（`init` / `inspect` / `build` / `stress` / `verify-external` / `adopt`）、`xcpc import` / `xcpc export`、`xcpc group`、`xcpc search` / `show` 与批量子命令，对应 MCP 工具同步补齐；内建出题工作流 Skill `xcpc-problem-creator`；CLI 输出与检查报告统一中文词表。
- **离线性能 Trace**：「设置 → 关于 → 性能 Trace」或 `XCPC_TRACE=1` 录制 Rust + 界面统一时间轴并导出 Chrome Trace（可拖入 Perfetto），配套 `scripts/trace-analyze.py`、`scripts/nsh-perf.sh` 卡顿归因与 `scripts/perf-gate.sh` 渲染性能门禁。

### 变更

- **流畅性（release 实测，15 题比赛）**：
  - 大纲切题拆成两次提交，点击到高亮 < 1ms；编辑器按文件保留最近 8 个实例，切回已打开的题约 2ms，并保留各自的撤销、光标与滚动。
  - 预览改为视口页优先 + 相邻页后台预热，并在输入期间按节奏合并编译、取消过期结果。
  - 检查结束时结果先上屏，包刷新合批并延后一帧（结果帧 48ms → 16ms）。
- **反馈规范**：后台行为成功静默；用户主动触发的短动作原位瞬态确认，长耗时的检查与导出改为单槽右下角提示。

### 修复

- 单题预览与导出不再出现题号前缀与「题面预览」页眉。
- 修复大数据单行预览、错解校验过滤与 MLE 归类；Linux 上子进程内存按进程隔离计量。
- 按 URL 解析 Polygon 比赛子题；Hydro 导出改为内嵌 PDF 题面。

## [0.2.0] - 2026-11-23

首个正式版本，整合完整 Phase 2（组卷、出版工作室、题库中心、校验与 MCP 工具集）与 Phase 2.5（产品化打磨、开放模板体系、国际化与多人协同工作区）。

### 核心亮点

- **出版工作室与实时排版**：
  - 内嵌 Typst 引擎 + 120Hz HiDPI 零闪白分块渲染、冷启动快照即时恢复与 `Ctrl+点击` 预览定位源码。
  - **题面 / 样例 / 题解**三栏编辑、`/` 斜杠命令菜单、居中素材库 Gallery、```` ```typst ```` 原始块透传、编辑器内悬浮公式预览、专注写作模式与实用子集 Vim 模式。
  - 原生支持 Typst `@preview` / `@local` 外部库生态（内置 CeTZ、Pinit、Mannot、Oxifmt、Tiptoe 离线回退包）与中英双语内置 3 题精品示例工作区（桌面端自动跟随界面语言，CLI 支持 `xcpc init --example --lang en|zh`）。
- **开放模板体系与声明式版式定制**：
  - 去预设特权，所有模板统一为工作区可自由编辑的 `.typ` 文件，预设种子涵盖 `olymp-classic`（XCPC 经典）、`cnoi-classic`（CCF CSP/NOI 风格）与 `minimal`。
  - 单入口 `#let render(doc)` 契约与多语声明式 `#let schema`（9 种参数控件），支持三级版式参数继承、单场比赛专属模板（`contests/<id>/template.typ`）与工作区公共宏 `templates/preamble.typ`。
- **题库中心、活引用组卷与定稿归档**：
  - 开放目录单一事实源（天然适配个人题库与多人 Git 协同出题）、内存索引 + `notify` 热刷新与外部编辑冲突保护。
  - 语义级 `toml_edit` 组卷（保留注释与未知字段）、批量挑题弹窗、复制题目、元数据与版式覆写、可逆比赛归档，以及将源题面、生效模板与 PDF 一并固化的定稿快照（`snapshot/`）。
- **国际化、Agent 原生支持与跨平台分发**：
  - 桌面端全面支持 **简体中文 / English** 自动识别与实时热切换，附中英双语 README；全局用户外观主题定制归口 `<dataDir>/theme.toml`。
  - 提供完整 `xcpc` CLI（`--json`）、stdio MCP 服务（支持 `--read-only`）与 `xcpc-lint` 印刷前预检；支持 Windows x86_64（安装器 / 便携 `.zip`）与 Linux x86_64（`.deb` / Arch `.pkg.tar.zst` / `AppImage` / `.tar.gz`）多平台分发及应用内更新引导。

## [0.2.0-beta.4] - 2026-11-23

### 新增

- **桌面端与声明式模板体系国际化（i18n）**：
  - 桌面端 GUI 全面支持 **简体中文** 与 **English**，启动时自动识别操作系统语言（Linux `LC_ALL`/`LC_MESSAGES`/`LANG`/`LANGUAGE`，Windows `GetUserDefaultLocaleName`），可在 **「设置 → 外观」** 实时热切换并持久化至全局 `prefs.json`，无需重启、零闪白、不丢失编辑器状态。
  - 原生文件/目录选择对话框标题、按钮与过滤器标签随界面语言本地化；`Cmd+K` 命令面板与 `/` 斜杠命令在展示当前界面语言的同时保留中英双语与拼音检索；`/` 斜杠插入片段（提示、交互、表格、子任务表）自动跟随当前题面/比赛的出版语言。
  - 声明式模板 `#let schema` 新增多态 `LocalizedText`（同时兼容单语纯字符串与 BCP 47 多语字典及完整 6 步回退链），`olymp-classic`、`cnoi-classic`、`minimal` 三大预设模板均补齐中英双语参数声明。
- **工作区定位与多人 Git 协同出题**：
  - 明确工作区为自包含开放项目目录（支持个人题库与多人 Git 协同比赛仓库并存），个人偏好/主题与项目内容彻底解耦。
  - 工作区级 `preamble.typ` 从 `.xcpc/preamble.typ` 迁移至 `templates/preamble.typ` 以天然纳入 Git 版本管理，同时将 `preamble` 保留为非可选模板名（`list_templates` 排除、`fork`/`install` 拒绝），文件监听与定稿快照同步适配。
  - 移除工作区级 `.xcpc/theme.toml` 覆写，界面主题定制统一归口至用户全局 `<dataDir>/theme.toml`，保持 `.xcpc/` 100% 本地化。
- **英文版 README 与英文界面截图**：
  - 新增 [`README_EN.md`](./README_EN.md) 及英文版出版工作室与题库中心截图（`docs/images/studio-en.png`、`docs/images/library-en.png`）。

### 修复

- 修复英文界面下题库右侧预览区 `Statement` / `Editorial` 标签页因固定宽度过窄导致文字换行的问题。

## [0.2.0-beta.3] - 2026-11-22

### 新增

- **去预设特权、单入口模板契约与比赛级版式定制**：
  - 模板统一为工作区可自由编辑的 `.typ` 文件，预设种子（`olymp-classic`、`cnoi-classic`、`minimal`）按需落盘，无运行时内置回退特权；缺失模板时明确提示并支持一键安装。
  - 引入单入口 `#let render(doc)` 模板契约与 `/xcpc/defaults.typ` 默认钩子，支持声明式 `#let schema`（新增 `image` 选图导入控件，共 9 种参数类型）及三级版式参数继承（模板默认 → 工作区默认 → 比赛/单题覆写）。
  - 支持单场比赛专属模板（`contests/<id>/template.typ`）：一键「独立为本场模板」自动迁移已有版式参数，编辑时用系统默认关联程序打开并在保存后实时热刷新，定稿快照完整归档模板与图片资产。
  - 桌面端属性面板拆分为「信息 / 版式」双 Tab，「设置 → 排版」支持从预设添加模板、编辑与调整工作区默认版式；CLI 与 MCP 同步新增 `xcpc template add`、`add_template` 等能力。
- **复制题目、比赛批量挑题弹窗与比赛归档**：
  - 支持在题库右键菜单、属性菜单、CLI（`xcpc problem duplicate`）与 MCP（`duplicate_problem`）中原子复制整道题目（含题面、样例、配图与题解，拒绝符号链接逃逸，保留 `problem.toml` 注释与未知字段）。
  - 新增比赛「从题库挑题」批量追加弹窗（支持关键词/标签/难度/来源筛选、跨筛选保留勾选顺序、已加入置灰、26 题上限提示及 `↑`/`↓`/`Enter`/`Space`/`Cmd+Enter` 键盘操作），覆盖比赛空态、详情页、右键菜单与大纲底部 4 处入口。
  - 新增可逆的比赛归档与取消归档（与定稿正交，归档后只读保护）；左侧比赛列表底部常驻可折叠「已归档」区块，CLI（`xcpc contest archive/unarchive`）与 MCP（`archive_contest`/`unarchive_contest`）全面对齐。
- **只读安装发布页引导与更新日志展示**：
  - Linux `.deb` / Arch `.pkg.tar.zst` / `AppImage` 等只读安装检测到新版本后，主按钮显示「前往发布页」并直达对应版本的 GitHub Release 页面，不再误触发 `.tar.gz` 下载。
  - 发布流水线自动从 `CHANGELOG.md` 提取当前版本段落签入 `latest.json` 并同步至 GitHub Release 正文，桌面端「设置 → 关于」支持轻量 Markdown 更新日志滚动展示。

### 改进

- 完成全链路易用性打磨：移除「信息」Tab 残留的旧版限值开关、挑题弹窗固定高度消除搜索跳动、复制新题自动清筛选并滚动定位、只读/已归档比赛在顶栏与样例页均清晰标示原因、模板弹窗就地校验并区分重名冲突错误。

### 修复

- 修复题库长名称归属比赛标签截断后胶囊右侧出现大片留白的问题，并统一加固 `Chip` 与来源标签截断约束。
- 修复比赛编辑页左侧大纲底部统计区（`ContestStats` `compact` 模式）异常空白与多行标签溢出窗口底边的问题。
- 修复 Linux GTK4 运行时隐藏标题栏左上角残留默认 menu 图标的问题。
- 修复 `minimal` 预设模板在仅加载内置/开发字体环境下编译中文与数学公式报错 `no font could be found` 的问题。

## [0.2.0-beta.2] - 2026-11-22

### 新增

- **Typst 外部库生态支持（`@preview` / `@local`）**：
  - 集成 `typst-kit` 标准包解析与自动下载能力，复用系统 Typst 缓存目录，支持 `TYPST_PACKAGE_PATH`、`TYPST_PACKAGE_CACHE_PATH` 与 `XCPC_OFFLINE=1` 离线开关，并进程级缓存不可变 `@preview` 源码保持毫秒级预览。
  - 内嵌 `@preview/cetz:0.5.3`、`@preview/oxifmt:1.0.0`、`@preview/pinit:0.2.2`、`@preview/mannot:0.4.0` 与 `@preview/tiptoe:0.4.0` 离线回退包，开箱离线零延迟编译。
  - 外部包编译诊断自动补全 `@{namespace}/{name}:{version}` 路径并优先沿调用栈回溯至用户源码位置。
- **新建工作区多模板选择**：
  - 桌面端「新建工作区」弹窗支持选择默认模板（`XCPC 经典 olymp-classic` / `OI 风格 cnoi-classic`），并说明支持按比赛切换或自定义 `templates/*.typ`。
- **全新 3 题精品示例工作区**：
  - `crc-check`（循环冗余校验）：模块化 `figures.typ` 编译期执行模 2 除法动态生成传输对比图、异或竖式与彩色长除法竖式，配合 `mannot` 公式下划线注解与 `pinit` 字段标注。
  - `tree-beacon`（树上信标）：CeTZ 有根树矢量图（重链高亮、子树区域与点权标注）+ 题解 Pinit 树形 DP 转移拆解。
  - `typst-showcase`（Typst 排版与绘图能力展示）：一屏呈现 CeTZ 最短路图/凸包坐标系、Pinit 斜率优化公式标注、Typst 脚本编程（杨辉三角与复杂度表）及 `.xcpc/preamble.typ` 自定义宏。
  - 附 `welcome-round`（`olymp-classic`）与 `oi-mock`（`cnoi-classic`）两场比赛，直观演示活引用多模板出版。

### 改进

- **CCF CSP/NOI 风格 `cnoi-classic` 模板大幅升级**：
  - 对齐 CSP-S 2024 官方试题版式，新增纵向转置题目汇总表、提交源程序文件名与编译选项表、注意事项、每页页眉与蓝色总页数页脚、汉字着重号（限定仅试题正文）、`#0000ff` 蓝框带行号样例盒，以及专属「参考题解」封面与目录表。
- **正文表格与代码块排版**：
  - `olymp-classic` 正文 Markdown 表格升级为居中纯正学术三线表；两套模板代码块默认关闭 JetBrains Mono 编程连字以避免与数学符号混淆。
- **Linux 顶栏与文档**：
  - 移除 Linux 打包时的 `--icon` 参数，消除 NSH 隐藏标题栏左上角未对齐小图标（应用图标仍由 `.desktop` 与全套 hicolor 图标提供）。
  - 更新 `README.md` 展示截图（XCPC `olymp-classic` 工作室与 3 题题库中心）并修正快速开始 CLI 示例。

### 修复

- 修复 Arch Linux `.pkg.tar.zst` 包因 CI artifact 丢失可执行权限导致启动报错 `/opt/xcpc-workstation/: 是一个目录` 的问题。
- 修复无方正字体环境下 CI 字体回退测试失败问题。

## [0.2.0-beta.1] - 2026-11-22

首次公开发布（Public Beta），涵盖完整 Phase 2（组卷、出版工作室、题库中心、校验与 MCP 工具集）与 Phase 2.5（产品化打磨、开放定制、实用子集 Vim 与高频写作工具）。

### 新增

- **组卷与冻结（Core / CLI / MCP）**：
  - 基于 `toml_edit` 实现保留注释与未知字段的语义级组卷操作（增删题、排序自动重排 `A–Z`、元数据覆写与还原、`xcpc contest compose|override|freeze|unfreeze`）。
  - 比赛冻结时将源题面与试题册/题解册 PDF 原子写入 `snapshot/` 并转为只读；解冻仅向 GUI/CLI 开放（不向 MCP 开放）并展示题库差异。
  - 新增 `xcpc-lint`（缺失题目/配图/编译错误阻止冻结；版心溢出、样例异常、缺章节/缺题解给出警告）与 `xcpc init [--example]`。
  - 完整 9 个 stdio MCP 工具（搜索、读写题面、组卷、覆写、冻结、编译诊断、页面渲染与 PDF 导出），支持 `--read-only`。
- **题库中心与出版工作室（Desktop）**：
  - 内存索引 + `notify` 文件监听实时联动，支持多维筛选、题解与代码预览、外部编辑无闪白原地刷新与未保存冲突二选一提示。
  - 出版工作室支持自绘顶栏、大纲页码跳转、可调分栏、**题面 / 样例 / 题解**三栏编辑、上下文右侧属性面板、预览 HUD 与 `Ctrl+点击` 预览定位源码。
  - `Cmd+K` 命令面板（Frecency 排序跳转 + 上下文动作）、按 PDF 新鲜度切换的分体导出按钮、冷启动预览快照即时恢复与 120Hz HiDPI 零闪白分块渲染。
- **产品化打磨与写作增强（Phase 2.5A）**：
  - 全新极简品牌 Icon（全尺寸 hicolor PNG/SVG、symbolic SVG、Windows `.ico`）与首启品牌引导页、内置示例工作区（「打开示例」/ `xcpc init --example`）。
  - 温润物理弹簧动效（`Animated.spring`）、分组式设置页（工作区 / 外观 / 快捷键 / 关于）、全局 `Ctrl+/` 快捷键速查与原生 `nsh/updater` 应用内更新。
  - `/` 斜杠命令菜单（公式、表格、图片、代码块及 XCPC 高频块）、居中轻量素材库 Gallery、```` ```typst ```` 原始块透传与专注写作模式。
  - PDF 写入完整元数据、默认导出文件名改为中文/标题名，并集中维护三段式中文错误文案。
- **开放定制、Vim 模式与高频工具（Phase 2.5B）**：
  - 开放定制三件套：`.xcpc/preamble.typ` 自定义 Typst 宏、`templates/*.typ` 自定义模板（附预置 `cnoi-classic` CNOI/OI 风格模板）、全局/工作区 `theme.toml` 外观与字体覆写。
  - 实用子集 Vim 模式（默认关闭，设置页一键开启；支持 `NORMAL / INSERT / VISUAL / V-LINE` 徽标、全角/半角自适应块状光标、`Esc` 与 `Ctrl+[`、常用移动/编辑与 `vi/ci/di/yi` 文本对象）。
  - 编辑器内悬浮迷你公式预览（复用 `xcpc-engine` 渲染，零打扰智能触发）与一键复制「带样例的完整 Markdown」。
- **多平台分发与 CI/CD**：
  - 新增 `packaging/` 打包工程与 GitHub Actions 发布流水线：支持 Windows x86_64（Inno Setup 每用户安装器 + 便携 `.zip`）与 Linux x86_64（`.deb`、Arch `.pkg.tar.zst`、`AppImage`、`.tar.gz`），桌面包均附带 `xcpc` CLI，并自动生成签名 `latest.json` 与 `SHA256SUMS.txt`。

### 变更

- 公开发布产物默认关闭 UPX 压缩，改由各封装格式自身压缩以消除 Windows 杀软误报并降低冷启动开销。
- 编辑器等宽字体栈补全跨平台等宽中文字体回退链（`Noto Sans Mono CJK SC` / `Source Han Sans HW SC` / `Sarasa Mono SC` / `NSimSun`）。

## [0.1.0] - 2026-11-20

### 新增

- **领域模型与存储层**（`xcpc-domain`、`xcpc-storage`）：
  - 定义 `Problem`、`Contest`（活引用与仅元数据覆写）、`Limits`、`SourceRef` 与工作区配置模型。
  - 实现基于 `workspace.toml` 的工作区发现、临时文件 + rename 原子写入、符号链接逃逸检测路径沙箱，以及冻结比赛只读守卫与快照读取。
- **格式转换与出版管线**（`xcpc-convert`）：
  - 基于 `pulldown-cmark`（GFM + Math）与 `mitex` 实现 Markdown + LaTeX 公式到 Typst 的胶水转换，支持按二级标题自动识别章节、从 `samples/` 组装样例表、映射题目 `assets/` 图片，以及中文关系符（`\le`/`\ge` → `⩽`/`⩾`）间距保留。
  - 实现 `publish` 管线与 `SourceMap`，将编译错误和版心溢出警告准确映射回工作区源文件行列。
- **排版引擎与 `olymp-classic` 模板**（`xcpc-engine`）：
  - 实现基于 `typst` + `comemo` 增量编译的原生 `Engine`，提供无 span 页面/区域指纹与分层局部光栅化支持。
  - 实现对齐 `polygon-chinese-template@10f9959` 的 `olymp-classic` 题面册模板（封面、试题列表、限制表、留白页、跨页重复表头超长样例表）及 A4 题解册模板（扉页、可跳转目录、书签、单色语法高亮代码块，中文粗斜体按 ctex founder 映射方正黑体/楷体）。
  - 在 release（及 `bundled-fonts` feature）构建中直接内嵌 Latin Modern 与 JetBrains Mono（24 个 OTF 文件），运行时按 `XCPC_FONT_DIRS` → 可执行文件旁 `fonts/`（未设 `XCPC_FONT_DIRS` 时）→ 内嵌字体 → 系统字体（含中文字体回退链）加载；其中 JetBrains Mono 专用于代码块，正常题面等宽字体保持与基准一致的 `Latin Modern Mono 12`。
- **构建与发布打包**：
  - 在 `.cargo/config.toml` 中集中配置 release 体积优化（`opt-level = "z"`、`lto = "fat"`、`codegen-units = 1`、`strip = true`、`panic = "abort"`）及 Windows 静态 CRT（`+crt-static`）。
  - 在 `flake.nix` 中接入 `cargo-xwin`、`lld`、`llvm`、`upx` 与 `x86_64-pc-windows-msvc` 目标标准库；新增 `scripts/package.sh` 统一打包 Linux/Windows 桌面端与 `xcpc` CLI 并用 UPX 压缩至 `target/dist/`。
- **CLI 与 MCP 服务**（`xcpc-mcp`）：
  - 提供 `xcpc build|check|render|list|fonts` 命令，支持 `--json` 机器可读输出、`--read-only` 与 `--system-fonts`。
  - 提供 `xcpc mcp` stdio JSON-RPC 服务，暴露 `compile_and_diagnose`、`render_page_png` 与 `export_pdf` 工具。
- **Linux 桌面端**（`apps/desktop`）：
  - 基于 NSH 0.6 构建题面/题解双栏编辑与实时预览应用，支持工作区选择器、记住上次打开工作区、比赛内按题号跳转、500ms 原子保存、编辑器诊断标记、PDF 导出、可见页分块光栅化与两阶段 GPU 合成器平滑缩放。
- **回归基准与文档**：
  - 建立 `contest-en`、`contest-zh`、`contest-zh-mixed` 三套 `tectonic` 基准回归用例及 `scripts/olymp-regress.sh` 自动化比对脚本，并补齐 `examples/workspace` 示例工作区与 `docs/` 全套规格文档。
