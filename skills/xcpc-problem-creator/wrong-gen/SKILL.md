---
name: xcpc-wrong-gen
description: 设计基于 testlib.h 的确定性对抗生成器、在 problem.toml 中声明 [[test]] 与 OI [[subtask]]（min/sum/depends）、编写定向错解（含 ILE 与部分分解 score/subtasks）并闭环验证卡点强度。
---

# 对抗造数、子任务设计与错解卡点指南

本子 Skill 隶属 [`xcpc-problem-creator`](../SKILL.md)，配合 [`mistake-taxonomy.md`](../mistake-taxonomy.md) 设计高强度测试数据、OI 子任务梯度与错解矩阵，确保错误思路与次优复杂度无一漏网。

---

## 1. 编写确定性生成器（`generators/<name>.cpp`）

### 1.1 `testlib.h` 最佳实践
1. **初始化**：首行调用 `registerGen(argc, argv, 1);`（第 3 个参数固定 `1`）。
2. **命名参数**：使用 `opt<int>("n")`、`opt<long long>("v", 1000000000LL)`、`opt<std::string>("mode", "rand")` 解析参数，清晰可读、不易错位。
3. **100% 确定性**：
   - 所有随机抽样必须只使用 `testlib` 提供的 `rnd.next(...)`、`rnd.wnext(...)`、`shuffle(...)`、`rnd.perm(...)`；
   - **严禁**使用 `rand()`、`std::mt19937(time(0))`、`std::random_device` 或依赖未定义参数求值顺序的写法（如 `printf("%d %d", rnd.next(1, n), rnd.next(1, n))` 应先赋给局部变量再输出）；
   - `registerGen` 会将全部 `argv` 哈希为随机种子，因此只要命令行末尾 seed 不同，同一命令就能确定性产出不同数据。
4. **严格合规输出**：生成的数据必须 100% 满足 `validator.cpp`（含对应 `--group <subtask_id>` 约束、无行末多余空格、LF 换行、末尾有 `\n`、严格满足 $\sum n$ 与图/树语义约束）。
5. **善用 `rnd.wnext(l, r, t)`**：通过权重参数 `t` 控制树深、区间长度、边权偏斜（`t > 0` 偏向极大，`t < 0` 偏向极小），比纯均匀随机更能覆盖极端形态。

### 1.2 生成器模板示例

```cpp
#include "testlib.h"
#include <iostream>
#include <string>
#include <vector>

int main(int argc, char* argv[]) {
    registerGen(argc, argv, 1);

    int n = opt<int>("n", 10);
    int maxv = opt<int>("maxv", 1000000000);
    std::string mode = opt<std::string>("mode", "rand");

    std::vector<int> a(n);
    if (mode == "equal") {
        int val = rnd.next(1, maxv);
        a.assign(n, val);
    } else if (mode == "inc") {
        for (int i = 0; i < n; ++i) a[i] = i + 1;
    } else {
        for (int i = 0; i < n; ++i) a[i] = rnd.next(1, maxv);
    }

    std::cout << n << "\n";
    for (int i = 0; i < n; ++i) {
        if (i) std::cout << ' ';
        std::cout << a[i];
    }
    std::cout << "\n";
    return 0;
}
```

---

## 2. 在 `problem.toml` 中声明 `[[test]]` 与 `[[subtask]]`

### 2.1 `[[test]]` 基础声明

- 传统题的 `samples/` 默认自动作为前序测试点（带 `sample` 标签）纳入构建；
- **交互题（存在 `interactor.cpp`）**：`samples/` 多为题面示例对话而非合法隐藏输入，**默认不自动纳入评测**；若确有可机器执行的交互样例输入，需在 `[[test]]` 中显式声明。

```toml
# 1. 手写边界测试（须为 LF 换行、无 BOM、末尾有 \n）
[[test]]
file = "tests/hand-min.in"
tag = "edge"
name = "min-n-1"

# 2. 批量随机小数据（供 brute 交叉验证）
[[test]]
gen = "gen_random"
args = "-n 15 -maxv 20 -mode rand {i}"
repeat = 8
tag = "small"

# 3. 满数据与对抗结构
[[test]]
gen = "gen_tree"
args = "-n 200000 -type chain {i}"
repeat = 3
tag = "max"
name = "max-chain"

[[test]]
gen = "gen_tree"
args = "-n 200000 -type star {i}"
repeat = 2
tag = "hack"
name = "hack-star"
```

**关键规则**：
- **`repeat` 与 `{i}`**：`repeat = N` 将 `{i}` 依次替换为 `1..=N`；若 `args` 未包含 `{i}` 且 `repeat > 1`，引擎会自动在末尾追加 ` {i}` 作为 seed。
- **不覆写手工样例**：仅当确实希望由生成器生成并回写 `samples/` 时才设置 `sample = true`；普通测试点不要加此字段。
- **大测试点安全抽查**：调试生成结果时，调用 `inspect_test_case` 查看生成命令、所属子任务、字节大小与前/后 N 行预览，切勿直接读取整份大 `.in` 文件。

### 2.2 OI 子任务声明（`[[subtask]]`）与梯度设计

采用顶层 `[[subtask]]` + `[[test]].subtask` 单归属设计；数据复用通过 `depends`（DAG）表达，引擎不会重复运行被依赖子任务的测试点：

```toml
[[subtask]]
id = "1"                          # 唯一标识（如 "1" / "small" / "chain"）
score = 20                        # 整数分值（>= 0）
scoring = "min"                   # 可选："min"（默认，捆绑全过得分）| "sum"（按点均分）

[[subtask]]
id = "2"
score = 30
depends = ["1"]                   # 前置依赖：若子任务 1 未过，子任务 2 自动得 0 分

[[subtask]]
id = "3"
score = 50
depends = ["2"]

[[test]]
gen = "gen_small"
args = "-n 100 {i}"
repeat = 5
subtask = "1"                     # 单归属到子任务 "1"（自动传 --group 1 给 validator）
tag = "small"

[[test]]
gen = "gen_mid"
args = "-n 5000 {i}"
repeat = 6
subtask = "2"
tag = "max"

[[test]]
gen = "gen_full"
args = "-n 200000 {i}"
repeat = 10
subtask = "3"
tag = "max"
```

**计分与设计准则**：
1. **`min` vs `sum`**：
   - `min`（默认捆绑）：子任务内任一点（或其传递 `depends` 任一点）得 0 分则整组 0 分，否则得 `score * min(ratio)`（支持 `quitp` 返回 `[0.0, 1.0]`）；**判定题、构造题、易蒙分题一律优先用 `min`**；
   - `sum`（按点均分）：每点均分 `score / count`，累加 `point_score * ratio`（若存在 `depends` 且前置依赖未通过/未满分，该子任务记 0 分）。
2. **梯度清晰**：每个子任务应对应一类明确的做法或性质（例如：`sub1` 指数/暴力 → `sub2` $O(n^2)$ → `sub3` 特殊性质如链/无修改 → `sub4` 正解）；若后置子任务在数据范围上严格包含前置子任务，务必配置 `depends` 防止漏卡小边界而白拿高分。
3. **严格门禁约束（`verify` 硬报错）**：
   - `depends` 存在环或引用未知子任务 ID；
   - `[[test]].subtask` 引用未知子任务；
   - 存在空子任务（0 个测试点）；
   - 声明了 `[[subtask]]` 但存在未归属任何子任务的非样例测试点；
   - 总分 ≠ 100 会报 warning（不阻断，但常规 OI 题应凑满 100）。

---

## 3. 设计定向错解与卡点闭环

### 3.1 选型原则（对照 `mistake-taxonomy.md`）
不要为了凑数写无意义的「永远输出 0」错解。每份错解应对应一类**真实选手极易踩中的错误思路或实现**：

- **传统题 WA 错解**：`wa_overflow.cpp`（`int` 溢出）、`wa_greedy.cpp`（典型假贪心）、`wa_boundary.cpp`（漏 $n=1$ 或差一）、`wa_dirty_state.cpp`（多测未清空）；
- **传统题 TLE 错解**：`tle_n2.cpp`（朴素 $O(n^2)$）、`tle_spfa.cpp`（SPFA 替代 Dijkstra）、`tle_unordered_map.cpp`（默认哈希未防卡）、`tle_memset.cpp`（多测每组清满上限数组）；
- **交互题 / 多轮通信错解**：
  - `ile_no_flush.cpp`（`expect = "ILE"`：写 `cin.tie(nullptr)` 且漏 `flush` 导致死锁）；
  - `wa_over_query.cpp`（`expect = "WA"`：超出查询次数上限 1 次，验证交互器严格卡限）；
  - `wa_non_adaptive.cpp`（`expect = "WA"`：针对自适应交互器假设答案固定而被反杀）；
  - `wa_bad_encode.cpp` / `tle_multi_run.cpp`（多轮通信编码超长、格式违规或累计 CPU 超限）；
- **OI 部分分解**：每个子任务梯度至少写一份目标部分分解，验证既能稳拿对应档分，又会在下一档子任务稳定挂掉。

### 3.2 `problem.toml` 声明

```toml
[solutions.wa_greedy]
expect = "WA"

[solutions.tle_n2]
expect = "TLE"

# 交互题漏 flush 死锁错解
[solutions.ile_no_flush]
expect = "ILE"

# 仅当某错解因算法特性确实可能同时触发 WA 或 TLE 时使用数组；严禁把 AC 混入错解 expect
[solutions.bad_heur]
expect = ["WA", "TLE"]

# OI 部分分解：同时校验总分与逐子任务结果
[solutions.sub1_brute]
score = 20
subtasks = { "1" = "AC", "2" = "TLE", "3" = "TLE" }
```

### 3.3 迭代卡点工作流
1. **写错解并局部测试**：调用 `build_package(mode: "debug", solutions: ["wa_greedy"])` 快速验证，无需重跑全部解法。
2. **若错解意外 AC 或多拿子任务分（数据漏网）**：
   - 分析该错解的弱点（可对照 [`mistake-taxonomy.md`](../mistake-taxonomy.md)）；
   - 编写针对性 hack generator（或手写 `tests/hack-*.in`）并加入对应 `[[test]]`（设 `tag = "hack"`，OI 题归属对应 `subtask`）；
   - 局部重跑确认该错解在新增数据上稳定挂掉。
3. **核对时间余量（针对 TLE 错解）**：
   - 关注报告顶部的「安全边际摘要」：**最慢 AC 解 vs 最快 TLE 错解耗时余量**；
   - 标程最慢须 **≤ 50% 时限**（多轮通信按累计 CPU 计），同时 TLE 错解应明显超出时限（最好在满数据上触发墙钟/CPU 强杀而非勉强卡线），避免换机评测出现误判。
4. **全量严格验收**：全部错解、子任务与数据定型后，调用 `build_package(mode: "verify")` 跑全量测试 × 全量解法，确认门禁全绿。
