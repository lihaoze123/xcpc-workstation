---
title: 官方 AI Skill 指南
---

# 官方 AI Skill 指南

除了提供底层 [MCP 服务](./mcp.md) 与 [CLI 工具](./cli.md) 外，XCPC Workstation 还提供三套官方 AI Skill，用于将算法竞赛出题、算法模板与团队参考手册（TRD）维护、以及插件开发的工程规范固化为可复用的 Agent 工作流。

---

## 安装与启用方式

### 1. 通过 `xcpc init --with-skills` 安装内置出题 Skill

初始化工作区时附加 `--with-skills`，`xcpc` 会将内嵌在二进制中的 **`xcpc-problem-creator`** 完整 Skill 套件原子化写入工作区的 `.agents/skills/xcpc-problem-creator/` 目录：

```bash
# 初始化空白工作区并安装内置出题 Skill
xcpc init ./my-contest --with-skills

# 或同时填充示例题目与比赛
xcpc init ./my-contest --example --with-skills
```

该命令会写入以下 5 个文件：

```text
.agents/skills/xcpc-problem-creator/
├── SKILL.md                    # 端到端出题编排主流程与反偷懒红线
├── mistake-taxonomy.md         # XCPC / OI 常见错解与对抗造数分类学
├── judge-validator/SKILL.md    # Validator、Checker、Interactor 与自测规范
├── solutions-tutorial/SKILL.md # 标程、暴力/部分分解、对拍与题解规范
└── wrong-gen/SKILL.md          # testlib.h 生成器、子任务与错解矩阵规范
```

!!! note "安全重复运行"
    `xcpc init --with-skills` 不会覆盖工作区中已手动编辑过的 `AGENTS.md`，可用于在已有工作区中补装或更新内置出题 Skill 文件。

### 2. 安装其他官方 Skill（`xcpc-trd-curator` 与 `xcpc-plugin-creator`）

仓库的 `skills/` 目录同时维护了其余两个官方 Skill：

- `skills/xcpc-trd-curator/SKILL.md`
- `skills/xcpc-plugin-creator/SKILL.md`

如需在工作区或全局 Agent 环境中使用它们，可直接将对应目录复制到工作区的 `.agents/skills/` 下（或复制到所用 AI 工具的全局 Skills 目录）。搭配 `xcpc mcp` 接入后，Agent 在识别到对应任务意图时会自动加载相关规范。

---

## 三大官方 Skill 详解

### 1. `xcpc-problem-creator` — 题目构建、验收与出版

- **适用场景**：端到端创作或重构算法竞赛题目，覆盖**传统题**、**stdio 交互题（`interactor.cpp`）**、**多轮通信题（`runs = N`）**与 **OI 子任务 / 部分分（`[[subtask]]`）**。
- **核心协作流程（7 步闭环）**：
  1. **审题与初始化**：确认题型、核心做法、数据范围、时限/内存与判定方式，调用 `inspect_package` / `init_package` 建立骨架。
  2. **编写题面与样例**：通过 `write_problem` 编写 `problem.toml`、`statement.md`（按二级标题划分章节，交互题补充 `## 交互方式`，OI 题补充 `## 子任务`）与 `samples/01..NN.{in,ans}`。
  3. **Validator / Checker / Interactor 与自测**：编写带 `testlib.h` 校验的 `validator.cpp`（支持 `--group` 子任务校验）、按需编写 `interactor.cpp` / 自定义 `checker.cpp`，并配齐正反例自测目录。
  4. **标程、暴力与对拍**：编写 `std.cpp` 与独立思路的 `brute.cpp`（或部分分解），使用 `stress_package` 进行多形态随机对拍，反例自动落盘 `tests/stress-<n>.in`。
  5. **对抗造数与错解矩阵**：依据 `mistake-taxonomy.md` 编写结构化生成器与定向错解（WA / TLE / ILE / 部分分解），用 `build_package(mode: "debug")` 确认每个错解被数据稳定卡掉。
  6. **严格验收与撰写题解**：执行全量 `build_package(mode: "verify")` 满足全部硬门禁（含标程最慢点 `≤ 50%` 时限、子任务依赖无环等），并撰写与标程一致的 `editorial.md`。
  7. **出版与视觉自检**：调用 `compile_and_diagnose` 检查题面与题解册无错误/无溢出，再调用 `render_page_png` 逐页目检公式、样例表与分页。
- **内建反偷懒红线**：严禁私自放宽 `[limits]`、严禁删除失败测试点或改松 `expect`/`score` 消红、严禁空壳 Validator 或静默覆盖手工样例输出、严禁以 `debug` 模式冒充全量 `verify` 验收。

### 2. `xcpc-trd-curator` — 算法模板库与团队参考手册（TRD）维护

- **适用场景**：整理与重构可验证算法模板（`snippets/`）、维护跨模板依赖与级联验证、展开单文件代码提交 OJ，以及编排出版赛场团队参考手册（`trds/`）。
- **核心协作流程**：
  1. **新增与沉淀模板**：先用 `search_snippets` 查重，再通过 `write_snippet` 编写合法 C++ 的 `code.hpp`（以 `#pragma once` 开头，依赖直接通过 `#include "<slug>/code.hpp"` 声明，无需重复维护依赖表）与 `notes.md`（支持无代码的纯公式卡片），配套 `self` / `problem` / `library_checker` 三源验证并调用 `verify_snippet` 确认 `passed`。
  2. **重构精简与级联回归**：重构底层模板前先用 `get_snippet` 检查 `reverse_deps` 与累计短哈希；修改后必须执行 `verify_snippet(slug, cascade: true)` 一次性回归全部下游依赖，禁止删除失败的 `[[verify]]` 项消红。
  3. **单文件展开**：通过 `bundle_code`（或终端 `xcpc bundle`）将模块化模板递归展开为单文件 C++，支持去注释紧凑模式。
  4. **组册与赛前定稿**：使用 `compose_trd` 编排章节与模板（`trd-compact` 横版多栏 / `trd-book` 竖版讲义），调用 `compile_and_diagnose(target: "trd")` 排查缺失传递依赖与环依赖，用 `render_page_png` 抽检目录与累计哈希边栏，最终通过 `freeze_trd(strict: true)` 固化快照。

### 3. `xcpc-plugin-creator` — 混合沙箱插件全生命周期开发

- **适用场景**：设计、编写、测试、热重载联调与打包 XCPC 插件，覆盖 **OJ 数据源（`oj_source`）**、**无头表单工具（`tool`）**与 **纯 Web `<WebView>` 交互工具（`tool` + `commands`）**。
- **核心协作流程（6 步闭环）**：
  1. **初始化脚手架**：根据复杂度选择 `xcpc plugin init <dir> --kind <tool|oj> --vite`（推荐，基于 `@xcpc/plugin-sdk` 与 Vite 单文件内联构建）或零构建纯文件模式。
  2. **实现 `manifest.toml` 与 QuickJS 逻辑**：遵循 `manifest.schema.json` 与最小权限原则（仅声明必需 `domains`、`credentials`、`workspace_read`），在 `src/index.ts` / `index.js` 中使用 `host.*` API 实现导出函数。
  3. **实现纯 Web UI（可选）**：单文件自包含 `ui.html`，消费宿主 `--xcpc-*` 主题变量并订阅 `window.xcpc.onThemeChange`，所有外部网络请求委托 `window.xcpc.runTool`，状态持久化使用 `window.xcpc.storage`。
  4. **构建与沙箱验证**：执行 `npm run build` 后运行 `xcpc plugin check dist`（静态校验与 QuickJS 导出干跑）与 `xcpc plugin test dist <action> --args ...`。
  5. **桌面端热重载联调**：运行 `xcpc plugin link dist`（含敏感权限时执行 `xcpc plugin trust <id>`）配合 `npx vite build --watch`，在桌面端逐项检查深浅主题切换、状态恢复与宿主能力桥接。
  6. **打包与冷安装验收**：运行 `xcpc plugin pack dist` 生成带 SHA-256 校验的 `.zip`，解除开发链接后用 `xcpc plugin install <.zip>` 做完整冷安装验收。
- **内建安全红线**：禁止在 WebView 内直连外网或引入外部 CDN、禁止假设 `allow-same-origin` 或访问 `localStorage` / `window.nsh`、禁止在 QuickJS 中混入 Node.js 或 DOM API、禁止跳过 `xcpc plugin check` 与 `xcpc plugin test` 直接交付。

---

## 推荐搭配方式

| 任务目标 | 推荐工具组合 |
| :--- | :--- |
| 端到端出题与验题 | `xcpc mcp` + `xcpc-problem-creator` Skill + [题目构建与导出](../problem-build/index.md) |
| 维护算法模板与赛前出册 | `xcpc mcp` + `xcpc-trd-curator` Skill + [算法模板与参考手册](../templates-trd/index.md) |
| 开发 OJ 数据源或交互工具插件 | `xcpc plugin` CLI + `xcpc-plugin-creator` Skill + [插件开发指南](../plugins/development.md) |
