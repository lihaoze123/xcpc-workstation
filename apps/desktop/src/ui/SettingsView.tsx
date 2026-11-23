// Grouped settings view: left category rail (workspace, appearance, shortcuts, about)
// paired with rounded cards on the right.

import {
  Animated,
  cleanup,
  Clipboard,
  colorScheme,
  computed,
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
import { Input, Progress, Switch } from "nsh/base";
import {
  checkForUpdate,
  downloadUpdate,
  installAndRestart,
  installOnQuit,
  shellInfo,
  type AppUpdate,
  type UpdateProgress,
} from "nsh/updater";
import { Preview, type AboutInfoJs, type ToolchainStatusJs } from "xcpc-workstation/native";
import { BrandIcon } from "./BrandIcon";
import { ErrorDisplay } from "./ErrorDisplay";
import { languagePref, locale, resolveLocalized, t, type LanguagePref, type MessageKey } from "./i18n";
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
  refresh,
  requestNewWorkspace,
  resetEditorTypography,
  revision,
  settingsSection,
  updateAppearanceMode,
  updateEditorFontSize,
  updateEditorLigatures,
  updateEditorLineHeight,
  updateEditorVimMode,
  updateLanguagePref,
  workspaceVersion,
  type SettingsSection,
  type UiError,
} from "./store";
import { encodeSingleOptionPatch, loadTemplatesCatalog, type TemplateOptionMap } from "./templates";
import { ForkTemplateModal, InstallPresetModal, TemplateOptionsEditor, TemplatePicker } from "./TemplateOptionsEditor";
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
import {
  dedupDetectedCompilers,
  hasInvalidCxxOverride,
  normalizeCxxOverrideInput,
  verdictBadgeStyle,
} from "./studio/buildLogic";
import {
  formatUpdateSummary,
  parseUpdateNotes,
  resolveReleasePageUrl,
  updateAvailableActionLabel,
  visibleUpdateNotes,
  type InlineSpan,
  type UpdateState,
} from "./updateNotes";

const NAV_ITEMS: readonly { id: SettingsSection; labelKey: MessageKey; icon: IconName }[] = [
  { id: "workspace", labelKey: "settings.nav.workspace", icon: "folder" },
  { id: "layout", labelKey: "settings.nav.layout", icon: "fileText" },
  { id: "toolchain", labelKey: "settings.nav.toolchain", icon: "squareCode" },
  { id: "appearance", labelKey: "settings.nav.appearance", icon: "palette" },
  { id: "shortcuts", labelKey: "settings.nav.shortcuts", icon: "code" },
  { id: "about", labelKey: "settings.nav.about", icon: "info" },
];

const NAV_ITEM_HEIGHT = 34;
const NAV_ITEM_GAP = 4;

const APPEARANCE_OPTIONS: readonly { value: AppearanceMode; labelKey: MessageKey }[] = [
  { value: "light", labelKey: "settings.appearance.light" },
  { value: "dark", labelKey: "settings.appearance.dark" },
  { value: "system", labelKey: "settings.appearance.system" },
];

const LANGUAGE_OPTIONS: readonly { value: LanguagePref; label: () => string }[] = [
  { value: "system", label: () => t("settings.appearance.system") },
  { value: "zh-CN", label: () => "简体中文" },
  { value: "en", label: () => "English" },
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
      <Section title={t("settings.workspace.title")} description={t("settings.workspace.desc")}>
        <SectionRow first trailing={<Btn label={t("common.revealInFolder")} icon="folderOpen" onPress={() => void reveal()} />}>
          <Folder path={info().root} note={info().readOnly ? t("common.readOnly") : undefined} />
        </SectionRow>
        <If when={recent().length > 0}>
          <View style={{ paddingLeft: 14, paddingTop: 10, paddingBottom: 2, borderTopWidth: 1 }} class="border-border">
            <Text style={{ fontSize: 12 }} class="text-muted-foreground">
              {t("settings.workspace.recent")}
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
          <Btn label={t("settings.workspace.openExisting")} icon="folderOpen" onPress={() => void pickWorkspace()} />
          <Btn label={t("settings.workspace.newWorkspace")} icon="folderPlus" onPress={() => requestNewWorkspace()} />
        </View>
      </Section>

      <Section
        title={t("settings.mcp.title")}
        description={
          info().readOnly
            ? t("settings.mcp.descReadOnly")
            : t("settings.mcp.desc")
        }
      >
        <SectionRow
          first
          trailing={
            <Btn
              label={copied() ? t("common.copied") : t("settings.mcp.copyConfig")}
              icon={copied() ? "check" : "copy"}
              variant="primary"
              onPress={copy}
              style={{ minWidth: 104 }}
            />
          }
        >
          <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
            <Icon name="bot" size={18} />
            <Text style={{ fontSize: 13, lineHeight: 18 }}>{t("settings.mcp.hint")}</Text>
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
            {showJson() ? t("settings.mcp.hideConfig") : t("settings.mcp.showConfig")}
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

function LayoutPane() {
  const err = signal<UiError | undefined>(undefined);
  const forkOpen = signal(false);
  const presetOpen = signal(false);

  const catalog = computed(() => {
    void revision();
    void workspaceVersion();
    return loadTemplatesCatalog();
  });

  const wsReadOnly = () => info().readOnly;
  const tplId = () => catalog().default || "olymp-classic";
  const tplInfo = computed(() => catalog().templates.find((t) => t.id === tplId()));
  const canInstallMissingPreset = () => !tplInfo() && catalog().presets.some((p) => p.id === tplId());
  const wsOpts = computed<TemplateOptionMap>(() => catalog().workspaceOptions[tplId()] ?? {});
  const hasBookFields = computed(() => (tplInfo()?.schema.fields ?? []).some((f) => f.scope === "book"));
  const hasCustomOpts = computed(() => Object.keys(wsOpts()).length > 0);

  const apply = (res: { ok: boolean; code?: string | null; error?: string | null }, ctx: string) => {
    err.set(errorOf(res, ctx));
    if (res.ok) refresh();
  };

  const resetAllOpts = () => {
    const keys = Object.keys(wsOpts());
    if (keys.length === 0) return;
    const clearMap: Record<string, null> = {};
    for (const k of keys) clearMap[k] = null;
    apply(
      preview.updateWorkspaceTemplate(null, false, JSON.stringify({ [tplId()]: clearMap })),
      t("ctx.restoreDefaultLayout"),
    );
  };

  return (
    <View style={{ gap: 24 }}>
      <Section
        title={t("settings.layout.defaultTemplateTitle")}
        description={t("settings.layout.defaultTemplateDesc")}
      >
        <View style={{ padding: 14, gap: 12 }}>
          <TemplatePicker
            templates={catalog().templates}
            value={tplId()}
            readOnly={wsReadOnly()}
            onChange={(nextId) => apply(preview.updateWorkspaceTemplate(nextId, false, null), t("ctx.switchDefaultTemplate"))}
            onAddFromPreset={!wsReadOnly() ? () => presetOpen.set(true) : undefined}
          />
          <If when={!tplInfo()}>
            <View class="border border-border bg-muted/40" style={{ borderRadius: 8, padding: 10, gap: 6 }}>
              <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                <Icon name="triangleAlert" size={14} tone="destructive" />
                <Text style={{ flex: 1, fontSize: 12.5, fontWeight: 500 }} class="text-foreground">
                  {t("settings.layout.missingTemplateTitle", { id: tplId() })}
                </Text>
              </View>
              <Text style={{ fontSize: 12, lineHeight: 16.5 }} class="text-secondary-foreground">
                {t("settings.layout.missingTemplateDesc")}
              </Text>
              <If when={!wsReadOnly()}>
                <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6, paddingTop: 2 }}>
                  <If
                    when={canInstallMissingPreset()}
                    else={
                      <Btn
                        variant="primary"
                        label={t("settings.layout.addFromPreset")}
                        icon="plus"
                        onPress={() => presetOpen.set(true)}
                      />
                    }
                  >
                    <Btn
                      variant="primary"
                      label={t("settings.layout.installFromPreset")}
                      icon="plus"
                      onPress={() => apply(preview.installPresetTemplate(tplId(), tplId()), t("ctx.installPresetTemplate"))}
                    />
                  </If>
                </View>
              </If>
            </View>
          </If>
          <If when={!wsReadOnly()}>
            <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 8, paddingTop: 2 }}>
              <Btn label={t("settings.layout.addFromPreset")} icon="plus" onPress={() => presetOpen.set(true)} />
              <If when={!!tplInfo()}>
                <Btn
                  label={t("settings.layout.editTemplate")}
                  icon="filePenLine"
                  tooltip={tip(t("settings.layout.editTemplateTooltip"))}
                  onPress={() => void preview.openTemplateFile(tplId()).then((res) => !res.ok && err.set(errorOf(res, t("ctx.openTemplateFile"))))}
                />
                <Btn
                  label={t("settings.layout.forkTemplate")}
                  icon="copy"
                  tooltip={tip(t("settings.layout.forkTemplateTooltip"))}
                  onPress={() => forkOpen.set(true)}
                />
              </If>
              <Btn
                label={t("settings.layout.preamble")}
                icon="code"
                tooltip={tip(t("settings.layout.preambleTooltip"))}
                onPress={() => void preview.openPreambleFile().then((res) => !res.ok && err.set(errorOf(res, t("ctx.openPreambleFile"))))}
              />
            </View>
          </If>
        </View>
      </Section>

      <If when={hasBookFields()}>
        <Section
          title={t("settings.layout.defaultParamsTitle")}
          description={t("settings.layout.defaultParamsDesc")}
          trailing={
            <If when={!wsReadOnly() && hasCustomOpts()}>
              <Btn label={t("common.restoreDefault")} icon="rotateCcw" variant="ghost" onPress={resetAllOpts} />
            </If>
          }
        >
          <View style={{ padding: 14 }}>
            <TemplateOptionsEditor
              ownerKey={`workspace:${info().root}:${tplId()}`}
              template={tplInfo()}
              scope="book"
              baseValues={{}}
              overrideValues={wsOpts()}
              fonts={catalog().fonts}
              readOnly={wsReadOnly()}
              restoreTooltip={t("common.restoreTemplateDefault")}
              onChange={(key, value) =>
                apply(
                  preview.updateWorkspaceTemplate(null, false, encodeSingleOptionPatch(tplId(), key, value)),
                  t("ctx.updateDefaultLayout"),
                )
              }
            />
          </View>
        </Section>
      </If>

      <If when={err()}>
        <ErrorDisplay error={err()} onDismiss={() => err.set(undefined)} />
      </If>

      <ForkTemplateModal
        open={forkOpen()}
        sourceId={tplId()}
        sourceName={tplInfo() ? resolveLocalized(tplInfo()!.name) || tplId() : tplId()}
        existingIds={catalog().templates.map((t) => t.id)}
        onCreated={(newId) => apply(preview.updateWorkspaceTemplate(newId, false, null), t("ctx.switchDefaultTemplate"))}
        onClose={() => forkOpen.set(false)}
      />

      <InstallPresetModal
        open={presetOpen()}
        presets={catalog().presets}
        existingIds={catalog().templates.map((t) => t.id)}
        initialTargetName={!tplInfo() ? tplId() : undefined}
        onInstalled={(installedId) => apply(preview.updateWorkspaceTemplate(installedId, false, null), t("ctx.switchDefaultTemplate"))}
        onClose={() => presetOpen.set(false)}
      />
    </View>
  );
}

function ToolchainPane() {
  const status = signal<ToolchainStatusJs>(preview.toolchainStatus());
  const draft = signal(status().overridePath ?? "");

  const redetect = () => {
    const next = preview.toolchainStatus();
    status.set(next);
    draft.set(next.overridePath ?? "");
  };

  const applyOverride = () => {
    const normalized = normalizeCxxOverrideInput(draft());
    const next = preview.setCxxOverride(normalized);
    status.set(next);
    draft.set(next.overridePath ?? "");
  };

  const clearOverride = () => {
    const next = preview.setCxxOverride(null);
    status.set(next);
    draft.set("");
  };

  const selectCompiler = (path: string) => {
    const next = preview.setCxxOverride(path);
    status.set(next);
    draft.set(next.overridePath ?? "");
  };

  const dark = () => colorScheme() === "dark";
  const invalidOverride = () => hasInvalidCxxOverride(status());
  const dirtyDraft = () => (normalizeCxxOverrideInput(draft()) ?? "") !== (status().overridePath ?? "");
  const compilers = computed(() => dedupDetectedCompilers(status().compilers, status().active));

  return (
    <View testID="settings-toolchain-pane" style={{ gap: 24 }}>
      <Section
        title={t("settings.toolchain.title")}
        description={t("settings.toolchain.desc")}
        trailing={
          <Btn
            testID="toolchain-redetect-btn"
            label={t("settings.toolchain.redetect")}
            icon="rotateCcw"
            onPress={redetect}
          />
        }
      >
        <SectionRow first>
          <If
            when={status().active}
            else={
              <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                <Icon name="triangleAlert" size={16} tone="destructive" />
                <Text testID="toolchain-not-found" style={{ fontSize: 13.5, fontWeight: 500 }} class="text-destructive">
                  {t("settings.toolchain.notFound")}
                </Text>
              </View>
            }
          >
            {(active) => {
              const badge = () => verdictBadgeStyle("AC", dark());
              return (
                <View style={{ gap: 4 }}>
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                    <Text testID="toolchain-active-name" style={{ fontSize: 13.5, fontWeight: 600, fontFamily: MONO }} class="text-foreground">
                      {active().name}
                    </Text>
                    <View
                      style={{
                        height: 20,
                        paddingLeft: 7,
                        paddingRight: 7,
                        borderRadius: 9999,
                        justifyContent: "center",
                        backgroundColor: badge().bg,
                      }}
                    >
                      <Text style={{ fontSize: 11, fontWeight: 600, color: badge().fg }}>
                        {t("settings.toolchain.active")}
                      </Text>
                    </View>
                  </View>
                  <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-secondary-foreground">
                    {active().version}
                  </Text>
                  <Text
                    tooltip={tip(active().path)}
                    style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: 11.5, lineHeight: 16, fontFamily: MONO }}
                    class="text-muted-foreground"
                  >
                    {active().path}
                  </Text>
                </View>
              );
            }}
          </If>
        </SectionRow>

        <SectionRow>
          <View style={{ gap: 8 }}>
            <View style={{ gap: 2 }}>
              <Text style={{ fontSize: 13, fontWeight: 500 }} class="text-foreground">
                {t("settings.toolchain.overrideTitle")}
              </Text>
              <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
                {t("settings.toolchain.overrideDesc")}
              </Text>
            </View>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
              <Input
                testID="toolchain-override-input"
                value={draft()}
                placeholder={t("settings.toolchain.overridePlaceholder")}
                autoCapitalize="none"
                autoCorrect={false}
                onChange={(e) => draft.set(e.value)}
                onSubmit={applyOverride}
                class="flex-1 h-8 px-2.5 rounded-md border border-border bg-card text-[12.5px] placeholder:text-muted-foreground focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring transition-colors"
                style={{ fontFamily: MONO }}
              />
              <Btn
                testID="toolchain-override-save"
                label={t("settings.toolchain.saveOverride")}
                variant="primary"
                disabled={!dirtyDraft()}
                onPress={applyOverride}
              />
              <If when={Boolean(status().overridePath)}>
                <Btn
                  testID="toolchain-override-clear"
                  label={t("settings.toolchain.clearOverride")}
                  onPress={clearOverride}
                />
              </If>
            </View>
            <If when={invalidOverride()}>
              <Text testID="toolchain-override-invalid" style={{ fontSize: 12, lineHeight: 16 }} class="text-destructive">
                {t("settings.toolchain.overrideInvalid", { path: status().overridePath ?? "" })}
              </Text>
            </If>
            <If when={Boolean(status().envOverride) && !status().overridePath}>
              <Text style={{ fontSize: 12, lineHeight: 16, fontFamily: MONO }} class="text-muted-foreground">
                {t("settings.toolchain.envNotice", { path: status().envOverride ?? "" })}
              </Text>
            </If>
          </View>
        </SectionRow>
      </Section>

      <Section title={t("settings.toolchain.detectedTitle")}>
        <If
          when={compilers().length > 0}
          else={
            <SectionRow first>
              <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
                {t("settings.toolchain.noDetected")}
              </Text>
            </SectionRow>
          }
        >
          <Each of={compilers()} key={(c) => c.path}>
            {(compiler, idx) => {
              const isActive = () => status().active?.path === compiler().path;
              return (
                <SectionRow
                  first={idx() === 0}
                  trailing={
                    isActive() ? (
                      <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                        <Text style={{ fontSize: 11.5, fontWeight: 500 }} class="text-primary">
                          {t("settings.toolchain.active")}
                        </Text>
                        <If when={Boolean(status().overridePath)}>
                          <Btn
                            label={t("settings.toolchain.clearOverride")}
                            variant="ghost"
                            onPress={clearOverride}
                          />
                        </If>
                      </View>
                    ) : (
                      <Btn
                        testID={`toolchain-use-${compiler().name}`}
                        label={t("settings.toolchain.useCompiler")}
                        onPress={() => selectCompiler(compiler().path)}
                      />
                    )
                  }
                >
                  <View style={{ minWidth: 0, gap: 2 }}>
                    <Text style={{ fontSize: 13, fontWeight: 500, fontFamily: MONO }} class="text-foreground">
                      {compiler().name}
                    </Text>
                    <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-secondary-foreground">
                      {compiler().version}
                    </Text>
                    <Text
                      tooltip={tip(compiler().path)}
                      style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: 11.5, lineHeight: 16, fontFamily: MONO }}
                      class="text-muted-foreground"
                    >
                      {compiler().path}
                    </Text>
                  </View>
                </SectionRow>
              );
            }}
          </Each>
        </If>
      </Section>

      <If when={!status().active}>
        <Section title={t("settings.toolchain.installTitle")}>
          <SectionRow first>
            <Text style={{ fontSize: 12.5, lineHeight: 19 }} class="text-secondary-foreground">
              {status().installGuide}
            </Text>
          </SectionRow>
        </Section>
      </If>
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

  const appearanceItems = computed(() =>
    APPEARANCE_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) })),
  );
  const languageItems = computed(() =>
    LANGUAGE_OPTIONS.map((o) => ({ value: o.value, label: o.label() })),
  );

  return (
    <View style={{ gap: 24 }}>
      <Card>
        <SectionRow
          first
          trailing={
            <Segmented
              items={appearanceItems()}
              value={appearanceMode()}
              onChange={(mode) => updateAppearanceMode(mode)}
              itemWidth={74}
            />
          }
        >
          <View style={{ gap: 2 }}>
            <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              {t("settings.appearance.themeMode")}
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              {t("settings.appearance.themeModeDesc")}
            </Text>
          </View>
        </SectionRow>
        <SectionRow
          trailing={
            <Segmented
              items={languageItems()}
              value={languagePref()}
              onChange={(pref) => updateLanguagePref(pref)}
              itemWidth={76}
            />
          }
        >
          <View style={{ gap: 2 }}>
            <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              {t("settings.appearance.language")}
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              {t("settings.appearance.languageDesc")}
            </Text>
          </View>
        </SectionRow>
        <SectionRow
          trailing={
            <Btn
              label={t("settings.appearance.openThemeFile")}
              icon="palette"
              onPress={() => void preview.openThemeFile()}
              testID="open-theme-file"
            />
          }
        >
          <View style={{ gap: 2 }}>
            <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              {t("settings.appearance.themeCustom")}
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              {t("settings.appearance.themeCustomDesc")}
            </Text>
          </View>
        </SectionRow>
      </Card>

      <Section
        title={t("settings.editor.title")}
        description={t("settings.editor.desc")}
        trailing={
          <If when={!isDefaultTypography()}>
            <Btn label={t("common.restoreDefault")} icon="rotateCcw" variant="ghost" onPress={resetEditorTypography} />
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
              {t("settings.editor.fontSize")}
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              {t("settings.editor.fontSizeDesc")}
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
              {t("settings.editor.lineHeight")}
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              {t("settings.editor.lineHeightDesc")}
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
              {t("settings.editor.ligatures")}
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              {t("settings.editor.ligaturesDesc")}
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
              {t("settings.editor.vimMode")}
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              {t("settings.editor.vimModeDesc")}
            </Text>
          </View>
        </SectionRow>
      </Section>
    </View>
  );
}

function renderInlineSpans(spans: readonly InlineSpan[]): JSX.Element {
  return spans.map((span) => {
    if (span.kind === "bold") {
      return (
        <Text style={{ fontWeight: 600 }} class="text-foreground">
          {span.text}
        </Text>
      );
    }
    if (span.kind === "code") {
      return (
        <Text style={{ fontSize: 11.5, fontFamily: MONO }} class="text-foreground">
          {span.text}
        </Text>
      );
    }
    return span.text;
  });
}

function UpdateNotesView(props: { notes: string }) {
  const blocks = computed(() => parseUpdateNotes(props.notes));
  return (
    <View testID="update-notes-panel" style={{ borderTopWidth: 1 }} class="border-border bg-muted/40">
      <Scroll style={{ maxHeight: 240 }}>
        <View style={{ paddingLeft: 14, paddingRight: 14, paddingTop: 10, paddingBottom: 12, gap: 6 }}>
          <Each
            of={blocks()}
            key={(b, i) => `${i}:${b.kind}:${b.kind === "heading" ? b.text : b.spans.map((s) => s.text).join("")}`}
          >
            {(block, index) => {
              const b = block();
              if (b.kind === "heading") {
                return (
                  <Text
                    style={{
                      fontSize: 12.5,
                      lineHeight: 18,
                      fontWeight: 600,
                      marginTop: index() > 0 ? 4 : 0,
                    }}
                    class="text-foreground"
                  >
                    {b.text}
                  </Text>
                );
              }
              if (b.kind === "bullet") {
                const indent = b.level === 1 ? 14 : 0;
                const marker = b.level === 1 ? "◦" : "•";
                return (
                  <View style={{ flexDirection: "row", alignItems: "flex-start", gap: 6, paddingLeft: indent }}>
                    <Text style={{ fontSize: 12, lineHeight: 18, width: 8 }} class="text-muted-foreground">
                      {marker}
                    </Text>
                    <Text style={{ flex: 1, minWidth: 0, fontSize: 12, lineHeight: 18 }} class="text-secondary-foreground">
                      {renderInlineSpans(b.spans)}
                    </Text>
                  </View>
                );
              }
              return (
                <Text style={{ fontSize: 12, lineHeight: 18 }} class="text-secondary-foreground">
                  {renderInlineSpans(b.spans)}
                </Text>
              );
            }}
          </Each>
        </View>
      </Scroll>
    </View>
  );
}

export function AboutPane() {
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
      const found = await checkForUpdate({ locale: locale() });
      if (!found) {
        updateState.set({ kind: "upToDate" });
        return;
      }
      updateState.set({ kind: "available", update: found });
    } catch (err) {
      if (isDevOrUnsupportedUpdateError(err)) {
        updateState.set({ kind: "error", message: t("settings.about.updateErr.unsupported") });
      } else if (isMissingOrPrivateUpdateChannelError(err)) {
        updateState.set({ kind: "error", message: t("settings.about.updateErr.channel") });
      } else {
        updateState.set({ kind: "error", message: t("settings.about.updateErr.checkFailed") });
      }
    }
  };

  const releaseOpenErr = signal<string | undefined>(undefined);

  const handleDownload = async (update: AppUpdate) => {
    releaseOpenErr.set(undefined);
    if (!update.canInstall) {
      try {
        const res = await openUrl(resolveReleasePageUrl(update));
        if (!res.opened) {
          releaseOpenErr.set(t("settings.about.updateErr.openReleaseFailed"));
        }
      } catch {
        releaseOpenErr.set(t("settings.about.updateErr.openReleaseFailed"));
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
        updateState.set({ kind: "error", message: t("settings.about.updateErr.unsupported") });
      } else {
        updateState.set({ kind: "error", message: t("settings.about.updateErr.downloadFailed") });
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
      updateState.set({ kind: "error", message: t("settings.about.updateErr.installFailed") });
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
      updateState.set({ kind: "error", message: t("settings.about.updateErr.scheduleFailed") });
    }
  };

  const updateSummary = (): string | undefined =>
    formatUpdateSummary(updateState(), devShell, formatVersion, formatBytes);

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

  const activeUpdateNotes = (): string => visibleUpdateNotes(updateState());

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
                    label={updateState().kind === "error" ? t("common.retry") : t("settings.about.checkNow")}
                    onPress={() => void handleCheckUpdate()}
                  />
                </If>
                <If when={updateState().kind === "checking"}>
                  <Btn label={t("settings.about.checking")} disabled />
                </If>
                <If when={updateState().kind === "available"}>
                  <Btn
                    testID="update-action-btn"
                    variant="primary"
                    label={updateAvailableActionLabel((updateState() as { kind: "available"; update: AppUpdate }).update)}
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
                        testID="update-install-on-quit-btn"
                        label={t("settings.about.installOnQuit")}
                        disabled={installing()}
                        onPress={() => {
                          const st = updateState();
                          if (st.kind === "ready") void handleInstallOnQuit(st.update);
                        }}
                      />
                    </If>
                    <Btn
                      testID="update-install-now-btn"
                      variant="primary"
                      label={t("settings.about.installNow")}
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
              {t("settings.about.checkUpdate")}
            </Text>
            <If when={Boolean(updateSummary())}>
              <Text
                testID="update-summary"
                style={{ fontSize: 12, lineHeight: 16, fontVariant: ["tabular-nums"] }}
                class="text-secondary-foreground"
              >
                {updateSummary() ?? ""}
              </Text>
            </If>
            <If when={Boolean(releaseOpenErr())}>
              <Text
                testID="release-open-error"
                style={{ fontSize: 12, lineHeight: 16 }}
                class="text-destructive"
              >
                {releaseOpenErr() ?? ""}
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

        <If when={activeUpdateNotes().length > 0}>
          <UpdateNotesView notes={activeUpdateNotes()} />
        </If>

        <SectionRow
          trailing={
            <Btn
              testID="replay-brand-btn"
              label={t("settings.about.play")}
              onPress={() => brandReplayOpen.set(true)}
            />
          }
        >
          <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
            {t("settings.about.replayIntro")}
          </Text>
        </SectionRow>

        <SectionRow
          trailing={
            <Btn
              testID="about-copy-diagnostics"
              label={copiedDiag() ? t("common.copied") : t("settings.about.copyDiagnostics")}
              icon={copiedDiag() ? "check" : "copy"}
              onPress={copyDiagnostics}
              style={{ minWidth: 124 }}
            />
          }
        >
          <View style={{ gap: 2 }}>
            <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              {t("settings.about.diagnostics")}
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              {t("settings.about.diagnosticsDesc")}
            </Text>
          </View>
        </SectionRow>
      </Card>

      {/* Credits & licenses */}
      <Section title={t("settings.about.creditsTitle")}>
        <SectionRow first>
          <View style={{ gap: 4 }}>
            <Text style={{ fontSize: 13, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              {t("settings.about.openSource")}
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 18 }} class="text-secondary-foreground">
              olymp.sty (GassaFM/olymp.sty), fuzzy-cnoi-statement (Wallbreaker5th), Typst, NSH, MiTeX, CeTZ (LGPL-3.0), oxifmt, Pinit, Mannot, Tiptoe
            </Text>
          </View>
        </SectionRow>
        <SectionRow>
          <View style={{ gap: 6 }}>
            <Text style={{ fontSize: 13, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              {t("settings.about.fontLicense")}
            </Text>
            <View style={{ gap: 3 }}>
              <Text style={{ fontSize: 12, lineHeight: 18 }} class="text-secondary-foreground">
                Latin Modern (GUST Font License)
              </Text>
              <Text style={{ fontSize: 12, lineHeight: 18 }} class="text-secondary-foreground">
                JetBrains Mono (SIL Open Font License, OFL)
              </Text>
            </View>
          </View>
        </SectionRow>
        <SectionRow>
          <View style={{ gap: 4 }}>
            <Text style={{ fontSize: 13, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              {t("settings.about.license")}
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
          {t("settings.about.dedication")}
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
                    {t(item().labelKey)}
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
          <If when={active() === "layout"}>
            <LayoutPane />
          </If>
          <If when={active() === "toolchain"}>
            <ToolchainPane />
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
