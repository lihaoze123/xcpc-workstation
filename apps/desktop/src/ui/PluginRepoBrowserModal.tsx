// Unified Plugin Repository Browser & Source Manager modal.
// Shared by Settings -> Plugins ("从仓库浏览与安装…") and Tools sidebar ("获取更多工具").

import { computed, effect, Scroll, signal, Text, View } from "nsh";
import { Input } from "nsh/base";
import type { PluginRepoConfigJs, PluginSummaryJs, RepoPluginEntryJs } from "xcpc-workstation/native";
import { Icon } from "./Icon";
import { Btn, TruncText } from "./kit";
import { DialogHeader, Modal } from "./Modal";
import {
  notifyPluginsChanged,
  preview,
  refreshPluginsOverview,
} from "./store";
import { MONO } from "./theme";
import { showToast } from "./Toast";
import { formatPluginError } from "./tools/pluginErrors";
import { zhEn } from "./training/locale";

import { FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "./tokens";
const DEFAULT_OFFICIAL_REPO_URL =
  "https://github.com/lihaoze123/xcpc-plugins/releases/download/official-latest/index.json";

interface CatalogEntryItem {
  repoUrl: string;
  repoName: string;
  entry: RepoPluginEntryJs;
}

interface RepoFetchError {
  repoUrl: string;
  repoLabel: string;
  message: string;
}

function isOfficialRepo(repo: PluginRepoConfigJs): boolean {
  const url = repo.url.trim();
  if (url === DEFAULT_OFFICIAL_REPO_URL) return true;
  if (url.includes("lihaoze123/xcpc-plugins")) return true;
  if ((repo.name ?? "").trim() === "XCPC Official Plugins") return true;
  return false;
}

function hasDistinctRepoName(repo: PluginRepoConfigJs): boolean {
  const name = (repo.name ?? "").trim();
  const url = repo.url.trim();
  return name.length > 0 && name !== url;
}

function deduplicateRepoErrors(errors: RepoFetchError[]): RepoFetchError[] {
  const seen = new Set<string>();
  const out: RepoFetchError[] = [];
  for (const err of errors) {
    const key = `${err.repoUrl.trim()}\0${err.message.trim()}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(err);
  }
  return out;
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
  const fetchedRepoNames = signal<Record<string, string>>({});
  const installedMap = signal<Record<string, PluginSummaryJs>>({});
  const items = signal<CatalogEntryItem[]>([]);
  const repoErrors = signal<RepoFetchError[]>([]);
  const loading = signal(false);
  const busyStages = signal<Record<string, string>>({});
  const newRepoUrl = signal("");
  const showRepoMgr = signal(false);
  const pendingRemoveRepo = signal<PluginRepoConfigJs | undefined>(undefined);

  const hasOfficialRepo = computed(() => repos().some(isOfficialRepo));
  const resolvedRepoName = (repo: PluginRepoConfigJs): string | undefined => {
    if (hasDistinctRepoName(repo)) return repo.name!.trim();
    const fetched = fetchedRepoNames()[repo.url.trim()];
    if (fetched && fetched !== repo.url.trim()) return fetched;
    return undefined;
  };
  const bannerErrors = computed(() => {
    const errs = repoErrors();
    if (!showRepoMgr()) return errs;
    const listedUrls = new Set(repos().map((r) => r.url));
    return errs.filter((e) => !e.repoUrl || !listedUrls.has(e.repoUrl));
  });

  const setBusyStage = (id: string, stage: string | undefined) => {
    const next = { ...busyStages() };
    if (stage === undefined) delete next[id];
    else next[id] = stage;
    busyStages.set(next);
  };

  const loadAll = async () => {
    loading.set(true);
    repoErrors.set([]);
    try {
      const overview = refreshPluginsOverview(false);
      const repoList = overview.repos ?? [];
      repos.set(repoList);
      const map: Record<string, PluginSummaryJs> = {};
      for (const p of overview.plugins ?? []) {
        map[p.id] = p;
      }
      installedMap.set(map);

      if (repoList.length === 0) {
        items.set([]);
        return;
      }

      if (typeof preview.fetchAllPluginRepoCatalogs === "function") {
        const catalog = await preview.fetchAllPluginRepoCatalogs();
        const collected: CatalogEntryItem[] = [];
        const namesPatch: Record<string, string> = { ...fetchedRepoNames() };
        for (const item of catalog.plugins ?? []) {
          if (item.repoName && item.repoName.trim() && item.repoName.trim() !== item.repoUrl.trim()) {
            namesPatch[item.repoUrl.trim()] = item.repoName.trim();
          }
          if (props.filterCapability) {
            const caps = item.entry.capabilities ?? [];
            if (caps.length > 0 && !caps.includes(props.filterCapability)) continue;
          }
          collected.push({
            repoUrl: item.repoUrl,
            repoName: item.repoName || item.repoUrl,
            entry: item.entry,
          });
        }
        fetchedRepoNames.set(namesPatch);
        const errors: RepoFetchError[] = (catalog.errors ?? []).map((e) => ({
          repoUrl: e.repoUrl,
          repoLabel: e.repoName && e.repoName.trim() && e.repoName.trim() !== e.repoUrl.trim()
            ? e.repoName.trim()
            : e.repoUrl,
          message: formatPluginError(e.error, zhEn("无法连接仓库源", "Unable to reach repository")),
        }));
        if (!catalog.ok && catalog.error && errors.length === 0) {
          errors.push({
            repoUrl: "",
            repoLabel: zhEn("插件仓库", "Plugin repository"),
            message: formatPluginError(catalog.error),
          });
        }
        items.set(collected);
        repoErrors.set(deduplicateRepoErrors(errors));
        return;
      }

      if (typeof preview.fetchPluginRepoCatalog !== "function") {
        items.set([]);
        return;
      }

      const seen = new Set<string>();
      const collected: CatalogEntryItem[] = [];
      const errors: RepoFetchError[] = [];
      const namesPatch: Record<string, string> = { ...fetchedRepoNames() };

      for (const repo of repoList) {
        try {
          const res = await preview.fetchPluginRepoCatalog(repo.url);
          if (!res?.ok) {
            errors.push({
              repoUrl: repo.url,
              repoLabel: hasDistinctRepoName(repo) ? repo.name!.trim() : repo.url,
              message: formatPluginError(
                res?.error,
                zhEn("无法连接仓库源", "Unable to reach repository"),
              ),
            });
            continue;
          }
          if (res.name && res.name.trim() && res.name.trim() !== repo.url.trim()) {
            namesPatch[repo.url.trim()] = res.name.trim();
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
        } catch (err) {
          errors.push({
            repoUrl: repo.url,
            repoLabel: hasDistinctRepoName(repo) ? repo.name!.trim() : repo.url,
            message: formatPluginError(err instanceof Error ? err.message : String(err)),
          });
        }
      }
      fetchedRepoNames.set(namesPatch);
      items.set(collected);
      repoErrors.set(deduplicateRepoErrors(errors));
    } catch (err) {
      repoErrors.set([
        {
          repoUrl: "",
          repoLabel: zhEn("插件仓库", "Plugin repository"),
          message: formatPluginError(err instanceof Error ? err.message : String(err)),
        },
      ]);
    } finally {
      loading.set(false);
    }
  };

  effect(() => {
    if (props.open) {
      void loadAll();
    } else {
      pendingRemoveRepo.set(undefined);
    }
  });

  const handleInstall = async (item: CatalogEntryItem) => {
    const id = item.entry.id;
    if (busyStages()[id]) return;
    setBusyStage(id, zhEn("下载中…", "Downloading…"));
    const stageTimer = setTimeout(() => {
      if (busyStages()[id]) {
        setBusyStage(id, zhEn("安装中…", "Installing…"));
      }
    }, 1400);
    try {
      const res = await preview.installPluginFromRepo(item.repoUrl, id);
      if (res.ok) {
        showToast({
          tone: "success",
          text: zhEn(`已安装「${item.entry.name}」`, `Installed "${item.entry.name}"`),
        });
        notifyPluginsChanged([id]);
        const overview = refreshPluginsOverview(false);
        const map: Record<string, PluginSummaryJs> = {};
        for (const p of overview.plugins ?? []) map[p.id] = p;
        installedMap.set(map);
        // Do not switch the background tool while the modal is still open (avoids jarring background jumps).
      } else {
        showToast({
          tone: "issues",
          text: formatPluginError(res.error, zhEn("安装失败", "Install failed")),
        });
      }
    } catch (err) {
      showToast({
        tone: "issues",
        text: formatPluginError(err instanceof Error ? err.message : String(err)),
      });
    } finally {
      clearTimeout(stageTimer);
      setBusyStage(id, undefined);
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
      showToast({ tone: "issues", text: formatPluginError(res.error) });
    }
  };

  const handleRestoreOfficialRepo = async () => {
    const res =
      typeof preview.restoreOfficialPluginRepo === "function"
        ? preview.restoreOfficialPluginRepo()
        : preview.addPluginRepo(DEFAULT_OFFICIAL_REPO_URL);
    if (res.ok) {
      showToast({ tone: "success", text: zhEn("已恢复官方插件仓库", "Official repository restored") });
      await loadAll();
    } else if (res.error) {
      showToast({ tone: "issues", text: formatPluginError(res.error) });
    }
  };

  const executeRemoveRepo = async (repo: PluginRepoConfigJs) => {
    const res = preview.removePluginRepo(repo.url);
    if (res.ok) {
      showToast({ tone: "info", text: zhEn("已移除插件仓库", "Repository removed") });
      await loadAll();
    } else if (res.error) {
      showToast({ tone: "issues", text: formatPluginError(res.error) });
    }
  };

  return (
    <>
      <Modal
        open={props.open}
        onClose={props.onClose}
        width={560}
        testID="plugin-repo-browser-modal"
      >
        <View style={{ padding: SPACE[16], gap: SPACE[12] }}>
          <View style={{ flexDirection: "row", alignItems: "flex-start", justifyContent: "space-between", gap: SPACE[12] }}>
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
            <View style={{ flexDirection: "row", gap: SPACE[6], flexShrink: 0 }}>
              <Btn
                testID="repo-browser-toggle-sources"
                variant={showRepoMgr() ? "primary" : "outline"}
                size="sm"
                label={zhEn(`仓库源 (${repos().length})`, `Sources (${repos().length})`)}
                onPress={() => showRepoMgr.set(!showRepoMgr())}
              />
              <Btn
                testID="repo-browser-refresh"
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
              testID="repo-manager-panel"
              class="bg-muted/50 border border-border"
              style={{ borderRadius: RADIUS.md, padding: SPACE[12], gap: SPACE[8] }}
            >
              <View style={{ flexDirection: "row", gap: SPACE[6], alignItems: "center" }}>
                <Input
                  testID="repo-url-input"
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
                  testID="repo-add-btn"
                  variant="outline"
                  size="sm"
                  label={zhEn("添加", "Add")}
                  disabled={!newRepoUrl().trim()}
                  onPress={() => void handleAddRepo()}
                />
              </View>

              <If when={!hasOfficialRepo()}>
                <View
                  style={{
                    flexDirection: "row",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: SPACE[8],
                    paddingTop: SPACE[2],
                  }}
                >
                  <Text style={{ fontSize: FONT_SIZE.caption }} class="text-secondary-foreground">
                    {zhEn("官方插件仓库未订阅", "Official plugin repository is not subscribed")}
                  </Text>
                  <Btn
                    testID="repo-restore-official-btn"
                    variant="outline"
                    size="sm"
                    icon="rotateCcw"
                    label={zhEn("恢复官方仓库", "Restore official repo")}
                    onPress={() => void handleRestoreOfficialRepo()}
                  />
                </View>
              </If>

              <If when={repos().length > 0}>
                <View style={{ gap: SPACE[4] }}>
                  <Each of={repos()} key={(r) => r.url}>
                    {(repo) => {
                      const displayName = () => resolvedRepoName(repo());
                      const failedMsg = () =>
                        repoErrors().find((e) => e.repoUrl === repo().url)?.message;
                      return (
                        <View
                          style={{
                            flexDirection: "row",
                            alignItems: "center",
                            justifyContent: "space-between",
                            gap: SPACE[8],
                            paddingTop: SPACE[4],
                            paddingBottom: SPACE[4],
                          }}
                        >
                          <View style={{ flex: 1, minWidth: 0, gap: SPACE[2] }}>
                            <If
                              when={Boolean(displayName())}
                              else={
                                <TruncText
                                  style={{ fontSize: FONT_SIZE.caption, fontFamily: MONO, numberOfLines: 1, textOverflow: "ellipsis" }}
                                  class="text-foreground"
                                >
                                  {repo().url}
                                </TruncText>
                              }
                            >
                              <TruncText
                                style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, numberOfLines: 1, textOverflow: "ellipsis" }}
                                class="text-foreground"
                              >
                                {displayName()}
                              </TruncText>
                              <TruncText
                                style={{ fontSize: FONT_SIZE.micro, fontFamily: MONO, numberOfLines: 1, textOverflow: "ellipsis" }}
                                class="text-muted-foreground"
                              >
                                {repo().url}
                              </TruncText>
                            </If>
                            <If
                              when={
                                repoErrors().find(
                                  (e) => e.repoUrl.trim() === repo().url.trim(),
                                )?.message
                              }
                            >
                              {(msg) => (
                                <TruncText
                                  style={{ fontSize: FONT_SIZE.micro, numberOfLines: 2, textOverflow: "ellipsis" }}
                                  class="text-destructive"
                                >
                                  {msg()}
                                </TruncText>
                              )}
                            </If>
                          </View>
                          <Btn
                            testID={`repo-remove-${repo().url}`}
                            variant="ghost"
                            size="sm"
                            label={zhEn("移除", "Remove")}
                            onPress={() => pendingRemoveRepo.set(repo())}
                          />
                        </View>
                      );
                    }}
                  </Each>
                </View>
              </If>
            </View>
          </If>

          {/* Inline per-repo fetch error banner (deduplicated: hidden when Sources panel already displays per-repo errors) */}
          <If when={!loading() && bannerErrors().length > 0 && items().length > 0}>
            <View
              testID="repo-errors-banner"
              class="bg-muted/60 border border-border"
              style={{ borderRadius: RADIUS.md, padding: SPACE[12], gap: SPACE[4] }}
            >
              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
                <Icon name="triangleAlert" size="sm" tone="destructive" />
                <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                  {zhEn(
                    `有 ${bannerErrors().length} 个仓库拉取失败`,
                    `${bannerErrors().length} repository source(s) failed to load`,
                  )}
                </Text>
              </View>
              <Each of={bannerErrors()} key={(e) => `${e.repoUrl}:${e.message}`}>
                {(err) => (
                  <TruncText
                    style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption, numberOfLines: 2, textOverflow: "ellipsis" }}
                    class="text-secondary-foreground"
                  >
                    {err().repoLabel}：{err().message}
                  </TruncText>
                )}
              </Each>
            </View>
          </If>

          {/* Fixed-height scroll body so switching from loading to list never jumps modal height (Polish 15) */}
          <Scroll style={{ height: 340 }} contentStyle={{ gap: SPACE[8], minHeight: 320 }}>
            <If when={loading() && items().length === 0}>
              <View style={{ flex: 1, minHeight: 300, alignItems: "center", justifyContent: "center" }}>
                <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                  {zhEn("正在获取插件目录…", "Loading plugin catalog…")}
                </Text>
              </View>
            </If>

            <If when={!loading() && items().length === 0}>
              <View style={{ flex: 1, minHeight: 300, padding: SPACE[24], alignItems: "center", justifyContent: "center", gap: SPACE[8] }}>
                <Icon name="circleAlert" size="lg" tone="muted-foreground" />
                <Text style={{ fontSize: FONT_SIZE.caption, textAlign: "center", maxWidth: 380, lineHeight: LINE_HEIGHT.body }} class="text-secondary-foreground">
                  {repoErrors().length > 0
                    ? showRepoMgr()
                      ? zhEn(
                          "无法拉取插件目录，请检查上方仓库源状态后重试。",
                          "Failed to load plugin catalog. Check repository sources above and retry.",
                        )
                      : zhEn(
                          `无法拉取插件目录：${repoErrors()[0]!.message}`,
                          `Failed to load plugin catalog: ${repoErrors()[0]!.message}`,
                        )
                    : repos().length === 0
                      ? zhEn(
                          "当前没有已订阅的插件仓库，可恢复官方仓库或添加自定义仓库。",
                          "No plugin repositories subscribed. Restore the official repository or add a custom source.",
                        )
                      : zhEn(
                          "当前仓库中没有可安装的插件，请稍后刷新重试。",
                          "No installable plugins found in the subscribed repositories.",
                        )}
                </Text>
                <If when={!hasOfficialRepo() && !showRepoMgr()}>
                  <Btn
                    testID="repo-empty-restore-official-btn"
                    variant="outline"
                    size="sm"
                    icon="rotateCcw"
                    label={zhEn("恢复官方仓库", "Restore official repo")}
                    onPress={() => void handleRestoreOfficialRepo()}
                  />
                </If>
              </View>
            </If>

            <Each of={items()} key={(it) => `${it.repoUrl}:${it.entry.id}`}>
              {(item) => {
                const installed = () => installedMap()[item().entry.id];
                const isSameSource = () =>
                  Boolean(installed() && installed()!.sourceRepo === item().repoUrl);
                const isOtherSource = () =>
                  Boolean(installed() && installed()!.sourceRepo !== item().repoUrl);
                const isSameVersion = () =>
                  Boolean(isSameSource() && installed()!.version === item().entry.version);
                const isUpdate = () =>
                  Boolean(isSameSource() && installed()!.version !== item().entry.version);
                const stage = () => busyStages()[item().entry.id];

                return (
                  <View
                    testID={`repo-plugin-row-${item().entry.id}`}
                    class="bg-card border border-border"
                    style={{
                      borderRadius: RADIUS.md,
                      padding: SPACE[12],
                      flexDirection: "row",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: SPACE[12],
                    }}
                  >
                    <View style={{ flex: 1, minWidth: 0, gap: SPACE[2] }}>
                      <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                        {item().entry.name}
                      </Text>
                      <If when={Boolean(item().entry.description)}>
                        <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption }} class="text-secondary-foreground">
                          {item().entry.description}
                        </Text>
                      </If>
                    </View>

                    <If
                      when={isOtherSource()}
                      else={
                        <If
                          when={isSameVersion()}
                          else={
                            <Btn
                              testID={`repo-install-${item().entry.id}`}
                              variant={isUpdate() ? "outline" : "primary"}
                              size="sm"
                              disabled={Boolean(stage())}
                              label={
                                stage() ??
                                (isUpdate()
                                  ? zhEn("更新", "Update")
                                  : zhEn("安装", "Install"))
                              }
                              onPress={() => void handleInstall(item())}
                              style={{ width: 84, flexShrink: 0 }}
                            />
                          }
                        >
                          <View style={{ width: 84, alignItems: "center", flexShrink: 0 }}>
                            <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-muted-foreground">
                              {zhEn("已安装", "Installed")}
                            </Text>
                          </View>
                        </If>
                      }
                    >
                      <View
                        testID={`repo-other-source-${item().entry.id}`}
                        class="bg-muted border border-border"
                        style={{
                          paddingLeft: SPACE[8],
                          paddingRight: SPACE[8],
                          paddingTop: SPACE[4],
                          paddingBottom: SPACE[4],
                          borderRadius: RADIUS.sm,
                          flexShrink: 0,
                        }}
                      >
                        <Text style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }} class="text-muted-foreground">
                          {zhEn("其他来源已安装", "Other source")}
                        </Text>
                      </View>
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

      {/* Confirmation dialog before removing a repository (Awkward 8) */}
      <Modal
        open={pendingRemoveRepo() !== undefined}
        onClose={() => pendingRemoveRepo.set(undefined)}
        width={420}
        testID="repo-remove-confirm-modal"
      >
        <If when={pendingRemoveRepo()}>
          {(repo) => (
            <View style={{ padding: SPACE[20], gap: SPACE[16] }}>
              <DialogHeader
                icon="trash"
                tone="destructive"
                title={zhEn("移除插件仓库？", "Remove plugin repository?")}
                description={
                  isOfficialRepo(repo())
                    ? zhEn(
                        "移除官方插件仓库后将不再显示官方推荐插件，后续可随时一键恢复。",
                        "Removing the official repository hides recommended plugins until you restore it.",
                      )
                    : zhEn(
                        `移除后将不再从此地址拉取插件目录：${repo().url}`,
                        `Plugins will no longer be fetched from: ${repo().url}`,
                      )
                }
              />
              <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: SPACE[8] }}>
                <Btn
                  testID="repo-remove-cancel"
                  variant="outline"
                  size="sm"
                  label={zhEn("取消", "Cancel")}
                  onPress={() => pendingRemoveRepo.set(undefined)}
                />
                <Btn
                  testID="repo-remove-confirm"
                  variant="danger"
                  size="sm"
                  label={zhEn("移除", "Remove")}
                  onPress={() => {
                    const target = repo();
                    pendingRemoveRepo.set(undefined);
                    void executeRemoveRepo(target);
                  }}
                />
              </View>
            </View>
          )}
        </If>
      </Modal>
    </>
  );
}
