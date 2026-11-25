/**
 * tests/frontend/grid/scrollSync.test.ts: tests for finding the Gantt
 * chart's scrolling layer.
 *
 * What it checks:
 *   `findGanttScroller` in `frontend/features/grid/useScrollSync.ts` asks
 *   the container for the element marked `data-gantt-scroller` and returns
 *   it, or `null` when the chart is not drawn yet.
 *
 * Why it matters:
 *   The grid and the chart scroll together only if the grid finds the
 *   chart's scroller. Looking for the wrong marker would silently break
 *   the side-by-side view: bars would drift away from their task rows.
 *
 * Main ideas:
 *   These tests run in Node, which has no web page. No DOM library (such
 *   as jsdom) is installed, so we pass a tiny fake container with only a
 *   `querySelector` method and record the selector it receives. The hook
 *   `useScrollSync` itself needs React rendering and is not tested here.
 */

import { describe, expect, it } from "vitest";
import { GANTT_SCROLLER_ATTRIBUTE, findGanttScroller } from "@frontend/features/grid/useScrollSync";

/**
 * Builds a fake container.
 *
 * @param found - What `querySelector` should return.
 * @returns The fake (typed as an `HTMLElement` for the call) and the list
 *   of selectors it was asked for.
 */
function fakeContainer(found: object | null): { container: HTMLElement; asked: string[] } {
  const asked: string[] = [];
  const fake = {
    querySelector: (selector: string) => {
      asked.push(selector);
      return found;
    },
  };
  return { container: fake as unknown as HTMLElement, asked };
}

describe("findGanttScroller", () => {
  // The marker is the attribute the chart puts on its scroller.
  it("uses the data-gantt-scroller attribute", () => {
    expect(GANTT_SCROLLER_ATTRIBUTE).toBe("data-gantt-scroller");
  });

  // It looks for "any element with the attribute" and returns the match.
  it("returns the element marked as the scroller", () => {
    const scroller = { id: "scroller" };
    const { container, asked } = fakeContainer(scroller);
    expect(findGanttScroller(container)).toBe(scroller);
    expect(asked).toEqual(["[data-gantt-scroller]"]);
  });

  // Before the chart is drawn there is no scroller: `null`, no crash.
  it("returns null when there is no scroller yet", () => {
    const { container } = fakeContainer(null);
    expect(findGanttScroller(container)).toBeNull();
  });
});
