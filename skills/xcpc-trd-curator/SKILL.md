---
name: xcpc-trd-curator
description: 在 XCPC Workstation 中维护可验证算法库（`snippets/`）、运行级联验板回归、单文件展开（Bundler）与编排出版 Living TRD 模板册（`trds/`）的标准工作流。
---

# XCPC 算法库与 Living TRD 编排 Skill

本 Skill 指导 Agent 在 **XCPC Workstation** 工作区中，遵循「单一事实源、可编译可验证、考场零敲错」原则，维护 `snippets/<slug>/` 算法库并出版 `trds/<id>/` 模板册。

## 1. 核心原则

1. **零冗余依赖声明**：跨算法依赖直接写在 `code.hpp` 的 `#include "<slug>/code.hpp"` 中，**不要**在 `snippet.toml` 手写 `deps`；Core 会自动解析依赖 DAG 并检测环。
2. **代码保持 100% 合法 C++**：`code.hpp` 应以 `#pragma once` 开头，外部编辑器配合 `-I snippets` 即可直接编译与跳转。排入模板册时引擎会自动剥离 `#pragma once` 与同库 `#include`，并按纸面最终代码行计算 KACTL++ 累计 6 位 MD5 校验哈希。
3. **支持纯公式 / 结论卡片**：无 `code.hpp`、仅有 `notes.md` 的条目可直接参与组册（如质数/原根表、数论恒等式、易错清单）。
4. **改底层必跑级联回归**：修改基础板子（如 `modint/code.hpp`）后，所有下游依赖条目会自动标脏（`stale`），必须执行 `verify_snippet(slug, cascade: true)` 跑通全链路回归。
5. **冻结只读与安全边界**：`bundle_code` 在 MCP 下严格受限于工作区路径沙箱；解冻 TRD 仅限人工通过 GUI 或 `xcpc trd unfreeze` 执行，MCP 永不暴露解冻工具。

---

## 2. MCP 工具与 CLI 对照

| 任务 | MCP 工具 | 权限 | 对应 CLI |
| :--- | :--- | :---: | :--- |
| 搜索算法库 | `search_snippets` | 只读 | `xcpc snippet list [--tag …] [--status …] [--formula-only]` |
| 查看条目详情与反向引用 | `get_snippet` | 只读 | `xcpc snippet show <slug>` |
| 创建 / 更新条目与文件 | `write_snippet` | 写入 | `xcpc snippet new <slug>` |
| 验板与级联回归 | `verify_snippet` | 只读执行 | `xcpc snippet verify [<slug>] [--cascade \| --all] [-j N]` |
| 单文件展开（提交 OJ） | `bundle_code` | 只读 | `xcpc bundle <file.cpp> [-I dir] [--compact] [-o out.cpp] [--copy]` / `--snippet <slug>` |
| 查看模板册列表 / 详情 | `get_trd` | 只读 | `xcpc trd list` / `xcpc trd show <id>` |
| 编排章节与元数据 | `compose_trd` | 写入 | `xcpc trd compose <id> --chapter "章节:slug1,slug2"` |
| Preflight 与编译诊断 | `compile_and_diagnose(target: "trd")` | 只读 | `xcpc trd check <id>` |
| 视觉自检（单页 PNG） | `render_page_png(target: "trd")` | 只读 | `xcpc trd render <id> [--page N]` |
| 导出模板册 PDF | `export_pdf(target: "trd")` | 写入 | `xcpc trd build <id> [-o out.pdf] [--emit-typst file.typ]` |
| 定稿冻结快照 | `freeze_trd`（支持 `strict: true`） | 写入 | `xcpc trd freeze <id> [--strict]` |

---

## 3. 标准工作流

### 3.1 新增或沉淀算法板子
1. 用 `search_snippets` 检查是否已有同功能条目，避免重复造轮子。
2. 用 `write_snippet` 写入 `snippet.toml`（`title`、`tags`、`time_complexity`、`space_complexity`、`source_problem`、`[[verify]]`）、`code.hpp`（或省略以建纯公式卡片）和 `notes.md`。
3. 编写独立自测 `test.cpp`（`kind = "self"`），或对接工作区题目（`kind = "problem"`） / Library Checker（`kind = "library_checker"`）的 `verify_*.cpp` 胶水代码。
4. 调用 `verify_snippet(slug)` 确认状态为 `passed`。

### 3.2 重构精简板子与级联回归
1. 先用 `get_snippet(slug)` 查看 `reverse_refs.reverse_deps`（受影响的下游板子）与 `printed_code.total_hash`。
2. 用 `write_snippet` 精简 `code.hpp` 或补充 `notes.md`。
3. 调用 `verify_snippet(slug, cascade: true)` 一次性验证本条目及全部传递下游依赖；任何失败必须定位修复，禁止删掉失败的 `[[verify]]` 项消红。

### 3.3 赛时展开与提交
- 在工作区内将模块化代码展开为单文件：`bundle_code(slug: "ntt", compact: true)` 或 `bundle_code(path: "solutions/sol.cpp")`。
- 赛场本地文件（可在工作区外）使用 CLI：`xcpc bundle main.cpp -I ./include --compact --copy` 或 `-o submit.cpp`。

### 3.4 组册、视觉自检与赛前定稿
1. 用 `compose_trd` 编排章节与模板（`trd-compact` 横版多栏赛场册 / `trd-book` 竖版中文讲义）。
2. 调用 `compile_and_diagnose(target: "trd", id: <id>)` 检查缺失传递依赖（`missing_dependency`）、环依赖、未验/过期告警及排版错误；若漏了上游依赖（如收了 `ntt` 漏了 `modint`），先补齐章节。
3. 调用 `render_page_png(target: "trd", id: <id>, page: 1)` 抽检目录、累计哈希边栏与数学公式排版。
4. 调用 `export_pdf(target: "trd", id: <id>)` 导出 PDF，赛前定稿时调用 `freeze_trd(id: <id>, strict: true)` 固化快照。
