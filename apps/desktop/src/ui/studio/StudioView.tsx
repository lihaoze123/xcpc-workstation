// The immersive publishing studio (§2.2): the outline of a contest, the editor with its
// merged single-row tab/toolbar bar (题面 | 样例 | 题解 + 4 compact insert icons), the
// paper preview, and the publishing drawer.
//
// The editor stays mounted when switching to 样例 (`display: none/flex`) so undo history,
// carets, and scroll survive. Typing debounces preview compilation (100 ms single problem,
// 200 ms contest) and auto-saves after 500 ms with SHA-256 conflict detection. Ctrl+click
// on the preview locates Markdown source; Mod+Shift+L locates the preview from the caret.

import { batch, cleanup, Clipboard, computed, effect, keymap, reduceMotion, signal, Text, untracked, View, windowSize } from "nsh";
import { ResizablePanel, ResizablePanelGroup, type EditorDiagnostic, type EditorRef } from "nsh/base";
import type {
  ActionResult,
  CleanUnusedAssetsResultJs,
  ExportBookStatusJs,
  PackageInspectionResultJs,
  PreviewDiagnostic,
  PreviewRequest,
  PreviewStart,
  SourceLocationJs,
  WorkspaceFile,
} from "xcpc-workstation/native";
import { createFreeze } from "../Contest";
import { ErrorDisplay } from "../ErrorDisplay";
import { locale, t, type MessageKey } from "../i18n";
import { Btn, IconBtn, Lock, Segmented, tip } from "../kit";
import { EmptyState } from "../layout";
import { AppMenu, type MenuEntry } from "../Menu";
import { adaptiveCompileDelayMs, minimalEdit, ratioOf, type Book } from "../logic";
import { setToastBottomLift, showToast } from "../Toast";
import {
  parseJsonReport,
  type PluginExportReport,
  type RegisteredExporter,
} from "../tools/types";
import {
  activeRevision,
  book as storeBook,
  closeStudio,
  dialog,
  errorOf,
  goSpace,
  info,
  loadPrefs,
  onExternalChange,
  openStudio,
  pluginsRevision,
  preview,
  refresh,
  registerQuitFlusher,
  savePrefs,
  splitRatioSignal,
  studio,
  studioActions,
  unfreezeFor,
  workspaceVersion,
  type OjPackageFormat,
  type StudioTarget,
  type UiError,
} from "../store";
import { afterNextPaint } from "../frame";
import { panelTransition } from "../motion";
import { traceInteraction, traceSpan, traceUntilPaint } from "../trace";
import { ms, popoverShadow, token } from "../theme";
import { TitleBar } from "../TitleBar";
import { AssetsGalleryModal } from "./AssetsGalleryModal";
import { BuildConsole, type BuildConsoleController } from "./BuildConsole";
import { ContestCheckBar } from "./ContestCheckBar";
import { contestBatch, contestCheck, ensureContestCheck, idsToCheck, startContestCheck } from "./contestCheck";
import type { StatusMarkKind } from "./StatusSlot";
import { markForAuditRow } from "./statusLogic";
import { ExportButton } from "./ExportButton";
import { Inspector, INSPECTOR_WIDTH } from "./Inspector";
import { MarkdownEditor, type MarkdownEditorActions } from "./MarkdownEditor";
import { InitPackageModal } from "./PackageModals";
import type { VimMode } from "./vim";
import { Outline } from "./Outline";
import { PreviewCanvas, type CanvasController } from "./PreviewCanvas";
import { Samples } from "./Samples";

import { FONT_SIZE, FONT_WEIGHT, RADIUS, ROW, SPACE } from "../tokens";
type StudioMode = "statement" | "authoring";
type Tab = "statement" | "samples" | "editorial";
const lastStudioModeByProblem = new Map<string, "statement" | "authoring">();
const SAVE_AFTER = 500;
const TAB_DEFS: readonly { value: Tab; labelKey: MessageKey; keys: string }[] = [
  { value: "statement", labelKey: "studio.tab.statement", keys: "alt+1" },
  { value: "samples", labelKey: "studio.tab.samples", keys: "alt+2" },
  { value: "editorial", labelKey: "studio.tab.editorial", keys: "alt+3" },
];

/** Computes a non-zero-width range on `lineStr` starting at `col` so diagnostics show a visible squiggle and hover target. */
function diagnosticRange(lineStr: string, line: number, column: number) {
  const startCol = Math.max(0, Math.min(column, lineStr.length));
  if (lineStr.length === 0) {
    return { start: { line, column: 0 }, end: { line, column: 0 } };
  }
  // If column is at/past end of line, mark the last character.
  if (startCol >= lineStr.length) {
    return { start: { line, column: Math.max(0, lineStr.length - 1) }, end: { line, column: lineStr.length } };
  }
  // Extend through a contiguous token (word/command/math run), else to the end of the line (capped at 40 chars).
  const rest = lineStr.slice(startCol);
  const tokenMatch = rest.match(/^(?:\\[A-Za-z]+|\$+|\S+)/);
  const spanLen = tokenMatch ? Math.min(tokenMatch[0].length, 40) : Math.min(rest.length, 24);
  const endCol = Math.min(lineStr.length, startCol + Math.max(1, spanLen));
  return { start: { line, column: startCol }, end: { line, column: endCol } };
}

export function StudioView(props: { active: boolean }) {
  // The studio keeps what it showed while hidden, so coming back is instant.
  const target = computed<StudioTarget | undefined>((prev) => studio() ?? prev);
  const studioMode = signal<StudioMode>("statement");
  const coverActive = signal(false);
  // 出题 is a problem's own panel; in a contest it is the selected problem's (§4), not the cover's.
  const isAuthoring = computed(() => {
    if (studioMode() !== "authoring") return false;
    const tg = target();
    if (tg?.target === "problem") return true;
    return tg?.target === "contest" && !coverActive() && !!item() && !item()!.missing;
  });
  const isFullConsole = computed(() => isAuthoring());
  const initialStudioTab = untracked(studio)?.initialTab;
  const tab = signal<Tab>(
    initialStudioTab && initialStudioTab !== "build"
      ? initialStudioTab
      : storeBook() === "editorial"
        ? "editorial"
        : "statement",
  );
  const book = computed<Book>(() => (tab() === "editorial" ? "editorial" : "statements"));
  const initialWinWidth = untracked(windowSize).width;
  const inspectorOpen = signal(
    initialWinWidth > 0 && initialWinWidth < 1200 ? false : (loadPrefs().propsOpen ?? false),
  );
  let userToggledInspector = false;
  let checkedNarrowDefault = initialWinWidth > 0;
  effect(() => {
    const w = windowSize().width;
    if (w <= 0 || checkedNarrowDefault) return;
    checkedNarrowDefault = true;
    if (!userToggledInspector && w < 1200 && untracked(inspectorOpen)) {
      inspectorOpen.set(false);
    }
  });
  const assetsOpen = signal(false);
  const assetsHasTypstBlock = signal(false);
  const previewCollapsed = signal(false);
  const focusMode = signal(false);
  const previewHidden = computed(() => focusMode() || previewCollapsed() || isFullConsole());

  // Lift the global bottom-right toast above the preview canvas's zoom pill when preview is shown.
  effect(() => {
    setToastBottomLift(props.active && !previewHidden());
  });
  cleanup(() => setToastBottomLift(false));

  // Soft visual transitions for focus mode & preview collapse/expand (GPU transform + opacity only;
  // never triggered by typing or scrolling, and never remounts MarkdownEditor).
  const outlineVisible = signal(true);
  const outlineShown = signal(true);
  const previewShown = signal(true);
  let outlineRaf: number | undefined;
  let outlineTimer: number | undefined;
  let previewRaf: number | undefined;
  let layoutMotionInit = false;

  effect(() => {
    const showOutline = !focusMode();
    if (outlineRaf !== undefined) {
      cancelAnimationFrame(outlineRaf);
      outlineRaf = undefined;
    }
    clearTimeout(outlineTimer);
    if (showOutline) {
      outlineVisible.set(true);
      if (reduceMotion()) {
        outlineShown.set(true);
      } else {
        outlineRaf = requestAnimationFrame(() => {
          outlineRaf = undefined;
          outlineShown.set(true);
        });
      }
    } else {
      outlineShown.set(false);
      const dur = ms(180);
      if (dur === 0) outlineVisible.set(false);
      else outlineTimer = setTimeout(() => outlineVisible.set(false), dur);
    }
  });

  effect(() => {
    const hidden = previewHidden();
    const first = !layoutMotionInit;
    layoutMotionInit = true;
    if (previewRaf !== undefined) {
      cancelAnimationFrame(previewRaf);
      previewRaf = undefined;
    }
    if (first || reduceMotion()) {
      previewShown.set(!hidden);
      return;
    }
    if (hidden) {
      previewShown.set(false);
    } else {
      previewShown.set(false);
      previewRaf = requestAnimationFrame(() => {
        previewRaf = undefined;
        previewShown.set(true);
      });
    }
  });

  cleanup(() => {
    if (outlineRaf !== undefined) cancelAnimationFrame(outlineRaf);
    clearTimeout(outlineTimer);
    if (previewRaf !== undefined) cancelAnimationFrame(previewRaf);
  });
  const assetsRefreshSeq = signal(0);
  const packageRefreshSeq = signal(0);
  const starts = signal<PreviewStart[]>([]);
  const diagnostics = signal<PreviewDiagnostic[]>([]);
  const status = signal<ExportBookStatusJs | undefined>(undefined);
  const exporting = signal<Book | OjPackageFormat | undefined>(undefined);
  const exportedSeq = signal(0);
  const lastOjRevealPath = signal<string | undefined>(undefined);
  const notice = signal<UiError | undefined>(undefined);
  const vimMode = signal<VimMode | undefined>(undefined);
  const freeze = createFreeze();
  let canvas: CanvasController | undefined;
  let editorActions: MarkdownEditorActions | undefined;
  const buildConsole = signal<BuildConsoleController | undefined>(undefined);

  const rev = activeRevision(() => props.active);

  const contest = computed(() => {
    void rev();
    const t = target();
    return t?.target === "contest" ? preview.contestDetail(t.id) : undefined;
  });
  const label = computed(() => {
    const c = contest();
    if (!c) return undefined;
    const want = target()?.label;
    return want && c.items.some((i) => i.label === want) ? want : c.items[0]?.label;
  });
  const item = computed(() => contest()?.items.find((i) => i.label === label()));
  const problemId = computed(() => (target()?.target === "problem" ? target()!.id : item()?.id));
  const packageInspectionState = signal<
    { slug: string; contestId: string | null; res: PackageInspectionResultJs } | undefined
  >(undefined);
  let packageInspectSeq = 0;
  cleanup(() => {
    packageInspectSeq++;
  });
  effect(() => {
    if (!props.active) return;
    void rev();
    void packageRefreshSeq();
    const t = target();
    const it = item();
    const slug = t?.target === "problem" ? t.id : t?.target === "contest" && it && !it.missing ? it.id : undefined;
    const contestId = t?.target === "contest" ? t.id : null;
    const seq = ++packageInspectSeq;
    if (!slug) {
      untracked(() => packageInspectionState.set(undefined));
      return;
    }
    void preview
      .inspectPackageAsync(slug, contestId)
      .then((res) => {
        if (seq === packageInspectSeq) packageInspectionState.set({ slug, contestId, res });
      })
      .catch(() => {
        if (seq === packageInspectSeq) packageInspectionState.set(undefined);
      });
  });
  const packageInspection = computed<PackageInspectionResultJs | undefined>(() => {
    const state = packageInspectionState();
    if (!state) return undefined;
    const t = target();
    const it = item();
    const slug = t?.target === "problem" ? t.id : t?.target === "contest" && it && !it.missing ? it.id : undefined;
    const contestId = t?.target === "contest" ? t.id : null;
    if (!slug || state.slug !== slug || state.contestId !== contestId) return undefined;
    return state.res;
  });
  const hasPackage = computed(() => {
    const insp = packageInspection();
    if (insp) return !!insp.hasPackage;
    void rev();
    void packageRefreshSeq();
    const pid = problemId();
    return pid ? preview.hasPackage(pid) : false;
  });
  const frozen = computed(() => !!contest()?.frozen);
  const archived = computed(() => !!contest()?.archived);
  const paths = computed(() => {
    const t = target();
    if (!t) return undefined;
    if (t.target === "problem") {
      const p = info().problems.find((x) => x.id === t.id);
      return p ? { statement: p.statement, editorial: p.editorial } : undefined;
    }
    const it = item();
    return it ? { statement: it.statement, editorial: it.editorial } : undefined;
  });
  const path = computed(() => {
    const p = paths();
    return p ? (tab() === "editorial" ? p.editorial : p.statement) : undefined;
  });

  // Clear transient notices and stale freeze errors when switching target or workspace.
  let lastFreezeOwner = "";
  effect(() => {
    const t = target();
    const wv = workspaceVersion();
    const owner = `${wv}:${t?.target ?? ""}:${t?.id ?? ""}`;
    untracked(() => {
      notice.set(undefined);
      lastOjRevealPath.set(undefined);
      assetsOpen.set(false);
      if (owner !== lastFreezeOwner) {
        lastFreezeOwner = owner;
        freeze.reset();
        statusSeq++;
        status.set(undefined);
      }
      if (t?.target !== "contest") coverActive.set(false);
    });
  });

  // Apply optional `initialTab` when navigating directly to Authoring or a Statement tab.
  // Switching problems inside the same contest keeps the user's current 「题面 | 出题」 mode.
  let lastContestId: string | undefined;
  effect(() => {
    const s = studio();
    if (!s) return;
    const want = s.initialTab;
    untracked(() => {
      if (want === "build" && s.target === "contest") {
        studioMode.set("authoring");
      } else if (want === "build" && s.target === "problem") {
        packageRefreshSeq.set(packageRefreshSeq() + 1);
        studioMode.set("authoring");
        lastStudioModeByProblem.set(s.id, "authoring");
      } else if (want && want !== "build") {
        studioMode.set("statement");
        if (s.target === "problem") lastStudioModeByProblem.set(s.id, "statement");
        tab.set(want);
        const b: Book = want === "editorial" ? "editorial" : "statements";
        if (storeBook() !== b) {
          storeBook.set(b);
          savePrefs({ lastBook: b });
        }
      } else if (s.target === "problem" && !want) {
        studioMode.set(lastStudioModeByProblem.get(s.id) ?? "statement");
      } else if (s.target === "contest") {
        if (s.id !== lastContestId) studioMode.set("statement");
      } else {
        studioMode.set("statement");
      }
      lastContestId = s.target === "contest" ? s.id : undefined;
    });
  });

  // ── The editor's file ──────────────────────────────────────────────────────
  // Its text is kept outside signals: typing must not re-render the studio.
  const file = signal<WorkspaceFile | undefined>(undefined);
  const conflict = signal<{ text: string; hash: string } | undefined>(undefined);
  let text = "";
  let dirty = false;
  let slashActive = false;
  let saveTimer: number | undefined;
  let compileTimer: number | undefined;
  let editor: EditorRef | undefined;
  let editorPath = "";

  // One MarkdownEditor per recently opened file (LRU): switching back to a file
  // is a display flip instead of rebuilding NSH's <Editor> (40-100ms), and each
  // file keeps its own undo stack, caret and scroll. Hidden ones are inactive.
  const EDITOR_SLOTS = 8;
  interface EditorSlot {
    path: string;
    /** What the editor was created with. */
    first: WorkspaceFile;
    /** Its text and disk hash when it was last the current file. */
    text: string;
    hash: string;
    ref?: EditorRef;
    actions?: MarkdownEditorActions;
    vim?: VimMode;
  }
  const slots = signal<EditorSlot[]>([]);
  /** A new slot's editor takes the focus when it mounts (the outgoing one had it). */
  let focusOnMount = false;
  const isCurrentSlot = (s: EditorSlot) => file()?.path === s.path;

  /** Makes `f` the current editor: reuses its slot or adds one, evicting the oldest. */
  function enterSlot(f: WorkspaceFile, focus: boolean) {
    focusOnMount = focus;
    const list = untracked(slots);
    const found = list.find((s) => s.path === f.path);
    if (found) {
      slots.set([...list.filter((s) => s !== found), found]);
      text = found.text;
      editor = found.ref;
      editorActions = found.actions;
      editorPath = found.ref ? f.path : "";
      vimMode.set(found.vim);
      if (focus) found.ref?.focus();
      // Changed on disk while hidden: bring it up to date as one undoable edit.
      if (f.hash !== found.hash) replaceText(f.text);
      return;
    }
    const slot: EditorSlot = { path: f.path, first: f, text: f.text, hash: f.hash };
    const next = [...list, slot];
    while (next.length > EDITOR_SLOTS) next.shift();
    slots.set(next);
    text = f.text;
    editor = undefined;
    editorActions = undefined;
    editorPath = "";
  }

  /**
   * Remembers the outgoing file's text and hash in its slot and takes the focus
   * from its (now hidden) editor, which would otherwise keep swallowing keys.
   * Returns whether it had the focus, so the incoming editor can take it.
   */
  function leaveSlot(): boolean {
    const f = untracked(file);
    const slot = f && untracked(slots).find((s) => s.path === f.path);
    if (!slot) return false;
    slot.text = text;
    slot.hash = f.hash;
    const focused = !!slot.actions?.isFocused();
    slot.ref?.blur();
    return focused;
  }
  let pendingCursor: { path: string; line: number; column: number } | undefined;
  let compileImmediate = false;

  let lastKeystrokeAt = 0;

  function scheduleCompile() {
    clearTimeout(compileTimer);
    const now = typeof performance !== "undefined" ? performance.now() : Date.now();
    const interKeystrokeMs = lastKeystrokeAt > 0 ? now - lastKeystrokeAt : 1000;
    lastKeystrokeAt = now;
    const isContest = untracked(target)?.target === "contest";
    const delay = adaptiveCompileDelayMs(isContest, interKeystrokeMs, canvas?.lastCompileMs() ?? 0);
    compileTimer = setTimeout(() => {
      compileTimer = undefined;
      canvas?.compile();
    }, delay);
  }

  function compileNow() {
    clearTimeout(compileTimer);
    compileTimer = undefined;
    canvas?.compile();
  }

  function save(): boolean {
    clearTimeout(saveTimer);
    saveTimer = undefined;
    const f = untracked(file);
    if (untracked(conflict)) return false;
    if (!dirty || !f || f.readOnly) return true;
    const res = preview.save(f.path, text, f.hash);
    if (res.ok) {
      dirty = false;
      file.set({ ...f, hash: res.hash });
      notice.set(undefined);
      assetsRefreshSeq.set(assetsRefreshSeq() + 1);
      scheduleStatus();
      return true;
    } else if (res.conflict) {
      const disk = preview.read(f.path);
      conflict.set({ text: disk.text, hash: disk.hash });
      return false;
    } else {
      notice.set(errorOf(res, t("ctx.save")));
      return false;
    }
  }
  cleanup(registerQuitFlusher(save));

  effect(() => {
    const next = path();
    untracked(() => {
      if (next === file()?.path) return;
      clearTimeout(compileTimer);
      compileTimer = undefined;
      save();
      const hadFocus = leaveSlot();
      batch(() => {
        conflict.set(undefined);
        dirty = false;
        if (!next) {
          text = "";
          editor = undefined;
          editorActions = undefined;
          editorPath = "";
          file.set(undefined);
        } else {
          const f = preview.read(next);
          enterSlot(f, hadFocus);
          file.set(f);
        }
      });
      scheduleStatus();
    });
  });

  /**
   * Puts `next` into the editor in place: awaits scroll offset before mutating `text`
   * and aborts if the user typed or switched files during the microtask, eliminating
   * async edit offset races.
   */
  function replaceText(next: string, force = false) {
    if (!editor) {
      text = next;
      return;
    }
    const ed = editor;
    const expectedPath = editorPath;
    void ed.scrollOffset().then((offset) => {
      if (editor !== ed || editorPath !== expectedPath) return;
      if (!force && dirty) return;
      const edit = minimalEdit(text, next);
      text = next;
      if (!edit) return;
      ed.edit([edit]);
      ed.setScrollOffset(offset);
    });
  }

  const unwatch = onExternalChange((changed, rescan) => {
    const f = untracked(file);
    if (f && (rescan || changed.includes(f.path))) {
      const disk = preview.read(f.path);
      if (disk.hash !== f.hash) {
        if (dirty) {
          conflict.set({ text: disk.text, hash: disk.hash });
        } else {
          replaceText(disk.text);
          file.set({ ...f, hash: disk.hash, readOnly: disk.readOnly });
        }
      }
    }
    compileNow();
    assetsRefreshSeq.set(assetsRefreshSeq() + 1);
    packageRefreshSeq.set(packageRefreshSeq() + 1);
    scheduleStatus();
  });
  cleanup(unwatch);

  function takeTheirs() {
    const c = conflict();
    const f = file();
    if (!c || !f) return;
    clearTimeout(saveTimer);
    dirty = false;
    replaceText(c.text, true);
    batch(() => {
      file.set({ ...f, hash: c.hash });
      conflict.set(undefined);
    });
    compileNow();
  }

  function keepMine() {
    const c = conflict();
    const f = file();
    if (!c || !f) return;
    const res = preview.save(f.path, text, c.hash);
    if (res.ok) {
      dirty = false;
      batch(() => {
        file.set({ ...f, hash: res.hash });
        conflict.set(undefined);
      });
    } else if (res.conflict) {
      const disk = preview.read(f.path);
      conflict.set({ text: disk.text, hash: disk.hash });
    } else {
      notice.set(errorOf(res, t("ctx.save")));
    }
  }

  function applyPending() {
    const p = pendingCursor;
    if (!p || !editor || editorPath !== p.path) return;
    pendingCursor = undefined;
    const at = { line: Math.max(0, p.line), column: Math.max(0, p.column) };
    editor.setSelections([{ anchor: at, head: at }]);
    editor.revealPosition(at, true);
    editor.focus();
  }

  const editorDiagnostics = computed<EditorDiagnostic[]>(() => {
    const curPath = file()?.path;
    if (!curPath) return [];
    const ls = text.split("\n");
    return diagnostics()
      .filter((d) => d.file === curPath && d.line != null)
      .map((d) => {
        const lineIdx = Math.max(0, d.line! - 1);
        const colIdx = Math.max(0, (d.column ?? 1) - 1);
        const lineStr = ls[lineIdx] ?? "";
        return {
          range: diagnosticRange(lineStr, lineIdx, colIdx),
          severity: d.severity === "error" ? "error" : "warning",
          message: d.message,
        };
      });
  });

  // ── Preview, export ────────────────────────────────────────────────────────

  function request(): PreviewRequest | undefined {
    const t = target();
    if (!t) return undefined;
    const f = file();
    return { target: t.target, id: t.id, book: book(), path: f?.path ?? "", text: f ? text : "" };
  }

  let statusTimer: number | undefined;
  let statusSeq = 0;
  function scheduleStatus(now = false) {
    clearTimeout(statusTimer);
    const run = () => {
      const req = untracked(request);
      const seq = ++statusSeq;
      if (!req) {
        status.set(undefined);
        return;
      }
      void preview
        .exportStatusAsync(req)
        .then((res) => {
          if (seq === statusSeq) status.set(res);
        })
        .catch(() => {
          if (seq === statusSeq) status.set(undefined);
        });
    };
    if (now) run();
    else statusTimer = setTimeout(run, 300);
  }
  cleanup(() => {
    statusSeq++;
    clearTimeout(statusTimer);
    clearTimeout(compileTimer);
    save();
  });

  async function exportBook(b: Book) {
    save();
    const req = request();
    if (!req || exporting()) return;
    exporting.set(b);
    const out = await preview.export({ ...req, book: b });
    exporting.set(undefined);
    if (out.error) {
      notice.set({ error: out.error, context: b === "editorial" ? t("ctx.exportEditorial") : t("ctx.exportStatements") });
    } else {
      notice.set(undefined);
      lastOjRevealPath.set(undefined);
      exportedSeq.set(exportedSeq() + 1);
    }
    scheduleStatus(true);
  }

  async function exportOjPackage(format: OjPackageFormat) {
    save();
    const cur = untracked(target);
    if (!cur || exporting()) return;
    exporting.set(format);
    const ctx = format === "domjudge" ? t("ctx.exportDomjudge") : t("ctx.exportHydro");
    const res = await preview.exportOjPackage(format, cur.target, cur.id);
    exporting.set(undefined);
    if (!res.ok) {
      notice.set({
        code: res.code,
        error: res.detail ?? res.error,
        context: ctx,
      });
      return;
    }
    notice.set(undefined);
    lastOjRevealPath.set(res.revealPath || res.paths[0]);
    exportedSeq.set(exportedSeq() + 1);
  }

  const pluginExporters = computed<RegisteredExporter[]>(() => {
    void pluginsRevision();
    return parseJsonReport<RegisteredExporter[]>(
      typeof preview.pluginListExporters === "function" ? preview.pluginListExporters() : undefined,
      [],
    );
  });

  async function exportViaPlugin(pluginId: string, exporterId: string) {
    save();
    const cur = untracked(target);
    if (!cur || exporting()) return;
    const res = await preview.pluginRunExporter(pluginId, exporterId, cur.target, cur.id);
    if (!res.ok) {
      showToast({ tone: "issues", text: res.error ?? t("tools.exportFailed") });
      return;
    }
    const report = parseJsonReport<PluginExportReport>(res, { out_dir: "", files: [] });
    if (report.out_dir) {
      lastOjRevealPath.set(report.out_dir);
    }
    exportedSeq.set(exportedSeq() + 1);
    showToast({
      tone: "success",
      text: report.summary || t("tools.exportSuccess"),
      action: report.out_dir
        ? {
            label: t("common.revealInFolder"),
            run: () => void preview.revealInFolder(report.out_dir),
          }
        : undefined,
    });
  }

  async function reveal(b: Book) {
    const ojPath = lastOjRevealPath();
    if (ojPath) {
      const res = await preview.revealInFolder(ojPath);
      if (!res.ok) notice.set(errorOf(res, t("ctx.revealInFolder")));
      return;
    }
    const s = status()?.[b];
    if (!s?.exists) return;
    const res = await preview.revealInFolder(s.absPath);
    if (!res.ok) notice.set(errorOf(res, t("ctx.revealInFolder")));
  }

  // ── Navigation ─────────────────────────────────────────────────────────────

  function setTab(next: Tab | "build") {
    if (next === "build") {
      studioMode.set("authoring");
      return;
    }
    if (studioMode() !== "statement") studioMode.set("statement");
    if (next === tab()) return;
    tab.set(next);
    const b: Book = next === "editorial" ? "editorial" : "statements";
    if (storeBook() !== b) {
      storeBook.set(b);
      savePrefs({ lastBook: b });
    }
  }

  function setStudioMode(next: StudioMode) {
    const t = target();
    if (!t) return;
    if (next === "authoring") packageRefreshSeq.set(packageRefreshSeq() + 1);
    if (t.target === "problem") lastStudioModeByProblem.set(t.id, next);
    studioMode.set(next);
  }

  const compactToggles = computed(() => target()?.target === "contest" && windowSize().width > 0 && windowSize().width < 1280);

  // ── Contest check (§4) ─────────────────────────────────────────────────────
  effect(() => {
    const tg = target();
    if (tg?.target === "contest") untracked(() => ensureContestCheck(tg.id));
  });

  // When a contest batch check moves on, the open problem's panel reads its new state.
  let lastBatchCompleted = -1;
  effect(() => {
    const b = contestBatch();
    const done = b ? b.completed + (b.running ? 0 : 1000) : -1;
    if (done === lastBatchCompleted) return;
    lastBatchCompleted = done;
    untracked(() => packageRefreshSeq.set(packageRefreshSeq() + 1));
  });

  /** A contest problem's outline mark: spinning while the batch checks it, else its state. */
  function outlineMark(problemId: string): StatusMarkKind {
    const b = contestBatch();
    if (b?.running && b.currentProblemId === problemId) return "running";
    const report = contestCheck()?.report;
    return markForAuditRow(report?.problems.find((r) => r.problem_id === problemId));
  }

  /** 「前往」 from the contest status popover: that problem's 出题 panel. */
  function goContestProblem(problemId: string) {
    const it = contest()?.items.find((i) => i.id === problemId);
    if (!it) return;
    selectLabel(it.label);
    setStudioMode("authoring");
  }

  const initPackageModalOpen = signal(false);

  function initPackageScaffold() {
    const tTarget = untracked(target);
    if (!tTarget || tTarget.target !== "problem" || info().readOnly) return;
    initPackageModalOpen.set(true);
  }

  function confirmInitPackageScaffold(opts: { interactive: boolean; runs: number | null; subtasks: boolean }) {
    const tTarget = untracked(target);
    if (!tTarget || tTarget.target !== "problem" || info().readOnly) return;
    const res = preview.initPackageWithOptions(tTarget.id, opts.interactive, opts.runs, opts.subtasks);
    if (!res.ok) {
      notice.set({ code: res.code, error: res.error, context: t("ctx.initPackage") });
      return;
    }
    notice.set(undefined);
    packageRefreshSeq.set(packageRefreshSeq() + 1);
    refresh();
    compileNow();
    studioMode.set("authoring");
  }

  function selectLabel(next: string, jump = true) {
    const t = target();
    if (!t || t.target !== "contest") return;
    coverActive.set(false);
    if (next !== label()) openStudio({ ...t, label: next });
    if (jump) canvas?.jumpTo(next);
  }

  // Outline clicks commit only the highlight with the input; the editor,
  // inspector and preview follow once that frame is on screen. A newer click
  // supersedes a pending one, so fast A→B→C only builds C.
  const pendingLabel = signal<string | undefined>(undefined);
  let pendingSelect = 0;
  // Leaving the studio cancels a pending swap (it would reopen the studio).
  cleanup(() => void (pendingSelect += 1));
  function selectLabelSoon(next: string) {
    traceInteraction("outline.select", () => {
      if (next === (pendingLabel() ?? label())) {
        coverActive.set(false);
        canvas?.jumpTo(next);
        return;
      }
      const token = ++pendingSelect;
      const contestId = target()?.id;
      batch(() => {
        coverActive.set(false);
        pendingLabel.set(next);
      });
      afterNextPaint(() => {
        if (token !== pendingSelect) return;
        if (target()?.id !== contestId) {
          pendingLabel.set(undefined);
          return;
        }
        traceUntilPaint("ux.content", "outline.select", () =>
          batch(() => {
            selectLabel(next);
            pendingLabel.set(undefined);
          }),
        );
      });
    });
  }

  /** Opens the source of `loc` in the editor: its problem, its tab, its line. */
  function locate(loc: SourceLocationJs) {
    const t = target();
    if (!t) return;
    let nextTab: Tab | undefined;
    let nextLabel: string | undefined;
    if (t.target === "problem") {
      const p = paths();
      if (loc.file === p?.statement) nextTab = "statement";
      else if (loc.file === p?.editorial) nextTab = "editorial";
    } else {
      for (const it of contest()?.items ?? []) {
        if (loc.file === it.statement) [nextTab, nextLabel] = ["statement", it.label];
        else if (loc.file === it.editorial) [nextTab, nextLabel] = ["editorial", it.label];
      }
    }
    if (!nextTab) return;
    pendingCursor = { path: loc.file, line: loc.line - 1, column: loc.column - 1 };
    batch(() => {
      if (nextLabel) selectLabel(nextLabel, false);
      setTab(nextTab!);
    });
    applyPending();
  }

  /** Scrolls the preview smoothly to the current caret's line in the active Markdown file. */
  async function locatePreview() {
    const f = untracked(file);
    const curTab = untracked(tab);
    if (!f || curTab === "samples" || untracked(isAuthoring)) return;
    const line = (await editorActions?.currentLine()) ?? 1;
    const loc = preview.locateSource(f.path, line);
    if (loc) canvas?.scrollToLocation(loc.page, loc.yPt, true);
  }

  function finding(d: PreviewDiagnostic) {
    if (d.file && d.line != null) locate({ file: d.file, line: d.line, column: d.column ?? 1 });
    else if (d.page != null) canvas?.goPage(d.page);
  }

  /** Persists clipboard image bytes into `assets/` and returns the relative path for editor insertion. */
  function pasteImageBytes(ext: string, bytes: Uint8Array): string | null {
    const cur = untracked(target);
    if (!cur || untracked(readOnly)) return null;
    try {
      const rel = preview.importImageBytes(
        cur.target,
        cur.id,
        cur.target === "contest" ? (untracked(label) ?? null) : null,
        ext,
        bytes,
      );
      notice.set(undefined);
      assetsRefreshSeq.set(assetsRefreshSeq() + 1);
      compileImmediate = true;
      return rel;
    } catch (err) {
      compileImmediate = false;
      notice.set({ error: err instanceof Error ? err.message : String(err), context: t("ctx.pasteImage") });
      return null;
    }
  }

  const hasAssets = () => {
    const t = target();
    return !!t && (t.target === "problem" || (t.target === "contest" && !!item()));
  };

  function detectTypstBlock(): boolean {
    const re = /(?:^|\n)[ \t]*```typst\b/i;
    if (re.test(text)) return true;
    const p = untracked(paths);
    const activePath = untracked(file)?.path;
    for (const candidate of [p?.statement, p?.editorial]) {
      if (!candidate || candidate === activePath) continue;
      try {
        if (re.test(preview.read(candidate).text)) return true;
      } catch {
        // Ignore missing/unreadable sibling doc.
      }
    }
    return false;
  }

  function openAssets() {
    if (!untracked(hasAssets)) return;
    assetsHasTypstBlock.set(detectTypstBlock());
    assetsOpen.set(true);
  }

  /** Imports an image via the system picker into `assets/` without closing the gallery. */
  async function importImageToGallery(): Promise<string | null> {
    const tTarget = untracked(target);
    if (!tTarget || untracked(readOnly)) return null;
    try {
      const rel = await preview.pickAndImportImage(
        tTarget.target,
        tTarget.id,
        tTarget.target === "contest" ? (untracked(label) ?? null) : null,
        {
          title: t("dialog.pickImage.title"),
          buttonLabel: t("dialog.pickImage.button"),
          filterLabel: t("dialog.pickImage.filter"),
        },
      );
      if (!rel) return null;
      notice.set(undefined);
      assetsRefreshSeq.set(assetsRefreshSeq() + 1);
      return rel;
    } catch (err) {
      notice.set({ error: err instanceof Error ? err.message : String(err), context: t("ctx.importImage") });
      return null;
    }
  }

  function reloadActiveFileFromDisk() {
    const f = untracked(file);
    if (!f) return;
    const disk = preview.read(f.path);
    if (disk.hash !== f.hash) {
      dirty = false;
      replaceText(disk.text, true);
      file.set({ ...f, hash: disk.hash, readOnly: disk.readOnly });
    }
  }

  function insertAssetFromGallery(relPath: string) {
    if (untracked(readOnly)) return;
    if (untracked(tab) === "samples") setTab("statement");
    compileImmediate = true;
    void editorActions?.insertImage(relPath);
  }

  function setAssetWidthFromGallery(assetName: string, widthPct: number, docPath?: string | null): ActionResult {
    const cur = untracked(target);
    if (!cur || untracked(readOnly)) return { ok: false, error: t("error.notEditable"), code: "read_only" };
    if (!save()) {
      return { ok: false, error: t("error.unsavedAbortWidth"), code: "conflict" };
    }
    const res = preview.setAssetWidth(
      cur.target,
      cur.id,
      cur.target === "contest" ? (untracked(label) ?? null) : null,
      assetName,
      widthPct,
      docPath ?? null,
    );
    if (res.ok) {
      reloadActiveFileFromDisk();
      assetsRefreshSeq.set(assetsRefreshSeq() + 1);
      refresh();
      compileNow();
      scheduleStatus();
    }
    return res;
  }

  function renameAssetFromGallery(oldName: string, newName: string): ActionResult {
    const cur = untracked(target);
    if (!cur || untracked(readOnly)) return { ok: false, error: t("error.notEditable"), code: "read_only" };
    // Persist unsaved edits first and abort if saving fails (e.g. external conflict).
    if (!save()) {
      return { ok: false, error: t("error.unsavedAbortRename"), code: "conflict" };
    }
    const res = preview.renameProblemAsset(
      cur.target,
      cur.id,
      cur.target === "contest" ? (untracked(label) ?? null) : null,
      oldName,
      newName,
    );
    if (res.ok) {
      reloadActiveFileFromDisk();
      assetsRefreshSeq.set(assetsRefreshSeq() + 1);
      refresh();
      compileNow();
      scheduleStatus();
    }
    return res;
  }

  function deleteAssetFromGallery(name: string): ActionResult {
    const cur = untracked(target);
    if (!cur || untracked(readOnly)) return { ok: false, error: t("error.notEditable"), code: "read_only" };
    const res = preview.deleteProblemAsset(
      cur.target,
      cur.id,
      cur.target === "contest" ? (untracked(label) ?? null) : null,
      name,
    );
    if (res.ok) {
      assetsRefreshSeq.set(assetsRefreshSeq() + 1);
      refresh();
      compileNow();
      scheduleStatus();
    }
    return res;
  }

  function cleanUnusedAssetsFromGallery(): CleanUnusedAssetsResultJs {
    const cur = untracked(target);
    if (!cur || untracked(readOnly)) {
      return { ok: false, removed: [], freedBytes: 0, error: t("error.notEditable"), code: "read_only" };
    }
    const res = preview.cleanUnusedAssets(
      cur.target,
      cur.id,
      cur.target === "contest" ? (untracked(label) ?? null) : null,
    );
    if (res.ok || res.removed.length > 0) {
      assetsRefreshSeq.set(assetsRefreshSeq() + 1);
      refresh();
      compileNow();
      scheduleStatus();
    }
    return res;
  }

  function compose(ids: string[]): boolean {
    const cur = target();
    if (!cur) return false;
    const current = item()?.id;
    const res = preview.composeContest(cur.id, ids);
    if (!res.ok) {
      notice.set(errorOf(res, t("ctx.reorderContest")));
      return false;
    }
    const at = current ? ids.indexOf(current) : -1;
    if (at >= 0) openStudio({ ...cur, label: String.fromCharCode(65 + at) });
    refresh();
    compileNow();
    return true;
  }

  function toggleInspector() {
    userToggledInspector = true;
    const next = !inspectorOpen();
    inspectorOpen.set(next);
    savePrefs({ propsOpen: next });
  }

  function togglePreview() {
    if (focusMode()) {
      batch(() => {
        focusMode.set(false);
        previewCollapsed.set(false);
      });
      return;
    }
    previewCollapsed.set(!previewCollapsed());
  }

  function toggleFocusMode() {
    focusMode.set(!focusMode());
  }

  // Another contest (or the one restored at launch): its problem's first page.
  let openedId: string | undefined;
  effect(() => {
    const t = target();
    const l = label();
    untracked(() => {
      const key = t ? `${t.target}:${t.id}` : undefined;
      if (key === openedId) return;
      openedId = key;
      coverActive.set(false);
      if (t?.target === "contest" && l && l !== contest()?.items[0]?.label) queueMicrotask(() => canvas?.jumpTo(l));
    });
  });

  // ── Keys and the command palette ───────────────────────────────────────────

  const on = (fn: () => void) => () => props.active && fn();
  keymap.bind(
    {
      "alt+1": on(() => setTab("statement")),
      "alt+2": on(() => setTab("samples")),
      "alt+3": on(() => setTab("editorial")),
      "alt+4": on(() => {
        if (target()?.target === "problem") setStudioMode("authoring");
      }),
      "mod+b": on(() => {
        if (isAuthoring()) buildConsole()?.triggerPrimaryRun();
      }),
      "mod+i": on(toggleInspector),
      "mod+s": on(save),
      "mod+e": on(() => void exportBook(book())),
      "mod+plus": on(() => canvas?.zoomBy(1.25)),
      "mod+minus": on(() => canvas?.zoomBy(1 / 1.25)),
      "mod+0": on(() => canvas?.fitWidth()),
      "mod+shift+l": on(() => void locatePreview()),
      "mod+shift+f": on(toggleFocusMode),
    },
    { when: "always" },
  );

  // Layered Escape: close assets gallery / find bar / HUD popover / notice / Inspector / Focus Mode before leaving Studio.
  function handleEscape() {
    if (assetsOpen()) {
      assetsOpen.set(false);
      return;
    }
    if (isAuthoring() && buildConsole()?.closePopovers()) return;
    if (editorActions?.closeFind()) return;
    if (canvas?.closeHud()) return;
    if (notice()) {
      notice.set(undefined);
      return;
    }
    if (inspectorOpen()) {
      toggleInspector();
      return;
    }
    if (focusMode()) {
      focusMode.set(false);
      return;
    }
    closeStudio();
  }
  keymap.bind("escape", on(handleEscape), { when: "notEditing" });

  effect(() => {
    if (!props.active) return;
    untracked(() => {
      const nextActions = {
        exportBook: (b: Book) => void exportBook(b),
        exportOjPackage: (format: OjPackageFormat) => void exportOjPackage(format),
        copyMarkdownWithSamples: () => {
          const id = problemId();
          if (!id) return;
          const isEditingStatement = tab() === "statement" && !!file();
          try {
            const md = preview.exportMarkdownWithSamples(id, isEditingStatement ? text : null);
            void Clipboard.writeText(md);
          } catch (err) {
            notice.set({ error: err instanceof Error ? err.message : String(err), context: t("ctx.copyMarkdownWithSamples") });
          }
        },
        reveal: () => void reveal(book()),
        freeze: () => {
          const t = target();
          if (t?.target === "contest") void freeze.freeze(t.id).then(() => compileNow());
        },
        unfreeze: () => target() && unfreezeFor.set(target()!.id),
        setTab,
        checkContest: () => {
          const tg = target();
          if (tg?.target !== "contest" || info().readOnly || frozen()) return;
          let ids = idsToCheck(contestCheck()?.report, false);
          if (ids.length === 0) ids = idsToCheck(contestCheck()?.report, true);
          const err = startContestCheck(tg.id, ids, "verify");
          if (err) notice.set(err);
        },
        runSanitize: () => {
          if (target()?.target !== "problem") return;
          setStudioMode("authoring");
          setTimeout(() => buildConsole()?.startRun("sanitize"), 0);
        },
        toggleInspector,
        openAssets,
        hasAssets: () => hasAssets(),
        hasPackage: () => hasPackage(),
        initPackage: initPackageScaffold,
        togglePreview,
        toggleFocusMode,
        focusMode: () => focusMode(),
        frozen: () => frozen(),
        archived: () => archived(),
        isContest: () => target()?.target === "contest",
      };
      studioActions.set(nextActions);
    });
  });
  cleanup(() => studioActions.set(undefined));

  // ── Layout ─────────────────────────────────────────────────────────────────

  const initialRatio = untracked(splitRatioSignal);
  const headerTitle = () => {
    const t = target();
    if (!t) return "";
    if (t.target === "problem") return info().problems.find((p) => p.id === t.id)?.title ?? t.id;
    return contest()?.name ?? t.id;
  };
  const headerSubtitle = () => {
    if (target()?.target !== "contest") return "";
    const it = item();
    return it ? `${it.label}. ${it.title}` : "";
  };
  const readOnly = () => !!file()?.readOnly || frozen() || archived();
  const isMarkdownTab = () => !isFullConsole() && (tab() === "statement" || tab() === "editorial");
  const tabs = computed(() =>
    TAB_DEFS.map((d) => ({
      value: d.value,
      label: t(d.labelKey),
      keys: d.keys,
    })),
  );
  const modeItems = computed(() => [
    { value: "statement" as StudioMode, label: t("studio.mode.statement") },
    { value: "authoring" as StudioMode, label: t("studio.mode.authoring") },
  ]);
  const editorPlaceholder = () =>
    tab() === "editorial"
      ? t("studio.placeholder.editorial")
      : t("studio.placeholder.statement");
  const publishLanguage = () => {
    const lang = contest()?.language;
    return lang === "english" || lang === "chinese" ? lang : undefined;
  };

  return (
    <View style={{ flex: 1 }} class="bg-background">
      <TitleBar
        leading={
          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6], flexShrink: 1, minWidth: 0, overflow: "hidden" }}>
            <IconBtn icon="arrowLeft" tooltip={tip(t("common.back"), "escape")} onPress={closeStudio} testID="studio-back" />
            <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6], flexShrink: 1, minWidth: 0, overflow: "hidden" }}>
              <Text
                style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: FONT_SIZE.body, fontWeight: FONT_WEIGHT.semibold, flexShrink: 1, minWidth: 0 }}
                class="text-foreground"
              >
                {headerTitle()}
              </Text>
              <If when={!!headerSubtitle() && (windowSize().width === 0 || windowSize().width >= 1120)}>
                <Text
                  style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: FONT_SIZE.caption, flexShrink: 2, minWidth: 0 }}
                  class="text-muted-foreground"
                >
                  {headerSubtitle()}
                </Text>
              </If>
            </View>
            <If when={target()?.target === "problem" || (target()?.target === "contest" && !coverActive() && !!item() && !item()!.missing)}>
              <View testID="studio-mode-switch" style={{ marginLeft: SPACE[4], flexShrink: 0 }}>
                <Segmented
                  items={modeItems()}
                  value={isAuthoring() ? "authoring" : "statement"}
                  onChange={setStudioMode}
                  itemWidth={locale() === "en" ? 76 : 64}
                />
              </View>
            </If>
            <If when={target()?.target === "contest" ? target()!.id : undefined}>
              {(cid) => (
                <View style={{ marginLeft: SPACE[2], flexShrink: 0 }}>
                  <ContestCheckBar
                    part="status"
                    contestId={cid()}
                    readOnly={info().readOnly || frozen() || archived()}
                    onGoProblem={goContestProblem}
                  />
                </View>
              )}
            </If>
            <If when={target()?.target === "contest" && frozen()}>
              <View style={{ flexShrink: 0 }}>
                <Lock closed size="sm" />
              </View>
            </If>
            <If when={target()?.target === "contest" && archived()}>
              <View
                testID="studio-archived-badge"
                class="bg-muted"
                style={{ height: 20, paddingLeft: SPACE[6], paddingRight: SPACE[6], borderRadius: RADIUS.xs, justifyContent: "center", flexShrink: 0 }} // token-exempt: compact status badge height
              >
                <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                  {t("contests.archived")}
                </Text>
              </View>
            </If>
          </View>
        }
        trailing={
          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[6], flexShrink: 0 }}>
            <If when={target()?.target === "contest" ? target()!.id : undefined}>
              {(cid) => (
                <ContestCheckBar
                  part="button"
                  contestId={cid()}
                  readOnly={info().readOnly || frozen() || archived()}
                  onGoProblem={goContestProblem}
                />
              )}
            </If>
            <If when={!isFullConsole()}>
              {/* In a narrow contest window the two view toggles drop their labels (tooltips keep them). */}
              <If
                when={!compactToggles()}
                else={
                  <>
                    <IconBtn
                      icon="fileText"
                      testID="toggle-preview"
                      tooltip={tip(previewHidden() ? t("studio.expandPreview") : t("studio.collapsePreview"))}
                      onPress={togglePreview}
                      style={!previewHidden() ? { backgroundColor: token("accent") } : undefined}
                    />
                    <IconBtn
                      icon="filePenLine"
                      testID="toggle-focus"
                      tooltip={tip(focusMode() ? t("studio.exitFocusMode") : t("studio.focusMode"), "mod+shift+f")}
                      onPress={toggleFocusMode}
                      style={focusMode() ? { backgroundColor: token("accent") } : undefined}
                    />
                  </>
                }
              >
                <Btn
                  label={t("studio.preview")}
                  icon="fileText"
                  variant="ghost"
                  testID="toggle-preview"
                  tooltip={tip(previewHidden() ? t("studio.expandPreview") : t("studio.collapsePreview"))}
                  onPress={togglePreview}
                  style={!previewHidden() ? { backgroundColor: token("accent") } : undefined}
                />
                <Btn
                  label={t("studio.focus")}
                  icon="filePenLine"
                  variant="ghost"
                  testID="toggle-focus"
                  tooltip={tip(focusMode() ? t("studio.exitFocusMode") : t("studio.focusMode"), "mod+shift+f")}
                  onPress={toggleFocusMode}
                  style={focusMode() ? { backgroundColor: token("accent") } : undefined}
                />
              </If>
              <ExportButton
                status={status()}
                book={book()}
                busy={exporting()}
                exportedSeq={exportedSeq()}
                canRevealOj={!!lastOjRevealPath()}
                pluginExporters={pluginExporters()}
                onExport={(b) => void exportBook(b)}
                onExportOj={(fmt) => void exportOjPackage(fmt)}
                onExportPlugin={(pid, eid) => void exportViaPlugin(pid, eid)}
                onReveal={(b) => void reveal(b)}
              />
            </If>
            <IconBtn
              icon="panelRight"
              testID="toggle-inspector"
              tooltip={tip(t("studio.properties"), "mod+i")}
              onPress={toggleInspector}
              style={inspectorOpen() ? { backgroundColor: token("accent") } : undefined}
            />
          </View>
        }
      />
      <View style={{ flex: 1, flexDirection: "row" }}>
        <If when={target()?.target === "contest" && contest()}>
          <View
            style={{
              flexDirection: "row",
              flexShrink: 0,
              display: outlineVisible() ? "flex" : "none",
              opacity: outlineShown() ? 1 : 0,
              transform: reduceMotion() ? undefined : outlineShown() ? "translateX(0px)" : "translateX(-10px)",
              ...panelTransition(),
            }}
          >
            <Outline
              detail={contest()!}
              starts={starts()}
              current={pendingLabel() ?? label()}
              coverActive={coverActive()}
              readOnly={info().readOnly}
              markFor={outlineMark}
              onSelect={(l) => selectLabelSoon(l)}
              onCover={() => {
                coverActive.set(true);
                canvas?.goPage(0);
              }}
              onReorder={compose}
              onPickProblems={() => {
                const cid = contest()?.id;
                if (cid) dialog.set({ kind: "pickProblems", contestId: cid });
              }}
            />
          </View>
        </If>
        <View style={{ flex: 1, minWidth: 0, overflow: "hidden" }}>
          <ResizablePanelGroup
            onResize={(e) => {
              if (previewHidden() || e.sizes.length !== 2) return;
              const r = ratioOf(e.sizes);
              if (r !== undefined) savePrefs({ splitRatio: r });
            }}
            handleClass="bg-border w-px group-hover:bg-primary/60 group-active:bg-primary transition-colors"
            handleThumbClass="h-6 w-[3px] rounded-full bg-muted-foreground/40 opacity-0 group-hover:opacity-100 group-active:opacity-100 transition-opacity"
          >
            <ResizablePanel size={initialRatio * 1000} minSize={320}>
              <View style={{ flex: 1 }} class="bg-card">
                {/* Single merged 38pt bar (Statement mode only): 3 crisp tabs on the left, 4 compact insert icons on the right */}
                <If when={!isFullConsole()}>
                  <View
                    style={{
                      height: ROW.table,
                      flexDirection: "row",
                      alignItems: "center",
                      justifyContent: "space-between",
                      paddingLeft: SPACE[6],
                      paddingRight: SPACE[6],
                      gap: SPACE[4],
                      borderBottomWidth: 1,
                    }}
                    class="border-border"
                  >
                    <Segmented items={tabs()} value={tab()} onChange={setTab} itemWidth={locale() === "en" ? 86 : 72} underline />
                    <If
                      when={readOnly()}
                      else={
                        <If when={isMarkdownTab() && !!file()}>
                          <View style={{ flexDirection: "row", alignItems: "center", gap: SPACE[2], flexShrink: 0 }}>
                            <If when={vimMode()}>
                              {(m) => (
                                <View
                                  testID="vim-mode-badge"
                                  class={m() === "insert" ? "bg-primary/15" : "bg-muted"}
                                  style={{
                                    height: 20, // token-exempt: compact vim mode badge height
                                    paddingLeft: SPACE[6],
                                    paddingRight: SPACE[6],
                                    marginRight: SPACE[4],
                                    borderRadius: RADIUS.xs,
                                    justifyContent: "center",
                                  }}
                                >
                                  <Text
                                    style={{ fontSize: FONT_SIZE.micro, fontWeight: FONT_WEIGHT.semibold, letterSpacing: 0.4 }}
                                    class={m() === "insert" ? "text-primary" : "text-secondary-foreground"}
                                  >
                                    {m() === "visual-line" ? "V-LINE" : m().toUpperCase()}
                                  </Text>
                                </View>
                              )}
                            </If>
                            <IconBtn
                              icon="sigma"
                              size="sm"
                              tooltip={tip(t("studio.toolbar.inlineMath"), "mod+m")}
                              onPress={() => editorActions?.inlineMath()}
                            />
                            <IconBtn
                              icon="squareCode"
                              size="sm"
                              tooltip={tip(t("studio.toolbar.displayMath"), "mod+shift+m")}
                              onPress={() => editorActions?.displayMath()}
                            />
                            <IconBtn
                              icon="table"
                              size="sm"
                              tooltip={tip(t("studio.toolbar.table"))}
                              onPress={() => editorActions?.table()}
                            />
                            <IconBtn
                              icon="image"
                              size="sm"
                              tooltip={tip(t("studio.toolbar.insertImage"))}
                              onPress={openAssets}
                            />
                          </View>
                        </If>
                      }
                    >
                      <View
                        testID="studio-readonly-badge"
                        class="bg-muted"
                        style={{ height: 22, paddingLeft: SPACE[8], paddingRight: SPACE[8], borderRadius: RADIUS.xs, justifyContent: "center" }} // token-exempt: compact readonly badge height
                      >
                        <Text style={{ fontSize: FONT_SIZE.caption }} class="text-muted-foreground">
                          {archived() ? t("contests.archived") : frozen() ? t("contests.finalized") : t("common.readOnlyWorkspace")}
                        </Text>
                      </View>
                    </If>
                  </View>
                </If>

                {/* Conflict banner in normal document flow so it never covers line 1 */}
                <If when={conflict() && isMarkdownTab()}>
                  <View
                    testID="conflict-bar"
                    style={{
                      height: ROW.table,
                      paddingLeft: SPACE[12],
                      paddingRight: SPACE[8],
                      borderBottomWidth: 1,
                      flexDirection: "row",
                      alignItems: "center",
                      gap: SPACE[6],
                    }}
                    class="bg-muted/60 border-border"
                  >
                    <Text style={{ flex: 1, fontSize: FONT_SIZE.caption }} class="text-secondary-foreground">
                      {t("studio.conflict.message")}
                    </Text>
                    <Btn label={t("studio.conflict.takeTheirs")} variant="ghost" onPress={takeTheirs} />
                    <Btn label={t("studio.conflict.keepMine")} onPress={keepMine} />
                  </View>
                </If>

                {/* Keep MarkdownEditor alive across 样例/构建 tab switches to preserve undo stack, caret, and scroll */}
                <View style={{ flex: 1, flexDirection: "row", justifyContent: "center", display: isMarkdownTab() ? "flex" : "none" }}>
                  <View
                    style={{
                      flex: 1,
                      minWidth: 0,
                      maxWidth: previewHidden() ? 760 : undefined,
                      position: "relative",
                    }}
                  >
                    <Each of={slots()} key={(slot) => slot.path}>
                      {(slot) => (
                        <View
                          // Stacked, not display:none: NSH tears down a display:none subtree's native
                          // views, and rebuilding <Editor>'s rows costs as much as a new editor.
                          style={{
                            position: "absolute",
                            top: 0,
                            right: 0,
                            bottom: 0,
                            left: 0,
                            visibility: isCurrentSlot(slot()) ? "visible" : "hidden",
                            pointerEvents: isCurrentSlot(slot()) ? "auto" : "none",
                          }}
                        >
                          <MarkdownEditor
                            active={() => props.active && isCurrentSlot(slot())}
                            hidden={!isMarkdownTab() || !isCurrentSlot(slot())}
                            previewHidden={previewHidden()}
                            typewriter={focusMode()}
                            publishLanguage={publishLanguage()}
                            onRef={(r) => {
                              const s = slot();
                              s.ref = r;
                              if (!isCurrentSlot(s)) return;
                              editor = r;
                              editorPath = s.path;
                              if (focusOnMount) r.focus();
                              focusOnMount = false;
                              queueMicrotask(applyPending);
                            }}
                            onActions={(a) => {
                              const s = slot();
                              s.actions = a;
                              if (isCurrentSlot(s)) editorActions = a;
                            }}
                            defaultValue={slot().first.text}
                            placeholder={editorPlaceholder()}
                            // A hidden editor never takes edits (they would not be saved).
                            readOnly={readOnly() || !isCurrentSlot(slot())}
                            diagnostics={isCurrentSlot(slot()) ? editorDiagnostics() : []}
                            onInsertImage={openAssets}
                            onPasteImage={pasteImageBytes}
                            onVimModeChange={(m) => {
                              const s = slot();
                              s.vim = m;
                              if (isCurrentSlot(s)) vimMode.set(m);
                            }}
                            onVimEscape={handleEscape}
                            onSlashActiveChange={(active) => {
                              if (!untracked(() => isCurrentSlot(slot()))) return;
                              slashActive = active;
                              if (active) {
                                clearTimeout(saveTimer);
                                saveTimer = undefined;
                                clearTimeout(compileTimer);
                                compileTimer = undefined;
                              } else if (dirty) {
                                clearTimeout(saveTimer);
                                saveTimer = setTimeout(save, SAVE_AFTER);
                                if (compileImmediate) {
                                  compileImmediate = false;
                                  compileNow();
                                } else {
                                  scheduleCompile();
                                }
                                scheduleStatus();
                              }
                            }}
                            onChange={(value) => {
                              if (!untracked(() => isCurrentSlot(slot()))) return;
                              if (value === text) return;
                              text = value;
                              dirty = true;
                              if (slashActive) {
                                clearTimeout(saveTimer);
                                saveTimer = undefined;
                                clearTimeout(compileTimer);
                                compileTimer = undefined;
                                return;
                              }
                              clearTimeout(saveTimer);
                              saveTimer = setTimeout(save, SAVE_AFTER);
                              if (compileImmediate) {
                                compileImmediate = false;
                                compileNow();
                              } else {
                                scheduleCompile();
                              }
                              scheduleStatus();
                            }}
                          />
                        </View>
                      )}
                    </Each>
                    <If when={!file() && target()?.target === "contest"}>
                      <EmptyState
                        icon="listPlus"
                        title={t("contests.emptyProblemsTitle")}
                        hint={t("studio.emptyContestHint")}
                        action={
                          <View style={{ flexDirection: "row", gap: SPACE[8] }}>
                            <Btn label={t("studio.goLibrary")} icon="libraryBig" onPress={() => goSpace("problems")} />
                            <Btn
                              label={t("library.newProblem")}
                              icon="plus"
                              variant="primary"
                              onPress={() => dialog.set({ kind: "newProblem", addTo: target()!.id })}
                              tooltip={tip(t("library.newProblem"), "mod+n")}
                            />
                          </View>
                        }
                      />
                    </If>
                  </View>
                </View>

                <If when={!isFullConsole() && tab() === "samples" ? problemId() : undefined}>
                  {(id) => (
                    <View style={{ flex: 1, flexDirection: "row", justifyContent: "center" }}>
                      <View
                        style={{
                          flex: 1,
                          minWidth: 0,
                          maxWidth: previewHidden() ? 760 : undefined,
                        }}
                      >
                        <Samples problemId={id()} readOnly={frozen() || archived() || info().readOnly} onSaved={compileNow} />
                      </View>
                    </View>
                  )}
                </If>

                <If when={isAuthoring() ? problemId() : undefined}>
                  {(id) => (
                    <BuildConsole
                      problemId={id()}
                      contestId={target()?.target === "contest" ? target()!.id : undefined}
                      readOnly={info().readOnly || frozen() || archived()}
                      inspectionResult={packageInspection()}
                      controller={(c) => buildConsole.set(c)}
                      onInitPackage={initPackageScaffold}
                      onCollapseInspector={() => {
                        if (inspectorOpen()) {
                          userToggledInspector = true;
                          inspectorOpen.set(false);
                        }
                      }}
                      onRefreshPackage={(opts) => {
                        const run = () =>
                          traceSpan("build", "refresh_package", () => {
                            // One batch: the package inspection reads both, and runs once.
                            batch(() => {
                              refresh();
                              packageRefreshSeq.set(packageRefreshSeq() + 1);
                            });
                            // A check changes no statement or sample: the preview stays.
                            if (!opts?.afterBuild) compileNow();
                          });
                        // After a check, its result, matrix and toast reach the screen first.
                        if (opts?.afterBuild) afterNextPaint(run);
                        else run();
                      }}
                      onSwitchToStatement={() => setStudioMode("statement")}
                    />
                  )}
                </If>

                <If when={notice()}>
                  <View
                    style={{
                      position: "absolute",
                      left: 12,
                      right: 12,
                      bottom: 12,
                      padding: SPACE[12],
                      borderRadius: RADIUS.md,
                      borderWidth: 1,
                      boxShadow: popoverShadow(),
                    }}
                    class="bg-popover border-border"
                  >
                    <ErrorDisplay
                      error={notice()}
                      locate={
                        target()?.target === "problem" &&
                        (notice()?.code === "not_verified" || notice()?.code === "missing_testdata")
                          ? [
                              {
                                label: t("packages.openAuthoring"),
                                onPress: () => {
                                  notice.set(undefined);
                                  setStudioMode("authoring");
                                },
                              },
                            ]
                          : undefined
                      }
                      onDismiss={() => notice.set(undefined)}
                    />
                  </View>
                </If>
              </View>
            </ResizablePanel>
            <ResizablePanel size={(1 - initialRatio) * 1000} minSize={320} visible={!previewHidden()}>
              <View
                style={{
                  flex: 1,
                  opacity: previewShown() ? 1 : 0,
                  transform: reduceMotion() ? undefined : previewShown() ? "translateX(0px)" : "translateX(8px)",
                  ...panelTransition(),
                }}
              >
                <PreviewCanvas
                  request={request}
                  controller={(c) => (canvas = c)}
                  onResult={(r) => {
                    // Cache-hit scroll/zoom updates (`compileMs === 0` with existing starts)
                    // do not change diagnostics or export status — skip redundant churn.
                    const isCompiled = r.compileMs > 0 || Boolean(r.error) || untracked(starts).length === 0;
                    if (!isCompiled) return;
                    batch(() => {
                      diagnostics.set(r.diagnostics);
                      if (r.pages.length > 0) starts.set(r.starts);
                    });
                    scheduleStatus();
                  }}
                  onLocate={locate}
                  onDiagnostic={finding}
                  inset={inspectorOpen() ? INSPECTOR_WIDTH : 0}
                  active={props.active && !previewHidden()}
                />
              </View>
            </ResizablePanel>
          </ResizablePanelGroup>
          <If when={target()}>
            {(t) => (
              <Inspector
                active={props.active}
                open={inspectorOpen()}
                topOffset={isAuthoring() ? 38 : 0}
                target={t().target}
                id={t().id}
                contest={contest()}
                item={isAuthoring() ? undefined : item()}
                freeze={freeze}
                assetsRefreshSeq={assetsRefreshSeq()}
                hasPackage={hasPackage()}
                hideBuildEntry={isAuthoring()}
                packageFreshness={packageInspection()?.freshnessStatus}
                onInitPackage={initPackageScaffold}
                onOpenBuild={() => setStudioMode("authoring")}
                onOpenAssets={openAssets}
                onChanged={() => {
                  compileNow();
                  scheduleStatus();
                }}
                onFinding={finding}
                onClose={toggleInspector}
              />
            )}
          </If>
          <If when={target() && hasAssets()}>
            <AssetsGalleryModal
              open={assetsOpen()}
              target={target()!.target}
              id={target()!.id}
              label={item()?.label}
              readOnly={info().readOnly || frozen() || archived()}
              hasTypstBlock={assetsHasTypstBlock()}
              refreshSeq={assetsRefreshSeq()}
              onClose={() => assetsOpen.set(false)}
              onImportImage={importImageToGallery}
              onInsertAsset={insertAssetFromGallery}
              onSetAssetWidth={setAssetWidthFromGallery}
              onRenameAsset={renameAssetFromGallery}
              onDeleteAsset={deleteAssetFromGallery}
              onCleanUnused={cleanUnusedAssetsFromGallery}
              onLocateReference={(refFile, refLine) => locate({ file: refFile, line: refLine, column: 1 })}
            />
          </If>
          <InitPackageModal
            open={initPackageModalOpen()}
            onClose={() => initPackageModalOpen.set(false)}
            onConfirm={confirmInitPackageScaffold}
          />
        </View>
      </View>
    </View>
  );
}
