import { traceIpcCall } from "./trace";
// The app's state shared by the shell and the views: the native bridge, the workspace,
// the preferences, where the user is (a space, or the studio on an object), the watch
// loop on external changes, and the studio's actions for the command palette.

import process from "process";
import { batch, computed, signal, Storage, untracked, type Signal } from "nsh";
import {
  Preview,
  type ActionResult,
  type AppPrefsJs,
  type BatchExportRequestJs,
  type BatchTaskSnapshotJs,
  type PackageWarningJs,
  type PluginsOverviewJs,
  type ToolchainStatusJs,
  type WorkspaceInfo,
} from "xcpc-workstation/native";
import type { Book, LibraryGroupBy, Space } from "./logic";
import { clampRatio, isLibraryGroupBy } from "./logic";
import {
  languagePref,
  locale,
  normalizeLanguagePref,
  normalizeLocale,
  systemLocale,
  t,
  type LanguagePref,
} from "./i18n";
import {
  appearanceMode,
  applyAppearancePrefs,
  applyThemeOverrides,
  DEFAULT_EDITOR_FONT_SIZE,
  DEFAULT_EDITOR_LIGATURES,
  DEFAULT_EDITOR_LINE_HEIGHT,
  editorFontSize,
  editorLigatures,
  editorLineHeight,
  editorVimMode,
  setAppearanceMode,
  setEditorFontSize,
  setEditorLigatures,
  setEditorLineHeight,
  setEditorVimMode,
  type AppearanceMode,
  type AppearancePrefs,
} from "./theme";

const UNTRACED_IPC_METHODS = new Set([
  "o11yEnvEnabled",
  "o11yPollDelta",
  "o11yClear",
  "traceStatus",
  "traceStart",
  "traceStop",
  "traceExportDialog",
]);

/** Wraps a native bridge object so every sync/async method call auto-records `cat: "ipc"` spans (and catches >50ms slow calls in always-on mode). */
export function createTracedPreview<T extends object>(target: T): T {
  return new Proxy(target, {
    get(obj, prop, receiver) {
      const value = Reflect.get(obj, prop, receiver);
      if (typeof value !== "function" || typeof prop !== "string") return value;
      if (UNTRACED_IPC_METHODS.has(prop)) {
        return (...args: unknown[]) => value.apply(obj, args);
      }
      return (...args: unknown[]) => traceIpcCall(prop, () => value.apply(obj, args));
    },
  });
}

export const preview: Preview = createTracedPreview(new Preview(process.dataDir));

// ── The C++ compiler, detected once and on 重新检测 (authoring-ui-refactor §6.1) ──────
// Status slots say 「没有找到 C++ 编译器」 before any check is started.

export const toolchainState = signal<ToolchainStatusJs | undefined>(undefined);

/** Detects the compilers again and remembers the result. */
export function refreshToolchain(): ToolchainStatusJs {
  const next = preview.toolchainStatus();
  toolchainState.set(next);
  return next;
}

let toolchainLoading = false;

/** Lazily detects the host C++ toolchain once (off-startup so cold launch never stalls ~112ms). */
export function ensureToolchainLoaded(): void {
  if (toolchainState() !== undefined || toolchainLoading) return;
  toolchainLoading = true;
  if (typeof preview.toolchainStatusAsync === "function") {
    void preview
      .toolchainStatusAsync()
      .then((next) => {
        toolchainState.set(next);
      })
      .finally(() => {
        toolchainLoading = false;
      });
    return;
  }
  try {
    refreshToolchain();
  } finally {
    toolchainLoading = false;
  }
}

/** Whether no C++ compiler is usable (unknown until the first detection). */
export function compilerMissing(): boolean {
  const s = toolchainState();
  return !!s && !s.active;
}

// Silent startup OJ auto-sync (runs at most once per session on a background thread; silent on success).
setTimeout(() => {
  try {
    if (typeof preview.startupSyncIfEnabled === "function") {
      void preview.startupSyncIfEnabled().then((synced) => {
        if (synced) refresh();
      });
    }
  } catch {
    // Ignore in headless tests or when no data directory is configured.
  }
}, 200);

/** What the studio shows: a problem, or a contest (and the problem of it in the editor). */
export type StudioTarget = {
  target: "problem" | "contest";
  id: string;
  label?: string;
  initialTab?: "statement" | "samples" | "editorial" | "build";
};

/** An error the views show with `ErrorDisplay`: the Core's code and raw text, and where it came from. */
export type UiError = { code?: string | null; error?: string | null; context?: string; referencing?: string[] };

export function errorOf(res: ActionResult, context?: string): UiError | undefined {
  if (res.ok) return undefined;
  return { code: res.code, error: res.error, context, referencing: res.referencingContests };
}

// ── Workspace ────────────────────────────────────────────────────────────────

export const info = signal<WorkspaceInfo>(preview.workspace());
/** Reloads and applies global `<dataDir>/theme.toml` overrides. */
export function reloadThemeOverrides() {
  try {
    applyThemeOverrides(preview.loadThemeOverrides());
  } catch {
    // Headless test environments without theme file stay on default palette.
  }
}

reloadThemeOverrides();
/** Bumped whenever the workspace's content may have changed (ours or another's). */
export const revision = signal(0);
/**
 * Returns a reactive revision getter that latches `revision()` while `active()` is false
 * and advances immediately when `active()` becomes true. Hidden mounted views subscribe to
 * this instead of `revision()` directly so background spaces do zero work during Studio edits.
 */
export function activeRevision(active: () => boolean): () => number {
  return computed<number>((prev) => {
    if (!active()) return prev ?? untracked(revision);
    return revision();
  });
}
/** Bumped whenever the open workspace switches to a different root, so views clear stale filters/errors. */
export const workspaceVersion = signal(0);
/** Why the last attempt to open a workspace failed, for the views that opened it. */
export const openError = signal<UiError | undefined>(undefined);
/** Whether a Polygon ZIP import is currently in flight. */
export const importingPolygon = signal(false);
/** Non-fatal warnings from the most recent Polygon import, shown cleanly until dismissed. */
export const importWarnings = signal<{ title: string; warnings: PackageWarningJs[] } | undefined>(undefined);

export function hasWorkspace(): boolean {
  return info().root !== "";
}

/** Reads the workspace again after a change and tells the views. */
export function refresh() {
  const next = preview.workspace();
  batch(() => {
    info.set(next);
    if (!next.trainingEnabled && space() === "training") {
      space.set("problems");
    }
    if (!next.snippetsEnabled && space() === "snippets") {
      space.set("problems");
    }
    revision.set(revision() + 1);
  });
}

// ── Preferences ──────────────────────────────────────────────────────────────

let prefs: AppPrefsJs = preview.loadPrefs();
let prefsDirty = false;
let prefsTimer: number | undefined;
let visitTimer: number | undefined;

// Initialize i18n from OS locale and persisted language preference.
try {
  systemLocale.set(normalizeLocale(preview.systemLocale()));
} catch {
  // Headless tests stay on default locale.
}
languagePref.set(normalizeLanguagePref(prefs.language));

export function updateLanguagePref(pref: LanguagePref): void {
  const next = normalizeLanguagePref(pref);
  languagePref.set(next);
  savePrefs({ language: next });
}

export function loadPrefs(): AppPrefsJs {
  return prefs;
}

/** Updates the in-memory `prefs` cache for fields already persisted by dedicated native setters. */
export function syncPrefsCache(patch: Partial<AppPrefsJs>) {
  prefs = { ...prefs, ...patch };
}

export let splitRatio = clampRatio(prefs.splitRatio);
export const splitRatioSignal = signal(splitRatio);

/** Persisted library left group sidebar open preference (default true). */
export const librarySidebarOpen = signal<boolean>(prefs.librarySidebarOpen ?? true);

export function setLibrarySidebarOpen(open: boolean) {
  librarySidebarOpen.set(open);
  savePrefs({ librarySidebarOpen: open });
}

export function toggleLibrarySidebar() {
  setLibrarySidebarOpen(!librarySidebarOpen());
}

/** Persisted library dynamic grouping mode (default "none"). */
export const libraryGroupBy = signal<LibraryGroupBy>(
  isLibraryGroupBy(prefs.libraryGroupBy) ? prefs.libraryGroupBy : "none",
);

export function setLibraryGroupBy(mode: LibraryGroupBy) {
  libraryGroupBy.set(mode);
  savePrefs({ libraryGroupBy: mode });
}

/** Flushes any pending debounced preferences write to disk immediately. */
export function flushPrefs() {
  if (prefsTimer !== undefined) {
    clearTimeout(prefsTimer);
    prefsTimer = undefined;
  }
  if (!prefsDirty) return;
  prefsDirty = false;
  preview.savePrefs(prefs);
}

// ── Unsaved-changes quit guard registry (NSH 0.7 `quitRequest`) ──────────────

const quitFlusherSet = new Set<() => boolean>();

/**
 * Registers a synchronous flush handler invoked on `Application.on("quitRequest")`.
 * Returning `false` indicates an unresolved conflict and cancels the quit so the
 * user never loses unsaved work.
 */
export function registerQuitFlusher(fn: () => boolean): () => void {
  quitFlusherSet.add(fn);
  return () => quitFlusherSet.delete(fn);
}

/** Runs `flushPrefs()` and every registered studio flusher; returns `true` when safe to quit. */
export function flushAllBeforeQuit(): boolean {
  flushPrefs();
  for (const fn of quitFlusherSet) {
    try {
      if (fn() === false) return false;
    } catch {
      // Best-effort flush; continue remaining flushers.
    }
  }
  return true;
}

/** Merges `patch` into the in-memory preferences immediately and debounces disk persistence
 *  so high-frequency gestures (splitter drags, view switches, drawer toggles) never block the UI thread. */
export function savePrefs(patch: Partial<AppPrefsJs>) {
  let changed = false;
  for (const key of Object.keys(patch) as (keyof AppPrefsJs)[]) {
    if (patch[key] !== prefs[key]) {
      changed = true;
      break;
    }
  }
  if (!changed) return;
  prefs = { ...prefs, ...patch };
  if (patch.splitRatio !== undefined) {
    const next = clampRatio(patch.splitRatio);
    if (Math.abs(next - splitRatio) > 0.002) {
      splitRatio = next;
      splitRatioSignal.set(splitRatio);
    }
  }
  prefsDirty = true;
  if (prefsTimer !== undefined) clearTimeout(prefsTimer);
  prefsTimer = setTimeout(() => {
    prefsTimer = undefined;
    if (!prefsDirty) return;
    prefsDirty = false;
    preview.savePrefs(prefs);
  }, 600);
}

// ── Where the user is ────────────────────────────────────────────────────────

function initialSpace(): Space {
  const s = prefs.lastSpace;
  if (s === "training") return info().trainingEnabled ? "training" : "problems";
  if (s === "snippets") return info().snippetsEnabled ? "snippets" : "problems";
  return s === "problems" || s === "contests" || s === "tools" || s === "settings" ? s : "problems";
}

export const space = signal<Space>(initialSpace());
export type SettingsSection = "workspace" | "layout" | "toolchain" | "plugins" | "appearance" | "shortcuts" | "about";
export const settingsSection = signal<SettingsSection>("workspace");
export const brandReplayOpen = signal(false);
export const newWorkspaceModalOpen = signal(false);

/** Bumped whenever plugins are enabled, disabled, installed, or reloaded so Tools/Palette/IO menus refresh. */
export const pluginsRevision = signal(0);
/** Per-plugin revision counters so modifying/installing an unrelated plugin never reloads an open PluginWebView. */
const pluginRevisions = new Map<string, Signal<number>>();

function getPluginRevisionSignal(pluginId: string): Signal<number> {
  let sig = pluginRevisions.get(pluginId);
  if (!sig) {
    sig = signal(0);
    pluginRevisions.set(pluginId, sig);
  }
  return sig;
}

/** Reactive per-plugin revision number (only increments when `pluginId` itself changes or on full rescan). */
export function pluginRevisionFor(pluginId: string): number {
  return getPluginRevisionSignal(pluginId)();
}

/** Bumps revision counters for specific plugin IDs (or all tracked plugins when `changedIds` is `undefined`). */
export function bumpPluginRevisions(changedIds?: readonly string[] | null) {
  if (changedIds === undefined || changedIds === null) {
    batch(() => {
      for (const sig of pluginRevisions.values()) {
        sig.set(sig() + 1);
      }
    });
    return;
  }
  if (changedIds.length === 0) return;
  batch(() => {
    for (const id of changedIds) {
      if (!id) continue;
      const sig = getPluginRevisionSignal(id);
      sig.set(sig() + 1);
    }
  });
}

/** Preloaded/cached plugins overview so Settings -> Plugins renders immediately with zero blank frame. */
export const cachedPluginsOverview = signal<PluginsOverviewJs | undefined>(undefined);
/** Shared reactive last-synced timestamp (ISO string) kept in sync between TrainingView and Settings -> Plugins. */
export const globalLastSyncedAt = signal<string | null>(null);

export function setGlobalLastSyncedAt(ts: string | null | undefined) {
  if (ts !== undefined) {
    globalLastSyncedAt.set(ts);
  }
}

export function refreshPluginsOverview(reload = false): PluginsOverviewJs {
  const next = reload ? preview.reloadPlugins() : preview.getPluginsOverview();
  cachedPluginsOverview.set(next);
  if (next?.lastSyncedAt !== undefined) {
    globalLastSyncedAt.set(next.lastSyncedAt ?? null);
  }
  return next;
}

export function notifyPluginsChanged(changedPluginIds?: readonly string[] | null) {
  try {
    if (typeof preview.getPluginsOverview === "function") {
      const next = preview.getPluginsOverview();
      cachedPluginsOverview.set(next);
      if (next?.lastSyncedAt !== undefined) {
        globalLastSyncedAt.set(next.lastSyncedAt ?? null);
      }
    }
  } catch {
    // Ignore in headless tests without a native plugin manager.
  }
  batch(() => {
    pluginsRevision.set(pluginsRevision() + 1);
    bumpPluginRevisions(changedPluginIds);
  });
}

setTimeout(() => {
  try {
    if (cachedPluginsOverview() === undefined && typeof preview.getPluginsOverview === "function") {
      cachedPluginsOverview.set(preview.getPluginsOverview());
    }
  } catch {
    // Ignore in headless tests.
  }
}, 0);

/** Currently requested plugin / headless tool / payload for the Tools space. */
export const activeToolPluginId = signal<string | undefined>(undefined);
export const activeToolId = signal<string | undefined>(undefined);
export const activeToolPayload = signal<unknown>(undefined);
export const activeToolMode = signal<"ui" | "headless" | undefined>(undefined);

/** Navigates to the Tools space, optionally selecting a plugin, headless tool, and initial payload. */
export function openToolPlugin(
  pluginId: string,
  opts?: { toolId?: string; payload?: unknown; mode?: "ui" | "headless" },
) {
  batch(() => {
    studio.set(undefined);
    activeToolPluginId.set(pluginId);
    activeToolId.set(opts?.toolId);
    if (opts && "payload" in opts) {
      activeToolPayload.set(opts.payload);
    }
    activeToolMode.set(opts?.mode);
    space.set("tools");
  });
  savePrefs({ lastSpace: "tools", lastTarget: null, lastId: null, lastLabel: null });
}

/** Opens the "New Workspace" modal so the user can choose between template and empty workspace. */
export function requestNewWorkspace() {
  newWorkspaceModalOpen.set(true);
}
export const studio = signal<StudioTarget | undefined>(restoreStudio());
export const book = signal<Book>(prefs.lastBook === "editorial" ? "editorial" : "statements");
export const paletteOpen = signal(false);

function restoreStudio(): StudioTarget | undefined {
  const w = info();
  if (w.root === "" || !prefs.lastId) return undefined;
  if (prefs.lastTarget === "problem" && w.problems.some((p) => p.id === prefs.lastId)) {
    return { target: "problem", id: prefs.lastId };
  }
  if (prefs.lastTarget === "contest") {
    const c = w.contests.find((x) => x.id === prefs.lastId);
    if (c) return { target: "contest", id: c.id, label: c.items.some((i) => i.label === prefs.lastLabel) ? prefs.lastLabel! : undefined };
  }
  return undefined;
}

export function goSpace(next: Space) {
  let target: Space = next;
  if (target === "training" && !info().trainingEnabled) target = "problems";
  if (target === "snippets" && !info().snippetsEnabled) target = "problems";
  batch(() => {
    studio.set(undefined);
    activeSnippetStudio.set(undefined);
    activeTrdStudio.set(undefined);
    space.set(target);
  });
  savePrefs({ lastSpace: target, lastTarget: null, lastId: null, lastLabel: null });
}

// ── Phase 5: Snippets & Living TRD navigation state ──────────────────────────

export const snippetsSubTab = signal<"library" | "trds">("library");
export const selectedSnippetSlug = signal<string | undefined>(undefined);
export const selectedTrdId = signal<string | undefined>(undefined);
export const activeSnippetStudio = signal<string | undefined>(undefined);
export const activeTrdStudio = signal<string | undefined>(undefined);
export const bundlerModalOpen = signal(false);
export const newSnippetSeed = signal<{ code?: string; sourceProblem?: string; title?: string } | undefined>(undefined);
export const newTrdModalOpen = signal(false);
export const snippetsRevision = signal(0);

export function bumpSnippetsRevision() {
  snippetsRevision.set(snippetsRevision() + 1);
}

export function openNewSnippetModal(seed?: { code?: string; sourceProblem?: string; title?: string }) {
  newSnippetSeed.set(seed ?? {});
}

export function openSnippetStudio(slug: string) {
  batch(() => {
    studio.set(undefined);
    activeTrdStudio.set(undefined);
    selectedSnippetSlug.set(slug);
    activeSnippetStudio.set(slug);
  });
}

export function closeSnippetStudio() {
  activeSnippetStudio.set(undefined);
}

export function openTrdStudio(id: string) {
  batch(() => {
    studio.set(undefined);
    activeSnippetStudio.set(undefined);
    selectedTrdId.set(id);
    activeTrdStudio.set(id);
  });
}

export function closeTrdStudio() {
  activeTrdStudio.set(undefined);
}

export function jumpToProblem(problemId: string) {
  batch(() => {
    studio.set(undefined);
    activeSnippetStudio.set(undefined);
    activeTrdStudio.set(undefined);
    selectedProblem.set(problemId);
    space.set("problems");
  });
  savePrefs({ lastSpace: "problems", lastTarget: null, lastId: null, lastLabel: null });
}

/** Controls the collapsible "Advanced" block in Settings -> Toolchain (e.g. expanded when jumping from offline Library Checker toast). */
export const toolchainAdvancedOpen = signal(false);

/** Navigates directly to a specific settings section. */
export function goSettings(section: SettingsSection, opts?: { expandAdvanced?: boolean }) {
  if (opts?.expandAdvanced) toolchainAdvancedOpen.set(true);
  settingsSection.set(section);
  goSpace("settings");
}

/** Opens `target` in the studio (the nav rail folds away). */
export function openStudio(target: StudioTarget) {
  studio.set(target);
  savePrefs({ lastTarget: target.target, lastId: target.id, lastLabel: target.label ?? null });
  const key = target.target === "contest" && target.label ? `contest_item:${target.id}:${target.label}` : `${target.target}:${target.id}`;
  if (visitTimer !== undefined) clearTimeout(visitTimer);
  visitTimer = setTimeout(() => {
    visitTimer = undefined;
    preview.recordVisit(key);
  }, 250);
}

export function closeStudio() {
  studio.set(undefined);
  savePrefs({ lastTarget: null, lastId: null, lastLabel: null });
}

// ── Opening workspaces ───────────────────────────────────────────────────────

/** Makes `next` the open workspace (or reports why not), restarting the watch loop. */
function adopt(next: WorkspaceInfo, failContext: string) {
  if (next.root === "") {
    openError.set({ error: next.error, context: failContext });
    return false;
  }
  const switched = next.root !== info().root;
  if (switched) {
    prefs = { ...prefs, recentWorkspaces: preview.recentWorkspaces() };
  }
  batch(() => {
    openError.set(next.error ? { error: next.error, context: failContext } : undefined);
    if (switched) {
      importWarnings.set(undefined);
      studio.set(undefined);
      activeSnippetStudio.set(undefined);
      activeTrdStudio.set(undefined);
      selectedProblem.set(undefined);
      selectedContest.set(undefined);
      selectedSnippetSlug.set(undefined);
      selectedTrdId.set(undefined);
      archivedExpanded.set(false);
      unfreezeFor.set(undefined);
      dialog.set(undefined);
      paletteOpen.set(false);
      batchTask.set(undefined);
      batchStreamGeneration += 1;
      workspaceVersion.set(workspaceVersion() + 1);
    }
    info.set(next);
    if (!next.trainingEnabled && space() === "training") {
      space.set("problems");
    }
    if (!next.snippetsEnabled && space() === "snippets") {
      space.set("problems");
    }
    revision.set(revision() + 1);
  });
  if (switched) {
    startWatch();
  }
  return true;
}

/** The system's folder dialog, then the workspace there. */
export async function pickWorkspace() {
  const before = info().root;
  const next = await preview.pickWorkspace({
    title: t("dialog.pickWorkspace.title"),
    buttonLabel: t("dialog.pickWorkspace.button"),
  });
  if (next.root === before && !next.error) return;
  if (next.root === "" && !next.error) return; // cancelled with none open
  if (next.root === before && next.error) {
    openError.set({ error: next.error, context: t("ctx.openWorkspace") });
    return;
  }
  adopt(next, t("ctx.openWorkspace"));
}

/** A folder to make a workspace in: opens it when it already is one. */
export async function newWorkspace(template?: string) {
  const picked = await preview.pickFolder({
    title: t("dialog.pickFolder.title"),
    buttonLabel: t("dialog.pickFolder.button"),
  });
  if (picked.cancelled) return;
  if (picked.error) {
    openError.set({ error: picked.error, context: t("ctx.newWorkspace") });
    return;
  }
  if (picked.isWorkspace && picked.workspaceRoot) {
    adopt(preview.openWorkspace(picked.workspaceRoot, false), t("ctx.openWorkspace"));
    return;
  }
  adopt(preview.initWorkspace(picked.path, template ?? null), t("ctx.newWorkspace"));
}

/** Opens a workspace from the recent list. */
export function openRecent(path: string) {
  if (path === info().root) return;
  adopt(preview.openWorkspace(path, false), t("ctx.openWorkspace"));
}

/** Picks a folder, initializes the built-in example workspace there, and opens its studio target. */
export async function openExampleWorkspace(template?: string) {
  const before = info().root;
  const next = await preview.pickAndInitExampleWorkspace(
    template ?? null,
    {
      title: t("dialog.pickExample.title"),
      buttonLabel: t("dialog.pickExample.button"),
    },
    locale(),
  );
  if (next.root === before && !next.error) return;
  if (next.root === "" && !next.error) return; // cancelled with none open
  if (next.root === before && next.error) {
    openError.set({ error: next.error, context: t("ctx.openExampleWorkspace") });
    return;
  }
  if (adopt(next, t("ctx.openExampleWorkspace"))) {
    brandReplayOpen.set(false);
    const preferredId = template === "cnoi-classic" ? "oi-mock" : "welcome-round";
    const welcome = next.contests.find((c) => c.id === preferredId) ?? next.contests[0];
    if (welcome) {
      openStudio({ target: "contest", id: welcome.id, label: welcome.items[0]?.label });
    } else if (next.problems[0]) {
      openStudio({ target: "problem", id: next.problems[0].id });
    }
  }
}

// ── Appearance & editor typography persistence ───────────────────────────────

const APPEARANCE_STORAGE_KEY = "xcpc.appearance.v1";

/** Restores persisted appearance mode and editor typography from NSH Storage. */
export async function hydrateAppearancePrefs(): Promise<void> {
  try {
    const raw = await Storage.getItem(APPEARANCE_STORAGE_KEY);
    if (!raw) return;
    const parsed = JSON.parse(raw) as AppearancePrefs;
    if (parsed && typeof parsed === "object") {
      applyAppearancePrefs(parsed);
    }
  } catch {
    // Ignore corrupt payloads or headless environments without Storage.
  }
}

/** Persists the current appearance mode and editor typography into NSH Storage. */
export function persistAppearancePrefs(): void {
  const payload: AppearancePrefs = {
    appearance: appearanceMode(),
    editorFontSize: editorFontSize(),
    editorLineHeight: editorLineHeight(),
    editorLigatures: editorLigatures(),
    editorVimMode: editorVimMode(),
  };
  try {
    void Storage.setItem(APPEARANCE_STORAGE_KEY, JSON.stringify(payload)).catch(() => {});
  } catch {
    // Headless test environments may not expose Storage.
  }
}

export function updateAppearanceMode(mode: AppearanceMode): void {
  setAppearanceMode(mode);
  persistAppearancePrefs();
}

export function updateEditorFontSize(size: number): void {
  setEditorFontSize(size);
  persistAppearancePrefs();
}

export function updateEditorLineHeight(lineHeight: number): void {
  setEditorLineHeight(lineHeight);
  persistAppearancePrefs();
}

export function updateEditorLigatures(enabled: boolean): void {
  setEditorLigatures(enabled);
  persistAppearancePrefs();
}

export function updateEditorVimMode(enabled: boolean): void {
  setEditorVimMode(enabled);
  persistAppearancePrefs();
}

export function resetEditorTypography(): void {
  batch(() => {
    setEditorFontSize(DEFAULT_EDITOR_FONT_SIZE);
    setEditorLineHeight(DEFAULT_EDITOR_LINE_HEIGHT);
    setEditorLigatures(DEFAULT_EDITOR_LIGATURES);
  });
  persistAppearancePrefs();
}

// ── External changes ─────────────────────────────────────────────────────────

type ChangeListener = (paths: readonly string[], rescan: boolean) => void;
const listeners = new Set<ChangeListener>();

/** Hears external changes (after the workspace index has taken them); returns a function that stops. */
export function onExternalChange(fn: ChangeListener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

let watchGeneration = 0;

/** The `nextChange()` loop: one per open workspace, ended by `"closed"`. */
export function startWatch() {
  const generation = ++watchGeneration;
  if (!hasWorkspace()) return;
  void (async () => {
    for (;;) {
      const ev = await preview.nextChange();
      if (generation !== watchGeneration) return;
      if (ev.kind === "closed") return;
      refresh();
      for (const fn of listeners) fn(ev.paths, ev.kind === "rescan");
    }
  })();
}

let pluginWatchStarted = false;

/** The `nextPluginChange()` loop: watches `<dataDir>/plugins/` and linked dev plugin targets for hot reload. */
export function startPluginWatch() {
  if (pluginWatchStarted) return;
  if (typeof preview.nextPluginChange !== "function") return;
  pluginWatchStarted = true;
  void (async () => {
    for (;;) {
      const ev = await preview.nextPluginChange();
      if (ev.kind === "closed") {
        pluginWatchStarted = false;
        return;
      }
      const changedIds = ev.changedPluginIds;
      if (ev.kind === "rescan") {
        notifyPluginsChanged(undefined);
      } else if (Array.isArray(changedIds)) {
        notifyPluginsChanged(changedIds);
      } else {
        notifyPluginsChanged([]);
      }
      refresh();
    }
  })();
}

/** Picks a Polygon ZIP via the native file dialog, imports it off the UI thread, and opens the imported item. */
export async function importPolygon() {
  if (!hasWorkspace() || info().readOnly || importingPolygon()) return;
  importingPolygon.set(true);
  try {
    const res = await preview.pickAndImportPolygon(null, {
      title: t("dialog.importPolygon.title"),
      buttonLabel: t("dialog.importPolygon.button"),
      filterLabel: t("dialog.importPolygon.filter"),
    });
    if (res.cancelled) return;
    if (!res.ok) {
      openError.set({
        code: res.code,
        error: res.detail ?? res.error,
        context: t("ctx.importPolygon"),
      });
      return;
    }
    batch(() => {
      openError.set(undefined);
      importWarnings.set(
        res.warnings.length > 0 ? { title: res.title || res.id, warnings: res.warnings } : undefined,
      );
    });
    refresh();
    if (res.kind === "contest" && res.id) {
      selectedContest.set(res.id);
      openStudio({ target: "contest", id: res.id });
    } else if (res.id) {
      selectedProblem.set(res.id);
      openStudio({ target: "problem", id: res.id });
    }
  } finally {
    importingPolygon.set(false);
  }
}

// ── Studio actions for the command palette ───────────────────────────────────

export type OjPackageFormat = "domjudge" | "hydro";

export type StudioActions = {
  exportBook(book: Book): void;
  exportOjPackage(format: OjPackageFormat): void;
  copyMarkdownWithSamples(): void;
  reveal(): void;
  freeze(): void;
  unfreeze(): void;
  setTab(tab: "statement" | "samples" | "editorial" | "build"): void;
  /** In a contest: check every problem not checked yet or with issues (§4). */
  checkContest(): void;
  runSanitize(): void;
  toggleInspector(): void;
  openAssets(): void;
  hasAssets(): boolean;
  hasPackage(): boolean;
  initPackage(): void;
  frozen(): boolean;
  archived(): boolean;
  isContest(): boolean;
};

export const studioActions = signal<StudioActions | undefined>(undefined);

/** Requests the views answer: a new/duplicate problem, new contest, adding/picking problems, deleting one, groups, or batch ops. */
export const dialog = signal<
  | { kind: "newProblem"; addTo?: string }
  | { kind: "duplicateProblem"; problemId: string }
  | { kind: "newContest"; withProblem?: string }
  | { kind: "addToContest"; problemId: string }
  | { kind: "pickProblems"; contestId: string }
  | { kind: "deleteProblem"; problemId: string }
  | { kind: "newGroup" }
  | { kind: "renameGroup"; name: string }
  | { kind: "deleteGroup"; name: string }
  | { kind: "batchTags"; problemIds: string[] }
  | { kind: "batchRating"; problemIds: string[] }
  | { kind: "batchDelete"; problemIds: string[] }
  | { kind: "batchAddToContest"; problemIds: string[] }
  | { kind: "batchExport"; problemIds: string[] }
  | undefined
>(undefined);

// ── Global Background Batch Task Stream (§5.4 / §7.1) ────────────────────────

/** Active or most recently completed background batch task snapshot (persists across space switches). */
export const batchTask = signal<BatchTaskSnapshotJs | undefined>(undefined);
let batchStreamGeneration = 0;

function streamBatchTask(taskId: number) {
  const gen = ++batchStreamGeneration;
  batchTask.set(preview.batchTaskStatus());
  void (async () => {
    for (;;) {
      const ev = await preview.nextBatchEvent(taskId);
      if (gen !== batchStreamGeneration) return;
      if (ev.kind === "closed") {
        const snap = preview.batchTaskStatus();
        if (snap.taskId === taskId) batchTask.set(snap);
        return;
      }
      batchTask.set(ev.snapshot);
      if (ev.kind === "all_finished" || ev.kind === "cancelled" || ev.kind === "error") {
        refresh();
        return;
      }
    }
  })();
}

export function startBatchBuildTask(ids: string[], mode: "verify" | "debug"): UiError | undefined {
  if (ids.length === 0) return undefined;
  const res = preview.startBatchBuild(ids, mode, null);
  if (!res.ok) return { code: res.code, error: res.error };
  streamBatchTask(res.taskId);
  return undefined;
}

export function startBatchExportTask(req: BatchExportRequestJs): UiError | undefined {
  if (req.problemIds.length === 0) return undefined;
  const res = preview.startBatchExport(req);
  if (!res.ok) return { code: res.code, error: res.error };
  streamBatchTask(res.taskId);
  return undefined;
}

export function cancelBatchTask() {
  preview.cancelBatchTask();
}

export function dismissBatchTask() {
  const cur = batchTask();
  if (cur?.running) return;
  batchTask.set(undefined);
}

/** The row selected in each space, kept across space switches. */
export const selectedProblem = signal<string | undefined>(undefined);
export const selectedContest = signal<string | undefined>(undefined);
/** Whether the collapsible "已归档" section in the contests sidebar is expanded. */
export const archivedExpanded = signal(false);

/** Navigates to the contests space, auto-expanding the archived section if the target contest is archived. */
export function jumpToContest(contestId: string) {
  const c = info().contests.find((x) => x.id === contestId);
  if (c?.archived) archivedExpanded.set(true);
  selectedContest.set(contestId);
  goSpace("contests");
}

/** The contest whose unfreeze waits for the user's confirmation (GUI only). */
export const unfreezeFor = signal<string | undefined>(undefined);
