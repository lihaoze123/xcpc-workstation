// The immersive publishing studio (§2.2): the outline of a contest, the editor with its
// merged single-row tab/toolbar bar (题面 | 样例 | 题解 + 4 compact insert icons), the
// paper preview, and the publishing drawer.
//
// The editor stays mounted when switching to 样例 (`display: none/flex`) so undo history,
// carets, and scroll survive. Typing debounces preview compilation (100 ms single problem,
// 200 ms contest) and auto-saves after 500 ms with SHA-256 conflict detection. Ctrl+click
// on the preview locates Markdown source; Mod+Shift+L locates the preview from the caret.

import { batch, cleanup, computed, effect, keymap, signal, Text, untracked, View } from "nsh";
import { ResizablePanel, ResizablePanelGroup, type EditorDiagnostic, type EditorRef } from "nsh/base";
import type {
  ExportBookStatusJs,
  PreviewDiagnostic,
  PreviewRequest,
  PreviewStart,
  SourceLocationJs,
  WorkspaceFile,
} from "xcpc-workstation/native";
import { createFreeze } from "../Contest";
import { ErrorDisplay } from "../ErrorDisplay";
import { Btn, IconBtn, Lock, Segmented, tip } from "../kit";
import { EmptyState } from "../layout";
import { minimalEdit, ratioOf, type Book } from "../logic";
import {
  book as storeBook,
  closeStudio,
  dialog,
  errorOf,
  goSpace,
  info,
  loadPrefs,
  onExternalChange,
  openStudio,
  preview,
  refresh,
  revision,
  savePrefs,
  splitRatioSignal,
  studio,
  studioActions,
  unfreezeFor,
  workspaceVersion,
  type StudioTarget,
  type UiError,
} from "../store";
import { popoverShadow, token } from "../theme";
import { TitleBar } from "../TitleBar";
import { ExportButton } from "./ExportButton";
import { Inspector, INSPECTOR_WIDTH } from "./Inspector";
import { MarkdownEditor, type MarkdownEditorActions } from "./MarkdownEditor";
import { Outline } from "./Outline";
import { PreviewCanvas, type CanvasController } from "./PreviewCanvas";
import { Samples } from "./Samples";

type Tab = "statement" | "samples" | "editorial";
const SAVE_AFTER = 500;
const TABS: { value: Tab; label: string; keys: string }[] = [
  { value: "statement", label: "题面", keys: "alt+1" },
  { value: "samples", label: "样例", keys: "alt+2" },
  { value: "editorial", label: "题解", keys: "alt+3" },
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
  const tab = signal<Tab>(storeBook() === "editorial" ? "editorial" : "statement");
  const book = computed<Book>(() => (tab() === "editorial" ? "editorial" : "statements"));
  const inspectorOpen = signal(loadPrefs().propsOpen ?? false);
  const coverActive = signal(false);
  const starts = signal<PreviewStart[]>([]);
  const diagnostics = signal<PreviewDiagnostic[]>([]);
  const status = signal<ExportBookStatusJs | undefined>(undefined);
  const exporting = signal<Book | undefined>(undefined);
  const exportedSeq = signal(0);
  const notice = signal<UiError | undefined>(undefined);
  const freeze = createFreeze();
  let canvas: CanvasController | undefined;
  let editorActions: MarkdownEditorActions | undefined;

  const contest = computed(() => {
    void revision();
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
  const frozen = computed(() => !!contest()?.frozen);
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
      if (owner !== lastFreezeOwner) {
        lastFreezeOwner = owner;
        freeze.reset();
      }
      if (t?.target !== "contest") coverActive.set(false);
    });
  });

  // ── The editor's file ──────────────────────────────────────────────────────
  // Its text is kept outside signals: typing must not re-render the studio.
  const file = signal<WorkspaceFile | undefined>(undefined);
  const conflict = signal<{ text: string; hash: string } | undefined>(undefined);
  let text = "";
  let dirty = false;
  let saveTimer: number | undefined;
  let compileTimer: number | undefined;
  let editor: EditorRef | undefined;
  let editorPath = "";
  let pendingCursor: { path: string; line: number; column: number } | undefined;
  let compileImmediate = false;

  function scheduleCompile() {
    clearTimeout(compileTimer);
    const delay = untracked(target)?.target === "contest" ? 200 : 100;
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

  function save() {
    clearTimeout(saveTimer);
    saveTimer = undefined;
    const f = untracked(file);
    if (!dirty || !f || f.readOnly || untracked(conflict)) return;
    const res = preview.save(f.path, text, f.hash);
    if (res.ok) {
      dirty = false;
      file.set({ ...f, hash: res.hash });
      notice.set(undefined);
      scheduleStatus();
    } else if (res.conflict) {
      const disk = preview.read(f.path);
      conflict.set({ text: disk.text, hash: disk.hash });
    } else {
      notice.set(errorOf(res, "保存"));
    }
  }

  effect(() => {
    const next = path();
    untracked(() => {
      if (next === file()?.path) return;
      clearTimeout(compileTimer);
      compileTimer = undefined;
      save();
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
          text = f.text;
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
      notice.set(errorOf(res, "保存"));
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
  function scheduleStatus(now = false) {
    clearTimeout(statusTimer);
    const run = () => {
      const req = untracked(request);
      status.set(req ? preview.exportStatus(req) : undefined);
    };
    if (now) run();
    else statusTimer = setTimeout(run, 300);
  }
  cleanup(() => {
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
      notice.set({ error: out.error, context: b === "editorial" ? "导出题解册" : "导出题面册" });
    } else {
      notice.set(undefined);
      exportedSeq.set(exportedSeq() + 1);
    }
    scheduleStatus(true);
  }

  async function reveal(b: Book) {
    const s = status()?.[b];
    if (!s?.exists) return;
    const res = await preview.revealInFolder(s.absPath);
    if (!res.ok) notice.set(errorOf(res, "在文件夹中显示"));
  }

  // ── Navigation ─────────────────────────────────────────────────────────────

  function setTab(next: Tab) {
    if (next === tab()) return;
    tab.set(next);
    const b: Book = next === "editorial" ? "editorial" : "statements";
    if (storeBook() !== b) {
      storeBook.set(b);
      savePrefs({ lastBook: b });
    }
  }

  function selectLabel(next: string, jump = true) {
    const t = target();
    if (!t || t.target !== "contest") return;
    coverActive.set(false);
    if (next !== label()) openStudio({ ...t, label: next });
    if (jump) canvas?.jumpTo(next);
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
    if (!f || untracked(tab) === "samples") return;
    const line = (await editorActions?.currentLine()) ?? 1;
    const loc = preview.locateSource(f.path, line);
    if (loc) canvas?.scrollToLocation(loc.page, loc.yPt, true);
  }

  function finding(d: PreviewDiagnostic) {
    if (d.file && d.line != null) locate({ file: d.file, line: d.line, column: d.column ?? 1 });
    else if (d.page != null) canvas?.goPage(d.page);
  }

  /** Opens the system image picker, copies the chosen image into `assets/`, inserts Markdown, and compiles immediately. */
  async function pickAndInsertImage() {
    const t = untracked(target);
    if (!t || untracked(readOnly) || untracked(tab) === "samples") return;
    try {
      const rel = await preview.pickAndImportImage(
        t.target,
        t.id,
        t.target === "contest" ? (untracked(label) ?? null) : null,
      );
      if (!rel) return;
      notice.set(undefined);
      compileImmediate = true;
      await editorActions?.insertImage(rel);
    } catch (err) {
      compileImmediate = false;
      notice.set({ error: err instanceof Error ? err.message : String(err), context: "导入图片" });
    }
  }

  /** Persists clipboard image bytes into `assets/` and returns the relative path for editor insertion. */
  function pasteImageBytes(ext: string, bytes: Uint8Array): string | null {
    const t = untracked(target);
    if (!t || untracked(readOnly)) return null;
    try {
      const rel = preview.importImageBytes(
        t.target,
        t.id,
        t.target === "contest" ? (untracked(label) ?? null) : null,
        ext,
        bytes,
      );
      notice.set(undefined);
      compileImmediate = true;
      return rel;
    } catch (err) {
      compileImmediate = false;
      notice.set({ error: err instanceof Error ? err.message : String(err), context: "粘贴图片" });
      return null;
    }
  }

  function compose(ids: string[]): boolean {
    const t = target();
    if (!t) return false;
    const current = item()?.id;
    const res = preview.composeContest(t.id, ids);
    if (!res.ok) {
      notice.set(errorOf(res, "调整题序"));
      return false;
    }
    const at = current ? ids.indexOf(current) : -1;
    if (at >= 0) openStudio({ ...t, label: String.fromCharCode(65 + at) });
    refresh();
    compileNow();
    return true;
  }

  function toggleInspector() {
    const next = !inspectorOpen();
    inspectorOpen.set(next);
    savePrefs({ propsOpen: next });
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
      "mod+i": on(toggleInspector),
      "mod+s": on(save),
      "mod+e": on(() => void exportBook(book())),
      "mod+plus": on(() => canvas?.zoomBy(1.25)),
      "mod+minus": on(() => canvas?.zoomBy(1 / 1.25)),
      "mod+0": on(() => canvas?.fitWidth()),
      "mod+shift+l": on(() => void locatePreview()),
    },
    { when: "always" },
  );

  // Layered Escape: close find bar / HUD popover / notice / Inspector before leaving Studio.
  keymap.bind(
    "escape",
    on(() => {
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
      closeStudio();
    }),
    { when: "notEditing" },
  );

  effect(() => {
    if (!props.active) return;
    untracked(() =>
      studioActions.set({
        exportBook: (b) => void exportBook(b),
        reveal: () => void reveal(book()),
        freeze: () => {
          const t = target();
          if (t?.target === "contest") void freeze.freeze(t.id).then(() => compileNow());
        },
        unfreeze: () => target() && unfreezeFor.set(target()!.id),
        setTab,
        toggleInspector,
        frozen: () => frozen(),
        isContest: () => target()?.target === "contest",
      }),
    );
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
  const readOnly = () => !!file()?.readOnly || frozen();
  const editorPlaceholder = () =>
    tab() === "editorial"
      ? "在此编写题解（支持 Markdown 与 LaTeX 公式）…"
      : "在此编写题面（使用 ## 标题划分章节）…";

  return (
    <View style={{ flex: 1 }} class="bg-background">
      <TitleBar
        leading={
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6, minWidth: 0 }}>
            <IconBtn icon="arrowLeft" tooltip={tip("返回", "escape")} onPress={closeStudio} testID="studio-back" />
            <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: 13.5, fontWeight: 500 }}>
              {headerTitle()}
            </Text>
            <If when={!!headerSubtitle()}>
              <Text style={{ numberOfLines: 1, textOverflow: "ellipsis", fontSize: 12.5, marginLeft: 4 }} class="text-muted-foreground">
                {headerSubtitle()}
              </Text>
            </If>
            <If when={target()?.target === "contest" && frozen()}>
              <Lock closed size={13} />
            </If>
          </View>
        }
        trailing={
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <ExportButton
              status={status()}
              book={book()}
              busy={exporting()}
              exportedSeq={exportedSeq()}
              onExport={(b) => void exportBook(b)}
              onReveal={(b) => void reveal(b)}
            />
            <IconBtn
              icon="panelRight"
              testID="toggle-inspector"
              tooltip={tip("属性", "mod+i")}
              onPress={toggleInspector}
              style={inspectorOpen() ? { backgroundColor: token("accent") } : undefined}
            />
          </View>
        }
      />
      <View style={{ flex: 1, flexDirection: "row" }}>
        <If when={target()?.target === "contest" && contest()}>
          <Outline
            detail={contest()!}
            starts={starts()}
            current={label()}
            coverActive={coverActive()}
            onSelect={(l) => selectLabel(l)}
            onCover={() => {
              coverActive.set(true);
              canvas?.goPage(0);
            }}
            onReorder={compose}
          />
        </If>
        <View style={{ flex: 1, minWidth: 0, overflow: "hidden" }}>
          <ResizablePanelGroup
            onResize={(e) => {
              const r = ratioOf(e.sizes);
              if (r !== undefined) savePrefs({ splitRatio: r });
            }}
            handleClass="bg-border w-px group-hover:bg-primary/60 group-active:bg-primary transition-colors"
            handleThumbClass="h-6 w-[3px] rounded-full bg-muted-foreground/40 opacity-0 group-hover:opacity-100 group-active:opacity-100 transition-opacity"
          >
            <ResizablePanel size={initialRatio * 1000} minSize={320}>
              <View style={{ flex: 1 }} class="bg-card">
                {/* Single merged 38pt bar: tabs on the left, 4 high-frequency insert icons or read-only badge on the right */}
                <View
                  style={{
                    height: 38,
                    flexDirection: "row",
                    alignItems: "center",
                    justifyContent: "space-between",
                    paddingLeft: 8,
                    paddingRight: 8,
                    borderBottomWidth: 1,
                  }}
                  class="border-border"
                >
                  <Segmented items={TABS} value={tab()} onChange={setTab} itemWidth={52} underline />
                  <If when={tab() !== "samples" && !!file()}>
                    <If
                      when={!readOnly()}
                      else={
                        <View
                          class="bg-muted"
                          style={{ height: 22, paddingLeft: 7, paddingRight: 7, borderRadius: 5, justifyContent: "center" }}
                        >
                          <Text style={{ fontSize: 11.5 }} class="text-muted-foreground">
                            只读
                          </Text>
                        </View>
                      }
                    >
                      <View style={{ flexDirection: "row", alignItems: "center", gap: 2 }}>
                        <IconBtn
                          icon="sigma"
                          size={26}
                          iconSize={14.5}
                          tooltip={tip("行内公式", "mod+m")}
                          onPress={() => editorActions?.inlineMath()}
                        />
                        <IconBtn
                          icon="squareCode"
                          size={26}
                          iconSize={14.5}
                          tooltip={tip("公式块", "mod+shift+m")}
                          onPress={() => editorActions?.displayMath()}
                        />
                        <IconBtn
                          icon="table"
                          size={26}
                          iconSize={14.5}
                          tooltip={tip("表格")}
                          onPress={() => editorActions?.table()}
                        />
                        <IconBtn
                          icon="image"
                          size={26}
                          iconSize={14.5}
                          tooltip={tip("导入图片")}
                          onPress={() => void pickAndInsertImage()}
                        />
                      </View>
                    </If>
                  </If>
                </View>

                {/* Conflict banner in normal document flow so it never covers line 1 */}
                <If when={conflict() && tab() !== "samples"}>
                  <View
                    testID="conflict-bar"
                    style={{
                      height: 38,
                      paddingLeft: 12,
                      paddingRight: 8,
                      borderBottomWidth: 1,
                      flexDirection: "row",
                      alignItems: "center",
                      gap: 6,
                    }}
                    class="bg-muted/60 border-border"
                  >
                    <Text style={{ flex: 1, fontSize: 12.5 }} class="text-secondary-foreground">
                      文件已在别处修改
                    </Text>
                    <Btn label="载入外部版本" variant="ghost" onPress={takeTheirs} />
                    <Btn label="保留我的版本" onPress={keepMine} />
                  </View>
                </If>

                {/* Keep MarkdownEditor alive across 样例 tab switches to preserve undo stack, caret, and scroll */}
                <View style={{ flex: 1, display: tab() === "samples" ? "none" : "flex" }}>
                  <Each of={file() ? [file()!] : []} key={(f) => f.path}>
                    {(f) => (
                      <MarkdownEditor
                        active={() => props.active}
                        hidden={tab() === "samples"}
                        onRef={(r) => {
                          editor = r;
                          editorPath = f().path;
                          queueMicrotask(applyPending);
                        }}
                        onActions={(a) => {
                          editorActions = a;
                        }}
                        defaultValue={f().text}
                        placeholder={editorPlaceholder()}
                        readOnly={readOnly()}
                        diagnostics={editorDiagnostics()}
                        onPasteImage={pasteImageBytes}
                        onChange={(value) => {
                          if (value === text) return;
                          text = value;
                          dirty = true;
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
                    )}
                  </Each>
                  <If when={!file() && target()?.target === "contest"}>
                    <EmptyState
                      icon="listPlus"
                      title="这场比赛还没有题目"
                      hint="从题库挑题加进来，或者直接新建一道"
                      action={
                        <View style={{ flexDirection: "row", gap: 8 }}>
                          <Btn label="去题库" icon="libraryBig" onPress={() => goSpace("problems")} />
                          <Btn
                            label="新建题目"
                            icon="plus"
                            variant="primary"
                            onPress={() => dialog.set({ kind: "newProblem", addTo: target()!.id })}
                            tooltip={tip("新建题目", "mod+n")}
                          />
                        </View>
                      }
                    />
                  </If>
                </View>

                <If when={tab() === "samples" ? problemId() : undefined}>
                  {(id) => <Samples problemId={id()} readOnly={frozen() || info().readOnly} onSaved={compileNow} />}
                </If>

                <If when={notice()}>
                  <View
                    style={{
                      position: "absolute",
                      left: 12,
                      right: 12,
                      bottom: 12,
                      padding: 10,
                      borderRadius: 8,
                      borderWidth: 1,
                      boxShadow: popoverShadow(),
                    }}
                    class="bg-popover border-border"
                  >
                    <ErrorDisplay error={notice()} onDismiss={() => notice.set(undefined)} />
                  </View>
                </If>
              </View>
            </ResizablePanel>
            <ResizablePanel size={(1 - initialRatio) * 1000} minSize={320}>
              <PreviewCanvas
                request={request}
                controller={(c) => (canvas = c)}
                onResult={(r) => {
                  batch(() => {
                    diagnostics.set(r.diagnostics);
                    if (r.pages.length > 0) starts.set(r.starts);
                  });
                  scheduleStatus();
                }}
                onLocate={locate}
                onDiagnostic={finding}
                inset={inspectorOpen() ? INSPECTOR_WIDTH : 0}
              />
            </ResizablePanel>
          </ResizablePanelGroup>
          <If when={target()}>
            {(t) => (
              <Inspector
                open={inspectorOpen()}
                target={t().target}
                id={t().id}
                contest={contest()}
                item={item()}
                freeze={freeze}
                onChanged={() => {
                  compileNow();
                  scheduleStatus();
                }}
                onFinding={finding}
                onClose={toggleInspector}
              />
            )}
          </If>
        </View>
      </View>
    </View>
  );
}
