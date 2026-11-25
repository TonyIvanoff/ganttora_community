/**
 * renderer/hitTest.ts: where is each bar, and which bar is under the mouse?
 *
 * What it is for:
 *   A canvas is only pixels: when the user clicks it, the browser tells us
 *   the x and y of the click, not *what* was clicked. "Hit-testing" answers
 *   that question with arithmetic: from the click position, the scroll
 *   position and the time scale we work out which task bar is there.
 *
 *   The same file also says where a bar is drawn (`barBox`), so drawing
 *   (`draw.ts`) and clicking use exactly the same geometry. If they used two
 *   separate calculations, a click could miss a bar by a pixel.
 *
 * Where it fits:
 *   - `draw.ts` calls `barBox` to paint bars and milestone diamonds.
 *   - `GanttView.tsx` calls `hitTestBar` when the chart is clicked and puts
 *     the result in the store as the selected task, and `hitTestBlocked`
 *     while the mouse hovers, to show a blocked piece's tooltip.
 *
 * Main ideas:
 *   - **Pure functions**: no canvas, no DOM, no React. Numbers in, numbers
 *     out. That makes them easy to unit-test (tests/frontend/gantt).
 *   - **Fast**: the row is found with one division (rows have a fixed
 *     height), so the cost does not depend on the number of tasks.
 *   - Coordinates are *viewport* coordinates in CSS pixels: (0, 0) is the
 *     top-left corner of the visible chart area, header included.
 */

import { BAR_HEIGHT, HEADER_HEIGHT, ROW_HEIGHT, rowTop } from "./layout";
import type { GanttModel, GanttRow, GanttSegment } from "./model";
import { msToX, type TimeScale } from "./timeScale";

/** Smallest width of a normal bar, so very short tasks stay visible and clickable. */
export const MIN_BAR_WIDTH = 2;

/** Extra pixels around a bar that still count as a click on it (easier to hit). */
export const HIT_TOLERANCE = 3;

/** Where one bar is on screen, in viewport CSS pixels. */
export interface BarBox {
  /** Left edge of the bar (for a milestone: left corner of the diamond). */
  x: number;
  /** Top edge. */
  y: number;
  /** Width. */
  width: number;
  /** Height. */
  height: number;
  /**
   * `true` for a milestone (marked as one, or zero duration: start equals
   * finish). It is drawn as a diamond centred on its start instead of a bar
   * (on its finish when it has blockers: the blocked time comes first).
   */
  isMilestone: boolean;
  /**
   * `true` for a summary task. The box is the full bar height (so clicks
   * hit it easily), but `draw.ts` paints a thinner bracket shape inside it.
   */
  isSummary: boolean;
}

/**
 * Computes where a task's bar is drawn.
 *
 * @param task - The prepared row (dates in ms).
 * @param row - Its row index (0 = first task).
 * @param scale - Date ↔ x mapping.
 * @param scrollLeft - Horizontal scroll position (CSS px).
 * @param scrollTop - Vertical scroll position (CSS px).
 * @returns The box, or `null` if the task has no valid dates (no bar).
 *
 * @example
 * const box = barBox(model.rows[0], 0, scale, 0, 0);
 * // box.y === HEADER_HEIGHT + (ROW_HEIGHT - BAR_HEIGHT) / 2 for the first row
 */
export function barBox(
  task: GanttRow,
  row: number,
  scale: TimeScale,
  scrollLeft: number,
  scrollTop: number,
): BarBox | null {
  if (Number.isNaN(task.startMs) || Number.isNaN(task.finishMs)) return null;
  // A project without tasks has nothing to span.
  if (task.isProject && !task.isSummary) return null;
  const top = rowTop(row, scrollTop) + (ROW_HEIGHT - BAR_HEIGHT) / 2;
  const startX = msToX(scale, task.startMs) - scrollLeft;

  // A summary is never drawn as a diamond: its dates span its subtasks.
  if (!task.isSummary && (task.isMilestone || task.finishMs === task.startMs)) {
    // Milestone: a diamond as tall as a bar, centred on the start. With
    // blockers the blocked pieces are drawn first and the diamond sits at
    // the finish, where the milestone is now reached.
    const half = BAR_HEIGHT / 2;
    const centreX = task.segments === null ? startX : msToX(scale, task.finishMs) - scrollLeft;
    return {
      x: centreX - half,
      y: top,
      width: BAR_HEIGHT,
      height: BAR_HEIGHT,
      isMilestone: true,
      isSummary: false,
    };
  }
  const width = Math.max(MIN_BAR_WIDTH, msToX(scale, task.finishMs) - scrollLeft - startX);
  return {
    x: startX,
    y: top,
    width,
    height: BAR_HEIGHT,
    isMilestone: false,
    isSummary: task.isSummary,
  };
}

/** Input for {@link hitTestBar}. */
export interface HitTestInput {
  /** The prepared Gantt data. */
  model: GanttModel;
  /** Date ↔ x mapping. */
  scale: TimeScale;
  /** Click x in viewport CSS pixels (0 = left edge of the chart area). */
  x: number;
  /** Click y in viewport CSS pixels (0 = top edge, above the header). */
  y: number;
  /** Horizontal scroll position (CSS px). */
  scrollLeft: number;
  /** Vertical scroll position (CSS px). */
  scrollTop: number;
}

/**
 * Finds the task bar at a point.
 *
 * @param input - See {@link HitTestInput}.
 * @returns The row index of the bar under the point, or `null` when the
 *   point is on the header, below the last row, or beside every bar.
 *
 * How it works:
 *   1. Points on the header never hit a bar.
 *   2. The row is `floor((y − header + scrollTop) / rowHeight)`.
 *   3. Anywhere in that row's height counts, as long as x is within the
 *      bar's left and right edge, widened by {@link HIT_TOLERANCE} pixels
 *      on each side. (Using the whole row height makes thin bars easier to
 *      click.)
 *
 * @example
 * const row = hitTestBar({ model, scale, x: 120, y: 60, scrollLeft: 0, scrollTop: 0 });
 * if (row !== null) select(model.rows[row].id);
 */
export function hitTestBar(input: HitTestInput): number | null {
  const { model, scale, x, y, scrollLeft, scrollTop } = input;
  if (y < HEADER_HEIGHT) return null;

  const row = Math.floor((y - HEADER_HEIGHT + scrollTop) / ROW_HEIGHT);
  const task = model.rows[row];
  // `row` can be past the end (click below the last task): no task there.
  if (row < 0 || task === undefined) return null;

  const box = barBox(task, row, scale, scrollLeft, scrollTop);
  if (box === null) return null;
  let left = box.x - HIT_TOLERANCE;
  let right = box.x + box.width + HIT_TOLERANCE;
  if (task.segments !== null) {
    // A milestone with blockers: its diamond is at the finish, but the
    // blocked pieces before it belong to it too.
    left = Math.min(left, msToX(scale, task.startMs) - scrollLeft - HIT_TOLERANCE);
    right = Math.max(right, msToX(scale, task.finishMs) - scrollLeft + HIT_TOLERANCE);
  }
  return x >= left && x <= right ? row : null;
}

/** The result of {@link hitTestBlocked}: the row and the blocked piece. */
export interface BlockedHit {
  /** Row index of the task. */
  row: number;
  /** The blocked piece under the point (its `blocker` is never `null`). */
  segment: GanttSegment;
}

/**
 * Finds the blocked piece of a bar at a point, for the
 * hover tooltip.
 *
 * @param input - See {@link HitTestInput}.
 * @returns The row and the blocked piece under the point, or `null` when
 *   the point is not on a blocked piece (also for every task without
 *   blockers: they are skipped after one check).
 *
 * @example
 * const hit = hitTestBlocked({ model, scale, x: 300, y: 90, scrollLeft: 0, scrollTop: 0 });
 * hit?.segment.blocker?.reason; // "Wood delivery delayed"
 */
export function hitTestBlocked(input: HitTestInput): BlockedHit | null {
  const { model, scale, x, y, scrollLeft, scrollTop } = input;
  if (y < HEADER_HEIGHT) return null;
  const row = Math.floor((y - HEADER_HEIGHT + scrollTop) / ROW_HEIGHT);
  const task = model.rows[row];
  if (row < 0 || task === undefined || task.segments === null) return null;
  // Only the bar's own height counts here (not the whole row), so the
  // tooltip appears when the mouse is really on the striped piece.
  const top = rowTop(row, scrollTop) + (ROW_HEIGHT - BAR_HEIGHT) / 2;
  if (y < top || y > top + BAR_HEIGHT) return null;
  for (const segment of task.segments) {
    if (segment.blocker === null) continue;
    const left = msToX(scale, segment.startMs) - scrollLeft;
    // At least MIN_BAR_WIDTH wide, like a bar, so tiny pieces can be found.
    const right = Math.max(left + MIN_BAR_WIDTH, msToX(scale, segment.finishMs) - scrollLeft);
    if (x >= left && x <= right) return { row, segment };
  }
  return null;
}

/** Size of a deadline arrow in CSS pixels. */
export const DEADLINE_SIZE = { width: 14, height: 20 };

/**
 * Where a task's deadline arrow is drawn: in the middle of its row, centred
 * on the deadline.
 *
 * @param task - The prepared row.
 * @param row - Its row index.
 * @param scale - Date ↔ x mapping.
 * @param scrollLeft - Horizontal scroll position (CSS px).
 * @param scrollTop - Vertical scroll position (CSS px).
 * @returns The box, or `null` without a deadline.
 */
export function deadlineBox(
  task: Pick<GanttRow, "deadlineMs">,
  row: number,
  scale: TimeScale,
  scrollLeft: number,
  scrollTop: number,
): { x: number; y: number; width: number; height: number } | null {
  if (Number.isNaN(task.deadlineMs)) return null;
  const { width, height } = DEADLINE_SIZE;
  return {
    x: msToX(scale, task.deadlineMs) - scrollLeft - width / 2,
    y: rowTop(row, scrollTop) + (ROW_HEIGHT - height) / 2,
    width,
    height,
  };
}

/**
 * Finds the deadline arrow at a point, for the hover tooltip.
 *
 * @param input - See {@link HitTestInput}.
 * @returns The row whose deadline it is, or `null`.
 *
 * @example
 * hitTestDeadline({ model, scale, x: 300, y: 90, scrollLeft: 0, scrollTop: 0 }); // 1
 */
export function hitTestDeadline(input: HitTestInput): number | null {
  const { model, scale, x, y, scrollLeft, scrollTop } = input;
  if (y < HEADER_HEIGHT) return null;
  const row = Math.floor((y - HEADER_HEIGHT + scrollTop) / ROW_HEIGHT);
  const task = model.rows[row];
  if (task === undefined) return null;
  const box = deadlineBox(task, row, scale, scrollLeft, scrollTop);
  return box !== null && x >= box.x && x <= box.x + box.width ? row : null;
}

/** Size of a "late to start" arrow in CSS pixels. */
export const LATE_START_SIZE = { width: 20, height: 14 };

/** Space between the arrow's tip and the bar, so a link's arrowhead and a risk border stay visible. */
const LATE_START_GAP = 16;

/**
 * Tells whether a task should have started on an earlier day but is still
 * at 0 %. Projects and summary tasks never are (their tasks show it), nor
 * milestones (see {@link isMissedMilestone}).
 *
 * @param task - Its start (ms), progress and kind.
 * @param todayMs - Today at 00:00 (see `localToday`).
 * @returns `true` if it is late to start. A task that starts today is not.
 *
 * @example
 * isLateToStart(row, localToday());
 */
export function isLateToStart(
  task: {
    startMs: number;
    percentComplete: number;
    isSummary: boolean;
    isProject: boolean;
    isMilestone: boolean;
  },
  todayMs: number,
): boolean {
  if (task.isSummary || task.isProject || task.isMilestone) return false;
  if (task.percentComplete > 0) return false;
  return task.startMs < todayMs; // `false` for NaN (no date)
}

/**
 * Tells whether a milestone's date has passed without it being reached.
 *
 * @param task - The prepared row.
 * @param todayMs - Today at 00:00.
 * @returns `true` if it is missed. One that is due today is not.
 *
 * Uses the *real* finish: the drawn one of 17:00 is the next midnight.
 */
export function isMissedMilestone(
  task: Pick<
    GanttRow,
    "realFinishMs" | "percentComplete" | "isMilestone" | "isSummary" | "isProject"
  >,
  todayMs: number,
): boolean {
  if (!task.isMilestone || task.isSummary || task.isProject) return false;
  return task.percentComplete < 100 && task.realFinishMs < todayMs;
}

/**
 * Where a task's "late to start" arrow is drawn: left of its bar, pointing
 * right at its start.
 *
 * @param task - The prepared row.
 * @param row - Its row index.
 * @param scale - Date ↔ x mapping.
 * @param scrollLeft - Horizontal scroll position (CSS px).
 * @param scrollTop - Vertical scroll position (CSS px).
 * @param todayMs - Today at 00:00, or `undefined` if it is not known.
 * @returns The box, or `null` when the task is not late to start.
 */
export function lateStartBox(
  task: GanttRow,
  row: number,
  scale: TimeScale,
  scrollLeft: number,
  scrollTop: number,
  todayMs: number | undefined,
): { x: number; y: number; width: number; height: number } | null {
  if (todayMs === undefined || !isLateToStart(task, todayMs)) return null;
  const bar = barBox(task, row, scale, scrollLeft, scrollTop);
  if (bar === null) return null;
  const { width, height } = LATE_START_SIZE;
  // With blockers a milestone's box is at its end; the start is further left.
  const left = Math.min(bar.x, msToX(scale, task.startMs) - scrollLeft);
  return {
    x: left - LATE_START_GAP - width,
    y: rowTop(row, scrollTop) + (ROW_HEIGHT - height) / 2,
    width,
    height,
  };
}

/**
 * Finds the "late to start" arrow at a point, for the hover tooltip.
 *
 * @param input - See {@link HitTestInput}.
 * @param todayMs - Today at 00:00.
 * @returns The row it belongs to, or `null`.
 *
 * @example
 * hitTestLateStart({ model, scale, x: 300, y: 90, scrollLeft: 0, scrollTop: 0 }, today); // 1
 */
export function hitTestLateStart(input: HitTestInput, todayMs: number): number | null {
  const { model, scale, x, y, scrollLeft, scrollTop } = input;
  if (y < HEADER_HEIGHT) return null;
  const row = Math.floor((y - HEADER_HEIGHT + scrollTop) / ROW_HEIGHT);
  const task = model.rows[row];
  if (task === undefined) return null;
  const box = lateStartBox(task, row, scale, scrollLeft, scrollTop, todayMs);
  return box !== null && x >= box.x && x <= box.x + box.width ? row : null;
}

/** Size of a finish arrow in CSS pixels: its head, its shaft, its shortest length. */
export const FINISH_ARROW = { headLength: 8, headHeight: 10, shaftHeight: 4, minLength: 14 };

/**
 * Where a task's finish arrow is drawn: along the lower edge of its row,
 * from the planned finish to the actual (or calculated) finish.
 *
 * @param task - The prepared row.
 * @param row - Its row index.
 * @param scale - Date ↔ x mapping.
 * @param scrollLeft - Horizontal scroll position (CSS px).
 * @param scrollTop - Vertical scroll position (CSS px).
 * @returns Its box (it points right when `late`), or `null` without one.
 */
export function finishArrowBox(
  task: Pick<GanttRow, "variance">,
  row: number,
  scale: TimeScale,
  scrollLeft: number,
  scrollTop: number,
): { x: number; y: number; width: number; height: number } | null {
  const v = task.variance;
  if (v === null) return null;
  const tip = msToX(scale, v.toMs) - scrollLeft;
  const width = Math.max(
    FINISH_ARROW.minLength,
    Math.abs(tip - (msToX(scale, v.fromMs) - scrollLeft)),
  );
  return {
    x: v.late ? tip - width : tip,
    y: rowTop(row, scrollTop) + ROW_HEIGHT - FINISH_ARROW.headHeight,
    width,
    height: FINISH_ARROW.headHeight,
  };
}

/**
 * Finds the finish arrow at a point, for the hover tooltip.
 *
 * @param input - See {@link HitTestInput}.
 * @returns The row it belongs to, or `null`.
 *
 * @example
 * hitTestFinishArrow({ model, scale, x: 300, y: 90, scrollLeft: 0, scrollTop: 0 }); // 1
 */
export function hitTestFinishArrow(input: HitTestInput): number | null {
  const { model, scale, x, y, scrollLeft, scrollTop } = input;
  if (y < HEADER_HEIGHT) return null;
  const row = Math.floor((y - HEADER_HEIGHT + scrollTop) / ROW_HEIGHT);
  const task = model.rows[row];
  if (task === undefined) return null;
  const box = finishArrowBox(task, row, scale, scrollLeft, scrollTop);
  return box !== null && x >= box.x && x <= box.x + box.width ? row : null;
}
