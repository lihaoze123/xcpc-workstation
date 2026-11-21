import "./src/ui/theme";
import { cleanup, computed, effect, keymap, mount, reduceMotion, registerFont, signal, Text, View, type JSX } from "nsh";
import { Preview } from "xcpc-workstation/native";
import { CommandPalette } from "./src/ui/CommandPalette";
import { UnfreezeDialog } from "./src/ui/Contest";
import { ContestsView } from "./src/ui/ContestsView";
import { Dialogs } from "./src/ui/Dialogs";
import { ErrorDisplay } from "./src/ui/ErrorDisplay";
import { Btn, Chevron, Kbd, tip } from "./src/ui/kit";
import { Icon } from "./src/ui/Icon";
import { LibraryView } from "./src/ui/LibraryView";
import { pathBase, windowTitle } from "./src/ui/logic";
import { AppMenu, type MenuEntry } from "./src/ui/Menu";
import { pageTransform, pageTransition } from "./src/ui/motion";
import { NavRail } from "./src/ui/NavRail";
import { NewWorkspaceModal } from "./src/ui/NewWorkspaceModal";
import { NoWorkspaceView } from "./src/ui/NoWorkspaceView";
import { formatAppVersion, readAboutInfo, SettingsView } from "./src/ui/SettingsView";
import { ShortcutsModal, shortcutsOpen } from "./src/ui/ShortcutsModal";
import { StudioView } from "./src/ui/studio/StudioView";
import { EASE_STANDARD, fontsReady, ms, popoverShadow } from "./src/ui/theme";
import { TITLE_BAR, TitleBar, useWindowTitle } from "./src/ui/TitleBar";
import {
  brandReplayOpen,
  goSpace,
  hasWorkspace,
  hydrateAppearancePrefs,
  info,
  newWorkspaceModalOpen,
  openError,
  openRecent,
  paletteOpen,
  pickWorkspace,
  preview,
  requestNewWorkspace,
  selectedContest,
  space,
  startWatch,
  studio,
} from "./src/ui/store";

// The shell: the title bar, the 3 spaces behind the nav rail, and the studio that
// takes the whole window when a problem or contest is open (the rail folds away). The
// spaces and the studio stay mounted once shown, so switching back is instant and
// keeps their state; only `display` changes. The root renders in the first frame, on
// the canvas color, whatever is still loading.

/** Keeps a space or studio mounted (`display: none/flex`) while revealing it with a restrained native spring. */
function SpaceStage(props: {
  visible: boolean;
  variant?: "space" | "studio";
  animateOnMount?: boolean;
  direction?: "row" | "column";
  children?: JSX.Element;
}) {
  const revealed = signal(props.visible && !props.animateOnMount);
  let initialized = false;
  let raf: number | undefined;

  effect(() => {
    const isVisible = props.visible;
    const first = !initialized;
    initialized = true;
    if (raf !== undefined) {
      cancelAnimationFrame(raf);
      raf = undefined;
    }
    if (!isVisible) {
      revealed.set(false);
    } else if (reduceMotion() || (first && !props.animateOnMount)) {
      revealed.set(true);
    } else {
      revealed.set(false);
      raf = requestAnimationFrame(() => {
        raf = undefined;
        revealed.set(true);
      });
    }
  });
  cleanup(() => {
    if (raf !== undefined) cancelAnimationFrame(raf);
  });

  return (
    <View
      style={{
        flex: 1,
        flexDirection: props.direction,
        display: props.visible ? "flex" : "none",
        opacity: revealed() ? 1 : 0,
        transform: pageTransform(revealed(), props.variant ?? "space"),
        ...pageTransition(220),
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
            label="返回"
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
  const appVersion = formatAppVersion(readAboutInfo());
  void hydrateAppearancePrefs();

  // Mounted synchronously on first use (no 1-frame blank flash), then kept mounted.
  const inStudio = computed(() => studio() !== undefined);
  const studioMounted = computed<boolean>((prev) => Boolean(prev) || inStudio());

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

  keymap.bind({
    "mod+k": () => hasWorkspace() && paletteOpen.set(!paletteOpen()),
    "mod+/": () => shortcutsOpen.set(!shortcutsOpen()),
    "mod+1": () => hasWorkspace() && goSpace("problems"),
    "mod+2": () => hasWorkspace() && goSpace("contests"),
    "mod+3": () => hasWorkspace() && goSpace("settings"),
    "mod+o": () => void pickWorkspace(),
  }, { when: "always" });

  startWatch();

  const workspaceEntries = computed<MenuEntry[]>(() => {
    const cur = info().root;
    const recents = preview.recentWorkspaces().filter((p) => p !== cur).slice(0, 5);
    return [
      ...recents.map((p): MenuEntry => ({ id: `recent:${p}`, label: pathBase(p), icon: "folder" })),
      ...(recents.length > 0 ? [{ separator: true as const, id: "sep-recent" }] : []),
      { id: "open", label: "打开其他工作区…", icon: "folderOpen", keys: "mod+o" },
      { id: "new", label: "新建工作区…", icon: "folderPlus" },
      { separator: true as const, id: "sep-reveal" },
      { id: "reveal", label: "在文件夹中显示", icon: "folder" },
    ];
  });

  const onWorkspaceAction = (id: string) => {
    if (id.startsWith("recent:")) openRecent(id.slice("recent:".length));
    else if (id === "open") void pickWorkspace();
    else if (id === "new") requestNewWorkspace();
    else if (id === "reveal" && info().root) void preview.revealInFolder(info().root);
  };

  const shell = (
    <SpaceStage visible={!inStudio()} variant="studio" direction="column">
      <TitleBar
        leading={
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6, minWidth: 0, maxWidth: 224 }}>
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
                    height: 28,
                    maxWidth: info().readOnly ? 168 : 220,
                    paddingLeft: 8,
                    paddingRight: 6,
                    borderRadius: 6,
                    flexDirection: "row",
                    alignItems: "center",
                    gap: 5,
                    flexShrink: 1,
                    minWidth: 0,
                    transitionProperty: "colors",
                    transitionDuration: ms(120),
                    transitionTimingFunction: EASE_STANDARD,
                  }}
                >
                  <Icon name="folder" size={14} tone="secondary-foreground" />
                  <Text
                    style={{ fontSize: 12.5, lineHeight: 18, fontWeight: 500, flexShrink: 1, numberOfLines: 1, textOverflow: "ellipsis" }}
                    class="text-foreground"
                  >
                    {pathBase(info().root)}
                  </Text>
                  <Chevron size={13} tone="muted-foreground" />
                </View>
              }
            />
            <If when={info().readOnly}>
              <View
                tooltip={tip("工作区以只读方式打开，写入操作已禁用")}
                class="bg-muted border border-border"
                style={{
                  height: 22,
                  paddingLeft: 6,
                  paddingRight: 6,
                  borderRadius: 5,
                  flexDirection: "row",
                  alignItems: "center",
                  gap: 4,
                  flexShrink: 0,
                }}
              >
                <Icon name="lock" size={11} tone="muted-foreground" />
                <Text style={{ fontSize: 11.5, lineHeight: 16 }} class="text-secondary-foreground">
                  只读
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
              height: 30,
              borderRadius: 8,
              paddingLeft: 10,
              paddingRight: 5,
              flexDirection: "row",
              alignItems: "center",
              gap: 8,
              transitionProperty: "colors",
              transitionDuration: ms(120),
              transitionTimingFunction: EASE_STANDARD,
            }}
          >
            <Icon name="search" size={14} tone="muted-foreground" />
            <Text style={{ flex: 1, fontSize: 12.5, lineHeight: 18 }} class="text-muted-foreground">
              跳转或执行操作
            </Text>
            <Kbd keys="mod+k" faint />
          </View>
        }
      />
      <View style={{ flex: 1, flexDirection: "row" }}>
        <NavRail space={space()} onChange={goSpace} />
        <SpaceStage visible={space() === "problems"} variant="space">
          <LibraryView active={!inStudio() && space() === "problems"} />
        </SpaceStage>
        <SpaceStage visible={space() === "contests"} variant="space">
          <ContestsView active={!inStudio() && space() === "contests"} />
        </SpaceStage>
        <SpaceStage visible={space() === "settings"} variant="space">
          <SettingsView />
        </SpaceStage>
      </View>
    </SpaceStage>
  );

  return (
    <View style={{ flex: 1 }} class="bg-background">
      <If
        when={hasWorkspace()}
        else={
          <View style={{ flex: 1 }}>
            <TitleBar borderless />
            <NoWorkspaceView version={appVersion} />
          </View>
        }
      >
        <View style={{ flex: 1 }}>
          {shell}
          <If when={studioMounted()}>
            <SpaceStage visible={inStudio()} variant="studio" animateOnMount>
              <StudioView active={inStudio()} />
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
                paddingLeft: 12,
                paddingRight: 10,
                paddingTop: 8,
                paddingBottom: 8,
                borderRadius: 8,
                boxShadow: popoverShadow(),
              }}
            >
              <ErrorDisplay error={openError()} onDismiss={() => openError.set(undefined)} />
            </View>
          </If>
        </View>
        <Dialogs
          onJumpContest={(contestId) => {
            selectedContest.set(contestId);
            goSpace("contests");
          }}
        />
        <UnfreezeDialog />
        <CommandPalette />
      </If>
      <If when={brandReplayOpen()}>
        <BrandReplayOverlay version={appVersion} />
      </If>
      <ShortcutsModal />
      <NewWorkspaceModal open={newWorkspaceModalOpen()} onClose={() => newWorkspaceModalOpen.set(false)} />
    </View>
  );
}

// Register bundled JetBrains Mono faces into the GUI text engine and flip `fontsReady`
// once all faces are installed so mounted `Editor`s re-measure glyph widths with real JetBrains Mono.
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

mount(App);
