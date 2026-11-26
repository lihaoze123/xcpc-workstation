// Zero-reflow drag reordering (§8.3, Phase 6G §5) for the outline, samples, and contest
// problem list: a 4-point threshold, rows measured once when drag starts, compositor-only
// `translateY` throughout, and on drop the dragged row starts at its exact offset relative
// to its new slot (zero jump) and springs to 0 via `SPRING.standard` preserving ~50ms
// release velocity while displaced siblings glide via `SPRING.standard`.

import {
  Animated,
  batch,
  computed,
  createAnimatedValue,
  signal,
  type CompositeAnimation,
  type MouseEvent,
  type PressEvent,
  type ViewRef,
  type ViewStyle,
} from "nsh";
import { dropIndex } from "../logic";
import { motionSlowFactor, scaledSpring, spatialEnabled, SPRING } from "../tokens";

export interface ReorderSlot {
  top: number;
  height: number;
}

export interface PointerSample {
  t: number;
  y: number;
}

const VELOCITY_WINDOW_MS = 50;
const MAX_RELEASE_VELOCITY = 3000;

/** Computes the dragged row's initial `translateY` relative to its new slot so its
 *  screen position at the moment of drop does not jump by a single pixel. */
export function computeDropDeltaY(
  slots: readonly ReorderSlot[],
  from: number,
  to: number,
  offset: number,
): number {
  const fromSlot = slots[from];
  const toSlot = slots[to];
  if (!fromSlot || !toSlot || from === to) return offset;
  const visualTop = fromSlot.top + offset;
  const newSlotTop = to < from ? toSlot.top : toSlot.top + toSlot.height - fromSlot.height;
  return visualTop - newSlotTop;
}

/** Computes signed vertical release velocity (pt/s) over the trailing ~50ms window. */
export function computeReleaseVelocity(samples: readonly PointerSample[], now: number): number {
  if (samples.length < 2) return 0;
  const cutoff = now - VELOCITY_WINDOW_MS;
  let first = samples[samples.length - 1];
  for (let i = samples.length - 2; i >= 0; i -= 1) {
    if (samples[i].t < cutoff) break;
    first = samples[i];
  }
  const last = samples[samples.length - 1];
  const dt = last.t - first.t;
  if (dt < 4) return 0;
  const raw = ((last.y - first.y) / dt) * 1000;
  return Math.max(-MAX_RELEASE_VELOCITY, Math.min(MAX_RELEASE_VELOCITY, raw));
}

export function createReorder(commit: (from: number, to: number) => void, gap = 0) {
  const drag = signal<{ from: number; to: number } | undefined>(undefined);
  const offset = signal(0);
  const settling = signal<{ dropped: number; sibLo: number; sibHi: number } | undefined>(undefined);
  const settleY = createAnimatedValue(0);
  const siblingY = createAnimatedValue(0);
  const rows: (ViewRef | undefined)[] = [];
  let container: ViewRef | undefined;
  let containerTop = 0;
  let slots: ReorderSlot[] = [];
  let press: { index: number; y: number; measured: boolean } | undefined;
  let dragged = false;
  let samples: PointerSample[] = [];
  let activeAnim: CompositeAnimation | undefined;

  function stopSettle() {
    if (activeAnim) {
      activeAnim.stop();
      activeAnim = undefined;
    }
    if (settling()) settling.set(undefined);
    settleY.set(0);
    siblingY.set(0);
  }

  function finish(to: number | undefined, from: number, releaseOffset: number, velocity: number) {
    const dest = to ?? from;
    const moved = dest !== from;
    const deltaY = computeDropDeltaY(slots, from, dest, releaseOffset);
    const shiftH = slots[from] ? slots[from].height + gap : 0;
    const sibDelta = !moved ? 0 : dest < from ? -shiftH : shiftH;
    const sibLo = !moved ? -1 : dest < from ? dest + 1 : from;
    const sibHi = !moved ? -2 : dest < from ? from : dest - 1;

    if (moved) commit(from, dest);

    if (!spatialEnabled() || (Math.abs(deltaY) < 0.5 && Math.abs(sibDelta) < 0.5 && Math.abs(velocity) < 5)) {
      stopSettle();
      return;
    }

    stopSettle();
    const slow = motionSlowFactor();
    const springCfg = scaledSpring(SPRING.standard);
    settleY.set(deltaY);
    siblingY.set(sibDelta);
    settling.set({ dropped: dest, sibLo, sibHi });

    const anims: CompositeAnimation[] = [
      Animated.spring(settleY, {
        toValue: 0,
        stiffness: springCfg.stiffness,
        damping: springCfg.damping,
        mass: springCfg.mass,
        velocity: velocity / slow,
      }),
    ];
    if (moved && Math.abs(sibDelta) >= 0.5) {
      anims.push(
        Animated.spring(siblingY, {
          toValue: 0,
          stiffness: springCfg.stiffness,
          damping: springCfg.damping,
          mass: springCfg.mass,
        }),
      );
    }
    const anim = anims.length === 1 ? anims[0] : Animated.parallel(anims);
    activeAnim = anim;
    void anim.start(({ finished }) => {
      if (activeAnim === anim) {
        activeAnim = undefined;
        if (finished) {
          settling.set(undefined);
          settleY.set(0);
          siblingY.set(0);
        }
      }
    });
  }

  /** Ends the drag; a release past the threshold before any move frame still lands. */
  function end(e?: MouseEvent) {
    const d = drag();
    const p = press;
    const releaseOffset = offset();
    const now = performance.now();
    if (e) samples.push({ t: now, y: e.pagePosition.y });
    const velocity = computeReleaseVelocity(samples, now);
    samples = [];
    press = undefined;
    if (!d && !p) return;
    batch(() => {
      drag.set(undefined);
      offset.set(0);
    });
    if (d) return finish(d.to, d.from, releaseOffset, velocity);
    if (p && p.measured && e && Math.abs(e.pagePosition.y - p.y) >= 4) {
      dragged = true;
      finish(dropIndex(slots, p.index, e.pagePosition.y), p.index, e.pagePosition.y - p.y, velocity);
    }
  }

  return {
    /** The drag in progress: the row's index and where it would land. */
    drag,
    /** How far the dragged row has moved, in points. */
    offset,
    /** Whether row `index` is currently dragged or springing into its new slot (keep elevated z-index). */
    isElevated(index: number): boolean {
      return drag()?.from === index || settling()?.dropped === index;
    },
    /** Compositor-only transform for row `index` during drag and spring settle. */
    rowTransform(index: number): ViewStyle["transform"] {
      if (drag()?.from === index) return `translateY(${offset()}px)`;
      const s = settling();
      if (!s) return undefined;
      if (s.dropped === index) return [{ translateY: settleY }];
      if (index >= s.sibLo && index <= s.sibHi) return [{ translateY: siblingY }];
      return undefined;
    },
    container(r: ViewRef) {
      container = r;
    },
    row(index: number, r: ViewRef) {
      rows[index] = r;
    },
    /** Truncates stale refs after rows are removed. */
    prune(count: number) {
      if (rows.length > count) rows.length = count;
    },
    /** A press on row `index` of `count`: the slots are measured now, once, for the drag it may start. */
    pressDown(index: number, e: PressEvent, count: number) {
      stopSettle();
      if (rows.length > count) rows.length = count;
      const p = { index, y: e.pagePosition.y, measured: false };
      press = p;
      dragged = false;
      samples = [{ t: performance.now(), y: e.pagePosition.y }];
      void Promise.all([container?.measure(), ...rows.slice(0, count).map((r) => r?.measure())]).then(([box, ...ms]) => {
        containerTop = box?.pagePosition.y ?? 0;
        slots = ms.map((m) => (m ? { top: m.pagePosition.y, height: m.bounds.size.height } : { top: 0, height: 0 }));
        p.measured = true;
      });
    },
    /** A press that ended a drag is no click. */
    clicked(): boolean {
      const was = dragged;
      dragged = false;
      return !was;
    },
    move(e: MouseEvent) {
      if (!press) return;
      if (e.pressedButton !== "left") return end(e);
      const now = performance.now();
      samples.push({ t: now, y: e.pagePosition.y });
      if (samples.length > 16) samples = samples.slice(-12);
      const dy = e.pagePosition.y - press.y;
      const d = drag();
      if (!d && (Math.abs(dy) < 4 || !press.measured)) return;
      dragged = true;
      const from = d ? d.from : press.index;
      const to = dropIndex(slots, from, e.pagePosition.y);
      batch(() => {
        offset.set(dy);
        if (!d || to !== d.to) drag.set({ from, to });
      });
    },
    end,
    /** Settles an in-flight drag if the pointer leaves the outer scroll area. */
    leave(e?: MouseEvent) {
      if (press || drag()) end(e);
    },
    /** Where the landing line goes, from the container's top; `undefined` while it would not move. */
    line: computed(() => {
      const d = drag();
      if (!d || d.to === d.from || !slots[d.to]) return undefined;
      const s = slots[d.to];
      const half = gap / 2;
      const y = d.to < d.from ? s.top - (d.to === 0 ? Math.min(half, 4) : half) : s.top + s.height + half;
      return y - containerTop - 1;
    }),
  };
}

export type Reorder = ReturnType<typeof createReorder>;
