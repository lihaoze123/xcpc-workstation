// Full-screen Living TRD Studio: left chapter outline + preflight missing-dep auto-fill,
// center live Typst A4 preview (`PreviewCanvas` with `target: "trd"`), right collapsible
// Properties drawer (`信息 | 版式` with `TemplateOptionsEditor`), Export PDF split button,
// and Finalize / Reopen flow.

import { cleanup, computed, effect, keymap, Scroll, signal, Text, View } from "nsh";
import { Input } from "nsh/base";
import type { PreviewRequest } from "xcpc-workstation/native";
import { Icon } from "../Icon";
import { t } from "../i18n";
import { Btn, createTransientFlag, DelayedSpinner, IconBtn, Lock, Segmented, tip } from "../kit";
import { AppMenu, type MenuEntry } from "../Menu";
import { Modal } from "../Modal";
import { PreviewCanvas, type CanvasController } from "../studio/PreviewCanvas";
import { TemplateOptionsEditor, TemplatePicker } from "../TemplateOptionsEditor";
import { TitleBar } from "../TitleBar";
import { showToast } from "../Toast";
import {
  activeTrdStudio,
  bumpSnippetsRevision,
  closeTrdStudio,
  info,
  preview,
  revision,
  snippetsRevision,
} from "../store";
import type { TemplateOptionMap } from "../templates";
import { MONO } from "../theme";
import {
  collectMissingDeps,
  encodeStringPatch,
  encodeTrdOptionPatch,
  fillMissingDepsInChapters,
  parseJsonPayload,
} from "./snippetLogic";
import type {
  SnippetListPayload,
  TrdChapter,
  TrdDetailPayload,
  TrdTemplatesCatalog,
  TrdUnfreezeDiff,
} from "./types";

export function TrdStudio(props: { active: boolean }) {
  const trdId = () => activeTrdStudio() ?? "";
  const payload = signal<TrdDetailPayload | undefined>(undefined);
  const allSnippets = signal<SnippetListPayload["snippets"]>([]);
  const catalog = signal<TrdTemplatesCatalog>({ default: "trd-compact", templates: [], presets: [], fonts: [] });
  const propsOpen = signal(true);
  const propTab = signal<"info" | "layout">("info");
  const newChapterTitle = signal("");
  const exporting = signal(false);
  const showExportSpinner = signal(false);
  let exportSpinTimer: ReturnType<typeof setTimeout> | undefined;
  let canvasCtrl: CanvasController | undefined;
  let fittedForId = "";

  effect(() => {
    const busy = exporting();
    clearTimeout(exportSpinTimer);
    if (!busy) showExportSpinner.set(false);
    else exportSpinTimer = setTimeout(() => showExportSpinner.set(true), 150);
  });
  cleanup(() => clearTimeout(exportSpinTimer));

  const exportedFlag = createTransientFlag(1800);
  const finalizeOpen = signal(false);
  const finalizing = signal(false);
  const reopenDiff = signal<TrdUnfreezeDiff | undefined>(undefined);
  const reopenOpen = signal(false);
  const deleteChapterTarget = signal<{ idx: number; title: string } | undefined>(undefined);

  // Editable Info fields
  const metaName = signal("");
  const metaAuthor = signal("");
  const metaDate = signal("");

  function loadAll(id: string) {
    if (!id) return;
    const trdRes = parseJsonPayload<TrdDetailPayload>(preview.loadTrdJson(id));
    if (trdRes) {
      payload.set(trdRes);
      metaName.set(trdRes.detail.meta.name);
      metaAuthor.set(trdRes.detail.meta.author ?? "");
      metaDate.set(trdRes.detail.meta.date ?? "");
    }
    const snipRes = parseJsonPayload<SnippetListPayload>(preview.listSnippetsJson());
    if (snipRes) allSnippets.set(snipRes.snippets ?? []);
    try {
      const cat = JSON.parse(preview.listTrdTemplatesJson()) as TrdTemplatesCatalog;
      catalog.set(cat);
    } catch {
      // Keep default empty catalog.
    }
  }

  let lastTrdId = "";
  effect(() => {
    const id = trdId();
    void revision();
    void snippetsRevision();
    if (!id) return;
    const changed = id !== lastTrdId;
    lastTrdId = id;
    loadAll(id);
    if (!changed) {
      canvasCtrl?.compile();
    }
  });

  const detail = () => payload()?.detail;
  const frozen = () => Boolean(detail()?.meta.frozen);
  const archived = () => Boolean(detail()?.meta.archived);
  const locked = () => info().readOnly || frozen() || archived();
  const chapters = () => detail()?.meta.chapter ?? [];
  const preflight = () => payload()?.preflight ?? [];
  const missingDeps = computed(() => collectMissingDeps(preflight()));
  const blockingErrors = computed(() => preflight().filter((i) => i.severity === "error"));
  const verifyWarnings = computed(() =>
    preflight().filter((i) => i.kind === "stale_snippet" || i.kind === "failed_snippet"),
  );
  const allIncludedSlugs = computed(() => {
    const set = new Set<string>();
    for (const ch of chapters()) {
      for (const s of ch.items ?? []) set.add(s);
    }
    return set;
  });

  function saveChapters(next: TrdChapter[]) {
    const id = trdId();
    if (!id || locked()) return;
    const res = preview.composeTrd(id, JSON.stringify(next), null);
    if (!res.ok) {
      showToast({ tone: "issues", text: res.error ?? t("snippets.error.updateFailed") });
      return;
    }
    bumpSnippetsRevision();
    loadAll(id);
    canvasCtrl?.compile();
  }

  function addChapter() {
    const title = newChapterTitle().trim();
    if (!title || locked()) return;
    if (chapters().some((c) => c.title.trim() === title)) {
      showToast({ tone: "issues", text: t("snippets.trd.duplicateChapter", { title }) });
      return;
    }
    newChapterTitle.set("");
    saveChapters([...chapters(), { title, items: [] }]);
  }

  function confirmRemoveChapter() {
    const target = deleteChapterTarget();
    if (!target) return;
    deleteChapterTarget.set(undefined);
    const next = chapters().filter((_, i) => i !== target.idx);
    saveChapters(next);
  }

  function moveChapter(idx: number, delta: number) {
    const list = chapters().slice();
    const target = idx + delta;
    if (target < 0 || target >= list.length) return;
    const [ch] = list.splice(idx, 1);
    list.splice(target, 0, ch);
    saveChapters(list);
  }

  function addSnippetToChapter(chIdx: number, slug: string) {
    const next = chapters().map((c, i) =>
      i === chIdx && !c.items.includes(slug) ? { ...c, items: [...c.items, slug] } : c,
    );
    saveChapters(next);
  }

  function removeSnippetFromChapter(chIdx: number, slug: string) {
    const next = chapters().map((c, i) =>
      i === chIdx ? { ...c, items: c.items.filter((s) => s !== slug) } : c,
    );
    saveChapters(next);
  }

  function moveSnippetInChapter(chIdx: number, itemIdx: number, delta: number) {
    const next = chapters().map((c, i) => {
      if (i !== chIdx) return c;
      const items = c.items.slice();
      const target = itemIdx + delta;
      if (target < 0 || target >= items.length) return c;
      const [s] = items.splice(itemIdx, 1);
      items.splice(target, 0, s);
      return { ...c, items };
    });
    saveChapters(next);
  }

  function fillMissingDeps() {
    const next = fillMissingDepsInChapters(chapters(), preflight());
    saveChapters(next);
    showToast({ tone: "success", text: t("snippets.trd.filledMissingDeps") });
  }

  function saveMetaInfo() {
    const id = trdId();
    if (!id || locked()) return;
    const patch = {
      name: metaName().trim() || detail()?.meta.name,
      author: encodeStringPatch(metaAuthor()),
      date: encodeStringPatch(metaDate()),
    };
    const res = preview.updateTrdMeta(id, JSON.stringify(patch));
    if (!res.ok) {
      showToast({ tone: "issues", text: res.error ?? t("snippets.error.updateFailed") });
      return;
    }
    bumpSnippetsRevision();
    loadAll(id);
    canvasCtrl?.compile();
  }

  function changeTemplate(tplId: string) {
    const id = trdId();
    if (!id || locked()) return;
    const res = preview.updateTrdMeta(id, JSON.stringify({ template: encodeStringPatch(tplId) }));
    if (!res.ok) {
      showToast({ tone: "issues", text: res.error ?? t("snippets.error.updateFailed") });
      return;
    }
    bumpSnippetsRevision();
    loadAll(id);
    canvasCtrl?.compile();
  }

  function updateTemplateOption(tplId: string, key: string, value: unknown) {
    const id = trdId();
    if (!id || locked()) return;
    const patchJson = JSON.stringify({
      template_options: encodeTrdOptionPatch(tplId, key, value),
    });
    const res = preview.updateTrdMeta(id, patchJson);
    if (!res.ok) {
      showToast({ tone: "issues", text: res.error ?? t("snippets.error.updateFailed") });
      return;
    }
    bumpSnippetsRevision();
    loadAll(id);
    canvasCtrl?.compile();
  }

  async function exportPdf() {
    const id = trdId();
    if (!id || exporting()) return;
    exporting.set(true);
    try {
      const out = await preview.exportTrdPdf(id);
      if (out.error) {
        showToast({ tone: "issues", text: out.error });
      } else {
        exportedFlag.trigger();
      }
    } finally {
      exporting.set(false);
    }
  }

  async function confirmFinalize() {
    const id = trdId();
    if (!id || finalizing()) return;
    finalizing.set(true);
    try {
      // Per review S-1: GUI always uses flexible finalize (strict gate remains CLI `--strict`).
      const res = await preview.freezeTrd(id, false);
      if (!res.ok) {
        showToast({ tone: "issues", text: res.error ?? t("snippets.error.finalizeFailed") });
        return;
      }
      finalizeOpen.set(false);
      bumpSnippetsRevision();
      loadAll(id);
      canvasCtrl?.compile();
    } finally {
      finalizing.set(false);
    }
  }

  function openReopenModal() {
    const id = trdId();
    if (!id) return;
    const diff = parseJsonPayload<TrdUnfreezeDiff>(preview.diffTrdForUnfreezeJson(id));
    reopenDiff.set(diff);
    reopenOpen.set(true);
  }

  function confirmReopen() {
    const id = trdId();
    if (!id) return;
    const res = preview.unfreezeTrd(id);
    if (!res.ok) {
      showToast({ tone: "issues", text: res.error ?? t("snippets.error.reopenFailed") });
      return;
    }
    reopenOpen.set(false);
    bumpSnippetsRevision();
    loadAll(id);
    canvasCtrl?.compile();
  }

  keymap.bind(
    {
      escape: () =>
        props.active &&
        !finalizeOpen() &&
        !reopenOpen() &&
        !deleteChapterTarget() &&
        closeTrdStudio(),
      "mod+i": () => props.active && propsOpen.set(!propsOpen()),
      "mod+e": () => props.active && void exportPdf(),
    },
    { when: "always" },
  );

  const previewRequest = computed<PreviewRequest>(() => ({
    target: "trd",
    id: trdId(),
    book: "statements",
    path: "",
    text: "",
  }));

  const effectiveTemplateId = () => detail()?.meta.template || catalog().default || "trd-compact";
  const activeTemplate = computed(() => {
    if (payload()?.localTemplate) return payload()!.localTemplate!;
    return catalog().templates.find((tpl) => tpl.id === effectiveTemplateId());
  });
  const overrideOptions = computed<TemplateOptionMap>(() => {
    const map = detail()?.meta["template-options"] ?? {};
    return map[effectiveTemplateId()] ?? {};
  });

  const exportMenuEntries = computed<MenuEntry[]>(() => {
    const st = trdId() ? preview.trdExportStatus(trdId()) : undefined;
    return [
      { id: "export", label: t("snippets.trd.exportPdf"), icon: "fileDown" },
      {
        id: "reveal",
        label: t("common.revealInFolder"),
        icon: "folderOpen",
        disabled: !st?.exists,
      },
    ];
  });

  return (
    <View style={{ flex: 1, flexDirection: "column" }} class="bg-background">
      <TitleBar
        leading={
          <View style={{ flexDirection: "row", alignItems: "center", gap: 8, minWidth: 0 }}>
            <Btn
              testID="trd-studio-back"
              icon="arrowLeft"
              variant="ghost"
              tooltip={tip(t("common.back"), "escape")}
              onPress={closeTrdStudio}
            />
            <Text
              style={{ fontSize: 14, fontWeight: 500, numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1 }}
              class="text-foreground"
            >
              {detail()?.meta.name ?? trdId()}
            </Text>
            <If when={frozen()}>
              <Lock closed size={13} />
            </If>
          </View>
        }
        trailing={
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <If when={!archived() && !info().readOnly}>
              <If
                when={!frozen()}
                else={
                  <Btn
                    variant="outline"
                    icon="lockOpen"
                    label={t("snippets.trd.reopen")}
                    onPress={openReopenModal}
                  />
                }
              >
                <Btn
                  variant="outline"
                  icon="stamp"
                  label={t("snippets.trd.finalize")}
                  tooltip={tip(t("snippets.trd.finalizeTooltip"))}
                  onPress={() => finalizeOpen.set(true)}
                />
              </If>
            </If>

            <View style={{ flexDirection: "row", alignItems: "center" }}>
              <Btn
                variant="primary"
                icon={exportedFlag.active() ? "check" : showExportSpinner() ? undefined : "fileDown"}
                label={exportedFlag.active() ? t("snippets.trd.exported") : t("snippets.trd.exportPdf")}
                disabled={exporting() || info().readOnly}
                onPress={() => void exportPdf()}
                style={{ borderTopRightRadius: 0, borderBottomRightRadius: 0, width: 108 }}
              >
                <DelayedSpinner active={exporting()} size={13} />
              </Btn>
              <If
                when={!exporting() && !info().readOnly}
                else={
                  <View
                    class="bg-primary"
                    style={{
                      width: 24,
                      height: 28,
                      borderTopRightRadius: 6,
                      borderBottomRightRadius: 6,
                      borderLeftWidth: 1,
                      borderLeftColor: "rgba(255,255,255,0.18)",
                      alignItems: "center",
                      justifyContent: "center",
                      opacity: 0.6,
                    }}
                  >
                    <Icon name="chevronDown" size={13} tone="primary-foreground" />
                  </View>
                }
              >
                <AppMenu
                  align="end"
                  entries={exportMenuEntries()}
                  onSelect={(id) => {
                    if (id === "export") void exportPdf();
                    else if (id === "reveal") {
                      const st = preview.trdExportStatus(trdId());
                      if (st.relPath) void preview.revealInFolder(st.relPath);
                    }
                  }}
                  trigger={
                    <View
                      class="bg-primary hover:bg-sidebar-primary focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
                      style={{
                        width: 24,
                        height: 28,
                        borderTopRightRadius: 6,
                        borderBottomRightRadius: 6,
                        borderLeftWidth: 1,
                        borderLeftColor: "rgba(255,255,255,0.18)",
                        alignItems: "center",
                        justifyContent: "center",
                      }}
                    >
                      <Icon name="chevronDown" size={13} tone="primary-foreground" />
                    </View>
                  }
                />
              </If>
            </View>

            <IconBtn
              icon="panelRight"
              tooltip={tip(t("palette.action.properties"), "mod+i")}
              tone={propsOpen() ? "foreground" : "secondary-foreground"}
              onPress={() => propsOpen.set(!propsOpen())}
            />
          </View>
        }
      />

      <View style={{ flex: 1, flexDirection: "row", minHeight: 0 }}>
        {/* Left chapter outline */}
        <View style={{ width: 272, borderRightWidth: 1, flexDirection: "column" }} class="border-border bg-background">
          <If when={missingDeps().length > 0 && !locked()}>
            <View
              style={{
                margin: 10,
                padding: 10,
                borderRadius: 8,
                borderWidth: 1,
                gap: 8,
              }}
              class="border-border bg-muted/60"
            >
              <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                <Icon name="triangleAlert" size={14} tone="chart-4" />
                <Text style={{ fontSize: 13, fontWeight: 500, flex: 1, fontVariant: ["tabular-nums"] }} class="text-foreground">
                  {t("snippets.trd.missingDepsBanner", { count: missingDeps().length })}
                </Text>
              </View>
              <Text style={{ fontSize: 12, fontFamily: MONO }} class="text-secondary-foreground">
                {missingDeps().join("  ")}
              </Text>
              <Btn
                variant="outline"
                icon="plus"
                label={t("snippets.trd.fillMissingDeps")}
                onPress={fillMissingDeps}
              />
            </View>
          </If>

          <Scroll style={{ flex: 1 }} contentStyle={{ padding: 10, gap: 10 }}>
            <Each of={chapters()} key={(c) => c.title}>
              {(ch, chIdx) => {
                const availableToAdd = computed<MenuEntry[]>(() => {
                  const used = allIncludedSlugs();
                  return allSnippets()
                    .filter((s) => !used.has(s.slug))
                    .map((s) => ({ id: s.slug, label: s.title, icon: s.has_code ? "code" : "sigma" }));
                });
                const chapterMenuEntries = computed<MenuEntry[]>(() => [
                  {
                    id: "up",
                    label: t("snippets.moveUp"),
                    icon: "chevronUp",
                    disabled: chIdx() === 0,
                  },
                  {
                    id: "down",
                    label: t("snippets.moveDown"),
                    icon: "chevronDown",
                    disabled: chIdx() >= chapters().length - 1,
                  },
                  { separator: true, id: "sep-del-ch" },
                  {
                    id: "delete",
                    label: t("snippets.trd.deleteChapterTitle"),
                    icon: "trash",
                    destructive: true,
                  },
                ]);
                return (
                  <View
                    style={{ borderRadius: 10, borderWidth: 1, padding: 10, gap: 6 }}
                    class="border-border bg-card"
                  >
                    <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                      <Text
                        style={{ flex: 1, fontSize: 13.5, fontWeight: 500, numberOfLines: 1, textOverflow: "ellipsis" }}
                        class="text-foreground"
                      >
                        {ch().title}
                      </Text>
                      <If when={!locked()}>
                        <AppMenu
                          align="end"
                          entries={chapterMenuEntries()}
                          onSelect={(act) => {
                            if (act === "up") moveChapter(chIdx(), -1);
                            else if (act === "down") moveChapter(chIdx(), 1);
                            else if (act === "delete") {
                              deleteChapterTarget.set({ idx: chIdx(), title: ch().title });
                            }
                          }}
                          trigger={
                            <View
                              style={{
                                width: 24,
                                height: 24,
                                borderRadius: 5,
                                alignItems: "center",
                                justifyContent: "center",
                              }}
                              class="hover:bg-muted"
                            >
                              <Icon name="ellipsis" size={14} tone="muted-foreground" />
                            </View>
                          }
                        />
                      </If>
                    </View>

                    <View style={{ gap: 2 }}>
                      <Each of={ch().items ?? []} key={(slug) => slug}>
                        {(itemSlug, itemIdx) => {
                          const meta = () => allSnippets().find((s) => s.slug === itemSlug());
                          const isMissing = () => !meta();
                          const itemMenuEntries = computed<MenuEntry[]>(() => [
                            {
                              id: "up",
                              label: t("snippets.moveUp"),
                              icon: "chevronUp",
                              disabled: itemIdx() === 0,
                            },
                            {
                              id: "down",
                              label: t("snippets.moveDown"),
                              icon: "chevronDown",
                              disabled: itemIdx() >= (ch().items ?? []).length - 1,
                            },
                            { separator: true, id: "sep-rm" },
                            {
                              id: "remove",
                              label: t("snippets.trd.removeItem"),
                              icon: "trash",
                              destructive: true,
                            },
                          ]);
                          return (
                            <View
                              style={{
                                height: 34,
                                paddingLeft: 8,
                                paddingRight: 4,
                                borderRadius: 6,
                                flexDirection: "row",
                                alignItems: "center",
                                gap: 8,
                              }}
                              class="hover:bg-muted"
                            >
                              <Icon
                                name={isMissing() ? "circleAlert" : meta()?.has_code === false ? "sigma" : "code"}
                                size={13.5}
                                tone={isMissing() ? "destructive" : "muted-foreground"}
                              />
                              <View style={{ flex: 1, minWidth: 0 }}>
                                <Text
                                  style={{ fontSize: 13, numberOfLines: 1, textOverflow: "ellipsis" }}
                                  class={isMissing() ? "text-destructive" : "text-foreground"}
                                >
                                  {meta()?.title ?? itemSlug()}
                                </Text>
                              </View>
                              <If when={!locked()}>
                                <AppMenu
                                  align="end"
                                  entries={itemMenuEntries()}
                                  onSelect={(act) => {
                                    if (act === "up") moveSnippetInChapter(chIdx(), itemIdx(), -1);
                                    else if (act === "down") moveSnippetInChapter(chIdx(), itemIdx(), 1);
                                    else if (act === "remove") removeSnippetFromChapter(chIdx(), itemSlug());
                                  }}
                                  trigger={
                                    <View
                                      style={{
                                        width: 22,
                                        height: 22,
                                        borderRadius: 5,
                                        alignItems: "center",
                                        justifyContent: "center",
                                      }}
                                      class="hover:bg-accent"
                                    >
                                      <Icon name="ellipsis" size={13} tone="muted-foreground" />
                                    </View>
                                  }
                                />
                              </If>
                            </View>
                          );
                        }}
                      </Each>
                    </View>

                    <If when={!locked() && availableToAdd().length > 0}>
                      <AppMenu
                        entries={availableToAdd()}
                        onSelect={(slug) => addSnippetToChapter(chIdx(), slug)}
                        trigger={
                          <View
                            style={{
                              height: 28,
                              borderRadius: 6,
                              flexDirection: "row",
                              alignItems: "center",
                              justifyContent: "center",
                              gap: 5,
                            }}
                            class="hover:bg-muted"
                          >
                            <Icon name="plus" size={13} tone="secondary-foreground" />
                            <Text style={{ fontSize: 12.5 }} class="text-secondary-foreground">
                              {t("snippets.trd.addSnippet")}
                            </Text>
                          </View>
                        }
                      />
                    </If>
                  </View>
                );
              }}
            </Each>
          </Scroll>

          <If when={!locked()}>
            <View
              style={{ padding: 10, borderTopWidth: 1, flexDirection: "row", alignItems: "center", gap: 6 }}
              class="border-border"
            >
              <Input
                value={newChapterTitle()}
                placeholder={t("snippets.trd.chapterTitlePlaceholder")}
                onChange={(e) => newChapterTitle.set(e.value)}
                onSubmit={addChapter}
                class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring"
                style={{ flex: 1, height: 30, borderRadius: 6, paddingLeft: 8, paddingRight: 8, fontSize: 13 }}
              />
              <Btn icon="plus" onPress={addChapter} disabled={!newChapterTitle().trim()} />
            </View>
          </If>
        </View>

        {/* Center live Typst A4 preview */}
        <View style={{ flex: 1, minWidth: 0 }}>
          <PreviewCanvas
            request={previewRequest}
            controller={(c) => {
              canvasCtrl = c;
            }}
            onResult={(r) => {
              const id = trdId();
              if (r.pages.length > 0 && id && fittedForId !== id) {
                fittedForId = id;
                setTimeout(() => canvasCtrl?.fitWidth(), 0);
              }
            }}
            hud
            active={props.active}
          />
        </View>

        {/* Right collapsible Properties drawer (320pt per §9.1.12) */}
        <If when={propsOpen()}>
          <View style={{ width: 320, borderLeftWidth: 1, flexDirection: "column" }} class="border-border bg-background">
            <View
              style={{
                height: 38,
                paddingLeft: 12,
                paddingRight: 12,
                justifyContent: "center",
                borderBottomWidth: 1,
              }}
              class="border-border"
            >
              <Segmented
                items={[
                  { value: "info" as const, label: t("snippets.trd.propInfo") },
                  { value: "layout" as const, label: t("snippets.trd.propLayout") },
                ]}
                value={propTab()}
                onChange={(v) => propTab.set(v)}
                itemWidth={72}
                underline
              />
            </View>

            <Scroll style={{ flex: 1 }} contentStyle={{ padding: 14, gap: 14 }}>
              <If when={propTab() === "info"}>
                <View style={{ gap: 12 }}>
                  <View style={{ gap: 4 }}>
                    <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
                      {t("snippets.trd.nameLabel")}
                    </Text>
                    <If
                      when={!locked()}
                      else={
                        <View style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 10, justifyContent: "center" }} class="bg-muted/50 border border-border">
                          <Text style={{ fontSize: 13 }} class="text-secondary-foreground">{metaName()}</Text>
                        </View>
                      }
                    >
                      <Input
                        value={metaName()}
                        onChange={(e) => metaName.set(e.value)}
                        onBlur={saveMetaInfo}
                        onSubmit={saveMetaInfo}
                        class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring"
                        style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 10, fontSize: 13 }}
                      />
                    </If>
                  </View>
                  <View style={{ gap: 4 }}>
                    <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
                      {t("snippets.trd.authorLabel")}
                    </Text>
                    <If
                      when={!locked()}
                      else={
                        <View style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 10, justifyContent: "center" }} class="bg-muted/50 border border-border">
                          <Text style={{ fontSize: 13 }} class="text-secondary-foreground">{metaAuthor() || "—"}</Text>
                        </View>
                      }
                    >
                      <Input
                        value={metaAuthor()}
                        onChange={(e) => metaAuthor.set(e.value)}
                        onBlur={saveMetaInfo}
                        onSubmit={saveMetaInfo}
                        class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring"
                        style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 10, fontSize: 13 }}
                      />
                    </If>
                  </View>
                  <View style={{ gap: 4 }}>
                    <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
                      {t("snippets.trd.dateLabel")}
                    </Text>
                    <If
                      when={!locked()}
                      else={
                        <View style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 10, justifyContent: "center" }} class="bg-muted/50 border border-border">
                          <Text style={{ fontSize: 13 }} class="text-secondary-foreground">{metaDate() || "—"}</Text>
                        </View>
                      }
                    >
                      <Input
                        value={metaDate()}
                        onChange={(e) => metaDate.set(e.value)}
                        onBlur={saveMetaInfo}
                        onSubmit={saveMetaInfo}
                        class="bg-muted border border-border focus:ring-1 focus:ring-inset focus:ring-ring"
                        style={{ height: 32, borderRadius: 6, paddingLeft: 10, paddingRight: 10, fontSize: 13 }}
                      />
                    </If>
                  </View>
                </View>
              </If>

              <If when={propTab() === "layout"}>
                <View style={{ gap: 14 }}>
                  <TemplatePicker
                    templates={catalog().templates}
                    value={effectiveTemplateId()}
                    readOnly={locked()}
                    onChange={changeTemplate}
                  />
                  <If when={!!activeTemplate()}>
                    <TemplateOptionsEditor
                      ownerKey={`trd:${trdId()}:${effectiveTemplateId()}`}
                      template={activeTemplate()}
                      scope="book"
                      baseValues={{}}
                      overrideValues={overrideOptions()}
                      fonts={catalog().fonts}
                      readOnly={locked()}
                      onChange={(key, val) => updateTemplateOption(effectiveTemplateId(), key, val)}
                    />
                  </If>
                </View>
              </If>
            </Scroll>
          </View>
        </If>
      </View>

      {/* Delete Chapter confirmation modal (per review 0.5 & §9.1.4) */}
      <Modal
        open={deleteChapterTarget() !== undefined}
        onClose={() => deleteChapterTarget.set(undefined)}
        width={420}
        testID="trd-delete-chapter-modal"
      >
        <View style={{ padding: 18, gap: 14 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
            <View
              style={{ width: 32, height: 32, borderRadius: 8, alignItems: "center", justifyContent: "center" }}
              class="bg-muted"
            >
              <Icon name="trash" size={16} tone="destructive" />
            </View>
            <View style={{ flex: 1, gap: 2 }}>
              <Text style={{ fontSize: 15, fontWeight: 600 }} class="text-foreground">
                {t("snippets.trd.deleteChapterTitle")}
              </Text>
              <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
                {deleteChapterTarget()
                  ? t("snippets.trd.deleteChapterDesc", { title: deleteChapterTarget()!.title })
                  : ""}
              </Text>
            </View>
          </View>
          <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
            <Btn variant="outline" label={t("common.cancel")} onPress={() => deleteChapterTarget.set(undefined)} />
            <Btn variant="danger" label={t("common.delete")} onPress={confirmRemoveChapter} />
          </View>
        </View>
      </Modal>

      {/* Finalize modal (S-1: no strict toggle in GUI) */}
      <Modal open={finalizeOpen()} onClose={() => finalizeOpen.set(false)} width={460} testID="trd-finalize-modal">
        <View style={{ padding: 18, gap: 14 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
            <View
              style={{ width: 32, height: 32, borderRadius: 8, alignItems: "center", justifyContent: "center" }}
              class="bg-muted"
            >
              <Icon name="stamp" size={16} tone="foreground" />
            </View>
            <View style={{ flex: 1, gap: 2 }}>
              <Text style={{ fontSize: 15, fontWeight: 600 }} class="text-foreground">
                {t("snippets.trd.finalizeConfirmTitle")}
              </Text>
              <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
                {t("snippets.trd.finalizeConfirmDesc")}
              </Text>
            </View>
          </View>

          <If when={blockingErrors().length > 0}>
            <View style={{ padding: 10, borderRadius: 8, borderWidth: 1, gap: 6 }} class="border-destructive/40 bg-destructive/5">
              <Text style={{ fontSize: 13, fontWeight: 500 }} class="text-destructive">
                {t("snippets.trd.finalizeBlocking")}
              </Text>
              <Each of={blockingErrors().slice(0, 8)} key={(e, i) => `${i}:${e.snippet}:${e.kind}`}>
                {(e) => (
                  <Text style={{ fontSize: 13 }} class="text-destructive">
                    {e().message}
                  </Text>
                )}
              </Each>
            </View>
          </If>

          <If when={verifyWarnings().length > 0}>
            <View style={{ padding: 10, borderRadius: 8, borderWidth: 1, gap: 6 }} class="border-border bg-muted/50">
              <Text style={{ fontSize: 13, fontWeight: 500 }} class="text-foreground">
                {t("snippets.trd.finalizeWarnings")}
              </Text>
              <Each of={verifyWarnings().slice(0, 8)} key={(w, i) => `${i}:${w.snippet}`}>
                {(w) => (
                  <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
                    {w().message}
                  </Text>
                )}
              </Each>
            </View>
          </If>

          <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
            <Btn variant="outline" label={t("common.cancel")} onPress={() => finalizeOpen.set(false)} />
            <Btn
              variant="primary"
              label={t("snippets.trd.finalize")}
              disabled={finalizing() || blockingErrors().length > 0}
              onPress={() => void confirmFinalize()}
            />
          </View>
        </View>
      </Modal>

      {/* Reopen modal (2.10: icon-square header + subtitle aligned with Finalize modal) */}
      <Modal open={reopenOpen()} onClose={() => reopenOpen.set(false)} width={460} testID="trd-reopen-modal">
        <View style={{ padding: 18, gap: 14 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
            <View
              style={{ width: 32, height: 32, borderRadius: 8, alignItems: "center", justifyContent: "center" }}
              class="bg-muted"
            >
              <Icon name="lockOpen" size={16} tone="foreground" />
            </View>
            <View style={{ flex: 1, gap: 2 }}>
              <Text style={{ fontSize: 15, fontWeight: 600 }} class="text-foreground">
                {t("snippets.trd.reopenTitle")}
              </Text>
              <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
                {t("snippets.trd.reopenDesc")}
              </Text>
            </View>
          </View>

          <If
            when={Boolean(reopenDiff()?.has_changes)}
            else={
              <Text style={{ fontSize: 13, lineHeight: 19 }} class="text-secondary-foreground">
                {t("snippets.trd.reopenNoDiff")}
              </Text>
            }
          >
            <View style={{ gap: 8 }}>
              <Text style={{ fontSize: 13 }} class="text-secondary-foreground">
                {t("snippets.trd.reopenDiffDesc")}
              </Text>
              <View style={{ padding: 10, borderRadius: 8, borderWidth: 1, gap: 6 }} class="border-border bg-muted/50">
                <Each of={reopenDiff()?.diffs ?? []} key={(d) => d.slug}>
                  {(d) => (
                    <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                      <Text style={{ fontSize: 13, fontFamily: MONO, fontWeight: 500 }} class="text-foreground">
                        {d().slug}
                      </Text>
                      <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                        {(d().changed_files ?? []).join("  ")}
                      </Text>
                    </View>
                  )}
                </Each>
              </View>
            </View>
          </If>

          <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
            <Btn variant="outline" label={t("common.cancel")} onPress={() => reopenOpen.set(false)} />
            <Btn variant="primary" label={t("snippets.trd.reopen")} onPress={confirmReopen} />
          </View>
        </View>
      </Modal>
    </View>
  );
}
