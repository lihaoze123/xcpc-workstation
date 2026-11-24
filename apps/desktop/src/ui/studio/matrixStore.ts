// Fine-grained per-cell signal store for the BuildConsole verdict matrix (Phase 3).
//
// Isolates each `(testIndex, solution)` cell behind its own signal, diffs incoming
// snapshots inside a single `batch()`, and only writes the cells that actually changed.
// `dispose()` clears internal maps on problem switches so signals never leak across problems.

import { batch, signal, untracked, type Signal } from "nsh";
import { matrixCellKey, type MatrixCellData } from "./buildLogic";

export function matrixCellsEqual(a: MatrixCellData | undefined, b: MatrixCellData | undefined): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return (
    a.test_name === b.test_name &&
    a.verdict === b.verdict &&
    a.cpu_time_ms === b.cpu_time_ms &&
    a.wall_time_ms === b.wall_time_ms &&
    a.peak_memory_kib === b.peak_memory_kib &&
    a.score_ratio === b.score_ratio &&
    a.stale === b.stale &&
    a.cache_hit === b.cache_hit &&
    a.checker_message === b.checker_message &&
    a.stderr === b.stderr &&
    a.output_preview === b.output_preview &&
    a.repro_cmd === b.repro_cmd &&
    a.transcript === b.transcript
  );
}

export interface MatrixCellStore {
  /** Returns the isolated signal for a single `(testIndex, solution)` cell. */
  cell(testIndex: number, solution: string): Signal<MatrixCellData | undefined>;
  /** Returns a signal that briefly turns true when a cell streams in during a live run. */
  fresh(testIndex: number, solution: string): Signal<boolean>;
  /**
   * Reconciles the full merged map into per-cell signals inside a single `batch()`.
   * Returns the number of cells that actually changed (0 when nothing changed).
   */
  reconcile(nextMap: Map<string, MatrixCellData>, streaming: boolean): number;
  /** Clears all cell values while preserving existing signal references. */
  reset(): void;
  /** Clears timers and prunes all accumulated signal maps (use on problem switch / unmount). */
  dispose(): void;
  /** Diagnostic helper: current number of tracked cell signals. */
  size(): number;
}

export function createMatrixCellStore(pulseDurationMs = 360): MatrixCellStore {
  const cells = new Map<string, Signal<MatrixCellData | undefined>>();
  const freshFlags = new Map<string, Signal<boolean>>();
  const freshTimers = new Map<string, ReturnType<typeof setTimeout>>();

  function getOrCreateCell(key: string): Signal<MatrixCellData | undefined> {
    let sig = cells.get(key);
    if (!sig) {
      sig = signal<MatrixCellData | undefined>(undefined);
      cells.set(key, sig);
    }
    return sig;
  }

  function getOrCreateFresh(key: string): Signal<boolean> {
    let sig = freshFlags.get(key);
    if (!sig) {
      sig = signal(false);
      freshFlags.set(key, sig);
    }
    return sig;
  }

  function triggerPulse(key: string): void {
    const sig = getOrCreateFresh(key);
    sig.set(true);
    const prev = freshTimers.get(key);
    if (prev !== undefined) clearTimeout(prev);
    const timer = setTimeout(() => {
      freshTimers.delete(key);
      sig.set(false);
    }, pulseDurationMs);
    freshTimers.set(key, timer);
  }

  function clearTimers(): void {
    for (const timer of freshTimers.values()) {
      clearTimeout(timer);
    }
    freshTimers.clear();
  }

  return {
    cell(testIndex: number, solution: string): Signal<MatrixCellData | undefined> {
      return getOrCreateCell(matrixCellKey(testIndex, solution));
    },

    fresh(testIndex: number, solution: string): Signal<boolean> {
      return getOrCreateFresh(matrixCellKey(testIndex, solution));
    },

    reconcile(nextMap: Map<string, MatrixCellData>, streaming: boolean): number {
      let changed = 0;
      batch(() => {
        for (const [key, nextVal] of nextMap) {
          const sig = getOrCreateCell(key);
          const prevVal = untracked(sig);
          if (!matrixCellsEqual(prevVal, nextVal)) {
            sig.set(nextVal);
            changed += 1;
            if (streaming && (!prevVal || prevVal.stale)) {
              triggerPulse(key);
            }
          }
        }
        for (const [key, sig] of cells) {
          if (!nextMap.has(key) && untracked(sig) !== undefined) {
            sig.set(undefined);
            changed += 1;
          }
        }
      });
      return changed;
    },

    reset(): void {
      clearTimers();
      batch(() => {
        for (const sig of cells.values()) {
          if (untracked(sig) !== undefined) sig.set(undefined);
        }
        for (const sig of freshFlags.values()) {
          if (untracked(sig)) sig.set(false);
        }
      });
    },

    dispose(): void {
      clearTimers();
      batch(() => {
        for (const sig of cells.values()) {
          if (untracked(sig) !== undefined) sig.set(undefined);
        }
        for (const sig of freshFlags.values()) {
          if (untracked(sig)) sig.set(false);
        }
      });
      cells.clear();
      freshFlags.clear();
    },

    size(): number {
      return cells.size;
    },
  };
}

/**
 * Pure keyboard step helper for navigating the verdict matrix (`ArrowUp/Down/Left/Right`
 * or `J/K`) without losing selection, clamping at bounds and optionally skipping empty cells.
 */
export function stepMatrixSelection(
  current: { testIndex: number; solution: string } | undefined,
  rowIndices: readonly number[],
  solutionNames: readonly string[],
  dir: "up" | "down" | "left" | "right",
  hasCell?: (testIndex: number, solution: string) => boolean,
): { testIndex: number; solution: string } | undefined {
  if (rowIndices.length === 0 || solutionNames.length === 0) return undefined;
  const exists = (r: number, c: number) => !hasCell || hasCell(rowIndices[r], solutionNames[c]);

  if (!current) {
    for (let r = 0; r < rowIndices.length; r += 1) {
      for (let c = 0; c < solutionNames.length; c += 1) {
        if (exists(r, c)) return { testIndex: rowIndices[r], solution: solutionNames[c] };
      }
    }
    return undefined;
  }

  const rIdx = rowIndices.indexOf(current.testIndex);
  const cIdx = solutionNames.indexOf(current.solution);
  const safeR = rIdx >= 0 ? rIdx : 0;
  const safeC = cIdx >= 0 ? cIdx : 0;

  const dr = dir === "up" ? -1 : dir === "down" ? 1 : 0;
  const dc = dir === "left" ? -1 : dir === "right" ? 1 : 0;

  let nr = safeR + dr;
  let nc = safeC + dc;
  while (nr >= 0 && nr < rowIndices.length && nc >= 0 && nc < solutionNames.length) {
    if (exists(nr, nc)) {
      return { testIndex: rowIndices[nr], solution: solutionNames[nc] };
    }
    nr += dr;
    nc += dc;
  }
  return { testIndex: rowIndices[safeR], solution: solutionNames[safeC] };
}
