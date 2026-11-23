---
name: xcpc-judge-validator
description: 编写严格的 testlib.h validator.cpp、选型或编写自定义 checker.cpp，以及构建 tests/validator 与 tests/checker 强制自测用例的指南。
---

# Validator 与 Checker 编写指南

本子 Skill 隶属 [`xcpc-problem-creator`](../SKILL.md)，负责把关题目包的输入合法性校验与输出正确性判定。

> **核心原则**：严禁空壳 `validator.cpp` 与假 `checker.cpp`。严格验收态（`verify`）强制要求 `validator` 与自定义 `checker` 分别通过正反两面自测。

---

## 1. 内置标准 Checker 选型

在 `problem.toml` 的 `[package]` 中声明 `checker`（若目录下存在 `checker.cpp`，则优先使用自定义 Checker；两者均省略时默认 `wcmp`）：

| Checker | 判定语义 | 典型适用场景 |
| :--- | :--- | :--- |
| `wcmp` | 逐空白分隔 token 精确比较（默认） | 绝大多数唯一解整数/字符串输出 |
| `ncmp` | 逐 64 位有符号整数比较 | 仅输出 64 位整数序列（拒绝非整数 token） |
| `yesno` / `nyesno` | 单组 / 多组大小写不敏感 `YES`/`NO` | 判定题（注意 `nyesno` 会校验组数） |
| `rcmp4` / `rcmp6` / `rcmp9` | 浮点绝对或相对误差 $\le 10^{-4} / 10^{-6} / 10^{-9}$ | 实数输出题（与题面精度声明严格对应） |
| `lcmp` / `fcmp` | 逐行忽略行末空白 / 逐字节严格含空格 | 行格式敏感的字符画、文本排版题 |
| `"checker.cpp"` | 自定义判定逻辑 | 构造题、多解等价、特殊误差/评分、顺序无关输出 |

---

## 2. 编写严格的 `validator.cpp`

XCPC Workstation 内置最新版 `testlib.h`（若题目或工作区目录自带则优先使用用户版本），固定以 `-O2`、默认 C++20 编译。

### 2.1 必守规范

1. **入口初始化**：首行调用 `registerValidation(argc, argv);`。
2. **多测必须标注组号**：多组测试每组开始前调用 `setTestCase(tc);`（`tc` 从 1 开始），确保报错能精准定位到第几组。
3. **严格空白与结尾**：
   - 每个字段之间的空格必须显式调用 `inf.readSpace()`（或使用 `inf.readInts(n, L, R, "a")` 等自带单空格校验的批量读入）；
   - 行末必须显式调用 `inf.readEoln()`；
   - 文件末尾必须调用 `inf.readEof()`；
   - **禁止**使用 `cin` / `scanf` 或跳过空白的宽松读取。
4. **参数命名**：每次读入传入可读变量名（如 `inf.readInt(1, 200000, "n")`），便于定位越界字段。
5. **完整语义校验**：用 `ensuref(cond, "msg", ...)` 核对所有题面隐含或显式约束：
   - 多测总和上界：$\sum n \le N_{\max}$、$\sum m \le M_{\max}$；
   - 集合/排列约束：元素互异、构成 $1..n$ 排列、严格递增/非降；
   - 图与树约束：无自环（`u != v`）、无重边（`set<pair<int,int>>`）、树连通无环（并查集 + 边数恰好 $n - 1$）、DAG 无环、二分图可二染色；
   - 字符串字符集：`inf.readToken("[a-z]{1,200000}", "s")` 或逐字符检查。
6. **防脏输入**：引擎会自动检测手写 `tests/` 与 `samples/` 中的 CRLF（`\r\n`）、UTF-8 BOM 和末尾缺换行符；写作手写测试点时务必使用纯 UTF-8（无 BOM）+ LF 换行 + 末尾保留 `\n`。

### 2.2 最小规范模板（多测树题示例）

```cpp
#include "testlib.h"
#include <numeric>
#include <vector>

struct DSU {
    std::vector<int> p;
    explicit DSU(int n) : p(n + 1) { std::iota(p.begin(), p.end(), 0); }
    int find(int x) { return p[x] == x ? x : p[x] = find(p[x]); }
    bool unite(int a, int b) {
        a = find(a); b = find(b);
        if (a == b) return false;
        p[a] = b;
        return true;
    }
};

int main(int argc, char* argv[]) {
    registerValidation(argc, argv);

    int T = inf.readInt(1, 10000, "T");
    inf.readEoln();

    long long sum_n = 0;
    for (int tc = 1; tc <= T; ++tc) {
        setTestCase(tc);
        int n = inf.readInt(1, 200000, "n");
        inf.readEoln();
        sum_n += n;
        ensuref(sum_n <= 200000, "sum of n exceeds 200000");

        inf.readInts(n, 1, 1000000000, "a");
        inf.readEoln();

        DSU dsu(n);
        for (int i = 1; i < n; ++i) {
            int u = inf.readInt(1, n, "u");
            inf.readSpace();
            int v = inf.readInt(1, n, "v");
            inf.readEoln();
            ensuref(u != v, "self-loop at vertex %d", u);
            ensuref(dsu.unite(u, v), "cycle detected on edge (%d, %d)", u, v);
        }
    }

    inf.readEof();
    return 0;
}
```

---

## 3. 编写自定义 `checker.cpp`

当题目存在多解、构造输出、自定义误差或特殊格式时，编写 `checker.cpp` 并在 `problem.toml` 设 `[package].checker = "checker.cpp"`（或省略 `[package]` 让其自动生效）。

### 3.1 必守规范

1. **入口初始化**：首行调用 `registerTestlibCmd(argc, argv);`。
2. **三流职责**：`inf`（输入）、`ouf`（选手/被测解法输出）、`ans`（标程或手工参考答案）。
3. **同源校验 `ouf` 与 `ans`（铁律）**：
   - 将解析与合法性检查封装为 `readAndCheck(InStream& stream, ...)`，分别对 `ans` 和 `ouf` 调用；
   - 若 `ans` 本身不合法，流读取会自动报 `_fail`（JE），立即暴露标程 bug；
   - 对最优化 + 构造题：若 `ouf` 合法且结果**严格优于** `ans`，必须调用 `quitf(_fail, "participant found better answer than jury")` 触发 JE，而非静默判 WA 或 AC！
4. **判定码使用**：
   - `_ok`：完全正确；
   - `_wa`：语义错误、构造不满足条件、非最优解；
   - `_pe`：格式错误（`testlib` 读取类型不匹配或多余 token 时会自动触发 `_pe`）；
   - `_fail`：标程答案不合法或不如选手答案（触发 Judge Error）。
5. **构造题与 `samples/` 衔接**：若已手工编写 `samples/NN.ans`，系统构建时**绝不覆盖**手工 `.ans`，而是同时用 `checker` 验证手工 `.ans` 与 `std` 输出均通过，天然兼容不同合法构造。

### 3.2 自定义 Checker 结构骨架

```cpp
#include "testlib.h"
#include <vector>

// 返回该流给出的有效指标（例如目标值或可行标记），不合法直接 stream.quitf(_wa, ...)
long long readAndVerify(InStream& stream, int n /*, problem input ... */) {
    // 使用 stream.readInt / stream.readLong 等校验范围与构造条件
    // 例：stream.quitf(_wa, "invalid construction at index %d", i);
    return 0;
}

int main(int argc, char* argv[]) {
    registerTestlibCmd(argc, argv);

    int n = inf.readInt();
    // 读取完整输入...

    long long jury_val = readAndVerify(ans, n);
    long long part_val = readAndVerify(ouf, n);

    if (part_val > jury_val) {
        quitf(_fail, "participant output (%lld) is better than jury (%lld)", part_val, jury_val);
    }
    if (part_val < jury_val) {
        quitf(_wa, "expected %lld, found %lld", jury_val, part_val);
    }
    quitf(_ok, "valid construction with value %lld", part_val);
}
```

---

## 4. 强制自测目录约定

在 `verify` 模式下，引擎按目录约定自动编译并运行全部自测，缺失或失败直接阻断验收：

```text
tests/
├── validator/
│   ├── valid/
│   │   ├── 01-min.in            # 必须通过：最小合法边界
│   │   └── 02-typical.in        # 必须通过：典型合法输入
│   └── invalid/
│       ├── 01-n-zero.in         # 必须拒绝：越界（n < 1）
│       ├── 02-sum-exceed.in     # 必须拒绝：sum(n) 超限
│       ├── 03-extra-space.in    # 必须拒绝：行末多余空格
│       └── 04-not-a-tree.in     # 必须拒绝：语义约束违背（成环/不连通）
└── checker/                     # 仅自定义 checker.cpp 必需
    ├── ok-same/{in,out,ans}     # 预期 OK：与 ans 一致
    ├── ok-alt/{in,out,ans}      # 预期 OK：不同但同样合法的构造
    ├── wa-bad/{in,out,ans}      # 预期 WA：不满足题目条件或非最优
    └── pe-fmt/{in,out,ans}      # 预期 PE：非法字符或多余输出（按需）
```

### 验收自查步骤
1. 用 `write_problem` 写好 `validator.cpp`（及自定义 `checker.cpp`）与上述自测文件。
2. 调用 `build_package(mode: "debug")`（无需等大测试集写完即可先验自测），确认 `validator_self_test` 与 `checker_self_test` 全绿。
3. 若任一 `invalid/*.in` 被意外放行，说明 `validator.cpp` 有漏洞，必须补强校验逻辑。
