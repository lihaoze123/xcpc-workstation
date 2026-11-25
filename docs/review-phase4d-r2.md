# Phase 4D 复审记录（工作副本修复）

> 复审对象：`ab2d96e` 之上**未提交**的工作副本（26 个文件，+3496/−1145），以及 `xcpc-plugins` 本地领先 `origin/main` 一个提交的 `a582da3`（**未推送**）。对照上一轮 `docs/review-phase4d.md` 的条目逐项复测。截图在 `docs/review-phase4d-r2/`。
> 环境与上一轮相同：全新 `<dataDir>`、线上官方仓库、`nsh dev`（debug 构建）、960×980 窗口，在 niri 下用 linux-cua 操作。
> 只记录不修。

## 总评

上一轮最要命的「插件工具点不动、输入被吞」已经**彻底修好**，信任门也真正落到了 Core 层。现在桌面端的插件系统**能用了**。

仍然挡路的问题集中在**发布链路**：官方仓库的 CI 从来没有成功跑过，SDK tarball 线上是 404，所以 `init --vite` 依旧装不上依赖。这部分修复只在本地，还没推送，而且推送了也跑不通（见阻断 3）。

| 上轮编号 | 结论 |
|---|---|
| 阻断 1 文件监听自激循环 | ✅ 已修复（Linux 上）。Windows 有新回归风险，见新问题 A |
| 阻断 2 带界面插件每次交互卡约 10 秒 | ⚠️ 大幅改善：engine 线程每次约 0.65s CPU（原来 10.4–12.5s），但离 1 帧还远 |
| 阻断 3 `init --vite` 依赖未发布的 npm 包 | ❌ 改成指向 release 里的 tarball，但这个 tarball 线上 404 |
| 阻断 4 `index.json` 不随 CI 更新 | ❌ 设计上改对了（`index.json` 作为 release 资产发布），但 CI 两次运行都失败，现在的 release 是手动发的 |
| 安全 1 信任确认只做在前端 | ✅ 已修复，Core 层强制执行 |
| 安全 2 坏插件冻结整个应用 | ✅ 已修复，坏插件超时期间界面照常响应 |
| 安全 3 仓库允许明文 http | ✅ 已修复，只放行回环地址 |
| 安全 4 `[unsandboxed-ui]` 标记、`ZIP_SLIP` 前缀重复 | ✅ 已修复 |
| 别扭 5–13 | 5、6、7、8、10、13 已修复；9 部分修复；11 修复但和「对话框居中」偏好冲突；12 只在未发布的插件版本里修了 |
| 打磨 14–19 | 14、16、17、18 已修复；19 除 `pack` 的默认输出位置（代码已改到上级目录，未实测）外都已修复；15 未复测 |

---

## 逐项复测

### 阻断 1：文件监听自激循环 ✅

- **实测**：空闲时引擎 CPU 约为 0；用 `inotifywait` 观察 4 秒，`plugins/` 下 `open` 事件为 0 次（上一轮 5 秒 953 次）。「复杂度与内存估算」里全选后输入 `5000`，值实时生效，表格同步刷新（`01`）。
- **热重载没被误伤**：dev link 一个带 `ui.js` 的插件后，原地写入（`>`）和原子替换（`mv tmp`）两种改法都能在 3 秒内热更新；新 link 的插件也会立刻出现在侧栏。
- **改法**：`is_relevant_plugin_watch_event` 只接受 `Create`、`Modify(Data|Name)`、`Remove` 三类事件，并补了单元测试。
- 上一轮建议补一个「用 `App.tsx` 启动、真实开启监听」的回归测试，这一轮**没有补**。现在只有事件分类的单元测试。

### 阻断 2：带界面插件的交互开销 ⚠️

- `diffVNodeTree` 改成按节点拆分 signal，只更新真正变化的节点，并把所有更新放进一次 `batch`。
- **实测**：位运算工作台点一个位格，nsh-engine 线程大约用 0.65s CPU（两段各约 0.3s），上一轮是 10.4–12.5s。`xcpc-plugin-ui` 线程基本不占 CPU。
- 两段 CPU 之间有约 1 秒完全空闲，三次点击都一样，所以从点击到界面变化大约要 1.6 秒。我核对了 NSH 的 waker 实现，没有发现问题。更可能的原因是 niri 对屏幕外窗口的帧回调做了节流（测试窗口在当前工作区的屏幕外列）。**这一点不确定**，建议在前台窗口、release 构建下复测墙钟时间。
- 首次进入位运算工作台要白屏约 5 秒，期间没看到 loading，进程常驻内存从 445MB 涨到 846MB。

### 阻断 3：`init --vite` 依赖装不上 ❌

- 脚手架现在生成 `"@xcpc/plugin-sdk": "https://github.com/lihaoze123/xcpc-plugins/releases/download/official-latest/xcpc-plugin-sdk.tgz"`。
- **实测**：这个 URL 返回 404，`npm install` 仍然第一步就失败。
- 打包 tarball 的步骤写在 `xcpc-plugins` 的 `a582da3` 里，这个提交**没有推送**。就算推送了也跑不通，原因见阻断 4。

### 阻断 4：官方仓库 CI ❌

客户端改为读 release 资产 `official-latest/index.json`，旧的 `main/index.json` 地址会自动迁移，方向是对的。但：

1. **CI 从来没有成功过**。`gh run list` 显示两次运行（`84bd574`、`e5c365b`）都失败了，失败在「Build xcpc CLI」这一步，`xcpc-engine` 的 `build.rs` 报 `XCPC_FONT_DIRS must be set`。现在线上的 release（04:57:28 发布）比失败的那次 CI 早 1 秒，是手动发的，里面的 `index.json` 还是旧格式：`releases/latest/download` 链接，仍然带 `has_unsandboxed_ui`。
2. CI checkout 的是 `xcpc-workstation` 的默认分支。远端 `main` 停在 `cbe5420`，**没有 `packages/plugin-sdk`**，也没有 4D 的 CLI。所以 `file:../xcpc-workstation/packages/plugin-sdk` 和「打包 SDK tarball」这两步就算过了字体那一关，也会失败。
3. 发布方式仍然是 `gh release delete` 后再 `create`。这中间有一个时间窗口，`index.json` 和所有 zip 都是 404，正在安装的用户会撞上。建议改成 `gh release upload --clobber`。
4. 图论画板「改边表时保留布局」（别扭 12）和描述对齐（打磨 16）也都在这个未推送的提交里，线上用户还拿不到。

---

## 安全

| 项 | 结果 |
|---|---|
| CLI 目录安装声明了域名的插件 | 待信任（`[pending-trust]`），`xcpc tool run` 返回 `PLUGIN_UNTRUSTED` ✅ |
| 开发链接（link）声明了域名的插件 | 同样是待信任 ✅ |
| 信任后在已安装副本里偷偷加一个域名 | 自动回到待信任 ✅ |
| 信任后把权限减到零 | 不需要信任 ✅ |
| GUI：「待信任」角标 → 确认弹窗列出域名 → 「信任并启用」+ Toast（`04`、`04b`） | ✅ |
| `xcpc plugin repo add http://example.com/...` | `HTTP_NOT_ALLOWED` ✅；`http://127.0.0.1` 放行 ✅ |
| 坏插件死循环期间切换到其他工具 | 立刻响应 ✅；回到坏插件后显示「插件操作超时」+「重新加载」（`03`），重新加载后恢复 ✅ |
| 仓库 SHA 不匹配 | 拒绝安装，提示「安装包校验失败，可能已被篡改，已取消安装」（`05`）✅ |
| MCP 是否开放了 `trust` / 仓库安装 | 没有开放 ✅ |
| `check` 的 dry-run | 现在能抓到 render 一执行就抛错的插件 ✅ |

**新发现**

- **B. 任何一个订阅的仓库都能「更新」掉别的来源装的同 id 插件，并且继承原有的信任**（中）。实测步骤：
  1. 先信任一个声明了 `example.com` 的 `net-tool`；
  2. 在另一个测试仓库里放一个同 id、更高版本、权限相同的包；
  3. 仓库浏览器里出现「更新」按钮，点一下就被替换了，名称变成「net-tool (被替换)」，仍然是已信任状态，不会再弹确认。

  信任只按「权限集合」判断，不记录来源仓库。建议在本地记录插件来自哪个仓库，跨仓库的同 id 条目不显示为「更新」；或者来源变化时重新要求信任。
- `install_from_repo_entry` 用的 `get_bytes` 会自动跟随重定向，但中间每一跳都不检查 https。因为下载后有 SHA 校验，影响有限，记下即可。

---

## 新问题

### A. Windows 上文件监听可能收不到「修改」事件（别扭，静态分析）

notify 8.2.2 的 Windows 后端把 `FILE_ACTION_MODIFIED` 上报为 `Modify(ModifyKind::Any)`（`notify-8.2.2/src/windows.rs:470`），而新的过滤只接受 `Modify(Data|Name)`。所以在 Windows 上，原地改写 `ui.js` / `index.js`（`vite build --watch` 就是原地写）很可能不会触发热重载。修法很小：把 `ModifyKind::Any` 也放进白名单，或者只排除 `Access` 和 `Modify(Metadata)`。项目会交叉打包 Windows 版本，建议手动验收一次。

### C. 任何插件变化都会让当前打开的界面插件重新挂载（别扭）

在位运算工作台里，用 CLI 装了两个**无关**的插件，当前界面就白屏数秒后重建（`02`）。状态因为存在 `storage` 里，所以没有丢。根源是 `pluginsRevision` 是全局计数，`PluginUiRenderer` 只要 revision 变了就重新挂载。建议只在当前插件自己的文件变化时才重新挂载：watcher 的事件里已经带了 `changed_paths`。

### D. 仓库拉取失败的提示仍然是原始错误，而且显示两遍（别扭）

- 拉取失败现在会提示了（上一轮 9 的一半）。但内容是 `HTTP_ERROR: http://127.0.0.1:18767/index.json: Connection Failed: Connect error: 连接被拒绝 (os error 111)`（`06`）。`formatPluginError` 只匹配英文的 `connection refused`，系统 locale 是中文时 OS 错误也是中文，所以没有命中。另外 ureq 的 `Connection Failed` / `Connect error` 也不在匹配列表里。
- 所有仓库都失败时，空态里有一个「恢复官方仓库」按钮，上方的仓库源列表里还有一个，同屏两个。
- 恢复官方仓库之后，同一条错误会在仓库行下方显示一次红字，又在列表上方的「有 1 个仓库拉取失败」块里再显示一次（`06b`）。

### E. `nsh test` 不是全绿（别扭）

默认环境下 205 个测试中 3 个失败：

- `tests::phase4c_plugin_toolbox_commands_and_render_bridge`：断言 4 个官方工具是内置插件，但 f916e01 已经把它们移到了官方仓库，属于过期断言。本轮没有改这个测试，可能是遗留问题。
- `tests::phase4a_plugins_overview_handles_repos_and_export`：期望 `repos.len() == 1`，实际是 2（默认订阅的官方仓库 + 测试加的）。同样可能是遗留问题。
- `training.test.tsx › TrainingView shell …`：**本轮修改过这个测试**，新断言是「空数据时顶栏不显示记一题按钮」。但无头 harness 读的是全局 `<dataDir>`，在有真实 OJ 数据的机器上（屏幕上能看到真实账号与 1889 题），onboarding 不会出现，于是失败。测试不封闭，CI 上大概率能过，在开发者本机必然失败。

我尝试用环境变量把整套测试的数据目录隔离开，结果失败数涨到 12 个（测试自己管理临时目录，和这些变量冲突），所以这个结果不作数。

### F. 手动记一题的弹窗改成了顶部对齐（待你决定）

聚焦题目输入框、候选列表展开后，输入框不再跳动，上一轮的 11 解决了（`07`）。做法是把弹窗固定在顶部，而不再垂直居中。这和之前定下的「对话框居中」偏好冲突，需要你决定接受哪个取舍。另一种做法是：弹窗保持居中，但给候选列表预留固定高度，或者改成浮层，不撑高弹窗。

### G. 打磨

- 仓库源列表里，手动添加的仓库仍然显示 URL，`index.json` 里的 `name`（本例是「测试仓库」）只在插件条目的 `repo_name` 里用到，没有回填到源列表。
- 手写的 `ui.js` 用 `export function render` 会报 `unsupported keyword: export`；只写 `function render` 会报「must export a render(sdk) function」，但没有提示正确写法是 `exports.render = …`。另外报错里的列号偏移很大（`1:162`，实际那一行只有约 60 个字符），看起来是外层包装代码的偏移没扣掉。
- 设置页里，内置的 AtCoder / Codeforces 也带「联网」角标。OJ 插件本来就必然联网，这个角标在 OJ 分区里没有提供信息量。

## 规格符合度（4D §6）对照

| 验收项 | 上轮 | 本轮 |
|---|---|---|
| 全新配置下「工具」空态展示 4 个官方工具，一键安装后切入 | ✅ | ✅（还支持连续安装多个，`00`） |
| 4 个工具逐个交互 | ❌ | ✅ 可用；位运算单次交互仍要 0.65s CPU |
| `init --vite → link → 热重载` | ⚠️ | ⚠️ link 和热重载都正常；`npm install` 仍然 404 |
| `check` / `pack` / `unlink` / 恢复内置 | ✅ | ✅（`check` 的 dry-run 增强了） |
| 官方仓库 CI 自动构建并更新 `index.json` | ❌ | ❌ CI 从未成功，修复未推送 |
| 社区 `registry.toml` 由 CI 处理 | ❌ | ⚠️ `build-index.mjs` 已经支持（附测试），但未推送 |
| 堆栈指向真实文件名 | ✅ | ✅（列号偏移见打磨） |

## 校验

- `cargo fmt --all -- --check` 通过；`cargo test -p xcpc-plugin -p xcpc-sync -p xcpc-mcp` 全部通过。
- `nsh test`：205 个测试中 202 个通过，3 个失败（见 E）。
- 测试期间在 `<dataDir>` 隔离环境里移除、恢复过官方仓库，也装过测试仓库的插件，都没有碰到你的真实配置。
