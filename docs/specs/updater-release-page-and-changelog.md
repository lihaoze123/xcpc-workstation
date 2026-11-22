# Spec：应用更新策略、只读安装发布页引导与更新日志展示

> 状态：已确认，待实现（2026-11-22）。  
> 关联文档：[architecture.md](../architecture.md)、[design-system.md](../design-system.md)、[CHANGELOG.md](../../CHANGELOG.md)。

---

## 1. 背景与现状分析

桌面端已接入 NSH 0.6.0 原生更新模块 `nsh/updater`（`apps/desktop/nsh.json` + `apps/desktop/src/ui/SettingsView.tsx` + `.github/workflows/release.yml`），检查更新已确认可正常工作，但存在两处体验问题：

1. **Linux 系统包与 AppImage 检测到新版本后直接跳转下载 `.tar.gz`**：
   - `SettingsView.tsx` 根据 `AppUpdate.canInstall` 决定点击行为：`canInstall === true` 时走应用内下载、校验、原子替换与重启；`canInstall === false` 时调用 `openUrl(update.url)`。
   - 上游 `nsh-updater` 的 `Installation::can_install()` 通过在当前可执行文件所在目录的**父目录**试建临时目录来判断是否支持同卷原子 `rename` 替换。
   - `.github/workflows/release.yml` 传给 `nsh update-manifest` 生成 `latest.json` 的是 `.tar.gz` / `.zip`（供应用内自更新解压使用），因此 `update.url` 指向 `.tar.gz` 直链。当用户通过只读方式安装时，点「前往下载」会直接拉起浏览器下载 `.tar.gz`，与用户的实际安装方式不匹配。
2. **发现新版本时不显示更新日志**：
   - `SettingsView.tsx` 已预留 `update.notes` 展示区域，但 `.github/workflows/release.yml` 调用 `nsh update-manifest` 时未传入 `--notes <FILE>`，导致 `latest.json` 中没有更新日志字段。
   - 现有 UI 仅在 `available` 状态展示 `update.notes`，且以原始纯文本 `<Text>` 渲染，未处理 Markdown 语法与长日志滚动。

### 1.1 上游 NSH 0.6.0 更新机制说明

上游 `nsh/updater` 提供两套能力：

| 机制 | API | 能力边界 |
| :--- | :--- | :--- |
| **整包自更新**（当前采用） | `checkForUpdate` / `downloadUpdate` / `installAndRestart` / `installOnQuit` | 下载对应平台压缩包，校验签名与 SHA256，原子替换整个应用目录（含 Rust 二进制、JS 前端、内嵌字体与 `xcpc` CLI）并重启；要求安装目录父目录对当前用户可写。 |
| **代码包 OTA 热更新** | `checkForPackage` / `downloadPackage` / `restartToPackage` | 将签名后的 JS/TS bundle 写入用户数据目录 `<data_dir>/<app_id>/updates/`，下次启动加载（不修改安装目录）。**限制**：本项目为 `--template mixed`，核心引擎、Typst、存储、Lint 与原生桥接均编译在 Rust 二进制中，OTA 仅能更新纯前端改动，且需额外维护根密钥与发布流。 |

### 1.2 各安装形态的 `canInstall` 行为矩阵

| 平台与安装形态 | 安装位置 | `canInstall` | 预期更新体验 |
| :--- | :--- | :---: | :--- |
| Windows Inno Setup 安装包 | `%LOCALAPPDATA%\Programs\XCPC Workstation`（每用户可写） | `true` | 应用内下载 → 校验 → 立即重启安装 / 退出时安装 |
| Windows 便携 `.zip` | 用户解压目录（可写） | `true` | 同上 |
| Linux 便携 `.tar.gz` | 用户解压目录（可写） | `true` | 同上 |
| Linux `.deb` / Arch `.pkg.tar.zst` | `/opt/xcpc-workstation`（`root` 拥有，归包管理器管辖） | `false` | 按钮显示「前往发布页」，打开该版本 GitHub Release 页面 |
| Linux `AppImage` | 运行时挂载在只读 FUSE `/tmp/.mount_…` | `false` | 与系统包统一，按钮显示「前往发布页」，打开该版本 GitHub Release 页面 |

---

## 2. 已确认的架构与产品共识

以下决策已与用户逐项确认：

1. **只做整包更新**：沿用 `nsh/updater` 的整包更新机制，**不引入** NSH 的 JS 层 OTA 热更新（`checkForPackage`）。
2. **只读安装统一引导至 GitHub Release 页面**：当 `canInstall === false`（包括 `.deb`、Arch `.pkg.tar.zst`、只读目录安装以及 `AppImage`）时，不再打开 `.tar.gz` 直链，而是打开该版本对应的 GitHub Release 页面，由用户按自身安装方式选用 `.deb` / `.pkg.tar.zst` / `AppImage` 或对照更新日志升级。
3. **不为 AppImage 单独实现自更新**：遵循「做减法（less is more）」，不在 Rust 侧额外维护 `$APPIMAGE` 下载、校验与覆盖重启逻辑。
4. **保持纯手动检查更新**：只在「设置 → 关于」点击「立即检查」时发起网络请求，启动时不自动静默检查，符合本地优先原则。
5. **文案保持极简**：检测到新版本时，无论 `canInstall` 真假，副标题均保持 `发现新版本 vX.Y.Z`；仅通过主按钮文案区分（`canInstall === true` 为「下载更新」，`canInstall === false` 为「前往发布页」）。
6. **更新日志以 `CHANGELOG.md` 为单一事实源**：CI 从仓库根目录 [CHANGELOG.md](../../CHANGELOG.md) 提取当前版本段落，同时喂给 `nsh update-manifest --notes`（写入并签入 `latest.json`，供应用内展示）和 GitHub Release 正文，避免双处维护。

---

## 3. 具体设计方案

### 3.1 发布页 URL 推导（`canInstall === false`）

在桌面端（`apps/desktop/src/ui/SettingsView.tsx` 或抽出的纯函数模块）新增 `resolveReleasePageUrl(update: Pick<AppUpdate, "url" | "version">): string`：

1. 优先解析 `update.url`：若匹配 GitHub Release 资产直链格式  
   `^https://github\.com/([^/]+/[^/]+)/releases/download/([^/]+)/.+$`  
   则转换为对应版本的 Release 页面：  
   `https://github.com/<owner>/<repo>/releases/tag/<tag>`。
   - 保留原始 `<tag>`（兼容带 `v` 前缀或不带 `v` 前缀、预发布标签等，也兼容 fork 仓库）。
2. 若 `update.url` 不匹配上述模式但已是合法 `https://` 页面链接（例如未来自定义镜像站），或无法解析出 GitHub 仓库路径：
   - 若已知默认仓库，可回退到 `https://github.com/lihaoze123/xcpc-workstation/releases/tag/v<version>`（去掉重复的 `v` 前缀）；若连 `version` 都为空则回退 `update.url`。
3. `handleDownload` 在 `!update.canInstall` 分支调用 `openUrl(resolveReleasePageUrl(update))`；失败时提示 `"无法打开发布页面，请稍后重试"`。

### 3.2 CI 提取 `CHANGELOG.md` 与签名写入

在 [.github/workflows/release.yml](../../.github/workflows/release.yml) 的 `release` job（以及可独立测试的提取脚本，如 `scripts/extract-changelog.py`）中：

1. **提取规则**：
   - 读取根目录 `CHANGELOG.md`，查找首个匹配 `^## \[?<version>\]?(\s|$)`（去掉可选前导 `v`，如 `0.2.0-beta.2` 匹配 `## [0.2.0-beta.2] - 2026-11-22`）的二级标题行。
   - 提取该标题之后、下一个 `^## ` 二级标题（或文件末尾）之前的内容，去除首尾多余空行。
   - 输出到临时文件（如 `release-notes.md`）；若未匹配到对应版本，不报错中断发布，输出空文件并标记未找到。
2. **喂给 `nsh update-manifest`**：
   - 若提取出的 `release-notes.md` 非空，在调用 `nsh update-manifest` 时追加 `--notes release-notes.md`。
   - `nsh update-manifest` 会将该内容写入 `latest.json` 并用 `NSH_UPDATER_SECRET_KEY` 整体签名，客户端 `checkForUpdate({ locale: "zh-CN" })` 即可直接拿到 `update.notes`。
3. **同步到 GitHub Release 正文**：
   - `softprops/action-gh-release@v2` 在 `release-notes.md` 非空时使用 `body_path: release-notes.md`（并关闭 `generate_release_notes` 或仅在未提取到时回退 `generate_release_notes: true`），确保 GitHub Release 页面与应用内展示完全一致。

### 3.3 桌面端更新日志展示（`SettingsView.tsx`）

1. **可见生命周期**：
   - 只要当前更新状态携带 `update`（`available`、`downloading`、`ready`、`scheduled`）且 `update.notes?.trim()` 非空，就展示更新日志区域，不再在开始下载后突然消失。
2. **轻量 Markdown 渲染**：
   - 不引入重型 Markdown 渲染依赖，编写小型纯解析函数把 Keep a Changelog 常见语法转为结构化节点并在 NSH `<View>` / `<Text>` 中渲染：
     - `### 小节标题`（如 `新增`、`改进`、`修复`、`变更`）：渲染为紧凑分组小标题（`fontWeight: 600`、`text-foreground`）。
     - `- ` / `  - ` 一级与二级无序列表：保留缩进层级与圆点前缀。
     - 行内 `**加粗**` 与 `` `行内代码` ``：使用嵌套 `<Text>` span 分别应用 `fontWeight: 600` 与等宽字体样式。
     - 普通段落：正常换行展示。
3. **布局与滚动**：
   - 放置在更新卡片底部的分隔区内（`borderTopWidth: 1`，`bg-muted/40`），外包 `<ScrollView style={{ maxHeight: 240 }}>`（短日志自适应内容高度，长日志内部滚动，不撑爆「关于」页面）。
   - 遵守 `docs/design-system.md` 规范：不暴露内部技术细节，不出现布局抖动。

---

## 4. 涉及文件

- [apps/desktop/src/ui/SettingsView.tsx](../../apps/desktop/src/ui/SettingsView.tsx)（以及按需抽出的纯函数工具文件，例如 `apps/desktop/src/ui/updateNotes.ts`）
- [scripts/extract-changelog.py](../../scripts/extract-changelog.py)（新建，供 CI 调用并便于本地单测）
- [.github/workflows/release.yml](../../.github/workflows/release.yml)
- 对应单元测试（TS 侧 `*.test.tsx` + Python/CI 提取脚本测试）

---

## 5. 验收标准与测试计划

1. **发布页跳转**：
   - 单元测试验证 `resolveReleasePageUrl` 对标准 GitHub 资产链接（含正式版与 `-beta.N` 预发布 tag）、非标准 URL、以及回退路径的输出。
   - Linux `.deb` / Arch / AppImage 安装下点击「前往发布页」，浏览器打开对应版本的 `https://github.com/lihaoze123/xcpc-workstation/releases/tag/v<version>`，不再触发 `.tar.gz` 下载。
   - Windows 安装包 / 绿色 `.zip` / Linux 用户目录 `.tar.gz` 保持 `canInstall === true`，按钮为「下载更新」，应用内下载并重启安装流程不受影响。
2. **更新日志提取与签名**：
   - 本地对当前 [CHANGELOG.md](../../CHANGELOG.md) 运行提取脚本，能精确提取 `0.2.0-beta.2`、`0.2.0-beta.1`、`0.1.0` 各自段落，且对不存在的版本安全返回空结果。
   - `nsh update-manifest --notes` 生成的 `latest.json` 包含对应 `notes` 字段且验签通过。
3. **桌面端更新日志渲染**：
   - 单元测试覆盖 `###` 小节标题、二级缩进列表、`**加粗**`、`` `行内代码` `` 与普通段落解析。
   - 组件测试验证 `available` / `downloading` / `ready` / `scheduled` 状态下均显示更新日志，且 `canInstall === false` 时按钮文案为「前往发布页」、副标题仍为 `发现新版本 vX.Y.Z`。
