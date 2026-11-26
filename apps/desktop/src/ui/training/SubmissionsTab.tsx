// Training space View 3: Submissions Search & One-Click Ingest.
// 3-way mode Segmented (Raw | By Problem | Unsolved Only), high-contrast filter chips
// (platform when >1, core verdicts + expandable other errors, difficulty buckets, drilldown pills),
// subtle hover ingest action & muted "已在题库" + "打开" landing on Editorial tab.

import { computed, Scroll, signal, Text, View } from "nsh";
import { Input } from "nsh/base";
import { Preview } from "xcpc-workstation/native";
import { t } from "../i18n";
import { Icon } from "../Icon";
import { Btn, IconBtn, RatingBadge, Segmented, tip } from "../kit";
import { AppMenu, type MenuEntry } from "../Menu";
import { colorTransition } from "../motion";
import { info, openStudio, preview, refresh } from "../store";
import { cardShadow, MONO } from "../theme";
import { showToast } from "../Toast";
import { isEn, zhEn } from "./locale";
import {
  DEFAULT_SUBMISSIONS_FILTER,
  formatTagLabel,
  formatUnixDateTime,
  hasActiveSubmissionsFilter,
  platformDisplayName,
  verdictDisplayLabel,
  verdictTone,
  type SubmissionsFilter,
  type SubmissionsPage,
  type SubmissionViewRow,
} from "./types";

import { CONTROL, FONT_SIZE, FONT_WEIGHT, RADIUS, SPACE } from "../tokens";
type SubmissionMode = "raw" | "byProblem" | "unsolved";

const CORE_VERDICTS = ["", "AC", "WA", "TLE"] as const;
const OTHER_VERDICTS = ["MLE", "RE", "CE"] as const;

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

/** High-contrast filter pill: unmistakable selected vs unselected states without using brand fill. */
function FilterChip(props: {
  label: string;
  active: boolean;
  onPress: () => void;
  testID?: string;
}) {
  return (
    <View
      testID={props.testID}
      onPress={props.onPress}
      class={
        props.active
          ? "bg-accent border border-foreground/40 active:opacity-90"
          : "bg-transparent border border-border/70 hover:bg-muted/80 active:bg-accent"
      }
      style={{
        height: CONTROL.sm,
        paddingLeft: SPACE[8],
        paddingRight: SPACE[8],
        borderRadius: RADIUS.sm,
        flexDirection: "row",
        alignItems: "center",
        justifyContent: "center",
        ...colorTransition(120),
      }}
    >
      <Text
        style={{
          fontSize: FONT_SIZE.caption,
          fontWeight: FONT_WEIGHT.semibold,
          fontVariant: ["tabular-nums"],
        }}
        class={props.active ? "text-foreground" : "text-muted-foreground"}
      >
        {props.label}
      </Text>
    </View>
  );
}

export function SubmissionsTab(props: {
  page: SubmissionsPage;
  platforms: readonly string[];
  filter: SubmissionsFilter;
  onFilterChange: (next: SubmissionsFilter) => void;
  onPageChange: (offset: number) => void;
  onReload: () => void;
  onOpenProblem?: (id: string, opts?: { initialTab?: "statement" | "editorial" }) => void;
}) {
  const ingestingKey = signal<string | undefined>(undefined);
  const hoveredRowKey = signal<string | undefined>(undefined);
  const showOtherVerdicts = signal<boolean>(false);

  const patch = (partial: Partial<SubmissionsFilter>) => {
    props.onFilterChange({ ...props.filter, ...partial });
  };

  const currentMode = (): SubmissionMode => {
    if (props.filter.unsolvedOnly) return "unsolved";
    if (props.filter.byProblem) return "byProblem";
    return "raw";
  };

  const setMode = (mode: SubmissionMode) => {
    if (mode === "raw") {
      patch({ byProblem: false, unsolvedOnly: false });
    } else if (mode === "byProblem") {
      patch({ byProblem: true, unsolvedOnly: false });
    } else {
      patch({
        byProblem: true,
        unsolvedOnly: true,
        verdict: props.filter.verdict === "AC" ? "" : props.filter.verdict,
      });
    }
  };

  const clearFilters = () => {
    props.onFilterChange({
      ...DEFAULT_SUBMISSIONS_FILTER,
      byProblem: props.filter.byProblem,
    });
  };

  const openProblemEditorial = (problemId: string) => {
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

  const ingestRow = (row: SubmissionViewRow) => {
    const key = `${row.platform}:${row.problem_key}`;
    if (ingestingKey() === key) return;
    ingestingKey.set(key);
    try {
      const isLocal = row.platform.trim().toLowerCase() === "local";
      const res = preview.trainingIngestProblem({
        url: row.problem_url ?? null,
        platform: isLocal ? null : row.platform,
        problemKey: isLocal ? null : row.problem_key,
        title: row.problem_title ?? (isLocal ? row.problem_key : null),
        rating: row.normalized_rating ?? row.raw_rating ?? null,
        tags: row.tags.length > 0 ? row.tags : null,
      });
      if (!res.ok) {
        showToast({ tone: "issues", text: res.error || t("training.submissions.ingestFailed") });
        return;
      }
      refresh();
      props.onReload();
      const pid = res.problemId ?? "";
      const displayTitle = res.title || pid;
      showToast({
        tone: "success",
        text: res.created
          ? t("training.submissions.ingestedToast", { title: displayTitle })
          : t("training.submissions.alreadyInLibraryToast", { title: displayTitle }),
        action: pid
          ? {
              label: t("training.submissions.openEditorial"),
              run: () => openProblemEditorial(pid),
            }
          : undefined,
        durationMs: 4500,
      });
    } finally {
      ingestingKey.set(undefined);
    }
  };

  const openExternal = (url: string | null | undefined) => {
    if (!url) return;
    const res = Preview.openExternalUrl(url);
    if (!res.ok && res.error) showToast({ tone: "issues", text: res.error });
  };

  const verdictBadgeClass = (verdict: string) => {
    const tone = verdictTone(verdict);
    if (tone === "ac") return "bg-chart-2/15 text-chart-2";
    if (tone === "wa") return "bg-destructive/12 text-destructive";
    if (tone === "tle" || tone === "attempted") return "bg-chart-4/15 text-chart-4";
    return "bg-muted text-secondary-foreground";
  };

  const modeItems = computed(() => [
    { value: "raw" as const, label: t("training.submissions.modeRaw"), testID: "submissions-mode-raw" },
    { value: "byProblem" as const, label: t("training.submissions.modeByProblem"), testID: "submissions-mode-by-problem" },
    { value: "unsolved" as const, label: t("training.submissions.unsolvedOnly"), testID: "submissions-mode-unsolved" },
  ]);

  const effectivePlatforms = computed(() => {
    const set = new Set<string>(props.platforms);
    for (const r of props.page.items) {
      if (r.platform) set.add(r.platform);
    }
    if (props.filter.platform) set.add(props.filter.platform);
    return [...set].sort();
  });

  const showPlatformCol = computed(() => effectivePlatforms().length > 1);

  const visibleCoreVerdicts = computed(() =>
    props.filter.unsolvedOnly ? CORE_VERDICTS.filter((v) => v !== "AC") : CORE_VERDICTS,
  );

  const activeDifficultyPreset = () => {
    const f = props.filter;
    return DIFFICULTY_PRESETS.find((p) => p.minRating === f.minRating && p.maxRating === f.maxRating);
  };

  const hasDifficultyFilter = () =>
    props.filter.minRating !== null || props.filter.maxRating !== null;

  const difficultyChipLabel = () => {
    const preset = activeDifficultyPreset();
    if (preset && preset.id !== "all") {
      return zhEn(`难度：${preset.labelZh}`, `Diff: ${preset.labelEn}`);
    }
    if (hasDifficultyFilter()) {
      const { minRating, maxRating } = props.filter;
      const range =
        minRating !== null && maxRating !== null
          ? `${minRating}–${maxRating}`
          : minRating !== null
            ? `≥${minRating}`
            : `<${maxRating}`;
      return zhEn(`难度：${range}`, `Diff: ${range}`);
    }
    return zhEn("难度：全部", "Diff: All");
  };

  const difficultyMenuEntries = computed<MenuEntry[]>(() =>
    DIFFICULTY_PRESETS.map((p) => ({
      id: p.id,
      label: p.id === "all" ? zhEn("全部", "All") : zhEn(p.labelZh, p.labelEn),
      checked: props.filter.minRating === p.minRating && props.filter.maxRating === p.maxRating,
    })),
  );

  const selectDifficultyPreset = (id: string) => {
    const preset = DIFFICULTY_PRESETS.find((p) => p.id === id);
    if (!preset) return;
    patch({ minRating: preset.minRating, maxRating: preset.maxRating });
  };

  const otherVerdictActive = () =>
    (OTHER_VERDICTS as readonly string[]).includes(props.filter.verdict);

  const activeFilterCount = computed(() => hasActiveSubmissionsFilter(props.filter));
  const rangeStart = () => (props.page.total === 0 ? 0 : props.page.offset + 1);
  const rangeEnd = () => Math.min(props.page.total, props.page.offset + props.page.items.length);
  const hasPrev = () => props.page.offset > 0;
  const hasNext = () => props.page.offset + props.page.limit < props.page.total;

  return (
    <View
      testID="training-submissions-root"
      style={{ flex: 1, height: "100%", minHeight: 0, overflow: "hidden", flexDirection: "column" }}
    >
      {/* Filter toolbar */}
      <View
        class="border-b border-border bg-card/50"
        style={{
          paddingTop: SPACE[12],
          paddingBottom: SPACE[12],
          paddingLeft: SPACE[20],
          paddingRight: SPACE[20],
          gap: SPACE[8],
          flexShrink: 0,
        }}
      >
        {/* Row 1: Search + 3-way mode Segmented (Raw | By Problem | Unsolved Only) + clear */}
        <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: SPACE[8] }}>
          <View
            class="bg-card border border-border focus-within:border-input"
            style={{
              width: 240,
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
              testID="submissions-search"
              class="bg-transparent placeholder:text-muted-foreground"
              style={{ flex: 1, height: CONTROL.sm, fontSize: FONT_SIZE.caption }}
              value={props.filter.search}
              placeholder={t("training.submissions.searchPlaceholder")}
              autoCapitalize="none"
              autoCorrect={false}
              onChange={(e) => patch({ search: e.value })}
            />
            <If when={props.filter.search.length > 0}>
              <IconBtn icon="x" size="sm" tone="muted-foreground" onPress={() => patch({ search: "" })} />
            </If>
          </View>

          <Segmented
            items={modeItems()}
            value={currentMode()}
            onChange={setMode}
            itemWidth={86}
          />

          <View style={{ flex: 1 }} />

          <If when={activeFilterCount()}>
            <Btn
              testID="submissions-clear-filters"
              label={t("training.submissions.clearFilters")}
              icon="filterX"
              variant="ghost"
              onPress={clearFilters}
            />
          </If>
        </View>

        {/* Row 2: Platform chips (only when >1) + Core Verdict chips + Difficulty filter + active drilldown pills */}
        <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: SPACE[6] }}>
          {/* Platforms (hidden when only 1 platform) */}
          <If when={effectivePlatforms().length > 1}>
            <FilterChip
              testID="submissions-platform-all"
              label={t("training.submissions.allPlatforms")}
              active={props.filter.platform === ""}
              onPress={() => patch({ platform: "" })}
            />
            <Each of={effectivePlatforms()} key={(p) => p}>
              {(plat) => (
                <FilterChip
                  testID={`submissions-platform-${plat()}`}
                  label={platformDisplayName(plat(), isEn())}
                  active={props.filter.platform === plat()}
                  onPress={() => patch({ platform: props.filter.platform === plat() ? "" : plat() })}
                />
              )}
            </Each>
            <View style={{ width: 8 }} />
          </If>

          {/* Core Verdicts (hides AC in unsolved-only mode) */}
          <Each of={visibleCoreVerdicts()} key={(v) => v || "all"}>
            {(v) => (
              <FilterChip
                testID={`submissions-verdict-${v() || "all"}`}
                label={v() === "" ? t("training.submissions.allVerdicts") : v()}
                active={props.filter.verdict === v()}
                onPress={() => patch({ verdict: v() })}
              />
            )}
          </Each>

          {/* Expandable Other Errors (MLE / RE / CE) */}
          <If
            when={showOtherVerdicts() || otherVerdictActive()}
            else={
              <FilterChip
                testID="submissions-verdict-other-toggle"
                label={zhEn("其他错误", "Other")}
                active={false}
                onPress={() => showOtherVerdicts.set(true)}
              />
            }
          >
            <Each of={OTHER_VERDICTS} key={(v) => v}>
              {(v) => (
                <FilterChip
                  testID={`submissions-verdict-${v()}`}
                  label={v()}
                  active={props.filter.verdict === v()}
                  onPress={() => patch({ verdict: props.filter.verdict === v() ? "" : v() })}
                />
              )}
            </Each>
          </If>

          <View style={{ width: 8 }} />

          {/* Compact difficulty dropdown chip (keeps all filters on one row at 960px) */}
          <AppMenu
            entries={difficultyMenuEntries()}
            onSelect={selectDifficultyPreset}
            minWidth={148}
            trigger={
              <View
                testID="submissions-diff-menu"
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
                  style={{
                    fontSize: FONT_SIZE.caption,
                    fontWeight: FONT_WEIGHT.semibold,
                    fontVariant: ["tabular-nums"],
                  }}
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

          <If when={props.filter.tag !== ""}>
            <View
              testID="submissions-active-tag-chip"
              onPress={() => patch({ tag: "" })}
              class="bg-accent border border-foreground/40 active:opacity-90"
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
                style={{
                  fontSize: FONT_SIZE.caption,
                  fontWeight: FONT_WEIGHT.semibold,
                  fontVariant: ["tabular-nums"],
                }}
                class="text-foreground"
              >
                {`#${formatTagLabel(props.filter.tag, isEn())}`}
              </Text>
              <Icon name="x" size="sm" tone="foreground" />
            </View>
          </If>
        </View>
      </View>

      {/* Submissions list */}
      <Scroll
        testID="training-submissions-scroll"
        style={{ flex: 1, minHeight: 0 }}
        contentStyle={{ padding: SPACE[20], gap: SPACE[12], paddingBottom: SPACE[32] }}
      >
        <If
          when={props.page.items.length > 0}
          else={
            <View
              testID="training-submissions-empty"
              class="bg-card border border-border"
              style={{
                borderRadius: RADIUS.lg,
                padding: SPACE[32],
                alignItems: "center",
                justifyContent: "center",
                gap: SPACE[6],
                boxShadow: cardShadow(),
              }}
            >
              <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold }} class="text-foreground">
                {t("training.submissions.emptyTitle")}
              </Text>
              <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                {t("training.submissions.emptyDesc")}
              </Text>
            </View>
          }
        >
          <View
            testID="training-submissions-table"
            class="bg-card border border-border"
            style={{ borderRadius: RADIUS.lg, overflow: "hidden", boxShadow: cardShadow() }}
          >
            <Each of={props.page.items} key={(r) => `${r.platform}:${r.submission_id}:${r.problem_key}`}>
              {(row, idx) => {
                const key = () => `${row().platform}:${row().submission_id}:${row().problem_key}`;
                const probKey = () => `${row().platform}:${row().problem_key}`;
                const libId = () => row().library_problem_id;
                const readOnly = () => info().readOnly;
                const isHovered = () => hoveredRowKey() === key();
                return (
                  <View>
                    <If when={idx() > 0}>
                      <View style={{ height: 1, marginLeft: SPACE[12], marginRight: SPACE[12] }} class="bg-border" />
                    </If>
                    <View
                      testID={`submission-row-${row().platform}-${row().problem_key}`}
                      onMouseEnter={() => hoveredRowKey.set(key())}
                      onMouseLeave={() => {
                        if (hoveredRowKey() === key()) hoveredRowKey.set(undefined);
                      }}
                      class="hover:bg-muted/40"
                      style={{
                        minHeight: 42,
                        paddingTop: SPACE[8],
                        paddingBottom: SPACE[8],
                        paddingLeft: SPACE[12],
                        paddingRight: SPACE[12],
                        flexDirection: "row",
                        alignItems: "center",
                        gap: SPACE[8],
                        ...colorTransition(100),
                      }}
                    >
                      {/* Platform (hidden when only 1 platform exists) */}
                      <If when={showPlatformCol()}>
                        <View style={{ width: 76, flexShrink: 0 }}>
                          <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                            {platformDisplayName(row().platform, isEn())}
                          </Text>
                        </View>
                      </If>

                      {/* Problem key + title */}
                      <View style={{ flex: 1, minWidth: 0, gap: SPACE[2] }}>
                        <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6], minWidth: 0 }}>
                          <Text
                            style={{ fontSize: FONT_SIZE.caption, fontFamily: MONO, fontWeight: FONT_WEIGHT.semibold, flexShrink: 0 }}
                            class="text-secondary-foreground"
                          >
                            {row().problem_key}
                          </Text>
                          <Text
                            onPress={row().problem_url ? () => openExternal(row().problem_url) : undefined}
                            style={{
                              fontSize: FONT_SIZE.body,
                              fontWeight: FONT_WEIGHT.semibold,
                              flexShrink: 1,
                              minWidth: 0,
                              numberOfLines: 1,
                              textOverflow: "ellipsis",
                            }}
                            class={row().problem_url ? "text-foreground hover:text-primary" : "text-foreground"}
                          >
                            {row().problem_title || row().problem_key}
                          </Text>
                        </View>
                        <If when={row().tags.length > 0}>
                          <Text
                            style={{ fontSize: FONT_SIZE.micro, numberOfLines: 1, textOverflow: "ellipsis" }}
                            class="text-muted-foreground"
                          >
                            {row().tags.slice(0, 4).map((tag) => formatTagLabel(tag, isEn())).join("  ")}
                          </Text>
                        </If>
                      </View>

                      {/* Difficulty */}
                      <View style={{ width: 46, alignItems: "center", flexShrink: 0 }}>
                        <RatingBadge rating={row().normalized_rating ?? row().raw_rating} />
                      </View>

                      {/* Verdict + fixed-width attempt-count slot so verdict column stays 100% aligned */}
                      <View
                        tooltip={row().language ? tip(row().language!) : undefined}
                        style={{ width: 92, flexDirection: "row", alignItems: "center", gap: SPACE[4], flexShrink: 0 }}
                      >
                        <View style={{ flex: 1, flexDirection: "row", justifyContent: "flex-end" }}>
                          <View
                            class={verdictBadgeClass(row().verdict)}
                            style={{ height: 20, paddingLeft: SPACE[6], paddingRight: SPACE[6], borderRadius: RADIUS.xs, justifyContent: "center" }} // token-exempt: compact verdict badge height
                          >
                            <Text style={{ fontSize: FONT_SIZE.micro, fontWeight: FONT_WEIGHT.semibold, fontVariant: ["tabular-nums"] }}>
                              {verdictDisplayLabel(row().verdict, isEn())}
                            </Text>
                          </View>
                        </View>
                        <View style={{ width: 28, alignItems: "flex-start", flexShrink: 0 }}>
                          <If when={props.filter.byProblem && row().attempt_count > 1}>
                            <Text style={{ fontSize: FONT_SIZE.micro, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                              {`×${row().attempt_count}`}
                            </Text>
                          </If>
                        </View>
                      </View>

                      {/* Submitted time */}
                      <View style={{ width: 116, flexShrink: 0 }}>
                        <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                          {formatUnixDateTime(row().submitted_at)}
                        </Text>
                      </View>

                      {/* Subtle ingest / already-in-library action slot (no outlined button wall) */}
                      <View style={{ width: 96, flexDirection: "row", alignItems: "center", justifyContent: "flex-end", gap: SPACE[6], flexShrink: 0 }}>
                        <If
                          when={libId()}
                          else={
                            <IconBtn
                              testID={`submission-ingest-${row().problem_key}`}
                              icon="plus"
                              size="sm"
                              tone={isHovered() ? "foreground" : "muted-foreground"}
                              disabled={readOnly() || ingestingKey() === probKey()}
                              tooltip={tip(t("training.submissions.ingest"))}
                              style={{ opacity: isHovered() ? 1 : 0.45 }}
                              onPress={() => ingestRow(row())}
                            />
                          }
                        >
                          {(pid) => (
                            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
                              <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                                {zhEn("已在题库", "In library")}
                              </Text>
                              <Text
                                testID={`submission-open-${row().problem_key}`}
                                onPress={() => openProblemEditorial(pid())}
                                style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold }}
                                class="text-primary hover:opacity-80"
                              >
                                {t("training.submissions.open")}
                              </Text>
                            </View>
                          )}
                        </If>
                      </View>
                    </View>
                  </View>
                );
              }}
            </Each>
          </View>

          {/* Pagination bar */}
          <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
            <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
              {t("training.submissions.pageSummary", {
                start: rangeStart(),
                end: rangeEnd(),
                total: props.page.total,
              })}
            </Text>
            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6] }}>
              <Btn
                testID="submissions-prev-page"
                label={t("training.submissions.prevPage")}
                variant="outline"
                disabled={!hasPrev()}
                onPress={() => props.onPageChange(Math.max(0, props.page.offset - props.page.limit))}
              />
              <Btn
                testID="submissions-next-page"
                label={t("training.submissions.nextPage")}
                variant="outline"
                disabled={!hasNext()}
                onPress={() => props.onPageChange(props.page.offset + props.page.limit)}
              />
            </View>
          </View>
        </If>
      </Scroll>
    </View>
  );
}
