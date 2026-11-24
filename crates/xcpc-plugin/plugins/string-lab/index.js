"use strict";

function computePrefixFunction(s) {
  const n = s.length;
  const pi = new Array(n).fill(0);
  for (let i = 1; i < n; i++) {
    let j = pi[i - 1];
    while (j > 0 && s[i] !== s[j]) j = pi[j - 1];
    if (s[i] === s[j]) j++;
    pi[i] = j;
  }
  return pi;
}

function computeZFunction(s) {
  const n = s.length;
  const z = new Array(n).fill(0);
  if (n > 0) z[0] = n;
  let l = 0;
  let r = 0;
  for (let i = 1; i < n; i++) {
    if (i <= r) z[i] = Math.min(r - i + 1, z[i - l]);
    while (i + z[i] < n && s[z[i]] === s[i + z[i]]) z[i]++;
    if (i + z[i] - 1 > r) {
      l = i;
      r = i + z[i] - 1;
    }
  }
  return z;
}

function computeManacher(s) {
  const n = s.length;
  const d1 = new Array(n).fill(0);
  for (let i = 0, l = 0, r = -1; i < n; i++) {
    let k = i > r ? 1 : Math.min(d1[l + r - i], r - i + 1);
    while (i - k >= 0 && i + k < n && s[i - k] === s[i + k]) k++;
    d1[i] = k;
    if (i + k - 1 > r) {
      l = i - k + 1;
      r = i + k - 1;
    }
  }

  const d2 = new Array(n).fill(0);
  for (let i = 0, l = 0, r = -1; i < n; i++) {
    let k = i > r ? 0 : Math.min(d2[l + r - i + 1], r - i + 1);
    while (i - k - 1 >= 0 && i + k < n && s[i - k - 1] === s[i + k]) k++;
    d2[i] = k;
    if (i + k - 1 > r) {
      l = i - k;
      r = i + k - 1;
    }
  }

  let bestLen = 0;
  let bestStart = 0;
  for (let i = 0; i < n; i++) {
    const len1 = d1[i] * 2 - 1;
    if (len1 > bestLen) {
      bestLen = len1;
      bestStart = i - d1[i] + 1;
    }
    const len2 = d2[i] * 2;
    if (len2 > bestLen) {
      bestLen = len2;
      bestStart = i - d2[i];
    }
  }

  return {
    d1,
    d2,
    longest: bestLen > 0 ? s.slice(bestStart, bestStart + bestLen) : "",
    longestStart: bestStart,
    longestLen: bestLen,
  };
}

function computeSuffixArray(s) {
  const n = s.length;
  const sa = Array.from({ length: n }, (_, i) => i);
  sa.sort((a, b) => {
    const saStr = s.slice(a);
    const sbStr = s.slice(b);
    if (saStr < sbStr) return -1;
    if (saStr > sbStr) return 1;
    return 0;
  });

  const rk = new Array(n).fill(0);
  for (let i = 0; i < n; i++) rk[sa[i]] = i;

  const lcp = new Array(n).fill(0);
  let k = 0;
  for (let i = 0; i < n; i++) {
    const r = rk[i];
    if (r === 0) {
      k = 0;
      continue;
    }
    const j = sa[r - 1];
    while (i + k < n && j + k < n && s[i + k] === s[j + k]) k++;
    lcp[r] = k;
    if (k > 0) k--;
  }
  return { sa, rk, lcp };
}

function findPatternMatches(text, pattern) {
  if (!pattern) return [];
  const sep = "\u0000";
  const combined = pattern + sep + text;
  const pi = computePrefixFunction(combined);
  const m = pattern.length;
  const matches = [];
  for (let i = m + 1; i < combined.length; i++) {
    if (pi[i] === m) {
      matches.push(i - 2 * m);
    }
  }
  return matches;
}

function runTool(toolId, inputs) {
  if (toolId !== "analyze") {
    throw new Error("Unknown tool: " + toolId);
  }
  const text = String((inputs && inputs.text) || "");
  if (text.length === 0) {
    throw new Error("目标字符串 S 不能为空");
  }
  if (text.length > 2000) {
    throw new Error("为保证表格可读性，字符串长度上限为 2000");
  }
  const pattern = inputs && inputs.pattern != null ? String(inputs.pattern) : "";

  const pi = computePrefixFunction(text);
  const z = computeZFunction(text);
  const manacher = computeManacher(text);
  const { sa, rk, lcp } = computeSuffixArray(text);
  const matches = pattern ? findPatternMatches(text, pattern) : [];

  const n = text.length;
  const indexHeaders = ["指标"];
  const charRow = ["S[i]"];
  const piRow = ["KMP π[i]"];
  const zRow = ["Z[i]"];
  const d1Row = ["Manacher d1[i]"];
  const d2Row = ["Manacher d2[i]"];
  for (let i = 0; i < n; i++) {
    indexHeaders.push(String(i));
    charRow.push(text[i]);
    piRow.push(String(pi[i]));
    zRow.push(String(z[i]));
    d1Row.push(String(manacher.d1[i]));
    d2Row.push(String(manacher.d2[i]));
  }

  const saRows = [];
  for (let rank = 0; rank < n; rank++) {
    const pos = sa[rank];
    const suffix = text.slice(pos);
    const display = suffix.length > 48 ? suffix.slice(0, 48) + "…" : suffix;
    saRows.push([String(rank), String(pos), String(lcp[rank]), display]);
  }

  const summaryParts = [
    "长度 |S| = " + n,
    "最长回文子串: \"" + manacher.longest + "\" (长度 " + manacher.longestLen + "，起点 " + manacher.longestStart + ")",
  ];
  if (pattern) {
    summaryParts.push(
      matches.length > 0
        ? "模式串 \"" + pattern + "\" 匹配位置 (0-indexed): [" + matches.join(", ") + "]"
        : "模式串 \"" + pattern + "\" 未出现"
    );
  }

  const blocks = [
    {
      type: "text",
      title: "核心摘要",
      content: summaryParts.join("\n"),
      monospace: true,
    },
    {
      type: "table",
      title: "前缀函数 / Z 函数 / Manacher 对齐表",
      headers: indexHeaders,
      rows: [charRow, piRow, zRow, d1Row, d2Row],
    },
    {
      type: "table",
      title: "后缀数组 (SA / RK / LCP)",
      headers: ["RK (排名)", "SA[rk] (起点)", "LCP[rk]", "后缀 S[SA[rk]..]"],
      align: ["right", "right", "right", "left"],
      rows: saRows,
    },
  ];

  // Also include RK vector as compact text for quick lookup
  blocks.push({
    type: "text",
    title: "RK 数组 (按原下标 i)",
    content: "RK = [" + rk.join(", ") + "]",
    monospace: true,
  });

  return {
    summary: summaryParts.join(" · "),
    blocks,
  };
}

module.exports = { runTool };
