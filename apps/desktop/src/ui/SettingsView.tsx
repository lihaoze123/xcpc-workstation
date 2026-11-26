// Grouped settings view: left category rail (workspace, appearance, shortcuts, about)
// paired with rounded cards on the right.

import {
  Animated,
  cleanup,
  Clipboard,
  computed,
  createAnimatedValue,
  effect,
  reduceMotion,
  Scroll,
  signal,
  Text,
  View,
  type CompositeAnimation,
  type JSX,
} from "nsh";
import { Input, Switch } from "nsh/base";
import { promptForUpdate, shellInfo } from "nsh/updater";
import { Preview, type AboutInfoJs, type ToolchainStatusJs } from "xcpc-workstation/native";
import { BrandIcon } from "./BrandIcon";
import { ErrorDisplay } from "./ErrorDisplay";
import { languagePref, locale, resolveLocalized, t, type LanguagePref, type MessageKey } from "./i18n";
import { Icon, type IconName } from "./Icon";
import { Btn, createTransientFlag, IconBtn, Segmented, tip } from "./kit";
import { Section, SectionRow } from "./layout";
import { abbreviateHome, pathBase } from "./logic";
import { colorTransition, EASE_EXPO_CURVE, indicatorTransition } from "./motion";
import { ShortcutsList } from "./ShortcutsModal";
import { PluginsPane } from "./PluginsPane";
import {
  activeRevision,
  brandReplayOpen,
  ensureToolchainLoaded,
  errorOf,
  info,
  openError,
  openRecent,
  pickWorkspace,
  preview,
  refresh,
  refreshToolchain,
  requestNewWorkspace,
  resetEditorTypography,
  settingsSection,
  syncPrefsCache,
  toolchainAdvancedOpen,
  toolchainState,
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
  customOverrideInputValue,
  dedupDetectedCompilers,
  hasInvalidCxxOverride,
  middleTruncatePath,
  normalizeCxxOverrideInput,
} from "./studio/buildLogic";

const NAV_ITEMS: readonly { id: SettingsSection; labelKey: MessageKey; icon: IconName }[] = [
  { id: "workspace", labelKey: "settings.nav.workspace", icon: "folder" },
  { id: "layout", labelKey: "settings.nav.layout", icon: "fileText" },
  { id: "toolchain", labelKey: "settings.nav.toolchain", icon: "squareCode" },
  { id: "plugins", labelKey: "settings.nav.plugins", icon: "sparkles" },
  { id: "appearance", labelKey: "settings.nav.appearance", icon: "palette" },
  { id: "shortcuts", labelKey: "settings.nav.shortcuts", icon: "keyboard" },
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
  return shortCommit && shortCommit.toLowerCase() !== "dev"
    ? `${base}-dev (${shortCommit})`
    : `${base}-dev`;
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

function WorkspacePane(props: { active: boolean }) {
  const copied = signal(false);
  const showJson = signal(false);
  const revealError = signal<UiError | undefined>(undefined);
  const rev = activeRevision(() => props.active);
  const root = computed(() => info().root);

  const snippet = computed(() => {
    void rev();
    return root() ? preview.mcpConfigSnippet() : "";
  });
  const recent = computed(() => {
    void workspaceVersion();
    const cur = root();
    return preview.recentWorkspaces().filter((p) => p !== cur).slice(0, 5);
  });

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

function LayoutPane(props: { active: boolean }) {
  const err = signal<UiError | undefined>(undefined);
  const forkOpen = signal(false);
  const presetOpen = signal(false);
  const rev = activeRevision(() => props.active);

  const catalog = computed(() => {
    void rev();
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

const EMPTY_TOOLCHAIN: ToolchainStatusJs = {
  active: null,
  overridePath: null,
  compilers: [],
  installGuide: "",
  installCommands: [],
};

function ToolchainPane() {
  const status = signal<ToolchainStatusJs>(toolchainState() ?? EMPTY_TOOLCHAIN);
  const compilers = computed(() => dedupDetectedCompilers(status().compilers, status().active));
  const draft = signal(customOverrideInputValue(status().overridePath, compilers()));
  const redetected = createTransientFlag(1400);
  const overrideApplied = createTransientFlag(1500);

  ensureToolchainLoaded();
  effect(() => {
    const next = toolchainState();
    if (!next) return;
    status.set(next);
    const deduped = dedupDetectedCompilers(next.compilers, next.active);
    draft.set(customOverrideInputValue(next.overridePath, deduped));
  });

  const initPrefs = preview.loadPrefs();
  const savedLcMirror = signal(initPrefs.libraryCheckerMirrorUrl ?? "");
  const savedLcRepo = signal(initPrefs.libraryCheckerLocalRepoPath ?? "");
  const lcMirror = signal(initPrefs.libraryCheckerMirrorUrl ?? "");
  const lcLocalRepo = signal(initPrefs.libraryCheckerLocalRepoPath ?? "");
  const lcSaved = createTransientFlag(1600);
  const lcDirty = () =>
    lcMirror().trim() !== savedLcMirror().trim() || lcLocalRepo().trim() !== savedLcRepo().trim();

  const saveLcConfig = () => {
    const mirror = lcMirror().trim();
    const repo = lcLocalRepo().trim();
    const res = preview.setLibraryCheckerConfig(mirror || null, repo || null);
    if (res.ok) {
      savedLcMirror.set(mirror);
      savedLcRepo.set(repo);
      syncPrefsCache({
        libraryCheckerMirrorUrl: mirror || null,
        libraryCheckerLocalRepoPath: repo || null,
      });
      lcSaved.trigger();
    }
  };

  const pickLcDir = async () => {
    const picked = await preview.pickFolder({
      title: t("settings.lc.localRepoLabel"),
    });
    if (!picked.cancelled && picked.path) {
      lcLocalRepo.set(picked.path);
    }
  };

  const syncFromStatus = (next: ToolchainStatusJs) => {
    status.set(next);
    toolchainState.set(next);
    syncPrefsCache({ cxxOverride: next.overridePath ?? null });
    const deduped = dedupDetectedCompilers(next.compilers, next.active);
    draft.set(customOverrideInputValue(next.overridePath, deduped));
  };

  const redetect = () => {
    syncFromStatus(refreshToolchain());
    redetected.trigger();
  };

  const applyOverride = () => {
    const normalized = normalizeCxxOverrideInput(draft());
    const next = preview.setCxxOverride(normalized);
    syncFromStatus(next);
    if (!next.error) overrideApplied.trigger();
  };

  const clearOverride = () => {
    syncFromStatus(preview.setCxxOverride(null));
  };

  const selectCompiler = (path: string) => {
    syncFromStatus(preview.setCxxOverride(path));
  };

  const invalidOverride = () => hasInvalidCxxOverride(status());
  const currentCustomVal = () => customOverrideInputValue(status().overridePath, compilers());
  const dirtyDraft = () => (normalizeCxxOverrideInput(draft()) ?? "") !== currentCustomVal();

  return (
    <View testID="settings-toolchain-pane" style={{ gap: 24 }}>
      <Section
        title={t("settings.toolchain.title")}
        description={t("settings.toolchain.desc")}
        trailing={
          <Btn
            testID="toolchain-redetect-btn"
            label={redetected.active() ? t("settings.toolchain.refreshed") : t("settings.toolchain.redetect")}
            icon={redetected.active() ? "check" : "rotateCcw"}
            style={{ minWidth: 96 }}
            onPress={redetect}
          />
        }
      >
        <If
          when={compilers().length > 0}
          else={
            <SectionRow first>
              <View style={{ gap: 2 }}>
                <Text testID="toolchain-not-found" style={{ fontSize: 14, fontWeight: 500 }} class="text-foreground">
                  {t("settings.toolchain.notFound")}
                </Text>
                <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                  {t("settings.toolchain.noDetected")}
                </Text>
              </View>
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
                        <Icon name="check" size={14} tone="secondary-foreground" />
                        <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
                          {t("settings.toolchain.active")}
                        </Text>
                        <If when={Boolean(status().overridePath) && idx() > 0}>
                          <Btn label={t("settings.toolchain.clearOverride")} variant="ghost" onPress={clearOverride} />
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
                    <Text
                      testID={isActive() ? "toolchain-active-name" : undefined}
                      style={{ fontSize: 14, fontWeight: isActive() ? 600 : 500 }}
                      class="text-foreground"
                    >
                      {compiler().name}
                    </Text>
                    <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-secondary-foreground">
                      {compiler().version}
                    </Text>
                    <Text
                      tooltip={tip(compiler().path)}
                      style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: 12, lineHeight: 16, fontFamily: MONO }}
                      class="text-muted-foreground"
                    >
                      {middleTruncatePath(compiler().path, 42)}
                    </Text>
                  </View>
                </SectionRow>
              );
            }}
          </Each>
        </If>

        <SectionRow>
          <View style={{ gap: 8 }}>
            <View style={{ gap: 2 }}>
              <Text style={{ fontSize: 13, fontWeight: 500 }} class="text-foreground">
                {t("settings.toolchain.customPathLabel")}
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
                class="flex-1 h-8 px-2.5 rounded-md border border-border bg-card text-[13px] placeholder:text-muted-foreground focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring transition-colors"
                style={{ fontFamily: MONO }}
              />
              <Btn
                testID="toolchain-override-save"
                label={overrideApplied.active() ? t("settings.toolchain.applied") : t("settings.toolchain.saveOverride")}
                icon={overrideApplied.active() ? "check" : undefined}
                variant={overrideApplied.active() ? "outline" : "primary"}
                disabled={!dirtyDraft() && !overrideApplied.active()}
                style={{ minWidth: 72 }}
                onPress={applyOverride}
              />
              <If when={Boolean(currentCustomVal())}>
                <Btn testID="toolchain-override-clear" label={t("settings.toolchain.clearOverride")} onPress={clearOverride} />
              </If>
            </View>
            <If when={Boolean(status().error) || invalidOverride()}>
              <Text testID="toolchain-override-invalid" style={{ fontSize: 12, lineHeight: 16 }} class="text-destructive">
                {status().error ?? t("settings.toolchain.overrideInvalid", { path: status().overridePath ?? "" })}
              </Text>
            </If>
          </View>
        </SectionRow>
      </Section>

      <If when={!status().active}>
        <Section title={t("settings.toolchain.installTitle")} description={t("settings.toolchain.installDesc")}>
          <Each of={status().installCommands} key={(c) => c.label}>
            {(c, idx) => (
              <SectionRow first={idx() === 0}>
                <View style={{ gap: 6 }}>
                  <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                    {c().label}
                  </Text>
                  <CopyableCommand command={c().command} />
                </View>
              </SectionRow>
            )}
          </Each>
        </Section>
      </If>

      <View style={{ gap: 10 }}>
        <View
          testID="toolchain-advanced-toggle"
          onPress={() => toolchainAdvancedOpen.set(!toolchainAdvancedOpen())}
          style={{ flexDirection: "row", alignItems: "center", gap: 4, alignSelf: "flex-start" }}
        >
          <Icon name={toolchainAdvancedOpen() ? "chevronDown" : "chevronRight"} size={13} tone="muted-foreground" />
          <Text style={{ fontSize: 13 }} class="text-secondary-foreground hover:text-foreground">
            {t("settings.toolchain.advanced")}
          </Text>
        </View>
        <If when={toolchainAdvancedOpen()}>
          <View testID="toolchain-advanced" style={{ gap: 14 }}>
            <Section title={t("settings.lc.title")} description={t("settings.lc.desc")}>
              <SectionRow first>
                <View style={{ gap: 6 }}>
                  <Text style={{ fontSize: 13, fontWeight: 500 }} class="text-foreground">
                    {t("settings.lc.mirrorLabel")}
                  </Text>
                  <Input
                    testID="lc-mirror-input"
                    value={lcMirror()}
                    placeholder={t("settings.lc.mirrorPlaceholder")}
                    onChange={(e) => lcMirror.set(e.value)}
                    class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring"
                    style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 10, fontSize: 12.5, fontFamily: MONO }}
                  />
                </View>
              </SectionRow>
              <SectionRow>
                <View style={{ gap: 6 }}>
                  <Text style={{ fontSize: 13, fontWeight: 500 }} class="text-foreground">
                    {t("settings.lc.localRepoLabel")}
                  </Text>
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                    <Input
                      testID="lc-repo-input"
                      value={lcLocalRepo()}
                      placeholder={t("settings.lc.localRepoPlaceholder")}
                      onChange={(e) => lcLocalRepo.set(e.value)}
                      class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring"
                      style={{ flex: 1, height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 10, fontSize: 12.5, fontFamily: MONO }}
                    />
                    <Btn variant="outline" icon="folderOpen" label={t("settings.lc.pickDir")} onPress={() => void pickLcDir()} />
                  </View>
                </View>
              </SectionRow>
              <SectionRow>
                <View style={{ flexDirection: "row", justifyContent: "flex-end" }}>
                  <Btn
                    testID="lc-save-btn"
                    variant={lcSaved.active() ? "outline" : "primary"}
                    icon={lcSaved.active() ? "check" : undefined}
                    label={lcSaved.active() ? t("settings.toolchain.applied") : t("settings.toolchain.saveOverride")}
                    disabled={!lcDirty() && !lcSaved.active()}
                    onPress={saveLcConfig}
                    style={{ minWidth: 78 }}
                  />
                </View>
              </SectionRow>
            </Section>

            <View style={{ paddingLeft: 17, gap: 6 }}>
              <Text style={{ fontSize: 12, lineHeight: 18 }} class="text-muted-foreground">
                {t("settings.toolchain.advancedPath")}
              </Text>
              <Text style={{ fontSize: 12, lineHeight: 18 }} class="text-muted-foreground">
                {t("settings.toolchain.advancedEnv")}
              </Text>
              <Text style={{ fontSize: 12, lineHeight: 18 }} class="text-muted-foreground">
                {t("settings.toolchain.advancedFlags")}
              </Text>
              <If when={Boolean(status().envOverride)}>
                <Text style={{ fontSize: 12, lineHeight: 18, fontFamily: MONO }} class="text-secondary-foreground">
                  {t("settings.toolchain.envNotice", { path: status().envOverride ?? "" })}
                </Text>
              </If>
            </View>
          </View>
        </If>
      </View>
    </View>
  );
}

/** A shell command in a code block with a copy button (Settings → Toolchain). */
function CopyableCommand(props: { command: string }) {
  const copied = createTransientFlag(1200);
  return (
    <View
      style={{
        flexDirection: "row",
        alignItems: "center",
        gap: 8,
        paddingLeft: 10,
        paddingRight: 4,
        paddingTop: 6,
        paddingBottom: 6,
        borderRadius: 6,
      }}
      class="bg-muted"
    >
      <Text style={{ flex: 1, minWidth: 0, fontSize: 13, lineHeight: 20, fontFamily: MONO }} class="text-foreground">
        {props.command}
      </Text>
      <IconBtn
        icon={copied.active() ? "check" : "copy"}
        size={26}
        tone="secondary-foreground"
        tooltip={tip(copied.active() ? t("common.copied") : t("settings.toolchain.copyCommand"))}
        onPress={() => {
          void Clipboard.writeText(props.command).then(() => copied.trigger());
        }}
      />
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

export function AboutPane() {
  const about = readAboutInfo();
  const devShell = isDevShell();
  const copiedDiag = signal(false);
  const checkingUpdate = signal(false);

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
    if (devShell || checkingUpdate()) return;
    checkingUpdate.set(true);
    try {
      await promptForUpdate({ quiet: false, locale: locale() });
    } finally {
      checkingUpdate.set(false);
    }
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
              <Btn
                testID="check-update-btn"
                label={checkingUpdate() ? t("settings.about.checking") : t("settings.about.checkNow")}
                disabled={checkingUpdate()}
                onPress={() => void handleCheckUpdate()}
              />
            )
          }
        >
          <View style={{ gap: 2 }}>
            <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              {t("settings.about.checkUpdate")}
            </Text>
            <If when={devShell}>
              <Text
                testID="update-summary"
                style={{ fontSize: 12, lineHeight: 16 }}
                class="text-secondary-foreground"
              >
                {t("update.devBuild")}
              </Text>
            </If>
          </View>
        </SectionRow>

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

export function SettingsView(props: { active?: boolean } = {}) {
  const isActive = () => props.active ?? true;
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
    // Keep opacity at 1 so switching settings panes (e.g. Workspace -> Plugins) never flashes a blank frame.
    paneOpacity.set(1);
    paneY.set(4);
    const anim = Animated.timing(paneY, { toValue: 0, duration: 150, easing: EASE_EXPO_CURVE });
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
      <Scroll smoothWheel style={{ flex: 1 }} class="bg-background">
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
            <WorkspacePane active={isActive()} />
          </If>
          <If when={active() === "layout"}>
            <LayoutPane active={isActive()} />
          </If>
          <If when={active() === "toolchain"}>
            <ToolchainPane />
          </If>
          <If when={active() === "plugins"}>
            <PluginsPane />
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
