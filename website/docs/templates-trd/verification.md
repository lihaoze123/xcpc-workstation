---
title: 三源自动验证
---

# 三源自动验证

为确保团队参考手册里的每一份算法代码都经过真实测试而非「凭印象维护」，XCPC Workstation 为算法模板设计了统一的**三源验证引擎**：支持自包含单元测试、工作区本地题库联测，以及直连 [Library Checker](https://judge.yosupo.jp/) 权威模板题库自动拉取验证。

## 三种验证来源

在 `snippets/<slug>/snippet.toml` 中可以声明一个或多个 `[[verify]]` 条目，引擎会依次执行全部验证源并汇总结果：

=== "`self` 自包含测试"

    适用于轻量单元测试或自带随机对拍的脚本：

    ```toml
    [[verify]]
    kind = "self"
    file = "test.cpp"
    ```

    - `file` 为相对于 `snippets/<slug>/` 的 C++ 文件路径，内部可直接 `#include "<slug>/code.hpp"`。
    - 编译时自动传入 `-I <workspace>/snippets`，退出码为 `0` 即判定通过（`AC`）；触发 `assert()` 失败时会自动清理缓存路径前缀，展示清晰的断言位置。

=== "`problem` 工作区题库关联"

    将模板直接对接到当前工作区 `problems/<problem>/` 下的真实题目数据与评测器：

    ```toml
    [[verify]]
    kind = "problem"
    problem = "my-poly-mul"
    adapter = "verify_local.cpp"
    ```

    - `adapter` 是位于 `snippets/<slug>/` 下的薄包装程序，负责读入题目输入、调用模板核心函数并输出结果。
    - 引擎优先读取 `build/problems/<problem>/tests/` 中已构建好的完整数据；若尚未构建且该题为构建包，会自动触发一次开发态构建；若为纯题面条目，则回退使用 `samples/` 中的样例。
    - 自动使用该题配置的 Checker（内置标准 Checker 或自定义 `checker.cpp`）与时间/内存限制进行评测。

=== "`library_checker` 权威模板题"

    直接引用 `yosupo06/library-checker-problems` 仓库中的标准题目对算法模板进行强数据验证：

    ```toml
    [[verify]]
    kind = "library_checker"
    problem = "convolution_mod"      # 支持题名或 "category/problem" 形式
    adapter = "verify_conv.cpp"
    ```

    - **轻量按需拉取与全局缓存**：首次验证时，仅拉取该题的 `info.toml`、`checker.cpp`、`sol/correct.cpp`、`gen/` 与 `common/`，缓存至 `~/.cache/xcpc/library-checker/<problem>/`（受 `XCPC_CACHE_DIR` 覆盖），随后在本地使用原生编译器生成测试数据并复用缓存，后续验证完全离线零网络开销。
    - **标准限制**：时间限制取自题目 `info.toml` 的 `timelimit`，内存限制采用 Library Checker 标准的 `1024 MiB`。

## Library Checker 镜像、本地仓库与离线降级

为适应国内网络环境与无网/弱网赛前备战场景，Library Checker 题源解析遵循严格的优先级顺序：

1. **本地缓存命中**：若 `~/.cache/xcpc/library-checker/<problem>/` 下已存在 `info.toml`、`checker.cpp` 与 `sol/correct.cpp`，直接离线复用，不发起任何网络请求。
2. **本地 Git 仓库**：若配置了本地 `library-checker-problems` 克隆路径（在设置 → 工具链的「权威题库验板源」中配置「本地克隆目录」，或设置环境变量 `XCPC_LC_REPO`），直接从本地仓库拷贝题源到缓存目录。
3. **远程 HTTP 拉取（支持镜像）**：从 GitHub Raw 拉取；可通过设置中的「镜像前缀」或环境变量 `XCPC_LC_MIRROR` 指定镜像前缀（例如 `https://ghproxy.net/https://raw.githubusercontent.com`）。

!!! note "离线安全降级（不误杀已验证状态）"
    若在离线或网络异常环境下触发包含 `library_checker` 的验证，且本地尚未缓存该题，该项验证会返回 `UNAVAILABLE` 而非判定为 `failed`。系统**绝不会**因为网络不可达而将原本已 `passed` 的模板降级为 `failed`：若内容哈希未变，保留原 `passed` 状态与磁盘缓存；若代码已修改，则保持 `stale`。

## 验证缓存与状态流转

每次验证完成后，结果以 JSON 形式原子写入工作区内的 `.xcpc/snippet-verify/<slug>.json`。

### 内容哈希覆盖范围

系统为每个模板计算确定性 SHA-256 验证键（`content_hash`），包含以下全部输入：

- 自身的 `snippet.toml` 与 `code.hpp`
- **所有传递依赖**的 `code.hpp`
- 每一个 `[[verify]]` 对应的测试/适配器源码文件
- 若包含 `kind = "problem"`，还包括对应题目的包内容哈希（或样例内容）

### 四种派生状态

读取状态时无需编译或执行任何代码，系统静态比对当前 `content_hash` 与 `.xcpc/snippet-verify/<slug>.json` 中记录的哈希即可即时得出：

| 状态 | 含义 |
| :--- | :--- |
| `passed` | 所有 `[[verify]]` 项全部通过，且当前内容哈希与缓存一致 |
| `stale` | 曾有验证记录，但自身代码、任意上游依赖、验证脚本或关联题目数据已发生变更 |
| `failed` | 当前内容哈希对应的最近一次验证存在失败项（`WA` / `TLE` / `MLE` / `RE` / `CE` / `JE`） |
| `unverified` | 从未运行过验证，或未声明任何 `[[verify]]` 条目 |

!!! tip "上游修改的自动过期传播"
    假设 `ntt` 依赖 `modint`。一旦你修改了 `snippets/modint/code.hpp`，即使未触碰 `ntt` 的任何文件，`ntt` 的 `content_hash` 也会立即变化，其状态会自动变为 `stale`，提醒你重新回归验证。

## 验证命令与级联回归

```bash
# 验证单个算法模板（若内容哈希未变且已有 passed/failed 缓存则直接命中返回）
xcpc snippet verify modint

# 级联回归：按拓扑顺序依次验证 modint 及其所有直接/传递下游依赖（如 ntt）
xcpc snippet verify modint --cascade

# 全库验证：按上游优先的拓扑顺序验证工作区中的所有模板
xcpc snippet verify --all

# 强制忽略已有缓存，重新编译并运行验证
xcpc snippet verify ntt --force

# 指定构建并行度（透传给底层构建引擎）
xcpc snippet verify --all -j 4
```

| 参数 | 说明 |
| :--- | :--- |
| `<slug>` | 指定要验证的模板 slug（与 `--all` 二选一） |
| `--cascade` | 同时按拓扑顺序验证依赖 `<slug>` 的全部下游模板 |
| `--all` | 按拓扑顺序验证工作区内的所有算法模板 |
| `--force` | 忽略 `.xcpc/snippet-verify/<slug>.json` 中的命中缓存，强制重跑 |
| `-j, --jobs <N>` | 指定底层题库构建并行度 |

在桌面端「模板」空间中，可以在条目详情页点击「运行验板」，或在编辑器「验板」标签页中点击「验证本条及下游依赖」并查看各项验证源的判定结果、通过用例数与错误信息；如需验证全库，可通过命令面板运行「验证全部模板」。
