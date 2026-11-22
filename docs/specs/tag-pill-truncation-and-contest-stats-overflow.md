# Spec：长名称标签截断异常与比赛页侧栏统计区溢出修复

> 状态：已定位根因，待实现（2026-11-22）。  
> 关联文档：[design-system.md](../design-system.md)。

---

## 1. 现象与影响

桌面端在展示较长的标签/比赛名称或较多比赛标签时，存在两处视觉与布局缺陷：

1. **长名称标签右侧出现大片空白**：在题目详情页（`ProblemInspector`）头部，归属比赛标签（带奖杯图标的 pill，例如 `2026 CCF 非专业级软件能力认证（示例）`）文字在约 `140pt` 处被正确截断并显示 `…`，但外层灰色胶囊本身仍被撑得很宽，文字右侧留下大片空白背景。
2. **比赛编辑页左侧栏底部标签溢出**：在比赛 Studio 左侧大纲栏（`Outline`，宽 `232pt`）底部的统计区（`ContestStats` `compact` 模式），难度条与标签组之间出现明显大于 `gap: 10` 的异常空白，且多行换行后的最后一行标签（如第 3 行 `implementation 1`）向下冲破底部 `padding: 12` 并贴近/溢出窗口底边。

---

## 2. 问题一：长名称标签截断后右侧留白

### 2.1 涉及代码

- `apps/desktop/src/ui/LibraryView.tsx`（L629–654，`ProblemInspector` 归属比赛 pill）：
  ```tsx
  <View
    onPress={() => jumpContest(c().id)}
    class="bg-muted hover:bg-accent ..."
    style={{
      height: 20,
      paddingLeft: 6,
      paddingRight: 6,
      borderRadius: 5,
      flexDirection: "row",
      alignItems: "center",
      gap: 4,
      // 缺失：maxWidth / minWidth: 0 / overflow: "hidden"
    }}
  >
    <Icon name="trophy" size={11} tone="muted-foreground" />
    <Text
      style={{ fontSize: 11.5, numberOfLines: 1, textOverflow: "ellipsis", maxWidth: 140 }}
      class="text-secondary-foreground"
    >
      {c().name}
    </Text>
  </View>
  ```
- 同类未做截断约束的标签组件：
  - `apps/desktop/src/ui/kit.tsx`（L217–244，`Chip`）：外层 `<View>` 无 `maxWidth` / `overflow`，内层 `<Text>` 无 `numberOfLines` / `textOverflow` / `flexShrink`。
  - `apps/desktop/src/ui/LibraryView.tsx`（L619–628，`sources` pill）：与归属比赛 pill 同处 `DetailHeader` 的 `second` 区域，同样无 `maxWidth` / 截断。

### 2.2 根因分析

NSH 0.6.0 底层采用 **Taffy 0.15** 布局引擎与 **Pango** 文本绘制，且 Flexbox 默认 **`flexShrink: 0`**（见 `apps/desktop/.nsh/types/nsh/types.d.ts` L261：`nothing shrinks unless flexShrink says so`）。`TextEngine` 将文本测量与绘制严格分为两阶段：

1. **外层容器测量阶段**：外层胶囊 `<View>` 处于 `DetailHeader` 的 `flexWrap: "wrap"` 行内且自身未声明 `maxWidth`。Taffy 计算该胶囊的自然宽度时，以无宽度限制（`MaxContent`）调用 `TextEngine::measure(prepared, None)`，得到整串未截断中文标题的完整单行宽度，并将外层 `<View>` 撑大到 `paddingLeft + Icon(11) + gap(4) + 完整文本宽 + paddingRight`。
2. **内层文本结算与绘制阶段**：Taffy 结算 `<Text>` 自身盒尺寸时，应用了 `<Text>` 上的 `maxWidth: 140`，将其盒宽 clamp 为 `140pt`；随后 `TextEngine::layout` 交由 Pango 在 `140pt` 内截断并绘制省略号 `…`。

因此，`<Text>` 自身只占 `140pt` 且带省略号，但外层胶囊 `<View>` 已按未截断文本的完整宽度撑开，造成右侧大片留白。

仓库内正常工作的截断写法（`apps/desktop/App.tsx` L211–235 的 `workspace-switcher`、`apps/desktop/src/ui/LibraryView.tsx` L57–79 的 `FilterMenu`）均遵循相反的约束归属：**`maxWidth` 放在外层 `<View>`，内层 `<Text>` 不写 `maxWidth`，改为 `flexShrink: 1, numberOfLines: 1, textOverflow: "ellipsis"`**。

### 2.3 解决方案

1. **修复 `LibraryView.tsx` 归属比赛 pill（L629–654）**：
   - 将 `maxWidth` 移至外层 `<View>`（例如 `maxWidth: 168`，约为原 `140pt` 文本 + 图标 + 内边距的视觉宽度），并补充 `minWidth: 0, overflow: "hidden"`。
   - 内层 `<Text>` 移除 `maxWidth: 140`，改为 `flexShrink: 1, minWidth: 0, numberOfLines: 1, textOverflow: "ellipsis"`。
   - 效果：短名称时胶囊紧贴文本宽度收缩；长名称时外层胶囊停在 `maxWidth`，内层 `<Text>` 精确填满图标右侧剩余空间并截断，无多余留白。
2. **统一加固 `Chip` 与 `sources` pill（防御窄容器横向溢出）**：
   - `apps/desktop/src/ui/kit.tsx`（`Chip`）：外层 `<View>` 增加 `maxWidth: "100%", minWidth: 0, overflow: "hidden"`（或支持可选 `maxWidth` prop，默认 `"100%"`），内层 `<Text>` 增加 `flexShrink: 1, minWidth: 0, numberOfLines: 1, textOverflow: "ellipsis"`；若发生截断可配合原有或按需 `tooltip` 展示完整名称。
   - `apps/desktop/src/ui/LibraryView.tsx`（`sources` pill，L619–628）：按与归属比赛 pill 相同的模式在外层 `<View>` 加 `maxWidth`（如 `180`）与 `minWidth: 0, overflow: "hidden"`，内层 `<Text>` 加 `flexShrink: 1, minWidth: 0, numberOfLines: 1, textOverflow: "ellipsis"`。

---

## 3. 问题二：比赛编辑页左侧栏底部标签溢出

### 3.1 涉及代码

- `apps/desktop/src/ui/studio/Outline.tsx`（L60–120）：
  ```tsx
  <View style={{ width: OUTLINE_WIDTH, borderRightWidth: 1 }} class="border-border bg-background">
    <Scroll style={{ flex: 1 }}>
      {/* 封面与 A..Z 题目列表 */}
    </Scroll>
    <If when={props.detail.items.length > 0}>
      <View style={{ padding: 12, borderTopWidth: 1 }} class="border-border">
        <ContestStats id={props.detail.id} compact />
      </View>
    </If>
  </View>
  ```
- `apps/desktop/src/ui/Contest.tsx`（L31–86，`ContestStats`）：
  ```tsx
  const bar = (
    <View style={{ gap: 8, flex: 1 }}>
      ...
    </View>
  );
  const tags = (
    <View style={{ flex: 1, flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
      <Each of={stats().tagCounts.slice(0, props.compact ? 8 : 24)} key={(t) => t.tag}>
        {(t) => <Chip label={`${t().tag} ${t().count}`} />}
      </Each>
    </View>
  );
  if (props.compact) {
    return (
      <View style={{ gap: 10 }}>
        {bar}
        <If when={stats().tagCounts.length > 0}>{tags}</If>
      </View>
    );
  }
  return (
    <View style={{ gap: 14 }}>
      <View style={{ flexDirection: "row", gap: 12 }}>{label("难度分布")}{bar}</View>
      <If when={stats().tagCounts.length > 0}>
        <View style={{ flexDirection: "row", gap: 12 }}>{label("标签")}{tags}</View>
      </If>
    </View>
  );
  ```

### 3.2 根因分析

1. **核心布局缺陷：`compact` 模式在纵向 `auto` 高度容器中复用了 `flex: 1`**：
   - 在非 `compact` 模式（`ContestsView.tsx` L396 的比赛详情页「概况」），`bar` 和 `tags` 位于**横向** `<View style={{ flexDirection: "row", gap: 12 }}>` 内，`flex: 1` 作用于水平主轴，用于占据左侧 `56pt` 标题右侧的剩余宽度。
   - 在 `compact` 模式（`Outline.tsx` L117），父容器 `<View style={{ gap: 10 }}>` 默认是**纵向（`column`）且高度为 `auto`**。NSH 中 `flex: 1` 展开为 `flexGrow: 1, flexShrink: 1, flexBasis: 0`——作用到纵向主轴后，`bar` 和多行换行的 `tags` 的垂直 `flexBasis` 变为 `0`：
     - 纵向高度汇总与剩余空间分配失真，`bar` 与 `tags` 之间出现明显大于 `10pt` 的异常空白；
     - `tags` 实际换行后的总高度（3 行）大于其分到的盒高，最后一行直接向下溢出底部 `padding: 12` 甚至窗口底边。
2. **缺少边界防御**：
   - `Outline.tsx` L62 的外层侧栏 `<View>` 和 L116 的统计区容器 `<View>` 均未设 `overflow: "hidden"`（默认 `"visible"`）。
   - `Contest.tsx` L59 的 `tags` 容器未设 `minWidth: 0`，在 `232pt` 边栏（净宽 `208pt`）下若出现单个超长标签，会横向撑破容器。

### 3.3 解决方案

1. **分离 `ContestStats` 横向与纵向模式的 `flex: 1`（`apps/desktop/src/ui/Contest.tsx`）**：
   - `bar` 仅在 `!props.compact` 时使用 `flex: 1, minWidth: 0`；`compact` 模式下不设 `flex: 1`（按内容自然撑开高度）。
   - `tags` 仅在 `!props.compact` 时使用 `flex: 1, minWidth: 0`；`compact` 模式下改为 `{ flexDirection: "row", flexWrap: "wrap", gap: 4, minWidth: 0 }`，让多行标签的真实总高度完整计入底部统计区，由上方的 `<Scroll style={{ flex: 1 }}>` 自动让出高度，不再向下溢出，同时消除 `bar` 与 `tags` 之间的异常空白。
2. **补充容器裁剪与单行上限防御（`apps/desktop/src/ui/studio/Outline.tsx`）**：
   - 左侧栏外层 `<View>`（L62）与底部统计区容器 `<View>`（L116）补充 `overflow: "hidden"`，避免极端情况下内容绘出边界。
   - 配合问题一中对 `<Chip>` 的 `maxWidth: "100%"` + 省略截断加固，确保单个超长标签在 `208pt` 净宽下也不会横向溢出。

---

## 4. 实施清单与验证

### 拟修改文件

| 文件 | 改动摘要 |
| :--- | :--- |
| `apps/desktop/src/ui/LibraryView.tsx` | 归属比赛 pill 与 `sources` pill：`maxWidth` 移至外层 `<View>`（加 `minWidth: 0, overflow: "hidden"`），内层 `<Text>` 改为 `flexShrink: 1, minWidth: 0, numberOfLines: 1, textOverflow: "ellipsis"` |
| `apps/desktop/src/ui/kit.tsx` | `Chip` 外层加 `maxWidth: "100%", minWidth: 0, overflow: "hidden"`，内层 `<Text>` 加 `flexShrink: 1, minWidth: 0, numberOfLines: 1, textOverflow: "ellipsis"` |
| `apps/desktop/src/ui/Contest.tsx` | `ContestStats` 仅在 `!props.compact` 时对 `bar` / `tags` 施加 `flex: 1`，并补 `minWidth: 0` |
| `apps/desktop/src/ui/studio/Outline.tsx` | 侧栏及底部统计区外层 `<View>` 补 `overflow: "hidden"` |

### 验收标准

1. **静态与自动化检查**：在开发环境运行 `nsh check` 与 `nsh test` 全部通过。
2. **长名称标签视觉验收**：打开题目详情页，包含长比赛名（如 `2026 CCF 非专业级软件能力认证（示例）`）的归属比赛 pill 截断后胶囊右边界紧贴 `…`（保留 `6pt` 右内边距），无多余空白；短名称 pill（如 `XCPC Workstation 示例赛`）宽度紧贴文字。
3. **比赛页侧栏统计区视觉验收**：打开含 8 个及以上标签的比赛 Studio，左侧栏底部难度条与标签区保持精确 `10pt` 间距，所有标签行完整落在底部 `12pt` 内边距之内、不贴底不溢出；同时验证比赛详情页（非 `compact` 模式）的「概况」区块横向对齐不受影响。
