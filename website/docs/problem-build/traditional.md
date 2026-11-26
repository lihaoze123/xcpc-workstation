---
title: 传统题与严格验收
---

# 传统题与严格验收

传统题以标准输入（`stdin`）读入数据、向标准输出（`stdout`）输出结果，并由 Checker 比对选手输出与标程答案。本页说明传统题的目录约定、`problem.toml` 声明方式、内置与自定义 Checker、校验器自测、严格验收门禁、有界对拍、ASan/UBSan 体检以及外部验题人试跑流程。

## 目录结构与 `problem.toml` 声明

一个完整的传统题构建目录位于 `problems/<slug>/` 下：

```text
problems/a-plus-b/
├── problem.toml              # 题目元数据、限制、[package]、[[test]]、[solutions.*]
├── statement.zh.md           # 题面正文
├── samples/                  # 自动作为前置测试点（tag = "sample"）
│   ├── 01.in
│   └── 01.ans                # 或 01.out
├── validator.cpp             # 输入格式校验器（基于 testlib.h）
├── checker.cpp               # 可选：自定义评测器（省略时默认 wcmp）
├── generators/
│   └── gen_rand.cpp          # 数据生成器
├── solutions/
│   ├── std.cpp               # 主标程（main = true）
│   ├── alt_ac.cpp            # 独立参考解（交叉验证）
│   └── brute_tle.cpp         # 预期失败解（检验数据强度）
└── tests/
    ├── hand-edge.in          # 手写边界数据
    ├── validator/
    │   ├── valid/            # 校验器正向自测（应通过）
    │   └── invalid/          # 校验器负向自测（应拒绝）
    └── checker/              # 自定义 checker.cpp 时需提供自测（每组一个子目录）
        ├── ok-01/            # 内含 in、out、ans 三个文件
        └── wa-01/            # 内含 in、out、ans 三个文件
```

在 `problem.toml` 中声明构建配置、测试点与解法预期：

```toml
title = "A + B Problem"

[limits]
time = "1s"
memory = "256MB"

[package]
checker = "ncmp"              # 内置标准 Checker 名称，或自定义路径如 "checker.cpp"

# 1. 手写测试点
[[test]]
file = "tests/hand-edge.in"
name = "min-bound"
tag = "edge"

# 2. 生成器测试点（{i} 自动展开为 1..=repeat）
[[test]]
gen = "gen_rand"
args = "--n 100000 --seed {i}"
repeat = 5
tag = "max"

# 3. 回写为样例的生成测试点（生成后自动写入 samples/）
[[test]]
gen = "gen_rand"
args = "--n 5 --seed 1"
sample = true

# 解法声明（未声明的 solutions/*.cpp 默认 expect = "AC"）
[solutions.std]
main = true

[solutions.alt_ac]
expect = "AC"

[solutions.brute_tle]
expect = ["TLE", "WA"]        # 支持单个字符串或数组；也支持 "ANY"
tags = ["max"]                # 可选：仅在指定 tag 的测试点上运行
```

!!! note "测试点展开顺序与样例自动收录"
    非交互题会自动把 `samples/*.in`（按文件名排序）作为前置测试点，标签固定为 `"sample"`；随后按 `problem.toml` 中 `[[test]]` 的声明顺序依次展开。若 `repeat > 1` 且 `args` 未包含 `{i}`，引擎会自动在参数末尾追加序号 `1..=repeat`。

## 内置标准 Checker 家族与自定义 `checker.cpp`

当 `[package].checker` 省略时：若题目目录存在 `checker.cpp`（或 `checker.{17,20,23}.cpp`），则自动使用该自定义评测器；否则默认使用 `wcmp`。

XCPC Workstation 内置了 9 个标准 `testlib.h` Checker，无需手工复制源码即可直接在 `[package].checker` 中按名称引用：

| Checker 名称 | 比较语义 | 典型适用场景 |
| :--- | :--- | :--- |
| `wcmp` | 逐 Token（空白分隔字符串）严格比对（默认） | 绝大多数字符串、混合输出题 |
| `ncmp` | 逐 64 位有符号整数比对 | 纯整数输出题 |
| `fcmp` | 逐行精确比对（对空格与行尾敏感） | 严格排版、ASCII 图形输出题 |
| `lcmp` | 逐行比对，每行内按 Token 比较（忽略多余空白） | 按行组织的多 Token 输出 |
| `rcmp4` | 浮点数相对或绝对误差 $\le 10^{-4}$ | 4 位精度实数输出 |
| `rcmp6` | 浮点数相对或绝对误差 $\le 10^{-6}$ | 6 位精度实数输出 |
| `rcmp9` | 浮点数相对或绝对误差 $\le 10^{-9}$ | 9 位精度实数输出 |
| `yesno` | 单组 `YES` / `NO`（大小写不敏感） | 单组判定题 |
| `nyesno` | 多组 `YES` / `NO`（大小写不敏感） | 多测判定题 |

若题目需要自定义 Special Judge，编写 `checker.cpp`（标准 `registerTestlibCmd(argc, argv)` 接口），并在 `tests/checker/` 下为每组自测创建子目录（目录内须包含 `in`、`out`、`ans` 三个文件）：

- 目录前缀 `ok-*`：预期 Checker 返回 `AC`（退出码 `0`）
- 目录前缀 `wa-*`：预期 Checker 返回 `WA`（退出码 `1` 或 `8`）
- 目录前缀 `pe-*`：预期 Checker 返回 `PE`（退出码 `2`）

严格验收要求自定义 `checker.cpp` 至少包含 **1 组 `ok-*` 与 1 组 `wa-*`** 自测并全部通过。

## 校验器（`validator.cpp`）与自测

`validator.cpp` 使用 `registerValidation(argc, argv)` 校验每一组测试数据（包括样例与手写/生成测试点）是否严格满足题面约束与格式规范（无多余空白、无非法字符、数值在范围内）。

为防止校验器本身存在逻辑漏洞（例如漏判上界或误杀合法数据），请在 `tests/validator/` 下维护双向自测集：

- `tests/validator/valid/`：合法输入样本，校验器必须返回退出码 `0`。
- `tests/validator/invalid/`：非法输入样本（越界、缺行、非法字符等），校验器必须返回非 `0` 退出码。

严格验收要求 `validator.cpp` 存在、全部测试数据通过校验，且 `valid/` 与 `invalid/` 各至少包含 1 组通过的自测用例。

## 严格验收门禁一览

执行 `xcpc package build <id> --strict` 时，引擎依次评估以下门禁：

| 门禁 ID | 检查项 | 开发态（Debug） | 严格验收（`--strict`） |
| :--- | :--- | :--- | :--- |
| `compile` | 所有工具与解法编译通过 | 失败阻断 | 失败阻断 |
| `validator` | `validator.cpp` 存在且 `valid`/`invalid` 自测各至少 1 组并全部通过 | 缺失/不足为警告；自测失败阻断 | **失败阻断** |
| `checker_self_tests` | 自定义 `checker.cpp` 具备至少 1 组 `ok` 与 `wa` 自测并全部通过 | 不足为警告；自测失败阻断 | **失败阻断** |
| `subtasks` | OI `[[subtask]]` 结构无环、无空子任务、无未归属非样例点 | 错误阻断；总分非 100 为警告 | 错误阻断；总分非 100 为警告 |
| `dirty_inputs` | 手写数据无 CRLF、UTF-8 BOM 或缺失末尾换行 | 警告 | 警告 |
| `validator_tests` | 所有测试点通过 `validator.cpp` | 失败阻断 | **失败阻断** |
| `solutions_expect` | 所有解法判定、得分及子任务断言符合 `problem.toml` 预期 | 失败阻断 | **失败阻断** |
| `std_time_margin` | 主标程最慢测试点用时 $\le 50\%$ 时限 | 警告 | **失败阻断** |
| `solution_diversity` | 至少包含 1 个独立参考 AC 解与 1 个预期非 AC 错解 | 警告 | 警告 |

所有阻断门禁全部通过后，引擎将验收结果持久化至 `build/problems/<slug>/verified_report.json`，并将题目新鲜度标记为 `verified_fresh`。

## 有界对拍（`xcpc package stress`）

在寻找错解反例或验证暴力与正解一致性时，可使用内置的有界对拍命令：

```bash
xcpc package stress a-plus-b \
  --gen gen_rand \
  --args "--n 20 --seed {i}" \
  --solutions brute_wa,alt_ac \
  --max-iter 200 \
  --timeout 30
```

- `--gen <name>`：指定 `generators/<name>.cpp`。
- `--args <template>`：生成器参数模板，`{i}` 或 `{seed}` 会替换为当前迭代编号。
- `--solutions <names>`：与主标程 `std` 对拍的目标解法（逗号分隔或重复传入；省略时默认对拍所有非主标程解法）。
- `--max-iter <N>`（默认 `100`）与 `--timeout <secs>`（默认 `30`）：双重边界控制，防止无限挂起。

一旦发现某组数据上目标解法与主标程判定不一致，引擎会立即停止对拍，并将首个反例原子保存至 `tests/stress-<n>.in`，同时给出可复现的生成器命令。

## ASan / UBSan 未定义行为体检（`--sanitize`）

C++ 中的数组越界、空指针解引用、有符号整数溢出等未定义行为在 `-O2` 下可能偶然通过测试。使用 `--sanitize` 可执行完全隔离的消毒器体检：

```bash
# 全量体检（生成器、校验器、自定义 Checker、交互器及所有预期 AC 解法）
xcpc package build a-plus-b --sanitize

# 仅对指定测试点与解法执行体检
xcpc package build a-plus-b --sanitize --tests 1,2,max --solutions std,alt_ac
```

体检模式的隔离与放宽策略：

- **编译标志**：`-O1 -std=c++<ver> -fsanitize=address,undefined -fno-omit-frame-pointer -g -fno-sanitize-recover=undefined`。
- **缓存与产物隔离**：可执行文件存入 `~/.cache/xcpc/sanitize/bin/`，报告独立写入 `build/problems/<slug>/sanitizer-report.json`，**绝不覆盖**正常的 `report.json`、`verified_report.json` 或 `samples/`。
- **限制自动放宽**：时间限制自动放宽为 $\max(5 \times \text{TL}, 5000\text{ms})$，并禁用虚拟内存 `RLIMIT_AS` 与 RSS MLE 拦截，避免 ASan 影子内存与插桩开销造成误报。
- **结构化诊断**：自动从 stderr 提取 `AddressSanitizer` / `UndefinedBehaviorSanitizer` 摘要与关键栈帧。

## 外部验题人试跑与收编

出题团队在交叉验题时，常需要试跑验题人提交的外部 `.cpp` 文件，而不希望污染工作区现有的 `solutions/` 目录和 `problem.toml`。

### 零污染试跑（`verify-external`）

```bash
# 在临时目录编译并评测外部解法，不修改工作区任何文件
xcpc package verify-external a-plus-b /tmp/tester_alice.cpp /tmp/tester_bob.cpp

# 若题目尚未构建测试数据，可附加 --auto-build 自动以开发态构建数据
xcpc package verify-external a-plus-b /tmp/tester_alice.cpp --auto-build

# 将试跑结果导出为简洁的 Markdown 验题报告
xcpc package verify-external a-plus-b /tmp/tester_alice.cpp --markdown /tmp/report.md
```

试跑结果会汇总每份提交的总体判定、通过点/总点数、最大耗时与峰值内存、相对主标程的耗时倍率（`ratio_vs_std`）以及首个失败点的详细信息。在桌面端，也支持直接选取整场验题目录（单层 `A.cpp` / `<slug>.cpp` 或双层 `<dir>/<tester_name>/<prob>.cpp`）批量试跑。

### 显式原子收编（`adopt`）

确认某份外部提交值得保留为题目的正式参考解或错解后，可通过 `adopt` 将其原子复制进 `problems/<slug>/solutions/` 并同步注册到 `problem.toml`：

```bash
xcpc package adopt a-plus-b /tmp/tester_alice.cpp \
  --name alice_ac \
  --expect AC

# 收编为预期错解并限定测试标签
xcpc package adopt a-plus-b /tmp/tester_greedy.cpp \
  --name greedy_wa \
  --expect WA,TLE \
  --tag max
```

若目标文件需作为主标程，可附加 `--main` 标志写入 `main = true`。
