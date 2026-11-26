# AGENTS.md

这份文件写给在本仓库中工作的 AI Agent（Claude Code、Codex、Cursor 等）。

## 项目

**XCPC Workstation** 是本地优先的**个人** XCPC 工作站：以题库为底座，提供活引用式组卷、与参考模板版式一致的 PDF 出版、本地 Polygon 出题验题、混合沙箱插件与训练看板，并原生支持 MCP。Phase 0–4（出版、本地 Polygon、插件体系、训练看板与纯 Web `<WebView>` 插件生态）均已落地，核心 crate 包括 `xcpc-domain`、`xcpc-storage`、`xcpc-convert`、`xcpc-engine`、`xcpc-judge`、`xcpc-packages`、`xcpc-plugin`、`xcpc-sync`、`xcpc-mcp`，桌面端位于 `apps/desktop`，插件 SDK 位于 `packages/plugin-sdk`。

完整规格在 `docs/` 下，按需阅读：

- `docs/product.md`：定位、明确不做的事、题目/比赛/训练的概念、`problem.toml` 格式、题面结构
- `docs/architecture.md`：技术栈、crate 划分、领域模型、存储布局、转换管线、MCP 与安全边界
- `docs/statement-style.md`：`olymp-classic` 的基准、版式数值、字体、题解册
- `docs/design-system.md`：界面信息架构、Token、文案与交互规范
- `docs/roadmap.md`：Phase 0–3 的范围和待定事项
- `docs/reference/`：v1 遗留物，**只供查阅，不要移植或迁移**

这些规格已经由用户确认。实现时与规格冲突的地方先问用户；设计决定发生变化时，同步更新对应的 `docs/` 文件。

## 开发环境与命令

- 用 `nix develop` 进入开发环境（direnv 会通过 `.envrc` 自动加载）。环境提供 fenix stable Rust（含 `wasm32-unknown-unknown` 与 `x86_64-pc-windows-msvc` target std）、clang/cmake/pkg-config、**NSH 0.6.0**、`cargo-xwin`/`lld`/`llvm`（Linux→Windows 交叉编译）、`upx`、`tectonic`，以及 Linux 上的 GTK4/WebKitGTK/GStreamer。
- **题面字体**：环境变量 `XCPC_FONT_DIRS` 指向 Latin Modern（正文、数学、样例与行内等宽）、JetBrains Mono（代码块）和方正（`foundertype-ctex-fonts`）的字体目录，引擎应优先从这里加载字体；release 构建同时内嵌 Latin Modern 与 JetBrains Mono。未设置 `XCPC_FONT_DIRS` 时（独立分发 / Windows），引擎自动扫描可执行文件旁的 `fonts/` 目录并回退到系统字体（含中文字体回退链）。`XCPC_FONTCONFIG_FILE` 是一份只包含这些字体的 fontconfig 配置，**只在**用 tectonic 生成基准 PDF 时作为 `FONTCONFIG_FILE` 使用，不要对整个 shell 设置（会影响 GUI）。
- NSH CLI：`nsh init`（本项目用 `--template mixed`）、`nsh dev`、`nsh run --target …`、`nsh check`（`nsh.json` 声明了 `"rust"` 时会先跑 `cargo check`）、`nsh test`（先跑 Rust 的 `cargo test`，再无头运行 `*.test.tsx`）、`nsh build`、`nsh package`。细节用 `nsh <cmd> --help` 查看。查 NSH 的 API 时读 `.nsh/types/nsh/{index,base}.d.ts`；那里只有 JS 侧的接口，Rust 侧还有更多（文件对话框、在文件管理器中显示等），查 `~/.cargo/registry/src/*/nsh-core-0.6.0/src`。
- 依赖 `XCPC_FONT_DIRS` 的命令（`xcpc`、`nsh test`、`nsh dev`、`scripts/package.sh`）要在开发环境里运行；不在 direnv 下时用 `nix develop -c <cmd>`。
- `.cargo/config.toml` 集中配置了 `[profile.dev.package."*"]`（`opt-level = 2`）、`[profile.release]`（`opt-level = "z"`、`lto = "fat"`、`codegen-units = 1`、`strip = true`、`panic = "abort"`）以及 `[target.x86_64-pc-windows-msvc]`（静态 CRT `+crt-static`）：`nsh` 为桌面宿主生成的 `apps/desktop/.nsh/rust` 是另一个 workspace，读不到根 `Cargo.toml` 的 profile。
- `scripts/package.sh`：统一打包桌面端（`nsh package`）与 `xcpc` CLI 的 release 产物，支持 `--target <linux|windows|all>`（默认 `linux`）、`--out <DIR>`（默认 `target/dist`）和 `--no-upx`，默认用 UPX 压缩后在 `target/dist/` 产出各自独立的归档。
- `xcpc`：`xcpc -w <工作区> build|check|render <problem|contest> <id> [--editorial] [--json]`、`xcpc list`、`xcpc mcp [--read-only]`；`build --emit-typst <文件>` 可以看生成的 Typst。基准用例的工作区在 `tests/fixtures/olymp-ref/<用例>/workspace/`，由 `scripts/fixture-to-workspace.py` 从参考模板的 Markdown 生成。桌面端用 `XCPC_WORKSPACE=<工作区> nsh dev`，或启动后用「打开工作区」选。NSH 的文件对话框等系统服务只开放给 Rust（`nsh_core::file_dialog`），JS 侧没有，需要时在 `apps/desktop/src/lib.rs` 里包一层。
- Rust：`cargo build`、`cargo test -p <crate> <test_name>`（运行单个测试）、`cargo clippy`、`cargo fmt`。Nix 文件用 `nix fmt` 格式化。
- 版本管理用 **jj**（与 Git colocated），不要用会改动仓库状态的 git 命令。
- 只在 Linux 上开发和验收（可通过 `scripts/package.sh --target windows` 在 Linux 上交叉打包 Windows 产物供手动验收）。

## 架构要点

- **单一 Rust Core**：NSH 桌面 GUI、`xcpc mcp`（stdio）和 `xcpc <subcommand> --json` 共用同一套 Core。业务逻辑只能写在 `crates/` 里，`apps/desktop` 和 `xcpc-mcp` 只是外壳。
- **数据流**：开放目录（单一事实源）→ domain → convert（Markdown + Math，按二级标题识别章节，样例取自 `samples/`）→ typst（`olymp-classic`）→ `typst-render`（预览用像素缓冲，或 PNG）或 `typst-pdf`。
- **题目**：目录名 slug 就是 ID，创建后不再改名；外部身份写在 `[[source]]` 中。**训练状态不写进任何内容文件**，在运行时由 `training/logs.jsonl` 和用户全局 `<dataDir>/oj_sync.db` 合并派生。
- **比赛**：活引用；覆写只允许改元数据（标题、限制、`show-metadata`）。冻结时把源文件和 PDF 复制进 `snapshot/`。
- UI 组件以 `nsh/base` 为基础：优先用现成组件，需要时可以基于 NSH 自己写组件（比如预览用的 `PageImage`），不引入第三方 UI 组件库。

## 不可违反的约束

- **题面版式以参考模板为唯一基准**：`lihaoze123/polygon-chinese-template@10f9959`，基准 PDF 由 tectonic 编译生成，详见 `docs/statement-style.md` §1。不要凭印象改版式，比如把题名改成居中、把页脚改成「第 X 页」都是错的。
- **看起来一样，不用魔数**：题面版式的验收标准是和基准并排看起来一样，不要求逐点重合，尊重 TeX 和 Typst 之间的差异，但固定元素（页眉、页脚、题头、表格）不能差太远。模板里只能写有出处的设计参数（字号、行距、间距、线宽等），不得为了对齐而反推偏移量，也不要模拟 TeX 的行距细节。改动模板后运行 `scripts/olymp-regress.sh`，逐页看并排图。详见 `docs/statement-style.md` §1.1.1。
- **只用 Typst 生成 PDF**：不导出 LaTeX，不写 v1 迁移代码，不做题解幻灯片，不做气球颜色。
- **不手写 Markdown/LaTeX 解析器**：用 `pulldown-cmark`（GFM + `ENABLE_MATH`）、`mitex`、`tylax`、`typlite`，自己只写薄胶水层。
- **安全边界在 Core 层强制执行**：写入一律原子化（临时文件 + rename）；冻结的比赛只读，写入返回明确错误码；**解冻只开放给 GUI/CLI（需二次确认），不开放给 MCP**；支持 `--read-only`；文件访问不得越出工作区目录。
- 不要超出当前阶段的范围（见 `docs/roadmap.md`）。`problem.toml` 读写要保留未知字段。
- **做减法（less is more）**：提方案时默认给最小集合，意义不明确的功能、控件、选项直接不做（不写成「推迟」，也不在界面上留占位）。项目尚未正式发布，不考虑迁移、向后兼容或用户习惯（比如快捷键编号可以随时重排）。
- **当且仅当本地 lint、clippy 与测试全部通过时才允许提交或推送**：每次 `jj commit` / 推送 / 打 tag 前，必须先在本地跑通与 `.github/workflows/lint.yml` 和 `.github/workflows/ci.yml` 一致的全部检查——包括 `cargo fmt --all -- --check`、`cargo clippy --workspace --exclude xcpc-desktop --all-targets -- -D warnings`、`cargo test --workspace --exclude xcpc-desktop`、桌面端 `cd apps/desktop && nsh check && nsh test`（改过脚本或插件文档时补跑 `bash -n scripts/*.sh packaging/linux/*.sh`、`shellcheck -e SC2086 scripts/*.sh packaging/linux/*.sh`、`python3 -m py_compile scripts/*.py` 与 `python3 scripts/check-plugin-docs.py`）；**严禁只跑部分 crate 的 clippy/test 就推送或发布**，未全部通过禁止提交和推送。
- **插件文档与代码同步维护**：修改 `crates/xcpc-plugin`、`packages/plugin-sdk` 或 `xcpc plugin` CLI（`crates/xcpc-mcp/src/main.rs`）时，必须在同一变更中同步更新 `docs/plugin-development.md` 与 `packages/plugin-sdk/README.md`（并通过 `python3 scripts/check-plugin-docs.py`）；发布新版本时同步更新 `xcpc-plugins/README.md` 与 `xcpc-plugin-sdk.tgz`。
- **UI 文案克制与流畅感硬约束**：主工作区不放冗余标题，不暴露技术细节（`SQLite FTS5`、`12ms`、`theme.toml`、`MCP Ready` 等）；后台自动行为成功静默，用户主动触发的动作结束时给明确反馈（短动作原位瞬态确认、长耗时任务单槽右下角 Toast，见 `docs/design-system.md` §6.5）；耗时不到 150ms 的操作不显示 loading；嵌套圆角同心；数字用 `tabular-nums`；不能有 1px 布局抖动。**交互流畅感是一等公民**：采用温润物理弹簧手感（`ζ = 0.86–0.90`，过冲 `≤ 1%`，随时可打断；按压 `scale(0.97)` 仅限纯色主按钮）；连续手势（缩放、平移、拖拽）全程只动 GPU 合成器 `transform`、结束时单次原子提交，禁止每帧重排或与 `scrollTo` 竞态；切换与刷新零闪白，高频输入 1 帧内响应（≤16ms）。Token 与工程铁律以 `docs/design-system.md`（尤其是 §8）为准。
