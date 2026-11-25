/**
 * tests/frontend/gantt/linkTypes.test.ts: tests for drawing the four link
 * types.
 *
 * What it checks:
 *   `linkPath` in `frontend/features/gantt/renderer/links.ts` gives the
 *   corner points of the arrow line for each link type:
 *   - FS (finish-to-start) uses the same route as `fsLinkPath`;
 *   - SS leaves the predecessor's start to the left and enters the
 *     successor's start from the left;
 *   - FF leaves the predecessor's finish to the right and enters the
 *     successor's finish from the right;
 *   - SF has two cases: a direct route when the successor finishes well to
 *     the left of the predecessor's start, and a detour otherwise.
 *
 * Why it matters:
 *   The arrow must start at the right end of one bar and point at the
 *   right end of the other, otherwise the chart tells the user a different
 *   story than the schedule ("does B wait for A's start or A's finish?").
 *
 * Main ideas:
 *   Bars are given by their start x, finish x and middle y. Lines only go
 *   horizontally or vertically ("orthogonal" routing), so every pair of
 *   neighbouring points shares an x or a y.
 */

import { describe, expect, it } from "vitest";
import {
  LINK_GAP,
  fsLinkPath,
  linkPath,
  type BarEnds,
  type LinkPath,
} from "@frontend/features/gantt/renderer/links";

const BAR = 14;
const ROW = 28;

/** Predecessor bar on row 0: x 100–200, y 14. */
const PRED: BarEnds = { startX: 100, finishX: 200, y: 14 };
/** Successor bar on row 1: x 150–260, y 42. */
const SUCC: BarEnds = { startX: 150, finishX: 260, y: 42 };

/** True if every segment of the path is horizontal or vertical. */
function orthogonal(path: LinkPath): boolean {
  return path.points.every((p, i) => {
    const next = path.points[i + 1];
    return next === undefined || next.x === p.x || next.y === p.y;
  });
}

describe("linkPath", () => {
  // FS is exactly the M0 route.
  it("routes FS like fsLinkPath", () => {
    expect(linkPath("finishToStart", PRED, SUCC, BAR, ROW)).toEqual(
      fsLinkPath(PRED.finishX, PRED.y, SUCC.startX, SUCC.y, BAR, ROW),
    );
  });

  // SS: out to the left of the leftmost start, down, in from the left.
  it("routes SS from start to start on the left", () => {
    const path = linkPath("startToStart", PRED, SUCC, BAR, ROW);
    expect(path).toEqual({
      points: [
        { x: 100, y: 14 },
        { x: 100 - LINK_GAP, y: 14 },
        { x: 100 - LINK_GAP, y: 42 },
        { x: 150, y: 42 },
      ],
      arrow: "right",
    });
  });

  // FF: out to the right of the rightmost finish, down, in from the right.
  it("routes FF from finish to finish on the right", () => {
    const path = linkPath("finishToFinish", PRED, SUCC, BAR, ROW);
    expect(path).toEqual({
      points: [
        { x: 200, y: 14 },
        { x: 260 + LINK_GAP, y: 14 },
        { x: 260 + LINK_GAP, y: 42 },
        { x: 260, y: 42 },
      ],
      arrow: "left",
    });
  });

  // SF, simple case: B finishes at x 60, well left of A's start (100).
  it("routes SF directly when the successor ends well to the left", () => {
    const succ: BarEnds = { startX: 20, finishX: 60, y: 42 };
    expect(linkPath("startToFinish", PRED, succ, BAR, ROW)).toEqual({
      points: [
        { x: 100, y: 14 },
        { x: 60 + LINK_GAP, y: 14 },
        { x: 60 + LINK_GAP, y: 42 },
        { x: 60, y: 42 },
      ],
      arrow: "left",
    });
  });

  // SF, detour: B finishes right of A's start, so the line steps out left
  // of A, runs in the gap between the rows, and enters B's finish from the
  // right.
  it("routes SF with a detour when the bars overlap", () => {
    const path = linkPath("startToFinish", PRED, SUCC, BAR, ROW);
    expect(path).toEqual({
      points: [
        { x: 100, y: 14 },
        { x: 100 - LINK_GAP, y: 14 },
        { x: 100 - LINK_GAP, y: 42 - ROW / 2 },
        { x: 260 + LINK_GAP, y: 42 - ROW / 2 },
        { x: 260 + LINK_GAP, y: 42 },
        { x: 260, y: 42 },
      ],
      arrow: "left",
    });
  });

  // SF detour upwards: a successor in an earlier row uses the gap below it.
  it("uses the gap below the successor when it is above", () => {
    const up: BarEnds = { ...SUCC, y: -14 };
    const path = linkPath("startToFinish", PRED, up, BAR, ROW);
    expect(path.points[2]).toEqual({ x: 100 - LINK_GAP, y: -14 + ROW / 2 });
  });

  // Every type, both directions: orthogonal, starts at the right end of the
  // predecessor and ends at the right end of the successor.
  it("starts and ends at the ends named by the link type", () => {
    const ends = {
      finishToStart: ["finishX", "startX"],
      startToStart: ["startX", "startX"],
      finishToFinish: ["finishX", "finishX"],
      startToFinish: ["startX", "finishX"],
    } as const;
    for (const [kind, [from, to]] of Object.entries(ends)) {
      for (const succ of [SUCC, { ...SUCC, y: -14 }]) {
        const path = linkPath(kind as keyof typeof ends, PRED, succ, BAR, ROW);
        expect(orthogonal(path), kind).toBe(true);
        expect(path.points[0], kind).toEqual({ x: PRED[from], y: PRED.y });
        const last = path.points[path.points.length - 1];
        if (kind === "finishToStart" && last?.y !== succ.y) {
          // FS L-shape ends on the bar's top/bottom edge above its start.
          expect(last?.x, kind).toBeGreaterThanOrEqual(succ.startX);
        } else {
          expect(last, kind).toEqual({ x: succ[to], y: succ.y });
        }
      }
    }
  });
});
