function fetchJson(url) {
  const res = host.http.fetch({ url, method: "GET" });
  if (res.status < 200 || res.status >= 300) {
    throw new Error("Codeforces HTTP " + res.status + " for " + url);
  }
  const data = JSON.parse(res.body);
  if (data.status !== "OK") {
    throw new Error("Codeforces API error: " + (data.comment || "unknown"));
  }
  return data.result;
}

function mapVerdict(verdict) {
  switch (verdict) {
    case "OK":
      return "AC";
    case "WRONG_ANSWER":
      return "WA";
    case "TIME_LIMIT_EXCEEDED":
      return "TLE";
    case "MEMORY_LIMIT_EXCEEDED":
      return "MLE";
    case "RUNTIME_ERROR":
      return "RE";
    case "COMPILATION_ERROR":
      return "CE";
    case "PRESENTATION_ERROR":
      return "PE";
    default:
      return "OTHER";
  }
}

function normalizeRating(raw) {
  if (raw == null || typeof raw !== "number" || !Number.isFinite(raw)) {
    return null;
  }
  return Math.round(raw);
}

function verify(ctx) {
  const handle = String((ctx && ctx.handle) || "").trim();
  if (!handle) {
    return { ok: false, message: "handle is required" };
  }
  try {
    const users = fetchJson(
      "https://codeforces.com/api/user.info?handles=" + encodeURIComponent(handle)
    );
    if (!Array.isArray(users) || users.length === 0) {
      return { ok: false, message: "User not found" };
    }
    return { ok: true, handle: users[0].handle };
  } catch (e) {
    return { ok: false, message: String(e && e.message ? e.message : e) };
  }
}

function syncAccount(ctx) {
  const handle = String((ctx && ctx.handle) || "").trim();
  const users = fetchJson(
    "https://codeforces.com/api/user.info?handles=" + encodeURIComponent(handle)
  );
  const user = users[0] || { handle };
  let ratingRows = [];
  try {
    ratingRows = fetchJson(
      "https://codeforces.com/api/user.rating?handle=" + encodeURIComponent(handle)
    );
  } catch (_) {
    ratingRows = [];
  }

  const rating_history = ratingRows.map((row) => ({
    contest_id: String(row.contestId),
    contest_name: String(row.contestName || ""),
    rating_before: Number(row.oldRating || 0),
    rating_after: Number(row.newRating || 0),
    rank: row.rank != null ? Number(row.rank) : null,
    at: Number(row.ratingUpdateTimeSeconds || 0),
  }));

  return {
    handle: user.handle || handle,
    rating: user.rating != null ? Number(user.rating) : null,
    max_rating: user.maxRating != null ? Number(user.maxRating) : null,
    rank: user.rank ? String(user.rank) : null,
    rating_history,
  };
}

const CF_PAGE_SIZE = 1000;
const CF_MAX_PAGES = 100;

function fetchPageWithRetry(url, pageIndex) {
  try {
    return fetchJson(url);
  } catch (firstErr) {
    try {
      return fetchJson(url);
    } catch (secondErr) {
      const detail = String(
        (secondErr && secondErr.message) ||
          (firstErr && firstErr.message) ||
          secondErr ||
          firstErr
      );
      if (pageIndex === 0) {
        throw new Error(detail);
      }
      throw new Error(
        "Codeforces pagination failed on page " + (pageIndex + 1) + ": " + detail
      );
    }
  }
}

function syncSubmissions(ctx, cursor) {
  const handle = String((ctx && ctx.handle) || "").trim();
  const lastSeenId = cursor != null && cursor !== "" ? Number(cursor) : 0;

  let maxId = lastSeenId;
  let minTestingId = null;
  const submissions = [];
  const seenIds = new Set();

  for (let page = 0; page < CF_MAX_PAGES; page++) {
    const from = page * CF_PAGE_SIZE + 1;
    const rows = fetchPageWithRetry(
      "https://codeforces.com/api/user.status?handle=" +
        encodeURIComponent(handle) +
        "&from=" +
        from +
        "&count=" +
        CF_PAGE_SIZE,
      page
    );
    if (!Array.isArray(rows) || rows.length === 0) {
      break;
    }

    let reachedCursor = false;
    for (const row of rows) {
      const id = Number(row.id);
      if (lastSeenId > 0 && id <= lastSeenId) {
        reachedCursor = true;
        continue;
      }
      if (seenIds.has(id)) {
        continue;
      }
      seenIds.add(id);
      if (row.verdict === "TESTING" || row.verdict == null) {
        if (minTestingId == null || id < minTestingId) {
          minTestingId = id;
        }
      } else if (id > maxId) {
        maxId = id;
      }
      const prob = row.problem || {};
      const cid = prob.contestId != null ? String(prob.contestId) : null;
      const idx = prob.index ? String(prob.index) : null;
      const problem_key = cid && idx ? cid + idx : String(prob.name || id);
      let problem_url = null;
      if (cid && idx) {
        const prefix = Number(cid) >= 100000 ? "gym" : "contest";
        problem_url = "https://codeforces.com/" + prefix + "/" + cid + "/problem/" + idx;
      }
      const raw_rating = prob.rating != null ? Number(prob.rating) : null;

      submissions.push({
        submission_id: String(id),
        handle,
        contest_id: cid,
        problem_index: idx,
        problem_key,
        problem_title: String(prob.name || problem_key),
        problem_url,
        raw_rating,
        normalized_rating: normalizeRating(raw_rating),
        tags: Array.isArray(prob.tags) ? prob.tags.map(String) : [],
        verdict: mapVerdict(row.verdict),
        language: row.programmingLanguage ? String(row.programmingLanguage) : null,
        submitted_at: Number(row.creationTimeSeconds || 0),
      });
    }

    if (reachedCursor || rows.length < CF_PAGE_SIZE) {
      break;
    }
  }

  // Never advance the cursor past any still-TESTING submission so its final verdict is picked up
  // on the next incremental sync.
  const safeMaxId =
    minTestingId != null
      ? Math.max(lastSeenId, Math.min(maxId, minTestingId - 1))
      : maxId;

  return {
    submissions,
    next_cursor: safeMaxId > 0 ? String(safeMaxId) : cursor || null,
  };
}

function fetchContests(_ctx) {
  const rows = fetchJson("https://codeforces.com/api/contest.list?gym=false");
  const contests = [];
  for (const row of rows) {
    if (row.phase !== "BEFORE" && row.phase !== "CODING") {
      continue;
    }
    contests.push({
      contest_id: String(row.id),
      name: String(row.name || ""),
      start_time: Number(row.startTimeSeconds || 0),
      duration_sec: Number(row.durationSeconds || 0),
      format: row.type ? String(row.type) : null,
      url: "https://codeforces.com/contest/" + row.id,
    });
  }
  contests.sort((a, b) => a.start_time - b.start_time);
  return contests;
}

function parseProblemUrl(rawUrl) {
  if (typeof rawUrl !== "string") {
    return null;
  }
  const trimmed = rawUrl.trim();
  const patterns = [
    /^https?:\/\/(?:www\.|m\.)?codeforces\.com\/problemset\/problem\/(\d+)\/([A-Za-z0-9]+)(?:[/?#].*)?$/i,
    /^https?:\/\/(?:www\.|m\.)?codeforces\.com\/contest\/(\d+)\/problem\/([A-Za-z0-9]+)(?:[/?#].*)?$/i,
    /^https?:\/\/(?:www\.|m\.)?codeforces\.com\/gym\/(\d+)\/problem\/([A-Za-z0-9]+)(?:[/?#].*)?$/i,
  ];
  for (const re of patterns) {
    const m = trimmed.match(re);
    if (m) {
      const contest_id = m[1];
      const problem_index = m[2].toUpperCase();
      return {
        platform: "codeforces",
        problem_id: contest_id + problem_index,
        contest_id,
        problem_index,
        url: trimmed,
      };
    }
  }
  return null;
}

module.exports = {
  verify,
  syncAccount,
  syncSubmissions,
  fetchContests,
  normalizeRating,
  parseProblemUrl,
};
