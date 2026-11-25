# 产品定位与功能范围

## 1. 定位

XCPC Workstation 是本地优先的 **个人 XCPC 工作站**。应用本身**不设账号体系与在线协同服务**；工作区是开放的项目目录（而非个人单例），既可以作为长期的个人题库与训练工作区，也可以为多人基于 Git 协同出题的赛事单独建仓并各自打开（详见 [specs/multi-author-workspace.md](./specs/multi-author-workspace.md)）。

- **题库是底座**：题目独立持久化，不依附于任何一场比赛，包含题面、样例、标签/难度、题解、代码。
- **比赛是题目的组合**：比赛通过「活引用 + 元数据覆写」组织题目，一键生成与参考模板版式一致的 PDF 题册和 A4 题解册；归档时可以冻结快照。
- **训练看板**：基于外部 OJ 同步数据和本地训练日志，提供热力图、能力画像、复盘与待补题队列。
- **Agent 是一等用户**：同一个 Rust 核心同时驱动桌面 GUI 和无头的 `xcpc mcp` / `xcpc --json`，详见 [architecture.md](./architecture.md#7-agent--mcp--cli)。

**优先级：出版优先。** 第一个可用版本只包含「本地题库 → PDF 出版 → Agent 可调用」这条链路，见 [roadmap.md](./roadmap.md)。

**定位不等于阶段。** 品牌、长期文案、icon 寓意与内置示例内容按完整的 XCPC Workstation 定位撰写，受众首先是做题人（命题人也是或曾经是做题人）；只有功能排期按 Phase 收窄，不在品牌或长期文案上写窄。

### 明确不做

- **不迁移 v1**：从零构建，不写任何迁移逻辑。
- **不导出 LaTeX**：PDF 只通过原生 Typst 生成。
- **不做题解幻灯片**：题解只出 A4 文字版。
- **不做气球颜色**。
- **不做在线多人协作与团队训练看板**（多人出题协同通过独立 Git 工作区交由 Git 本身完成）。
- **Web 版远期待定**：当前不做，代码里也不为它预留。

## 2. 题目（Problem）

每道题一个目录 `problems/<slug>/`，**目录名就是题目 ID**。ID 是可读的 slug（如 `cf-1999a`、`my-segment-tree-beats`），由用户或导入器生成，**创建后不再改名**，因为比赛通过 ID 引用题目。

```toml
# problems/cf-1999a/problem.toml
title = "Two Sum"
rating = 800
tags = ["greedy"]
author = "…"                 # 可选

[limits]
time = "1s"
memory = "256MB"
input = ""                   # 输入文件名，空表示标准输入
output = ""

# 外部身份：可以有多个（如 CF 原题和洛谷搬运），原创题不写
[[source]]
platform = "codeforces"
id = "1999A"
url = "https://codeforces.com/problemset/problem/1999/A"

[[source]]
platform = "luogu"
id = "CF1999A"

# 单题级模板参数（可选，按模板名隔离；比赛中可由 [[problem]] 覆写）
[template-options.olymp-classic]
sample-layout = "wide"       # 可选，例：长样例改上下宽版

# 题目包（Phase 3，见 §7）：只声明路径约定推导不出来的东西
[[test]]                     # 有顺序，编号即顺序
gen = "random"               # generators/random.cpp
args = ["--n", "100000"]
[[test]]
file = "tests/handmade-1.in"

[solutions.std]              # solutions/std.cpp
main = true                  # 用它生成 .ans
[solutions.brute]
expect = ["tle", "ac"]       # 整题结论之一：ac / wa / tle / mle / re / any
```

一道题包含以下资产：

1. **题面**：`statement.md`（默认是 Markdown + Math），双语时为 `statement.zh.md` / `statement.en.md`。Polygon 导入的 LaTeX 题面会在导入时一次性转换为 `.md`（复杂 LaTeX 环境经 `tylax` 转为 ```` ```typst ```` 原始块），保持 `.md` 为工作区单一事实源。
2. **样例**：`samples/01.in`、`samples/01.ans`……**只放在这里**，不写进题面正文，由模板渲染成样例章节。
3. **配图**：`assets/`。
4. **题解**：`editorial.md`，出版时排进 A4 题解册。
5. **题目包**（Phase 3）：`solutions/`、`generators/`、`tests/`、`checker.cpp`、`validator.cpp`、`interactor.cpp`，见 §7。
6. **元数据**：`problem.toml`，内容见上面的示例。**这里不存训练状态。**

### 2.1 题面结构

题面是单个 Markdown 文件，用**约定的二级标题划分章节**，按标题文字识别（中英文都可以）：

```markdown
给定一个长度为 $n$ 的数组……        ← 第一个标题之前的内容是题目描述

## 输入                              ← 也可以写 输入格式 / Input
## 输出                              ← 输出格式 / Output
## 交互                              ← Interaction
## 提示                              ← Note / Notes
```

识别出的章节会映射为 `olymp-classic` 的关键词（如 `输入格式`）。无法识别的二级标题按普通小节原样输出。样例章节由模板根据 `samples/` 自动生成。

- **识别的标题**（去掉末尾冒号，英文不分大小写）：`输入`/`输入格式`/`Input`、`输出`/`输出格式`/`Output`、`交互`/`交互协议`/`Interaction`、`提示`/`说明`/`备注`/`Note(s)`/`Hint`、`样例解释`/`样例说明`/`Explanation`、`评分`/`Scoring`、`约束`/`数据范围`/`Constraints`、`背景`/`题目背景`/`Background`、`题目要求`/`Specification`、`图表说明`/`Illustration`、`子任务`/`Subtasks`。`题目描述`/`Description` 会被去掉，因为描述本来就没有标题。完整列表见 `xcpc-convert` 的 `markdown::section`。
- **样例的位置**：默认放在输入、输出、交互这几节之后，下一个其他章节之前；题面里没有这几节时放在最后。需要放在别处时写一个 `## 样例`（或 `Examples`）标题，样例表就放在那里，标题下面的正文接在表后。`samples/` 为空时不生成样例章节。
- 三级及更深的标题输出为加粗的段落。配图写相对题目目录的路径（`![](assets/a.png)`），不能指向题目目录之外，也不能是网络地址。可在图片语法后紧跟 `{width=NN%}` 属性（范围 `10%–100%`，支持小数，如 `![](assets/a.png){width=60%}`）以按版心百分比居中块级排版（`#align(center, image(..., width: NN%))`）；不带宽度属性时作为行内图片排版。

### 2.2 训练状态（运行时派生）

训练状态**不写进任何内容文件**，在运行时由两部分合并得出：

- `training/logs.jsonl`：追加写入的本地训练日志，支持两类记录：工作区题目状态打卡（`problem_id` / `source` + `todo` / `attempted` / `ac` / `upsolved` / `clear`），以及不依赖题库目录的独立手工练题记录（`title` 及可选 `url` / `platform` / `rating` / `tags` / `note`）；两者均会与外部同步记录合并去重后计入训练看板。思路与总结直接写入题目的 `editorial.md`。
- `<dataDir>/oj_sync.db`：用户全局的外部 OJ 同步记录（多工作区共享），用 `[[source]]` 中的 `platform` + `id` 匹配到题目。

合并规则是取最高状态：`ac` / `upsolved` > `attempted` > `todo`。任意一个 source 有 AC，这道题就算 AC。

## 3. 比赛（Contest）

```toml
# contests/icpc-2026-regional/contest.toml
name = "ICPC 2026 Regional"
location = "上海"             # 可选
date = "2026 年 11 月 20 日"  # 可选，页眉右侧和封面
author = "命题组"             # 可选，封面作者行，默认「出题人」
language = "chinese"          # chinese | english（决定读取 statement.zh/en.md 与限值语言）
template = "olymp-classic"    # 可选，缺省沿用 workspace.toml；若存在 contests/<id>/template.typ 则优先用专属模板
frozen = false                # 冻结后只读，从 snapshot/ 渲染
archived = false              # 可选，默认 false；归档后收纳至已归档列表并强制只读（与 frozen 正交）

# 比赛级模板参数（按模板名隔离，覆写 workspace.toml 中的默认值；cover / blank-pages / show-metadata 等均由模板 schema 声明）
[template-options.cnoi-classic]
cover = "on"
compile-opts = "-O2 -std=c++20 -static"

[[problem]]
id = "cf-1999a"               # 题库中的题目 ID
label = "A"

[[problem]]
id = "my-segment-tree-beats"
label = "B"
title = "线段树"              # 以下为元数据覆写，都可省略
[problem.limits]
time = "2s"
[problem.options.cnoi-classic]
prob-type = "交互型"          # 单题级模板参数覆写（按模板名隔离）
```

- **活引用**：`contest.toml` 按顺序列出题目 ID 和题号（A/B/C…）。题库中修正一处错别字，所有引用该题且**未冻结**的比赛都会同步更新。
- **元数据与单题版式覆写**：可以按题覆写**元数据**（`title`、`limits`）以及当前模板声明的**单题级模板参数**（`[problem.options.<template>]`，如 `show-metadata`、`sample-layout`、`prob-type`）。**题面正文、样例、配图不能覆写**；需要改动时，可在题库中通过「复制题目」（GUI / `xcpc problem duplicate` / MCP `duplicate_problem`）完整递归复制整道题目录为独立新题（不追踪血缘关系，拒绝符号链接逃逸），再在比赛中引用新题。
- **比赛专属模板**：每场比赛可在 `contests/<id>/template.typ` 放置独立模板文件（存在即优先于 `contest.toml` 的 `template`），并可在 `contests/<id>/assets/` 存放封面徽标等比赛专属资源。
- **题号由顺序决定**：增删和移动题目后，Core 按顺序重排为 A、B、C……并写回 `label`；超过 26 题报错。
- **冻结快照**：
  - 冻结前完整编译并跑 lint，有 error 就拒绝冻结（warning 不拦截）。
  - 冻结时，把每道题合并覆写后的全部源文件（`problem.toml`、`statement*.md`、`editorial.md`、`samples/`、`assets/`，不含题目包的程序和测试）、生效的 `template.typ` 副本（无论是 `contests/<id>/template.typ` 还是 `templates/<name>.typ`）、`templates/preamble.typ`（若存在）与合并解析后的模板参数一并固化到 `contests/<id>/snapshot/`，并保存当时导出的 `snapshot/<id>.pdf` 和 `snapshot/<id>-editorial.pdf`（没有题解时不生成）。快照（含 PDF）是归档物，进 Git。之后比赛只从快照渲染，不再受题库、工作区模板或外部样式修改影响。
  - 冻结后，Core 层对整场比赛强制只读，包括快照。
  - **只能由人在 GUI 或 CLI 中解冻**（`xcpc contest unfreeze`，需要二次确认），确认时列出快照与题库当前版本有差异的题（题面已改、样例已改、题目已删除，逐文件比较字节），CLI 先打印差异再要求输入比赛 ID（`--yes` 跳过）。解冻后删除快照目录，恢复活引用。**MCP 不提供解冻能力。**
- **比赛归档（Archive）**：应用内不提供「删除比赛」，而是通过 `contest.toml` 的 `archived = true` 实现可逆收纳（取消归档时移除该键）。`archived` 与 `frozen` 完全正交：未定稿或已定稿的比赛均可归档；归档后 Core 层强制拦截一切写操作并返回 `contest_archived`，仅允许「取消归档」恢复。
- **删除题目**：被未冻结的比赛（无论是否已归档）引用时拒绝删除并列出这些比赛；只被冻结的比赛引用时允许。
- **组题辅助**（Phase 2）：每题起止页（取真实编译结果）、难度分布和标签计数。不做按条件抽题，交给 Agent 组合 `search_problems` 与 `compose_contest`。

## 4. 个人训练、插件扩展与选手工具箱（Phase 4）

详细规格见 [specs/phase-4-training-and-ecosystem.md](./specs/phase-4-training-and-ecosystem.md)（2026-11-24 深度对齐确认，按 4A / 4B / 4C 三步推进；多人出题仓可通过 `workspace.toml` 中 `training = false` 隐藏训练视图）。

### 4.1 混合式沙箱插件体系（`xcpc-plugin`）

- 采用「Rust Core 内嵌 QuickJS 沙箱（`index.js`）+ 声明式表单 / 纯 Web `<WebView>` 沙箱（`ui.html`）」的混合模式（后端能力由 GUI / CLI / MCP 共用），全局存放于 `<dataDir>/plugins/`。
- 开放五类扩展点：**OJ 数据源（`oj_source`）**、**工具箱扩展（`tool`）**、**命令面板扩展（`commands`）**、**导入/导出扩展（`importer`/`exporter`）** 与 **凭据托管（`credentials`，系统钥匙串 + 域名白名单隔离）**。
- 主仓库内置 **Codeforces** 与 **AtCoder**（每平台单个 handle，含跨平台难度统一换算）作为默认插件；声明外网域名、凭据或 `workspace_read` 的外部插件由 Core 强制进入「待信任」状态，经确认后放行。

### 4.2 同步记录与精选题库分层

- 海量的提交记录、Rating、近期比赛与题目元信息存放在用户全局目录 `<dataDir>/oj_sync.db`（多工作区共享，可随时删除重建），不进入 `problems/`。
- **完全应用内收题**（不做 Competitive Companion 端口监听）：只有主动从同步记录一键「收入题库」、按 URL 快速新建、写题解或加入比赛时，题目才会写入 `problems/<slug>/`，同时生成对应的 `[[source]]`。

### 4.3 看板、比赛日历、提交检索与题解聚合（数据看板驱动）

- **总览**：各平台当前/最高 Rating；今日 / 本周 / 本月 / 累计过题与提交数（合并外部同步与 `training/logs.jsonl` 本地手工记录并按题去重）；全局时间窗口（近 30 天 / 90 天 / 今年 / 全部）；**双模式年度热力图**（每日题数 / 每日最高难度，借鉴 `cptracker`）；带段位色带的 Rating 曲线；**有效难度突破趋势**（每周/月 AC 难度 P75 / P90）；难度分桶分布与支持按最低难度过滤的高难标签短板（不做雷达图、环形图等装饰图表）。
- **比赛日历**：聚合已启用 OJ 插件的近期比赛，本地时间排期与倒计时，一键打开赛页。
- **提交**：支持「按提交 / 按题目去重」切换，筛选未过题并一键静默「收入题库」，并支持快速录入本地手工练题记录（写入 `training/logs.jsonl`）；不做独立复盘页与独立待补队列，思路与总结直接写入题目的 `editorial.md`。
- **题解**：直接聚合工作区中所有非空的 `problems/<slug>/editorial.md`（单一事实源，不额外建笔记系统），左侧按更新时间倒序列出题目、难度/标签与正文摘要，右侧提供实时排版预览并支持一键跳转工作室编辑。

### 4.4 独立「工具」Tab（纯净插件化工具箱）

左侧导航常驻 `工具 (Cmd+4)`（多人出题仓亦可用），宿主保持 100% 纯净插件容器，零硬编码业务工具。4 个官方离线工具（图论与几何画板、数论与位运算工作台、复杂度与内存估算、字符串算法调试器）托管于独立官方插件仓库 `xcpc-plugins`，桌面端默认预置订阅并支持一键安装；自定义界面采用纯 Web `ui.html` 在严格 CSP + `<iframe sandbox="allow-scripts">` 的 `<WebView>` 沙箱内运行，通过 `window.xcpc` 接入受控宿主能力。

### 4.5 题单（远期）

题单（`sheets/*.toml`）排入远期待定；当前由「比赛」与「题库分组」覆盖组织需求。

## 5. 代码模板册（TRD）

见 [statement-style.md](./statement-style.md#5-代码模板册trd后续阶段)。

## 6. 赛事包与 OJ 生态（Phase 3B 起）

| 平台 / 格式 | 导入 | 导出 | 核心内容 |
| :--- | :---: | :---: | :--- |
| **Codeforces Polygon（ZIP）** | 完整（Phase 3B） | — | 单题（`problem.xml`）与整场（`contest.xml` 展开子题并生成 `contest.toml`）；多语言 LaTeX 题面一次性转 `.md`（复杂环境经 `tylax` 转 ```` ```typst ```` 块）；全量导入样例、配图、`validator`/`checker`/`generators`/`solutions` 与自测；交互/非 C++/子任务保留文件并输出 `warnings` |
| **DOMjudge（ICPC Package）** | 后续阶段 | 支持（Phase 3B） | 对齐 `Polygon2DOMjudge` 的最小集合 ZIP（单题 / 整场 `A-<slug>.zip`…）：`domjudge-problem.ini`（无气球颜色）、`problem.yaml`、`data/sample|secret/`、`problem.pdf`、标准/自定义 `output_validators/`（Kattis 42/43 适配），严格剔除内部源码与解法 |
| **Hydro OJ** | 后续阶段 | 支持（Phase 3B） | 最小集合 ZIP（单题 / 整场批量）：`problem.yaml`、`additional_file/problem.pdf` + `problem_zh/en.md`（`@[pdf](file://problem.pdf?noDisposition=1)`）、`testdata/config.yaml` 与 `.in/.out`、自定义 `testlib` checker |
| **Competitive Companion（`:10043`）** | 远期待定 | — | Phase 4 改为完全应用内提供（从同步记录一键收题 + 按 URL 快速新建），不常驻监听本地端口 |
| **FPS（XML）** | 后续阶段 | 后续阶段 | 题面、样例、Base64 图片、标程 |
| **QOJ / 通用 Markdown 包** | 后续阶段 | 后续阶段 | Markdown 题面 + 题解 + 静态资源 |
| **PDF** | — | 完整 | 整本题册（封面、题表、补白）、单题、A4 题解册、TRD |

## 7. 本地 Polygon：原生题目包与严格验收（Phase 3）

详细规格见 [specs/local-polygon.md](./specs/local-polygon.md)（2026-11-23 调整并确认）。Phase 3 拆分为 **3A（本地传统题 XCPC 完整闭环）** 与 **3B（Polygon ZIP 导入 + DOMjudge / Hydro 最小集合导出）**；stdio 交互题（`interactor.cpp`）与 OI 子任务 / 部分分延后至后续阶段。

### 7.1 执行引擎与可复现边界

采用**本机原生 `g++` / `clang++` + 原生执行**（放弃 Wasm / fuel 路线），追求 100% 真实竞赛 C++ 兼容（`<bits/stdc++.h>`、64 位 `long`、PBDS、大栈与异常）、极速造数/对拍与贴近真实 OJ 的时限校准：

- **可复现边界**：`testlib` PRNG 保证相同参数生成的测试数据逐字节一致、validator/checker 逻辑判定一致；不强求跨机器的 CPU 耗时逐位一致。
- **轻量进程监控**：Linux 用 `rlimit` + `wait4 rusage`（CPU 时间 + 峰值 RSS）+ wall-clock 兜底强杀；Windows 用 Job Object。不做 OS 强安全沙箱（仅面向出题人本地可信代码）。
- **阶段差异化并发**：编译、数据生成、validator、checker 默认多核并行；**解法运行默认串行（`-j 1`）** 以防多核争抢导致计时失真，允许 `--jobs` 显式覆盖。
- 比赛对 `limits` 的覆写只影响题面，不影响题目包的判定。

### 7.2 工具链与语言

- 自动探测本机 `g++` / `clang++`（支持 `XCPC_CXX` 与桌面端「设置 → 工具链」覆写路径，缺失时给出平台安装指引）；不再使用 `xcpc.lock`。
- 固定 `-O2`，标准由后缀决定（`.17.cpp` / `.20.cpp` / `.23.cpp`，默认 C++20）。内置最新版 `testlib.h`（规避 GCC 15 + `-O2` 下旧版 `ncmp` 误报；工作区自带时优先）。
- Core 预留极薄的 `Language` 抽象层，Phase 3A 专注 C/C++，多语言（Python / Rust / Java 等）留待后续讨论。

### 7.3 题目包结构与声明

- **路径约定**：`checker.cpp`（可选）、`validator.cpp`、`interactor.cpp`（后续阶段：存在即交互题）、`generators/<name>.cpp`、`solutions/<name>.cpp`、`tests/*.in`（手写测试与对拍反例）、`tests/validator/{valid,invalid}/*.in`（Validator 自测）、`tests/checker/{ok,wa,pe}-*/{in,out,ans}`（自定义 Checker 自测）。
- **内置标准 Checker 家族**：`wcmp`（默认）、`ncmp`、`fcmp`、`lcmp`、`rcmp4`、`rcmp6`、`rcmp9`、`yesno`、`nyesno`（`[package].checker` 指定，存在 `checker.cpp` 时优先使用）。
- **`problem.toml` 声明**：`[[test]]`（`file` 或 `gen` + `args`，支持声明式 `repeat = N` 与展示/过滤标签 `tag`、`name`、`sample = true`）和 `[solutions.<name>]`（`main = true` 表示标程；`expect` 为结论或结论列表；可选 `tags` 过滤供暴力参考解交叉验证小数据）。Phase 3 不做子任务与部分分。
- **样例衔接**：`samples/` 默认自动作为前序测试点纳入构建（过 validator，已有手工 `.ans` 不静默覆盖，同时验证手工 `.ans` 与 `std` 均过 checker，天然兼容构造题）；仅显式标记 `sample = true` 的 `[[test]]` 写入 `samples/`。自动检测手写数据的 CRLF、UTF-8 BOM 与缺尾换行。
- **交互（后续阶段）**：只支持 stdio 交互，interactor 按 testlib `interactor <in> <out>` 约定，通过两条原生 OS pipe 并发连接，双方互相等待由超时判 TLE。

### 7.4 双档门禁、操作与 Agent 协同

- **双档门禁**：
  - 开发态（默认 `xcpc package build` / MCP `debug`）：支持 `--tests` / `--solutions` 子集局部运行（Invocation），缺 validator 只报 warning，**绝不签发整题 `verified` 状态**；
  - 严格验收态（`--strict` / MCP `verify` / 比赛冻结 / 导出 OJ 包）：强制要求 `validator.cpp` 存在且至少通过 1 组 `valid` + 1 组 `invalid` 自测、自定义 `checker.cpp` 至少通过 1 组 `OK` + 1 组 `WA` 自测、全量测试过 validator、全部解法符合 `expect`、标程最慢耗时 `≤ 50%` 时限；缺独立参考解或错解报 warning。
- **动静分离**：`xcpc check` 与比赛冻结不暗中编译跑数，仅核对最近一次验收构建报告的内容哈希（纯出版题跳过）。
- **操作**：
  - `xcpc package init <id>`：生成带 `testlib.h` 最佳实践的脚手架；
  - `xcpc package build <id> [--strict] [--tests …] [--solutions …] [--jobs N]`：编译 → 自测 → 生成 → validator → 标程 → 全解法 → 安全边际摘要与单步复现命令；
  - `xcpc package stress <id> --gen <name> --args "…" [--solutions a,b]`：递增种子对拍，反例写出 `tests/stress-<n>.in`。
- 产物放在 `build/problems/<slug>/`（不进 Git）；编译、生成、运行三级内容哈希缓存放在 `~/.cache/xcpc/`，跨工作区共享。
- **GUI**：单题与比赛工作室中选中题目时的「出题」Tab（轻量出题控制台：检查 / 运行选中、题型设置、测试×解法矩阵、点击单元格看判定与交互日志、一键脚手架、跳转外部编辑器；状态以「未检查 / 有问题 / 已通过」在状态位就地呈现）+「设置 → 工具链」。界面结构与用词以 [specs/authoring-ui-refactor.md](./specs/authoring-ui-refactor.md) 为准。
- **Agent / Skill 协同**：提供 5 个 MCP 工具（`inspect_package`、`inspect_test_case`、`init_package`、`build_package`、`stress_package`），仓库内建 `skills/xcpc-problem-creator/`（支持 `xcpc init --with-skills` 安装至工作区）并同步升级 `AGENTS.md`。
- **生态包（Phase 3B，`crates/xcpc-packages`）**：
  - **Polygon ZIP 导入**：支持单题（`problem.xml`）与整场（`contest.xml`，展开全部子题并生成 `contest.toml`）；题面一次性转为 `statement(.zh/.en).md`（复杂 LaTeX 环境经 `tylax` 转为 ```` ```typst ```` 原始块，配图入 `assets/`）；全量导入 `samples/`、声明式 `[[test]]` 生成器（手写 / `from-file` 多测 / 缺源码时回退静态 `tests/*.in`）、`validator.cpp`、`checker.cpp`（标准 `std::*` 映射到 `[package].checker`）、`solutions/*` 与自测；slug 冲突拒绝（支持指定目标 `id`），交互 / 非 C++ / 子任务保留文件并输出 `warnings`；含 Zip Slip 防护与原子暂存。
  - **DOMjudge / Hydro 最小集合 ZIP 导出**：支持单题与整场批量导出（`A-<slug>.zip`、`B-<slug>.zip`……），严格剔除 `validator.cpp`、`solutions/`、`generators/`、`editorial.md` 与 `tests/` 自测；遵守「动静分离」门禁（核对最新严格构建报告 `verified == true` 且哈希未过期，直接打包 `build/problems/<slug>/` 既有数据，CLI `--force` 仅跳过门禁）。DOMjudge 对齐 `Polygon2DOMjudge`（无气球颜色，内置 `problem.pdf`、标准 checker 映射与自定义 `output_validators/` Kattis 42/43 适配层）；Hydro 内置 `additional_file/problem.pdf` 并在 `problem_zh/en.md` 中以 `@[pdf](file://problem.pdf?noDisposition=1)` 嵌入展示，附 `testdata/config.yaml`。
  - **入口**：CLI（`xcpc import polygon`、`xcpc export <domjudge|hydro>`）、MCP（`import_polygon`、`export_oj_package`）与桌面端轻量入口（`Cmd+K`、题库/比赛顶栏「导入 Polygon」、出版工作室导出分体按钮菜单）。
