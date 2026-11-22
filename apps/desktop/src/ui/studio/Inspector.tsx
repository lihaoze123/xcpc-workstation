// The publishing drawer on the right of the studio: slides over the preview with a
// `translateX` (never squeezing the editor) and splits into two tabs (`信息` | `版式`):
// - `信息`: freeze status, contest/problem metadata, per-problem limits overrides, and assets.
// - `版式`: template selection, shared publishing switches, and dynamic controls rendered
//   from the active template's `#let schema = (...)` (with override bars & one-click restore),
//   plus lightweight bridges to fork a custom template or open external style files.

import { cleanup, computed, effect, Scroll, signal, Text, untracked, View, type JSX } from "nsh";
import { Input, Switch } from "nsh/base";
import type { ContestDetailJs, ContestItemDetailJs, PreviewDiagnostic } from "xcpc-workstation/native";
import { freezeBlockedCopy, type Freeze } from "../Contest";
import { ErrorDisplay } from "../ErrorDisplay";
import { Icon } from "../Icon";
import { Btn, DelayedSpinner, IconBtn, Segmented, tip } from "../kit";
import { Modal } from "../Modal";
import { colorTransition, panelTransition } from "../motion";
import { errorOf, goSettings, info, preview, refresh, revision, unfreezeFor, type UiError } from "../store";
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
  open: boolean;
  target: "problem" | "contest";
  id: string;
  contest?: ContestDetailJs;
  item?: ContestItemDetailJs;
  freeze: Freeze;
  assetsRefreshSeq?: number;
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

  const showAssets = () => props.target === "problem" || (props.target === "contest" && !!props.item);

  const catalog = computed<TemplatesCatalog>(() => {
    void revision();
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
    void revision();
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

  return (
    <View
      testID="inspector"
      style={{
        position: "absolute",
        top: 0,
        right: 0,
        bottom: 0,
        width: INSPECTOR_WIDTH,
        borderLeftWidth: 1,
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
            { value: "info" as InspectorTab, label: "信息" },
            { value: "layout" as InspectorTab, label: "版式" },
          ]}
          itemWidth={58}
          value={activeTab()}
          onChange={(v) => activeTab.set(v)}
        />
        <View style={{ flex: 1 }} />
        <IconBtn icon="x" tooltip={tip("关闭", "mod+i")} onPress={props.onClose} />
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
              <ProblemInfoTab id={props.id} done={done} hasFollowingSection={showAssets()} />
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
                    {`素材 ${assetCount()} 个`}
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
              <ProblemLayoutTab id={props.id} catalog={catalog()} done={done} />
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
        sourceName={forkModal()?.sourceName ?? "XCPC 经典"}
        onCreated={(newId) => {
          if (props.target === "contest" && props.contest && !props.contest.frozen && !info().readOnly) {
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
        onInstalled={(installedId) => {
          if (props.target === "contest" && props.contest && !props.contest.frozen && !info().readOnly) {
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
          <IconBtn icon="rotateCcw" size={20} iconSize={12.5} tooltip={tip("还原为题库的值")} onPress={() => props.onRestore?.()} />
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

function Toggle(props: { label: string; checked: boolean; readOnly?: boolean; overridden?: boolean; onChange: (v: boolean) => void; onRestore?: () => void }) {
  return (
    <View style={{ flexDirection: "row", alignItems: "center", height: 24, gap: 6 }}>
      <View style={{ position: "absolute", left: -7, top: 4, bottom: 4, width: 2, borderRadius: 1, opacity: props.overridden ? 1 : 0 }} class="bg-primary" />
      <Text style={{ flex: 1, fontSize: 13 }} class={props.readOnly ? "text-secondary-foreground" : "text-foreground"}>
        {props.label}
      </Text>
      <If when={props.overridden && !props.readOnly}>
        <IconBtn icon="rotateCcw" size={20} iconSize={12.5} tooltip={tip("还原为比赛的默认值")} onPress={() => props.onRestore?.()} />
      </If>
      <Switch
        checked={props.checked}
        disabled={props.readOnly}
        onChange={(e) => !props.readOnly && props.onChange(e.checked)}
        trackClass={
          props.readOnly
            ? "w-8 h-[18px] rounded-full bg-input checked:bg-primary opacity-50"
            : "w-8 h-[18px] rounded-full bg-input checked:bg-primary transition-colors duration-[120ms]"
        }
        thumbClass="size-[14px] m-[2px] rounded-full bg-white shadow-sm checked:translate-x-[14px] transition-transform duration-[120ms]"
      />
    </View>
  );
}

const nul = (v: string) => (v === "" ? null : v);

// ── 信息 Tab ─────────────────────────────────────────────────────────────────

function ProblemInfoTab(props: {
  id: string;
  done: (res: { ok: boolean; code?: string | null; error?: string | null }) => void;
  hasFollowingSection?: boolean;
}) {
  const d = computed(() => {
    void revision();
    return preview.problemDetail(props.id);
  });
  const wsReadOnly = () => info().readOnly;
  const write = (patch: Parameters<typeof preview.updateProblemMeta>[1]) => !wsReadOnly() && props.done(preview.updateProblemMeta(props.id, patch));
  const owner = () => `problem:${props.id}`;
  return (
    <Section title="题目信息" last={!props.hasFollowingSection}>
      <Field
        ownerKey={owner()}
        label="标题"
        readOnly={wsReadOnly()}
        value={d().title}
        onCommit={(v) => {
          if (!v) return false;
          write({ title: v });
        }}
      />
      <Field
        ownerKey={owner()}
        label="难度"
        numeric
        readOnly={wsReadOnly()}
        placeholder="未分级"
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
        label="标签"
        readOnly={wsReadOnly()}
        value={d().tags.join(", ")}
        placeholder="dp, greedy"
        onCommit={(v) => write({ tags: v.split(/[,，\s]+/).filter(Boolean) })}
      />
      <View style={{ flexDirection: "row", gap: 8 }}>
        <View style={{ flex: 1 }}>
          <Field ownerKey={owner()} label="时间限制" readOnly={wsReadOnly()} value={d().limits.time ?? ""} placeholder="1 s" onCommit={(v) => write({ time: nul(v) })} />
        </View>
        <View style={{ flex: 1 }}>
          <Field ownerKey={owner()} label="空间限制" readOnly={wsReadOnly()} value={d().limits.memory ?? ""} placeholder="256 MB" onCommit={(v) => write({ memory: nul(v) })} />
        </View>
      </View>
      <View style={{ flexDirection: "row", gap: 8 }}>
        <View style={{ flex: 1 }}>
          <Field ownerKey={owner()} label="输入文件" readOnly={wsReadOnly()} value={d().limits.input ?? ""} placeholder="标准输入" onCommit={(v) => write({ input: nul(v) })} />
        </View>
        <View style={{ flex: 1 }}>
          <Field ownerKey={owner()} label="输出文件" readOnly={wsReadOnly()} value={d().limits.output ?? ""} placeholder="标准输出" onCommit={(v) => write({ output: nul(v) })} />
        </View>
      </View>
      <Field ownerKey={owner()} label="作者" readOnly={wsReadOnly()} value={d().author ?? ""} onCommit={(v) => write({ author: nul(v) })} />
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
  const frozen = () => c().frozen;
  const archived = () => c().archived;
  const wsReadOnly = () => info().readOnly;
  const locked = () => frozen() || archived() || wsReadOnly();
  const meta = (patch: Parameters<typeof preview.updateContestMeta>[1]) => !locked() && props.done(preview.updateContestMeta(c().id, patch));
  const over = (patch: Parameters<typeof preview.setOverride>[2]) => !locked() && props.item && props.done(preview.setOverride(c().id, props.item.label, patch));
  const f = props.freeze;
  const contestOwner = () => `contest:${c().id}`;

  return (
    <View>
      <Section>
        <View class="border border-border bg-muted/40" style={{ borderRadius: 8, padding: 10, gap: 6 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <Icon name={archived() ? "folder" : frozen() ? "lock" : "stamp"} size={14} tone={archived() || frozen() ? "secondary-foreground" : "foreground"} />
            <Text style={{ flex: 1, fontSize: 12.5, fontWeight: 500 }}>
              {archived() ? (frozen() ? "已归档 · 已定稿" : "已归档") : frozen() ? "已定稿" : "未定稿"}
            </Text>
            <If
              when={archived()}
              else={
                <If
                  when={frozen()}
                  else={
                    <Btn
                      disabled={f.busy() || wsReadOnly() || c().items.length === 0}
                      tooltip={wsReadOnly() ? tip("只读工作区无法定稿") : undefined}
                      onPress={() => void f.freeze(c().id).then(() => props.done({ ok: true }))}
                      style={{ minWidth: 74 }}
                    >
                      <View style={{ width: 15, height: 15, alignItems: "center", justifyContent: "center" }}>
                        <If when={f.busyVisible()} else={<Icon name="stamp" size={15} tone="foreground" />}>
                          <DelayedSpinner active={f.busy()} size={13} />
                        </If>
                      </View>
                      <Text class="text-foreground" style={{ fontSize: 13 }}>
                        定稿
                      </Text>
                    </Btn>
                  }
                >
                  <Btn
                    label="重新编辑"
                    icon="lockOpen"
                    disabled={wsReadOnly()}
                    tooltip={wsReadOnly() ? tip("只读工作区无法重新编辑") : undefined}
                    onPress={() => unfreezeFor.set(c().id)}
                    testID="inspector-unfreeze"
                  />
                </If>
              }
            >
              <Btn
                label="取消归档"
                icon="rotateCcw"
                disabled={wsReadOnly()}
                tooltip={wsReadOnly() ? tip("只读工作区无法取消归档") : undefined}
                onPress={() => props.done(preview.setContestArchived(c().id, false))}
                testID="inspector-unarchive"
              />
            </If>
          </View>
          <Text style={{ fontSize: 12, lineHeight: 17 }} class="text-secondary-foreground">
            {archived()
              ? "比赛已归档，当前只读；取消归档后可恢复编辑"
              : frozen()
                ? "内容已锁定，仅支持查看和导出"
                : "定稿后，题库里的修改不再影响这场比赛"}
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

      <Section title="比赛信息" last={!props.item && !props.hasFollowingSection}>
        <Field
          ownerKey={contestOwner()}
          label="比赛名称"
          readOnly={locked()}
          value={c().name}
          onCommit={(v) => {
            if (!v) return false;
            meta({ name: v });
          }}
        />
        <View style={{ flexDirection: "row", gap: 8 }}>
          <View style={{ flex: 1 }}>
            <Field ownerKey={contestOwner()} label="日期" readOnly={locked()} value={c().date ?? ""} placeholder="2026-04-12" onCommit={(v) => meta({ date: nul(v) })} />
          </View>
          <View style={{ flex: 1 }}>
            <Field ownerKey={contestOwner()} label="地点" readOnly={locked()} value={c().location ?? ""} onCommit={(v) => meta({ location: nul(v) })} />
          </View>
        </View>
        <Field ownerKey={contestOwner()} label="署名" readOnly={locked()} value={c().author ?? ""} onCommit={(v) => meta({ author: nul(v) })} />
        <View style={{ flexDirection: "row", alignItems: "center", height: 28 }}>
          <Text style={{ flex: 1, fontSize: 13 }} class={locked() ? "text-secondary-foreground" : "text-foreground"}>
            语言
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
            <Section title={`${item().label} 题在这场比赛中的设置`} last={!props.hasFollowingSection}>
              <Field
                ownerKey={itemOwner()}
                label="标题"
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
                    label="时间限制"
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
                    label="空间限制"
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
                    label="输入文件"
                    readOnly={locked()}
                    value={item().limits.input ?? ""}
                    placeholder={item().originalLimits.input ?? "标准输入"}
                    overridden={item().overriddenInput}
                    onCommit={(v) => over({ input: nul(v) })}
                    onRestore={() => over({ input: null })}
                  />
                </View>
                <View style={{ flex: 1 }}>
                  <Field
                    ownerKey={itemOwner()}
                    label="输出文件"
                    readOnly={locked()}
                    value={item().limits.output ?? ""}
                    placeholder={item().originalLimits.output ?? "标准输出"}
                    overridden={item().overriddenOutput}
                    onCommit={(v) => over({ output: nul(v) })}
                    onRestore={() => over({ output: null })}
                  />
                </View>
              </View>
              <Toggle
                label="显示时间和内存限制"
                readOnly={locked()}
                checked={item().showMetadata}
                overridden={item().overriddenShowMetadata}
                onChange={(v) => over({ showMetadata: v })}
                onRestore={() => over({ showMetadata: null })}
              />
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
  const frozen = () => c().frozen;
  const archived = () => c().archived;
  const wsReadOnly = () => info().readOnly;
  const locked = () => frozen() || archived() || wsReadOnly();
  const meta = (patch: Parameters<typeof preview.updateContestMeta>[1]) => !locked() && props.done(preview.updateContestMeta(c().id, patch));
  const over = (patch: Parameters<typeof preview.setOverride>[2]) => !locked() && props.item && props.done(preview.setOverride(c().id, props.item.label, patch));

  const confirmRestoreOpen = signal(false);

  const hasLocal = () => c().hasContestTemplate;
  const effTemplateId = () => c().effectiveTemplate || props.catalog.default || "olymp-classic";
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
      <Section title="模板">
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
                    overridden={templateOverridden()}
                    readOnly={locked()}
                    onChange={(tplId) => meta({ template: tplId === props.catalog.default ? null : tplId })}
                    onRestore={() => meta({ template: null })}
                    onAddFromPreset={!locked() ? props.onRequestPreset : undefined}
                  />
                  <If when={!locked()}>
                    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
                      <Btn
                        label="编辑模板"
                        icon="filePenLine"
                        onPress={() => void preview.openTemplateFile(effTemplateId()).then((res) => !res.ok && props.done(res))}
                      />
                      <Btn
                        label="独立为本场模板"
                        icon="copy"
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
                      {`模板「${effTemplateId()}」不存在`}
                    </Text>
                  </View>
                  <Text style={{ fontSize: 12, lineHeight: 16.5 }} class="text-secondary-foreground">
                    {`工作区中缺少 templates/${effTemplateId()}.typ，无法排版预览。`}
                  </Text>
                  <If when={!locked()}>
                    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6, paddingTop: 2 }}>
                      <If when={canInstallMissingPreset()}>
                        <Btn
                          variant="primary"
                          label="从预设安装"
                          icon="plus"
                          onPress={() => props.done(preview.installPresetTemplate(effTemplateId(), effTemplateId()))}
                        />
                      </If>
                      <Btn label="从预设添加模板…" icon="plus" onPress={props.onRequestPreset} />
                    </View>
                  </If>
                </View>
                <If when={props.catalog.templates.length > 0}>
                  <TemplatePicker
                    templates={props.catalog.templates}
                    value={effTemplateId()}
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
                本场专属模板
              </Text>
            </View>
            <Text style={{ fontSize: 12, lineHeight: 16.5 }} class="text-secondary-foreground">
              {localTplInfo()?.name
                ? `当前使用本场比赛独立模板「${localTplInfo()!.name}」，修改不会影响其他比赛。`
                : "当前使用本场比赛独立模板，修改不会影响其他比赛。"}
            </Text>
            <If when={!locked()}>
              <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
                <Btn
                  label="编辑本场模板"
                  icon="filePenLine"
                  onPress={() => void preview.openContestTemplateFile(c().id).then((res) => !res.ok && props.done(res))}
                />
                <Btn
                  label="恢复引用工作区模板"
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
          restoreTooltip={hasLocal() ? "恢复模板默认值" : "还原为工作区默认值"}
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
            <Section title={`${item().label} 题版式设置`} last={locked()}>
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
                restoreTooltip="还原为题库的值"
                hideGroupLabels
                onChange={(key, value) => over({ optionsJson: encodeSingleOptionPatch(optsNs(), key, value) })}
              />
            </Section>
          );
        }}
      </If>

      <If when={!locked()}>
        <Section title="进阶定制" last>
          <View style={{ gap: 6 }}>
            <If when={!hasLocal() && !!tplInfo()}>
              <Btn
                label="基于当前模板新建副本"
                icon="copy"
                onPress={() => props.onRequestFork(effTemplateId(), tplInfo()?.name ?? effTemplateId())}
              />
            </If>
            <Btn
              label="自定义样式代码"
              icon="code"
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
                恢复引用工作区模板？
              </Text>
              <Text style={{ fontSize: 12, lineHeight: 17 }} class="text-muted-foreground">
                将删除本场的专属模板文件（template.typ），改用工作区 templates/ 下的模板。
              </Text>
            </View>
          </View>
          <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
            <Btn label="取消" onPress={() => confirmRestoreOpen.set(false)} />
            <Btn
              label="恢复引用"
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
  catalog: TemplatesCatalog;
  done: (res: { ok: boolean; code?: string | null; error?: string | null }) => void;
}) {
  const d = computed(() => {
    void revision();
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
      <Section title="当前模板" last={!hasProblemFields() && wsReadOnly()}>
        <View class="border border-border bg-muted/40" style={{ borderRadius: 8, padding: 10, gap: 6 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <Icon name={tplInfo() ? "fileText" : "triangleAlert"} size={14} tone={tplInfo() ? "secondary-foreground" : "destructive"} />
            <Text style={{ flex: 1, fontSize: 13, fontWeight: 500 }} class="text-foreground">
              {tplInfo()?.name ?? `${tplId()}（缺失）`}
            </Text>
          </View>
          <Text style={{ fontSize: 12, lineHeight: 16.5 }} class="text-secondary-foreground">
            {tplInfo()
              ? "单题预览沿用工作区默认模板与版式参数。"
              : `工作区中缺少 templates/${tplId()}.typ，可从预设安装或在设置中切换模板。`}
          </Text>
          <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6, paddingTop: 2 }}>
            <If when={!wsReadOnly() && canInstallPreset()}>
              <Btn
                variant="primary"
                label="从预设安装"
                icon="plus"
                onPress={() => props.done(preview.installPresetTemplate(tplId(), tplId()))}
              />
            </If>
            <Btn label="调整工作区默认版式" icon="settings" onPress={() => goSettings("layout")} />
          </View>
        </View>
      </Section>

      <If when={hasProblemFields()}>
        <Section title="本题版式设置" last={wsReadOnly()}>
          <TemplateOptionsEditor
            ownerKey={`problem:${props.id}`}
            template={tplInfo()}
            scope="problem"
            baseValues={{}}
            overrideValues={probOpts()}
            fonts={props.catalog.fonts}
            readOnly={wsReadOnly()}
            restoreTooltip="恢复模板默认值"
            hideGroupLabels
            onChange={(key, value) => write({ templateOptionsJson: encodeSingleOptionPatch(tplId(), key, value) })}
          />
        </Section>
      </If>

      <If when={!wsReadOnly()}>
        <Section title="进阶定制" last>
          <Btn
            label="自定义样式代码"
            icon="code"
            onPress={() => void preview.openPreambleFile().then((res) => !res.ok && props.done(res))}
          />
        </Section>
      </If>
    </View>
  );
}
