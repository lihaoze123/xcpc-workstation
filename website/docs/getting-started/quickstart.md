# 快速上手

XCPC Workstation 内置了完整的示例工作区，无需手动配置即可在几分钟内走通题面排版、整本 PDF 出版与命令行校验流程。你可以任选桌面端或命令行开始。

## 方式一：桌面端体验（推荐）

1. 启动 **XCPC Workstation**。
2. 在欢迎页点击 **「新建工作区」**，在弹窗中选择默认模板后点击 **「从示例创建」** 并指定一个空目录释放内置示例工作区；或选择 **「创建空工作区」** 直接创建自己的空白题库。
3. 内置中文示例工作区包含：
   - **4 个持久化分组**与 **12 道示例题目**（涵盖日常题面、Typst 排版示例与可构建验收的题目包）
   - **2 场示例比赛**：采用 `olymp-classic` 模板的 `welcome-round`，以及采用 `cnoi-classic` 模板的 `oi-mock`
   - **4 个示例算法模板**与 **1 份示例团队参考手册（TRD）**（`icpc-regional`）
4. 进入工作区后，可以按以下顺序熟悉核心界面：
   - 在 **题库** 视图浏览分组、检索题目、预览右侧题面，并尝试勾选多道题执行批量操作。
   - 点击任意题目或进入 **比赛 → `welcome-round`** 打开 **出版工作室**，在编辑器修改 Markdown 或公式，观察右侧页面实时增量刷新。
   - 点击顶部导出按钮，分别导出 **题面册 PDF** 与 **题解册 PDF**。
   - 按 ++ctrl+k++ 打开全局命令面板快速跳转题目、比赛或切换视图；输入 `>` 仅检索可执行动作；按 ++ctrl+slash++ 查看完整快捷键列表。

!!! tip "切换到英文示例或界面"
    桌面端支持简体中文与 English 实时热切换（位于 **「设置 → 外观」**）。如需体验英文示例工作区，可通过命令行执行 `xcpc init --example --lang en ./my-xcpc-en` 后在桌面端打开该目录。

---

## 方式二：命令行（CLI）完整走通

以下命令演示如何用 `xcpc` 从零释放示例工作区，并完成检查、PDF 构建与逐页 PNG 渲染。

### 1. 初始化示例工作区

```bash
# 释放中文示例工作区到 ./my-xcpc
xcpc init --example ./my-xcpc
```

常用可选参数：

- `--lang en`（或 `--lang english`）：释放英文示例工作区（包含 `welcome-round` 与 `minimal-round` 两场比赛）。
- `--with-skills`：同时将官方 AI Skill 复制到工作区的 `.agents/skills/` 目录。
- 不带 `--example` 时（`xcpc init ./my-xcpc`）：创建一个干净的空白工作区（含 `workspace.toml`、`problems/`、`contests/`、`templates/`、`.gitignore` 与 `AGENTS.md`）。

### 2. 查看工作区清单与字体环境

```bash
# 列出工作区中的全部题目与比赛
xcpc -w ./my-xcpc list

# 查看排版引擎当前识别到的字体家族
xcpc fonts
```

!!! note "工作区定位规则"
    `xcpc` 按以下优先级确定工作区根目录（以存在 `workspace.toml` 为标志）：
    1. 命令行参数 `-w <DIR>` / `--workspace <DIR>`
    2. 环境变量 `XCPC_WORKSPACE`
    3. 从当前工作目录逐级向上查找

    如果在 `./my-xcpc` 目录内执行命令，后续示例中的 `-w ./my-xcpc` 均可省略。

### 3. 排版检查与预检（`xcpc check`）

`xcpc check` 会完整编译目标文档并运行排版与印刷前预检（不写出任何文件）：

```bash
# 检查整场比赛试题册
xcpc -w ./my-xcpc check contest welcome-round

# 检查整场比赛题解册
xcpc -w ./my-xcpc check contest welcome-round --editorial

# 检查单道题目（--json 输出结构化诊断结果）
xcpc -w ./my-xcpc check problem crc-check --json
```

退出码约定：`0` 表示无错误，`1` 表示文档或预检存在错误，`2` 表示用法、工作区或 I/O 错误。

### 4. 构建 PDF（`xcpc build`）

```bash
# 构建整本试题册与题解册 PDF（默认输出至 my-xcpc/build/）
xcpc -w ./my-xcpc build contest welcome-round
xcpc -w ./my-xcpc build contest welcome-round --editorial

# 仅构建单道题目，或显式指定输出路径
xcpc -w ./my-xcpc build problem crc-check -o ./my-xcpc/build/crc-check.pdf

# 将多道题临时合并为一本 PDF 册子
xcpc -w ./my-xcpc build problems crc-check tree-beacon --title "周末加练" --cover true
```

如需排查排版细节，可以附加 `--emit-typst <file.typ>` 将转换管线生成的 Typst 中间源码写出查看。

### 5. 渲染单页 PNG 预览（`xcpc render`）

```bash
# 渲染 welcome-round 第 1 页为 PNG（默认 scale = 2.0，约 144 DPI）
xcpc -w ./my-xcpc render contest welcome-round --page 1

# 渲染整本试题册的所有页面到指定目录
xcpc -w ./my-xcpc render contest welcome-round -o ./my-xcpc/build/pages --scale 2.0
```

生成的图片命名为 `page-1.png`、`page-2.png`……

## 继续探索

- 想进一步了解开放目录布局与多人 Git 协作：阅读 [工作区与开放目录](../publishing/workspace.md)。
- 想在示例工作区中体验题目数据构建与严格验收：阅读 [题目构建与导出](../problem-build/index.md)。
- 想定制 PDF 版式或切换 `cnoi-classic` 模板：阅读 [模板与主题定制](../publishing/templates.md)。
- 想将 `xcpc` 接入 Claude Desktop、Cursor 或 Codex：阅读 [MCP 服务接入](../cli-mcp/mcp.md)。
