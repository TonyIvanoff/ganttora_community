/**
 * tests/frontend/gantt/links.test.ts: tests for drawing dependency arrows.
 *
 * What it checks:
 *   `fsLinkPath` in `frontend/features/gantt/renderer/links.ts` plans the
 *   line of a finish-to-start arrow: from the end of the predecessor's bar to
 *   the successor's bar, using only horizontal and vertical segments
 *   ("orthogonal routing"), with the arrow head pointing the right way.
 *
 * Why it matters:
 *   Arrows that cut diagonally through bars, or point the wrong way, make a
 *   plan unreadable. Two shapes exist: a simple "L" when the successor
 *   starts clearly to the right, and a detour ("S" shape) when it starts
 *   before the predecessor ends (for example with a lead).
 *
 * Main ideas:
 *   Bars are 14 px tall, rows 28 px. Points are in CSS pixels; y grows
 *   downwards (screen coordinates).
 */

import { describe, expect, it } from "vitest";
import { LINK_GAP, fsLinkPath, type Point } from "@frontend/features/gantt/renderer/links";

const BAR = 14;
const ROW = 28;

/** True if every segment of the path is horizontal or vertical. */
function isOrthogonal(points: readonly Point[]): boolean {
  return points.every((p, i) => {
    const next = points[i + 1];
    return next === undefined || next.x === p.x || next.y === p.y;
  });
}

describe("fsLinkPath", () => {
  // Successor one row below and well to the right: an "L" — right, then
  // down onto the top edge of the successor's bar, arrow pointing down.
  it("draws an L shape down to a later task", () => {
    const path = fsLinkPath(100, 50, 200, 78, BAR, ROW);
    expect(path.arrow).toBe("down");
    expect(path.points).toEqual([
      { x: 100, y: 50 },
      { x: 206, y: 50 },
      { x: 206, y: 78 - BAR / 2 },
    ]);
  });

  // Successor above (a predecessor lower in the list): the L goes up and
  // ends on the bottom edge of the bar, arrow pointing up.
  it("draws an L shape up to a task in an earlier row", () => {
    const path = fsLinkPath(100, 78, 200, 22, BAR, ROW);
    expect(path.arrow).toBe("up");
    expect(path.points.at(-1)).toEqual({ x: 206, y: 22 + BAR / 2 });
  });

  // Successor starts before the predecessor ends: go out to the right, down
  // half a row into the gap between rows, back left, and in from the left
  // with the arrow pointing right.
  it("draws a detour when the successor starts earlier", () => {
    const path = fsLinkPath(100, 50, 90, 78, BAR, ROW);
    expect(path.arrow).toBe("right");
    expect(path.points).toEqual([
      { x: 100, y: 50 },
      { x: 100 + LINK_GAP, y: 50 },
      { x: 100 + LINK_GAP, y: 78 - ROW / 2 },
      { x: 90 - LINK_GAP, y: 78 - ROW / 2 },
      { x: 90 - LINK_GAP, y: 78 },
      { x: 90, y: 78 },
    ]);
  });

  // The L needs at least LINK_GAP px of horizontal room; just below that
  // the detour is used (the edge case between the two shapes).
  it("switches to the detour when there is too little room", () => {
    // turnX = toX + 6 must be >= fromX + 8, so toX = 102 is the smallest L.
    expect(fsLinkPath(100, 50, 102, 78, BAR, ROW).arrow).toBe("down");
    expect(fsLinkPath(100, 50, 101, 78, BAR, ROW).arrow).toBe("right");
  });

  // Both tasks on the same row cannot use the L (it would end inside the
  // bar); the detour is used and ends pointing right.
  it("uses the detour for two bars on the same row", () => {
    const path = fsLinkPath(100, 50, 300, 50, BAR, ROW);
    expect(path.arrow).toBe("right");
    expect(path.points.at(-1)).toEqual({ x: 300, y: 50 });
  });

  // Whatever the shape, the path starts at the predecessor's end, ends on
  // the successor, and has only horizontal and vertical segments.
  it("is always orthogonal and starts at the predecessor", () => {
    const cases: Array<[number, number, number, number]> = [
      [100, 50, 200, 78],
      [100, 78, 200, 22],
      [100, 50, 90, 78],
      [100, 50, 90, 22],
      [100, 50, 300, 50],
      [0, 0, 0, 280],
    ];
    for (const [fx, fy, tx, ty] of cases) {
      const path = fsLinkPath(fx, fy, tx, ty, BAR, ROW);
      expect(path.points[0]).toEqual({ x: fx, y: fy });
      expect(isOrthogonal(path.points)).toBe(true);
    }
  });
});
