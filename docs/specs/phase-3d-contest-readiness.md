# Spec：Phase 3D 整场验题与质量体检（Contest Readiness）

> 状态：已确认（2026-11-24）
> 关联文档：[roadmap.md](../roadmap.md)、[local-polygon.md](./local-polygon.md)、[phase-3c-interactive-communication-subtasks.md](./phase-3c-interactive-communication-subtasks.md)

## 1. 目标与定位

Phase 3A–3C 解决了「单题维度」的严谨出题与验收闭环；但在真实办赛（一场 10–13 题的 XCPC 或 4–6 题的 OI 比赛）中，定稿冻结前最容易出事故的环节集中在三处：

1. **缺乏整场全局视野**：需要逐题点开才能确认哪道题验收过期、哪道题标程卡时限、哪道题缺错解或数据体积过大；
2. **隐蔽未定义行为（UB）**：标程、Generator、Validator、Checker、Interactor 在 `-O2` 下碰巧通过，换评测机编译器或内存布局即崩溃或行为漂移；
3. **验题人协作污染工作区**：验题人发来若干 `.cpp` 解法，需要反复复制进 `solutions/` 再手动清理，既易污染 Git 历史又难横向汇总耗时对比。

Phase 3D 围绕**整场验题与质量体检**补齐这三块能力，全部下沉 Core，同时提供 CLI、MCP 与桌面端视图。

---

## 2. Sanitizer（ASan / UBSan）UB 体检模式

### 2.1 触发与扫描范围

- **显式体检动作**：通过 CLI `xcpc package build <id> --sanitize`、MCP `build_package(mode: "sanitize")` 或桌面端「UB 体检」按钮触发。
- **全工具链覆盖**：使用 `-fsanitize=address,undefined -fno-omit-frame-pointer -g` 编译并运行：
  - `generators/*`（数据生成器）
  - `validator.cpp`
  - `checker.cpp`（若自定义）
  - `interactor.cpp`（若交互题）
  - 所有预期 AC 解法（`main = true` 及 `expect` 含 `"AC"` 或部分分预期通过对应子任务的解法）

### 2.2 隔离、时限与门禁策略

- **独立缓存与零污染**：Sanitizer 编译产物与运行结果单独隔离，**绝不覆盖**正常 `-O2` 构建的缓存、耗时统计与 `verified` 签名。
- **自动放宽时限**：运行期自动将时限放宽（默认 `5× TL`，下限 `5s`），避免因 ASan/UBSan 自身 2–5 倍插桩开销误报 TLE，聚焦捕获越界访问、栈/堆溢出、未初始化内存、有符号整数溢出与空指针解引用。
- **非强制阻断 `--strict`**：体检结果（通过 / 发现 UB 详情与堆栈摘要 / 当前工具链不支持 ASan）记录在报告中并在体检视图醒目展示，**不强制阻断普通 `--strict` 验收与冻结**（兼容缺少 ASan 运行库的平台环境）。

---

## 3. 验题人外部提交试跑（零污染）

### 3.1 支持的输入形态

- **单题单文件**：对当前题目选择一个或多个外部 `.cpp` 文件直接试跑；
- **整场单目录**：指向一个目录，按文件名前缀/主干自动匹配比赛题号（`A.cpp`、`a.cpp`、`A-tree.cpp`）或题目 slug（`tree-dp.cpp`），无法匹配的文件列出提示并跳过；
- **多验题人二级目录**：指向 `<dir>/<tester_name>/<prob>.cpp` 结构，自动按验题人分组并生成多验题人 × 题目的横向结果矩阵。

### 3.2 零污染执行与可选「一键收编」

- **只读不污染工作区**：外部代码在临时构建目录编译，针对题目已构建的测试数据（若未构建则提示先构建或按选项自动构建）运行，**绝不写入 `solutions/`、不修改 `problem.toml`**。
- **对比与导出**：报告汇总每份外部提交的判定、通过测试数 / 得分、最慢耗时、与标程最慢耗时的比值（如 `1.35× std`）以及首个失败测试点诊断；支持一键导出为 **Markdown 验题报告**。
- **可选「一键收编」**：对表现优异的外部解法（例如独立思路 AC 解、有效暴力或典型错解），桌面端与 CLI 提供显式动作将其原子拷贝进 `problems/<slug>/solutions/` 并写入 `problem.toml` 的 `[solutions.<name>]`。

---

## 4. 比赛级「验题体检（Contest Readiness）」闭环

### 4.1 聚合体检维度

针对一场比赛的全部题目，聚合展示（单题维度零重复计算，直接读取各题验收报告、`xcpc-lint` 结果与可选 Sanitizer 报告）：

- **验收状态**：已验收（新鲜）/ 验收已过期 / 验收失败 / 未构建 / 纯出版题；
- **时限安全边际**：标程最慢耗时占比（如 `0.38s / 2.00s = 19%`）、最慢 AC 解 vs 最快 TLE 错解耗时余量；
- **数据与评测机预算预警**：最大单点输入/输出体积（默认 > 20MB 预警）、单题全测试点累计标程耗时（过长预警，防 OJ 评测队列拥堵）；
- **覆盖度与完整性**：独立参考解数量、错解数量、Validator / Checker 自测状态、Sanitizer UB 体检状态、题面/题解/配图 `xcpc-lint` 告警数。

### 4.2 桌面端、CLI 与 MCP 入口

- **桌面端**：比赛内新增「验题体检」视图（表格总览 + 风险高亮），提供四个高频动作：
  1. **一键验收全部过期/未验题**（顺序执行并实时更新进度）；
  2. **一键整场 UB 体检**（对全场比赛运行 Sanitizer 体检）；
  3. **导入验题人目录试跑**（调起原生文件/目录选择器）；
  4. **导出整场验题体检报告（Markdown）**。
- **CLI**：
  - `xcpc contest audit <id> [--json] [--markdown <path>]`
  - `xcpc contest verify-external <id> <path> [--json] [--markdown <path>]`
  - `xcpc package build <id> --sanitize [--tests …] [--solutions …] [--json]`
- **MCP**：
  - `audit_contest`（只读，`--read-only` 可用）：返回整场比赛逐题健康度、风险预警与 `next_steps` 引导；
  - `build_package` 扩展 `mode: "sanitize"`；
  - `run_external_solution`（只读测试，写入受限在临时目录）：供 Agent 验证临时草稿解或验题提交而不污染 `solutions/`。
