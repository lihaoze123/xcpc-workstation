---
name: xcpc-judge-validator
description: 编写严格的 testlib.h validator.cpp（含 --group 子任务分组校验）、checker.cpp（含 quitp 部分分与二段复判）、interactor.cpp（stdio 交互协议、防死锁 ILE、防信息泄露）、runs = N 多轮通信以及构建强制自测用例的指南。
---

# Validator、Checker 与 Interactor 编写指南

本子 Skill 隶属 [`xcpc-problem-creator`](../SKILL.md)，负责把关题目包的输入合法性校验、输出判定、交互协议与多轮通信执行。

> **核心原则**：严禁空壳 `validator.cpp`、假 `checker.cpp` 与敷衍 `interactor.cpp`。严格验收态（`verify`）强制要求 `validator`、自定义 `checker` 分别通过正反两面自测，并要求交互题通过标程 + 错解验证协议正确性。

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
| `"checker.cpp"` | 自定义判定逻辑 | 构造题、多解等价、`quitp` 部分分、二段复判、无交互器多轮通信 |

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

### 2.3 OI 子任务分组校验（`--group <id>`）

当 `[[test]]` 声明了 `subtask = "<id>"` 时，引擎运行 `validator` 会**自动附带 `--group <id>`** 参数（未使用 `validator.group()` 的简单 validator 会安全忽略该参数，完全向后兼容）：

```cpp
std::string grp = validator.group(); // 无 --group 时为空串 ""
int max_n = 200000;
if (grp == "1") max_n = 100;
else if (grp == "2") max_n = 5000;

int n = inf.readInt(1, max_n, "n");
// 特殊性质示例：子任务 "chain" 要求树为链
if (grp == "chain") {
    ensuref(max_deg <= 2, "subtask chain requires max degree <= 2");
}
```

- 无 `--group` 调用（如 `samples/` 和根目录 `tests/validator/{valid,invalid}/*.in`）应按全题最大范围校验；
- 子任务专属约束务必写入对应 `grp == "<id>"` 分支，并在 `tests/validator/{valid,invalid}/<subtask_id>/*.in` 补齐正反自测，防止造错数据导致子任务越界或特殊性质失效。

---

## 3. 编写自定义 `checker.cpp`

当题目存在多解、构造输出、自定义误差、`quitp` 部分分、二段复判或无交互器多轮通信时，编写 `checker.cpp` 并在 `problem.toml` 设 `[package].checker = "checker.cpp"`（或省略 `[package].checker` 让 `checker.cpp` 自动生效）。

### 3.1 必守规范

1. **入口初始化**：首行调用 `registerTestlibCmd(argc, argv);`。
2. **三流职责**：`inf`（输入）、`ouf`（选手输出，或二段复判时 `interactor` 写出的 `tout`）、`ans`（标程或手工参考答案）。
3. **同源校验 `ouf` 与 `ans`（铁律）**：
   - 将解析与合法性检查封装为 `readAndVerify(InStream& stream, ...)`，分别对 `ans` 和 `ouf` 调用；
   - 若 `ans` 本身不合法，流读取会自动报 `_fail`（JE），立即暴露标程 bug；
   - 对最优化 + 构造题：若 `ouf` 合法且结果**严格优于** `ans`，必须调用 `quitf(_fail, "participant found better answer than jury")` 触发 JE，而非静默判 WA 或 AC！
4. **判定码与 `quitp` 部分分**：
   - `_ok`：完全正确（得分比例 `1.0`）；
   - `_wa`：语义错误、构造不满足条件、非最优解（得分比例 `0.0`）；
   - `_pe`：格式错误（`testlib` 读取类型不匹配或多余 token 时会自动触发 `_pe`）；
   - `_fail`：标程答案不合法或不如选手答案（触发 Judge Error）；
   - **`quitp(double ratio, const char* fmt, ...)`**：引擎解析 `testlib.h` 的 `quitp` 退出码（exit code 7）与比例 `ratio` $\in [0.0, 1.0]$，用于按比例给分（如查询次数越少得分越高、近似解按比值计分）；务必保证 `ratio` 严格落在 `[0.0, 1.0]` 且无 `NaN`（满分为 `quitf(_ok, ...)`，零分为 `quitf(_wa, ...)`）。
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

## 4. 编写 `interactor.cpp`（stdio 交互题）

题目根目录存在 `interactor.cpp` 时，引擎自动识别为交互题，并通过双管道代理运行器转发 `solution <-> interactor` 双向流，同时录制有界交互日志（`transcript`：头尾各若干行 `→` 交互器发出 / `←` 选手发出）用于失败诊断与对拍。

### 4.1 `testlib.h` 协议与流约定

- **入口初始化**：首行调用 `registerInteraction(argc, argv);`（参数为 `<input> <output>`）。
- **四流职责**：
  - `inf`：读取当前测试点的隐藏输入文件（`<test.in>`）；
  - `std::cout`：向选手进程的 `stdin` 发送初始信息与每次询问回复（**每次发送后必须 flush**）；
  - `ouf`：读取选手进程的 `stdout`（询问与最终答案）；
  - `tout`：写出供二段 `checker` 复判（或多轮通信传递给下一轮）的输出文件。

### 4.2 协议设计、Flush 纪律与防死锁（`ILE`）

引擎严格区分 **`TLE`**（选手 CPU 时间超过时限）与 **`ILE`（Idleness Limit Exceeded）**（选手未用满 CPU 时限，但因未 flush、协议错位或双方互相等待导致墙钟超时）：

1. **每次输出后立即 flush**：`interactor.cpp` 与解法在每次向对方发送完整消息后，必须调用 `std::endl` 或 `std::cout << std::flush;`。若解法写了 `cin.tie(nullptr)`，读取前不会自动 flush `cout`，极易触发 `ILE`。
2. **明确轮次与终止协议**：
   - 在题面 `## 交互方式` 中写清初始输入格式、询问格式（如 `? x`）、回复格式、提交答案格式（如 `! ans`）与最大询问次数 $Q$；
   - 收到非法询问或超过 $Q$ 次询问时，交互器应立即调用 `quitf(_wa, ...)`（若协议约定先回 `-1` 告知选手退出，发送 `-1` 并 flush 后立刻 `quitf(_wa, ...)`，**切勿等待选手再发输入**）；
   - 选手提前退出（EOF）时，`ouf.readToken()` / `ouf.readInt()` 会自动以 `_wa` / `_pe` 结束交互器，避免死等。
3. **`samples/` 默认不纳入评测**：交互题的 `samples/` 多为题面展示用的示例对话，引擎默认不自动将其加入评测；若确有可机器执行的交互样例输入，必须在 `[[test]]` 中显式声明。

### 4.3 防信息泄露与自适应交互器（Adaptive vs Non-adaptive）

1. **明确声明是否自适应**：在 `## 交互方式` 中写明交互器是**非自适应（non-adaptive，隐藏数据在交互开始前固定）**还是**自适应（adaptive，可根据选手询问动态调整）**。
2. **自适应交互器一致性铁律**：自适应交互器必须维护合法解空间，保证在交互结束时**始终存在至少一个满足所有历史回答的合法隐藏对象**；严禁前后回答自相矛盾。
3. **防止格式与侧信道泄露**：
   - 回复格式固定，不得因隐藏值不同而输出多余空格、调试信息或不同错误提示；
   - 不在回复中提前暴露隐藏状态，避免计算耗时随隐藏值显著波动导致时序侧信道；
   - `quitf` 的诊断文本只供裁判/本地日志查看，不会发给选手 `stdin`。

### 4.4 判定模式与二段复判（`interactor` + `checker`）

- **默认模式（单段）**：不配置 `[package].checker` 且无 `checker.cpp` 时，直接由 `interactor.cpp` 调用 `quitf(_ok, ...)` / `quitf(_wa, ...)` / `quitp(ratio, ...)` 给出最终判定。
- **二段复判模式**：当 `problem.toml` 显式配置了 `[package].checker` 或目录存在 `checker.cpp` 时，`interactor.cpp` 正常结束后（`quitf(_ok, ...)`），引擎会将其写出的 `tout` 交给 `checker` 进行最终复判。适用于「交互只负责采集选手决策并记录到 `tout`，由统一 `checker` 核验最优性或与 `ans` 比较」的场景。

### 4.5 最小规范模板（猜数交互题示例）

```cpp
#include "testlib.h"
#include <iostream>
#include <string>

int main(int argc, char* argv[]) {
    registerInteraction(argc, argv);

    int n = inf.readInt(1, 1000000, "n");
    int secret = inf.readInt(1, n, "secret");
    int max_q = inf.readInt(1, 100, "max_q");

    // 发送初始信息并立即 flush
    std::cout << n << std::endl;

    int used = 0;
    while (true) {
        std::string op = ouf.readToken("?|!", "op");
        if (op == "?") {
            int x = ouf.readInt(1, n, "x");
            if (++used > max_q) {
                quitf(_wa, "query limit exceeded: %d > %d", used, max_q);
            }
            if (x < secret) std::cout << "<" << std::endl;
            else if (x > secret) std::cout << ">" << std::endl;
            else std::cout << "=" << std::endl;
        } else {
            int guess = ouf.readInt(1, n, "guess");
            if (guess != secret) {
                quitf(_wa, "wrong answer: expected %d, got %d", secret, guess);
            }
            // 若启用二段复判，可写入 tout << guess << "\n";
            quitf(_ok, "guessed %d in %d queries", secret, used);
        }
    }
}
```

---

## 5. stdio 多轮通信题（`runs = N`）

对齐 Polygon `run-twice`（`<judging run-count="N">`）模型，在 `problem.toml` 中配置：

```toml
[package]
runs = 2                          # 可选，默认 1；>= 2 启用多轮通信模式
# checker = "checker.cpp"         # 可选
```

### 5.1 两种统一形态

1. **有 `interactor.cpp`（推荐用于编解码/带噪声/带篡改通信）**：
   - 第 1 轮运行 `interactor <test.in> <pass1.out>` 与第 1 轮选手进程交互，交互器可将编码结果、篡改后的消息、轮次标记与原始答案一起写入 `tout`（即 `pass1.out`）；
   - 第 $k$ 轮（$2 \le k \le N$）将 `pass(k-1).out` 作为新的 `inf` 再次启动 `interactor` 与第 $k$ 轮选手进程交互（可在 `tout` 首行写轮次号 `2` 区分不同轮次）；
   - 全部 $N$ 轮结束后，若显式配置了 `checker`，再以 `<test.in> <passN.out> <test.ans>` 复判。
2. **无 `interactor.cpp`（纯管道串联）**：
   - 第 1 轮选手读 `<test.in>` 写 `<pass1.out>`；第 $k$ 轮选手读 `<pass(k-1).out>` 写 `<passk.out>`；最终由 `checker` 判定（标程同样跑完 $N$ 轮生成 `.ans`）。

### 5.2 资源计量与防侧信道规范

- **累计 CPU 时限**：总 CPU 时间按各轮累计计入题目时限（$\sum_{k=1}^N \text{cpu}_k \le \text{time\_limit}$）；内存限制每轮独立检查；任一轮出现 `RE` / `TLE` / `ILE` / `WA` 立即中止后续轮次。
- **严防跨轮侧信道偷渡信息**：
  - 各轮选手进程互相隔离（无共享内存/临时文件），唯一合法信道是经交互器或规范格式传递的消息；
  - 在 `interactor.cpp` 中必须**严格校验并规范化**选手第 $k$ 轮发出的消息：校验字符集（如仅 `'0'`/`'1'`）、长度上限、数值范围，剥离多余空白/前导零/行末空格后再写入 `tout`；
  - 写进 `tout` 传给下一轮的数据，若需携带原始输入或密钥供最终复判，**只能由交互器从 `inf` 读入并留在交互器内部**，绝不能原样打印给下一轮选手 `std::cout`。

---

## 6. 强制自测目录约定

在 `verify` 模式下，引擎按目录约定自动编译并运行全部自测，缺失或失败直接阻断验收：

```text
tests/
├── validator/
│   ├── valid/
│   │   ├── 01-min.in            # 必须通过（无 --group）：最小合法边界
│   │   ├── 02-typical.in        # 必须通过（无 --group）：典型合法输入
│   │   └── 1/                   # 可选（OI 子任务）：自动带 --group 1 运行
│   │       └── 01-sub1.in
│   └── invalid/
│       ├── 01-n-zero.in         # 必须拒绝（无 --group）：越界（n < 1）
│       ├── 02-sum-exceed.in     # 必须拒绝（无 --group）：sum(n) 超限
│       ├── 03-extra-space.in    # 必须拒绝（无 --group）：行末多余空格
│       ├── 04-not-a-tree.in     # 必须拒绝（无 --group）：语义约束违背
│       └── 1/                   # 可选（OI 子任务）：自动带 --group 1 拒绝
│           └── 01-exceed-sub1.in
└── checker/                     # 仅自定义 checker.cpp 必需（含二段复判）
    ├── ok-same/{in,out,ans}     # 预期 OK：与 ans 一致
    ├── ok-alt/{in,out,ans}      # 预期 OK：不同但同样合法的构造
    ├── wa-bad/{in,out,ans}      # 预期 WA：不满足题目条件或非最优
    └── pe-fmt/{in,out,ans}      # 预期 PE：非法字符或多余输出（按需）
```

### 交互题门禁说明
- 交互题**不强制**要求单独的 `tests/interactor/` 自测目录，而是通过 **标程（AC）+ 定向错解（WA / ILE / TLE）** 联合验证 `interactor.cpp` 的正常流程与异常拦截（若缺少错解，引擎会给出醒目 warning）。
- 交互题仍强制要求 `validator.cpp` 及其 `valid` / `invalid` 自测；若同时使用自定义 `checker.cpp` 做二段复判，仍强制要求 `tests/checker/{ok,wa}-*` 自测。

### 验收自查步骤
1. 用 `write_problem` 写好 `validator.cpp`（及按需编写 `checker.cpp` / `interactor.cpp`）与上述自测文件。
2. 调用 `build_package(mode: "debug")`（无需等大测试集写完即可先验自测），确认 `validator_self_test` 与 `checker_self_test` 全绿。
3. 若任一 `invalid/*.in`（或 `invalid/<subtask_id>/*.in`）被意外放行，说明 `validator.cpp` 有漏洞，必须补强校验逻辑。
