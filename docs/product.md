# 产品定位与功能范围

## 1. 定位

XCPC Workstation 是本地优先的 **个人 XCPC 工作站**。它**面向个人使用，不区分用户，也不做多人协作**；工作区可以放进 Git 做版本管理和备份。

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
- **不做多人协作**，也不做团队训练看板。
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

1. **题面**：`statement.md`（默认是 Markdown + Math），双语时为 `statement.zh.md` / `statement.en.md`。也支持 `.tex` / `.typ`，但 LaTeX 输入要到 Polygon 导入时才实现。
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

- `training/logs.jsonl`：本地事件日志，记录手动标记、VP、补题、耗时、错因标签和复习提醒。手动修改状态也写成一条事件。
- `.xcpc/oj_sync.db`：外部 OJ 同步来的提交记录，用 `[[source]]` 中的 `platform` + `id` 匹配到题目。

合并规则是取最高状态：`ac` / `upsolved` > `attempted` > `todo`。任意一个 source 有 AC，这道题就算 AC。

## 3. 比赛（Contest）

```toml
# contests/icpc-2026-regional/contest.toml
name = "ICPC 2026 Regional"
location = "上海"             # 可选
date = "2026 年 11 月 20 日"  # 可选，页眉右侧和封面
author = "命题组"             # 可选，封面作者行，默认「出题人」
language = "chinese"          # chinese | english
show-metadata = true          # 限制表，可按题覆写
cover = true                  # 默认：不少于 2 道题时有封面
blank-pages = false           # 每题从奇数页开始，空出的偶数页写「此页故意留白」
frozen = false                # 冻结后只读，从 snapshot/ 渲染

[[problem]]
id = "cf-1999a"               # 题库中的题目 ID
label = "A"

[[problem]]
id = "my-segment-tree-beats"
label = "B"
title = "线段树"              # 以下为元数据覆写，都可省略
show-metadata = false
[problem.limits]
time = "2s"
```

- **活引用**：`contest.toml` 按顺序列出题目 ID 和题号（A/B/C…）。题库中修正一处错别字，所有引用该题且**未冻结**的比赛都会同步更新。
- **元数据覆写**：可以按题覆写**元数据**，包括 `title`、`limits`（时间、内存、输入输出文件名）、`show-metadata`。**题面正文、样例、配图不能覆写**；需要改动时，在题库里复制出一道新题（不提供专门的 fork 功能），再在比赛中引用新题。
- **题号由顺序决定**：增删和移动题目后，Core 按顺序重排为 A、B、C……并写回 `label`；超过 26 题报错。
- **冻结快照**：
  - 冻结前完整编译并跑 lint，有 error 就拒绝冻结（warning 不拦截）。
  - 冻结时，把每道题合并覆写后的全部源文件（`problem.toml`、`statement*.md`、`editorial.md`、`samples/`、`assets/`，不含题目包的程序和测试）复制到 `contests/<id>/snapshot/<label>/`，并保存当时导出的 `snapshot/<id>.pdf` 和 `snapshot/<id>-editorial.pdf`（没有题解时不生成）。快照（含 PDF）是归档物，进 Git。之后比赛只从快照渲染，`contest.toml` 里的覆写不再生效。
  - 冻结后，Core 层对整场比赛强制只读，包括快照。
  - **只能由人在 GUI 或 CLI 中解冻**（`xcpc contest unfreeze`，需要二次确认），确认时列出快照与题库当前版本有差异的题（题面已改、样例已改、题目已删除，逐文件比较字节），CLI 先打印差异再要求输入比赛 ID（`--yes` 跳过）。解冻后删除快照目录，恢复活引用。**MCP 不提供解冻能力。**
- **删除题目**：被未冻结的比赛引用时拒绝删除并列出这些比赛；只被冻结的比赛引用时允许。
- **组题辅助**（Phase 2）：每题起止页（取真实编译结果）、难度分布和标签计数。不做按条件抽题，交给 Agent 组合 `search_problems` 与 `compose_contest`。

## 4. 个人训练与 OJ 同步（Phase 4）

### 4.1 分级同步（`xcpc-sync`）

| 梯队 | 平台 | 接入方式 |
| :--- | :--- | :--- |
| 第一梯队 | **Codeforces**、**AtCoder**（官方 / Kenkoooo）、**QOJ** | 免鉴权的公开 API |
| 第二梯队 | **洛谷**、**牛客**、**LeetCode** | 可选的本地 Cookie/Token，存放在系统钥匙串（`keyring`）中；遇到风控时降级为提示手动刷新 |

### 4.2 同步记录与精选题库分层

- 海量的提交记录、Rating、参赛记录和赛程缓存只存放在 `.xcpc/oj_sync.db`，不进入 `problems/`。
- 只有主动写题解、加入题单或比赛、或用 Competitive Companion 抓题时，题目才会「收入题库」，写入 `problems/<slug>/`，同时生成对应的 `[[source]]`。

### 4.3 看板与复盘

- **总览**：各平台 Rating；今日 / 本周 / 本月 / 累计的 AC 数和提交数；提交热力图与 AC 热力图双模式切换，带跨年选择器。
- **画像**：Rating 曲线叠加、能力雷达图、平台过题环形图、难度分布、标签 AC Top 榜、近期提交时间线。
- **复盘**：每场外部比赛或本地 VP 都可以写复盘笔记，自动标出「赛时 AC / 赛后已补 / 待补」，未补的题进入待补队列。

### 4.4 题单

题单（`sheets/*.toml`）可以自建或导入，对照同步记录自动显示完成进度，也可以整份排册导出为 PDF。

## 5. 代码模板册（TRD）

见 [statement-style.md](./statement-style.md#5-代码模板册trd后续阶段)。

## 6. 赛事包与 OJ 生态（后续阶段）

| 平台 / 格式 | 导入 | 导出 | 核心内容 |
| :--- | :---: | :---: | :--- |
| **Codeforces Polygon（ZIP）** | 完整 | — | `contest.xml`/`problem.xml`、多语言 `statement.tex`、样例、配图、限制、交互题标记 |
| **Competitive Companion（`:10043`）** | 实时抓取 | — | 浏览器插件一键把题目元数据、样例、限制和链接收入题库 |
| **DOMjudge（ICPC Package）** | 支持 | 支持 | `problem.yaml`、`problem_statement/`、`data/sample/` |
| **Hydro OJ** | 支持 | 支持 | `problem.yaml`、`problem_zh/en.md`、样例与附加文件 |
| **FPS（XML）** | 支持 | 支持 | 题面、样例、Base64 图片、标程 |
| **QOJ / 通用 Markdown 包** | 支持 | 支持 | Markdown 题面 + 题解 + 静态资源 |
| **PDF** | — | 完整 | 整本题册（封面、题表、补白）、单题、A4 题解册、TRD |

## 7. 本地 Polygon：可复现的题目包（Phase 3）

设计参考 [`rindag-devs/hull`](https://hull.aberter0x3f.top/)，但**不依赖 Nix，也不与 Hull 工程互通**。可复现性做到 mise 那一层：工具链按版本加哈希锁定，用到时自动下载，不追求 Nix 级的封闭构建。

### 7.1 可复现契约

**同一个 xcpc 版本、同一份 `xcpc.lock`** 下：生成的测试数据逐字节相同，validator、checker 和各解法的判定相同，**tick 数也相同**，与机器和操作系统无关。

- 所有程序用锁定的 wasi-sdk 编译为 WASI p1 的 wasm32，在 xcpc 内嵌的 wasmtime 中运行：时钟固定为 0，没有真实随机源。
- **时间只用 tick**（wasmtime fuel），不测真实耗时。tick 上限 = `limits.time` × 1e10（与 Hull 相同），内存上限 = `limits.memory`（线性内存，栈与堆共享）。构建报告把 tick 换算成「≈ 秒」展示，用于判断标程占时限的比例。
- 跨 xcpc 版本升级 wasmtime 后 fuel 计价可能变化，构建报告里记录运行器的计价版本，漂移时给提示，不做兼容。
- 比赛对 `limits` 的覆写只影响题面，不影响题目包的判定。

### 7.2 工具链

- `Toolchain`：名字 + 版本 + 各平台 url/sha256 + 解包规则，锁在**工作区级**的 `xcpc.lock`，下载到 `~/.cache/xcpc/toolchains/`。第一次用到时，如果没有锁文件，就写入当前 xcpc 推荐的版本。
- `Language`：用某个工具链把源码变成「wasm 模块 + 预打开目录」。现在只有 C/C++（wasi-sdk 加锁定版本的 `testlib.h`）。语言标准由后缀决定（`.17.cpp` / `.20.cpp` / `.23.cpp`，默认 C++20），固定 `-O2`。工具链提供 `bits/stdc++.h` shim；影子栈大小等于内存上限；开启 wasm 异常。
- wasm32 是 ILP32，`long` 只有 32 位。这一点不消除：构建时打开转换警告，lint 对单独的 `long` 给 warning。
- 以后加 Python（CPython 的 wasi 构建）时，只加一个 `Language` 和一个工具链条目，再给时限乘一个语言倍率。选手本机的开发调试环境（g++、clangd、gdb）也可以复用 `Toolchain` 层，但不在可复现契约之内（见路线图「待定」）。

### 7.3 题目包

- **路径约定**：`checker.cpp`（缺省时用内置的逐 token 比较）、`validator.cpp`、`interactor.cpp`（有它就是交互题）、`generators/<name>.cpp`、`solutions/<name>.cpp`、`tests/<name>.in`（手写测试）。
- **`problem.toml` 只声明** `[[test]]`（`gen` + `args` 或 `file`，不支持循环和模板；`sample = true` 的测试构建后写回 `samples/`）和 `[solutions.<name>]`（`main = true` 表示标程；`expect` 是整题结论或结论列表）。**没有子任务和部分分**：整题结论取按测试顺序第一个非 AC 的结论，checker 返回部分分按 FAIL 处理。
- **协议按 testlib**：checker 用 `checker <in> <out> <ans>` 加退出码；validator 只看退出码；generator 的随机数由参数决定。不专门支持 CPLib，也不做结构化输出。
- **交互**：只支持 stdio 交互，interactor 按 testlib 的 `interactor <in> <out>` 约定。选手程序和 interactor 在同一个运行器里通过两条有界管道连接，按 fuel 让出、轮转调度，所以交互过程逐位可复现；双方互相等待判为 TLE。tick 和内存上限只算选手程序。不做文件交互、通信题、提交答案题和自定义 judger。

### 7.4 操作

- **构建** `xcpc package build <id>`：编译 → 生成 → validator 校验全部测试（含样例）→ 标程生成 `.ans` → 全部解法跑全部测试 → 对照 `expect`。任何一步不符合预期都算失败。标程最大 tick 超过时限的 50% 时给 warning。
- **对拍** `xcpc package stress <id> --gen <name> --args "…" [--solutions a,b]`：把递增的种子追加为 generator 的最后一个参数，与标程比较，不一致时写出 `tests/stress-<n>.in`。
- 产物放在 `build/problems/<slug>/`（测试数据、wasm、报告），不进 Git。缓存按内容哈希放在 `~/.cache/xcpc/`，编译、生成、运行三级缓存，跨工作区共享。
- GUI：出版工作室单题上下文有第 4 个 Tab「构建报告」（行是测试，列是解法，单元格是判定和 tick），只在有 `solutions/` 时出现；失败汇入 Preflight 徽章。对拍只走 CLI 和 MCP。
- **Polygon 导入**和本地 Polygon 同期做：testlib 程序落到约定路径，测试脚本展开成 `[[test]]`，解法 tag 映射为 `expect`，题面 LaTeX 用 `tylax` 转换。导入后跑一次构建即可验证。
