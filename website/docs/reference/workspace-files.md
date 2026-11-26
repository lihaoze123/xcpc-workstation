---
title: 配置文件速查
---

# 配置文件速查

XCPC Workstation 采用**开放目录作为单一事实源**，所有工作区配置、题目元数据、比赛编排、算法模板与团队参考手册（TRD）均以标准 TOML 文件存储，天然适合 Git 版本管理与人工审阅。

> **未知字段与注释保留策略**：Core 对 `workspace.toml`、`problem.toml`、`contest.toml`、`snippet.toml` 与 `trd.toml` 的写操作均基于语义级 `toml_edit` 原位修改并配合原子落盘（临时文件 + rename）。你在文件中手写的注释与自定义未知字段在桌面端、CLI 或 MCP 编辑后都会被完整保留。
>
> 例外：`problem.toml` 中的 `[[source]]` 条目启用了 `deny_unknown_fields`，仅接受 `platform`、`id`、`url` 三个字段。

---

## 1. `workspace.toml`（工作区根配置）

位于工作区根目录，标识工作区边界并存放工作区级共享设置（不包含个人偏好或界面主题，后者保存在用户全局 `<dataDir>/prefs.json` 与 `<dataDir>/theme.toml`）。

### 字段说明

| 字段 | 类型 | 必填 | 默认值 | 说明 |
| :--- | :--- | :--- | :--- | :--- |
| `template` | `string` | 否 | — | 工作区默认题面模板名（如 `"olymp-classic"`、`"cnoi-classic"` 或自定义 `templates/<name>.typ`）。 |
| `groups` | `string[]` | 否 | `[]` | 声明的持久化题目分组列表（按声明顺序展示）。 |
| `training` | `bool` | 否 | `true` | 是否在桌面端启用「训练」空间。 |
| `snippets` | `bool` | 否 | `true` | 是否在桌面端启用「模板」空间（算法模板与团队参考手册）。 |
| `[template-options.<tpl>]` | `table` | 否 | `{}` | 工作区级默认模板参数，按模板 ID 命名空间隔离（被比赛/单题同名键覆写）。 |

### 最小示例

```toml
template = "olymp-classic"
groups = ["区域赛选拔", "热身赛"]
training = true
snippets = true

[template-options.olymp-classic]
body-size = "11pt"
```

---

## 2. `problem.toml`（题目元数据与构建规格）

位于 `problems/<slug>/problem.toml`。题目目录名 `<slug>` 即题目 ID，创建后不改名；训练状态不写入本文件（运行时由 `training/logs.jsonl` 与全局 `<dataDir>/oj_sync.db` 合并派生）。

### 核心字段

| 字段 | 类型 | 必填 | 说明 |
| :--- | :--- | :--- | :--- |
| `title` | `string` | 是 | 题目名称。 |
| `group` | `string` | 否 | 所属持久化分组名称。 |
| `rating` | `integer` | 否 | 题目难度分（非负整数）。 |
| `tags` | `string[]` | 否 | 算法/题型标签列表。 |
| `author` | `string` | 否 | 出题人信息。 |
| `[limits]` | `table` | 否 | 时空与文件限制：`time`（如 `"1s"`）、`memory`（如 `"256MB"`）、`input`、`output`（省略或为空表示标准输入输出）。 |
| `[[source]]` | `table[]` | 否 | 外部 OJ 身份标识：`platform`（必填）、`id`（必填）、`url`（可选）。严格校验仅允许此三字段。 |
| `[template-options.<tpl>]` | `table` | 否 | 单题级模板参数覆写，按模板 ID 命名空间隔离。 |

### 题目构建与导出字段（可选）

| 字段 | 类型 | 说明 |
| :--- | :--- | :--- |
| `[package]` | `table` | `checker`（`wcmp`/`ncmp`/`fcmp`/`lcmp`/`rcmp4`/`rcmp6`/`rcmp9`/`yesno`/`nyesno` 或 `"checker.cpp"`，省略时优先使用 `checker.cpp` 否则 `wcmp`）；`runs`（多轮通信轮数，`>= 2` 启用）。 |
| `[[subtask]]` | `table[]` | OI 子任务：`id`（必填）、`score`（非负整数，必填）、`depends`（依赖子任务 ID 列表）、`scoring`（`"min"` 捆绑计分或 `"sum"` 均分，默认 `"min"`）。 |
| `[[test]]` | `table[]` | 测试点声明：手工文件 `file`（如 `"tests/hand-01.in"`）或生成器 `gen`（对应 `generators/<gen>.cpp`）+ `args`（支持 `{i}` 展开为 `1..=repeat`）、`repeat`、`tag`、`name`、`sample`（`true` 时写回 `samples/`）、`subtask`。 |
| `[solutions.<name>]` | `table` | 解法声明（对应 `solutions/<name>.cpp`）：`main`（是否为主标程）、`expect`（期望结论，如 `"AC"` 或 `["WA", "TLE"]`）、`tags`（仅在匹配标签的测试点上运行）、`score`（期望总分，整数或 `[min, max]`）、`subtasks`（按子任务 ID 映射期望结论）。 |

### 最小示例

```toml
title = "树上信标"
group = "区域赛选拔"
rating = 1900
tags = ["tree", "dp"]
author = "命题组"

[limits]
time = "1s"
memory = "256MB"

[[source]]
platform = "codeforces"
id = "1999A"
url = "https://codeforces.com/problemset/problem/1999/A"

[package]
checker = "wcmp"

[[test]]
gen = "gen_random"
args = "--n 100000 --seed {i}"
repeat = 5
tag = "max"

[solutions.std]
main = true
expect = "AC"

[solutions.brute]
expect = ["TLE", "WA"]
```

---

## 3. `contest.toml`（比赛组卷与覆写）

位于 `contests/<id>/contest.toml`。比赛采用**活引用**机制引用题库中的题目；`[[problem]]` 覆写仅允许修改元数据与单题模板选项，不覆写题面正文、样例或配图。

### 字段说明

| 字段 | 类型 | 必填 | 默认值 | 说明 |
| :--- | :--- | :--- | :--- | :--- |
| `name` | `string` | 是 | — | 比赛名称。 |
| `location` | `string` | 否 | — | 举办地点。 |
| `date` | `string` | 否 | — | 比赛日期文本。 |
| `author` | `string` | 否 | — | 命题/编排者。 |
| `language` | `string` | 否 | `"chinese"` | 出版语言：`"chinese"` 或 `"english"`。 |
| `show-metadata` | `bool` | 否 | `true` | 是否显示题目限制表（单题可覆写）。 |
| `cover` | `bool` | 否 | 题目数 `>= 2` 时为 `true` | 是否生成带试题列表的封面页。 |
| `blank-pages` | `bool` | 否 | `false` | 是否强制每道题从奇数页开始（双面打印留白页）。 |
| `template` | `string` | 否 | 继承工作区默认 | 本场比赛使用的模板名（如存在 `contests/<id>/template.typ` 则优先使用专属模板）。 |
| `[template-options.<tpl>]` | `table` | 否 | `{}` | 比赛级模板参数覆写。 |
| `frozen` | `bool` | 否 | `false` | 是否已定稿冻结；为 `true` 时只读并从 `snapshot/` 渲染。 |
| `archived` | `bool` | 否 | `false` | 是否已归档（与 `frozen` 正交；归档后只读并收入侧栏「已归档」区）。 |
| `[[problem]]` | `table[]` | 否 | `[]` | 按序排列的引用题目列表：`id`（题库 slug，必填）、`label`（题号如 `"A"`，必填）、可选覆写 `title`、`limits`、`show-metadata` 与 `options.<tpl>`。 |

### 最小示例

```toml
name = "2026 秋季选拔赛"
location = "北京"
date = "2026 年 11 月"
language = "chinese"
show-metadata = true
blank-pages = false
template = "olymp-classic"
frozen = false

[[problem]]
id = "tree-beacon"
label = "A"

[[problem]]
id = "crc-check"
label = "B"
title = "循环冗余校验（改编）"
limits = { time = "2s", memory = "512MB" }
```

---

## 4. `snippet.toml`（算法模板元数据与验证源）

位于 `snippets/<slug>/snippet.toml`。同目录包含 `code.hpp`（纯公式/结论卡可省略）、`notes.md` 与验证代码。模板间依赖关系不写入本文件，而是由 Core 从 `code.hpp` 中的 `#include "<slug>/code.hpp"`（支持 `./` 或 `../` 前缀）自动解析拓扑图。验证状态（`passed` / `stale` / `failed` / `unverified`）缓存于 `.xcpc/snippet-verify/<slug>.json`，不污染本文件。

### 字段说明

| 字段 | 类型 | 必填 | 说明 |
| :--- | :--- | :--- | :--- |
| `title` | `string` | 是 | 算法模板标题。 |
| `tags` | `string[]` | 否 | 分类标签列表。 |
| `time_complexity` | `string` | 否 | 时间复杂度说明（兼容别名 `time-complexity`）。 |
| `space_complexity` | `string` | 否 | 空间复杂度说明（兼容别名 `space-complexity`）。 |
| `description` | `string` | 否 | 简短用途说明。 |
| `source_problem` | `string` | 否 | 提取来源的工作区题目 ID（兼容别名 `source-problem`）。 |
| `[[verify]]` | `table[]` | 否 | 验证源声明，按 `kind` 分为三类：<br>- `kind = "self"`：`file`（目录内自测源文件路径，退出码 0 即通过）；<br>- `kind = "problem"`：`problem`（工作区题目 ID）+ `adapter`（适配代码路径）；<br>- `kind = "library_checker"`：`problem`（Library Checker 题目标识）+ `adapter`（适配代码路径）。 |

### 最小示例

```toml
title = "快速数论变换（NTT）"
tags = ["poly", "math"]
time_complexity = "O(n log n)"
space_complexity = "O(n)"
description = "998244353 模数下的多项式乘法"

[[verify]]
kind = "self"
file = "test.cpp"

[[verify]]
kind = "library_checker"
problem = "convolution_mod"
adapter = "verify_conv.cpp"
```

---

## 5. `trd.toml`（团队参考手册编排）

位于 `trds/<id>/trd.toml`。以活引用方式组织算法模板为章节；定稿冻结后，源文件、依赖闭包与导出 PDF 固化至 `trds/<id>/snapshot/` 并进入只读保护。

### 字段说明

| 字段 | 类型 | 必填 | 默认值 | 说明 |
| :--- | :--- | :--- | :--- | :--- |
| `name` | `string` | 是 | — | 手册名称。 |
| `author` | `string` | 否 | — | 队伍或作者名称。 |
| `date` | `string` | 否 | — | 日期文本。 |
| `template` | `string` | 否 | `"trd-compact"` | 排版模板名（内置预设种子 `trd-compact` / `trd-book`，或自定义 `templates/<name>.typ`）。 |
| `frozen` | `bool` | 否 | `false` | 是否已定稿冻结；为 `true` 时只读并从 `snapshot/` 渲染。 |
| `archived` | `bool` | 否 | `false` | 是否已归档（与 `frozen` 正交）。 |
| `[template-options.<tpl>]` | `table` | 否 | `{}` | 手册级模板参数覆写（如 `columns`、`hash-interval`、`strip-comments`）。 |
| `[[chapter]]` | `table[]` | 否 | `[]` | 有序章节列表：`title`（章节标题，必填）、`items`（算法模板 slug 列表）。 |

### 最小示例

```toml
name = "2026 ICPC 区域赛参考手册"
author = "队名"
date = "2026 年 11 月"
template = "trd-compact"
frozen = false
archived = false

[template-options.trd-compact]
columns = 3
hash-interval = 10
strip-comments = false

[[chapter]]
title = "数学与多项式"
items = ["modint", "ntt"]

[[chapter]]
title = "数据结构"
items = ["dsu", "segtree"]
```
