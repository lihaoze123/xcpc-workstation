// The statement and editorial editor: Markdown source styled in `EDITOR_MONO` with quiet
// paper-and-ink semantic highlights (restrained warm ochre math commands, muted fenced-code
// tokens, clean inline math without gray blocks). Exposes imperative formatting/find/cursor
// actions for StudioView's merged single-row bar, native `Mod+F` find & replace, clipboard
// image paste, restrained typing ergonomics (`$`/`` ` `` pairing, list continuation, Tab
// indent), diagnostic hover tooltips via `Overlay`, and `overviewRuler` marks.

import { cleanup, Clipboard, colorScheme, computed, effect, keymap, Overlay, pixelRatio, reduceMotion, signal, Text, untracked, View, type Bounds, type InputRef } from "nsh";
import {
  Editor,
  Input,
  type EditorDecoration,
  type EditorDiagnostic,
  type EditorPosition,
  type EditorRange,
  type EditorRef,
  type EditorSearch,
  type EditorSearchResult,
  type EditorSelection,
} from "nsh/base";
import { PageImage, Preview, VimKeyBridge, type MarkdownSpanJs } from "xcpc-workstation/native";
import { preview } from "../store";
import { Btn, IconBtn, tip } from "../kit";
import {
  EASE_STANDARD,
  EDITOR_MONO,
  editorFontSize,
  editorInk,
  editorLigatures,
  editorLineHeight,
  editorMono,
  editorVimMode,
  ms,
  popoverShadow,
  type EditorInk,
} from "../theme";
import {
  INITIAL_VIM_STATE,
  stepVim,
  VIM_NORMAL_KEYMAP,
  type VimMode,
  type VimState,
} from "./vim";
import {
  matchSlashTrigger,
  matchesSlashCommand,
  SLASH_BASIC_COUNT,
  SLASH_COMMANDS,
  SlashPopover,
  type SlashCommand,
} from "./SlashPopover";

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
  previewHidden?: boolean;
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
  onVimModeChange?: (mode: VimMode | undefined) => void;
  onVimEscape?: () => void;
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
  let slashAnchorSeq = 0;
  let slashLastGridCol = 0;
  let slashPointerInteractAt = 0;
  let slashBlurTimer: number | undefined;
  let dismissedSlash: { line: number; startColumn: number } | undefined;
  let emptyHintTimer: number | undefined;
  let emptyHintRaf: number | undefined;
  const focused = signal(false);
  const caret = signal<EditorPosition | undefined>({ line: 0, column: 0 });
  const mdSpans = signal<MarkdownSpanJs[]>(Preview.markdownSpans(text));
  const hover = signal<{ diagnostics: EditorDiagnostic[]; bounds: Bounds } | undefined>(undefined);
  const slashState = signal<
    | {
        line: number;
        startColumn: number;
        endColumn: number;
        query: string;
        items: readonly SlashCommand[];
      }
    | undefined
  >(undefined);
  const slashAnchor = signal<Bounds | undefined>(undefined);
  const slashActiveIndex = signal(0);
  const slashNavSeq = signal(0);
  const emptyLineHint = signal<{ top: number; left: number; height: number } | undefined>(undefined);
  const emptyLineHintVisible = signal(false);
  const mathPreview = signal<
    | { key: string; width: number; height: number; error?: string | null; bounds: Bounds }
    | undefined
  >(undefined);
  let mathPreviewTimer: number | undefined;
  let mathPreviewSeq = 0;
  // Find & Replace bar state (declared before Vim/math effects so computed signals never hit TDZ).
  const findOpen = signal(false);
  const replaceOpen = signal(false);
  const findQuery = signal("");
  const replaceQuery = signal("");
  const caseSensitive = signal(false);
  const searchResult = signal<EditorSearchResult>({ count: 0, current: null });

  const vimState = signal<VimState>(INITIAL_VIM_STATE);
  const vimActive = computed(
    () => editorVimMode() && !props.readOnly && !props.hidden && !slashState() && !findOpen(),
  );
  effect(() => {
    const enabled = editorVimMode() && !props.readOnly;
    if (!enabled) {
      untracked(() => {
        if (vimState().mode !== "normal" || vimState().pending) vimState.set(INITIAL_VIM_STATE);
        props.onVimModeChange?.(undefined);
      });
      return;
    }
    props.onVimModeChange?.(vimState().mode);
  });
  cleanup(() => props.onVimModeChange?.(undefined));

  function dispatchVimKey(key: string, ctrl = false) {
    if (!untracked(vimActive) || !editor || !untracked(focused)) return;
    const curSel = lastSelections[lastSelections.length - 1];
    const head = curSel?.head ?? untracked(caret) ?? { line: 0, column: 0 };
    const res = stepVim(
      untracked(vimState),
      key,
      { ctrl, alt: false, meta: false },
      lines(),
      head,
    );
    if (!res.handled) {
      if (key === "Escape") props.onVimEscape?.();
      return;
    }
    vimState.set(res.state);
    if (res.command === "undo") {
      editor.run("undo");
      return;
    }
    if (res.command === "redo") {
      editor.run("redo");
      return;
    }
    if (res.edit) {
      applyEdit(editor, [res.edit]);
    }
    if (res.selection) {
      editor.setSelections([res.selection]);
      lastSelections = [res.selection];
      const pos =
        res.selection.anchor.line === res.selection.head.line &&
        res.selection.anchor.column === res.selection.head.column
          ? res.selection.head
          : undefined;
      caret.set(pos);
    }
  }

  cleanup(() => {
    clearTimeout(mathPreviewTimer);
    mathPreviewSeq++;
  });

  // Smart floating mini math preview: triggers on any math span when preview is hidden,
  // or on display math / LaTeX-command inline math when side-by-side preview is visible.
  effect(() => {
    const pos = caret();
    const spans = mdSpans();
    const isFocused = focused();
    const hasSlash = !!slashState();
    const hasFind = findOpen();
    const isHidden = !!props.hidden;
    const isPreviewHidden = !!props.previewHidden;
    const dark = colorScheme() === "dark";
    const dpr = pixelRatio();

    clearTimeout(mathPreviewTimer);
    mathPreviewTimer = undefined;
    if (!isFocused || isHidden || hasSlash || hasFind || !pos) {
      mathPreviewSeq++;
      if (untracked(mathPreview)) mathPreview.set(undefined);
      return;
    }
    const found = extractMathAtCaret(text, spans, pos, isPreviewHidden);
    if (!found) {
      mathPreviewSeq++;
      if (untracked(mathPreview)) mathPreview.set(undefined);
      return;
    }
    const seq = ++mathPreviewSeq;
    const delay = isPreviewHidden ? 150 : 250;
    mathPreviewTimer = setTimeout(() => {
      mathPreviewTimer = undefined;
      const ed = editor;
      if (!ed || seq !== mathPreviewSeq) return;
      const renderScale = Math.max(1.5, dpr * 1.5);
      void Promise.all([
        ed.caretBounds(),
        preview.renderMathPreview(found.latex, found.display, dark, renderScale),
      ]).then(([cb, res]) => {
        if (seq !== mathPreviewSeq || !cb || !untracked(focused) || untracked(slashState)) return;
        if (!res.key && !res.error) {
          mathPreview.set(undefined);
          return;
        }
        mathPreview.set({
          key: res.key,
          width: Math.min(460, Math.max(24, res.width)),
          height: Math.min(200, Math.max(18, res.height)),
          error: res.error,
          bounds: cb.pageBounds,
        });
      });
    }, delay);
  });

  function setSlashActive(next: boolean) {
    if (slashActive === next) return;
    slashActive = next;
    props.onSlashActiveChange?.(next);
  }

  function closeSlash(dismissCurrent = false) {
    if (dismissCurrent) {
      const cur = untracked(slashState);
      if (cur) dismissedSlash = { line: cur.line, startColumn: cur.startColumn };
    }
    slashAnchorSeq++;
    if (untracked(slashState)) slashState.set(undefined);
    if (untracked(slashAnchor)) slashAnchor.set(undefined);
    slashActiveIndex.set(0);
    setSlashActive(false);
  }

  function clearEmptyLineHint() {
    if (emptyHintTimer !== undefined) {
      clearTimeout(emptyHintTimer);
      emptyHintTimer = undefined;
    }
    if (emptyHintRaf !== undefined) {
      cancelAnimationFrame(emptyHintRaf);
      emptyHintRaf = undefined;
    }
    if (untracked(emptyLineHintVisible)) emptyLineHintVisible.set(false);
    if (untracked(emptyLineHint)) emptyLineHint.set(undefined);
  }

  function scheduleEmptyLineHint(pos: EditorPosition | undefined) {
    clearEmptyLineHint();
    if (
      props.readOnly ||
      props.hidden ||
      !untracked(focused) ||
      untracked(slashState) ||
      untracked(findOpen) ||
      text.length === 0 ||
      !pos ||
      pos.column !== 0
    ) {
      return;
    }
    if ((lines()[pos.line] ?? "") !== "") return;
    const expectedLine = pos.line;
    emptyHintTimer = setTimeout(() => {
      emptyHintTimer = undefined;
      const ed = editor;
      const cur = untracked(caret);
      if (
        !ed ||
        props.readOnly ||
        props.hidden ||
        !untracked(focused) ||
        untracked(slashState) ||
        untracked(findOpen) ||
        text.length === 0 ||
        !cur ||
        cur.line !== expectedLine ||
        cur.column !== 0 ||
        (lines()[expectedLine] ?? "") !== ""
      ) {
        return;
      }
      void ed.caretBounds().then((cb) => {
        if (!cb || editor !== ed || untracked(slashState) || !untracked(focused)) return;
        const latest = untracked(caret);
        if (!latest || latest.line !== expectedLine || latest.column !== 0 || (lines()[expectedLine] ?? "") !== "") {
          return;
        }
        emptyLineHint.set({
          top: cb.bounds.origin.y,
          left: cb.bounds.origin.x + 2,
          height: cb.bounds.size.height,
        });
        emptyHintRaf = requestAnimationFrame(() => {
          emptyHintRaf = undefined;
          if (untracked(emptyLineHint)) emptyLineHintVisible.set(true);
        });
      });
    }, 350) as unknown as number;
  }

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

  // Clear hover tooltip, slash popover, and empty-line hint when hidden, read-only, or unfocused.
  effect(() => {
    if (props.hidden || props.readOnly || !focused()) {
      hover.set(undefined);
      clearEmptyLineHint();
      if (props.hidden || props.readOnly) closeSlash();
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
    if (slashBlurTimer !== undefined) clearTimeout(slashBlurTimer);
    cancelTypewriterAnim();
    clearEmptyLineHint();
    closeSlash();
  });

  /** Synchronously evaluates whether the caret is on a valid `/query` trigger and updates `slashState`. */
  function syncSlashState(pos: EditorPosition | undefined) {
    const ed = editor;
    if (!ed || props.readOnly || props.hidden || !untracked(focused) || !pos) {
      closeSlash();
      return;
    }
    const lineStr = lines()[pos.line] ?? "";
    const matched = matchSlashTrigger(lineStr, pos.column);
    if (!matched) {
      dismissedSlash = undefined;
      closeSlash();
      return;
    }
    if (dismissedSlash && dismissedSlash.line === pos.line && dismissedSlash.startColumn === matched.startColumn) {
      return;
    }
    dismissedSlash = undefined;

    // Suppress slash popover inside math formulas and inline/fenced code blocks.
    let spans = untracked(mdSpans);
    if (frame !== undefined) {
      spans = Preview.markdownSpans(text);
      mdSpans.set(spans);
    }
    if (isInMathOrCodeContext(spans, { line: pos.line, column: matched.startColumn })) {
      closeSlash();
      return;
    }

    const items = SLASH_COMMANDS.filter((cmd) => matchesSlashCommand(cmd, matched.query));
    if (items.length === 0) {
      closeSlash();
      return;
    }

    clearEmptyLineHint();
    const prev = untracked(slashState);
    const queryChanged = !prev || prev.query !== matched.query || prev.line !== pos.line || prev.startColumn !== matched.startColumn;
    slashState.set({
      line: pos.line,
      startColumn: matched.startColumn,
      endColumn: matched.endColumn,
      query: matched.query,
      items,
    });
    if (queryChanged) {
      slashActiveIndex.set(0);
      slashLastGridCol = 0;
      slashNavSeq.set(untracked(slashNavSeq) + 1);
    } else if (untracked(slashActiveIndex) >= items.length) {
      slashActiveIndex.set(Math.max(0, items.length - 1));
    }
    setSlashActive(true);

    const seq = ++slashAnchorSeq;
    void ed.caretBounds().then((cb) => {
      if (seq !== slashAnchorSeq || !cb || !untracked(slashState)) return;
      slashAnchor.set(cb.pageBounds);
    });
  }

  /** Accepts a slash command, replacing `/query` and placing the caret inside the inserted template. */
  function acceptSlashCommand(cmd: SlashCommand) {
    const st = untracked(slashState);
    const ed = editor;
    closeSlash();
    if (!st || !ed || props.readOnly) return;

    const lineStr = lines()[st.line] ?? "";
    const slashEditRange: EditorRange = {
      start: { line: st.line, column: st.startColumn },
      end: { line: st.line, column: st.endColumn },
    };

    if (cmd.kind === "image") {
      applyEdit(ed, [{ range: slashEditRange, text: "" }]);
      const pos: EditorPosition = { line: st.line, column: st.startColumn };
      ed.setSelections([{ anchor: pos, head: pos }]);
      void props.onInsertImage?.();
      return;
    }

    const template = cmd.template ?? "";
    const isMultiLine = template.includes("\n");
    const beforeSlash = lineStr.slice(0, st.startColumn);
    const afterSlash = lineStr.slice(st.endColumn);
    const onlyWhitespaceBefore = beforeSlash.trim() === "";
    const onlyWhitespaceAfter = afterSlash.trim() === "";

    let range: EditorRange = slashEditRange;
    let insertText = template;
    let baseLine = st.line;
    let baseCol = st.startColumn;

    if (isMultiLine && onlyWhitespaceBefore && onlyWhitespaceAfter) {
      range = {
        start: { line: st.line, column: 0 },
        end: { line: st.line, column: lineStr.length },
      };
      baseLine = st.line;
      baseCol = 0;
    } else if (isMultiLine && !onlyWhitespaceBefore) {
      insertText = `\n\n${template}`;
      baseLine = st.line + 2;
      baseCol = 0;
    }

    applyEdit(ed, [{ range, text: insertText }]);
    if (cmd.caret) {
      const pos: EditorPosition = {
        line: baseLine + cmd.caret.line,
        column: cmd.caret.line === 0 ? baseCol + cmd.caret.column : cmd.caret.column,
      };
      ed.setSelections([{ anchor: pos, head: pos }]);
    }
    ed.focus();
  }

  function acceptSlashSelected() {
    const st = untracked(slashState);
    if (!st || st.items.length === 0) return;
    const idx = Math.min(Math.max(0, untracked(slashActiveIndex)), st.items.length - 1);
    acceptSlashCommand(st.items[idx]);
  }

  function cancelSlash() {
    closeSlash(true);
    editor?.focus();
  }

  function updateSlashQuery(nextQuery: string) {
    const st = untracked(slashState);
    if (!st) return;
    const items = SLASH_COMMANDS.filter((cmd) => matchesSlashCommand(cmd, nextQuery));
    slashState.set({ ...st, query: nextQuery, items });
    slashActiveIndex.set(0);
    slashLastGridCol = 0;
    slashNavSeq.set(untracked(slashNavSeq) + 1);
  }

  function handleSlashEmptyBackspace() {
    const st = untracked(slashState);
    const ed = editor;
    closeSlash();
    if (st && ed && !props.readOnly) {
      applyEdit(ed, [
        {
          range: {
            start: { line: st.line, column: st.startColumn },
            end: { line: st.line, column: st.endColumn },
          },
          text: "",
        },
      ]);
      const pos: EditorPosition = { line: st.line, column: st.startColumn };
      ed.setSelections([{ anchor: pos, head: pos }]);
    }
    ed?.focus();
  }

  function handleSlashLeadingSpace() {
    const st = untracked(slashState);
    const ed = editor;
    closeSlash(true);
    if (st && ed && !props.readOnly) {
      const at: EditorPosition = { line: st.line, column: st.endColumn };
      applyEdit(ed, [{ range: { start: at, end: at }, text: " " }]);
      const after: EditorPosition = { line: st.line, column: st.endColumn + 1 };
      ed.setSelections([{ anchor: after, head: after }]);
    }
    ed?.focus();
  }

  function moveSlashSelection(dir: "up" | "down" | "left" | "right") {
    const st = untracked(slashState);
    if (!st || st.items.length === 0) return;
    const total = st.items.length;
    const idx = untracked(slashActiveIndex);
    const filtering = st.query.trim().length > 0;
    const bump = () => slashNavSeq.set(untracked(slashNavSeq) + 1);

    if (filtering) {
      if (dir === "down") {
        slashActiveIndex.set((idx + 1) % total);
        bump();
      } else if (dir === "up") {
        slashActiveIndex.set((idx - 1 + total) % total);
        bump();
      }
      return;
    }

    if (dir === "left" && idx < SLASH_BASIC_COUNT) {
      const next = (idx - 1 + SLASH_BASIC_COUNT) % SLASH_BASIC_COUNT;
      slashLastGridCol = next;
      slashActiveIndex.set(next);
      bump();
      return;
    }
    if (dir === "right" && idx < SLASH_BASIC_COUNT) {
      const next = (idx + 1) % SLASH_BASIC_COUNT;
      slashLastGridCol = next;
      slashActiveIndex.set(next);
      bump();
      return;
    }
    if (dir === "down") {
      if (idx < SLASH_BASIC_COUNT) {
        slashLastGridCol = idx;
        slashActiveIndex.set(total > SLASH_BASIC_COUNT ? SLASH_BASIC_COUNT : 0);
      } else if (idx < total - 1) {
        slashActiveIndex.set(idx + 1);
      } else {
        slashActiveIndex.set(slashLastGridCol);
      }
      bump();
      return;
    }
    if (dir === "up") {
      if (idx === SLASH_BASIC_COUNT) {
        slashActiveIndex.set(slashLastGridCol);
      } else if (idx > SLASH_BASIC_COUNT) {
        slashActiveIndex.set(idx - 1);
      } else {
        slashLastGridCol = idx;
        slashActiveIndex.set(total - 1);
      }
      bump();
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

  /** Toggles or switches the ATX heading level (`# ` .. `#### `) on the current line(s). */
  async function toggleHeading(level: 1 | 2 | 3 | 4) {
    const sel = await primary();
    const ed = editor;
    if (!sel || !ed || props.readOnly) return;
    const [a, b] = order(sel);
    const ls = lines();
    const edits: { range: EditorRange; text: string }[] = [];
    let firstDelta = 0;
    let lastDelta = 0;
    for (let line = a.line; line <= b.line; line++) {
      const lineStr = ls[line] ?? "";
      const res = toggleHeadingLine(lineStr, level);
      edits.push({
        range: { start: { line, column: 0 }, end: { line, column: res.replaceEnd } },
        text: res.insert,
      });
      if (line === a.line) firstDelta = res.delta;
      if (line === b.line) lastDelta = res.delta;
    }
    if (edits.length === 0) return;
    applyEdit(ed, edits);
    const nextA = { line: a.line, column: Math.max(0, a.column + firstDelta) };
    const nextB = { line: b.line, column: Math.max(0, b.column + lastDelta) };
    ed.setSelections([{ anchor: nextA, head: nextB }]);
    ed.focus();
  }

  /** Toggles unordered list (`- `), ordered list (`1. `), or blockquote (`> `) on the current line(s). */
  async function toggleLinePrefix(kind: "ul" | "ol" | "quote") {
    const sel = await primary();
    const ed = editor;
    if (!sel || !ed || props.readOnly) return;
    const [a, b] = order(sel);
    const ls = lines();
    const edits: { range: EditorRange; text: string }[] = [];
    let firstDelta = 0;
    let lastDelta = 0;
    let orderIdx = 1;
    for (let line = a.line; line <= b.line; line++) {
      const lineStr = ls[line] ?? "";
      const res = togglePrefixLine(lineStr, kind, orderIdx++);
      edits.push({
        range: { start: { line, column: res.replaceStart }, end: { line, column: res.replaceEnd } },
        text: res.insert,
      });
      if (line === a.line) firstDelta = res.delta;
      if (line === b.line) lastDelta = res.delta;
    }
    if (edits.length === 0) return;
    applyEdit(ed, edits);
    const nextA = { line: a.line, column: Math.max(0, a.column + firstDelta) };
    const nextB = { line: b.line, column: Math.max(0, b.column + lastDelta) };
    ed.setSelections([{ anchor: nextA, head: nextB }]);
    ed.focus();
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

  /** Continues `- `, `* `, `+ `, and numbered `1. ` lists on Enter; accepts slash command when popover is open. */
  async function handleEnter() {
    if (untracked(slashState)) {
      acceptSlashSelected();
      return;
    }
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

  /** Indents a list item cleanly at column 0 when pressing Tab at its prefix; accepts slash command when popover is open. */
  async function handleTab() {
    if (untracked(slashState)) {
      acceptSlashSelected();
      return;
    }
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
      "mod+1": () => canFormat() && void toggleHeading(1),
      "mod+2": () => canFormat() && void toggleHeading(2),
      "mod+3": () => canFormat() && void toggleHeading(3),
      "mod+4": () => canFormat() && void toggleHeading(4),
      "mod+b": () => canFormat() && void wrap("**"),
      "mod+shift+i": () => canFormat() && void wrap("*"),
      "mod+`": () => canFormat() && void wrap("`"),
      "mod+m": () => canFormat() && void wrap("$"),
      "mod+shift+m": () => canFormat() && void block("$$\n\n$$", { line: 1, column: 0 }),
      "mod+shift+8": () => canFormat() && void toggleLinePrefix("ul"),
      "mod+shift+7": () => canFormat() && void toggleLinePrefix("ol"),
      "mod+shift+.": () => canFormat() && void toggleLinePrefix("quote"),
      "mod+f": () => props.active() && !props.hidden && openFind(),
    },
    { when: "always" },
  );

  // Bind typing ergonomics and clipboard image paste after `Editor`'s own focus keymap flushes,
  // so these handlers cleanly take precedence while this editor is focused and editable.
  const canType = () => canFormat() && (!vimActive() || vimState().mode === "insert");
  effect(() => {
    if (!canType()) return;
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

  // Vim keymap bindings: intercept keys BEFORE `<Editor>`'s hidden text input sink
  // via `when: "always", forFocus: true`. In Insert mode only `escape` is bound so typing
  // flows natively (including IME); in Normal/Visual modes all printable/editing keys are
  // consumed by `stepVim` while `mod+*` and `alt+*` app shortcuts remain untouched.
  effect(() => {
    if (!props.active() || !vimActive() || !focused()) return;
    const mode = vimState().mode;
    let cancelled = false;
    const unbinds: (() => void)[] = [];
    queueMicrotask(() => {
      if (cancelled) return;
      unbinds.push(
        keymap.bind(
          {
            escape: () => dispatchVimKey("Escape", false),
            "ctrl+[": () => dispatchVimKey("Escape", false),
          },
          { when: "always", forFocus: true },
        ),
      );
      if (mode !== "insert") {
        const table: Record<string, () => void> = {};
        for (const entry of VIM_NORMAL_KEYMAP) {
          table[entry.stroke] = () => dispatchVimKey(entry.key, !!entry.ctrl);
        }
        unbinds.push(keymap.bind(table, { when: "always", forFocus: true, repeat: true }));
      }
    });
    cleanup(() => {
      cancelled = true;
      for (const fn of unbinds) fn();
    });
  });

  // Intercept arrow/Tab/Escape navigation whenever the slash popover is open.
  // Focus moves to the popover's filter input while open, so Editor's native bindings
  // are inactive and these keymap bindings take effect immediately without conflict.
  effect(() => {
    const st = slashState();
    if (!st || !props.active() || props.hidden || props.readOnly) return;
    const inBasicGrid = st.query.trim().length === 0 && slashActiveIndex() < SLASH_BASIC_COUNT;
    const unbinds: (() => void)[] = [
      keymap.bind("up", () => moveSlashSelection("up"), { when: "always", repeat: true }),
      keymap.bind("down", () => moveSlashSelection("down"), { when: "always", repeat: true }),
      keymap.bind("tab", () => acceptSlashSelected(), { when: "always" }),
      keymap.bind("escape", () => cancelSlash(), { when: "always" }),
    ];
    if (inBasicGrid) {
      unbinds.push(
        keymap.bind("left", () => moveSlashSelection("left"), { when: "always", repeat: true }),
        keymap.bind("right", () => moveSlashSelection("right"), { when: "always", repeat: true }),
      );
    }
    cleanup(() => {
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
        clearEmptyLineHint();
        if (untracked(slashState) && editor) {
          const seq = ++slashAnchorSeq;
          void editor.caretBounds().then((cb) => {
            if (seq !== slashAnchorSeq || !cb || !untracked(slashState)) return;
            slashAnchor.set(cb.pageBounds);
          });
        }
      }}
    >
      <VimKeyBridge
        active={props.active() && vimActive() && focused()}
        onEscape={() => dispatchVimKey("Escape", false)}
      />
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
      <View style={{ flex: 1, position: "relative", overflow: "hidden" }}>
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
          }}
          styles={{
            lineNumber: { fontSize: 12, fontVariant: ["tabular-nums"] },
            activeLineNumber: { fontSize: 12, fontVariant: ["tabular-nums"] },
          }}
          scrollBeyondLastLine={120}
          onFocus={() => {
            if (slashBlurTimer !== undefined) {
              clearTimeout(slashBlurTimer);
              slashBlurTimer = undefined;
            }
            focused.set(true);
            scheduleEmptyLineHint(untracked(caret));
          }}
          onBlur={() => {
            focused.set(false);
            clearEmptyLineHint();
          }}
          onSelectionChange={(e) => {
            lastSelections = e.selections;
            const s = e.selections.length === 1 ? e.selections[0] : undefined;
            const pos = s && s.anchor.line === s.head.line && s.anchor.column === s.head.column ? s.head : undefined;
            caret.set(pos);
            syncSlashState(pos);
            if (!untracked(slashState)) {
              scheduleEmptyLineHint(pos);
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
            clearEmptyLineHint();
            if (suppressAutoClose) {
              suppressAutoClose = false;
            } else if (!props.readOnly && focused() && editor) {
              maybeAutoClosePair(prevText, e.value);
            }
            if (!props.readOnly && focused() && editor) {
              const inferred = inferCaretAfterEdit(prevText, e.value, untracked(caret));
              syncSlashState(inferred);
              if (!untracked(slashState)) {
                scheduleEmptyLineHint(inferred);
              }
              scheduleTypewriterScroll();
            }
            restyle();
            props.onChange(e.value);
          }}
        />
        <If when={emptyLineHint()}>
          {(hint) => (
            <View
              testID="empty-line-slash-hint"
              style={{
                position: "absolute",
                top: hint().top,
                left: hint().left,
                height: hint().height,
                justifyContent: "center",
                pointerEvents: "none",
                opacity: emptyLineHintVisible() ? 1 : 0,
                transitionProperty: "opacity",
                transitionDuration: reduceMotion() ? 0 : 160,
                transitionTimingFunction: EASE_STANDARD,
              }}
            >
              <Text
                style={{
                  fontFamily: editorLigatures() || editorMono() === "monospace" ? editorMono() : `'JetBrains Mono NL', ${editorMono()}`,
                  fontSize: editorFontSize(),
                  lineHeight: Math.round(editorFontSize() * editorLineHeight()),
                }}
                class="text-muted-foreground/40"
              >
                输入 / 快速插入
              </Text>
            </View>
          )}
        </If>
      </View>
      <If when={Boolean(slashState() && slashAnchor())}>
        <SlashPopover
          anchor={slashAnchor()!}
          query={slashState()?.query ?? ""}
          items={slashState()?.items ?? SLASH_COMMANDS}
          activeIndex={slashActiveIndex()}
          navSeq={slashNavSeq()}
          onQueryChange={updateSlashQuery}
          onActiveChange={(i) => {
            slashActiveIndex.set(i);
            const cur = untracked(slashState);
            if (cur && !cur.query.trim() && i < SLASH_BASIC_COUNT) slashLastGridCol = i;
          }}
          onSelect={acceptSlashCommand}
          onSubmit={acceptSlashSelected}
          onCancel={cancelSlash}
          onEmptyBackspace={handleSlashEmptyBackspace}
          onLeadingSpace={handleSlashLeadingSpace}
          onPointerInteract={() => {
            slashPointerInteractAt = Date.now();
          }}
          onBlurOutside={() => {
            if (untracked(slashState)) closeSlash(true);
          }}
        />
      </If>
      <If when={mathPreview() && !slashState() && !hover()}>
        <Overlay anchor={mathPreview()!.bounds} placement="top" align="start" offset={8} margin={10}>
          <View
            testID="math-mini-preview"
            class="bg-popover border border-border"
            style={{
              maxWidth: 480,
              paddingTop: 4,
              paddingBottom: 4,
              paddingLeft: 6,
              paddingRight: 6,
              borderRadius: 8,
              boxShadow: popoverShadow(),
              pointerEvents: "none",
            }}
          >
            <If
              when={!!mathPreview()!.key}
              else={
                <Text style={{ fontSize: 11.5, lineHeight: 16 }} class="text-destructive">
                  {mathPreview()!.error ?? "公式语法有误"}
                </Text>
              }
            >
              <View style={{ width: mathPreview()!.width, height: mathPreview()!.height, overflow: "hidden" }}>
                <PageImage
                  layers={[{ key: mathPreview()!.key, left: 0, top: 0, width: 1, height: 1 }]}
                  style={{ width: "100%", height: "100%" }}
                />
              </View>
            </If>
          </View>
        </Overlay>
      </If>
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

/** Extracts the LaTeX body of the math span at `pos`, applying the smart zero-noise trigger rule. */
export function extractMathAtCaret(
  source: string,
  spans: readonly MarkdownSpanJs[],
  pos: EditorPosition,
  previewHidden: boolean,
): { latex: string; display: boolean } | undefined {
  let matched: MarkdownSpanJs | undefined;
  for (const s of spans) {
    if (s.kind !== "inlineMath" && s.kind !== "displayMath") continue;
    const start: EditorPosition = { line: s.startLine, column: s.startColumn };
    const end: EditorPosition = { line: s.endLine, column: s.endColumn };
    if (cmpPos(pos, start) >= 0 && cmpPos(pos, end) <= 0) {
      matched = s;
      break;
    }
  }
  if (!matched) return undefined;
  const ls = source.split("\n");
  let raw = "";
  if (matched.startLine === matched.endLine) {
    raw = (ls[matched.startLine] ?? "").slice(matched.startColumn, matched.endColumn);
  } else {
    const parts: string[] = [];
    for (let i = matched.startLine; i <= matched.endLine; i++) {
      const lineStr = ls[i] ?? "";
      if (i === matched.startLine) parts.push(lineStr.slice(matched.startColumn));
      else if (i === matched.endLine) parts.push(lineStr.slice(0, matched.endColumn));
      else parts.push(lineStr);
    }
    raw = parts.join("\n");
  }
  const display = matched.kind === "displayMath";
  const delim = display ? "$$" : "$";
  let inner = raw.trim();
  if (inner.startsWith(delim)) inner = inner.slice(delim.length);
  if (inner.endsWith(delim)) inner = inner.slice(0, inner.length - delim.length);
  inner = inner.trim();
  if (!inner) return undefined;
  if (!previewHidden && !display && !inner.includes("\\")) return undefined;
  return { latex: inner, display };
}

/** Computes the line-start edit to toggle or switch an ATX heading (`# ` .. `#### `). */
export function toggleHeadingLine(
  lineStr: string,
  level: 1 | 2 | 3 | 4,
): { replaceEnd: number; insert: string; delta: number } {
  const target = `${"#".repeat(level)} `;
  const m = lineStr.match(/^(#{1,6})\s+/);
  if (m) {
    if (m[1].length === level) {
      return { replaceEnd: m[0].length, insert: "", delta: -m[0].length };
    }
    return { replaceEnd: m[0].length, insert: target, delta: target.length - m[0].length };
  }
  return { replaceEnd: 0, insert: target, delta: target.length };
}

/** Computes the line edit to toggle unordered list (`- `), ordered list (`1. `), or blockquote (`> `). */
export function togglePrefixLine(
  lineStr: string,
  kind: "ul" | "ol" | "quote",
  orderIndex = 1,
): { replaceStart: number; replaceEnd: number; insert: string; delta: number } {
  if (kind === "quote") {
    const m = lineStr.match(/^(\s*)>\s?/);
    if (m) {
      const removed = m[0].length - m[1].length;
      return { replaceStart: m[1].length, replaceEnd: m[0].length, insert: "", delta: -removed };
    }
    return { replaceStart: 0, replaceEnd: 0, insert: "> ", delta: 2 };
  }
  const indentMatch = lineStr.match(/^(\s*)/);
  const indent = indentMatch ? indentMatch[1].length : 0;
  const rest = lineStr.slice(indent);
  if (kind === "ul") {
    const ul = rest.match(/^[-*+]\s+/);
    if (ul) {
      return { replaceStart: indent, replaceEnd: indent + ul[0].length, insert: "", delta: -ul[0].length };
    }
    const ol = rest.match(/^\d+\.\s+/);
    if (ol) {
      return { replaceStart: indent, replaceEnd: indent + ol[0].length, insert: "- ", delta: 2 - ol[0].length };
    }
    return { replaceStart: indent, replaceEnd: indent, insert: "- ", delta: 2 };
  }
  const ol = rest.match(/^\d+\.\s+/);
  if (ol) {
    return { replaceStart: indent, replaceEnd: indent + ol[0].length, insert: "", delta: -ol[0].length };
  }
  const prefix = `${orderIndex}. `;
  const ul = rest.match(/^[-*+]\s+/);
  if (ul) {
    return { replaceStart: indent, replaceEnd: indent + ul[0].length, insert: prefix, delta: prefix.length - ul[0].length };
  }
  return { replaceStart: indent, replaceEnd: indent, insert: prefix, delta: prefix.length };
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

function inferCaretAfterEdit(
  prevText: string,
  nextText: string,
  fallback: EditorPosition | undefined,
): EditorPosition | undefined {
  if (prevText === nextText) return fallback;
  const minLen = Math.min(prevText.length, nextText.length);
  let prefix = 0;
  while (prefix < minLen && prevText.charCodeAt(prefix) === nextText.charCodeAt(prefix)) prefix++;
  let suffix = 0;
  const maxSuffix = minLen - prefix;
  while (
    suffix < maxSuffix &&
    prevText.charCodeAt(prevText.length - 1 - suffix) === nextText.charCodeAt(nextText.length - 1 - suffix)
  ) {
    suffix++;
  }
  return offsetToPosition(nextText, nextText.length - suffix);
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
