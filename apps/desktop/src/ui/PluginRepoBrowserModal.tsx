// Unified Plugin Repository Browser & Source Manager modal.
// Shared by Settings -> Plugins ("从仓库浏览与安装…") and Tools sidebar ("获取更多工具").

import { effect, Scroll, signal, Text, View } from "nsh";
import { Input } from "nsh/base";
import type { PluginRepoConfigJs, PluginSummaryJs, RepoPluginEntryJs } from "xcpc-workstation/native";
import { Icon } from "./Icon";
import { Btn } from "./kit";
import { DialogHeader, Modal } from "./Modal";
import {
  notifyPluginsChanged,
  preview,
  refreshPluginsOverview,
} from "./store";
import { MONO } from "./theme";
import { showToast } from "./Toast";
import { zhEn } from "./training/locale";

interface CatalogEntryItem {
  repoUrl: string;
  repoName: string;
  entry: RepoPluginEntryJs;
}

export function PluginRepoBrowserModal(props: {
  open: boolean;
  onClose: () => void;
  /** Optional capability filter (e.g. `"tool"` when opened from the Tools sidebar). */
  filterCapability?: string;
  /** Called right after a plugin is installed/updated so callers can switch to it. */
  onInstalled?: (pluginId: string) => void;
}) {
  const repos = signal<PluginRepoConfigJs[]>([]);
  const installedMap = signal<Record<string, PluginSummaryJs>>({});
  const items = signal<CatalogEntryItem[]>([]);
  const loading = signal(false);
  const loadError = signal<string | undefined>(undefined);
  const busyId = signal<string | undefined>(undefined);
  const newRepoUrl = signal("");
  const showRepoMgr = signal(false);

  const loadAll = async () => {
    loading.set(true);
    loadError.set(undefined);
    try {
      const overview = refreshPluginsOverview(false);
      const repoList = overview.repos ?? [];
      repos.set(repoList);
      const map: Record<string, PluginSummaryJs> = {};
      for (const p of overview.plugins ?? []) {
        map[p.id] = p;
      }
      installedMap.set(map);

      if (repoList.length === 0 || typeof preview.fetchPluginRepoCatalog !== "function") {
        items.set([]);
        return;
      }

      const seen = new Set<string>();
      const collected: CatalogEntryItem[] = [];
      let anyError: string | undefined;

      for (const repo of repoList) {
        const res = await preview.fetchPluginRepoCatalog(repo.url);
        if (!res?.ok) {
          anyError = res?.error ?? anyError;
          continue;
        }
        for (const entry of res.plugins ?? []) {
          if (seen.has(entry.id)) continue;
          if (props.filterCapability) {
            const caps = entry.capabilities ?? [];
            if (caps.length > 0 && !caps.includes(props.filterCapability)) continue;
          }
          seen.add(entry.id);
          collected.push({
            repoUrl: repo.url,
            repoName: res.name || repo.name || repo.url,
            entry,
          });
        }
      }
      items.set(collected);
      if (collected.length === 0 && anyError) {
        loadError.set(anyError);
      }
    } catch (err) {
      loadError.set(err instanceof Error ? err.message : String(err));
    } finally {
      loading.set(false);
    }
  };

  effect(() => {
    if (props.open) {
      void loadAll();
    }
  });

  const handleInstall = async (item: CatalogEntryItem) => {
    if (busyId()) return;
    busyId.set(item.entry.id);
    try {
      const res = await preview.installPluginFromRepo(item.repoUrl, item.entry.id);
      if (res.ok) {
        showToast({
          tone: "success",
          text: zhEn(`已安装「${item.entry.name}」`, `Installed "${item.entry.name}"`),
        });
        notifyPluginsChanged();
        const overview = refreshPluginsOverview(false);
        const map: Record<string, PluginSummaryJs> = {};
        for (const p of overview.plugins ?? []) map[p.id] = p;
        installedMap.set(map);
        props.onInstalled?.(item.entry.id);
      } else {
        showToast({ tone: "issues", text: res.error ?? zhEn("安装失败", "Install failed") });
      }
    } catch (err) {
      showToast({ tone: "issues", text: err instanceof Error ? err.message : String(err) });
    } finally {
      busyId.set(undefined);
    }
  };

  const handleAddRepo = async () => {
    const url = newRepoUrl().trim();
    if (!url) return;
    const res = preview.addPluginRepo(url);
    if (res.ok) {
      newRepoUrl.set("");
      showToast({ tone: "success", text: zhEn("已添加插件仓库", "Repository added") });
      await loadAll();
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  const handleRemoveRepo = async (url: string) => {
    const res = preview.removePluginRepo(url);
    if (res.ok) {
      showToast({ tone: "info", text: zhEn("已移除插件仓库", "Repository removed") });
      await loadAll();
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  return (
    <Modal open={props.open} onClose={props.onClose} width={560} testID="plugin-repo-browser-modal">
      <View style={{ padding: 18, gap: 14, maxHeight: 580 }}>
        <View style={{ flexDirection: "row", alignItems: "flex-start", justifyContent: "space-between", gap: 12 }}>
          <View style={{ flex: 1 }}>
            <DialogHeader
              icon="squareCode"
              title={
                props.filterCapability === "tool"
                  ? zhEn("获取工具插件", "Browse Tool Plugins")
                  : zhEn("插件仓库浏览与管理", "Plugin Repositories")
              }
              description={zhEn(
                "插件在独立沙箱中运行，安装后立即可用。",
                "Plugins run in an isolated sandbox and are ready immediately after install.",
              )}
            />
          </View>
          <View style={{ flexDirection: "row", gap: 6 }}>
            <Btn
              testID="repo-browser-toggle-sources"
              variant={showRepoMgr() ? "primary" : "outline"}
              size="sm"
              label={zhEn(`仓库源 (${repos().length})`, `Sources (${repos().length})`)}
              onPress={() => showRepoMgr.set(!showRepoMgr())}
            />
            <Btn
              variant="outline"
              size="sm"
              icon="rotateCcw"
              disabled={loading()}
              onPress={() => void loadAll()}
            />
          </View>
        </View>

        <If when={showRepoMgr()}>
          <View
            class="bg-muted/50 border border-border"
            style={{ borderRadius: 8, padding: 10, gap: 8 }}
          >
            <View style={{ flexDirection: "row", gap: 6, alignItems: "center" }}>
              <Input
                value={newRepoUrl()}
                placeholder="https://.../index.json"
                autoCapitalize="none"
                autoCorrect={false}
                onChange={(e) => newRepoUrl.set(e.value)}
                onSubmit={() => void handleAddRepo()}
                class="h-8 px-2.5 rounded-md border border-border bg-card text-[12px] placeholder:text-muted-foreground"
                style={{ flex: 1, fontFamily: MONO }}
              />
              <Btn
                variant="outline"
                size="sm"
                label={zhEn("添加", "Add")}
                disabled={!newRepoUrl().trim()}
                onPress={() => void handleAddRepo()}
              />
            </View>
            <If when={repos().length > 0}>
              <View style={{ gap: 4 }}>
                <Each of={repos()} key={(r) => r.url}>
                  {(repo) => (
                    <View
                      style={{
                        flexDirection: "row",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 8,
                        paddingTop: 2,
                        paddingBottom: 2,
                      }}
                    >
                      <View style={{ flex: 1, minWidth: 0 }}>
                        <Text
                          style={{ fontSize: 12, fontWeight: 500, numberOfLines: 1, textOverflow: "ellipsis" }}
                          class="text-foreground"
                        >
                          {repo().name || repo().url}
                        </Text>
                        <Text
                          style={{ fontSize: 11, fontFamily: MONO, numberOfLines: 1, textOverflow: "ellipsis" }}
                          class="text-muted-foreground"
                        >
                          {repo().url}
                        </Text>
                      </View>
                      <Btn
                        variant="ghost"
                        size="sm"
                        label={zhEn("移除", "Remove")}
                        onPress={() => void handleRemoveRepo(repo().url)}
                      />
                    </View>
                  )}
                </Each>
              </View>
            </If>
          </View>
        </If>

        <Scroll style={{ maxHeight: 360 }} contentStyle={{ gap: 8 }}>
          <If when={loading() && items().length === 0}>
            <View style={{ padding: 24, alignItems: "center" }}>
              <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
                {zhEn("正在获取插件目录…", "Loading plugin catalog…")}
              </Text>
            </View>
          </If>

          <If when={!loading() && items().length === 0}>
            <View style={{ padding: 24, alignItems: "center", gap: 6 }}>
              <Icon name="circleAlert" size={20} tone="muted-foreground" />
              <Text style={{ fontSize: 12.5 }} class="text-secondary-foreground">
                {loadError()
                  ? zhEn(`无法拉取插件目录：${loadError()}`, `Failed to fetch catalog: ${loadError()}`)
                  : zhEn("暂无可用插件，请检查仓库源配置。", "No plugins available. Check your repository sources.")}
              </Text>
            </View>
          </If>

          <Each of={items()} key={(it) => it.entry.id}>
            {(item) => {
              const installed = () => installedMap()[item().entry.id];
              const isSameVersion = () =>
                Boolean(installed() && installed()!.version === item().entry.version);
              const isUpdate = () =>
                Boolean(installed() && installed()!.version !== item().entry.version);
              const isBusy = () => busyId() === item().entry.id;

              return (
                <View
                  testID={`repo-plugin-row-${item().entry.id}`}
                  class="bg-card border border-border"
                  style={{
                    borderRadius: 9,
                    padding: 12,
                    flexDirection: "row",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 12,
                  }}
                >
                  <View style={{ flex: 1, minWidth: 0, gap: 3 }}>
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
                            {zhEn("官方", "Official")}
                          </Text>
                        </View>
                      </If>
                    </View>
                    <If when={Boolean(item().entry.description)}>
                      <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-secondary-foreground">
                        {item().entry.description}
                      </Text>
                    </If>
                  </View>

                  <If
                    when={isSameVersion()}
                    else={
                      <Btn
                        testID={`repo-install-${item().entry.id}`}
                        variant={isUpdate() ? "outline" : "primary"}
                        size="sm"
                        disabled={Boolean(busyId())}
                        label={
                          isBusy()
                            ? zhEn("安装中…", "Installing…")
                            : isUpdate()
                              ? zhEn("更新", "Update")
                              : zhEn("安装", "Install")
                        }
                        onPress={() => void handleInstall(item())}
                      />
                    }
                  >
                    <Text style={{ fontSize: 12, fontWeight: 500, paddingRight: 6 }} class="text-muted-foreground">
                      {zhEn("已安装", "Installed")}
                    </Text>
                  </If>
                </View>
              );
            }}
          </Each>
        </Scroll>

        <View style={{ flexDirection: "row", justifyContent: "flex-end" }}>
          <Btn variant="outline" size="sm" label={zhEn("完成", "Done")} onPress={props.onClose} />
        </View>
      </View>
    </Modal>
  );
}
