// Host-provided structured output block renderer for headless `index.js` tool plugins:
// `text`, `markdown`, `table`, `svg` (via `resvg` PNG preview + Copy SVG + Save PNG),
// `image` (PNG preview + Save PNG), and `graph` (read-only Canvas + open in graph-editor).

import { Canvas, Clipboard, colorScheme, computed, Image, Text, View, type JSX } from "nsh";
import { Preview } from "xcpc-workstation/native";
import { t } from "../i18n";
import { Btn } from "../kit";
import { openToolPlugin } from "../store";
import { cardShadow, MONO, token } from "../theme";
import { showToast } from "../Toast";
import type { GraphEdge, GraphNode, ToolOutputBlock } from "./types";

function BlockCard(props: {
  title?: string | null;
  actions?: JSX.Element;
  children?: JSX.Element;
}) {
  return (
    <View
      class="bg-card border border-border"
      style={{
        borderRadius: 10,
        overflow: "hidden",
        boxShadow: cardShadow(),
      }}
    >
      <If when={Boolean(props.title) || Boolean(props.actions)}>
        <View
          class="bg-muted/50 border-b border-border"
          style={{
            minHeight: 34,
            paddingLeft: 12,
            paddingRight: 10,
            paddingTop: 6,
            paddingBottom: 6,
            flexDirection: "row",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 8,
          }}
        >
          <Text
            style={{ fontSize: 12.5, fontWeight: 600, flexShrink: 1, numberOfLines: 1, textOverflow: "ellipsis" }}
            class="text-foreground"
          >
            {props.title ?? ""}
          </Text>
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
      <View style={{ padding: 12 }}>
        <Text
          style={{
            fontSize: 12.5,
            lineHeight: 19,
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
      <View style={{ padding: 12, gap: 4 }}>
        <Each of={lines()} key="index">
          {(line) => {
            const raw = line();
            if (raw.startsWith("### ")) {
              return (
                <Text style={{ fontSize: 13, fontWeight: 600, marginTop: 4 }} class="text-foreground">
                  {raw.slice(4)}
                </Text>
              );
            }
            if (raw.startsWith("## ")) {
              return (
                <Text style={{ fontSize: 14, fontWeight: 600, marginTop: 6 }} class="text-foreground">
                  {raw.slice(3)}
                </Text>
              );
            }
            if (raw.startsWith("# ")) {
              return (
                <Text style={{ fontSize: 15, fontWeight: 700, marginTop: 6 }} class="text-foreground">
                  {raw.slice(2)}
                </Text>
              );
            }
            if (raw.trim() === "") {
              return <View style={{ height: 4 }} />;
            }
            return (
              <Text
                style={{ fontSize: 12.5, lineHeight: 19, fontVariant: ["tabular-nums"] }}
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

function TableBlockView(props: { block: Extract<ToolOutputBlock, { type: "table" }> }) {
  const textAlignFor = (colIdx: number): "left" | "center" | "right" => {
    const a = props.block.align?.[colIdx];
    if (a === "right") return "right";
    if (a === "center") return "center";
    return "left";
  };

  return (
    <BlockCard title={props.block.title}>
      <View>
        <View
          class="bg-muted/30 border-b border-border"
          style={{ flexDirection: "row", paddingLeft: 12, paddingRight: 12, paddingTop: 7, paddingBottom: 7, gap: 8 }}
        >
          <Each of={props.block.headers} key="index">
            {(header, idx) => (
              <Text
                style={{
                  flex: 1,
                  fontSize: 11.5,
                  fontWeight: 600,
                  textAlign: textAlignFor(idx()),
                }}
                class="text-muted-foreground"
              >
                {header()}
              </Text>
            )}
          </Each>
        </View>
        <Each of={props.block.rows} key="index">
          {(row, rIdx) => (
            <View
              class={rIdx() + 1 < props.block.rows.length ? "border-b border-border" : undefined}
              style={{ flexDirection: "row", paddingLeft: 12, paddingRight: 12, paddingTop: 7, paddingBottom: 7, gap: 8 }}
            >
              <Each of={row()} key="index">
                {(cell, cIdx) => (
                  <Text
                    style={{
                      flex: 1,
                      fontSize: 12,
                      lineHeight: 17,
                      fontFamily: MONO,
                      fontVariant: ["tabular-nums"],
                      textAlign: textAlignFor(cIdx()),
                    }}
                    class="text-foreground"
                  >
                    {cell()}
                  </Text>
                )}
              </Each>
            </View>
          )}
        </Each>
      </View>
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

  return (
    <BlockCard
      title={props.block.title ?? "SVG"}
      actions={
        <View style={{ flexDirection: "row", gap: 4 }}>
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
      <View style={{ padding: 12, alignItems: "center", justifyContent: "center" }}>
        <If
          when={rendered().ok && rendered().pngBase64}
          else={
            <Text style={{ fontSize: 12 }} class="text-destructive">
              {rendered().error ?? t("tools.svgRenderFailed")}
            </Text>
          }
        >
          {(b64) => (
            <Image
              src={`data:image/png;base64,${b64()}`}
              style={{
                width: Math.min(props.block.width ?? 560, 680),
                height: Math.min(props.block.height ?? 360, 420),
                objectFit: "contain",
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
      <View style={{ padding: 12, alignItems: "center", justifyContent: "center" }}>
        <Image
          src={`data:image/png;base64,${props.block.png_base64}`}
          style={{
            width: Math.min(props.block.width ?? 560, 680),
            height: Math.min(props.block.height ?? 360, 420),
            objectFit: "contain",
          }}
        />
      </View>
    </BlockCard>
  );
}

function placePreviewNodes(nodes: readonly GraphNode[], w: number, h: number): { id: string; label: string; x: number; y: number; color?: string | null }[] {
  const n = nodes.length;
  if (n === 0) return [];
  const pad = 32;
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
            variant="primary"
            icon="arrowRight"
            label={t("tools.openInGraphEditor")}
            onPress={openInEditor}
          />
        ) : undefined
      }
    >
      <View style={{ padding: 12, gap: 6 }}>
        <Canvas
          style={{
            width: "100%",
            height: 260,
            borderRadius: 6,
            borderWidth: 1,
            borderColor: token("border"),
          }}
          draw={(cx, size) => {
            const dark = colorScheme() === "dark";
            cx.clearRect(0, 0, size.width, size.height);
            cx.fillStyle = dark ? "#18181b" : "#ffffff";
            cx.fillRect(0, 0, size.width, size.height);

            const placed = placePreviewNodes(props.block.nodes, size.width, size.height);
            const byId = new Map(placed.map((n) => [n.id, n]));
            const strokeCol = dark ? "#94a3b8" : "#475569";
            const textCol = dark ? "#f8fafc" : "#0f172a";
            const nodeR = 15;
            const isDir = Boolean(props.block.directed);

            cx.strokeStyle = strokeCol;
            cx.lineWidth = 1.8;
            cx.font = "11px JetBrains Mono, monospace";
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

            cx.font = "600 12px JetBrains Mono, monospace";
            for (const nd of placed) {
              cx.beginPath();
              cx.arc(nd.x, nd.y, nodeR, 0, Math.PI * 2);
              cx.fillStyle = nd.color || (dark ? "#27272a" : "#dbeafe");
              cx.fill();
              cx.strokeStyle = strokeCol;
              cx.lineWidth = 1.8;
              cx.stroke();
              cx.fillStyle = nd.color ? "#0f172a" : textCol;
              cx.fillText(nd.label, nd.x, nd.y);
            }
          }}
        />
        <Text style={{ fontSize: 11.5, fontVariant: ["tabular-nums"] }} class="text-muted-foreground">
          {t("tools.graphStats", { nodes: props.block.nodes.length, edges: props.block.edges.length })}
        </Text>
      </View>
    </BlockCard>
  );
}

export function OutputRenderer(props: {
  blocks: readonly ToolOutputBlock[];
  summary?: string | null;
  graphEditorEnabled: boolean;
}) {
  return (
    <View style={{ gap: 12 }}>
      <If when={Boolean(props.summary)}>
        <Text style={{ fontSize: 12.5, fontVariant: ["tabular-nums"] }} class="text-secondary-foreground">
          {props.summary ?? ""}
        </Text>
      </If>
      <Each of={props.blocks} key="index">
        {(blk) => {
          const b = blk();
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
