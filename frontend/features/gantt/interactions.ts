/**
 * features/gantt/interactions.ts: what dragging a bar means.
 *
 * What it is for:
 *   The Gantt chart lets the user change the plan with the mouse, like MS
 *   Project:
 *   - drag the **middle of a bar** left or right → the task moves;
 *   - drag the **right edge of a bar** → the task gets longer or shorter;
 *   - drag **from one bar up or down onto another bar** → a finish-to-start
 *     link is created between them.
 *   While dragging, a dashed "ghost" shows the result and a tooltip shows
 *   the new dates. Esc cancels. When the mouse is released, exactly **one
 *   command** is sent to Rust (decision D16), so one Undo takes the whole
 *   drag back.
 *
 *   This file holds the *decisions* as small pure functions: which part of
 *   a bar is under the mouse, which kind of drag a movement is, where the
 *   new dates snap to, and which command to send. `GanttView.tsx` connects
 *   them to the real mouse events and to Rust.
 *
 * Where it fits:
 *   - `GanttView.tsx` calls these functions from its pointer handlers.
 *   - `renderer/hitTest.ts` gives the bar geometry (the same as drawing).
 *   - `features/calendars/workingTime.ts` snaps dates to working days.
 *   - Rust (`ipc/client.ts`: `calendarWorkingMinutes`,
 *     `calendarAddWorkingTime`) does the calendar maths for durations.
 *
 * Main ideas:
 *   - **Moving an auto-scheduled task** cannot just "set its start": the
 *     engine calculates starts. MS Project solves this with a *constraint*:
 *     dragging sets "Start No Earlier Than <new date>". We do the same. A
 *     **manually scheduled** task simply gets the new start.
 *   - **Snapping**: the new start is the first working moment of the day
 *     under the mouse (e.g. 08:00, or the next Monday if it is a Saturday);
 *     the new finish is the last working moment of its day (e.g. 17:00).
 *   - **A threshold** of a few pixels separates a click (select) from a
 *     drag, so a slightly shaky click never moves a task.
 *   - Summary tasks cannot be moved or resized (their dates come from their
 *     subtasks), but links can start or end on them.
 */

import { snapToWorkingDay, isoDateTime } from "@frontend/features/calendars/workingTime";
import type { Calendar, Command, TaskRow, Uuid } from "@frontend/ipc/types";
import { DAY_MS, parseIsoLocal, startOfDay } from "./renderer/dates";
import { HIT_TOLERANCE, barBox, hitTestBar } from "./renderer/hitTest";
import { HEADER_HEIGHT, ROW_HEIGHT } from "./renderer/layout";
import type { GanttModel } from "./renderer/model";
import type { TimeScale } from "./renderer/timeScale";

/** How far (CSS px) the mouse must move before a press becomes a drag. */
export const DRAG_THRESHOLD = 4;

/** Width (CSS px) of the zone at a bar's right end that resizes it. */
export const RESIZE_ZONE = 6;

/** Which part of a bar the mouse is on. */
export type BarPart = "body" | "finishEdge";

/** The result of {@link hitTestPart}: a row and the part of its bar. */
export interface PartHit {
  /** Row index of the bar. */
  row: number;
  /** The part under the mouse. */
  part: BarPart;
}

/** The three kinds of drag. */
export type DragKind = "move" | "resize" | "link";

/**
 * Finds the bar and the part of it under the mouse.
 *
 * @param model - The prepared Gantt data.
 * @param scale - Date ↔ x mapping.
 * @param x - Mouse x in viewport CSS pixels (0 = left edge of the chart).
 * @param y - Mouse y in viewport CSS pixels (0 = top, above the header).
 * @param scrollLeft - Horizontal scroll position.
 * @param scrollTop - Vertical scroll position.
 * @returns The row and part, or `null` beside every bar. The right
 *   {@link RESIZE_ZONE} pixels of a normal bar (not a milestone, not a
 *   summary, at least 3 zones wide) are the `"finishEdge"`.
 *
 * @example
 * const hit = hitTestPart(model, scale, 120, 60, 0, 0);
 * if (hit?.part === "finishEdge") scroller.style.cursor = "ew-resize";
 */
export function hitTestPart(
  model: GanttModel,
  scale: TimeScale,
  x: number,
  y: number,
  scrollLeft: number,
  scrollTop: number,
): PartHit | null {
  const row = hitTestBar({ model, scale, x, y, scrollLeft, scrollTop });
  if (row === null) return null;
  const task = model.rows[row];
  if (task === undefined) return null;
  const box = barBox(task, row, scale, scrollLeft, scrollTop);
  if (box === null) return null;
  const right = box.x + box.width;
  const resizable = !box.isMilestone && !box.isSummary && box.width >= 3 * RESIZE_ZONE;
  // The edge zone reaches a little past the bar (the click tolerance), so
  // the edge is as easy to grab as the bar itself.
  const onEdge = resizable && x >= right - RESIZE_ZONE && x <= right + HIT_TOLERANCE;
  return { row, part: onEdge ? "finishEdge" : "body" };
}

/**
 * Decides what a movement since the mouse press means.
 *
 * @param part - The part of the bar that was pressed.
 * @param dx - Horizontal movement in CSS px since the press.
 * @param dy - Vertical movement in CSS px since the press.
 * @param canChangeDates - `false` for summary tasks (only links allowed).
 * @returns `null` while the movement is below {@link DRAG_THRESHOLD} (still
 *   a click); otherwise the kind of drag. Pressing the right edge always
 *   resizes; pressing the body moves when the movement is mostly
 *   sideways, and links when it is mostly up or down (as in MS Project).
 *
 * @example
 * classifyDrag("body", 30, 2, true);  // "move"
 * classifyDrag("body", 3, 25, true);  // "link"
 * classifyDrag("body", 2, 1, true);   // null (still a click)
 */
export function classifyDrag(
  part: BarPart,
  dx: number,
  dy: number,
  canChangeDates: boolean,
): DragKind | null {
  if (Math.max(Math.abs(dx), Math.abs(dy)) < DRAG_THRESHOLD) return null;
  if (part === "finishEdge" && canChangeDates) return "resize";
  if (Math.abs(dy) > Math.abs(dx)) return "link";
  return canChangeDates ? "move" : "link";
}

/**
 * Turns a horizontal movement into a whole number of days.
 *
 * @param dx - Movement in CSS px.
 * @param scale - The time scale (pixels per day).
 * @returns Days, rounded to the nearest whole day (negative = earlier).
 *
 * @example
 * daysFromPixels(60, { pxPerDay: 28 } as TimeScale); // 2
 */
export function daysFromPixels(dx: number, scale: Pick<TimeScale, "pxPerDay">): number {
  // `Math.round(-0.4)` gives `-0`; `+ 0` turns it into a plain 0.
  return Math.round(dx / scale.pxPerDay) + 0;
}

/**
 * Where a moved bar starts: the first working moment of the day the bar's
 * start was dragged to.
 *
 * @param startMs - The task's start before the drag (ms).
 * @param days - Whole days moved (from {@link daysFromPixels}).
 * @param calendar - The task's calendar, or `null` if unknown (then no
 *   working-day snapping: the day's midnight is used).
 * @returns The new start (ms). Moving onto a Saturday gives Monday's first
 *   working moment.
 *
 * @example
 * // Standard calendar, Monday 08:00 moved by 5 days → Saturday → Monday 08:00 a week later
 * snapMoveStart(Date.UTC(2027, 2, 1, 8), 5, standard); // Date.UTC(2027, 2, 8, 8)
 */
export function snapMoveStart(startMs: number, days: number, calendar: Calendar | null): number {
  const day = startOfDay(startMs) + days * DAY_MS;
  if (calendar === null) return day;
  const snapped = snapToWorkingDay(calendar, day, "start");
  return Number.isNaN(snapped) ? day : snapped;
}

/**
 * Where a resized bar ends: the last working moment of the day its right
 * edge was dragged to, never before the start's own day.
 *
 * @param startMs - The task's start (ms); the finish never goes before it.
 * @param finishMs - The task's finish before the drag (ms).
 * @param days - Whole days the edge moved.
 * @param calendar - The task's calendar, or `null` (no snapping: the end
 *   of the day is used).
 * @returns The new finish (ms).
 *
 * @example
 * // Mon 08:00 → Fri 17:00, edge dragged 1 day right → Saturday → back to Fri 17:00
 * snapResizeFinish(Date.UTC(2027, 2, 1, 8), Date.UTC(2027, 2, 5, 17), 1, standard);
 */
export function snapResizeFinish(
  startMs: number,
  finishMs: number,
  days: number,
  calendar: Calendar | null,
): number {
  // The finish 17:00 belongs to its own day; a finish at exactly midnight
  // belongs to the day before (hence "- 1 ms").
  const startDay = startOfDay(startMs);
  const day = Math.max(startDay, startOfDay(finishMs - 1) + days * DAY_MS);
  if (calendar === null) return day + DAY_MS;
  const snapped = snapToWorkingDay(calendar, day, "finish");
  // Snapping backwards may land before the start (e.g. a task starting on
  // Monday, dragged to Sunday): then the start's own working day ends it.
  if (Number.isNaN(snapped) || snapped <= startMs) {
    const sameDay = snapToWorkingDay(calendar, startDay, "start");
    const end = snapToWorkingDay(calendar, Number.isNaN(sameDay) ? startDay : sameDay, "finish");
    return Number.isNaN(end) || end <= startMs ? startMs : end;
  }
  return snapped;
}

/**
 * Builds the command for a moved bar.
 *
 * @param task - The task's row from the snapshot.
 * @param newStartMs - The snapped new start (from {@link snapMoveStart}).
 * @returns For an auto-scheduled task: "Start No Earlier Than" the new
 *   start (`setTaskConstraint`). For a manual task: the new manual start
 *   (`setTaskManualStart`). `null` if the start did not change.
 *
 * @example
 * moveCommand(row, Date.UTC(2027, 2, 8, 8));
 * // { type: "setTaskConstraint", id, constraint: { type: "startNoEarlierThan", date: "2027-03-08T08:00:00" } }
 */
export function moveCommand(task: TaskRow, newStartMs: number): Command | null {
  if (task.start !== null && parseIsoLocal(task.start) === newStartMs) return null;
  const date = isoDateTime(newStartMs);
  if (task.mode === "manual") return { type: "setTaskManualStart", id: task.id, start: date };
  return {
    type: "setTaskConstraint",
    id: task.id,
    constraint: { type: "startNoEarlierThan", date },
  };
}

/**
 * Builds the command for a resized bar.
 *
 * @param task - The task's row from the snapshot.
 * @param minutes - The new duration in working minutes (from Rust,
 *   `calendarWorkingMinutes(start, newFinish)`).
 * @returns `setTaskDuration`, or `null` if the duration did not change.
 *   Negative values are treated as 0.
 */
export function resizeCommand(task: TaskRow, minutes: number): Command | null {
  const clamped = Math.max(0, Math.round(minutes));
  if (clamped === task.durationMinutes) return null;
  return { type: "setTaskDuration", id: task.id, duration: { minutes: clamped } };
}

/**
 * Builds the command for a link dragged from one bar to another.
 *
 * @param predecessor - The task the drag started on.
 * @param successor - The task the mouse was released on.
 * @param newId - A fresh UUID for the link (entity IDs are UUIDs, never
 *   row numbers). The caller makes it with `crypto.randomUUID()`, so this
 *   function stays pure.
 * @returns An `addDependency` command (finish-to-start, no lag), or `null`
 *   when both are the same task. Rust refuses links that would create a
 *   loop or already exist; the error banner then explains why.
 */
export function linkCommand(predecessor: Uuid, successor: Uuid, newId: Uuid): Command | null {
  if (predecessor === successor) return null;
  return {
    type: "addDependency",
    dependency: {
      id: newId,
      predecessor,
      successor,
      kind: "finishToStart",
      lag: { minutes: 0 },
      lagPercent: null,
    },
  };
}

/**
 * Finds the row under a viewport y, for the link target (any part of the
 * row counts, so thin bars and diamonds are easy to hit).
 *
 * @param y - Mouse y in viewport CSS px.
 * @param scrollTop - Vertical scroll position.
 * @param rowCount - Number of rows.
 * @returns The row index, or `null` on the header or below the last row.
 */
export function rowAtY(y: number, scrollTop: number, rowCount: number): number | null {
  if (y < HEADER_HEIGHT) return null;
  const row = Math.floor((y - HEADER_HEIGHT + scrollTop) / ROW_HEIGHT);
  return row >= 0 && row < rowCount ? row : null;
}
