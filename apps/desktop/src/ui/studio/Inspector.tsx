// The publishing drawer on the right of the studio: slides over the preview with a
// `translateX` (never squeezing the editor) and splits into two tabs (`信息` | `版式`):
// - `信息`: freeze status, contest/problem metadata, per-problem limits overrides, and assets.
// - `版式`: template selection, shared publishing switches, and dynamic controls rendered
//   from the active template's `#let schema = (...)` (with override bars & one-click restore),
//   plus lightweight bridges to fork a custom template or open external style files.

import { cleanup, computed, effect, reduceMotion, Scroll, signal, Text, untracked, View, type JSX } from "nsh";
import { Input } from "nsh/base";
import type { ContestDetailJs, ContestItemDetailJs, PreviewDiagnostic } from "xcpc-workstation/native";
import { freezeBlockedCopy, type Freeze } from "../Contest";
import { ErrorDisplay } from "../ErrorDisplay";
import { resolveLocalized, t } from "../i18n";
import { Icon } from "../Icon";
import { Btn, DelayedSpinner, IconBtn, Segmented, tip } from "../kit";
import { Modal } from "../Modal";
import { colorTransition, panelTransition } from "../motion";
import { activeRevision, errorOf, goSettings, info, preview, refresh, unfreezeFor, type UiError } from "../store";
import {
  encodeSingleOptionPatch,
  loadTemplatesCatalog,
  LOCAL_TEMPLATE_NAMESPACE,
  parseContestTemplateInfo,
  parseOptionsByTemplate,
  type TemplateOptionMap,
  type TemplateOptionValue,
  type TemplatesCatalog,
} from "../templates";
import { ForkTemplateModal, InstallPresetModal, TemplateOptionsEditor, TemplatePicker } from "../TemplateOptionsEditor";

export const INSPECTOR_WIDTH = 332;
type InspectorTab = "info" | "layout";

export function Inspector(props: {
  active?: boolean;
  open: boolean;
  topOffset?: number;
  target: "problem" | "contest";
  id: string;
  contest?: ContestDetailJs;
  item?: ContestItemDetailJs;
  freeze: Freeze;
  assetsRefreshSeq?: number;
  hasPackage?: boolean;
  hideBuildEntry?: boolean;
  packageFreshness?: string;
  onInitPackage?: () => void;
  onOpenBuild?: () => void;
  onOpenAssets?: () => void;
  onChanged: () => void;
  onFinding: (d: PreviewDiagnostic) => void;
  onClose: () => void;
}) {
  // Defer mounting the form fields until the drawer is opened at least once so opening
  // Studio with the drawer closed skips mounting 10+ Input controls on frame 0.
  const everOpened = computed<boolean>((prev) => Boolean(prev) || props.open);
  const activeTab = signal<InspectorTab>("info");
  const error = signal<UiError | undefined>(undefined);
  const assetCount = signal(0);
  const forkModal = signal<{ sourceId: string; sourceName: string } | undefined>(undefined);
  const presetModalOpen = signal(false);
  let assetCountSeq = 0;

  effect(() => {
    void props.target;
    void props.id;
    untracked(() => error.set(undefined));
  });

  const isActive = () => (props.active ?? true) && props.open;
  const rev = activeRevision(isActive);

  const showAssets = () => props.target === "problem" || (props.target === "contest" && !!props.item);

  const catalog = computed<TemplatesCatalog>(() => {
    void rev();
    if (!everOpened()) {
      return { default: "olymp-classic", workspaceOptions: {}, templates: [], presets: [], fonts: [] };
    }
    return loadTemplatesCatalog();
  });

  effect(() => {
    if (!showAssets()) {
      assetCount.set(0);
      return;
    }
    if (!isActive()) return;
    void rev();
    void props.assetsRefreshSeq;
    const target = props.target;
    const id = props.id;
    const label = target === "contest" ? (props.item?.label ?? null) : null;
    const seq = ++assetCountSeq;
    void preview
      .listProblemAssets(target, id, label)
      .then((list) => {
        if (seq === assetCountSeq) assetCount.set(list.length);
      })
      .catch(() => {
        if (seq === assetCountSeq) assetCount.set(0);
      });
  });

  const done = (res: { ok: boolean; code?: string | null; error?: string | null }) => {
    error.set(errorOf(res));
    refresh();
    props.onChanged();
  };

  // Hide closed drawer with `display: "none"` after the slide-out spring settles so
  // off-screen `<Input>` controls never leak into Studio's Tab focus chain.
  const displayed = signal(props.open);
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  effect(() => {
    const isOpen = props.open;
    if (hideTimer !== undefined) {
      clearTimeout(hideTimer);
      hideTimer = undefined;
    }
    if (isOpen) {
      displayed.set(true);
    } else if (reduceMotion()) {
      displayed.set(false);
    } else {
      hideTimer = setTimeout(() => {
        hideTimer = undefined;
        displayed.set(false);
      }, 240);
    }
  });
  cleanup(() => {
    if (hideTimer !== undefined) clearTimeout(hideTimer);
  });

  return (
    <View
      testID="inspector"
      style={{
        position: "absolute",
        top: props.topOffset ?? 0,
        right: 0,
        bottom: 0,
        width: INSPECTOR_WIDTH,
        borderLeftWidth: 1,
        display: displayed() ? "flex" : "none",
        flexDirection: "column",
        opacity: props.open ? 1 : 0,
        transform: props.open ? "translateX(0px)" : `translateX(${INSPECTOR_WIDTH + 1}px)`,
        ...panelTransition(),
        pointerEvents: props.open ? "auto" : "none",
      }}
      class="border-border bg-background"
    >
      <View
        style={{
          height: 42,
          flexDirection: "row",
          alignItems: "center",
          paddingLeft: 12,
          paddingRight: 8,
          gap: 8,
          borderBottomWidth: 1,
        }}
        class="border-border"
      >
        <Segmented
          items={[
            { value: "info" as InspectorTab, label: t("inspector.tab.info") },
            { value: "layout" as InspectorTab, label: t("inspector.tab.layout") },
          ]}
          itemWidth={62}
          value={activeTab()}
          onChange={(v) => activeTab.set(v)}
        />
        <View style={{ flex: 1 }} />
        <IconBtn icon="x" tooltip={tip(t("common.close"), "mod+i")} onPress={props.onClose} />
      </View>

      <If when={everOpened()}>
        <Scroll style={{ flex: 1 }}>
          <If when={activeTab() === "info"}>
            <If when={props.target === "contest" && props.contest}>
              <ContestInfoTab
                contest={props.contest!}
                item={props.item}
                freeze={props.freeze}
                done={done}
                onFinding={props.onFinding}
                hasFollowingSection={showAssets()}
              />
            </If>
            <If when={props.target === "problem"}>
              <If when={(props.hasPackage && !props.hideBuildEntry) || (!props.hasPackage && !info().readOnly)}>
                <Section>
                  <If
                    when={props.hasPackage}
                    else={
                      <View
                        testID="inspector-init-package-btn"
                        onPress={() => props.onInitPackage?.()}
                        class="border border-border bg-card hover:bg-muted active:bg-accent"
                        style={{
                          paddingLeft: 10,
                          paddingRight: 8,
                          paddingTop: 8,
                          paddingBottom: 8,
                          borderRadius: 7,
                          flexDirection: "row",
                          alignItems: "center",
                          gap: 8,
                          ...colorTransition(140),
                        }}
                      >
                        <Icon name="code" size={15} tone="secondary-foreground" />
                        <View style={{ flex: 1, minWidth: 0, gap: 1 }}>
                          <Text style={{ fontSize: 12.5, fontWeight: 500 }} class="text-foreground">
                            {t("inspector.package.init")}
                          </Text>
                          <Text style={{ fontSize: 11.5, lineHeight: 15 }} class="text-muted-foreground">
                            {t("inspector.package.initHint")}
                          </Text>
                        </View>
                        <Icon name="plus" size={14} tone="muted-foreground" />
                      </View>
                    }
                  >
                    <View
                      testID="inspector-open-build-btn"
                      onPress={() => props.onOpenBuild?.()}
                      class="border border-border bg-card hover:bg-muted active:bg-accent"
                      style={{
                        height: 34,
                        paddingLeft: 10,
                        paddingRight: 8,
                        borderRadius: 7,
                        flexDirection: "row",
                        alignItems: "center",
                        gap: 8,
                        ...colorTransition(140),
                      }}
                    >
                      <Icon name="code" size={15} tone="secondary-foreground" />
                      <Text style={{ flex: 1, fontSize: 12.5 }} class="text-foreground">
                        {t("inspector.package.openBuild")}
                      </Text>
                      <Text
                        style={{ fontSize: 11.5 }}
                        class={
                          props.packageFreshness === "failed"
                            ? "text-destructive"
                            : props.packageFreshness === "stale" || props.packageFreshness === "unverified"
                              ? "text-chart-4"
                              : "text-muted-foreground"
                        }
                      >
                        {props.packageFreshness === "verified_fresh"
                          ? t("inspector.package.verified")
                          : props.packageFreshness === "stale"
                            ? t("inspector.package.stale")
                            : props.packageFreshness === "failed"
                              ? t("inspector.package.failed")
                              : t("inspector.package.unverified")}
                      </Text>
                      <Icon name="chevronRight" size={14} tone="muted-foreground" />
                    </View>
                  </If>
                </Section>
              </If>
              <ProblemInfoTab id={props.id} rev={rev} done={done} hasFollowingSection={showAssets()} />
            </If>
            <If when={showAssets()}>
              <View style={{ padding: 14 }}>
                <View
                  testID="inspector-assets-btn"
                  onPress={() => props.onOpenAssets?.()}
                  class="border border-border bg-card hover:bg-muted active:bg-accent"
                  style={{
                    height: 34,
                    paddingLeft: 10,
                    paddingRight: 8,
                    borderRadius: 7,
                    flexDirection: "row",
                    alignItems: "center",
                    gap: 8,
                    ...colorTransition(140),
                  }}
                >
                  <Icon name="image" size={15} tone="secondary-foreground" />
                  <Text style={{ flex: 1, fontSize: 12.5, fontVariant: ["tabular-nums"] }} class="text-foreground">
                    {t("inspector.assetsCount", { count: assetCount() })}
                  </Text>
                  <Icon name="chevronRight" size={14} tone="muted-foreground" />
                </View>
              </View>
            </If>
          </If>

          <If when={activeTab() === "layout"}>
            <If when={props.target === "contest" && props.contest}>
              <ContestLayoutTab
                contest={props.contest!}
                item={props.item}
                catalog={catalog()}
                done={done}
                onRequestFork={(sourceId, sourceName) => forkModal.set({ sourceId, sourceName })}
                onRequestPreset={() => presetModalOpen.set(true)}
              />
            </If>
            <If when={props.target === "problem"}>
              <ProblemLayoutTab id={props.id} rev={rev} catalog={catalog()} done={done} />
            </If>
          </If>

          <If when={error()}>
            <View style={{ paddingLeft: 14, paddingRight: 14, paddingBottom: 14 }}>
              <ErrorDisplay error={error()} onDismiss={() => error.set(undefined)} />
            </View>
          </If>
        </Scroll>
      </If>

      <ForkTemplateModal
        open={!!forkModal()}
        sourceId={forkModal()?.sourceId ?? "olymp-classic"}
        sourceName={forkModal()?.sourceName ?? t("newWorkspace.tpl.olympClassic")}
        existingIds={catalog().templates.map((t) => t.id)}
        onCreated={(newId) => {
          if (props.target === "contest" && props.contest && !props.contest.frozen && !props.contest.archived && !info().readOnly) {
            done(preview.updateContestMeta(props.contest.id, { template: newId }));
          } else {
            refresh();
            props.onChanged();
          }
        }}
        onClose={() => forkModal.set(undefined)}
      />

      <InstallPresetModal
        open={presetModalOpen()}
        presets={catalog().presets}
        existingIds={catalog().templates.map((t) => t.id)}
        initialTargetName={
          props.target === "contest" && props.contest && !props.contest.hasContestTemplate
            ? (() => {
                const eff = props.contest.effectiveTemplate || catalog().default || "olymp-classic";
                return catalog().templates.some((t) => t.id === eff) ? undefined : eff;
              })()
            : undefined
        }
        onInstalled={(installedId) => {
          if (props.target === "contest" && props.contest && !props.contest.frozen && !props.contest.archived && !info().readOnly) {
            done(preview.updateContestMeta(props.contest.id, { template: installedId === catalog().default ? null : installedId }));
          } else {
            refresh();
            props.onChanged();
          }
        }}
        onClose={() => presetModalOpen.set(false)}
      />
    </View>
  );
}

function Section(props: { title?: string; children?: JSX.Element; last?: boolean }) {
  return (
    <View style={{ padding: 14, gap: 10, borderBottomWidth: props.last ? 0 : 1 }} class="border-border">
      {props.title && (
        <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: 12.5, fontWeight: 600, lineHeight: 17 }} class="text-foreground">
          {props.title}
        </Text>
      )}
      {props.children}
    </View>
  );
}

/**
 * A text field that writes on Return, on blur, or when its owner switches while focused.
 * Deduplicates `onSubmit` + `onBlur` double triggers and rolls back `draft` when `onCommit`
 * returns `false` (invalid input).
 */
function Field(props: {
  ownerKey: string;
  label: string;
  value: string;
  placeholder?: string;
  readOnly?: boolean;
  overridden?: boolean;
  numeric?: boolean;
  onCommit: (value: string) => boolean | void;
  onRestore?: () => void;
}) {
  const draft = signal(props.value);
  let focused = false;
  let lastOwner = props.ownerKey;
  let lastValue = props.value;
  let lastCommitted = props.value;
  let lastCommitFn = props.onCommit;

  function commitTo(fn: (v: string) => boolean | void, expectedValue: string) {
    if (props.readOnly) return;
    const trimmed = untracked(draft).trim();
    if (trimmed === lastCommitted) return;
    if (trimmed === expectedValue) {
      lastCommitted = trimmed;
      return;
    }
    const ok = fn(trimmed);
    if (ok === false) {
      draft.set(expectedValue);
      lastCommitted = expectedValue;
    } else {
      lastCommitted = trimmed;
    }
  }

  effect(() => {
    const owner = props.ownerKey;
    const v = props.value;
    const fn = props.onCommit;
    untracked(() => {
      if (owner !== lastOwner) {
        if (focused) commitTo(lastCommitFn, lastValue);
        focused = false;
        lastOwner = owner;
        lastValue = v;
        lastCommitted = v;
        lastCommitFn = fn;
        draft.set(v);
        return;
      }
      lastValue = v;
      lastCommitFn = fn;
      if (!focused) {
        lastCommitted = v;
        draft.set(v);
      }
    });
  });

  cleanup(() => {
    if (focused) commitTo(lastCommitFn, lastValue);
  });

  return (
    <View style={{ gap: 4 }}>
      <View style={{ flexDirection: "row", alignItems: "center", height: 16 }}>
        <Text style={{ flex: 1, fontSize: 12 }} class="text-secondary-foreground">
          {props.label}
        </Text>
        <If when={props.overridden && !props.readOnly}>
          <IconBtn icon="rotateCcw" size={20} iconSize={12.5} tooltip={tip(t("common.restoreLibraryValue"))} onPress={() => props.onRestore?.()} />
        </If>
      </View>
      <View style={{ flexDirection: "row" }}>
        <View style={{ position: "absolute", left: -7, top: 4, bottom: 4, width: 2, borderRadius: 1, opacity: props.overridden ? 1 : 0 }} class="bg-primary" />
        <Input
          readonly={props.readOnly}
          value={draft()}
          placeholder={props.placeholder}
          keyboardType={props.numeric ? "number-pad" : undefined}
          autoCapitalize="none"
          autoCorrect={false}
          submitBehavior="blurAndSubmit"
          onChange={(e) => draft.set(e.value)}
          onFocus={() => {
            if (!props.readOnly) focused = true;
          }}
          onBlur={() => {
            if (!focused) return;
            focused = false;
            commitTo(lastCommitFn, lastValue);
          }}
          onSubmit={() => {
            commitTo(lastCommitFn, lastValue);
          }}
          class={
            props.readOnly
              ? "flex-1 h-8 px-2.5 rounded-md border border-border bg-muted/50 text-[13px] text-secondary-foreground placeholder:text-muted-foreground"
              : "flex-1 h-8 px-2.5 rounded-md border border-border bg-card text-[13px] placeholder:text-muted-foreground focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring transition-colors"
          }
          style={{ fontVariant: ["tabular-nums"] }}
        />
      </View>
    </View>
  );
}

const nul = (v: string) => (v === "" ? null : v);

// ── 信息 Tab ─────────────────────────────────────────────────────────────────

function ProblemInfoTab(props: {
  id: string;
  rev: () => number;
  done: (res: { ok: boolean; code?: string | null; error?: string | null }) => void;
  hasFollowingSection?: boolean;
}) {
  const d = computed(() => {
    void props.rev();
    return preview.problemDetail(props.id);
  });
  const wsReadOnly = () => info().readOnly;
  const write = (patch: Parameters<typeof preview.updateProblemMeta>[1]) => !wsReadOnly() && props.done(preview.updateProblemMeta(props.id, patch));
  const owner = () => `problem:${props.id}`;
  return (
    <Section title={t("inspector.problemInfo")} last={!props.hasFollowingSection}>
      <Field
        ownerKey={owner()}
        label={t("inspector.field.title")}
        readOnly={wsReadOnly()}
        value={d().title}
        onCommit={(v) => {
          if (!v) return false;
          write({ title: v });
        }}
      />
      <Field
        ownerKey={owner()}
        label={t("inspector.field.rating")}
        numeric
        readOnly={wsReadOnly()}
        placeholder={t("inspector.field.unrated")}
        value={d().rating == null ? "" : String(d().rating)}
        onCommit={(v) => {
          if (v === "") {
            write({ rating: null });
            return true;
          }
          const n = Number(v);
          if (!Number.isFinite(n) || n < 0) return false;
          write({ rating: Math.round(n) });
          return true;
        }}
      />
      <Field
        ownerKey={owner()}
        label={t("inspector.field.tags")}
        readOnly={wsReadOnly()}
        value={d().tags.join(", ")}
        placeholder="dp, greedy"
        onCommit={(v) => write({ tags: v.split(/[,，\s]+/).filter(Boolean) })}
      />
      <View style={{ flexDirection: "row", gap: 8 }}>
        <View style={{ flex: 1 }}>
          <Field ownerKey={owner()} label={t("inspector.field.timeLimit")} readOnly={wsReadOnly()} value={d().limits.time ?? ""} placeholder="1 s" onCommit={(v) => write({ time: nul(v) })} />
        </View>
        <View style={{ flex: 1 }}>
          <Field ownerKey={owner()} label={t("inspector.field.memoryLimit")} readOnly={wsReadOnly()} value={d().limits.memory ?? ""} placeholder="256 MB" onCommit={(v) => write({ memory: nul(v) })} />
        </View>
      </View>
      <View style={{ flexDirection: "row", gap: 8 }}>
        <View style={{ flex: 1 }}>
          <Field ownerKey={owner()} label={t("inspector.field.inputFile")} readOnly={wsReadOnly()} value={d().limits.input ?? ""} placeholder={t("inspector.field.stdin")} onCommit={(v) => write({ input: nul(v) })} />
        </View>
        <View style={{ flex: 1 }}>
          <Field ownerKey={owner()} label={t("inspector.field.outputFile")} readOnly={wsReadOnly()} value={d().limits.output ?? ""} placeholder={t("inspector.field.stdout")} onCommit={(v) => write({ output: nul(v) })} />
        </View>
      </View>
      <Field ownerKey={owner()} label={t("inspector.field.author")} readOnly={wsReadOnly()} value={d().author ?? ""} onCommit={(v) => write({ author: nul(v) })} />
    </Section>
  );
}

function ContestInfoTab(props: {
  contest: ContestDetailJs;
  item?: ContestItemDetailJs;
  freeze: Freeze;
  done: (res: { ok: boolean; code?: string | null; error?: string | null }) => void;
  onFinding: (d: PreviewDiagnostic) => void;
  hasFollowingSection?: boolean;
}) {
  const c = () => props.contest;
  const frozen = () => c()?.frozen ?? false;
  const archived = () => c()?.archived ?? false;
  const wsReadOnly = () => info().readOnly;
  const locked = () => !c() || frozen() || archived() || wsReadOnly();
  const meta = (patch: Parameters<typeof preview.updateContestMeta>[1]) => {
    const cur = c();
    if (!cur || locked()) return;
    props.done(preview.updateContestMeta(cur.id, patch));
  };
  const over = (patch: Parameters<typeof preview.setOverride>[2]) => {
    const cur = c();
    if (!cur || locked() || !props.item) return;
    props.done(preview.setOverride(cur.id, props.item.label, patch));
  };
  const f = props.freeze;
  const contestOwner = () => `contest:${c()?.id ?? ""}`;

  return (
    <View>
      <Section>
        <View class="border border-border bg-muted/40" style={{ borderRadius: 8, padding: 10, gap: 6 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <Icon name={archived() ? "folder" : frozen() ? "lock" : "stamp"} size={14} tone={archived() || frozen() ? "secondary-foreground" : "foreground"} />
            <Text style={{ flex: 1, fontSize: 12.5, fontWeight: 500 }}>
              {archived() ? (frozen() ? t("contests.archivedAndFinalized") : t("contests.archived")) : frozen() ? t("contests.finalized") : t("contests.unfinalized")}
            </Text>
            <If
              when={archived()}
              else={
                <If
                  when={frozen()}
                  else={
                    <Btn
                      disabled={f.busy() || wsReadOnly() || c().items.length === 0}
                      tooltip={wsReadOnly() ? tip(t("contests.readOnlyCannotFinalize")) : undefined}
                      onPress={() => void f.freeze(c().id).then(() => props.done({ ok: true }))}
                      style={{ minWidth: 74 }}
                    >
                      <View style={{ width: 15, height: 15, alignItems: "center", justifyContent: "center" }}>
                        <If when={f.busyVisible()} else={<Icon name="stamp" size={15} tone="foreground" />}>
                          <DelayedSpinner active={f.busy()} size={13} />
                        </If>
                      </View>
                      <Text class="text-foreground" style={{ fontSize: 13 }}>
                        {t("contests.finalize")}
                      </Text>
                    </Btn>
                  }
                >
                  <Btn
                    label={t("contests.reopen")}
                    icon="lockOpen"
                    disabled={wsReadOnly()}
                    tooltip={wsReadOnly() ? tip(t("contests.readOnlyCannotReopen")) : undefined}
                    onPress={() => unfreezeFor.set(c().id)}
                    testID="inspector-unfreeze"
                  />
                </If>
              }
            >
              <Btn
                label={t("contests.unarchive")}
                icon="rotateCcw"
                disabled={wsReadOnly()}
                tooltip={wsReadOnly() ? tip(t("contests.readOnlyCannotUnarchive")) : undefined}
                onPress={() => props.done(preview.setContestArchived(c().id, false))}
                testID="inspector-unarchive"
              />
            </If>
          </View>
          <Text style={{ fontSize: 12, lineHeight: 17 }} class="text-secondary-foreground">
            {archived()
              ? t("inspector.status.archivedHint")
              : frozen()
                ? t("inspector.status.finalizedHint")
                : t("inspector.status.editableHint")}
          </Text>
        </View>
        <ErrorDisplay
          error={f.error() ? { ...f.error()!, context: undefined } : undefined}
          locate={f.findings().slice(0, 3).map((d) => ({
            label: d.file ? `${d.file.split("/").slice(-2).join("/")}${d.line ? `:${d.line}` : ""}` : d.message,
            onPress: () => props.onFinding(d),
          }))}
          action={
            f.findings().length > 0 ? (
              <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
                {freezeBlockedCopy(f.findings().length)}
              </Text>
            ) : undefined
          }
        />
      </Section>

      <Section title={t("inspector.contestInfo")} last={!props.item && !props.hasFollowingSection}>
        <Field
          ownerKey={contestOwner()}
          label={t("inspector.field.contestName")}
          readOnly={locked()}
          value={c().name}
          onCommit={(v) => {
            if (!v) return false;
            meta({ name: v });
          }}
        />
        <View style={{ flexDirection: "row", gap: 8 }}>
          <View style={{ flex: 1 }}>
            <Field ownerKey={contestOwner()} label={t("inspector.field.date")} readOnly={locked()} value={c().date ?? ""} placeholder="2026-04-12" onCommit={(v) => meta({ date: nul(v) })} />
          </View>
          <View style={{ flex: 1 }}>
            <Field ownerKey={contestOwner()} label={t("inspector.field.location")} readOnly={locked()} value={c().location ?? ""} onCommit={(v) => meta({ location: nul(v) })} />
          </View>
        </View>
        <Field ownerKey={contestOwner()} label={t("inspector.field.signature")} readOnly={locked()} value={c().author ?? ""} onCommit={(v) => meta({ author: nul(v) })} />
        <View style={{ flexDirection: "row", alignItems: "center", height: 28 }}>
          <Text style={{ flex: 1, fontSize: 13 }} class={locked() ? "text-secondary-foreground" : "text-foreground"}>
            {t("inspector.field.language")}
          </Text>
          <Segmented
            disabled={locked()}
            items={[
              { value: "chinese", label: "中文" },
              { value: "english", label: "English" },
            ]}
            itemWidth={62}
            value={c().language === "english" ? "english" : "chinese"}
            onChange={(v) => !locked() && meta({ language: v })}
          />
        </View>
      </Section>

      <If when={props.item}>
        {(item) => {
          const itemOwner = () => `contest:${c().id}:item:${item().label}:${item().id}`;
          return (
            <Section title={t("inspector.itemOverrides", { label: item().label })} last={!props.hasFollowingSection}>
              <Field
                ownerKey={itemOwner()}
                label={t("inspector.field.title")}
                readOnly={locked()}
                value={item().title}
                placeholder={item().originalTitle}
                overridden={item().overriddenTitle}
                onCommit={(v) => over({ title: v || null })}
                onRestore={() => over({ title: null })}
              />
              <View style={{ flexDirection: "row", gap: 8 }}>
                <View style={{ flex: 1 }}>
                  <Field
                    ownerKey={itemOwner()}
                    label={t("inspector.field.timeLimit")}
                    readOnly={locked()}
                    value={item().limits.time ?? ""}
                    placeholder={item().originalLimits.time ?? ""}
                    overridden={item().overriddenTime}
                    onCommit={(v) => over({ time: nul(v) })}
                    onRestore={() => over({ time: null })}
                  />
                </View>
                <View style={{ flex: 1 }}>
                  <Field
                    ownerKey={itemOwner()}
                    label={t("inspector.field.memoryLimit")}
                    readOnly={locked()}
                    value={item().limits.memory ?? ""}
                    placeholder={item().originalLimits.memory ?? ""}
                    overridden={item().overriddenMemory}
                    onCommit={(v) => over({ memory: nul(v) })}
                    onRestore={() => over({ memory: null })}
                  />
                </View>
              </View>
              <View style={{ flexDirection: "row", gap: 8 }}>
                <View style={{ flex: 1 }}>
                  <Field
                    ownerKey={itemOwner()}
                    label={t("inspector.field.inputFile")}
                    readOnly={locked()}
                    value={item().limits.input ?? ""}
                    placeholder={item().originalLimits.input ?? t("inspector.field.stdin")}
                    overridden={item().overriddenInput}
                    onCommit={(v) => over({ input: nul(v) })}
                    onRestore={() => over({ input: null })}
                  />
                </View>
                <View style={{ flex: 1 }}>
                  <Field
                    ownerKey={itemOwner()}
                    label={t("inspector.field.outputFile")}
                    readOnly={locked()}
                    value={item().limits.output ?? ""}
                    placeholder={item().originalLimits.output ?? t("inspector.field.stdout")}
                    overridden={item().overriddenOutput}
                    onCommit={(v) => over({ output: nul(v) })}
                    onRestore={() => over({ output: null })}
                  />
                </View>
              </View>
            </Section>
          );
        }}
      </If>
    </View>
  );
}

// ── 版式 Tab ─────────────────────────────────────────────────────────────────

function ContestLayoutTab(props: {
  contest: ContestDetailJs;
  item?: ContestItemDetailJs;
  catalog: TemplatesCatalog;
  done: (res: { ok: boolean; code?: string | null; error?: string | null }) => void;
  onRequestFork: (sourceId: string, sourceName: string) => void;
  onRequestPreset: () => void;
}) {
  const c = () => props.contest;
  const frozen = () => c()?.frozen ?? false;
  const archived = () => c()?.archived ?? false;
  const wsReadOnly = () => info().readOnly;
  const locked = () => !c() || frozen() || archived() || wsReadOnly();
  const meta = (patch: Parameters<typeof preview.updateContestMeta>[1]) => {
    const cur = c();
    if (!cur || locked()) return;
    props.done(preview.updateContestMeta(cur.id, patch));
  };
  const over = (patch: Parameters<typeof preview.setOverride>[2]) => {
    const cur = c();
    if (!cur || locked() || !props.item) return;
    props.done(preview.setOverride(cur.id, props.item.label, patch));
  };

  const confirmRestoreOpen = signal(false);

  const hasLocal = () => c().hasContestTemplate;
  const effTemplateId = () => c().effectiveTemplate || props.catalog.default || "olymp-classic";
  const wsDefaultName = computed(() => {
    const defId = props.catalog.default || "olymp-classic";
    const found = props.catalog.templates.find((t) => t.id === defId);
    return found ? resolveLocalized(found.name) || defId : defId;
  });
  const localTplInfo = computed(() => (hasLocal() ? parseContestTemplateInfo(c().contestTemplateJson) : undefined));
  const wsTplInfo = computed(() => props.catalog.templates.find((t) => t.id === effTemplateId()));
  const tplInfo = computed(() => (hasLocal() ? localTplInfo() : wsTplInfo()));
  const isMissingTemplate = () => !hasLocal() && !wsTplInfo();
  const canInstallMissingPreset = () => isMissingTemplate() && props.catalog.presets.some((p) => p.id === effTemplateId());
  const templateOverridden = () => !hasLocal() && !!c().template && c().template !== props.catalog.default;

  const optsNs = () => (hasLocal() ? LOCAL_TEMPLATE_NAMESPACE : effTemplateId());
  const wsBookOpts = computed<TemplateOptionMap>(() =>
    hasLocal() ? {} : (props.catalog.workspaceOptions[effTemplateId()] ?? {}),
  );
  const contestBookOpts = computed<TemplateOptionMap>(() => parseOptionsByTemplate(c().templateOptionsJson)[optsNs()] ?? {});

  const bookContext = computed<Record<string, TemplateOptionValue>>(() => ({
    language: c().language,
  }));

  const hasBookFields = computed(() => (tplInfo()?.schema.fields ?? []).some((f) => f.scope === "book"));
  const hasProblemFields = computed(() => (tplInfo()?.schema.fields ?? []).some((f) => f.scope === "problem"));

  return (
    <View>
      <Section title={t("template.label")}>
        <If
          when={hasLocal()}
          else={
            <If
              when={isMissingTemplate()}
              else={
                <View style={{ gap: 8 }}>
                  <TemplatePicker
                    templates={props.catalog.templates}
                    value={effTemplateId()}
                    inheritedLabel={t("template.restoreWorkspaceDefaultNamed", { name: wsDefaultName() })}
                    overridden={templateOverridden()}
                    readOnly={locked()}
                    onChange={(tplId) => meta({ template: tplId === props.catalog.default ? null : tplId })}
                    onRestore={() => meta({ template: null })}
                    onAddFromPreset={!locked() ? props.onRequestPreset : undefined}
                  />
                  <If when={!locked()}>
                    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
                      <Btn
                        label={t("settings.layout.editTemplate")}
                        icon="filePenLine"
                        tooltip={tip(t("settings.layout.editTemplateTooltip"))}
                        onPress={() => void preview.openTemplateFile(effTemplateId()).then((res) => !res.ok && props.done(res))}
                      />
                      <Btn
                        label={t("inspector.layout.detach")}
                        icon="copy"
                        tooltip={tip(t("inspector.layout.detachTooltip"))}
                        onPress={() => props.done(preview.detachContestTemplate(c().id))}
                      />
                    </View>
                  </If>
                </View>
              }
            >
              <View style={{ gap: 8 }}>
                <View class="border border-border bg-muted/40" style={{ borderRadius: 8, padding: 10, gap: 6 }}>
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                    <Icon name="triangleAlert" size={14} tone="destructive" />
                    <Text style={{ flex: 1, fontSize: 12.5, fontWeight: 500 }} class="text-foreground">
                      {t("inspector.layout.missingTitle", { id: effTemplateId() })}
                    </Text>
                  </View>
                  <Text style={{ fontSize: 12, lineHeight: 16.5 }} class="text-secondary-foreground">
                    {t("inspector.layout.missingDesc")}
                  </Text>
                  <If when={!locked()}>
                    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6, paddingTop: 2 }}>
                      <If when={canInstallMissingPreset()}>
                        <Btn
                          variant="primary"
                          label={t("settings.layout.installFromPreset")}
                          icon="plus"
                          onPress={() => props.done(preview.installPresetTemplate(effTemplateId(), effTemplateId()))}
                        />
                      </If>
                      <Btn
                        variant={canInstallMissingPreset() ? undefined : "primary"}
                        label={t("template.addFromPresetEllipsis")}
                        icon="plus"
                        onPress={props.onRequestPreset}
                      />
                    </View>
                  </If>
                </View>
                <If when={props.catalog.templates.length > 0}>
                  <TemplatePicker
                    templates={props.catalog.templates}
                    value={effTemplateId()}
                    inheritedLabel={t("template.restoreWorkspaceDefaultNamed", { name: wsDefaultName() })}
                    overridden={templateOverridden()}
                    readOnly={locked()}
                    onChange={(tplId) => meta({ template: tplId === props.catalog.default ? null : tplId })}
                    onRestore={() => meta({ template: null })}
                    onAddFromPreset={!locked() ? props.onRequestPreset : undefined}
                  />
                </If>
              </View>
            </If>
          }
        >
          <View class="border border-border bg-muted/40" style={{ borderRadius: 8, padding: 10, gap: 8 }}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
              <Icon name="fileText" size={14} tone="primary" />
              <Text style={{ flex: 1, fontSize: 13, fontWeight: 600 }} class="text-foreground">
                {t("inspector.layout.localTitle")}
              </Text>
            </View>
            <Text style={{ fontSize: 12, lineHeight: 16.5 }} class="text-secondary-foreground">
              {localTplInfo()?.name
                ? t("inspector.layout.localDescNamed", { name: resolveLocalized(localTplInfo()!.name) })
                : t("inspector.layout.localDesc")}
            </Text>
            <If when={!locked()}>
              <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
                <Btn
                  label={t("inspector.layout.editLocal")}
                  icon="filePenLine"
                  tooltip={tip(t("settings.layout.editTemplateTooltip"))}
                  onPress={() => void preview.openContestTemplateFile(c().id).then((res) => !res.ok && props.done(res))}
                />
                <Btn
                  label={t("inspector.layout.restoreWorkspaceRef")}
                  icon="rotateCcw"
                  onPress={() => confirmRestoreOpen.set(true)}
                />
              </View>
            </If>
          </View>
        </If>
      </Section>

      <If when={hasBookFields()}>
        <TemplateOptionsEditor
          ownerKey={`contest:${c().id}:${optsNs()}`}
          template={tplInfo()}
          scope="book"
          baseValues={wsBookOpts()}
          overrideValues={contestBookOpts()}
          context={bookContext()}
          fonts={props.catalog.fonts}
          contestId={c().id}
          readOnly={locked()}
          restoreTooltip={hasLocal() ? t("common.restoreTemplateDefault") : t("common.restoreWorkspaceDefault")}
          sectioned
          lastSection={!props.item || !hasProblemFields() ? locked() : false}
          onChange={(key, value) => meta({ templateOptionsJson: encodeSingleOptionPatch(optsNs(), key, value) })}
        />
      </If>

      <If when={hasProblemFields() ? props.item : undefined}>
        {(item) => {
          const origOpts = computed<TemplateOptionMap>(() => parseOptionsByTemplate(item().originalOptionsJson)[optsNs()] ?? {});
          const overOpts = computed<TemplateOptionMap>(() => parseOptionsByTemplate(item().overrideOptionsJson)[optsNs()] ?? {});
          return (
            <Section title={t("inspector.layout.itemLayout", { label: item().label })} last={locked()}>
              <TemplateOptionsEditor
                ownerKey={`contest:${c().id}:item:${item().label}:${item().id}:${optsNs()}`}
                template={tplInfo()}
                scope="problem"
                baseValues={origOpts()}
                overrideValues={overOpts()}
                context={bookContext()}
                fonts={props.catalog.fonts}
                contestId={c().id}
                readOnly={locked()}
                restoreTooltip={t("common.restoreLibraryValue")}
                hideGroupLabels
                onChange={(key, value) => over({ optionsJson: encodeSingleOptionPatch(optsNs(), key, value) })}
              />
            </Section>
          );
        }}
      </If>

      <If when={!locked()}>
        <Section title={t("inspector.layout.advanced")} last>
          <View style={{ gap: 6 }}>
            <If when={!hasLocal() && !!tplInfo()}>
              <Btn
                label={t("inspector.layout.forkCurrent")}
                icon="copy"
                tooltip={tip(t("inspector.layout.forkCurrentTooltip"))}
                onPress={() => props.onRequestFork(effTemplateId(), tplInfo() ? resolveLocalized(tplInfo()!.name) || effTemplateId() : effTemplateId())}
              />
            </If>
            <Btn
              label={t("settings.layout.preamble")}
              icon="code"
              tooltip={tip(t("settings.layout.preambleTooltip"))}
              onPress={() => void preview.openPreambleFile().then((res) => !res.ok && props.done(res))}
            />
          </View>
        </Section>
      </If>

      <Modal open={confirmRestoreOpen()} onClose={() => confirmRestoreOpen.set(false)} width={380}>
        <View style={{ padding: 18, gap: 14 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
            <View class="bg-muted border border-border" style={{ width: 32, height: 32, borderRadius: 8, alignItems: "center", justifyContent: "center" }}>
              <Icon name="rotateCcw" size={15} tone="foreground" />
            </View>
            <View style={{ flex: 1 }}>
              <Text style={{ fontSize: 14.5, fontWeight: 600 }} class="text-foreground">
                {t("inspector.layout.restoreConfirmTitle")}
              </Text>
              <Text style={{ fontSize: 12, lineHeight: 17 }} class="text-muted-foreground">
                {t("inspector.layout.restoreConfirmDesc")}
              </Text>
            </View>
          </View>
          <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
            <Btn label={t("common.cancel")} onPress={() => confirmRestoreOpen.set(false)} />
            <Btn
              label={t("inspector.layout.restoreConfirmAction")}
              variant="danger"
              onPress={() => {
                confirmRestoreOpen.set(false);
                props.done(preview.removeContestTemplate(c().id));
              }}
            />
          </View>
        </View>
      </Modal>
    </View>
  );
}

function ProblemLayoutTab(props: {
  id: string;
  rev: () => number;
  catalog: TemplatesCatalog;
  done: (res: { ok: boolean; code?: string | null; error?: string | null }) => void;
}) {
  const d = computed(() => {
    void props.rev();
    return preview.problemDetail(props.id);
  });
  const wsReadOnly = () => info().readOnly;
  const tplId = () => props.catalog.default || "olymp-classic";
  const tplInfo = computed(() => props.catalog.templates.find((t) => t.id === tplId()));
  const canInstallPreset = () => !tplInfo() && props.catalog.presets.some((p) => p.id === tplId());
  const probOpts = computed<TemplateOptionMap>(() => parseOptionsByTemplate(d().templateOptionsJson)[tplId()] ?? {});
  const hasProblemFields = computed(() => (tplInfo()?.schema.fields ?? []).some((f) => f.scope === "problem"));

  const write = (patch: Parameters<typeof preview.updateProblemMeta>[1]) => !wsReadOnly() && props.done(preview.updateProblemMeta(props.id, patch));

  return (
    <View>
      <Section title={t("inspector.layout.currentTemplate")} last={!hasProblemFields() && wsReadOnly()}>
        <View class="border border-border bg-muted/40" style={{ borderRadius: 8, padding: 10, gap: 6 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <Icon name={tplInfo() ? "fileText" : "triangleAlert"} size={14} tone={tplInfo() ? "secondary-foreground" : "destructive"} />
            <Text style={{ flex: 1, fontSize: 13, fontWeight: 500 }} class="text-foreground">
              {tplInfo() ? resolveLocalized(tplInfo()!.name) || tplId() : t("contests.missingSuffix", { title: tplId() })}
            </Text>
          </View>
          <Text style={{ fontSize: 12, lineHeight: 16.5 }} class="text-secondary-foreground">
            {tplInfo()
              ? t("inspector.layout.problemFollowsDefault")
              : t("inspector.layout.problemMissingTemplate")}
          </Text>
          <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6, paddingTop: 2 }}>
            <If when={!wsReadOnly() && canInstallPreset()}>
              <Btn
                variant="primary"
                label={t("settings.layout.installFromPreset")}
                icon="plus"
                onPress={() => props.done(preview.installPresetTemplate(tplId(), tplId()))}
              />
            </If>
            <Btn label={t("inspector.layout.adjustWorkspaceDefault")} icon="settings" onPress={() => goSettings("layout")} />
          </View>
        </View>
      </Section>

      <If when={hasProblemFields()}>
        <Section title={t("inspector.layout.problemSettings")} last={wsReadOnly()}>
          <TemplateOptionsEditor
            ownerKey={`problem:${props.id}`}
            template={tplInfo()}
            scope="problem"
            baseValues={{}}
            overrideValues={probOpts()}
            fonts={props.catalog.fonts}
            readOnly={wsReadOnly()}
            restoreTooltip={t("common.restoreTemplateDefault")}
            hideGroupLabels
            onChange={(key, value) => write({ templateOptionsJson: encodeSingleOptionPatch(tplId(), key, value) })}
          />
        </Section>
      </If>

      <If when={!wsReadOnly()}>
        <Section title={t("inspector.layout.advanced")} last>
          <Btn
            label={t("settings.layout.preamble")}
            icon="code"
            tooltip={tip(t("settings.layout.preambleTooltip"))}
            onPress={() => void preview.openPreambleFile().then((res) => !res.ok && props.done(res))}
          />
        </Section>
      </If>
    </View>
  );
}
