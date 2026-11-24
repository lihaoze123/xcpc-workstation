// The few dialogs: a new problem, a new contest, adding a problem to a contest, deleting one.
// Errors replace the field hint in place (§8.3), and Return submits.

import { batch, cleanup, computed, effect, Scroll, signal, Text, untracked, View, type InputRef, type KeyEvent, type ViewRef } from "nsh";
import { Input } from "nsh/base";
import { Btn, Chip, DelayedSpinner, Segmented } from "./kit";
import type { ResolvedProblemUrlJs } from "xcpc-workstation/native";
import { t } from "./i18n";
import { ErrorDisplay } from "./ErrorDisplay";
import { DialogHeader, Modal } from "./Modal";
import { Icon } from "./Icon";
import { errorCopy, filterAddableContests, partitionBatchDelete, suggestDuplicateId, validId } from "./logic";
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
  startBatchExportTask,
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
  const url = signal("");
  const id = signal("");
  const title = signal("");
  const idTouched = signal(false);
  const titleTouched = signal(false);
  const resolvingUrl = signal(false);
  const resolved = signal<ResolvedProblemUrlJs | undefined>(undefined);
  const urlError = signal<string | undefined>(undefined);
  const error = signal<UiError | undefined>(undefined);
  let idInput: InputRef | undefined;
  let urlTimer: number | undefined;
  let resolveSeq = 0;

  cleanup(() => {
    clearTimeout(urlTimer);
  });

  const triggerResolveUrl = (rawUrl: string) => {
    const trimmed = rawUrl.trim();
    clearTimeout(urlTimer);
    if (!trimmed) {
      batch(() => {
        resolved.set(undefined);
        urlError.set(undefined);
        resolvingUrl.set(false);
      });
      return;
    }
    if (!trimmed.startsWith("http://") && !trimmed.startsWith("https://")) {
      return;
    }
    const seq = ++resolveSeq;
    urlTimer = setTimeout(() => {
      resolvingUrl.set(true);
      urlError.set(undefined);
      void preview
        .resolveProblemUrl(trimmed)
        .then((res) => {
          if (seq !== resolveSeq) return;
          resolvingUrl.set(false);
          if (!res.ok) {
            resolved.set(undefined);
            urlError.set(res.error ?? t("dialogs.newProblem.urlUnsupported"));
            return;
          }
          batch(() => {
            resolved.set(res);
            urlError.set(undefined);
            error.set(undefined);
            if (!idTouched() && res.suggestedSlug) {
              id.set(res.suggestedSlug);
            }
            if (!titleTouched() && res.title) {
              title.set(res.title);
            }
          });
        })
        .catch(() => {
          if (seq !== resolveSeq) return;
          resolvingUrl.set(false);
        });
    }, 240);
  };

  const submit = () => {
    const pid = id().trim();
    if (!validId(pid)) return error.set({ code: "invalid_id" });
    const meta = resolved();
    let createdId = pid;
    if (meta && meta.ok) {
      const res = preview.createProblemFromUrl(
        pid,
        title().trim() || meta.title || pid,
        meta.platform ?? "",
        meta.problemKey ?? "",
        meta.url || url().trim(),
        meta.tags ?? [],
        meta.rating ?? null,
      );
      if (!res.ok) return error.set({ code: res.code, error: res.error });
      createdId = res.problemId || pid;
    } else {
      const res = preview.createProblem(pid, title().trim() || pid);
      if (!res.ok) return error.set(errorOf(res));
    }
    if (props.addTo) {
      const c = info().contests.find((x) => x.id === props.addTo);
      if (c && !c.items.some((i) => i.id === createdId)) {
        const cRes = preview.composeContest(c.id, [...c.items.map((i) => i.id), createdId]);
        if (!cRes.ok) {
          refresh();
          return error.set(errorOf(cRes));
        }
      }
    }
    refresh();
    dialog.set(undefined);
    openStudio({ target: "problem", id: createdId });
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
      <View style={{ gap: 6 }}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
          <Text style={{ fontSize: 12.5, fontWeight: 500 }}>{t("dialogs.newProblem.urlLabel")}</Text>
          <DelayedSpinner active={resolvingUrl()} size={12} />
          <If when={!resolvingUrl() && !!urlError()}>
            <Text style={{ flexShrink: 1, numberOfLines: 1, textOverflow: "ellipsis", fontSize: 12 }} class="text-muted-foreground">
              {urlError() ?? ""}
            </Text>
          </If>
        </View>
        <Input
          class={`${INPUT_BASE} border-border`}
          style={{ fontFamily: MONO }}
          value={url()}
          placeholder={t("dialogs.newProblem.urlPlaceholder")}
          autoCapitalize="none"
          autoCorrect={false}
          submitBehavior="submit"
          onChange={(e) => {
            url.set(e.value);
            triggerResolveUrl(e.value);
          }}
          onSubmit={() => triggerResolveUrl(url())}
        />
        <If when={resolved()?.ok}>
          <View style={{ gap: 6 }}>
            <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 6 }}>
              <Chip label={`${resolved()!.platform} ${resolved()!.problemKey}`} />
              <If when={resolved()!.rating != null}>
                <Chip label={`★ ${resolved()!.rating}`} />
              </If>
              <Each of={(resolved()!.tags ?? []).slice(0, 4)} key={(tg) => tg}>
                {(tg) => <Chip label={tg()} />}
              </Each>
            </View>
            <If when={!!resolved()!.existingProblemId}>
              <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
                <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                  {t("dialogs.newProblem.urlExistsHint", { id: resolved()!.existingProblemId! })}
                </Text>
                <Btn
                  label={t("dialogs.newProblem.openExisting")}
                  size="sm"
                  variant="ghost"
                  onPress={() => {
                    const existingId = resolved()!.existingProblemId!;
                    dialog.set(undefined);
                    openStudio({ target: "problem", id: existingId });
                  }}
                />
              </View>
            </If>
          </View>
        </If>
      </View>

      <Field
        label={t("dialogs.newProblem.titleLabel")}
        value={title()}
        placeholder={t("dialogs.newProblem.titlePlaceholder")}
        autoFocus
        onChange={(v) =>
          batch(() => {
            titleTouched.set(true);
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
  const addedId = signal<string | undefined>(undefined);
  let closeTimer: ReturnType<typeof setTimeout> | undefined;
  let rootRef: ViewRef | undefined;
  const raf = requestAnimationFrame(() => rootRef?.focus());
  cleanup(() => {
    cancelAnimationFrame(raf);
    if (closeTimer !== undefined) clearTimeout(closeTimer);
  });

  const add = (cid: string) => {
    const c = contests().find((x) => x.id === cid);
    if (!c) return;
    const res = preview.composeContest(cid, [...c.items.map((i) => i.id), props.problemId]);
    if (!res.ok) return error.set(errorOf(res));
    refresh();
    addedId.set(cid);
    if (closeTimer !== undefined) clearTimeout(closeTimer);
    closeTimer = setTimeout(() => {
      closeTimer = undefined;
      dialog.set(undefined);
    }, 750);
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
                    <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", flex: 1, fontSize: 13 }} class={off() && addedId() !== c().id ? "text-muted-foreground" : "text-foreground"}>
                      {c().name}
                    </Text>
                    {c().frozen && <Icon name="lock" size={13} tone="muted-foreground" />}
                    <If
                      when={addedId() === c().id}
                      else={
                        <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                          {has()
                            ? t("dialogs.addToContest.alreadyIn")
                            : c().frozen
                              ? t("contests.finalized")
                              : full()
                                ? t("dialogs.addToContest.full")
                                : t("contests.problemCount", { count: c().items.length })}
                        </Text>
                      }
                    >
                      <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
                        <Icon name="check" size={12.5} tone="primary" />
                        <Text style={{ fontSize: 12, fontWeight: 500 }} class="text-primary">
                          {t("dialogs.addToContest.added")}
                        </Text>
                      </View>
                    </If>
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

// ── Group CRUD Dialogs (§5.4) ────────────────────────────────────────────────

function NewGroup(props: { open: boolean }) {
  const name = signal("");
  const error = signal<UiError | undefined>(undefined);

  effect(() => {
    if (!props.open) return;
    batch(() => {
      name.set("");
      error.set(undefined);
    });
  });

  const submit = () => {
    const trimmed = name().trim();
    if (!trimmed) return error.set({ code: "invalid_group_name" });
    const res = preview.createGroup(trimmed);
    if (!res.ok) return error.set(errorOf(res));
    refresh();
    dialog.set(undefined);
  };

  return (
    <Form
      head={<DialogHeader icon="folderPlus" title={t("dialogs.newGroup.title")} description={t("dialogs.newGroup.desc")} />}
      submit={t("common.create")}
      onSubmit={submit}
    >
      <Field
        label={t("dialogs.newGroup.nameLabel")}
        hint={t("dialogs.newGroup.nameHint")}
        error={message(error())}
        value={name()}
        placeholder={t("dialogs.newGroup.namePlaceholder")}
        autoFocus
        onChange={(v) =>
          batch(() => {
            name.set(v);
            error.set(undefined);
          })
        }
        onSubmit={submit}
      />
    </Form>
  );
}

function RenameGroup(props: { open: boolean; oldName: string }) {
  const name = signal("");
  const error = signal<UiError | undefined>(undefined);

  effect(() => {
    if (!props.open) return;
    batch(() => {
      name.set(props.oldName);
      error.set(undefined);
    });
  });

  const submit = () => {
    const trimmed = name().trim();
    if (!trimmed) return error.set({ code: "invalid_group_name" });
    if (trimmed === props.oldName) {
      dialog.set(undefined);
      return;
    }
    const res = preview.renameGroup(props.oldName, trimmed);
    if (!res.ok) return error.set(errorOf(res));
    refresh();
    dialog.set(undefined);
  };

  return (
    <Form
      head={<DialogHeader icon="pencil" title={t("dialogs.renameGroup.title")} description={t("dialogs.renameGroup.desc")} />}
      submit={t("dialogs.renameGroup.submit")}
      onSubmit={submit}
    >
      <Field
        label={t("dialogs.newGroup.nameLabel")}
        hint={t("dialogs.newGroup.nameHint")}
        error={message(error())}
        value={name()}
        placeholder={t("dialogs.newGroup.namePlaceholder")}
        autoFocus
        onChange={(v) =>
          batch(() => {
            name.set(v);
            error.set(undefined);
          })
        }
        onSubmit={submit}
      />
    </Form>
  );
}

function DeleteGroup(props: { name: string }) {
  const error = signal<UiError | undefined>(undefined);
  const remove = () => {
    const res = preview.deleteGroup(props.name);
    if (!res.ok) return error.set(errorOf(res));
    refresh();
    dialog.set(undefined);
  };

  return (
    <View style={{ padding: 20, gap: 16 }}>
      <DialogHeader
        icon="trash"
        tone="destructive"
        title={t("dialogs.deleteGroup.title", { name: props.name })}
        description={t("dialogs.deleteGroup.desc")}
      />
      <If when={error()}>
        <ErrorDisplay error={error()} onDismiss={() => error.set(undefined)} />
      </If>
      <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
        <Btn label={t("common.cancel")} variant="ghost" onPress={() => dialog.set(undefined)} />
        <Btn label={t("common.delete")} variant="danger" onPress={remove} testID="confirm-delete-group" />
      </View>
    </View>
  );
}

// ── Batch Operation Dialogs (§5.4) ───────────────────────────────────────────

type TagMode = "keep" | "remove" | "add";

function BatchTags(props: { open: boolean; problemIds: readonly string[] }) {
  const modes = signal<Record<string, TagMode>>({});
  const addInput = signal("");
  const error = signal<UiError | undefined>(undefined);

  const existingTags = computed(() => {
    if (!props.open || props.problemIds.length === 0) return [];
    const idSet = new Set(props.problemIds);
    const all = preview.searchProblems({});
    const counts = new Map<string, number>();
    for (const p of all) {
      if (!idSet.has(p.id)) continue;
      for (const tag of p.tags) {
        counts.set(tag, (counts.get(tag) ?? 0) + 1);
      }
    }
    return [...counts.entries()]
      .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
      .map(([tag, count]) => ({ tag, count }));
  });

  effect(() => {
    if (!props.open) return;
    batch(() => {
      modes.set({});
      addInput.set("");
      error.set(undefined);
    });
  });

  const cycleTag = (tag: string) => {
    const cur = modes()[tag] ?? "keep";
    const next: TagMode = cur === "keep" ? "remove" : cur === "remove" ? "add" : "keep";
    modes.set({ ...modes(), [tag]: next });
  };

  const submit = () => {
    const m = modes();
    const toAdd: string[] = [];
    const toRemove: string[] = [];
    for (const [tag, mode] of Object.entries(m)) {
      if (mode === "add") toAdd.push(tag);
      else if (mode === "remove") toRemove.push(tag);
    }
    const extra = addInput()
      .split(/[,，\s]+/)
      .map((s) => s.trim())
      .filter(Boolean);
    for (const tg of extra) {
      if (!toAdd.includes(tg)) toAdd.push(tg);
    }
    if (toAdd.length === 0 && toRemove.length === 0) {
      dialog.set(undefined);
      return;
    }
    const res = preview.batchEditTags([...props.problemIds], toAdd, toRemove);
    if (!res.ok && res.succeeded === 0 && res.error) {
      return error.set({ code: res.code, error: res.error });
    }
    refresh();
    dialog.set(undefined);
  };

  return (
    <Form
      head={
        <DialogHeader
          icon="pencil"
          title={t("dialogs.batchTags.title")}
          description={t("dialogs.batchTags.desc", { count: props.problemIds.length })}
        />
      }
      submit={t("dialogs.batchTags.submit")}
      onSubmit={submit}
    >
      <View style={{ gap: 6 }}>
        <Text style={{ fontSize: 12 }} class="text-muted-foreground">
          {existingTags().length > 0 ? t("dialogs.batchTags.existing") : t("dialogs.batchTags.noExisting")}
        </Text>
        <If when={existingTags().length > 0}>
          <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
            <Each of={existingTags()} key={(item) => item.tag}>
              {(item) => {
                const mode = () => modes()[item().tag] ?? "keep";
                const cls = () =>
                  mode() === "remove"
                    ? "bg-destructive/15 border border-destructive/40"
                    : mode() === "add"
                      ? "bg-primary/15 border border-primary/50"
                      : "bg-muted border border-border hover:bg-accent";
                const textCls = () =>
                  mode() === "remove"
                    ? "text-destructive"
                    : mode() === "add"
                      ? "text-primary"
                      : "text-foreground";
                const badge = () =>
                  mode() === "remove"
                    ? `− ${t("dialogs.batchTags.stateRemove")}`
                    : mode() === "add"
                      ? `+ ${t("dialogs.batchTags.stateAdd")}`
                      : `${item().count}/${props.problemIds.length}`;
                return (
                  <View
                    onPress={() => cycleTag(item().tag)}
                    class={cls()}
                    style={{
                      height: 24,
                      paddingLeft: 8,
                      paddingRight: 8,
                      borderRadius: 6,
                      flexDirection: "row",
                      alignItems: "center",
                      gap: 6,
                      transitionProperty: "colors",
                      transitionDuration: ms(120),
                      transitionTimingFunction: EASE_STANDARD,
                    }}
                  >
                    <Text style={{ fontSize: 12 }} class={textCls()}>
                      {item().tag}
                    </Text>
                    <Text style={{ fontSize: 11, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                      {badge()}
                    </Text>
                  </View>
                );
              }}
            </Each>
          </View>
        </If>
      </View>
      <Field
        label={t("dialogs.batchTags.addLabel")}
        value={addInput()}
        placeholder={t("dialogs.batchTags.addPlaceholder")}
        error={message(error())}
        autoFocus
        onChange={(v) =>
          batch(() => {
            addInput.set(v);
            error.set(undefined);
          })
        }
        onSubmit={submit}
      />
    </Form>
  );
}

const RATING_PRESETS = [800, 1200, 1400, 1600, 1900, 2100, 2400];

function BatchRating(props: { open: boolean; problemIds: readonly string[] }) {
  const raw = signal("");
  const errText = signal<string | undefined>(undefined);

  effect(() => {
    if (!props.open) return;
    batch(() => {
      raw.set("");
      errText.set(undefined);
    });
  });

  const applyRating = (val: number | null) => {
    const res = preview.batchSetRating([...props.problemIds], val);
    if (!res.ok && res.succeeded === 0 && res.error) {
      errText.set(errorCopy(res.code, res.error).message);
      return;
    }
    refresh();
    dialog.set(undefined);
  };

  const submit = () => {
    const s = raw().trim();
    if (!s) {
      errText.set(t("dialogs.batchRating.invalid"));
      return;
    }
    const n = Number(s);
    if (!Number.isInteger(n) || n < 0 || n > 4000) {
      errText.set(t("dialogs.batchRating.invalid"));
      return;
    }
    applyRating(n);
  };

  return (
    <View style={{ padding: 20, gap: 16 }}>
      <DialogHeader
        icon="scale"
        title={t("dialogs.batchRating.title")}
        description={t("dialogs.batchRating.desc", { count: props.problemIds.length })}
      />
      <View style={{ gap: 10 }}>
        <Field
          label={t("dialogs.batchRating.label")}
          hint={t("dialogs.batchRating.hint")}
          error={errText()}
          value={raw()}
          placeholder={t("dialogs.batchRating.placeholder")}
          mono
          autoFocus
          onChange={(v) =>
            batch(() => {
              raw.set(v);
              errText.set(undefined);
            })
          }
          onSubmit={submit}
        />
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
          <Each of={RATING_PRESETS} key={(n) => String(n)}>
            {(n) => (
              <Btn
                variant="outline"
                label={String(n())}
                onPress={() => {
                  raw.set(String(n()));
                  errText.set(undefined);
                }}
                textStyle={{ fontVariant: ["tabular-nums"] }}
              />
            )}
          </Each>
        </View>
      </View>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 8, marginTop: 4 }}>
        <Btn label={t("dialogs.batchRating.clear")} variant="outline" onPress={() => applyRating(null)} />
        <View style={{ flex: 1 }} />
        <Btn label={t("common.cancel")} variant="ghost" onPress={() => dialog.set(undefined)} />
        <Btn variant="primary" label={t("dialogs.batchRating.submit")} onPress={submit} />
      </View>
    </View>
  );
}

function BatchDelete(props: { problemIds: readonly string[]; onJumpContest?: (contestId: string) => void }) {
  const error = signal<UiError | undefined>(undefined);
  const partition = computed(() => partitionBatchDelete(props.problemIds, info().contests));
  const titleById = computed(() => new Map(info().problems.map((p) => [p.id, p.title])));

  const remove = () => {
    const res = preview.batchDeleteProblems([...props.problemIds]);
    if (!res.ok && res.deleted === 0 && res.error) {
      return error.set({ code: res.code, error: res.error });
    }
    dialog.set(undefined);
    refresh();
  };

  const jumpContest = (cid: string) => {
    dialog.set(undefined);
    if (props.onJumpContest) props.onJumpContest(cid);
    else jumpToContest(cid);
  };

  return (
    <View style={{ padding: 20, gap: 16 }}>
      <If
        when={partition().deletable.length > 0}
        else={
          <View style={{ gap: 12 }}>
            <DialogHeader
              icon="triangleAlert"
              tone="secondary-foreground"
              title={t("dialogs.batchDelete.allBlockedTitle")}
              description={t("dialogs.batchDelete.allBlockedDesc", { count: partition().blocked.length })}
            />
            <Scroll style={{ maxHeight: 200 }}>
              <View style={{ gap: 6, paddingLeft: 44 }}>
                <Each of={partition().blocked} key={(b) => b.id}>
                  {(b) => (
                    <View style={{ gap: 3 }}>
                      <Text style={{ fontSize: 12.5, fontWeight: 500 }} class="text-foreground">
                        {titleById().get(b().id) ?? b().id}
                      </Text>
                      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
                        <Each of={b().contests} key={(c) => c.id}>
                          {(c) => (
                            <View
                              onPress={() => jumpContest(c().id)}
                              class="border border-border bg-muted/60 hover:bg-muted"
                              style={{
                                height: 22,
                                paddingLeft: 6,
                                paddingRight: 6,
                                borderRadius: 4,
                                flexDirection: "row",
                                alignItems: "center",
                                gap: 4,
                              }}
                            >
                              <Icon name="trophy" size={11} tone="muted-foreground" />
                              <Text style={{ fontSize: 11.5 }} class="text-secondary-foreground">
                                {c().name}
                              </Text>
                            </View>
                          )}
                        </Each>
                      </View>
                    </View>
                  )}
                </Each>
              </View>
            </Scroll>
          </View>
        }
      >
        <View style={{ gap: 12 }}>
          <DialogHeader
            icon="trash"
            tone="destructive"
            title={
              partition().blocked.length > 0
                ? t("dialogs.batchDelete.partialTitle", {
                    deletable: partition().deletable.length,
                    blocked: partition().blocked.length,
                  })
                : t("dialogs.batchDelete.confirmTitle", { count: partition().deletable.length })
            }
            description={
              partition().blocked.length > 0
                ? t("dialogs.batchDelete.partialDesc", {
                    deletable: partition().deletable.length,
                    blocked: partition().blocked.length,
                  })
                : t("dialogs.batchDelete.confirmDesc")
            }
          />
          <If when={partition().blocked.length > 0}>
            <Scroll style={{ maxHeight: 160 }}>
              <View style={{ gap: 8, paddingLeft: 44 }}>
                <Each of={partition().blocked} key={(b) => b.id}>
                  {(b) => (
                    <View style={{ gap: 2, minWidth: 0 }}>
                      <Text
                        style={{ fontSize: 12.5, fontWeight: 500, numberOfLines: 1, textOverflow: "ellipsis" }}
                        class="text-foreground"
                      >
                        {titleById().get(b().id) ?? b().id}
                      </Text>
                      <Text
                        style={{ fontSize: 11.5, numberOfLines: 1, textOverflow: "ellipsis" }}
                        class="text-muted-foreground"
                      >
                        {b().contests.map((c) => c.name).join(" / ")}
                      </Text>
                    </View>
                  )}
                </Each>
              </View>
            </Scroll>
          </If>
        </View>
      </If>

      <If when={error()}>
        <ErrorDisplay error={error()} onDismiss={() => error.set(undefined)} />
      </If>

      <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
        <Btn
          label={partition().deletable.length > 0 ? t("common.cancel") : t("common.gotIt")}
          variant="ghost"
          onPress={() => dialog.set(undefined)}
        />
        <If when={partition().deletable.length > 0}>
          <Btn
            label={t("dialogs.batchDelete.submitCount", { count: partition().deletable.length })}
            variant="danger"
            onPress={remove}
            testID="confirm-batch-delete"
          />
        </If>
      </View>
    </View>
  );
}

function BatchAddToContest(props: { problemIds: readonly string[] }) {
  const error = signal<UiError | undefined>(undefined);
  const addedContestId = signal<string | undefined>(undefined);
  const addedCount = signal(0);
  let closeTimer: ReturnType<typeof setTimeout> | undefined;
  cleanup(() => {
    if (closeTimer !== undefined) clearTimeout(closeTimer);
  });

  const contests = () => filterAddableContests(info().contests);

  const netAddFor = (c: { items: readonly { id: string }[] }) => {
    const existing = new Set(c.items.map((x) => x.id));
    let n = 0;
    for (const id of props.problemIds) {
      if (!existing.has(id)) n += 1;
    }
    return n;
  };

  const add = (cid: string, count: number) => {
    const res = preview.batchAddToContest(cid, [...props.problemIds]);
    if (!res.ok && res.succeeded === 0 && res.error) {
      return error.set({ code: res.code, error: res.error });
    }
    refresh();
    addedContestId.set(cid);
    addedCount.set(res.succeeded || count);
    if (closeTimer !== undefined) clearTimeout(closeTimer);
    closeTimer = setTimeout(() => {
      closeTimer = undefined;
      dialog.set(undefined);
    }, 750);
  };

  return (
    <View style={{ padding: 6, gap: 2 }}>
      <View style={{ padding: 14, paddingBottom: 10 }}>
        <DialogHeader
          icon="listPlus"
          title={t("dialogs.batchAddToContest.title")}
          description={t("dialogs.batchAddToContest.desc", { count: props.problemIds.length })}
        />
      </View>
      <If when={contests().length > 0}>
        <Scroll style={{ maxHeight: 280 }}>
          <View style={{ gap: 2 }}>
            <Each of={contests()} key={(c) => c.id}>
              {(c) => {
                const net = () => netAddFor(c());
                const exceeds = () => c().items.length + net() > 26;
                const off = () => c().frozen || net() === 0 || exceeds();
                return (
                  <View
                    disabled={off()}
                    onPress={() => {
                      if (!off()) add(c().id, net());
                    }}
                    class={off() ? "" : "hover:bg-muted active:bg-accent"}
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
                    <Text
                      style={{ numberOfLines: 1, textOverflow: "ellipsis", flex: 1, fontSize: 13 }}
                      class={off() && addedContestId() !== c().id ? "text-muted-foreground" : "text-foreground"}
                    >
                      {c().name}
                    </Text>
                    {c().frozen && <Icon name="lock" size={13} tone="muted-foreground" />}
                    <If
                      when={addedContestId() === c().id}
                      else={
                        <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                          {c().frozen
                            ? t("contests.finalized")
                            : net() === 0
                              ? t("dialogs.batchAddToContest.allAlreadyIn")
                              : exceeds()
                                ? t("dialogs.batchAddToContest.exceedsCap")
                                : t("dialogs.batchAddToContest.netAdd", { count: net() })}
                        </Text>
                      }
                    >
                      <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
                        <Icon name="check" size={12.5} tone="primary" />
                        <Text style={{ fontSize: 12, fontWeight: 500, fontVariant: ["tabular-nums"] }} class="text-primary">
                          {t("dialogs.batchAddToContest.addedCount", { count: addedCount() })}
                        </Text>
                      </View>
                    </If>
                  </View>
                );
              }}
            </Each>
          </View>
        </Scroll>
      </If>
      <If when={error()}>
        <View style={{ paddingLeft: 10, paddingRight: 10, paddingTop: 4, paddingBottom: 4 }}>
          <ErrorDisplay error={error()} onDismiss={() => error.set(undefined)} />
        </View>
      </If>
    </View>
  );
}

type ExportFormat = "domjudge" | "hydro" | "merged_pdf";
type PdfLang = "chinese" | "english";
type PdfBook = "statements" | "editorial";

function BatchExport(props: { open: boolean; problemIds: readonly string[] }) {
  const format = signal<ExportFormat>("domjudge");
  const pdfTitle = signal("");
  const pdfLang = signal<PdfLang>("chinese");
  const pdfBook = signal<PdfBook>("statements");
  const error = signal<UiError | undefined>(undefined);

  effect(() => {
    if (!props.open) return;
    batch(() => {
      format.set("domjudge");
      pdfTitle.set("");
      pdfLang.set("chinese");
      pdfBook.set("statements");
      error.set(undefined);
    });
  });

  const ojGate = computed(() => {
    if (!props.open || props.problemIds.length === 0) return { ready: 0, skipped: 0 };
    const idSet = new Set(props.problemIds);
    const all = preview.searchProblems({});
    let ready = 0;
    for (const p of all) {
      if (idSet.has(p.id) && p.packageStatus === "verified_fresh") ready += 1;
    }
    return { ready, skipped: Math.max(0, props.problemIds.length - ready) };
  });

  const isOj = () => format() === "domjudge" || format() === "hydro";
  const submitDisabled = () => isOj() && ojGate().ready === 0;

  const submit = () => {
    if (submitDisabled()) return;
    const err = startBatchExportTask({
      format: format(),
      problemIds: [...props.problemIds],
      title: format() === "merged_pdf" ? pdfTitle().trim() || null : null,
      language: format() === "merged_pdf" ? pdfLang() : null,
      book: format() === "merged_pdf" ? pdfBook() : null,
    });
    if (err) {
      error.set(err);
      return;
    }
    dialog.set(undefined);
  };

  return (
    <View style={{ padding: 20, gap: 16 }}>
      <DialogHeader
        icon="fileDown"
        title={t("dialogs.batchExport.title")}
        description={t("dialogs.batchExport.desc", { count: props.problemIds.length })}
      />

      <View style={{ gap: 12 }}>
        <View style={{ gap: 6 }}>
          <Text style={{ fontSize: 12.5, fontWeight: 500 }}>{t("dialogs.batchExport.formatLabel")}</Text>
          <Segmented<ExportFormat>
            items={[
              { value: "domjudge", label: t("dialogs.batchExport.formatDomjudge") },
              { value: "hydro", label: t("dialogs.batchExport.formatHydro") },
              { value: "merged_pdf", label: t("dialogs.batchExport.formatMergedPdf") },
            ]}
            value={format()}
            onChange={(v) => format.set(v)}
            itemWidth={120}
          />
        </View>

        <If
          when={isOj()}
          else={
            <View style={{ gap: 10 }}>
              <Field
                label={t("dialogs.batchExport.pdfTitleLabel")}
                value={pdfTitle()}
                placeholder={t("dialogs.batchExport.pdfTitlePlaceholder", { count: props.problemIds.length })}
                onChange={(v) => pdfTitle.set(v)}
                onSubmit={submit}
              />
              <View style={{ flexDirection: "row", gap: 16 }}>
                <View style={{ gap: 6 }}>
                  <Text style={{ fontSize: 12.5, fontWeight: 500 }}>{t("dialogs.batchExport.pdfLangLabel")}</Text>
                  <Segmented<PdfLang>
                    items={[
                      { value: "chinese", label: t("dialogs.batchExport.pdfLangZh") },
                      { value: "english", label: t("dialogs.batchExport.pdfLangEn") },
                    ]}
                    value={pdfLang()}
                    onChange={(v) => pdfLang.set(v)}
                    itemWidth={80}
                  />
                </View>
                <View style={{ gap: 6 }}>
                  <Text style={{ fontSize: 12.5, fontWeight: 500 }}>{t("dialogs.batchExport.pdfBookLabel")}</Text>
                  <Segmented<PdfBook>
                    items={[
                      { value: "statements", label: t("dialogs.batchExport.pdfBookStatements") },
                      { value: "editorial", label: t("dialogs.batchExport.pdfBookEditorial") },
                    ]}
                    value={pdfBook()}
                    onChange={(v) => pdfBook.set(v)}
                    itemWidth={80}
                  />
                </View>
              </View>
            </View>
          }
        >
          <View
            style={{
              padding: 10,
              borderRadius: 8,
              borderWidth: 1,
              flexDirection: "row",
              alignItems: "center",
              gap: 8,
            }}
            class="border-border bg-muted/40"
          >
            <Icon
              name={ojGate().ready > 0 ? "shieldCheck" : "circleAlert"}
              size={15}
              tone={ojGate().ready > 0 ? "primary" : "muted-foreground"}
            />
            <Text style={{ fontSize: 12.5, fontVariant: ["tabular-nums"], flex: 1 }} class="text-secondary-foreground">
              {ojGate().ready > 0
                ? t("dialogs.batchExport.ojGateSummary", { ready: ojGate().ready, skipped: ojGate().skipped })
                : t("dialogs.batchExport.ojGateNone")}
            </Text>
          </View>
        </If>
      </View>

      <If when={error()}>
        <ErrorDisplay error={error()} onDismiss={() => error.set(undefined)} />
      </If>

      <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
        <Btn label={t("common.cancel")} variant="ghost" onPress={() => dialog.set(undefined)} />
        <Btn
          variant="primary"
          label={t("dialogs.batchExport.submit")}
          disabled={submitDisabled()}
          onPress={submit}
          testID="confirm-batch-export"
        />
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
  const lastGroupName = signal("");
  const lastBatchIds = signal<string[]>([]);

  effect(() => {
    const cur = d();
    if (!cur) return;
    if (cur.kind === "newProblem") lastAddTo.set(cur.addTo);
    else if (cur.kind === "duplicateProblem") lastDuplicateProblemId.set(cur.problemId);
    else if (cur.kind === "newContest") lastWithProblem.set(cur.withProblem);
    else if (cur.kind === "addToContest") lastAddProblemId.set(cur.problemId);
    else if (cur.kind === "pickProblems") lastPickContestId.set(cur.contestId);
    else if (cur.kind === "deleteProblem") lastDeleteProblemId.set(cur.problemId);
    else if (cur.kind === "renameGroup" || cur.kind === "deleteGroup") lastGroupName.set(cur.name);
    else if (
      cur.kind === "batchTags" ||
      cur.kind === "batchRating" ||
      cur.kind === "batchDelete" ||
      cur.kind === "batchAddToContest" ||
      cur.kind === "batchExport"
    ) {
      lastBatchIds.set(cur.problemIds);
    }
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
      <Modal open={d()?.kind === "newGroup"} onClose={close} width={400} testID="dialog-new-group">
        <NewGroup open={d()?.kind === "newGroup"} />
      </Modal>
      <Modal open={d()?.kind === "renameGroup"} onClose={close} width={400} testID="dialog-rename-group">
        <RenameGroup open={d()?.kind === "renameGroup"} oldName={lastGroupName()} />
      </Modal>
      <Modal open={d()?.kind === "deleteGroup"} onClose={close} width={420} testID="dialog-delete-group">
        <DeleteGroup name={lastGroupName()} />
      </Modal>
      <Modal open={d()?.kind === "batchTags"} onClose={close} width={440} testID="dialog-batch-tags">
        <BatchTags open={d()?.kind === "batchTags"} problemIds={lastBatchIds()} />
      </Modal>
      <Modal open={d()?.kind === "batchRating"} onClose={close} width={440} testID="dialog-batch-rating">
        <BatchRating open={d()?.kind === "batchRating"} problemIds={lastBatchIds()} />
      </Modal>
      <Modal open={d()?.kind === "batchDelete"} onClose={close} width={440} testID="dialog-batch-delete">
        <BatchDelete problemIds={lastBatchIds()} onJumpContest={props.onJumpContest} />
      </Modal>
      <Modal open={d()?.kind === "batchAddToContest"} onClose={close} width={380} testID="dialog-batch-add-to-contest">
        <BatchAddToContest problemIds={lastBatchIds()} />
      </Modal>
      <Modal open={d()?.kind === "batchExport"} onClose={close} width={440} testID="dialog-batch-export">
        <BatchExport open={d()?.kind === "batchExport"} problemIds={lastBatchIds()} />
      </Modal>
    </>
  );
}
