---
title: 算法模板与单文件展开
---

# 算法模板与单文件展开

每个算法模板存放在 `snippets/<slug>/` 目录下，目录名 `slug` 即其不可变 ID。模板既可以是带 `code.hpp` 的可复用 C++ 代码单元，也可以是仅含 `notes.md` 的纯公式与结论卡片。

## 目录结构与 `snippet.toml`

标准代码模板目录包含 `snippet.toml`、`code.hpp`（可选）、`notes.md`（可选）以及若干验证脚本（`test.cpp`、`verify_*.cpp`）：

```toml
title = "快速数论变换（NTT）"
tags = ["poly", "math"]
time_complexity = "O(n log n)"
space_complexity = "O(n)"
description = "998244353 模数下的多项式乘法"
source_problem = "poly-mul"          # 可选：记录提炼自哪道工作区题目

[[verify]]
kind = "self"
file = "test.cpp"

[[verify]]
kind = "library_checker"
problem = "convolution_mod"
adapter = "verify_conv.cpp"
```

| 字段 | 必填 | 说明 |
| :--- | :--- | :--- |
| `title` | 是 | 显示标题（用于搜索、手册小节标题） |
| `tags` | 否 | 字符串数组，用于按标签筛选 |
| `time_complexity` | 否 | 时间复杂度说明（亦兼容 `time-complexity`） |
| `space_complexity` | 否 | 空间复杂度说明（亦兼容 `space-complexity`） |
| `description` | 否 | 简短用途说明（用于元信息展示与检索，并作为 `item.description` 传入 Typst 模板） |
| `source_problem` | 否 | 关联的工作区题目 slug（亦兼容 `source-problem`） |
| `[[verify]]` | 否 | 验证来源声明，支持 `self`、`problem`、`library_checker`（详见[三源自动验证](./verification.md)） |

### 纯公式与结论卡片

当目录中**不包含 `code.hpp`**（仅保留 `snippet.toml` 与 `notes.md`）时，该条目会被识别为**纯公式卡片**（例如数论结论、常见反演公式、计算几何推导等）。纯公式卡片在手册中直接以 Markdown + LaTeX 数学排版呈现，不参与代码哈希计算，也不会在严格定稿时要求代码验证。

### 常用管理命令

```bash
# 列出所有模板（支持按标签、验证状态或纯公式卡片筛选）
xcpc snippet list
xcpc snippet list --tag poly --status passed
xcpc snippet list --formula-only

# 查看单个模板详情（含直接依赖、反向引用、关联题目、总校验哈希与验证状态）
xcpc snippet show ntt

# 创建新模板（附加 --formula-only 创建纯公式卡片）
xcpc snippet new segtree --title "懒标记线段树" --tag ds --time "O(log n)" --space "O(n)"
xcpc snippet new mobius-notes --title "莫比乌斯反演常用结论" --formula-only

# 删除模板
xcpc snippet delete old-template --yes
```

## 依赖 DAG 与头文件引用约定

模板之间的依赖关系**不在 `snippet.toml` 中重复手写**，而是由系统直接解析 `code.hpp` 中的标准 C++ `#include` 指令自动派生：

```cpp
// snippets/ntt/code.hpp
#pragma once
#include "modint/code.hpp"
#include <algorithm>
#include <vector>
```

- **引用格式**：使用 `#include "<slug>/code.hpp"`（亦兼容 `"../<slug>/code.hpp"` 等相对路径形式）。
- **原生可编译**：验证引擎编译时会自动传入 `-I <workspace>/snippets`，因此模板和本地解题代码无需任何自定义预处理即可被 `g++` / `clang++` 直接编译。
- **拓扑排序与环检测**：系统全局扫描 `snippets/*/code.hpp` 构建有向无环图（DAG），自动维护每个条目的直接依赖（`deps`）、传递依赖（`transitive_deps`）与反向依赖（`reverse_deps`）；若存在循环 `#include`，会在展开、验证或手册预检时给出精确的环路径报错。
- **排版自动剥离**：在生成团队参考手册的印刷视图时，`#pragma once` 与 `#include "<slug>/code.hpp"` 行会自动从代码体中剥离，并转为条目头部的依赖索引标注，避免浪费纸面空间。

## 单文件展开器（`xcpc bundle`）

在赛场或日常刷题中，本地代码可以自由 `#include` 工作区模板或自定义头文件；提交到 OJ 时，使用 `xcpc bundle` 将所有本地依赖按拓扑顺序内联展开为一份自包含、可直接提交的单文件 C++ 源码：

```bash
# 展开本地解题文件并直接复制到系统剪贴板
xcpc bundle main.cpp --copy

# 展开本地解题文件并写出到指定路径，同时附带额外头文件搜索目录
xcpc bundle main.cpp -I ./include -I ../lib -o submit.cpp

# 直接展开某个工作区模板及其全部传递依赖（输出到 stdout）
xcpc bundle --snippet ntt

# 紧凑模式：剥离注释、合并多余空行并省略边界分隔注释，适合直接提交
xcpc bundle main.cpp --compact --copy
```

| 参数 | 说明 |
| :--- | :--- |
| `<file.cpp>` | 待展开的本地 C++ 源文件（与 `--snippet` 二选一） |
| `--snippet <slug>` | 展开工作区内的指定模板及其所有传递依赖 |
| `-I, --include-dir <DIR>` | 额外的 `#include "..."` 搜索目录（可重复指定） |
| `-o, --output <PATH>` | 将展开后的代码写入指定文件（未指定 `-o` 且未指定 `--copy` 时打印到 stdout） |
| `--copy` | 将展开后的代码复制到系统剪贴板 |
| `--compact` | 剥离 C/C++ 注释、折叠多余空行并省略 `// --- <rel> ---` 文件边界注释 |

展开过程会自动完成以下处理：

1. 按深度优先拓扑顺序递归解析并内联所有本地 `#include "..."`，自动处理 `#pragma once` 去重，保证每个头文件仅内联一次。
2. 收集并去重所有系统头文件 `#include <...>`，统一提升至输出文件顶部。
3. 在非 `--compact` 模式下保留 `// --- <rel> ---` 边界注释，便于人工审阅与调试。

## KACTL++ 逐行累计校验短哈希

在纸质参考手册抄写代码时，一处拼写错误就可能耗费大量现场调试时间。XCPC Workstation 内置了与 KACTL 体系兼容的纯 Rust **逐行累计 6 位 MD5 短哈希**引擎：

### 预处理与计算原理

1. 对印刷视图代码（已剥离 `#pragma once` 与模板间 `#include "<slug>/code.hpp"`；若开启 `strip-comments` 则先剥离注释并折叠空行）逐行累加前缀 `1..=N`。
2. 剥离 C/C++ 注释（正确保留字符串字面量、字符字面量与 C++11 原始字符串 `R"delim(...)delim"`），并移除所有 POSIX 空白字符（空格、制表符、换行符等）。
3. 对压缩后的字节流计算 RFC 1321 MD5 摘要，取前 6 位小写十六进制字符作为校验短哈希。
4. 默认每隔 `hash-interval` 行（默认 `10` 行）、在顶层函数/结构体闭合 `}` 处以及代码最后一行印出累计哈希（若某行位于未闭合的多行块注释 `/* ...` 内部则自动跳过，避免干扰现场核对）。

### 赛场终端一键核对

该算法与 Linux 赛场环境下的单行核对命令**逐字节等价**（当 `hash-interval > 0` 时，该命令也会自动印在手册目录下方）：

```bash
sed -n '1,Np' a.cpp | cpp -dD -P -fpreprocessed | tr -d '[:space:]' | md5sum | cut -c-6
```

抄写完代码后，先将 `N` 设为最后一行核对总哈希；若不一致，用二分法将 `N` 设为手册旁注的中间检查点行号，3–4 次即可将笔误精确锁定到 10 行以内。
