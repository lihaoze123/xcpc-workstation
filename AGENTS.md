# AGENTS.md

这份文件写给在本仓库中工作的 AI Agent（Claude Code、Codex、Cursor 等）。

## 项目

**XCPC Workstation** 是本地优先的**个人** XCPC 工作站：以题库为底座，提供活引用式组卷、与参考模板版式一致的 PDF 出版、训练看板，并原生支持 MCP。**当前优先级是出版**。目前处于 Phase 0（技术验证）之前，仓库里只有 Nix 开发环境和 `docs/`。

完整规格在 `docs/` 下，按需阅读：

- `docs/product.md`：定位、明确不做的事、题目/比赛/训练的概念、`problem.toml` 格式、题面结构
- `docs/architecture.md`：技术栈、crate 划分、领域模型、存储布局、转换管线、MCP 与安全边界
- `docs/statement-style.md`：`olymp-classic` 的基准、版式数值、字体、题解册
- `docs/design-system.md`：界面信息架构、Token、文案与交互规范
- `docs/roadmap.md`：Phase 0–3 的范围和待定事项
- `docs/reference/`：v1 遗留物，**只供查阅，不要移植或迁移**

这些规格已经由用户确认。实现时与规格冲突的地方先问用户；设计决定发生变化时，同步更新对应的 `docs/` 文件。

## 开发环境与命令

- 用 `nix develop` 进入开发环境（direnv 会通过 `.envrc` 自动加载）。环境提供 fenix stable Rust、clang/cmake/pkg-config、**NSH 0.6.0**、`tectonic`，以及 Linux 上的 GTK4/WebKitGTK/GStreamer。
- **题面字体**：环境变量 `XCPC_FONT_DIRS` 指向 Latin Modern 和方正（`foundertype-ctex-fonts`）的字体目录，引擎应优先从这里加载字体。`XCPC_FONTCONFIG_FILE` 是一份只包含这些字体的 fontconfig 配置，**只在**用 tectonic 生成基准 PDF 时作为 `FONTCONFIG_FILE` 使用，不要对整个 shell 设置（会影响 GUI）。
- NSH CLI：`nsh init`（本项目用 `--template mixed`）、`nsh dev`、`nsh run --target …`、`nsh check`（`nsh.json` 声明了 `"rust"` 时会先跑 `cargo check`）、`nsh test`（先跑 Rust 的 `cargo test`，再无头运行 `*.test.tsx`）、`nsh build`、`nsh package`。细节用 `nsh <cmd> --help` 查看。查 NSH 的 API 时读 `.nsh/types/nsh/{index,base}.d.ts`。
- Rust：`cargo build`、`cargo test -p <crate> <test_name>`（运行单个测试）、`cargo clippy`、`cargo fmt`。Nix 文件用 `nix fmt` 格式化。
- 版本管理用 **jj**（与 Git colocated），不要用会改动仓库状态的 git 命令。
- 只在 Linux 上开发和验收。

## 架构要点

- **单一 Rust Core**：NSH 桌面 GUI、`xcpc mcp`（stdio）和 `xcpc <subcommand> --json` 共用同一套 Core。业务逻辑只能写在 `crates/` 里，`apps/desktop` 和 `xcpc-mcp` 只是外壳。
- **数据流**：开放目录（单一事实源）→ domain → convert（Markdown + Math，按二级标题识别章节，样例取自 `samples/`）→ typst（`olymp-classic`）→ `typst-render`（Canvas/PNG）或 `typst-pdf`。
- **题目**：目录名 slug 就是 ID，创建后不再改名；外部身份写在 `[[source]]` 中。**训练状态不写进任何内容文件**，在运行时由 `training/logs.jsonl` 和 `.xcpc/oj_sync.db` 合并派生。
- **比赛**：活引用；覆写只允许改元数据（标题、限制、`show-metadata`）。冻结时把源文件和 PDF 复制进 `snapshot/`。
- UI 组件只用 `nsh/base`。

## 不可违反的约束

- **题面版式以参考模板为唯一基准**：`lihaoze123/polygon-chinese-template@10f9959`，基准 PDF 由 tectonic 编译生成，详见 `docs/statement-style.md` §1。不要凭印象改版式，比如把题名改成居中、把页脚改成「第 X 页」都是错的。
- **看起来一样，不用魔数**：题面版式的验收标准是和基准并排看起来一样，不要求逐点重合，尊重 TeX 和 Typst 之间的差异，但固定元素（页眉、页脚、题头、表格）不能差太远。模板里只能写有出处的设计参数（字号、行距、间距、线宽等），不得为了对齐而反推偏移量，也不要模拟 TeX 的行距细节。改动模板后运行 `scripts/olymp-regress.sh`，逐页看并排图。详见 `docs/statement-style.md` §1.1.1。
- **只用 Typst 生成 PDF**：不导出 LaTeX，不写 v1 迁移代码，不做题解幻灯片，不做气球颜色。
- **不手写 Markdown/LaTeX 解析器**：用 `pulldown-cmark`（GFM + `ENABLE_MATH`）、`mitex`、`tylax`、`typlite`，自己只写薄胶水层。
- **安全边界在 Core 层强制执行**：写入一律原子化（临时文件 + rename）；冻结的比赛只读，写入返回明确错误码；**解冻只开放给 GUI/CLI（需二次确认），不开放给 MCP**；支持 `--read-only`；文件访问不得越出工作区目录。
- 不要超出当前阶段的范围（见 `docs/roadmap.md`）。`problem.toml` 要预留 Phase 5 的字段，但现在不实现。
- **UI 文案克制**：主工作区不放冗余标题，不暴露技术细节（`SQLite FTS5`、`12ms`、`theme.toml`、`MCP Ready` 等）；成功状态不提示；耗时不到 150ms 的操作不显示 loading；嵌套圆角同心；数字用 `tabular-nums`；不能有 1px 布局抖动。Token 以 `docs/design-system.md` 为准。
