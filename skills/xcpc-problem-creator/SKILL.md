---
name: xcpc-problem-creator
description: 在 XCPC Workstation 中端到端创作、验证与出版算法竞赛题目（传统题、stdio 交互题、stdio 多轮通信题与 OI 子任务 / 部分分）的标准流程（题面、Validator/Checker/Interactor、标程/暴力/部分分解与对拍、对抗造数与错解卡点、严格验收与 PDF 视觉自检）。
---

# XCPC 出题编排 Skill（传统题 / 交互题 / 多轮通信 / OI 子任务）

本 Skill 指导 Agent 在 **XCPC Workstation** 工作区中，遵循标准 Polygon 工程方法论与本仓库的双档门禁机制，完成算法竞赛题目（支持传统题、`interactor.cpp` stdio 交互题、`runs = N` 多轮通信题与 `[[subtask]]` OI 子任务 / 部分分）的端到端出题、验证与出版。

## 1. 反偷懒纪律（不可违反的红线）

验收不通过时，必须定位根因并修复对应代码、数据或题面，**严禁以下任何绕过行为**：

1. **禁止私自放宽 `[limits]`**：不得为了压低标程占比或避免 TLE/MLE/ILE，在未经用户确认的情况下调大 `time` / `memory`。
2. **禁止删除或绕过失败测试点**：不得将触发 WA/TLE/ILE/Validator 报错的测试从 `[[test]]` 移除来「消红」。
3. **禁止改松 `expect` / `score` / `subtasks`**：不得把 `expect = "WA"` 改成 `["AC", "WA"]`，不得放宽部分分解的 `score` 区间或 `subtasks` 预期，也不得把应 AC 的参考解改成允许 WA/TLE/ILE。
4. **禁止空壳 Validator / 假 Checker / 敷衍 Interactor**：`validator.cpp` 必须逐项校验全部边界、空白、语义与 `--group` 子任务约束；自定义 `checker.cpp` 必须同时校验 `ouf` 与 `ans` 并具备有效的 `wa-*` 自测；`interactor.cpp` 必须严格校验查询格式、次数上限与协议边界，严禁死锁或泄露隐藏信息。
5. **禁止静默覆盖手工 `samples/NN.ans`**：样例是题面单一事实源；尤其是构造题/多解答案题，不得覆盖已手工编写的 `.ans`。
6. **禁止跳过自测或用 `debug` 冒充验收**：只有全量 `build_package(mode: "verify")` 通过并签发 `verified = true` 才算完成题目包验收。
7. **保持 `statement.md` 单一事实源**：`[[subtask]]` 不会自动注入题面正文，交互协议（`## 交互方式`）与子任务分值表（`## 子任务`）必须与 `problem.toml` 人工保持一致。

---

## 2. MCP 工具全景

### 2.1 题目包与整场验题工具（7 个）

| 工具 | 权限 | 用途 |
| :--- | :---: | :--- |
| `inspect_package` | 只读 | 查看题目包全景：文件清单、`interactive`/`runs`/`subtasks` 配置、展开后的测试点、最近报告新鲜度、逐项 `gates` 与 `next_steps` |
| `inspect_test_case` | 只读 | 安全查看指定测试点的生成命令、所属子任务、文件大小与前/后 N 行预览（**严禁直接读取大型 `.in` 全文撑爆上下文**） |
| `init_package` | 写入 | 生成带 `testlib.h` 最佳实践的脚手架与自测目录（不覆盖已有文件）；支持 `interactive`、`runs`、`subtasks` 选项（自动生成含 flush 与防死锁示范的 `interactor.cpp` 及 `[[subtask]]` 模板） |
| `build_package` | 执行 | `mode: "debug"`（局部调试，不签发 `verified`）、`mode: "verify"`（全量严格验收）或 `mode: "sanitize"`（隔离的 ASan/UBSan 未定义行为体检，不覆盖 `report.json` 与验收状态） |
| `stress_package` | 执行 | 有界对拍：递增种子比较两份解法（同步支持交互题），捕获反例自动落盘 `tests/stress-<n>.in` 并附带截断输入/交互日志（不自动改写 `problem.toml`） |
| `audit_contest` | 只读 | 整场比赛验题体检：聚合逐题验收新鲜度、时限安全边际、数据体积与累计耗时、解法覆盖度、UB 体检状态与 `xcpc-lint` 告警，并给出 `next_steps` |
| `run_external_solution` | 只读 | 零污染试跑外部 `.cpp` 解法或验题人目录（编译与运行隔离在临时目录，不写入 `solutions/` 或 `problem.toml`） |

### 2.2 出版与题库工具

| 工具 | 用途 |
| :--- | :--- |
| `get_problem` / `write_problem` | 读取或原子写入 `problem.toml`、`statement.md`、`editorial.md`、`samples/*`、源码（含 `interactor.cpp`）与手写测试 |
| `compile_and_diagnose` | 编译单题题面或题解册（`book: "statements" \| "editorial"`），返回编译错误、lint 警告（含缺 `## 交互方式` / `## 子任务` 提示）与版心溢出页码 |
| `render_page_png` | 渲染指定页 PNG 供视觉自检（核对公式、样例表、配图与分页） |

> 对应 CLI 命令：`xcpc package init <id> [--interactive] [--runs N] [--subtasks]`、`xcpc package build <id> [--strict | --sanitize] [--tests …] [--solutions …]`、`xcpc package stress <id> ...`、`xcpc package verify-external <id> <files...>`、`xcpc package adopt <id> <file>`、`xcpc contest audit <id> [--markdown <path>]`、`xcpc contest verify-external <id> <path> [--markdown <path>]`、`xcpc check problem <id>`。

---

## 3. 端到端出题工作流

按以下 7 步推进，每步完成后确认状态再进入下一步：

### Step 1：审题与初始化
1. 与用户确认题型（传统题 / stdio 交互题 / `runs = N` 多轮通信 / OI 子任务与部分分）、核心做法、数据范围、`[limits]`（时限/内存）与输出判定方式（内置 Checker / 自定义 Checker / `interactor.cpp` / 二段复判 / `quitp` 部分分）。
2. 调用 `inspect_package` 检查题目现状；若尚未建立题目包骨架，调用 `init_package`（按需传入 `interactive`、`runs`、`subtasks`）生成基础目录与模板文件。

### Step 2：编写题面与样例
1. 使用 `write_problem` 编写 `problem.toml`、`statement.md`（按 `## 输入`、`## 输出`、`## 提示` 划分章节；**样例只放 `samples/01.in` & `01.ans`，不写进题面正文**）。
   - 交互题 / 多轮通信题：必须写清 **`## 交互方式`** 章节（明确交互流程、每轮输入输出格式、查询次数上限、flush 要求、交互器是否自适应 / adaptive，以及多轮通信的轮次标识与信道限制）。
   - OI 子任务题：必须写清 **`## 子任务`** 章节（列出各子任务分值、数据范围、特殊性质、依赖关系与捆绑/按点计分规则，与 `problem.toml` 严格对齐）。
2. 确保题面约束精确无歧义：明确 0/1-indexed、多测 $\sum n$ 上界、图是否连通/无重边自环、模数、误差要求与无解输出格式。
3. 手写 1–3 组覆盖核心逻辑与边界的 `samples/01..NN.{in,ans}`，并在 `## 提示` 中补充必要的样例解释。
   - **注意**：交互题的 `samples/` 多为题面展示用的示例对话而非合法隐藏输入，引擎**默认不自动将其纳入评测**；若确有可机器执行的交互样例输入，需在 `[[test]]` 中显式声明。

### Step 3：Validator、Checker 与 Interactor（详见子 Skill）
- 阅读并遵循 [`judge-validator/SKILL.md`](./judge-validator/SKILL.md)。
- 编写严格的 `validator.cpp`，补充 `tests/validator/valid/*.in` 与 `tests/validator/invalid/*.in`；若声明了 `[[subtask]]`，在 `validator.cpp` 中通过 `validator.group()` 校验各子任务专属约束，并补充 `tests/validator/{valid,invalid}/<subtask_id>/*.in`。
- 若为交互题或带交互器的多轮通信题，编写 `interactor.cpp`（`registerInteraction`、严格 flush、防死锁 `ILE`、防信息泄露、按需二段复判或 `quitp`）。
- 若内置标准 Checker 无法满足判定需求（含多轮无交互器通信题与二段复判），编写自定义 `checker.cpp` 并补充 `tests/checker/{ok,wa,pe}-*/{in,out,ans}`。
- 用 `build_package(mode: "debug")` 验证自测全部通过。

### Step 4：标程、暴力/部分分解与对拍（详见子 Skill）
- 阅读并遵循 [`solutions-tutorial/SKILL.md`](./solutions-tutorial/SKILL.md)。
- 编写干净高效的 `solutions/std.cpp`（`main = true`），以及采用**独立思路**的 `solutions/brute.cpp`（传统题用 `tags = ["sample", "small"]`，OI 题可配 `score` / `subtasks` 断言验证对应子任务得分）。
- 先写小规模生成器，调用 `stress_package` 进行充分的随机与特殊形态对拍（同步支持交互题）；发现反例立即定位根因（交互题结合 `transcript` 中的 `→` / `←` 对话排查），修复后将有价值的 `tests/stress-<n>.in` 正式登记到 `[[test]]`。

### Step 5：对抗造数、子任务梯度与错解矩阵（详见子 Skill 与错解分类学）
- 阅读并遵循 [`wrong-gen/SKILL.md`](./wrong-gen/SKILL.md) 与 [`mistake-taxonomy.md`](./mistake-taxonomy.md)。
- 编写结构化生成器 `generators/<name>.cpp`，在 `problem.toml` 中用 `[[test]]`（含 `repeat = N`、`tag`、`name`、OI 题单归属 `subtask`）与 `[[subtask]]`（含 `id`、`score`、`scoring`、`depends`）声明完整测试集。
- 编写覆盖核心错因的定向错解（`WA` / `TLE` / `ILE` / 部分分解 `score` & `subtasks`），用 `build_package(mode: "debug", solutions: [...])` 逐个确认每个错解都被数据稳定卡掉（交互题也借此反向验证 `interactor.cpp` 行为）。

### Step 6：严格验收与撰写题解
1. 调用 `build_package(mode: "verify")` 执行全量严格验收，逐项检查第 5 节门禁清单与安全边际摘要（标程耗时占比、最慢 AC vs 最快 TLE 余量、最大输入体积、子任务总分与逐解得分）。
2. 按 [`solutions-tutorial/SKILL.md`](./solutions-tutorial/SKILL.md) 撰写 `editorial.md`（与题面符号、下标和标程实现完全一致；OI 题按子任务梯度由浅入深展开）。

### Step 7：出版与视觉自检
1. 调用 `compile_and_diagnose(target: "problem", id: <slug>, book: "statements")` 与 `book: "editorial"`，确保零 error、无版心溢出警告、无缺章节（含 `## 交互方式` / `## 子任务`）或样例不成对警告。
2. 调用 `render_page_png` 逐页检查题面与题解的渲染图，确认数学公式、样例表、配图与分页整洁可读。

---

## 4. 数据量与覆盖度基线

非签到题的正式测试集（不含 `samples/`）应满足以下基线：

| 题型 | 测试点数量基线 | 必须覆盖的测试类别 |
| :--- | :--- | :--- |
| **单组测试**（无 $T$） | **≥ 30 组** | `edge`（最小/退化/全等/单点）、`small`（暴力交叉验证）、`random`（多密度/多分布）、`max`（满数据压测）、`hack`（定向卡错解与卡常） |
| **多组测试**（有 $T$） | **≥ 20 组** | 除上述类别外，必须包含「大 $T$ 小 $n$」「小 $T$ 大 $n$」「混合规模」「$\sum n$ 卡满」「大答案后接小答案/无解（测脏状态）」 |
| **交互题 / 多轮通信** | **≥ 20 组** | 覆盖最小边界、答案位于端点/极端分布、卡满查询次数上限（含差一）、高 I/O 满数据；多轮通信另覆盖信道边界与多轮累计 CPU 满载 |
| **OI 子任务** | **每子任务 ≥ 5 组，全题 ≥ 25 组** | 每个子任务独立覆盖其专属边界与满规模压测，确保子任务间存在清晰难度/复杂度断层且无空子任务 |

- **测试点标签约定**：建议统一使用 `edge`、`small`、`random`、`max`、`hack` 标注 `[[test]].tag`，方便 `brute` 通过 `tags = ["sample", "small"]` 精确过滤，也方便局部调试。
- **单点体量控制**：检查报告中的最大输入体积，避免无意义的几十 MB 冗余 I/O 干扰时限评测。

---

## 5. 严格验收门禁清单（`verify`）

调用 `build_package(mode: "verify")` 并复核以下全部项目：

### 硬门禁（引擎强制校验，全部通过才签发 `verified = true`）
- [ ] `validator.cpp` 存在且编译通过
- [ ] `tests/validator/valid/*.in`（≥ 1 组）全部通过、`tests/validator/invalid/*.in`（≥ 1 组）全部被拒绝（OI 题另校验 `tests/validator/{valid,invalid}/<subtask_id>/*.in`）
- [ ] 若使用自定义 `checker.cpp`（含二段复判）：`tests/checker/ok-*`（≥ 1 组）与 `wa-*`（≥ 1 组）全部匹配预期
- [ ] 全部测试点（传统题含 `samples/`；交互题按 `[[test]]` 显式声明）通过 `validator`（带子任务时自动传入 `--group <id>`）
- [ ] 标程（`main = true`）全量 `AC`（OI 题须获全部子任务满分），且最慢测试点耗时 **≤ 50% 时限**（多轮通信按各轮累计 CPU 计）
- [ ] 全部解法（含手工 `samples/NN.ans` 校验）结果符合 `expect`、`score` 与 `subtasks` 断言
- [ ] OI 子任务完整性（声明 `[[subtask]]` 时）：`depends` 无环且无未知 ID、`[[test]].subtask` 合法、无空子任务、无未归属子任务的非样例测试点

### 告警与质量门禁（引擎 Warning / Skill 强约束，非极简签到题不得省略）
- [ ] 手写 `tests/` 与 `samples/` 无脏输入（无 CRLF、无 UTF-8 BOM、无缺尾换行；引擎报 warning 不阻断，但应清零以免 `testlib` validator 误伤）
- [ ] 交互题至少包含针对性的错解（如漏 flush `ILE`、超查询上限 `WA`、错误策略 `WA`），验证 `interactor.cpp` 未放水（缺错解时引擎会报 warning）
- [ ] OI 子任务总分通常为 100（总分 ≠ 100 时引擎会报 warning），且各梯度均有对应部分分解验证可达性与区分度
- [ ] 至少 1 份独立暴力/参考解通过全部 `sample` + `small` 测试点，且 `stress_package` 零反例
- [ ] 针对本题关键陷阱的错解（WA / TLE / ILE）全部按预期挂掉，最慢 AC 解与最快 TLE 错解保留充足余量
- [ ] 测试点数量与分布达到第 4 节基线
- [ ] `editorial.md` 已完成，题面与题解册均通过 `compile_and_diagnose`（无缺 `## 交互方式` / `## 子任务` 警告）且 `render_page_png` 目检无误

---

## 6. 子 Skill 与参考文档索引

- [`judge-validator/SKILL.md`](./judge-validator/SKILL.md)：`validator.cpp`（含 `--group` 子任务校验）、`checker.cpp`（含 `quitp` 与二段复判）、`interactor.cpp`（交互协议、防死锁 `ILE`、防信息泄露）、`runs = N` 多轮通信与目录自测指南
- [`solutions-tutorial/SKILL.md`](./solutions-tutorial/SKILL.md)：标程性能红线、交互 flush / 多轮累计 CPU、独立暴力与 OI 部分分解（`score`/`subtasks`）、`stress_package` 对拍闭环与 `editorial.md` 写作规范
- [`wrong-gen/SKILL.md`](./wrong-gen/SKILL.md)：`testlib.h` 确定性生成器、`[[test]]` 与 `[[subtask]]` 声明（`min`/`sum`/`depends`）、定向错解（含 `ILE` 与部分分解）卡点闭环
- [`mistake-taxonomy.md`](./mistake-taxonomy.md)：XCPC & OI 常见错解与对抗造数分类学（溢出、贪心、边界、图树退化、反哈希、精度、复杂度退化、多测脏状态、交互题、多轮通信、OI 子任务与部分分）
