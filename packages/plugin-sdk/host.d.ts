// @xcpc/plugin-sdk/host — QuickJS sandbox Host API & plugin module contracts

export interface HostHttpRequest {
  url: string;
  method?: "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD";
  headers?: Record<string, string>;
  body?: string;
  credentialHeader?:
    | { header: string; key: string; prefix?: string }
    | Record<string, string>;
  timeoutMs?: number;
}

export interface HostHttpResponse {
  status: number;
  headers: Record<string, string>;
  body: string;
}

export interface HtmlElementMatch {
  text: string;
  html: string;
  attrs: Record<string, string>;
}

export interface WorkspaceProblemSummary {
  id: string;
  title: string;
  tags: string[];
  difficulty?: string;
}

export interface WorkspaceProblemDetail extends WorkspaceProblemSummary {
  statement_md?: string;
  editorial_md?: string;
}

export interface RenderedRgba {
  width: number;
  height: number;
  rgba_base64: string;
}

export type HighlightTokenKind =
  | "keyword"
  | "type"
  | "string"
  | "number"
  | "comment"
  | "operator"
  | "ident"
  | "plain";

export interface HighlightToken {
  kind: HighlightTokenKind;
  text: string;
}

export interface HighlightLine {
  tokens: HighlightToken[];
}

export interface HighlightResult {
  language: string;
  lines: HighlightLine[];
}

export interface GraphNode {
  id: string;
  label?: string;
  x?: number;
  y?: number;
  color?: string;
}

export interface GraphEdge {
  from: string;
  to: string;
  weight?: string;
  label?: string;
  directed?: boolean;
}

export type ToolOutputBlock =
  | { type: "text"; title?: string; content: string; monospace?: boolean }
  | { type: "markdown"; title?: string; content: string }
  | {
      type: "table";
      title?: string;
      headers: string[];
      rows: string[][];
      align?: string[];
    }
  | {
      type: "graph";
      title?: string;
      directed?: boolean;
      weighted?: boolean;
      one_indexed?: boolean;
      nodes: GraphNode[];
      edges: GraphEdge[];
      layout?: string;
    }
  | { type: "svg"; title?: string; svg: string; width?: number; height?: number }
  | {
      type: "image";
      title?: string;
      png_base64: string;
      width?: number;
      height?: number;
    };

export interface ToolExecutionResult {
  blocks: ToolOutputBlock[];
  summary?: string;
}

export type PluginCommandAction =
  | { type: "message"; text: string }
  | {
      type: "open_tool";
      plugin_id: string;
      tool_id?: string;
      initial_inputs?: unknown;
    }
  | { type: "tool_result"; result: ToolExecutionResult }
  | { type: "none" };

export interface ImportFileInput {
  name: string;
  content_base64: string;
  text?: string;
}

export interface DraftSample {
  input: string;
  output: string;
}

export interface ProblemDraft {
  id?: string;
  title: string;
  statement_md: string;
  editorial_md?: string;
  tags?: string[];
  rating?: number;
  time_limit?: string;
  memory_limit?: string;
  samples: DraftSample[];
  source?: { platform: string; id: string; url?: string };
  assets?: Record<string, string>;
}

export interface ContestDraft {
  id?: string;
  title: string;
  subtitle?: string;
  problems: ProblemDraft[];
}

export type ImportDraft =
  | ({ kind: "problem" } & ProblemDraft)
  | ({ kind: "contest" } & ContestDraft);

export interface ExportContext {
  kind: "problem" | "contest";
  id: string;
  title: string;
  metadata: Record<string, unknown>;
  statement_md?: string;
  editorial_md?: string;
  samples?: DraftSample[];
  pdf_base64?: string;
  problems?: ExportContext[];
}

export interface ExportFileEntry {
  path: string;
  content_base64?: string;
  text?: string;
}

export interface ExportOutput {
  files: ExportFileEntry[];
  summary?: string;
}

export interface OjContext {
  handle: string;
  since?: string | null;
  cursor?: string | null;
}

export interface OjVerifyResult {
  ok: boolean;
  handle?: string;
  message?: string;
}

export interface OjSummarySync {
  handle: string;
  rating?: number | null;
  max_rating?: number | null;
  rank?: string | null;
  solved_count: number;
  submission_count: number;
  solved_problem_keys?: string[];
}

export interface OjSubmission {
  id: string;
  problem_key: string;
  problem_title?: string;
  verdict: string;
  submitted_at: string;
  language?: string;
  rating?: number | null;
  tags?: string[];
}

export interface OjSubmissionsBatch {
  submissions: OjSubmission[];
  next_cursor?: string | null;
}

export interface OjRatingChange {
  contest_id: string;
  contest_name: string;
  old_rating: number;
  new_rating: number;
  rank?: number | null;
  timestamp: string;
}

export interface OjContest {
  id: string;
  name: string;
  start_time: string;
  duration_seconds: number;
  url?: string;
}

export interface ParsedProblemUrl {
  platform: string;
  problem_key: string;
  url?: string;
}

export interface OjSourcePlugin {
  verify(ctx: OjContext): OjVerifyResult | Promise<OjVerifyResult>;
  syncSummary?(ctx: OjContext): OjSummarySync | Promise<OjSummarySync>;
  syncAccount?(ctx: OjContext): OjSummarySync | Promise<OjSummarySync>;
  syncSubmissions?(ctx: OjContext): OjSubmissionsBatch | Promise<OjSubmissionsBatch>;
  syncRatingHistory?(ctx: OjContext): OjRatingChange[] | Promise<OjRatingChange[]>;
  listContests?(ctx?: OjContext): OjContest[] | Promise<OjContest[]>;
  normalizeRating(raw: number): number | null;
  parseProblemUrl?(url: string): ParsedProblemUrl | null;
}

export interface ToolPlugin {
  runTool(
    toolId: string,
    inputs: Record<string, unknown>,
    ctx?: { plugin_id: string; tool_id: string },
  ): ToolExecutionResult | Promise<ToolExecutionResult>;
}

export interface CommandPlugin {
  runCommand(
    commandId: string,
    ctx?: Record<string, unknown>,
  ): PluginCommandAction | Promise<PluginCommandAction>;
}

export interface ImporterPlugin {
  importPackage(
    files: ImportFileInput[],
    importerId?: string,
  ): ImportDraft | Promise<ImportDraft>;
}

export interface ExporterPlugin {
  exportPackage(
    ctx: ExportContext,
    exporterId?: string,
  ): ExportOutput | Promise<ExportOutput>;
}

export type PluginModuleExports = Partial<
  OjSourcePlugin & ToolPlugin & CommandPlugin & ImporterPlugin & ExporterPlugin
>;

export interface HostApi {
  readonly http: {
    fetch(req: HostHttpRequest): HostHttpResponse;
  };
  readonly credentials: {
    get(key: string): string | null;
  };
  readonly html: {
    select(html: string, cssSelector: string): HtmlElementMatch[];
  };
  readonly kv: {
    get<T = unknown>(key: string): T | null;
    set(key: string, value: unknown): void;
    delete(key: string): boolean;
  };
  readonly workspace: {
    listProblems(): WorkspaceProblemSummary[];
    getProblem(id: string): WorkspaceProblemDetail | null;
  };
  readonly render: {
    svgToPng(svg: string, scale?: number): string;
    svgToRgba(svg: string, scale?: number): RenderedRgba;
  };
  highlight(
    code: string,
    lang?:
      | "cpp"
      | "c"
      | "rust"
      | "python"
      | "js"
      | "ts"
      | "json"
      | "toml"
      | "text"
      | string,
  ): HighlightResult;
  log(level: "debug" | "info" | "warn" | "error", msg: unknown): void;
}

declare global {
  const host: HostApi;
}

export {};
