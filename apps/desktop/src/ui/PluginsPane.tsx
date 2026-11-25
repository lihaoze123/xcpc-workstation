// Settings -> Plugins management view (Phase 4 UX overhaul):
// Two clean sections — "OJ 账号" (handle input with inline auto-verify/save slot,
// sync controls with Toast feedback) and "工具与扩展" (clean plugin list, single
// "安装插件…" menu, developer actions tucked into ⋯ overflow menus).

import { computed, effect, signal, Text, View } from "nsh";
import { Input, Switch } from "nsh/base";
import type { PluginSummaryJs, PluginsOverviewJs } from "xcpc-workstation/native";
import { t, type MessageKey } from "./i18n";
import { Icon } from "./Icon";
import { Btn, DelayedSpinner, tip } from "./kit";
import { Section, SectionRow } from "./layout";
import { AppMenu, MoreTrigger, type MenuEntry } from "./Menu";
import { DialogHeader, Modal } from "./Modal";
import {
  cachedPluginsOverview,
  globalLastSyncedAt,
  notifyPluginsChanged,
  pluginsRevision,
  preview,
  refresh,
  refreshPluginsOverview,
  setGlobalLastSyncedAt,
} from "./store";
import { MONO } from "./theme";
import { showToast } from "./Toast";
import { zhEn } from "./training/locale";

const INPUT_CLASS =
  "h-8 px-2.5 rounded-md border border-border bg-card text-[12.5px] placeholder:text-muted-foreground focus:ring-1 focus:ring-inset focus:ring-ring transition-colors";

const BUILTIN_DESC_KEYS: Record<string, MessageKey> = {
  codeforces: "settings.plugins.desc.codeforces",
  atcoder: "settings.plugins.desc.atcoder",
  "graph-editor": "settings.plugins.desc.graphEditor",
  "complexity-mem": "settings.plugins.desc.complexityMem",
  "math-bitwise": "settings.plugins.desc.mathBitwise",
  "string-lab": "settings.plugins.desc.stringLab",
};

function pluginDescription(plugin: PluginSummaryJs): string {
  const key = BUILTIN_DESC_KEYS[plugin.id];
  if (key) return t(key);
  return plugin.description.trim();
}

function formatSyncTime(iso?: string | null): string {
  if (!iso) return t("settings.plugins.neverSynced");
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function CredentialRows(props: { plugin: PluginSummaryJs; onReload: () => void }) {
  const p = () => props.plugin;
  const credDrafts = signal<Record<string, string>>({});
  const credSavedKey = signal<string | undefined>(undefined);

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
                  onBlur={() => {
                    if (val().length > 0) saveCredential(key);
                  }}
                />
                <View
                  style={{
                    width: 22,
                    height: 22,
                    alignItems: "center",
                    justifyContent: "center",
                    flexShrink: 0,
                  }}
                >
                  <If when={credSavedKey() === key}>
                    <Icon name="check" size={14} tone="chart-2" />
                  </If>
                </View>
              </View>
            );
          }}
        </Each>
      </View>
    </If>
  );
}

/** Section 1 row: one OJ platform with handle input, inline fixed-width verify status slot, fixed error slot, switch, and ⋯ menu. */
function OjAccountRow(props: {
  plugin: PluginSummaryJs;
  first?: boolean;
  onReload: () => void;
}) {
  const p = () => props.plugin;
  const handleDraft = signal(p().handle ?? "");
  const verifying = signal(false);
  const verifyStatus = signal<{ ok: boolean; text: string } | undefined>(undefined);
  let lastCommitted = (p().handle ?? "").trim();
  let lastFailedDraft: string | undefined;

  // Keep unedited handleDraft in sync when an account is updated externally (e.g. Training onboarding).
  effect(() => {
    const incomingRaw = p().handle ?? "";
    const incoming = incomingRaw.trim();
    if (incoming !== lastCommitted) {
      if (handleDraft().trim() === lastCommitted) {
        handleDraft.set(incomingRaw);
        verifyStatus.set(undefined);
        lastFailedDraft = undefined;
      }
      lastCommitted = incoming;
    }
  });

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
      showToast({ tone: "success", text: t("settings.plugins.exported") });
      props.onReload();
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  const commitAndVerify = async (force = false) => {
    if (verifying()) return;
    const trimmed = handleDraft().trim();
    if (!force && (trimmed === lastCommitted || trimmed === lastFailedDraft)) return;

    if (trimmed.length === 0) {
      const saveRes = preview.setSyncHandle(p().id, "");
      if (!saveRes.ok) {
        if (saveRes.error) showToast({ tone: "issues", text: saveRes.error });
        return;
      }
      lastCommitted = "";
      lastFailedDraft = undefined;
      verifyStatus.set(undefined);
      props.onReload();
      return;
    }

    verifying.set(true);
    try {
      // Verify the candidate handle BEFORE saving to sync_config.json so invalid handles are never persisted.
      const res = await preview.verifyPlugin(p().id, trimmed);
      if (res.ok) {
        const saveRes = preview.setSyncHandle(p().id, trimmed);
        if (!saveRes.ok) {
          if (saveRes.error) showToast({ tone: "issues", text: saveRes.error });
          return;
        }
        lastCommitted = trimmed;
        lastFailedDraft = undefined;
        verifyStatus.set({
          ok: true,
          text: res.message || t("settings.plugins.verifyOk"),
        });
        props.onReload();
      } else {
        lastFailedDraft = trimmed;
        verifyStatus.set({
          ok: false,
          text: res.error || res.message || t("settings.plugins.verifyFailed"),
        });
      }
    } catch (err) {
      lastFailedDraft = trimmed;
      verifyStatus.set({
        ok: false,
        text: err instanceof Error ? err.message : t("settings.plugins.verifyFailed"),
      });
    } finally {
      verifying.set(false);
    }
  };

  const errorText = () => {
    const st = verifyStatus();
    if (st && !st.ok) return st.text;
    if (st && st.ok) return undefined;
    return p().lastError || undefined;
  };

  const moreEntries = (): MenuEntry[] => {
    const items: MenuEntry[] = [];
    if (handleDraft().trim().length > 0) {
      items.push({
        id: "verify",
        label: t("settings.plugins.reverify"),
        icon: "rotateCcw",
        disabled: verifying(),
      });
    }
    if (p().origin === "builtin") {
      items.push({
        id: "export",
        label: t("settings.plugins.exportLocalCopy"),
        icon: "copy",
      });
    }
    return items;
  };

  const desc = () => pluginDescription(p());

  return (
    <View
      style={{
        paddingTop: 12,
        paddingBottom: 8,
        paddingLeft: 14,
        paddingRight: 12,
        gap: 6,
        borderTopWidth: props.first ? 0 : 1,
      }}
      class="border-border"
    >
      <View style={{ flexDirection: "row", alignItems: "flex-start", gap: 12 }}>
        <View style={{ width: 210, minHeight: 32, justifyContent: "center", flexShrink: 0, gap: 2 }}>
          <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
            {p().name}
          </Text>
          <If when={desc().length > 0}>
            <Text
              style={{ fontSize: 11.5, lineHeight: 15, numberOfLines: 1, textOverflow: "ellipsis" }}
              class="text-muted-foreground"
            >
              {desc()}
            </Text>
          </If>
        </View>

        {/* Handle input + fixed-height single-line error slot beneath it (zero vertical shift) */}
        <View style={{ flex: 1, minWidth: 0, gap: 4 }}>
          <View
            style={{
              height: 32,
              borderRadius: 6,
              borderWidth: 1,
              flexDirection: "row",
              alignItems: "center",
              paddingLeft: 10,
              paddingRight: 4,
              gap: 4,
            }}
            class="border-border bg-card focus-within:border-primary focus-within:ring-1 focus-within:ring-inset focus-within:ring-ring transition-colors"
          >
            <Input
              testID={`oj-handle-${p().id}`}
              class="bg-transparent placeholder:text-muted-foreground"
              style={{ flex: 1, height: 28, fontSize: 12.5, fontFamily: MONO }}
              value={handleDraft()}
              placeholder={t("settings.plugins.handlePlaceholder")}
              autoCapitalize="none"
              autoCorrect={false}
              submitBehavior="blurAndSubmit"
              onChange={(e) => {
                handleDraft.set(e.value);
                lastFailedDraft = undefined;
                if (verifyStatus() !== undefined) verifyStatus.set(undefined);
              }}
              onSubmit={() => void commitAndVerify(true)}
              onBlur={() => void commitAndVerify(false)}
            />
            <View
              testID={`oj-status-${p().id}`}
              tooltip={verifyStatus() ? tip(verifyStatus()!.text) : undefined}
              style={{
                width: 22,
                height: 22,
                alignItems: "center",
                justifyContent: "center",
                flexShrink: 0,
              }}
            >
              <If when={verifying()}>
                <DelayedSpinner active={true} size={13} />
              </If>
              <If when={!verifying() && verifyStatus() !== undefined}>
                <Icon
                  name={verifyStatus()?.ok ? "check" : "circleAlert"}
                  size={14}
                  tone={verifyStatus()?.ok ? "chart-2" : "destructive"}
                />
              </If>
            </View>
          </View>
          <View style={{ height: 16, justifyContent: "center" }}>
            <If when={!!errorText()}>
              <Text
                testID={`oj-error-${p().id}`}
                style={{ fontSize: 11.5, lineHeight: 16, numberOfLines: 1, textOverflow: "ellipsis" }}
                class="text-destructive"
                tooltip={tip(errorText()!)}
              >
                {errorText()}
              </Text>
            </If>
          </View>
        </View>

        <View style={{ height: 32, flexDirection: "row", alignItems: "center", gap: 8, flexShrink: 0 }}>
          <Switch
            checked={p().enabled}
            onChange={(e) => toggleEnabled(e.checked)}
            trackClass="w-8 h-[18px] rounded-full bg-input checked:bg-primary transition-colors duration-[120ms]"
            thumbClass="size-[14px] m-[2px] rounded-full bg-white shadow-sm checked:translate-x-[14px] transition-transform duration-[120ms]"
          />
          <If when={moreEntries().length > 0}>
            <AppMenu
              align="end"
              entries={moreEntries()}
              onSelect={(id) => {
                if (id === "verify") void commitAndVerify(true);
                else if (id === "export") exportBuiltin();
              }}
              trigger={<MoreTrigger testID={`oj-more-${p().id}`} />}
            />
          </If>
        </View>
      </View>

      <CredentialRows plugin={p()} onReload={props.onReload} />
    </View>
  );
}

/** Section 2 row: clean tool/extension row with toggle switch and ⋯ overflow menu. */
function ToolPluginRow(props: {
  plugin: PluginSummaryJs;
  first?: boolean;
  onReload: () => void;
}) {
  const p = () => props.plugin;

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
      showToast({ tone: "success", text: t("settings.plugins.exported") });
      props.onReload();
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  const moreEntries = (): MenuEntry[] => {
    const items: MenuEntry[] = [];
    if (p().origin === "builtin") {
      items.push({
        id: "export",
        label: t("settings.plugins.exportLocalCopy"),
        icon: "copy",
      });
    }
    return items;
  };

  const desc = () => pluginDescription(p());

  return (
    <View
      style={{
        paddingTop: 12,
        paddingBottom: 12,
        paddingLeft: 14,
        paddingRight: 12,
        gap: 8,
        borderTopWidth: props.first ? 0 : 1,
      }}
      class="border-border"
    >
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 12 }}>
        <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              {p().name}
            </Text>
            <If when={Boolean(p().isLinked)}>
              <View
                class="bg-muted border border-border"
                style={{ paddingLeft: 5, paddingRight: 5, paddingTop: 1, paddingBottom: 1, borderRadius: 4 }}
                tooltip={p().linkTarget ? tip(p().linkTarget!) : undefined}
              >
                <Text style={{ fontSize: 10.5, fontWeight: 500 }} class="text-secondary-foreground">
                  {t("tools.devLink")}
                </Text>
              </View>
            </If>
          </View>
          <If when={desc().length > 0}>
            <Text style={{ fontSize: 12, lineHeight: 16 }} class="text-muted-foreground">
              {desc()}
            </Text>
          </If>
        </View>

        <View style={{ flexDirection: "row", alignItems: "center", gap: 8, flexShrink: 0 }}>
          <Switch
            checked={p().enabled}
            onChange={(e) => toggleEnabled(e.checked)}
            trackClass="w-8 h-[18px] rounded-full bg-input checked:bg-primary transition-colors duration-[120ms]"
            thumbClass="size-[14px] m-[2px] rounded-full bg-white shadow-sm checked:translate-x-[14px] transition-transform duration-[120ms]"
          />
          <If when={moreEntries().length > 0}>
            <AppMenu
              align="end"
              entries={moreEntries()}
              onSelect={(id) => {
                if (id === "export") exportBuiltin();
              }}
              trigger={<MoreTrigger testID={`plugin-more-${p().id}`} />}
            />
          </If>
        </View>
      </View>

      <CredentialRows plugin={p()} onReload={props.onReload} />
      <If when={!!p().lastError}>
        <Text
          style={{ fontSize: 11.5, lineHeight: 16, numberOfLines: 1, textOverflow: "ellipsis" }}
          class="text-destructive"
          tooltip={tip(p().lastError!)}
        >
          {p().lastError}
        </Text>
      </If>
    </View>
  );
}

export function PluginsPane() {
  const initial = cachedPluginsOverview() ?? refreshPluginsOverview(false);
  if (globalLastSyncedAt() === null && initial.lastSyncedAt) {
    setGlobalLastSyncedAt(initial.lastSyncedAt);
  }
  const overview = signal<PluginsOverviewJs>(initial);
  const syncing = signal(false);
  const pendingTrustPlugin = signal<PluginSummaryJs | undefined>(undefined);

  const syncOverviewFromStore = (reload = false) => {
    const next = refreshPluginsOverview(reload);
    overview.set(next);
    setGlobalLastSyncedAt(next.lastSyncedAt ?? null);
    if (reload) notifyPluginsChanged();
  };

  let lastPluginsRev = pluginsRevision();
  effect(() => {
    const rev = pluginsRevision();
    if (rev === lastPluginsRev) return;
    lastPluginsRev = rev;
    const next = cachedPluginsOverview() ?? refreshPluginsOverview(false);
    overview.set(next);
    setGlobalLastSyncedAt(next.lastSyncedAt ?? null);
  });

  const reloadOverview = () => {
    syncOverviewFromStore(true);
  };

  const ojPlugins = computed(() =>
    overview().plugins.filter((p) => p.capabilities.includes("oj_source")),
  );
  const toolPlugins = computed(() =>
    overview().plugins.filter((p) => !p.capabilities.includes("oj_source")),
  );

  const hasConfiguredAccount = computed(() =>
    ojPlugins().some((p) => p.enabled && (p.handle ?? "").trim().length > 0),
  );

  const effectiveLastSyncedAt = () => globalLastSyncedAt() ?? overview().lastSyncedAt ?? null;

  const runManualSync = async (full = false) => {
    if (syncing() || !hasConfiguredAccount()) return;
    syncing.set(true);
    showToast({
      tone: "info",
      text: full ? t("settings.plugins.fullSyncingToast") : t("settings.plugins.syncingToast"),
      durationMs: 60000,
    });
    try {
      const report = await preview.syncOj(null, full);
      syncOverviewFromStore(false);
      refresh();
      if (!report.ok && report.error) {
        showToast({ tone: "issues", text: report.error });
        return;
      }
      const platforms = report.platforms ?? [];
      if (platforms.length === 0) {
        showToast({ tone: "info", text: t("settings.plugins.fillAccountFirst") });
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
          text:
            subs > 0
              ? t("settings.plugins.syncSuccessCount", { count: subs })
              : t("settings.plugins.syncUpToDate"),
        });
      }
    } catch (err) {
      showToast({
        tone: "issues",
        text: err instanceof Error ? err.message : String(err),
      });
    } finally {
      syncing.set(false);
    }
  };

  const toggleAutoSync = (enabled: boolean) => {
    const res = preview.setAutoSyncOnStartup(enabled);
    if (res.ok) {
      syncOverviewFromStore(false);
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  const installFromDialog = async (kind: "dir" | "zip") => {
    try {
      const res = await preview.installPluginFromDialog(kind, {
        title: kind === "zip" ? t("settings.plugins.installZipTitle") : t("settings.plugins.installDirTitle"),
        buttonLabel: t("settings.plugins.install"),
        filterLabel: kind === "zip" ? t("settings.plugins.zipFilter") : null,
      });
      if (res.cancelled) return;
      if (res.ok) {
        const nextOverview = res.overview ?? refreshPluginsOverview(true);
        const installed = res.pluginId
          ? nextOverview.plugins.find((p) => p.id === res.pluginId)
          : undefined;
        const needsTrustConfirm =
          installed !== undefined &&
          installed.origin !== "builtin" &&
          (installed.hasUnsandboxedUi || installed.domains.length > 0 || installed.workspaceRead);

        if (needsTrustConfirm && installed) {
          // Disable third-party plugin immediately until the user explicitly confirms trust.
          preview.setPluginEnabled(installed.id, false);
          syncOverviewFromStore(false);
          notifyPluginsChanged();
          pendingTrustPlugin.set(installed);
          return;
        }

        if (res.overview) {
          cachedPluginsOverview.set(res.overview);
          overview.set(res.overview);
          setGlobalLastSyncedAt(res.overview.lastSyncedAt ?? null);
          notifyPluginsChanged();
        } else {
          reloadOverview();
        }
        showToast({
          tone: "success",
          text: t("settings.plugins.installSuccess", { id: res.pluginId ?? "" }),
        });
      } else if (res.error) {
        showToast({ tone: "issues", text: res.error });
      }
    } catch (err) {
      showToast({
        tone: "issues",
        text: err instanceof Error ? err.message : String(err),
      });
    }
  };

  const confirmTrustPlugin = () => {
    const plugin = pendingTrustPlugin();
    if (!plugin) return;
    const res = preview.setPluginEnabled(plugin.id, true);
    pendingTrustPlugin.set(undefined);
    syncOverviewFromStore(false);
    notifyPluginsChanged();
    if (res.ok) {
      showToast({
        tone: "success",
        text: t("settings.plugins.installSuccess", { id: plugin.id }),
      });
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  };

  const cancelTrustPlugin = () => {
    if (!pendingTrustPlugin()) return;
    pendingTrustPlugin.set(undefined);
    showToast({
      tone: "info",
      text: zhEn("插件已安装但保持禁用", "Plugin installed and kept disabled"),
    });
  };

  const syncMoreEntries = (): MenuEntry[] => [
    {
      id: "fullSync",
      label: t("settings.plugins.fullResync"),
      icon: "rotateCcw",
      disabled: syncing() || !hasConfiguredAccount(),
    },
  ];

  const installMenuEntries = (): MenuEntry[] => [
    { id: "dir", label: t("settings.plugins.installDir"), icon: "folderPlus" },
    { id: "zip", label: t("settings.plugins.installZip"), icon: "fileDown" },
  ];

  const toolsMoreEntries = (): MenuEntry[] => [
    { id: "reload", label: t("settings.plugins.reloadLocal"), icon: "rotateCcw" },
  ];

  return (
    <View style={{ gap: 22 }}>
      {/* Section 1: OJ Accounts & Sync */}
      <Section title={t("settings.plugins.ojAccountsSection")}>
        <If when={ojPlugins().length === 0}>
          <View style={{ padding: 14 }}>
            <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
              {t("settings.plugins.empty")}
            </Text>
          </View>
        </If>
        <Each of={ojPlugins()} key={(p) => `${p.id}:${p.origin}`}>
          {(plugin) => (
            <OjAccountRow
              plugin={plugin()}
              first={plugin().id === ojPlugins()[0]?.id}
              onReload={() => syncOverviewFromStore(false)}
            />
          )}
        </Each>

        {/* Sync trigger & status row */}
        <SectionRow
          first={ojPlugins().length === 0}
          trailing={
            <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
              <Btn
                testID="plugins-sync-now"
                label={syncing() ? t("settings.plugins.syncing") : t("settings.plugins.syncNow")}
                variant="outline"
                size="sm"
                icon="rotateCcw"
                disabled={syncing() || !hasConfiguredAccount()}
                tooltip={!hasConfiguredAccount() ? tip(t("settings.plugins.fillAccountFirst")) : undefined}
                onPress={() => void runManualSync(false)}
              />
              <AppMenu
                align="end"
                entries={syncMoreEntries()}
                onSelect={(id) => {
                  if (id === "fullSync") void runManualSync(true);
                }}
                trigger={<MoreTrigger testID="plugins-sync-more" />}
              />
            </View>
          }
        >
          <View style={{ gap: 2 }}>
            <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
              {t("settings.plugins.syncNowLabel")}
            </Text>
            <Text style={{ fontSize: 12, lineHeight: 16, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
              {hasConfiguredAccount()
                ? t("settings.plugins.lastSyncedAt", { time: formatSyncTime(effectiveLastSyncedAt()) })
                : t("settings.plugins.fillAccountFirst")}
            </Text>
          </View>
        </SectionRow>

        {/* Auto-sync toggle */}
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
          <Text style={{ fontSize: 13.5, lineHeight: 18, fontWeight: 500 }} class="text-foreground">
            {t("settings.plugins.autoSyncLabel")}
          </Text>
        </SectionRow>
      </Section>

      {/* Section 2: Tools & Extensions */}
      <Section
        title={t("settings.plugins.toolsSection")}
        trailing={
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <AppMenu
              align="end"
              entries={installMenuEntries()}
              onSelect={(id) => {
                if (id === "dir" || id === "zip") void installFromDialog(id);
              }}
              trigger={
                <Btn
                  testID="plugins-install-trigger"
                  label={t("settings.plugins.installPlugin")}
                  size="sm"
                  variant="outline"
                  icon="plus"
                />
              }
            />
            <AppMenu
              align="end"
              entries={toolsMoreEntries()}
              onSelect={(id) => {
                if (id === "reload") reloadOverview();
              }}
              trigger={<MoreTrigger testID="plugins-tools-more" />}
            />
          </View>
        }
      >
        <If when={toolPlugins().length === 0}>
          <View style={{ padding: 14 }}>
            <Text style={{ fontSize: 12.5 }} class="text-muted-foreground">
              {t("settings.plugins.empty")}
            </Text>
          </View>
        </If>
        <Each of={toolPlugins()} key={(p) => `${p.id}:${p.origin}:${p.enabled}:${p.lastError ?? ""}`}>
          {(plugin) => (
            <ToolPluginRow
              plugin={plugin()}
              first={plugin().id === toolPlugins()[0]?.id}
              onReload={reloadOverview}
            />
          )}
        </Each>
      </Section>

      {/* Third-party plugin trust confirmation modal */}
      <Modal
        open={pendingTrustPlugin() !== undefined}
        onClose={cancelTrustPlugin}
        width={460}
        testID="plugin-trust-modal"
      >
        <If when={pendingTrustPlugin()}>
          {(plugin) => (
            <View style={{ padding: 20, gap: 16 }}>
              <DialogHeader
                icon="triangleAlert"
                tone={plugin().hasUnsandboxedUi ? "destructive" : "foreground"}
                title={zhEn(
                  `信任并启用「${plugin().name}」？`,
                  `Trust and enable "${plugin().name}"?`,
                )}
                description={zhEn(
                  "第三方插件在启用前需要你的明确确认，请核对以下权限：",
                  "Review the permissions requested by this third-party plugin before enabling it:",
                )}
              />

              <View style={{ gap: 10 }}>
                <If when={plugin().hasUnsandboxedUi}>
                  <View
                    testID="plugin-trust-unsandboxed-warning"
                    class="bg-destructive/10 border border-destructive/30"
                    style={{ borderRadius: 8, padding: 10, gap: 4 }}
                  >
                    <Text style={{ fontSize: 12.5, fontWeight: 600 }} class="text-destructive">
                      {zhEn("非沙箱桌面界面代码（ui.js）", "Unsandboxed desktop UI code (ui.js)")}
                    </Text>
                    <Text style={{ fontSize: 12, lineHeight: 17 }} class="text-foreground">
                      {zhEn(
                        "此插件包含自定义桌面界面脚本，将在非沙箱环境中运行并可访问桌面应用接口。仅在信任插件来源时启用。",
                        "This plugin includes custom desktop UI code that runs unsandboxed with access to desktop app APIs. Only enable plugins from sources you trust.",
                      )}
                    </Text>
                  </View>
                </If>

                <If when={plugin().domains.length > 0}>
                  <View
                    testID="plugin-trust-domains"
                    class="bg-muted/60 border border-border"
                    style={{ borderRadius: 8, padding: 10, gap: 6 }}
                  >
                    <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-secondary-foreground">
                      {zhEn("请求访问的网络域名", "Requested network domains")}
                    </Text>
                    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
                      <Each of={plugin().domains} key={(d) => d}>
                        {(domain) => (
                          <View
                            class="bg-card border border-border"
                            style={{ borderRadius: 5, paddingLeft: 7, paddingRight: 7, height: 22, justifyContent: "center" }}
                          >
                            <Text style={{ fontSize: 11.5, fontFamily: MONO }} class="text-foreground">
                              {domain()}
                            </Text>
                          </View>
                        )}
                      </Each>
                    </View>
                  </View>
                </If>

                <If when={plugin().workspaceRead}>
                  <View
                    class="bg-muted/60 border border-border"
                    style={{ borderRadius: 8, padding: 10 }}
                  >
                    <Text style={{ fontSize: 12, lineHeight: 17 }} class="text-secondary-foreground">
                      {zhEn("请求只读访问当前工作区文件。", "Requests read-only access to the current workspace.")}
                    </Text>
                  </View>
                </If>
              </View>

              <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
                <Btn
                  testID="plugin-trust-cancel"
                  label={zhEn("取消", "Cancel")}
                  variant="outline"
                  onPress={cancelTrustPlugin}
                />
                <Btn
                  testID="plugin-trust-confirm"
                  label={zhEn("信任并启用", "Trust & Enable")}
                  variant="primary"
                  onPress={confirmTrustPlugin}
                />
              </View>
            </View>
          )}
        </If>
      </Modal>
    </View>
  );
}
