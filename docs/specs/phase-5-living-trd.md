# Phase 5 规格：Living TRD（活模板册）与可验证算法库

2026-11-26 深度对齐确认，作为 Phase 4 之后的下一阶段核心演进。将 **Typst 实时排版引擎**、`xcpc-judge` **原生 C++ 评测器**、**开放目录活引用架构**与 **Agent / MCP 协同**融为一体，打造「写得干净、验得扎实、赛场零敲错、平时一键展开提交」的个人与队内算法库及纸质参考手册（TRD）体系。

---

## 1. 原则与定位

1. **双层活引用（与「题库 → 比赛」完全对称）**：
   - 底层是独立持久化的**算法库（`snippets/<slug>/`）**，每条算法以可读 slug 作为不可变 ID；
   - 上层是按章节活引用算法条目的**模板册（`trds/<id>/`）**，一个工作区可维护多本册子（如「ICPC 赛场极速版」「个人完整讲义」），一处修订同步更新所有未冻结模板册，定稿时可冻结归档快照（`snapshot/`）。
2. **单一事实源，零冗余配置**：
   - 代码文件（`code.hpp`）保持 100% 合法可编译的 C++；
   - 跨算法依赖直接写在 `#include "<slug>/code.hpp"` 中，由 Core 自动扫描构建依赖 DAG（外部编辑器配置 `-I snippets` 即可跳转补全，无需在 TOML 重复维护 `deps`）；
   - 同样支持**纯公式 / 结论卡片**（无 `code.hpp`，仅由 `notes.md` 组成），用于数论结论表、原根/质数表、微积分与几何公式、易错清单等，与代码条目无缝混排。
3. **赛场真实性优先（不做华而不实的装饰）**：
   - 贴合国内 ICPC / CCPC 赛制，**不设页数上限**，不做 25 页红线预算仪表盘；
   - **校验哈希按最终印在纸上的代码逐行计算**，校验命令只依赖标准 Linux 基础工具（`cpp`、`tr`、`md5sum`），考场裸机零依赖可用。
4. **安全与隔离边界**：
   - 工作区 Git 保持轻量纯净：Library Checker 权威验板题源码与生成的测试数据一律落盘全局缓存 `~/.cache/xcpc/library-checker/`，不污染 `problems/`；
   - 赛时单文件展开器（Bundler）：CLI 与 GUI 允许读取用户显式指定的本地 `.cpp` 及其相对路径 / `-I` 本地头文件；MCP 侧继续严格遵守工作区路径沙箱。

### 明确不做（Phase 5 范围外）

- 不做 25 页页数红线预算仪表盘与自动压页推荐；
- 不做强制性的模板册冻结硬门禁（默认柔性 warning，严格模式为可选开关）；
- 不做应用内完整 C++ IDE / LSP 调试器（复杂编码仍走外部编辑器 + `-I snippets`）；
- 不做离线模拟打字练习小游戏或多人在线协同编辑服务。

---

## 2. 存储布局与领域模型

在现有开放目录中新增对称的两个顶层目录 `snippets/` 与 `trds/`（不与 Typst 题面模板目录 `templates/` 冲突）：

```text
xcpc-workspace/
├── workspace.toml            # 新增可选 `snippets = false`（多人纯出题仓隐藏「模板」空间）
├── templates/                # 既有目录：题面模板 + 新增 TRD 预设种子（trd-compact.typ / trd-book.typ）
├── snippets/
│   ├── modint/
│   │   ├── snippet.toml      # 元数据、标签、复杂度、验板配置
│   │   ├── code.hpp          # 可选：合法 C++ 头文件（省略即为纯公式/结论卡片）
│   │   ├── notes.md          # 可选：Markdown + Math 说明、推导与坑点
│   │   ├── test.cpp          # 可选：独立单元测试 / 随机对拍
│   │   └── verify_*.cpp      # 可选：对接工作区题目或 Library Checker 的验板胶水代码
│   ├── ntt/
│   │   ├── snippet.toml
│   │   ├── code.hpp          # 内含 `#include "modint/code.hpp"`
│   │   ├── notes.md
│   │   └── verify_conv.cpp
│   └── number-theory-facts/  # 纯公式/结论卡片（无 code.hpp）
│       ├── snippet.toml
│       └── notes.md
└── trds/
    └── icpc-2026-regional/
        ├── trd.toml          # 书名、作者、模板名、模板参数、章节与活引用列表、frozen / archived
        ├── template.typ      # 可选：本册专属 Typst 模板
        ├── assets/           # 可选：封面徽标等专属静态资源
        └── snapshot/         # 冻结后生成：条目源文件副本 + 生效模板/参数 + 当时导出的 PDF（进 Git）
```

### 2.1 `snippet.toml` 最小格式

```toml
title = "快速数论变换（NTT）"
tags = ["poly", "math"]
time_complexity = "O(n log n)"
space_complexity = "O(n)"
description = "998244353 模数下的多项式乘法"

# 可选：多条验板声明（可混用自测 / 本地题 / Library Checker）
[[verify]]
kind = "self"
file = "test.cpp"                      # 独立单测或随机对拍（退出码 0 通过）

[[verify]]
kind = "problem"
problem = "my-poly-mul"                # 关联工作区 problems/<slug>/ 跑全量测试数据
adapter = "verify_local.cpp"           # #include 本板子并按该题 I/O 求解

[[verify]]
kind = "library_checker"
problem = "convolution_mod"            # yosupo06/library-checker-problems 题名
adapter = "verify_conv.cpp"
```

- 依赖关系**不写入 `snippet.toml`**，由 Core 解析 `code.hpp` 中的本地 `#include "<slug>/code.hpp"` 自动构建 DAG；检测环依赖时直接报结构化错误。
- `snippet.toml` 读写继续沿用 `toml_edit` 保留注释与未知字段；验板状态与最新耗时不写进 `snippet.toml`，由本地缓存报告按内容哈希派生。

### 2.2 `trd.toml` 最小格式

```toml
name = "2026 ICPC 区域赛模板册"
author = "队名"
date = "2026 年 11 月"
template = "trd-compact"               # 或 trd-book；存在 trds/<id>/template.typ 时优先
frozen = false
archived = false

[template-options.trd-compact]
columns = 3
hash-interval = 10                     # 每隔 N 行 / 函数块边界印累计哈希（0 表示仅印总哈希）
strip-comments = false

[[chapter]]
title = "数学与多项式"
items = ["modint", "ntt", "number-theory-facts"]

[[chapter]]
title = "数据结构"
items = ["dsu", "segtree"]
```

- **依赖完整性 Preflight**：组册编译与检查时，Core 校验每条被引用板子的全部传递依赖是否均已包含在当前 `trd.toml` 中；缺失上游依赖（如收了 `ntt` 却漏了 `modint`）时在 Preflight 中报出缺失清单，并提供一键补齐入口。
- **冻结与解冻**：语义与比赛冻结完全一致（固化所有条目源文件、生效模板与 PDF 至 `snapshot/`；冻结后只读；解冻仅开放给 GUI / CLI 并二次确认，不开放给 MCP）。**冻结门禁默认柔性**：预检仅对「失败 / 已过期」代码条目输出警告（「未验」不作为默认警告），不阻断冻结；CLI 额外提供 `--strict` 用于赛前强制全绿定稿（GUI 定稿对话框保持简洁，不放严格模式开关）。

---

## 3. 核心引擎能力

### 3.1 依赖 DAG 与通用单文件展开器（C++ Bundler）

面向**赛时模块化写代码 → 一键单文件交 OJ** 的高频场景，Core 提供统一的递归展开引擎，同时支持两种输入模式：

1. **赛时解文件展开**：输入任意本地 `.cpp` 文件路径或直接粘贴的代码文本，允许传入零个或多个 `-I <dir>` 搜索路径（默认自动包含当前文件所在目录与工作区 `snippets/` 目录）；
2. **单板子展开**：按 slug（如 `ntt`）直接展开该板子及其全部传递依赖。

展开规则：
- 递归解析双引号 `#include "..."`，按搜索路径顺序解析至真实文件（支持相对路径、`-I` 本地自定义头文件与工作区 `snippets/<slug>/code.hpp`）；
- 按拓扑序内联每个本地头文件**恰好一次**（自动识别并剔除 `#pragma once` 与经典头文件保护宏，正确处理菱形依赖如 `A → B, C → modint`）；
- 保留并按首次出现顺序去重系统头文件 `#include <...>`；
- 支持两种输出风格：默认附轻量边界注释（`// --- modint/code.hpp ---`），以及 `--compact` 纯净输出（剥离注释与多余空行）；
- 检测到环依赖或找不到本地头文件时返回带源文件与行号的结构化错误。

### 3.2 考场防敲错「累计分块校验哈希」（KACTL++）

解决传统 KACTL 整段代码只有一个哈希、长板子敲错无法定位的痛点：

- **印刷视图预处理**：排入模板册时，默认剥离 `#pragma once` 与指向同库其他条目的 `#include "<slug>/code.hpp"`（改为在条目标题下方印一行「→ 依赖条目的标题 · 第 N 页」），保证选手在考场照纸敲入的即为纯净算法代码。
- **确定性累计哈希算法**：
  - 严格基于**最终印在纸上的代码行**从第 1 行起逐行累计计算：去除 C/C++ 注释（`//...`、`/*...*/`，正确保留字符串与字符字面量）并剔除全部空白字符后取 MD5 前 6 位十六进制；
  - Rust Core 内置零子进程纯 Rust 实现，并在单元测试中与考场标准命令 `cpp -dD -P -fpreprocessed | tr -d '[:space:]' | md5sum | cut -c-6` 做逐字节等价校验；
  - 边栏每隔 $N$ 行（或在顶层函数/结构体右花括号 `}` 处）印出淡色的「行号 6 位累计哈希」，并在末行加粗标出整段总哈希；行号即核对命令中的 `N`；
  - 模板封面内页自动印出考场一行核对命令（`sed -n '1,Np' a.cpp | cpp -dD -P -fpreprocessed | tr -d '[:space:]' | md5sum | cut -c-6`），考场敲错时可二分行号快速定位。

### 3.3 双轨自动验板、Library Checker 全局直验与依赖级联

全面复用 `crates/xcpc-judge` 的本机原生 `g++` / `clang++` 工具链、`testlib.h`、进程监控与内容哈希缓存：

1. **三类验证源**：
   - `self`：编译并运行条目内 `test.cpp`（自动附加 `-I snippets`），退出码 `0` 视为通过，记录耗时与峰值内存；
   - `problem`：使用工作区 `problems/<slug>/` 已构建的测试数据与 checker，运行 `adapter` 代码（自动 `-I snippets`），验证全量 AC；
   - `library_checker`：对接全球权威题库 `yosupo06/library-checker-problems`：
     - 首次验证指定题名（如 `convolution_mod`）时，按需拉取该题轻量源码（`info.toml`、`gen/`、`checker.cpp`、`sol/correct.cpp` 及共享 `common/random.h`，仅数十 KB）至全局 `~/.cache/xcpc/library-checker/<problem>/`；
     - 在设置页与环境变量支持配置 GitHub 镜像前缀或本地 `library-checker-problems` 克隆目录，确保国内网络稳定且缓存后完全离线可用；
     - 在全局缓存目录内由 `xcpc-judge` 原生编译生成器造数并编译 `checker.cpp`，再用 `adapter` 跑全量测试点，缓存测试数据与结果哈希（工作区 Git 零新增文件）。
2. **依赖级联标脏与回归**：
   - 每条算法的验板缓存键包含：**自身 `code.hpp` + 全部传递依赖的 `code.hpp` + 验板代码/测试数据内容哈希**；
   - 当底层板子（如 `modint/code.hpp`）被修改时，所有下游依赖条目自动即时变为「已过期（需复验）」状态；
   - 提供一键「验证本条及所有下游依赖」（CLI `xcpc snippet verify <slug> --cascade` / GUI 按钮 / MCP 工具）。

---

## 4. Typst 模板契约与预设种子

复用 Phase 2.5C 的磁盘平权模板机制：所有模板均为可编辑的 `.typ` 文件，导出 `#let render(doc)` 与 `#let schema = (...)`，并在 `schema` 中声明 `kind: "trd"` 以与题面模板清晰隔离（互不污染下拉列表）。

内置两套预设种子（新建工作区或「从预设添加模板」时落盘至 `templates/`）：

1. **`trd-compact`（赛场紧凑三栏横版）**：
   - 横版 A4，默认 3 栏，无衬线正文；`schema` 只开放栏数（2 / 3 / 4）、字号、哈希间隔、是否剥离注释；
   - 目录与考场核对命令放在第一栏顶部，不设封面；灰度印刷，单色语法高亮（复用 `mono.tmTheme`），右侧「行号 哈希」边栏，长行悬挂缩进并标 `↪`。
2. **`trd-book`（竖版中文讲义 / 复习册）**：
   - 竖版 A4，单栏或双栏可选，衬线正文（Latin Modern Roman + 方正书宋），标题用 Latin Modern Sans + 方正黑体；
   - 首页上部为书名区，连续排版、每章不强制换页，适合大段数学推导、公式表格与代码混排的长期算法笔记。

两套模板的具体版式参数见 [statement-style.md §5.1](../statement-style.md#51-版式规范)。

---

## 5. 桌面端界面与跨模块联动

### 5.1 导航与「模板」空间

- 左侧导航升级为 **6 项**：`题库 (Cmd+1)` / `比赛 (Cmd+2)` / `模板 (Cmd+3)` / `训练 (Cmd+4)` / `工具 (Cmd+5)` / `设置 (Cmd+6)`（若 `workspace.toml` 设置 `snippets = false`，自动隐藏「模板」项，快捷键按可见项顺序自然对应）。
- 「模板」空间顶栏提供两个分段视图：**`算法库` | `模板册`**，工具栏常驻轻量入口「展开提交代码」（亦挂入 `Cmd+K`）：
  - **算法库视图**：列表/分组筛选（标签、验板状态：全部 / 未验 / 已过期 / 失败 / 已通过），右侧详情以单条渲染预览为主体（标题下方紧凑单行展示依赖/被依赖/关联题目，为空不显示），并提供快捷动作（复制单文件、运行验板、打开编辑）；
  - **模板册视图**：列出所有模板册（含冻结锁标与归档收纳），点击进入复用出版工作室底座的「模板册工作室」（左栏章节大纲与排序/删除确认 + 实时 Typst 增量预览 + 右侧 320pt「信息 | 版式」属性抽屉 + 导出分体按钮 + 定稿/重新编辑）；
  - **单条算法工作室**：左栏自动保存编辑器，Tab 切换 `代码 | 说明 | 验板`（纯公式卡片默认聚焦 `说明`），右栏按引用该条目的模板册版式实时预览单条渲染效果（含累计哈希边栏预览），`验板` 页合并展示用例与结果及级联复验入口；
  - **单文件展开弹窗/抽屉**：支持选择或拖入本地 `.cpp` 文件、配置额外 `-I` 头文件路径或直接粘贴代码，实时预览展开结果并一键复制。

### 5.2 题库与题解双向联动

- **沉淀为板子**：在题库详情的 `solutions/` 代码查看器中提供「沉淀为算法模板」——自动抽取代码并打开新建算法条目表单（可选记录来源题目引用）。
- **反向索引**：算法库内存索引自动扫描工作区 `snippets/` 间的 `#include` 依赖，以及声明了 `kind = "problem"` 验板或标注来源的题目，在算法详情页展示「依赖 / 被依赖 / 关联题目」。

### 5.3 内置示例工作区种子

在内置示例工作区（`xcpc init --example` / 品牌页「打开示例」）中补充 3–5 条精选示范条目与 1 本示例模板册，展示完整能力：
- `modint`（带 `test.cpp` 自测）→ `ntt`（`#include "modint/code.hpp"` 依赖链，附 Library Checker `convolution_mod` 验板声明）；
- `dsu`（简洁常用数据结构，带自测）；
- `number-theory-facts`（纯公式/结论卡片，展示无代码条目与数学公式混排）；
- 1 本示例 `trd.toml` 模板册。

---

## 6. CLI、MCP 与 Agent Skill

### 6.1 CLI 子命令（全部支持 `--json`）

- `xcpc snippet list|show|new|delete`
- `xcpc snippet verify <slug> [--cascade] [--all] [--jobs N]`（支持单条、级联下游或全库验板）
- `xcpc bundle <file.cpp> [-I <dir>...] [--copy] [-o <out.cpp>] [--compact]` 与 `xcpc bundle --snippet <slug> [--copy] [-o <out.cpp>] [--compact]`
- `xcpc trd list|show|compose|freeze|unfreeze|check|build|render`（`freeze` 支持 `--strict`）

### 6.2 MCP Tools（遵循 `--read-only` 与工作区路径沙箱）

- 只读工具：`search_snippets`、`get_snippet`（含依赖与反向索引）、`get_trd`、`bundle_code`（仅限工作区内路径或 snippet slug）、`verify_snippet`（运行验板并返回结构化结果与耗时，不改写源文件）；
- 写入工具（`--read-only` 禁用）：`write_snippet`、`compose_trd`、`freeze_trd`（**不提供解冻工具**），以及复用 `export_pdf` / `render_page_png` / `compile_and_diagnose` 支持 `target = "trd"`。
- 配套新增/扩展工作区 Skill，支持 Agent 协助编写随机对拍测试、重构精简板子并自动跑通下游级联回归、从题解中沉淀模板与编排模板册。

---

## 7. 分期落地计划

1. **Phase 5A：算法库底座、依赖展开器与本地验板**
   - `snippets/` 领域模型与存储读写（`toml_edit`、路径沙箱、原子写入、`notify` 监听）；
   - `#include` 依赖 DAG 解析、环检测与通用单文件展开器（支持本地 `.cpp` + `-I` 目录 + `snippets/`）；
   - 累计分块校验哈希引擎（含与 Linux `cpp | tr | md5sum` 逐字节对齐测试）；
   - 本地 `self`（`test.cpp`）与 `problem`（工作区题目）双轨验板 + 依赖级联标脏与回归；
   - `xcpc snippet` / `xcpc bundle` CLI 闭环。
2. **Phase 5B：Library Checker 全局直验与 TRD 出版引擎**
   - `~/.cache/xcpc/library-checker/` 按需拉取、镜像/本地仓配置与 `xcpc-judge` 原生造数验板；
   - `trds/` 活引用组册、依赖完整性 Preflight、柔性/严格冻结快照与解冻差异比对；
   - `trd-compact` 与 `trd-book` 两套平权 Typst 模板种子（`#let render(doc)` + `#let schema`）；
   - `xcpc trd` CLI 与示例工作区示范板子种子。
3. **Phase 5C：桌面端「模板」空间、题解双向联动与 MCP / Skill**
   - 左侧导航「模板」空间（算法库 / 模板册 / 工作室实时预览 / 单文件展开弹窗）；
   - 题库代码 & 题解代码块一键「沉淀为板子」与反向索引；
   - 完整 MCP 工具集、工作区 `AGENTS.md` 与 Skill 升级、设置页 Library Checker 源配置。
