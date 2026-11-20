# 交接（2026-11-20）

## 状态

Phase 1（最短出版链路）已完成，历史中按层提交（`jj log` 可见）：

1. `domain, storage`：模型、工作区读写、路径沙箱、原子写入、冻结只读守卫。
2. `tests`：基准用例改为工作区格式（`tests/fixtures/olymp-ref/*/workspace`）。
3. `convert, engine`：Markdown → Typst、出版管线、内嵌 MiTeX、溢出检查。
4. `xcpc`：`build|check|render|list|fonts|mcp`，`--json`。
5. `desktop, docs`：编辑 + 预览、示例工作区 `examples/workspace`。
6. `engine`：高于一整页的样例表跨页并重复表头。
7. `desktop`：记住上次打开的工作区（`new Preview(process.dataDir)`，存于应用数据目录 `last-workspace`）。
8. `engine, convert`：题解册版式（扉页 + 目录、每题一页、单色代码高亮），中文粗体/斜体按 ctex founder 映射到方正黑体/楷体。

验证：`cargo test --workspace`、`nsh test`（8 个）、`cargo clippy` 均通过；`scripts/olymp-regress.sh` 三套用例页数一致，与改动前逐像素相同。

## 已确认的设计决定（用户）

- 题解册：与题面册同一套书（版心、字体、页眉页脚横线、`Page X of Y` 都相同）；规格写在 `docs/statement-style.md` §3。
- **不加装饰**：正文和界面里不用 `·`、`|` 分隔符、分隔线、底色框、引导点，层级靠间距、字号、字重和对齐（`docs/design-system.md` §1.4）。题面册继承基准的页眉页脚横线保留。
- 只记上一个工作区；「最近工作区」菜单留到完善 UI/UX 时做（已写入路线图）。
- 长样例：放得下保持基准行为，放不下才跨页，不加「（续）」。

## 未验证 / 待办

- **桌面端未在真实窗口里复验最后两项**：「记住工作区」修正后（构造函数改为接收 `process.dataDir`）只跑了 `nsh test`，没在 `nsh dev` 里实测重启后自动打开；题解册在桌面预览里也未看过（CLI 导出的 PDF 已看过）。先跑：
  `cd apps/desktop && XCPC_WORKSPACE=../../examples/workspace nsh dev`，退出后不带环境变量再启动，应直接打开示例工作区。
- 文件夹选择器在真实桌面上弹出已验证，但「选中文件夹 → 打开」一步未通过自动化走通，需手动试一次。
- Phase 2（notify 监听、冻结与人工解冻等）按用户要求尚未讨论，开始前先与用户确认设计（例如外部修改与编辑器未保存内容冲突时怎么办）。

## 注意事项

- NSH 的系统服务（文件对话框等）只开放给 Rust：`nsh_core::file_dialog`。但 `nsh_core::element::data_dir()` 等依赖响应式运行时，**不能在 class 构造函数里调用**（会 panic，`cargo test` 测不出，必须跑 `nsh test`）。
- MiTeX 0.2.4 输出的部分符号名在 Typst 0.15 已改名，`xcpc-convert` 的 `markdown::RENAMED` 负责替换；测试 `every_mitex_symbol_evaluates` 会在升级 mitex/Typst 后发现新的失效符号。
- 用 `jj`，不要用改动仓库状态的 git 命令。
