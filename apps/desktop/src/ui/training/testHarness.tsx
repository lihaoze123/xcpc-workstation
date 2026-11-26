// Headless test harness for `training.test.tsx` (mounted via `open()` from `nsh/test`).
// Exercises real NSH Yoga layout, scrolling at 960x980, empty & local-only states,
// ManualLogModal submission, SubmissionsTab interactions, and EditorialsTab filtering/preview.

import "../theme";
import { mount, signal, Storage, Text, View } from "nsh";
import { languagePref } from "../i18n";
import { TrainingView } from "../TrainingView";
import { OverviewTab } from "./OverviewTab";
import { SubmissionsTab } from "./SubmissionsTab";
import {
  DEFAULT_SUBMISSIONS_FILTER,
  type EditorialSummaryItem,
  type SubmissionsFilter,
  type SubmissionsPage,
  type TimeWindow,
  type TrainingDashboardData,
} from "./types";

import { FONT_SIZE } from "../tokens";
function makeSampleDashboard(): TrainingDashboardData {
  return {
    window: "all",
    last_synced_at: "2026-11-24T12:00:00Z",
    has_local_records: false,
    local_solved_count: 0,
    accounts: [
      {
        platform: "codeforces",
        handle: "tourist",
        rating: 3769,
        max_rating: 4009,
        rank: "legendary grandmaster",
        solved_count: 1500,
        submission_count: 2400,
        updated_at: "2026-11-24T12:00:00Z",
      },
    ],
    kpi: {
      today_ac: 2,
      today_submissions: 3,
      week_ac: 14,
      week_submissions: 20,
      month_ac: 45,
      month_submissions: 70,
      window_ac: 1500,
      window_submissions: 2400,
      total_ac: 1500,
      total_submissions: 2400,
      streak_days: 7,
      longest_streak_days: 30,
      unsolved_attempted_count: 12,
      library_todo_count: 5,
      library_attempted_count: 3,
    },
    available_years: [2026, 2025, 2024],
    heatmap_year: 2026,
    heatmap: [{ date: "2026-11-24", ac_count: 2, submission_count: 3, max_rating: 2600 }],
    rating_series: [
      {
        platform: "codeforces",
        points: [
          { contest_id: "1", contest_name: "CF Round 1", rating_before: 1500, rating_after: 1800, delta: 300, rank: 10, at: 1700000000 },
          { contest_id: "2", contest_name: "CF Round 2", rating_before: 1800, rating_after: 3500, delta: 1700, rank: 1, at: 1730000000 },
        ],
      },
    ],
    difficulty_trend: [
      { period: "2026-10", p75: 2400, p90: 2800, median: 2100, ac_count: 20, rated_ac_count: 18 },
      { period: "2026-11", p75: 2500, p90: 3000, median: 2200, ac_count: 25, rated_ac_count: 22 },
    ],
    difficulty_buckets: [
      { label: "1600–1899", min_rating: 1600, max_rating: 1899, ac_count: 40 },
      { label: "2400+", min_rating: 2400, max_rating: null, ac_count: 85 },
    ],
    tag_distribution: [
      { tag: "dp", ac_count: 120, avg_rating: 2200 },
      { tag: "graphs", ac_count: 95, avg_rating: 2300 },
    ],
    unsolved_problems: [],
    workspace_backlog: [],
  };
}

function makeSampleSubmissions(): SubmissionsPage {
  return {
    total: 2,
    offset: 0,
    limit: 50,
    items: [
      {
        platform: "codeforces",
        submission_id: "101",
        handle: "tourist",
        problem_key: "1900A",
        problem_title: "Cover in Water",
        problem_url: "https://codeforces.com/problemset/problem/1900/A",
        raw_rating: 800,
        normalized_rating: 800,
        tags: ["greedy"],
        verdict: "WA",
        language: "C++23 (GCC 14)",
        submitted_at: 1730000000,
        attempt_count: 2,
        solved: false,
        library_problem_id: "cf-1900a",
      },
      {
        platform: "local",
        submission_id: "manual:1",
        handle: "",
        problem_key: "tree-dp-note",
        problem_title: "Tree DP Practice",
        problem_url: null,
        raw_rating: 1900,
        normalized_rating: 1900,
        tags: ["dp"],
        verdict: "ATTEMPTED",
        language: null,
        submitted_at: 1730003600,
        attempt_count: 1,
        solved: false,
        library_problem_id: null,
      },
    ],
  };
}

const SAMPLE_EDITORIALS: EditorialSummaryItem[] = [
  {
    problemId: "tree-dp",
    title: "树形 DP 经典题",
    rating: 2100,
    tags: ["dp", "trees"],
    sources: [{ platform: "codeforces", id: "1900E", url: "https://codeforces.com/problemset/problem/1900/E" }],
    excerpt: "按子树大小做启发式合并，时间复杂度 O(n log n)。",
    updatedAt: "2026-11-24T10:00:00Z",
  },
  {
    problemId: "graph-flow",
    title: "最小割建模",
    rating: 2400,
    tags: ["graphs", "flows"],
    sources: [{ platform: "atcoder", id: "abc300_g", url: null }],
    excerpt: "使用最大权闭合子图转化为 Dinic 最小割求解。",
    updatedAt: "2026-11-23T08:00:00Z",
  },
];

function filterSampleEditorials(queryJson: string) {
  const q = JSON.parse(queryJson || "{}") as {
    search?: string;
    tag?: string;
    min_rating?: number;
    max_rating?: number;
  };
  const search = (q.search ?? "").trim().toLowerCase();
  const tag = (q.tag ?? "").trim().toLowerCase();
  const items = SAMPLE_EDITORIALS.filter((it) => {
    if (search) {
      const hay = `${it.problemId} ${it.title} ${it.excerpt} ${it.tags.join(" ")}`.toLowerCase();
      if (!hay.includes(search)) return false;
    }
    if (tag && !it.tags.some((tg) => tg.toLowerCase() === tag)) return false;
    if (q.min_rating != null && (it.rating == null || it.rating < q.min_rating)) return false;
    if (q.max_rating != null && (it.rating == null || it.rating > q.max_rating)) return false;
    return true;
  });
  return {
    items,
    total: items.length,
    availableTags: ["dp", "flows", "graphs", "trees"],
    error: null,
  };
}

function makeEmptyDashboard(): TrainingDashboardData {
  return {
    ...makeSampleDashboard(),
    last_synced_at: null,
    has_local_records: false,
    local_solved_count: 0,
    accounts: [],
    kpi: {
      today_ac: 0,
      today_submissions: 0,
      week_ac: 0,
      week_submissions: 0,
      month_ac: 0,
      month_submissions: 0,
      window_ac: 0,
      window_submissions: 0,
      total_ac: 0,
      total_submissions: 0,
      streak_days: 0,
      longest_streak_days: 0,
      unsolved_attempted_count: 0,
      library_todo_count: 0,
      library_attempted_count: 0,
    },
    heatmap: [],
    rating_series: [],
    difficulty_trend: [],
    difficulty_buckets: [],
    tag_distribution: [],
    unsolved_problems: [],
    workspace_backlog: [],
  };
}

function TrainingTestHarness() {
  languagePref.set("zh-CN");
  const mode = signal("");
  void Storage.getItem("mode").then((m) => mode.set(m ?? "overview"));
  const win = signal<TimeWindow>("all");
  const filter = signal<SubmissionsFilter>({ ...DEFAULT_SUBMISSIONS_FILTER });
  const openedRecord = signal("");
  const submittedManualLog = signal("");

  return (
    <View style={{ flex: 1, height: "100%", minHeight: 0, overflow: "hidden", flexDirection: "column" }} class="bg-background">
      <If when={mode() === "shell"}>
        <View style={{ flex: 1, height: "100%", minHeight: 0, overflow: "hidden", flexDirection: "column" }}>
          <Text testID="harness-manual-log-payload" style={{ fontSize: FONT_SIZE.micro }}>
            {submittedManualLog()}
          </Text>
          <Text testID="harness-shell-opened" style={{ fontSize: FONT_SIZE.micro }}>
            {openedRecord()}
          </Text>
          <TrainingView
            active
            onOpenProblem={(id, opts) => {
              openedRecord.set(`${id}:${opts?.initialTab ?? ""}`);
            }}
            submitManualLog={(reqJson) => {
              submittedManualLog.set(reqJson);
              const parsed = JSON.parse(reqJson) as { create_in_library?: boolean };
              return {
                ok: true,
                libraryProblemId: parsed.create_in_library ? "manual-new-prob" : null,
                createdInLibrary: Boolean(parsed.create_in_library),
                error: null,
              };
            }}
            fetchDashboard={() => makeEmptyDashboard()}
            fetchOjSources={() => [
              { id: "codeforces", name: "Codeforces", handle: "" },
              { id: "atcoder", name: "AtCoder", handle: "" },
            ]}
            fetchContests={() => []}
            fetchSubmissions={() => ({ total: 0, offset: 0, limit: 50, items: [] })}
            fetchEditorials={filterSampleEditorials}
          />
        </View>
      </If>

      <If when={mode() === "overview"}>
        <OverviewTab
          data={makeSampleDashboard()}
          window={win()}
          tagMinRating={null}
          onWindowChange={(w) => win.set(w)}
          onYearChange={() => {}}
          onTagMinRatingChange={() => {}}
          onDrilldownBucket={() => {}}
          onDrilldownTag={() => {}}
        />
      </If>

      <If when={mode() === "local-overview"}>
        <OverviewTab
          data={{
            ...makeSampleDashboard(),
            has_local_records: true,
            local_solved_count: 3,
            accounts: [],
            rating_series: [],
            kpi: {
              ...makeSampleDashboard().kpi,
              total_ac: 3,
              total_submissions: 3,
              window_ac: 3,
              window_submissions: 3,
            },
          }}
          hasConfiguredAccounts={false}
          window={win()}
          tagMinRating={null}
          onWindowChange={(w) => win.set(w)}
          onYearChange={() => {}}
          onTagMinRatingChange={() => {}}
          onDrilldownBucket={() => {}}
          onDrilldownTag={() => {}}
        />
      </If>

      <If when={mode() === "empty"}>
        <OverviewTab
          data={{
            ...makeSampleDashboard(),
            has_local_records: false,
            local_solved_count: 0,
            accounts: [],
            kpi: {
              ...makeSampleDashboard().kpi,
              total_submissions: 0,
              total_ac: 0,
              window_ac: 0,
              window_submissions: 0,
            },
            rating_series: [],
          }}
          ojSources={[
            { id: "codeforces", name: "Codeforces", handle: "" },
            { id: "atcoder", name: "AtCoder", handle: "" },
          ]}
          hasConfiguredAccounts={false}
          window={win()}
          tagMinRating={null}
          onWindowChange={(w) => win.set(w)}
          onYearChange={() => {}}
          onTagMinRatingChange={() => {}}
          onDrilldownBucket={() => {}}
          onDrilldownTag={() => {}}
          onOpenManualLog={() => openedRecord.set("manual-log-opened")}
        />
      </If>

      <If when={mode() === "submissions"}>
        <View style={{ flex: 1, height: "100%", minHeight: 0, overflow: "hidden", flexDirection: "column" }}>
          <Text testID="harness-filter-state" style={{ fontSize: FONT_SIZE.micro }}>
            {JSON.stringify(filter())}
          </Text>
          <Text testID="harness-opened-state" style={{ fontSize: FONT_SIZE.micro }}>
            {openedRecord()}
          </Text>
          <SubmissionsTab
            page={makeSampleSubmissions()}
            platforms={["codeforces", "local"]}
            filter={filter()}
            onFilterChange={(next) => filter.set(next)}
            onPageChange={() => {}}
            onReload={() => {}}
            onOpenProblem={(id, opts) => {
              openedRecord.set(`${id}:${opts?.initialTab ?? ""}`);
            }}
          />
        </View>
      </If>
    </View>
  );
}

mount(TrainingTestHarness);
