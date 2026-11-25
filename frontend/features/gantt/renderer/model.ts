/**
 * renderer/model.ts: turns schedule tasks into a form that is fast to draw.
 *
 * What it is for:
 *   The schedule arrives with dates as text and links as ids. Drawing needs
 *   numbers and row positions. Converting 10,000 dates on *every* frame would
 *   waste time, so we convert once, when the schedule changes, and keep the
 *   result: the *Gantt model*.
 *
 * Where it fits:
 *   `GanttView.tsx` calls `prepareGanttModel` whenever the task list changes
 *   (wrapped in `useMemo`). The renderer (`draw.ts`) reads the model.
 *
 * Main ideas:
 *   - A `Map` from id to row number lets us find each predecessor's row in
 *     one step instead of searching the whole list ("O(1) lookup").
 *   - `maxLinkRowSpan` remembers the longest link, measured in rows. The
 *     renderer needs it to know how far above and below the screen it must
 *     look for links that cross the visible area.
 *   - **Blockers**: a task with blockers brings its bar
 *     *pieces* (`segments`): work, blocked, work… Only those tasks get a
 *     `segments` list; for every other task it is `null`, so the renderer
 *     can keep its fast one-rectangle path for them.
 *   - Pure function: no drawing, no React; easy to test.
 */

import type { Constraint, DependencyType, RiskPriority } from "@frontend/ipc/types";
import { parseIsoLocal } from "./dates";
import { noDayFill, type DayFill } from "./dayFill";

/** One incoming link of a source task: where it comes from and its type. */
export interface GanttSourceLink {
  /** ID of the predecessor task. */
  predecessor: string;
  /** Link type: finish-to-start, start-to-start, finish-to-finish, start-to-finish. */
  kind: DependencyType;
}

/**
 * What the chart knows about the blocker behind a blocked piece of a bar:
 * enough for the hover tooltip (reason, length, reported day).
 */
export interface GanttBlockerInfo {
  /** The blocker's UUID. */
  id: string;
  /** Why the task is blocked (the user's text). */
  reason: string;
  /** Blocked time in working minutes. */
  durationMinutes: number;
  /** The day it was reported ("YYYY-MM-DD"), or `null`. */
  reportedOn: string | null;
}

/** One piece of a source task's bar (see `fromSnapshot.ts`). */
export interface GanttSourceSegment {
  /** ISO start of the piece. */
  start: string;
  /** ISO finish of the piece. */
  finish: string;
  /** The blocker for a blocked piece, `null` for a work piece. */
  blocker: GanttBlockerInfo | null;
}

/** One piece of a bar, ready to draw. */
export interface GanttSegment {
  /** Start in milliseconds. */
  startMs: number;
  /** Finish in milliseconds. */
  finishMs: number;
  /** The blocker for a blocked piece (drawn in the blocker colour), `null` for work. */
  blocker: GanttBlockerInfo | null;
}

/**
 * The minimum a task needs to appear in the Gantt chart. Both the mock data
 * and the schedule from Rust (`fromSnapshot.ts`) have at least these fields.
 */
export interface GanttSourceTask {
  /** Unique id (UUID text). */
  id: string;
  /** Display name. */
  name: string;
  /** ISO start date-time without time zone. */
  start: string;
  /** ISO finish date-time without time zone. */
  finish: string;
  /** On the critical path? */
  isCritical: boolean;
  /**
   * Ids of predecessor tasks. Used for the links when {@link links} is
   * missing (the mock data); such links are drawn finish-to-start.
   */
  predecessors: readonly string[];
  /**
   * The incoming links with their type. Optional: when present, the links
   * are drawn from this list (all four types) instead of `predecessors`.
   */
  links?: readonly GanttSourceLink[];
  /** Manually scheduled task: drawn in its own colour. Missing = auto. */
  isManual?: boolean;
  /** Deadline (ISO text) or `null`/missing: drawn as a small arrow marker. */
  deadline?: string | null;
  /** The task finishes after its deadline: the marker is drawn in red. */
  missedDeadline?: boolean;
  /**
   * The task's date rule. Rules with a date (SNET, MSO, FNLT…) get a small
   * pin at that date. Missing = "as soon as possible" (no pin).
   */
  constraint?: Constraint;
  /**
   * Summary task (has subtasks): drawn as a thin bracket bar. Optional
   * because the mock data has no outline; missing means `false`.
   *
   * Beginner note: `?:` marks an *optional* property: the object may leave
   * it out entirely.
   */
  isSummary?: boolean;
  /**
   * Indent level, 1 = top level. Level 1 is a *task*, deeper levels are
   * *sub-tasks* (drawn in cyan). Missing means level 1.
   */
  outlineLevel?: number;
  /**
   * Drawn as a diamond even if start and finish differ (a task marked as a
   * milestone). Missing means: a diamond only for zero duration.
   */
  isMilestone?: boolean;
  /**
   * The bar's pieces when the task has blockers, in time
   * order. Missing or empty: the bar is one piece from `start` to `finish`.
   */
  segments?: readonly GanttSourceSegment[];
  /** The task's own calendar, or `null`/missing for the project's. */
  calendarId?: string | null;
  /** How much is done, 0–100. Missing means 0. */
  percentComplete?: number;
  /** A project row (top level, holds tasks). Missing means `false`. */
  isProject?: boolean;
  /** The highest priority of its risks; missing or `null` = no risk. */
  riskPriority?: RiskPriority | null;
  /** The finish saved by "Set plan", if any. */
  plannedFinish?: string | null;
  /** When the work really finished, if known. */
  actualFinish?: string | null;
}

/** How a task's finish differs from its plan. */
export interface FinishVariance {
  /** Where the arrow starts: the planned finish, as drawn (ms). */
  fromMs: number;
  /** Where it points: the actual or calculated finish, as drawn (ms). */
  toMs: number;
  /** The real planned finish (ms), for texts. */
  plannedMs: number;
  /** The real actual or calculated finish (ms), for texts. */
  finishMs: number;
  /** After the plan (`true`) or before it (`false`). */
  late: boolean;
  /** Finished: compared with the actual finish. Else a forecast. */
  done: boolean;
}

/**
 * Compares a task's finish with its planned finish, by day.
 *
 * @param task - The source task.
 * @param drawn - Turns a date-time into the place where it is drawn (ms).
 * @returns The difference, or `null`: no plan, on the planned day, a group
 *   or project, or done without a known actual finish.
 */
function finishVariance(
  task: GanttSourceTask,
  drawn: (iso: string) => number,
): FinishVariance | null {
  if (task.isSummary === true || task.isProject === true || task.plannedFinish == null) return null;
  const done = (task.percentComplete ?? 0) >= 100;
  const finish = done ? task.actualFinish : task.finish;
  if (finish == null || finish.slice(0, 10) === task.plannedFinish.slice(0, 10)) return null;
  const plannedMs = parseIsoLocal(task.plannedFinish);
  const finishMs = parseIsoLocal(finish);
  if (Number.isNaN(plannedMs) || Number.isNaN(finishMs)) return null;
  return {
    fromMs: drawn(task.plannedFinish),
    toMs: drawn(finish),
    plannedMs,
    finishMs,
    late: finishMs > plannedMs,
    done,
  };
}

/** One incoming link of a row, ready to draw. */
export interface GanttLinkIn {
  /** Row number of the predecessor. */
  row: number;
  /** Link type. */
  kind: DependencyType;
}

/** One row of the chart, ready to draw. */
export interface GanttRow {
  /** Task id: a click on this row selects this task (see `hitTest.ts`). */
  id: string;
  /** Task name, shown in the hover tooltip. */
  name: string;
  /** Where the bar is drawn from (ms): full days fill their cell (dayFill.ts). */
  startMs: number;
  /** Where the bar is drawn to (ms). */
  finishMs: number;
  /** The real start (ms, UTC math, see dates.ts), for drags. */
  realStartMs: number;
  /** The real finish (ms). */
  realFinishMs: number;
  /** The task's own calendar, or `null` for the project's. */
  calendarId: string | null;
  /** How much is done, 0–100. */
  percentComplete: number;
  /** A project row. */
  isProject: boolean;
  /** The highest priority of its risks (border colour), or `null`. */
  riskPriority: RiskPriority | null;
  /** Which project it belongs to (0 = first), or `null` without projects. */
  projectIndex: number | null;
  /** On the critical path? (Not drawn; the Indicators column shows it.) */
  isCritical: boolean;
  /** Row numbers of this task's predecessors (links end at this row). */
  predecessorRows: number[];
  /** The same links with their type, in the same order as `predecessorRows`. */
  links: GanttLinkIn[];
  /** Manually scheduled (drawn in the manual-task colour). */
  isManual: boolean;
  /** Deadline in ms, or `NaN` if the task has none. */
  deadlineMs: number;
  /** Finishes after its deadline (deadline marker in red). */
  missedDeadline: boolean;
  /** Finishes on another day than planned (an arrow), or `null`. */
  variance: FinishVariance | null;
  /** Date of a dated constraint in ms, or `NaN` for ASAP / ALAP. */
  constraintMs: number;
  /** Summary task: drawn as a thin bracket bar (see `draw.ts`). */
  isSummary: boolean;
  /** Sub-task (inside another task): drawn in the lighter blue. */
  isSubtask: boolean;
  /**
   * Marked as a milestone: drawn as a diamond at its start (or, when it
   * has blockers, after the blocked time, at its finish).
   */
  isMilestone: boolean;
  /**
   * The bar's pieces (work and blocked) when the task has blockers, or
   * `null` for the usual one-piece bar. `null` (not an empty list) keeps
   * the check in the drawing loop as cheap as possible.
   */
  segments: readonly GanttSegment[] | null;
}

/** Everything the renderer needs about the data, prepared once. */
export interface GanttModel {
  /** Rows in display order: row 0 is drawn at the top. */
  rows: GanttRow[];
  /** Earliest start of all tasks (ms), or `NaN` if there are no tasks. */
  minMs: number;
  /** Latest finish of all tasks (ms), or `NaN` if there are no tasks. */
  maxMs: number;
  /** Largest distance, in rows, between a task and one of its predecessors. */
  maxLinkRowSpan: number;
  /**
   * How many rows are critical. (The status bar counts the store's tasks
   * itself, in `shell/AppShell.tsx`; this number is for the renderer's
   * callers and tests.)
   */
  criticalCount: number;
}

/**
 * Builds the Gantt model from schedule tasks.
 *
 * @param tasks - Tasks in display order.
 * @param fill - Where a moment is drawn (`dayFill.ts`); default: as it is.
 * @returns The prepared model. Tasks with unreadable dates are kept (so row
 *   numbers stay aligned with the task list) but get `NaN` dates; the
 *   renderer skips drawing a bar for them. Predecessor ids that are not in
 *   the list are ignored.
 *
 * @example
 * const model = prepareGanttModel(tasks);
 * model.rows[0].startMs; // a number
 *
 * Beginner note: `readonly T[]` means "an array we promise not to change".
 * TypeScript then refuses code that would push to or modify it.
 */
export function prepareGanttModel(
  tasks: readonly GanttSourceTask[],
  fill: DayFill = noDayFill,
): GanttModel {
  // With project rows, level 1 is a project, 2 a task, 3 and deeper a sub-task.
  const taskLevel = tasks.some((task) => task.isProject === true) ? 2 : 1;
  // First pass: id → row number.
  const rowById = new Map<string, number>();
  tasks.forEach((task, row) => rowById.set(task.id, row));

  let minMs = Number.POSITIVE_INFINITY;
  let maxMs = Number.NEGATIVE_INFINITY;
  let maxLinkRowSpan = 0;
  let criticalCount = 0;

  let projectIndex: number | null = null;

  // Second pass: convert dates, resolve links, collect totals.
  const rows: GanttRow[] = tasks.map((task, row) => {
    if (task.isProject === true) projectIndex = (projectIndex ?? -1) + 1;
    const calendarId = task.calendarId ?? null;
    /** Reads a date and gives the moment it is drawn at. */
    const drawn = (iso: string): number => fill(parseIsoLocal(iso), calendarId);
    const realStartMs = parseIsoLocal(task.start);
    const realFinishMs = parseIsoLocal(task.finish);
    const startMs = fill(realStartMs, calendarId);
    const finishMs = fill(realFinishMs, calendarId);
    // Comparisons with NaN are always false, so bad dates do not affect min/max.
    if (startMs < minMs) minMs = startMs;
    if (finishMs > maxMs) maxMs = finishMs;
    if (task.isCritical) criticalCount++;

    // Links with a type if we have them, otherwise plain predecessors
    // drawn as finish-to-start (the mock data has no link types).
    const sourceLinks: readonly GanttSourceLink[] =
      task.links ?? task.predecessors.map((id) => ({ predecessor: id, kind: "finishToStart" }));
    const predecessorRows: number[] = [];
    const links: GanttLinkIn[] = [];
    for (const link of sourceLinks) {
      const predRow = rowById.get(link.predecessor);
      if (predRow === undefined) continue;
      predecessorRows.push(predRow);
      links.push({ row: predRow, kind: link.kind });
      maxLinkRowSpan = Math.max(maxLinkRowSpan, Math.abs(row - predRow));
    }

    // A deadline right of every bar must still be reachable by scrolling.
    const deadlineMs = task.deadline == null ? Number.NaN : drawn(task.deadline);
    if (deadlineMs > maxMs) maxMs = deadlineMs;
    const variance = finishVariance(task, drawn);
    if (variance !== null) maxMs = Math.max(maxMs, variance.fromMs, variance.toMs);
    // `"date" in c` is true only for the six dated constraint kinds.
    const c = task.constraint;
    const constraintMs = c !== undefined && "date" in c ? drawn(c.date) : Number.NaN;
    // Bar pieces only for tasks with blockers (most tasks have none).
    const segments =
      task.segments === undefined || task.segments.length === 0
        ? null
        : task.segments.map((seg) => ({
            startMs: drawn(seg.start),
            finishMs: drawn(seg.finish),
            blocker: seg.blocker,
          }));

    return {
      id: task.id,
      name: task.name,
      startMs,
      finishMs,
      realStartMs,
      realFinishMs,
      calendarId,
      percentComplete: task.percentComplete ?? 0,
      isProject: task.isProject === true,
      riskPriority: task.riskPriority ?? null,
      projectIndex,
      isCritical: task.isCritical,
      predecessorRows,
      links,
      isManual: task.isManual === true,
      deadlineMs,
      missedDeadline: task.missedDeadline === true,
      variance,
      constraintMs,
      // `=== true` turns "missing" (undefined) into false.
      isSummary: task.isSummary === true,
      isSubtask: (task.outlineLevel ?? 1) > taskLevel,
      isMilestone: task.isMilestone === true,
      segments,
    };
  });

  const hasDates = Number.isFinite(minMs) && Number.isFinite(maxMs);
  return {
    rows,
    minMs: hasDates ? minMs : Number.NaN,
    maxMs: hasDates ? maxMs : Number.NaN,
    maxLinkRowSpan,
    criticalCount,
  };
}
