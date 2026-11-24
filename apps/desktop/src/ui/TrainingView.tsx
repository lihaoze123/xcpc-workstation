// Training space shell (Phase 4B):
// Top bar with 3-view segmented switch (Overview | Contests | Submissions) on the left,
// last-synced timestamp + manual Sync button on the right (reusing Phase 4A sync + Toast).

import { computed, effect, signal, Text, View } from "nsh";
import { t } from "./i18n";
import { Btn, DelayedSpinner, Segmented } from "./kit";
import { preview, refresh, revision, workspaceVersion } from "./store";
import { showToast } from "./Toast";
import { ContestsTab } from "./training/ContestsTab";
import { OverviewTab } from "./training/OverviewTab";
import { SubmissionsTab } from "./training/SubmissionsTab";
import {
  DEFAULT_SUBMISSIONS_FILTER,
  drilldownFromBucket,
  drilldownFromTag,
  formatSyncTimestamp,
  type ContestCalendarItem,
  type DifficultyBucket,
  type SubmissionsFilter,
  type SubmissionsPage,
  type TimeWindow,
  type TrainingDashboardData,
  type TrainingTab,
} from "./training/types";

const PAGE_LIMIT = 50;

const EMPTY_SUBMISSIONS_PAGE: SubmissionsPage = {
  total: 0,
  offset: 0,
  limit: PAGE_LIMIT,
  items: [],
};

export function TrainingView(props: { active: boolean }) {
  const tab = signal<TrainingTab>("overview");
  const windowVal = signal<TimeWindow>("all");
  const heatmapYear = signal<number | null>(null);
  const tagMinRating = signal<number | null>(null);

  const contestsPlatform = signal<string>("");
  const includeFinished = signal<boolean>(false);

  const subFilter = signal<SubmissionsFilter>(DEFAULT_SUBMISSIONS_FILTER);
  const subOffset = signal<number>(0);

  const dashboard = signal<TrainingDashboardData | undefined>(undefined);
  const contests = signal<ContestCalendarItem[]>([]);
  const submissions = signal<SubmissionsPage>(EMPTY_SUBMISSIONS_PAGE);

  const syncing = signal(false);
  const reloadTick = signal(0);

  const loadDashboard = () => {
    try {
      const res = preview.trainingDashboard(windowVal(), heatmapYear(), tagMinRating());
      if (res.ok && res.reportJson) {
        dashboard.set(JSON.parse(res.reportJson) as TrainingDashboardData);
      }
    } catch {
      // Ignore in headless tests without a native sync DB.
    }
  };

  const loadContests = () => {
    try {
      const plat = contestsPlatform().trim();
      const res = preview.trainingContests(plat || null, includeFinished());
      if (res.ok && res.reportJson) {
        contests.set(JSON.parse(res.reportJson) as ContestCalendarItem[]);
      }
    } catch {
      // Ignore in headless tests.
    }
  };

  const loadSubmissions = () => {
    try {
      const f = subFilter();
      const res = preview.trainingSubmissions({
        byProblem: f.byProblem,
        platform: f.platform || null,
        verdict: f.verdict || null,
        unsolvedOnly: f.unsolvedOnly,
        minRating: f.minRating,
        maxRating: f.maxRating,
        tag: f.tag || null,
        search: f.search.trim() || null,
        limit: PAGE_LIMIT,
        offset: subOffset(),
      });
      if (res.ok && res.reportJson) {
        submissions.set(JSON.parse(res.reportJson) as SubmissionsPage);
      }
    } catch {
      // Ignore in headless tests.
    }
  };

  // Reload active data when view is active or workspace/filter inputs change.
  effect(() => {
    void revision();
    void workspaceVersion();
    void reloadTick();
    if (!props.active) return;
    loadDashboard();
  });

  effect(() => {
    void revision();
    void reloadTick();
    void contestsPlatform();
    void includeFinished();
    if (!props.active || tab() !== "contests") return;
    loadContests();
  });

  effect(() => {
    void revision();
    void workspaceVersion();
    void reloadTick();
    void subFilter();
    void subOffset();
    if (!props.active || tab() !== "submissions") return;
    loadSubmissions();
  });

  const knownPlatforms = computed(() => {
    const set = new Set<string>();
    for (const a of dashboard()?.accounts ?? []) set.add(a.platform);
    for (const c of contests()) set.add(c.platform);
    return [...set].sort();
  });

  const runSync = async () => {
    if (syncing()) return;
    syncing.set(true);
    try {
      const report = await preview.syncOj(null, false);
      if (report.error) {
        showToast({ tone: "issues", text: report.error });
        return;
      }
      const platforms = report.platforms ?? [];
      const totalSubs = platforms.reduce((acc, p) => acc + p.submissionsSynced, 0);
      const failed = platforms.filter((p) => !p.ok);
      if (platforms.length === 0) {
        showToast({ tone: "info", text: t("settings.plugins.syncNoHandles") });
      } else if (failed.length > 0) {
        showToast({
          tone: "issues",
          text: t("settings.plugins.syncPartialFailure", {
            failed: failed.map((f) => f.platform).join(", "),
          }),
        });
      } else {
        showToast({
          tone: "success",
          text: t("settings.plugins.syncSuccess", {
            platforms: platforms.length,
            submissions: totalSubs,
          }),
        });
      }
      refresh();
      reloadTick.set(reloadTick() + 1);
    } finally {
      syncing.set(false);
    }
  };

  const applyDrilldown = (nextFilter: SubmissionsFilter) => {
    subFilter.set(nextFilter);
    subOffset.set(0);
    tab.set("submissions");
  };

  const handleDrilldownBucket = (bucket: DifficultyBucket) => {
    applyDrilldown(drilldownFromBucket(bucket));
  };

  const handleDrilldownTag = (tag: string, minRating: number | null) => {
    applyDrilldown(drilldownFromTag(tag, minRating));
  };

  const tabItems = computed(() => [
    { value: "overview" as const, label: t("training.tab.overview"), testID: "training-tab-overview" },
    { value: "contests" as const, label: t("training.tab.contests"), testID: "training-tab-contests" },
    { value: "submissions" as const, label: t("training.tab.submissions"), testID: "training-tab-submissions" },
  ]);

  return (
    <View testID="training-view" style={{ flex: 1, flexDirection: "column" }} class="bg-background">
      {/* Top toolbar: left 3-view Segmented, right last-synced + manual Sync button */}
      <View
        class="border-b border-border bg-background"
        style={{
          height: 46,
          paddingLeft: 20,
          paddingRight: 20,
          flexDirection: "row",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 12,
          flexShrink: 0,
        }}
      >
        <Segmented
          items={tabItems()}
          value={tab()}
          onChange={(next) => tab.set(next)}
          itemWidth={82}
        />

        <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
          <Text
            testID="training-last-synced"
            style={{ fontSize: 12, fontVariant: ["tabular-nums"] }}
            class="text-muted-foreground"
          >
            {t("settings.plugins.lastSyncedAt", {
              time: formatSyncTimestamp(dashboard()?.last_synced_at, t("settings.plugins.neverSynced")),
            })}
          </Text>
          <DelayedSpinner active={syncing()} />
          <Btn
            testID="training-sync-btn"
            label={t("training.sync")}
            icon="rotateCcw"
            variant="outline"
            disabled={syncing()}
            onPress={() => void runSync()}
          />
        </View>
      </View>

      {/* Active tab body */}
      <If when={tab() === "overview"}>
        <OverviewTab
          data={dashboard()}
          window={windowVal()}
          tagMinRating={tagMinRating()}
          onWindowChange={(w) => windowVal.set(w)}
          onYearChange={(y) => heatmapYear.set(y)}
          onTagMinRatingChange={(m) => tagMinRating.set(m)}
          onDrilldownBucket={handleDrilldownBucket}
          onDrilldownTag={handleDrilldownTag}
        />
      </If>

      <If when={tab() === "contests"}>
        <ContestsTab
          contests={contests()}
          platforms={knownPlatforms()}
          platformFilter={contestsPlatform()}
          includeFinished={includeFinished()}
          onPlatformChange={(p) => contestsPlatform.set(p)}
          onIncludeFinishedChange={(v) => includeFinished.set(v)}
        />
      </If>

      <If when={tab() === "submissions"}>
        <SubmissionsTab
          page={submissions()}
          platforms={knownPlatforms()}
          filter={subFilter()}
          onFilterChange={(next) => {
            subFilter.set(next);
            subOffset.set(0);
          }}
          onPageChange={(nextOffset) => subOffset.set(nextOffset)}
          onReload={() => reloadTick.set(reloadTick() + 1)}
        />
      </If>
    </View>
  );
}
