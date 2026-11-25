// Pure Tools space (`工具`, Cmd+4): host contains zero hardcoded business utilities.
// Dynamically lists enabled `tool` plugins and hosts either their desktop `ui.js`
// interactive panel (via Core QuickJS `PluginUiRenderer`) or their headless `index.js` tool form + renderer.

import { computed, effect, Scroll, signal, Text, View } from "nsh";
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
  pluginsRevision,
  preview,
  refreshPluginsOverview,
} from "./store";
import { showToast } from "./Toast";
import { HeadlessToolRunner } from "./tools/HeadlessToolRunner";
import { PluginUiRenderer } from "./tools/PluginUiRenderer";
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

  const selectedPluginId = signal<string>("");

  // Sync external navigation requests (`openToolPlugin`) and keep selection valid when plugins change.
  effect(() => {
    const groups = toolGroups();
    const requested = activeToolPluginId();
    if (requested && groups.some((g) => g.plugin_id === requested)) {
      selectedPluginId.set(requested);
      activeToolPluginId.set(undefined);
      activeToolMode.set(undefined);
      return;
    }

    const cur = selectedPluginId();
    if (!cur || !groups.some((g) => g.plugin_id === cur)) {
      const first = groups[0];
      selectedPluginId.set(first?.plugin_id ?? "");
    }
  });

  const activeGroup = computed<PluginToolGroup | undefined>(() =>
    toolGroups().find((g) => g.plugin_id === selectedPluginId()),
  );

  const graphEditorEnabled = computed(() =>
    toolGroups().some((g) => g.plugin_id === "graph-editor" && g.has_ui),
  );

  const selectPlugin = (group: PluginToolGroup) => {
    selectedPluginId.set(group.plugin_id);
    activeToolId.set(undefined);
    activeToolPayload.set(undefined);
  };

  // Empty-state recommended tools from subscribed repositories (including default official repo).
  const recommended = signal<RecommendedToolItem[]>([]);
  const loadingRecommended = signal(false);
  const installingId = signal<string | undefined>(undefined);
  const repoBrowserOpen = signal(false);
  let lastCatalogRev = -1;

  effect(() => {
    const rev = pluginsRevision();
    const empty = toolGroups().length === 0;
    if (!empty) return;
    if (lastCatalogRev === rev) return;
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

  const performInstall = async (item: RecommendedToolItem) => {
    if (installingId()) return;
    installingId.set(item.entry.id);
    try {
      const res = await preview.installPluginFromRepo(item.repoUrl, item.entry.id);
      if (res.ok) {
        showToast({ tone: "success", text: t("tools.installedToast", { name: item.entry.name }) });
        selectedPluginId.set(item.entry.id);
        notifyPluginsChanged();
      } else {
        showToast({ tone: "issues", text: res.error ?? t("tools.uiLoadFailed") });
      }
    } catch (err) {
      showToast({ tone: "issues", text: err instanceof Error ? err.message : String(err) });
    } finally {
      installingId.set(undefined);
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
              const isSelected = () => selectedPluginId() === group().plugin_id;
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
                        const isBusy = () => installingId() === item().entry.id;
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
                              <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                                <Text style={{ fontSize: 13.5, fontWeight: 600 }} class="text-foreground">
                                  {item().entry.name}
                                </Text>
                                <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
                                  v{item().entry.version}
                                </Text>
                                <If when={Boolean(item().entry.official)}>
                                  <View class="bg-muted border border-border" style={{ paddingLeft: 5, paddingRight: 5, borderRadius: 4 }}>
                                    <Text style={{ fontSize: 10.5, fontWeight: 500 }} class="text-secondary-foreground">
                                      {t("tools.officialBadge")}
                                    </Text>
                                  </View>
                                </If>
                              </View>
                              <If when={Boolean(item().entry.description)}>
                                <Text style={{ fontSize: 12, lineHeight: 17 }} class="text-secondary-foreground">
                                  {item().entry.description}
                                </Text>
                              </If>
                            </View>
                            <Btn
                              testID={`install-recommended-${item().entry.id}`}
                              variant="primary"
                              size="sm"
                              disabled={Boolean(installingId())}
                              label={isBusy() ? t("tools.installing") : t("tools.install")}
                              onPress={() => requestInstall(item())}
                            />
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

              {/* Render interactive UI (`ui.js`) if available, otherwise headless (`index.js`) runner */}
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
                  <PluginUiRenderer
                    pluginId={group().plugin_id}
                    revision={pluginsRevision()}
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
        onInstalled={(id) => selectedPluginId.set(id)}
      />
    </View>
  );
}
