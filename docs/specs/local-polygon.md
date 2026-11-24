# Spec：Phase 3 本地 Polygon（原生工具链、严格验收门禁与 Agent 协同出题）

> 状态：已确认（2026-11-23）
> 关联文档：[product.md](../product.md)、[architecture.md](../architecture.md)、[roadmap.md](../roadmap.md)

## 1. 背景与核心路线变更

原 Phase 3 规划采用 `wasi-sdk` + `wasmtime` fuel 计 tick 以追求跨机器逐位一致的时间判定。经对比评估，该路线在竞赛 C++ 生态下存在明显短板：

1. **C++ 兼容性与时限失真**：wasm32 为 ILP32（`long` 仅 32 位），需自维护 `<bits/stdc++.h>` shim，缺少 `<ext/pb_ds>` 与硬件 SIMD，且 fuel 计量相对原生慢 3–8 倍，导致卡常与「标程 ≤ 50% 时限」的评估偏离真实 OJ 评测机。
2. **体积与维护成本**：静态内嵌 `wasmtime` 显著增加二进制体积与编译耗时，首次使用需下载 ~100MB `wasi-sdk`，且交互题需手写 fuel 轮转调度。

因此，Phase 3 正式调整为 **本机原生 `g++` / `clang++` + 原生执行** 路线：追求 100% 竞赛 C++ 兼容、极致造数与对拍速度、贴近真实 OJ 的时限校准，并对齐标准 Polygon 完整出题链路（参考《Polygon：从入门到入门》与 `ICPC-Problem-Creator.skill` 方法论）。

---

## 2. 阶段划分

| 阶段 | 范围 | 核心交付 |
| :--- | :--- | :--- |
| **Phase 3A** | **本地传统题（XCPC）完整闭环** | `crates/xcpc-judge`、CLI `xcpc package`、5 个 MCP 工具、工作区 `AGENTS.md` 升级、`skills/xcpc-problem-creator/` 与 `xcpc init --with-skills`、桌面端「构建」Tab（轻量出题控制台）与「设置 → 工具链」 |
| **Phase 3B** | **生态数据包（`crates/xcpc-packages`）** | Polygon ZIP 导入（单题 `problem.xml` 与整场 `contest.xml`，题面一次性转为 `.md` + `tylax` 转换复杂环境为 ```` ```typst ```` 原始块，全量导入题目包与自测，slug 冲突拒绝，交互 / 非 C++ / 子任务保留文件并输出 `warnings`，Zip Slip 防护与原子暂存）、DOMjudge（对齐 `Polygon2DOMjudge`，含 `problem.pdf` 与 Kattis 42/43 适配层，无气球颜色）与 Hydro 最小集合 ZIP 导出（单题 / 整场批量，严格剔除内部文件，遵守动静分离门禁）、CLI（`xcpc import polygon` / `xcpc export`）、MCP（`import_polygon` / `export_oj_package`）与桌面端轻量入口 |
| **Phase 3C** | **交互题、stdio 多轮通信与 OI 子任务** | 详见 [phase-3c-interactive-communication-subtasks.md](./phase-3c-interactive-communication-subtasks.md)：stdio 交互题（`interactor.cpp`、双管道代理日志、ILE 判定、对拍）、stdio 多轮通信（`runs = N`，对齐 Polygon `run-twice`）、OI 子任务 / 部分分（`[[subtask]]`、`--group`、部分分断言与 `quitp`）、Polygon/Hydro/DOMjudge 全量联动与 GUI/MCP/Skill 升级 |
| **Phase 3D** | **整场验题与质量体检** | 详见 [phase-3d-contest-readiness.md](./phase-3d-contest-readiness.md)：Sanitizer（ASan/UBSan）UB 体检模式、验题人外部提交零污染试跑与一键收编、比赛级「验题体检」视图 + `xcpc contest audit` CLI + `audit_contest` MCP |

---

## 3. 执行引擎与工具链

### 3.1 编译器探测与 `testlib.h`

- **去掉 `xcpc.lock`**：不再下载整个编译器工具链。
- **编译器发现**：自动探测本机 `PATH` 中的 `g++` / `clang++`，支持环境变量 `XCPC_CXX` 与桌面端「设置 → 工具链」手动指定路径。
- **编译约定**：固定 `-O2`，语言标准由文件后缀决定（`.17.cpp` / `.20.cpp` / `.23.cpp`，默认 C++20）；在 `xcpc-judge` 内预留极薄的 `Language` 抽象层，Phase 3A 先专注 C/C++，多语言（Python / Rust / Java 等）留待后续讨论。
- **内置最新版 `testlib.h`**：内嵌于 `xcpc-judge`（规避 GCC 15 + `-O2` 下旧版 `ncmp` 误报问题）；工作区或题目目录自带 `testlib.h` 时优先使用用户版本。
- **设置页工具链引导**：桌面端新增 **「设置 → 工具链」**，检测本机编译器状态、允许路径覆写、缺失时按平台给出安装指引（完整的 `algo-bootstrap` 风格编辑器/clangd 配置助手独立排期，不阻塞主链路）。

### 3.2 轻量原生进程监控与并发策略

- **进程限制与计量**：
  - Linux：`rlimit`（内存/栈/输出大小）+ `wait4 rusage`（CPU 时间 + 峰值 RSS）+ wall-clock 兜底强杀；
  - Windows：Job Object（进程树管理与内存限制）+ `GetProcessTimes` + wall-clock 兜底强杀；
  - **不做 OS 强安全沙箱**（面向出题人本地可信代码，安全隔离由用户侧兜底）。
- **阶段差异化并发**：
  - 编译、测试数据生成、Validator、Checker 默认多核并行提速；
  - **解法运行（Solutions）默认串行（`-j 1`）**，避免多核缓存与睿频争抢导致耗时失真或误报 TLE；允许 `--jobs` 显式覆盖。
- **三级内容哈希缓存**：编译产物、生成数据、解法运行结果按内容哈希缓存在 `~/.cache/xcpc/`（跨工作区共享）；工作区内的构建产物与报告落盘于 `build/problems/<slug>/`（不进 Git）。

---

## 4. 题目包结构与 `problem.toml` 声明

### 4.1 约定路径

```text
problems/<slug>/
├── problem.toml
├── statement.md / editorial.md
├── samples/                      # 01.in, 01.ans, ...（自动纳入前序测试点）
├── checker.cpp                   # 可选；自定义 checker
├── validator.cpp                 # 严检模式下必需
├── interactor.cpp                # 后续阶段：存在即视为交互题
├── generators/<name>.cpp
├── solutions/<name>.cpp
└── tests/
    ├── *.in                      # 手写测试 / 对拍反例（stress-<n>.in）
    ├── validator/
    │   ├── valid/*.in            # Validator 自测：必须通过
    │   └── invalid/*.in          # Validator 自测：必须拒绝
    └── checker/
        ├── ok-* /{in,out,ans}    # 自定义 Checker 自测：预期 OK
        ├── wa-* /{in,out,ans}    # 自定义 Checker 自测：预期 WA
        └── pe-* /{in,out,ans}    # 自定义 Checker 自测：预期 PE
```

### 4.2 `problem.toml` 声明扩展

保持极简声明式，通过 `toml_edit` 局部写回并保留未知字段：

```toml
[package]                         # 可选；省略时存在 checker.cpp 用之，否则默认 wcmp
checker = "wcmp"                  # wcmp | ncmp | fcmp | lcmp | rcmp4 | rcmp6 | rcmp9 | yesno | nyesno | "checker.cpp"

[[test]]
file = "tests/hand-01.in"
tag = "edge"                      # 可选展示与过滤标签
name = "min-bound"                # 可选人类可读名称

[[test]]
gen = "gen_tree"
args = "-n 200000 -w 5 {i}"       # {i} 替换为 1..=repeat；未含 {i} 且 repeat > 1 时自动追加为末尾 seed
repeat = 10                       # 可选，默认 1；100% 静态可展开、逐点缓存
tag = "max"
# sample = true                   # 可选：显式标记时同步写回 samples/

[solutions.std]
main = true                       # 标程（有且仅有一个）

[solutions.brute]
expect = "AC"                     # 支持单值或数组，如 ["WA", "TLE"]
tags = ["sample", "small"]        # 可选：仅在匹配 tag 的测试点上运行（如暴力交叉验证）

[solutions.greedy_wa]
expect = "WA"
```

### 4.3 样例衔接、内置 Checker 与防脏检查

- **样例单一事实源与构造题兼容**：`samples/` 默认自动作为前序测试点纳入构建，受 `validator` 校验；若 `samples/NN.ans` 已手工存在，**绝不静默覆盖**，而是同时校验手工 `.ans` 与 `std` 输出均通过 `checker`（天然支持构造题自定义样例输出）。仅当 `[[test]]` 显式标记 `sample = true` 时才写回 `samples/`。
- **内置标准 Checker 家族**：开箱内置 `wcmp`、`ncmp`、`fcmp`、`lcmp`、`rcmp4`、`rcmp6`、`rcmp9`、`yesno`、`nyesno`（基于最新版 `testlib.h`）。
- **防脏输入检查**：自动检测手写 `tests/` 与 `samples/` 中的 CRLF（`\r\n`）、UTF-8 BOM、末尾缺换行符，避免 `testlib` validator 无故 FAIL。

---

## 5. 双档严谨门禁与动静分离

### 5.1 开发态（`debug`）vs 严格验收态（`verify` / `--strict`）

| 维度 | 开发态（默认 `build` / MCP `debug`） | 严格验收态（`--strict` / MCP `verify` / 冻结 / 导出） |
| :--- | :--- | :--- |
| **执行范围** | 支持 `--tests` / `--solutions` 子集过滤（Invocation） | 必须全量测试 × 全量解法 |
| **签发 `verified`** | 局部跑或未过门禁时 `verified = false`，绝不覆盖整题验收状态 | 全部硬门禁通过才签发 `verified = true`（绑定内容哈希） |
| **Validator** | 缺失报 warning，不阻断 | **硬门禁**：必须存在 `validator.cpp`，且 `tests/validator/valid/` 与 `invalid/` 各至少通过 1 组自测（防空 validator） |
| **自定义 Checker** | 自测缺失报 warning | **硬门禁**：`tests/checker/ok-*` 与 `wa-*` 各至少通过 1 组自测（防假 checker） |
| **标程与错解** | 标程 > 50% 时限报 warning；不符 `expect` 报失败 | **硬门禁**：全部测试过 validator、全部解法符合 `expect`、标程最慢耗时 ≤ 50% 时限 |
| **交叉验证与错解数** | 缺独立参考解 / 缺错解报 **warning**（不阻断签到题） | 同样报 **warning**（醒目提示，不阻断简单题）；数据量基线（如单测 ≥ 30 / 多测 ≥ 20）由 Skill 方法论约束 |

### 5.2 动静分离（`xcpc check` / `contest freeze`）

- 纯出版题（无 `solutions/` 且无 `[[test]]`）跳过题目包门禁。
- 含题目包的题目在 `xcpc check` 与 `contest freeze` 时**不暗中触发编译或跑数据**，而是瞬间核对最近一次验收构建报告的内容哈希：若 `verified == true` 且哈希未过期则放行；若过期、未构建或验收失败则拦截冻结并提示先执行 `xcpc package build --strict`。

### 5.3 安全边际摘要与单步复现

- 构建报告顶部汇总：**最大输入体积**、**标程最慢占比**（如 `0.38s / 2.00s = 19%`）、**最慢 AC 解 vs 最快 TLE 错解耗时余量**。
- 任一失败项均附带可直接复制的**单步复现命令**（如 `xcpc package build <id> --tests <t> --solutions <s>`）。

---

## 6. CLI、MCP 与 Skill 协同体系

### 6.1 CLI 子命令

- `xcpc package init <id>`：生成带 `testlib.h` 最佳实践（`setTestCase` / `opt<T>`）的脚手架与基础自测用例（不覆盖已有文件）。
- `xcpc package build <id> [--strict] [--tests …] [--solutions …] [--jobs N] [--json]`
- `xcpc package stress <id> --gen <name> --args "…" [--solutions a,b] [--max-iter N] [--timeout S] [--json]`：捕获反例落盘 `tests/stress-<n>.in`（不自动改写 `problem.toml`）。
- `xcpc init [dir] [--example] [--with-skills]`：`--with-skills` 可选将内建出题 Skill 拷贝至工作区（如 `.agents/skills/xcpc-problem-creator/`）。

### 6.2 Phase 3A 完整 5 个 MCP 工具

| 工具 | 权限 | 职责 |
| :--- | :---: | :--- |
| `inspect_package` | 只读（`--read-only` 可用） | 返回题目包全景：文件清单、展开后测试点、最近报告新鲜度、逐项 `gates` 状态与 `next_steps` 动态引导 |
| `inspect_test_case` | 只读（`--read-only` 可用） | 查看指定测试点生成命令、文件大小与前/后 N 行安全预览（防止大数据撑爆上下文） |
| `init_package` | 写入 | 生成 `testlib.h` 骨架与基础自测目录（不覆盖已有文件） |
| `build_package` | 执行 | 支持 `mode: "verify" \| "debug"` 与子集过滤；返回编译错误（行列）、validator 失败详情、测试×解法矩阵、安全边际摘要、单步复现命令、逐项 `gates` 与 `next_steps` |
| `stress_package` | 执行 | 带迭代上限与时间预算的对拍，落盘 `tests/stress-<n>.in` 并返回复现种子、生成命令、截断输入、标程 vs 错解输出与 checker 诊断 |

### 6.3 工作区 `AGENTS.md` 与内建 `skills/xcpc-problem-creator/`

- 仓库内建 `skills/xcpc-problem-creator/`（主 `SKILL.md` + 3 个职责分离的子 Skill：`judge-validator`、`solutions-tutorial`、`wrong-gen` + `mistake-taxonomy.md` 错解分类学），全部围绕 XCPC Workstation 的 5 个 MCP 工具与 `verify` 门禁编写。
- 升级 `xcpc init` 内嵌的 `AGENTS.md`，写入标准出题链路、反偷懒纪律（禁止私自放宽 `limits`、删掉挂掉的测试点或改松 `expect`），并指向该 Skill。

---

## 7. 桌面端 GUI（轻量出题控制台）

> 2026-11-24 更新：本节描述的 Phase 3A 初版界面已由 [authoring-ui-refactor.md](./authoring-ui-refactor.md) 收敛（Tab 改名「出题」、动作下沉到面板工具行、「验证 / 严格验收 / 构建」统一为「检查」、状态统一为「未检查 / 有问题 / 已通过」）。冲突处以该规格为准。

- **单题第 4 个 Tab「构建」**（仅当存在 `solutions/` 或 `[[test]]` 时显示，或通过入口一键初始化脚手架）：
  - 顶部动作栏：**构建** / **严格验收** / **取消**、局部运行选择器（指定解法或测试点子集）、一键生成脚手架；
  - 顶部安全边际摘要条 + 门禁状态指示（失败汇入预览 HUD 的 Preflight 徽章）；
  - 主体 **测试 × 解法矩阵**（色块判定 + 耗时/内存，点击任意单元格展开右侧/底部诊断详情：生成命令、checker 输出、stderr 与单步复现）；
  - 支持结构化快捷配置（切换内置/自定义 checker、修改解法 `expect`/`main`、一键新增手写/生成测试点、清理脏输入）以及复用内置 `<Editor>` 的**轻量单文件快编**（纯文本保存 +「在外部打开」逃生舱），保存后经 `notify` 自动标脏。
  - **不做**：重型应用内 C++ IDE（LSP / clangd / 断点调试器 / 复杂工程树）。
- **「设置 → 工具链」**：检测 `g++` / `clang++` 版本与路径、允许手动指定覆写、缺失时展示按平台的安装指引。

---

## 8. 明确不做

- `wasi-sdk` / `wasmtime` / `xcpc.lock` / wasm64；
- Phase 3A–3D 内的 OS 强安全沙箱与外部选手竞赛判题（见远期候选概念 [local-contest-solver-concept.md](./local-contest-solver-concept.md)）；
- Nix / Hull 互通、CPLib 专门支持、函数式 `grader.cpp` 通信题 / 提交答案题 / 自定义 judger；
- 重型应用内 C++ IDE（LSP / 补全 / 调试器；仅保留轻量单文件快编与外部编辑器跳转）；
- Phase 3 内的多语言执行（Python / Rust / Java 等，留待后续经 `Language` 层加入）。
