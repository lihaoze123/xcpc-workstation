import "./src/ui/theme";
import { cleanup, computed, effect, keymap, mount, registerFont, signal, Text, View, type JSX } from "nsh";
import { Preview } from "xcpc-workstation/native";
import { CommandPalette } from "./src/ui/CommandPalette";
import { UnfreezeDialog } from "./src/ui/Contest";
import { ContestsView } from "./src/ui/ContestsView";
import { Dialogs } from "./src/ui/Dialogs";
import { ErrorDisplay } from "./src/ui/ErrorDisplay";
import { Btn, Chevron, Kbd, tip } from "./src/ui/kit";
import { InsetPanel } from "./src/ui/layout";
import { Icon } from "./src/ui/Icon";
import { t } from "./src/ui/i18n";
import { LibraryView } from "./src/ui/LibraryView";
import { pathBase, visibleSpaces, windowTitle } from "./src/ui/logic";
import { AppMenu, type MenuEntry } from "./src/ui/Menu";
import { colorTransition, studioExitMs, studioTransform, studioTransition } from "./src/ui/tokens";
import { NavRail } from "./src/ui/NavRail";
import { NewWorkspaceModal } from "./src/ui/NewWorkspaceModal";
import { NoWorkspaceView } from "./src/ui/NoWorkspaceView";
import { formatAppVersion, readAboutInfo, SettingsView } from "./src/ui/SettingsView";
import { ShortcutsModal, shortcutsOpen } from "./src/ui/ShortcutsModal";
import { BundlerModal } from "./src/ui/snippets/BundlerModal";
import { NewSnippetModal, NewTrdModal } from "./src/ui/snippets/NewSnippetModal";
import { SnippetsView } from "./src/ui/snippets/SnippetsView";
import { SnippetStudio } from "./src/ui/snippets/SnippetStudio";
import { TrdStudio } from "./src/ui/snippets/TrdStudio";
import { StudioView } from "./src/ui/studio/StudioView";
import { ToastHost } from "./src/ui/Toast";
import { ToolsView } from "./src/ui/ToolsView";
import { TrainingView } from "./src/ui/TrainingView";
import interVariableFontUrl from "./src/ui/fonts/InterVariable.woff2?url";
import jetBrainsMonoRegularFontUrl from "./src/ui/fonts/JetBrainsMono-Regular.woff2?url";
import jetBrainsMonoSemiBoldFontUrl from "./src/ui/fonts/JetBrainsMono-SemiBold.woff2?url";
import { fontsReady, popoverShadow, uiFontSource } from "./src/ui/theme";
import { TITLE_BAR, TitleBar, useWindowTitle } from "./src/ui/TitleBar";
import { bindDiagnosticsLifecycle, openDiagnosticsWindow } from "./src/ui/o11y/windowManager";
import { syncTraceStatus, traceInteraction } from "./src/ui/trace";
import {
  activeSnippetStudio,
  activeTrdStudio,
  brandReplayOpen,
  flushPrefs,
  goSpace,
  hasWorkspace,
  hydrateAppearancePrefs,
  importWarnings,
  info,
  jumpToContest,
  newWorkspaceModalOpen,
  openError,
  openRecent,
  paletteOpen,
  pickWorkspace,
  preview,
  requestNewWorkspace,
  selectedContest,
  selectedProblem,
  space,
  startPluginWatch,
  startWatch,
  studio,
  workspaceVersion,
} from "./src/ui/store";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "./src/ui/tokens";
// The shell: the title bar, the 3 spaces behind the nav rail, and the studio that
// takes the whole window when a problem or contest is open (the rail folds away). The
// spaces and the studio stay mounted once shown, so switching back is instant and
// keeps their state; only `display` changes. The root renders in the first frame, on
// the canvas color, whatever is still loading.

/** Keeps a space or studio mounted (`display: none/flex`). Full-window studio switches use a
 *  short pure-opacity fade; peer space switches (`题库 ⇄ 比赛 ⇄ 设置`) swap in 1 frame so
 *  GTK4 never allocates a full-screen offscreen FBO (NavRail's spring pill provides the motion). */
function SpaceStage(props: {
  visible: boolean;
  variant?: "space" | "studio";
  animateOnMount?: boolean;
  direction?: "row" | "column";
  children?: JSX.Element;
}) {
  const isStudio = props.variant === "studio";
  const revealed = signal(props.visible && (!isStudio || !props.animateOnMount));
  const displayed = signal(props.visible);
  let initialized = false;
  let raf: number | undefined;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;

  effect(() => {
    const isVisible = props.visible;
    const first = !initialized;
    initialized = true;
    if (raf !== undefined) {
      cancelAnimationFrame(raf);
      raf = undefined;
    }
    if (hideTimer !== undefined) {
      clearTimeout(hideTimer);
      hideTimer = undefined;
    }
    if (!isStudio) {
      // Peer spaces swap synchronously in 1 frame (zero blank frame, no full-screen FBO);
      // NavRail's spring indicator carries the spatial cue cleanly.
      displayed.set(isVisible);
      revealed.set(isVisible);
      return;
    }
    if (isVisible) {
      const wasDisplayed = displayed();
      displayed.set(true);
      if (wasDisplayed || (first && !props.animateOnMount)) {
        // Re-entering mid-exit interrupts the native spring from its current position/velocity.
        revealed.set(true);
      } else {
        revealed.set(false);
        raf = requestAnimationFrame(() => {
          raf = undefined;
          revealed.set(true);
        });
      }
    } else {
      revealed.set(false);
      const exitMs = studioExitMs();
      if (exitMs === 0) {
        displayed.set(false);
      } else {
        hideTimer = setTimeout(() => {
          hideTimer = undefined;
          displayed.set(false);
        }, exitMs);
      }
    }
  });
  cleanup(() => {
    if (raf !== undefined) cancelAnimationFrame(raf);
    if (hideTimer !== undefined) clearTimeout(hideTimer);
  });

  return (
    <View
      class={isStudio ? "bg-background" : undefined}
      style={{
        flex: isStudio ? undefined : 1,
        ...(isStudio ? { position: "absolute", top: 0, left: 0, right: 0, bottom: 0 } : { minWidth: 0, minHeight: 0 }),
        flexDirection: props.direction,
        display: displayed() ? "flex" : "none",
        pointerEvents: props.visible ? "auto" : "none",
        ...(isStudio
          ? {
              opacity: revealed() ? 1 : 0,
              transform: studioTransform(revealed()),
              ...studioTransition(),
            }
          : undefined),
      }}
    >
      {props.children}
    </View>
  );
}

/** Full-screen overlay that replays the brand page from Settings -> About without disturbing the open workspace. */
function BrandReplayOverlay(props: { version: string }) {
  const close = () => brandReplayOpen.set(false);
  keymap.bind({ escape: close }, { when: "always" });

  return (
    <View
      testID="brand-replay-overlay"
      style={{ position: "absolute", top: 0, left: 0, right: 0, bottom: 0, flexDirection: "column" }}
      class="bg-background"
    >
      <TitleBar
        borderless
        leading={
          <Btn
            testID="brand-replay-close"
            label={t("common.back")}
            icon="arrowLeft"
            variant="ghost"
            onPress={close}
          />
        }
      />
      <NoWorkspaceView version={props.version} replayOnly />
    </View>
  );
}

function App() {
  cleanup(flushPrefs);
  const appVersion = formatAppVersion(readAboutInfo());
  void hydrateAppearancePrefs();

  // Mounted synchronously on first use (no 1-frame blank flash), then kept mounted.
  const inStudio = computed(() => studio() !== undefined);
  const inSnippetStudio = computed(() => activeSnippetStudio() !== undefined);
  const inTrdStudio = computed(() => activeTrdStudio() !== undefined);
  const inAnyStudio = computed(() => inStudio() || inSnippetStudio() || inTrdStudio());
  const studioMounted = computed<boolean>((prev) => Boolean(prev) || inStudio());
  const snippetStudioMounted = computed<boolean>((prev) => Boolean(prev) || inSnippetStudio());
  const trdStudioMounted = computed<boolean>((prev) => Boolean(prev) || inTrdStudio());

  // After browsing settles outside Studio (6s idle), drop heavy Typst AST/comemo/WASM
  // caches while keeping already-rasterized thumbnail pixels warm so visuals never flash.
  let idleReclaimTimer: ReturnType<typeof setTimeout> | undefined;
  effect(() => {
    clearTimeout(idleReclaimTimer);
    idleReclaimTimer = undefined;
    const activeStudio = inAnyStudio();
    void space();
    void selectedProblem();
    void selectedContest();
    if (activeStudio) return;
    idleReclaimTimer = setTimeout(() => {
      idleReclaimTimer = undefined;
      if (!inAnyStudio() && typeof preview.reclaimMemory === "function") {
        preview.reclaimMemory(false);
      }
    }, 6000);
  });
  cleanup(() => clearTimeout(idleReclaimTimer));

  useWindowTitle(() => {
    const t = studio();
    let s: { problemTitle?: string; label?: string; contestName?: string } | undefined;
    if (t?.target === "problem") s = { problemTitle: info().problems.find((p) => p.id === t.id)?.title ?? t.id };
    else if (t?.target === "contest") {
      const c = info().contests.find((x) => x.id === t.id);
      const it = c?.items.find((i) => i.label === t.label) ?? c?.items[0];
      s = { contestName: c?.name ?? t.id, problemTitle: it?.title, label: it?.label };
    }
    return windowTitle({
      workspace: hasWorkspace(),
      workspaceName: hasWorkspace() ? pathBase(info().root) : undefined,
      space: space(),
      studio: s,
    });
  });

  const goVisibleSlot = (idx: number) => {
    if (inAnyStudio()) return;
    const list = visibleSpaces({
      hasWorkspace: hasWorkspace(),
      snippetsEnabled: info().snippetsEnabled,
      trainingEnabled: info().trainingEnabled,
    });
    const target = list[idx];
    if (target) traceInteraction("nav.space", () => goSpace(target), { space: target });
  };

  keymap.bind({
    "mod+k": () => traceInteraction("palette.toggle", () => paletteOpen.set(!paletteOpen())),
    "mod+/": () => shortcutsOpen.set(!shortcutsOpen()),
    "ctrl+shift+d": () => openDiagnosticsWindow(),
    "mod+1": () => goVisibleSlot(0),
    "mod+2": () => goVisibleSlot(1),
    "mod+3": () => goVisibleSlot(2),
    "mod+4": () => goVisibleSlot(3),
    "mod+5": () => goVisibleSlot(4),
    "mod+6": () => goVisibleSlot(5),
    "mod+o": () => void pickWorkspace(),
  }, { when: "always" });

  bindDiagnosticsLifecycle();
  startWatch();
  startPluginWatch();

  const workspaceRoot = computed(() => info().root);
  const workspaceEntries = computed<MenuEntry[]>(() => {
    void workspaceVersion();
    const cur = workspaceRoot();
    const recents = preview.recentWorkspaces().filter((p) => p !== cur).slice(0, 5);
    return [
      ...recents.map((p): MenuEntry => ({ id: `recent:${p}`, label: pathBase(p), icon: "folder" })),
      ...(recents.length > 0 ? [{ separator: true as const, id: "sep-recent" }] : []),
      { id: "open", label: t("shell.openOtherWorkspace"), icon: "folderOpen", keys: "mod+o" },
      { id: "new", label: t("shell.newWorkspaceEllipsis"), icon: "folderPlus" },
      { separator: true as const, id: "sep-reveal" },
      { id: "reveal", label: t("common.revealInFolder"), icon: "folder" },
    ];
  });

  const onWorkspaceAction = (id: string) => {
    if (id.startsWith("recent:")) openRecent(id.slice("recent:".length));
    else if (id === "open") void pickWorkspace();
    else if (id === "new") requestNewWorkspace();
    else if (id === "reveal" && info().root) void preview.revealInFolder(info().root);
  };

  const shell = (
    <View
      style={{
        flex: 1,
        flexDirection: "column",
        pointerEvents: inAnyStudio() ? "none" : "auto",
      }}
    >
      <TitleBar
        borderless
        leading={
          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6], minWidth: 0, maxWidth: 224 }}>
            <AppMenu
              entries={workspaceEntries()}
              onSelect={onWorkspaceAction}
              minWidth={200}
              trigger={
                <View
                  testID="workspace-switcher"
                  tooltip={tip(info().root)}
                  class="hover:bg-muted active:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
                  style={{
                    height: CONTROL.md,
                    maxWidth: info().readOnly ? 168 : 220,
                    paddingLeft: SPACE[8],
                    paddingRight: SPACE[6],
                    borderRadius: RADIUS.sm,
                    flexDirection: "row",
                    alignItems: "center",
                    gap: SPACE[4],
                    flexShrink: 1,
                    minWidth: 0,
                    ...colorTransition(),
                  }}
                >
                  <Icon name="folder" size="sm" tone="secondary-foreground" />
                  <Text
                    style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.body, fontWeight: FONT_WEIGHT.semibold, flexShrink: 1, numberOfLines: 1, textOverflow: "ellipsis" }}
                    class="text-foreground"
                  >
                    {pathBase(info().root)}
                  </Text>
                  <Chevron size="sm" tone="muted-foreground" />
                </View>
              }
            />
            <If when={info().readOnly}>
              <View
                tooltip={tip(t("common.readOnlyWorkspaceDisabled"))}
                class="bg-muted border border-border"
                style={{
                  height: 22, // token-exempt: compact inline status badge height
                  paddingLeft: SPACE[6],
                  paddingRight: SPACE[6],
                  borderRadius: RADIUS.xs,
                  flexDirection: "row",
                  alignItems: "center",
                  gap: SPACE[4],
                  flexShrink: 0,
                }}
              >
                <Icon name="lock" size="sm" tone="muted-foreground" />
                <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption }} class="text-secondary-foreground">
                  {t("common.readOnly")}
                </Text>
              </View>
            </If>
          </View>
        }
        center={
          <View
            testID="palette-trigger"
            onPress={() => paletteOpen.set(true)}
            class="border border-border bg-card hover:bg-muted active:bg-accent"
            style={{
              width: 320,
              height: CONTROL.md,
              borderRadius: RADIUS.md,
              paddingLeft: SPACE[12],
              paddingRight: SPACE[4],
              flexDirection: "row",
              alignItems: "center",
              gap: SPACE[8],
              ...colorTransition(),
            }}
          >
            <Icon name="search" size="sm" tone="muted-foreground" />
            <Text style={{ flex: 1, fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.body }} class="text-muted-foreground">
              {t("shell.paletteTrigger")}
            </Text>
            <Kbd keys="mod+k" faint />
          </View>
        }
      />
      <View style={{ flex: 1, flexDirection: "row", minHeight: 0 }}>
        <NavRail
          space={space()}
          snippetsEnabled={info().snippetsEnabled}
          trainingEnabled={info().trainingEnabled}
          hasWorkspace
          onChange={(next) => traceInteraction("nav.space", () => goSpace(next), { space: next })}
        />
        <InsetPanel flushLeft testID="shell-inset-panel">
          <SpaceStage visible={space() === "problems"} variant="space">
            <LibraryView active={!inAnyStudio() && space() === "problems"} />
          </SpaceStage>
          <SpaceStage visible={space() === "contests"} variant="space">
            <ContestsView active={!inAnyStudio() && space() === "contests"} />
          </SpaceStage>
          <SpaceStage visible={info().snippetsEnabled && space() === "snippets"} variant="space">
            <If when={info().snippetsEnabled}>
              <SnippetsView active={!inAnyStudio() && space() === "snippets"} />
            </If>
          </SpaceStage>
          <SpaceStage visible={info().trainingEnabled && space() === "training"} variant="space">
            <If when={info().trainingEnabled}>
              <TrainingView active={!inAnyStudio() && space() === "training"} />
            </If>
          </SpaceStage>
          <SpaceStage visible={space() === "tools"} variant="space">
            <ToolsView active={!inAnyStudio() && space() === "tools"} />
          </SpaceStage>
          <SpaceStage visible={space() === "settings"} variant="space">
            <SettingsView active={!inAnyStudio() && space() === "settings"} />
          </SpaceStage>
        </InsetPanel>
      </View>
    </View>
  );

  const noWorkspaceShell = (
    <If
      when={space() === "tools" || space() === "settings"}
      else={
        <View style={{ flex: 1 }}>
          <TitleBar borderless />
          <NoWorkspaceView version={appVersion} />
        </View>
      }
    >
      <View style={{ flex: 1, flexDirection: "column" }}>
        <TitleBar
          borderless
          leading={
            <Btn
              testID="no-workspace-back"
              label={t("common.back")}
              icon="arrowLeft"
              variant="ghost"
              onPress={() => goSpace("problems")}
            />
          }
          trailing={
            <Btn
              size="sm"
              variant="outline"
              icon="folderOpen"
              label={t("brand.openExistingFolder")}
              onPress={() => void pickWorkspace()}
            />
          }
        />
        <View style={{ flex: 1, flexDirection: "row", minHeight: 0 }}>
          <NavRail
            space={space()}
            hasWorkspace={false}
            snippetsEnabled={false}
            trainingEnabled={false}
            onChange={(next) => traceInteraction("nav.space", () => goSpace(next), { space: next })}
          />
          <InsetPanel flushLeft>
            <SpaceStage visible={space() === "tools"} variant="space">
              <ToolsView active={space() === "tools"} />
            </SpaceStage>
            <SpaceStage visible={space() === "settings"} variant="space">
              <SettingsView active={space() === "settings"} />
            </SpaceStage>
          </InsetPanel>
        </View>
      </View>
    </If>
  );

  return (
    <View style={{ flex: 1 }} class="bg-background">
      <If when={hasWorkspace()} else={noWorkspaceShell}>
        <View style={{ flex: 1 }}>
          {shell}
          <If when={studioMounted()}>
            <SpaceStage visible={inStudio()} variant="studio" animateOnMount>
              <StudioView active={inStudio()} />
            </SpaceStage>
          </If>
          <If when={snippetStudioMounted()}>
            <SpaceStage visible={inSnippetStudio()} variant="studio" animateOnMount>
              <SnippetStudio active={inSnippetStudio()} />
            </SpaceStage>
          </If>
          <If when={trdStudioMounted()}>
            <SpaceStage visible={inTrdStudio()} variant="studio" animateOnMount>
              <TrdStudio active={inTrdStudio()} />
            </SpaceStage>
          </If>
          <If when={openError() && (inStudio() || space() !== "settings")}>
            <View
              class="bg-popover border border-border"
              style={{
                position: "absolute",
                top: TITLE_BAR + 8,
                right: 16,
                maxWidth: 420,
                paddingLeft: SPACE[12],
                paddingRight: SPACE[12],
                paddingTop: SPACE[8],
                paddingBottom: SPACE[8],
                borderRadius: RADIUS.md,
                boxShadow: popoverShadow(),
              }}
            >
              <ErrorDisplay error={openError()} onDismiss={() => openError.set(undefined)} />
            </View>
          </If>
          <If when={!openError() ? importWarnings() : undefined}>
            {(iw) => (
              <View
                testID="import-warnings-notice"
                class="bg-popover border border-border"
                style={{
                  position: "absolute",
                  top: TITLE_BAR + 8,
                  right: 16,
                  maxWidth: 420,
                  paddingLeft: SPACE[12],
                  paddingRight: SPACE[12],
                  paddingTop: SPACE[8],
                  paddingBottom: SPACE[8],
                  borderRadius: RADIUS.md,
                  gap: SPACE[4],
                  boxShadow: popoverShadow(),
                }}
              >
                <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
                  <Icon name="triangleAlert" size="sm" tone="chart-4" />
                  <Text style={{ flex: 1, fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                    {iw().warnings.length === 1
                      ? t("packages.importWarningsOne", { title: iw().title })
                      : t("packages.importWarningsMany", { title: iw().title, count: iw().warnings.length })}
                  </Text>
                  <View
                    testID="import-warnings-dismiss"
                    onPress={() => importWarnings.set(undefined)}
                    class="hover:bg-muted active:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
                    style={{
                      width: 18,
                      height: 18, // token-exempt: compact inline icon badge box
                      borderRadius: RADIUS.xs,
                      alignItems: "center",
                      justifyContent: "center",
                      ...colorTransition(),
                    }}
                  >
                    <Icon name="x" size="sm" tone="muted-foreground" />
                  </View>
                </View>
                <Each of={iw().warnings.slice(0, 3)} key={(w, i) => `${i}:${w.code}:${w.path ?? ""}`}>
                  {(w) => (
                    <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption }} class="text-secondary-foreground">
                      {w().message}
                    </Text>
                  )}
                </Each>
              </View>
            )}
          </If>
        </View>
        <Dialogs onJumpContest={(contestId) => jumpToContest(contestId)} />
        <UnfreezeDialog />
        <BundlerModal />
        <NewSnippetModal />
        <NewTrdModal />
      </If>
      <CommandPalette />
      <If when={brandReplayOpen()}>
        <BrandReplayOverlay version={appVersion} />
      </If>
      <ShortcutsModal />
      <NewWorkspaceModal open={newWorkspaceModalOpen()} onClose={() => newWorkspaceModalOpen.set(false)} />
      <ToastHost />
    </View>
  );
}

// Sync JS trace collector at startup when launched with `XCPC_TRACE=1`.
syncTraceStatus();

async function registerBundledUiFonts(): Promise<void> {
  try {
    const [interRes, monoRegularRes, monoSemiBoldRes] = await Promise.allSettled([
      registerFont(interVariableFontUrl),
      registerFont(jetBrainsMonoRegularFontUrl),
      registerFont(jetBrainsMonoSemiBoldFontUrl),
    ]);
    const hasInter = interRes.status === "fulfilled";
    const monoLoaded = (monoRegularRes.status === "fulfilled" ? 1 : 0) + (monoSemiBoldRes.status === "fulfilled" ? 1 : 0);
    if (hasInter && monoLoaded === 2) {
      uiFontSource.set("bundled Inter + JetBrains Mono");
    } else if (hasInter && monoLoaded === 1) {
      uiFontSource.set("bundled Inter + JetBrains Mono (partial)");
    } else if (hasInter) {
      uiFontSource.set("bundled Inter (JetBrains Mono fallback)");
    } else if (monoLoaded === 2) {
      uiFontSource.set("bundled JetBrains Mono (Inter fallback)");
    } else if (monoLoaded === 1) {
      uiFontSource.set("bundled JetBrains Mono (partial, Inter fallback)");
    } else {
      uiFontSource.set("system fallback");
    }
  } catch {
    uiFontSource.set("system fallback");
  }
}

// Load extra JetBrains Mono faces (Italic/Bold/NL) asynchronously for the editor
// without blocking the first frame.
function loadExtraEditorFonts(): void {
  try {
    const fonts = Preview.uiFonts();
    if (fonts.length > 0) {
      void Promise.all(fonts.map((bytes) => registerFont(bytes).catch(() => {}))).finally(() => fontsReady.set(true));
    } else {
      fontsReady.set(true);
    }
  } catch {
    fontsReady.set(true);
  }
}

// Register bundled UI fonts (Inter + JetBrains Mono Regular/SemiBold) before mounting
// the root component so the very first frame renders with zero font-swap flash or shift.
void registerBundledUiFonts().finally(() => {
  mount(App);
  loadExtraEditorFonts();
});
