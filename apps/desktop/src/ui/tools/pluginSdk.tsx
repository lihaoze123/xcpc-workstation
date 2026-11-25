// Desktop Plugin UI SDK (`ui.js`, Obsidian / VS Code trust model).
// Injects NSH primitives, reactive host design-system components (`Btn`, `Segmented`,
// `Chip`, `Input`, `If`), theme tokens, clipboard, toast, and Rust Core / `resvg` hooks.

import {
  Canvas,
  Clipboard,
  colorScheme,
  computed,
  effect,
  Scroll,
  signal,
  Text,
  untracked,
  View,
  type ImageStyle,
  type JSX,
  type TextStyle,
  type ViewStyle,
} from "nsh";
import { Input as NshInput } from "nsh/base";
import { Preview } from "xcpc-workstation/native";
import { Icon, type IconName, type IconTone } from "../Icon";
import { Btn, Chip, Segmented } from "../kit";
import { preview } from "../store";
import { token } from "../theme";
import { showToast, type ToastTone } from "../Toast";
import { BUILTIN_UI_MODULES } from "./builtinUi";
import { OutputRenderer } from "./OutputRenderer";
import type { ToolExecutionResult, ToolOutputBlock } from "./types";

export type MaybeAccessor<T> = T | (() => T);

function resolve<T>(v: MaybeAccessor<T>): T {
  return typeof v === "function" ? (v as () => T)() : v;
}

function resolveTextChildren(ch: unknown): string {
  if (typeof ch === "function") return resolveTextChildren((ch as () => unknown)());
  if (Array.isArray(ch)) return ch.map((item) => resolveTextChildren(item)).join("");
  if (ch === null || ch === undefined || typeof ch === "boolean") return "";
  return String(ch);
}

export type SdkButtonVariant = "default" | "primary" | "outline" | "destructive" | "danger" | "ghost";

export interface PluginButtonProps {
  variant?: MaybeAccessor<SdkButtonVariant>;
  size?: "sm" | "md";
  icon?: MaybeAccessor<IconName | undefined>;
  disabled?: MaybeAccessor<boolean>;
  onPress?: () => void;
  style?: MaybeAccessor<ViewStyle>;
  textStyle?: MaybeAccessor<TextStyle>;
  testID?: string;
  children?: MaybeAccessor<unknown>;
}

export interface PluginIconProps {
  name: MaybeAccessor<IconName>;
  size?: number;
  tone?: MaybeAccessor<IconTone | undefined>;
  style?: MaybeAccessor<ImageStyle | undefined>;
}

export interface PluginOutputProps {
  blocks: MaybeAccessor<readonly ToolOutputBlock[]>;
  summary?: MaybeAccessor<string | null | undefined>;
  resultSeq?: MaybeAccessor<number | undefined>;
}

export interface PluginSegmentedItem {
  value: string;
  label: string;
  testID?: string;
}

export interface PluginSegmentedProps {
  items: MaybeAccessor<readonly PluginSegmentedItem[]>;
  value: MaybeAccessor<string>;
  onChange: (value: string) => void;
  itemWidth?: number;
  underline?: boolean;
  disabled?: MaybeAccessor<boolean>;
}

export interface PluginChipProps {
  label: MaybeAccessor<string>;
  active?: MaybeAccessor<boolean>;
  onPress?: () => void;
  maxWidth?: ViewStyle["maxWidth"];
}

export interface PluginInputProps {
  value?: MaybeAccessor<string>;
  placeholder?: MaybeAccessor<string>;
  multiline?: boolean;
  disabled?: MaybeAccessor<boolean>;
  onChangeText?: (text: string) => void;
  onChange?: (e: { value: string }) => void;
  style?: MaybeAccessor<TextStyle>;
  testID?: string;
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
  untracked: typeof untracked;
  ui: {
    View: (props: Record<string, unknown>) => JSX.Element;
    Text: (props: Record<string, unknown>) => JSX.Element;
    Scroll: typeof Scroll;
    Canvas: typeof Canvas;
    Button: (props: PluginButtonProps) => JSX.Element;
    Icon: (props: PluginIconProps) => JSX.Element;
    Segmented: (props: PluginSegmentedProps) => JSX.Element;
    Chip: (props: PluginChipProps) => JSX.Element;
    Input: (props: PluginInputProps) => JSX.Element;
    If: (props: PluginIfProps) => JSX.Element;
    Output: (props: PluginOutputProps) => JSX.Element;
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

function mapButtonVariant(v: SdkButtonVariant | undefined): "primary" | "outline" | "ghost" | "danger" {
  switch (v) {
    case "default":
    case "primary":
      return "primary";
    case "destructive":
    case "danger":
      return "danger";
    case "ghost":
      return "ghost";
    default:
      return "outline";
  }
}

/** Reactive View adapter allowing `style`, `class`, `disabled`, and `children` to be getter functions. */
export function SdkView(props: Record<string, unknown>): JSX.Element {
  const rest: Record<string, unknown> = {};
  for (const k of Object.keys(props)) {
    if (k !== "style" && k !== "class" && k !== "disabled" && k !== "children") {
      rest[k] = props[k];
    }
  }
  return (
    <View
      {...rest}
      style={resolve(props.style as MaybeAccessor<ViewStyle | undefined>)}
      class={resolve(props.class as MaybeAccessor<string | undefined>)}
      disabled={resolve(props.disabled as MaybeAccessor<boolean | undefined>)}
    >
      {(typeof props.children === "function" ? (props.children as () => unknown)() : props.children) as JSX.Element}
    </View>
  );
}

/** Reactive Text adapter allowing `style`, `class`, and text `children` to be getter functions. */
export function SdkText(props: Record<string, unknown>): JSX.Element {
  const rest: Record<string, unknown> = {};
  for (const k of Object.keys(props)) {
    if (k !== "style" && k !== "class" && k !== "children") {
      rest[k] = props[k];
    }
  }
  return (
    <Text
      {...rest}
      style={resolve(props.style as MaybeAccessor<TextStyle | undefined>)}
      class={resolve(props.class as MaybeAccessor<string | undefined>)}
    >
      {resolveTextChildren(props.children)}
    </Text>
  );
}

/** Styled button adapter backed by host `Btn`, supporting reactive `variant`, `disabled`, `style`, and `children`. */
export function SdkButton(props: PluginButtonProps): JSX.Element {
  const renderBtn = (variant: "primary" | "outline" | "ghost" | "danger") => {
    const ch = () => resolve(props.children);
    const isPrim = () => {
      const c = ch();
      return typeof c === "string" || typeof c === "number";
    };
    return (
      <Btn
        testID={props.testID}
        variant={variant}
        size={props.size ?? "sm"}
        icon={resolve(props.icon)}
        disabled={Boolean(resolve(props.disabled))}
        onPress={props.onPress}
        style={resolve(props.style)}
        textStyle={resolve(props.textStyle)}
        label={isPrim() ? String(ch()) : undefined}
      >
        {isPrim() ? undefined : (ch() as JSX.Element)}
      </Btn>
    );
  };

  if (typeof props.variant === "function") {
    return (
      <Each of={[mapButtonVariant(resolve(props.variant))]} key={(v) => v}>
        {(v) => renderBtn(v())}
      </Each>
    );
  }
  return renderBtn(mapButtonVariant(props.variant));
}

/** Host `Icon` adapter exposing Lucide line icons to `ui.js` plugins. */
export function SdkIcon(props: PluginIconProps): JSX.Element {
  return (
    <Icon
      name={resolve(props.name)}
      size={props.size}
      tone={resolve(props.tone)}
      style={resolve(props.style)}
    />
  );
}

/** Host `Segmented` adapter (light gray track + white active pill) for plugin option groups. */
export function SdkSegmented(props: PluginSegmentedProps): JSX.Element {
  const items = () => resolve(props.items);
  const computedWidth = () => {
    if (props.itemWidth) return props.itemWidth;
    const maxLen = items().reduce((m, it) => Math.max(m, it.label.length), 2);
    return Math.max(54, Math.min(116, maxLen * 14 + 20));
  };
  return (
    <Segmented
      items={items()}
      value={resolve(props.value)}
      onChange={props.onChange}
      itemWidth={computedWidth()}
      underline={props.underline}
      disabled={Boolean(resolve(props.disabled))}
    />
  );
}

/** Host `Chip` adapter supporting reactive `label` and `active` state. */
export function SdkChip(props: PluginChipProps): JSX.Element {
  if (typeof props.active === "function") {
    return (
      <Each of={[Boolean(resolve(props.active))]} key={(a) => (a ? "1" : "0")}>
        {(a) => (
          <Chip
            label={resolve(props.label)}
            active={a()}
            onPress={props.onPress}
            maxWidth={props.maxWidth}
          />
        )}
      </Each>
    );
  }
  return (
    <Chip
      label={resolve(props.label)}
      active={Boolean(props.active)}
      onPress={props.onPress}
      maxWidth={props.maxWidth}
    />
  );
}

/** Styled text input adapter bridging `onChangeText` to NSH's `Input` `onChange`, with reactive props. */
export function SdkInput(props: PluginInputProps): JSX.Element {
  const multiline = Boolean(props.multiline);
  return (
    <NshInput
      testID={props.testID}
      value={resolve(props.value) ?? ""}
      placeholder={resolve(props.placeholder)}
      disabled={Boolean(resolve(props.disabled))}
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
        ...resolve(props.style),
      }}
    />
  );
}

/** Reactive conditional container supporting `when` as a value or thunk, and `children` as an element or render thunk. */
export function SdkIf(props: PluginIfProps): JSX.Element {
  const condVal = () => (typeof props.when === "function" ? (props.when as () => unknown)() : props.when);
  let seq = 0;
  const rendered = computed<{ key: number; el: JSX.Element } | null>(() => {
    const val = condVal();
    if (!val) return null;
    const ch = props.children;
    const el = untracked(() => (typeof ch === "function" ? (ch as (v: unknown) => JSX.Element)(val) : (ch as JSX.Element)));
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

/** Host `OutputRenderer` adapter so `ui.js` plugins share identical table widths, spacing, and typography. */
export function SdkOutput(props: PluginOutputProps): JSX.Element {
  return (
    <OutputRenderer
      blocks={resolve(props.blocks) ?? []}
      summary={resolve(props.summary)}
      resultSeq={resolve(props.resultSeq)}
      graphEditorEnabled={false}
    />
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
    untracked,
    ui: {
      View: SdkView,
      Text: SdkText,
      Scroll,
      Canvas,
      Button: SdkButton,
      Icon: SdkIcon,
      Segmented: SdkSegmented,
      Chip: SdkChip,
      Input: SdkInput,
      If: SdkIf,
      Output: SdkOutput,
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
