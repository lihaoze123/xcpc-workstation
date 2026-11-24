# Spec：Phase 3C 交互题、stdio 多轮通信题与 OI 子任务 / 部分分

> 状态：已确认（2026-11-24）
> 关联文档：[roadmap.md](../roadmap.md)、[local-polygon.md](./local-polygon.md)、[product.md](../product.md)、[architecture.md](../architecture.md)

## 1. 目标与范围

在 Phase 3A（本地传统题闭环）与 Phase 3B（Polygon 导入 + DOMjudge / Hydro 导出）的基础上，补齐竞赛出题中最核心的三类高阶题型：**stdio 交互题**、**stdio 多轮通信题（run-twice / multi-run）** 与 **OI 子任务 / 部分分**，并打通本地构建、严格门禁、对拍、生态数据包导入导出、桌面端「构建」Tab 以及 CLI / MCP / 出题 Skill 全链路。

### 明确不做（Out of Scope）

- **函数式 Grader 通信题 / 函数式交互题**（`grader.cpp` + `<problem>.h` 头文件链接模式）；
- **提交答案题（Output-only）** 与 **自定义 Judger**；
- **将 `[[subtask]]` 自动注入题面正文**：`statement.md` 仍为题面单一事实源，不由构建配置篡改题面排版（仅通过 `xcpc-lint` 给出非阻断提示）。

---

## 2. stdio 交互题（`interactor.cpp`）

### 2.1 目录约定与 Checker 协作（对齐 Polygon 标准）

- 题目根目录存在 `interactor.cpp` 时自动识别为交互题（遵循标准 `testlib.h` `registerInteraction(argc, argv)` 协议，参数 `<input> <output>`）。
- **Checker 协作模式**：
  - **默认**：仅由 `interactor.cpp` 直接给出最终判定（AC / WA / PE / `quitp` 部分分）；
  - **二段复判**：当 `problem.toml` 显式配置了 `[package].checker` 或题目目录存在 `checker.cpp` 时，在 `interactor.cpp` 正常结束后，将其写出的 `tout` 文件交给 `checker` 进行最终复判。

### 2.2 双管道代理运行器与诊断

- **双管道代理与截断交互日志**：`xcpc-judge` 运行器使用两条受控代理管道转发 `solution <-> interactor` 双向流，同时按有界缓冲（头尾各 N 行 / 字节上限）录制交互日志（transcript），防止大 I/O 爆内存。测试失败时，诊断详情与 MCP / GUI 均展示双向着色的 `→`（interactor → solution）与 `←`（solution → interactor）对话片段及 `interactor` 退出信息。
- **区分 CPU TLE 与死锁 ILE**：
  - 解法 CPU 时间超过时限 → 判 **`TLE`**；
  - 解法未用满 CPU 时限，但因未 flush、协议卡死或双方互相等待导致 wall-clock 超过空闲/总时限上限 → 判 **`ILE`（Idleness Limit Exceeded）**。
- **对拍支持**：`xcpc package stress` 与 MCP `stress_package` 同步支持交互题，反例落盘 `tests/stress-<n>.in` 并附带截断交互日志。

### 2.3 `samples/` 策略与严格门禁

- **`samples/` 默认不纳入评测**：交互题的 `samples/` 多为题面展示用的示例对话而非合法隐藏输入，默认不自动加入前序测试点；若确有可机器执行的交互样例输入，需在 `[[test]]` 中显式声明。
- **严格门禁（`--strict` / `verify`）**：
  - 仍强制要求 `validator.cpp` 及其自测（`valid` ≥ 1、`invalid` ≥ 1）；
  - **不强制**单独的 `tests/interactor/` 自测目录，改为通过全量标程（AC）+ 错解验证 `interactor.cpp` 行为（缺错解报醒目 warning）；
  - 若存在自定义 `checker.cpp` 参与复判，仍要求其 `ok-*` / `wa-*` 自测通过。

---

## 3. stdio 多轮通信题（`runs = N`）

对齐 Polygon `run-twice`（`<judging run-count="N">`）模型，天然复用 stdio 交互器与测试管线，零非标准 `testlib.h` 扩展。

### 3.1 `problem.toml` 配置

```toml
[package]
runs = 2                          # 可选，默认 1；>= 2 启用多轮通信模式
# checker = "checker.cpp"         # 可选
```

### 3.2 执行与计时规则

- **两形态统一**：
  - **有 `interactor.cpp`**：第 1 轮运行 `interactor <test.in> <pass1.out>` 与第 1 轮选手进程交互，交互器可将状态/篡改后的消息/轮次标记写入 `tout`（即 `pass1.out`）；第 $k$ 轮（$2 \le k \le N$）将 `pass(k-1).out` 作为 `inf` 再次启动 `interactor` 与第 $k$ 轮选手进程交互；全部轮次结束后若有显式 `checker`，再以 `<test.in> <passN.out> <test.ans>` 复判。
  - **无 `interactor.cpp`**：第 1 轮选手进程读 `<test.in>` 写 `<pass1.out>`，第 $k$ 轮读 `<pass(k-1).out>` 写 `<passk.out>`，最终由 `checker` 判定。
- **资源计量**：总 CPU 时间按各轮累计计入题目时限（`sum(cpu_k) <= time_limit`）；内存限制每轮独立检查；任一轮出现 RE / TLE / ILE / WA 即中止后续轮次。
- **标程生成标准输出**：仅在无 `interactor.cpp` 且需要 `.ans` 时由标程跑完 $N$ 轮产出最终输出。

---

## 4. OI 子任务 / 部分分（`[[subtask]]`）

### 4.1 `problem.toml` 声明结构

采用顶层 `[[subtask]]` + 现有 `[[test]]` 单归属字段设计，保持扁平可局部编辑（`toml_edit`），数据复用通过 `depends` 表达（不重复运行同一测试点）：

```toml
[[subtask]]
id = "1"                          # 唯一标识（字符串，如 "1" / "small" / "chain"）
score = 30                        # 整数分值（>= 0）
scoring = "min"                   # 可选："min"（默认，捆绑全过得分）| "sum"（按点均分）

[[subtask]]
id = "2"
score = 70
depends = ["1"]                   # 可选：前置依赖子任务 ID 列表（形成 DAG）

[[test]]
gen = "gen_small"
args = "{i}"
repeat = 5
subtask = "1"                     # 单归属到指定子任务
tag = "small"

[[test]]
gen = "gen_full"
args = "{i}"
repeat = 10
subtask = "2"
tag = "max"
```

### 4.2 计分规则与 `quitp` 部分分

- **`min`（默认捆绑测试）**：子任务内任一测试点（或其依赖子任务任一测试点）得分为 0 则整个子任务得 0 分；否则子任务得分为 `score * min(test_ratio)`（正常 AC 时 `ratio = 1.0`，支持 `testlib.h` `quitp(ratio, ...)` 返回 `[0.0, 1.0]` 部分分比例）。
- **`sum`（按点均分）**：子任务内每个测试点均分 `score / count`，累加各点 `point_score * test_ratio`（若存在 `depends` 且前置依赖未满分/未通过，则该子任务按依赖规则记 0 分）。
- 题目总分为所有 `[[subtask]].score` 之和。

### 4.3 部分分解预期断言（`[solutions.<name>]`）

在保留现有 `main`、`expect`、`tags` 的基础上，新增两个可选断言字段：

```toml
[solutions.brute]
score = 30                        # 精确总分，或区间如 score = [20, 40]
subtasks = { "1" = "AC", "2" = ["WA", "TLE"] }  # 逐子任务预期，复用 ExpectVerdict
```

- 同时声明 `score` 与 `subtasks` 时，两者均须满足；
- `main = true` 的标程默认必须获得所有子任务满分（100% 总分）。

### 4.4 Validator 分组校验（`--group`）与严格门禁

- **自动传入 `--group <id>`**：当测试点归属 `subtask = "<id>"` 时，运行 `validator` 自动附带 `--group <id>` 参数（`testlib.h` `validator.group()` 原生支持；未使用 `group()` 的简单 validator 会安全忽略该参数，完全向后兼容）。
- **分组自测目录**：除根目录 `tests/validator/{valid,invalid}/*.in`（无 `--group` 运行）外，支持可选子目录 `tests/validator/{valid,invalid}/<subtask_id>/*.in`（自动带 `--group <subtask_id>` 运行）。
- **严格门禁（`--strict` / `verify`）**：
  - **硬 error（阻断 `verified`）**：`depends` 存在环或引用未知子任务 ID；`[[test]].subtask` 引用未知子任务；空子任务（0 个测试点）；声明了 `[[subtask]]` 但存在未归属任何子任务的非样例测试点；标程未获满分；任一解法不满足 `score` / `subtasks` / `expect` 断言。
  - **warning（不阻断）**：全部子任务分值之和 ≠ 100。

---

## 5. 生态数据包（Polygon / Hydro / DOMjudge）联动

1. **Polygon ZIP 导入（`crates/xcpc-packages`）**：
   - 完整导入 `<interactor>` 为 `interactor.cpp`、解析 `<judging run-count="N">` 为 `[package].runs`；
   - 完整解析 `<groups>` / `<group>` 为 `[[subtask]]`（`points` → `score`、`points-policy="complete-group"|"each-test"` → `scoring = "min"|"sum"`、`dependencies` → `depends`），`<test group="...">` 映射为 `[[test]].subtask`；
   - 移除 Phase 3B 的对应不支持占位 warning。
2. **Hydro ZIP 导出**：
   - 交互题：`testdata/config.yaml` 写入 `type: interactive` 与 `interactor: interactor.cpp`（附内置 `testlib.h`）；
   - OI 子任务：`testdata/config.yaml` 生成标准 Hydro `subtasks:` 列表（含 `score`、`type: min|sum`、`if: [...]` 依赖与 `cases:`）；
   - 多轮通信（`runs > 1`）：按目标平台支持度附带结构化 warning。
3. **DOMjudge ZIP 导出**：
   - 交互题：`problem.yaml` 写入 `validation: custom interactive`，将 `interactor.cpp` 与 Kattis 42/43 适配层、内置 `testlib.h` 打包进 `output_validators/`（若同时存在独立 `checker.cpp` 或 `runs > 1` 则输出明确 warning）；
   - OI 子任务：DOMjudge 原生为 ICPC 制不支持子任务计分，导出时**平铺全部测试点进 `data/secret/` 并附带结构化 warning**（不阻断导出）。

---

## 6. 题面 Lint、桌面端 GUI、CLI / MCP 与出题 Skill

- **`xcpc-lint`**：保持 `statement.md` 单一事实源；仅增加两条非阻断 warning：存在 `interactor.cpp` 但题面缺 `## 交互方式` 章节、声明 `[[subtask]]` 但题面缺 `## 子任务` 章节。
- **桌面端「出题」Tab**（2026-11-24 起界面结构以 [authoring-ui-refactor.md](./authoring-ui-refactor.md) §5 为准：题型改为侧栏「题型」条目统一配置并按题型显隐对象，去掉徽标行；矩阵颜色只表达「不符合预期」；交互日志去行底色、按轮分组）：
  - ~~顶部展示「交互题 / 多轮通信（×N）/ OI 子任务（总分 S）」紧凑徽标~~（已由侧栏「题型」摘要取代）；
  - 对象树支持创建、编辑、删除 `interactor.cpp`；
  - 单元格诊断面板支持渲染双向着色交互日志（`→` / `←`）与 `ILE` / `quitp` 得分详情；
  - 测试 × 解法矩阵支持按子任务分组折叠展示、每解法总分与各子任务得分行；
  - 提供子任务管理（增删改分值/依赖/评分模式）、测试点 `subtask` 归属设置、以及部分分解 `score` / `subtasks` 预期编辑。
- **CLI / MCP / Skill**：
  - `xcpc package init <id> [--interactive] [--runs N] [--subtasks]` 与 MCP `init_package` 同步支持生成 `interactor.cpp`（含 flush 与防死锁示范）及 `[[subtask]]` 脚手架；
  - `inspect_package` / `build_package` / `stress_package` 扩展返回 `interactive`、`runs`、`transcript`、`subtasks` 分组汇总与解法得分；
  - 升级 `skills/xcpc-problem-creator/`，补充交互题协议设计、防死锁与防信息泄露规范、多轮通信模式、以及 OI 子任务梯度与部分分解校验方法论。
