---
title: 交互、通信与 OI 子任务
---

# 交互、通信与 OI 子任务

除传统题外，XCPC Workstation 原生支持基于标准输入输出（stdio）的交互题、多轮通信题（`runs = N`）以及带依赖 DAG 与部分分的 OI 子任务评判体系。所有能力均在本地原生进程运行器中实现，开箱即用。

## stdio 交互题

### 自动识别与交互器约定

当题目目录下存在 `interactor.cpp`（或 `interactor.{17,20,23}.cpp`）时，构建引擎会自动将该题识别为**交互题**：

- 交互器使用 `testlib.h` 的 `registerInteraction(argc, argv)` 初始化，由引擎以 `interactor <input_path> <tout_path>` 启动。
- 交互题**不会**自动将 `samples/*.in` 加入测试点列表（因为题面样例通常展示交互过程示例而非隐藏输入），所有测试数据均通过 `[[test]]` 显式声明。
- 默认采用**单阶段判定**：直接由 `interactor.cpp` 的退出码与 stderr 决定最终判定（`_ok`、`_wa`、`_pe` 或 `quitp` 部分分）。
- 若在 `[package].checker` 显式配置了 Checker 或题目目录存在 `checker.cpp`，则自动启用**两阶段判定**：交互器运行结束后，引擎将交互器写入的 `<tout_path>` 交给 Checker（`checker <in> <tout> <ans>`）做二次复核。

```bash
# 快速生成交互题脚手架（包含 interactor.cpp 与交互版 solutions/std.cpp）
xcpc package init guess-number --interactive
```

### 双管道代理、ILE 死锁检测与双向日志

引擎在交互器与选手程序之间建立**两条受控代理管道**（`Interactor → Solution` 与 `Solution → Interactor`），而非直接裸连进程：

1. **精准区分 TLE 与 ILE**：
   - 运行器独立监控选手进程的 CPU 时间与双管道的最近 I/O 活跃时间戳。
   - 空闲超时阈值设为 $\min(\text{TL} + 1500\text{ms}, \text{wall\_limit})$，确保选手在合法计算耗时较长时不会误判，而一旦双方因忘记刷新缓冲区（如未 `fflush(stdout)` / `std::endl`）陷入相互等待死锁，会立即触发 **`ILE`（Idleness Limit Exceeded）** 并终止双进程。
   - 选手 CPU 时间超限则准确判定为 **`TLE`**。
2. **双向交互日志（Transcript）**：
   - 代理线程实时按行记录交互双方收发的消息内容、方向（`→` / `←`）与多轮轮次编号，采用有界首尾窗口（保留首尾各最多 30 行、单行最多 512 字符）并统计省略条数及双向总字节数，确保极端大交互量下也不会耗尽内存。
   - 发生 `WA`、`ILE` 或 `RE` 时，可在桌面端诊断面板或 CLI JSON 报告中直接回放双向交互记录，快速定位协议偏差或死锁位置。

## 多轮通信题（`runs = N`）

对于「编码器 + 解码器」或需要多次独立调用同一可执行文件的通信题，在 `problem.toml` 中声明 `runs`（$\ge 2$）：

```toml
[package]
runs = 2
```

也可通过脚手架命令直接生成：

```bash
xcpc package init encode-decode --interactive --runs 2
```

多轮通信的执行与计费规则如下：

- **轮次串联**：
  - 交互模式（存在 `interactor.cpp`）：第 $k$ 轮交互器写出的 `<tout_path>` 自动成为第 $k+1$ 轮交互器的 `<input_path>`，每轮均创建全新的选手进程，杜绝跨轮内存泄漏。
  - 非交互模式（无 `interactor.cpp`）：第 $k$ 轮选手的 `stdout` 自动重定向为第 $k+1$ 轮选手的 `stdin`。
- **累计 CPU 计时与单轮内存限制**：
  - 所有轮次的 CPU 耗时累加计算，每轮启动时剩余时间预算为 `time_limit_ms - total_cpu_ms`；一旦累计 CPU 时间超过 `time_limit_ms`，立即判定为 `TLE` 并中止后续轮次。
  - 内存限制按单轮独立考核，报告中记录所有轮次的峰值 RSS。
- **短路与得分聚合**：任意一轮返回非 `AC` 判定时立即中止后续轮次，并在诊断信息中标明出错轮次（如「第 2 轮（共 2 轮）」）；全部通过时取各轮 `score_ratio` 的最小值。

!!! warning "外部 OJ 导出说明"
    DOMjudge 与 Hydro 目前均不原生支持 Polygon 风格的多轮通信（`runs >= 2`）。导出这类题目时，系统会生成结构化警告提示，需按目标 OJ 规则手工适配。

## OI 子任务与部分分

### `[[subtask]]` 声明与 DAG 依赖

在 `problem.toml` 中通过 `[[subtask]]` 声明子任务，并在每个非样例 `[[test]]` 上通过 `subtask = "<id>"` 指定唯一归属：

```toml
[[subtask]]
id = "1"
score = 20
scoring = "min"               # 捆绑计分（默认）

[[subtask]]
id = "2"
score = 30
depends = ["1"]               # 前置依赖：必须先通过子任务 1
scoring = "min"

[[subtask]]
id = "3"
score = 50
depends = ["2"]
scoring = "sum"               # 按测试点均分计分

[[test]]
gen = "gen_tree"
args = "--n 100 --seed {i}"
repeat = 3
subtask = "1"

[[test]]
gen = "gen_tree"
args = "--n 5000 --seed {i}"
repeat = 4
subtask = "2"

[[test]]
gen = "gen_tree"
args = "--n 100000 --seed {i}"
repeat = 5
subtask = "3"
```

构建引擎在运行前会静态校验子任务配置（`subtasks` 门禁）：

- 子任务 `id` 必须非空且唯一；所有 `depends` 必须指向已声明的子任务且不能形成循环依赖（强制为有向无环图 DAG）。
- 每个声明的子任务必须至少包含 1 个测试点（禁止空子任务）。
- 当题目声明了 `[[subtask]]` 时，所有非样例测试点都必须归属到一个有效子任务（单归属原则，测试点复用通过 `depends` 表达）。
- 若所有子任务分值之和不等于 `100`，会发出警告（不阻断构建）。

### 计分策略与 `quitp` 部分分

Checker 或 Interactor 可通过 `testlib.h` 的 `quitp(ratio, ...)` 返回单点部分分（退出码 `7`，stderr 格式为 `points <ratio> [message]`，其中 `ratio` 钳位在 $[0.0, 1.0]$；`ratio > 0` 记为 `AC` 并保留比例，`ratio == 0` 记为 `WA`）。

两种子任务计分策略的计算规则如下：

| 策略（`scoring`） | 语义 | 前置依赖要求 | 子任务得分公式 |
| :--- | :--- | :--- | :--- |
| `min`（默认） | 捆绑计分 | 自身及所有传递依赖子任务的全部测试点均不得失败 | $\text{score} \times \min(\text{自身与传递依赖各点的 } \text{ratio})$；任一点非 `AC` 或 `ratio == 0` 则得 `0` |
| `sum` | 按点均分 | 所有传递依赖子任务均须获得满分（`AC` 且 `ratio == 1.0`） | 若依赖均满分，则 $\frac{\text{score}}{k} \sum_{i=1}^{k} \text{ratio}_i$（$k$ 为本子任务测试点数）；否则得 `0` |

### `validator --group` 子任务约束校验

当测试点归属于某个子任务 `subtask = "<id>"` 时，引擎调用校验器会自动传入 `--group <id>` 参数（兼容 `testlib.h` 的 `validator.group()` 接口）：

```cpp
#include "testlib.h"
#include <string>

int main(int argc, char* argv[]) {
    registerValidation(argc, argv);
    std::string grp = validator.group();
    int max_n = (grp == "1") ? 100 : (grp == "2" ? 5000 : 100000);
    int n = inf.readInt(1, max_n, "n");
    inf.readEoln();
    inf.readEof();
}
```

### 解法得分与逐子任务断言

在 OI 题目中，除了整体 `expect` 外，还可为每个解法声明总分预期（精确值或闭区间 `[min, max]`）以及逐子任务预期判定：

```toml
[solutions.std]
main = true                    # 主标程强制要求所有子任务拿满总分

[solutions.brute_n2]
score = 50                     # 精确总分断言
subtasks = { "1" = "AC", "2" = "AC", "3" = "TLE" }

[solutions.greedy_partial]
score = [20, 40]               # 闭区间总分断言
subtasks = { "1" = "AC", "2" = ["WA", "TLE"] }
```

!!! tip "子任务断言与 `expect` 的默认行为"
    对于非主标程解法，只要配置了 `score` 或 `subtasks` 且未显式填写 `expect`，引擎会自动将其整体 `expect` 默认为 `"ANY"`，避免与部分分解法的非 `AC` 整体结果冲突。
