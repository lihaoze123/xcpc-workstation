import { describe, expect, test } from "nsh/test";
import { spaceName, windowTitle } from "./logic";
import {
  buildYearHeatmapGrid,
  DEFAULT_SUBMISSIONS_FILTER,
  drilldownFromBucket,
  drilldownFromTag,
  formatCountdown,
  formatDuration,
  hasActiveSubmissionsFilter,
  heatmapCellColor,
  platformDisplayName,
  verdictTone,
  type DifficultyBucket,
} from "./training/types";
import { goSpace, info, space } from "./store";

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

  test("time & platform formatting helpers behave cleanly", () => {
    expect(platformDisplayName("codeforces")).toBe("Codeforces");
    expect(platformDisplayName("atcoder")).toBe("AtCoder");
    expect(formatDuration(7200)).toBe("2h");
    expect(formatDuration(5400)).toBe("1h 30m");
    expect(formatDuration(2700)).toBe("45m");
    expect(formatCountdown(100_000, 10_000)).toBe("1d 1h");
    expect(formatCountdown(13_600, 10_000)).toBe("1h 0m");
    expect(verdictTone("AC")).toBe("ac");
    expect(verdictTone("WA")).toBe("wa");
    expect(verdictTone("TLE")).toBe("tle");
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
