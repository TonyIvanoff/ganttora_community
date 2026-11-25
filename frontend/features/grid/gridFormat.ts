/**
 * features/grid/gridFormat.ts: turns a grid row into the text its cells show.
 *
 * What it is for:
 *   The pure (non-React) half of the grid cells: the translation keys of
 *   constraint types and modes, the text of each cell ({@link cellText}),
 *   which editor a column uses ({@link editorKind}) and the text an editor
 *   starts with ({@link editorStartText}).
 *
 * Where it fits:
 *   Used by `GridCell.tsx` and `TaskGrid.tsx`; the keys also by the Task
 *   Information dialog. Kept apart from the components so React's "fast
 *   refresh" (instant reload while developing) works for `GridCell.tsx`.
 */

import { expectedPercent } from "@frontend/features/calendars/workingTime";
import { parseIsoLocal } from "@frontend/features/gantt/renderer/dates";
import type { TFunction } from "i18next";
import { formatScheduleDate } from "@frontend/i18n/format";
import type { Calendar, Constraint, TaskMode, TaskRow } from "@frontend/ipc/types";
import { minutesToDays } from "@frontend/lib/units";
import { dayOf, type GridColumnId } from "./cellEdit";
import type { GridRow } from "./gridRows";
import { formatPredecessors, type PredecessorContext } from "./predecessors";

/** Translation keys of the constraint types (typed, so a typo fails the build). */
export const CONSTRAINT_KEYS = {
  asSoonAsPossible: "constraint.type.asSoonAsPossible",
  asLateAsPossible: "constraint.type.asLateAsPossible",
  startNoEarlierThan: "constraint.type.startNoEarlierThan",
  startNoLaterThan: "constraint.type.startNoLaterThan",
  finishNoEarlierThan: "constraint.type.finishNoEarlierThan",
  finishNoLaterThan: "constraint.type.finishNoLaterThan",
  mustStartOn: "constraint.type.mustStartOn",
  mustFinishOn: "constraint.type.mustFinishOn",
} as const satisfies Record<Constraint["type"], string>;

/** Translation keys of the scheduling modes. */
export const MODE_KEYS = {
  auto: "grid.mode.auto",
  manual: "grid.mode.manual",
} as const satisfies Record<TaskMode, string>;

/** Everything a cell needs to turn a row into text. */
export interface CellFormat {
  /** Unit words, link codes, number format (from `useUnitContext`). */
  units: PredecessorContext;
  /** Date style (from the "Date format" setting). */
  dateLocale: string;
  /** Row number (1-based) of a task ID, for the Predecessors column. */
  rowNumberOf: (id: string) => number | undefined;
  /** All calendars, for the Task calendar column. */
  calendars: readonly Calendar[];
}

/**
 * The text a cell shows (not used for the Indicators column).
 *
 * @param column - The column.
 * @param row - The row.
 * @param format - See {@link CellFormat}.
 * @param t - The translate function.
 * @returns The text, or "" when there is nothing to show.
 *
 * @example
 * cellText("duration", row, format, t); // "5 days"
 */
export function cellText(
  column: GridColumnId,
  row: GridRow,
  format: CellFormat,
  t: TFunction,
): string {
  const task = row.task;
  switch (column) {
    case "rowNumber":
      return String(row.index + 1);
    case "outline":
      return row.outlineNumber;
    case "name":
      return row.name;
    case "duration":
      // A project without tasks has no duration.
      return task === null || (task.isProject === true && !task.isSummary)
        ? ""
        : t("units.days", { count: minutesToDays(task.durationMinutes, format.units.basis) });
    case "start":
      return formatScheduleDate(row.start, format.dateLocale) ?? "";
    case "finish":
      return formatScheduleDate(row.finish, format.dateLocale) ?? "";
    case "plannedFinish":
      return formatScheduleDate(task?.plannedFinish ?? null, format.dateLocale) ?? "";
    case "actualFinish":
      return formatScheduleDate(task?.actualFinish ?? null, format.dateLocale) ?? "";
    // A project has no constraint, deadline or task calendar of its own (its
    // start date is stored as a constraint, but shown as its start).
    case "constraintType":
      return task === null || task.isProject === true
        ? ""
        : t(CONSTRAINT_KEYS[task.constraint.type]);
    case "constraintDate":
      return task === null || task.isProject === true || !("date" in task.constraint)
        ? ""
        : (formatScheduleDate(task.constraint.date, format.dateLocale) ?? "");
    case "deadline":
      return task === null || task.isProject === true
        ? ""
        : (formatScheduleDate(task.deadline, format.dateLocale) ?? "");
    case "calendar":
      // Empty: the project calendar.
      return task === null || task.isProject === true
        ? ""
        : (format.calendars.find((c) => c.id === task.calendarId)?.name ?? "");
    case "complete":
      return task === null || (task.isProject === true && !task.isSummary)
        ? ""
        : t("grid.percent", { percent: task.percentComplete ?? 0 });
    case "predecessors":
      return task === null ? "" : formatPredecessors(task.links, format.rowNumberOf, format.units);
    case "mode":
      return task === null ? "" : t(MODE_KEYS[task.mode]);
    case "indicators":
      return "";
  }
}

/** The kinds of cell editors. */
export type EditorKind = "text" | "date" | "list" | "percent";

/** One choice of a list editor. */
export interface EditorOption {
  /** What is saved. */
  value: string;
  /** What the user sees. */
  label: string;
}

/**
 * The choices of a list editor.
 *
 * @param column - The column.
 * @param calendars - All calendars.
 * @param t - The translate function.
 * @returns The choices; none for columns without a list.
 *
 * @example
 * editorOptions("mode", [], t)[0].value; // "auto"
 */
export function editorOptions(
  column: GridColumnId,
  calendars: readonly Calendar[],
  t: TFunction,
): EditorOption[] {
  if (column === "mode") {
    return (["auto", "manual"] as const).map((m) => ({ value: m, label: t(MODE_KEYS[m]) }));
  }
  if (column === "constraintType") {
    return Object.entries(CONSTRAINT_KEYS).map(([value, key]) => ({ value, label: t(key) }));
  }
  if (column === "calendar") {
    return [
      { value: "", label: t("taskInfo.projectCalendar") },
      ...calendars.map((c) => ({ value: c.id, label: c.name })),
    ];
  }
  return [];
}

/**
 * Which editor a column uses.
 *
 * @param column - The column.
 * @returns "date" for the dates, "list" for the mode, constraint and task calendar, "percent" for % complete, else "text".
 */
export function editorKind(column: GridColumnId): EditorKind {
  switch (column) {
    case "start":
    case "finish":
    case "plannedFinish":
    case "actualFinish":
    case "constraintDate":
    case "deadline":
      return "date";
    case "mode":
    case "constraintType":
    case "calendar":
      return "list";
  }
  if (column === "complete") return "percent";
  return "text";
}

/**
 * The text an editor starts with.
 *
 * @param column - The column.
 * @param row - The row (must be a real task).
 * @param format - See {@link CellFormat}.
 * @param t - The translate function.
 * @returns For dates "YYYY-MM-DD", for lists the saved value, else the
 *   cell's text.
 */
export function editorStartText(
  column: GridColumnId,
  row: GridRow,
  format: CellFormat,
  t: TFunction,
): string {
  if (column === "start") return dayOf(row.start);
  if (column === "finish") return dayOf(row.finish);
  if (column === "actualFinish") return dayOf(row.task?.actualFinish ?? null);
  if (column === "plannedFinish") return dayOf(row.task?.plannedFinish ?? null);
  if (column === "deadline") return dayOf(row.task?.deadline ?? null);
  if (column === "constraintType") return row.task?.constraint.type ?? "asSoonAsPossible";
  if (column === "constraintDate") {
    const constraint = row.task?.constraint;
    return constraint !== undefined && "date" in constraint ? dayOf(constraint.date) : "";
  }
  if (column === "calendar") return row.task?.calendarId ?? "";
  if (column === "mode") return row.task?.mode ?? "auto";
  if (column === "complete") return String(row.task?.percentComplete ?? 0);
  return cellText(column, row, format, t);
}

/** Where the active cell goes after saving an edit. */
export type EditMove = "down" | "up" | "right" | "left" | "none";

/** The percentages to choose from. */
const PERCENT_CHOICES = [0, 10, 25, 50, 75, 90, 100];

/**
 * The percentages to offer, plus the current one if it is another number
 * (so that opening the list never changes a value).
 *
 * @param current - The current percentage as text.
 */
export function percentChoices(current: string): number[] {
  return [...new Set([...PERCENT_CHOICES, Number(current) || 0])].sort((a, b) => a - b);
}

/** A task is behind when it lacks at least this much progress (percent). */
export const BEHIND_TOLERANCE = 10;

/** What is wrong with a task's progress. */
export interface ProgressProblem {
  /**
   * `overdue`: its finish has passed. `behind`: less done than expected.
   * `milestone`: a milestone's date has passed and it is not reached.
   */
  kind: "overdue" | "behind" | "milestone";
  /** Progress now. */
  actual: number;
  /** Progress expected by today. */
  expected: number;
}

/**
 * How much of a task should be done by today.
 *
 * @param task - The row. Projects, summary tasks and milestones give `null`.
 * @param calendars - All calendars.
 * @param projectCalendarId - For tasks without their own calendar.
 * @param todayMs - Today at 00:00.
 * @returns 0–100 (100 once the finish day has passed), or `null` if unknown.
 *
 * @example
 * expectedProgress(task, calendars, snapshot.calendarId, localToday()); // 57
 */
export function expectedProgress(
  task: TaskRow,
  calendars: readonly Calendar[],
  projectCalendarId: string | null,
  todayMs: number,
): number | null {
  if (task.isProject === true || task.isSummary || task.isMilestone) return null;
  const startMs = parseIsoLocal(task.start ?? "");
  const finishMs = parseIsoLocal(task.finish ?? "");
  if (Number.isNaN(startMs) || Number.isNaN(finishMs)) return null;
  if (finishMs < todayMs) return 100;
  const id = task.calendarId ?? projectCalendarId;
  const calendar = calendars.find((c) => c.id === id);
  if (calendar === undefined) return null;
  const blocked = (task.segments ?? [])
    .filter((segment) => segment.kind.type === "blocked")
    .map((s) => ({ startMs: parseIsoLocal(s.start), finishMs: parseIsoLocal(s.finish) }));
  return expectedPercent(calendar, startMs, finishMs, todayMs, blocked);
}

/**
 * Compares a task's progress with its dates.
 *
 * @param task - The row. Projects and summary tasks have none.
 * @param calendars - All calendars.
 * @param projectCalendarId - For tasks without their own calendar.
 * @param todayMs - Today at 00:00.
 * @returns The problem, or `null`. A task at 0 % that is still before its
 *   finish has none here: "late to start" already says it.
 *
 * @example
 * progressProblem(task, calendars, snapshot.calendarId, localToday())?.expected; // 57
 */
export function progressProblem(
  task: TaskRow,
  calendars: readonly Calendar[],
  projectCalendarId: string | null,
  todayMs: number,
): ProgressProblem | null {
  const actual = task.percentComplete ?? 0;
  if (actual >= 100) return null;
  // A milestone is reached or not: only its date counts.
  if (task.isMilestone && !task.isSummary && task.isProject !== true) {
    const missed = parseIsoLocal(task.finish ?? "") < todayMs;
    return missed ? { kind: "milestone", actual, expected: 100 } : null;
  }
  const expected = expectedProgress(task, calendars, projectCalendarId, todayMs);
  if (expected === null) return null;
  if (parseIsoLocal(task.finish ?? "") < todayMs) return { kind: "overdue", actual, expected };
  if (actual === 0 || expected - actual < BEHIND_TOLERANCE) return null;
  return { kind: "behind", actual, expected };
}
