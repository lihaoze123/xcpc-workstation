---
title: 数据包导入与导出
---

# 数据包导入与导出

XCPC Workstation 原生支持从 **Codeforces Polygon** 标准数据包（单题或整场 ZIP）无损导入至本地开放目录工作区，并可将通过验收的题目或整场比赛导出为 **DOMjudge** 或 **Hydro OJ** 可直接上传的最小集合 ZIP 数据包。

## 导入 Polygon 数据包

### 单题与整场导入

`xcpc import polygon` 会自动识别压缩包内的 `problem.xml`（单题）或 `contest.xml`（整场），在内存中完成全部校验与转换后原子写入工作区（解析失败时不修改工作区任何文件，且内置 Zip Slip 路径穿越防护与体积/条目数上限保护）：

```bash
# 导入单题 Polygon ZIP（默认使用 problem.xml 中的 short-name 作为 slug）
xcpc import polygon './a-plus-b-12$linux.zip'

# 显式指定目标题目 slug 与所属分组
xcpc import polygon './a-plus-b-12$linux.zip' --id my-a-plus-b --group "2026 训练赛"

# 导入整场 Polygon 比赛 ZIP（自动创建所有题目与 contests/<id>/contest.toml）
xcpc import polygon ./regional-contest.zip

# 整场导入默认按比赛名称自动创建并归属分组；可覆写分组名或关闭自动分组
xcpc import polygon ./regional-contest.zip --group "ICPC 2026 区域赛"
xcpc import polygon ./regional-contest.zip --no-auto-group
```

| 参数 | 说明 |
| :--- | :--- |
| `--id <ID>` | 覆写导入后的题目 slug（单题）或比赛 ID（整场） |
| `--group <NAME>` | 将导入的题目统一归入指定的持久化分组（不存在时自动注册到 `workspace.toml`） |
| `--no-auto-group` | 仅对整场导入生效：关闭「按比赛名自动归入分组」的默认行为 |

### 自动还原的内容

导入器会尽可能完整地将 Polygon 资产映射到工作区结构：

- **题面转换**：自动解析 `statement-sections/<lang>/` 或 `statements/<lang>/problem.tex`（优先还原中英文），将 LaTeX 章节、数学公式、列表与表格转换为工作区 Markdown（`statement.zh.md` / `statement.en.md`），并保留原样例到 `samples/`。
- **元数据与限制**：还原标题、时间/内存限制、标准 Checker 名称或自定义 `checker.cpp`、`validator.cpp`、所有生成器与解法源码及其预期判定（将 Polygon 标签映射为 `main = true` 与 `AC`、`WA`、`TLE`、`MLE`、`PE` 等）。
- **交互、通信与子任务**：自动识别并还原 `interactor.cpp`、`[package].runs` 多轮通信配置，以及 Polygon 的测试点分组（`<groups>`）与依赖关系到 `[[subtask]]`。
- **冲突保护**：若目标题目 slug 或比赛 ID 在工作区中已存在，导入器会直接拒绝并报错，不会覆盖已有数据。

## 导出 OJ 数据包（DOMjudge / Hydro）

### 「动静分离」与验收哈希门控

`xcpc export` 严格遵守「动静分离」原则：**导出过程绝不编译或执行任何 C++ 代码**，而是直接打包 `build/problems/<slug>/tests/` 中已构建好的测试数据，并在打包前用 Typst 引擎实时编译题面 PDF 嵌入包内。

默认情况下，导出器会先执行静态新鲜度检查（`package_hash` 比对 `verified_report.json`）：

- 仅当题目处于 **`verified_fresh`**（最新严格验收通过且此后文件未改动）时才允许导出。
- 导出整场比赛（`export <format> contest <id>`）时，会先预检场内**所有题目**，只要有一题未通过检查就整体中止，不会在磁盘留下残缺输出。
- 批量导出多题（`export <format> problems <id1> <id2> ...`）时，未通过验收或缺少测试数据的题目会被记为跳过并汇总报告，不中断其余题目的导出。
- 如确需跳过严格验收门控（例如开发期临时测试上传），可传入 `--force` 参数；但即使使用 `--force`，题目仍必须已通过开发态构建产出 `build/problems/<slug>/tests/` 中的测试数据。

### CLI 用法

格式名支持 `domjudge`（别名 `dj`）与 `hydro`（别名 `hydrooj`）：

```bash
# 导出单题为 DOMjudge ZIP（默认输出至 build/<slug>-domjudge.zip）
xcpc export domjudge problem a-plus-b -o ./dist/a-plus-b-dj.zip

# 导出单题为 Hydro ZIP
xcpc export hydro problem tree-dp -o ./dist/tree-dp-hydro.zip

# 导出整场比赛（输出为目录，内含按题号命名的 <Label>-<slug>.zip）
xcpc export domjudge contest icpc-2026 -o ./dist/icpc-2026-dj/

# 批量导出多个题目（未指定 -o 时输出至 build/batch-<format>-<YYYYMMDD-HHMMSS>/）
xcpc export hydro problems a-plus-b tree-dp guess-number

# 强制导出未严格验收的题目（仍需已有构建好的测试数据）
xcpc export domjudge problem a-plus-b --force
```

!!! tip "插件自定义导出器"
    `xcpc export` 与 `xcpc import` 同样支持由已启用插件提供的自定义格式（`<plugin>:<exporter>` / `<plugin>:<importer>`），可附加 `--with-pdf` 将编译好的题面 PDF 以 Base64 传给插件导出器。详见 [插件生态与开发](../plugins/index.md)。

## 导出 ZIP 内部结构对照

=== "DOMjudge（对齐 Polygon2DOMjudge）"

    ```text
    <problem>.zip
    ├── domjudge-problem.ini          # short-name、externalid、timelimit（LF 换行，不含 color）
    ├── problem.yaml                  # name、limits.memory（MiB）、validation、validator_flags
    ├── problem.pdf                   # 由当前工作区 Typst 模板实时编译的题面 PDF
    ├── data/
    │   ├── sample/                   # 自适应补零（>=100 点自动 3 位），保持字典序
    │   │   ├── 01.in
    │   │   └── 01.ans
    │   └── secret/
    │       ├── 01.in
    │       └── 01.ans
    └── output_validators/            # 仅交互题或自定义 checker.cpp 时生成
        ├── interactor/               # 或 checker/
        │   ├── interactor.cpp        # （或 checker.cpp）
        │   ├── testlib.h             # 有效 testlib.h
        │   ├── build                 # 0755 可执行编译脚本
        │   └── run                   # 0755 可执行运行封装（将 testlib 退出码映射为 42/43）
    ```

    - **内置 Checker 映射**：`wcmp`/`ncmp`/`lcmp` → `default`（`case_sensitive`）；`fcmp` → `space_change_sensitive case_sensitive`；`rcmp4`/`rcmp6`/`rcmp9` → `float_tolerance 1e-4/1e-6/1e-9`；`yesno`/`nyesno` → `default`（大小写不敏感）。
    - **平台差异警告**：DOMjudge 为 ICPC 赛制，导出包含 `[[subtask]]` 的题目时会将所有隐藏测试点扁平化放入 `data/secret/` 并产出警告；交互题仅打包 `interactor.cpp`（不附带独立 Checker）。

=== "Hydro OJ"

    ```text
    <problem>.zip
    ├── problem.yaml                  # title 与 tag 列表
    ├── problem_zh.md                 # 或 problem_en.md，内嵌 @[pdf](file://problem.pdf?noDisposition=1)
    ├── additional_file/
    │   └── problem.pdf               # 由当前工作区 Typst 模板实时编译的题面 PDF
    └── testdata/
        ├── config.yaml               # type、time、memory、checker/interactor 与 subtasks 配置
        ├── checker.cpp               # 非默认 Checker 或自定义 checker.cpp 时包含
        ├── interactor.cpp            # 交互题时包含（附 testlib.h）
        ├── 01.in
        └── 01.out
    ```

    - **OI 子任务完整保留**：若题目声明了 `[[subtask]]`，导出器会在 `testdata/config.yaml` 中完整生成 `subtasks` 列表，包含分值（`score`）、计分类型（`min`/`sum`）、前置依赖索引（`if`）与对应测试点映射（`cases`）。
    - **内置 Checker 映射**：`wcmp`/`ncmp`/`lcmp` 映射为 `checker_type: default`；其余内置 Checker（`fcmp`/`rcmp*`/`yesno`/`nyesno`）与自定义 `checker.cpp` 均打包为 `checker_type: testlib` 并附带对应源码。
