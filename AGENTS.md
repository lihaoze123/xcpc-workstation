# AGENTS.md

这份文件写给在本仓库中工作的 AI Agent（Claude Code、Codex、Cursor 等）。

## 项目

**XCPC Workstation** 是本地优先、以桌面端为主的个人/小队 XCPC 全能工作站：以题库为底座，提供活引用式比赛组卷、与 `GassaFM/olymp.sty` 1:1 一致的 PDF 出版、训练 Dashboard，并原生支持 MCP。目前处于 **Phase 0（技术验证）之前**，仓库里只有 Nix 开发环境和 `docs/`，还没有业务代码。

完整规格在 `docs/` 下，按需阅读：

- `docs/product.md`：产品定位、题目 / 比赛 / 训练的概念与功能范围、OJ 生态
- `docs/architecture.md`：技术栈、crate 划分、领域模型、存储布局、转换管线、MCP 与安全边界
- `docs/statement-style.md`：`olymp-classic` 复刻清单、主题定制、lint、TRD、双语
- `docs/design-system.md`：界面信息架构、Token、文案与交互规范
- `docs/roadmap.md`：Phase 0–5
- `docs/reference/`：v1 遗留物，**只供查阅，不要移植或迁移**

这些规格已经由用户确认。实现时与规格冲突的地方先问用户；设计决定发生变化时，同步更新对应的 `docs/` 文件。

## 开发环境与命令

- 用 `nix develop` 进入开发环境（`.envrc` 已配置 `use flake`，direnv 会自动加载）。环境提供 fenix stable Rust（含 `wasm32-unknown-unknown` 目标）、clang/cmake/pkg-config、**NSH 0.6.0** CLI，以及 Linux 上的 GTK4/WebKitGTK/GStreamer。
- NSH CLI：`nsh init`（可选 TSX / Rust / TSX + Rust crate 模板）、`nsh dev`（原生宿主 + 热重载）、`nsh run --target …`（GTK/浏览器/TUI 等宿主）、`nsh check`（TS 类型检查；`nsh.json` 声明了 `"rust"` 时会先跑 `cargo check`）、`nsh test`（先跑 Rust crate 的 `cargo test`，再无头运行 `*.test.tsx`）、`nsh build`、`nsh package`。细节用 `nsh <cmd> --help` 查看。
- Rust：`cargo build`、`cargo test -p <crate> <test_name>`（运行单个测试）、`cargo clippy`、`cargo fmt`。Nix 文件用 `nix fmt` 格式化。
- 版本管理用 **jj**（与 Git colocated），不要用会改动仓库状态的 git 命令。

## 架构要点

- **单一 Rust Core**：NSH 桌面 GUI、`xcpc mcp`（stdio）和 `xcpc <subcommand> --json` 共用同一套 Core。业务逻辑只能写在 `crates/` 里，`apps/desktop` 和 `xcpc-mcp` 只是外壳。
- **规划的 crate**：`xcpc-domain`、`xcpc-storage`、`xcpc-convert`、`xcpc-engine`、`xcpc-lint`、`xcpc-packages`、`xcpc-sync`、`xcpc-mcp`，以及 `apps/desktop`。
- **数据流**：开放目录（单一事实源）→ domain → convert（md / LaTeX / Typst 直通）→ olymp 胶水层 → typst 增量编译 → `typst-render`（Canvas / PNG）或 `typst-pdf`。
- **存储**：`problems/`、`contests/` 等目录是事实源，`.xcpc/index.db`（FTS5）是可以重建的派生索引，外部 OJ 同步的海量数据只放在 `.xcpc/oj_sync.db`。

## 不可违反的约束

- **默认题面严格 1:1 复刻 `GassaFM/olymp.sty`**，清单见 `docs/statement-style.md` §1。
- **只用 Typst 生成 PDF**：不做 LaTeX `.tex` 导出器，不写任何 v1 迁移代码。
- **不手写 Markdown/LaTeX 解析器**：用 `pulldown-cmark`（GFM + `ENABLE_MATH`）、`mitex`、`tylax`、`typlite`，自己只写薄胶水层。
- **安全边界在 Core 层强制执行**：写入一律原子化（临时文件 + rename）；`is_frozen = true` 的比赛只读，写入要返回明确错误码；支持 `--read-only`；文件访问不得越出工作区目录。
- `problem.toml` 要预留 Phase 5 的字段（`subtasks` / `generators` / `validators` / `checkers` / `solutions`），但现在不实现。
- **UI 文案克制**：主工作区不放冗余标题，不暴露技术细节（`SQLite FTS5`、`12ms`、`theme.toml`、`MCP Ready` 等）；成功状态不提示；耗时不到 150ms 的操作不显示 loading；嵌套圆角同心；数字用 `tabular-nums`；不能有 1px 布局抖动。颜色等 Token 以 `docs/design-system.md` 为准。
