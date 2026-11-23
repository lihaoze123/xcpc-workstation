---
name: xcpc-problem-creator
description: 在 XCPC Workstation 中端到端创作、验证与出版传统算法竞赛题目的标准流程（题面、Validator/Checker、标程/暴力对拍、对抗造数与错解卡点、严格验收与 PDF 视觉自检）。
---

# XCPC 出题编排 Skill（Phase 3A 传统题）

本 Skill 指导 Agent 在 **XCPC Workstation** 工作区中，遵循标准 Polygon 工程方法论与本仓库的双档门禁机制，完成一道传统算法竞赛题目的端到端出题、验证与出版。

## 1. 反偷懒纪律（不可违反的红线）

验收不通过时，必须定位根因并修复对应代码、数据或题面，**严禁以下任何绕过行为**：

1. **禁止私自放宽 `[limits]`**：不得为了压低标程占比或避免 TLE/MLE，在未经用户确认的情况下调大 `time` / `memory`。
2. **禁止删除或绕过失败测试点**：不得将触发 WA/TLE/Validator 报错的测试从 `[[test]]` 移除来「消红」。
3. **禁止改松 `expect`**：不得把 `expect = "WA"` 改成 `["AC", "WA"]`，也不得把应 AC 的参考解改成允许 WA/TLE。
4. **禁止空壳 Validator / 假 Checker**：`validator.cpp` 必须逐项校验全部边界、空白与语义约束；自定义 `checker.cpp` 必须同时校验 `ouf` 与 `ans`，且具备真正能拦截错误输出的 `wa-*` 自测。
5. **禁止静默覆盖手工 `samples/NN.ans`**：样例是题面单一事实源；尤其是构造题/多解答案题，不得覆盖已手工编写的 `.ans`。
6. **禁止跳过自测或用 `debug` 冒充验收**：只有全量 `build_package(mode: "verify")` 通过并签发 `verified = true` 才算完成题目包验收。

---

## 2. MCP 工具全景

### 2.1 题目包工具（5 个）

| 工具 | 权限 | 用途 |
| :--- | :---: | :--- |
| `inspect_package` | 只读 | 查看题目包全景：文件清单、展开后的测试点、最近报告新鲜度、逐项 `gates` 与 `next_steps` |
| `inspect_test_case` | 只读 | 安全查看指定测试点的生成命令、文件大小与前/后 N 行预览（**严禁直接读取大型 `.in` 全文撑爆上下文**） |
| `init_package` | 写入 | 生成带 `testlib.h` 最佳实践的脚手架与自测目录（不覆盖已有文件） |
| `build_package` | 执行 | `mode: "debug"`（支持 `tests` / `solutions` 子集局部调试，不签发 `verified`）或 `mode: "verify"`（全量严格验收） |
| `stress_package` | 执行 | 有界对拍：递增种子比较两份解法，捕获反例自动落盘 `tests/stress-<n>.in`（不自动改写 `problem.toml`） |

### 2.2 出版与题库工具

| 工具 | 用途 |
| :--- | :--- |
| `get_problem` / `write_problem` | 读取或原子写入 `problem.toml`、`statement.md`、`editorial.md`、`samples/*`、源码与手写测试 |
| `compile_and_diagnose` | 编译单题题面或题解册（`book: "statements" \| "editorial"`），返回编译错误、lint 警告与版心溢出页码 |
| `render_page_png` | 渲染指定页 PNG 供视觉自检（核对公式、样例表、配图与分页） |

> 对应 CLI 命令：`xcpc package init <id>`、`xcpc package build <id> [--strict] [--tests …] [--solutions …]`、`xcpc package stress <id> ...`、`xcpc check problem <id>`。

---

## 3. 端到端出题工作流

按以下 7 步推进，每步完成后确认状态再进入下一步：

### Step 1：审题与初始化
1. 与用户确认题意、核心做法、数据范围、`[limits]`（时限/内存）与输出判定方式（标准 Checker 或自定义 Checker）。
2. 调用 `inspect_package` 检查题目现状；若尚未建立题目包骨架，调用 `init_package` 生成基础目录与模板文件。

### Step 2：编写题面与样例
1. 使用 `write_problem` 编写 `problem.toml`、`statement.md`（按 `## 输入`、`## 输出`、`## 提示` 划分章节；**样例只放 `samples/01.in` & `01.ans`，不写进题面正文**）。
2. 确保题面约束精确无歧义：明确 0/1-indexed、多测 $\sum n$ 上界、图是否连通/无重边自环、模数、误差要求与无解输出格式。
3. 手写 1–3 组覆盖核心逻辑与边界的 `samples/01..NN.{in,ans}`，并在 `## 提示` 中补充必要的样例解释。

### Step 3：Validator 与 Checker（详见子 Skill）
- 阅读并遵循 [`judge-validator/SKILL.md`](./judge-validator/SKILL.md)。
- 编写严格的 `validator.cpp`，补充 `tests/validator/valid/*.in` 与 `tests/validator/invalid/*.in`。
- 若内置标准 Checker（`wcmp`/`ncmp`/`fcmp`/`lcmp`/`rcmp4`/`rcmp6`/`rcmp9`/`yesno`/`nyesno`）无法满足判定需求，编写自定义 `checker.cpp` 并补充 `tests/checker/{ok,wa,pe}-*/{in,out,ans}`。
- 用 `build_package(mode: "debug")` 验证自测全部通过。

### Step 4：标程、暴力/独立参考解与对拍（详见子 Skill）
- 阅读并遵循 [`solutions-tutorial/SKILL.md`](./solutions-tutorial/SKILL.md)。
- 编写干净高效的 `solutions/std.cpp`（`main = true`），以及采用**独立思路**的 `solutions/brute.cpp`（`expect = "AC"`, `tags = ["sample", "small"]`）。
- 先写小规模生成器，调用 `stress_package` 进行充分的随机与特殊形态对拍；发现反例立即定位根因，修复后将有价值的 `tests/stress-<n>.in` 正式登记到 `[[test]]`。

### Step 5：对抗造数与错解矩阵（详见子 Skill 与错解分类学）
- 阅读并遵循 [`wrong-gen/SKILL.md`](./wrong-gen/SKILL.md) 与 [`mistake-taxonomy.md`](./mistake-taxonomy.md)。
- 编写结构化生成器 `generators/<name>.cpp`，在 `problem.toml` 中用 `[[test]]`（含 `repeat = N`、`tag`、`name`）声明完整测试集。
- 编写覆盖核心错因的定向错解（`expect = "WA"` / `"TLE"` / `["WA", "TLE"]`），用 `build_package(mode: "debug", solutions: [...])` 逐个确认每个错解都被数据稳定卡掉；若错解意外 AC，补强 hack 数据而非降低要求。

### Step 6：严格验收与撰写题解
1. 调用 `build_package(mode: "verify")` 执行全量严格验收，逐项检查第 5 节门禁清单与安全边际摘要（标程耗时占比、最慢 AC vs 最快 TLE 余量、最大输入体积）。
2. 按 [`solutions-tutorial/SKILL.md`](./solutions-tutorial/SKILL.md) 撰写 `editorial.md`（与题面符号、下标和标程实现完全一致）。

### Step 7：出版与视觉自检
1. 调用 `compile_and_diagnose(target: "problem", id: <slug>, book: "statements")` 与 `book: "editorial"`，确保零 error、无版心溢出警告、无缺章节或样例不成对警告。
2. 调用 `render_page_png` 逐页检查题面与题解的渲染图，确认数学公式、样例表、配图与分页整洁可读。

---

## 4. 数据量与覆盖度基线

非签到题的正式测试集（不含 `samples/`）应满足以下基线：

| 题型 | 测试点数量基线 | 必须覆盖的测试类别 |
| :--- | :--- | :--- |
| **单组测试**（无 $T$） | **≥ 30 组** | `edge`（最小/退化/全等/单点）、`small`（暴力交叉验证）、`random`（多密度/多分布）、`max`（满数据压测）、`hack`（定向卡错解与卡常） |
| **多组测试**（有 $T$） | **≥ 20 组** | 除上述类别外，必须包含「大 $T$ 小 $n$」「小 $T$ 大 $n$」「混合规模」「$\sum n$ 卡满」「大答案后接小答案/无解（测脏状态）」 |

- **测试点标签约定**：建议统一使用 `edge`、`small`、`random`、`max`、`hack` 标注 `[[test]].tag`，方便 `brute` 通过 `tags = ["sample", "small"]` 精确过滤，也方便局部调试。
- **单点体量控制**：检查报告中的最大输入体积，避免无意义的几十 MB 冗余 I/O 干扰时限评测。

---

## 5. 严格验收门禁清单（`verify`）

调用 `build_package(mode: "verify")` 并复核以下全部项目：

### 硬门禁（引擎强制校验，全部通过才签发 `verified = true`）
- [ ] `validator.cpp` 存在且编译通过
- [ ] `tests/validator/valid/*.in`（≥ 1 组）全部通过、`tests/validator/invalid/*.in`（≥ 1 组）全部被拒绝
- [ ] 若使用自定义 `checker.cpp`：`tests/checker/ok-*`（≥ 1 组）与 `wa-*`（≥ 1 组）全部匹配预期
- [ ] 全部测试点（含 `samples/`）通过 `validator`
- [ ] 标程（`main = true`）全量 `AC`，且最慢测试点耗时 **≤ 50% 时限**
- [ ] 全部解法（含手工 `samples/NN.ans` 校验）结果符合 `expect`

### 告警与质量门禁（引擎 Warning / Skill 强约束，非极简签到题不得省略）
- [ ] 手写 `tests/` 与 `samples/` 无脏输入（无 CRLF、无 UTF-8 BOM、无缺尾换行；引擎报 warning 不阻断，但应清零以免 `testlib` validator 误伤）
- [ ] 至少 1 份独立暴力/参考解通过全部 `sample` + `small` 测试点，且 `stress_package` 零反例
- [ ] 针对本题关键陷阱的错解（WA / TLE）全部按预期挂掉，最慢 AC 解与最快 TLE 错解保留充足余量
- [ ] 测试点数量与分布达到第 4 节基线
- [ ] `editorial.md` 已完成，题面与题解册均通过 `compile_and_diagnose` 且 `render_page_png` 目检无误

---

## 6. 子 Skill 与参考文档索引

- [`judge-validator/SKILL.md`](./judge-validator/SKILL.md)：`validator.cpp`、`checker.cpp` 编写规范、内置 Checker 选型与目录自测指南
- [`solutions-tutorial/SKILL.md`](./solutions-tutorial/SKILL.md)：标程性能红线、独立暴力交叉验证、`stress_package` 对拍闭环与 `editorial.md` 写作规范
- [`wrong-gen/SKILL.md`](./wrong-gen/SKILL.md)：`testlib.h` 确定性生成器、`[[test]]` 展开与定向错解卡点闭环
- [`mistake-taxonomy.md`](./mistake-taxonomy.md)：XCPC 常见错解与对抗造数分类学（溢出、贪心、边界、图树退化、反哈希、精度、复杂度退化、多测脏状态）
