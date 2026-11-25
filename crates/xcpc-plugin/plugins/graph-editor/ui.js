"use strict";

function render(sdk) {
  const { h, signal, effect, untracked, ui, token, colorScheme } = sdk;
  const { View, Text, Canvas, Button, Segmented, Chip, Input, If } = ui;
  const runUntracked = typeof untracked === "function" ? untracked : (fn) => fn();

  const NODE_R = 18;
  const isDark = () => Boolean(colorScheme && colorScheme() === "dark");
  const PALETTE = [
    { id: null, name: "默认" },
    { id: "cyan", name: "青蓝", light: "#E0F2FE", dark: "#164E63", lightStroke: "#0284C7", darkStroke: "#38BDF8" },
    { id: "green", name: "绿", light: "#DCFCE7", dark: "#14532D", lightStroke: "#15803D", darkStroke: "#4ADE80" },
    { id: "amber", name: "橙", light: "#FEF3C7", dark: "#78350F", lightStroke: "#B45309", darkStroke: "#FBBF24" },
    { id: "red", name: "红", light: "#FEE2E2", dark: "#7F1D1D", lightStroke: "#B91C1C", darkStroke: "#F87171" },
  ];

  function findPaletteEntry(colorKey) {
    if (!colorKey) return null;
    return PALETTE.find((p) => p.id === colorKey || p.light === colorKey || p.dark === colorKey) || null;
  }

  function resolveNodeFill(colorKey, dark, fallback) {
    if (!colorKey) return fallback;
    const entry = findPaletteEntry(colorKey);
    if (entry && entry.id) return dark ? entry.dark : entry.light;
    return colorKey;
  }

  function resolveNodeStroke(colorKey, dark, isSel, fallback) {
    if (isSel) return token("primary");
    const entry = findPaletteEntry(colorKey);
    if (entry && entry.id) return dark ? entry.darkStroke : entry.lightStroke;
    return fallback;
  }

  function resolveNodeText(colorKey, dark, fallback) {
    if (!colorKey) return fallback;
    const entry = findPaletteEntry(colorKey);
    if (entry && entry.id) return dark ? "#FAFAFA" : "#09090B";
    return "#09090B";
  }

  const SAMPLES = {
    tree: {
      format: "parent_array",
      directed: false,
      oneIndexed: true,
      layout: "tree",
      text: "1 1 2 2 3 3",
    },
    weighted: {
      format: "edge_list",
      directed: true,
      oneIndexed: true,
      layout: "force",
      text: "1 2 5\n1 3 2\n2 4 1\n2 5 4\n3 5 3\n4 5 7",
    },
    geometry: {
      format: "coords",
      directed: false,
      oneIndexed: true,
      layout: "coords",
      text: "1 0 0\n2 4 0\n3 2 3\n4 6 3\nedge 1 2\nedge 2 3\nedge 3 1\nedge 2 4\nedge 3 4",
    },
  };

  const FORMAT_ITEMS = [
    { value: "edge_list", label: "边表", testID: "graph-format-edge_list" },
    { value: "parent_array", label: "父数组", testID: "graph-format-parent_array" },
    { value: "adj_list", label: "邻接表", testID: "graph-format-adj_list" },
    { value: "coords", label: "点坐标", testID: "graph-format-coords" },
  ];

  const INDEX_ITEMS = [
    { value: "1", label: "从 1 开始", testID: "graph-index-1" },
    { value: "0", label: "从 0 开始", testID: "graph-index-0" },
  ];

  const DIR_ITEMS = [
    { value: "undirected", label: "无向图", testID: "graph-dir-undirected" },
    { value: "directed", label: "有向图", testID: "graph-dir-directed" },
  ];

  const LAYOUT_ITEMS = [
    { value: "force", label: "力导向", testID: "graph-layout-force" },
    { value: "tree", label: "树", testID: "graph-layout-tree" },
    { value: "circle", label: "圆形", testID: "graph-layout-circle" },
    { value: "coords", label: "按坐标", testID: "graph-layout-coords" },
  ];

  const format = signal("edge_list"); // "edge_list" | "parent_array" | "adj_list" | "coords"
  const oneIndexed = signal(true);
  const directed = signal(false);
  const layoutMode = signal("force"); // "force" | "tree" | "circle" | "coords"
  const editorText = signal("1 2 5\n1 3 2\n2 4 1\n2 5 4\n3 5 3");
  const nodes = signal([]);
  const edges = signal([]);
  const selectedId = signal(null);
  const connectMode = signal(false);
  const connectFrom = signal(null);
  const canvasSize = signal({ width: 420, height: 420 });

  let draggingId = null;
  let dragMoved = false;
  let suppressTextParse = false;
  let hasUserDragged = false;
  let hasMeasuredCanvas = false;
  let lastAppliedPayload = null;

  function escapeXml(str) {
    return String(str)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  function clampNodesToBounds(list, w, h) {
    const pad = Math.min(48, Math.max(NODE_R + 14, Math.min(w, h) * 0.12));
    const minX = pad;
    const maxX = Math.max(pad, w - pad);
    const minY = pad;
    const maxY = Math.max(pad, h - pad);
    return list.map((nd) => ({
      ...nd,
      x: Math.max(minX, Math.min(maxX, Number.isFinite(nd.x) ? nd.x : w / 2)),
      y: Math.max(minY, Math.min(maxY, Number.isFinite(nd.y) ? nd.y : h / 2)),
    }));
  }

  function layoutNodes(rawNodes, rawEdges, mode, w, h) {
    const list = rawNodes.map((n) => ({ ...n }));
    const n = list.length;
    if (n === 0) return list;
    const pad = Math.min(52, Math.max(NODE_R + 18, Math.min(w, h) * 0.14));
    const cx = w / 2;
    const cy = h / 2;
    const radius = Math.max(36, Math.min(w, h) / 2 - pad);

    if (mode === "coords" && list.some((nd) => nd.x != null && nd.y != null)) {
      let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
      for (const nd of list) {
        if (nd.x != null && nd.y != null) {
          minX = Math.min(minX, nd.x);
          maxX = Math.max(maxX, nd.x);
          minY = Math.min(minY, nd.y);
          maxY = Math.max(maxY, nd.y);
        }
      }
      const sx = Math.max(1e-6, maxX - minX);
      const sy = Math.max(1e-6, maxY - minY);
      const usableW = Math.max(1, w - 2 * pad);
      const usableH = Math.max(1, h - 2 * pad);
      for (let i = 0; i < n; i++) {
        if (list[i].x == null || list[i].y == null) {
          const a = (2 * Math.PI * i) / n - Math.PI / 2;
          list[i].x = cx + radius * Math.cos(a);
          list[i].y = cy + radius * Math.sin(a);
        } else {
          list[i].x = pad + ((list[i].x - minX) / sx) * usableW;
          list[i].y = h - pad - ((list[i].y - minY) / sy) * usableH;
        }
      }
      return clampNodesToBounds(list, w, h);
    }

    if (mode === "tree") {
      const adj = new Map();
      for (const nd of list) adj.set(nd.id, []);
      for (const e of rawEdges) {
        if (adj.has(e.from)) adj.get(e.from).push(e.to);
        if (adj.has(e.to)) adj.get(e.to).push(e.from);
      }
      const depth = new Map();
      const levels = [];
      for (const nd of list) {
        if (depth.has(nd.id)) continue;
        const q = [nd.id];
        depth.set(nd.id, 0);
        while (q.length > 0) {
          const u = q.shift();
          const d = depth.get(u);
          if (!levels[d]) levels[d] = [];
          levels[d].push(u);
          for (const v of adj.get(u) || []) {
            if (!depth.has(v)) {
              depth.set(v, d + 1);
              q.push(v);
            }
          }
        }
      }
      const pos = new Map();
      const maxD = Math.max(1, levels.length - 1);
      for (let d = 0; d < levels.length; d++) {
        const row = levels[d];
        const y = levels.length === 1 ? cy : pad + (d / maxD) * (h - 2 * pad);
        for (let i = 0; i < row.length; i++) {
          const x = row.length === 1 ? cx : pad + ((i + 0.5) / row.length) * (w - 2 * pad);
          pos.set(row[i], { x, y });
        }
      }
      for (const nd of list) {
        const p = pos.get(nd.id) || { x: cx, y: cy };
        nd.x = p.x;
        nd.y = p.y;
      }
      return clampNodesToBounds(list, w, h);
    }

    for (let i = 0; i < n; i++) {
      const a = (2 * Math.PI * i) / n - Math.PI / 2;
      list[i].x = cx + radius * Math.cos(a);
      list[i].y = cy + radius * Math.sin(a);
    }

    if (mode === "circle" || n <= 2) {
      return clampNodesToBounds(list, w, h);
    }

    const idx = new Map();
    list.forEach((nd, i) => idx.set(nd.id, i));
    const usableW = Math.max(80, w - 2 * pad);
    const usableH = Math.max(80, h - 2 * pad);
    const area = usableW * usableH;
    const k = Math.sqrt(area / Math.max(1, n)) * 0.85;
    let temp = Math.min(usableW, usableH) / 6;

    for (let iter = 0; iter < 55; iter++) {
      const dx = new Array(n).fill(0);
      const dy = new Array(n).fill(0);
      for (let i = 0; i < n; i++) {
        for (let j = i + 1; j < n; j++) {
          const vx = list[i].x - list[j].x;
          const vy = list[i].y - list[j].y;
          const dist = Math.hypot(vx, vy) || 0.01;
          const rep = (k * k) / dist;
          dx[i] += (vx / dist) * rep;
          dy[i] += (vy / dist) * rep;
          dx[j] -= (vx / dist) * rep;
          dy[j] -= (vy / dist) * rep;
        }
      }
      for (const e of rawEdges) {
        const u = idx.get(e.from);
        const v = idx.get(e.to);
        if (u == null || v == null || u === v) continue;
        const vx = list[u].x - list[v].x;
        const vy = list[u].y - list[v].y;
        const dist = Math.hypot(vx, vy) || 0.01;
        const attr = (dist * dist) / k;
        dx[u] -= (vx / dist) * attr;
        dy[u] -= (vy / dist) * attr;
        dx[v] += (vx / dist) * attr;
        dy[v] += (vy / dist) * attr;
      }
      for (let i = 0; i < n; i++) {
        const disp = Math.hypot(dx[i], dy[i]) || 0.01;
        const step = Math.min(disp, temp);
        list[i].x = Math.max(pad, Math.min(w - pad, list[i].x + (dx[i] / disp) * step));
        list[i].y = Math.max(pad, Math.min(h - pad, list[i].y + (dy[i] / disp) * step));
      }
      temp *= 0.93;
    }

    // Re-center bounding box inside viewport with generous padding
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    for (const nd of list) {
      minX = Math.min(minX, nd.x);
      maxX = Math.max(maxX, nd.x);
      minY = Math.min(minY, nd.y);
      maxY = Math.max(maxY, nd.y);
    }
    const spanX = maxX - minX;
    const spanY = maxY - minY;
    if (spanX > 1 && spanY > 1) {
      const scale = Math.min(1, usableW / spanX, usableH / spanY);
      const midX = (minX + maxX) / 2;
      const midY = (minY + maxY) / 2;
      for (const nd of list) {
        nd.x = cx + (nd.x - midX) * scale;
        nd.y = cy + (nd.y - midY) * scale;
      }
    }

    return clampNodesToBounds(list, w, h);
  }

  function parseTextToGraph(fmt, text, keepPositions) {
    const prevPos = new Map();
    if (keepPositions) {
      for (const nd of nodes()) prevPos.set(nd.id, { x: nd.x, y: nd.y, color: nd.color });
    }
    const lines = String(text || "")
      .split(/\r?\n/)
      .map((l) => l.trim())
      .filter((l) => l.length > 0 && !l.startsWith("#") && !l.startsWith("//"));

    const nodeMap = new Map();
    const parsedEdges = [];

    function ensure(id, extra) {
      const k = String(id);
      if (!nodeMap.has(k)) {
        const old = prevPos.get(k);
        nodeMap.set(k, {
          id: k,
          label: k,
          x: extra && extra.x != null ? extra.x : old ? old.x : null,
          y: extra && extra.y != null ? extra.y : old ? old.y : null,
          color: (extra && extra.color) || (old && old.color) || null,
        });
      } else if (extra) {
        const cur = nodeMap.get(k);
        if (extra.x != null) cur.x = extra.x;
        if (extra.y != null) cur.y = extra.y;
      }
    }

    const startIdx = oneIndexed() ? 1 : 0;
    if (fmt === "parent_array") {
      const toks = lines.join(" ").split(/[\s,]+/).filter(Boolean);
      if (toks.length > 0) {
        const firstVal = Number(toks[0]);
        const hasRoot = firstVal === 0 || firstVal === -1 || (!oneIndexed() && firstVal < 0);
        if (hasRoot) {
          for (let i = 0; i < toks.length; i++) {
            const u = String(startIdx + i);
            ensure(u);
            if (Number(toks[i]) >= startIdx) {
              ensure(toks[i]);
              parsedEdges.push({ from: String(toks[i]), to: u, weight: null });
            }
          }
        } else {
          ensure(String(startIdx));
          for (let i = 0; i < toks.length; i++) {
            const u = String(startIdx + i + 1);
            ensure(toks[i]);
            ensure(u);
            parsedEdges.push({ from: String(toks[i]), to: u, weight: null });
          }
        }
      }
    } else if (fmt === "adj_list") {
      for (const line of lines) {
        const c = line.indexOf(":");
        if (c < 0) continue;
        const u = line.slice(0, c).trim();
        if (!u) continue;
        ensure(u);
        for (const item of line.slice(c + 1).trim().split(/[\s,]+/).filter(Boolean)) {
          const m = item.match(/^([^\(\)]+)(?:\(([^\)]+)\))?$/);
          if (m) {
            ensure(m[1]);
            parsedEdges.push({ from: u, to: m[1], weight: m[2] || null });
          }
        }
      }
    } else if (fmt === "coords") {
      let autoId = startIdx;
      for (const line of lines) {
        const p = line.split(/[\s,]+/).filter(Boolean);
        if (p[0] === "e" || p[0] === "edge") {
          if (p.length >= 3) {
            ensure(p[1]);
            ensure(p[2]);
            parsedEdges.push({ from: p[1], to: p[2], weight: p[3] || null });
          }
        } else if (p.length >= 3 && Number.isFinite(Number(p[1])) && Number.isFinite(Number(p[2]))) {
          ensure(p[0], { x: Number(p[1]), y: Number(p[2]) });
        } else if (p.length === 2 && Number.isFinite(Number(p[0])) && Number.isFinite(Number(p[1]))) {
          ensure(String(autoId++), { x: Number(p[0]), y: Number(p[1]) });
        }
      }
    } else {
      for (const line of lines) {
        const p = line.split(/[\s,]+/).filter(Boolean);
        if (p.length === 1) ensure(p[0]);
        else if (p.length >= 2) {
          ensure(p[0]);
          ensure(p[1]);
          parsedEdges.push({ from: p[0], to: p[1], weight: p[2] || null });
        }
      }
    }

    const sz = canvasSize();
    const rawList = [...nodeMap.values()];
    const needsLayout = !keepPositions || rawList.some((nd) => nd.x == null || nd.y == null);
    const placed = needsLayout
      ? layoutNodes(rawList, parsedEdges, layoutMode(), sz.width, sz.height)
      : clampNodesToBounds(rawList, sz.width, sz.height);
    nodes.set(placed);
    edges.set(parsedEdges);
  }

  function serializeGraphToText(curNodes, curEdges) {
    let fmt = format();
    if (fmt === "parent_array") {
      fmt = "edge_list";
      format.set("edge_list");
    }
    const lines = [];
    if (fmt === "coords") {
      const hVal = canvasSize().height;
      for (const nd of curNodes) {
        lines.push(nd.id + " " + Math.round(nd.x) + " " + Math.round(hVal - nd.y));
      }
      for (const e of curEdges) {
        lines.push("edge " + e.from + " " + e.to + (e.weight ? " " + e.weight : ""));
      }
    } else if (fmt === "adj_list") {
      const adj = new Map();
      for (const nd of curNodes) adj.set(nd.id, []);
      for (const e of curEdges) {
        if (!adj.has(e.from)) adj.set(e.from, []);
        adj.get(e.from).push(e.weight ? e.to + "(" + e.weight + ")" : e.to);
      }
      for (const [u, vs] of adj.entries()) {
        lines.push(u + ": " + vs.join(" "));
      }
    } else {
      const touched = new Set();
      for (const e of curEdges) {
        touched.add(e.from);
        touched.add(e.to);
        lines.push(e.from + " " + e.to + (e.weight ? " " + e.weight : ""));
      }
      for (const nd of curNodes) {
        if (!touched.has(nd.id)) lines.push(nd.id);
      }
    }
    suppressTextParse = true;
    editorText.set(lines.join("\n"));
    suppressTextParse = false;
  }

  function applyInitialPayload() {
    const payload = typeof sdk.initialPayload === "function" ? sdk.initialPayload() : sdk.initialPayload;
    if (!payload || !Array.isArray(payload.nodes) || payload === lastAppliedPayload) return;
    lastAppliedPayload = payload;
    runUntracked(() => {
      if (payload.directed != null) directed.set(Boolean(payload.directed));
      if (payload.one_indexed != null) oneIndexed.set(Boolean(payload.one_indexed));
      if (payload.layout) layoutMode.set(String(payload.layout));
      const sz = canvasSize();
      const incomingNodes = payload.nodes.map((n) => ({
        id: String(n.id),
        label: String(n.label || n.id),
        x: n.x != null ? Number(n.x) : null,
        y: n.y != null ? Number(n.y) : null,
        color: n.color || null,
      }));
      const incomingEdges = (payload.edges || []).map((e) => ({
        from: String(e.from),
        to: String(e.to),
        weight: e.weight != null ? String(e.weight) : e.label != null ? String(e.label) : null,
      }));
      const placed = incomingNodes.some((n) => n.x == null || n.y == null)
        ? layoutNodes(incomingNodes, incomingEdges, layoutMode(), sz.width, sz.height)
        : clampNodesToBounds(incomingNodes, sz.width, sz.height);
      nodes.set(placed);
      edges.set(incomingEdges);
      serializeGraphToText(placed, incomingEdges);
    });
  }

  // Initial load
  parseTextToGraph(format(), editorText(), false);
  if (effect) {
    effect(() => {
      applyInitialPayload();
    });
  } else {
    applyInitialPayload();
  }

  function hitNode(x, y) {
    const list = nodes();
    for (let i = list.length - 1; i >= 0; i--) {
      if (Math.hypot(list[i].x - x, list[i].y - y) <= NODE_R + 4) {
        return list[i];
      }
    }
    return null;
  }

  function nextNodeId() {
    const existing = new Set(nodes().map((n) => n.id));
    let cur = oneIndexed() ? 1 : 0;
    while (existing.has(String(cur))) cur++;
    return String(cur);
  }

  function applyLayout(mode) {
    layoutMode.set(mode);
    hasUserDragged = false;
    const sz = canvasSize();
    const next = layoutNodes(nodes(), edges(), mode, sz.width, sz.height);
    nodes.set(next);
    if (format() === "coords") {
      serializeGraphToText(next, edges());
    }
  }

  function buildCurrentSvg() {
    const sz = canvasSize();
    const w = Math.max(320, Math.round(sz.width));
    const h = Math.max(240, Math.round(sz.height));
    const list = nodes();
    const edList = edges();
    const isDir = directed();
    const byId = new Map(list.map((n) => [n.id, n]));
    const strokeCol = "#52525b";
    const textCol = "#09090b";
    const parts = [
      '<svg xmlns="http://www.w3.org/2000/svg" width="' + w + '" height="' + h + '" viewBox="0 0 ' + w + " " + h + '">',
      '<rect width="100%" height="100%" fill="#ffffff"/>',
    ];
    if (isDir) {
      parts.push(
        '<defs><marker id="arr" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M 0 1 L 10 5 L 0 9 z" fill="' + strokeCol + '"/></marker></defs>'
      );
    }
    for (const e of edList) {
      const u = byId.get(e.from);
      const v = byId.get(e.to);
      if (!u || !v) continue;
      const dx = v.x - u.x;
      const dy = v.y - u.y;
      const d = Math.hypot(dx, dy) || 1;
      const x1 = u.x + (dx / d) * NODE_R;
      const y1 = u.y + (dy / d) * NODE_R;
      const x2 = v.x - (dx / d) * (isDir ? NODE_R + 4 : NODE_R);
      const y2 = v.y - (dy / d) * (isDir ? NODE_R + 4 : NODE_R);
      parts.push(
        '<line x1="' + x1.toFixed(1) + '" y1="' + y1.toFixed(1) + '" x2="' + x2.toFixed(1) + '" y2="' + y2.toFixed(1) + '" stroke="' + strokeCol + '" stroke-width="2"' + (isDir ? ' marker-end="url(#arr)"' : "") + "/>"
      );
      if (e.weight) {
        parts.push(
          '<text x="' + ((x1 + x2) / 2).toFixed(1) + '" y="' + ((y1 + y2) / 2 - 6).toFixed(1) + '" text-anchor="middle" font-family="monospace" font-size="12" fill="' + textCol + '">' + escapeXml(e.weight) + "</text>"
        );
      }
    }
    for (const nd of list) {
      const fillCol = resolveNodeFill(nd.color, false, "#f4f4f5");
      parts.push(
        '<circle cx="' + nd.x.toFixed(1) + '" cy="' + nd.y.toFixed(1) + '" r="' + NODE_R + '" fill="' + escapeXml(fillCol) + '" stroke="' + strokeCol + '" stroke-width="2"/>'
      );
      parts.push(
        '<text x="' + nd.x.toFixed(1) + '" y="' + (nd.y + 4).toFixed(1) + '" text-anchor="middle" font-family="monospace" font-size="13" font-weight="600" fill="' + textCol + '">' + escapeXml(nd.label || nd.id) + "</text>"
      );
    }
    parts.push("</svg>");
    return parts.join("");
  }

  async function handleExportSvg() {
    try {
      const svg = buildCurrentSvg();
      if (sdk.saveText) {
        const ok = await sdk.saveText(svg, "graph.svg");
        if (ok && sdk.toast) sdk.toast("已导出 SVG");
      } else if (sdk.copyText) {
        await sdk.copyText(svg);
        if (sdk.toast) sdk.toast("已复制 SVG 源码");
      }
    } catch (err) {
      if (sdk.toast) {
        sdk.toast("导出 SVG 失败：" + (err && err.message ? err.message : String(err)), "issues");
      }
    }
  }

  async function handleExportPng() {
    if (!sdk.renderSvgToPng || !sdk.savePng) return;
    try {
      const svg = buildCurrentSvg();
      const b64 = await sdk.renderSvgToPng(svg, 2.0);
      const ok = await sdk.savePng(b64, "graph.png");
      if (ok && sdk.toast) sdk.toast("已导出 PNG");
    } catch (err) {
      if (sdk.toast) {
        sdk.toast("导出 PNG 失败：" + (err && err.message ? err.message : String(err)), "issues");
      }
    }
  }

  function applySample(key) {
    const s = SAMPLES[key];
    if (!s) return;
    hasUserDragged = false;
    format.set(s.format);
    directed.set(s.directed);
    oneIndexed.set(s.oneIndexed);
    layoutMode.set(s.layout);
    editorText.set(s.text);
    selectedId.set(null);
    parseTextToGraph(s.format, s.text, false);
  }

  function recolorSelected(color) {
    const sid = selectedId();
    if (!sid) return;
    nodes.update((list) => list.map((n) => (n.id === sid ? { ...n, color } : n)));
  }

  function deleteSelected() {
    const sid = selectedId();
    if (!sid) return;
    const nextNodes = nodes().filter((n) => n.id !== sid);
    const nextEdges = edges().filter((e) => e.from !== sid && e.to !== sid);
    nodes.set(nextNodes);
    edges.set(nextEdges);
    selectedId.set(null);
    serializeGraphToText(nextNodes, nextEdges);
  }

  return h(
    View,
    { style: { flex: 1, flexDirection: "row", minHeight: 0, backgroundColor: token("background") } },
    // Left pane: text editor & controls (width 271 - 1 border - 24 padding = 246px inner;
    // Segmented track chrome is 6px -> 240px items width = 4×60 or 2×120, all edges flush)
    h(
      View,
      {
        style: {
          width: 271,
          flexShrink: 0,
          borderRightWidth: 1,
          borderColor: token("border"),
          padding: 12,
          gap: 10,
          backgroundColor: token("card"),
        },
      },
      // Format selector
      h(
        View,
        { style: { gap: 4 } },
        h(Text, { style: { fontSize: 11.5, color: token("muted-foreground") } }, "输入格式"),
        h(Segmented, {
          items: FORMAT_ITEMS,
          value: () => format(),
          itemWidth: 60,
          onChange: (id) => {
            format.set(id);
            parseTextToGraph(id, editorText(), false);
          },
        })
      ),
      // Index & Direction selectors (uniform full-width 246px)
      h(
        View,
        { style: { gap: 6 } },
        h(Segmented, {
          items: INDEX_ITEMS,
          value: () => (oneIndexed() ? "1" : "0"),
          itemWidth: 120,
          onChange: (v) => {
            oneIndexed.set(v === "1");
            parseTextToGraph(format(), editorText(), false);
          },
        }),
        h(Segmented, {
          items: DIR_ITEMS,
          value: () => (directed() ? "directed" : "undirected"),
          itemWidth: 120,
          onChange: (v) => directed.set(v === "directed"),
        })
      ),
      // Layout selector
      h(
        View,
        { style: { gap: 4 } },
        h(Text, { style: { fontSize: 11.5, color: token("muted-foreground") } }, "布局方式"),
        h(Segmented, {
          items: LAYOUT_ITEMS,
          value: () => layoutMode(),
          itemWidth: 60,
          onChange: (id) => applyLayout(id),
        })
      ),
      // Presets & connect mode
      h(
        View,
        { style: { flexDirection: "row", flexWrap: "wrap", gap: 6, alignItems: "center" } },
        h(Button, { testID: "graph-sample-tree", variant: "outline", size: "sm", onPress: () => applySample("tree") }, "示例树"),
        h(Button, { testID: "graph-sample-weighted", variant: "outline", size: "sm", onPress: () => applySample("weighted") }, "带权图"),
        h(Button, { testID: "graph-sample-geometry", variant: "outline", size: "sm", onPress: () => applySample("geometry") }, "几何点集"),
        h(
          View,
          {
            testID: "graph-connect-toggle",
            onPress: () => {
              connectMode.set(!connectMode());
              connectFrom.set(null);
            },
            style: () => ({
              height: 28,
              paddingLeft: 10,
              paddingRight: 10,
              borderRadius: 6,
              borderWidth: 1,
              borderColor: connectMode() ? token("primary") : token("border"),
              backgroundColor: connectMode() ? token("accent") : token("card"),
              flexDirection: "row",
              alignItems: "center",
              gap: 6,
            }),
          },
          h(View, {
            testID: "graph-connect-dot",
            style: () => ({
              width: 7,
              height: 7,
              borderRadius: 3.5,
              backgroundColor: connectMode() ? token("primary") : token("muted-foreground"),
              opacity: connectMode() ? 1 : 0.4,
            }),
          }),
          h(
            Text,
            {
              style: () => ({
                fontSize: 12.5,
                fontWeight: "500",
                color: connectMode() ? token("primary") : token("foreground"),
              }),
            },
            "连边模式"
          )
        )
      ),
      // Graph text input (two-way synced)
      h(Input, {
        testID: "graph-editor-input",
        multiline: true,
        value: () => editorText(),
        placeholder: "每行一条边 u v [w]",
        onChangeText: (txt) => {
          editorText.set(txt);
          if (!suppressTextParse) {
            parseTextToGraph(format(), txt, true);
          }
        },
        style: { flex: 1, fontFamily: "JetBrains Mono, monospace", fontSize: 12.5 },
      }),
      // Selected node inspector or hint
      h(
        If,
        {
          when: () => selectedId(),
          else: h(
            Text,
            { style: { fontSize: 11, lineHeight: 16, color: token("muted-foreground") } },
            "拖拽节点调整位置，点击空白加点，开启「连边模式」依次点击两点连边。"
          ),
        },
        () =>
          h(
            View,
            {
              style: {
                padding: 8,
                borderRadius: 6,
                borderWidth: 1,
                borderColor: token("border"),
                gap: 6,
              },
            },
            h(
              View,
              { style: { flexDirection: "row", justifyContent: "space-between", alignItems: "center" } },
              h(
                Text,
                { style: { fontSize: 12, fontWeight: "600", color: token("foreground") } },
                () => "选中节点：" + (selectedId() || "")
              ),
              h(
                Button,
                {
                  testID: "graph-delete-node",
                  variant: "outline",
                  size: "sm",
                  style: () => ({ borderColor: token("destructive") }),
                  textStyle: () => ({ color: token("destructive") }),
                  onPress: deleteSelected,
                },
                "删除"
              )
            ),
            h(
              View,
              { style: { flexDirection: "row", gap: 6 } },
              ...PALETTE.map((sw) =>
                h(View, {
                  testID: "graph-color-" + (sw.id || "default"),
                  onPress: () => recolorSelected(sw.id),
                  style: () => {
                    const dark = isDark();
                    const cur = nodes().find((n) => n.id === selectedId());
                    const activeColor = cur ? cur.color || null : null;
                    const isPicked = activeColor === sw.id;
                    return {
                      width: 22,
                      height: 22,
                      borderRadius: 11,
                      backgroundColor: sw.id ? (dark ? sw.dark : sw.light) : token("muted"),
                      borderWidth: 2,
                      borderColor: isPicked ? token("primary") : "transparent",
                    };
                  },
                })
              )
            )
          )
      )
    ),
    // Right pane: toolbar + interactive Canvas
    h(
      View,
      { style: { flex: 1, minWidth: 0, padding: 12, gap: 8 } },
      h(
        View,
        { style: { flexDirection: "row", justifyContent: "space-between", alignItems: "center", gap: 8 } },
        h(
          Text,
          {
            testID: "graph-stats",
            style: { fontSize: 12, fontVariant: ["tabular-nums"], color: token("muted-foreground") },
          },
          () =>
            "节点 " +
            nodes().length +
            "   边 " +
            edges().length +
            (connectFrom() ? "   连边起点：" + connectFrom() : "")
        ),
        h(
          View,
          { style: { flexDirection: "row", gap: 6 } },
          h(
            Button,
            {
              testID: "graph-relayout",
              variant: "outline",
              size: "sm",
              onPress: () => applyLayout(layoutMode()),
            },
            "重新布局"
          ),
          h(Button, { testID: "graph-export-svg", variant: "outline", size: "sm", onPress: handleExportSvg }, "导出 SVG"),
          h(Button, { testID: "graph-export-png", variant: "default", size: "sm", onPress: handleExportPng }, "导出 PNG")
        )
      ),
      h(Canvas, {
        testID: "graph-canvas",
        style: {
          flex: 1,
          borderRadius: 8,
          borderWidth: 1,
          borderColor: token("border"),
          backgroundColor: token("card"),
        },
        onLayout: (e) => {
          const w = e && e.bounds && e.bounds.size ? e.bounds.size.width : 0;
          const hVal = e && e.bounds && e.bounds.size ? e.bounds.size.height : 0;
          if (w > 40 && hVal > 40) {
            const prev = canvasSize();
            const changed = Math.abs(prev.width - w) > 2 || Math.abs(prev.height - hVal) > 2;
            canvasSize.set({ width: w, height: hVal });
            if (!hasMeasuredCanvas || (changed && !hasUserDragged)) {
              hasMeasuredCanvas = true;
              nodes.set(layoutNodes(nodes(), edges(), layoutMode(), w, hVal));
            } else if (changed) {
              nodes.set(clampNodesToBounds(nodes(), w, hVal));
            }
          }
        },
        onPressDown: (e) => {
          const x = e.position.x;
          const y = e.position.y;
          const hit = hitNode(x, y);
          dragMoved = false;
          if (hit) {
            if (connectMode()) {
              const from = connectFrom();
              if (!from) {
                connectFrom.set(hit.id);
              } else if (from !== hit.id) {
                const nextEdges = [...edges(), { from, to: hit.id, weight: null }];
                edges.set(nextEdges);
                connectFrom.set(null);
                serializeGraphToText(nodes(), nextEdges);
              } else {
                connectFrom.set(null);
              }
              selectedId.set(hit.id);
              return;
            }
            draggingId = hit.id;
            selectedId.set(hit.id);
          } else {
            draggingId = null;
            selectedId.set(null);
            connectFrom.set(null);
            if (!connectMode()) {
              const id = nextNodeId();
              const sz = canvasSize();
              const pad = NODE_R + 6;
              const cx = Math.max(pad, Math.min(sz.width - pad, x));
              const cy = Math.max(pad, Math.min(sz.height - pad, y));
              const nextNodes = [...nodes(), { id, label: id, x: cx, y: cy, color: null }];
              nodes.set(nextNodes);
              selectedId.set(id);
              hasUserDragged = true;
              serializeGraphToText(nextNodes, edges());
            }
          }
        },
        onMouseMove: (e) => {
          if (!draggingId) return;
          dragMoved = true;
          hasUserDragged = true;
          const sz = canvasSize();
          const pad = NODE_R + 4;
          const x = Math.max(pad, Math.min(sz.width - pad, e.position.x));
          const y = Math.max(pad, Math.min(sz.height - pad, e.position.y));
          nodes.update((list) => list.map((n) => (n.id === draggingId ? { ...n, x, y } : n)));
        },
        onMouseUp: () => {
          if (draggingId && dragMoved && format() === "coords") {
            serializeGraphToText(nodes(), edges());
          }
          draggingId = null;
          dragMoved = false;
        },
        onMouseLeave: () => {
          if (draggingId && dragMoved && format() === "coords") {
            serializeGraphToText(nodes(), edges());
          }
          draggingId = null;
          dragMoved = false;
        },
        draw: (cx, size) => {
          const dark = colorScheme && colorScheme() === "dark";
          const list = nodes();
          const edList = edges();
          const isDir = directed();
          const sel = selectedId();
          const cFrom = connectFrom();

          cx.clearRect(0, 0, size.width, size.height);
          cx.fillStyle = token("card");
          cx.fillRect(0, 0, size.width, size.height);

          // Subtle dot grid
          cx.fillStyle = dark ? "rgba(255,255,255,0.06)" : "rgba(9,9,11,0.06)";
          for (let gx = 24; gx < size.width; gx += 28) {
            for (let gy = 24; gy < size.height; gy += 28) {
              cx.fillRect(gx, gy, 1.5, 1.5);
            }
          }

          const byId = new Map(list.map((n) => [n.id, n]));
          const strokeCol = token("secondary-foreground");
          const textCol = token("foreground");
          const nodeBg = token("muted");
          const primaryCol = token("primary");

          // Draw edges
          cx.strokeStyle = strokeCol;
          cx.lineWidth = 1.8;
          cx.font = "12px JetBrains Mono, monospace";
          cx.textAlign = "center";
          cx.textBaseline = "middle";

          for (const e of edList) {
            const u = byId.get(e.from);
            const v = byId.get(e.to);
            if (!u || !v) continue;
            const dx = v.x - u.x;
            const dy = v.y - u.y;
            const dist = Math.hypot(dx, dy) || 1;
            const ux = dx / dist;
            const uy = dy / dist;
            const x1 = u.x + ux * NODE_R;
            const y1 = u.y + uy * NODE_R;
            const x2 = v.x - ux * (isDir ? NODE_R + 3 : NODE_R);
            const y2 = v.y - uy * (isDir ? NODE_R + 3 : NODE_R);

            cx.beginPath();
            cx.moveTo(x1, y1);
            cx.lineTo(x2, y2);
            cx.stroke();

            if (isDir) {
              const headLen = 9;
              const ang = Math.atan2(dy, dx);
              cx.fillStyle = strokeCol;
              cx.beginPath();
              cx.moveTo(x2, y2);
              cx.lineTo(x2 - headLen * Math.cos(ang - Math.PI / 6), y2 - headLen * Math.sin(ang - Math.PI / 6));
              cx.lineTo(x2 - headLen * Math.cos(ang + Math.PI / 6), y2 - headLen * Math.sin(ang + Math.PI / 6));
              cx.closePath();
              cx.fill();
            }

            if (e.weight) {
              const mx = (x1 + x2) / 2;
              const my = (y1 + y2) / 2 - 10;
              cx.fillStyle = textCol;
              cx.fillText(String(e.weight), mx, my);
            }
          }

          // Draw nodes
          cx.font = "600 13px JetBrains Mono, monospace";
          for (const nd of list) {
            const isSel = nd.id === sel || nd.id === cFrom;
            cx.beginPath();
            cx.arc(nd.x, nd.y, NODE_R, 0, Math.PI * 2);
            cx.fillStyle = resolveNodeFill(nd.color, dark, nodeBg);
            cx.fill();
            cx.lineWidth = isSel ? 2.5 : 1.8;
            cx.strokeStyle = resolveNodeStroke(nd.color, dark, isSel, strokeCol);
            cx.stroke();

            cx.fillStyle = resolveNodeText(nd.color, dark, textCol);
            cx.fillText(String(nd.label || nd.id), nd.x, nd.y);
          }
        },
      })
    )
  );
}

module.exports = { render };
