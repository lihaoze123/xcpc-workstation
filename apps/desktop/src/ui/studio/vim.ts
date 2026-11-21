// Pure, deterministic Vim practical-subset state machine for MarkdownEditor.
// Supports Normal / Insert / Visual / Visual-Line modes, core motions
// (`hjkl`, `w/b/e`, `0/$`, `gg/G`), editing (`i/a/I/A/o/O`, `x`, `dd/yy/p/P`,
// `u`, `Ctrl+r`), Visual (`v/V` + `d/y/c`), and inner text objects
// (`ci/di/yi` with `$`, `(`, `)`, `"`, `'`, `` ` ``).

import type { EditorPosition, EditorRange, EditorSelection } from "nsh/base";

export type VimMode = "normal" | "insert" | "visual" | "visual-line";

export interface VimState {
  mode: VimMode;
  pending: string;
  register: string;
  registerLinewise: boolean;
  visualAnchor?: EditorPosition;
}

export interface VimStepResult {
  state: VimState;
  handled: boolean;
  edit?: { range: EditorRange; text: string };
  selection?: EditorSelection;
  command?: "undo" | "redo";
}

export const INITIAL_VIM_STATE: VimState = {
  mode: "normal",
  pending: "",
  register: "",
  registerLinewise: false,
};

/** Canonical NSH keymap bindings active in Normal / Visual / Visual-Line modes.
 *  Consumes all printable keys and editing keys before `<Editor>`'s hidden text input
 *  sink receives them, while leaving `mod+*` and `alt+*` app shortcuts untouched. */
export const VIM_NORMAL_KEYMAP: readonly { stroke: string; key: string; ctrl?: boolean }[] = [
  ...Array.from("abcdefghijklmnopqrstuvwxyz", (ch) => ({ stroke: ch, key: ch })),
  ...Array.from("abcdefghijklmnopqrstuvwxyz", (ch) => ({ stroke: `shift+${ch}`, key: ch.toUpperCase() })),
  ...Array.from("0123456789", (ch) => ({ stroke: ch, key: ch })),
  { stroke: "shift+1", key: "!" },
  { stroke: "shift+2", key: "@" },
  { stroke: "shift+3", key: "#" },
  { stroke: "shift+4", key: "$" },
  { stroke: "shift+5", key: "%" },
  { stroke: "shift+6", key: "^" },
  { stroke: "shift+7", key: "&" },
  { stroke: "shift+8", key: "*" },
  { stroke: "shift+9", key: "(" },
  { stroke: "shift+0", key: ")" },
  { stroke: "-", key: "-" },
  { stroke: "=", key: "=" },
  { stroke: "[", key: "[" },
  { stroke: "]", key: "]" },
  { stroke: "\\", key: "\\" },
  { stroke: ";", key: ";" },
  { stroke: "'", key: "'" },
  { stroke: "`", key: "`" },
  { stroke: ",", key: "," },
  { stroke: ".", key: "." },
  { stroke: "/", key: "/" },
  { stroke: "shift+-", key: "_" },
  { stroke: "shift+=", key: "+" },
  { stroke: "shift+[", key: "{" },
  { stroke: "shift+]", key: "}" },
  { stroke: "shift+\\", key: "|" },
  { stroke: "shift+;", key: ":" },
  { stroke: "shift+'", key: '"' },
  { stroke: "shift+`", key: "~" },
  { stroke: "shift+,", key: "<" },
  { stroke: "shift+.", key: ">" },
  { stroke: "shift+/", key: "?" },
  { stroke: "space", key: " " },
  { stroke: "enter", key: "Enter" },
  { stroke: "backspace", key: "Backspace" },
  { stroke: "delete", key: "Delete" },
  { stroke: "tab", key: "Tab" },
  { stroke: "ctrl+r", key: "r", ctrl: true },
];

function clampLine(lines: readonly string[], line: number): number {
  return Math.max(0, Math.min(Math.max(0, lines.length - 1), line));
}

function clampCol(lineStr: string, col: number, inclusiveEnd = false): number {
  const max = inclusiveEnd ? lineStr.length : Math.max(0, lineStr.length - 1);
  return Math.max(0, Math.min(max, col));
}

function isWordChar(ch: string): boolean {
  return /[\w\u4e00-\u9fff]/.test(ch);
}

function nextWordStart(lines: readonly string[], pos: EditorPosition): EditorPosition {
  let line = clampLine(lines, pos.line);
  let col = pos.column;
  const cur = lines[line] ?? "";
  if (col < cur.length) {
    const startWord = isWordChar(cur[col]!);
    const startSpace = /\s/.test(cur[col]!);
    if (!startSpace) {
      while (col < cur.length && !/\s/.test(cur[col]!) && isWordChar(cur[col]!) === startWord) col++;
    }
    while (col < cur.length && /\s/.test(cur[col]!)) col++;
    if (col < cur.length) return { line, column: col };
  }
  if (line + 1 < lines.length) {
    line += 1;
    const next = lines[line] ?? "";
    let c = 0;
    while (c < next.length && /\s/.test(next[c]!)) c++;
    return { line, column: clampCol(next, c) };
  }
  return { line, column: clampCol(cur, cur.length) };
}

function prevWordStart(lines: readonly string[], pos: EditorPosition): EditorPosition {
  let line = clampLine(lines, pos.line);
  let col = pos.column;
  const cur = lines[line] ?? "";
  if (col > 0) {
    col = Math.min(col - 1, cur.length - 1);
    while (col > 0 && /\s/.test(cur[col]!)) col--;
    if (col >= 0 && !/\s/.test(cur[col]!)) {
      const w = isWordChar(cur[col]!);
      while (col > 0 && !/\s/.test(cur[col - 1]!) && isWordChar(cur[col - 1]!) === w) col--;
      return { line, column: col };
    }
  }
  if (line > 0) {
    line -= 1;
    const prev = lines[line] ?? "";
    let c = Math.max(0, prev.length - 1);
    while (c > 0 && /\s/.test(prev[c]!)) c--;
    if (prev.length > 0 && !/\s/.test(prev[c]!)) {
      const w = isWordChar(prev[c]!);
      while (c > 0 && !/\s/.test(prev[c - 1]!) && isWordChar(prev[c - 1]!) === w) c--;
    }
    return { line, column: clampCol(prev, c) };
  }
  return { line: 0, column: 0 };
}

function wordEnd(lines: readonly string[], pos: EditorPosition): EditorPosition {
  let line = clampLine(lines, pos.line);
  let col = pos.column + 1;
  while (line < lines.length) {
    const s = lines[line] ?? "";
    while (col < s.length && /\s/.test(s[col]!)) col++;
    if (col < s.length) {
      const w = isWordChar(s[col]!);
      while (col + 1 < s.length && !/\s/.test(s[col + 1]!) && isWordChar(s[col + 1]!) === w) col++;
      return { line, column: col };
    }
    line++;
    col = 0;
  }
  const last = Math.max(0, lines.length - 1);
  return { line: last, column: clampCol(lines[last] ?? "", (lines[last] ?? "").length) };
}

function cmpPos(a: EditorPosition, b: EditorPosition): number {
  return a.line !== b.line ? a.line - b.line : a.column - b.column;
}

function sliceRange(lines: readonly string[], range: EditorRange): string {
  if (range.start.line === range.end.line) {
    return (lines[range.start.line] ?? "").slice(range.start.column, range.end.column);
  }
  const parts: string[] = [];
  for (let i = range.start.line; i <= range.end.line; i++) {
    const s = lines[i] ?? "";
    if (i === range.start.line) parts.push(s.slice(range.start.column));
    else if (i === range.end.line) parts.push(s.slice(0, range.end.column));
    else parts.push(s);
  }
  return parts.join("\n");
}

function findInnerObject(lineStr: string, line: number, col: number, obj: string): EditorRange | undefined {
  if (obj === "(" || obj === ")" || obj === "b") {
    let open = -1;
    let depth = 0;
    for (let i = Math.min(col, lineStr.length - 1); i >= 0; i--) {
      if (lineStr[i] === ")" && i !== col) depth++;
      else if (lineStr[i] === "(") {
        if (depth === 0) {
          open = i;
          break;
        }
        depth--;
      }
    }
    if (open < 0) return undefined;
    depth = 0;
    for (let i = open + 1; i < lineStr.length; i++) {
      if (lineStr[i] === "(") depth++;
      else if (lineStr[i] === ")") {
        if (depth === 0) {
          return { start: { line, column: open + 1 }, end: { line, column: i } };
        }
        depth--;
      }
    }
    return undefined;
  }
  if (obj === "$" || obj === "\"" || obj === "'" || obj === "`") {
    // Symmetric delimiter on current line: find enclosing pair around `col`, or next pair on line.
    const positions: number[] = [];
    for (let i = 0; i < lineStr.length; i++) {
      if (lineStr[i] === obj) positions.push(i);
    }
    for (let i = 0; i + 1 < positions.length; i += 2) {
      const a = positions[i]!;
      const b = positions[i + 1]!;
      if (col >= a && col <= b) {
        return { start: { line, column: a + 1 }, end: { line, column: b } };
      }
    }
    for (let i = 0; i + 1 < positions.length; i += 2) {
      const a = positions[i]!;
      const b = positions[i + 1]!;
      if (a >= col) {
        return { start: { line, column: a + 1 }, end: { line, column: b } };
      }
    }
  }
  return undefined;
}

function visualSelectionFor(
  mode: "visual" | "visual-line",
  lines: readonly string[],
  anchor: EditorPosition,
  head: EditorPosition,
): EditorSelection {
  if (mode === "visual-line") {
    const startLine = Math.min(anchor.line, head.line);
    const endLine = Math.max(anchor.line, head.line);
    const endCol = (lines[endLine] ?? "").length;
    return { anchor: { line: startLine, column: 0 }, head: { line: endLine, column: endCol } };
  }
  const forward = cmpPos(head, anchor) >= 0;
  if (forward) {
    const endLineStr = lines[head.line] ?? "";
    return { anchor, head: { line: head.line, column: Math.min(endLineStr.length, head.column + 1) } };
  }
  const anchorLineStr = lines[anchor.line] ?? "";
  return { anchor: { line: anchor.line, column: Math.min(anchorLineStr.length, anchor.column + 1) }, head };
}

function visualRangeOf(
  mode: "visual" | "visual-line",
  lines: readonly string[],
  anchor: EditorPosition,
  head: EditorPosition,
): { range: EditorRange; linewise: boolean } {
  if (mode === "visual-line") {
    const startLine = Math.min(anchor.line, head.line);
    const endLine = Math.max(anchor.line, head.line);
    if (endLine + 1 < lines.length) {
      return {
        range: { start: { line: startLine, column: 0 }, end: { line: endLine + 1, column: 0 } },
        linewise: true,
      };
    }
    if (startLine > 0) {
      const prevLen = (lines[startLine - 1] ?? "").length;
      return {
        range: { start: { line: startLine - 1, column: prevLen }, end: { line: endLine, column: (lines[endLine] ?? "").length } },
        linewise: true,
      };
    }
    return {
      range: { start: { line: 0, column: 0 }, end: { line: endLine, column: (lines[endLine] ?? "").length } },
      linewise: true,
    };
  }
  const a = cmpPos(anchor, head) <= 0 ? anchor : head;
  const b = cmpPos(anchor, head) <= 0 ? head : anchor;
  const endLineStr = lines[b.line] ?? "";
  return {
    range: { start: a, end: { line: b.line, column: Math.min(endLineStr.length, b.column + 1) } },
    linewise: false,
  };
}

/** Advances the Vim state machine by one key event. */
export function stepVim(
  state: VimState,
  key: string,
  modifiers: { ctrl?: boolean; alt?: boolean; meta?: boolean },
  lines: readonly string[],
  caret: EditorPosition,
): VimStepResult {
  const safeLines = lines.length > 0 ? lines : [""];
  const pos: EditorPosition = {
    line: clampLine(safeLines, caret.line),
    column: Math.max(0, Math.min((safeLines[clampLine(safeLines, caret.line)] ?? "").length, caret.column)),
  };

  // Never hijack meta/alt app shortcuts.
  if (modifiers.meta || modifiers.alt) {
    return { state, handled: false };
  }

  // Insert mode: only Escape returns to Normal.
  if (state.mode === "insert") {
    if (key === "Escape") {
      const lineStr = safeLines[pos.line] ?? "";
      const nextPos = { line: pos.line, column: clampCol(lineStr, Math.max(0, pos.column - 1)) };
      return {
        state: { ...state, mode: "normal", pending: "", visualAnchor: undefined },
        handled: true,
        selection: { anchor: nextPos, head: nextPos },
      };
    }
    return { state, handled: false };
  }

  // Ctrl+r redo in Normal mode.
  if (modifiers.ctrl) {
    if (key.toLowerCase() === "r" && state.mode === "normal") {
      return { state: { ...state, pending: "" }, handled: true, command: "redo" };
    }
    return { state, handled: false };
  }

  // Escape in Visual / pending Normal.
  if (key === "Escape") {
    if (state.mode === "visual" || state.mode === "visual-line") {
      const p = { line: pos.line, column: clampCol(safeLines[pos.line] ?? "", pos.column) };
      return {
        state: { ...state, mode: "normal", pending: "", visualAnchor: undefined },
        handled: true,
        selection: { anchor: p, head: p },
      };
    }
    if (state.pending) {
      return { state: { ...state, pending: "" }, handled: true };
    }
    // Let Escape bubble to StudioView's layered Escape handler.
    return { state, handled: false };
  }

  // Pending inner text-object (`ci`, `di`, `yi` + delimiter).
  if (state.pending === "ci" || state.pending === "di" || state.pending === "yi") {
    const op = state.pending[0]!;
    const lineStr = safeLines[pos.line] ?? "";
    const range = findInnerObject(lineStr, pos.line, pos.column, key);
    if (!range) {
      return { state: { ...state, pending: "" }, handled: true };
    }
    const yanked = sliceRange(safeLines, range);
    if (op === "y") {
      return {
        state: { ...state, pending: "", register: yanked, registerLinewise: false },
        handled: true,
        selection: { anchor: range.start, head: range.start },
      };
    }
    const nextMode: VimMode = op === "c" ? "insert" : "normal";
    return {
      state: { ...state, mode: nextMode, pending: "", register: yanked, registerLinewise: false },
      handled: true,
      edit: { range, text: "" },
      selection: { anchor: range.start, head: range.start },
    };
  }

  // Pending single-char operator (`g`, `d`, `y`, `c`).
  if (state.pending === "g") {
    if (key === "g") {
      const target = { line: 0, column: 0 };
      const sel =
        state.mode === "visual" || state.mode === "visual-line"
          ? visualSelectionFor(state.mode, safeLines, state.visualAnchor ?? pos, target)
          : { anchor: target, head: target };
      return { state: { ...state, pending: "" }, handled: true, selection: sel };
    }
    return { state: { ...state, pending: "" }, handled: true };
  }

  if (state.pending === "d" || state.pending === "y" || state.pending === "c") {
    if (key === "i") {
      return { state: { ...state, pending: `${state.pending}i` }, handled: true };
    }
    if (key === state.pending && (state.pending === "d" || state.pending === "y")) {
      const lineText = safeLines[pos.line] ?? "";
      if (state.pending === "y") {
        return {
          state: { ...state, pending: "", register: lineText, registerLinewise: true },
          handled: true,
        };
      }
      // `dd`: delete entire line.
      let range: EditorRange;
      let nextLine = pos.line;
      if (safeLines.length === 1) {
        range = { start: { line: 0, column: 0 }, end: { line: 0, column: lineText.length } };
      } else if (pos.line + 1 < safeLines.length) {
        range = { start: { line: pos.line, column: 0 }, end: { line: pos.line + 1, column: 0 } };
      } else {
        const prevLen = (safeLines[pos.line - 1] ?? "").length;
        range = { start: { line: pos.line - 1, column: prevLen }, end: { line: pos.line, column: lineText.length } };
        nextLine = pos.line - 1;
      }
      const nextPos = { line: nextLine, column: 0 };
      return {
        state: { ...state, pending: "", register: lineText, registerLinewise: true },
        handled: true,
        edit: { range, text: "" },
        selection: { anchor: nextPos, head: nextPos },
      };
    }
    return { state: { ...state, pending: "" }, handled: true };
  }

  // Visual mode operations (`d`, `x`, `y`, `c`).
  if (state.mode === "visual" || state.mode === "visual-line") {
    const anchor = state.visualAnchor ?? pos;
    if (key === "d" || key === "x" || key === "y" || key === "c") {
      const { range, linewise } = visualRangeOf(state.mode, safeLines, anchor, pos);
      const yanked = linewise
        ? safeLines.slice(Math.min(anchor.line, pos.line), Math.max(anchor.line, pos.line) + 1).join("\n")
        : sliceRange(safeLines, range);
      if (key === "y") {
        const p = range.start;
        return {
          state: { ...state, mode: "normal", pending: "", visualAnchor: undefined, register: yanked, registerLinewise: linewise },
          handled: true,
          selection: { anchor: p, head: p },
        };
      }
      const nextMode: VimMode = key === "c" ? "insert" : "normal";
      const nextPos = { line: range.start.line, column: linewise ? 0 : range.start.column };
      return {
        state: { ...state, mode: nextMode, pending: "", visualAnchor: undefined, register: yanked, registerLinewise: linewise },
        handled: true,
        edit: { range, text: "" },
        selection: { anchor: nextPos, head: nextPos },
      };
    }
  }

  // Motions (shared by Normal & Visual).
  const moveTo = (target: EditorPosition): VimStepResult => {
    const clamped = {
      line: clampLine(safeLines, target.line),
      column: clampCol(safeLines[clampLine(safeLines, target.line)] ?? "", target.column),
    };
    const sel =
      state.mode === "visual" || state.mode === "visual-line"
        ? visualSelectionFor(state.mode, safeLines, state.visualAnchor ?? pos, clamped)
        : { anchor: clamped, head: clamped };
    return { state: { ...state, pending: "" }, handled: true, selection: sel };
  };

  const curLineStr = safeLines[pos.line] ?? "";
  switch (key) {
    case "h":
      return moveTo({ line: pos.line, column: Math.max(0, pos.column - 1) });
    case "l":
      return moveTo({ line: pos.line, column: pos.column + 1 });
    case "j":
      return moveTo({ line: pos.line + 1, column: pos.column });
    case "k":
      return moveTo({ line: pos.line - 1, column: pos.column });
    case "0":
      return moveTo({ line: pos.line, column: 0 });
    case "$":
      return moveTo({ line: pos.line, column: Math.max(0, curLineStr.length - 1) });
    case "w":
      return moveTo(nextWordStart(safeLines, pos));
    case "b":
      return moveTo(prevWordStart(safeLines, pos));
    case "e":
      return moveTo(wordEnd(safeLines, pos));
    case "G":
      return moveTo({ line: safeLines.length - 1, column: 0 });
    case "g":
      return { state: { ...state, pending: "g" }, handled: true };
    default:
      break;
  }

  if (state.mode !== "normal") {
    return { state, handled: true };
  }

  // Normal-mode commands.
  switch (key) {
    case "i":
      return { state: { ...state, mode: "insert", pending: "" }, handled: true };
    case "a": {
      const col = clampCol(curLineStr, pos.column + 1, true);
      const p = { line: pos.line, column: col };
      return { state: { ...state, mode: "insert", pending: "" }, handled: true, selection: { anchor: p, head: p } };
    }
    case "I": {
      const m = curLineStr.match(/^\s*/);
      const p = { line: pos.line, column: m ? m[0].length : 0 };
      return { state: { ...state, mode: "insert", pending: "" }, handled: true, selection: { anchor: p, head: p } };
    }
    case "A": {
      const p = { line: pos.line, column: curLineStr.length };
      return { state: { ...state, mode: "insert", pending: "" }, handled: true, selection: { anchor: p, head: p } };
    }
    case "o": {
      const end = { line: pos.line, column: curLineStr.length };
      const next = { line: pos.line + 1, column: 0 };
      return {
        state: { ...state, mode: "insert", pending: "" },
        handled: true,
        edit: { range: { start: end, end }, text: "\n" },
        selection: { anchor: next, head: next },
      };
    }
    case "O": {
      const start = { line: pos.line, column: 0 };
      return {
        state: { ...state, mode: "insert", pending: "" },
        handled: true,
        edit: { range: { start, end: start }, text: "\n" },
        selection: { anchor: start, head: start },
      };
    }
    case "x": {
      if (pos.column >= curLineStr.length) return { state, handled: true };
      const ch = curLineStr[pos.column] ?? "";
      const range = { start: pos, end: { line: pos.line, column: pos.column + 1 } };
      const nextCol = Math.max(0, Math.min(pos.column, curLineStr.length - 2));
      const p = { line: pos.line, column: nextCol };
      return {
        state: { ...state, register: ch, registerLinewise: false },
        handled: true,
        edit: { range, text: "" },
        selection: { anchor: p, head: p },
      };
    }
    case "d":
    case "y":
    case "c":
      return { state: { ...state, pending: key }, handled: true };
    case "p":
    case "P": {
      if (!state.register) return { state, handled: true };
      if (state.registerLinewise) {
        if (key === "p") {
          const end = { line: pos.line, column: curLineStr.length };
          const next = { line: pos.line + 1, column: 0 };
          return {
            state,
            handled: true,
            edit: { range: { start: end, end }, text: `\n${state.register}` },
            selection: { anchor: next, head: next },
          };
        }
        const start = { line: pos.line, column: 0 };
        return {
          state,
          handled: true,
          edit: { range: { start, end: start }, text: `${state.register}\n` },
          selection: { anchor: start, head: start },
        };
      }
      const col = key === "p" ? clampCol(curLineStr, pos.column + 1, true) : pos.column;
      const at = { line: pos.line, column: col };
      const endCol = col + state.register.length;
      const p = { line: pos.line, column: Math.max(col, endCol - 1) };
      return {
        state,
        handled: true,
        edit: { range: { start: at, end: at }, text: state.register },
        selection: { anchor: p, head: p },
      };
    }
    case "u":
      return { state, handled: true, command: "undo" };
    case "v": {
      const sel = visualSelectionFor("visual", safeLines, pos, pos);
      return { state: { ...state, mode: "visual", pending: "", visualAnchor: pos }, handled: true, selection: sel };
    }
    case "V": {
      const sel = visualSelectionFor("visual-line", safeLines, pos, pos);
      return { state: { ...state, mode: "visual-line", pending: "", visualAnchor: pos }, handled: true, selection: sel };
    }
    default:
      return { state, handled: false };
  }
}
