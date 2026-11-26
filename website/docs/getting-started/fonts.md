# 字体配置

XCPC Workstation 内置了西文与等宽字体，安装后即可在无 TeX 环境下直接生成完整 PDF。本页说明内置字体清单、中文字体回退机制、如何配置方正字体以达到与 `olymp.sty` 基准一致的版式，以及如何用 `xcpc fonts` 诊断字体加载结果。

## 内置字体

发行版二进制文件已直接内嵌以下字体，无需在操作系统中额外安装：

| 角色 | 内置字体 | 用途 |
| :--- | :--- | :--- |
| 西文正文 / 标题 | **Latin Modern Roman / Sans**（10pt / 12pt / 17pt） | 题面西文正文、章节标题、页眉页脚（与 XeLaTeX 默认一致） |
| 数学公式 | **Latin Modern Math** | 行内与行间数学公式 |
| 行内等宽与样例 | **Latin Modern Mono**（10pt / 12pt） | 样例输入输出、文件名、行内代码 |
| 代码块 | **JetBrains Mono**（Regular / Italic / Bold / BoldItalic） | 题解与文档中的多行代码块、团队参考手册（TRD）代码区 |

## 中文字体角色与回退链

中文排版按四个传统字族角色匹配字体。引擎在加载字体目录后，按以下优先级依次查找各角色对应的可用字体：

| 角色（Typst 符号） | 首选字体（基准） | 自动回退顺序 |
| :--- | :--- | :--- |
| **宋体正文**（`cjk-song`） | `FZShuSong-Z01`（方正书宋） | `Source Han Serif SC` → `Noto Serif CJK SC` → `SimSun` → `NSimSun` → `Songti SC` |
| **黑体标题/加粗**（`cjk-hei`） | `FZHei-B01`（方正黑体） | `Source Han Sans SC` → `Noto Sans CJK SC` → `SimHei` → `Microsoft YaHei` → `PingFang SC` |
| **楷体作者/斜体**（`cjk-kai`） | `FZKai-Z03`（方正楷体） | `KaiTi` → `STKaiti` → `Kaiti SC` → `Source Han Serif SC` → `Noto Serif CJK SC` → `SimSun` |
| **仿宋**（`cjk-fangsong`） | `FZFangSong-Z02`（方正仿宋） | `FangSong` → `STFangsong` → `Source Han Serif SC` → `Noto Serif CJK SC` → `SimSun` |

当首选方正字体不可用而触发回退时，排版仍可正常完成，引擎会在编译诊断中输出一条非致命警告以提示发生了字体替换。

## 字体加载规则与优先级

引擎按以下两套模式决定字体搜索范围：

1. **未设置 `XCPC_FONT_DIRS`（独立安装包默认行为）**：
   - 首先扫描**可执行文件同级的 `fonts/` 目录**（如果存在）。
   - 然后扫描操作系统已安装的系统字体，并叠加二进制内置的 Latin Modern 与 JetBrains Mono。
2. **设置了 `XCPC_FONT_DIRS`（确定性排版模式）**：
   - 只扫描 `XCPC_FONT_DIRS` 指定的目录列表（Linux 用 `:` 分隔，Windows 用 `;` 分隔）以及二进制内置字体，**默认不扫描系统字体**，从而保证不同机器或 CI 环境下产出完全一致的 PDF。
   - 如需在指定 `XCPC_FONT_DIRS` 的同时仍然允许读取系统字体，可在 CLI 附加全局参数 `--system-fonts`。

## 配置方正字体以对齐 `olymp.sty` 基准

`olymp-classic` 模板的视觉基准采用 `ctexart` 的 `fontset=founder` 配置（方正书宋、方正黑体、方正楷体、方正仿宋）。由于字体版权原因，发行版不随包附带方正字体文件。如果你需要导出与 `olymp.sty` 中文参考模板完全一致的印刷级 PDF，请自行合法获取这 4 套方正字体（`.ttf` / `.otf`）：

- `FZShuSong-Z01`（方正书宋）
- `FZHei-B01`（方正黑体）
- `FZKai-Z03`（方正楷体）
- `FZFangSong-Z02`（方正仿宋）

随后任选以下两种方式之一配置：

=== "方式一：放入可执行文件旁的 fonts/ 目录（推荐桌面用户）"

    将字体文件复制到可执行文件同级目录下的 `fonts/` 文件夹中，重启应用即可自动识别：

    - **Windows 安装版 / 便携版**：`<安装目录>\fonts\`（与 `XCPC Workstation.exe` / `xcpc.exe` 同级）
    - **Linux `.deb` / `.pkg.tar.zst`**：`/opt/xcpc-workstation/fonts/`
    - **Linux `.tar.gz` 解压版**：`<解压目录>/fonts/`

    也可以直接将方正字体安装到操作系统全局字体目录（Windows `C:\Windows\Fonts`，Linux `~/.local/share/fonts/` 并执行 `fc-cache -fv`），未设置 `XCPC_FONT_DIRS` 时同样会被自动发现。

=== "方式二：通过环境变量 XCPC_FONT_DIRS 指定（推荐 CI / 多环境隔离）"

    将字体集中存放到某个目录（例如 `~/fonts/founder`），在启动桌面端或 CLI 前设置环境变量：

    ```bash
    # Linux / macOS
    export XCPC_FONT_DIRS="$HOME/fonts/founder"
    xcpc -w ./my-xcpc build contest welcome-round
    ```

    ```powershell
    # Windows PowerShell
    $env:XCPC_FONT_DIRS = "D:\fonts\founder"
    xcpc -w .\my-xcpc build contest welcome-round
    ```

    多个目录在 Linux 下用 `:` 拼接，在 Windows 下用 `;` 拼接。

## 使用 `xcpc fonts` 诊断字体

运行 `xcpc fonts` 可列出排版引擎当前实际看到的全部字体家族名称：

```bash
# 列出当前环境下可见的字体家族
xcpc fonts

# 以 JSON 格式输出，便于脚本检查
xcpc fonts --json

# 若设置了 XCPC_FONT_DIRS，同时检查叠加系统字体后的结果
xcpc --system-fonts fonts
```

检查输出中是否包含 `FZShuSong-Z01`、`FZHei-B01`、`FZKai-Z03`、`FZFangSong-Z02`（或你期望使用的思源 / Noto CJK 字体）。如果某款字体未出现在列表中，请确认：

1. 文件扩展名为 `.ttf`、`.otf`、`.ttc` 或 `.otc` 且文件可读。
2. 环境变量 `XCPC_FONT_DIRS` 是否被误设为空目录或覆盖了系统字体扫描。
3. 在模板参数（如 `olymp-classic` 的 `cjk-body-font`、`cjk-heading-font`）中填写的字体名称，与 `xcpc fonts` 输出的家族名称完全一致。
