// Training space shell (Phase 4B+):
// Top bar with 4-view segmented switch (Overview | Contests | Submissions | Editorials) on the left,
// manual log ("记一题") + conditional OJ last-synced & Sync controls on the right.

import { computed, effect, signal, Text, View } from "nsh";
import { t } from "./i18n";
import { Btn, Segmented } from "./kit";
import {
  globalLastSyncedAt,
  info,
  openStudio,
  pluginsRevision,
  preview,
  refresh,
  refreshPluginsOverview,
  revision,
  setGlobalLastSyncedAt,
  workspaceVersion,
} from "./store";
import { showToast } from "./Toast";
import { ContestsTab } from "./training/ContestsTab";
import { EditorialsTab } from "./training/EditorialsTab";
import { zhEn } from "./training/locale";
import { ManualLogModal } from "./training/ManualLogModal";
import type { OnboardingOjSource } from "./training/OnboardingEmptyState";
import { OverviewTab } from "./training/OverviewTab";
import { SubmissionsTab } from "./training/SubmissionsTab";
import {
  DEFAULT_EDITORIAL_FILTER,
  DEFAULT_SUBMISSIONS_FILTER,
  drilldownFromBucket,
  drilldownFromTag,
  formatSyncTimestamp,
  type ContestCalendarItem,
  type DifficultyBucket,
  type EditorialFilter,
  type EditorialSummaryItem,
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

export interface TrainingViewProps {
  active: boolean;
  onOpenProblem?: (id: string, opts?: { initialTab?: "statement" | "editorial" }) => void;
  submitManualLog?: (reqJson: string) => {
    ok: boolean;
    libraryProblemId?: string | null;
    createdInLibrary: boolean;
    error?: string | null;
  };
  fetchEditorials?: (queryJson: string) => {
    items: EditorialSummaryItem[];
    total: number;
    availableTags: string[];
    error?: string | null;
  };
  fetchDashboard?: (
    window: TimeWindow,
    year: number | null,
    tagMinRating: number | null,
  ) => TrainingDashboardData | undefined;
  fetchOjSources?: () => OnboardingOjSource[];
  fetchContests?: (platform: string | null, includeFinished: boolean) => ContestCalendarItem[];
  fetchSubmissions?: (filter: SubmissionsFilter, offset: number) => SubmissionsPage;
}

export function TrainingView(props: TrainingViewProps) {
  const tab = signal<TrainingTab>("overview");
  const windowVal = signal<TimeWindow>("all");
  const heatmapYear = signal<number | null>(null);
  const tagMinRating = signal<number | null>(null);

  const contestsPlatform = signal<string>("");
  const includeFinished = signal<boolean>(false);

  const subFilter = signal<SubmissionsFilter>(DEFAULT_SUBMISSIONS_FILTER);
  const subOffset = signal<number>(0);

  const editorialFilter = signal<EditorialFilter>({ ...DEFAULT_EDITORIAL_FILTER });
  const editorials = signal<EditorialSummaryItem[]>([]);
  const editorialsTotal = signal<number>(0);
  const editorialTags = signal<string[]>([]);

  const dashboard = signal<TrainingDashboardData | undefined>(undefined);
  const contests = signal<ContestCalendarItem[]>([]);
  const seenContestPlatforms = signal<string[]>([]);
  const submissions = signal<SubmissionsPage>(EMPTY_SUBMISSIONS_PAGE);
  const ojSources = signal<OnboardingOjSource[]>([]);

  const manualLogOpen = signal(false);
  const syncing = signal(false);
  const reloadTick = signal(0);

  const loadOjSources = () => {
    try {
      if (props.fetchOjSources) {
        ojSources.set(props.fetchOjSources());
        return;
      }
      const ov = refreshPluginsOverview(false);
      const sources: OnboardingOjSource[] = (ov.plugins ?? [])
        .filter((p) => p.enabled && p.capabilities.includes("oj_source"))
        .map((p) => ({
          id: p.id,
          name: p.name,
          handle: (p.handle ?? "").trim(),
        }));
      ojSources.set(sources);
    } catch {
      // Ignore in headless tests without plugin overview.
    }
  };

  const dashboardCache = new Map<string, TrainingDashboardData>();
  let dashboardReqSeq = 0;

  const loadDashboard = () => {
    const win = windowVal();
    const year = heatmapYear();
    const minRating = tagMinRating();
    const cacheKey = `${win}|${year ?? ""}|${minRating ?? ""}`;
    try {
      if (props.fetchDashboard) {
        const next = props.fetchDashboard(win, year, minRating);
        dashboard.set(next);
        if (next) {
          dashboardCache.set(cacheKey, next);
          setGlobalLastSyncedAt(next.last_synced_at);
        }
        return;
      }
      // Show cached snapshot immediately (stale-while-revalidate) so space switch never blocks.
      const cached = dashboardCache.get(cacheKey);
      if (cached && dashboard() !== cached) {
        dashboard.set(cached);
      }
      if (typeof preview.trainingDashboardAsync === "function") {
        const seq = ++dashboardReqSeq;
        void preview
          .trainingDashboardAsync(win, year, minRating)
          .then((res) => {
            if (seq !== dashboardReqSeq) return;
            if (res.ok && res.reportJson) {
              const next = JSON.parse(res.reportJson) as TrainingDashboardData;
              dashboardCache.set(cacheKey, next);
              dashboard.set(next);
              setGlobalLastSyncedAt(next.last_synced_at);
            }
          })
          .catch(() => {
            // Ignore in headless tests without a native sync DB.
          });
        return;
      }
      const res = preview.trainingDashboard(win, year, minRating);
      if (res.ok && res.reportJson) {
        const next = JSON.parse(res.reportJson) as TrainingDashboardData;
        dashboardCache.set(cacheKey, next);
        dashboard.set(next);
        setGlobalLastSyncedAt(next.last_synced_at);
      }
    } catch {
      // Ignore in headless tests without a native sync DB.
    }
  };

  const loadContests = () => {
    try {
      const plat = contestsPlatform().trim();
      if (props.fetchContests) {
        const items = props.fetchContests(plat || null, includeFinished());
        contests.set(items);
        if (items.length > 0) {
          const prev = seenContestPlatforms();
          const merged = new Set<string>(prev);
          for (const c of items) {
            if (c.platform) merged.add(c.platform);
          }
          if (merged.size !== prev.length) {
            seenContestPlatforms.set([...merged].sort());
          }
        }
        return;
      }
      const res = preview.trainingContests(plat || null, includeFinished());
      if (res.ok && res.reportJson) {
        const items = JSON.parse(res.reportJson) as ContestCalendarItem[];
        contests.set(items);
        if (items.length > 0) {
          const prev = seenContestPlatforms();
          const merged = new Set<string>(prev);
          for (const c of items) {
            if (c.platform) merged.add(c.platform);
          }
          if (merged.size !== prev.length) {
            seenContestPlatforms.set([...merged].sort());
          }
        }
      }
    } catch {
      // Ignore in headless tests.
    }
  };

  const loadSubmissions = () => {
    try {
      const f = subFilter();
      if (props.fetchSubmissions) {
        submissions.set(props.fetchSubmissions(f, subOffset()));
        return;
      }
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

  const loadEditorials = () => {
    try {
      const f = editorialFilter();
      const query: Record<string, unknown> = {};
      if (f.search.trim()) query.search = f.search.trim();
      if (f.tag) query.tag = f.tag;
      if (f.minRating !== null) query.min_rating = f.minRating;
      if (f.maxRating !== null) query.max_rating = f.maxRating;
      const fetchFn = props.fetchEditorials ?? ((json: string) => preview.trainingListEditorials(json));
      const res = fetchFn(JSON.stringify(query));
      if (!res.error) {
        editorials.set(res.items ?? []);
        editorialsTotal.set(res.total ?? 0);
        editorialTags.set(res.availableTags ?? []);
      }
    } catch {
      // Ignore in headless tests.
    }
  };

  // Reload active data when view is active or workspace/filter inputs change.
  effect(() => {
    void revision();
    void workspaceVersion();
    void pluginsRevision();
    void reloadTick();
    if (!props.active) return;
    loadOjSources();
    loadDashboard();
  });

  effect(() => {
    void revision();
    void pluginsRevision();
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

  effect(() => {
    void revision();
    void workspaceVersion();
    void reloadTick();
    void editorialFilter();
    if (!props.active || tab() !== "editorials") return;
    loadEditorials();
  });

  const hasConfiguredAccounts = computed(() => {
    if ((dashboard()?.accounts.length ?? 0) > 0) return true;
    return ojSources().some((s) => s.handle.length > 0);
  });

  const hasAnyOverviewData = computed(() => {
    const cur = dashboard();
    if (!cur) return false;
    return (
      cur.accounts.length > 0 ||
      Boolean(cur.has_local_records) ||
      cur.kpi.total_ac > 0 ||
      cur.kpi.total_submissions > 0 ||
      cur.rating_series.length > 0
    );
  });

  const showingOnboarding = computed(() => tab() === "overview" && !hasAnyOverviewData());

  const knownContestPlatforms = computed(() => {
    const set = new Set<string>(seenContestPlatforms());
    for (const s of ojSources()) {
      if (s.id) set.add(s.id);
    }
    for (const a of dashboard()?.accounts ?? []) {
      if (a.platform) set.add(a.platform);
    }
    for (const c of contests()) {
      if (c.platform) set.add(c.platform);
    }
    const activePlat = contestsPlatform().trim();
    if (activePlat) set.add(activePlat);
    return [...set].sort();
  });

  const knownSubmissionPlatforms = computed(() => {
    const set = new Set<string>(knownContestPlatforms());
    if (dashboard()?.has_local_records || submissions().items.some((r) => r.platform === "local")) {
      set.add("local");
    }
    if (subFilter().platform) set.add(subFilter().platform);
    return [...set].sort();
  });

  const runSync = async () => {
    if (syncing() || !hasConfiguredAccounts()) return;
    syncing.set(true);
    showToast({
      tone: "info",
      text: zhEn("正在同步…", "Syncing…"),
      durationMs: 60_000,
    });
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
        showToast({ tone: "info", text: zhEn("先填写账号", "Configure an account first") });
      } else if (failed.length > 0) {
        showToast({
          tone: "issues",
          text: t("settings.plugins.syncPartialFailure", {
            failed: failed.map((f) => f.platform).join(", "),
          }),
        });
      } else if (totalSubs > 0) {
        showToast({
          tone: "success",
          text: zhEn(`已同步，新增 ${totalSubs} 条提交`, `Synced, ${totalSubs} new submissions`),
        });
      } else {
        showToast({
          tone: "success",
          text: zhEn("已是最新", "Already up to date"),
        });
      }
      refresh();
      loadOjSources();
      reloadTick.set(reloadTick() + 1);
    } catch (err) {
      showToast({
        tone: "issues",
        text: err instanceof Error ? err.message : String(err),
      });
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

  const tabItems = computed(() => [
    { value: "overview" as const, label: t("training.tab.overview"), testID: "training-tab-overview" },
    { value: "contests" as const, label: t("training.tab.contests"), testID: "training-tab-contests" },
    { value: "submissions" as const, label: t("training.tab.submissions"), testID: "training-tab-submissions" },
    { value: "editorials" as const, label: t("training.tab.editorials"), testID: "training-tab-editorials" },
  ]);

  const readOnly = () => info().readOnly;

  return (
    <View
      testID="training-view"
      style={{ flex: 1, height: "100%", minHeight: 0, overflow: "hidden", flexDirection: "column" }}
      class="bg-background"
    >
      {/* Top toolbar: left 4-view Segmented, right manual log + conditional OJ sync controls */}
      <View
        testID="training-topbar"
        class="border-b border-border bg-background"
        style={{
          minHeight: 50,
          paddingTop: 10,
          paddingBottom: 10,
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
          itemWidth={80}
        />

        <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
          <If when={hasConfiguredAccounts()}>
            <Text
              testID="training-last-synced"
              style={{ fontSize: 12, fontVariant: ["tabular-nums"] }}
              class="text-muted-foreground"
            >
              {t("settings.plugins.lastSyncedAt", {
                time: formatSyncTimestamp(
                  globalLastSyncedAt() ?? dashboard()?.last_synced_at,
                  t("settings.plugins.neverSynced"),
                ),
              })}
            </Text>
            <Btn
              testID="training-sync-btn"
              label={t("training.sync")}
              icon="rotateCcw"
              variant="outline"
              disabled={syncing()}
              onPress={() => void runSync()}
            />
          </If>

          <If when={!showingOnboarding()}>
            <Btn
              testID="training-manual-log-btn"
              label={t("training.manualLog.button")}
              icon="plus"
              variant="outline"
              disabled={readOnly()}
              onPress={() => manualLogOpen.set(true)}
            />
          </If>
        </View>
      </View>

      {/* Active tab body: constrained flex container so child <Scroll> scrolls cleanly */}
      <View
        testID="training-body"
        style={{ flex: 1, minHeight: 0, overflow: "hidden", flexDirection: "column" }}
      >
        <If when={tab() === "overview"}>
          <OverviewTab
            data={dashboard()}
            ojSources={ojSources()}
            hasConfiguredAccounts={hasConfiguredAccounts()}
            window={windowVal()}
            tagMinRating={tagMinRating()}
            onWindowChange={(w) => windowVal.set(w)}
            onYearChange={(y) => heatmapYear.set(y)}
            onTagMinRatingChange={(m) => tagMinRating.set(m)}
            onDrilldownBucket={handleDrilldownBucket}
            onDrilldownTag={handleDrilldownTag}
            onOpenManualLog={() => manualLogOpen.set(true)}
            onAccountConfigured={() => {
              loadOjSources();
              reloadTick.set(reloadTick() + 1);
              void runSync();
            }}
          />
        </If>

        <If when={tab() === "contests"}>
          <ContestsTab
            contests={contests()}
            platforms={knownContestPlatforms()}
            platformFilter={contestsPlatform()}
            includeFinished={includeFinished()}
            onPlatformChange={(p) => contestsPlatform.set(p)}
            onIncludeFinishedChange={(v) => includeFinished.set(v)}
          />
        </If>

        <If when={tab() === "submissions"}>
          <SubmissionsTab
            page={submissions()}
            platforms={knownSubmissionPlatforms()}
            filter={subFilter()}
            onFilterChange={(next) => {
              subFilter.set(next);
              subOffset.set(0);
            }}
            onPageChange={(nextOffset) => subOffset.set(nextOffset)}
            onReload={() => reloadTick.set(reloadTick() + 1)}
            onOpenProblem={props.onOpenProblem}
          />
        </If>

        <If when={tab() === "editorials"}>
          <EditorialsTab
            active={props.active && tab() === "editorials"}
            items={editorials()}
            total={editorialsTotal()}
            availableTags={editorialTags()}
            filter={editorialFilter()}
            onFilterChange={(next) => editorialFilter.set(next)}
            onOpenProblem={props.onOpenProblem}
            onOpenManualLog={() => manualLogOpen.set(true)}
          />
        </If>
      </View>

      <ManualLogModal
        open={manualLogOpen()}
        onClose={() => manualLogOpen.set(false)}
        submitRecord={props.submitManualLog}
        onSaved={(res) => {
          reloadTick.set(reloadTick() + 1);
          if (res.openEditorial && res.libraryProblemId) {
            openProblemEditorial(res.libraryProblemId);
          } else {
            showToast({
              tone: "success",
              text: t("training.manualLog.savedToast"),
            });
          }
        }}
      />
    </View>
  );
}
