// Settings -> Plugins management view (Phase 4A):
// Header sync & install actions, installed/built-in plugin cards (enable/disable,
// export local copy, OJ handle, masked credentials, verify, error stack), and
// decentralized index.json repository subscriptions with permission preview.

import { batch, signal, Text, View, type JSX } from "nsh";
import { Input, Switch } from "nsh/base";
import type {
  PluginSummaryJs,
  PluginsOverviewJs,
  RepoCatalogResultJs,
  RepoPluginEntryJs,
} from "xcpc-workstation/native";
import { t, type MessageKey } from "./i18n";
import { Icon } from "./Icon";
import { Btn, Chip, createTransientFlag, DelayedSpinner } from "./kit";
import { Section, SectionRow } from "./layout";
import { preview, refresh } from "./store";
import { cardShadow, MONO } from "./theme";
import { showToast } from "./Toast";

const INPUT_CLASS =
  "h-8 px-2.5 rounded-md border border-border bg-card text-[12.5px] placeholder:text-muted-foreground focus:ring-1 focus:ring-inset focus:ring-ring transition-colors";

function Card(props: { children?: JSX.Element }) {
  return (
    <View
      style={{
        borderRadius: 10,
        borderWidth: 1,
        overflow: "hidden",
        boxShadow: cardShadow(),
      }}
      class="bg-card border-border"
    >
      {props.children}
    </View>
  );
}

function originLabel(origin: string): string {
  if (origin === "builtin") return t("settings.plugins.origin.builtin");
  if (origin === "local_override") return t("settings.plugins.origin.localOverride");
  return t("settings.plugins.origin.local");
}

function capabilityLabel(cap: string): string {
  const map: Record<string, MessageKey> = {
    oj_source: "settings.plugins.cap.ojSource",
    tool: "settings.plugins.cap.tool",
    commands: "settings.plugins.cap.commands",
    importer: "settings.plugins.cap.importer",
    exporter: "settings.plugins.cap.exporter",
    credentials: "settings.plugins.cap.credentials",
  };
  const key = map[cap];
  return key ? t(key) : cap;
}

function formatSyncTime(iso?: string | null): string {
  if (!iso) return t("settings.plugins.neverSynced");
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function PluginRow(props: {
  plugin: PluginSummaryJs;
  onReload: () => void;
}) {
  const p = () => props.plugin;
  const handleDraft = signal(p().handle ?? "");
  const handleSaved = createTransientFlag(1600);
  const exportedFlag = createTransientFlag(1800);
  const verifying = signal(false);
  const verifyFeedback = signal<{ ok: boolean; text: string } | undefined>(undefined);
  const errorExpanded = signal(false);
  const credDrafts = signal<Record<string, string>>({});
  const credSavedKey = signal<string | undefined>(undefined);

  const isOj = () => p().capabilities.includes("oj_source");

  const saveHandle = () => {
    const res = preview.setSyncHandle(p().id, handleDraft().trim());
    if (res.ok) {
      handleSaved.trigger();
      props.onReload();
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  const toggleEnabled = (next: boolean) => {
    const res = preview.setPluginEnabled(p().id, next);
    if (res.ok) {
      props.onReload();
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  const exportBuiltin = () => {
    const res = preview.exportBuiltinPlugin(p().id);
    if (res.ok) {
      exportedFlag.trigger();
      props.onReload();
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  const runVerify = async () => {
    if (verifying()) return;
    // Save latest handle draft before verifying so the user doesn't have to blur first.
    if (isOj() && handleDraft().trim() !== (p().handle ?? "")) {
      preview.setSyncHandle(p().id, handleDraft().trim());
    }
    verifying.set(true);
    verifyFeedback.set(undefined);
    try {
      const res = await preview.verifyPlugin(p().id);
      if (res.ok) {
        verifyFeedback.set({
          ok: true,
          text: res.message || t("settings.plugins.verifyOk"),
        });
      } else {
        verifyFeedback.set({
          ok: false,
          text: res.error || res.message || t("settings.plugins.verifyFailed"),
        });
      }
      props.onReload();
    } finally {
      verifying.set(false);
    }
  };

  const saveCredential = (key: string) => {
    const val = credDrafts()[key] ?? "";
    const res = preview.setPluginCredential(p().id, key, val);
    if (res.ok) {
      credDrafts.set({ ...credDrafts(), [key]: "" });
      credSavedKey.set(key);
      setTimeout(() => {
        if (credSavedKey() === key) credSavedKey.set(undefined);
      }, 1600);
      props.onReload();
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  return (
    <View
      style={{
        paddingTop: 14,
        paddingBottom: 14,
        paddingLeft: 16,
        paddingRight: 16,
        gap: 10,
      }}
      class="border-b border-border"
    >
      {/* Top header line: name, version, origin, capabilities, unsandboxed warning, enable switch */}
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 10 }}>
        <View style={{ flex: 1, minWidth: 0, gap: 4 }}>
          <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 6 }}>
            <Text style={{ fontSize: 13.5, fontWeight: 500 }} class="text-foreground">
              {p().name}
            </Text>
            <Text
              style={{ fontSize: 11.5, fontFamily: MONO, fontVariant: ["tabular-nums"] }}
              class="text-muted-foreground"
            >
              {`v${p().version}`}
            </Text>
            <Chip label={originLabel(p().origin)} />
            <Each of={p().capabilities} key={(c) => c}>
              {(cap) => <Chip label={capabilityLabel(cap())} />}
            </Each>
            <If when={p().hasUnsandboxedUi}>
              <View
                style={{
                  flexDirection: "row",
                  alignItems: "center",
                  gap: 4,
                  paddingLeft: 6,
                  paddingRight: 6,
                  height: 20,
                  borderRadius: 5,
                }}
                class="bg-destructive/10"
              >
                <Icon name="triangleAlert" size={11.5} tone="destructive" />
                <Text style={{ fontSize: 11, fontWeight: 500 }} class="text-destructive">
                  {t("settings.plugins.unsandboxedUiWarning")}
                </Text>
              </View>
            </If>
          </View>
          <If when={p().description.trim().length > 0}>
            <Text style={{ fontSize: 12.5, lineHeight: 17 }} class="text-muted-foreground">
              {p().description}
            </Text>
          </If>
          <If when={p().domains.length > 0}>
            <Text
              style={{ fontSize: 11.5, fontFamily: MONO }}
              class="text-muted-foreground"
            >
              {t("settings.plugins.domains", { domains: p().domains.join(", ") })}
            </Text>
          </If>
        </View>

        <View style={{ flexDirection: "row", alignItems: "center", gap: 8, flexShrink: 0 }}>
          <If when={p().origin === "builtin"}>
            <Btn
              label={exportedFlag.active() ? t("settings.plugins.exported") : t("settings.plugins.exportLocalCopy")}
              size="sm"
              variant="ghost"
              icon={exportedFlag.active() ? "check" : "copy"}
              onPress={exportBuiltin}
            />
          </If>
          <Switch
            checked={p().enabled}
            onChange={(e) => toggleEnabled(e.checked)}
            trackClass="w-8 h-[18px] rounded-full bg-input checked:bg-primary transition-colors duration-[120ms]"
            thumbClass="size-[14px] m-[2px] rounded-full bg-white shadow-sm checked:translate-x-[14px] transition-transform duration-[120ms]"
          />
        </View>
      </View>

      {/* OJ handle & verify */}
      <If when={isOj()}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
          <Text style={{ fontSize: 12, width: 72, flexShrink: 0 }} class="text-secondary-foreground">
            {t("settings.plugins.handleLabel")}
          </Text>
          <Input
            class={INPUT_CLASS}
            style={{ flex: 1, fontFamily: MONO }}
            value={handleDraft()}
            placeholder={t("settings.plugins.handlePlaceholder")}
            autoCapitalize="none"
            autoCorrect={false}
            submitBehavior="blurAndSubmit"
            onChange={(e) => handleDraft.set(e.value)}
            onSubmit={saveHandle}
            onBlur={saveHandle}
          />
          <Btn
            label={handleSaved.active() ? t("common.saved") : t("common.save")}
            size="sm"
            variant="ghost"
            onPress={saveHandle}
          />
          <Btn
            label={t("settings.plugins.verify")}
            size="sm"
            variant="outline"
            disabled={verifying()}
            onPress={() => void runVerify()}
          />
          <DelayedSpinner active={verifying()} size={14} />
        </View>
      </If>

      {/* Transient verify feedback */}
      <If when={verifyFeedback() !== undefined}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 6, paddingLeft: 80 }}>
          <Icon
            name={verifyFeedback()?.ok ? "check" : "circleAlert"}
            size={13}
            tone={verifyFeedback()?.ok ? "chart-2" : "destructive"}
          />
          <Text
            style={{ fontSize: 12 }}
            class={verifyFeedback()?.ok ? "text-foreground" : "text-destructive"}
          >
            {verifyFeedback()?.text ?? ""}
          </Text>
        </View>
      </If>

      {/* Declared credentials (masked paste inputs) */}
      <If when={p().credentials.length > 0}>
        <View style={{ gap: 6 }}>
          <Each of={p().credentials} key={(c) => c.name}>
            {(cred) => {
              const key = cred().name;
              const val = () => credDrafts()[key] ?? "";
              return (
                <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                  <Text
                    style={{ fontSize: 12, width: 72, flexShrink: 0, numberOfLines: 1, textOverflow: "ellipsis" }}
                    class="text-secondary-foreground"
                  >
                    {cred().label}
                  </Text>
                  <Input
                    class={INPUT_CLASS}
                    style={{ flex: 1, fontFamily: MONO }}
                    masked={cred().secret}
                    value={val()}
                    placeholder={
                      cred().hasValue
                        ? t("settings.plugins.credentialSetPlaceholder")
                        : cred().placeholder || t("settings.plugins.credentialEmptyPlaceholder")
                    }
                    autoCapitalize="none"
                    autoCorrect={false}
                    submitBehavior="blurAndSubmit"
                    onChange={(e) => credDrafts.set({ ...credDrafts(), [key]: e.value })}
                    onSubmit={() => saveCredential(key)}
                  />
                  <Btn
                    label={credSavedKey() === key ? t("common.saved") : t("common.save")}
                    size="sm"
                    variant="ghost"
                    onPress={() => saveCredential(key)}
                  />
                </View>
              );
            }}
          </Each>
        </View>
      </If>

      {/* Expandable last error stack */}
      <If when={!!p().lastError}>
        <View style={{ gap: 4 }}>
          <View
            onPress={() => errorExpanded.set(!errorExpanded())}
            style={{ flexDirection: "row", alignItems: "center", gap: 6 }}
          >
            <Icon
              name={errorExpanded() ? "chevronDown" : "chevronRight"}
              size={13}
              tone="destructive"
            />
            <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-destructive">
              {t("settings.plugins.lastErrorTitle")}
            </Text>
          </View>
          <If when={errorExpanded()}>
            <View
              style={{
                padding: 10,
                borderRadius: 6,
                borderWidth: 1,
              }}
              class="bg-destructive/5 border-destructive/20"
            >
              <Text style={{ fontSize: 11.5, fontFamily: MONO, lineHeight: 16 }} class="text-destructive">
                {p().lastError ?? ""}
              </Text>
            </View>
          </If>
        </View>
      </If>
    </View>
  );
}

function RepoCatalogCard(props: {
  repoUrl: string;
  catalog: RepoCatalogResultJs;
  installingId: string | undefined;
  onInstall: (entry: RepoPluginEntryJs) => void;
}) {
  const plugins = () => props.catalog.plugins ?? [];
  return (
    <View style={{ gap: 8, paddingTop: 8 }}>
      <If when={!props.catalog.ok}>
        <Text style={{ fontSize: 12 }} class="text-destructive">
          {props.catalog.error || t("settings.plugins.repoFetchFailed")}
        </Text>
      </If>
      <If when={props.catalog.ok && plugins().length === 0}>
        <Text style={{ fontSize: 12 }} class="text-muted-foreground">
          {t("settings.plugins.repoEmpty")}
        </Text>
      </If>
      <If when={props.catalog.ok && plugins().length > 0}>
        <View style={{ gap: 8 }}>
          <Each of={plugins()} key={(e) => e.id}>
            {(entry) => {
              const perms = () => entry().permissions;
              const isInstalling = () => props.installingId === entry().id;
              return (
                <View
                  style={{
                    padding: 10,
                    borderRadius: 8,
                    borderWidth: 1,
                    gap: 6,
                  }}
                  class="bg-muted/40 border-border"
                >
                  <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
                    <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
                      <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                        <Text style={{ fontSize: 13, fontWeight: 500 }} class="text-foreground">
                          {entry().name}
                        </Text>
                        <Text
                          style={{ fontSize: 11.5, fontFamily: MONO, fontVariant: ["tabular-nums"] }}
                          class="text-muted-foreground"
                        >
                          {`v${entry().version}`}
                        </Text>
                        <If when={perms()?.hasUnsandboxedUi}>
                          <View
                            style={{
                              flexDirection: "row",
                              alignItems: "center",
                              gap: 4,
                              paddingLeft: 6,
                              paddingRight: 6,
                              height: 18,
                              borderRadius: 4,
                            }}
                            class="bg-destructive/10"
                          >
                            <Icon name="triangleAlert" size={11} tone="destructive" />
                            <Text style={{ fontSize: 10.5, fontWeight: 500 }} class="text-destructive">
                              {t("settings.plugins.unsandboxedUiWarning")}
                            </Text>
                          </View>
                        </If>
                      </View>
                      <If when={entry().description.trim().length > 0}>
                        <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                          {entry().description}
                        </Text>
                      </If>
                    </View>
                    <Btn
                      label={isInstalling() ? t("settings.plugins.installing") : t("settings.plugins.install")}
                      size="sm"
                      variant="primary"
                      disabled={props.installingId !== undefined}
                      onPress={() => props.onInstall(entry())}
                    />
                  </View>

                  {/* Permission preview before installing */}
                  <If when={perms() !== undefined}>
                    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
                      <If when={(perms()?.domains.length ?? 0) > 0}>
                        <Chip
                          label={t("settings.plugins.permDomains", {
                            domains: perms()!.domains.join(", "),
                          })}
                        />
                      </If>
                      <If when={perms()?.workspaceRead}>
                        <Chip label={t("settings.plugins.permWorkspaceRead")} />
                      </If>
                    </View>
                  </If>
                </View>
              );
            }}
          </Each>
        </View>
      </If>
    </View>
  );
}

export function PluginsPane() {
  const overview = signal<PluginsOverviewJs>(preview.getPluginsOverview());
  const syncing = signal(false);
  const repoInput = signal("");
  const loadingRepoUrl = signal<string | undefined>(undefined);
  const catalogs = signal<Record<string, RepoCatalogResultJs>>({});
  const installingPluginId = signal<string | undefined>(undefined);

  const reloadOverview = () => {
    overview.set(preview.reloadPlugins());
  };

  const runManualSync = async () => {
    if (syncing()) return;
    syncing.set(true);
    try {
      const report = await preview.syncOj(null, false);
      overview.set(preview.getPluginsOverview());
      refresh();
      if (!report.ok && report.error) {
        showToast({ tone: "issues", text: report.error });
        return;
      }
      const platforms = report.platforms ?? [];
      if (platforms.length === 0) {
        showToast({ tone: "info", text: t("settings.plugins.syncNoHandles") });
        return;
      }
      const failed = platforms.filter((p) => !p.ok);
      if (failed.length > 0) {
        showToast({
          tone: "issues",
          text: t("settings.plugins.syncPartialFailure", {
            failed: failed.map((f) => f.platform).join(", "),
          }),
        });
      } else {
        const subs = platforms.reduce((acc, p) => acc + p.submissionsSynced, 0);
        showToast({
          tone: "success",
          text: t("settings.plugins.syncSuccess", {
            platforms: platforms.length,
            submissions: subs,
          }),
        });
      }
    } finally {
      syncing.set(false);
    }
  };

  const toggleAutoSync = (enabled: boolean) => {
    const res = preview.setAutoSyncOnStartup(enabled);
    if (res.ok) {
      overview.set(preview.getPluginsOverview());
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  const installFromDialog = async (kind: "dir" | "zip") => {
    const res = await preview.installPluginFromDialog(kind, {
      title: kind === "zip" ? t("settings.plugins.installZipTitle") : t("settings.plugins.installDirTitle"),
      buttonLabel: t("settings.plugins.install"),
      filterLabel: kind === "zip" ? t("settings.plugins.zipFilter") : null,
    });
    if (res.cancelled) return;
    if (res.ok) {
      if (res.overview) overview.set(res.overview);
      else reloadOverview();
      showToast({
        tone: "success",
        text: t("settings.plugins.installSuccess", { id: res.pluginId ?? "" }),
      });
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  const addRepo = () => {
    const url = repoInput().trim();
    if (!url) return;
    const res = preview.addPluginRepo(url);
    if (res.ok) {
      batch(() => {
        repoInput.set("");
        overview.set(preview.getPluginsOverview());
      });
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  const removeRepo = (url: string) => {
    const res = preview.removePluginRepo(url);
    if (res.ok) {
      const nextCatalogs = { ...catalogs() };
      delete nextCatalogs[url];
      batch(() => {
        catalogs.set(nextCatalogs);
        overview.set(preview.getPluginsOverview());
      });
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  const loadRepoCatalog = async (url: string) => {
    if (loadingRepoUrl() !== undefined) return;
    loadingRepoUrl.set(url);
    try {
      const cat = await preview.fetchPluginRepoCatalog(url);
      catalogs.set({ ...catalogs(), [url]: cat });
      overview.set(preview.getPluginsOverview());
    } finally {
      loadingRepoUrl.set(undefined);
    }
  };

  const installFromRepo = async (repoUrl: string, entry: RepoPluginEntryJs) => {
    if (installingPluginId() !== undefined) return;
    installingPluginId.set(entry.id);
    try {
      const res = await preview.installPluginFromRepo(repoUrl, entry.id);
      if (res.ok) {
        if (res.overview) overview.set(res.overview);
        else reloadOverview();
        showToast({
          tone: "success",
          text: t("settings.plugins.installSuccess", { id: res.pluginId ?? entry.id }),
        });
      } else if (res.error) {
        showToast({ tone: "issues", text: res.error });
      }
    } finally {
      installingPluginId.set(undefined);
    }
  };

  return (
    <View style={{ gap: 20 }}>
      {/* Sync & Management actions */}
      <Section title={t("settings.plugins.syncSection")}>
        <Card>
          <SectionRow
            first
            trailing={
              <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                <DelayedSpinner active={syncing()} size={14} />
                <Btn
                  label={t("settings.plugins.syncNow")}
                  variant="primary"
                  size="sm"
                  icon="rotateCcw"
                  disabled={syncing()}
                  onPress={() => void runManualSync()}
                />
              </View>
            }
          >
            <View style={{ gap: 2 }}>
              <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
                {t("settings.plugins.syncNowLabel")}
              </Text>
              <Text style={{ fontSize: 12, lineHeight: 16, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                {t("settings.plugins.lastSyncedAt", {
                  time: formatSyncTime(overview().lastSyncedAt),
                })}
              </Text>
            </View>
          </SectionRow>
          <SectionRow
            trailing={
              <Switch
                checked={overview().autoSyncOnStartup}
                onChange={(e) => toggleAutoSync(e.checked)}
                trackClass="w-8 h-[18px] rounded-full bg-input checked:bg-primary transition-colors duration-[120ms]"
                thumbClass="size-[14px] m-[2px] rounded-full bg-white shadow-sm checked:translate-x-[14px] transition-transform duration-[120ms]"
              />
            }
          >
            <View style={{ gap: 2 }}>
              <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
                {t("settings.plugins.autoSyncLabel")}
              </Text>
              <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
                {t("settings.plugins.autoSyncHint")}
              </Text>
            </View>
          </SectionRow>
          <SectionRow
            trailing={
              <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                <Btn
                  label={t("settings.plugins.reloadLocal")}
                  size="sm"
                  variant="ghost"
                  icon="rotateCcw"
                  onPress={reloadOverview}
                />
                <Btn
                  label={t("settings.plugins.installDir")}
                  size="sm"
                  variant="outline"
                  icon="folderPlus"
                  onPress={() => void installFromDialog("dir")}
                />
                <Btn
                  label={t("settings.plugins.installZip")}
                  size="sm"
                  variant="outline"
                  icon="fileDown"
                  onPress={() => void installFromDialog("zip")}
                />
              </View>
            }
          >
            <View style={{ gap: 2 }}>
              <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
                {t("settings.plugins.localPluginsLabel")}
              </Text>
              <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
                {t("settings.plugins.localPluginsHint")}
              </Text>
            </View>
          </SectionRow>
        </Card>
      </Section>

      {/* Installed & Built-in plugins */}
      <Section title={t("settings.plugins.installedSection")}>
        <Card>
          <If when={overview().plugins.length === 0}>
            <View style={{ padding: 16 }}>
              <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
                {t("settings.plugins.empty")}
              </Text>
            </View>
          </If>
          <Each of={overview().plugins} key={(p) => `${p.id}:${p.origin}:${p.enabled}:${p.lastError ?? ""}`}>
            {(plugin) => <PluginRow plugin={plugin()} onReload={reloadOverview} />}
          </Each>
        </Card>
      </Section>

      {/* Decentralized plugin repositories */}
      <Section title={t("settings.plugins.reposSection")}>
        <Card>
          <View style={{ padding: 14, gap: 12 }}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
              <Input
                class={INPUT_CLASS}
                style={{ flex: 1, fontFamily: MONO }}
                value={repoInput()}
                placeholder={t("settings.plugins.repoUrlPlaceholder")}
                autoCapitalize="none"
                autoCorrect={false}
                submitBehavior="submit"
                onChange={(e) => repoInput.set(e.value)}
                onSubmit={addRepo}
              />
              <Btn
                label={t("settings.plugins.addRepo")}
                size="sm"
                variant="outline"
                icon="plus"
                disabled={repoInput().trim().length === 0}
                onPress={addRepo}
              />
            </View>

            <If when={overview().repos.length === 0}>
              <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                {t("settings.plugins.reposEmpty")}
              </Text>
            </If>

            <Each of={overview().repos} key={(r) => r.url}>
              {(repo) => {
                const url = repo().url;
                const isLoading = () => loadingRepoUrl() === url;
                const cat = () => catalogs()[url];
                return (
                  <View
                    style={{
                      paddingTop: 10,
                      borderTopWidth: 1,
                      gap: 6,
                    }}
                    class="border-border"
                  >
                    <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
                      <View style={{ flex: 1, minWidth: 0 }}>
                        <If when={!!repo().name}>
                          <Text style={{ fontSize: 13, fontWeight: 500 }} class="text-foreground">
                            {repo().name ?? ""}
                          </Text>
                        </If>
                        <Text
                          style={{
                            fontSize: 12,
                            fontFamily: MONO,
                            numberOfLines: 1,
                            textOverflow: "ellipsis",
                          }}
                          class="text-muted-foreground"
                        >
                          {url}
                        </Text>
                      </View>
                      <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexShrink: 0 }}>
                        <DelayedSpinner active={isLoading()} size={13} />
                        <Btn
                          label={t("settings.plugins.loadCatalog")}
                          size="sm"
                          variant="ghost"
                          disabled={loadingRepoUrl() !== undefined}
                          onPress={() => void loadRepoCatalog(url)}
                        />
                        <Btn
                          label={t("settings.plugins.removeRepo")}
                          size="sm"
                          variant="ghost"
                          onPress={() => removeRepo(url)}
                        />
                      </View>
                    </View>
                    <If when={cat() !== undefined}>
                      <RepoCatalogCard
                        repoUrl={url}
                        catalog={cat()!}
                        installingId={installingPluginId()}
                        onInstall={(entry) => void installFromRepo(url, entry)}
                      />
                    </If>
                  </View>
                );
              }}
            </Each>
          </View>
        </Card>
      </Section>
    </View>
  );
}
