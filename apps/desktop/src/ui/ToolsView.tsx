// Pure Tools space (`工具`, Cmd+4): host contains zero hardcoded business utilities.
// Dynamically lists enabled `tool` plugins and hosts either their pure-Web `ui.html`
// interactive panel (via `<PluginWebView>`) or their headless `index.js` tool form + renderer.

import { batch, computed, effect, Scroll, signal, Text, View } from "nsh";
import type { RepoPluginEntryJs } from "xcpc-workstation/native";
import { t } from "./i18n";
import { Icon } from "./Icon";
import { Btn } from "./kit";
import { PluginRepoBrowserModal } from "./PluginRepoBrowserModal";
import {
  activeToolId,
  activeToolMode,
  activeToolPayload,
  activeToolPluginId,
  goSettings,
  notifyPluginsChanged,
  pluginRevisionFor,
  pluginsRevision,
  preview,
  refreshPluginsOverview,
} from "./store";
import { showToast } from "./Toast";
import { HeadlessToolRunner } from "./tools/HeadlessToolRunner";
import { formatPluginError } from "./tools/pluginErrors";
import { PluginWebView } from "./tools/PluginWebView";
import { parseJsonReport, type PluginToolGroup } from "./tools/types";
import { zhEn } from "./training/locale";

interface RecommendedToolItem {
  repoUrl: string;
  repoName: string;
  entry: RepoPluginEntryJs;
}

function consumeInitialPayload(): unknown {
  const p = activeToolPayload();
  if (p !== undefined) {
    queueMicrotask(() => {
      if (activeToolPayload() === p) {
        activeToolPayload.set(undefined);
      }
    });
  }
  return p;
}

export function ToolsView(props: { active?: boolean }) {
  const toolGroups = computed<PluginToolGroup[]>(() => {
    void pluginsRevision();
    return parseJsonReport<PluginToolGroup[]>(preview.pluginListTools(), []);
  });

  const installedToolIds = computed<Set<string>>(
    () => new Set(toolGroups().map((g) => g.plugin_id)),
  );

  const selectedPluginId = signal<string>("");
  // Keep user on the recommended grid while installing multiple official tools in a row (Awkward 5).
  const stayOnRecommended = signal<boolean>(false);

  // Sync external navigation requests (`openToolPlugin`) and keep selection valid when plugins change.
  effect(() => {
    const groups = toolGroups();
    const requested = activeToolPluginId();
    if (requested && groups.some((g) => g.plugin_id === requested)) {
      batch(() => {
        stayOnRecommended.set(false);
        selectedPluginId.set(requested);
        activeToolPluginId.set(undefined);
        activeToolMode.set(undefined);
      });
      return;
    }

    if (groups.length === 0) {
      selectedPluginId.set("");
      return;
    }

    if (stayOnRecommended()) {
      return;
    }

    const cur = selectedPluginId();
    if (!cur || !groups.some((g) => g.plugin_id === cur)) {
      const first = groups[0];
      selectedPluginId.set(first?.plugin_id ?? "");
    }
  });

  const activeGroup = computed<PluginToolGroup | undefined>(() =>
    stayOnRecommended() ? undefined : toolGroups().find((g) => g.plugin_id === selectedPluginId()),
  );

  const graphEditorEnabled = computed(() =>
    toolGroups().some((g) => g.plugin_id === "graph-editor" && g.has_ui),
  );

  const selectPlugin = (group: PluginToolGroup) => {
    batch(() => {
      stayOnRecommended.set(false);
      selectedPluginId.set(group.plugin_id);
      activeToolId.set(undefined);
      activeToolPayload.set(undefined);
    });
  };

  const openRecommendedTool = (pluginId: string) => {
    batch(() => {
      stayOnRecommended.set(false);
      selectedPluginId.set(pluginId);
      activeToolId.set(undefined);
      activeToolPayload.set(undefined);
    });
  };

  // Empty-state recommended tools from subscribed repositories (including default official repo).
  const recommended = signal<RecommendedToolItem[]>([]);
  const loadingRecommended = signal(false);
  // Per-plugin install stage so multiple cards can be installed independently without graying out others (Awkward 6).
  const installingStages = signal<Record<string, string>>({});
  const repoBrowserOpen = signal(false);
  let lastCatalogRev = -1;

  effect(() => {
    const rev = pluginsRevision();
    const empty = toolGroups().length === 0;
    if (!empty) return;
    if (lastCatalogRev === rev && recommended().length > 0) return;
    lastCatalogRev = rev;

    void (async () => {
      const overview = refreshPluginsOverview(false);
      const repos = overview.repos ?? [];
      if (repos.length === 0 || typeof preview.fetchPluginRepoCatalog !== "function") {
        recommended.set([]);
        return;
      }
      loadingRecommended.set(true);
      try {
        const installedIds = new Set((overview.plugins ?? []).map((p) => p.id));
        const seen = new Set<string>();
        const found: RecommendedToolItem[] = [];
        for (const repo of repos) {
          const catalog = await preview.fetchPluginRepoCatalog(repo.url);
          if (!catalog?.ok) continue;
          for (const entry of catalog.plugins ?? []) {
            if (seen.has(entry.id) || installedIds.has(entry.id)) continue;
            const caps = entry.capabilities ?? [];
            if (caps.length > 0 && !caps.includes("tool")) continue;
            seen.add(entry.id);
            found.push({
              repoUrl: repo.url,
              repoName: catalog.name || repo.name || repo.url,
              entry,
            });
          }
        }
        recommended.set(found);
      } catch {
        recommended.set([]);
      } finally {
        loadingRecommended.set(false);
      }
    })();
  });

  const setInstallStage = (id: string, stage: string | undefined) => {
    const next = { ...installingStages() };
    if (stage === undefined) delete next[id];
    else next[id] = stage;
    installingStages.set(next);
  };

  const performInstall = async (item: RecommendedToolItem) => {
    const id = item.entry.id;
    if (installingStages()[id]) return;
    // Stay on recommended grid after install so the user can install multiple tools in a row.
    stayOnRecommended.set(true);
    setInstallStage(id, zhEn("下载中…", "Downloading…"));
    const stageTimer = setTimeout(() => {
      if (installingStages()[id]) {
        setInstallStage(id, t("tools.installing"));
      }
    }, 1400);
    try {
      const res = await preview.installPluginFromRepo(item.repoUrl, id);
      if (res.ok) {
        showToast({ tone: "success", text: t("tools.installedToast", { name: item.entry.name }) });
        notifyPluginsChanged([id]);
      } else {
        showToast({ tone: "issues", text: formatPluginError(res.error, t("tools.uiLoadFailed")) });
      }
    } catch (err) {
      showToast({
        tone: "issues",
        text: formatPluginError(err instanceof Error ? err.message : String(err)),
      });
    } finally {
      clearTimeout(stageTimer);
      setInstallStage(id, undefined);
    }
  };

  const requestInstall = (item: RecommendedToolItem) => {
    void performInstall(item);
  };

  return (
    <View testID="tools-view" style={{ flex: 1, flexDirection: "row", minHeight: 0 }} class="bg-background">
      {/* Dynamic left plugin list */}
      <View
        style={{ width: 204, flexShrink: 0, borderRightWidth: 1, flexDirection: "column" }}
        class="border-border bg-background"
      >
        <Scroll style={{ flex: 1 }} contentStyle={{ padding: 8, gap: 4 }}>
          <Each of={toolGroups()} key={(g) => g.plugin_id}>
            {(group) => {
              const isSelected = () =>
                !stayOnRecommended() && selectedPluginId() === group().plugin_id;
              return (
                <View
                  testID={`tool-item-${group().plugin_id}`}
                  onPress={() => selectPlugin(group())}
                  class={
                    isSelected()
                      ? "bg-accent"
                      : "hover:bg-muted/70 active:bg-accent/60"
                  }
                  style={{
                    paddingLeft: 10,
                    paddingRight: 10,
                    paddingTop: 8,
                    paddingBottom: 8,
                    borderRadius: 7,
                    gap: 2,
                  }}
                >
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                    <Text
                      style={{
                        fontSize: 13,
                        fontWeight: 500,
                        numberOfLines: 1,
                        textOverflow: "ellipsis",
                        flexShrink: 1,
                      }}
                      class={isSelected() ? "text-foreground" : "text-secondary-foreground"}
                    >
                      {group().plugin_name}
                    </Text>
                    <If when={Boolean(group().is_linked)}>
                      <View
                        class="bg-muted border border-border"
                        style={{ paddingLeft: 4, paddingRight: 4, borderRadius: 4, flexShrink: 0 }}
                      >
                        <Text style={{ fontSize: 10, fontWeight: 500 }} class="text-secondary-foreground">
                          {t("tools.devLink")}
                        </Text>
                      </View>
                    </If>
                  </View>
                  <If when={Boolean(group().plugin_description)}>
                    <Text
                      style={{ fontSize: 11, lineHeight: 15, numberOfLines: 2, textOverflow: "ellipsis" }}
                      class="text-muted-foreground"
                    >
                      {group().plugin_description}
                    </Text>
                  </If>
                </View>
              );
            }}
          </Each>
        </Scroll>
        <View
          class="border-t border-border"
          style={{ padding: 8, flexShrink: 0 }}
        >
          <Btn
            testID="tools-browse-more"
            variant="ghost"
            size="sm"
            icon="plus"
            label={zhEn("获取更多工具", "Get more tools")}
            onPress={() => repoBrowserOpen.set(true)}
            style={{ width: "100%", justifyContent: "flex-start" }}
          />
        </View>
      </View>

      {/* Main plugin container */}
      <View style={{ flex: 1, minWidth: 0, minHeight: 0, flexDirection: "column" }}>
        <If
          when={activeGroup()}
          else={
            <View style={{ flex: 1, minHeight: 0 }}>
              <If
                when={recommended().length > 0}
                else={
                  <View style={{ flex: 1, alignItems: "center", justifyContent: "center", padding: 32, gap: 10 }}>
                    <Icon name="squareCode" size={28} tone="muted-foreground" />
                    <Text style={{ fontSize: 14, fontWeight: 500 }} class="text-foreground">
                      {t("tools.emptyTitle")}
                    </Text>
                    <Text style={{ fontSize: 12.5, textAlign: "center", maxWidth: 360 }} class="text-secondary-foreground">
                      {t("tools.emptyDesc")}
                    </Text>
                    <Btn
                      variant="outline"
                      size="sm"
                      icon="settings"
                      label={t("tools.managePlugins")}
                      onPress={() => goSettings("plugins")}
                    />
                  </View>
                }
              >
                <Scroll style={{ flex: 1 }} contentStyle={{ padding: 28, gap: 16, maxWidth: 720, alignSelf: "center", width: "100%" }}>
                  <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 12 }}>
                    <View style={{ gap: 3 }}>
                      <Text style={{ fontSize: 15, fontWeight: 600 }} class="text-foreground">
                        {t("tools.recommendedTitle")}
                      </Text>
                      <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
                        {t("tools.recommendedDesc")}
                      </Text>
                    </View>
                    <Btn
                      variant="outline"
                      size="sm"
                      icon="settings"
                      label={t("tools.managePlugins")}
                      onPress={() => goSettings("plugins")}
                    />
                  </View>

                  <View style={{ gap: 10 }}>
                    <Each of={recommended()} key={(item) => item.entry.id}>
                      {(item) => {
                        const stage = () => installingStages()[item().entry.id];
                        const isInstalled = () => installedToolIds().has(item().entry.id);
                        return (
                          <View
                            testID={`recommended-tool-${item().entry.id}`}
                            class="bg-card border border-border"
                            style={{
                              borderRadius: 10,
                              padding: 14,
                              flexDirection: "row",
                              alignItems: "center",
                              justifyContent: "space-between",
                              gap: 14,
                            }}
                          >
                            <View style={{ flex: 1, minWidth: 0, gap: 4 }}>
                              <Text style={{ fontSize: 13.5, fontWeight: 600 }} class="text-foreground">
                                {item().entry.name}
                              </Text>
                              <If when={Boolean(item().entry.description)}>
                                <Text style={{ fontSize: 12, lineHeight: 17 }} class="text-secondary-foreground">
                                  {item().entry.description}
                                </Text>
                              </If>
                            </View>
                            <If
                              when={isInstalled()}
                              else={
                                <Btn
                                  testID={`install-recommended-${item().entry.id}`}
                                  variant="primary"
                                  size="sm"
                                  disabled={Boolean(stage())}
                                  label={stage() ?? t("tools.install")}
                                  onPress={() => requestInstall(item())}
                                  style={{ width: 84, flexShrink: 0 }}
                                />
                              }
                            >
                              <Btn
                                testID={`open-recommended-${item().entry.id}`}
                                variant="outline"
                                size="sm"
                                label={zhEn("打开", "Open")}
                                onPress={() => openRecommendedTool(item().entry.id)}
                                style={{ width: 84, flexShrink: 0 }}
                              />
                            </If>
                          </View>
                        );
                      }}
                    </Each>
                  </View>
                </Scroll>
              </If>
            </View>
          }
        >
          {(group) => (
            <View style={{ flex: 1, minHeight: 0, flexDirection: "column" }}>
              {/* Compact plugin header bar: name only, no duplicate description or mode switcher */}
              <View
                class="border-b border-border bg-background"
                style={{
                  height: 44,
                  paddingLeft: 16,
                  paddingRight: 16,
                  flexDirection: "row",
                  alignItems: "center",
                  flexShrink: 0,
                }}
              >
                <Text
                  style={{ fontSize: 13.5, fontWeight: 600, numberOfLines: 1, textOverflow: "ellipsis" }}
                  class="text-foreground"
                >
                  {group().plugin_name}
                </Text>
              </View>

              {/* Render interactive UI (`ui.html`) if available, otherwise headless (`index.js`) runner */}
              <View style={{ flex: 1, minHeight: 0 }}>
                <If
                  when={group().has_ui}
                  else={
                    <Each of={[group()]} key={(g) => g.plugin_id}>
                      {(g) => (
                        <HeadlessToolRunner
                          group={g()}
                          initialToolId={activeToolId()}
                          initialInputs={activeToolPayload()}
                          graphEditorEnabled={graphEditorEnabled()}
                        />
                      )}
                    </Each>
                  }
                >
                  <PluginWebView
                    pluginId={group().plugin_id}
                    revision={pluginRevisionFor(group().plugin_id)}
                    getInitialPayload={consumeInitialPayload}
                  />
                </If>
              </View>
            </View>
          )}
        </If>
      </View>

      <PluginRepoBrowserModal
        open={repoBrowserOpen()}
        onClose={() => repoBrowserOpen.set(false)}
        filterCapability="tool"
        onInstalled={(id) => {
          stayOnRecommended.set(false);
          selectedPluginId.set(id);
        }}
      />
    </View>
  );
}
