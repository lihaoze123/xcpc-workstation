// Top-level 「模板」 space: isomorphic 52pt toolbar (`算法库 | 模板册`, `展开提交代码`, `新建`),
// master-detail snippet library centered on live rendered preview (1.2 / S-6), and
// Living TRD list with finalized lock icons and collapsible Archived section.

import { cleanup, Clipboard, computed, effect, keymap, Scroll, signal, Text, View } from "nsh";
import { Input } from "nsh/base";
import type { PreviewRequest } from "xcpc-workstation/native";
import { Icon } from "../Icon";
import { t, type MessageKey } from "../i18n";
import { Btn, Chip, createTransientFlag, DelayedSpinner, Lock, Segmented } from "../kit";
import { DetailHeader, EmptyState, GUTTER, Spacer, Toolbar } from "../layout";
import { AppMenu, MoreTrigger, type MenuEntry } from "../Menu";
import { Modal } from "../Modal";
import { PreviewCanvas, type CanvasController } from "../studio/PreviewCanvas";
import { showToast } from "../Toast";
import {
  activeRevision,
  bundlerModalOpen,
  bumpSnippetsRevision,
  goSettings,
  info,
  jumpToProblem,
  newTrdModalOpen,
  openNewSnippetModal,
  openSnippetStudio,
  openTrdStudio,
  preview,
  selectedSnippetSlug,
  selectedTrdId,
  snippetsRevision,
  snippetsSubTab,
} from "../store";
import { cardShadow, EASE_STANDARD, MONO, ms } from "../theme";
import {
  filterSnippets,
  parseJsonPayload,
  partitionTrds,
  resolveSnippetPreviewTemplate,
  snippetStatusStyle,
  type SnippetStatusFilter,
} from "./snippetLogic";
import type {
  SnippetDetailPayload,
  SnippetListPayload,
  SnippetSummaryItem,
  TrdDetailPayload,
  TrdSummaryItem,
} from "./types";
import { runSnippetVerifyJob } from "./verifyJob";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "../tokens";
// Per review S-4: formula cards are a type (Σ icon), not a verification status.
const STATUS_FILTERS: { id: SnippetStatusFilter; labelKey: MessageKey }[] = [
  { id: "all", labelKey: "snippets.filter.all" },
  { id: "unverified", labelKey: "snippets.filter.unverified" },
  { id: "stale", labelKey: "snippets.filter.stale" },
  { id: "failed", labelKey: "snippets.filter.failed" },
  { id: "passed", labelKey: "snippets.filter.passed" },
];

const LIST_PANE_WIDTH = 310;

export function SnippetsView(props: { active: boolean }) {
  const snippetList = signal<SnippetSummaryItem[]>([]);
  const allTags = signal<string[]>([]);
  const trdList = signal<TrdSummaryItem[]>([]);
  const query = signal("");
  const statusFilter = signal<SnippetStatusFilter>("all");
  const tagFilter = signal<string>("");
  const archivedOpen = signal(false);
  const deleteSnippetTarget = signal<SnippetSummaryItem | undefined>(undefined);
  const deleteTrdTarget = signal<TrdSummaryItem | undefined>(undefined);

  function loadLists() {
    if (!info().root) return;
    const snips = parseJsonPayload<SnippetListPayload>(preview.listSnippetsJson());
    if (snips) {
      snippetList.set(snips.snippets ?? []);
      allTags.set(snips.tags ?? []);
    }
    const trds = parseJsonPayload<{ trds: TrdSummaryItem[] }>(preview.listTrdsJson());
    if (trds) {
      trdList.set(trds.trds ?? []);
    }
  }

  const rev = activeRevision(() => props.active);
  const workspaceRoot = computed(() => info().root);

  effect(() => {
    if (!props.active) return;
    void workspaceRoot();
    void rev();
    void snippetsRevision();
    loadLists();
  });

  const filteredSnippets = computed(() =>
    filterSnippets(snippetList(), {
      query: query(),
      statusFilter: statusFilter(),
      tag: tagFilter(),
    }),
  );

  effect(() => {
    const list = filteredSnippets();
    const cur = selectedSnippetSlug();
    if (list.length === 0) {
      if (cur !== undefined) queueMicrotask(() => selectedSnippetSlug.set(undefined));
    } else if (!cur || !list.some((s) => s.slug === cur)) {
      queueMicrotask(() => selectedSnippetSlug.set(list[0].slug));
    }
  });

  const trdParts = computed(() => partitionTrds(trdList()));
  effect(() => {
    const all = trdList();
    const cur = selectedTrdId();
    if (all.length === 0) {
      if (cur !== undefined) queueMicrotask(() => selectedTrdId.set(undefined));
    } else if (!cur || !all.some((x) => x.id === cur)) {
      const next = trdParts().active[0]?.id ?? trdParts().archived[0]?.id;
      if (next !== cur) queueMicrotask(() => selectedTrdId.set(next));
    }
  });

  function confirmDeleteSnippet() {
    const target = deleteSnippetTarget();
    if (!target) return;
    const res = preview.deleteSnippet(target.slug);
    deleteSnippetTarget.set(undefined);
    if (!res.ok) {
      showToast({ tone: "issues", text: res.error ?? t("snippets.error.deleteFailed") });
      return;
    }
    bumpSnippetsRevision();
  }

  function confirmDeleteTrd() {
    const target = deleteTrdTarget();
    if (!target) return;
    const res = preview.deleteTrd(target.id);
    deleteTrdTarget.set(undefined);
    if (!res.ok) {
      showToast({ tone: "issues", text: res.error ?? t("snippets.error.deleteFailed") });
      return;
    }
    bumpSnippetsRevision();
  }

  keymap.bind(
    {
      j: () => {
        if (!props.active) return;
        if (snippetsSubTab() === "library") {
          const list = filteredSnippets();
          const idx = list.findIndex((s) => s.slug === selectedSnippetSlug());
          if (idx >= 0 && idx + 1 < list.length) selectedSnippetSlug.set(list[idx + 1].slug);
        }
      },
      k: () => {
        if (!props.active) return;
        if (snippetsSubTab() === "library") {
          const list = filteredSnippets();
          const idx = list.findIndex((s) => s.slug === selectedSnippetSlug());
          if (idx > 0) selectedSnippetSlug.set(list[idx - 1].slug);
        }
      },
    },
    { when: "notEditing" },
  );

  const activeEmpty = () =>
    snippetsSubTab() === "library" ? snippetList().length === 0 : trdList().length === 0;

  return (
    <View style={{ flex: 1, flexDirection: "column" }} class="bg-background">
      {/* Unified 52pt top bar: vertically centered Segmented, Bundler trigger, fixed-width New button */}
      <Toolbar>
        <Segmented
          items={[
            { value: "library" as const, label: t("snippets.tab.library") },
            { value: "trds" as const, label: t("snippets.tab.trds") },
          ]}
          value={snippetsSubTab()}
          onChange={(v) => snippetsSubTab.set(v)}
          itemWidth={92}
          centered
        />
        <Spacer />
        <Btn
          testID="snippets-open-bundler"
          variant="outline"
          icon="fileCode2"
          label={t("snippets.bundler")}
          onPress={() => bundlerModalOpen.set(true)}
        />
        <If when={!activeEmpty()}>
          <Btn
            testID="snippets-new-primary"
            variant="primary"
            icon="plus"
            label={snippetsSubTab() === "library" ? t("snippets.newSnippet") : t("snippets.newTrd")}
            disabled={info().readOnly}
            onPress={() => {
              if (snippetsSubTab() === "library") openNewSnippetModal();
              else newTrdModalOpen.set(true);
            }}
            style={{ width: 124 }}
          />
        </If>
      </Toolbar>

      {/* Sub-view 1: 算法库 */}
      <If when={snippetsSubTab() === "library"}>
        <If
          when={snippetList().length > 0}
          else={
            <EmptyState
              icon="code"
              title={t("snippets.emptyTitle")}
              hint={t("snippets.emptyDesc")}
              action={
                <Btn
                  variant="primary"
                  icon="plus"
                  label={t("snippets.newSnippet")}
                  disabled={info().readOnly}
                  onPress={() => openNewSnippetModal()}
                />
              }
            />
          }
        >
          <View style={{ flex: 1, flexDirection: "row", minHeight: 0 }}>
            {/* Left list column */}
            <View style={{ width: LIST_PANE_WIDTH, borderRightWidth: 1, flexDirection: "column" }} class="border-border">
              <View style={{ padding: SPACE[12], gap: SPACE[8], borderBottomWidth: 1 }} class="border-border">
                <View
                  class="bg-muted border border-border focus-within:ring-1 focus-within:ring-inset focus-within:ring-ring"
                  style={{
                    height: CONTROL.lg,
                    borderRadius: RADIUS.sm,
                    paddingLeft: SPACE[8],
                    paddingRight: SPACE[6],
                    flexDirection: "row",
                    alignItems: "center",
                    gap: SPACE[6],
                  }}
                >
                  <Icon name="search" size="sm" tone="muted-foreground" />
                  <Input
                    value={query()}
                    placeholder={t("snippets.searchPlaceholder")}
                    onChange={(e) => query.set(e.value)}
                    class="bg-transparent placeholder:text-muted-foreground"
                    style={{ flex: 1, height: CONTROL.md, fontSize: FONT_SIZE.body }}
                  />
                </View>

                <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
                  <AppMenu
                    entries={STATUS_FILTERS.map((f) => ({
                      id: f.id,
                      label: t(f.labelKey),
                      checked: statusFilter() === f.id,
                    }))}
                    onSelect={(id) => statusFilter.set(id as SnippetStatusFilter)}
                    trigger={
                      <View
                        class="bg-card border border-border hover:bg-muted"
                        style={{
                          height: CONTROL.sm,
                          paddingLeft: SPACE[8],
                          paddingRight: SPACE[6],
                          borderRadius: RADIUS.sm,
                          flexDirection: "row",
                          alignItems: "center",
                          gap: SPACE[4],
                        }}
                      >
                        <Text style={{ fontSize: FONT_SIZE.caption }} class="text-foreground">
                          {t(STATUS_FILTERS.find((f) => f.id === statusFilter())!.labelKey)}
                        </Text>
                        <Icon name="chevronDown" size="sm" tone="muted-foreground" />
                      </View>
                    }
                  />

                  <If when={allTags().length > 0}>
                    <AppMenu
                      entries={[
                        { id: "", label: t("snippets.filter.all"), checked: !tagFilter() },
                        ...allTags().map((tg) => ({ id: tg, label: tg, checked: tagFilter() === tg })),
                      ]}
                      onSelect={(id) => tagFilter.set(id)}
                      trigger={
                        <View
                          class="bg-card border border-border hover:bg-muted"
                          style={{
                            height: CONTROL.sm,
                            paddingLeft: SPACE[8],
                            paddingRight: SPACE[6],
                            borderRadius: RADIUS.sm,
                            flexDirection: "row",
                            alignItems: "center",
                            gap: SPACE[4],
                          }}
                        >
                          <Text style={{ fontSize: FONT_SIZE.caption }} class="text-foreground">
                            {tagFilter() || t("snippets.newModal.tags")}
                          </Text>
                          <Icon name="chevronDown" size="sm" tone="muted-foreground" />
                        </View>
                      }
                    />
                  </If>
                  <Spacer />
                  <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                    {t("snippets.count", { count: filteredSnippets().length })}
                  </Text>
                </View>
              </View>

              <If
                when={filteredSnippets().length > 0}
                else={
                  <EmptyState
                    icon="searchX"
                    title={t("snippets.filterEmptyTitle")}
                    action={
                      <Btn
                        variant="outline"
                        icon="filterX"
                        label={t("snippets.filterEmptyClear")}
                        onPress={() => {
                          query.set("");
                          statusFilter.set("all");
                          tagFilter.set("");
                        }}
                      />
                    }
                  />
                }
              >
                <Scroll style={{ flex: 1 }} contentStyle={{ padding: SPACE[8], gap: SPACE[2] }}>
                  <Each of={filteredSnippets()} key={(s) => s.slug}>
                    {(s) => {
                      const selected = () => s().slug === selectedSnippetSlug();
                      const st = () => snippetStatusStyle(s().status, s().has_code);
                      return (
                        <View
                          testID={`snippet-row-${s().slug}`}
                          onPress={(e) => {
                            selectedSnippetSlug.set(s().slug);
                            if (e.clickCount >= 2) openSnippetStudio(s().slug);
                          }}
                          class={selected() ? "bg-accent" : "hover:bg-muted active:bg-accent"}
                          style={{
                            height: 54,
                            paddingLeft: SPACE[12],
                            paddingRight: SPACE[12],
                            borderRadius: RADIUS.md,
                            flexDirection: "row",
                            alignItems: "center",
                            gap: SPACE[8],
                            transitionProperty: "colors",
                            transitionDuration: ms(120),
                            transitionTimingFunction: EASE_STANDARD,
                          }}
                        >
                          <Icon name={s().has_code ? "code" : "sigma"} size="sm" tone="secondary-foreground" />
                          <View style={{ flex: 1, minWidth: 0, gap: SPACE[2] }}>
                            <Text
                              style={{ fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.body, numberOfLines: 1, textOverflow: "ellipsis" }}
                              class="text-foreground"
                            >
                              {s().title}
                            </Text>
                            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                              <Text style={{ fontSize: FONT_SIZE.caption, fontFamily: MONO }} class="text-muted-foreground">
                                {s().slug}
                              </Text>
                              <If when={s().has_code}>
                                <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                                  {t("snippets.lines", { count: s().code_lines })}
                                </Text>
                              </If>
                            </View>
                          </View>
                          <View
                            style={{
                              height: 20, // token-exempt: compact status badge height
                              paddingLeft: SPACE[6],
                              paddingRight: SPACE[6],
                              borderRadius: RADIUS.xs,
                              justifyContent: "center",
                              backgroundColor: st().bg,
                            }}
                          >
                            <Text style={{ fontSize: FONT_SIZE.caption, color: st().fg }}>
                              {t(st().labelKey as MessageKey)}
                            </Text>
                          </View>
                        </View>
                      );
                    }}
                  </Each>
                </Scroll>
              </If>
            </View>

            {/* Right detail pane centered on live rendered snippet preview */}
            <View style={{ flex: 1, minWidth: 0 }}>
              <If when={!!selectedSnippetSlug()}>
                <SnippetDetailPane
                  slug={selectedSnippetSlug()!}
                  allSnippets={snippetList()}
                  active={props.active && snippetsSubTab() === "library"}
                  onJumpSnippet={(targetSlug) => selectedSnippetSlug.set(targetSlug)}
                  onRequestDelete={(item) => deleteSnippetTarget.set(item)}
                />
              </If>
            </View>
          </View>
        </If>
      </If>

      {/* Sub-view 2: 模板册 */}
      <If when={snippetsSubTab() === "trds"}>
        <If
          when={trdList().length > 0}
          else={
            <EmptyState
              icon="bookOpen"
              title={t("snippets.trd.emptyTitle")}
              hint={t("snippets.trd.emptyDesc")}
              action={
                <Btn
                  variant="primary"
                  icon="plus"
                  label={t("snippets.newTrd")}
                  disabled={info().readOnly}
                  onPress={() => newTrdModalOpen.set(true)}
                />
              }
            />
          }
        >
          <View style={{ flex: 1, flexDirection: "row", minHeight: 0 }}>
            <View style={{ width: LIST_PANE_WIDTH, borderRightWidth: 1, flexDirection: "column" }} class="border-border">
              <Scroll style={{ flex: 1 }} contentStyle={{ padding: SPACE[8], gap: SPACE[2] }}>
                <Each of={trdParts().active} key={(trd) => trd.id}>
                  {(trd) => (
                    <TrdRow
                      item={trd()}
                      selected={trd().id === selectedTrdId()}
                      onSelect={() => selectedTrdId.set(trd().id)}
                      onOpen={() => openTrdStudio(trd().id)}
                    />
                  )}
                </Each>

                <If when={trdParts().archived.length > 0}>
                  <View style={{ marginTop: SPACE[8] }}>
                    <View
                      onPress={() => archivedOpen.set(!archivedOpen())}
                      style={{
                        height: CONTROL.md,
                        paddingLeft: SPACE[8],
                        paddingRight: SPACE[8],
                        borderRadius: RADIUS.sm,
                        flexDirection: "row",
                        alignItems: "center",
                        gap: SPACE[6],
                      }}
                      class="hover:bg-muted"
                    >
                      <Icon
                        name={archivedOpen() ? "chevronDown" : "chevronRight"}
                        size="sm"
                        tone="muted-foreground"
                      />
                      <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                        {t("snippets.trd.archived")}
                      </Text>
                      <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                        {String(trdParts().archived.length)}
                      </Text>
                    </View>
                    <If when={archivedOpen()}>
                      <View style={{ gap: SPACE[2], marginTop: SPACE[2] }}>
                        <Each of={trdParts().archived} key={(trd) => trd.id}>
                          {(trd) => (
                            <TrdRow
                              item={trd()}
                              muted
                              selected={trd().id === selectedTrdId()}
                              onSelect={() => selectedTrdId.set(trd().id)}
                              onOpen={() => openTrdStudio(trd().id)}
                            />
                          )}
                        </Each>
                      </View>
                    </If>
                  </View>
                </If>
              </Scroll>
            </View>

            <View style={{ flex: 1, minWidth: 0 }}>
              <If when={!!selectedTrdId()}>
                <TrdDetailPane
                  id={selectedTrdId()!}
                  allSnippets={snippetList()}
                  active={props.active && snippetsSubTab() === "trds"}
                  onRequestDelete={(item) => deleteTrdTarget.set(item)}
                />
              </If>
            </View>
          </View>
        </If>
      </If>

      {/* Delete Snippet confirm modal */}
      <Modal
        open={deleteSnippetTarget() !== undefined}
        onClose={() => deleteSnippetTarget.set(undefined)}
        width={420}
      >
        <View style={{ padding: SPACE[16], gap: SPACE[12] }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
            <View
              style={{ width: 32, height: CONTROL.lg, borderRadius: RADIUS.md, alignItems: "center", justifyContent: "center" }}
              class="bg-muted"
            >
              <Icon name="trash" size="md" tone="destructive" />
            </View>
            <View style={{ flex: 1, gap: SPACE[2] }}>
              <Text style={{ fontSize: FONT_SIZE.strong, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                {t("snippets.deleteTitle")}
              </Text>
              <Text style={{ fontSize: FONT_SIZE.body }} class="text-secondary-foreground">
                {deleteSnippetTarget() ? t("snippets.deleteDesc", { title: deleteSnippetTarget()!.title }) : ""}
              </Text>
            </View>
          </View>
          <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: SPACE[8] }}>
            <Btn variant="outline" label={t("common.cancel")} onPress={() => deleteSnippetTarget.set(undefined)} />
            <Btn variant="danger" label={t("common.delete")} onPress={confirmDeleteSnippet} />
          </View>
        </View>
      </Modal>

      {/* Delete TRD confirm modal */}
      <Modal open={deleteTrdTarget() !== undefined} onClose={() => deleteTrdTarget.set(undefined)} width={420}>
        <View style={{ padding: SPACE[16], gap: SPACE[12] }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
            <View
              style={{ width: 32, height: CONTROL.lg, borderRadius: RADIUS.md, alignItems: "center", justifyContent: "center" }}
              class="bg-muted"
            >
              <Icon name="trash" size="md" tone="destructive" />
            </View>
            <View style={{ flex: 1, gap: SPACE[2] }}>
              <Text style={{ fontSize: FONT_SIZE.strong, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                {t("snippets.trd.deleteTitle")}
              </Text>
              <Text style={{ fontSize: FONT_SIZE.body }} class="text-secondary-foreground">
                {deleteTrdTarget() ? t("snippets.trd.deleteDesc", { name: deleteTrdTarget()!.name }) : ""}
              </Text>
            </View>
          </View>
          <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: SPACE[8] }}>
            <Btn variant="outline" label={t("common.cancel")} onPress={() => deleteTrdTarget.set(undefined)} />
            <Btn variant="danger" label={t("common.delete")} onPress={confirmDeleteTrd} />
          </View>
        </View>
      </Modal>
    </View>
  );
}

function SnippetDetailPane(props: {
  slug: string;
  allSnippets: readonly SnippetSummaryItem[];
  active: boolean;
  onJumpSnippet: (slug: string) => void;
  onRequestDelete: (item: SnippetSummaryItem) => void;
}) {
  const payload = signal<SnippetDetailPayload | undefined>(undefined);
  const previewTemplate = signal<string>("trd-compact");
  const copiedFlag = createTransientFlag(1600);
  const verifying = signal(false);
  const showVerifySpinner = signal(false);
  let verifySpinTimer: ReturnType<typeof setTimeout> | undefined;
  let canvasCtrl: CanvasController | undefined;

  effect(() => {
    const busy = verifying();
    clearTimeout(verifySpinTimer);
    if (!busy) showVerifySpinner.set(false);
    else verifySpinTimer = setTimeout(() => showVerifySpinner.set(true), 150);
  });

  function load(s: string) {
    const parsed = parseJsonPayload<SnippetDetailPayload>(preview.loadSnippetJson(s));
    payload.set(parsed);
    const trds = parseJsonPayload<{ trds: TrdSummaryItem[] }>(preview.listTrdsJson())?.trds ?? [];
    const trdDetails: { id: string; template?: string | null; chapters?: { title: string; items: string[] }[] }[] = [];
    for (const trd of trds) {
      const det = parseJsonPayload<TrdDetailPayload>(preview.loadTrdJson(trd.id));
      if (det) {
        trdDetails.push({ id: trd.id, template: det.detail.meta.template, chapters: det.detail.meta.chapter ?? [] });
      }
    }
    previewTemplate.set(resolveSnippetPreviewTemplate(s, trdDetails, "trd-compact"));
  }

  const rev = activeRevision(() => props.active);
  let lastLoadedSlug = "";
  effect(() => {
    if (!props.active) return;
    const s = props.slug;
    void rev();
    void snippetsRevision();
    const changed = s !== lastLoadedSlug;
    lastLoadedSlug = s;
    load(s);
    if (!changed) {
      canvasCtrl?.compile();
    }
  });

  cleanup(() => {
    clearTimeout(verifySpinTimer);
  });

  function copyBundled() {
    const res = preview.bundleSnippetJson(props.slug, false);
    const parsed = parseJsonPayload<{ code: string }>(res);
    if (parsed?.code) {
      void Clipboard.writeText(parsed.code);
      copiedFlag.trigger();
    } else if (res.error) {
      showToast({ tone: "issues", text: res.error });
    }
  }

  function runVerify() {
    runSnippetVerifyJob({
      mode: "single",
      slug: props.slug,
      force: true,
      onStart: () => verifying.set(true),
      onFinish: () => {
        verifying.set(false);
        load(props.slug);
      },
    });
  }

  const detail = () => payload()?.detail;
  const hasCode = () => Boolean(detail()?.code);
  const statusInfo = computed(() => {
    const p = payload();
    if (!p) return undefined;
    return snippetStatusStyle(p.status, Boolean(p.detail.code));
  });

  const deps = () => detail()?.reverse_refs?.deps ?? [];
  const reverseDeps = () => detail()?.reverse_refs?.reverse_deps ?? [];
  const relatedProblems = () => detail()?.reverse_refs?.related_problems ?? [];
  const hasRelations = () => deps().length > 0 || reverseDeps().length > 0 || relatedProblems().length > 0;

  const failedCases = computed(() =>
    (payload()?.report?.cases ?? []).filter((c) => !c.passed),
  );

  const previewRequest = computed<PreviewRequest>(() => ({
    target: "snippet",
    id: props.slug,
    book: previewTemplate(),
    path: "",
    text: "",
  }));

  const moreEntries = computed<MenuEntry[]>(() => [
    { id: "reveal", label: t("common.revealInFolder"), icon: "folderOpen" },
    { separator: true, id: "sep-del" },
    {
      id: "delete",
      label: t("common.delete"),
      icon: "trash",
      destructive: true,
      disabled: info().readOnly,
    },
  ]);

  const titleOf = (s: string) => props.allSnippets.find((x) => x.slug === s)?.title ?? s;

  return (
    <View style={{ flex: 1, flexDirection: "column" }}>
      <View style={{ borderBottomWidth: 1 }} class="border-border">
        <DetailHeader
          title={detail()?.meta.title ?? props.slug}
          badge={
            statusInfo() ? (
              <View
                style={{
                  height: 20, // token-exempt: compact status badge height
                  paddingLeft: SPACE[8],
                  paddingRight: SPACE[8],
                  borderRadius: RADIUS.xs,
                  justifyContent: "center",
                  backgroundColor: statusInfo()!.bg,
                }}
              >
                <Text style={{ fontSize: FONT_SIZE.caption, color: statusInfo()!.fg }}>
                  {t(statusInfo()!.labelKey as MessageKey)}
                </Text>
              </View>
            ) : undefined
          }
          second={
            detail() &&
            ((detail()!.meta.tags ?? []).length > 0 ||
              detail()!.meta.time_complexity ||
              detail()!.meta.space_complexity) ? (
              <>
                <If when={!!detail()!.meta.time_complexity}>
                  <Chip label={detail()!.meta.time_complexity!} />
                </If>
                <If when={!!detail()!.meta.space_complexity}>
                  <Chip label={detail()!.meta.space_complexity!} />
                </If>
                <Each of={detail()!.meta.tags ?? []} key={(tg) => tg}>
                  {(tg) => <Chip label={tg()} />}
                </Each>
              </>
            ) : undefined
          }
          actions={
            <>
              <Btn variant="primary" icon="pencil" label={t("common.edit")} onPress={() => openSnippetStudio(props.slug)} />
              <If when={hasCode()}>
                <Btn
                  variant="outline"
                  icon={copiedFlag.active() ? "check" : "copy"}
                  label={copiedFlag.active() ? t("snippets.action.copied") : t("snippets.action.copySingleFile")}
                  onPress={copyBundled}
                  style={{ width: 112 }}
                />
                <Btn
                  variant="outline"
                  icon={showVerifySpinner() ? undefined : "play"}
                  label={verifying() ? t("snippets.action.verifying") : t("snippets.action.verify")}
                  disabled={verifying()}
                  onPress={runVerify}
                  style={{ width: 104 }}
                >
                  <DelayedSpinner active={verifying()} size={13} />
                </Btn>
              </If>
              <AppMenu
                align="end"
                entries={moreEntries()}
                onSelect={(id) => {
                  if (id === "reveal") void preview.revealInFolder(`snippets/${props.slug}`);
                  else if (id === "delete" && detail()) {
                    props.onRequestDelete({
                      slug: props.slug,
                      title: detail()!.meta.title,
                      tags: detail()!.meta.tags ?? [],
                      has_code: hasCode(),
                      has_notes: Boolean(detail()!.notes),
                      verify_count: (detail()!.meta.verify ?? []).length,
                      deps: deps(),
                      code_lines: 0,
                      status: payload()!.status,
                      reverse_deps: reverseDeps(),
                      related_problems: relatedProblems(),
                    });
                  }
                }}
                trigger={<MoreTrigger />}
              />
            </>
          }
        />

        {/* S-6: Compact inline relation row directly under header (hidden when empty) */}
        <If when={hasRelations()}>
          <View
            style={{
              paddingLeft: GUTTER,
              paddingRight: GUTTER,
              paddingBottom: SPACE[12],
              flexDirection: "row",
              flexWrap: "wrap",
              alignItems: "center",
              gap: SPACE[12],
            }}
          >
            <If when={deps().length > 0}>
              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
                <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                  {t("snippets.section.deps")}
                </Text>
                <Each of={deps()} key={(id) => id}>
                  {(id) => <Chip label={titleOf(id())} onPress={() => props.onJumpSnippet(id())} />}
                </Each>
              </View>
            </If>
            <If when={reverseDeps().length > 0}>
              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
                <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                  {t("snippets.section.reverseDeps")}
                </Text>
                <Each of={reverseDeps()} key={(id) => id}>
                  {(id) => <Chip label={titleOf(id())} onPress={() => props.onJumpSnippet(id())} />}
                </Each>
              </View>
            </If>
            <If when={relatedProblems().length > 0}>
              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
                <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                  {t("snippets.section.relatedProblems")}
                </Text>
                <Each of={relatedProblems()} key={(id) => id}>
                  {(id) => <Chip label={id()} onPress={() => jumpToProblem(id())} />}
                </Each>
              </View>
            </If>
          </View>
        </If>
      </View>

      {/* Main body: live rendered snippet preview (1.2) + floating failure overlay (J) */}
      <View style={{ flex: 1, minHeight: 0, position: "relative" }}>
        <PreviewCanvas
          request={previewRequest}
          controller={(c) => {
            canvasCtrl = c;
            setTimeout(() => canvasCtrl?.fitWidth(), 0);
          }}
          hud={false}
          active={props.active}
        />

        <If when={failedCases().length > 0}>
          <View
            style={{
              position: "absolute",
              top: 12,
              left: 16,
              right: 16,
              padding: SPACE[12],
              borderRadius: RADIUS.lg,
              borderWidth: 1,
              gap: SPACE[6],
              boxShadow: cardShadow(),
            }}
            class="border-border bg-popover"
          >
            <Each of={failedCases()} key={(c, i) => `${i}:${c.target}`}>
              {(c) => {
                const isUnavail = () => c().verdict === "UNAVAILABLE";
                return (
                  <View style={{ gap: SPACE[4] }}>
                    <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
                      <Icon
                        name="circleAlert"
                        size="sm"
                        tone={isUnavail() ? "chart-4" : "destructive"}
                      />
                      <Text
                        style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold, fontFamily: MONO }}
                        class={isUnavail() ? "text-foreground" : "text-destructive"}
                      >
                        {isUnavail() ? t("snippets.verify.unavailable") : c().verdict}
                      </Text>
                      <Text style={{ fontSize: FONT_SIZE.body, fontFamily: MONO, flex: 1 }} class="text-secondary-foreground">
                        {c().target}
                      </Text>
                      <If when={isUnavail()}>
                        <Btn
                          variant="outline"
                          label={t("snippets.verify.goSettings")}
                          onPress={() => goSettings("toolchain", { expandAdvanced: true })}
                        />
                      </If>
                    </View>
                    <If when={!!c().message}>
                      <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.body }} class="text-secondary-foreground">
                        {c().message!}
                      </Text>
                    </If>
                  </View>
                );
              }}
            </Each>
          </View>
        </If>
      </View>
    </View>
  );
}

function TrdRow(props: {
  item: TrdSummaryItem;
  selected: boolean;
  muted?: boolean;
  onSelect: () => void;
  onOpen: () => void;
}) {
  return (
    <View
      testID={`trd-row-${props.item.id}`}
      onPress={(e) => {
        props.onSelect();
        if (e.clickCount >= 2) props.onOpen();
      }}
      class={props.selected ? "bg-accent" : "hover:bg-muted active:bg-accent"}
      style={{
        height: 54,
        paddingLeft: SPACE[12],
        paddingRight: SPACE[12],
        borderRadius: RADIUS.md,
        flexDirection: "row",
        alignItems: "center",
        gap: SPACE[8],
        opacity: props.muted && !props.selected ? 0.78 : 1,
        transitionProperty: "colors",
        transitionDuration: ms(120),
        transitionTimingFunction: EASE_STANDARD,
      }}
    >
      <View style={{ flex: 1, minWidth: 0, gap: SPACE[2] }}>
        <Text
          style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: FONT_SIZE.body, lineHeight: LINE_HEIGHT.body }}
          class="text-foreground"
        >
          {props.item.name}
        </Text>
        <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[8] }}>
          <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
            {t("snippets.trd.chapterCount", {
              chapters: props.item.chapter_count,
              items: props.item.item_count,
            })}
          </Text>
          <If when={props.item.frozen}>
            <Text style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.caption }} class="text-muted-foreground">
              {t("snippets.trd.finalized")}
            </Text>
          </If>
        </View>
      </View>
      <Lock closed={props.item.frozen} size="sm" />
    </View>
  );
}

function TrdDetailPane(props: {
  id: string;
  allSnippets: readonly SnippetSummaryItem[];
  active: boolean;
  onRequestDelete: (item: TrdSummaryItem) => void;
}) {
  const payload = signal<TrdDetailPayload | undefined>(undefined);
  const rev = activeRevision(() => props.active);

  effect(() => {
    if (!props.active) return;
    void rev();
    void snippetsRevision();
    payload.set(parseJsonPayload<TrdDetailPayload>(preview.loadTrdJson(props.id)));
  });

  const detail = () => payload()?.detail;
  const frozen = () => Boolean(detail()?.meta.frozen);
  const archived = () => Boolean(detail()?.meta.archived);
  const chapters = () => detail()?.meta.chapter ?? [];

  function toggleArchive() {
    const res = preview.setTrdArchived(props.id, !archived());
    if (!res.ok) {
      showToast({ tone: "issues", text: res.error ?? t("snippets.error.actionFailed") });
      return;
    }
    bumpSnippetsRevision();
  }

  const moreEntries = computed<MenuEntry[]>(() => [
    {
      id: "archive",
      label: archived() ? t("snippets.trd.unarchive") : t("snippets.trd.archive"),
      icon: "folder",
      disabled: info().readOnly,
    },
    { id: "reveal", label: t("common.revealInFolder"), icon: "folderOpen" },
    { separator: true, id: "sep-del" },
    {
      id: "delete",
      label: t("common.delete"),
      icon: "trash",
      destructive: true,
      disabled: info().readOnly || frozen() || archived(),
    },
  ]);

  return (
    <View style={{ flex: 1, flexDirection: "column" }}>
      <View style={{ borderBottomWidth: 1 }} class="border-border">
        <DetailHeader
          title={detail()?.meta.name ?? props.id}
          badge={
            frozen() ? (
              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[4] }}>
                <Lock closed size="sm" />
                <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                  {t("snippets.trd.finalized")}
                </Text>
              </View>
            ) : undefined
          }
          second={
            detail() && (detail()!.meta.author || detail()!.meta.date) ? (
              <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[12] }}>
                <If when={!!detail()!.meta.author}>
                  <Text style={{ fontSize: FONT_SIZE.body }} class="text-secondary-foreground">
                    {detail()!.meta.author!}
                  </Text>
                </If>
                <If when={!!detail()!.meta.date}>
                  <Text style={{ fontSize: FONT_SIZE.body, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                    {detail()!.meta.date!}
                  </Text>
                </If>
              </View>
            ) : undefined
          }
          actions={
            <>
              <Btn
                variant="primary"
                icon={frozen() || archived() ? "bookOpen" : "pencil"}
                label={frozen() || archived() ? t("common.view") : t("common.edit")}
                onPress={() => openTrdStudio(props.id)}
              />
              <AppMenu
                align="end"
                entries={moreEntries()}
                onSelect={(id) => {
                  if (id === "archive") toggleArchive();
                  else if (id === "reveal") void preview.revealInFolder(`trds/${props.id}`);
                  else if (id === "delete" && detail()) {
                    props.onRequestDelete({
                      id: props.id,
                      name: detail()!.meta.name,
                      author: detail()!.meta.author,
                      date: detail()!.meta.date,
                      template: detail()!.meta.template,
                      frozen: frozen(),
                      archived: archived(),
                      chapter_count: chapters().length,
                      item_count: chapters().reduce((acc, c) => acc + (c.items ?? []).length, 0),
                    });
                  }
                }}
                trigger={<MoreTrigger />}
              />
            </>
          }
        />
      </View>

      <Scroll style={{ flex: 1 }} contentStyle={{ padding: GUTTER, gap: SPACE[12] }}>
        <Each of={chapters()} key={(c) => c.title}>
          {(ch) => (
            <View
              style={{ borderRadius: RADIUS.lg, borderWidth: 1, padding: SPACE[12], gap: SPACE[8], boxShadow: cardShadow() }}
              class="border-border bg-card"
            >
              <Text style={{ fontSize: FONT_SIZE.strong, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                {ch().title}
              </Text>
              <View style={{ flexDirection: "row", flexWrap: "wrap", gap: SPACE[6] }}>
                <Each of={ch().items ?? []} key={(slug) => slug}>
                  {(slug) => {
                    const meta = () => props.allSnippets.find((s) => s.slug === slug());
                    return (
                      <If
                        when={Boolean(meta())}
                        else={
                          <View
                            style={{
                              height: 22, // token-exempt: compact status badge height
                              paddingLeft: SPACE[8],
                              paddingRight: SPACE[8],
                              borderRadius: RADIUS.xs,
                              flexDirection: "row",
                              alignItems: "center",
                              gap: SPACE[4],
                              backgroundColor: "rgba(244,63,94,0.12)",
                            }}
                          >
                            <Icon name="circleAlert" size="sm" tone="destructive" />
                            <Text style={{ fontSize: FONT_SIZE.caption }} class="text-destructive">
                              {slug()}
                            </Text>
                          </View>
                        }
                      >
                        <Chip label={meta()!.title} />
                      </If>
                    );
                  }}
                </Each>
              </View>
            </View>
          )}
        </Each>
      </Scroll>
    </View>
  );
}
