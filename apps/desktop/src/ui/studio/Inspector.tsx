// The publishing drawer on the right of the studio: it slides over the preview with a
// `translateX` (never squeezing the editor) and writes where the context says: a lone
// problem's `problem.toml`, or the contest's metadata and the current problem's
// overrides. An overridden field has a thin bar on its left and a button to restore the
// library's value; setting the library's value back drops the override. A frozen
// contest's drawer renders each control in its own read-only state (never dimming the
// whole section) with 重新编辑 in the status card.

import { cleanup, computed, effect, Scroll, signal, Text, untracked, View, type JSX } from "nsh";
import { Input, Switch } from "nsh/base";
import type { ContestDetailJs, ContestItemDetailJs, PreviewDiagnostic } from "xcpc-workstation/native";
import { freezeBlockedCopy, type Freeze } from "../Contest";
import { ErrorDisplay } from "../ErrorDisplay";
import { Icon } from "../Icon";
import { Btn, DelayedSpinner, IconBtn, Segmented, tip } from "../kit";
import { errorOf, info, preview, refresh, revision, unfreezeFor, type UiError } from "../store";
import { EASE_DRAWER, ms } from "../theme";

export const INSPECTOR_WIDTH = 300;

export function Inspector(props: {
  open: boolean;
  target: "problem" | "contest";
  id: string;
  contest?: ContestDetailJs;
  item?: ContestItemDetailJs;
  freeze: Freeze;
  onChanged: () => void;
  onFinding: (d: PreviewDiagnostic) => void;
  onClose: () => void;
}) {
  const error = signal<UiError | undefined>(undefined);
  effect(() => {
    void props.target;
    void props.id;
    untracked(() => error.set(undefined));
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
        transform: props.open ? "translateX(0px)" : `translateX(${INSPECTOR_WIDTH + 1}px)`,
        transitionProperty: "transform",
        transitionDuration: ms(170),
        transitionTimingFunction: EASE_DRAWER,
        pointerEvents: props.open ? "auto" : "none",
      }}
      class="border-border bg-background"
    >
      <View style={{ height: 42, flexDirection: "row", alignItems: "center", paddingLeft: 14, paddingRight: 8, borderBottomWidth: 1 }} class="border-border">
        <Text style={{ flex: 1, fontSize: 13, fontWeight: 600 }}>属性</Text>
        <IconBtn icon="x" tooltip={tip("关闭", "mod+i")} onPress={props.onClose} />
      </View>
      <Scroll style={{ flex: 1 }}>
        <If when={props.target === "contest" && props.contest}>
          <ContestFields contest={props.contest!} item={props.item} freeze={props.freeze} done={done} onFinding={props.onFinding} />
        </If>
        <If when={props.target === "problem"}>
          <ProblemFields id={props.id} done={done} />
        </If>
        <If when={error()}>
          <View style={{ paddingLeft: 14, paddingRight: 14, paddingBottom: 14 }}>
            <ErrorDisplay error={error()} onDismiss={() => error.set(undefined)} />
          </View>
        </If>
      </Scroll>
    </View>
  );
}

function Section(props: { title?: string; children?: JSX.Element; last?: boolean }) {
  return (
    <View style={{ padding: 14, gap: 10, borderBottomWidth: props.last ? 0 : 1 }} class="border-border">
      {props.title && (
        <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: 12, fontWeight: 500 }} class="text-secondary-foreground">
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

function ProblemFields(props: { id: string; done: (res: { ok: boolean; code?: string | null; error?: string | null }) => void }) {
  const d = computed(() => {
    void revision();
    return preview.problemDetail(props.id);
  });
  const wsReadOnly = () => info().readOnly;
  const write = (patch: Parameters<typeof preview.updateProblemMeta>[1]) => !wsReadOnly() && props.done(preview.updateProblemMeta(props.id, patch));
  const owner = () => `problem:${props.id}`;
  return (
    <Section title="题目信息" last>
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

function ContestFields(props: {
  contest: ContestDetailJs;
  item?: ContestItemDetailJs;
  freeze: Freeze;
  done: (res: { ok: boolean; code?: string | null; error?: string | null }) => void;
  onFinding: (d: PreviewDiagnostic) => void;
}) {
  const c = () => props.contest;
  const frozen = () => c().frozen;
  const wsReadOnly = () => info().readOnly;
  const locked = () => frozen() || wsReadOnly();
  const meta = (patch: Parameters<typeof preview.updateContestMeta>[1]) => !locked() && props.done(preview.updateContestMeta(c().id, patch));
  const over = (patch: Parameters<typeof preview.setOverride>[2]) => !locked() && props.item && props.done(preview.setOverride(c().id, props.item.label, patch));
  const f = props.freeze;
  const cover = () => (c().cover == null ? "auto" : c().cover ? "on" : "off");
  const contestOwner = () => `contest:${c().id}`;

  return (
    <View>
      <Section>
        <View class="border border-border bg-muted/40" style={{ borderRadius: 8, padding: 10, gap: 6 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <Icon name={frozen() ? "lock" : "stamp"} size={14} tone={frozen() ? "secondary-foreground" : "foreground"} />
            <Text style={{ flex: 1, fontSize: 12.5, fontWeight: 500 }}>
              {frozen() ? "已定稿" : "未定稿"}
            </Text>
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
          </View>
          <Text style={{ fontSize: 12, lineHeight: 17 }} class="text-secondary-foreground">
            {frozen() ? "内容已锁定，仅支持查看和导出" : "定稿后，题库里的修改不再影响这场比赛"}
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
      <Section title="比赛信息" last={!props.item}>
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
        <View style={{ flexDirection: "row", alignItems: "center", height: 28 }}>
          <Text style={{ flex: 1, fontSize: 13 }} class={locked() ? "text-secondary-foreground" : "text-foreground"}>
            封面
          </Text>
          <Segmented
            disabled={locked()}
            items={[
              { value: "auto", label: "自动" },
              { value: "on", label: "开" },
              { value: "off", label: "关" },
            ]}
            itemWidth={42}
            value={cover()}
            onChange={(v) => !locked() && meta({ cover: v === "auto" ? null : v === "on" })}
          />
        </View>
        <Toggle label="显示时间和内存限制" readOnly={locked()} checked={c().showMetadata} onChange={(v) => meta({ showMetadata: v })} />
        <Toggle label="补空白页，方便双面打印" readOnly={locked()} checked={c().blankPages} onChange={(v) => meta({ blankPages: v })} />
      </Section>
      <If when={props.item}>
        {(item) => {
          const itemOwner = () => `contest:${c().id}:item:${item().label}:${item().id}`;
          return (
            <Section title={`${item().label} 题在这场比赛中的设置`} last>
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
