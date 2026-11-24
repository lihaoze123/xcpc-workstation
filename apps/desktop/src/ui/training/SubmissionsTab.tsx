// Training space View 3: Submissions Search & One-Click Ingest.
// Supports raw vs. per-problem deduplication, unsolved-only toggle, multi-dimensional filters
// (including drill-down from Overview), external URL open, and silent one-click ingest into
// the workspace library with a bottom-right Toast ("已收入题库 · 打开").

import { computed, Scroll, signal, Text, View } from "nsh";
import { Input, Switch } from "nsh/base";
import { Preview } from "xcpc-workstation/native";
import { t } from "../i18n";
import { Icon } from "../Icon";
import { Btn, Chip, IconBtn, RatingBadge } from "../kit";
import { info, openStudio, preview, refresh } from "../store";
import { cardShadow, MONO } from "../theme";
import { showToast } from "../Toast";
import {
  DEFAULT_SUBMISSIONS_FILTER,
  formatUnixDateTime,
  hasActiveSubmissionsFilter,
  platformDisplayName,
  verdictTone,
  type SubmissionsFilter,
  type SubmissionsPage,
  type SubmissionViewRow,
} from "./types";

const VERDICT_OPTIONS = ["", "AC", "WA", "TLE", "MLE", "RE", "CE"] as const;

export function SubmissionsTab(props: {
  page: SubmissionsPage;
  platforms: readonly string[];
  filter: SubmissionsFilter;
  onFilterChange: (next: SubmissionsFilter) => void;
  onPageChange: (offset: number) => void;
  onReload: () => void;
}) {
  const ingestingKey = signal<string | undefined>(undefined);

  const patch = (partial: Partial<SubmissionsFilter>) => {
    props.onFilterChange({ ...props.filter, ...partial });
  };

  const clearFilters = () => {
    props.onFilterChange({
      ...DEFAULT_SUBMISSIONS_FILTER,
      byProblem: props.filter.byProblem,
    });
  };

  const openProblemStudio = (problemId: string, toEditorial = false) => {
    openStudio({
      target: "problem",
      id: problemId,
      initialTab: toEditorial ? "editorial" : undefined,
    });
  };

  const ingestRow = (row: SubmissionViewRow) => {
    const key = `${row.platform}:${row.problem_key}`;
    if (ingestingKey() === key) return;
    ingestingKey.set(key);
    try {
      const res = preview.trainingIngestProblem({
        url: row.problem_url ?? null,
        platform: row.platform,
        problemKey: row.problem_key,
        title: row.problem_title ?? null,
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
              run: () => openProblemStudio(pid, true),
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
    if (tone === "tle") return "bg-chart-4/15 text-chart-4";
    return "bg-muted text-secondary-foreground";
  };

  const activeFilterCount = computed(() => hasActiveSubmissionsFilter(props.filter));
  const rangeStart = () => (props.page.total === 0 ? 0 : props.page.offset + 1);
  const rangeEnd = () => Math.min(props.page.total, props.page.offset + props.page.items.length);
  const hasPrev = () => props.page.offset > 0;
  const hasNext = () => props.page.offset + props.page.limit < props.page.total;

  return (
    <View style={{ flex: 1 }}>
      {/* Filter toolbar */}
      <View
        class="border-b border-border bg-card/50"
        style={{
          paddingTop: 10,
          paddingBottom: 10,
          paddingLeft: 20,
          paddingRight: 20,
          gap: 8,
        }}
      >
        {/* Row 1: Search + mode toggle (by-problem) + unsolved-only + clear */}
        <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 10 }}>
          <View
            class="bg-card border border-border focus-within:border-input"
            style={{
              width: 240,
              height: 28,
              borderRadius: 6,
              paddingLeft: 8,
              paddingRight: 6,
              flexDirection: "row",
              alignItems: "center",
              gap: 6,
            }}
          >
            <Icon name="search" size={13} tone="muted-foreground" />
            <Input
              testID="submissions-search"
              class="bg-transparent placeholder:text-muted-foreground"
              style={{ flex: 1, height: 24, fontSize: 12.5 }}
              value={props.filter.search}
              placeholder={t("training.submissions.searchPlaceholder")}
              autoCapitalize="none"
              autoCorrect={false}
              onChange={(e) => patch({ search: e.value })}
            />
            <If when={props.filter.search.length > 0}>
              <IconBtn icon="x" size={18} iconSize={11} tone="muted-foreground" onPress={() => patch({ search: "" })} />
            </If>
          </View>

          {/* Raw vs By-Problem toggle */}
          <View
            class="bg-muted border border-border"
            style={{ flexDirection: "row", padding: 2, borderRadius: 7, gap: 2 }}
          >
            <View
              testID="submissions-by-raw"
              onPress={() => patch({ byProblem: false })}
              class={!props.filter.byProblem ? "bg-card border border-border" : "hover:bg-background/60"}
              style={{ height: 22, paddingLeft: 8, paddingRight: 8, borderRadius: 5, justifyContent: "center" }}
            >
              <Text
                style={{ fontSize: 11.5, fontWeight: !props.filter.byProblem ? 500 : 400 }}
                class={!props.filter.byProblem ? "text-foreground" : "text-secondary-foreground"}
              >
                {t("training.submissions.modeRaw")}
              </Text>
            </View>
            <View
              testID="submissions-by-problem"
              onPress={() => patch({ byProblem: true })}
              class={props.filter.byProblem ? "bg-card border border-border" : "hover:bg-background/60"}
              style={{ height: 22, paddingLeft: 8, paddingRight: 8, borderRadius: 5, justifyContent: "center" }}
            >
              <Text
                style={{ fontSize: 11.5, fontWeight: props.filter.byProblem ? 500 : 400 }}
                class={props.filter.byProblem ? "text-foreground" : "text-secondary-foreground"}
              >
                {t("training.submissions.modeByProblem")}
              </Text>
            </View>
          </View>

          {/* Unsolved only switch */}
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <Switch
              testID="submissions-unsolved-only"
              checked={props.filter.unsolvedOnly}
              onChange={(e) => patch({ unsolvedOnly: e.checked, byProblem: e.checked ? true : props.filter.byProblem })}
            />
            <Text style={{ fontSize: 12 }} class="text-secondary-foreground">
              {t("training.submissions.unsolvedOnly")}
            </Text>
          </View>

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

        {/* Row 2: Platform chips + Verdict chips + active drilldown chips (rating range / tag) */}
        <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 6 }}>
          {/* Platforms */}
          <If when={props.platforms.length > 0}>
            <Chip
              label={t("training.submissions.allPlatforms")}
              active={props.filter.platform === ""}
              onPress={() => patch({ platform: "" })}
            />
            <Each of={props.platforms} key={(p) => p}>
              {(plat) => (
                <Chip
                  label={platformDisplayName(plat())}
                  active={props.filter.platform === plat()}
                  onPress={() => patch({ platform: props.filter.platform === plat() ? "" : plat() })}
                />
              )}
            </Each>
            <View style={{ width: 1, height: 14, marginLeft: 2, marginRight: 2 }} class="bg-border" />
          </If>

          {/* Verdicts */}
          <Each of={VERDICT_OPTIONS} key={(v) => v || "all"}>
            {(v) => (
              <Chip
                label={v() === "" ? t("training.submissions.allVerdicts") : v()}
                active={props.filter.verdict === v()}
                onPress={() => patch({ verdict: v() })}
              />
            )}
          </Each>

          {/* Active drilldown pills */}
          <If when={props.filter.minRating !== null || props.filter.maxRating !== null}>
            <Chip
              active
              label={
                props.filter.minRating !== null && props.filter.maxRating !== null
                  ? `${props.filter.minRating}–${props.filter.maxRating} ×`
                  : props.filter.minRating !== null
                    ? `≥${props.filter.minRating} ×`
                    : `<${props.filter.maxRating} ×`
              }
              onPress={() => patch({ minRating: null, maxRating: null })}
            />
          </If>
          <If when={props.filter.tag !== ""}>
            <Chip
              active
              label={`#${props.filter.tag} ×`}
              onPress={() => patch({ tag: "" })}
            />
          </If>
        </View>
      </View>

      {/* Submissions list */}
      <Scroll style={{ flex: 1 }} contentStyle={{ padding: 20, gap: 12, paddingBottom: 32 }}>
        <If
          when={props.page.items.length > 0}
          else={
            <View
              testID="training-submissions-empty"
              class="bg-card border border-border"
              style={{
                borderRadius: 10,
                padding: 28,
                alignItems: "center",
                justifyContent: "center",
                gap: 6,
                boxShadow: cardShadow(),
              }}
            >
              <Text style={{ fontSize: 13.5, fontWeight: 500 }} class="text-foreground">
                {t("training.submissions.emptyTitle")}
              </Text>
              <Text style={{ fontSize: 12 }} class="text-muted-foreground">
                {t("training.submissions.emptyDesc")}
              </Text>
            </View>
          }
        >
          <View
            testID="training-submissions-table"
            class="bg-card border border-border"
            style={{ borderRadius: 10, overflow: "hidden", boxShadow: cardShadow() }}
          >
            <Each of={props.page.items} key={(r) => `${r.platform}:${r.submission_id}:${r.problem_key}`}>
              {(row, idx) => {
                const key = () => `${row().platform}:${row().problem_key}`;
                const libId = () => row().library_problem_id;
                const readOnly = () => info().readOnly;
                return (
                  <View>
                    <If when={idx() > 0}>
                      <View style={{ height: 1, marginLeft: 14, marginRight: 14 }} class="bg-border" />
                    </If>
                    <View
                      testID={`submission-row-${row().platform}-${row().problem_key}`}
                      style={{
                        minHeight: 44,
                        paddingTop: 8,
                        paddingBottom: 8,
                        paddingLeft: 14,
                        paddingRight: 14,
                        flexDirection: "row",
                        alignItems: "center",
                        gap: 10,
                      }}
                    >
                      {/* Platform */}
                      <View style={{ width: 78, flexShrink: 0 }}>
                        <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
                          {platformDisplayName(row().platform)}
                        </Text>
                      </View>

                      {/* Problem key + title */}
                      <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
                        <View style={{ flexDirection: "row", alignItems: "center", gap: 6, minWidth: 0 }}>
                          <Text
                            style={{ fontSize: 12, fontFamily: MONO, fontWeight: 500, flexShrink: 0 }}
                            class="text-secondary-foreground"
                          >
                            {row().problem_key}
                          </Text>
                          <Text
                            onPress={row().problem_url ? () => openExternal(row().problem_url) : undefined}
                            style={{
                              fontSize: 13,
                              fontWeight: 500,
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
                            style={{ fontSize: 11, numberOfLines: 1, textOverflow: "ellipsis" }}
                            class="text-muted-foreground"
                          >
                            {row().tags.slice(0, 4).join(" · ")}
                          </Text>
                        </If>
                      </View>

                      {/* Difficulty */}
                      <View style={{ width: 46, alignItems: "center", flexShrink: 0 }}>
                        <RatingBadge rating={row().normalized_rating ?? row().raw_rating} />
                      </View>

                      {/* Verdict + attempt count in by-problem mode */}
                      <View style={{ width: 92, flexDirection: "row", alignItems: "center", justifyContent: "flex-end", gap: 5, flexShrink: 0 }}>
                        <View
                          class={verdictBadgeClass(row().verdict)}
                          style={{ height: 20, paddingLeft: 6, paddingRight: 6, borderRadius: 4, justifyContent: "center" }}
                        >
                          <Text style={{ fontSize: 11, fontWeight: 600, fontVariant: ["tabular-nums"] }}>
                            {row().verdict}
                          </Text>
                        </View>
                        <If when={props.filter.byProblem && row().attempt_count > 1}>
                          <Text style={{ fontSize: 11, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                            {`×${row().attempt_count}`}
                          </Text>
                        </If>
                      </View>

                      {/* Language */}
                      <View style={{ width: 74, flexShrink: 0 }}>
                        <Text
                          style={{ fontSize: 11.5, numberOfLines: 1, textOverflow: "ellipsis" }}
                          class="text-muted-foreground"
                        >
                          {row().language || "—"}
                        </Text>
                      </View>

                      {/* Submitted time */}
                      <View style={{ width: 116, flexShrink: 0 }}>
                        <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
                          {formatUnixDateTime(row().submitted_at)}
                        </Text>
                      </View>

                      {/* Ingest / Open in Studio button */}
                      <View style={{ width: 92, alignItems: "flex-end", flexShrink: 0 }}>
                        <If
                          when={libId()}
                          else={
                            <Btn
                              testID={`submission-ingest-${row().problem_key}`}
                              label={t("training.submissions.ingest")}
                              icon="plus"
                              variant="outline"
                              disabled={readOnly() || ingestingKey() === key()}
                              onPress={() => ingestRow(row())}
                            />
                          }
                        >
                          {(pid) => (
                            <Btn
                              testID={`submission-open-${row().problem_key}`}
                              label={t("training.submissions.open")}
                              icon="arrowRight"
                              variant="ghost"
                              onPress={() => openProblemStudio(pid(), false)}
                            />
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
            <Text style={{ fontSize: 12, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
              {t("training.submissions.pageSummary", {
                start: rangeStart(),
                end: rangeEnd(),
                total: props.page.total,
              })}
            </Text>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
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
