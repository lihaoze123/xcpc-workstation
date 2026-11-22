// The few dialogs: a new problem, a new contest, adding a problem to a contest, deleting one.
// Errors replace the field hint in place (§8.3), and Return submits.

import { batch, cleanup, effect, Scroll, signal, Text, untracked, View, type InputRef, type KeyEvent, type ViewRef } from "nsh";
import { Input } from "nsh/base";
import { Btn } from "./kit";
import { t } from "./i18n";
import { ErrorDisplay } from "./ErrorDisplay";
import { DialogHeader, Modal } from "./Modal";
import { Icon } from "./Icon";
import { errorCopy, filterAddableContests, suggestDuplicateId, validId } from "./logic";
import { PickProblemsBody } from "./PickProblemsModal";
import { EASE_STANDARD, MONO, ms } from "./theme";
import {
  dialog,
  errorOf,
  goSpace,
  info,
  jumpToContest,
  openStudio,
  preview,
  refresh,
  selectedContest,
  selectedProblem,
  type UiError,
} from "./store";

const message = (e: UiError | undefined) => {
  if (!e) return undefined;
  if (e.code === "invalid_id") return t("dialogs.invalidId");
  if (e.code === "conflict") return t("dialogs.duplicateIdConflict");
  return errorCopy(e.code, e.error).message;
};

/** Turn an English title into an editable slug suggestion; non-ASCII yields "". */
function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48);
}

const INPUT_BASE = "h-8 px-2.5 rounded-md border bg-card text-[13px] placeholder:text-muted-foreground focus:ring-1 focus:ring-inset focus:ring-ring transition-colors";

function Field(props: {
  label: string;
  hint?: string;
  error?: string;
  value: string;
  placeholder?: string;
  autoFocus?: boolean;
  mono?: boolean;
  inputRef?: (ref: InputRef) => void;
  onChange: (v: string) => void;
  onSubmit: () => void;
}) {
  return (
    <View style={{ gap: 6 }}>
      <View style={{ flexDirection: "row", alignItems: "baseline", gap: 8 }}>
        <Text style={{ fontSize: 12.5, fontWeight: 500 }}>{props.label}</Text>
        {(props.error || props.hint) && (
          <Text style={{ flexShrink: 1, numberOfLines: 1, textOverflow: "ellipsis", fontSize: 12 }} class={props.error ? "text-destructive" : "text-muted-foreground"}>
            {props.error ?? props.hint}
          </Text>
        )}
      </View>
      <Input
        ref={props.inputRef}
        class={`${INPUT_BASE} ${props.error ? "border-destructive" : "border-border"}`}
        style={props.mono ? { fontFamily: MONO } : undefined}
        value={props.value}
        placeholder={props.placeholder}
        autoFocus={props.autoFocus}
        autoCapitalize="none"
        autoCorrect={false}
        submitBehavior="submit"
        onChange={(e) => props.onChange(e.value)}
        onSubmit={() => props.onSubmit()}
      />
    </View>
  );
}

/** A small form: its fields, then 取消 and the submit button. What went wrong takes the
 * place of the ID field's hint, so nothing moves and no room is kept empty for it. */
function Form(props: { head: any; submit: string; onSubmit: () => void; children?: any }) {
  return (
    <View style={{ padding: 20, gap: 16 }}>
      {props.head}
      <View style={{ gap: 12 }}>{props.children}</View>
      <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8, marginTop: 4 }}>
        <Btn label={t("common.cancel")} variant="ghost" onPress={() => dialog.set(undefined)} />
        <Btn variant="primary" label={props.submit} onPress={() => props.onSubmit()} style={{ minWidth: 72 }} />
      </View>
    </View>
  );
}

function NewProblem(props: { addTo?: string }) {
  const id = signal("");
  const title = signal("");
  const idTouched = signal(false);
  const error = signal<UiError | undefined>(undefined);
  let idInput: InputRef | undefined;

  const submit = () => {
    const pid = id().trim();
    if (!validId(pid)) return error.set({ code: "invalid_id" });
    const res = preview.createProblem(pid, title().trim() || pid);
    if (!res.ok) return error.set(errorOf(res));
    if (props.addTo) {
      const c = info().contests.find((x) => x.id === props.addTo);
      if (c) {
        const cRes = preview.composeContest(c.id, [...c.items.map((i) => i.id), pid]);
        if (!cRes.ok) {
          refresh();
          return error.set(errorOf(cRes));
        }
      }
    }
    refresh();
    dialog.set(undefined);
    openStudio({ target: "problem", id: pid });
  };

  const onTitleSubmit = () => {
    if (!id().trim()) {
      idInput?.focus();
      return;
    }
    submit();
  };

  return (
    <Form
      head={<DialogHeader icon="filePenLine" title={t("dialogs.newProblem.title")} description={t("dialogs.newProblem.desc")} />}
      submit={t("common.create")}
      onSubmit={submit}
    >
      <Field
        label={t("dialogs.newProblem.titleLabel")}
        value={title()}
        placeholder={t("dialogs.newProblem.titlePlaceholder")}
        autoFocus
        onChange={(v) =>
          batch(() => {
            title.set(v);
            if (!idTouched()) {
              id.set(slugify(v));
              error.set(undefined);
            }
          })
        }
        onSubmit={onTitleSubmit}
      />
      <Field
        label={t("dialogs.newProblem.idLabel")}
        hint={t("dialogs.newProblem.idHint")}
        error={message(error())}
        value={id()}
        placeholder="two-sum"
        mono
        inputRef={(r) => {
          idInput = r;
        }}
        onChange={(v) =>
          batch(() => {
            idTouched.set(true);
            id.set(v.toLowerCase());
            error.set(undefined);
          })
        }
        onSubmit={submit}
      />
    </Form>
  );
}

function DuplicateProblem(props: { open: boolean; problemId: string }) {
  const id = signal("");
  const title = signal("");
  const error = signal<UiError | undefined>(undefined);
  let idInput: InputRef | undefined;

  effect(() => {
    const isOpen = props.open;
    const sourceId = props.problemId;
    if (!isOpen || !sourceId) return;
    untracked(() => {
      const ws = info();
      const source = ws.problems.find((p) => p.id === sourceId);
      const sourceTitle = source?.title || sourceId;
      batch(() => {
        title.set(t("dialogs.duplicateProblem.copySuffix", { title: sourceTitle }));
        id.set(suggestDuplicateId(sourceId, ws.problems.map((p) => p.id)));
        error.set(undefined);
      });
    });
  });

  const submit = () => {
    const pid = id().trim();
    if (!validId(pid)) return error.set({ code: "invalid_id" });
    const nextTitle = title().trim();
    const res = preview.duplicateProblem(props.problemId, pid, nextTitle || null);
    if (!res.ok) return error.set(errorOf(res));
    refresh();
    dialog.set(undefined);
    selectedProblem.set(pid);
  };

  const onTitleSubmit = () => {
    if (!id().trim()) {
      idInput?.focus();
      return;
    }
    submit();
  };

  return (
    <Form
      head={<DialogHeader icon="copy" title={t("dialogs.duplicateProblem.title")} description={t("dialogs.duplicateProblem.desc")} />}
      submit={t("dialogs.duplicateProblem.submit")}
      onSubmit={submit}
    >
      <Field
        label={t("dialogs.newProblem.titleLabel")}
        value={title()}
        placeholder={t("dialogs.duplicateProblem.titlePlaceholder")}
        autoFocus
        onChange={(v) => title.set(v)}
        onSubmit={onTitleSubmit}
      />
      <Field
        label={t("dialogs.newProblem.idLabel")}
        hint={t("dialogs.duplicateProblem.idHint")}
        error={message(error())}
        value={id()}
        placeholder="problem-copy"
        mono
        inputRef={(r) => {
          idInput = r;
        }}
        onChange={(v) =>
          batch(() => {
            id.set(v.toLowerCase());
            error.set(undefined);
          })
        }
        onSubmit={submit}
      />
    </Form>
  );
}

function NewContest(props: { withProblem?: string }) {
  const id = signal("");
  const name = signal("");
  const idTouched = signal(false);
  const error = signal<UiError | undefined>(undefined);
  let idInput: InputRef | undefined;

  const submit = () => {
    const cid = id().trim();
    if (!validId(cid)) return error.set({ code: "invalid_id" });
    const res = preview.createContest(cid, name().trim() || cid);
    if (!res.ok) return error.set(errorOf(res));
    if (props.withProblem) {
      const cRes = preview.composeContest(cid, [props.withProblem]);
      if (!cRes.ok) {
        refresh();
        return error.set(errorOf(cRes));
      }
    }
    refresh();
    dialog.set(undefined);
    selectedContest.set(cid);
    goSpace("contests");
  };

  const onNameSubmit = () => {
    if (!id().trim()) {
      idInput?.focus();
      return;
    }
    submit();
  };

  return (
    <Form
      head={<DialogHeader icon="trophy" title={t("dialogs.newContest.title")} description={t("dialogs.newContest.desc")} />}
      submit={t("common.create")}
      onSubmit={submit}
    >
      <Field
        label={t("dialogs.newContest.nameLabel")}
        value={name()}
        placeholder={t("dialogs.newContest.namePlaceholder")}
        autoFocus
        onChange={(v) =>
          batch(() => {
            name.set(v);
            if (!idTouched()) {
              id.set(slugify(v));
              error.set(undefined);
            }
          })
        }
        onSubmit={onNameSubmit}
      />
      <Field
        label={t("dialogs.newProblem.idLabel")}
        hint={t("dialogs.newContest.idHint")}
        error={message(error())}
        value={id()}
        placeholder="spring-2026"
        mono
        inputRef={(r) => {
          idInput = r;
        }}
        onChange={(v) =>
          batch(() => {
            idTouched.set(true);
            id.set(v.toLowerCase());
            error.set(undefined);
          })
        }
        onSubmit={submit}
      />
    </Form>
  );
}

function AddToContest(props: { problemId: string }) {
  const error = signal<UiError | undefined>(undefined);
  const contests = () => filterAddableContests(info().contests);
  const isDisabled = (c: (typeof info extends () => { contests: (infer C)[] } ? C : never)) =>
    c.frozen || c.items.length >= 26 || c.items.some((i) => i.id === props.problemId);

  // Highlighted index across `0..contests.length - 1` (contests) and `contests.length` ("新建比赛").
  const enabledIndices = () => {
    const list = contests();
    const out: number[] = [];
    for (let i = 0; i < list.length; i++) {
      if (!isDisabled(list[i])) out.push(i);
    }
    out.push(list.length); // "新建比赛" is always enabled
    return out;
  };

  const highlighted = signal<number>(enabledIndices()[0] ?? 0);
  let rootRef: ViewRef | undefined;
  const raf = requestAnimationFrame(() => rootRef?.focus());
  cleanup(() => cancelAnimationFrame(raf));

  const add = (cid: string) => {
    const c = contests().find((x) => x.id === cid);
    if (!c) return;
    const res = preview.composeContest(cid, [...c.items.map((i) => i.id), props.problemId]);
    if (!res.ok) return error.set(errorOf(res));
    refresh();
    dialog.set(undefined);
  };

  const activate = (idx: number) => {
    const list = contests();
    if (idx === list.length) {
      dialog.set({ kind: "newContest", withProblem: props.problemId });
      return;
    }
    const c = list[idx];
    if (c && !isDisabled(c)) add(c.id);
  };

  const onKeyDown = (e: KeyEvent) => {
    const valid = enabledIndices();
    if (valid.length === 0) return;
    if (e.key === "ArrowDown") {
      const pos = valid.indexOf(highlighted());
      highlighted.set(valid[(pos + 1) % valid.length]);
    } else if (e.key === "ArrowUp") {
      const pos = valid.indexOf(highlighted());
      highlighted.set(valid[(pos - 1 + valid.length) % valid.length]);
    } else if (e.key === "Enter") {
      activate(highlighted());
    }
  };

  return (
    <View
      ref={(r) => {
        rootRef = r;
      }}
      focusable
      onKeyDown={onKeyDown}
      style={{ padding: 6, gap: 2 }}
    >
      <View style={{ padding: 14, paddingBottom: 10 }}>
        <DialogHeader icon="listPlus" title={t("dialogs.addToContest.title")} description={t("dialogs.addToContest.desc")} />
      </View>
      <If when={contests().length > 0}>
        <Scroll style={{ maxHeight: 280 }}>
          <View style={{ gap: 2 }}>
            <Each of={contests()} key={(c) => c.id}>
              {(c, i) => {
                const has = () => c().items.some((x) => x.id === props.problemId);
                const full = () => c().items.length >= 26;
                const off = () => c().frozen || has() || full();
                const focused = () => !off() && highlighted() === i();
                return (
                  <View
                    disabled={off()}
                    onPress={() => {
                      if (!off()) add(c().id);
                    }}
                    onMouseMove={() => {
                      if (!off() && highlighted() !== i()) highlighted.set(i());
                    }}
                    class={off() ? "" : focused() ? "bg-accent active:bg-accent" : "hover:bg-muted active:bg-accent"}
                    style={{
                      height: 34,
                      paddingLeft: 10,
                      paddingRight: 10,
                      borderRadius: 6,
                      flexDirection: "row",
                      alignItems: "center",
                      gap: 8,
                      opacity: off() ? 0.55 : 1,
                      transitionProperty: "colors",
                      transitionDuration: ms(120),
                      transitionTimingFunction: EASE_STANDARD,
                    }}
                  >
                    <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", flex: 1, fontSize: 13 }} class={off() ? "text-muted-foreground" : "text-foreground"}>
                      {c().name}
                    </Text>
                    {c().frozen && <Icon name="lock" size={13} tone="muted-foreground" />}
                    <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                      {has()
                        ? t("dialogs.addToContest.alreadyIn")
                        : c().frozen
                          ? t("contests.finalized")
                          : full()
                            ? t("dialogs.addToContest.full")
                            : t("contests.problemCount", { count: c().items.length })}
                    </Text>
                  </View>
                );
              }}
            </Each>
          </View>
        </Scroll>
        <View style={{ height: 1, marginTop: 4, marginBottom: 4, marginLeft: 4, marginRight: 4 }} class="bg-border" />
      </If>
      <View
        onPress={() => dialog.set({ kind: "newContest", withProblem: props.problemId })}
        onMouseMove={() => {
          const newIdx = contests().length;
          if (highlighted() !== newIdx) highlighted.set(newIdx);
        }}
        class={highlighted() === contests().length ? "bg-accent active:bg-accent" : "hover:bg-muted active:bg-accent"}
        style={{
          height: 34,
          paddingLeft: 10,
          paddingRight: 10,
          borderRadius: 6,
          flexDirection: "row",
          alignItems: "center",
          gap: 8,
          transitionProperty: "colors",
          transitionDuration: ms(120),
          transitionTimingFunction: EASE_STANDARD,
        }}
      >
        <Icon name="plus" size={15} tone="primary" />
        <Text style={{ fontSize: 13 }} class="text-primary">
          {t("contests.newContest")}
        </Text>
      </View>
      <If when={error()}>
        <View style={{ paddingLeft: 10, paddingRight: 10, paddingTop: 4, paddingBottom: 4 }}>
          <ErrorDisplay error={error()} onDismiss={() => error.set(undefined)} />
        </View>
      </If>
    </View>
  );
}

function DeleteProblem(props: { problemId: string; onJumpContest?: (contestId: string) => void }) {
  const error = signal<UiError | undefined>(undefined);
  const p = () => info().problems.find((x) => x.id === props.problemId);
  // Only unfrozen contests block deletion; finalized contests keep their own snapshot.
  const used = () => info().contests.filter((c) => !c.frozen && c.items.some((i) => i.id === props.problemId));
  const remove = () => {
    const res = preview.deleteProblem(props.problemId);
    if (!res.ok) return error.set(errorOf(res));
    dialog.set(undefined);
    selectedProblem.set(undefined);
    refresh();
  };
  const jumpContest = (cid: string) => {
    dialog.set(undefined);
    if (props.onJumpContest) {
      props.onJumpContest(cid);
    } else {
      jumpToContest(cid);
    }
  };
  return (
    <View style={{ padding: 20, gap: 16 }}>
      <If
        when={used().length === 0}
        else={
          <View style={{ gap: 12 }}>
            <DialogHeader
              icon="triangleAlert"
              tone="destructive"
              title={t("dialogs.deleteProblem.blockedTitle", { title: p()?.title ?? props.problemId })}
              description={t("dialogs.deleteProblem.blockedDesc")}
            />
            <View style={{ gap: 4, paddingLeft: 44 }}>
              <Each of={used()} key={(c) => c.id}>
                {(c) => (
                  <View
                    onPress={() => jumpContest(c().id)}
                    class="border border-border bg-muted/50 hover:bg-muted active:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
                    style={{
                      height: 32,
                      paddingLeft: 10,
                      paddingRight: 10,
                      borderRadius: 6,
                      flexDirection: "row",
                      alignItems: "center",
                      gap: 8,
                      transitionProperty: "colors",
                      transitionDuration: ms(120),
                      transitionTimingFunction: EASE_STANDARD,
                    }}
                  >
                    <Icon name="trophy" size={14} tone="secondary-foreground" />
                    <Text style={{ flex: 1, fontSize: 13, numberOfLines: 1, textOverflow: "ellipsis" }} class="text-foreground">
                      {c().name}
                    </Text>
                    <Icon name="arrowRight" size={13} tone="muted-foreground" />
                  </View>
                )}
              </Each>
            </View>
          </View>
        }
      >
        <DialogHeader
          icon="trash"
          tone="destructive"
          title={t("dialogs.deleteProblem.confirmTitle", { title: p()?.title ?? props.problemId })}
          description={t("dialogs.deleteProblem.confirmDesc")}
        />
      </If>
      <If when={error()}>
        <ErrorDisplay error={error()} onDismiss={() => error.set(undefined)} />
      </If>
      <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
        <Btn label={used().length === 0 ? t("common.cancel") : t("common.gotIt")} variant="ghost" onPress={() => dialog.set(undefined)} />
        <If when={used().length === 0}>
          <Btn label={t("common.delete")} variant="danger" onPress={remove} testID="confirm-delete" />
        </If>
      </View>
    </View>
  );
}

export function Dialogs(props: { onJumpContest?: (contestId: string) => void } = {}) {
  const d = () => dialog();
  const close = () => dialog.set(undefined);

  // Cache last active payload so Modal's 140 ms exit fade does not flash blank labels.
  const lastAddTo = signal<string | undefined>(undefined);
  const lastDuplicateProblemId = signal("");
  const lastWithProblem = signal<string | undefined>(undefined);
  const lastAddProblemId = signal("");
  const lastPickContestId = signal("");
  const lastDeleteProblemId = signal("");
  effect(() => {
    const cur = d();
    if (!cur) return;
    if (cur.kind === "newProblem") lastAddTo.set(cur.addTo);
    else if (cur.kind === "duplicateProblem") lastDuplicateProblemId.set(cur.problemId);
    else if (cur.kind === "newContest") lastWithProblem.set(cur.withProblem);
    else if (cur.kind === "addToContest") lastAddProblemId.set(cur.problemId);
    else if (cur.kind === "pickProblems") lastPickContestId.set(cur.contestId);
    else if (cur.kind === "deleteProblem") lastDeleteProblemId.set(cur.problemId);
  });

  return (
    <>
      <Modal open={d()?.kind === "newProblem"} onClose={close} testID="dialog-new-problem">
        <NewProblem addTo={lastAddTo()} />
      </Modal>
      <Modal open={d()?.kind === "duplicateProblem"} onClose={close} testID="dialog-duplicate-problem">
        <DuplicateProblem open={d()?.kind === "duplicateProblem"} problemId={lastDuplicateProblemId()} />
      </Modal>
      <Modal open={d()?.kind === "newContest"} onClose={close} testID="dialog-new-contest">
        <NewContest withProblem={lastWithProblem()} />
      </Modal>
      <Modal open={d()?.kind === "addToContest"} onClose={close} width={360} testID="dialog-add-to-contest">
        <AddToContest problemId={lastAddProblemId()} />
      </Modal>
      <Modal open={d()?.kind === "pickProblems"} onClose={close} width={640} testID="dialog-pick-problems">
        <PickProblemsBody contestId={lastPickContestId()} />
      </Modal>
      <Modal open={d()?.kind === "deleteProblem"} onClose={close} width={420} testID="dialog-delete-problem">
        <DeleteProblem problemId={lastDeleteProblemId()} onJumpContest={props.onJumpContest} />
      </Modal>
    </>
  );
}
