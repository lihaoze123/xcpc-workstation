// The 样例 tab: each sample as a precision 8px card with a compact header bar (drag handle,
// `样例 01`, muted delete with lightweight undo) and a 50/50 split body for `.in` and `.ans`
// in `EDITOR_MONO` with 72–280 pt auto-height internal scrolling.

import { cleanup, effect, Scroll, signal, Text, untracked, View } from "nsh";
import { Input } from "nsh/base";
import { ErrorDisplay } from "../ErrorDisplay";
import { t } from "../i18n";
import { Icon } from "../Icon";
import { Btn, IconBtn, tip } from "../kit";
import { EmptyState } from "../layout";
import { cleanSample, moveItem, samplesEqual } from "../logic";
import { errorOf, preview, revision, type UiError } from "../store";
import { cardShadow, EDITOR_MONO, popoverShadow } from "../theme";
import { createReorder } from "./reorder";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "../tokens";
type Sample = { key: number; input: string; output: string };
const SAVE_AFTER = 500;
const UNDO_MS = 5000;
const CARD_GAP = 10;
let nextKey = 1;

export function Samples(props: { problemId: string; readOnly: boolean; onSaved: () => void }) {
  const list = signal<Sample[]>([]);
  const error = signal<UiError | undefined>(undefined);
  const deleted = signal<{ sample: Sample; index: number } | undefined>(undefined);
  let dirty = false;
  let timer: number | undefined;
  let undoTimer: number | undefined;
  let loadedFor = "";

  function load() {
    const d = preview.problemDetail(props.problemId);
    const next = d.samples.map((s) => ({ input: s.input, output: s.output }));
    if (samplesEqual(next, list())) return;
    list.set(next.map((s) => ({ key: nextKey++, ...s })));
  }

  // Another problem, or the files changed outside: read them again, unless edits wait
  // to be saved (they win; the save rewrites the set).
  effect(() => {
    const id = props.problemId;
    void revision();
    untracked(() => {
      if (id !== loadedFor) {
        flush();
        loadedFor = id;
        list.set([]);
        deleted.set(undefined);
        clearTimeout(undoTimer);
        dirty = false;
      }
      if (!dirty) load();
    });
  });

  function flush() {
    clearTimeout(timer);
    timer = undefined;
    if (!dirty || !loadedFor) return;
    dirty = false;
    const res = preview.saveSamples(loadedFor, list().map(({ input, output }) => ({ input, output })));
    error.set(errorOf(res, t("ctx.saveSamples")));
    if (res.ok) props.onSaved();
  }
  cleanup(() => {
    clearTimeout(undoTimer);
    flush();
  });

  function change(next: Sample[]) {
    list.set(next);
    r.prune(next.length);
    dirty = true;
    clearTimeout(timer);
    timer = setTimeout(flush, SAVE_AFTER);
  }

  const update = (key: number, patch: Partial<Sample>) => change(list().map((s) => (s.key === key ? { ...s, ...patch } : s)));
  const addSample = () => {
    deleted.set(undefined);
    change([...list(), { key: nextKey++, input: "", output: "" }]);
  };
  const removeSample = (sample: Sample, index: number) => {
    clearTimeout(undoTimer);
    deleted.set({ sample, index });
    change(list().filter((x) => x.key !== sample.key));
    undoTimer = setTimeout(() => deleted.set(undefined), UNDO_MS);
  };
  const undoDelete = () => {
    const d = deleted();
    if (!d) return;
    clearTimeout(undoTimer);
    deleted.set(undefined);
    const next = list().slice();
    next.splice(Math.min(d.index, next.length), 0, d.sample);
    change(next);
  };

  const r = createReorder((from, to) => change(moveItem(list(), from, to)), CARD_GAP);
  const canDrag = () => !props.readOnly && list().length > 1;

  return (
    <View
      style={{ flex: 1 }}
      class="bg-card"
      onMouseMove={(e) => r.move(e)}
      onMouseUp={(e) => r.end(e)}
      onMouseLeave={(e) => r.leave(e)}
    >
      <If
        when={list().length > 0}
        else={
          <View style={{ flex: 1 }}>
            <EmptyState
              icon="fileText"
              title={t("samples.emptyTitle")}
              hint={props.readOnly ? t("samples.emptyReadOnlyHint") : t("samples.emptyHint")}
              action={
                !props.readOnly ? (
                  <Btn label={t("samples.add")} icon="plus" variant="primary" onPress={addSample} />
                ) : undefined
              }
            />
            <If when={deleted()}>
              {(d) => (
                <View style={{ padding: SPACE[12] }}>
                  <UndoBar index={d().index} onUndo={undoDelete} onDismiss={() => deleted.set(undefined)} />
                </View>
              )}
            </If>
          </View>
        }
      >
        <Scroll style={{ flex: 1 }}>
          <View ref={(v) => r.container(v)} style={{ padding: SPACE[12], gap: CARD_GAP }}>
            <Each of={list()} key={(s) => s.key}>
              {(s, index) => {
                const dragging = () => r.drag()?.from === index();
                return (
                  <View
                    ref={(v) => r.row(index(), v)}
                    class="border border-border bg-card"
                    style={{
                      borderRadius: RADIUS.md,
                      overflow: "hidden",
                      zIndex: dragging() ? 2 : 0,
                      transform: dragging() ? `translateY(${r.offset()}px)` : "none",
                      boxShadow: dragging() ? popoverShadow() : cardShadow(),
                    }}
                  >
                    <View
                      class="bg-muted/50 border-b border-border"
                      style={{
                        height: CONTROL.lg,
                        paddingLeft: SPACE[12],
                        paddingRight: SPACE[6],
                        flexDirection: "row",
                        alignItems: "center",
                        justifyContent: "space-between",
                      }}
                    >
                      <View
                        onPressDown={(e) => canDrag() && r.pressDown(index(), e, list().length)}
                        style={{
                          flexDirection: "row",
                          alignItems: "center",
                          gap: SPACE[6],
                          height: CONTROL.md,
                          paddingRight: SPACE[8],
                          cursor: canDrag() ? "grab" : "default",
                        }}
                      >
                        <If when={canDrag()}>
                          <Icon name="gripVertical" size="sm" tone="muted-foreground" />
                        </If>
                        <Text
                          style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }}
                          class="text-secondary-foreground"
                        >
                          {t("samples.cardTitle", { num: String(index() + 1).padStart(2, "0") })}
                        </Text>
                      </View>
                      <If when={!props.readOnly}>
                        <IconBtn
                          icon="trash"
                          size="sm"
                          tone="muted-foreground"
                          tooltip={tip(t("samples.deleteTooltip"))}
                          onPress={() => removeSample(s(), index())}
                        />
                      </If>
                    </View>
                    <View style={{ flexDirection: "row" }}>
                      <View style={{ flex: 1, minWidth: 0 }}>
                        <View style={{ paddingLeft: SPACE[12], paddingTop: SPACE[6], paddingBottom: SPACE[2] }}>
                          <Text style={{ fontSize: FONT_SIZE.micro }} class="text-muted-foreground">
                            {t("samples.input")}
                          </Text>
                        </View>
                        <SampleField
                          value={s().input}
                          placeholder={t("samples.input")}
                          readOnly={props.readOnly}
                          onChange={(v) => update(s().key, { input: v })}
                        />
                      </View>
                      <View style={{ width: 1 }} class="bg-border" />
                      <View style={{ flex: 1, minWidth: 0 }}>
                        <View style={{ paddingLeft: SPACE[12], paddingTop: SPACE[6], paddingBottom: SPACE[2] }}>
                          <Text style={{ fontSize: FONT_SIZE.micro }} class="text-muted-foreground">
                            {t("samples.output")}
                          </Text>
                        </View>
                        <SampleField
                          value={s().output}
                          placeholder={t("samples.output")}
                          readOnly={props.readOnly}
                          onChange={(v) => update(s().key, { output: v })}
                        />
                      </View>
                    </View>
                  </View>
                );
              }}
            </Each>
            <If when={r.line() !== undefined}>
              <View
                style={{ position: "absolute", left: 12, right: 12, top: r.line()!, height: 2, borderRadius: 1 }} // token-exempt: 2px drag-drop insertion line
                class="bg-primary"
              />
            </If>
            <If when={deleted()}>
              {(d) => <UndoBar index={d().index} onUndo={undoDelete} onDismiss={() => deleted.set(undefined)} />}
            </If>
            <If when={!props.readOnly}>
              <View style={{ flexDirection: "row" }}>
                <Btn label={t("samples.add")} icon="plus" onPress={addSample} />
              </View>
            </If>
            <ErrorDisplay error={error()} onDismiss={() => error.set(undefined)} />
          </View>
        </Scroll>
      </If>
    </View>
  );
}

function UndoBar(props: { index: number; onUndo: () => void; onDismiss: () => void }) {
  return (
    <View
      class="bg-muted/70 border border-border"
      style={{
        height: CONTROL.lg,
        borderRadius: RADIUS.md,
        paddingLeft: SPACE[12],
        paddingRight: SPACE[6],
        flexDirection: "row",
        alignItems: "center",
        gap: SPACE[6],
      }}
    >
      <Text style={{ flex: 1, fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
        {t("samples.deletedBanner", { num: String(props.index + 1).padStart(2, "0") })}
      </Text>
      <Btn label={t("common.undo")} variant="ghost" onPress={props.onUndo} />
      <IconBtn icon="x" size="sm" tone="muted-foreground" onPress={props.onDismiss} />
    </View>
  );
}

function SampleField(props: { value: string; placeholder: string; readOnly: boolean; onChange: (v: string) => void }) {
  const height = signal(72);
  return (
    <Input
      multiline
      readonly={props.readOnly}
      value={props.value}
      placeholder={props.placeholder}
      autoCapitalize="none"
      autoCorrect={false}
      onContentSizeChange={(e) => {
        const next = Math.max(72, Math.min(280, Math.ceil(e.size.height) + 14));
        if (next !== height()) height.set(next);
      }}
      onChange={(e) => {
        const raw = e.value;
        props.onChange(raw.includes("\r") || raw.includes("\uFEFF") ? cleanSample(raw) : raw);
      }}
      class={
        props.readOnly
          ? "w-full px-2.5 py-1.5 bg-muted/50 text-secondary-foreground placeholder:text-muted-foreground"
          : "w-full px-2.5 py-1.5 bg-transparent placeholder:text-muted-foreground focus:ring-1 focus:ring-inset focus:ring-ring transition-colors"
      }
      style={{ fontFamily: EDITOR_MONO, fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.body, height: height() }}
    />
  );
}
