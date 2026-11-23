# 概念 Spec：Lemon-like 本地比赛求解器与 XCPC 反作弊审计（候选 Phase 3E）

> 状态：概念已对齐，待排期（2026-11-24）
> 排期定位：候选 **Phase 3E**（Phase 3D「验题人外部提交试跑」的自然延伸），待 Phase 3C / 3D 落地后评估是否排在 Phase 4 之前。
> 关联文档：[roadmap.md](../roadmap.md)、[phase-3d-contest-readiness.md](./phase-3d-contest-readiness.md)、[phase-3c-interactive-communication-subtasks.md](./phase-3c-interactive-communication-subtasks.md)

## 1. 愿景与核心思想

传统办赛面临两难：组织 XCPC 比赛必须部署和维护重型实时评测机群（DOMjudge / Hydro Judge）以防恶意代码与峰值排队；而传统 OI 本地评测器（Lemon / LemonLime）仅支持单次离线收卷，不支持 XCPC 实时反馈与罚时体系。

本构想将 **Optimistic Rollup + 欺诈证明（Fraud Proof）** 的思想引入算法竞赛：

- **赛中极轻量化**：无需重型实时评测机。既可以完全无赛中评测（纯 Lemon / 盲打 XCPC），也可以由极轻量的静态/无状态服务分发 WASM 评测包在选手本地运行以提供即时反馈，服务端仅收集带时间戳与元数据的源码提交并展示「乐观实时榜」；
- **赛后本地权威求解**：赛后组织者将全部提交归档导入 XCPC Workstation，在本地安全沙箱中基于权威测试数据统一重放所有提交，求解最终官方榜单（ICPC 罚时榜 / OI 得分榜 / IOI 最高分榜）；
- **Play-Game 反作弊审计**：若选手在赛中篡改本地评测结果或元数据戏弄 OJ，赛后权威重放将自动生成确定性欺诈证明并判定为 **`CHEATER`**。

---

## 2. 架构边界

- **XCPC Workstation 负责**：定义**开放提交归档协议** + 内置**本地安全权威求解器、欺诈审计引擎、断点续测缓存与榜单/滚榜生成器**。
- **彻底解耦赛中端**：赛中轻量收题服务器、WASM 选手本地评测页或第三方收集脚本均作为独立薄外壳，只需按开放协议产出提交归档即可无缝接入。

---

## 3. 不可信代码安全沙箱

与 Phase 3A–3D 面向出题人可信代码不同，本地比赛求解器运行的是**外部不可信选手代码**，必须默认强隔离：

- **Linux 原生轻量沙箱**：默认通过 `bwrap`（Bubblewrap）或 Linux user/mount/network/pid namespace + 断网（`--unshare-net`）+ 只读空工作目录 + `rlimit` 隔离每个选手进程，杜绝偷读测试数据、读取宿主文件、发起网络请求或 fork bomb。
- **Fail-Closed 原则**：本机未检测到可用沙箱运行时默认拒绝执行外部选手代码，仅允许组织者显式传入 `--unsafe-no-sandbox` 绕过（例如已在专用离线虚拟机中运行）。

---

## 4. 开放提交归档协议与赛制求解

### 4.1 两种输入形态

1. **标准开放归档（推荐，通吃 XCPC / OI / IOI）**：
   ```text
   archive/
   ├── submissions.jsonl          # 按时间排序的提交事件流
   └── codes/<submission_id>.cpp  # 原始提交源码
   ```
   每行 `submissions.jsonl` 记录包含：`submission_id`、`contestant`（ID / 显示名 / 队伍属性）、`problem`（题号或 slug）、`language`、`code_path`、`code_sha256`、`server_timestamp`（服务端权威时间戳），以及可选的 `claimed_verdict` 与 `judge_scope`（`"none" | "pretest" | "full"`）。
2. **传统 Lemon / NOI 文件夹兼容**：
   直接导入 `contestants/<选手名>/<题号或slug>.cpp` 目录结构，自动转化为 `judge_scope = "none"` 的单次提交集合。

### 4.2 支持的赛制模式

- **ICPC 模式**：按权威重放结果取每题**最早一次权威 AC**，之前有效错误提交累计罚时（默认 20 分钟，CE 默认不计罚时，均可配置），忽略首 AC 之后的重复提交；支持封榜时间点以生成滚榜数据。
- **OI 模式（经典 Lemon）**：取每题最后一次提交的权威得分（支持 Phase 3C 子任务与 `quitp` 部分分）。
- **IOI 模式**：取每题历次提交中的最高权威得分（或按子任务取最高分）。

---

## 5. 反作弊防线与分层审计模型

### 5.1 协议级三道防线

1. **防掉包与防改时**：校验归档内源码真实 SHA-256 与 `code_sha256` 一致，结合 `server_timestamp` 防赛后篡改源码或倒改提交时间；
2. **`judge_scope` 分层（`none` / `pretest` / `full`）**：
   - `none`：无赛中反馈，纯权威求解；
   - `pretest`：赛中 WASM 仅含 Pretest（不怕被逆向），反作弊审计只在 Pretest 子集上比对 `claimed_verdict`，最终判定取全量 System Tests；
   - `full`：赛中本地评测覆盖全量数据，反作弊直接在全量数据上比对 `claimed_verdict`；
3. **可选蜜罐测试（`honeypot`）**：允许组织者在赛后权威重测时追加同分布新随机种子测试点；若选手在全量数据上通过（疑似逆向 WASM 打表）却在蜜罐点确定性 WA，自动标记 `SUSPECT` 提示人工审查。

### 5.2 分层裁定状态

| 裁定状态 | 触发条件 | 榜单与后续处理 |
| :--- | :--- | :--- |
| **`CLEAN`** | 权威结果与 `claimed_verdict` 一致（或 `judge_scope = none`） | 正常计入官方榜 |
| **`REJUDGED`** | 合理硬件/边界差异（如本地 AC → 权威 TLE/MLE，或 Pretest AC → System Test WA） | 按权威结果重算罚时/得分，**不判作弊** |
| **`SUSPECT`** | 可疑异常（如异常 RE、挂蜜罐测试点、异常高频/规律提交） | 进入人工复核队列，支持组织者查看证据后手工裁定「赦免」或「追加 DQ」 |
| **`CHEATER`** | 确定性欺诈（同组数据上声明 AC 但权威确定性 CE/WA/PE、或源码/元数据哈希校验失败） | 自动标红并移出正式榜（DQ），保留完整审计证据链（支持组织者显式覆写） |

---

## 6. 断点续测、工作台与输出产物

- **内容哈希断点续测**：复用 `xcpc-judge` 缓存架构，按 `(code_sha256, problem_package_hash, sandbox_profile)` 缓存评测结果。评测中途中断续跑、追加补交选手或组织者修改 `SUSPECT` 裁定后重算榜单均可在毫秒级完成。
- **输出产物**：
  - 独立离线 **HTML 成绩单与审计报告**（单文件可分发，含排行榜、逐题统计与公开/内部版审计视图）；
  - **CSV 成绩表**（便于教务导入与评奖）；
  - **滚榜 JSON**（兼容 XCPCIO / ICPC Resolver 格式，直接驱动赛后滚榜）。
- **桌面端形态**：比赛内提供「赛后求解与审计」工作台，展示实时沙箱评测进度、权威榜单、点击任意选手查看逐发提交的权威重放与欺诈证据，以及一键裁定复核队列。
