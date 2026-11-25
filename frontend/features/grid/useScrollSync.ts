/**
 * features/grid/useScrollSync.ts: keeps the grid and the Gantt chart
 * scrolled to the same rows.
 *
 * What it is for:
 *   The grid (left) and the Gantt chart (right) show the same rows side by
 *   side, each with its own scrolling area. When the user scrolls one, the
 *   other must follow, or the bars no longer sit next to their tasks.
 *
 * Where it fits:
 *   Called by `TaskGrid.tsx`. The Gantt chart (`features/gantt/GanttView.tsx`)
 *   marks its scrolling layer with the attribute `data-gantt-scroller`; the
 *   hook finds that element inside the container that `shell/AppShell.tsx`
 *   wraps around the chart.
 *
 * Main ideas:
 *   - Both areas use the same geometry: a header of `HEADER_HEIGHT` pixels
 *     and rows of `ROW_HEIGHT` pixels (`features/gantt/renderer/layout.ts`),
 *     so equal `scrollTop` values mean equal rows.
 *   - **No endless ping-pong.** Setting `scrollTop` on one element fires a
 *     "scroll" event on it, which would set the first one again… We only
 *     copy the value when it really differs (by more than half a pixel), so
 *     the second event finds both equal and stops.
 *   - **Finding the chart's scroller.** By its marker attribute,
 *     `[data-gantt-scroller]` (a CSS *attribute selector*: "any element that
 *     has this attribute"). A marker is more reliable than guessing from CSS
 *     which element scrolls, and does not depend on the chart's inner
 *     structure.
 */

import { useEffect, type RefObject } from "react";

/** The attribute the Gantt chart puts on its scrolling element. */
export const GANTT_SCROLLER_ATTRIBUTE = "data-gantt-scroller";

/**
 * Finds the Gantt chart's scrolling element inside a container.
 *
 * @param container - The element around the Gantt chart.
 * @returns The element marked with `data-gantt-scroller`, or `null` if
 *   there is none (yet), e.g. before the chart is drawn.
 *
 * @example
 * findGanttScroller(document.body); // <div data-gantt-scroller …> or null
 */
export function findGanttScroller(container: HTMLElement): HTMLElement | null {
  // `querySelector` returns the first matching element inside, or `null`.
  return container.querySelector<HTMLElement>(`[${GANTT_SCROLLER_ATTRIBUTE}]`);
}

/**
 * Keeps the vertical scroll position of the grid and the chart equal.
 *
 * @param gridScroller - The grid's scrolling element.
 * @param ganttContainer - The element that contains the Gantt chart.
 * @param resubscribeKey - Any value that changes when the chart may have
 *   been rebuilt (e.g. mock on/off); the hook then looks for the chart's
 *   scroller again.
 *
 * @example
 * useScrollSync(scrollerRef, ganttContainerRef, isDemoData);
 *
 * Beginner note: the function returned from `useEffect` is the *clean-up*:
 * React calls it before running the effect again and when the grid
 * disappears, so no listener is left behind.
 */
export function useScrollSync(
  gridScroller: RefObject<HTMLElement | null>,
  ganttContainer: RefObject<HTMLElement | null>,
  resubscribeKey: unknown,
): void {
  useEffect(() => {
    const grid = gridScroller.current;
    const container = ganttContainer.current;
    if (grid === null || container === null) return;
    const gantt = findGanttScroller(container);
    if (gantt === null) return;

    /** Copies `from.scrollTop` to `to` if they differ. */
    const copy = (from: HTMLElement, to: HTMLElement) => {
      if (Math.abs(from.scrollTop - to.scrollTop) > 0.5) to.scrollTop = from.scrollTop;
    };
    const onGridScroll = () => copy(grid, gantt);
    const onGanttScroll = () => copy(gantt, grid);
    // `passive: true` promises we never call preventDefault, which lets the
    // browser scroll smoothly without waiting for our code.
    grid.addEventListener("scroll", onGridScroll, { passive: true });
    gantt.addEventListener("scroll", onGanttScroll, { passive: true });
    // Start equal: the chart may already be scrolled.
    copy(gantt, grid);
    return () => {
      grid.removeEventListener("scroll", onGridScroll);
      gantt.removeEventListener("scroll", onGanttScroll);
    };
  }, [gridScroller, ganttContainer, resubscribeKey]);
}
