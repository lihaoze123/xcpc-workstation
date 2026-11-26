// Host-provided structured output block renderer for headless `index.js` tool plugins:
// `text`, `markdown`, `table`, `svg` (via `resvg` PNG preview + Copy SVG + Save PNG),
// `image` (PNG preview + Save PNG), and `graph` (read-only Canvas + open in graph-editor).

import { Canvas, Clipboard, computed, Image, Scroll, Text, View, type JSX } from "nsh";
import { Preview } from "xcpc-workstation/native";
import { t } from "../i18n";
import { Btn, TruncText } from "../kit";
import { openToolPlugin } from "../store";
import { cardShadow, MONO, SANS, token } from "../theme";
import { showToast } from "../Toast";
import type { GraphEdge, GraphNode, ToolOutputBlock } from "./types";

import { FONT_SIZE, FONT_WEIGHT, LINE_HEIGHT, RADIUS, SPACE } from "../tokens";
function BlockCard(props: {
  title?: string | null;
  actions?: JSX.Element;
  children?: JSX.Element;
}) {
  return (
    <View
      class="bg-card border border-border"
      style={{
        borderRadius: RADIUS.lg,
        overflow: "hidden",
        boxShadow: cardShadow(),
      }}
    >
      <If when={Boolean(props.title) || Boolean(props.actions)}>
        <View
          class="bg-muted/50 border-b border-border"
          style={{
            minHeight: 34,
            paddingLeft: SPACE[12],
            paddingRight: SPACE[12],
            paddingTop: SPACE[6],
            paddingBottom: SPACE[6],
            flexDirection: "row",
            alignItems: "center",
            justifyContent: "space-between",
            gap: SPACE[8],
          }}
        >
          <TruncText
            style={{ fontSize: FONT_SIZE.caption, fontWeight: FONT_WEIGHT.semibold, flexShrink: 1, numberOfLines: 1, textOverflow: "ellipsis" }}
            class="text-foreground"
          >
            {props.title ?? ""}
          </TruncText>
          {props.actions}
        </View>
      </If>
      {props.children}
    </View>
  );
}

function TextBlockView(props: { block: Extract<ToolOutputBlock, { type: "text" }> }) {
  const copy = () => {
    void Clipboard.writeText(props.block.content);
    showToast({ tone: "success", text: t("tools.copied") });
  };
  return (
    <BlockCard
      title={props.block.title}
      actions={
        <Btn size="sm" variant="ghost" icon="copy" label={t("common.copy")} onPress={copy} />
      }
    >
      <View style={{ padding: SPACE[12] }}>
        <Text
          style={{
            fontSize: FONT_SIZE.caption,
            lineHeight: LINE_HEIGHT.body,
            fontFamily: props.block.monospace ? MONO : undefined,
            fontVariant: ["tabular-nums"],
          }}
          class="text-foreground"
        >
          {props.block.content}
        </Text>
      </View>
    </BlockCard>
  );
}

function MarkdownBlockView(props: { block: Extract<ToolOutputBlock, { type: "markdown" }> }) {
  const copy = () => {
    void Clipboard.writeText(props.block.content);
    showToast({ tone: "success", text: t("tools.copied") });
  };
  const lines = () => props.block.content.split(/\r?\n/);
  return (
    <BlockCard
      title={props.block.title}
      actions={
        <Btn size="sm" variant="ghost" icon="copy" label={t("common.copy")} onPress={copy} />
      }
    >
      <View style={{ padding: SPACE[12], gap: SPACE[4] }}>
        <Each of={lines()} key="index">
          {(line) => {
            const raw = line();
            if (raw.startsWith("### ")) {
              return (
                <Text style={{ fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold, marginTop: SPACE[4] }} class="text-foreground">
                  {raw.slice(4)}
                </Text>
              );
            }
            if (raw.startsWith("## ")) {
              return (
                <Text style={{ fontSize: FONT_SIZE.strong, fontWeight: FONT_WEIGHT.semibold, marginTop: SPACE[6] }} class="text-foreground">
                  {raw.slice(3)}
                </Text>
              );
            }
            if (raw.startsWith("# ")) {
              return (
                <Text style={{ fontSize: FONT_SIZE.strong, fontWeight: FONT_WEIGHT.semibold, marginTop: SPACE[6] }} class="text-foreground">
                  {raw.slice(2)}
                </Text>
              );
            }
            if (raw.trim() === "") {
              return <View style={{ height: 4 }} />;
            }
            return (
              <Text
                style={{ fontSize: FONT_SIZE.caption, lineHeight: LINE_HEIGHT.body, fontVariant: ["tabular-nums"] }}
                class="text-foreground"
              >
                {raw}
              </Text>
            );
          }}
        </Each>
      </View>
    </BlockCard>
  );
}

const CJK_RE = /[\u3000-\u303f\u3400-\u4dbf\u4e00-\u9fff\uff00-\uffef]/;
const PURE_NUMERIC_RE = /^[+-]?[\d,]+(?:\.\d+)?(?:e[+-]?\d+)?%?$/i;
const ENGLISH_PROSE_RE = /[A-Za-z]{4,}\s+[A-Za-z]{4,}/;

/** Returns true only for numeric, formula, code, or index cells — never for Chinese or English prose. */
export function isMonoTableCell(raw: string): boolean {
  const s = raw.trim();
  if (!s) return false;
  if (CJK_RE.test(s)) return false;
  if (ENGLISH_PROSE_RE.test(s)) return false;
  return true;
}

/** Estimates visual character units for a string (CJK ≈ 1.75× ASCII width). */
function measureVisualUnits(raw: string): number {
  let units = 0;
  for (const ch of raw) {
    units += CJK_RE.test(ch) ? 1.75 : 1;
  }
  return units;
}

interface ComputedColSpec {
  align: "left" | "center" | "right";
  fixedWidth?: number;
  flexWeight: number;
  extraRightPad: number;
}

/** Computes content-aware column widths, alignments, and inter-column padding for a table. */
export function computeTableColumnSpecs(
  headers: readonly string[],
  rows: readonly (readonly string[])[],
  explicitAlign?: readonly string[] | null,
): ComputedColSpec[] {
  const colCount = headers.length;
  if (colCount === 0) return [];

  const aligns: ("left" | "center" | "right")[] = [];
  const maxUnits: number[] = [];
  const isCompactNumeric: boolean[] = [];

  for (let c = 0; c < colCount; c++) {
    const header = String(headers[c] ?? "");
    let maxU = measureVisualUnits(header);
    let nonEmpty = 0;
    let numericCount = 0;

    for (const row of rows) {
      const cell = String(row[c] ?? "").trim();
      const u = measureVisualUnits(cell);
      if (u > maxU) maxU = u;
      if (cell.length > 0) {
        nonEmpty++;
        if (PURE_NUMERIC_RE.test(cell)) numericCount++;
      }
    }

    const allNumeric = nonEmpty > 0 && numericCount === nonEmpty;
    const explicit = explicitAlign?.[c];
    const align: "left" | "center" | "right" =
      explicit === "right" || explicit === "center" || explicit === "left"
        ? explicit
        : allNumeric
          ? "right"
          : "left";

    aligns.push(align);
    maxUnits.push(maxU);
    isCompactNumeric.push(allNumeric && maxU <= 13);
  }

  // Only pin compact numeric columns to fixed width when at least one other column can flex.
  const hasFlexibleCol = isCompactNumeric.some((compact) => !compact);

  const specs: ComputedColSpec[] = [];
  for (let c = 0; c < colCount; c++) {
    const estPx = Math.ceil(maxUnits[c] * 7.6) + 12;
    const nextAlign = c + 1 < colCount ? aligns[c + 1] : undefined;
    // Extra breathing room when a right-aligned column meets a left-aligned column (e.g. LCP | 后缀).
    const extraRightPad = aligns[c] === "right" && nextAlign === "left" ? 10 : 0;

    if (hasFlexibleCol && isCompactNumeric[c]) {
      specs.push({
        align: aligns[c],
        fixedWidth: Math.max(44, Math.min(116, estPx)) + extraRightPad,
        flexWeight: 0,
        extraRightPad,
      });
    } else {
      specs.push({
        align: aligns[c],
        flexWeight: Math.max(60, estPx),
        extraRightPad,
      });
    }
  }

  return specs;
}

function TableBlockView(props: { block: Extract<ToolOutputBlock, { type: "table" }> }) {
  const isWideMatrix = () => props.block.headers.length > 5;
  const colSpecs = () =>
    computeTableColumnSpecs(props.block.headers, props.block.rows, props.block.align);

  const wideFirstColWidth = () => {
    let maxU = measureVisualUnits(String(props.block.headers[0] ?? ""));
    for (const row of props.block.rows) {
      const u = measureVisualUnits(String(row[0] ?? ""));
      if (u > maxU) maxU = u;
    }
    return Math.max(76, Math.min(120, Math.ceil(maxU * 7.6) + 14));
  };

  const renderTableInner = () => {
    const wide = isWideMatrix();
    const specs = wide ? [] : colSpecs();
    const firstW = wide ? wideFirstColWidth() : 0;
    return (
      <View style={wide ? { minWidth: "100%", flexGrow: 1 } : { width: "100%" }}>
        <View
          class="bg-muted/30 border-b border-border"
          style={{
            flexDirection: "row",
            width: "100%",
            paddingLeft: SPACE[12],
            paddingRight: SPACE[12],
            paddingTop: SPACE[8],
            paddingBottom: SPACE[8],
            gap: wide ? SPACE[6] : SPACE[16],
          }}
        >
          <Each of={props.block.headers} key="index">
            {(header, idx) => {
              const i = idx();
              if (wide) {
                return (
                  <Text
                    style={
                      i === 0
                        ? {
                            width: firstW,
                            flexShrink: 0,
                            fontSize: FONT_SIZE.caption,
                            fontWeight: FONT_WEIGHT.semibold,
                            textAlign: "left",
                            numberOfLines: 1,
                          }
                        : {
                            flexGrow: 1,
                            flexShrink: 0,
                            minWidth: 32,
                            fontSize: FONT_SIZE.caption,
                            fontWeight: FONT_WEIGHT.semibold,
                            textAlign: "center",
                            fontVariant: ["tabular-nums"],
                            numberOfLines: 1,
                          }
                    }
                    class="text-muted-foreground"
                  >
                    {header()}
                  </Text>
                );
              }
              const spec = specs[i] ?? { align: "left" as const, flexWeight: 100, extraRightPad: 0 };
              return (
                <Text
                  style={{
                    ...(spec.fixedWidth != null
                      ? { width: spec.fixedWidth, flexShrink: 0 }
                      : { flex: spec.flexWeight, minWidth: 0 }),
                    paddingRight: spec.extraRightPad || undefined,
                    fontSize: FONT_SIZE.caption,
                    fontWeight: FONT_WEIGHT.semibold,
                    textAlign: spec.align,
                    numberOfLines: 1,
                  }}
                  class="text-muted-foreground"
                >
                  {header()}
                </Text>
              );
            }}
          </Each>
        </View>
        <Each of={props.block.rows} key="index">
          {(row, rIdx) => (
            <View
              class={rIdx() + 1 < props.block.rows.length ? "border-b border-border" : undefined}
              style={{
                flexDirection: "row",
                width: "100%",
                paddingLeft: SPACE[12],
                paddingRight: SPACE[12],
                paddingTop: SPACE[8],
                paddingBottom: SPACE[8],
                gap: wide ? SPACE[6] : SPACE[16],
              }}
            >
              <Each of={row()} key="index">
                {(cell, cIdx) => {
                  const i = cIdx();
                  const val = cell();
                  if (wide) {
                    return (
                      <Text
                        style={
                          i === 0
                            ? {
                                width: firstW,
                                flexShrink: 0,
                                fontSize: FONT_SIZE.caption,
                                lineHeight: LINE_HEIGHT.caption,
                                fontVariant: ["tabular-nums"],
                                textAlign: "left",
                                numberOfLines: 1,
                              }
                            : {
                                flexGrow: 1,
                                flexShrink: 0,
                                minWidth: 32,
                                fontSize: FONT_SIZE.caption,
                                lineHeight: LINE_HEIGHT.caption,
                                fontVariant: ["tabular-nums"],
                                textAlign: "center",
                                numberOfLines: 1,
                              }
                        }
                        class="text-foreground"
                      >
                        {val}
                      </Text>
                    );
                  }
                  const spec = specs[i] ?? { align: "left" as const, flexWeight: 100, extraRightPad: 0 };
                  return (
                    <Text
                      style={{
                        ...(spec.fixedWidth != null
                          ? { width: spec.fixedWidth, flexShrink: 0 }
                          : { flex: spec.flexWeight, minWidth: 0 }),
                        paddingRight: spec.extraRightPad || undefined,
                        fontSize: FONT_SIZE.caption,
                        lineHeight: LINE_HEIGHT.caption,
                        fontVariant: ["tabular-nums"],
                        textAlign: spec.align,
                      }}
                      class="text-foreground"
                    >
                      {val}
                    </Text>
                  );
                }}
              </Each>
            </View>
          )}
        </Each>
      </View>
    );
  };

  return (
    <BlockCard title={props.block.title}>
      <If when={isWideMatrix()} else={renderTableInner()}>
        <Scroll horizontal style={{ width: "100%" }}>
          {renderTableInner()}
        </Scroll>
      </If>
    </BlockCard>
  );
}

function SvgBlockView(props: { block: Extract<ToolOutputBlock, { type: "svg" }> }) {
  const rendered = computed(() => Preview.pluginRenderSvgPng(props.block.svg, 2.0));

  const copySvg = () => {
    void Clipboard.writeText(props.block.svg);
    showToast({ tone: "success", text: t("tools.copiedSvg") });
  };

  const savePng = async () => {
    const r = rendered();
    if (!r.ok || !r.pngBase64) return;
    const saved = await Preview.pluginSavePngDialog(r.pngBase64, "tool-output.png", {
      title: t("tools.savePngTitle"),
      buttonLabel: t("common.save"),
      filterLabel: t("tools.pngFilter"),
    });
    if (saved.ok) {
      showToast({ tone: "success", text: t("tools.savedPng") });
    } else if (!saved.cancelled && saved.error) {
      showToast({ tone: "issues", text: saved.error });
    }
  };

  const natW = props.block.width ?? 640;
  const natH = props.block.height ?? 420;
  const displayH = Math.min(360, Math.max(180, Math.round((natH / Math.max(1, natW)) * 560)));

  return (
    <BlockCard
      title={props.block.title ?? "SVG"}
      actions={
        <View style={{ flexDirection: "row", gap: SPACE[4] }}>
          <Btn size="sm" variant="ghost" icon="copy" label={t("tools.copySvg")} onPress={copySvg} />
          <Btn
            size="sm"
            variant="outline"
            icon="fileDown"
            label={t("tools.savePng")}
            disabled={!rendered().ok}
            onPress={() => void savePng()}
          />
        </View>
      }
    >
      <View style={{ padding: SPACE[12], alignItems: "center", justifyContent: "center", overflow: "hidden" }}>
        <If
          when={rendered().ok && rendered().pngBase64}
          else={
            <Text style={{ fontSize: FONT_SIZE.caption }} class="text-destructive">
              {rendered().error ?? t("tools.svgRenderFailed")}
            </Text>
          }
        >
          {(b64) => (
            <Image
              src={`data:image/png;base64,${b64()}`}
              style={{
                width: "100%",
                maxWidth: Math.min(natW, 640),
                height: displayH,
                objectFit: "contain",
                overflow: "hidden",
              }}
            />
          )}
        </If>
      </View>
    </BlockCard>
  );
}

function ImageBlockView(props: { block: Extract<ToolOutputBlock, { type: "image" }> }) {
  const savePng = async () => {
    const saved = await Preview.pluginSavePngDialog(props.block.png_base64, "tool-output.png", {
      title: t("tools.savePngTitle"),
      buttonLabel: t("common.save"),
      filterLabel: t("tools.pngFilter"),
    });
    if (saved.ok) {
      showToast({ tone: "success", text: t("tools.savedPng") });
    } else if (!saved.cancelled && saved.error) {
      showToast({ tone: "issues", text: saved.error });
    }
  };

  return (
    <BlockCard
      title={props.block.title ?? t("tools.imageOutput")}
      actions={
        <Btn size="sm" variant="outline" icon="fileDown" label={t("tools.savePng")} onPress={() => void savePng()} />
      }
    >
      <View style={{ padding: SPACE[12], alignItems: "center", justifyContent: "center", overflow: "hidden" }}>
        <Image
          src={`data:image/png;base64,${props.block.png_base64}`}
          style={{
            width: "100%",
            maxWidth: Math.min(props.block.width ?? 640, 640),
            height: Math.min(props.block.height ?? 360, 380),
            objectFit: "contain",
            overflow: "hidden",
          }}
        />
      </View>
    </BlockCard>
  );
}

function placePreviewNodes(nodes: readonly GraphNode[], w: number, h: number): { id: string; label: string; x: number; y: number; color?: string | null }[] {
  const n = nodes.length;
  if (n === 0) return [];
  const pad = 36;
  const cx = w / 2;
  const cy = h / 2;
  const r = Math.max(30, Math.min(w, h) / 2 - pad);
  const hasCoords = nodes.every((nd) => nd.x != null && nd.y != null);
  if (hasCoords) {
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    for (const nd of nodes) {
      minX = Math.min(minX, nd.x!);
      maxX = Math.max(maxX, nd.x!);
      minY = Math.min(minY, nd.y!);
      maxY = Math.max(maxY, nd.y!);
    }
    const sx = Math.max(1e-6, maxX - minX);
    const sy = Math.max(1e-6, maxY - minY);
    return nodes.map((nd) => ({
      id: nd.id,
      label: nd.label ?? nd.id,
      x: pad + ((nd.x! - minX) / sx) * (w - 2 * pad),
      y: pad + ((nd.y! - minY) / sy) * (h - 2 * pad),
      color: nd.color,
    }));
  }
  return nodes.map((nd, i) => {
    const ang = (2 * Math.PI * i) / n - Math.PI / 2;
    return {
      id: nd.id,
      label: nd.label ?? nd.id,
      x: cx + r * Math.cos(ang),
      y: cy + r * Math.sin(ang),
      color: nd.color,
    };
  });
}

function GraphBlockView(props: {
  block: Extract<ToolOutputBlock, { type: "graph" }>;
  graphEditorEnabled: boolean;
}) {
  const openInEditor = () => {
    openToolPlugin("graph-editor", { payload: props.block, mode: "ui" });
  };

  return (
    <BlockCard
      title={props.block.title ?? t("tools.graphOutput")}
      actions={
        props.graphEditorEnabled ? (
          <Btn
            testID="tool-open-in-graph-editor"
            size="sm"
            variant="outline"
            icon="arrowRight"
            label={t("tools.openInGraphEditor")}
            onPress={openInEditor}
          />
        ) : undefined
      }
    >
      <View style={{ padding: SPACE[12], gap: SPACE[6] }}>
        <Canvas
          style={{
            width: "100%",
            height: 260,
            borderRadius: RADIUS.sm,
            borderWidth: 1,
            borderColor: token("border"),
          }}
          draw={(cx, size) => {
            cx.clearRect(0, 0, size.width, size.height);
            cx.fillStyle = token("card");
            cx.fillRect(0, 0, size.width, size.height);

            const placed = placePreviewNodes(props.block.nodes, size.width, size.height);
            const byId = new Map(placed.map((n) => [n.id, n]));
            const strokeCol = token("secondary-foreground");
            const textCol = token("foreground");
            const nodeFill = token("muted");
            const nodeR = 15;
            const isDir = Boolean(props.block.directed);

            cx.strokeStyle = strokeCol;
            cx.lineWidth = 1.8;
            cx.font = `11px ${SANS}`;
            cx.textAlign = "center";
            cx.textBaseline = "middle";

            for (const e of props.block.edges as GraphEdge[]) {
              const u = byId.get(e.from);
              const v = byId.get(e.to);
              if (!u || !v) continue;
              const dx = v.x - u.x;
              const dy = v.y - u.y;
              const dist = Math.hypot(dx, dy) || 1;
              const ux = dx / dist;
              const uy = dy / dist;
              const x1 = u.x + ux * nodeR;
              const y1 = u.y + uy * nodeR;
              const x2 = v.x - ux * (isDir ? nodeR + 3 : nodeR);
              const y2 = v.y - uy * (isDir ? nodeR + 3 : nodeR);

              cx.beginPath();
              cx.moveTo(x1, y1);
              cx.lineTo(x2, y2);
              cx.stroke();

              if (isDir || e.directed) {
                const ang = Math.atan2(dy, dx);
                const head = 8;
                cx.fillStyle = strokeCol;
                cx.beginPath();
                cx.moveTo(x2, y2);
                cx.lineTo(x2 - head * Math.cos(ang - Math.PI / 6), y2 - head * Math.sin(ang - Math.PI / 6));
                cx.lineTo(x2 - head * Math.cos(ang + Math.PI / 6), y2 - head * Math.sin(ang + Math.PI / 6));
                cx.closePath();
                cx.fill();
              }

              const wLabel = e.weight ?? e.label;
              if (wLabel) {
                cx.fillStyle = textCol;
                cx.fillText(String(wLabel), (x1 + x2) / 2, (y1 + y2) / 2 - 8);
              }
            }

            const contrastTextForColor = (color: string, fallback: string): string => {
              const s = color.trim();
              let r: number | undefined;
              let g: number | undefined;
              let b: number | undefined;
              const hex6 = /^#([0-9a-fA-F]{6})$/.exec(s);
              if (hex6) {
                r = parseInt(hex6[1].slice(0, 2), 16);
                g = parseInt(hex6[1].slice(2, 4), 16);
                b = parseInt(hex6[1].slice(4, 6), 16);
              } else {
                const hex3 = /^#([0-9a-fA-F]{3})$/.exec(s);
                if (hex3) {
                  r = parseInt(hex3[1][0] + hex3[1][0], 16);
                  g = parseInt(hex3[1][1] + hex3[1][1], 16);
                  b = parseInt(hex3[1][2] + hex3[1][2], 16);
                } else {
                  const rgb = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/i.exec(s);
                  if (rgb) {
                    r = Number(rgb[1]);
                    g = Number(rgb[2]);
                    b = Number(rgb[3]);
                  }
                }
              }
              if (r === undefined || g === undefined || b === undefined) return fallback;
              const lum = 0.299 * r + 0.587 * g + 0.114 * b;
              return lum < 140 ? "#FAFAFA" : "#09090B";
            };

            cx.font = `600 12px ${SANS}`;
            for (const nd of placed) {
              cx.beginPath();
              cx.arc(nd.x, nd.y, nodeR, 0, Math.PI * 2);
              cx.fillStyle = nd.color || nodeFill;
              cx.fill();
              cx.strokeStyle = strokeCol;
              cx.lineWidth = 1.8;
              cx.stroke();
              cx.fillStyle = nd.color ? contrastTextForColor(nd.color, textCol) : textCol;
              cx.fillText(nd.label, nd.x, nd.y);
            }
          }}
        />
        <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
          {t("tools.graphStats", { nodes: props.block.nodes.length, edges: props.block.edges.length })}
        </Text>
      </View>
    </BlockCard>
  );
}

export function OutputRenderer(props: {
  blocks: readonly ToolOutputBlock[];
  summary?: string | null;
  resultSeq?: number;
  graphEditorEnabled: boolean;
}) {
  const keyedBlocks = () => {
    const seq = props.resultSeq ?? 0;
    return props.blocks.map((block, idx) => ({
      key: `${seq}:${idx}:${block.type}`,
      block,
    }));
  };

  return (
    <View style={{ gap: SPACE[12] }}>
      <If when={Boolean(props.summary)}>
        <Text style={{ fontSize: FONT_SIZE.caption, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
          {props.summary ?? ""}
        </Text>
      </If>
      <Each of={keyedBlocks()} key={(item) => item.key}>
        {(item) => {
          const b = item().block;
          switch (b.type) {
            case "text":
              return <TextBlockView block={b} />;
            case "markdown":
              return <MarkdownBlockView block={b} />;
            case "table":
              return <TableBlockView block={b} />;
            case "svg":
              return <SvgBlockView block={b} />;
            case "image":
              return <ImageBlockView block={b} />;
            case "graph":
              return <GraphBlockView block={b} graphEditorEnabled={props.graphEditorEnabled} />;
          }
        }}
      </Each>
    </View>
  );
}
