# XCPC Workstation

本地优先的**个人 XCPC 工作站**（以 Linux 桌面端为主）：以开放目录题库为单一事实源，提供活引用组卷、与 [`lihaoze123/polygon-chinese-template`](https://github.com/lihaoze123/polygon-chinese-template)（`olymp.sty` 中文版 @ `10f9959`）版式一致的 PDF 出版，并以 **CLI + stdio MCP** 作为一等接口；桌面 GUI、CLI 与 MCP 共用同一套 Rust Core。

## 当前里程碑：`v0.1.0`（Phase 0–1 出版链路）

- **开放目录工作区与领域模型**（`xcpc-domain` / `xcpc-storage`）：支持 `workspace.toml`、`problems/<id>/`（`problem.toml`、`statement.md`、`samples/`、`editorial.md`、`assets/`）与 `contests/<id>/contest.toml`（活引用与元数据覆写），Core 层强制执行原子写入、路径沙箱（含符号链接防御）与冻结只读守卫。
- **出版管线与原生 Typst 引擎**（`xcpc-convert` / `xcpc-engine`）：基于 `pulldown-cmark` + 内嵌 `mitex` 将 Markdown 与数学公式转换为 Typst（不手写解析器），识别标准章节与 `samples/` 样例表，将源码/版心溢出诊断映射回 Markdown 行列；内嵌 `olymp-classic` 模板，支持单题、带封面与试题列表的整本试题册、可选奇偶留白页、跨页样例表，以及 A4 题解册（扉页、目录链接与书签、单色高亮代码块）。
- **确定性字体**：开发环境通过 `XCPC_FONT_DIRS` 锁定 Latin Modern、JetBrains Mono 与方正（`foundertype-ctex-fonts`）；release 构建直接内嵌 Latin Modern 与 JetBrains Mono（3.09 MiB，24 个 OTF 文件）作为兜底。未设置 `XCPC_FONT_DIRS`（独立分发或 Windows）时，引擎自动扫描可执行文件同级的 `fonts/` 目录并回退到系统字体（含中文字体回退链）。
- **CLI 与 MCP 服务**（`xcpc`）：提供 `build` / `check` / `render` / `list` / `fonts` 子命令（支持 `--json`、确定性退出码与 `--read-only`）及 `xcpc mcp`（`compile_and_diagnose`、`render_page_png`、`export_pdf`）。
- **Linux 桌面工作室**（`apps/desktop`，NSH 0.6）：打开工作区（并记忆上次工作区）、切换题目/比赛与题面/题解、按题号跳页、500ms 原子保存、编辑器诊断标记、导出 PDF 到 `build/`，以及基于像素缓冲与两阶段合成器缩放的实时预览。

## 快速开始

仓库使用 Nix Flake 管理工具链与锁定字体（`.envrc` 支持 `direnv` 自动加载；也可直接用 `nix develop -c <cmd>`）：

```bash
# 进入开发环境
nix develop

# 用内置示例工作区构建整本试题册与题解册 PDF（默认输出到 examples/workspace/build/）
cargo run -p xcpc-mcp -- -w examples/workspace build contest spring-training
cargo run -p xcpc-mcp -- -w examples/workspace build contest spring-training --editorial

# 启动桌面端预览与编辑示例工作区
cd apps/desktop && XCPC_WORKSPACE=../../examples/workspace nsh dev

# 启动 stdio MCP 服务（只读模式加 --read-only）
cargo run -p xcpc-mcp -- -w examples/workspace mcp

# 打包发布产物（桌面端 + xcpc CLI，经 UPX 压缩后输出到 target/dist/）
scripts/package.sh --target linux   # 或 windows / all
```

## 项目结构

```text
crates/
  xcpc-domain       # Problem / Contest / TrainingLog 领域模型与规则
  xcpc-storage      # 工作区目录发现、原子写入、路径沙箱、冻结只读守卫
  xcpc-convert      # Markdown + Math → Typst、publish 管线与源码诊断映射
  xcpc-engine       # Typst World、olymp-classic 模板、MiTeX 作用域、release 字体内嵌
  xcpc-mcp          # `xcpc` CLI 与 stdio MCP Server
apps/
  desktop           # NSH 0.6 桌面端（Rust native module + TSX UI）
examples/workspace  # 示例工作区
tests/fixtures/
  olymp-ref         # `polygon-chinese-template` 基准 PDF 与工作区回归用例
scripts/            # 基准 PDF 生成与视觉回归脚本
docs/               # 产品、架构、题面版式、设计系统与路线图规格
```

## 规格文档

详细设计与后续规划见 [`docs/`](./docs/README.md)：

- [产品规格](./docs/product.md)：定位、核心概念、`problem.toml` 与题面结构
- [系统架构](./docs/architecture.md)：技术栈、crate 划分、领域模型、双层存储与安全边界
- [题面样式与出版](./docs/statement-style.md)：`olymp-classic` 基准参数、字体、题解册与后续规划
- [界面设计系统](./docs/design-system.md)：信息架构、Token、文案克制与流畅感工程铁律
- [路线图](./docs/roadmap.md)：Phase 0–3 范围与技术验证记录

## 测试与回归验证

```bash
nix develop -c cargo fmt --all -- --check
nix develop -c cargo clippy --workspace --exclude xcpc-desktop --all-targets -- -D warnings
nix develop -c cargo test --workspace
nix develop -c cargo test --release -p xcpc-engine
nix develop -c scripts/olymp-regress.sh
cd apps/desktop && nix develop -c nsh test
```

## 许可证

本项目采用 [Apache License 2.0](./LICENSE) 许可。
