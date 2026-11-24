// Desktop Plugin UI SDK (`ui.js`, Obsidian / VS Code trust model).
// Injects NSH primitives, styled `Button` / `Input` / reactive `If`, theme tokens,
// clipboard, toast, and Rust Core / `resvg` hooks into `ui.js` plugins.

import {
  Canvas,
  Clipboard,
  colorScheme,
  computed,
  effect,
  Scroll,
  signal,
  Text,
  View,
  type JSX,
  type TextStyle,
  type ViewStyle,
} from "nsh";
import { Input as NshInput } from "nsh/base";
import { Preview } from "xcpc-workstation/native";
import { preview } from "../store";
import { token } from "../theme";
import { showToast, type ToastTone } from "../Toast";
import { BUILTIN_UI_MODULES } from "./builtinUi";
import type { ToolExecutionResult } from "./types";

export interface PluginButtonProps {
  variant?: "default" | "outline" | "destructive" | "ghost";
  size?: "sm" | "md";
  disabled?: boolean;
  onPress?: () => void;
  style?: ViewStyle;
  children?: unknown;
}

export interface PluginInputProps {
  value?: string;
  placeholder?: string;
  multiline?: boolean;
  onChangeText?: (text: string) => void;
  onChange?: (e: { value: string }) => void;
  style?: TextStyle;
}

export interface PluginIfProps {
  when: unknown;
  else?: JSX.Element;
  children?: unknown;
}

export interface PluginUiSdk {
  h: (type: unknown, props?: Record<string, unknown> | null, ...children: unknown[]) => JSX.Element;
  signal: typeof signal;
  computed: typeof computed;
  effect: typeof effect;
  ui: {
    View: typeof View;
    Text: typeof Text;
    Scroll: typeof Scroll;
    Canvas: typeof Canvas;
    Button: (props: PluginButtonProps) => JSX.Element;
    Input: (props: PluginInputProps) => JSX.Element;
    If: (props: PluginIfProps) => JSX.Element;
  };
  token: typeof token;
  colorScheme: typeof colorScheme;
  runTool: (toolId: string, inputs?: Record<string, unknown>) => Promise<ToolExecutionResult>;
  renderSvgToPng: (svg: string, scale?: number) => Promise<string>;
  savePng: (pngBase64: string, defaultName?: string) => Promise<boolean>;
  saveText: (text: string, defaultName?: string) => Promise<boolean>;
  copyText: (text: string) => Promise<void>;
  toast: (text: string, tone?: ToastTone) => void;
  initialPayload: () => unknown;
}

function flattenChildren(raw: unknown[]): unknown[] {
  const out: unknown[] = [];
  for (const item of raw) {
    if (Array.isArray(item)) {
      out.push(...flattenChildren(item));
    } else if (item !== null && item !== undefined && typeof item !== "boolean") {
      out.push(item);
    }
  }
  return out;
}

/** Styled button adapter matching the `ui.js` Plugin SDK contract (`default | outline | destructive | ghost`). */
export function SdkButton(props: PluginButtonProps): JSX.Element {
  const variant = props.variant ?? "default";
  const sm = (props.size ?? "md") === "sm";
  const bgClass =
    variant === "default"
      ? "bg-primary hover:bg-sidebar-primary active:opacity-90"
      : variant === "destructive"
        ? "bg-chart-5 hover:opacity-90 active:opacity-80"
        : variant === "ghost"
          ? "hover:bg-muted active:bg-accent"
          : "border border-border bg-card hover:bg-muted active:bg-accent";
  const textClass =
    variant === "default" || variant === "destructive" ? "text-primary-foreground" : "text-foreground";
  const isPrimitive = typeof props.children === "string" || typeof props.children === "number";

  return (
    <View
      disabled={props.disabled}
      onPress={() => {
        if (!props.disabled) props.onPress?.();
      }}
      class={bgClass}
      style={{
        height: sm ? 28 : 32,
        paddingLeft: sm ? 10 : 12,
        paddingRight: sm ? 10 : 12,
        borderRadius: 6,
        alignItems: "center",
        justifyContent: "center",
        flexDirection: "row",
        gap: 6,
        opacity: props.disabled ? 0.5 : 1,
        ...props.style,
      }}
    >
      {isPrimitive ? (
        <Text style={{ fontSize: sm ? 12 : 13, fontWeight: 500 }} class={textClass}>
          {String(props.children)}
        </Text>
      ) : (
        (props.children as JSX.Element)
      )}
    </View>
  );
}

/** Styled text input adapter bridging `onChangeText` to NSH's `Input` `onChange`. */
export function SdkInput(props: PluginInputProps): JSX.Element {
  const multiline = Boolean(props.multiline);
  return (
    <NshInput
      value={props.value ?? ""}
      placeholder={props.placeholder}
      multiline={multiline}
      autoCapitalize="none"
      autoCorrect={false}
      onChange={(e) => {
        props.onChangeText?.(e.value);
        props.onChange?.(e);
      }}
      class="border border-border bg-background text-foreground placeholder:text-muted-foreground focus:border-ring"
      style={{
        borderRadius: 6,
        paddingLeft: 10,
        paddingRight: 10,
        paddingTop: multiline ? 8 : 4,
        paddingBottom: multiline ? 8 : 4,
        minHeight: multiline ? 120 : 32,
        fontSize: 13,
        ...props.style,
      }}
    />
  );
}

/** Reactive conditional container supporting `when` as a value or thunk, and `children` as an element or reactive render thunk. */
export function SdkIf(props: PluginIfProps): JSX.Element {
  const cond = () => (typeof props.when === "function" ? Boolean((props.when as () => unknown)()) : Boolean(props.when));
  let seq = 0;
  const rendered = computed<{ key: number; el: JSX.Element } | null>(() => {
    if (!cond()) return null;
    const ch = props.children;
    const el = typeof ch === "function" ? (ch as () => JSX.Element)() : (ch as JSX.Element);
    seq += 1;
    return { key: seq, el };
  });
  return (
    <If when={Boolean(rendered())} else={props.else}>
      <Each of={rendered() ? [rendered()!] : []} key={(item) => item.key}>
        {(item) => item().el}
      </Each>
    </If>
  );
}

/** Hyperscript helper passed as `sdk.h` to `ui.js` plugins. */
export function sdkH(
  type: unknown,
  props?: Record<string, unknown> | null,
  ...rawChildren: unknown[]
): JSX.Element {
  const flat = flattenChildren(rawChildren);
  const childProp =
    flat.length === 0 ? undefined : flat.length === 1 ? flat[0] : flat;
  const merged: Record<string, unknown> = props ? { ...props } : {};
  if (childProp !== undefined) {
    merged.children = childProp;
  }
  const Comp = type as (p: Record<string, unknown>) => JSX.Element;
  return <Comp {...merged} />;
}

export interface CreatePluginSdkOptions {
  pluginId: string;
  getInitialPayload?: () => unknown;
  runToolOverride?: (toolId: string, inputs?: Record<string, unknown>) => Promise<ToolExecutionResult>;
}

/** Builds the `PluginUiSdk` object injected into a desktop `ui.js` plugin's `render(sdk)`. */
export function createPluginUiSdk(opts: CreatePluginSdkOptions): PluginUiSdk {
  return {
    h: sdkH,
    signal,
    computed,
    effect,
    ui: {
      View,
      Text,
      Scroll,
      Canvas,
      Button: SdkButton,
      Input: SdkInput,
      If: SdkIf,
    },
    token,
    colorScheme,
    runTool: async (toolId: string, inputs?: Record<string, unknown>) => {
      if (opts.runToolOverride) {
        return opts.runToolOverride(toolId, inputs);
      }
      const res = await preview.pluginRunTool(
        opts.pluginId,
        toolId,
        JSON.stringify(inputs ?? {}),
      );
      if (!res.ok || !res.reportJson) {
        throw new Error(res.error ?? "tool execution failed");
      }
      return JSON.parse(res.reportJson) as ToolExecutionResult;
    },
    renderSvgToPng: async (svg: string, scale = 2.0) => {
      const res = Preview.pluginRenderSvgPng(svg, scale);
      if (!res.ok || !res.pngBase64) {
        throw new Error(res.error ?? "SVG render failed");
      }
      return res.pngBase64;
    },
    savePng: async (pngBase64: string, defaultName = "output.png") => {
      const res = await Preview.pluginSavePngDialog(pngBase64, defaultName, null);
      return Boolean(res.ok);
    },
    saveText: async (text: string, defaultName = "output.txt") => {
      const res = await Preview.pluginSaveTextDialog(text, defaultName, null);
      return Boolean(res.ok);
    },
    copyText: async (text: string) => {
      await Clipboard.writeText(text);
    },
    toast: (text: string, tone: ToastTone = "success") => {
      showToast({ tone, text });
    },
    initialPayload: () => opts.getInitialPayload?.(),
  };
}

export type MountPluginUiResult =
  | { ok: true; element: JSX.Element }
  | { ok: false; error: string };

/** Evaluates a desktop plugin's `ui.js` source and calls `module.exports.render(sdk)` inside an error boundary. */
export function mountPluginUi(
  code: string,
  sdk: PluginUiSdk,
  opts?: { builtinPluginId?: string },
): MountPluginUiResult {
  if (opts?.builtinPluginId) {
    const builtinMod = BUILTIN_UI_MODULES[opts.builtinPluginId];
    if (builtinMod && typeof builtinMod.render === "function") {
      try {
        return { ok: true, element: builtinMod.render(sdk) };
      } catch (err) {
        return { ok: false, error: err instanceof Error ? err.message : String(err) };
      }
    }
  }
  try {
    const mod: { exports: Record<string, unknown> } = { exports: {} };
    const factory = new Function("sdk", "module", "exports", code) as (
      sdk: PluginUiSdk,
      module: { exports: Record<string, unknown> },
      exports: Record<string, unknown>,
    ) => unknown;
    factory(sdk, mod, mod.exports);
    const renderFn =
      typeof mod.exports?.render === "function"
        ? (mod.exports.render as (s: PluginUiSdk) => JSX.Element)
        : typeof mod.exports?.default === "function"
          ? (mod.exports.default as (s: PluginUiSdk) => JSX.Element)
          : undefined;
    if (!renderFn) {
      return { ok: false, error: "ui.js must export a render(sdk) function" };
    }
    const element = renderFn(sdk);
    return { ok: true, element };
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    return { ok: false, error: msg };
  }
}
