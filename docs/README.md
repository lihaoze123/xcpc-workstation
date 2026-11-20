# XCPC Workstation 文档

XCPC Workstation 是本地优先的 **个人 XCPC 工作站**（以 Linux 桌面端为主）。题库是它的底座，在此之上提供比赛组卷、与 [`polygon-chinese-template`](https://github.com/lihaoze123/polygon-chinese-template)（`olymp.sty` 中文版）版式一致的题面出版、训练看板，并把 Agent/MCP 作为一等用户来支持。当前优先级是**出版**。

| 文档 | 内容 |
| :--- | :--- |
| [product.md](./product.md) | 产品定位、核心概念（题目 / 比赛 / 训练）、功能范围、OJ 生态互通 |
| [architecture.md](./architecture.md) | 技术栈、crate 划分、领域模型、双层存储、格式转换管线、MCP/CLI 与安全边界 |
| [statement-style.md](./statement-style.md) | 默认题面 `olymp-classic` 的基准与版式细则、字体、题解册、`xcpc-lint`、TRD、双语 |
| [design-system.md](./design-system.md) | 应用界面的信息架构、色彩/圆角/字体/动效 Token、文案克制规范与交互工艺细则 |
| [roadmap.md](./roadmap.md) | Phase 0–3 的范围，以及待定事项 |
| [reference/](./reference/) | v1 遗留参考材料（只读，不迁移，详见下文） |

## 参考材料（`reference/`）

本项目是全新产品，**不迁移 v1 的数据，也不保留 v1 的代码**。下面这些文件只用于查阅：

- `v1_lib.typ`：v1 的 Typst 题面模板，字体回退链和 i18n 词条可以参考。版式**不能**参考它，版式以 [statement-style.md](./statement-style.md#11-基准) 中的参考模板为准。
- `v1_polygon_converter.ts`：v1 的 Polygon ZIP 解析器，可以参考 Polygon XML 和 statement 的各种边界情况。
- `color_system_preview.html`：色彩 Token 与界面骨架的交互式预览，在浏览器中打开即可查看精确色板和布局克制程度。
