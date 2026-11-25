// @xcpc/plugin-sdk/ui — Typed `window.xcpc` capability bridge for WebView plugins

import type {
  ToolExecutionResult,
  WorkspaceProblemDetail,
  WorkspaceProblemSummary,
} from "./host";

export type XcpcColorScheme = "light" | "dark";

export interface XcpcTheme {
  scheme: XcpcColorScheme;
  tokens?: Record<string, string>;
}

export interface WorkspaceContestSummary {
  id: string;
  title: string;
  subtitle?: string;
  problem_count?: number;
  frozen?: boolean;
}

export interface WorkspaceContestDetail extends WorkspaceContestSummary {
  problems?: WorkspaceProblemSummary[];
  [key: string]: unknown;
}

export interface XcpcPickedFile {
  name: string;
  content: string;
  base64?: string;
}

export interface XcpcPickFileOptions {
  title?: string;
  extensions?: string[];
}

export interface XcpcStorageBridge {
  get<T = unknown>(key: string): Promise<T | null>;
  set(key: string, value: unknown): Promise<void>;
  remove(key: string): Promise<void>;
}

export interface XcpcWorkspaceBridge {
  getProblem(id: string): Promise<WorkspaceProblemDetail | null>;
  listProblems(): Promise<WorkspaceProblemSummary[]>;
  getContest(id: string): Promise<WorkspaceContestDetail | null>;
  listContests(): Promise<WorkspaceContestSummary[]>;
}

export interface XcpcClipboardBridge {
  writeText(text: string): Promise<void>;
  readText(): Promise<string>;
}

export interface XcpcFilesBridge {
  saveText(defaultName: string, content: string): Promise<string | null>;
  savePng(defaultName: string, pngBase64: string): Promise<string | null>;
  pickFile(options?: XcpcPickFileOptions): Promise<XcpcPickedFile | null>;
}

export interface XcpcBridge {
  runTool<T = ToolExecutionResult>(
    toolId: string,
    inputs?: Record<string, unknown>,
  ): Promise<T>;
  readonly storage: XcpcStorageBridge;
  readonly workspace: XcpcWorkspaceBridge;
  toast(message: string, kind?: "info" | "success" | "error"): void;
  readonly clipboard: XcpcClipboardBridge;
  readonly files: XcpcFilesBridge;
  openTool(pluginId: string, toolId?: string, payload?: unknown): void;
  openExternal(url: string): void;
  readonly theme: XcpcTheme;
  onThemeChange(callback: (theme: XcpcTheme) => void): () => void;
  readonly initialPayload?: unknown;
}

declare global {
  interface Window {
    xcpc?: XcpcBridge;
  }
}

/**
 * Returns the injected `window.xcpc` capability bridge inside an XCPC Workstation WebView.
 * Throws if called outside an XCPC Workstation plugin WebView host.
 */
export function getXcpc(): XcpcBridge;
