/**
 * renderer/links.ts: the shape of a dependency arrow between two bars.
 *
 * What it is for:
 *   A finish-to-start (FS) link means "B can start only after A finishes".
 *   Gantt charts draw it as an arrow from the end of bar A to the start of
 *   bar B. This file computes the corner points of that arrow as a pure
 *   function; `draw.ts` then draws the lines. `linkPath` handles all four
 *   link types; each one leaves and enters the bars at the ends the type
 *   names (Start or Finish):
 *
 * ```
 *  SS: start → start          FF: finish → finish       SF: start → finish
 *   ┌─┌──A──┐                 ┌──A──┐─┐                 ┌─┌──A──┐
 *   │ └─────┘                 └─────┘ │                 │ └─────┘
 *   └─►┌──B──┐            ┌──B──┐◄────┘                 └──────────┐
 *      └─────┘            └─────┘                ┌──B──┐◄─────────┘
 * ```
 * (The lines are "routed": only horizontal and vertical pieces, called
 * elbow lines, so they never cut diagonally through other bars.)
 *
 * The two FS shapes (like MS Project):
 *
 * ```
 *  Case 1: room between the bars       Case 2: B starts too early
 *  ┌──A──┐──────┐                      ┌──A──┐─┐
 *  └─────┘      │                      └─────┘ │
 *               ▼                        ┌─────┘
 *             ┌──B──┐                    └─►┌──B──┐
 * ```
 */

/** A point in CSS pixels on the canvas. */
export interface Point {
  /** Horizontal position. */
  x: number;
  /** Vertical position. */
  y: number;
}

/** Which way the arrowhead points. */
export type ArrowDirection = "down" | "up" | "right" | "left";

/** A computed link: a polyline plus an arrowhead at its last point. */
export interface LinkPath {
  /** Corner points from the predecessor to the successor (at least 2). */
  points: Point[];
  /** Direction of the arrowhead at the last point. */
  arrow: ArrowDirection;
}

/** Horizontal distance the line runs out of the predecessor before turning. */
export const LINK_GAP = 8;

/** How far into the successor bar the vertical line lands (case 1). */
const LINK_INSET = 6;

/**
 * Computes the corner points of a finish-to-start link.
 *
 * @param fromX - x of the predecessor's finish (right end of bar A).
 * @param fromY - y of the middle of bar A.
 * @param toX - x of the successor's start (left end of bar B).
 * @param toY - y of the middle of bar B.
 * @param barHeight - Height of a bar; the vertical line stops at B's edge.
 * @param rowHeight - Height of a row; case 2 turns half-way between rows.
 * @returns The polyline and arrow direction. All coordinates are CSS pixels.
 *
 * @example
 * // A ends at x=100 on row y=10; B starts at x=200 on y=38 → case 1
 * fsLinkPath(100, 10, 200, 38, 14, 28).arrow === "down";
 */
export function fsLinkPath(
  fromX: number,
  fromY: number,
  toX: number,
  toY: number,
  barHeight: number,
  rowHeight: number,
): LinkPath {
  const below = toY >= fromY;
  const turnX = toX + LINK_INSET;

  // Case 1: B starts far enough to the right: go right, then straight down
  // (or up) onto B's top (or bottom) edge.
  if (turnX >= fromX + LINK_GAP && toY !== fromY) {
    const edgeY = below ? toY - barHeight / 2 : toY + barHeight / 2;
    return {
      points: [
        { x: fromX, y: fromY },
        { x: turnX, y: fromY },
        { x: turnX, y: edgeY },
      ],
      arrow: below ? "down" : "up",
    };
  }

  // Case 2: B starts before (or just after) A ends: step out to the right,
  // go to the gap between the rows, run left, and enter B from the left.
  const outX = fromX + LINK_GAP;
  const inX = toX - LINK_GAP;
  const midY = below ? toY - rowHeight / 2 : toY + rowHeight / 2;
  return {
    points: [
      { x: fromX, y: fromY },
      { x: outX, y: fromY },
      { x: outX, y: midY },
      { x: inX, y: midY },
      { x: inX, y: toY },
      { x: toX, y: toY },
    ],
    arrow: "right",
  };
}

/** The x positions of one bar's two ends, in CSS pixels. */
export interface BarEnds {
  /** Left end (the start). */
  startX: number;
  /** Right end (the finish). */
  finishX: number;
  /** y of the middle of the bar. */
  y: number;
}

/**
 * Computes the corner points of a link of any type.
 *
 * @param kind - The link type (see `DependencyType` in `ipc/types.ts`).
 * @param pred - The predecessor's ends (the task the link comes from).
 * @param succ - The successor's ends (the task the arrow points at).
 * @param barHeight - Height of a bar (FS lands on the bar's edge).
 * @param rowHeight - Height of a row (lines turn half-way between rows).
 * @returns The polyline and arrow direction, in CSS pixels.
 *
 * @example
 * const path = linkPath("startToStart",
 *   { startX: 100, finishX: 200, y: 10 }, { startX: 150, finishX: 250, y: 38 }, 14, 28);
 * path.arrow; // "right": SS arrows enter the successor's start from the left
 *
 * Beginner note: `switch (kind)` picks one `case` by the value of `kind`.
 * Because `kind` can only be one of four strings, TypeScript checks that
 * every case returns, so no link type can be forgotten.
 */
export function linkPath(
  kind: "finishToStart" | "startToStart" | "finishToFinish" | "startToFinish",
  pred: BarEnds,
  succ: BarEnds,
  barHeight: number,
  rowHeight: number,
): LinkPath {
  switch (kind) {
    case "finishToStart":
      return fsLinkPath(pred.finishX, pred.y, succ.startX, succ.y, barHeight, rowHeight);
    case "startToStart": {
      // Leave A's start to the left, run down beside both bars, enter B's start.
      const x = Math.min(pred.startX, succ.startX) - LINK_GAP;
      return {
        points: [
          { x: pred.startX, y: pred.y },
          { x, y: pred.y },
          { x, y: succ.y },
          { x: succ.startX, y: succ.y },
        ],
        arrow: "right",
      };
    }
    case "finishToFinish": {
      // Leave A's finish to the right, run down right of both, enter B's finish.
      const x = Math.max(pred.finishX, succ.finishX) + LINK_GAP;
      return {
        points: [
          { x: pred.finishX, y: pred.y },
          { x, y: pred.y },
          { x, y: succ.y },
          { x: succ.finishX, y: succ.y },
        ],
        arrow: "left",
      };
    }
    case "startToFinish": {
      const inX = succ.finishX + LINK_GAP;
      const outX = pred.startX - LINK_GAP;
      // Simple case: B finishes well left of A's start. Run left along A's
      // row, then down, then into B's finish.
      if (inX <= outX) {
        return {
          points: [
            { x: pred.startX, y: pred.y },
            { x: inX, y: pred.y },
            { x: inX, y: succ.y },
            { x: succ.finishX, y: succ.y },
          ],
          arrow: "left",
        };
      }
      // Otherwise: step out left of A, go to the gap between the rows, run
      // right past B's finish, and enter it from the right.
      const below = succ.y >= pred.y;
      const midY = below ? succ.y - rowHeight / 2 : succ.y + rowHeight / 2;
      return {
        points: [
          { x: pred.startX, y: pred.y },
          { x: outX, y: pred.y },
          { x: outX, y: midY },
          { x: inX, y: midY },
          { x: inX, y: succ.y },
          { x: succ.finishX, y: succ.y },
        ],
        arrow: "left",
      };
    }
  }
}
