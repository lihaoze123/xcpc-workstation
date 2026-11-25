"use strict";

function escapeXml(str) {
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&apos;");
}

function parseGraphText(format, text, opts) {
  const oneIndexed = opts.one_indexed !== false;
  const directed = Boolean(opts.directed);
  let weighted = Boolean(opts.weighted);

  const lines = String(text || "")
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l.length > 0 && !l.startsWith("#") && !l.startsWith("//"));

  const nodeMap = new Map();
  const edges = [];

  function ensureNode(id, extra) {
    const key = String(id);
    if (!nodeMap.has(key)) {
      nodeMap.set(key, {
        id: key,
        label: key,
        x: extra && extra.x != null ? extra.x : null,
        y: extra && extra.y != null ? extra.y : null,
        color: extra && extra.color ? extra.color : null,
      });
    } else if (extra) {
      const existing = nodeMap.get(key);
      if (extra.x != null) existing.x = extra.x;
      if (extra.y != null) existing.y = extra.y;
      if (extra.color) existing.color = extra.color;
    }
    return nodeMap.get(key);
  }

  if (format === "parent_array") {
    const tokens = lines.join(" ").split(/[\s,]+/).filter(Boolean);
    const startIdx = oneIndexed ? 1 : 0;
    if (tokens.length > 0) {
      const firstVal = Number(tokens[0]);
      // If first element is 0 or -1 (root sentinel), treat tokens as full p[start..start+n-1].
      // Otherwise treat tokens as parents of nodes (start+1 .. start+tokens.length).
      const hasRootSentinel = firstVal === 0 || firstVal === -1 || (!oneIndexed && firstVal < 0);
      if (hasRootSentinel) {
        for (let i = 0; i < tokens.length; i++) {
          const u = String(startIdx + i);
          ensureNode(u);
          const p = tokens[i];
          const pNum = Number(p);
          if (pNum >= startIdx) {
            ensureNode(p);
            edges.push({ from: String(p), to: u, weight: null, label: null, directed });
          }
        }
      } else {
        ensureNode(String(startIdx));
        for (let i = 0; i < tokens.length; i++) {
          const u = String(startIdx + i + 1);
          const p = String(tokens[i]);
          ensureNode(p);
          ensureNode(u);
          edges.push({ from: p, to: u, weight: null, label: null, directed });
        }
      }
    }
  } else if (format === "adj_list") {
    for (const line of lines) {
      const colon = line.indexOf(":");
      if (colon < 0) {
        const parts = line.split(/\s+/).filter(Boolean);
        if (parts.length >= 1) {
          const u = parts[0];
          ensureNode(u);
          for (let i = 1; i < parts.length; i++) {
            const m = parts[i].match(/^([^\(\)]+)(?:\(([^\)]+)\))?$/);
            if (m) {
              const v = m[1];
              const w = m[2] != null ? m[2] : null;
              if (w != null) weighted = true;
              ensureNode(v);
              edges.push({ from: u, to: v, weight: w, label: w, directed });
            }
          }
        }
        continue;
      }
      const u = line.slice(0, colon).trim();
      if (!u) continue;
      ensureNode(u);
      const rhs = line.slice(colon + 1).trim();
      if (!rhs) continue;
      const items = rhs.split(/[\s,]+/).filter(Boolean);
      for (const item of items) {
        const m = item.match(/^([^\(\)]+)(?:\(([^\)]+)\))?$/);
        if (m) {
          const v = m[1];
          const w = m[2] != null ? m[2] : null;
          if (w != null) weighted = true;
          ensureNode(v);
          edges.push({ from: u, to: v, weight: w, label: w, directed });
        }
      }
    }
  } else if (format === "coords") {
    let autoId = oneIndexed ? 1 : 0;
    for (const line of lines) {
      const parts = line.split(/[\s,]+/).filter(Boolean);
      if (parts[0] === "e" || parts[0] === "edge") {
        if (parts.length >= 3) {
          const u = parts[1];
          const v = parts[2];
          const w = parts.length >= 4 ? parts[3] : null;
          if (w != null) weighted = true;
          ensureNode(u);
          ensureNode(v);
          edges.push({ from: u, to: v, weight: w, label: w, directed });
        }
        continue;
      }
      if (parts.length >= 3 && Number.isFinite(Number(parts[1])) && Number.isFinite(Number(parts[2]))) {
        ensureNode(parts[0], { x: Number(parts[1]), y: Number(parts[2]) });
      } else if (parts.length === 2 && Number.isFinite(Number(parts[0])) && Number.isFinite(Number(parts[1]))) {
        ensureNode(String(autoId++), { x: Number(parts[0]), y: Number(parts[1]) });
      }
    }
  } else {
    // edge_list
    for (const line of lines) {
      const parts = line.split(/[\s,]+/).filter(Boolean);
      if (parts.length === 1) {
        ensureNode(parts[0]);
      } else if (parts.length >= 2) {
        const u = parts[0];
        const v = parts[1];
        const w = parts.length >= 3 ? parts[2] : null;
        if (w != null) weighted = true;
        ensureNode(u);
        ensureNode(v);
        edges.push({ from: u, to: v, weight: w, label: w, directed });
      }
    }
  }

  return {
    nodes: [...nodeMap.values()],
    edges,
    directed,
    weighted,
    one_indexed: oneIndexed,
  };
}

function layoutGraph(graph, layout, width, height) {
  const nodes = graph.nodes.map((n) => ({ ...n }));
  const n = nodes.length;
  if (n === 0) return nodes;

  const pad = 48;
  const cx = width / 2;
  const cy = height / 2;
  const radius = Math.max(40, Math.min(width, height) / 2 - pad);

  if (layout === "coords" && nodes.some((nd) => nd.x != null && nd.y != null)) {
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    for (const nd of nodes) {
      if (nd.x != null && nd.y != null) {
        minX = Math.min(minX, nd.x);
        maxX = Math.max(maxX, nd.x);
        minY = Math.min(minY, nd.y);
        maxY = Math.max(maxY, nd.y);
      }
    }
    const spanX = Math.max(1e-6, maxX - minX);
    const spanY = Math.max(1e-6, maxY - minY);
    for (let i = 0; i < n; i++) {
      if (nodes[i].x == null || nodes[i].y == null) {
        const a = (2 * Math.PI * i) / n;
        nodes[i].x = cx + radius * Math.cos(a);
        nodes[i].y = cy + radius * Math.sin(a);
      } else {
        nodes[i].x = pad + ((nodes[i].x - minX) / spanX) * (width - 2 * pad);
        nodes[i].y = height - pad - ((nodes[i].y - minY) / spanY) * (height - 2 * pad);
      }
    }
    return nodes;
  }

  if (layout === "tree") {
    const adj = new Map();
    for (const nd of nodes) adj.set(nd.id, []);
    for (const e of graph.edges) {
      if (adj.has(e.from)) adj.get(e.from).push(e.to);
      if (adj.has(e.to)) adj.get(e.to).push(e.from);
    }
    const depth = new Map();
    const levels = [];
    for (const start of nodes) {
      if (depth.has(start.id)) continue;
      const q = [start.id];
      depth.set(start.id, 0);
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
    const maxLevel = Math.max(1, levels.length - 1);
    for (let d = 0; d < levels.length; d++) {
      const row = levels[d];
      const y = levels.length === 1 ? cy : pad + (d / maxLevel) * (height - 2 * pad);
      for (let i = 0; i < row.length; i++) {
        const x = row.length === 1 ? cx : pad + ((i + 0.5) / row.length) * (width - 2 * pad);
        pos.set(row[i], { x, y });
      }
    }
    for (const nd of nodes) {
      const p = pos.get(nd.id) || { x: cx, y: cy };
      nd.x = p.x;
      nd.y = p.y;
    }
    return nodes;
  }

  // Initial circle placement
  for (let i = 0; i < n; i++) {
    const angle = (2 * Math.PI * i) / n - Math.PI / 2;
    nodes[i].x = cx + radius * Math.cos(angle);
    nodes[i].y = cy + radius * Math.sin(angle);
  }

  if (layout === "circle" || n <= 2) {
    return nodes;
  }

  // Deterministic Fruchterman-Reingold force-directed layout
  const idxMap = new Map();
  nodes.forEach((nd, i) => idxMap.set(nd.id, i));
  const area = (width - 2 * pad) * (height - 2 * pad);
  const k = Math.sqrt(area / Math.max(1, n));
  let temp = Math.min(width, height) / 6;

  for (let iter = 0; iter < 60; iter++) {
    const dx = new Array(n).fill(0);
    const dy = new Array(n).fill(0);

    for (let i = 0; i < n; i++) {
      for (let j = i + 1; j < n; j++) {
        const vx = nodes[i].x - nodes[j].x;
        const vy = nodes[i].y - nodes[j].y;
        const dist = Math.hypot(vx, vy) || 0.01;
        const rep = (k * k) / dist;
        const fx = (vx / dist) * rep;
        const fy = (vy / dist) * rep;
        dx[i] += fx;
        dy[i] += fy;
        dx[j] -= fx;
        dy[j] -= fy;
      }
    }

    for (const e of graph.edges) {
      const u = idxMap.get(e.from);
      const v = idxMap.get(e.to);
      if (u == null || v == null || u === v) continue;
      const vx = nodes[u].x - nodes[v].x;
      const vy = nodes[u].y - nodes[v].y;
      const dist = Math.hypot(vx, vy) || 0.01;
      const attr = (dist * dist) / k;
      const fx = (vx / dist) * attr;
      const fy = (vy / dist) * attr;
      dx[u] -= fx;
      dy[u] -= fy;
      dx[v] += fx;
      dy[v] += fy;
    }

    for (let i = 0; i < n; i++) {
      const disp = Math.hypot(dx[i], dy[i]) || 0.01;
      const step = Math.min(disp, temp);
      nodes[i].x = Math.max(pad, Math.min(width - pad, nodes[i].x + (dx[i] / disp) * step));
      nodes[i].y = Math.max(pad, Math.min(height - pad, nodes[i].y + (dy[i] / disp) * step));
    }
    temp *= 0.94;
  }

  return nodes;
}

function renderGraphSvg(nodes, edges, directed, width, height) {
  const byId = new Map();
  for (const nd of nodes) byId.set(nd.id, nd);
  const r = 18;
  const parts = [];
  parts.push(
    '<svg xmlns="http://www.w3.org/2000/svg" width="' +
      width +
      '" height="' +
      height +
      '" viewBox="0 0 ' +
      width +
      " " +
      height +
      '">'
  );
  parts.push('<rect width="100%" height="100%" fill="#ffffff" rx="8"/>');

  if (directed) {
    parts.push(
      '<defs><marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M 0 1 L 10 5 L 0 9 z" fill="#475569"/></marker></defs>'
    );
  }

  for (const e of edges) {
    const u = byId.get(e.from);
    const v = byId.get(e.to);
    if (!u || !v) continue;
    const dx = v.x - u.x;
    const dy = v.y - u.y;
    const dist = Math.hypot(dx, dy) || 1;
    const x1 = u.x + (dx / dist) * r;
    const y1 = u.y + (dy / dist) * r;
    const x2 = v.x - (dx / dist) * (directed ? r + 4 : r);
    const y2 = v.y - (dy / dist) * (directed ? r + 4 : r);

    parts.push(
      '<line x1="' +
        x1.toFixed(1) +
        '" y1="' +
        y1.toFixed(1) +
        '" x2="' +
        x2.toFixed(1) +
        '" y2="' +
        y2.toFixed(1) +
        '" stroke="#475569" stroke-width="2"' +
        (directed ? ' marker-end="url(#arrow)"' : "") +
        "/>"
    );

    const lbl = e.weight != null ? e.weight : e.label;
    if (lbl != null && String(lbl).length > 0) {
      const mx = (x1 + x2) / 2;
      const my = (y1 + y2) / 2 - 6;
      parts.push(
        '<text x="' +
          mx.toFixed(1) +
          '" y="' +
          my.toFixed(1) +
          '" text-anchor="middle" font-family="monospace" font-size="12" fill="#0f172a">' +
          escapeXml(lbl) +
          "</text>"
      );
    }
  }

  for (const nd of nodes) {
    const fill = nd.color || "#eef2ff";
    parts.push(
      '<circle cx="' +
        nd.x.toFixed(1) +
        '" cy="' +
        nd.y.toFixed(1) +
        '" r="' +
        r +
        '" fill="' +
        escapeXml(fill) +
        '" stroke="#334155" stroke-width="2"/>'
    );
    parts.push(
      '<text x="' +
        nd.x.toFixed(1) +
        '" y="' +
        (nd.y + 4).toFixed(1) +
        '" text-anchor="middle" font-family="monospace" font-size="13" font-weight="600" fill="#0f172a">' +
        escapeXml(nd.label || nd.id) +
        "</text>"
    );
  }

  parts.push("</svg>");
  return parts.join("");
}

function runTool(toolId, inputs) {
  if (toolId !== "parse-graph") {
    throw new Error("Unknown tool: " + toolId);
  }
  const format = String((inputs && inputs.format) || "edge_list");
  const text = String((inputs && inputs.text) || "");
  const layout = String((inputs && inputs.layout) || (format === "coords" ? "coords" : "force"));
  const width = 640;
  const height = 420;

  const parsed = parseGraphText(format, text, inputs || {});
  const placedNodes = layoutGraph(parsed, layout, width, height);
  const svg = renderGraphSvg(placedNodes, parsed.edges, parsed.directed, width, height);

  const layoutLabel =
    layout === "tree"
      ? "树"
      : layout === "circle"
        ? "圆形"
        : layout === "coords"
          ? "按坐标"
          : "力导向";

  return {
    summary:
      "已解析 " +
      placedNodes.length +
      " 个节点、" +
      parsed.edges.length +
      " 条边（" +
      (parsed.directed ? "有向" : "无向") +
      "，" +
      layoutLabel +
      "）",
    blocks: [
      {
        type: "graph",
        title: "图结构",
        directed: parsed.directed,
        weighted: parsed.weighted,
        one_indexed: parsed.one_indexed,
        nodes: placedNodes,
        edges: parsed.edges,
        layout,
      },
      {
        type: "svg",
        title: "矢量预览",
        svg,
        width,
        height,
      },
    ],
  };
}

function runCommand(commandId) {
  if (commandId === "open") {
    return { type: "open_tool", plugin_id: "graph-editor" };
  }
  return { type: "none" };
}

module.exports = { runTool, runCommand };
