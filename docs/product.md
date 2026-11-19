# 产品定位与功能范围

## 1. 定位

XCPC Workstation 是本地优先的 **个人 XCPC 工作站**。它**面向个人使用，不区分用户，也不做多人协作**；工作区可以放进 Git 做版本管理和备份。

- **题库是底座**：题目独立持久化，不依附于任何一场比赛，包含题面、样例、标签/难度、题解、代码。
- **比赛是题目的组合**：比赛通过「活引用 + 元数据覆写」组织题目，一键生成与参考模板版式一致的 PDF 题册和 A4 题解册；归档时可以冻结快照。
- **训练看板**：基于外部 OJ 同步数据和本地训练日志，提供热力图、能力画像、复盘与待补题队列。
- **Agent 是一等用户**：同一个 Rust 核心同时驱动桌面 GUI 和无头的 `xcpc mcp` / `xcpc --json`，详见 [architecture.md](./architecture.md#7-agent--mcp--cli)。

**优先级：出版优先。** 第一个可用版本只包含「本地题库 → PDF 出版 → Agent 可调用」这条链路，见 [roadmap.md](./roadmap.md)。

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

# Phase 5 预留命名空间（subtasks / generators / validators / checkers / solutions），当前只做解析兼容
```

一道题包含以下资产：

1. **题面**：`statement.md`（默认是 Markdown + Math），双语时为 `statement.zh.md` / `statement.en.md`。也支持 `.tex` / `.typ`，但 LaTeX 输入要到 Polygon 导入时才实现。
2. **样例**：`samples/01.in`、`samples/01.ans`……**只放在这里**，不写进题面正文，由模板渲染成样例章节。
3. **配图**：`assets/`。
4. **题解**：`editorial.md`，出版时排进 A4 题解册。
5. **代码**：`code/`，存放标程、个人 AC 代码、对拍脚本等。
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

### 2.2 训练状态（运行时派生）

训练状态**不写进任何内容文件**，在运行时由两部分合并得出：

- `training/logs.jsonl`：本地事件日志，记录手动标记、VP、补题、耗时、错因标签和复习提醒。手动修改状态也写成一条事件。
- `.xcpc/oj_sync.db`：外部 OJ 同步来的提交记录，用 `[[source]]` 中的 `platform` + `id` 匹配到题目。

合并规则是取最高状态：`ac` / `upsolved` > `attempted` > `todo`。任意一个 source 有 AC，这道题就算 AC。

## 3. 比赛（Contest）

- **活引用**：`contest.toml` 按顺序列出题目 ID 和题号（A/B/C…）。题库中修正一处错别字，所有引用该题且**未冻结**的比赛都会同步更新。
- **元数据覆写**：可以按题覆写**元数据**，包括 `title`、`limits`（时间、内存、输入输出文件名）、`show-metadata`。**题面正文、样例、配图不能覆写**；需要改动时，在题库里 fork 出一道新题，再在比赛中引用新题。
- **冻结快照**：
  - 冻结时，把每道题合并覆写后的全部源文件（`problem.toml`、`statement*.md`、`samples/`、`assets/`）复制到 `contests/<id>/snapshot/<label>/`，并保存当时导出的 PDF。之后比赛只从快照渲染。
  - 冻结后，Core 层对整场比赛强制只读，包括快照。
  - **只能由人在 GUI 或 CLI 中解冻**（`xcpc contest unfreeze`，需要二次确认），解冻后删除快照目录，恢复活引用。**MCP 不提供解冻能力。**
- **组题辅助**（Phase 2）：实时统计难度分布、标签覆盖率和预估页数；支持按条件抽题。

## 4. 个人训练与 OJ 同步（后续阶段）

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

## 7. 远期：Hull 式声明式本地 Polygon（Phase 5）

参考 [`rindag-devs/hull`](https://hull.aberter0x3f.top/)。`problem.toml` 预留了声明式命名空间（`subtasks` / `generators` / `validators` / `checkers` / `solutions`），Phase 1–4 不实现。远期计划基于 `wasmtime`（WASI/WASM 确定性沙箱）实现造数据、校验、Checker、多解法期望结果和对拍，兼容 Testlib / CPLib，并与 Hull 工程互通。
