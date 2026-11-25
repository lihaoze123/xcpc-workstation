# Phase 4 规格：插件体系、个人训练看板与选手工具箱

2026-11-24 深度对齐确认（候选 Phase 3E 排到远期）。遵循「**插件驱动扩展 + 数据看板驱动 + 状态归日志、笔记归题解 + 离线实战工具箱**」原则，按 **Phase 4A → 4B → 4C** 三步落地。

## 1. 原则与边界

### 核心设计原则
1. **双层插件体系（Core 沙箱 + 桌面 UI 信任模型）**：
   - **Core 层（`index.js`，GUI / CLI / MCP 共用）**：Rust Core 内嵌 QuickJS 沙箱运行，宿主提供域名白名单 HTTP、HTML 选择器、系统钥匙串凭据托管、Rust 高性能渲染/分词原语（`highlight` / `svgToRgba` / `typstToRgba`）与可选 `workspace_read` 受控只读查询。官方 `codeforces` / `atcoder` 同步器自身即作为预装插件运行在这套协议上。
   - **桌面 UI 层（可选 `ui.js`，仅桌面端，Obsidian / VS Code 信任模型）**：允许需要 120fps 实时交互画布或高度自定义界面的工具插件直接加载桌面端 JS 模块并调用 NSH 组件、`<Canvas>`、主题 Token 与 Rust 渲染 Hook（安装/启用时显式标明「包含非沙箱桌面界面代码，仅安装信任来源」）。
2. **数据看板驱动**：借鉴 `lihaoze123/cptracker`，打开「训练」直接落地在高密度量化看板，聚焦**每日题数 / 每日最高难度双模式热力图**、**跨平台难度统一换算**、**有效难度突破趋势（P75/P90）**、**Rating 曲线**、**高难标签短板**与**近期比赛日历**。
3. **只记状态与轻量打卡，笔记归题解**：本地训练记录（`training/logs.jsonl`）支持工作区题目状态打卡（`todo` / `attempted` / `ac` / `upsolved` / `clear`）与独立手工练题记录（标题、可选链接/平台/难度/标签/简注），与外部 OJ 同步合并去重后汇入看板；思路、错因与总结直接写进题目自己的 `editorial.md`，并通过训练空间的「题解」Tab 聚合浏览，绝不造第二套笔记或复盘系统。
4. **全局共享与工作区隔离**：插件目录（`<dataDir>/plugins/`）、账号配置（每平台单 handle）、凭据与同步缓存（`<dataDir>/oj_sync.db`）存放在用户全局目录，多工作区共享；`training/logs.jsonl` 留在工作区进 Git；训练状态**永不写入 `problem.toml`**。工作区在 `workspace.toml` 中设 `training = false` 时隐藏「训练」入口与题库状态列，多人出题仓零打扰。
5. **完全应用内收题 + 纯净插件化工具箱**：不做 Competitive Companion 端口监听，改为应用内「从同步记录一键收入题库」与「按 URL 快速新建题目」；左侧导航新增常驻「工具」Tab 作为纯净的工具插件容器，4 大官方离线工具全部以「内置默认插件」形态交付（与 `codeforces`/`atcoder` 同机制，可禁用、可导出本地副本魔改），全面狗粮化验证双层 Plugin SDK。

### 明确不做（Phase 4 范围外）
- 不做 **比赛复盘页与独立复盘笔记**（`training/reviews/*.md`）；
- 不做 **独立待补队列页**（想补的题直接收入题库标 `todo` / `attempted` 或打标签）；
- 不做 **耗时、VP 计时器、错因标签录入表单**；
- 不做 **Competitive Companion（`:10043`）端口监听**（远期按需评估）；
- 不做 **单平台多账号（小号）合并**（每平台只配 1 个主 handle）；
- 不做 **题单（`sheets/*.toml`）**；
- 不做 **看板卡片注入扩展**（插件扩展点收敛为 `oj_source`、`tool`、`commands`、`importer`/`exporter` 与 `credentials`）；
- 不做 **装饰性图表**（能力雷达图、平台环形图等）、**盲盒练题**、**间隔复习提醒**、**年度总结幻灯片**。

---

## 2. 插件系统规格（`crates/xcpc-plugin`）

### 2.1 包形态、预装覆盖与去中心化分发
- **包形态（零构建步骤）**：每个插件是一个目录（分发可打包为 `.zip`），包含 `manifest.toml` 与入口脚本（`index.js` 为 Core 沙箱入口，可选 `ui.js` 为桌面 UI 入口，两者可独立存在或协同工作；标准 JS 即可直接运行，复杂插件也可由作者自行用 TS 打包为单文件）。
- **内置默认 + 本地同名覆盖**：官方预装插件（4A 预装 `codeforces`、`atcoder`；4C 预装 4 个离线工具插件 `graph-editor`、`math-bitwise`、`complexity-mem`、`string-lab`）内嵌于二进制并随软件自动更新；用户点「导出本地副本」或手动放入 `<dataDir>/plugins/<id>/` 时，本地目录优先覆盖内置版本，且均可一键禁用。
- **安装与去中心化仓库订阅**：
  - 支持本地选目录或 `.zip` 安装（ZIP 解压强制 Zip Slip 路径穿越防护）；
  - 支持在「设置 → 插件」添加任意社区仓库 URL（标准 `index.json`，类似 Mihon / Legado 去中心化源，应用本身不运营中心商店），列表展示可用插件、安装前展示权限摘要（若含 `ui.js` 显式警示非沙箱信任提示）、按 SHA-256 校验包体后一键安装与检查更新。

### 2.2 沙箱、宿主原语与安全边界
- **Core 层（`index.js`，Capability-based 沙箱）**：
  - **硬边界**：禁止任意文件系统访问、禁止启动子进程、单次调用强制超时（默认 30s）与内存上限（默认 64MB）。
  - **受控宿主 API（`host.*`）**：
    - `host.http.fetch(req)`：强制校验目标主机命中 `manifest.toml` 的 `domains` 白名单，带超时与限流保护；
    - `host.credentials.get(key)`：从系统钥匙串（`keyring`）读取该插件专属凭据，且**仅允许附加到发往该插件白名单域名的请求中**；
    - `host.html.select(html, cssSelector)`：Rust 侧轻量 HTML 解析器，便于社区编写网页爬取规则；
    - `host.highlight(code, lang)` 与 `host.render.svgToPng(svg, scale)` / `svgToRgba`（复用仓库已有 `resvg` + `tiny-skia` + 内嵌 JetBrains Mono）：让纯 JS 插件零依赖实现 CodeSnap 级毫秒渲染；
    - `host.kv`：插件专属小体积状态存储（用于增量同步游标等）；
    - `host.workspace.*`（**可选只读权限**）：仅当 `manifest.toml` 显式声明 `workspace_read = true`（安装前明示）时开放，提供受控只读查询（`listProblems` / `getProblem` 等），绝不暴露任意路径读文件，**严禁插件直接写工作区**。
- **桌面 UI 层（可选 `ui.js`，Obsidian / VS Code 信任模型）**：
  - 直接运行在桌面端 NSH JS 渲染层，通过宿主注入的 Plugin SDK 访问 NSH 基础组件、`<Canvas>`、主题 Token、剪贴板以及调用自身 Core 侧 `index.js` 和 Rust 渲染原语；
  - 适用于高交互自定义面板（如 `another_graph_editor` 级交互画板、实时调参版 CodeSnap 等）；安装/启用前向用户显式提示非沙箱信任边界。

### 2.3 开放的 5 个扩展点
1. **`oj_source`（OJ 数据源）**：
   - `verify(ctx)`（可选）→ 验证账号/凭据是否有效；
   - `syncAccount(ctx)` → 返回账号当前/最高 Rating、rank 与 Rating 历史；
   - `syncSummary(ctx)`（可选，对标 `ojhunt-lite`）→ 轻量汇总查询（仅返回过题数、提交数、可选已过题号集合与 Rating，适配 HDU / POJ / UVA / VJudge / 洛谷等无需全量逐条流水的平台）；
   - `syncSubmissions(ctx, cursor)`（可选）→ 增量返回提交列表（含 `problem_key`、标题、URL、原始难度、tags、规范化 verdict、语言、提交时间）；
   - `fetchContests(ctx)`（可选）→ 返回近期/未来比赛列表（名称、开始时间、时长、赛制、URL）；
   - `normalizeRating(raw)` → 将平台原始难度换算为统一 Rating 标尺（用于跨平台统计）；
   - `parseProblemUrl(url)` / `fetchProblem(ctx, id)`（可选）→ 解析 URL 并按需补全题目标题、难度、tags、样例或题面。
2. **`tool`（工具箱扩展）**：
   - **无头模式（`index.js`，GUI / CLI 共用）**：声明 `tools` 列表与 `inputs` schema，`runTool(toolId, inputs)` 返回结构化输出块（`text` / `markdown` / `table` / `graph` / `svg` / `image`），由宿主「工具」Tab 复用内置渲染器呈现（`svg`/`image` 支持高清直出预览、一键复制图片与保存 PNG；`graph` 支持一键送入内置图论画板继续编辑），并可在 CLI `xcpc tool run <plugin>:<tool>` 调用；
   - **桌面交互面板模式（`ui.js`，仅桌面端）**：直接注册自定义工具页面，挂载到「工具」Tab 左侧列表。
3. **`commands`（`Cmd+K` 命令面板挂载）**：
   - 插件在 `manifest.toml` 中声明轻量命令列表（ID、标题、可选上下文限制 `global | problem | contest`），统一注册进桌面端 `Cmd+K` 命令面板；触发时传入当前上下文（题目/比赛 ID），执行无头动作或打开对应工具面板，让插件自然融入日常工作流。
4. **`importer` / `exporter`（自定义格式导入导出）**：
   - **`importer`**：声明支持的文件扩展名（如 `.xml` / `.zip`），`importPackage(files)` 解析外部格式（如 FPS XML、QOJ 包、DOMjudge/Hydro 包等）并返回标准化的 `ProblemDraft` / `ContestDraft`（元数据、题面 Markdown、样例、配图与可选题目包文件），**仍由宿主 Core 执行 slug 去重、路径沙箱、Zip Slip 防护与原子落盘**；
   - **`exporter`**：声明目标格式与输出扩展名，`exportPackage(ctx)` 接收宿主提供的只读题目/比赛数据（含 `xcpc-engine` 编译的 PDF 字节与已验证测试数据清单），返回归档文件映射，**由宿主 Core 校验门禁（动静分离）并原子写入 `build/`**；接入 GUI 导入/导出菜单与 CLI `xcpc import/export`。
5. **`credentials`（凭据托管）**：
   - 在「设置 → 插件」统一提供脱敏粘贴输入框（不做内置登录 WebView），底层落盘系统钥匙串，支持一键点击「验证」调用 `verify()`。

### 2.4 插件开发与调试体验（DX）
- `xcpc plugin init <dir> [--kind oj|tool]`：生成 `manifest.toml`、`index.js` 与完整类型声明 `xcpc-plugin.d.ts`（供编辑器与 AI Agent 自动补全 `host.*` 契约）；
- `xcpc plugin test <dir> [action] [--json]`：直接运行本地目录插件，输出结构化结果与耗时，无需先打包安装；
- 桌面端「设置 → 插件」支持一键「重载本地插件」（热更新免重启）与查看插件最近一次错误堆栈。

---

## 3. 数据归属与存储布局

### 3.1 用户全局目录（`<dataDir>`，不进 Git）
- 用户设置 `[sync]`：各已启用 `oj_source` 插件的单个 handle、`auto_sync_on_startup`（默认 `true`）、上次同步时间。
- `<dataDir>/plugins/`：已安装/预装插件目录。
- `<dataDir>/oj_sync.db`（SQLite，可随时删除重建）：
  - `oj_accounts`：`platform`（PK）、`handle`、`rating`、`max_rating`、`rank`、`updated_at`；
  - `oj_rating_history`：`platform`、`contest_id`（联合 PK）、`contest_name`、`rating_before`、`rating_after`、`rank`、`at`；
  - `oj_submissions`：`platform`、`submission_id`（联合 PK）、`handle`、`contest_id`、`problem_index`、`problem_key`、`problem_title`、`problem_url`、`raw_rating`、`normalized_rating`、`tags_json`、`verdict`、`language`、`submitted_at`；
  - `oj_contests`：`platform`、`contest_id`（联合 PK）、`name`、`start_time`、`duration_sec`、`url`、`updated_at`；
  - `oj_problem_meta`：题目元信息缓存（标题、难度、URL、tags），供补全提交与按 URL 新建题目使用。

### 3.2 工作区目录（进 Git，单一事实源）
```text
xcpc-workspace/
├── workspace.toml            # 可选：training = false（默认 true）
└── training/
    └── logs.jsonl            # 追加写入的极简状态事件（每行一个 JSON）
```
- `logs.jsonl` 向后兼容两类记录（每行至少包含 `problem_id`、`source` 或 `title` 之一）：
  - 工作区题目状态打卡：`{ "at": "...", "problem_id": "cf-1999a", "source": { "platform": "codeforces", "id": "1999A" }, "status": "upsolved" }`；
  - 独立手工练题记录（无须先建题库目录）：`{ "at": "...", "status": "ac", "title": "QOJ 1234", "url": "https://...", "platform": "qoj", "rating": 2100, "tags": ["dp"], "note": "..." }`（可选字段为空时省略）。
- 运行时状态派生规则（`xcpc-domain`）：合并 `<dataDir>/oj_sync.db`（按 `[[source]]` 匹配）与 `training/logs.jsonl` 最新手动状态（仅匹配 `problem_id` 或 `source`，独立手工记录不影响无关题目）；优先级 `ac` / `upsolved` > `attempted` > `todo` > `none`（`clear` 清除手动覆盖；外部已 AC 且手动标 `upsolved` 时展示「已补」）。同时，训练看板将 `oj_sync.db` 与 `training/logs.jsonl` 合并去重后统一统计。

---

## 4. 导航布局与三步实施计划

左侧导航固定 **5 个空间**：`题库 (Cmd+1)`、`比赛 (Cmd+2)`、`训练 (Cmd+3)`、`工具 (Cmd+4)`、`设置 (Cmd+5)`。
- `workspace.toml` 设 `training = false` 时隐藏「训练」与题库训练状态列；
- 「工具」始终可用（即使在多人出题仓或未打开工作区时也能使用）；
- 「设置 → 插件」统一管理插件安装（本地目录/ZIP）、启停、OJ handle、凭据与同步策略。

### Phase 4A：插件底座 + 官方 OJ 插件 + 训练状态与题库联动
1. **`crates/xcpc-plugin`**：内嵌 QuickJS 沙箱、`manifest.toml` 解析、白名单 HTTP、HTML 选择器、`keyring` 凭据托管与插件启停/安装管理。
2. **预装官方插件**：`codeforces`（`user.info` / `user.rating` / `user.status` / `contest.list`）与 `atcoder`（Kenkoooo 提交与题目难度 + 官方 Algorithm Rating 历史 + 比赛日程 + 难度统一换算）。
3. **`crates/xcpc-sync`**：调度全部已启用 `oj_source` 插件增量落盘 `<dataDir>/oj_sync.db`（支持启动静默同步与手动同步）。
4. **训练状态与题库联动**：`training/logs.jsonl` 原子追加与运行时状态派生；题库筛选新增「训练状态」，状态列融入紧凑标记，右键/详情一键切换状态；支持新建题目时粘贴 URL 自动解析（走插件 `parseProblemUrl` + `oj_sync.db` 补全）。
5. **设置页与 CLI**：「设置 → 插件」完整管理界面；CLI 新增 `xcpc plugin list|install|enable|disable`、`xcpc sync [--platform …] [--full] [--json]`、`xcpc training status`、`xcpc problem from-url`。

### Phase 4B：高密度训练看板 + 近期比赛日历 + 提交检索与一键收题 + 题解聚合 + MCP
1. **「训练」空间四视图（`总览` | `比赛日历` | `提交` | `题解`）**，右上角常驻手动「同步」按钮与上次同步时间：
   - **`总览`**：
     - 顶部指标卡：各平台当前/最高 Rating（段位着色、`tabular-nums`）、今日/本周/本月/累计去重 AC 与提交数（合并外部 OJ 同步与 `training/logs.jsonl` 本地记录并去重）、连续过题天数（Streak），以及全局时间窗口切换（`近 30 天` / `近 90 天` / `今年` / `全部`）；
     - **双模式年度热力图**（借鉴 `cptracker`）：支持「**每日题数 / 每日最高难度**」切换 + 跨年选择器；
     - **Rating 曲线**：带经典段位背景色带，悬停显示比赛名、排名与 Rating 变化；
     - **有效难度突破趋势**：按周/月追踪 AC 难度 P75 / P90 走势，量化是否在持续打硬仗；
     - **难度分桶与高难标签短板（双栏）**：按统一 Rating 分桶的过题柱状图 + 支持按最低难度过滤（`全部 / ≥1600 / ≥1900 / ≥2200`）的标签分布，点击可钻取至「提交」页。
   - **`比赛日历`**：聚合所有已启用 `oj_source` 插件的近期/未来比赛，本地时间排期、倒计时显示、按平台筛选、一键在浏览器打开赛页。
   - **`提交`**：支持「**按提交 / 按题目去重**」切换（方便筛出 WA/TLE 未过题），合并展示外部同步记录与本地手工记录，多维筛选（平台/结果/难度/标签/搜索），支持快速录入独立手工练题记录（写入 `training/logs.jsonl`），每行支持一键「收入题库」（静默入库 + 右下角单槽 Toast「已收入题库 · 打开」直达工作室写 `editorial.md`）。
   - **`题解`**：直接扫描并聚合工作区内所有非空的 `problems/<slug>/editorial.md`（保持单一事实源），左侧按 `editorial.md` 修改时间倒序列出题目标题、难度、标签、来源与自动提取的纯文本摘要，右侧实时预览所选题目的题解排版并提供直达工作室编辑的入口。
2. **精简 3 个 MCP 工具与 CLI**（写操作受 `--read-only` 约束）：
   - `get_training_dashboard`（只读：指标、难度/标签分布、P75/P90 趋势、同步未过题与题库 `todo`/`attempted` 题）；
   - `set_training_status`（写：追加 `training/logs.jsonl`）；
   - `ingest_problem`（写：按同步记录或 URL 收入题库，自动去重并补全元信息）。
   - CLI：`xcpc training dashboard [--json]`、`xcpc training contests [--json]`、`xcpc training submissions [--by-problem] [--unsolved] [--json]`。

### Phase 4C：独立「工具」Tab（纯净插件容器 + 4 个官方预装工具插件 + `commands` & `importer`/`exporter`）
左侧导航 `工具 (Cmd+4)`（多人出题仓亦常驻可用），**宿主本身零硬编码业务小工具**，仅提供纯净的工具插件容器、基础输出渲染器（文本 / Markdown / 表格 / `svg`&`image` `resvg` 直出预览与导出 PNG / 基础只读图视图）与桌面 `ui.js` Plugin SDK：
1. **双层插件运行时与扩展点收口**：
   - 无头 `index.js` 插件：按 `inputs` schema 渲染参数表单并复用宿主基础渲染器，CLI 提供 `xcpc tool list|run`；
   - 桌面 UI `ui.js` 插件（Obsidian / VS Code 信任模型）：加载自定义 NSH 界面模块并注入 Plugin SDK（含 `<Canvas>`、主题 Token、剪贴板与 Rust 渲染 Hook）；
   - `commands` 接入桌面端 `Cmd+K` 命令面板（带当前题目/比赛上下文）；
   - `importer` / `exporter` 接入题库/比赛导入导出菜单与 CLI，保持一切落盘经由 Core 沙箱与门禁守卫。
2. **4 个官方内置默认工具插件**（全面狗粮化验证协议，默认启用、可禁用、可导出本地副本魔改）：
   - `graph-editor`（`ui.js` 标杆插件，对标 `another_graph_editor`）：文本边表 / 父数组 / 邻接表 / 几何坐标 ↔ 画布双向联动，0/1-indexed、有向/无向、权值、节点着色与重标号、拖拽与树/力导向/坐标系布局、导出 PNG/SVG（并接收无头插件 `graph` 输出一键打开）；
   - `math-bitwise`（数论与位运算工作台，`index.js` + 可选 `ui.js` 位翻转交互）：64 位按位翻转、进制互转、质因数分解、$\varphi(n)$、`gcd`/`exgcd`、快速幂与模逆元、CRT、常用竞赛模数与原根速查；
   - `complexity-mem`（复杂度与内存估算器，`index.js` 无头标杆）：估算常见复杂度 1s/2s 运算量边界与数组精确 MB 占用；
   - `string-lab`（字符串算法调试器，`index.js` 无头标杆）：一键表格对齐展示 KMP 前缀函数 $\pi$、Z 函数、Manacher 回文半径、后缀数组（SA / LCP）。

---

## 5. 验收清单

- **4A**：官方 `codeforces` / `atcoder` 插件通过 QuickJS 沙箱完成增量同步并落盘 `<dataDir>/oj_sync.db`，支持 `syncSummary` 轻量汇总模式，域名白名单拦截未授权主机请求；多工作区共享缓存；题库派生状态与按 URL 新建题目可用；`training = false` 时多人出题仓完全隐藏训练视图。
- **4B**：数千条提交下看板流畅无卡顿，双模式热力图、P75/P90 有效难度趋势、高难标签短板与点击钻取准确；比赛日历聚合正常；「提交」页按题去重与一键静默入库（Toast 打开）顺畅；3 个 MCP 工具与 CLI `--json` 有完整集成测试。
- **4C**：4 大内置工具纯离线实时响应（图论画板对标 `another_graph_editor` 双向交互）；无头 `index.js` 插件（如 CodeSnap `svg` 输出）在「工具」Tab 与 `xcpc tool run` 均可用；桌面 UI `ui.js` 插件安装前展示非沙箱信任提示并在启用后可正常渲染自定义 NSH 交互面板。
