// Desktop Plugin UI SDK (`ui.js`, Obsidian / VS Code trust model).
// Injects NSH primitives, reactive host design-system components (`Btn`, `Segmented`,
// `Chip`, `Input`, `If`), theme tokens, clipboard, toast, and Rust Core / `resvg` hooks.

import {
  Canvas,
  Clipboard,
  colorScheme,
  computed,
  effect,
  Image,
  keymap,
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
import { Editor as NshEditor, Input as NshInput, Switch as NshSwitch } from "nsh/base";
import { Preview } from "xcpc-workstation/native";
import { Icon, type IconName, type IconTone } from "../Icon";
import { Btn, Chip, Segmented, tip } from "../kit";
import { AppMenu, type MenuEntry } from "../Menu";
import { DialogHeader, Modal } from "../Modal";
import { hasWorkspace, openToolPlugin, preview, studio } from "../store";
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
  tooltip?: MaybeAccessor<string | undefined>;
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

export interface PluginEachProps<T = unknown> {
  of: MaybeAccessor<readonly T[]>;
  key?: ((item: T, index: number) => string | number) | string;
  children?: (item: () => T, index: () => number) => JSX.Element;
}

export interface PluginFlexProps {
  gap?: MaybeAccessor<number | undefined>;
  align?: MaybeAccessor<ViewStyle["alignItems"] | undefined>;
  justify?: MaybeAccessor<ViewStyle["justifyContent"] | undefined>;
  wrap?: MaybeAccessor<boolean | undefined>;
  padding?: MaybeAccessor<number | undefined>;
  style?: MaybeAccessor<ViewStyle | undefined>;
  class?: MaybeAccessor<string | undefined>;
  testID?: string;
  onPress?: () => void;
  children?: unknown;
}

export interface PluginCardProps {
  style?: MaybeAccessor<ViewStyle | undefined>;
  class?: MaybeAccessor<string | undefined>;
  testID?: string;
  children?: unknown;
}

export interface PluginSwitchProps {
  checked?: MaybeAccessor<boolean>;
  value?: MaybeAccessor<boolean>;
  disabled?: MaybeAccessor<boolean>;
  label?: MaybeAccessor<string | undefined>;
  onChange?: (checked: boolean) => void;
  style?: MaybeAccessor<ViewStyle | undefined>;
  testID?: string;
}

export interface PluginSelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}

export interface PluginSelectProps {
  options: MaybeAccessor<readonly PluginSelectOption[]>;
  value: MaybeAccessor<string>;
  placeholder?: MaybeAccessor<string | undefined>;
  disabled?: MaybeAccessor<boolean>;
  onChange: (value: string) => void;
  style?: MaybeAccessor<ViewStyle | undefined>;
  minWidth?: number;
  testID?: string;
}

export type PluginBadgeTone = "neutral" | "success" | "warning" | "danger";

export interface PluginBadgeProps {
  tone?: MaybeAccessor<PluginBadgeTone | undefined>;
  style?: MaybeAccessor<ViewStyle | undefined>;
  textStyle?: MaybeAccessor<TextStyle | undefined>;
  children?: MaybeAccessor<unknown>;
  label?: MaybeAccessor<string | undefined>;
}

export interface CanvasPoint2D {
  x: number;
  y: number;
}

export interface CanvasViewport2D {
  width: number;
  height: number;
}

export interface FitBoundsResult {
  scale: number;
  offsetX: number;
  offsetY: number;
  centerX: number;
  centerY: number;
}

export interface UseToolOptions {
  debounceMs?: number;
  immediate?: boolean;
  enabled?: MaybeAccessor<boolean>;
}

export interface UseToolHandle {
  data: () => ToolExecutionResult | undefined;
  loading: () => boolean;
  error: () => string | undefined;
  refresh: () => void;
}

export interface PluginStorageApi {
  get: <T = unknown>(key: string) => T | undefined;
  set: (key: string, value: unknown) => void;
  delete: (key: string) => boolean;
}

export type PluginMenuItem =
  | {
      id: string;
      label: string;
      icon?: IconName;
      checked?: boolean;
      keys?: string;
      disabled?: boolean;
      destructive?: boolean;
      note?: string;
    }
  | { separator: true; id: string };

export interface PluginMenuProps {
  items: MaybeAccessor<readonly PluginMenuItem[]>;
  onSelect: (id: string) => void;
  trigger: JSX.Element;
  align?: "start" | "end";
  stayOpen?: boolean;
  minWidth?: number;
}

export interface PluginModalProps {
  open: MaybeAccessor<boolean>;
  onClose: () => void;
  title?: MaybeAccessor<string | undefined>;
  description?: MaybeAccessor<string | undefined>;
  icon?: IconName;
  width?: number;
  children?: unknown;
}

export interface PluginImageProps {
  src?: MaybeAccessor<string | undefined>;
  pngBase64?: MaybeAccessor<string | undefined>;
  svg?: MaybeAccessor<string | undefined>;
  svgScale?: number;
  width?: MaybeAccessor<number | string | undefined>;
  height?: MaybeAccessor<number | string | undefined>;
  fit?: "contain" | "cover" | "fill" | "scale-down" | "none";
  style?: MaybeAccessor<ImageStyle | undefined>;
  testID?: string;
}

export interface PluginCodeEditorProps {
  value?: MaybeAccessor<string>;
  language?: MaybeAccessor<string | undefined>;
  placeholder?: MaybeAccessor<string | undefined>;
  readOnly?: MaybeAccessor<boolean>;
  onChange?: (text: string) => void;
  style?: MaybeAccessor<ViewStyle | undefined>;
  testID?: string;
}

export interface PickFileOptions {
  extensions?: string[];
  title?: string;
}

export interface PickedFileResult {
  name: string;
  text?: string;
  contentBase64: string;
}

export interface PluginWorkspaceProblemSummary {
  id: string;
  title: string;
  tags: string[];
  rating?: number | null;
}

export interface PluginWorkspaceProblemDetail extends PluginWorkspaceProblemSummary {
  statementMd: string;
  editorialMd: string;
  samples: Array<{ input: string; answer: string }>;
}

export interface PluginWorkspaceApi {
  hasWorkspace: () => boolean;
  currentProblemId: () => string | undefined;
  listProblems: () => PluginWorkspaceProblemSummary[];
  getProblem: (id: string) => PluginWorkspaceProblemDetail | null;
}

export interface ShortcutOptions {
  when?: "always" | "notEditing";
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
    Each: <T = unknown>(props: PluginEachProps<T>) => JSX.Element;
    Row: (props: PluginFlexProps) => JSX.Element;
    Col: (props: PluginFlexProps) => JSX.Element;
    Card: (props: PluginCardProps) => JSX.Element;
    Switch: (props: PluginSwitchProps) => JSX.Element;
    Select: (props: PluginSelectProps) => JSX.Element;
    Badge: (props: PluginBadgeProps) => JSX.Element;
    Menu: (props: PluginMenuProps) => JSX.Element;
    Modal: (props: PluginModalProps) => JSX.Element;
    Image: (props: PluginImageProps) => JSX.Element;
    CodeEditor: (props: PluginCodeEditorProps) => JSX.Element;
    Output: (props: PluginOutputProps) => JSX.Element;
  };
  canvas: {
    fitBounds: (
      points: readonly CanvasPoint2D[],
      viewport: CanvasViewport2D,
      padding?: number,
    ) => FitBoundsResult;
  };
  token: typeof token;
  colorScheme: typeof colorScheme;
  runTool: (toolId: string, inputs?: Record<string, unknown>) => Promise<ToolExecutionResult>;
  useTool: (
    toolId: string,
    inputs: MaybeAccessor<Record<string, unknown>>,
    options?: UseToolOptions,
  ) => UseToolHandle;
  onCleanup: (fn: () => void) => void;
  onShortcut: (keystroke: string, handler: () => void, options?: ShortcutOptions) => () => void;
  storage: PluginStorageApi;
  workspace: PluginWorkspaceApi;
  openTool: (pluginId: string, payload?: unknown) => void;
  pickFile: (options?: PickFileOptions) => Promise<PickedFileResult | null>;
  readClipboard: () => Promise<string>;
  openExternal: (url: string) => boolean;
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

/** Reactive View adapter allowing `style`, `class`, `disabled`, `tooltip`, and `children` to be getter functions. */
export function SdkView(props: Record<string, unknown>): JSX.Element {
  const rest: Record<string, unknown> = {};
  for (const k of Object.keys(props)) {
    if (k !== "style" && k !== "class" && k !== "disabled" && k !== "tooltip" && k !== "children") {
      rest[k] = props[k];
    }
  }
  const tipText = () => resolve(props.tooltip as MaybeAccessor<string | undefined>);
  return (
    <View
      {...rest}
      style={resolve(props.style as MaybeAccessor<ViewStyle | undefined>)}
      class={resolve(props.class as MaybeAccessor<string | undefined>)}
      disabled={resolve(props.disabled as MaybeAccessor<boolean | undefined>)}
      tooltip={tipText() ? tip(tipText()!) : undefined}
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
        tooltip={resolve(props.tooltip) ? tip(resolve(props.tooltip)!) : undefined}
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

/** Reactive list renderer wrapping NSH `<Each>`. */
export function SdkEach<T = unknown>(props: PluginEachProps<T>): JSX.Element {
  const keyFn =
    typeof props.key === "function"
      ? props.key
      : typeof props.key === "string"
        ? (item: T) => String((item as Record<string, unknown>)?.[props.key as string] ?? "")
        : (_item: T, index: number) => index;
  return (
    <Each of={resolve(props.of) ?? []} key={keyFn}>
      {(item, index) => (props.children ? props.children(item, index) : <View />)}
    </Each>
  );
}

function makeFlexAdapter(direction: "row" | "column") {
  return function SdkFlex(props: PluginFlexProps): JSX.Element {
    const mergedStyle = (): ViewStyle => {
      const base: ViewStyle = { flexDirection: direction };
      const gap = resolve(props.gap);
      if (gap !== undefined) base.gap = gap;
      const align = resolve(props.align);
      if (align !== undefined) base.alignItems = align;
      const justify = resolve(props.justify);
      if (justify !== undefined) base.justifyContent = justify;
      const wrap = resolve(props.wrap);
      if (wrap !== undefined) base.flexWrap = wrap ? "wrap" : "nowrap";
      const padding = resolve(props.padding);
      if (padding !== undefined) base.padding = padding;
      return { ...base, ...resolve(props.style) };
    };
    return (
      <View
        testID={props.testID}
        onPress={props.onPress}
        class={resolve(props.class)}
        style={mergedStyle()}
      >
        {(typeof props.children === "function" ? (props.children as () => unknown)() : props.children) as JSX.Element}
      </View>
    );
  };
}

/** Horizontal Flex container primitive. */
export const SdkRow = makeFlexAdapter("row");

/** Vertical Flex container primitive. */
export const SdkCol = makeFlexAdapter("column");

/** Design-system surface card container (`bg-card`, `border-border`, `borderRadius: 10`). */
export function SdkCard(props: PluginCardProps): JSX.Element {
  return (
    <View
      testID={props.testID}
      class={resolve(props.class) ? `bg-card border border-border ${resolve(props.class)}` : "bg-card border border-border"}
      style={{
        borderRadius: 10,
        padding: 12,
        gap: 8,
        ...resolve(props.style),
      }}
    >
      {(typeof props.children === "function" ? (props.children as () => unknown)() : props.children) as JSX.Element}
    </View>
  );
}

/** Boolean toggle switch backed by `nsh/base` `Switch`. */
export function SdkSwitch(props: PluginSwitchProps): JSX.Element {
  const isChecked = () => {
    if (props.checked !== undefined) return Boolean(resolve(props.checked));
    return Boolean(resolve(props.value));
  };
  const labelText = () => resolve(props.label);
  const sw = (
    <NshSwitch
      testID={props.testID}
      checked={isChecked()}
      disabled={Boolean(resolve(props.disabled))}
      onChange={(e) => props.onChange?.(e.checked)}
      trackClass="w-8 h-[18px] rounded-full bg-input checked:bg-primary disabled:opacity-50 transition-colors duration-[120ms]"
      thumbClass="size-[14px] m-[2px] rounded-full bg-white shadow-sm checked:translate-x-[14px] transition-transform duration-[120ms]"
    />
  );
  if (props.label === undefined) {
    return props.style ? <View style={resolve(props.style)}>{sw}</View> : sw;
  }
  return (
    <View
      style={{
        flexDirection: "row",
        alignItems: "center",
        gap: 8,
        ...resolve(props.style),
      }}
    >
      {sw}
      <If when={Boolean(labelText())}>
        <Text style={{ fontSize: 12.5 }} class="text-foreground">
          {labelText() ?? ""}
        </Text>
      </If>
    </View>
  );
}

/** Dropdown selector backed by host `AppMenu`. */
export function SdkSelect(props: PluginSelectProps): JSX.Element {
  const options = () => resolve(props.options) ?? [];
  const currentValue = () => resolve(props.value);
  const currentLabel = () => {
    const cur = currentValue();
    const hit = options().find((o) => o.value === cur);
    return hit?.label ?? resolve(props.placeholder) ?? cur ?? "";
  };
  const disabled = () => Boolean(resolve(props.disabled));
  return (
    <AppMenu
      minWidth={props.minWidth ?? 160}
      entries={options().map((o) => ({
        id: o.value,
        label: o.label,
        checked: o.value === currentValue(),
        disabled: o.disabled,
      }))}
      onSelect={(val) => {
        if (!disabled()) props.onChange(val);
      }}
      trigger={
        <View
          testID={props.testID}
          class={
            disabled()
              ? "border border-border bg-muted/40 opacity-50"
              : "border border-border bg-background hover:bg-muted/60 active:bg-accent/60"
          }
          style={{
            height: 32,
            paddingLeft: 10,
            paddingRight: 8,
            borderRadius: 6,
            flexDirection: "row",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 8,
            minWidth: props.minWidth ?? 120,
            ...resolve(props.style),
          }}
        >
          <Text
            style={{ fontSize: 12.5, numberOfLines: 1, textOverflow: "ellipsis", flexShrink: 1 }}
            class="text-foreground"
          >
            {currentLabel()}
          </Text>
          <Icon name="chevronDown" size={13} tone="muted-foreground" />
        </View>
      }
    />
  );
}

/** Restrained neutral / semantic status badge (never uses brand tint backgrounds). */
export function SdkBadge(props: PluginBadgeProps): JSX.Element {
  const tone = (): PluginBadgeTone => resolve(props.tone) ?? "neutral";
  const badgeClasses = () => {
    switch (tone()) {
      case "success":
        return "bg-muted border border-border";
      case "warning":
        return "bg-muted border border-border";
      case "danger":
        return "bg-destructive/10 border border-destructive/30";
      default:
        return "bg-muted border border-border";
    }
  };
  const textClass = () => {
    switch (tone()) {
      case "success":
        return "text-chart-2";
      case "warning":
        return "text-chart-4";
      case "danger":
        return "text-destructive";
      default:
        return "text-secondary-foreground";
    }
  };
  const textContent = () =>
    props.label !== undefined
      ? String(resolve(props.label) ?? "")
      : resolveTextChildren(props.children);

  return (
    <View
      class={badgeClasses()}
      style={{
        paddingLeft: 6,
        paddingRight: 6,
        paddingTop: 1,
        paddingBottom: 1,
        borderRadius: 5,
        alignSelf: "flex-start",
        ...resolve(props.style),
      }}
    >
      <Text
        class={textClass()}
        style={{ fontSize: 11, fontWeight: 500, lineHeight: 15, ...resolve(props.textStyle) }}
      >
        {textContent()}
      </Text>
    </View>
  );
}

/** Host `AppMenu` adapter providing native boundary flipping, keyboard navigation, and consistent shadows. */
export function SdkMenu(props: PluginMenuProps): JSX.Element {
  return (
    <AppMenu
      entries={[...(resolve(props.items) ?? [])] as MenuEntry[]}
      onSelect={props.onSelect}
      trigger={props.trigger}
      align={props.align}
      stayOpen={props.stayOpen}
      minWidth={props.minWidth}
    />
  );
}

/** Host `Modal` adapter with optional `DialogHeader` title/description. */
export function SdkModal(props: PluginModalProps): JSX.Element {
  const title = () => resolve(props.title);
  const desc = () => resolve(props.description);
  return (
    <Modal open={Boolean(resolve(props.open))} onClose={props.onClose} width={props.width ?? 440}>
      <View style={{ padding: 18, gap: 14 }}>
        <If when={Boolean(title())}>
          <DialogHeader
            icon={props.icon ?? "squareCode"}
            title={title() ?? ""}
            description={desc()}
          />
        </If>
        {(typeof props.children === "function" ? (props.children as () => unknown)() : props.children) as JSX.Element}
      </View>
    </Modal>
  );
}

/** Image primitive supporting direct `src`, `pngBase64`, or live `svg` (rasterized via `resvg`). */
export function SdkImage(props: PluginImageProps): JSX.Element {
  const resolvedSrc = computed<string | undefined>(() => {
    const direct = resolve(props.src);
    if (direct) return direct;
    const b64 = resolve(props.pngBase64);
    if (b64) return b64.startsWith("data:") ? b64 : `data:image/png;base64,${b64}`;
    const svg = resolve(props.svg);
    if (svg) {
      const rendered = Preview.pluginRenderSvgPng(svg, props.svgScale ?? 2.0);
      if (rendered.ok && rendered.pngBase64) {
        return `data:image/png;base64,${rendered.pngBase64}`;
      }
    }
    return undefined;
  });

  return (
    <If when={Boolean(resolvedSrc())}>
      <Image
        testID={props.testID}
        src={resolvedSrc()!}
        style={{
          width: resolve(props.width) as ImageStyle["width"],
          height: resolve(props.height) as ImageStyle["height"],
          objectFit: props.fit ?? "contain",
          ...resolve(props.style),
        }}
      />
    </If>
  );
}

/** Code editor primitive backed by host NSH `Editor` (syntax highlighting, line numbers, undo/redo). */
export function SdkCodeEditor(props: PluginCodeEditorProps): JSX.Element {
  return (
    <NshEditor
      testID={props.testID}
      value={resolve(props.value) ?? ""}
      language={resolve(props.language) ?? "text"}
      placeholder={resolve(props.placeholder)}
      readOnly={Boolean(resolve(props.readOnly))}
      onChange={(e) => props.onChange?.(e.value)}
      class="border border-border bg-background text-foreground"
      style={{
        flex: 1,
        minHeight: 160,
        borderRadius: 8,
        ...resolve(props.style),
      }}
    />
  );
}

/** Computes a uniform scale & translation that centers `points` inside `viewport` with `padding`. */
export function fitBounds(
  points: readonly CanvasPoint2D[],
  viewport: CanvasViewport2D,
  padding = 32,
): FitBoundsResult {
  const w = Math.max(1, viewport.width);
  const h = Math.max(1, viewport.height);
  if (!points || points.length === 0) {
    return { scale: 1, offsetX: w / 2, offsetY: h / 2, centerX: 0, centerY: 0 };
  }
  let minX = Infinity;
  let maxX = -Infinity;
  let minY = Infinity;
  let maxY = -Infinity;
  for (const pt of points) {
    if (pt.x < minX) minX = pt.x;
    if (pt.x > maxX) maxX = pt.x;
    if (pt.y < minY) minY = pt.y;
    if (pt.y > maxY) maxY = pt.y;
  }
  const centerX = (minX + maxX) / 2;
  const centerY = (minY + maxY) / 2;
  const spanX = Math.max(1e-6, maxX - minX);
  const spanY = Math.max(1e-6, maxY - minY);
  const availW = Math.max(1, w - padding * 2);
  const availH = Math.max(1, h - padding * 2);
  const scale =
    points.length === 1
      ? 1
      : Math.max(0.05, Math.min(4, Math.min(availW / spanX, availH / spanY)));
  const offsetX = w / 2 - centerX * scale;
  const offsetY = h / 2 - centerY * scale;
  return { scale, offsetX, offsetY, centerX, centerY };
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
  storageOverride?: PluginStorageApi;
}

function createPluginStorage(pluginId: string, override?: PluginStorageApi): PluginStorageApi {
  if (override) return override;
  const fallback = new Map<string, unknown>();
  const hasNativeKv =
    typeof preview.pluginKvGet === "function" &&
    typeof preview.pluginKvSet === "function" &&
    typeof preview.pluginKvDelete === "function";

  return {
    get<T = unknown>(key: string): T | undefined {
      if (!hasNativeKv) {
        return fallback.get(key) as T | undefined;
      }
      try {
        const res = preview.pluginKvGet(pluginId, key);
        if (!res.ok || !res.found || res.valueJson === undefined || res.valueJson === null) {
          return undefined;
        }
        return JSON.parse(res.valueJson) as T;
      } catch {
        return fallback.get(key) as T | undefined;
      }
    },
    set(key: string, value: unknown): void {
      if (!hasNativeKv) {
        fallback.set(key, value);
        return;
      }
      try {
        const encoded = JSON.stringify(value === undefined ? null : value);
        const res = preview.pluginKvSet(pluginId, key, encoded);
        if (!res.ok) {
          fallback.set(key, value);
        }
      } catch {
        fallback.set(key, value);
      }
    },
    delete(key: string): boolean {
      if (!hasNativeKv) {
        return fallback.delete(key);
      }
      try {
        const res = preview.pluginKvDelete(pluginId, key);
        return Boolean(res.ok && res.found);
      } catch {
        return fallback.delete(key);
      }
    },
  };
}

/** Builds a `PluginUiSdk` together with its `cleanup()` runner (called on unmount / hot reload). */
export function createPluginUiSdkWithCleanup(opts: CreatePluginSdkOptions): {
  sdk: PluginUiSdk;
  cleanup: () => void;
} {
  const cleanups: Array<() => void> = [];
  let disposed = false;

  const onCleanup = (fn: () => void) => {
    if (typeof fn !== "function") return;
    if (disposed) {
      try {
        fn();
      } catch {
        // Ignore cleanup errors on already-disposed SDK.
      }
      return;
    }
    cleanups.push(fn);
  };

  const cleanup = () => {
    if (disposed) return;
    disposed = true;
    for (let i = cleanups.length - 1; i >= 0; i -= 1) {
      try {
        cleanups[i]();
      } catch {
        // Ensure remaining cleanups still run.
      }
    }
    cleanups.length = 0;
  };

  const runTool = async (toolId: string, inputs?: Record<string, unknown>) => {
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
  };

  const useTool = (
    toolId: string,
    inputsAccessor: MaybeAccessor<Record<string, unknown>>,
    options?: UseToolOptions,
  ): UseToolHandle => {
    const debounceMs = Math.max(0, options?.debounceMs ?? 120);
    const data = signal<ToolExecutionResult | undefined>(undefined);
    const loading = signal<boolean>(false);
    const error = signal<string | undefined>(undefined);
    const refreshTick = signal<number>(0);

    let reqSeq = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let firstRun = true;

    const clearPending = () => {
      if (timer !== undefined) {
        clearTimeout(timer);
        timer = undefined;
      }
    };

    const execute = (payload: Record<string, unknown>) => {
      if (disposed) return;
      const mySeq = ++reqSeq;
      loading.set(true);
      error.set(undefined);
      runTool(toolId, payload)
        .then((res) => {
          if (disposed || mySeq !== reqSeq) return;
          data.set(res);
          error.set(undefined);
          loading.set(false);
        })
        .catch((err) => {
          if (disposed || mySeq !== reqSeq) return;
          error.set(err instanceof Error ? err.message : String(err));
          loading.set(false);
        });
    };

    onCleanup(() => {
      reqSeq += 1;
      clearPending();
    });

    effect(() => {
      void refreshTick();
      const enabled = options?.enabled === undefined ? true : Boolean(resolve(options.enabled));
      if (!enabled) {
        clearPending();
        loading.set(false);
        return;
      }
      const snapshot = resolve(inputsAccessor) ?? {};
      const immediate = firstRun && (options?.immediate ?? true);
      firstRun = false;
      clearPending();
      if (immediate || debounceMs === 0) {
        execute(snapshot);
      } else {
        timer = setTimeout(() => {
          timer = undefined;
          execute(snapshot);
        }, debounceMs);
      }
    });

    return {
      data,
      loading,
      error,
      refresh: () => {
        firstRun = true;
        refreshTick.set(refreshTick() + 1);
      },
    };
  };

  const storage = createPluginStorage(opts.pluginId, opts.storageOverride);

  const onShortcut = (
    keystroke: string,
    handler: () => void,
    options?: ShortcutOptions,
  ): (() => void) => {
    if (disposed || typeof keymap?.bind !== "function") {
      return () => {};
    }
    const unbind = keymap.bind(
      keystroke,
      () => {
        if (!disposed) handler();
      },
      { when: options?.when ?? "notEditing" },
    );
    onCleanup(unbind);
    return unbind;
  };

  const workspace: PluginWorkspaceApi = {
    hasWorkspace: () => hasWorkspace(),
    currentProblemId: () => {
      const s = studio();
      return s?.target === "problem" ? s.id : undefined;
    },
    listProblems: () => {
      if (!hasWorkspace()) return [];
      return (preview.searchProblems({}) ?? []).map((p) => ({
        id: p.id,
        title: p.title,
        tags: [...(p.tags ?? [])],
        rating: p.rating ?? null,
      }));
    },
    getProblem: (id: string) => {
      if (!hasWorkspace() || !id) return null;
      const detail = preview.problemDetail(id);
      if (!detail || !detail.id) return null;
      const statement = preview.read(`problems/${id}/statement.md`);
      const editorial = preview.read(`problems/${id}/editorial.md`);
      return {
        id: detail.id,
        title: detail.title,
        tags: [...(detail.tags ?? [])],
        rating: detail.rating ?? null,
        statementMd: statement.text ?? "",
        editorialMd: editorial.text ?? "",
        samples: (detail.samples ?? []).map((s) => ({
          input: s.input ?? "",
          answer: s.output ?? "",
        })),
      };
    },
  };

  const sdk: PluginUiSdk = {
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
      Each: SdkEach,
      Row: SdkRow,
      Col: SdkCol,
      Card: SdkCard,
      Switch: SdkSwitch,
      Select: SdkSelect,
      Badge: SdkBadge,
      Menu: SdkMenu,
      Modal: SdkModal,
      Image: SdkImage,
      CodeEditor: SdkCodeEditor,
      Output: SdkOutput,
    },
    canvas: {
      fitBounds,
    },
    token,
    colorScheme,
    runTool,
    useTool,
    onCleanup,
    onShortcut,
    storage,
    workspace,
    openTool: (pluginId: string, payload?: unknown) => {
      openToolPlugin(pluginId, { payload });
    },
    pickFile: async (options?: PickFileOptions) => {
      const res = await Preview.pluginPickFileDialog(
        options?.extensions ?? null,
        options?.title ?? null,
      );
      if (res.cancelled) return null;
      if (!res.ok || !res.contentBase64) {
        throw new Error(res.error ?? "failed to read selected file");
      }
      return {
        name: res.name ?? "file",
        text: res.text ?? undefined,
        contentBase64: res.contentBase64,
      };
    },
    readClipboard: async () => {
      return Clipboard.readText();
    },
    openExternal: (url: string) => {
      const trimmed = String(url ?? "").trim();
      if (!/^https?:\/\//i.test(trimmed)) {
        throw new Error("openExternal only permits http:// and https:// URLs");
      }
      const res = Preview.openExternalUrl(trimmed);
      return Boolean(res.ok);
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

  return { sdk, cleanup };
}

/** Builds the `PluginUiSdk` object injected into a desktop `ui.js` plugin's `render(sdk)`. */
export function createPluginUiSdk(opts: CreatePluginSdkOptions): PluginUiSdk {
  return createPluginUiSdkWithCleanup(opts).sdk;
}

export type MountPluginUiResult =
  | { ok: true; element: JSX.Element }
  | { ok: false; error: string };

/** Evaluates a desktop plugin's `ui.js` source and calls `module.exports.render(sdk)` inside an error boundary. */
export function mountPluginUi(
  code: string,
  sdk: PluginUiSdk,
  opts?: { builtinPluginId?: string; pluginId?: string },
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
    const sourceUrlId = opts?.pluginId ?? opts?.builtinPluginId ?? "plugin";
    const annotated = `${code}\n//# sourceURL=xcpc-plugin://${sourceUrlId}/ui.js`;
    const factory = new Function("sdk", "module", "exports", annotated) as (
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
