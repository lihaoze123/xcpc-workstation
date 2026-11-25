// @xcpc/plugin-sdk/ui — Desktop NSH UI SDK types & virtual module exports

import type { ToolExecutionResult, ToolOutputBlock } from "./host";

export type MaybeAccessor<T> = T | (() => T);

export interface Signal<T> {
  (): T;
  set(next: T | ((prev: T) => T)): void;
}

export type StyleObject = Record<string, unknown>;
export type ElementNode = unknown;

export type SdkButtonVariant =
  | "default"
  | "primary"
  | "outline"
  | "destructive"
  | "danger"
  | "ghost";

export interface PluginButtonProps {
  variant?: MaybeAccessor<SdkButtonVariant>;
  size?: "sm" | "md";
  icon?: MaybeAccessor<string | undefined>;
  disabled?: MaybeAccessor<boolean>;
  onPress?: () => void;
  style?: MaybeAccessor<StyleObject>;
  textStyle?: MaybeAccessor<StyleObject>;
  tooltip?: MaybeAccessor<string | undefined>;
  testID?: string;
  children?: MaybeAccessor<unknown>;
}

export interface PluginIconProps {
  name: MaybeAccessor<string>;
  size?: number;
  tone?: MaybeAccessor<string | undefined>;
  style?: MaybeAccessor<StyleObject | undefined>;
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
  maxWidth?: number | string;
}

export interface PluginInputProps {
  value?: MaybeAccessor<string>;
  placeholder?: MaybeAccessor<string>;
  multiline?: boolean;
  disabled?: MaybeAccessor<boolean>;
  onChangeText?: (text: string) => void;
  onChange?: (e: { value: string }) => void;
  style?: MaybeAccessor<StyleObject>;
  testID?: string;
}

export interface PluginIfProps {
  when: unknown;
  else?: ElementNode;
  children?: unknown;
}

export interface PluginEachProps<T = unknown> {
  of: MaybeAccessor<readonly T[]>;
  key?: ((item: T, index: number) => string | number) | string;
  children?: (item: () => T, index: () => number) => ElementNode;
}

export interface PluginFlexProps {
  gap?: MaybeAccessor<number | undefined>;
  align?: MaybeAccessor<string | undefined>;
  justify?: MaybeAccessor<string | undefined>;
  wrap?: MaybeAccessor<boolean | undefined>;
  padding?: MaybeAccessor<number | undefined>;
  style?: MaybeAccessor<StyleObject | undefined>;
  class?: MaybeAccessor<string | undefined>;
  testID?: string;
  onPress?: () => void;
  children?: unknown;
}

export interface PluginCardProps {
  style?: MaybeAccessor<StyleObject | undefined>;
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
  style?: MaybeAccessor<StyleObject | undefined>;
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
  style?: MaybeAccessor<StyleObject | undefined>;
  minWidth?: number;
  testID?: string;
}

export type PluginBadgeTone = "neutral" | "success" | "warning" | "danger";

export interface PluginBadgeProps {
  tone?: MaybeAccessor<PluginBadgeTone | undefined>;
  style?: MaybeAccessor<StyleObject | undefined>;
  textStyle?: MaybeAccessor<StyleObject | undefined>;
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
  get<T = unknown>(key: string): T | undefined;
  set(key: string, value: unknown): void;
  delete(key: string): boolean;
}

export type PluginMenuItem =
  | {
      id: string;
      label: string;
      icon?: string;
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
  trigger: ElementNode;
  align?: "start" | "end";
  stayOpen?: boolean;
  minWidth?: number;
}

export interface PluginModalProps {
  open: MaybeAccessor<boolean>;
  onClose: () => void;
  title?: MaybeAccessor<string | undefined>;
  description?: MaybeAccessor<string | undefined>;
  icon?: string;
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
  style?: MaybeAccessor<StyleObject | undefined>;
  testID?: string;
}

export interface PluginCodeEditorProps {
  value?: MaybeAccessor<string>;
  language?: MaybeAccessor<string | undefined>;
  placeholder?: MaybeAccessor<string | undefined>;
  readOnly?: MaybeAccessor<boolean>;
  onChange?: (text: string) => void;
  style?: MaybeAccessor<StyleObject | undefined>;
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
  hasWorkspace(): boolean;
  currentProblemId(): string | undefined;
  listProblems(): PluginWorkspaceProblemSummary[];
  getProblem(id: string): PluginWorkspaceProblemDetail | null;
}

export interface ShortcutOptions {
  when?: "always" | "notEditing";
}

export interface PluginUiComponents {
  View: (props: Record<string, unknown>) => ElementNode;
  Text: (props: Record<string, unknown>) => ElementNode;
  Scroll: (props: Record<string, unknown>) => ElementNode;
  Canvas: (props: Record<string, unknown>) => ElementNode;
  Button: (props: PluginButtonProps) => ElementNode;
  Icon: (props: PluginIconProps) => ElementNode;
  Segmented: (props: PluginSegmentedProps) => ElementNode;
  Chip: (props: PluginChipProps) => ElementNode;
  Input: (props: PluginInputProps) => ElementNode;
  If: (props: PluginIfProps) => ElementNode;
  Each: <T = unknown>(props: PluginEachProps<T>) => ElementNode;
  Row: (props: PluginFlexProps) => ElementNode;
  Col: (props: PluginFlexProps) => ElementNode;
  Card: (props: PluginCardProps) => ElementNode;
  Switch: (props: PluginSwitchProps) => ElementNode;
  Select: (props: PluginSelectProps) => ElementNode;
  Badge: (props: PluginBadgeProps) => ElementNode;
  Menu: (props: PluginMenuProps) => ElementNode;
  Modal: (props: PluginModalProps) => ElementNode;
  Image: (props: PluginImageProps) => ElementNode;
  CodeEditor: (props: PluginCodeEditorProps) => ElementNode;
  Output: (props: PluginOutputProps) => ElementNode;
}

export interface PluginUiSdk {
  h: (
    type: unknown,
    props?: Record<string, unknown> | null,
    ...children: unknown[]
  ) => ElementNode;
  signal: <T>(initial: T) => Signal<T>;
  computed: <T>(fn: () => T) => () => T;
  effect: (fn: () => void) => () => void;
  untracked: <T>(fn: () => T) => T;
  ui: PluginUiComponents;
  canvas: {
    fitBounds: (
      points: readonly CanvasPoint2D[],
      viewport: CanvasViewport2D,
      padding?: number,
    ) => FitBoundsResult;
  };
  token: (name: string) => string;
  colorScheme: () => "light" | "dark";
  runTool: (
    toolId: string,
    inputs?: Record<string, unknown>,
  ) => Promise<ToolExecutionResult>;
  useTool: (
    toolId: string,
    inputs: MaybeAccessor<Record<string, unknown>>,
    options?: UseToolOptions,
  ) => UseToolHandle;
  onCleanup: (fn: () => void) => void;
  onShortcut: (
    keystroke: string,
    handler: () => void,
    options?: ShortcutOptions,
  ) => () => void;
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
  toast: (text: string, tone?: "success" | "issues" | "info") => void;
  initialPayload: () => unknown;
}

export declare const h: PluginUiSdk["h"];
export declare function Fragment(props: { children?: unknown }): ElementNode;
export declare const signal: PluginUiSdk["signal"];
export declare const computed: PluginUiSdk["computed"];
export declare const effect: PluginUiSdk["effect"];
export declare const untracked: PluginUiSdk["untracked"];
export declare const ui: PluginUiSdk["ui"];
export declare const canvas: PluginUiSdk["canvas"];
export declare const token: PluginUiSdk["token"];
export declare const colorScheme: PluginUiSdk["colorScheme"];
export declare const runTool: PluginUiSdk["runTool"];
export declare const useTool: PluginUiSdk["useTool"];
export declare const onCleanup: PluginUiSdk["onCleanup"];
export declare const onShortcut: PluginUiSdk["onShortcut"];
export declare const storage: PluginUiSdk["storage"];
export declare const workspace: PluginUiSdk["workspace"];
export declare const openTool: PluginUiSdk["openTool"];
export declare const pickFile: PluginUiSdk["pickFile"];
export declare const readClipboard: PluginUiSdk["readClipboard"];
export declare const openExternal: PluginUiSdk["openExternal"];
export declare const renderSvgToPng: PluginUiSdk["renderSvgToPng"];
export declare const savePng: PluginUiSdk["savePng"];
export declare const saveText: PluginUiSdk["saveText"];
export declare const copyText: PluginUiSdk["copyText"];
export declare const toast: PluginUiSdk["toast"];
export declare const initialPayload: PluginUiSdk["initialPayload"];
export declare function getSdk(): PluginUiSdk;

declare global {
  namespace JSX {
    type Element = ElementNode;
    interface ElementChildrenAttribute {
      children: {};
    }
    interface IntrinsicElements {
      [elemName: string]: Record<string, unknown>;
    }
  }
}
