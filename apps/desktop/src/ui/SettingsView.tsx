// Grouped settings view: left category rail (workspace, appearance, shortcuts, about)
// paired with rounded cards on the right.

import {
  Animated,
  cleanup,
  Clipboard,
  createAnimatedValue,
  effect,
  openUrl,
  reduceMotion,
  Scroll,
  signal,
  Text,
  View,
  type CompositeAnimation,
  type JSX,
} from "nsh";
import { Progress, Switch } from "nsh/base";
import {
  checkForUpdate,
  downloadUpdate,
  installAndRestart,
  installOnQuit,
  shellInfo,
  type AppUpdate,
  type UpdateProgress,
} from "nsh/updater";
import { Preview, type AboutInfoJs } from "xcpc-workstation/native";
import { BrandIcon } from "./BrandIcon";
import { ErrorDisplay } from "./ErrorDisplay";
import { Icon, type IconName } from "./Icon";
import { Btn, DelayedSpinner, IconBtn, Segmented, tip } from "./kit";
import { Section, SectionRow } from "./layout";
import { abbreviateHome, pathBase } from "./logic";
import { colorTransition, EASE_EXPO_CURVE, indicatorTransition } from "./motion";
import { ShortcutsList } from "./ShortcutsModal";
import {
  brandReplayOpen,
  errorOf,
  info,
  openError,
  openRecent,
  pickWorkspace,
  preview,
  requestNewWorkspace,
  resetEditorTypography,
  revision,
  settingsSection,
  updateAppearanceMode,
  updateEditorFontSize,
  updateEditorLigatures,
  updateEditorLineHeight,
  updateEditorVimMode,
  workspaceVersion,
  type SettingsSection,
  type UiError,
} from "./store";
import {
  appearanceMode,
  cardShadow,
  DEFAULT_EDITOR_FONT_SIZE,
  DEFAULT_EDITOR_LIGATURES,
  DEFAULT_EDITOR_LINE_HEIGHT,
  EASE_STANDARD,
  editorFontSize,
  editorLigatures,
  editorLineHeight,
  editorVimMode,
  MONO,
  ms,
  type AppearanceMode,
} from "./theme";

const NAV_ITEMS: readonly { id: SettingsSection; label: string; icon: IconName }[] = [
  { id: "workspace", label: "工作区", icon: "folder" },
  { id: "appearance", label: "外观", icon: "palette" },
  { id: "shortcuts", label: "快捷键", icon: "code" },
  { id: "about", label: "关于", icon: "info" },
];

const NAV_ITEM_HEIGHT = 34;
const NAV_ITEM_GAP = 4;

const APPEARANCE_OPTIONS: readonly { value: AppearanceMode; label: string }[] = [
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
  { value: "system", label: "跟随系统" },
];

const MIN_FONT_SIZE = 12;
const MAX_FONT_SIZE = 18;
const FONT_SIZE_STEP = 0.5;
const MIN_LINE_HEIGHT = 1.4;
const MAX_LINE_HEIGHT = 1.8;
const LINE_HEIGHT_STEP = 0.05;

export function readAboutInfo(): AboutInfoJs {
  try {
    return Preview.aboutInfo();
  } catch {
    return {
      version: "0.1.0",
      commit: "unknown",
      buildDate: "unknown",
      isDev: true,
      os: "unknown",
      arch: "unknown",
      fontSource: "unknown",
    };
  }
}

function formatVersion(version: string): string {
  return version.startsWith("v") ? version : `v${version}`;
}

function shortCommitOf(commit: string): string {
  const trimmed = commit.trim();
  if (!trimmed || trimmed.toLowerCase() === "unknown") return "";
  return trimmed.slice(0, 7);
}

/** Unified app version formatter shared by About page and Brand page footer (§2.7). */
export function formatAppVersion(about: Pick<AboutInfoJs, "version" | "commit" | "isDev">): string {
  const base = formatVersion(about.version);
  if (!about.isDev) return base;
  const shortCommit = shortCommitOf(about.commit);
  return shortCommit ? `${base}-dev (${shortCommit})` : `${base}-dev`;
}

function formatDiagnostics(about: AboutInfoJs): string {
  const commit = shortCommitOf(about.commit) || "unknown";
  return [
    `XCPC Workstation ${formatAppVersion(about)}`,
    `Commit: ${commit}`,
    `Build Date: ${about.buildDate}`,
    `Platform: ${about.os} (${about.arch})`,
    `Fonts: ${about.fontSource}`,
  ].join("\n");
}

function Card(props: { children?: JSX.Element }) {
  return (
    <View style={{ borderRadius: 10, borderWidth: 1, overflow: "hidden", boxShadow: cardShadow() }} class="bg-card border-border">
      {props.children}
    </View>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(1)} KB`;
  return `${(kb / 1024).toFixed(1)} MB`;
}

function isDevOrUnsupportedUpdateError(err: unknown): boolean {
  const text = String(err ?? "").toLowerCase();
  return (
    text.includes("unavailable") ||
    text.includes("unsupported") ||
    text.includes("not supported") ||
    text.includes("no updater") ||
    text.includes("err_feature_unavailable")
  );
}

function isMissingOrPrivateUpdateChannelError(err: unknown): boolean {
  const text = String(err ?? "").toLowerCase();
  return (
    text.includes("404") ||
    text.includes("not found") ||
    text.includes("401") ||
    text.includes("403") ||
    text.includes("unauthorized") ||
    text.includes("forbidden") ||
    text.includes("private") ||
    text.includes("no release") ||
    text.includes("no manifest") ||
    (text.includes("manifest") && (text.includes("missing") || text.includes("not found") || text.includes("404")))
  );
}

function isDevShell(): boolean {
  try {
    return shellInfo().version == null;
  } catch {
    return true;
  }
}

/** A folder: its name over its path, with the full path on hover. */
function Folder(props: { path: string; note?: string }) {
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }} tooltip={tip(props.path)}>
      <Icon name="folder" size={18} />
      <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
          <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1, fontSize: 13.5, lineHeight: 18, fontWeight: 500 }}>
            {pathBase(props.path)}
          </Text>
          {props.note && (
            <Text style={{ fontSize: 12 }} class="text-muted-foreground">
              {props.note}
            </Text>
          )}
        </View>
        <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: 12, lineHeight: 16, fontFamily: MONO }} class="text-muted-foreground">
          {abbreviateHome(props.path)}
        </Text>
      </View>
    </View>
  );
}

/** Numeric stepper for editor font size and line height. */
function Stepper(props: {
  valueLabel: string;
  canDec: boolean;
  canInc: boolean;
  onDec: () => void;
  onInc: () => void;
  testIDPrefix?: string;
}) {
  return (
    <View
      style={{
        height: 28,
        borderRadius: 7,
        borderWidth: 1,
        flexDirection: "row",
        alignItems: "center",
        paddingLeft: 2,
        paddingRight: 2,
        gap: 2,
      }}
      class="border-border bg-muted/50"
    >
      <IconBtn
        testID={props.testIDPrefix ? `${props.testIDPrefix}-dec` : undefined}
        icon="minus"
        size={22}
        iconSize={13}
        disabled={!props.canDec}
        onPress={props.onDec}
      />
      <Text
        style={{
          minWidth: 54,
          textAlign: "center",
          fontSize: 12.5,
          lineHeight: 16,
          fontVariant: ["tabular-nums"],
        }}
        class="text-foreground"
      >
        {props.valueLabel}
      </Text>
      <IconBtn
        testID={props.testIDPrefix ? `${props.testIDPrefix}-inc` : undefined}
        icon="plus"
        size={22}
        iconSize={13}
        disabled={!props.canInc}
        onPress={props.onInc}
      />
    </View>
  );
}

function WorkspacePane() {
  const copied = signal(false);
  const showJson = signal(false);
  const revealError = signal<UiError | undefined>(undefined);

  const snippet = () => {
    void revision();
    return info().root ? preview.mcpConfigSnippet() : "";
  };
  const recent = () => {
    void info();
    return preview.recentWorkspaces().filter((p) => p !== info().root).slice(0, 5);
  };

  let copyTimer: number | undefined;
  cleanup(() => clearTimeout(copyTimer));

  effect(() => {
    void workspaceVersion();
    revealError.set(undefined);
  });

  const copy = () => {
    void Clipboard.writeText(snippet()).then(() => {
      copied.set(true);
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => copied.set(false), 1500);
    });
  };
  const reveal = async () => {
    const res = await preview.revealInFolder(info().root);
    revealError.set(res.ok ? undefined : errorOf(res));
  };

  const sectionError = () => openError() ?? revealError();
  const clearErrors = () => {
    openError.set(undefined);
    revealError.set(undefined);
  };

  return (
    <View style={{ gap: 24 }}>
      <Section title="当前工作区" description="题目和比赛都保存在这个文件夹里，可以用 Git 管理。">
        <SectionRow first trailing={<Btn label="在文件夹中显示" icon="folderOpen" onPress={() => void reveal()} />}>
          <Folder path={info().root} note={info().readOnly ? "只读" : undefined} />
        </SectionRow>
        <If when={recent().length > 0}>
          <View style={{ paddingLeft: 14, paddingTop: 10, paddingBottom: 2, borderTopWidth: 1 }} class="border-border">
            <Text style={{ fontSize: 12 }} class="text-muted-foreground">
              最近打开
            </Text>
          </View>
          <View style={{ padding: 4, paddingTop: 2 }}>
            <Each of={recent()} key={(p) => p}>
              {(path) => (
                <View
                  onPress={() => openRecent(path())}
                  class="hover:bg-muted active:bg-accent"
                  style={{
                    height: 48,
                    paddingLeft: 10,
                    paddingRight: 10,
                    borderRadius: 8,
                    flexDirection: "row",
                    alignItems: "center",
                    gap: 8,
                    ...colorTransition(120),
                  }}
                >
                  <View style={{ flex: 1, minWidth: 0 }}>
                    <Folder path={path()} />
                  </View>
                  <Icon name="arrowRight" size={15} tone="muted-foreground" />
                </View>
              )}
            </Each>
          </View>
        </If>
        <If when={sectionError()}>
          <View style={{ paddingLeft: 14, paddingRight: 14, paddingTop: 10, paddingBottom: 10, borderTopWidth: 1 }} class="border-border bg-muted/40">
            <ErrorDisplay error={sectionError()} onDismiss={clearErrors} />
          </View>
        </If>
        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "flex-end", gap: 8, padding: 12, borderTopWidth: 1 }} class="border-border bg-muted/40">
          <Btn label="打开已有文件夹…" icon="folderOpen" onPress={() => void pickWorkspace()} />
          <Btn label="新建工作区…" icon="folderPlus" onPress={() => requestNewWorkspace()} />
        </View>
      </Section>

      <Section
        title="AI 助手接入"
        description={
          info().readOnly
            ? "当前工作区以只读方式打开。如需限制 AI 助手仅读取，可在配置参数中加入 --read-only。"
            : "让 AI 助手通过 MCP 直接读写这个工作区里的题目和比赛。"
        }
      >
        <SectionRow
          first
          trailing={
            <Btn
              label={copied() ? "已复制" : "复制配置"}
              icon={copied() ? "check" : "copy"}
              variant="primary"
              onPress={copy}
              style={{ minWidth: 104 }}
            />
          }
        >
          <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
            <Icon name="bot" size={18} />
            <Text style={{ fontSize: 13, lineHeight: 18 }}>把这段配置粘贴到助手的 MCP 设置里</Text>
          </View>
        </SectionRow>
        <View
          onPress={() => showJson.set(!showJson())}
          class="hover:bg-muted active:bg-accent border-border"
          style={{
            height: 36,
            paddingLeft: 14,
            flexDirection: "row",
            alignItems: "center",
            gap: 6,
            borderTopWidth: 1,
            ...colorTransition(120),
          }}
        >
          <Icon
            name="chevronDown"
            size={14}
            tone="muted-foreground"
            style={{
              transform: showJson() ? "rotate(0deg)" : "rotate(-90deg)",
              transitionProperty: "transform",
              transitionDuration: ms(120),
              transitionTimingFunction: EASE_STANDARD,
            }}
          />
          <Text style={{ fontSize: 12.5 }} class="text-secondary-foreground">
            {showJson() ? "收起配置内容" : "查看配置内容"}
          </Text>
        </View>
        <If when={showJson()}>
          <View style={{ borderTopWidth: 1 }} class="border-border bg-muted/40">
            <Scroll horizontal style={{ maxWidth: "100%" }}>
              <View style={{ padding: 14 }}>
                <Text style={{ fontSize: 12, fontFamily: MONO, lineHeight: 18 }} class="text-secondary-foreground">
                  {snippet()}
                </Text>
              </View>
            </Scroll>
          </View>
        </If>
      </Section>
    </View>
  );
}

function AppearancePane() {
  const isDefaultTypography = () =>
    editorFontSize() === DEFAULT_EDITOR_FONT_SIZE &&
    editorLineHeight() === DEFAULT_EDITOR_LINE_HEIGHT &&
    editorLigatures() === DEFAULT_EDITOR_LIGATURES;

  const decFontSize = () => updateEditorFontSize(Math.max(MIN_FONT_SIZE, editorFontSize() - FONT_SIZE_STEP));
  const incFontSize = () => updateEditorFontSize(Math.min(MAX_FONT_SIZE, editorFontSize() + FONT_SIZE_STEP));
  const decLineHeight = () => updateEditorLineHeight(Math.max(MIN_LINE_HEIGHT, Number((editorLineHeight() - LINE_HEIGHT_STEP).toFixed(2))));
  const incLineHeight = () => updateEditorLineHeight(Math.min(MAX_LINE_HEIGHT, Number((editorLineHeight() + LINE_HEIGHT_STEP).toFixed(2))));

  return (
    <View style={{ gap: 24 }}>
      <Card>
        <SectionRow
          first
          trailing={
            <Segmented
              items={APPEARANCE_OPTIONS}
              value={appearanceMode()}
              onChange={(mode) => updateAppearanceMode(mode)}
              itemWidth={74}
            />
          }
        >
          <View style={{ gap: 2 }}>
            <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              主题模式
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              浅色、深色或跟随系统
            </Text>
          </View>
        </SectionRow>
        <SectionRow
          trailing={
            <Btn
              label="打开 theme.toml"
              icon="palette"
              onPress={() => void preview.openThemeFile()}
              testID="open-theme-file"
            />
          }
        >
          <View style={{ gap: 2 }}>
            <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              主题定制
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              支持全局与工作区 .xcpc/theme.toml 覆写配色、字体与圆角
            </Text>
          </View>
        </SectionRow>
      </Card>

      <Section
        title="编辑器排版"
        description="调整题面与题解源码编辑区的字号、行高与连字显示。"
        trailing={
          <If when={!isDefaultTypography()}>
            <Btn label="恢复默认" icon="rotateCcw" variant="ghost" onPress={resetEditorTypography} />
          </If>
        }
      >
        <SectionRow
          first
          trailing={
            <Stepper
              testIDPrefix="editor-font-size"
              valueLabel={`${editorFontSize().toFixed(1).replace(/\.0$/, "")} px`}
              canDec={editorFontSize() > MIN_FONT_SIZE}
              canInc={editorFontSize() < MAX_FONT_SIZE}
              onDec={decFontSize}
              onInc={incFontSize}
            />
          }
        >
          <View style={{ gap: 2 }}>
            <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              字体大小
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              范围 12 至 18 px
            </Text>
          </View>
        </SectionRow>

        <SectionRow
          trailing={
            <Stepper
              testIDPrefix="editor-line-height"
              valueLabel={editorLineHeight().toFixed(2)}
              canDec={editorLineHeight() > MIN_LINE_HEIGHT}
              canInc={editorLineHeight() < MAX_LINE_HEIGHT}
              onDec={decLineHeight}
              onInc={incLineHeight}
            />
          }
        >
          <View style={{ gap: 2 }}>
            <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              行高倍数
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              范围 1.40 至 1.80
            </Text>
          </View>
        </SectionRow>

        <SectionRow
          trailing={
            <Switch
              testID="editor-ligatures"
              checked={editorLigatures()}
              onChange={(e) => updateEditorLigatures(e.checked)}
              trackClass="w-8 h-[18px] rounded-full bg-input checked:bg-primary transition-colors duration-[120ms]"
              thumbClass="size-[14px] m-[2px] rounded-full bg-white shadow-sm checked:translate-x-[14px] transition-transform duration-[120ms]"
            />
          }
        >
          <View style={{ gap: 2 }}>
            <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              编程连字
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              合并显示箭头与比较运算符等连字字形
            </Text>
          </View>
        </SectionRow>

        <SectionRow
          trailing={
            <Switch
              testID="editor-vim-mode"
              checked={editorVimMode()}
              onChange={(e) => updateEditorVimMode(e.checked)}
              trackClass="w-8 h-[18px] rounded-full bg-input checked:bg-primary transition-colors duration-[120ms]"
              thumbClass="size-[14px] m-[2px] rounded-full bg-white shadow-sm checked:translate-x-[14px] transition-transform duration-[120ms]"
            />
          }
        >
          <View style={{ gap: 2 }}>
            <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              Vim 模式
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              支持 Normal / Insert / Visual、常用移动与文本对象（ci$/ci(/ci"）
            </Text>
          </View>
        </SectionRow>
      </Section>
    </View>
  );
}

type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "upToDate" }
  | { kind: "available"; update: AppUpdate }
  | { kind: "downloading"; update: AppUpdate; progress: UpdateProgress }
  | { kind: "ready"; update: AppUpdate }
  | { kind: "scheduled"; update: AppUpdate }
  | { kind: "error"; message: string };

function AboutPane() {
  const about = readAboutInfo();
  const devShell = isDevShell();
  const copiedDiag = signal(false);
  const updateState = signal<UpdateState>({ kind: "idle" });
  const installing = signal(false);

  let diagTimer: number | undefined;
  cleanup(() => clearTimeout(diagTimer));

  const copyDiagnostics = () => {
    void Clipboard.writeText(formatDiagnostics(about)).then(() => {
      copiedDiag.set(true);
      clearTimeout(diagTimer);
      diagTimer = setTimeout(() => copiedDiag.set(false), 1800);
    });
  };

  const handleCheckUpdate = async () => {
    if (devShell) return;
    updateState.set({ kind: "checking" });
    try {
      const found = await checkForUpdate({ locale: "zh-CN" });
      if (!found) {
        updateState.set({ kind: "upToDate" });
        return;
      }
      updateState.set({ kind: "available", update: found });
    } catch (err) {
      if (isDevOrUnsupportedUpdateError(err)) {
        updateState.set({ kind: "error", message: "当前构建未启用自动更新" });
      } else if (isMissingOrPrivateUpdateChannelError(err)) {
        updateState.set({ kind: "error", message: "暂无可用更新或更新通道尚未公开" });
      } else {
        updateState.set({ kind: "error", message: "检查更新失败，请检查网络后重试" });
      }
    }
  };

  const handleDownload = async (update: AppUpdate) => {
    if (!update.canInstall) {
      try {
        await openUrl(update.url);
      } catch {
        updateState.set({ kind: "error", message: "无法打开下载链接，请稍后重试" });
      }
      return;
    }
    updateState.set({ kind: "downloading", update, progress: { received: 0, total: null } });
    try {
      await downloadUpdate((progress) => {
        updateState.set({ kind: "downloading", update, progress });
      });
      updateState.set({ kind: "ready", update });
    } catch (err) {
      if (isDevOrUnsupportedUpdateError(err)) {
        updateState.set({ kind: "error", message: "当前构建未启用自动更新" });
      } else {
        updateState.set({ kind: "error", message: "下载更新失败，请检查网络后重试" });
      }
    }
  };

  const handleInstallNow = async () => {
    if (installing()) return;
    installing.set(true);
    try {
      await installAndRestart();
    } catch {
      installing.set(false);
      updateState.set({ kind: "error", message: "安装更新失败，请稍后重试" });
    }
  };

  const handleInstallOnQuit = async (update: AppUpdate) => {
    if (installing()) return;
    installing.set(true);
    try {
      await installOnQuit();
      installing.set(false);
      updateState.set({ kind: "scheduled", update });
    } catch {
      installing.set(false);
      updateState.set({ kind: "error", message: "准备退出时安装失败，请稍后重试" });
    }
  };

  const updateSummary = (): string | undefined => {
    if (devShell) return "开发构建不提供自动更新";
    const st = updateState();
    switch (st.kind) {
      case "idle":
        return undefined;
      case "checking":
        return "正在检查更新…";
      case "upToDate":
        return "已是最新版本";
      case "available":
        return `发现新版本 ${formatVersion(st.update.version)}`;
      case "downloading": {
        const { received, total } = st.progress;
        if (total && total > 0) {
          const pct = Math.min(100, Math.round((received / total) * 100));
          return `正在下载 ${formatVersion(st.update.version)}（${pct}%，${formatBytes(received)} / ${formatBytes(total)}）`;
        }
        return `正在下载 ${formatVersion(st.update.version)}（已接收 ${formatBytes(received)}）`;
      }
      case "ready":
        return `${formatVersion(st.update.version)} 已下载完成，可以安装`;
      case "scheduled":
        return `${formatVersion(st.update.version)} 已就绪，将在退出应用时安装`;
      case "error":
        return st.message;
    }
  };

  const downloadPercent = () => {
    const st = updateState();
    if (st.kind !== "downloading") return 0;
    const { received, total } = st.progress;
    if (!total || total <= 0) return 0;
    return Math.min(100, Math.max(0, (received / total) * 100));
  };

  const isDownloadIndeterminate = () => {
    const st = updateState();
    return st.kind === "downloading" && (!st.progress.total || st.progress.total <= 0);
  };

  return (
    <View style={{ gap: 24 }}>
      {/* Centered brand mark, wordmark, and version subtitle */}
      <View style={{ alignItems: "center", paddingTop: 12, paddingBottom: 4, gap: 12 }}>
        <BrandIcon testID="about-brand-icon" size={88} replayOnPress />
        <View style={{ alignItems: "center", gap: 4 }}>
          <View style={{ flexDirection: "row", alignItems: "baseline", gap: 6 }}>
            <Text style={{ fontSize: 22, lineHeight: 28, fontWeight: 700, letterSpacing: -0.35 }} class="text-foreground">
              XCPC
            </Text>
            <Text style={{ fontSize: 22, lineHeight: 28, fontWeight: 400, letterSpacing: -0.25 }} class="text-secondary-foreground">
              Workstation
            </Text>
          </View>
          <Text
            testID="about-version"
            style={{ fontSize: 13, lineHeight: 18, fontVariant: ["tabular-nums"] }}
            class="text-muted-foreground"
          >
            {formatAppVersion(about)}
          </Text>
        </View>
      </View>

      {/* Software update, first-launch replay, and diagnostics */}
      <Card>
        <SectionRow
          first
          trailing={
            devShell ? undefined : (
              <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                <If when={updateState().kind === "checking"}>
                  <DelayedSpinner active size={14} />
                </If>
                <If
                  when={
                    updateState().kind === "idle" ||
                    updateState().kind === "upToDate" ||
                    updateState().kind === "error"
                  }
                >
                  <Btn
                    testID="check-update-btn"
                    label={updateState().kind === "error" ? "重试" : "立即检查"}
                    onPress={() => void handleCheckUpdate()}
                  />
                </If>
                <If when={updateState().kind === "checking"}>
                  <Btn label="检查中…" disabled />
                </If>
                <If when={updateState().kind === "available"}>
                  <Btn
                    variant="primary"
                    label={(updateState() as { kind: "available"; update: AppUpdate }).update.canInstall ? "下载更新" : "前往下载"}
                    onPress={() => {
                      const st = updateState();
                      if (st.kind === "available") void handleDownload(st.update);
                    }}
                  />
                </If>
                <If when={updateState().kind === "ready" || updateState().kind === "scheduled"}>
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                    <If when={updateState().kind === "ready"}>
                      <Btn
                        label="退出时安装"
                        disabled={installing()}
                        onPress={() => {
                          const st = updateState();
                          if (st.kind === "ready") void handleInstallOnQuit(st.update);
                        }}
                      />
                    </If>
                    <Btn
                      variant="primary"
                      label="立即重启安装"
                      disabled={installing()}
                      onPress={() => void handleInstallNow()}
                    />
                  </View>
                </If>
              </View>
            )
          }
        >
          <View style={{ gap: 2 }}>
            <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              检查更新
            </Text>
            <If when={Boolean(updateSummary())}>
              <Text style={{ fontSize: 12, lineHeight: 16, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
                {updateSummary() ?? ""}
              </Text>
            </If>
          </View>
        </SectionRow>

        <If when={updateState().kind === "downloading"}>
          <View style={{ paddingLeft: 14, paddingRight: 14, paddingBottom: 14, gap: 6 }}>
            <Progress
              value={downloadPercent()}
              indeterminate={isDownloadIndeterminate()}
              trackClass="h-1.5 rounded-full bg-muted overflow-hidden"
              indicatorClass="h-full rounded-full bg-primary transition-all duration-[120ms]"
            />
          </View>
        </If>

        <If when={updateState().kind === "available" && Boolean((updateState() as { kind: "available"; update: AppUpdate }).update.notes)}>
          <View style={{ paddingLeft: 14, paddingRight: 14, paddingTop: 10, paddingBottom: 12, borderTopWidth: 1 }} class="border-border bg-muted/40">
            <Text style={{ fontSize: 12, lineHeight: 18 }} class="text-secondary-foreground">
              {(updateState() as { kind: "available"; update: AppUpdate }).update.notes ?? ""}
            </Text>
          </View>
        </If>

        <SectionRow
          trailing={
            <Btn
              testID="replay-brand-btn"
              label="播放"
              onPress={() => brandReplayOpen.set(true)}
            />
          }
        >
          <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
            重播首次引导
          </Text>
        </SectionRow>

        <SectionRow
          trailing={
            <Btn
              testID="about-copy-diagnostics"
              label={copiedDiag() ? "已复制" : "复制诊断信息"}
              icon={copiedDiag() ? "check" : "copy"}
              onPress={copyDiagnostics}
              style={{ minWidth: 124 }}
            />
          }
        >
          <View style={{ gap: 2 }}>
            <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              诊断信息
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              用于反馈问题时附带环境信息
            </Text>
          </View>
        </SectionRow>
      </Card>

      {/* Credits & licenses */}
      <Section title="致谢与许可">
        <SectionRow first>
          <View style={{ gap: 4 }}>
            <Text style={{ fontSize: 13, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              开源致谢
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 18 }} class="text-secondary-foreground">
              olymp.sty（GassaFM/olymp.sty）、fuzzy-cnoi-statement（Wallbreaker5th）、Typst、NSH、MiTeX、CeTZ（LGPL-3.0）、oxifmt、Pinit
            </Text>
          </View>
        </SectionRow>
        <SectionRow>
          <View style={{ gap: 6 }}>
            <Text style={{ fontSize: 13, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              字体许可
            </Text>
            <View style={{ gap: 3 }}>
              <Text style={{ fontSize: 12, lineHeight: 18 }} class="text-secondary-foreground">
                Latin Modern（GUST Font License）
              </Text>
              <Text style={{ fontSize: 12, lineHeight: 18 }} class="text-secondary-foreground">
                JetBrains Mono（SIL Open Font License, OFL）
              </Text>
            </View>
          </View>
        </SectionRow>
        <SectionRow>
          <View style={{ gap: 4 }}>
            <Text style={{ fontSize: 13, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              许可证
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 18 }} class="text-secondary-foreground">
              Apache-2.0 License
            </Text>
          </View>
        </SectionRow>
      </Section>

      {/* Quiet dedication footer */}
      <View style={{ alignItems: "center", paddingTop: 4, paddingBottom: 12 }}>
        <Text style={{ fontSize: 12, lineHeight: 18 }} class="text-muted-foreground">
          献给每一个和 WA 较劲到深夜的人
        </Text>
      </View>
    </View>
  );
}

export function SettingsView() {
  const active = () => settingsSection();
  const activeIndex = () => Math.max(0, NAV_ITEMS.findIndex((it) => it.id === active()));

  const paneOpacity = createAnimatedValue(1);
  const paneY = createAnimatedValue(0);
  let firstRender = true;
  let running: CompositeAnimation | undefined;

  effect(() => {
    void active();
    if (firstRender) {
      firstRender = false;
      return;
    }
    if (running) {
      running.stop();
      running = undefined;
    }
    if (reduceMotion()) {
      paneOpacity.set(1);
      paneY.set(0);
      return;
    }
    paneOpacity.set(0);
    paneY.set(6);
    const anim = Animated.parallel([
      Animated.timing(paneOpacity, { toValue: 1, duration: 180, easing: EASE_EXPO_CURVE }),
      Animated.timing(paneY, { toValue: 0, duration: 200, easing: EASE_EXPO_CURVE }),
    ]);
    running = anim;
    void anim.start(() => {
      if (running === anim) running = undefined;
    });
  });

  cleanup(() => {
    running?.stop();
    running = undefined;
  });

  return (
    <View style={{ flex: 1, flexDirection: "row" }} class="bg-background">
      {/* Left section rail */}
      <View
        style={{
          width: 196,
          borderRightWidth: 1,
          paddingTop: 20,
          paddingBottom: 20,
          paddingLeft: 12,
          paddingRight: 12,
          flexShrink: 0,
        }}
        class="border-border bg-background"
      >
        <View style={{ gap: NAV_ITEM_GAP }}>
          <View
            class="bg-accent"
            style={{
              position: "absolute",
              left: 0,
              right: 0,
              top: 0,
              height: NAV_ITEM_HEIGHT,
              borderRadius: 7,
              transform: `translateY(${activeIndex() * (NAV_ITEM_HEIGHT + NAV_ITEM_GAP)}px)`,
              ...indicatorTransition(),
            }}
          />
          <Each of={NAV_ITEMS} key={(it) => it.id}>
            {(item) => {
              const selected = () => active() === item().id;
              return (
                <View
                  testID={`settings-nav-${item().id}`}
                  onPress={() => settingsSection.set(item().id)}
                  class={selected() ? "" : "hover:bg-muted active:bg-accent"}
                  style={{
                    height: NAV_ITEM_HEIGHT,
                    paddingLeft: 10,
                    paddingRight: 10,
                    borderRadius: 7,
                    flexDirection: "row",
                    alignItems: "center",
                    gap: 9,
                    ...colorTransition(120),
                  }}
                >
                  <Icon
                    name={item().icon}
                    size={15}
                    tone={selected() ? "foreground" : "muted-foreground"}
                  />
                  <Text
                    style={{
                      fontSize: 13,
                      lineHeight: 18,
                      fontWeight: selected() ? 500 : 400,
                    }}
                    class={selected() ? "text-foreground" : "text-secondary-foreground"}
                  >
                    {item().label}
                  </Text>
                </View>
              );
            }}
          </Each>
        </View>
      </View>

      {/* Right detail pane */}
      <Scroll style={{ flex: 1 }} class="bg-background">
        <View
          style={{
            width: 640,
            maxWidth: "100%",
            alignSelf: "center",
            paddingTop: 28,
            paddingBottom: 36,
            paddingLeft: 28,
            paddingRight: 28,
            opacity: paneOpacity,
            transform: [{ translateY: paneY }],
          }}
        >
          <If when={active() === "workspace"}>
            <WorkspacePane />
          </If>
          <If when={active() === "appearance"}>
            <AppearancePane />
          </If>
          <If when={active() === "shortcuts"}>
            <ShortcutsList />
          </If>
          <If when={active() === "about"}>
            <AboutPane />
          </If>
        </View>
      </Scroll>
    </View>
  );
}
