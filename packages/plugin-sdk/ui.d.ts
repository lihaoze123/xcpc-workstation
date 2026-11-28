// @xcpc-workstation/plugin-sdk/ui — Typed `window.xcpc` capability bridge for WebView plugins

import type {
  CreateProblemInput,
  RenderedTypstOutput,
  RenderMathOptions,
  RenderTypstOptions,
  SetSamplesResult,
  ToolExecutionResult,
  UpdateProblemPatch,
  UpsertSnippetInput,
  WorkspaceContestDetail,
  WorkspaceContestSummary,
  WorkspaceProblemDetail,
  WorkspaceProblemSummary,
  WorkspaceSample,
  WorkspaceSnippetDetail,
  WorkspaceSnippetSummary,
  WorkspaceTrdDetail,
  WorkspaceTrdSummary,
} from "./host";

export type {
  CreateProblemInput,
  RenderedTypstOutput,
  RenderMathOptions,
  RenderTypstOptions,
  SetSamplesResult,
  UpdateProblemPatch,
  UpsertSnippetInput,
  WorkspaceContestDetail,
  WorkspaceContestSummary,
  WorkspaceProblemDetail,
  WorkspaceProblemSummary,
  WorkspaceSample,
  WorkspaceSnippetDetail,
  WorkspaceSnippetSummary,
  WorkspaceTrdDetail,
  WorkspaceTrdSummary,
};

export type XcpcColorScheme = "light" | "dark";

export interface XcpcTheme {
  scheme: XcpcColorScheme;
  tokens?: Record<string, string>;
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

export interface XcpcPickFilesOptions extends XcpcPickFileOptions {
  multiple?: boolean;
  /** Single-file byte limit (default 10 MiB, max 30 MiB total across all files). */
  maxBytes?: number;
}

export interface XcpcSaveBinaryOptions {
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
  listSnippets(): Promise<WorkspaceSnippetSummary[]>;
  getSnippet(id: string): Promise<WorkspaceSnippetDetail | null>;
  listTrds(): Promise<WorkspaceTrdSummary[]>;
  getTrd(id: string): Promise<WorkspaceTrdDetail | null>;
  createProblem(input: CreateProblemInput): Promise<WorkspaceProblemDetail>;
  updateProblem(id: string, patch: UpdateProblemPatch): Promise<WorkspaceProblemDetail>;
  setSamples(problemId: string, samples: WorkspaceSample[]): Promise<SetSamplesResult>;
  upsertSnippet(input: UpsertSnippetInput): Promise<WorkspaceSnippetDetail>;
}

export interface XcpcRenderBridge {
  renderTypst(source: string, opts?: RenderTypstOptions): Promise<RenderedTypstOutput>;
  renderMath(latex: string, opts?: RenderMathOptions): Promise<RenderedTypstOutput>;
}

export interface XcpcOjAccount {
  platform: string;
  handle: string;
  rating?: number;
  max_rating?: number;
  rank?: string;
}

export interface XcpcOjBridge {
  /**
   * Lists the user's bound OJ accounts (platform, handle, cached rating/rank).
   * Read-only; exposes no credentials or private submission data.
   */
  listAccounts(): Promise<XcpcOjAccount[]>;
}

export interface XcpcClipboardBridge {
  writeText(text: string): Promise<void>;
  /**
   * Copies an image to the system clipboard.
   * Accepts either raw PNG base64 (matching `files.savePng`) or a
   * `data:image/...;base64,...` data URL (such as `canvas.toDataURL("image/png")`).
   */
  writeImage(pngBase64: string): Promise<void>;
}

export interface XcpcFilesBridge {
  saveText(defaultName: string, content: string): Promise<string | null>;
  savePng(defaultName: string, pngBase64: string): Promise<string | null>;
  saveBinary(
    defaultName: string,
    base64: string,
    options?: XcpcSaveBinaryOptions,
  ): Promise<string | null>;
  pickFile(options?: XcpcPickFileOptions): Promise<XcpcPickedFile | null>;
  pickFiles(options?: XcpcPickFilesOptions): Promise<XcpcPickedFile[] | null>;
}

export interface XcpcBridge {
  runTool<T = ToolExecutionResult>(
    toolId: string,
    inputs?: Record<string, unknown>,
  ): Promise<T>;
  readonly storage: XcpcStorageBridge;
  readonly workspace: XcpcWorkspaceBridge;
  readonly render: XcpcRenderBridge;
  readonly oj: XcpcOjBridge;
  toast(message: string, kind?: "info" | "success" | "error"): void;
  readonly clipboard: XcpcClipboardBridge;
  readonly files: XcpcFilesBridge;
  openTool(pluginId: string, toolId?: string, payload?: unknown): void;
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
