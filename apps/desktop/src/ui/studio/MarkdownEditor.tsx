// The statement and editorial editor: Markdown source styled in `EDITOR_MONO` with quiet
// paper-and-ink semantic highlights (restrained warm ochre math commands, muted fenced-code
// tokens, clean inline math without gray blocks). Exposes imperative formatting/find/cursor
// actions for StudioView's merged single-row bar, native `Mod+F` find & replace, clipboard
// image paste, restrained typing ergonomics (`$`/`` ` `` pairing, list continuation, Tab
// indent), diagnostic hover tooltips via `Overlay`, and `overviewRuler` marks.

import { cleanup, Clipboard, computed, effect, keymap, Overlay, signal, Text, untracked, View, type Bounds, type InputRef } from "nsh";
import {
  Editor,
  Input,
  type EditorCompletionAcceptEvent,
  type EditorCompletionItem,
  type EditorCompletionRequest,
  type EditorDecoration,
  type EditorDiagnostic,
  type EditorPosition,
  type EditorRange,
  type EditorRef,
  type EditorSearch,
  type EditorSearchResult,
  type EditorSelection,
} from "nsh/base";
import { Preview, type MarkdownSpanJs } from "xcpc-workstation/native";
import { Btn, IconBtn, tip } from "../kit";
import {
  EASE_STANDARD,
  EDITOR_MONO,
  editorFontSize,
  editorInk,
  editorLigatures,
  editorLineHeight,
  editorMono,
  ms,
  popoverShadow,
  type EditorInk,
} from "../theme";

interface SlashCommand {
  readonly id: string;
  readonly label: string;
  readonly description: string;
  readonly keywords: readonly string[];
  readonly kind?: "image";
  readonly template?: string;
  readonly caret?: { readonly line: number; readonly column: number };
}

const SLASH_COMMANDS: readonly SlashCommand[] = [
  {
    id: "display-math",
    label: "行间公式",
    description: "居中显示的独立公式块",
    keywords: ["math", "formula", "equation", "display", "gongshi", "gs", "$$"],
    template: "$$\n\n$$",
    caret: { line: 1, column: 0 },
  },
  {
    id: "table",
    label: "Markdown 表格",
    description: "插入标准数据表格",
    keywords: ["table", "grid", "markdown", "biaoge", "bg"],
    template: "| 列 1 | 列 2 |\n| --- | --- |\n|  |  |",
    caret: { line: 2, column: 2 },
  },
  {
    id: "image",
    label: "插入图片",
    description: "从素材库选择或导入图片",
    keywords: ["image", "img", "pic", "picture", "figure", "tupian", "tp"],
    kind: "image",
  },
  {
    id: "cpp",
    label: "C++ 代码块",
    description: "插入 C++ 源代码块",
    keywords: ["cpp", "c++", "code", "source", "daima", "dm"],
    template: "```cpp\n\n```",
    caret: { line: 1, column: 0 },
  },
  {
    id: "cases",
    label: "分段函数",
    description: "插入 cases 分段公式",
    keywords: ["cases", "piecewise", "fenduan", "fd", "math"],
    template: "$$\nf(x) = \\begin{cases}\n  , & x \\le 0 \\\\\n  , & x > 0\n\\end{cases}\n$$",
    caret: { line: 2, column: 2 },
  },
  {
    id: "matrix",
    label: "矩阵",
    description: "插入 bmatrix 方括号矩阵",
    keywords: ["matrix", "bmatrix", "juzhen", "jz", "math"],
    template: "$$\n\\begin{bmatrix}\n  a & b \\\\\n  c & d\n\\end{bmatrix}\n$$",
    caret: { line: 2, column: 2 },
  },
  {
    id: "subtasks",
    label: "子任务评分表",
    description: "插入 Subtask 分值与特殊性质表",
    keywords: ["subtask", "subtasks", "score", "zirenwu", "zrw", "table"],
    template:
      "| 子任务 | 分值 | 特殊性质 |\n| :---: | :---: | --- |\n| 1 | 20 | $n \\le 10$ |\n| 2 | 30 | $n \\le 1000$ |\n| 3 | 50 | 无特殊限制 |",
    caret: { line: 2, column: 15 },
  },
  {
    id: "hint",
    label: "提示章节",
    description: "添加提示与数据范围章节",
    keywords: ["hint", "note", "tishi", "ts", "tszj", "h2", "section"],
    template: "## 提示\n\n",
    caret: { line: 2, column: 0 },
  },
  {
    id: "interaction",
    label: "交互章节",
    description: "添加交互题协议说明章节",
    keywords: ["interaction", "interactive", "protocol", "jiaohu", "jh", "jhzj", "h2", "section"],
    template: "## 交互方式\n\n",
    caret: { line: 2, column: 0 },
  },
  {
    id: "typst",
    label: "原始 Typst 块",
    description: "用 Typst 直接绘图或排版",
    keywords: ["typst", "raw", "vector", "diagram", "tu", "yst"],
    template: "```typst\n\n```",
    caret: { line: 1, column: 0 },
  },
];

/**
 * Parses a slash-command trigger immediately before `column` on `lineStr`.
 * Triggers only when `/` is at column 0 or preceded by whitespace, and the query
 * contains no whitespace or `/`.
 */
function matchSlashTrigger(
  lineStr: string,
  column: number,
): { startColumn: number; endColumn: number; query: string } | undefined {
  if (column <= 0 || column > lineStr.length) return undefined;
  const before = lineStr.slice(0, column);
  const m = before.match(/(?:^|\s)\/([^\s/]*)$/);
  if (!m) return undefined;
  const query = m[1];
  const slashCol = column - query.length - 1;
  return { startColumn: slashCol, endColumn: column, query };
}

/**
 * Strict prefix matching against command `id`, `label` (or its whitespace-separated tokens),
 * and `keywords` (English keywords + pinyin / pinyin-initial prefixes). Never uses loose
 * subsequence matching so `/tu` only hits genuine prefix matches.
 */
function matchesSlashCommand(cmd: SlashCommand, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  if (cmd.id.toLowerCase().startsWith(q)) return true;
  const labelLower = cmd.label.toLowerCase();
  if (labelLower.startsWith(q)) return true;
  for (const part of labelLower.split(/\s+/)) {
    if (part.startsWith(q)) return true;
  }
  for (const kw of cmd.keywords) {
    if (kw.toLowerCase().startsWith(q)) return true;
  }
  return false;
}

/** Returns true when `pos` falls inside an inline/display math or inline/fenced code span. */
function isInMathOrCodeContext(spans: readonly MarkdownSpanJs[], pos: EditorPosition): boolean {
  for (const s of spans) {
    if (
      s.kind !== "inlineMath" &&
      s.kind !== "displayMath" &&
      s.kind !== "code" &&
      s.kind !== "inlineCode" &&
      s.kind !== "codeBlock"
    ) {
      continue;
    }
    const start: EditorPosition = { line: s.startLine, column: s.startColumn };
    const end: EditorPosition = { line: s.endLine, column: s.endColumn };
    if (cmpPos(pos, start) >= 0 && cmpPos(pos, end) <= 0) {
      return true;
    }
  }
  return false;
}

function decoration(s: MarkdownSpanJs, ink: EditorInk): EditorDecoration | undefined {
  const range = { start: { line: s.startLine, column: s.startColumn }, end: { line: s.endLine, column: s.endColumn } };
  switch (s.kind) {
    case "heading":
      return { range, class: "font-semibold text-foreground" };
    case "strong":
      return { range, class: "font-semibold" };
    case "emphasis":
      return { range, class: "italic" };
    case "strikethrough":
      return { range, class: "line-through text-secondary-foreground" };
    case "inlineMath":
      return { range, class: "text-foreground" };
    case "displayMath":
      return { range, class: "text-foreground", lineClass: "bg-muted/35" };
    case "mathCommand":
      return { range, class: ink.mathCommand };
    case "mathOperator":
      return { range, class: ink.mathOperator };
    case "code":
      return { range, class: "bg-muted/75 text-foreground" };
    case "codeBlock":
      return { range, class: "text-secondary-foreground", lineClass: "bg-muted/45" };
    case "codeKeyword":
      return { range, class: ink.codeKeyword };
    case "codeComment":
      return { range, class: ink.codeComment };
    case "codeString":
      return { range, class: ink.codeString };
    case "codeNumber":
      return { range, class: ink.codeNumber };
    case "link":
      return { range, class: "text-primary underline" };
    case "url":
    case "markup":
      return { range, class: "text-muted-foreground/45" };
    default:
      return undefined;
  }
}

export interface MarkdownEditorActions {
  inlineMath(): void;
  displayMath(): void;
  table(): void;
  insertImage(rel: string): Promise<void>;
  openFind(): void;
  closeFind(): boolean;
  isFindOpen(): boolean;
  currentLine(): Promise<number>;
}

export function MarkdownEditor(props: {
  active: () => boolean;
  hidden?: boolean;
  typewriter?: boolean;
  defaultValue: string;
  placeholder?: string;
  readOnly: boolean;
  diagnostics: EditorDiagnostic[];
  onRef: (ref: EditorRef) => void;
  onActions?: (actions: MarkdownEditorActions) => void;
  onInsertImage?: () => void | Promise<void>;
  onPasteImage?: (ext: string, bytes: Uint8Array) => string | null | Promise<string | null>;
  onSlashActiveChange?: (active: boolean) => void;
  onChange: (value: string) => void;
}) {
  let editor: EditorRef | undefined;
  let findInput: InputRef | undefined;
  let text = props.defaultValue;
  let lastSelections: EditorSelection[] = [{ anchor: { line: 0, column: 0 }, head: { line: 0, column: 0 } }];
  let suppressAutoClose = false;
  let slashActive = false;
  let editorHeight = 0;
  let lastWheelAt = 0;
  let typewriterRaf: number | undefined;
  let typewriterAnimRaf: number | undefined;
  const focused = signal(false);
  const caret = signal<EditorPosition | undefined>({ line: 0, column: 0 });
  const mdSpans = signal<MarkdownSpanJs[]>(Preview.markdownSpans(text));
  const hover = signal<{ diagnostics: EditorDiagnostic[]; bounds: Bounds } | undefined>(undefined);

  function setSlashActive(next: boolean) {
    if (slashActive === next) return;
    slashActive = next;
    props.onSlashActiveChange?.(next);
  }

  // Find & Replace bar state
  const findOpen = signal(false);
  const replaceOpen = signal(false);
  const findQuery = signal("");
  const replaceQuery = signal("");
  const caseSensitive = signal(false);
  const searchResult = signal<EditorSearchResult>({ count: 0, current: null });

  const searchObj = computed<EditorSearch | null>(() => {
    if (!findOpen()) return null;
    const q = findQuery();
    if (!q) return null;
    return { query: q, caseSensitive: caseSensitive() };
  });

  // Merge paper-and-ink Markdown decorations (reactive to `colorScheme()`) with matched `$`/`$$`
  // delimiter highlights and overview ruler marks. Backend `markdownSpans` emits outer containers
  // (`inlineMath`, `displayMath`, `codeBlock`) before inner tokens (`mathCommand`, `mathOperator`,
  // `codeKeyword`, ...), so fine-grained ink colors naturally override outer text classes.
  const allDecorations = computed<EditorDecoration[]>(() => {
    const ink = editorInk();
    const spans = mdSpans();
    const base: EditorDecoration[] = [];
    for (const s of spans) {
      const d = decoration(s, ink);
      if (d) base.push(d);
    }
    const pos = focused() && !props.hidden ? caret() : undefined;
    if (pos) {
      const pair = matchedMathDelimiters(text, spans, pos);
      if (pair) {
        base.push(
          { range: pair[0], class: ink.mathDelimiterMatch },
          { range: pair[1], class: ink.mathDelimiterMatch },
        );
      }
    }
    const diags = props.diagnostics;
    if (diags.length === 0) return base;
    const marks: EditorDecoration[] = diags.map((d) => ({
      range: d.range,
      overviewRuler: {
        lane: "right",
        class: d.severity === "error" ? "bg-destructive" : "bg-chart-4",
      },
    }));
    return [...base, ...marks];
  });

  // Clear hover tooltip and native completion menu when hidden or unfocused/read-only.
  effect(() => {
    if (props.hidden || props.readOnly || !focused()) {
      hover.set(undefined);
      setSlashActive(false);
      editor?.hideCompletions();
    }
  });

  let frame: number | undefined;
  const restyle = () => {
    if (frame !== undefined) return;
    frame = requestAnimationFrame(() => {
      frame = undefined;
      mdSpans.set(Preview.markdownSpans(text));
    });
  };
  function cancelTypewriterAnim() {
    if (typewriterRaf !== undefined) {
      cancelAnimationFrame(typewriterRaf);
      typewriterRaf = undefined;
    }
    if (typewriterAnimRaf !== undefined) {
      cancelAnimationFrame(typewriterAnimRaf);
      typewriterAnimRaf = undefined;
    }
  }
  cleanup(() => {
    if (frame !== undefined) cancelAnimationFrame(frame);
    cancelTypewriterAnim();
    setSlashActive(false);
  });

  /** Serves native `Editor` completions only when `/` is at line start or after whitespace. */
  function handleCompletionRequest(event: EditorCompletionRequest) {
    const ed = editor;
    if (!ed || props.readOnly || props.hidden) {
      ed?.presentCompletions(event.id, []);
      setSlashActive(false);
      return;
    }
    const lineIdx = event.position.line;
    const lineStr = lines()[lineIdx] ?? "";
    const matched = matchSlashTrigger(lineStr, event.position.column);
    if (!matched) {
      ed.presentCompletions(event.id, []);
      setSlashActive(false);
      return;
    }

    // Suppress slash menu inside math formulas and inline/fenced code blocks.
    let spans = untracked(mdSpans);
    if (frame !== undefined) {
      spans = Preview.markdownSpans(text);
      mdSpans.set(spans);
    }
    if (isInMathOrCodeContext(spans, { line: lineIdx, column: matched.startColumn })) {
      ed.presentCompletions(event.id, []);
      setSlashActive(false);
      return;
    }

    const matchedCommands = SLASH_COMMANDS.filter((cmd) => matchesSlashCommand(cmd, matched.query));
    if (matchedCommands.length === 0) {
      ed.presentCompletions(event.id, [], true);
      setSlashActive(false);
      return;
    }

    const beforeSlash = lineStr.slice(0, matched.startColumn);
    const afterSlash = lineStr.slice(matched.endColumn);
    const onlyWhitespaceBefore = beforeSlash.trim() === "";
    const onlyWhitespaceAfter = afterSlash.trim() === "";
    const slashEditRange: EditorRange = {
      start: { line: lineIdx, column: matched.startColumn },
      end: { line: lineIdx, column: matched.endColumn },
    };
    const qLower = matched.query.trim().toLowerCase();

    const items: EditorCompletionItem[] = matchedCommands.map((cmd, index) => {
      // Pre-filtered via strict prefix rules; keep `label` as `filterText` when it directly
      // starts with the query (for match highlight), else prefix with `matched.query` so
      // native completion filtering retains keyword/pinyin matches.
      const filterText = !qLower || cmd.label.toLowerCase().startsWith(qLower) ? cmd.label : `${matched.query} ${cmd.label}`;
      const sortText = String(index).padStart(2, "0");
      if (cmd.kind === "image") {
        return {
          label: cmd.label,
          detail: cmd.description,
          filterText,
          sortText,
          insertText: "",
          range: slashEditRange,
        };
      }
      const template = cmd.template ?? "";
      const isMultiLine = template.includes("\n");
      let range: EditorRange = slashEditRange;
      let insertText = template;
      if (isMultiLine && onlyWhitespaceBefore && onlyWhitespaceAfter) {
        range = {
          start: { line: lineIdx, column: 0 },
          end: { line: lineIdx, column: lineStr.length },
        };
      } else if (isMultiLine && !onlyWhitespaceBefore) {
        insertText = `\n\n${template}`;
      }
      return {
        label: cmd.label,
        detail: cmd.description,
        filterText,
        sortText,
        insertText,
        range,
      };
    });

    // `incomplete = true` ensures every subsequent keystroke/backspace re-requests with an up-to-date `range` covering `/query`.
    ed.presentCompletions(event.id, items, true);
    setSlashActive(true);
  }

  /** Handles post-insertion actions after native completion accepts a slash command. */
  function handleCompletionAccept(event: EditorCompletionAcceptEvent) {
    setSlashActive(false);
    const ed = editor;
    if (!ed || props.readOnly) return;
    const cmd = SLASH_COMMANDS.find((c) => c.label === event.item.label);
    if (!cmd) return;
    if (cmd.kind === "image") {
      void props.onInsertImage?.();
      return;
    }
    if (cmd.caret && event.item.range) {
      const inserted = event.item.insertText ?? "";
      const startsWithBlankLines = inserted.startsWith("\n\n");
      const baseLine = startsWithBlankLines ? event.item.range.start.line + 2 : event.item.range.start.line;
      const baseCol = startsWithBlankLines ? 0 : event.item.range.start.column;
      const pos: EditorPosition = {
        line: baseLine + cmd.caret.line,
        column: cmd.caret.line === 0 ? baseCol + cmd.caret.column : cmd.caret.column,
      };
      ed.setSelections([{ anchor: pos, head: pos }]);
    }
  }

  /** Smoothly keeps the active line near ~45% of the viewport height in Focus Mode without fighting wheel scroll. */
  function scheduleTypewriterScroll() {
    if (!props.typewriter || props.hidden || !focused() || !editor || editorHeight <= 0) return;
    if (Date.now() - lastWheelAt < 400) return;
    if (typewriterRaf !== undefined) cancelAnimationFrame(typewriterRaf);
    typewriterRaf = requestAnimationFrame(() => {
      typewriterRaf = undefined;
      const ed = editor;
      if (!ed || !props.typewriter || props.hidden || !focused() || editorHeight <= 0) return;
      if (Date.now() - lastWheelAt < 400) return;
      void Promise.all([ed.caretBounds(), ed.scrollOffset()]).then(([cb, offset]) => {
        if (!cb || !props.typewriter || editor !== ed || Date.now() - lastWheelAt < 400) return;
        const caretMidY = cb.bounds.origin.y + cb.bounds.size.height / 2;
        const targetY = editorHeight * 0.45;
        const dy = caretMidY - targetY;
        if (Math.abs(dy) < 4) return;
        const startY = offset.y;
        const destY = Math.max(0, startY + dy);
        if (Math.abs(destY - startY) < 2) return;

        if (typewriterAnimRaf !== undefined) {
          cancelAnimationFrame(typewriterAnimRaf);
          typewriterAnimRaf = undefined;
        }
        const dur = ms(140);
        if (dur === 0) {
          ed.setScrollOffset({ x: offset.x, y: destY });
          return;
        }
        const t0 = performance.now();
        const step = (now: number) => {
          typewriterAnimRaf = undefined;
          if (!props.typewriter || editor !== ed || Date.now() - lastWheelAt < 400) return;
          const p = Math.min(1, (now - t0) / dur);
          const eased = 1 - Math.pow(1 - p, 3);
          ed.setScrollOffset({ x: offset.x, y: startY + (destY - startY) * eased });
          if (p < 1) typewriterAnimRaf = requestAnimationFrame(step);
        };
        typewriterAnimRaf = requestAnimationFrame(step);
      });
    });
  }

  const lines = () => text.split("\n");

  async function primary(): Promise<EditorSelection | undefined> {
    return editor ? (await editor.selections())[0] : undefined;
  }

  function applyEdit(ed: EditorRef, edits: { range: { start: EditorPosition; end: EditorPosition }; text: string }[]) {
    suppressAutoClose = true;
    ed.edit(edits);
  }

  /**
   * Wraps the selection in `before`/`after` (toggling off if already wrapped with symmetric
   * delimiters), or puts the caret between them. When `caretBeforeClose` is true and text
   * is selected on one line (links/images), places the caret inside `()`.
   */
  async function wrap(before: string, after = before, caretBeforeClose = false) {
    const sel = await primary();
    if (!sel || !editor || props.readOnly) return;
    const [a, b] = order(sel);
    const ed = editor;
    const inner = await ed.selectedText();

    // Symmetric toggle-unwrap when the selection is already surrounded by `before`/`after`.
    if (before === after && a.line === b.line) {
      const lineStr = lines()[a.line] ?? "";
      if (
        a.column >= before.length &&
        lineStr.slice(a.column - before.length, a.column) === before &&
        lineStr.slice(b.column, b.column + after.length) === after
      ) {
        applyEdit(ed, [
          {
            range: {
              start: { line: a.line, column: a.column - before.length },
              end: { line: b.line, column: b.column + after.length },
            },
            text: inner,
          },
        ]);
        const start = { line: a.line, column: a.column - before.length };
        const end = { line: a.line, column: start.column + inner.length };
        ed.setSelections([{ anchor: start, head: end }]);
        ed.focus();
        return;
      }
      if (inner.length >= before.length + after.length && inner.startsWith(before) && inner.endsWith(after)) {
        const unwrapped = inner.slice(before.length, inner.length - after.length);
        applyEdit(ed, [{ range: { start: a, end: b }, text: unwrapped }]);
        const end = { line: a.line, column: a.column + unwrapped.length };
        ed.setSelections([{ anchor: a, head: end }]);
        ed.focus();
        return;
      }
    }

    applyEdit(ed, [{ range: { start: a, end: b }, text: `${before}${inner}${after}` }]);
    if (caretBeforeClose && inner && !inner.includes("\n") && after.endsWith(")")) {
      const col = a.column + before.length + inner.length + after.length - 1;
      const pos = { line: a.line, column: col };
      ed.setSelections([{ anchor: pos, head: pos }]);
    } else {
      const start = { line: a.line, column: a.column + before.length };
      const end = inner && !inner.includes("\n") ? { line: a.line, column: start.column + inner.length } : start;
      ed.setSelections([{ anchor: start, head: end }]);
    }
    ed.focus();
  }

  /** Puts `block` on lines of its own after the caret's line, the caret at `caret` in it. */
  async function block(content: string, caret: { line: number; column: number }) {
    const sel = await primary();
    if (!sel || !editor || props.readOnly) return;
    const line = order(sel)[1].line;
    const end = (lines()[line] ?? "").length;
    const blank = (lines()[line] ?? "").trim() === "";
    const at = { line, column: blank ? 0 : end };
    applyEdit(editor, [{ range: { start: at, end: blank ? { line, column: end } : at }, text: blank ? content : `\n\n${content}` }]);
    const top = blank ? line : line + 2;
    const pos = { line: top + caret.line, column: caret.column };
    editor.setSelections([{ anchor: pos, head: pos }]);
    editor.focus();
  }

  /** Inserts `![selectedText](rel)` (or `![](rel)` without selection) at the caret and focuses the editor. */
  async function insertImage(rel: string) {
    const ed = editor;
    if (!ed || props.readOnly) return;
    const sel = await primary();
    if (!sel) return;
    const [a, b] = order(sel);
    const alt = (await ed.selectedText()).replace(/\r?\n/g, " ");
    const md = `![${alt}](${rel})`;
    applyEdit(ed, [{ range: { start: a, end: b }, text: md }]);
    const pos = { line: a.line, column: a.column + md.length };
    ed.setSelections([{ anchor: pos, head: pos }]);
    ed.focus();
  }

  /**
   * Complements NSH's native `autoClose` (brackets & quotes) with `$` and `` ` `` pairing:
   * - typing `$` / `` ` `` inserts its closing mate with caret centered (or wraps a single-line selection);
   * - typing `$` / `` ` `` when the same char is already immediately to the right skips over it;
   * - avoids interfering with `$$` display math fences, `` ``` `` code fences, or escaped `\$` / `` \` ``.
   */
  function maybeAutoClosePair(prevText: string, nextText: string) {
    const ed = editor;
    if (!ed || lastSelections.length !== 1) return;
    const sel = lastSelections[0];
    const [a, b] = order(sel);

    // Case 1: single character typed at a collapsed caret.
    if (a.line === b.line && a.column === b.column && nextText.length === prevText.length + 1) {
      let pos: EditorPosition = a;
      let off = positionToOffset(prevText, a);
      const matchesCaret =
        off >= 0 &&
        prevText.slice(0, off) === nextText.slice(0, off) &&
        prevText.slice(off) === nextText.slice(off + 1);
      if (!matchesCaret) {
        // Fallback when `onSelectionChange` has not fired yet for the current caret.
        let p = 0;
        while (p < prevText.length && prevText.charCodeAt(p) === nextText.charCodeAt(p)) p++;
        if (prevText.slice(p) !== nextText.slice(p + 1)) return;
        off = p;
        pos = offsetToPosition(prevText, p);
      }
      const ch = nextText[off];
      if (ch !== "$" && ch !== "`") return;

      const prevLine = prevText.split("\n")[pos.line] ?? "";
      const col = pos.column;
      if (col > 0 && prevLine[col - 1] === "\\") return;

      // Skip-over when the closing delimiter is already right after the caret (`$x|$`, `$|$`, `` `a|` ``).
      if (prevLine[col] === ch) {
        applyEdit(ed, [{ range: { start: { line: pos.line, column: col }, end: { line: pos.line, column: col + 1 } }, text: "" }]);
        const after = { line: pos.line, column: col + 1 };
        lastSelections = [{ anchor: after, head: after }];
        caret.set(after);
        ed.setSelections(lastSelections);
        return;
      }

      // Do not break `$$` display math blocks or `` ``` `` fenced code blocks.
      if (ch === "$" && col >= 1 && prevLine[col - 1] === "$") return;
      if (ch === "`" && col >= 2 && prevLine[col - 1] === "`" && prevLine[col - 2] === "`") return;

      const mid = { line: pos.line, column: col + 1 };
      applyEdit(ed, [{ range: { start: mid, end: mid }, text: ch }]);
      lastSelections = [{ anchor: mid, head: mid }];
      caret.set(mid);
      ed.setSelections(lastSelections);
      return;
    }

    // Case 2: single-line non-empty selection replaced by `$` or `` ` `` -> wrap the selection.
    if (a.line === b.line && a.column < b.column) {
      const startOff = positionToOffset(prevText, a);
      const endOff = positionToOffset(prevText, b);
      if (startOff < 0 || endOff < startOff) return;
      if (nextText.length !== prevText.length - (endOff - startOff) + 1) return;
      const ch = nextText[startOff];
      if (ch !== "$" && ch !== "`") return;
      if (prevText.slice(0, startOff) !== nextText.slice(0, startOff) || prevText.slice(endOff) !== nextText.slice(startOff + 1)) {
        return;
      }
      const selected = prevText.slice(startOff, endOff);
      applyEdit(ed, [
        {
          range: { start: a, end: { line: a.line, column: a.column + 1 } },
          text: `${ch}${selected}${ch}`,
        },
      ]);
      lastSelections = [
        {
          anchor: { line: a.line, column: a.column + 1 },
          head: { line: a.line, column: a.column + 1 + selected.length },
        },
      ];
      caret.set(undefined);
      ed.setSelections(lastSelections);
    }
  }

  /** Deletes empty `$|$` or `` `|` `` pairs together without disturbing `$$` or `` ``` `` fences. */
  async function handleBackspace() {
    const ed = editor;
    if (!ed) return;
    const sels = await ed.selections();
    if (sels.length === 1) {
      const s = sels[0];
      if (s.anchor.line === s.head.line && s.anchor.column === s.head.column) {
        const { line, column } = s.head;
        const lineStr = lines()[line] ?? "";
        const left = column > 0 ? lineStr[column - 1] : "";
        const right = column < lineStr.length ? lineStr[column] : "";
        if (
          (left === "$" || left === "`") &&
          right === left &&
          (column < 2 || lineStr[column - 2] !== left) &&
          (column + 1 >= lineStr.length || lineStr[column + 1] !== left)
        ) {
          applyEdit(ed, [
            {
              range: { start: { line, column: column - 1 }, end: { line, column: column + 1 } },
              text: "",
            },
          ]);
          return;
        }
      }
    }
    ed.run("deleteBackward");
  }

  /** Continues `- `, `* `, `+ `, and numbered `1. ` lists on Enter; clears empty list prefixes. */
  async function handleEnter() {
    const ed = editor;
    if (!ed) return;
    const sels = await ed.selections();
    if (sels.length === 1) {
      const s = sels[0];
      if (s.anchor.line === s.head.line && s.anchor.column === s.head.column) {
        const { line, column } = s.head;
        const lineStr = lines()[line] ?? "";

        const ul = lineStr.match(/^(\s*)([-*+])(\s+)(.*)$/);
        if (ul) {
          const prefixLen = ul[1].length + ul[2].length + ul[3].length;
          if (column >= prefixLen) {
            if (ul[4].trim() === "") {
              applyEdit(ed, [{ range: { start: { line, column: 0 }, end: { line, column: lineStr.length } }, text: "" }]);
              return;
            }
            const nextPrefix = `${ul[1]}${ul[2]} `;
            applyEdit(ed, [{ range: { start: { line, column }, end: { line, column } }, text: `\n${nextPrefix}` }]);
            const pos = { line: line + 1, column: nextPrefix.length };
            ed.setSelections([{ anchor: pos, head: pos }]);
            return;
          }
        }

        const ol = lineStr.match(/^(\s*)(\d+)\.(\s+)(.*)$/);
        if (ol) {
          const prefixLen = ol[1].length + ol[2].length + 1 + ol[3].length;
          if (column >= prefixLen) {
            if (ol[4].trim() === "") {
              applyEdit(ed, [{ range: { start: { line, column: 0 }, end: { line, column: lineStr.length } }, text: "" }]);
              return;
            }
            const nextPrefix = `${ol[1]}${Number.parseInt(ol[2], 10) + 1}. `;
            applyEdit(ed, [{ range: { start: { line, column }, end: { line, column } }, text: `\n${nextPrefix}` }]);
            const pos = { line: line + 1, column: nextPrefix.length };
            ed.setSelections([{ anchor: pos, head: pos }]);
            return;
          }
        }
      }
    }
    ed.run("newline");
  }

  /** Indents a list item cleanly at column 0 when pressing Tab at its prefix; delegates all else to native `indent`. */
  async function handleTab() {
    const ed = editor;
    if (!ed) return;
    const sels = await ed.selections();
    if (sels.length === 1) {
      const s = sels[0];
      if (s.anchor.line === s.head.line && s.anchor.column === s.head.column) {
        const { line, column } = s.head;
        const lineStr = lines()[line] ?? "";
        const m = lineStr.match(/^(\s*(?:[-*+]|\d+\.)\s+)/);
        if (m && column <= m[1].length) {
          applyEdit(ed, [{ range: { start: { line, column: 0 }, end: { line, column: 0 } }, text: "  " }]);
          const pos = { line, column: column + 2 };
          ed.setSelections([{ anchor: pos, head: pos }]);
          return;
        }
      }
    }
    ed.run("indent");
  }

  /** Pastes clipboard images into `assets/` only when the clipboard has no text; otherwise runs native text paste. */
  async function handlePaste() {
    const ed = editor;
    if (!ed || props.readOnly) return;
    if (props.onPasteImage) {
      try {
        const hasText = await Clipboard.hasText();
        if (!hasText && (await Clipboard.hasImage())) {
          const img = await Clipboard.readImage();
          if (img?.url) {
            const res = await fetch(img.url);
            const contentType = res.headers.get("content-type") ?? "";
            const bytes = new Uint8Array(await res.arrayBuffer());
            if (bytes.length > 0) {
              const rel = await props.onPasteImage(imageExtFrom(contentType, bytes), bytes);
              if (rel) {
                await insertImage(rel);
                return;
              }
            }
          }
        }
      } catch {
        // Fall through to native text paste if clipboard image reading is unavailable.
      }
    }
    ed.run("paste");
  }

  function openFind() {
    if (editor) {
      void editor.selectedText().then((sel) => {
        if (sel && !sel.includes("\n") && sel.length <= 120) findQuery.set(sel);
      });
    }
    findOpen.set(true);
    queueMicrotask(() => findInput?.focus());
  }

  function closeFind(): boolean {
    if (!findOpen()) return false;
    findOpen.set(false);
    replaceOpen.set(false);
    editor?.focus();
    return true;
  }

  const actions: MarkdownEditorActions = {
    inlineMath: () => void wrap("$"),
    displayMath: () => void block("$$\n\n$$", { line: 1, column: 0 }),
    table: () => void block("| 列 1 | 列 2 |\n| --- | --- |\n|  |  |", { line: 2, column: 2 }),
    insertImage,
    openFind,
    closeFind,
    isFindOpen: () => findOpen(),
    currentLine: async () => {
      const sel = await primary();
      return (sel?.head.line ?? 0) + 1;
    },
  };
  props.onActions?.(actions);

  // Formatting shortcuts strictly scoped to when this editor is visible and focused.
  const canFormat = () => props.active() && !props.hidden && focused() && !props.readOnly;
  keymap.bind(
    {
      "mod+b": () => canFormat() && void wrap("**"),
      "mod+shift+i": () => canFormat() && void wrap("*"),
      "mod+m": () => canFormat() && void wrap("$"),
      "mod+shift+m": () => canFormat() && void block("$$\n\n$$", { line: 1, column: 0 }),
      "mod+f": () => props.active() && !props.hidden && openFind(),
    },
    { when: "always" },
  );

  // Bind typing ergonomics and clipboard image paste after `Editor`'s own focus keymap flushes,
  // so these handlers cleanly take precedence while this editor is focused and editable.
  // Native completion bindings activate on top of these when the completion list opens.
  effect(() => {
    if (!canFormat()) return;
    let cancelled = false;
    const unbinds: (() => void)[] = [];
    queueMicrotask(() => {
      if (cancelled) return;
      unbinds.push(
        keymap.bind(
          {
            enter: () => void handleEnter(),
            tab: () => void handleTab(),
            "mod+v": () => void handlePaste(),
          },
          { when: "always", forFocus: true },
        ),
        keymap.bind("backspace", () => void handleBackspace(), { when: "always", forFocus: true, repeat: true }),
      );
    });
    cleanup(() => {
      cancelled = true;
      for (const fn of unbinds) fn();
    });
  });

  const matchLabel = () => {
    if (!findQuery()) return "";
    const r = searchResult();
    if (r.count === 0) return "无匹配";
    return r.current != null ? `${r.current + 1} / ${r.count}` : `${r.count} 处`;
  };

  return (
    <View
      style={{ flex: 1 }}
      onLayout={(e) => {
        editorHeight = e.bounds.size.height;
      }}
      onWheel={() => {
        lastWheelAt = Date.now();
        cancelTypewriterAnim();
      }}
    >
      <If when={findOpen()}>
        <View
          testID="find-bar"
          class="bg-muted/50 border-b border-border"
          style={{ paddingLeft: 10, paddingRight: 8, paddingTop: 6, paddingBottom: 6, gap: 6 }}
        >
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <If when={!props.readOnly}>
              <IconBtn
                icon={replaceOpen() ? "chevronDown" : "chevronRight"}
                size={24}
                iconSize={13}
                tooltip={tip(replaceOpen() ? "收起替换" : "展开替换")}
                onPress={() => replaceOpen.set(!replaceOpen())}
              />
            </If>
            <Input
              ref={(r) => (findInput = r)}
              value={findQuery()}
              placeholder="查找"
              autoCapitalize="none"
              autoCorrect={false}
              submitBehavior="submit"
              onChange={(e) => findQuery.set(e.value)}
              onSubmit={() => editor?.run("findNext")}
              onKeyPress={(e) => {
                if (e.key === "Escape") closeFind();
              }}
              class="flex-1 h-7 px-2 rounded-md border border-border bg-card text-[12.5px] placeholder:text-muted-foreground focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring transition-colors"
              style={{ fontFamily: EDITOR_MONO }}
            />
            <Text
              style={{ minWidth: 46, textAlign: "right", fontSize: 11.5, fontVariant: ["tabular-nums"] }}
              class={findQuery() && searchResult().count === 0 ? "text-destructive" : "text-muted-foreground"}
            >
              {matchLabel()}
            </Text>
            <View
              onPress={() => caseSensitive.set(!caseSensitive())}
              tooltip={tip("区分大小写")}
              class={caseSensitive() ? "bg-accent border border-border" : "hover:bg-muted"}
              style={{
                width: 24,
                height: 24,
                borderRadius: 5,
                alignItems: "center",
                justifyContent: "center",
                transitionProperty: "colors",
                transitionDuration: ms(120),
                transitionTimingFunction: EASE_STANDARD,
              }}
            >
              <Text style={{ fontSize: 11, fontWeight: 600 }} class={caseSensitive() ? "text-foreground" : "text-secondary-foreground"}>
                Aa
              </Text>
            </View>
            <IconBtn
              icon="chevronLeft"
              size={24}
              iconSize={14}
              disabled={!findQuery() || searchResult().count === 0}
              tooltip={tip("上一个匹配")}
              onPress={() => editor?.run("findPrevious")}
            />
            <IconBtn
              icon="chevronRight"
              size={24}
              iconSize={14}
              disabled={!findQuery() || searchResult().count === 0}
              tooltip={tip("下一个匹配")}
              onPress={() => editor?.run("findNext")}
            />
            <IconBtn icon="x" size={24} iconSize={13} tooltip={tip("关闭", "escape")} onPress={closeFind} />
          </View>
          <If when={replaceOpen() && !props.readOnly}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 6, paddingLeft: 30 }}>
              <Input
                value={replaceQuery()}
                placeholder="替换为"
                autoCapitalize="none"
                autoCorrect={false}
                submitBehavior="submit"
                onChange={(e) => replaceQuery.set(e.value)}
                onSubmit={() => editor?.replace(replaceQuery())}
                onKeyPress={(e) => {
                  if (e.key === "Escape") closeFind();
                }}
                class="flex-1 h-7 px-2 rounded-md border border-border bg-card text-[12.5px] placeholder:text-muted-foreground focus:border-primary focus:ring-1 focus:ring-inset focus:ring-ring transition-colors"
                style={{ fontFamily: EDITOR_MONO }}
              />
              <Btn
                label="替换"
                disabled={!findQuery() || searchResult().count === 0}
                onPress={() => editor?.replace(replaceQuery())}
                style={{ height: 26, paddingLeft: 8, paddingRight: 8 }}
                textStyle={{ fontSize: 12 }}
              />
              <Btn
                label="全部"
                disabled={!findQuery() || searchResult().count === 0}
                onPress={() => void editor?.replaceAll(replaceQuery())}
                style={{ height: 26, paddingLeft: 8, paddingRight: 8 }}
                textStyle={{ fontSize: 12 }}
              />
            </View>
          </If>
        </View>
      </If>
      <Editor
        ref={(r) => {
          editor = r;
          props.onRef(r);
        }}
        style={{ flex: 1 }}
        defaultValue={props.defaultValue}
        placeholder={props.placeholder}
        language="markdown"
        highlighting={false}
        folding={false}
        autoClose
        tabSize={2}
        hardTabs={false}
        softWrap
        readOnly={props.readOnly}
        fontFamily={editorLigatures() || editorMono() === "monospace" ? editorMono() : `'JetBrains Mono NL', ${editorMono()}`}
        fontSize={editorFontSize()}
        lineHeight={Math.round(editorFontSize() * editorLineHeight())}
        activeLine
        completion={!props.readOnly}
        completionTriggerCharacters={["/"]}
        search={searchObj()}
        decorations={allDecorations()}
        diagnostics={props.diagnostics}
        class="bg-card"
        classes={{
          content: "pl-3 pr-5 pt-3",
          gutter: "pl-2.5 pr-2",
          activeLine: "bg-muted/45",
          lineNumber: "text-muted-foreground/45",
          activeLineNumber: "text-secondary-foreground",
          completionMenu: "bg-popover border border-border rounded-[10px] p-1",
          completionItem: "px-2 rounded-[6px] text-foreground",
          selectedCompletionItem: "bg-accent text-foreground",
          completionMatch: "font-semibold text-primary",
          completionDetail: "text-muted-foreground",
        }}
        styles={{
          lineNumber: { fontSize: 12, fontVariant: ["tabular-nums"] },
          activeLineNumber: { fontSize: 12, fontVariant: ["tabular-nums"] },
          completionMenu: { boxShadow: popoverShadow() },
          completionItem: { fontSize: 12.5 },
          completionDetail: { fontSize: 11.5 },
        }}
        scrollBeyondLastLine={120}
        onFocus={() => focused.set(true)}
        onBlur={() => {
          focused.set(false);
          setSlashActive(false);
        }}
        onCompletionRequest={handleCompletionRequest}
        onCompletionAccept={handleCompletionAccept}
        onSelectionChange={(e) => {
          lastSelections = e.selections;
          const s = e.selections.length === 1 ? e.selections[0] : undefined;
          const pos = s && s.anchor.line === s.head.line && s.anchor.column === s.head.column ? s.head : undefined;
          caret.set(pos);
          if (slashActive) {
            const lineStr = pos ? (lines()[pos.line] ?? "") : "";
            if (!pos || !matchSlashTrigger(lineStr, pos.column)) {
              setSlashActive(false);
            }
          }
          if (pos) scheduleTypewriterScroll();
        }}
        onSearchChange={(e) => searchResult.set(e.result)}
        onFindWithSelection={(e) => {
          if (e.query) findQuery.set(e.query);
          openFind();
        }}
        onHover={(e) => {
          hover.set(e.diagnostics.length > 0 ? { diagnostics: e.diagnostics, bounds: e.pageBounds } : undefined);
        }}
        onHoverEnd={() => hover.set(undefined)}
        onChange={(e) => {
          const prevText = text;
          text = e.value;
          if (suppressAutoClose) {
            suppressAutoClose = false;
          } else if (!props.readOnly && focused() && editor) {
            maybeAutoClosePair(prevText, e.value);
          }
          if (!props.readOnly && focused() && editor) {
            scheduleTypewriterScroll();
          }
          restyle();
          props.onChange(e.value);
        }}
      />
      <If when={hover()}>
        {(h) => (
          <Overlay anchor={h().bounds} placement="top" align="start" offset={6} margin={8}>
            <View
              class="bg-popover border border-border"
              style={{
                maxWidth: 340,
                paddingTop: 6,
                paddingBottom: 6,
                paddingLeft: 10,
                paddingRight: 10,
                borderRadius: 8,
                gap: 4,
                boxShadow: popoverShadow(),
                pointerEvents: "none",
              }}
            >
              <Each of={h().diagnostics} key={(_, i) => String(i)}>
                {(d) => (
                  <View style={{ flexDirection: "row", alignItems: "flex-start", gap: 6 }}>
                    <View
                      style={{ width: 6, height: 6, borderRadius: 3, marginTop: 6 }}
                      class={d().severity === "error" ? "bg-destructive" : "bg-chart-4"}
                    />
                    <Text style={{ flex: 1, fontSize: 12, lineHeight: 17 }} class="text-foreground">
                      {d().message}
                    </Text>
                  </View>
                )}
              </Each>
            </View>
          </Overlay>
        )}
      </If>
    </View>
  );
}

function order(sel: EditorSelection) {
  const { anchor: x, head: y } = sel;
  return x.line < y.line || (x.line === y.line && x.column <= y.column) ? [x, y] : [y, x];
}

function cmpPos(a: EditorPosition, b: EditorPosition): number {
  return a.line !== b.line ? a.line - b.line : a.column - b.column;
}

/**
 * Finds the matched opening and closing `$` / `$$` delimiter ranges when `pos` is inside or
 * touching a paired `inlineMath` / `displayMath` span (or sitting inside an empty `$|$` / `$$|$$`).
 */
function matchedMathDelimiters(
  src: string,
  spans: readonly MarkdownSpanJs[],
  pos: EditorPosition,
): [{ start: EditorPosition; end: EditorPosition }, { start: EditorPosition; end: EditorPosition }] | undefined {
  const ls = src.split("\n");
  let fallback: [{ start: EditorPosition; end: EditorPosition }, { start: EditorPosition; end: EditorPosition }] | undefined;

  for (const s of spans) {
    if (s.kind !== "inlineMath" && s.kind !== "displayMath") continue;
    const len = s.kind === "displayMath" ? 2 : 1;
    if (s.startColumn < len) continue;
    const delim = len === 2 ? "$$" : "$";
    const openLine = ls[s.startLine] ?? "";
    const closeLine = ls[s.endLine] ?? "";
    if (openLine.slice(s.startColumn - len, s.startColumn) !== delim) continue;
    if (closeLine.slice(s.endColumn, s.endColumn + len) !== delim) continue;

    const openStart: EditorPosition = { line: s.startLine, column: s.startColumn - len };
    const openEnd: EditorPosition = { line: s.startLine, column: s.startColumn };
    const closeStart: EditorPosition = { line: s.endLine, column: s.endColumn };
    const closeEnd: EditorPosition = { line: s.endLine, column: s.endColumn + len };

    if (cmpPos(pos, openStart) < 0 || cmpPos(pos, closeEnd) > 0) continue;
    const pair: [{ start: EditorPosition; end: EditorPosition }, { start: EditorPosition; end: EditorPosition }] = [
      { start: openStart, end: openEnd },
      { start: closeStart, end: closeEnd },
    ];
    // Prefer a formula whose interior contains the caret over one merely touched on its outer edge.
    if (cmpPos(pos, openEnd) >= 0 && cmpPos(pos, closeStart) <= 0) return pair;
    fallback ??= pair;
  }
  if (fallback) return fallback;

  // Empty paired `$|$` or `$$|$$` (e.g. immediately after typing `$` with auto-close).
  const lineStr = ls[pos.line] ?? "";
  const col = pos.column;
  if (
    col >= 2 &&
    lineStr.slice(col - 2, col) === "$$" &&
    lineStr.slice(col, col + 2) === "$$"
  ) {
    return [
      { start: { line: pos.line, column: col - 2 }, end: { line: pos.line, column: col } },
      { start: { line: pos.line, column: col }, end: { line: pos.line, column: col + 2 } },
    ];
  }
  if (
    col >= 1 &&
    lineStr[col - 1] === "$" &&
    lineStr[col] === "$" &&
    (col < 2 || lineStr[col - 2] !== "$") &&
    (col + 1 >= lineStr.length || lineStr[col + 1] !== "$")
  ) {
    return [
      { start: { line: pos.line, column: col - 1 }, end: { line: pos.line, column: col } },
      { start: { line: pos.line, column: col }, end: { line: pos.line, column: col + 1 } },
    ];
  }
  return undefined;
}

function offsetToPosition(src: string, offset: number): EditorPosition {
  let line = 0;
  let lineStart = 0;
  for (;;) {
    const nl = src.indexOf("\n", lineStart);
    if (nl < 0 || nl >= offset) return { line, column: offset - lineStart };
    line++;
    lineStart = nl + 1;
  }
}

function positionToOffset(src: string, pos: EditorPosition): number {
  let line = 0;
  let offset = 0;
  while (line < pos.line) {
    const nl = src.indexOf("\n", offset);
    if (nl < 0) return -1;
    offset = nl + 1;
    line++;
  }
  const lineEnd = src.indexOf("\n", offset);
  const maxCol = (lineEnd < 0 ? src.length : lineEnd) - offset;
  if (pos.column < 0 || pos.column > maxCol) return -1;
  return offset + pos.column;
}

function imageExtFrom(contentType: string, bytes: Uint8Array): string {
  const ct = contentType.toLowerCase();
  if (ct.includes("jpeg") || ct.includes("jpg")) return "jpg";
  if (ct.includes("webp")) return "webp";
  if (ct.includes("svg")) return "svg";
  if (ct.includes("png")) return "png";
  if (bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) return "jpg";
  if (
    bytes.length >= 12 &&
    bytes[0] === 0x52 &&
    bytes[1] === 0x49 &&
    bytes[2] === 0x46 &&
    bytes[3] === 0x46 &&
    bytes[8] === 0x57 &&
    bytes[9] === 0x45 &&
    bytes[10] === 0x42 &&
    bytes[11] === 0x50
  ) {
    return "webp";
  }
  return "png";
}
