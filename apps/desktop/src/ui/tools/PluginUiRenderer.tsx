// Native NSH renderer for Core QuickJS declarative plugin UI sessions (`PluginUiSession`).
// Zero `eval` / `new Function()` — works identically in dev and hardened release builds.

import {
  Canvas,
  cleanup as nshOnCleanup,
  Clipboard,
  colorScheme,
  effect,
  keymap,
  Scroll,
  signal,
  Text,
  untracked,
  View,
  type JSX,
  type ViewStyle,
} from "nsh";
import { Preview } from "xcpc-workstation/native";
import { t } from "../i18n";
import { Icon, type IconName, type IconTone } from "../Icon";
import { openToolPlugin, preview } from "../store";
import { token } from "../theme";
import { showToast, type ToastTone } from "../Toast";
import {
  SdkBadge,
  SdkButton,
  SdkCard,
  SdkChip,
  SdkCodeEditor,
  SdkCol,
  SdkIcon,
  SdkImage,
  SdkInput,
  SdkMenu,
  SdkModal,
  SdkOutput,
  SdkRow,
  SdkSegmented,
  SdkSelect,
  SdkSwitch,
  SdkText,
  SdkView,
  type PluginBadgeTone,
  type PluginMenuItem,
  type PluginSegmentedItem,
  type PluginSelectOption,
  type SdkButtonVariant,
} from "./pluginSdk";
import type { ToolOutputBlock } from "./types";

interface SerializedVNode {
  type: string;
  props: Record<string, unknown>;
  children: SerializedVNode[];
}

interface UiHostEffect {
  id: number;
  kind: string;
  payload: Record<string, unknown>;
}

interface UiShortcutSpec {
  id: string;
  keystroke: string;
  when: "always" | "notEditing";
}

interface UiRenderFrame {
  tree: SerializedVNode;
  effects: UiHostEffect[];
  shortcuts: UiShortcutSpec[];
}

const THEME_TOKEN_KEYS = [
  "background",
  "foreground",
  "card",
  "card-foreground",
  "popover",
  "popover-foreground",
  "primary",
  "primary-foreground",
  "secondary",
  "secondary-foreground",
  "muted",
  "muted-foreground",
  "accent",
  "accent-foreground",
  "destructive",
  "border",
  "input",
  "ring",
  "chart-1",
  "chart-2",
  "chart-3",
  "chart-4",
  "chart-5",
] as const;

function snapshotThemeTokens(): string {
  const map: Record<string, string> = {};
  for (const k of THEME_TOKEN_KEYS) {
    try {
      map[k] = String(token(k));
    } catch {
      // Ignore unknown token.
    }
  }
  return JSON.stringify(map);
}

/** Replays a Canvas2D command list recorded inside Core QuickJS onto an NSH Canvas context. */
export function replayCanvasOps(cx: any, ops: unknown): void {
  if (!Array.isArray(ops)) return;
  for (const op of ops) {
    if (!Array.isArray(op) || op.length === 0) continue;
    const cmd = op[0];
    switch (cmd) {
      case "fillStyle":
        cx.fillStyle = String(op[1]);
        break;
      case "strokeStyle":
        cx.strokeStyle = String(op[1]);
        break;
      case "lineWidth":
        cx.lineWidth = Number(op[1]);
        break;
      case "font":
        cx.font = String(op[1]);
        break;
      case "textAlign":
        cx.textAlign = String(op[1]);
        break;
      case "textBaseline":
        cx.textBaseline = String(op[1]);
        break;
      case "globalAlpha":
        cx.globalAlpha = Number(op[1]);
        break;
      case "clearRect":
        cx.clearRect(Number(op[1]), Number(op[2]), Number(op[3]), Number(op[4]));
        break;
      case "fillRect":
        cx.fillRect(Number(op[1]), Number(op[2]), Number(op[3]), Number(op[4]));
        break;
      case "strokeRect":
        cx.strokeRect(Number(op[1]), Number(op[2]), Number(op[3]), Number(op[4]));
        break;
      case "beginPath":
        cx.beginPath();
        break;
      case "closePath":
        cx.closePath();
        break;
      case "moveTo":
        cx.moveTo(Number(op[1]), Number(op[2]));
        break;
      case "lineTo":
        cx.lineTo(Number(op[1]), Number(op[2]));
        break;
      case "arc":
        cx.arc(
          Number(op[1]),
          Number(op[2]),
          Number(op[3]),
          Number(op[4]),
          Number(op[5]),
          Boolean(op[6]),
        );
        break;
      case "stroke":
        cx.stroke();
        break;
      case "fill":
        cx.fill();
        break;
      case "fillText":
        cx.fillText(String(op[1]), Number(op[2]), Number(op[3]));
        break;
      case "strokeText":
        cx.strokeText(String(op[1]), Number(op[2]), Number(op[3]));
        break;
      case "save":
        cx.save();
        break;
      case "restore":
        cx.restore();
        break;
      case "translate":
        cx.translate(Number(op[1]), Number(op[2]));
        break;
      case "scale":
        cx.scale(Number(op[1]), Number(op[2]));
        break;
      case "rotate":
        cx.rotate(Number(op[1]));
        break;
      case "setLineDash":
        if (typeof cx.setLineDash === "function" && Array.isArray(op[1])) {
          cx.setLineDash(op[1].map(Number));
        }
        break;
    }
  }
}

export function PluginUiRenderer(props: {
  pluginId: string;
  revision: number;
  getInitialPayload?: () => unknown;
}) {
  const frame = signal<UiRenderFrame | null>(null);
  const frameSeq = signal<number>(0);
  const error = signal<string | null>(null);

  let sessionId: number | null = null;
  let unbindShortcuts: Array<() => void> = [];
  let moveTimer: ReturnType<typeof setTimeout> | undefined;
  let pendingMove: { cb: string; payload: unknown } | null = null;

  const clearShortcuts = () => {
    for (const fn of unbindShortcuts) {
      try {
        fn();
      } catch {
        // Ignore unbind errors.
      }
    }
    unbindShortcuts = [];
  };

  const disposeSession = () => {
    clearShortcuts();
    if (moveTimer !== undefined) {
      clearTimeout(moveTimer);
      moveTimer = undefined;
    }
    pendingMove = null;
    if (sessionId !== null) {
      const sid = sessionId;
      sessionId = null;
      preview.pluginUiDispose(sid);
    }
  };

  nshOnCleanup(() => {
    disposeSession();
  });

  const syncShortcuts = (specs: UiShortcutSpec[]) => {
    clearShortcuts();
    if (!specs || specs.length === 0) return;
    for (const sc of specs) {
      try {
        const unbind = keymap.bind(
          sc.keystroke,
          () => dispatch(sc.id),
          { when: sc.when === "always" ? "always" : "notEditing" },
        );
        unbindShortcuts.push(unbind);
      } catch {
        // Skip malformed keystroke.
      }
    }
  };

  const runEffects = (sid: number, effects: UiHostEffect[]) => {
    for (const ef of effects) {
      const p = ef.payload ?? {};
      switch (ef.kind) {
        case "toast":
          showToast({
            tone: (p.tone as ToastTone) ?? "success",
            text: String(p.text ?? ""),
          });
          break;
        case "copyText":
          void Clipboard.writeText(String(p.text ?? ""));
          break;
        case "openTool":
          openToolPlugin(String(p.pluginId ?? ""), { payload: p.payload });
          break;
        case "openExternal":
          Preview.openExternalUrl(String(p.url ?? ""));
          break;
        case "saveText":
          void (async () => {
            try {
              const res = await Preview.pluginSaveTextDialog(
                String(p.text ?? ""),
                String(p.defaultName ?? "output.txt"),
                null,
              );
              resolveEffect(sid, ef.id, { value: Boolean(res.ok) });
            } catch (err) {
              resolveEffect(sid, ef.id, {
                error: err instanceof Error ? err.message : String(err),
              });
            }
          })();
          break;
        case "savePng":
          void (async () => {
            try {
              const res = await Preview.pluginSavePngDialog(
                String(p.pngBase64 ?? ""),
                String(p.defaultName ?? "output.png"),
                null,
              );
              resolveEffect(sid, ef.id, { value: Boolean(res.ok) });
            } catch (err) {
              resolveEffect(sid, ef.id, {
                error: err instanceof Error ? err.message : String(err),
              });
            }
          })();
          break;
        case "pickFile":
          void (async () => {
            try {
              const exts = Array.isArray(p.extensions) ? p.extensions.map(String) : null;
              const title = p.title ? String(p.title) : null;
              const res = await Preview.pluginPickFileDialog(exts, title);
              if (res.cancelled) {
                resolveEffect(sid, ef.id, { value: null });
              } else if (!res.ok || !res.contentBase64) {
                resolveEffect(sid, ef.id, { error: res.error ?? "failed to read file" });
              } else {
                resolveEffect(sid, ef.id, {
                  value: {
                    name: res.name ?? "file",
                    text: res.text ?? undefined,
                    contentBase64: res.contentBase64,
                  },
                });
              }
            } catch (err) {
              resolveEffect(sid, ef.id, {
                error: err instanceof Error ? err.message : String(err),
              });
            }
          })();
          break;
        case "readClipboard":
          void (async () => {
            try {
              const txt = await Clipboard.readText();
              resolveEffect(sid, ef.id, { value: txt });
            } catch (err) {
              resolveEffect(sid, ef.id, {
                error: err instanceof Error ? err.message : String(err),
              });
            }
          })();
          break;
      }
    }
  };

  const applyFrameResult = (res: {
    ok: boolean;
    sessionId?: number | null;
    frameJson?: string | null;
    error?: string | null;
  }) => {
    if (!res.ok || !res.frameJson) {
      error.set(res.error ?? t("tools.uiLoadFailed"));
      return;
    }
    try {
      const parsed = JSON.parse(res.frameJson) as UiRenderFrame;
      error.set(null);
      frame.set(parsed);
      frameSeq.set(frameSeq() + 1);
      syncShortcuts(parsed.shortcuts ?? []);
      if (sessionId !== null && parsed.effects && parsed.effects.length > 0) {
        runEffects(sessionId, parsed.effects);
      }
    } catch (err) {
      error.set(err instanceof Error ? err.message : String(err));
    }
  };

  const dispatch = (cbId: unknown, payload?: unknown) => {
    if (sessionId === null || typeof cbId !== "string" || !cbId) return;
    const payloadJson = payload === undefined ? null : JSON.stringify(payload);
    const res = preview.pluginUiDispatch(sessionId, cbId, payloadJson);
    applyFrameResult(res);
  };

  const dispatchThrottledMove = (cbId: unknown, payload: unknown) => {
    if (typeof cbId !== "string" || !cbId) return;
    pendingMove = { cb: cbId, payload };
    if (moveTimer !== undefined) return;
    moveTimer = setTimeout(() => {
      moveTimer = undefined;
      const cur = pendingMove;
      pendingMove = null;
      if (cur) dispatch(cur.cb, cur.payload);
    }, 16);
  };

  const resolveEffect = (sid: number, effectId: number, result: unknown) => {
    if (sessionId !== sid) return;
    const res = preview.pluginUiResolveEffect(sid, effectId, JSON.stringify(result));
    applyFrameResult(res);
  };

  // Mount (or remount on pluginId / pluginsRevision change)
  effect(() => {
    const id = props.pluginId;
    void props.revision;
    untracked(() => {
      disposeSession();
      const payload = props.getInitialPayload?.();
      const payloadJson = payload === undefined ? null : JSON.stringify(payload);
      const res = preview.pluginUiMount(
        id,
        payloadJson,
        snapshotThemeTokens(),
        colorScheme(),
      );
      if (res.ok && res.sessionId != null) {
        sessionId = res.sessionId;
      }
      applyFrameResult(res);
    });
  });

  // Sync light/dark theme changes into active QuickJS session
  let lastScheme = colorScheme();
  effect(() => {
    const scheme = colorScheme();
    if (scheme === lastScheme) return;
    lastScheme = scheme;
    if (sessionId === null) return;
    untracked(() => {
      if (sessionId === null) return;
      const res = preview.pluginUiUpdateTheme(sessionId, snapshotThemeTokens(), scheme);
      applyFrameResult(res);
    });
  });

  const renderVNode = (node: SerializedVNode): JSX.Element => {
    const p = node.props ?? {};
    const kids = (node.children ?? []).map(renderVNode);
    const childEl = kids.length === 0 ? undefined : kids.length === 1 ? kids[0] : kids;

    switch (node.type) {
      case "View":
        return (
          <SdkView
            testID={p.testID as string | undefined}
            class={p.class as string | undefined}
            style={p.style as ViewStyle | undefined}
            disabled={p.disabled as boolean | undefined}
            tooltip={p.tooltip as string | undefined}
            onPress={typeof p.onPress === "string" ? () => dispatch(p.onPress) : undefined}
          >
            {childEl}
          </SdkView>
        );
      case "Row":
        return (
          <SdkRow
            testID={p.testID as string | undefined}
            gap={p.gap as number | undefined}
            align={p.align as ViewStyle["alignItems"] | undefined}
            justify={p.justify as ViewStyle["justifyContent"] | undefined}
            wrap={p.wrap as boolean | undefined}
            padding={p.padding as number | undefined}
            class={p.class as string | undefined}
            style={p.style as ViewStyle | undefined}
            onPress={typeof p.onPress === "string" ? () => dispatch(p.onPress) : undefined}
          >
            {childEl}
          </SdkRow>
        );
      case "Col":
        return (
          <SdkCol
            testID={p.testID as string | undefined}
            gap={p.gap as number | undefined}
            align={p.align as ViewStyle["alignItems"] | undefined}
            justify={p.justify as ViewStyle["justifyContent"] | undefined}
            wrap={p.wrap as boolean | undefined}
            padding={p.padding as number | undefined}
            class={p.class as string | undefined}
            style={p.style as ViewStyle | undefined}
            onPress={typeof p.onPress === "string" ? () => dispatch(p.onPress) : undefined}
          >
            {childEl}
          </SdkCol>
        );
      case "Card":
        return (
          <SdkCard
            testID={p.testID as string | undefined}
            class={p.class as string | undefined}
            style={p.style as ViewStyle | undefined}
          >
            {childEl}
          </SdkCard>
        );
      case "Text":
        return (
          <SdkText
            testID={p.testID as string | undefined}
            class={p.class as string | undefined}
            style={p.style as Record<string, unknown> | undefined}
          >
            {String(p.text ?? "")}
          </SdkText>
        );
      case "Scroll":
        return (
          <Scroll
            testID={p.testID as string | undefined}
            style={p.style as ViewStyle | undefined}
            contentStyle={p.contentStyle as ViewStyle | undefined}
          >
            {childEl as JSX.Element}
          </Scroll>
        );
      case "Button":
        return (
          <SdkButton
            testID={p.testID as string | undefined}
            variant={p.variant as SdkButtonVariant | undefined}
            size={p.size as "sm" | "md" | undefined}
            icon={p.icon as IconName | undefined}
            disabled={p.disabled as boolean | undefined}
            style={p.style as ViewStyle | undefined}
            textStyle={p.textStyle as Record<string, unknown> | undefined}
            tooltip={p.tooltip as string | undefined}
            onPress={typeof p.onPress === "string" ? () => dispatch(p.onPress) : undefined}
          >
            {p.label !== undefined ? String(p.label) : childEl}
          </SdkButton>
        );
      case "Icon":
        return (
          <SdkIcon
            name={(p.name as IconName) ?? "squareCode"}
            size={p.size as number | undefined}
            tone={p.tone as IconTone | undefined}
            style={p.style as Record<string, unknown> | undefined}
          />
        );
      case "Segmented":
        return (
          <SdkSegmented
            items={(p.items as PluginSegmentedItem[]) ?? []}
            value={String(p.value ?? "")}
            itemWidth={p.itemWidth as number | undefined}
            underline={p.underline as boolean | undefined}
            disabled={p.disabled as boolean | undefined}
            onChange={(val) => dispatch(p.onChange, val)}
          />
        );
      case "Chip":
        return (
          <SdkChip
            label={String(p.label ?? "")}
            active={Boolean(p.active)}
            maxWidth={p.maxWidth as ViewStyle["maxWidth"]}
            onPress={typeof p.onPress === "string" ? () => dispatch(p.onPress) : undefined}
          />
        );
      case "Input":
        return (
          <SdkInput
            testID={p.testID as string | undefined}
            value={String(p.value ?? "")}
            placeholder={p.placeholder as string | undefined}
            multiline={Boolean(p.multiline)}
            disabled={Boolean(p.disabled)}
            style={p.style as Record<string, unknown> | undefined}
            onChangeText={(txt) => {
              if (typeof p.onChangeText === "string") dispatch(p.onChangeText, txt);
              if (typeof p.onChange === "string") dispatch(p.onChange, { value: txt });
            }}
          />
        );
      case "Switch":
        return (
          <SdkSwitch
            testID={p.testID as string | undefined}
            checked={p.checked !== undefined ? Boolean(p.checked) : Boolean(p.value)}
            disabled={Boolean(p.disabled)}
            label={p.label as string | undefined}
            style={p.style as ViewStyle | undefined}
            onChange={(next) => dispatch(p.onChange, next)}
          />
        );
      case "Select":
        return (
          <SdkSelect
            testID={p.testID as string | undefined}
            options={(p.options as PluginSelectOption[]) ?? []}
            value={String(p.value ?? "")}
            placeholder={p.placeholder as string | undefined}
            disabled={Boolean(p.disabled)}
            minWidth={p.minWidth as number | undefined}
            style={p.style as ViewStyle | undefined}
            onChange={(val) => dispatch(p.onChange, val)}
          />
        );
      case "Badge":
        return (
          <SdkBadge
            tone={p.tone as PluginBadgeTone | undefined}
            style={p.style as ViewStyle | undefined}
            textStyle={p.textStyle as Record<string, unknown> | undefined}
            label={p.label !== undefined ? String(p.label) : undefined}
          >
            {p.label !== undefined ? undefined : childEl}
          </SdkBadge>
        );
      case "Menu":
        return (
          <SdkMenu
            items={(p.items as PluginMenuItem[]) ?? []}
            align={p.align as "start" | "end" | undefined}
            stayOpen={Boolean(p.stayOpen)}
            minWidth={p.minWidth as number | undefined}
            onSelect={(id) => dispatch(p.onSelect, id)}
            trigger={
              p.triggerNode
                ? renderVNode(p.triggerNode as SerializedVNode)
                : <View />
            }
          />
        );
      case "Modal":
        return (
          <SdkModal
            open={Boolean(p.open)}
            onClose={() => dispatch(p.onClose)}
            title={p.title as string | undefined}
            description={p.description as string | undefined}
            icon={p.icon as IconName | undefined}
            width={p.width as number | undefined}
          >
            {childEl}
          </SdkModal>
        );
      case "Image":
        return (
          <SdkImage
            testID={p.testID as string | undefined}
            src={p.src as string | undefined}
            pngBase64={p.pngBase64 as string | undefined}
            svg={p.svg as string | undefined}
            svgScale={p.svgScale as number | undefined}
            width={p.width as number | string | undefined}
            height={p.height as number | string | undefined}
            fit={p.fit as "contain" | "cover" | "fill" | "scale-down" | "none" | undefined}
            style={p.style as Record<string, unknown> | undefined}
          />
        );
      case "CodeEditor":
        return (
          <SdkCodeEditor
            testID={p.testID as string | undefined}
            value={String(p.value ?? "")}
            language={p.language as string | undefined}
            placeholder={p.placeholder as string | undefined}
            readOnly={Boolean(p.readOnly)}
            style={p.style as ViewStyle | undefined}
            onChange={(txt) => dispatch(p.onChange, txt)}
          />
        );
      case "Output":
        return (
          <SdkOutput
            blocks={(p.blocks as ToolOutputBlock[]) ?? []}
            summary={p.summary as string | undefined}
            resultSeq={frameSeq()}
          />
        );
      case "Canvas": {
        const ops = p.drawOps;
        return (
          <Canvas
            testID={p.testID as string | undefined}
            style={p.style as ViewStyle | undefined}
            onLayout={
              typeof p.onLayout === "string"
                ? (e) => dispatch(p.onLayout, { bounds: e?.bounds })
                : undefined
            }
            onPressDown={
              typeof p.onPressDown === "string"
                ? (e) => dispatch(p.onPressDown, { position: e.position })
                : undefined
            }
            onMouseMove={
              typeof p.onMouseMove === "string"
                ? (e) => dispatchThrottledMove(p.onMouseMove, { position: e.position })
                : undefined
            }
            onMouseUp={
              typeof p.onMouseUp === "string"
                ? () => dispatch(p.onMouseUp)
                : undefined
            }
            onMouseLeave={
              typeof p.onMouseLeave === "string"
                ? () => dispatch(p.onMouseLeave)
                : undefined
            }
            draw={(cx) => {
              void frameSeq();
              replayCanvasOps(cx, ops);
            }}
          />
        );
      }
      default:
        return <View style={p.style as ViewStyle | undefined}>{childEl as JSX.Element}</View>;
    }
  };

  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <If
        when={!error() && frame()}
        else={
          <View style={{ flex: 1, alignItems: "center", justifyContent: "center", padding: 24, gap: 8 }}>
            <Icon name="circleAlert" size={22} tone="destructive" />
            <Text style={{ fontSize: 13.5, fontWeight: 500 }} class="text-foreground">
              {t("tools.uiErrorTitle")}
            </Text>
            <Text style={{ fontSize: 12, textAlign: "center", maxWidth: 460 }} class="text-muted-foreground">
              {error() ?? t("tools.uiLoadFailed")}
            </Text>
          </View>
        }
      >
        <Each of={frame() ? [{ seq: frameSeq(), tree: frame()!.tree }] : []} key={(it) => it.seq}>
          {(it) => renderVNode(it().tree)}
        </Each>
      </If>
    </View>
  );
}
