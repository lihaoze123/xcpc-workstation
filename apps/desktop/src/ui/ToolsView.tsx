// Pure Tools space (`工具`, Cmd+4): host contains zero hardcoded business utilities.
// Dynamically lists enabled `tool` plugins and hosts either their desktop `ui.js`
// interactive panel (via `PluginUiSdk`) or their headless `index.js` tool form + renderer.

import { computed, effect, Scroll, signal, Text, View } from "nsh";
import { t } from "./i18n";
import { Icon } from "./Icon";
import { Btn } from "./kit";
import {
  activeToolId,
  activeToolMode,
  activeToolPayload,
  activeToolPluginId,
  goSettings,
  pluginsRevision,
  preview,
} from "./store";
import { HeadlessToolRunner } from "./tools/HeadlessToolRunner";
import { createPluginUiSdk, mountPluginUi } from "./tools/pluginSdk";
import { parseJsonReport, type PluginToolGroup } from "./tools/types";

function PluginUiHost(props: { pluginId: string; isBuiltin: boolean }) {
  const mounted = computed(() => {
    const rev = pluginsRevision();
    const id = props.pluginId;
    const res = preview.pluginUiSource(id);
    if (!res.ok || !res.source) {
      return { ok: false as const, key: `${id}:${rev}:err`, error: res.error ?? t("tools.uiLoadFailed") };
    }
    const sdk = createPluginUiSdk({
      pluginId: id,
      getInitialPayload: () => activeToolPayload(),
    });
    const out = mountPluginUi(res.source, sdk, {
      builtinPluginId: props.isBuiltin ? id : undefined,
    });
    if (!out.ok) {
      return { ok: false as const, key: `${id}:${rev}:err`, error: out.error };
    }
    return { ok: true as const, key: `${id}:${rev}:ok`, element: out.element };
  });

  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <If
        when={mounted().ok}
        else={
          <View style={{ flex: 1, alignItems: "center", justifyContent: "center", padding: 24, gap: 8 }}>
            <Icon name="circleAlert" size={22} tone="destructive" />
            <Text style={{ fontSize: 13.5, fontWeight: 500 }} class="text-foreground">
              {t("tools.uiErrorTitle")}
            </Text>
            <Text style={{ fontSize: 12, textAlign: "center", maxWidth: 420 }} class="text-muted-foreground">
              {!mounted().ok ? (mounted() as { ok: false; error: string }).error : ""}
            </Text>
          </View>
        }
      >
        <Each of={mounted().ok ? [mounted() as { ok: true; key: string; element: ReturnType<typeof View> }] : []} key={(item) => item.key}>
          {(item) => item().element}
        </Each>
      </If>
    </View>
  );
}

export function ToolsView(props: { active?: boolean }) {
  const toolGroups = computed<PluginToolGroup[]>(() => {
    void pluginsRevision();
    return parseJsonReport<PluginToolGroup[]>(preview.pluginListTools(), []);
  });

  const selectedPluginId = signal<string>("");
  const viewMode = signal<"ui" | "headless">("ui");

  // Sync external navigation requests (`openToolPlugin`) and keep selection valid when plugins change.
  effect(() => {
    const groups = toolGroups();
    const requested = activeToolPluginId();
    if (requested && groups.some((g) => g.plugin_id === requested)) {
      selectedPluginId.set(requested);
      const target = groups.find((g) => g.plugin_id === requested)!;
      const reqMode = activeToolMode();
      if (reqMode === "headless" && target.tools.length > 0) {
        viewMode.set("headless");
      } else if (reqMode === "ui" && target.has_ui) {
        viewMode.set("ui");
      } else {
        viewMode.set(target.has_ui ? "ui" : "headless");
      }
      activeToolPluginId.set(undefined);
      activeToolMode.set(undefined);
      return;
    }

    const cur = selectedPluginId();
    if (!cur || !groups.some((g) => g.plugin_id === cur)) {
      const first = groups[0];
      selectedPluginId.set(first?.plugin_id ?? "");
      if (first) {
        viewMode.set(first.has_ui ? "ui" : "headless");
      }
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
    viewMode.set(group.has_ui ? "ui" : "headless");
  };

  return (
    <View testID="tools-view" style={{ flex: 1, flexDirection: "row", minHeight: 0 }} class="bg-background">
      {/* Dynamic left plugin list */}
      <View
        style={{ width: 224, flexShrink: 0, borderRightWidth: 1, flexDirection: "column" }}
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
                  <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 6 }}>
                    <Text
                      style={{
                        fontSize: 13,
                        fontWeight: isSelected() ? 600 : 500,
                        flexShrink: 1,
                        numberOfLines: 1,
                        textOverflow: "ellipsis",
                      }}
                      class="text-foreground"
                    >
                      {group().plugin_name}
                    </Text>
                    <If when={group().has_ui}>
                      <View
                        style={{ paddingLeft: 5, paddingRight: 5, height: 16, borderRadius: 4, justifyContent: "center" }}
                        class="bg-muted"
                      >
                        <Text style={{ fontSize: 10 }} class="text-muted-foreground">
                          UI
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
      </View>

      {/* Main plugin container */}
      <View style={{ flex: 1, minWidth: 0, minHeight: 0, flexDirection: "column" }}>
        <If
          when={activeGroup()}
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
          {(group) => (
            <View style={{ flex: 1, minHeight: 0, flexDirection: "column" }}>
              {/* Compact plugin header bar with optional UI / Headless mode switch */}
              <View
                class="border-b border-border bg-background"
                style={{
                  height: 44,
                  paddingLeft: 16,
                  paddingRight: 16,
                  flexDirection: "row",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: 12,
                  flexShrink: 0,
                }}
              >
                <View style={{ flexDirection: "row", alignItems: "center", gap: 8, flexShrink: 1, minWidth: 0 }}>
                  <Text
                    style={{ fontSize: 13.5, fontWeight: 600, numberOfLines: 1, textOverflow: "ellipsis" }}
                    class="text-foreground"
                  >
                    {group().plugin_name}
                  </Text>
                  <Text
                    style={{ fontSize: 12, numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1 }}
                    class="text-muted-foreground"
                  >
                    {group().plugin_description}
                  </Text>
                </View>
                <If when={group().has_ui && group().tools.length > 0}>
                  <View style={{ flexDirection: "row", gap: 4, flexShrink: 0 }}>
                    <Btn
                      testID="tool-mode-ui"
                      size="sm"
                      variant={viewMode() === "ui" ? "primary" : "ghost"}
                      label={t("tools.modeUi")}
                      onPress={() => viewMode.set("ui")}
                    />
                    <Btn
                      testID="tool-mode-headless"
                      size="sm"
                      variant={viewMode() === "headless" ? "primary" : "ghost"}
                      label={t("tools.modeHeadless")}
                      onPress={() => viewMode.set("headless")}
                    />
                  </View>
                </If>
              </View>

              {/* Render either Desktop UI (`ui.js`) or Headless (`index.js`) mode */}
              <View style={{ flex: 1, minHeight: 0 }}>
                <If
                  when={viewMode() === "ui" && group().has_ui}
                  else={
                    <HeadlessToolRunner
                      group={group()}
                      initialToolId={activeToolId()}
                      initialInputs={activeToolPayload()}
                      graphEditorEnabled={graphEditorEnabled()}
                    />
                  }
                >
                  <PluginUiHost pluginId={group().plugin_id} isBuiltin={group().is_builtin} />
                </If>
              </View>
            </View>
          )}
        </If>
      </View>
    </View>
  );
}
