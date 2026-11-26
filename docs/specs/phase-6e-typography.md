# Phase 6E 规格：界面排版

2026-11-26 grill-me 对齐确认，在 Phase 6D（Token 收敛）之上进行。

现状：界面从未设置 `fontFamily`，Linux 上 `sans-serif` 解析为 `Noto Sans CJK SC`，西文与数字使用其自带的宽松字形；Windows 则落到 Segoe UI / 微软雅黑，两平台观感不一致。等宽字体被用于 slug、OJ 题号等标识符（约 100 处），界面「程序员味」偏重。中文字形决定整体气质，西文与数字决定细节精致度，二者分开处理。

本规格落地后同步改写 [design-system.md](../design-system.md) §5「字体排印与信息密度」，以及「关于」页的字体许可条目（加入 Inter，JetBrains Mono 改为同时用于界面）。

---

## 1. 字体链（`apps/desktop/src/ui/theme.ts`）

```ts
const CJK = `"Noto Sans CJK SC", "Source Han Sans SC", "Microsoft YaHei UI", "Microsoft YaHei"`;
export const SANS = `Inter, ${CJK}, sans-serif`;
export const MONO = `"JetBrains Mono", ${CJK}, monospace`;
```

- **西文与数字：内嵌 Inter**（可变字重 WOFF2，约 300KB，SIL OFL）。界面专用字体，大 x 高度，自带 `tabular-nums`，两平台一致。
- **中文：不内嵌**，显式列出各平台最好的系统界面中文字体，不再依赖 fontconfig 对 `sans-serif` 的解析。完整思源黑体（8–16MB）超过整个安装包；子集化又会让题名等用户内容混入两种中文字体。
  - 已知代价：微软雅黑只有 Regular / Bold，600 在 Windows 上显示为 700，中文标题略重。接受。
- **等宽：内嵌 JetBrains Mono** Regular 与 SemiBold 两个 WOFF2（约 200KB）。引擎内嵌的那份编进二进制供 Typst 使用，界面拿不到，需另带一份。去掉 `Maple Mono`。
- `SANS` 作为根级默认字体统一生效，业务组件不再写 `fontFamily`（`MONO` 除外）。
- `EDITOR_MONO` 不变，编辑器有自己的字体设置。

## 2. 等宽字体的使用范围

只在三类内容上使用 `MONO`：

1. 代码（片段、TRD、源码预览）；
2. 文件路径与命令；
3. 构建、判题、插件运行日志。

slug、OJ 题号（如 `CF 1900A`）、版本号、数值一律改用 `SANS` + `tabular-nums`。需要列对齐的数字靠 `tabular-nums`，不靠等宽字体。

## 3. 字距（`apps/desktop/src/ui/tokens/type.ts`）

出处：Inter 作者公开的 Dynamic Metrics 公式 `tracking = −0.0223 + 0.185·e^(−0.1745·size)`（单位 em）。代入 6 档字号：

| Token | 字号 | 公式结果 | 折算 | 取值 |
| :--- | :--- | :--- | :--- | :--- |
| `micro` | 11 | +0.0048em | +0.05px | 0 |
| `caption` | 12 | +0.0006em | ≈0 | 0 |
| `body` | 13 | −0.0031em | −0.04px | 0 |
| `strong` | 14 | −0.0062em | −0.09px | 0 |
| `title` | 18 | −0.0143em | −0.26px | **−0.25** |
| `display` | 26 | −0.0203em | −0.53px | **−0.5** |

- 绝对值小于 0.1px 的调整在 1x 屏上不可见，取 0。
- `letterSpacing` 同样作用于中文；title 的 −0.25px 约为一个汉字宽度的 1.4%，不显拥挤。display 只显示数字。
- 字号、行高沿用 Phase 6D §2.1；字重仍只有 400 / 600，不因 Inter 为可变字体而引入 500。

## 4. 字体加载

- 字体文件作为应用资源随包分发，用 `registerFont("asset:/…")` 注册。
- **必须在首帧渲染前注册完成**（入口处 `await` 全部 `registerFont` 后再挂载根组件）。否则启动时先用回落字体画一帧再换成 Inter，构成闪烁与布局跳动，违反「零闪白、无 1px 抖动」。
- 注册失败时静默回落到字体链后续字体，不阻塞启动，不弹提示；诊断信息中的「字体来源」如实记录。

## 5. 执行顺序与验收

每步一个 jj 提交；提交前跑通与 `.github/workflows/lint.yml` 一致的本地检查。

1. **文档同步**：改写 `design-system.md` §5 与「关于」页字体许可条目。
2. **字体与字距**：加入字体资源、首帧前注册、`SANS` / `MONO` 字体链、`TYPE` 的 `letterSpacing`。
3. **等宽范围收缩**：逐处复核约 100 处 `fontFamily: MONO`，按 §2 改为 `SANS` + `tabular-nums` 或保留。
4. **验收**：在 Linux 上浅色 / 深色各截题库、比赛、训练总览、出版工作室，与改动前并排对比；冷启动时确认首帧即为 Inter、无字体替换闪烁。Windows 产物交叉打包后手动确认字体链与 600 字重表现。截图放临时目录，不进仓库。

## 6. 明确不做

- 内嵌任何中文字体或其子集。
- Inter 的可选字形特性（`cv*` / `ss*`），默认字形即可。
- 在 6 档之外新增字号，或为非 title / display 档设置字距。
- 用户可选的界面字体设置。
