# 规格：Vello 预览渲染引擎与 `nsh-gpu` 零拷贝演进（原 Phase 6H，已延后）

> 状态：**整体延后至「待定与远期」**（2026-11-28 排期对齐确认）。Phase 6 在 6A–6G（`v0.6.0-beta.3`）收口；本规格（含 6H.1 与 6H.2）待上游 `longbridge/nsh` 合入 Windows D3D12 `GpuView` 且 `nsh-gpu` 与 Vello 对齐同一 `wgpu` 主版本后再统一评估重启。

2026-11-27 `/grill-me` 对齐确认。目标是在**保留 NSH 原生控件、中文 IME 与 `<WebView>` 插件融合优势**的前提下，将桌面端 Typst 实时预览的光栅化与缩放管线从 `typst-render`（`tiny-skia` CPU）平滑演进为基于 Vello（Sparse Strips）的 GPU 矢量渲染，并最终通过 NSH 上游 `nsh-gpu`（`GpuView`）实现零拷贝实时矢量缩放。

---

## 1. 背景与上游技术核查（2026-11-27）

1. **为什么不走 NSH `<Canvas>` 路径回放（路径 A1 否决）**：
   - NSH `CanvasOp` 缺少按整形结果绘制的 `DrawGlyphRun` 原语；Typst 排版输出的是 HarfBuzz 整形后的 `(Font, GlyphId, offset)`，若改走 `fillText(string)` 会破坏数学公式与中英混排定位，若用 `ttf-parser` 把整页 800–1500 个汉字展开成 5–10 万条贝塞尔段喂给 Direct2D `ID2D1PathGeometry`，CPU 构图反而更慢且 9–10pt 小字号缺少 hinting/高质量抗锯齿。
2. **NSH 上游 `nsh-gpu`（`GpuView`）现状**：
   - `longbridge/nsh` `main`（面向 `v0.7.2`）已合入 PR #3223（`gpu_view: Draw a view with an app's own wgpu renderer`）与 PR #3240（macOS/iOS），新增 `nsh-gpu` crate 与 `<GpuView>` 元素，允许 App 自带 `wgpu` 渲染器在独立渲染线程直接绘入 `GpuFrame::target(&wgpu::TextureView)`，不阻塞主线程，不再需要我们自行向上游提案 `external_texture` 概念。
   - **当前阻塞点**：`GpuView` 的 **Windows D3D12 后端尚未合入**（PR #3223 标注为 follow-up PR；Windows 端已有 `shader.rs` D3D12↔D3D11 `SharedTexture` 基础与 PR #3405 脏区局部重绘）；此外 NSH 0.7.2 的 `nsh-gpu` 锁定 **`wgpu 29`**，而上游 `vello_gpu 0.4.0` 已升级到 `wgpu 30`，接入时需对齐同一 `wgpu` 主版本。
3. **`tinymist-viewer` 参考与差异**：
   - `Myriad-Dreamin/tinymist` 的 `crates/tinymist-viewer/src/render.rs`（`typst 0.15.1` + `vello 0.7.0`）因为是 LSP 跨进程通过 WebSocket 推送 `reflexo` 矢量 IR，viewer 端没有 `PagedDocument` 与 `typst-render`，因此必须自己实现全量原语并把字形经 `reflexo` 转成 SVG 路径字符串；其核心可借鉴点在于**按页缓存矢量场景（`VecScene`）供缩放/平移直接套新 `Affine` 矩阵复用**，以及 `Tiling`（约 40 行 clip + 步长平铺）与 SVG（`resvg` 光栅化为 `peniko::ImageData`）的处理方式。
   - XCPC Workstation 是**进程内直连 `PagedDocument`**，不需要 `reflexo` 跨进程 IR；可直接遍历 `typst::layout::Frame`，利用 `vello_cpu`/`vello_gpu` 原生字形缓存与提示能力，并用现成的 `typst-render` 作为罕见原语的单页安全兜底。
4. **上限边界**：
   - 本阶段优化的是**编译后光栅化、高倍缩放瓦片与像素缓存开销**；`typst::compile`（含 CeTZ / MiTeX 增量排版）仍是 CPU 阶段，不在本阶段改造范围。

---

## 2. 两阶段演进路线（6H.1 → 6H.2）

### 2.1 Phase 6H.1：解耦 Vello 渲染器 + `RgbaPixels`（立即可落地）

- **不修改 NSH 上游**，保持现有 `PreviewCanvas`、手势期合成器 `transform`、`PageImage`、512px 分块瓦片与 `PIXELS`（48 MiB）契约不变，前端零改动。
- 在 `crates/xcpc-engine` 内通过**可选 Cargo feature `vello-preview`**（`src/vello/`）实现与 NSH 解耦的 `Typst Frame → Vello` 场景转换器、按页矢量场景缓存与渲染后端，通过 `Compiled` 渲染接口输出 `RgbaPixels` 喂给 `xcpc-app::PIXELS`。
- Linux 与 Windows 立即可用，受前置 PoC 量化门控约束（见 §5）。

### 2.2 Phase 6H.2：接入 `nsh-gpu`（`<GpuPageView>` 零拷贝直连）

- **前置条件**：`longbridge/nsh` 合入 Windows D3D12 `GpuView` 后端，且 `nsh-gpu` 与所选 Vello crate 的 `wgpu` 主版本对齐。
- **架构升级**：
  - 复用 6H.1 已建成的 `Typst Frame → Vello` 转换器与按页矢量场景缓存，不重写渲染核心；
  - 在 `apps/desktop` 中基于 `nsh-gpu` 新增 `<GpuPageView>`（实现 `nsh_gpu::GpuRenderer`），渲染线程直接按当前视口与缩放倍数将缓存场景画入 `GpuFrame::target(&wgpu::TextureView)`，直接共享 NSH 的 `wgpu::Device`（不再维护独立 GPU 设备）；
  - **撤掉 512px 分块瓦片与 48 MiB `PIXELS` 像素缓存**（仅保留冷启动首帧快照与单页回退位图上传纹理），手势缩放期间实时矢量重绘（零糊图、零瓦片拼接、零 CPU 像素回传）；
  - 无 GPU / 设备初始化失败环境静默回退现有 `PageImage` + CPU 路径。

---

## 3. 后端选型、适用范围与内存治理

### 3.1 Sparse Strips 双后端架构（`vello_gpu` + `vello_cpu`）

- **主架构**：采用 Linebender Sparse Strips 体系（`vello_gpu` 原 `vello_hybrid` + `vello_cpu`），共享同一套 `Typst Frame → RenderContext` 场景编码层：
  - **GPU 主路径（`vello_gpu`）**：CPU 轻量稀疏条带编码 + GPU 片元着色器光栅化合成，对核显/集显兼容性远优于纯 Compute Shader，且 6H.2 可直接绘制到 `nsh-gpu` 的 `GpuFrame::target`；
  - **CPU 同构路径（`vello_cpu`）**：纯 SIMD 多线程 CPU 光栅化，零 GPU 上下文与显存开销，用于无 GPU 机器、远程桌面、无头测试或 GPU 设备丢失时的透明回退（前置 PoC 中同步对比经典 `vello` Compute Shader，以实测数据决定最终 GPU crate）。
- **GPU 上下文生命周期与内存红线（6H.1）**：
  - 6H.1 尚未共享 NSH 的 `wgpu::Device`，独立创建 `wgpu::Device` 会有约 40–80 MB 进程私有内存/显存提交；
  - 必须采用**按需懒初始化**（仅在真正进入预览渲染时创建），并接入现有 `reclaim_memory(clear_pixels)` 回收链路——离开工作室空闲超时（4s）后**主动销毁离屏 GPU 渲染上下文与暂存缓冲**，守住 Windows 空闲内存既有基线（≤ 100 MB）。

### 3.2 适用范围与 Crate 边界

- **仅用于桌面端实时预览**：覆盖出版工作室（单题 / 比赛 / 题解）、题库详情连续滚动阅读器、TRD / 算法模板预览、编辑器悬浮迷你公式预览。
- **保持不变的入口**：`xcpc` CLI（`xcpc render`）、MCP（`render_page_png`）、视觉回归基准（`scripts/olymp-regress.sh`）以及 PDF 导出（`typst-pdf`）**继续使用 `typst-render` / `typst-pdf`**。
- **Crate 与 Feature 隔离**：实现放在 `crates/xcpc-engine/src/vello/`，受 `vello-preview` 可选 feature 控制；仅 `apps/desktop`（经 `xcpc-app` feature 透传）开启，`xcpc-mcp` / CLI 默认不编译 Vello 与 `wgpu`，保持 CLI ~50 MB 内存、~144 ms 热耗时与极小体积。

---

## 4. `Typst Frame → Vello` 转换器边界与缓存

### 4.1 原语支持矩阵

| Typst 0.15 `FrameItem` / 特性 | 处理策略 | 说明 |
| :--- | :--- | :--- |
| `Group`（`transform` + `clip`） | **原生支持** | 递归压栈 `Affine` 与 `Curve` 裁剪层 |
| `Text`（轮廓字形，含数学/中文/等宽） | **原生支持** | 直接送入 Vello 字形管线（内置字形缓存与提示），保留 Typst 精确 `(x_advance, x_offset, y_advance, y_offset)` 与 fill/stroke |
| `Shape`（`Line` / `Rect` / `Curve`，填充与描边/虚线） | **原生支持** | 映射至 `kurbo::BezPath`、`Stroke`（cap/join/miter/dash）与 `Fill`（NonZero / EvenOdd） |
| `Paint::Solid` / `Paint::Gradient`（Linear/Radial/Conic） | **原生支持** | 完整映射颜色空间转换（sRGB）与多停靠点渐变 |
| `Paint::Tiling`（Pattern 平铺） | **原生支持（轻量）** | 借鉴 `tinymist-viewer`：对目标路径建 clip 层并按步长平铺子场景 |
| `Image`（PNG / JPEG / GIF / WebP） | **原生支持** | 解码并按内容哈希缓存 `peniko::ImageData` |
| `Image`（SVG） | **原生支持** | 复用 `Cargo.lock` 已有 `resvg` 光栅化后缓存为 `peniko::ImageData` |
| 彩色位图 / SVG / COLR Emoji 字形、内嵌 PDF 图片等罕见原语 | **透明单页回退 `typst-render`** | 转换前/转换期检测到不支持原语即中止该页构建，静默改由 `typst-render` 出图并向 `xcpc-trace` 记一条 `WARN`；6H.2 阶段将回退位图上传为 GPU 纹理绘制 |
| `Link` / `Tag` | **跳过** | 非绘制原语；正反向定位继续走既有 `locate_point` / `locate_source` |

### 4.2 按页矢量场景缓存

- 以既有 span 无关的 `page_fingerprint(page)`（`u128`）为键，在 `xcpc-engine` 内缓存每页已转换的不可变矢量场景（LRU 有界上限，如最近 16–32 页，随 `reclaim_memory` 清空）。
- 编辑单页时仅重新转换指纹变化的页；缩放、平移与瓦片重绘直接对缓存场景施加新的 `Affine`（平移 + 缩放）矩阵，**零重复遍历 Typst `Frame` 与零重复解码图片**。

---

## 5. 前置 PoC 量化门控（6H.1 硬前提）

在修改桌面端主链路前，先在 `crates/xcpc-engine` 新增独立基准（如 `examples/vello_bench.rs`），覆盖三类真实负载：
1. `tests/fixtures/olymp-ref/contest-zh` 中文多页题面（1.5× 基础整页 + 4× 局部瓦片）；
2. 含复杂 CeTZ 几何/图论矢量的题面页；
3. TRD 横版三栏密集代码页。

在 **Linux** 与 **Windows（Intel Arc 核显）** 上分别实测 `typst-render`、`vello_cpu`、`vello_gpu` 与经典 `vello`，满足以下**全部三项门控**后方可合入桌面主链路：

| 门控维度 | 通过标准 | 未达标时的决策 |
| :--- | :--- | :--- |
| **性能** | 复杂矢量页或高倍缩放（≥ 2.5×）光栅化 P95 相比 `typst-render` 提速 **≥ 2×**，且普通中文单页耗时不退化 | 若 6H.1 下 GPU 读回相比 `vello_cpu` 无明显优势（或更慢），6H.1 自动收敛为默认 **`vello_cpu`**，GPU 渲染器留至 6H.2 `nsh-gpu` 零拷贝直连再启用 |
| **画质** | 3 套 `olymp-ref` 基准用例（共 13 页）自动化像素回归无缺字、无错位、无裁切错误；9–10pt 中英文与数学公式人工并排目视确认笔画清晰、不发虚 | 调整字形提示/抗锯齿参数或排查转换差异，不达标不合入 |
| **内存** | Windows 活跃单题预览增量可控，离开工作室 4s 空闲回收后 GPU 上下文彻底释放，进程私有内存回到既有基线（**≤ 100 MB**） | 若独立 `wgpu::Device` 无法在 Windows 上干净释放回 ≤ 100 MB，则 6H.1 改用 `vello_cpu`、GPU 留给 6H.2 共享 NSH 设备 |

---

## 6. 界面克制与可观测性

- **设置页零新增开关**：遵循 [design-system.md](../design-system.md) 与 `AGENTS.md` 原则，不在「设置」页面暴露「硬件加速 / 渲染后端切换」选项，也不在主工作区显示任何渲染后端标签。
- **诊断窗口集成**：在独立诊断窗口（`nsh-o11y`）的 Metrics 与 Logs 视图展示当前生效预览后端（`vello-gpu` / `vello-cpu` / `typst-render`）、单页场景转换耗时、光栅化耗时与单页回退次数。
- **开发者排障开关**：支持环境变量 `XCPC_PREVIEW_RENDERER=auto|vello-gpu|vello-cpu|typst`（默认 `auto`），便于本地 A/B 对比、Trace 录制与问题定位。

---

## 7. 执行顺序与验收

每步保持独立小提交；提交前在远端构建机跑通与 `.github/workflows/lint.yml` / `ci.yml` 一致的全部检查（含 `vello-preview` 开启与关闭两种编译组合）。

1. **6H.0 前置 PoC 与选型门控**：搭建 `xcpc-engine` `vello-preview` feature 与最小 `Frame → Vello` 原型，完成 `vello_bench`（Linux + Windows 实测）、`olymp-ref` 13 页画质比对与内存回收验证，记录结论到本规格。
2. **6H.1a 完整转换器与场景缓存**：补齐 `Tiling` / SVG（`resvg`）/ 渐变 / 裁剪 / 单页 `typst-render` 回退（带 `WARN` 日志）与按页 `page_fingerprint` 矢量场景缓存，补充 `olymp-ref` 像素相似度回归测试。
3. **6H.1b 接入 `xcpc-app` 与空闲回收**：将桌面端预览与悬浮数学预览接入 Vello 渲染器，接入 `XCPC_PREVIEW_RENDERER` 开关、诊断窗口指标上报与 `reclaim_memory` GPU 上下文释放；在 Windows 实机用 `cua-driver` 与诊断窗口复测帧率、画质与内存基线。
4. **6H.2 `nsh-gpu` 零拷贝直连（上游就绪后）**：升级至含 Windows D3D12 `GpuView` 的 NSH 版本并对齐 `wgpu`，实现 `<GpuPageView>`、移除 512px 瓦片与 48 MiB `PIXELS` 缓存、实现手势实时矢量缩放与无 GPU 自动回退 `PageImage`，完成双平台帧率与内存验收。

---

## 8. 明确不做

- 不采用 NSH `<Canvas>` / `CanvasOp` 路径回放方案（A1）。
- 不引入 `reflexo` 跨进程矢量 IR 或 WebSocket 预览协议。
- 不替换 `xcpc` CLI（`xcpc render`）、MCP（`render_page_png`）、`olymp-regress.sh` 基准图与 PDF 导出（`typst-pdf`），不给 CLI 引入 `wgpu` 依赖。
- 不在「设置」页面新增硬件加速或渲染引擎切换开关。
- 不在缩放/平移时重复遍历 Typst `Frame`。
- 不在 6H 阶段重写或并行化 `typst::compile` 排版编译本身。
