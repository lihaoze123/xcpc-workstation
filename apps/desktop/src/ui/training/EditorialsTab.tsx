// Training space View 4: Editorials list & live Typst editorial preview.
// Split layout matching LibraryView: left searchable/filterable list of non-empty editorials,
// right DetailHeader with metadata pills + primary Edit button + live PreviewCanvas (book="editorial").

import { computed, effect, Scroll, signal, Text, View } from "nsh";
import { Input, ResizablePanel, ResizablePanelGroup } from "nsh/base";
import type { PreviewRequest } from "xcpc-workstation/native";
import { t } from "../i18n";
import { Icon } from "../Icon";
import { Btn, Chip, IconBtn, RatingBadge, tip } from "../kit";
import { DetailHeader, EmptyState, FilterEmpty, SectionEmpty } from "../layout";
import { AppMenu, type MenuEntry } from "../Menu";
import { colorTransition } from "../motion";
import { info, openStudio } from "../store";
import { PreviewCanvas } from "../studio/PreviewCanvas";
import { isEn, zhEn } from "./locale";
import {
  DEFAULT_EDITORIAL_FILTER,
  formatIsoDateShort,
  formatTagLabel,
  platformDisplayName,
  type EditorialFilter,
  type EditorialSummaryItem,
} from "./types";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "../tokens";
interface DifficultyPreset {
  id: string;
  labelZh: string;
  labelEn: string;
  minRating: number | null;
  maxRating: number | null;
}

const DIFFICULTY_PRESETS: readonly DifficultyPreset[] = [
  { id: "all", labelZh: "全部难度", labelEn: "All Diff", minRating: null, maxRating: null },
  { id: "lt1200", labelZh: "<1200", labelEn: "<1200", minRating: null, maxRating: 1199 },
  { id: "1200", labelZh: "1200–1599", labelEn: "1200–1599", minRating: 1200, maxRating: 1599 },
  { id: "1600", labelZh: "1600–1999", labelEn: "1600–1999", minRating: 1600, maxRating: 1999 },
  { id: "2000", labelZh: "2000–2399", labelEn: "2000–2399", minRating: 2000, maxRating: 2399 },
  { id: "2400", labelZh: "2400+", labelEn: "2400+", minRating: 2400, maxRating: null },
];

export interface EditorialsTabProps {
  active: boolean;
  items: readonly EditorialSummaryItem[];
  total: number;
  availableTags: readonly string[];
  filter: EditorialFilter;
  onFilterChange: (next: EditorialFilter) => void;
  onOpenProblem?: (id: string, opts?: { initialTab?: "statement" | "editorial" }) => void;
  onOpenManualLog?: () => void;
}

export function EditorialsTab(props: EditorialsTabProps) {
  const selectedId = signal<string | undefined>(undefined);

  // Keep a valid selection whenever items change.
  effect(() => {
    const list = props.items;
    const cur = selectedId();
    if (list.length === 0) {
      if (cur !== undefined) selectedId.set(undefined);
      return;
    }
    if (!cur || !list.some((it) => it.problemId === cur)) {
      selectedId.set(list[0].problemId);
    }
  });

  const selectedItem = computed(() => {
    const id = selectedId();
    if (!id) return undefined;
    return props.items.find((it) => it.problemId === id);
  });

  const patch = (partial: Partial<EditorialFilter>) => {
    props.onFilterChange({ ...props.filter, ...partial });
  };

  const hasActiveFilter = () =>
    props.filter.search.trim() !== "" ||
    props.filter.tag !== "" ||
    props.filter.minRating !== null ||
    props.filter.maxRating !== null;

  const openEditorialEditor = (problemId: string) => {
    if (props.onOpenProblem) {
      props.onOpenProblem(problemId, { initialTab: "editorial" });
    } else {
      openStudio({
        target: "problem",
        id: problemId,
        initialTab: "editorial",
      });
    }
  };

  const tagMenuEntries = computed<MenuEntry[]>(() => [
    {
      id: "__all__",
      label: t("training.editorials.allTags"),
      checked: props.filter.tag === "",
    },
    ...props.availableTags.map((tg) => ({
      id: tg,
      label: formatTagLabel(tg, isEn()),
      checked: props.filter.tag === tg,
    })),
  ]);

  const difficultyMenuEntries = computed<MenuEntry[]>(() =>
    DIFFICULTY_PRESETS.map((p) => ({
      id: p.id,
      label: p.id === "all" ? zhEn("全部", "All") : zhEn(p.labelZh, p.labelEn),
      checked: props.filter.minRating === p.minRating && props.filter.maxRating === p.maxRating,
    })),
  );

  const hasDifficultyFilter = () =>
    props.filter.minRating !== null || props.filter.maxRating !== null;

  const difficultyChipLabel = () => {
    const preset = DIFFICULTY_PRESETS.find(
      (p) => p.minRating === props.filter.minRating && p.maxRating === props.filter.maxRating,
    );
    if (preset && preset.id !== "all") {
      return zhEn(`难度：${preset.labelZh}`, `Diff: ${preset.labelEn}`);
    }
    return zhEn("难度：全部", "Diff: All");
  };

  const tagChipLabel = () => {
    if (!props.filter.tag) return t("training.editorials.tagFilterAll");
    return `${t("training.editorials.tagPrefix")}${formatTagLabel(props.filter.tag, isEn())}`;
  };

  const previewRequest = (): PreviewRequest | undefined => {
    const item = selectedItem();
    if (!item) return undefined;
    return {
      target: "problem",
      id: item.problemId,
      book: "editorial",
      path: "",
      text: "",
    };
  };

  const readOnly = () => info().readOnly;

  return (
    <View
      testID="training-editorials-root"
      style={{ flex: 1, height: "100%", minHeight: 0, overflow: "hidden", flexDirection: "column" }}
    >
      <If
        when={props.total > 0 || hasActiveFilter()}
        else={
          <View
            testID="training-editorials-empty"
            style={{ flex: 1, alignItems: "center", justifyContent: "center", padding: SPACE[32] }}
          >
            <EmptyState
              icon="fileText"
              title={t("training.editorials.emptyTitle")}
              hint={t("training.editorials.emptyDesc")}
              action={
                props.onOpenManualLog ? (
                  <Btn
                    testID="editorials-empty-manual-log-btn"
                    label={t("training.editorials.emptyAction")}
                    icon="filePenLine"
                    variant="primary"
                    disabled={readOnly()}
                    onPress={props.onOpenManualLog}
                  />
                ) : undefined
              }
            />
          </View>
        }
      >
        <ResizablePanelGroup handleClass="bg-border w-px group-hover:bg-primary/50 group-active:bg-primary transition-colors">
          {/* Left pane: search, filters, scrollable editorial list */}
          <ResizablePanel minSize={280}>
            <View
              testID="training-editorials-list-pane"
              style={{ flex: 1, height: "100%", minHeight: 0, borderRightWidth: 1, flexDirection: "column" }}
              class="border-border bg-background"
            >
              {/* Search + filter toolbar */}
              <View
                class="border-b border-border bg-card/50"
                style={{
                  paddingTop: SPACE[12],
                  paddingBottom: SPACE[12],
                  paddingLeft: SPACE[12],
                  paddingRight: SPACE[12],
                  gap: SPACE[8],
                  flexShrink: 0,
                }}
              >
                <View
                  class="bg-card border border-border focus-within:border-input"
                  style={{
                    height: CONTROL.md,
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
                    testID="editorials-search"
                    class="bg-transparent placeholder:text-muted-foreground"
                    style={{ flex: 1, height: CONTROL.sm, fontSize: FONT_SIZE.caption }}
                    value={props.filter.search}
                    placeholder={t("training.editorials.searchPlaceholder")}
                    autoCapitalize="none"
                    autoCorrect={false}
                    onChange={(e) => patch({ search: e.value })}
                  />
                  <If when={props.filter.search.length > 0}>
                    <IconBtn
                      icon="x"
                      size="sm"
                      tone="muted-foreground"
                      onPress={() => patch({ search: "" })}
                    />
                  </If>
                </View>

                <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: SPACE[6] }}>
                  <AppMenu
                    entries={tagMenuEntries()}
                    onSelect={(id) => patch({ tag: id === "__all__" ? "" : id })}
                    minWidth={156}
                    trigger={
                      <View
                        testID="editorials-tag-menu"
                        class={
                          props.filter.tag !== ""
                            ? "bg-accent border border-foreground/40 active:opacity-90"
                            : "bg-transparent border border-border/70 hover:bg-muted/80 active:bg-accent"
                        }
                        style={{
                          height: CONTROL.sm,
                          paddingLeft: SPACE[8],
                          paddingRight: SPACE[6],
                          borderRadius: RADIUS.sm,
                          flexDirection: "row",
                          alignItems: "center",
                          gap: SPACE[4],
                          ...colorTransition(120),
                        }}
                      >
                        <Text
                          style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }}
                          class={props.filter.tag !== "" ? "text-foreground" : "text-muted-foreground"}
                        >
                          {tagChipLabel()}
                        </Text>
                        <Icon
                          name="chevronDown"
                          size="sm"
                          tone={props.filter.tag !== "" ? "foreground" : "muted-foreground"}
                        />
                      </View>
                    }
                  />

                  <AppMenu
                    entries={difficultyMenuEntries()}
                    onSelect={(id) => {
                      const preset = DIFFICULTY_PRESETS.find((p) => p.id === id);
                      if (preset) patch({ minRating: preset.minRating, maxRating: preset.maxRating });
                    }}
                    minWidth={148}
                    trigger={
                      <View
                        testID="editorials-diff-menu"
                        class={
                          hasDifficultyFilter()
                            ? "bg-accent border border-foreground/40 active:opacity-90"
                            : "bg-transparent border border-border/70 hover:bg-muted/80 active:bg-accent"
                        }
                        style={{
                          height: CONTROL.sm,
                          paddingLeft: SPACE[8],
                          paddingRight: SPACE[6],
                          borderRadius: RADIUS.sm,
                          flexDirection: "row",
                          alignItems: "center",
                          gap: SPACE[4],
                          ...colorTransition(120),
                        }}
                      >
                        <Text
                          style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }}
                          class={hasDifficultyFilter() ? "text-foreground" : "text-muted-foreground"}
                        >
                          {difficultyChipLabel()}
                        </Text>
                        <Icon
                          name="chevronDown"
                          size="sm"
                          tone={hasDifficultyFilter() ? "foreground" : "muted-foreground"}
                        />
                      </View>
                    }
                  />

                  <View style={{ flex: 1 }} />

                  <If when={hasActiveFilter()}>
                    <Btn
                      testID="editorials-clear-filters"
                      label={t("training.submissions.clearFilters")}
                      icon="filterX"
                      variant="ghost"
                      size="sm"
                      onPress={() => props.onFilterChange({ ...DEFAULT_EDITORIAL_FILTER })}
                    />
                  </If>
                </View>
              </View>

              {/* Editorial list */}
              <Scroll
                testID="training-editorials-scroll"
                style={{ flex: 1, minHeight: 0 }}
                contentStyle={{ padding: SPACE[12], gap: SPACE[6], paddingBottom: SPACE[24] }}
              >
                <If
                  when={props.items.length > 0}
                  else={
                    <View testID="training-editorials-no-match" style={{ flex: 1 }}>
                      <FilterEmpty
                        message={t("training.editorials.noMatchTitle")}
                        clearLabel={t("training.submissions.clearFilters")}
                        onClear={() => props.onFilterChange({ ...DEFAULT_EDITORIAL_FILTER })}
                      />
                    </View>
                  }
                >
                  <Each of={props.items} key={(it) => it.problemId}>
                    {(item) => {
                      const isSelected = () => selectedId() === item().problemId;
                      const primarySource = () => item().sources[0];
                      return (
                        <View
                          testID={`editorial-item-${item().problemId}`}
                          onPress={() => selectedId.set(item().problemId)}
                          class={
                            isSelected()
                              ? "bg-accent border border-foreground/30"
                              : "bg-card border border-border hover:bg-muted/60"
                          }
                          style={{
                            borderRadius: RADIUS.md,
                            paddingTop: SPACE[12],
                            paddingBottom: SPACE[12],
                            paddingLeft: SPACE[12],
                            paddingRight: SPACE[12],
                            gap: SPACE[4],
                            ...colorTransition(100),
                          }}
                        >
                          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[8] }}>
                            <Text
                              style={{
                                fontSize: FONT_SIZE.body,
                                fontWeight: FONT_WEIGHT.semibold,
                                flex: 1,
                                minWidth: 0,
                                numberOfLines: 1,
                                textOverflow: "ellipsis",
                              }}
                              class="text-foreground"
                            >
                              {item().title || item().problemId}
                            </Text>
                            <If when={item().rating != null}>
                              <RatingBadge rating={item().rating} />
                            </If>
                          </View>

                          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: SPACE[8] }}>
                            <Text
                              style={{
                                fontSize: FONT_SIZE.caption,
                                fontVariant: ["tabular-nums"],
                                flex: 1,
                                minWidth: 0,
                                numberOfLines: 1,
                                textOverflow: "ellipsis",
                              }}
                              class="text-muted-foreground"
                            >
                              {primarySource()
                                ? `${platformDisplayName(primarySource()!.platform, isEn())} ${primarySource()!.id}`
                                : item().problemId}
                            </Text>
                            <Text
                              style={{ fontSize: FONT_SIZE.micro, fontVariant: ["tabular-nums"], flexShrink: 0 }}
                              class="text-muted-foreground"
                            >
                              {formatIsoDateShort(item().updatedAt)}
                            </Text>
                          </View>

                          <If when={item().excerpt.length > 0}>
                            <Text
                              style={{
                                fontSize: FONT_SIZE.caption,
                                lineHeight: LINE_HEIGHT.caption,
                                numberOfLines: 2,
                                textOverflow: "ellipsis",
                              }}
                              class="text-secondary-foreground"
                            >
                              {item().excerpt}
                            </Text>
                          </If>

                          <If when={item().tags.length > 0}>
                            <View style={{ flexDirection: "row", flexWrap: "wrap", gap: SPACE[4], paddingTop: SPACE[2] }}>
                              <Each of={item().tags.slice(0, 4)} key={(tg) => tg}>
                                {(tg) => <Chip label={formatTagLabel(tg(), isEn())} />}
                              </Each>
                            </View>
                          </If>
                        </View>
                      );
                    }}
                  </Each>
                </If>
              </Scroll>
            </View>
          </ResizablePanel>

          {/* Right pane: header + primary Edit button + live Typst editorial preview */}
          <ResizablePanel minSize={320}>
            <View
              testID="training-editorials-preview-pane"
              style={{ flex: 1, height: "100%", minHeight: 0, flexDirection: "column" }}
              class="bg-card"
            >
              <If
                when={selectedItem()}
                else={
                  <SectionEmpty message={t("training.editorials.selectHint")} style={{ flex: 1 }} />
                }
              >
                {(cur) => (
                  <>
                    <View style={{ borderBottomWidth: 1 }} class="border-border">
                      <DetailHeader
                        title={cur().title || cur().problemId}
                        badge={cur().rating != null ? <RatingBadge rating={cur().rating} /> : undefined}
                        second={
                          cur().tags.length > 0 || cur().sources.length > 0 ? (
                            <>
                              <Each of={cur().tags} key={(tg) => tg}>
                                {(tg) => <Chip label={formatTagLabel(tg(), isEn())} />}
                              </Each>
                              <Each of={cur().sources} key={(s) => `${s.platform}:${s.id}`}>
                                {(s) => (
                                  <View
                                    style={{
                                      height: 20, // token-exempt: compact tag chip height
                                      maxWidth: 180,
                                      minWidth: 0,
                                      overflow: "hidden",
                                      paddingLeft: SPACE[6],
                                      paddingRight: SPACE[6],
                                      borderRadius: RADIUS.xs,
                                      flexDirection: "row",
                                      alignItems: "center",
                                      gap: SPACE[4],
                                    }}
                                    class="bg-muted"
                                  >
                                    <Icon name="link" size="sm" tone="muted-foreground" />
                                    <Text
                                      style={{
                                        fontSize: FONT_SIZE.caption,
                                        fontVariant: ["tabular-nums"],
                                        flexShrink: 1,
                                        minWidth: 0,
                                        numberOfLines: 1,
                                        textOverflow: "ellipsis",
                                      }}
                                      class="text-secondary-foreground"
                                    >
                                      {`${platformDisplayName(s().platform, isEn())} ${s().id}`}
                                    </Text>
                                  </View>
                                )}
                              </Each>
                            </>
                          ) : undefined
                        }
                        actions={
                          <Btn
                            testID="editorial-edit-btn"
                            variant="primary"
                            icon={readOnly() ? "fileText" : "pencil"}
                            label={readOnly() ? t("common.view") : t("common.edit")}
                            tooltip={tip(t("training.editorials.editTooltip"))}
                            onPress={() => openEditorialEditor(cur().problemId)}
                          />
                        }
                      />
                    </View>
                    <PreviewCanvas request={previewRequest} controller={() => {}} hud={false} active={props.active} />
                  </>
                )}
              </If>
            </View>
          </ResizablePanel>
        </ResizablePanelGroup>
      </If>
    </View>
  );
}
