// Zero-reflow drag reordering (§8.3) for the outline and the samples: a 4-point
// threshold, the rows measured once when the drag starts, only the dragged row follows
// the pointer (a compositor `translateY`), a thin line marks where it lands, and the
// drop swaps the slots in 180 ms (ease-out) with the order kept as it was dropped until
// the write lands.

import { batch, computed, signal, type MouseEvent, type PressEvent, type ViewRef } from "nsh";
import { dropIndex } from "../logic";
import { spatialEnabled } from "../tokens";

export function createReorder(commit: (from: number, to: number) => void, gap = 0) {
  const drag = signal<{ from: number; to: number } | undefined>(undefined);
  const offset = signal(0);
  const rows: (ViewRef | undefined)[] = [];
  let container: ViewRef | undefined;
  let containerTop = 0;
  let slots: { top: number; height: number }[] = [];
  let press: { index: number; y: number; measured: boolean } | undefined;
  let dragged = false;

  function finish(to: number | undefined, from: number) {
    if (to === undefined || to === from) return;
    if (spatialEnabled()) container?.animateLayout({ type: "easeOut", duration: 180 });
    commit(from, to);
  }

  /** Ends the drag; a release past the threshold before any move frame still lands. */
  function end(e?: MouseEvent) {
    const d = drag();
    const p = press;
    press = undefined;
    if (!d && !p) return;
    batch(() => {
      drag.set(undefined);
      offset.set(0);
    });
    if (d) return finish(d.to, d.from);
    if (p && p.measured && e && Math.abs(e.pagePosition.y - p.y) >= 4) {
      dragged = true;
      finish(dropIndex(slots, p.index, e.pagePosition.y), p.index);
    }
  }

  return {
    /** The drag in progress: the row's index and where it would land. */
    drag,
    /** How far the dragged row has moved, in points. */
    offset,
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
      if (rows.length > count) rows.length = count;
      const p = { index, y: e.pagePosition.y, measured: false };
      press = p;
      dragged = false;
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
