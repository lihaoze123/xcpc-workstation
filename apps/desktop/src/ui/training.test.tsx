import { describe, expect, open, test } from "nsh/test";
import { spaceName, windowTitle } from "./logic";
import { goSpace, info, space } from "./store";
import {
  buildYearHeatmapGrid,
  DEFAULT_SUBMISSIONS_FILTER,
  drilldownFromBucket,
  drilldownFromTag,
  formatCountdown,
  formatDuration,
  formatTagLabel,
  groupContestsByTime,
  hasActiveSubmissionsFilter,
  heatmapCellColor,
  niceRatingTicks,
  niceXTickIndices,
  platformDisplayName,
  verdictDisplayLabel,
  verdictTone,
  type ContestCalendarItem,
  type DifficultyBucket,
} from "./training/types";

describe("Training space helpers & drill-down state", () => {
  test("drilldownFromBucket fills byProblem, AC verdict, and rating bounds", () => {
    const bucket: DifficultyBucket = {
      label: "1900–2099",
      min_rating: 1900,
      max_rating: 2100,
      ac_count: 12,
    };
    const f = drilldownFromBucket(bucket);
    expect(f.byProblem).toBe(true);
    expect(f.verdict).toBe("AC");
    expect(f.minRating).toBe(1900);
    expect(f.maxRating).toBe(2100);
    expect(f.tag).toBe("");
    expect(hasActiveSubmissionsFilter(f)).toBe(true);
    expect(hasActiveSubmissionsFilter(DEFAULT_SUBMISSIONS_FILTER)).toBe(false);
  });

  test("drilldownFromTag fills tag, minRating, byProblem, and AC verdict", () => {
    const f = drilldownFromTag("graphs", 1900);
    expect(f.byProblem).toBe(true);
    expect(f.verdict).toBe("AC");
    expect(f.tag).toBe("graphs");
    expect(f.minRating).toBe(1900);
    expect(f.maxRating).toBeNull();
    expect(hasActiveSubmissionsFilter(f)).toBe(true);
  });

  test("buildYearHeatmapGrid covers full year (leap and non-leap) and merges sparse cells", () => {
    const grid2024 = buildYearHeatmapGrid(2024, [
      { date: "2024-02-29", ac_count: 3, submission_count: 5, max_rating: 2100 },
    ]);
    expect(grid2024.days.length).toBe(366);
    const leapDay = grid2024.days.find((d) => d.date === "2024-02-29");
    expect(leapDay).toBeDefined();
    expect(leapDay!.acCount).toBe(3);
    expect(leapDay!.submissionCount).toBe(5);
    expect(leapDay!.maxRating).toBe(2100);

    const grid2025 = buildYearHeatmapGrid(2025, []);
    expect(grid2025.days.length).toBe(365);
    expect(grid2025.monthStarts.length).toBe(12);
  });

  test("dual-mode heatmapCellColor distinguishes count vs rating modes and empty days", () => {
    const emptyDay = {
      date: "2025-01-01",
      weekIndex: 0,
      dayOfWeek: 3,
      month: 1,
      acCount: 0,
      submissionCount: 0,
      maxRating: null,
    };
    const highRatingLowCount = {
      ...emptyDay,
      acCount: 1,
      submissionCount: 2,
      maxRating: 2500,
    };
    const emptyColor = heatmapCellColor(emptyDay, "count", false);
    const countColor = heatmapCellColor(highRatingLowCount, "count", false);
    const ratingColor = heatmapCellColor(highRatingLowCount, "rating", false);

    expect(countColor).not.toBe(emptyColor);
    expect(ratingColor).not.toBe(emptyColor);
    expect(ratingColor).not.toBe(countColor);
  });

  test("time, duration, rating ticks, local platform/verdict, and contest grouping helpers behave cleanly", () => {
    expect(platformDisplayName("codeforces")).toBe("Codeforces");
    expect(platformDisplayName("atcoder")).toBe("AtCoder");
    expect(platformDisplayName("local", false)).toBe("本地");
    expect(platformDisplayName("local", true)).toBe("Local");

    // Chinese defaults
    expect(formatDuration(7200)).toBe("2 小时");
    expect(formatDuration(8100)).toBe("2 小时 15 分");
    expect(formatDuration(2700)).toBe("45 分");
    expect(formatDuration(7 * 86_400)).toBe("7 天");
    expect(formatCountdown(100_000, 10_000)).toBe("1 天");
    expect(formatCountdown(13_600, 10_000)).toBe("1 小时");

    // English mode
    expect(formatDuration(7200, true)).toBe("2h");
    expect(formatDuration(5400, true)).toBe("1h 30m");
    expect(formatDuration(7 * 86_400, true)).toBe("7d");
    expect(formatCountdown(100_000, 10_000, true)).toBe("1d 1h");

    // Nice rating ticks cover 3000+ cleanly
    const highTicks = niceRatingTicks(1200, 4000);
    expect(highTicks.some((v) => v >= 3000)).toBe(true);
    expect(highTicks.some((v) => v >= 3500)).toBe(true);
    expect(niceXTickIndices(10, 5)).toEqual([0, 2, 5, 7, 9]);

    // Contest grouping into Today / This Week / Later / Finished
    const now = Math.floor(new Date(2026, 10, 24, 12, 0, 0).getTime() / 1000);
    const contests: ContestCalendarItem[] = [
      { platform: "codeforces", contest_id: "1", name: "Today Round", start_time: now + 3600, duration_sec: 7200, end_time: now + 10800, status: "upcoming", updated_at: "" },
      { platform: "atcoder", contest_id: "2", name: "Week Round", start_time: now + 3 * 86_400, duration_sec: 6000, end_time: now + 3 * 86_400 + 6000, status: "upcoming", updated_at: "" },
      { platform: "codeforces", contest_id: "3", name: "Later Round", start_time: now + 12 * 86_400, duration_sec: 7200, end_time: now + 12 * 86_400 + 7200, status: "upcoming", updated_at: "" },
      { platform: "codeforces", contest_id: "4", name: "Old Round", start_time: now - 86_400, duration_sec: 7200, end_time: now - 79_200, status: "finished", updated_at: "" },
    ];
    const groups = groupContestsByTime(contests, now);
    expect(groups.map((g) => g.group)).toEqual(["today", "thisWeek", "later", "finished"]);

    expect(verdictTone("AC")).toBe("ac");
    expect(verdictTone("WA")).toBe("wa");
    expect(verdictTone("TLE")).toBe("tle");
    expect(verdictTone("ATTEMPTED")).toBe("attempted");
    expect(verdictTone("TODO")).toBe("todo");
    expect(verdictDisplayLabel("ATTEMPTED", false)).toBe("尝试中");
    expect(verdictDisplayLabel("ATTEMPTED", true)).toBe("Attempted");
    expect(verdictDisplayLabel("TODO", false)).toBe("待补");
    expect(verdictDisplayLabel("TODO", true)).toBe("Todo");

    // Chinese tag localization & English / unknown fallback
    expect(formatTagLabel("dp")).toBe("动态规划");
    expect(formatTagLabel("graphs")).toBe("图论");
    expect(formatTagLabel("constructive algorithms")).toBe("构造");
    expect(formatTagLabel("dfs and similar")).toBe("搜索");
    expect(formatTagLabel("fft")).toBe("FFT");
    expect(formatTagLabel("custom-rare-tag")).toBe("custom-rare-tag");
    expect(formatTagLabel("dp", true)).toBe("dp");
  });

  test("goSpace('training') falls back to 'problems' when workspace.trainingEnabled is false", () => {
    const prevInfo = info();
    const prevSpace = space();
    try {
      info.set({ ...prevInfo, trainingEnabled: false });
      goSpace("training");
      expect(space()).toBe("problems");

      info.set({ ...prevInfo, trainingEnabled: true });
      goSpace("training");
      expect(space()).toBe("training");
      expect(windowTitle({ workspace: true, workspaceName: "ws", space: "training" })).toBe(
        `${spaceName("training")} — ws`,
      );
    } finally {
      info.set(prevInfo);
      space.set(prevSpace);
    }
  });
});

describe("Training UI headless layout & interaction tests (960x980)", () => {
  test("Overview scrolls cleanly to 标签分布 and shows 3 focused KPI cards without redundant month card", async () => {
    const app = await open("./training/testHarness.tsx", {
      width: 960,
      height: 980,
      storage: { mode: "overview" },
    });

    expect(app.shows("id:training-overview-scroll")).toBe(true);
    expect(app.shows("id:training-kpi-window")).toBe(true);
    expect(app.shows("id:training-kpi-streak")).toBe(true);
    expect(app.shows("id:training-kpi-today")).toBe(true);
    expect(app.shows("id:training-rating-chart")).toBe(true);
    expect(app.shows("id:training-kpi-month")).toBe(false);
    expect(app.shows("id:training-window-caption")).toBe(false);

    // Scroll down to reach the bottom distributions section
    await app.scroll("id:training-overview-scroll", 400);
    expect(app.shows("id:training-tags-card")).toBe(true);
    expect(app.shows("id:training-tags-header")).toBe(true);
    const texts = app.texts();
    expect(texts).toContain("标签分布");
    expect(texts).toContain("标签");
    expect(texts).toContain("过题");
    expect(texts).toContain("均难");
    expect(texts).toContain("动态规划");
    expect(texts).toContain("图论");
    expect(texts).toContain("做出的题有多难");
  });

  test("Local-only user sees Overview dashboard (not onboarding) and hides empty Rating chart", async () => {
    const app = await open("./training/testHarness.tsx", {
      width: 960,
      height: 980,
      storage: { mode: "local-overview" },
    });

    expect(app.shows("id:training-overview-empty")).toBe(false);
    expect(app.shows("id:training-kpi-window")).toBe(true);
    expect(app.shows("id:training-kpi-streak")).toBe(true);
    expect(app.shows("id:training-kpi-today")).toBe(true);
    expect(app.shows("id:training-rating-chart")).toBe(false);
    expect(app.shows("id:training-trend-chart")).toBe(true);
  });

  test("Overview renders dual-path onboarding (manual log + OJ handles) when unconfigured", async () => {
    const app = await open("./training/testHarness.tsx", {
      width: 960,
      height: 980,
      storage: { mode: "empty" },
    });

    expect(app.shows("id:training-overview-empty")).toBe(true);
    expect(app.shows("id:training-onboarding-manual-card")).toBe(true);
    expect(app.shows("id:training-onboarding-manual-log-btn")).toBe(true);
    expect(app.shows("id:training-onboarding-oj-card")).toBe(true);
    expect(app.shows("id:training-onboarding-input-codeforces")).toBe(true);
    expect(app.shows("id:training-onboarding-input-atcoder")).toBe(true);
    expect(app.shows("id:training-onboarding-error-codeforces")).toBe(true);
    expect(app.shows("id:training-onboarding-error-atcoder")).toBe(true);
    expect(app.shows("id:training-kpi-window")).toBe(false);
    const texts = app.texts();
    expect(texts).toContain("手动记一题");
    expect(texts).toContain("绑定 Codeforces / AtCoder 账号");
  });

  test("SubmissionsTab displays local platform/verdict, hides AC in unsolved mode, and opens editorial", async () => {
    const app = await open("./training/testHarness.tsx", {
      width: 960,
      height: 980,
      storage: { mode: "submissions" },
    });

    expect(app.shows("id:training-submissions-scroll")).toBe(true);
    expect(app.shows("id:submissions-mode-unsolved")).toBe(true);
    expect(app.shows("id:submissions-verdict-AC")).toBe(true);
    expect(app.shows("id:submissions-diff-menu")).toBe(true);
    expect(app.shows("id:submissions-platform-local")).toBe(true);
    expect(app.shows("id:submission-ingest-tree-dp-note")).toBe(true);
    // Row tags & local platform/verdict are localized in Chinese
    const initialTexts = app.texts();
    expect(initialTexts).toContain("贪心");
    expect(initialTexts).toContain("本地");
    expect(initialTexts).toContain("尝试中");
    expect(initialTexts).not.toContain("greedy");

    // Select AC first, then switch into 仅未过题: AC chip hides and verdict resets to ""
    await app.press("id:submissions-verdict-AC");
    await app.press("id:submissions-mode-unsolved");
    expect(app.shows("id:submissions-verdict-AC")).toBe(false);
    const afterUnsolved = app.texts().find((s) => s.startsWith("{")) ?? "{}";
    const parsedUnsolved = JSON.parse(afterUnsolved) as {
      byProblem: boolean;
      unsolvedOnly: boolean;
      verdict: string;
    };
    expect(parsedUnsolved.unsolvedOnly).toBe(true);
    expect(parsedUnsolved.byProblem).toBe(true);
    expect(parsedUnsolved.verdict).toBe("");

    // Open compact difficulty dropdown chip and select 1600–1999
    await app.press("id:submissions-diff-menu");
    await app.press("1600–1999");
    await new Promise((r) => setTimeout(r, 20));
    const afterDiff = app.texts().find((s) => s.startsWith("{")) ?? "{}";
    const parsedDiff = JSON.parse(afterDiff) as { minRating: number | null; maxRating: number | null };
    expect(parsedDiff.minRating).toBe(1600);
    expect(parsedDiff.maxRating).toBe(1999);

    // Click Open on an already-ingested problem and verify editorial handoff
    await app.press("id:submission-open-1900A");
    expect(app.texts()).toContain("cf-1900a:editorial");
  });

  test("TrainingView shell renders 4 tabs, hides Sync when unconfigured, and submits ManualLogModal", async () => {
    const app = await open("./training/testHarness.tsx", {
      width: 960,
      height: 980,
      storage: { mode: "shell" },
    });

    expect(app.shows("id:training-view")).toBe(true);
    expect(app.shows("id:training-topbar")).toBe(true);
    expect(app.shows("id:training-tab-overview")).toBe(true);
    expect(app.shows("id:training-tab-contests")).toBe(true);
    expect(app.shows("id:training-tab-submissions")).toBe(true);
    expect(app.shows("id:training-tab-editorials")).toBe(true);
    expect(app.shows("id:training-manual-log-btn")).toBe(true);
    // Sync button & last-synced label are hidden when no OJ accounts are configured
    expect(app.shows("id:training-sync-btn")).toBe(false);
    expect(app.shows("id:training-last-synced")).toBe(false);

    // Open ManualLogModal, type a title, and submit
    await app.press("id:training-manual-log-btn");
    expect(app.shows("id:manual-log-modal")).toBe(true);
    await app.type("id:manual-log-problem-input", "My Custom DP Problem");
    await app.press("id:manual-log-save");
    await new Promise((r) => setTimeout(r, 30));

    const payloadText = app.texts().find((s) => s.includes("My Custom DP Problem")) ?? "";
    expect(payloadText.length).toBeGreaterThan(0);
    const parsed = JSON.parse(payloadText) as {
      title: string;
      status: string;
      create_in_library: boolean;
    };
    expect(parsed.title).toBe("My Custom DP Problem");
    expect(parsed.status).toBe("ac");
    expect(parsed.create_in_library).toBe(false);
  });

  test("Editorials tab renders items, filters by search/tag, and opens Studio on editorial tab", async () => {
    const app = await open("./training/testHarness.tsx", {
      width: 960,
      height: 980,
      storage: { mode: "shell" },
    });

    await app.press("id:training-tab-editorials");
    expect(app.shows("id:training-editorials-root")).toBe(true);
    expect(app.shows("id:editorial-item-tree-dp")).toBe(true);
    expect(app.shows("id:editorial-item-graph-flow")).toBe(true);

    // Filter by tag via dropdown
    await app.press("id:editorials-tag-menu");
    await app.press("动态规划 >> nth=2");
    await new Promise((r) => setTimeout(r, 20));
    expect(app.shows("id:editorial-item-tree-dp")).toBe(true);
    expect(app.shows("id:editorial-item-graph-flow")).toBe(false);

    // Clear filters and filter by search
    await app.press("id:editorials-clear-filters");
    expect(app.shows("id:editorial-item-graph-flow")).toBe(true);
    await app.type("id:editorials-search", "最小割");
    await new Promise((r) => setTimeout(r, 20));
    expect(app.shows("id:editorial-item-graph-flow")).toBe(true);
    expect(app.shows("id:editorial-item-tree-dp")).toBe(false);

    // Click Edit button on selected editorial and verify Studio handoff on editorial tab
    await app.press("id:editorial-edit-btn");
    expect(app.texts()).toContain("graph-flow:editorial");
  });

  test("ContestsTab selecting a single platform keeps 全部平台 visible and clickable", async () => {
    const app = await open("./training/testHarness.tsx", {
      width: 960,
      height: 980,
      storage: { mode: "shell" },
    });

    await app.press("id:training-tab-contests");
    expect(app.shows("id:training-contests-scroll")).toBe(true);
    expect(app.shows("id:contests-platform-bar")).toBe(true);
    expect(app.shows("id:contests-platform-all")).toBe(true);
    expect(app.shows("id:contests-platform-codeforces")).toBe(true);

    // Filter to a single platform: platform bar and 全部平台 must remain visible and clickable
    await app.press("id:contests-platform-codeforces");
    expect(app.shows("id:contests-platform-bar")).toBe(true);
    expect(app.shows("id:contests-platform-all")).toBe(true);
    expect(app.texts()).toContain("全部平台");

    // Clicking 全部平台 returns cleanly without trapping the user
    await app.press("id:contests-platform-all");
    expect(app.shows("id:contests-platform-bar")).toBe(true);
    expect(app.shows("id:contests-platform-codeforces")).toBe(true);
  });
});
