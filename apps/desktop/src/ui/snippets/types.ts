// TypeScript models for Phase 5 Snippets, C++ Bundler, and Living TRD JSON payloads.

import type { TemplateInfo, TemplateOptionsByTemplate } from "../templates";

export type SnippetVerifyStatus = "unverified" | "passed" | "failed" | "stale";

export type SnippetVerifySpec =
  | { kind: "self"; file: string }
  | { kind: "problem"; problem: string; adapter: string }
  | { kind: "library_checker"; problem: string; adapter: string };

export interface SnippetSummaryItem {
  slug: string;
  title: string;
  tags: string[];
  time_complexity?: string | null;
  space_complexity?: string | null;
  description?: string | null;
  has_code: boolean;
  has_notes: boolean;
  verify_count: number;
  deps: string[];
  code_lines: number;
  code_hash?: string | null;
  status: SnippetVerifyStatus;
  reverse_deps: string[];
  related_problems: string[];
}

export interface SnippetListPayload {
  snippets: SnippetSummaryItem[];
  tags: string[];
}

export interface SnippetTextFile {
  path: string;
  text: string;
}

export interface SnippetMeta {
  title: string;
  tags: string[];
  time_complexity?: string | null;
  space_complexity?: string | null;
  description?: string | null;
  source_problem?: string | null;
  verify: SnippetVerifySpec[];
}

export interface SnippetReverseRefs {
  deps: string[];
  transitive_deps: string[];
  reverse_deps: string[];
  related_problems: string[];
}

export interface CodeHashBlock {
  line: number;
  hash: string;
  is_final: boolean;
}

export interface PrintedSnippetCode {
  code: string;
  lines: string[];
  stripped_deps: string[];
  hash_blocks: CodeHashBlock[];
  total_hash: string;
}

export interface SnippetDetail {
  slug: string;
  dir: string;
  meta: SnippetMeta;
  code?: SnippetTextFile | null;
  notes?: SnippetTextFile | null;
  test_file?: SnippetTextFile | null;
  verify_files: SnippetTextFile[];
  reverse_refs: SnippetReverseRefs;
  printed_code?: PrintedSnippetCode | null;
}

export interface SnippetVerifyCaseResult {
  kind: string;
  target: string;
  adapter?: string | null;
  passed: boolean;
  verdict: string;
  time_ms: number;
  memory_kib: number;
  passed_cases: number;
  total_cases: number;
  message?: string | null;
  compile_stderr?: string | null;
}

export interface SnippetVerifyReport {
  slug: string;
  content_hash: string;
  status: SnippetVerifyStatus;
  verified_at_epoch_secs: number;
  total_time_ms: number;
  max_memory_kib: number;
  cases: SnippetVerifyCaseResult[];
}

export interface SnippetDetailPayload {
  detail: SnippetDetail;
  status: SnippetVerifyStatus;
  report?: SnippetVerifyReport | null;
}

export interface SnippetVerifyJobState {
  seq: number;
  running: boolean;
  mode: string;
  target?: string | null;
  completed: number;
  total: number;
  reports: SnippetVerifyReport[];
  error?: string | null;
  errorCode?: string | null;
}

export interface BundleResult {
  code: string;
  system_includes: string[];
  inlined_files: string[];
  inlined_snippets: string[];
}

export interface TrdChapter {
  title: string;
  items: string[];
}

export interface TrdMeta {
  name: string;
  author?: string | null;
  date?: string | null;
  template?: string | null;
  frozen: boolean;
  archived: boolean;
  "template-options"?: TemplateOptionsByTemplate;
  chapter?: TrdChapter[];
}

export interface TrdSummaryItem {
  id: string;
  name: string;
  author?: string | null;
  date?: string | null;
  template?: string | null;
  frozen: boolean;
  archived: boolean;
  chapter_count: number;
  item_count: number;
}

export interface TrdDetail {
  id: string;
  dir: string;
  meta: TrdMeta;
  has_local_template: boolean;
  has_snapshot: boolean;
}

export type TrdPreflightKind =
  | "missing_snippet"
  | "missing_dependency"
  | "dependency_cycle"
  | "unverified_snippet"
  | "stale_snippet"
  | "failed_snippet";

export interface TrdPreflightIssue {
  severity: "error" | "warning";
  kind: TrdPreflightKind;
  snippet: string;
  missing_deps: string[];
  message: string;
}

export interface TrdDetailPayload {
  detail: TrdDetail;
  preflight: TrdPreflightIssue[];
  localTemplate?: TemplateInfo | null;
}

export interface TrdUnfreezeSnippetDiff {
  slug: string;
  kinds: string[];
  changed_files: string[];
}

export interface TrdUnfreezeDiff {
  id: string;
  diffs: TrdUnfreezeSnippetDiff[];
  has_changes: boolean;
}

export interface TrdTemplatesCatalog {
  default: string;
  templates: TemplateInfo[];
  presets: { id: string; name: unknown; description: unknown }[];
  fonts: string[];
}
