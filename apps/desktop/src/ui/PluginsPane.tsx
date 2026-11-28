// Settings -> Plugins management view (Phase 4 UX overhaul):
// Two clean sections — "OJ 账号" (handle input with inline auto-verify/save slot,
// sync controls with Toast feedback) and "工具与扩展" (clean plugin list, single
// "安装插件…" menu, developer actions tucked into ⋯ overflow menus).

import { computed, effect, signal, Text, View } from "nsh";
import { Input, Switch } from "nsh/base";
import type { PluginSummaryJs, PluginsOverviewJs } from "xcpc-workstation/native";
import { t, type MessageKey } from "./i18n";
import { Icon } from "./Icon";
import { Btn, DelayedSpinner, tip, TruncText } from "./kit";
import { Section, SectionEmpty, SectionRow } from "./layout";
import { AppMenu, MoreTrigger, type MenuEntry } from "./Menu";
import { DialogHeader, Modal } from "./Modal";
import { PluginRepoBrowserModal } from "./PluginRepoBrowserModal";
import { PluginTrustModal } from "./PluginTrustModal";
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
import { formatPluginError } from "./tools/pluginErrors";
import { zhEn } from "./training/locale";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "./tokens";
type ExtendedPluginSummary = PluginSummaryJs & {
  pendingTrust?: boolean;
  overridesBuiltin?: boolean;
  hasNetwork?: boolean;
};

function isOverridesBuiltin(p: ExtendedPluginSummary): boolean {
  return Boolean(p.overridesBuiltin || p.origin === "local_override");
}

function hasNetworkAccess(p: ExtendedPluginSummary): boolean {
  return Boolean(p.hasNetwork || (p.domains && p.domains.length > 0));
}

function isPendingTrust(p: ExtendedPluginSummary): boolean {
  return Boolean(p.pendingTrust);
}

function PluginBadges(props: { plugin: ExtendedPluginSummary; hideBuiltinNetwork?: boolean }) {
  const p = () => props.plugin;
  const showNetworkBadge = () =>
    hasNetworkAccess(p()) && !(props.hideBuiltinNetwork && p().origin === "builtin");
  return (
    <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: SPACE[4] }}>
      <If when={Boolean(p().isLinked)}>
        <View
          testID={`plugin-badge-linked-${p().id}`}
          class="bg-muted border border-border"
          style={{ paddingLeft: SPACE[4], paddingRight: SPACE[4], paddingTop: SPACE[2], paddingBottom: SPACE[2], borderRadius: RADIUS.xs }}
          tooltip={p().linkTarget ? tip(p().linkTarget!) : undefined}
        >
          <Text style={{ fontSize: FONT_SIZE.micro, fontWeight: FONT_WEIGHT.semibold }} class="text-secondary-foreground">
            {t("tools.devLink")}
          </Text>
        </View>
      </If>
      <If when={isOverridesBuiltin(p())}>
        <View
          testID={`plugin-badge-override-${p().id}`}
          class="bg-muted border border-border"
          style={{ paddingLeft: SPACE[4], paddingRight: SPACE[4], paddingTop: SPACE[2], paddingBottom: SPACE[2], borderRadius: RADIUS.xs }}
        >
          <Text style={{ fontSize: FONT_SIZE.micro, fontWeight: FONT_WEIGHT.semibold }} class="text-secondary-foreground">
            {zhEn("覆写内置", "Overrides built-in")}
          </Text>
        </View>
      </If>
      <If when={showNetworkBadge()}>
        <View
          testID={`plugin-badge-network-${p().id}`}
          class="bg-muted border border-border"
          style={{ paddingLeft: SPACE[4], paddingRight: SPACE[4], paddingTop: SPACE[2], paddingBottom: SPACE[2], borderRadius: RADIUS.xs }}
          tooltip={p().domains.length > 0 ? tip(p().domains.join(", ")) : undefined}
        >
          <Text style={{ fontSize: FONT_SIZE.micro, fontWeight: FONT_WEIGHT.semibold }} class="text-secondary-foreground">
            {zhEn("联网", "Network")}
          </Text>
        </View>
      </If>
      <If when={Boolean(p().capabilities?.includes("template"))}>
        <View
          testID={`plugin-badge-template-${p().id}`}
          class="bg-muted border border-border"
          style={{ paddingLeft: SPACE[4], paddingRight: SPACE[4], paddingTop: SPACE[2], paddingBottom: SPACE[2], borderRadius: RADIUS.xs }}
        >
          <Text style={{ fontSize: FONT_SIZE.micro, fontWeight: FONT_WEIGHT.semibold }} class="text-secondary-foreground">
            {zhEn("排版模板", "Template")}
          </Text>
        </View>
      </If>
      <If when={Boolean(p().workspaceWrite)}>
        <View
          testID={`plugin-badge-ws-write-${p().id}`}
          class="bg-muted border border-border"
          style={{ paddingLeft: SPACE[4], paddingRight: SPACE[4], paddingTop: SPACE[2], paddingBottom: SPACE[2], borderRadius: RADIUS.xs }}
        >
          <Text style={{ fontSize: FONT_SIZE.micro, fontWeight: FONT_WEIGHT.semibold }} class="text-secondary-foreground">
            {zhEn("工作区写入", "Workspace write")}
          </Text>
        </View>
      </If>
      <If when={isPendingTrust(p())}>
        <View
          testID={`plugin-badge-untrusted-${p().id}`}
          class="bg-muted border border-destructive/40"
          style={{ paddingLeft: SPACE[4], paddingRight: SPACE[4], paddingTop: SPACE[2], paddingBottom: SPACE[2], borderRadius: RADIUS.xs }}
        >
          <Text style={{ fontSize: FONT_SIZE.micro, fontWeight: FONT_WEIGHT.semibold }} class="text-destructive">
            {zhEn("待信任", "Untrusted")}
          </Text>
        </View>
      </If>
    </View>
  );
}

const INPUT_CLASS =
  "h-8 px-2.5 rounded-md border border-border bg-card text-[12.5px] placeholder:text-muted-foreground focus:ring-1 focus:ring-inset focus:ring-ring transition-colors";

const BUILTIN_DESC_KEYS: Record<string, MessageKey> = {
  codeforces: "settings.plugins.desc.codeforces",
  atcoder: "settings.plugins.desc.atcoder",
};

function pluginDescription(plugin: PluginSummaryJs): string {
  const key = BUILTIN_DESC_KEYS[plugin.id];
  if (key && plugin.origin === "builtin") return t(key);
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
      <View style={{ gap: SPACE[6] }}>
        <Each of={p().credentials} key={(c) => c.name}>
          {(cred) => {
            const key = cred().name;
            const val = () => credDrafts()[key] ?? "";
            return (
              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                <TruncText
                  style={{ fontSize: FONT_SIZE.caption, width: 72, flexShrink: 0, numberOfLines: 1, textOverflow: "ellipsis" }}
                  class="text-secondary-foreground"
                >
                  {cred().label}
                </TruncText>
                <Input
                  class={INPUT_CLASS}
                  style={{ flex: 1 }}
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
                    height: 22, // token-exempt: compact inline icon box
                    alignItems: "center",
                    justifyContent: "center",
                    flexShrink: 0,
                  }}
                >
                  <If when={credSavedKey() === key}>
                    <Icon name="check" size="sm" tone="chart-2" />
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
  plugin: ExtendedPluginSummary;
  first?: boolean;
  onReload: () => void;
  onRequestTrust: (plugin: ExtendedPluginSummary) => void;
  onUnlinkOrRevert: (plugin: ExtendedPluginSummary) => void;
  onRequestUninstall: (plugin: ExtendedPluginSummary) => void;
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
    if (p().isLinked) {
      items.push({
        id: "unlink",
        label: zhEn("取消开发链接", "Unlink dev plugin"),
        icon: "x",
      });
    } else if (isOverridesBuiltin(p())) {
      items.push({
        id: "revert",
        label: zhEn("恢复内置版本", "Restore built-in"),
        icon: "rotateCcw",
      });
    } else if (p().origin === "local") {
      items.push({
        id: "uninstall",
        label: zhEn("卸载", "Uninstall"),
        icon: "trash",
        destructive: true,
      });
    }
    return items;
  };

  const desc = () => pluginDescription(p());

  return (
    <View
      style={{
        paddingTop: SPACE[12],
        paddingBottom: SPACE[8],
        paddingLeft: SPACE[12],
        paddingRight: SPACE[12],
        gap: SPACE[6],
        borderTopWidth: props.first ? 0 : 1,
      }}
      class="border-border"
    >
      <View style={{ flexDirection: "row", alignItems: "flex-start", gap: SPACE[12] }}>
        <View style={{ width: 210, minHeight: 32, justifyContent: "center", flexShrink: 0, gap: SPACE[2] }}>
          <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: SPACE[6] }}>
            <Text style={{ fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
              {p().name}
            </Text>
            <PluginBadges plugin={p()} hideBuiltinNetwork />
          </View>
          <If when={desc().length > 0}>
            <TruncText
              style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption, numberOfLines: 1, textOverflow: "ellipsis" }}
              class="text-muted-foreground"
            >
              {desc()}
            </TruncText>
          </If>
        </View>

        {/* Handle input + fixed-height single-line error slot beneath it (zero vertical shift) */}
        <View style={{ flex: 1, minWidth: 0, gap: SPACE[4] }}>
          <View
            style={{
              height: CONTROL.lg,
              borderRadius: RADIUS.sm,
              borderWidth: 1,
              flexDirection: "row",
              alignItems: "center",
              paddingLeft: SPACE[12],
              paddingRight: SPACE[4],
              gap: SPACE[4],
            }}
            class="border-border bg-card focus-within:border-primary focus-within:ring-1 focus-within:ring-inset focus-within:ring-ring transition-colors"
          >
            <Input
              testID={`oj-handle-${p().id}`}
              class="bg-transparent placeholder:text-muted-foreground"
              style={{ flex: 1, height: CONTROL.md, fontSize: FONT_SIZE.caption }}
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
                height: 22, // token-exempt: compact inline icon box
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
                  size="sm"
                  tone={verifyStatus()?.ok ? "chart-2" : "destructive"}
                />
              </If>
            </View>
          </View>
          <View style={{ height: 16, justifyContent: "center" }}>
            <If when={!!errorText()}>
              <TruncText
                testID={`oj-error-${p().id}`}
                style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption, numberOfLines: 1, textOverflow: "ellipsis" }}
                class="text-destructive"
                tooltip={tip(errorText()!)}
              >
                {errorText()}
              </TruncText>
            </If>
          </View>
        </View>

        <View style={{ height: CONTROL.lg, flexDirection: "row", alignItems: "center", gap: SPACE[8], flexShrink: 0 }}>
          <If
            when={isPendingTrust(p())}
            else={
              <Switch
                checked={p().enabled}
                onChange={(e) => toggleEnabled(e.checked)}
                trackClass="w-8 h-[18px] rounded-full bg-input checked:bg-primary transition-colors duration-[120ms]"
                thumbClass="size-[14px] m-[2px] rounded-full bg-white shadow-sm checked:translate-x-[14px] transition-transform duration-[120ms]"
              />
            }
          >
            <Btn
              testID={`plugin-trust-btn-${p().id}`}
              variant="outline"
              size="sm"
              label={zhEn("确认信任", "Trust")}
              onPress={() => props.onRequestTrust(p())}
            />
          </If>
          <If when={moreEntries().length > 0}>
            <AppMenu
              align="end"
              entries={moreEntries()}
              onSelect={(id) => {
                if (id === "verify") void commitAndVerify(true);
                else if (id === "export") exportBuiltin();
                else if (id === "unlink" || id === "revert") props.onUnlinkOrRevert(p());
                else if (id === "uninstall") props.onRequestUninstall(p());
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

/** Section 2 row: clean tool/extension row with status badges, trust confirmation, toggle switch and ⋯ overflow menu. */
function ToolPluginRow(props: {
  plugin: ExtendedPluginSummary;
  first?: boolean;
  onReload: () => void;
  onRequestTrust: (plugin: ExtendedPluginSummary) => void;
  onUnlinkOrRevert: (plugin: ExtendedPluginSummary) => void;
  onRequestUninstall: (plugin: ExtendedPluginSummary) => void;
}) {
  const p = () => props.plugin;

  const toggleEnabled = (next: boolean) => {
    const res = preview.setPluginEnabled(p().id, next);
    if (res.ok) {
      props.onReload();
    } else if (res.error) {
      showToast({ tone: "issues", text: formatPluginError(res.error) });
    }
  };

  const exportBuiltin = () => {
    const res = preview.exportBuiltinPlugin(p().id);
    if (res.ok) {
      showToast({ tone: "success", text: t("settings.plugins.exported") });
      props.onReload();
    } else if (res.error) {
      showToast({ tone: "issues", text: formatPluginError(res.error) });
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
    if (p().isLinked) {
      items.push({
        id: "unlink",
        label: zhEn("取消开发链接", "Unlink dev plugin"),
        icon: "x",
      });
    } else if (isOverridesBuiltin(p())) {
      items.push({
        id: "revert",
        label: zhEn("恢复内置版本", "Restore built-in"),
        icon: "rotateCcw",
      });
    } else if (p().origin === "local") {
      items.push({
        id: "uninstall",
        label: zhEn("卸载", "Uninstall"),
        icon: "trash",
        destructive: true,
      });
    }
    return items;
  };

  const desc = () => pluginDescription(p());

  return (
    <View
      style={{
        paddingTop: SPACE[12],
        paddingBottom: SPACE[12],
        paddingLeft: SPACE[12],
        paddingRight: SPACE[12],
        gap: SPACE[8],
        borderTopWidth: props.first ? 0 : 1,
      }}
      class="border-border"
    >
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[12] }}>
        <View style={{ flex: 1, minWidth: 0, gap: SPACE[2] }}>
          <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: SPACE[6] }}>
            <Text style={{ fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
              {p().name}
            </Text>
            <PluginBadges plugin={p()} />
          </View>
          <If when={desc().length > 0}>
            <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption }} class="text-muted-foreground">
              {desc()}
            </Text>
          </If>
        </View>

        <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8], flexShrink: 0 }}>
          <If when={!!p().lastError}>
            <View tooltip={tip(p().lastError!)}>
              <Icon name="triangleAlert" size="sm" tone="destructive" />
            </View>
          </If>
          <If
            when={isPendingTrust(p())}
            else={
              <Switch
                checked={p().enabled}
                onChange={(e) => toggleEnabled(e.checked)}
                trackClass="w-8 h-[18px] rounded-full bg-input checked:bg-primary transition-colors duration-[120ms]"
                thumbClass="size-[14px] m-[2px] rounded-full bg-white shadow-sm checked:translate-x-[14px] transition-transform duration-[120ms]"
              />
            }
          >
            <Btn
              testID={`plugin-trust-btn-${p().id}`}
              variant="outline"
              size="sm"
              label={zhEn("确认信任", "Trust")}
              onPress={() => props.onRequestTrust(p())}
            />
          </If>
          <If when={moreEntries().length > 0}>
            <AppMenu
              align="end"
              entries={moreEntries()}
              onSelect={(id) => {
                if (id === "export") exportBuiltin();
                else if (id === "unlink" || id === "revert") props.onUnlinkOrRevert(p());
                else if (id === "uninstall") props.onRequestUninstall(p());
              }}
              trigger={<MoreTrigger testID={`plugin-more-${p().id}`} />}
            />
          </If>
        </View>
      </View>

      <CredentialRows plugin={p()} onReload={props.onReload} />
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
  const pendingTrustPlugin = signal<ExtendedPluginSummary | undefined>(undefined);
  const pendingUninstall = signal<ExtendedPluginSummary | undefined>(undefined);
  const repoBrowserOpen = signal(false);

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
        cachedPluginsOverview.set(nextOverview);
        overview.set(nextOverview);
        setGlobalLastSyncedAt(nextOverview.lastSyncedAt ?? null);
        notifyPluginsChanged(res.pluginId ? [res.pluginId] : []);

        const installed = res.pluginId
          ? (nextOverview.plugins.find((p) => p.id === res.pluginId) as ExtendedPluginSummary | undefined)
          : undefined;

        if (installed && isPendingTrust(installed)) {
          pendingTrustPlugin.set(installed);
          return;
        }

        showToast({
          tone: "success",
          text: t("settings.plugins.installSuccess", { id: res.pluginId ?? "" }),
        });
      } else if (res.error) {
        showToast({ tone: "issues", text: formatPluginError(res.error) });
      }
    } catch (err) {
      showToast({
        tone: "issues",
        text: formatPluginError(err instanceof Error ? err.message : String(err)),
      });
    }
  };

  const confirmTrustPlugin = () => {
    const plugin = pendingTrustPlugin();
    if (!plugin) return;
    const res = preview.trustPlugin(plugin.id);
    pendingTrustPlugin.set(undefined);
    syncOverviewFromStore(false);
    notifyPluginsChanged([plugin.id]);
    if (res.ok) {
      showToast({
        tone: "success",
        text: zhEn(`已信任并启用「${plugin.name}」`, `Trusted and enabled "${plugin.name}"`),
      });
    } else if (res.error) {
      showToast({ tone: "issues", text: formatPluginError(res.error) });
    }
  };

  const cancelTrustPlugin = () => {
    if (!pendingTrustPlugin()) return;
    pendingTrustPlugin.set(undefined);
    showToast({
      tone: "info",
      text: zhEn("插件已安装，待确认信任后可用", "Plugin installed; awaiting trust confirmation"),
    });
  };

  const executeUninstall = (plugin: ExtendedPluginSummary) => {
    const res = preview.uninstallPlugin(plugin.id);
    if (res.ok) {
      if (plugin.isLinked) {
        showToast({
          tone: "info",
          text: zhEn(`已取消开发链接「${plugin.name}」`, `Unlinked "${plugin.name}"`),
        });
      } else if (isOverridesBuiltin(plugin)) {
        showToast({
          tone: "success",
          text: zhEn(`已恢复内置版本「${plugin.name}」`, `Restored built-in "${plugin.name}"`),
        });
      } else {
        showToast({
          tone: "info",
          text: zhEn(`已卸载「${plugin.name}」`, `Uninstalled "${plugin.name}"`),
        });
      }
      reloadOverview();
    } else if (res.error) {
      showToast({ tone: "issues", text: formatPluginError(res.error) });
    }
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
    { id: "repo", label: zhEn("从仓库浏览与安装…", "Browse repositories…"), icon: "search" },
    { id: "dir", label: t("settings.plugins.installDir"), icon: "folderPlus" },
    { id: "zip", label: t("settings.plugins.installZip"), icon: "fileDown" },
  ];

  return (
    <View style={{ gap: SPACE[24] }}>
      {/* Section 1: OJ Accounts & Sync */}
      <Section title={t("settings.plugins.ojAccountsSection")}>
        <If when={ojPlugins().length === 0}>
          <SectionEmpty message={t("settings.plugins.empty")} />
        </If>
        <Each
          of={ojPlugins() as ExtendedPluginSummary[]}
          key={(p) => `${p.id}:${p.origin}:${Boolean(p.pendingTrust)}`}
        >
          {(plugin) => (
            <OjAccountRow
              plugin={plugin()}
              first={plugin().id === ojPlugins()[0]?.id}
              onReload={() => syncOverviewFromStore(false)}
              onRequestTrust={(p) => pendingTrustPlugin.set(p)}
              onUnlinkOrRevert={executeUninstall}
              onRequestUninstall={(p) => pendingUninstall.set(p)}
            />
          )}
        </Each>

        {/* Sync trigger & status row */}
        <SectionRow
          first={ojPlugins().length === 0}
          trailing={
            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
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
          <View style={{ gap: SPACE[2] }}>
            <Text style={{ fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
              {t("settings.plugins.syncNowLabel")}
            </Text>
            <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
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
          <Text style={{ fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
            {t("settings.plugins.autoSyncLabel")}
          </Text>
        </SectionRow>
      </Section>

      {/* Section 2: Tools & Extensions */}
      <Section
        title={t("settings.plugins.toolsSection")}
        trailing={
          <AppMenu
            align="end"
            entries={installMenuEntries()}
            onSelect={(id) => {
              if (id === "repo") repoBrowserOpen.set(true);
              else if (id === "dir" || id === "zip") void installFromDialog(id);
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
        }
      >
        <If when={toolPlugins().length === 0}>
          <SectionEmpty message={t("settings.plugins.empty")} />
        </If>
        <Each
          of={toolPlugins() as ExtendedPluginSummary[]}
          key={(p) => `${p.id}:${p.origin}:${p.enabled}:${Boolean(p.pendingTrust)}:${p.lastError ?? ""}`}
        >
          {(plugin) => (
            <ToolPluginRow
              plugin={plugin()}
              first={plugin().id === toolPlugins()[0]?.id}
              onReload={reloadOverview}
              onRequestTrust={(p) => pendingTrustPlugin.set(p)}
              onUnlinkOrRevert={executeUninstall}
              onRequestUninstall={(p) => pendingUninstall.set(p)}
            />
          )}
        </Each>
      </Section>

      {/* Third-party plugin trust confirmation modal (only for network domains / workspace read) */}
      <PluginTrustModal
        plugin={pendingTrustPlugin()}
        onConfirm={confirmTrustPlugin}
        onCancel={cancelTrustPlugin}
      />

      {/* Uninstall confirmation modal for locally installed plugins */}
      <Modal
        open={pendingUninstall() !== undefined}
        onClose={() => pendingUninstall.set(undefined)}
        width={420}
        testID="plugin-uninstall-modal"
      >
        <If when={pendingUninstall()}>
          {(plugin) => (
            <View style={{ padding: SPACE[20], gap: SPACE[16] }}>
              <DialogHeader
                icon="trash"
                tone="destructive"
                title={zhEn(`卸载「${plugin().name}」？`, `Uninstall "${plugin().name}"?`)}
                description={zhEn(
                  "将从本地插件目录中移除此插件。你随时可以从仓库或安装包重新安装。",
                  "This removes the plugin from your local plugins directory. You can reinstall it anytime.",
                )}
              />
              <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: SPACE[8] }}>
                <Btn
                  testID="plugin-uninstall-cancel"
                  label={zhEn("取消", "Cancel")}
                  variant="outline"
                  onPress={() => pendingUninstall.set(undefined)}
                />
                <Btn
                  testID="plugin-uninstall-confirm"
                  label={zhEn("卸载", "Uninstall")}
                  variant="danger"
                  onPress={() => {
                    const target = plugin();
                    pendingUninstall.set(undefined);
                    executeUninstall(target);
                  }}
                />
              </View>
            </View>
          )}
        </If>
      </Modal>

      <PluginRepoBrowserModal
        open={repoBrowserOpen()}
        onClose={() => repoBrowserOpen.set(false)}
        onInstalled={(id) => {
          const next = refreshPluginsOverview(false);
          overview.set(next);
          const installed = next.plugins.find((p) => p.id === id) as ExtendedPluginSummary | undefined;
          if (installed && isPendingTrust(installed)) {
            repoBrowserOpen.set(false);
            pendingTrustPlugin.set(installed);
          }
        }}
      />
    </View>
  );
}
