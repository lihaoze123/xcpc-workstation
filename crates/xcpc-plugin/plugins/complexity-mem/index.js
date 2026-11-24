"use strict";

const TYPE_SIZES = {
  bool: 1,
  char: 1,
  int16: 2,
  int32: 4,
  int64: 8,
  int128: 16,
  float: 4,
  double: 8,
  long_double: 16,
  pair_int32: 8,
  pair_int64: 16,
};

function formatOps(v) {
  if (!Number.isFinite(v)) return "∞ (溢出)";
  if (v < 1e4) return String(Math.round(v));
  const exp = Math.floor(Math.log10(v));
  const mant = v / Math.pow(10, exp);
  return mant.toFixed(2) + " × 10^" + exp;
}

function judgeVerdict(ops, budget) {
  if (!Number.isFinite(ops) || ops > budget * 3) return "必然 TLE";
  if (ops > budget) return "高风险 TLE";
  if (ops > budget * 0.35) return "卡常 / 需优化";
  return "充裕通过";
}

function runComplexity(inputs) {
  const n = Math.max(1, Number(inputs && inputs.n != null ? inputs.n : 100000));
  const tl = Math.max(0.05, Number(inputs && inputs.time_limit_sec != null ? inputs.time_limit_sec : 1));
  const cf = Math.max(0.1, Number(inputs && inputs.const_factor != null ? inputs.const_factor : 1));
  const budget = 1e8 * tl;
  const log2n = Math.log2(Math.max(2, n));

  const items = [
    ["O(log n)", log2n],
    ["O(√n)", Math.sqrt(n)],
    ["O(n)", n],
    ["O(n log n)", n * log2n],
    ["O(n log² n)", n * log2n * log2n],
    ["O(n √n)", n * Math.sqrt(n)],
    ["O(n²)", n * n],
    ["O(n² log n)", n * n * log2n],
    ["O(n³)", n * n * n],
    ["O(2^n)", n <= 60 ? Math.pow(2, n) : Infinity],
    ["O(n · 2^n)", n <= 55 ? n * Math.pow(2, n) : Infinity],
  ];

  const rows = items.map(([label, baseOps]) => {
    const ops = baseOps * cf;
    const ratio = Number.isFinite(ops) ? ((ops / budget) * 100).toFixed(2) + "%" : "> 9999%";
    return [label, formatOps(ops), ratio, judgeVerdict(ops, budget)];
  });

  return {
    summary: "n = " + n + "，时限 " + tl + "s（基准预算 " + formatOps(budget) + " 次运算，常数因子 " + cf + "）",
    blocks: [
      {
        type: "table",
        title: "时间复杂度可行性评估",
        headers: ["复杂度", "估算运算量", "占预算比例", "判定"],
        align: ["left", "right", "right", "center"],
        rows,
      },
    ],
  };
}

function parseDimensions(raw) {
  const cleaned = String(raw || "1")
    .replace(/[\[\]()]/g, " ")
    .trim();
  const parts = cleaned.split(/[xX×*,\s]+/).filter(Boolean);
  if (parts.length === 0) {
    throw new Error("请输入至少一个有效的正整数维度，例如 200005 或 1000x1000");
  }
  const dims = [];
  for (const p of parts) {
    const num = Number(p);
    if (!Number.isFinite(num) || num <= 0 || Math.floor(num) !== num) {
      throw new Error("无效维度: " + p);
    }
    dims.push(num);
  }
  return dims;
}

function runMemory(inputs) {
  const dataType = String((inputs && inputs.data_type) || "int64").trim();
  const elemSize = TYPE_SIZES[dataType];
  if (!elemSize) {
    throw new Error("不支持的数据类型: " + dataType);
  }
  const dims = parseDimensions(inputs && inputs.dimensions != null ? inputs.dimensions : "1000000");
  const limitMb = Math.max(1, Number(inputs && inputs.limit_mb != null ? inputs.limit_mb : 256));

  let totalElements = 1;
  for (const d of dims) {
    totalElements *= d;
  }
  const bytes = totalElements * elemSize;
  const kib = bytes / 1024;
  const mib = bytes / (1024 * 1024);
  const pct = (mib / limitMb) * 100;
  const status = mib > limitMb ? "超出限制 (MLE)" : mib > limitMb * 0.75 ? "偏高（注意额外开销）" : "安全";

  return {
    summary: dataType + "[" + dims.join("][") + "] = " + mib.toFixed(3) + " MiB (" + pct.toFixed(2) + "% of " + limitMb + " MiB)",
    blocks: [
      {
        type: "table",
        title: "数组内存精确计算",
        headers: ["指标", "数值"],
        align: ["left", "right"],
        rows: [
          ["数据类型", dataType + " (" + elemSize + " bytes)"],
          ["维度", dims.join(" × ")],
          ["总元素数", totalElements.toLocaleString("en-US")],
          ["精确字节数 (B)", bytes.toLocaleString("en-US")],
          ["KiB", kib.toFixed(2)],
          ["MiB", mib.toFixed(4)],
          ["占内存限制 (" + limitMb + " MiB)", pct.toFixed(2) + "%"],
          ["评估", status],
        ],
      },
    ],
  };
}

function runTool(toolId, inputs) {
  if (toolId === "complexity") {
    return runComplexity(inputs || {});
  }
  if (toolId === "memory") {
    return runMemory(inputs || {});
  }
  throw new Error("Unknown tool: " + toolId);
}

module.exports = { runTool };
