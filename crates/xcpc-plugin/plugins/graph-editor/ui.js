"use strict";

function render(sdk) {
  const { h, signal, effect, ui, token, colorScheme } = sdk;
  const { View, Text, Canvas, Button, Input } = ui;

  const NODE_R = 18;
  const PALETTE = [
    { name: "默认", color: "#dbeafe" },
    { name: "翠绿", color: "#bbf7d0" },
    { name: "琥珀", color: "#fde68a" },
    { name: "珊瑚", color: "#fecaca" },
    { name: "紫藤", color: "#e9d5ff" },
  ];

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
    coords: {
      format: "coords",
      directed: false,
      oneIndexed: true,
      layout: "coords",
      text: "1 0 0\n2 4 0\n3 2 3\n4 6 3\nedge 1 2\nedge 2 3\nedge 3 1\nedge 2 4\nedge 3 4",
    },
  };

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
  const canvasSize = signal({ width: 640, height: 460 });

  let draggingId = null;
  let dragMoved = false;
  let suppressTextParse = false;

  function escapeXml(str) {
    return String(str)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  function layoutNodes(rawNodes, rawEdges, mode, w, h) {
    const list = rawNodes.map((n) => ({ ...n }));
    const n = list.length;
    if (n === 0) return list;
    const pad = 42;
    const cx = w / 2;
    const cy = h / 2;
    const radius = Math.max(40, Math.min(w, h) / 2 - pad);

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
      for (let i = 0; i < n; i++) {
        if (list[i].x == null || list[i].y == null) {
          const a = (2 * Math.PI * i) / n;
          list[i].x = cx + radius * Math.cos(a);
          list[i].y = cy + radius * Math.sin(a);
        } else {
          list[i].x = pad + ((list[i].x - minX) / sx) * (w - 2 * pad);
          list[i].y = h - pad - ((list[i].y - minY) / sy) * (h - 2 * pad);
        }
      }
      return list;
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
      return list;
    }

    for (let i = 0; i < n; i++) {
      const a = (2 * Math.PI * i) / n - Math.PI / 2;
      list[i].x = cx + radius * Math.cos(a);
      list[i].y = cy + radius * Math.sin(a);
    }

    if (mode === "circle" || n <= 2) return list;

    const idx = new Map();
    list.forEach((nd, i) => idx.set(nd.id, i));
    const area = Math.max(1000, (w - 2 * pad) * (h - 2 * pad));
    const k = Math.sqrt(area / Math.max(1, n));
    let temp = Math.min(w, h) / 6;

    for (let iter = 0; iter < 50; iter++) {
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
      temp *= 0.94;
    }
    return list;
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
      : rawList;
    nodes.set(placed);
    edges.set(parsedEdges);
  }

  function serializeGraphToText(curNodes, curEdges) {
    const fmt = format();
    const lines = [];
    if (fmt === "coords") {
      for (const nd of curNodes) {
        lines.push(nd.id + " " + Math.round(nd.x) + " " + Math.round(nd.y));
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
      // edge_list (also fallback after interactive edits on parent_array)
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
    if (!payload || !Array.isArray(payload.nodes)) return;
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
      : incomingNodes;
    nodes.set(placed);
    edges.set(incomingEdges);
    serializeGraphToText(placed, incomingEdges);
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

  function buildCurrentSvg() {
    const sz = canvasSize();
    const w = Math.max(320, Math.round(sz.width));
    const h = Math.max(240, Math.round(sz.height));
    const list = nodes();
    const edList = edges();
    const isDir = directed();
    const byId = new Map(list.map((n) => [n.id, n]));
    const parts = [
      '<svg xmlns="http://www.w3.org/2000/svg" width="' + w + '" height="' + h + '" viewBox="0 0 ' + w + " " + h + '">',
      '<rect width="100%" height="100%" fill="#ffffff"/>',
    ];
    if (isDir) {
      parts.push(
        '<defs><marker id="arr" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M 0 1 L 10 5 L 0 9 z" fill="#475569"/></marker></defs>'
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
        '<line x1="' + x1.toFixed(1) + '" y1="' + y1.toFixed(1) + '" x2="' + x2.toFixed(1) + '" y2="' + y2.toFixed(1) + '" stroke="#475569" stroke-width="2"' + (isDir ? ' marker-end="url(#arr)"' : "") + "/>"
      );
      if (e.weight) {
        parts.push(
          '<text x="' + ((x1 + x2) / 2).toFixed(1) + '" y="' + ((y1 + y2) / 2 - 6).toFixed(1) + '" text-anchor="middle" font-family="monospace" font-size="12" fill="#0f172a">' + escapeXml(e.weight) + "</text>"
        );
      }
    }
    for (const nd of list) {
      parts.push(
        '<circle cx="' + nd.x.toFixed(1) + '" cy="' + nd.y.toFixed(1) + '" r="' + NODE_R + '" fill="' + escapeXml(nd.color || "#dbeafe") + '" stroke="#334155" stroke-width="2"/>'
      );
      parts.push(
        '<text x="' + nd.x.toFixed(1) + '" y="' + (nd.y + 4).toFixed(1) + '" text-anchor="middle" font-family="monospace" font-size="13" font-weight="600" fill="#0f172a">' + escapeXml(nd.label || nd.id) + "</text>"
      );
    }
    parts.push("</svg>");
    return parts.join("");
  }

  async function handleExportSvg() {
    const svg = buildCurrentSvg();
    if (sdk.saveText) {
      const ok = await sdk.saveText(svg, "graph.svg");
      if (ok && sdk.toast) sdk.toast("已导出 SVG");
    } else if (sdk.copyText) {
      await sdk.copyText(svg);
      if (sdk.toast) sdk.toast("已复制 SVG 源码");
    }
  }

  async function handleExportPng() {
    if (!sdk.renderSvgToPng || !sdk.savePng) return;
    const svg = buildCurrentSvg();
    const b64 = await sdk.renderSvgToPng(svg, 2.0);
    const ok = await sdk.savePng(b64, "graph.png");
    if (ok && sdk.toast) sdk.toast("已导出 PNG");
  }

  function applySample(key) {
    const s = SAMPLES[key];
    if (!s) return;
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
    { style: { flex: 1, flexDirection: "row", backgroundColor: token("background") } },
    // Left pane: text editor & controls
    h(
      View,
      {
        style: {
          width: 300,
          borderRightWidth: 1,
          borderColor: token("border"),
          padding: 12,
          gap: 10,
          backgroundColor: token("card"),
        },
      },
      h(
        View,
        { style: { flexDirection: "row", flexWrap: "wrap", gap: 4 } },
        ...[
          ["edge_list", "边表"],
          ["parent_array", "父数组"],
          ["adj_list", "邻接表"],
          ["coords", "坐标"],
        ].map(([id, label]) =>
          h(
            Button,
            {
              variant: format() === id ? "default" : "outline",
              size: "sm",
              onPress: () => {
                format.set(id);
                parseTextToGraph(id, editorText(), false);
              },
            },
            label
          )
        )
      ),
      h(
        View,
        { style: { flexDirection: "row", flexWrap: "wrap", gap: 4 } },
        h(
          Button,
          {
            variant: oneIndexed() ? "default" : "outline",
            size: "sm",
            onPress: () => {
              oneIndexed.set(!oneIndexed());
              parseTextToGraph(format(), editorText(), false);
            },
          },
          oneIndexed() ? "1-indexed" : "0-indexed"
        ),
        h(
          Button,
          {
            variant: directed() ? "default" : "outline",
            size: "sm",
            onPress: () => directed.set(!directed()),
          },
          directed() ? "有向图" : "无向图"
        ),
        h(
          Button,
          {
            variant: connectMode() ? "default" : "outline",
            size: "sm",
            onPress: () => {
              connectMode.set(!connectMode());
              connectFrom.set(null);
            },
          },
          connectMode() ? "连边模式: 开" : "连边模式"
        )
      ),
      h(
        View,
        { style: { flexDirection: "row", flexWrap: "wrap", gap: 4 } },
        ...[
          ["force", "力导向"],
          ["tree", "树"],
          ["circle", "圆形"],
          ["coords", "坐标"],
        ].map(([id, label]) =>
          h(
            Button,
            {
              variant: layoutMode() === id ? "default" : "outline",
              size: "sm",
              onPress: () => {
                layoutMode.set(id);
                const sz = canvasSize();
                nodes.set(layoutNodes(nodes(), edges(), id, sz.width, sz.height));
              },
            },
            label
          )
        )
      ),
      h(
        View,
        { style: { flexDirection: "row", gap: 4 } },
        h(Button, { variant: "outline", size: "sm", onPress: () => applySample("tree") }, "示例树"),
        h(Button, { variant: "outline", size: "sm", onPress: () => applySample("weighted") }, "带权图"),
        h(Button, { variant: "outline", size: "sm", onPress: () => applySample("coords") }, "坐标")
      ),
      h(Input, {
        multiline: true,
        value: editorText(),
        placeholder: "每行一条边 u v [w]",
        onChangeText: (txt) => {
          editorText.set(txt);
          if (!suppressTextParse) {
            parseTextToGraph(format(), txt, true);
          }
        },
        style: { flex: 1, fontFamily: "JetBrains Mono, monospace", fontSize: 12.5 },
      }),
      selectedId()
        ? h(
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
              h(Text, { style: { fontSize: 12, fontWeight: "600", color: token("foreground") } }, "选中节点: " + selectedId()),
              h(Button, { variant: "destructive", size: "sm", onPress: deleteSelected }, "删除")
            ),
            h(
              View,
              { style: { flexDirection: "row", gap: 6 } },
              ...PALETTE.map((sw) =>
                h(View, {
                  onPress: () => recolorSelected(sw.color),
                  style: {
                    width: 22,
                    height: 22,
                    borderRadius: 11,
                    backgroundColor: sw.color,
                    borderWidth: 1,
                    borderColor: token("border"),
                  },
                })
              )
            )
          )
        : h(
            Text,
            { style: { fontSize: 11, color: token("muted-foreground") } },
            "提示：拖拽节点调整位置，点击空白加点，开启「连边模式」依次点击两点连边。"
          )
    ),
    // Right pane: toolbar + interactive Canvas
    h(
      View,
      { style: { flex: 1, padding: 12, gap: 8 } },
      h(
        View,
        { style: { flexDirection: "row", justifyContent: "space-between", alignItems: "center" } },
        h(
          Text,
          { style: { fontSize: 12, color: token("muted-foreground") } },
          "节点 " + nodes().length + " · 边 " + edges().length + (connectFrom() ? " · 连边起点: " + connectFrom() : "")
        ),
        h(
          View,
          { style: { flexDirection: "row", gap: 6 } },
          h(
            Button,
            {
              variant: "outline",
              size: "sm",
              onPress: () => {
                const sz = canvasSize();
                nodes.set(layoutNodes(nodes(), edges(), layoutMode(), sz.width, sz.height));
              },
            },
            "重新布局"
          ),
          h(Button, { variant: "outline", size: "sm", onPress: handleExportSvg }, "导出 SVG"),
          h(Button, { variant: "default", size: "sm", onPress: handleExportPng }, "导出 PNG")
        )
      ),
      h(Canvas, {
        style: {
          flex: 1,
          borderRadius: 8,
          borderWidth: 1,
          borderColor: token("border"),
          backgroundColor: token("card"),
        },
        onLayout: (e) => {
          if (e && e.bounds && e.bounds.size && e.bounds.size.width > 0 && e.bounds.size.height > 0) {
            canvasSize.set({ width: e.bounds.size.width, height: e.bounds.size.height });
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
              const nextNodes = [...nodes(), { id, label: id, x, y, color: null }];
              nodes.set(nextNodes);
              selectedId.set(id);
              serializeGraphToText(nextNodes, edges());
            }
          }
        },
        onMouseMove: (e) => {
          if (!draggingId) return;
          dragMoved = true;
          const x = e.position.x;
          const y = e.position.y;
          nodes.update((list) => list.map((n) => (n.id === draggingId ? { ...n, x, y } : n)));
        },
        onMouseUp: () => {
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
          cx.fillStyle = dark ? "#18181b" : "#ffffff";
          cx.fillRect(0, 0, size.width, size.height);

          // Subtle dot grid
          cx.fillStyle = dark ? "rgba(255,255,255,0.06)" : "rgba(15,23,42,0.07)";
          for (let gx = 24; gx < size.width; gx += 28) {
            for (let gy = 24; gy < size.height; gy += 28) {
              cx.fillRect(gx, gy, 1.5, 1.5);
            }
          }

          const byId = new Map(list.map((n) => [n.id, n]));
          const strokeCol = dark ? "#94a3b8" : "#475569";
          const textCol = dark ? "#f8fafc" : "#0f172a";
          const primaryCol = token("primary");

          // Draw edges
          cx.strokeStyle = strokeCol;
          cx.lineWidth = 2;
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
            cx.fillStyle = nd.color || (dark ? "#27272a" : "#dbeafe");
            cx.fill();
            cx.lineWidth = isSel ? 3 : 2;
            cx.strokeStyle = isSel ? primaryCol : strokeCol;
            cx.stroke();

            cx.fillStyle = nd.color ? "#0f172a" : textCol;
            cx.fillText(String(nd.label || nd.id), nd.x, nd.y);
          }
        },
      })
    )
  );
}

module.exports = { render };
