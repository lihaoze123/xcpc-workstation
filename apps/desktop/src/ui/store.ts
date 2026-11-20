// The app's state shared by the shell and the views: the native bridge, the workspace,
// the preferences, where the user is (a space, or the studio on an object), the watch
// loop on external changes, and the studio's actions for the command palette.

import process from "process";
import { batch, signal } from "nsh";
import {
  Preview,
  type ActionResult,
  type AppPrefsJs,
  type WorkspaceInfo,
} from "xcpc-workstation/native";
import type { Book, Space } from "./logic";
import { clampRatio } from "./logic";

export const preview = new Preview(process.dataDir);

/** What the studio shows: a problem, or a contest (and the problem of it in the editor). */
export type StudioTarget = { target: "problem" | "contest"; id: string; label?: string };

/** An error the views show with `ErrorDisplay`: the Core's code and raw text, and where it came from. */
export type UiError = { code?: string | null; error?: string | null; context?: string; referencing?: string[] };

export function errorOf(res: ActionResult, context?: string): UiError | undefined {
  if (res.ok) return undefined;
  return { code: res.code, error: res.error, context, referencing: res.referencingContests };
}

// ── Workspace ────────────────────────────────────────────────────────────────

export const info = signal<WorkspaceInfo>(preview.workspace());
/** Bumped whenever the workspace's content may have changed (ours or another's). */
export const revision = signal(0);
/** Bumped whenever the open workspace switches to a different root, so views clear stale filters/errors. */
export const workspaceVersion = signal(0);
/** Why the last attempt to open a workspace failed, for the views that opened it. */
export const openError = signal<UiError | undefined>(undefined);

export function hasWorkspace(): boolean {
  return info().root !== "";
}

/** Reads the workspace again after a change and tells the views. */
export function refresh() {
  batch(() => {
    info.set(preview.workspace());
    revision.set(revision() + 1);
  });
}

// ── Preferences ──────────────────────────────────────────────────────────────

let prefs: AppPrefsJs = preview.loadPrefs();

export function loadPrefs(): AppPrefsJs {
  return prefs;
}

export let splitRatio = clampRatio(prefs.splitRatio);
export const splitRatioSignal = signal(splitRatio);

/** Merges `patch` into the preferences and saves them (the bridge keeps the fields it reserves). */
export function savePrefs(patch: Partial<AppPrefsJs>) {
  prefs = { ...preview.loadPrefs(), ...patch };
  if (patch.splitRatio !== undefined) {
    splitRatio = clampRatio(patch.splitRatio);
    splitRatioSignal.set(splitRatio);
  }
  preview.savePrefs(prefs);
}

// ── Where the user is ────────────────────────────────────────────────────────

function initialSpace(): Space {
  const s = prefs.lastSpace;
  return s === "problems" || s === "contests" || s === "settings" ? s : "problems";
}

export const space = signal<Space>(initialSpace());
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
  batch(() => {
    studio.set(undefined);
    space.set(next);
  });
  savePrefs({ lastSpace: next, lastTarget: null, lastId: null, lastLabel: null });
}

/** Opens `target` in the studio (the nav rail folds away). */
export function openStudio(target: StudioTarget) {
  studio.set(target);
  savePrefs({ lastTarget: target.target, lastId: target.id, lastLabel: target.label ?? null });
  preview.recordVisit(target.target === "contest" && target.label ? `contest_item:${target.id}:${target.label}` : `${target.target}:${target.id}`);
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
  batch(() => {
    openError.set(next.error ? { error: next.error, context: failContext } : undefined);
    if (switched) {
      studio.set(undefined);
      selectedProblem.set(undefined);
      selectedContest.set(undefined);
      unfreezeFor.set(undefined);
      dialog.set(undefined);
      paletteOpen.set(false);
      workspaceVersion.set(workspaceVersion() + 1);
    }
    info.set(next);
    revision.set(revision() + 1);
  });
  if (switched) startWatch();
  return true;
}

/** The system's folder dialog, then the workspace there. */
export async function pickWorkspace() {
  const before = info().root;
  const next = await preview.pickWorkspace();
  if (next.root === before && !next.error) return;
  if (next.root === "" && !next.error) return; // cancelled with none open
  if (next.root === before && next.error) {
    openError.set({ error: next.error, context: "打开工作区" });
    return;
  }
  adopt(next, "打开工作区");
}

/** A folder to make a workspace in: opens it when it already is one. */
export async function newWorkspace() {
  const picked = await preview.pickFolder();
  if (picked.cancelled) return;
  if (picked.error) {
    openError.set({ error: picked.error, context: "新建工作区" });
    return;
  }
  if (picked.isWorkspace && picked.workspaceRoot) {
    adopt(preview.openWorkspace(picked.workspaceRoot, false), "打开工作区");
    return;
  }
  adopt(preview.initWorkspace(picked.path), "新建工作区");
}

/** Opens a workspace from the recent list. */
export function openRecent(path: string) {
  if (path === info().root) return;
  adopt(preview.openWorkspace(path, false), "打开工作区");
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

// ── Studio actions for the command palette ───────────────────────────────────

export type StudioActions = {
  exportBook(book: Book): void;
  reveal(): void;
  freeze(): void;
  unfreeze(): void;
  setTab(tab: "statement" | "samples" | "editorial"): void;
  toggleInspector(): void;
  frozen(): boolean;
  isContest(): boolean;
};

export const studioActions = signal<StudioActions | undefined>(undefined);

/** Requests the views answer: a new problem/contest dialog, adding a problem to a contest, deleting one. */
export const dialog = signal<
  | { kind: "newProblem"; addTo?: string }
  | { kind: "newContest"; withProblem?: string }
  | { kind: "addToContest"; problemId: string }
  | { kind: "deleteProblem"; problemId: string }
  | undefined
>(undefined);

/** The row selected in each space, kept across space switches. */
export const selectedProblem = signal<string | undefined>(undefined);
export const selectedContest = signal<string | undefined>(undefined);

/** The contest whose unfreeze waits for the user's confirmation (GUI only). */
export const unfreezeFor = signal<string | undefined>(undefined);
