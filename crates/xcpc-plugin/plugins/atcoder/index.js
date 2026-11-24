function fetchJson(url) {
  const res = host.http.fetch({ url, method: "GET" });
  if (res.status < 200 || res.status >= 300) {
    throw new Error("AtCoder HTTP " + res.status + " for " + url);
  }
  return JSON.parse(res.body);
}

function mapVerdict(result) {
  switch (result) {
    case "AC":
      return "AC";
    case "WA":
      return "WA";
    case "TLE":
      return "TLE";
    case "MLE":
      return "MLE";
    case "RE":
      return "RE";
    case "CE":
      return "CE";
    default:
      return "OTHER";
  }
}

function atcoderRank(rating) {
  if (rating == null) return null;
  if (rating < 400) return "gray";
  if (rating < 800) return "brown";
  if (rating < 1200) return "green";
  if (rating < 1600) return "cyan";
  if (rating < 2000) return "blue";
  if (rating < 2400) return "yellow";
  if (rating < 2800) return "orange";
  return "red";
}

function normalizeRating(raw) {
  if (raw == null || typeof raw !== "number" || !Number.isFinite(raw)) {
    return null;
  }
  // Apply standard AtCoder positive-difficulty smoothing below 400, then map to CF-equivalent scale.
  const effective = raw < 400 ? 400 / Math.exp((400 - raw) / 400) : raw;
  return Math.round(effective * 0.8 + 800);
}

function verify(ctx) {
  const handle = String((ctx && ctx.handle) || "").trim();
  if (!handle) {
    return { ok: false, message: "handle is required" };
  }
  try {
    const res = host.http.fetch({
      url: "https://atcoder.jp/users/" + encodeURIComponent(handle) + "/history/json",
      method: "GET",
    });
    if (res.status !== 200) {
      return { ok: false, message: "AtCoder HTTP " + res.status };
    }
    const data = JSON.parse(res.body);
    if (!Array.isArray(data)) {
      return { ok: false, message: "Unexpected history response" };
    }
    return { ok: true, handle };
  } catch (e) {
    return { ok: false, message: String(e && e.message ? e.message : e) };
  }
}

function syncAccount(ctx) {
  const handle = String((ctx && ctx.handle) || "").trim();
  const rows = fetchJson(
    "https://atcoder.jp/users/" + encodeURIComponent(handle) + "/history/json"
  );

  const rating_history = [];
  let currentRating = null;
  let maxRating = null;

  for (const row of rows) {
    // Algorithm rating only: skip Heuristic contests (ContestType === 1 or ahc prefix).
    if (row.ContestType === 1) {
      continue;
    }
    const screenName = String(row.ContestScreenName || "");
    if (/^ahc\d+/i.test(screenName)) {
      continue;
    }
    if (!row.IsRated) {
      continue;
    }
    const newRating = Number(row.NewRating || 0);
    const oldRating = Number(row.OldRating || 0);
    const contestId = screenName.split(".")[0] || String(row.ContestName || "");
    const endMs = Date.parse(String(row.EndTime || ""));
    const at = Number.isFinite(endMs) ? Math.floor(endMs / 1000) : 0;

    currentRating = newRating;
    if (maxRating == null || newRating > maxRating) {
      maxRating = newRating;
    }

    rating_history.push({
      contest_id: contestId,
      contest_name: String(row.ContestName || contestId),
      rating_before: oldRating,
      rating_after: newRating,
      rank: row.Place != null ? Number(row.Place) : null,
      at,
    });
  }

  return {
    handle,
    rating: currentRating,
    max_rating: maxRating,
    rank: atcoderRank(currentRating),
    rating_history,
  };
}

function syncSubmissions(ctx, cursor) {
  const handle = String((ctx && ctx.handle) || "").trim();
  const fromSecond = cursor != null && cursor !== "" ? Number(cursor) : 0;

  const rows = fetchJson(
    "https://kenkoooo.com/atcoder/atcoder-api/v3/user/submissions?user=" +
      encodeURIComponent(handle) +
      "&from_second=" +
      fromSecond
  );

  let problemsMap = {};
  try {
    const problems = fetchJson("https://kenkoooo.com/atcoder/resources/problems.json");
    if (Array.isArray(problems)) {
      for (const p of problems) {
        problemsMap[p.id] = p;
      }
    }
  } catch (_) {}

  let modelsMap = {};
  try {
    const models = fetchJson("https://kenkoooo.com/atcoder/resources/problem-models.json");
    if (models && typeof models === "object") {
      modelsMap = models;
    }
  } catch (_) {}

  let maxEpoch = fromSecond;
  const submissions = [];
  for (const row of rows) {
    const epoch = Number(row.epoch_second || 0);
    if (epoch > maxEpoch) {
      maxEpoch = epoch;
    }
    const pid = String(row.problem_id || "");
    const cid = row.contest_id ? String(row.contest_id) : null;
    const pmeta = problemsMap[pid] || {};
    const pmodel = modelsMap[pid] || {};
    const raw_rating =
      typeof pmodel.difficulty === "number" && Number.isFinite(pmodel.difficulty)
        ? pmodel.difficulty
        : null;

    submissions.push({
      submission_id: String(row.id),
      handle,
      contest_id: cid,
      problem_index: pmeta.problem_index ? String(pmeta.problem_index) : null,
      problem_key: pid,
      problem_title: String(pmeta.title || pmeta.name || pid),
      problem_url: cid
        ? "https://atcoder.jp/contests/" + cid + "/tasks/" + pid
        : null,
      raw_rating,
      normalized_rating: normalizeRating(raw_rating),
      tags: [],
      verdict: mapVerdict(row.result),
      language: row.language ? String(row.language) : null,
      submitted_at: epoch,
    });
  }

  const next_cursor = rows.length > 0 ? String(maxEpoch + 1) : cursor || null;
  return { submissions, next_cursor };
}

function parseDurationToSeconds(text) {
  const m = String(text || "").trim().match(/^(\d+):(\d+)$/);
  if (!m) return 0;
  return Number(m[1]) * 3600 + Number(m[2]) * 60;
}

function fetchContests(_ctx) {
  // Prefer official AtCoder contests page via host.html.select; fallback to Kenkoooo contests.json.
  try {
    const res = host.http.fetch({ url: "https://atcoder.jp/contests/", method: "GET" });
    if (res.status === 200 && res.body) {
      const rows = host.html.select(
        res.body,
        "#contest-table-upcoming tbody tr, #contest-table-action tbody tr"
      );
      if (rows.length > 0) {
        const contests = [];
        for (const row of rows) {
          const cells = host.html.select("<table><tr>" + row.html + "</tr></table>", "td");
          if (cells.length < 3) continue;
          const timeText = cells[0].text.trim();
          const links = host.html.select(cells[1].html, "a");
          if (links.length === 0) continue;
          const href = links[0].attrs.href || "";
          const name = links[0].text.trim();
          const cidMatch = href.match(/\/contests\/([^/?#]+)/);
          const contest_id = cidMatch ? cidMatch[1] : name;
          const parsedMs = Date.parse(timeText.replace(" ", "T"));
          const start_time = Number.isFinite(parsedMs) ? Math.floor(parsedMs / 1000) : 0;
          const duration_sec = parseDurationToSeconds(cells[2].text);
          contests.push({
            contest_id,
            name,
            start_time,
            duration_sec,
            format: "AtCoder",
            url: href.startsWith("http") ? href : "https://atcoder.jp" + href,
          });
        }
        if (contests.length > 0) {
          contests.sort((a, b) => a.start_time - b.start_time);
          return contests;
        }
      }
    }
  } catch (_) {}

  const list = fetchJson("https://kenkoooo.com/atcoder/resources/contests.json");
  const nowSec = Math.floor(Date.now() / 1000);
  const upcoming = [];
  for (const c of list) {
    const start = Number(c.start_epoch_second || 0);
    const dur = Number(c.duration_second || 0);
    if (start + dur >= nowSec) {
      upcoming.push({
        contest_id: String(c.id),
        name: String(c.title || c.id),
        start_time: start,
        duration_sec: dur,
        format: "AtCoder",
        url: "https://atcoder.jp/contests/" + c.id,
      });
    }
  }
  upcoming.sort((a, b) => a.start_time - b.start_time);
  return upcoming;
}

function parseProblemUrl(rawUrl) {
  if (typeof rawUrl !== "string") {
    return null;
  }
  const trimmed = rawUrl.trim();
  const m = trimmed.match(
    /^https?:\/\/atcoder\.jp\/contests\/([^/?#]+)\/tasks\/([^/?#]+)(?:[/?#].*)?$/i
  );
  if (!m) {
    return null;
  }
  const contest_id = m[1];
  const problem_id = m[2];
  const suffixMatch = problem_id.match(/_([A-Za-z0-9]+)$/);
  const problem_index = suffixMatch ? suffixMatch[1].toUpperCase() : null;
  return {
    platform: "atcoder",
    problem_id,
    contest_id,
    problem_index,
    url: trimmed,
  };
}

module.exports = {
  verify,
  syncAccount,
  syncSubmissions,
  fetchContests,
  normalizeRating,
  parseProblemUrl,
};
