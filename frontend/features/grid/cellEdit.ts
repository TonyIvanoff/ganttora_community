/**
 * features/grid/cellEdit.ts: turns a value typed into a grid cell into
 * document commands.
 *
 * What it is for:
 *   When the user finishes editing a cell (Enter, Tab, or clicking away),
 *   the grid calls {@link commandsForCellEdit}. It reads the text, checks
 *   it, and answers with the commands to send to Rust, or with the key of
 *   an error message to show next to the cell.
 *
 * Where it fits:
 *   Called by `TaskGrid.tsx`. The commands go through the store
 *   (`applyCommands` in `state/store.ts`) to Rust (`project_apply`).
 *
 * Main ideas:
 *   - **Which cells can be edited** is decided by {@link isEditable}, the
 *     same rules as MS Project: a summary task's duration, start and finish
 *     come from its subtasks, so they are read-only.
 *   - **Typing a start date on an automatic task** sets a "Start No Earlier
 *     Than" constraint, and typing a finish date sets "Finish No Earlier
 *     Than", exactly as MS Project does. On a manual task the start is
 *     simply stored (`setTaskManualStart`). The *dates* are still calculated
 *     by Rust (decision D16).
 *   - **Dates are typed in a date field** (`DateField.tsx`, which reports
 *     "YYYY-MM-DD"). The time of day is kept from the old value, so a
 *     task that starts at 08:00 still starts at 08:00 on the new day. See
 *     {@link withDate}.
 *   - Pure function: text in, commands out. Easy to test.
 */

import type { Command, Constraint, IsoDateTime, TaskMode, TaskRow } from "@frontend/ipc/types";
import { localToday } from "@frontend/features/gantt/renderer/dates";
import { parseDuration } from "@frontend/lib/units";
import { linkCommands, parsePredecessors, type PredecessorContext } from "./predecessors";

/**
 * The grid's columns, in display order.
 *
 * `rowNumber` is the ID column (the row number, as MS Project shows it);
 * `outline` is the WBS outline number ("1.2.3"); `indicators` shows a
 * symbol for what the row is, whether it is critical, blockers, risks…
 */
export const GRID_COLUMN_IDS = [
  "rowNumber",
  "indicators",
  "outline",
  "name",
  "duration",
  "complete",
  "start",
  "finish",
  "plannedFinish",
  "actualFinish",
  "constraintType",
  "constraintDate",
  "deadline",
  "calendar",
  "predecessors",
  "mode",
] as const;

/** One of {@link GRID_COLUMN_IDS}. */
export type GridColumnId = (typeof GRID_COLUMN_IDS)[number];

/** The columns that can ever be edited in the grid. */
export const EDITABLE_COLUMNS: ReadonlySet<GridColumnId> = new Set<GridColumnId>([
  "name",
  "duration",
  "start",
  "finish",
  "plannedFinish",
  "actualFinish",
  "constraintType",
  "constraintDate",
  "deadline",
  "calendar",
  "complete",
  "predecessors",
  "mode",
]);

/**
 * Tells whether a cell may be edited.
 *
 * @param column - The column.
 * @param task - The task of the row.
 * @returns `true` if the cell accepts typing.
 *
 * @example
 * isEditable("duration", summaryTask); // false: rolled up from subtasks
 */
export function isEditable(column: GridColumnId, task: TaskRow): boolean {
  if (!EDITABLE_COLUMNS.has(column)) return false;
  // A project may have its own start date; its other values come from its tasks.
  if (task.isProject === true && column === "start") return true;
  const rolledUp = [
    "duration",
    "start",
    "finish",
    "plannedFinish",
    "actualFinish",
    "complete",
  ].includes(column);
  if ((task.isSummary || task.isProject === true) && rolledUp) return false;
  // A project is always what its tasks make it, and has no predecessors.
  // Nor has it constraints, a deadline or a task calendar (as in its window).
  const tasksOnly = [
    "mode",
    "predecessors",
    "constraintType",
    "constraintDate",
    "deadline",
    "calendar",
  ].includes(column);
  if (task.isProject === true && tasksOnly) return false;
  // A manual task's finish would need a new duration computed from dates:
  // that is scheduling, which belongs in Rust. Edit the duration instead.
  if (column === "finish" && task.mode === "manual") return false;
  return true;
}

/**
 * Puts a new calendar day into a date-time, keeping its time of day.
 *
 * @param previous - The old value, e.g. "2027-03-01T08:00:00", or `null`.
 * @param date - The new day as "YYYY-MM-DD" (from the date field).
 * @returns The new date-time, e.g. "2027-03-08T08:00:00". Without a
 *   previous value the time is 00:00; the engine then moves it to the next
 *   working time. `null` if `date` is not a valid "YYYY-MM-DD".
 *
 * @example
 * withDate("2027-03-01T08:00:00", "2027-03-08"); // "2027-03-08T08:00:00"
 * withDate(null, "2027-03-08");                  // "2027-03-08T00:00:00"
 */
export function withDate(previous: IsoDateTime | null, date: string): IsoDateTime | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) return null;
  const time = previous !== null && previous.length >= 19 ? previous.slice(11, 19) : "00:00:00";
  return `${date}T${time}`;
}

/**
 * The day part of a date-time, for a date field.
 *
 * @param iso - E.g. "2027-03-01T08:00:00", or `null`.
 * @returns "2027-03-01", or "" for `null`.
 */
export function dayOf(iso: IsoDateTime | null): string {
  return iso === null ? "" : iso.slice(0, 10);
}

/**
 * Reads a percentage.
 *
 * @param text - E.g. "50", "50 %"; empty means 0.
 * @returns A whole number from 0 to 100, or `null` if it is not one.
 *
 * @example
 * parsePercent("50%"); // 50
 * parsePercent("120"); // null
 */
export function parsePercent(text: string): number | null {
  const match = /^\s*(\d{1,3})?\s*%?\s*$/.exec(text);
  if (match === null) return null;
  const percent = Number(match[1] ?? 0);
  return percent <= 100 ? percent : null;
}

/**
 * The commands that set a task's progress. A task that reaches 100 % gets
 * today as its actual finish, unless it has one. (Below 100 % Rust clears it.)
 *
 * @param task - The task as it is now.
 * @param percent - The new progress, 0 to 100.
 * @param todayMs - Today at 00:00 (see `localToday`).
 * @returns One or two commands, for one undo step.
 *
 * @example
 * percentCommands(task, 100); // [setTaskPercentComplete, setTaskActualFinish]
 */
export function percentCommands(
  task: Pick<TaskRow, "id" | "finish" | "actualFinish">,
  percent: number,
  todayMs: number = localToday(),
): Command[] {
  const id = task.id;
  const commands: Command[] = [{ type: "setTaskPercentComplete", id, percent }];
  if (percent === 100 && task.actualFinish === undefined) {
    const day = new Date(todayMs).toISOString().slice(0, 10);
    const actualFinish = withDate(task.finish, day);
    commands.push({ type: "setTaskActualFinish", id, actualFinish });
  }
  return commands;
}

/**
 * The commands that set a task's actual finish. Giving one means the task is
 * done, so its progress becomes 100 %.
 *
 * @param task - The task as it is now.
 * @param day - "YYYY-MM-DD", or "" to clear it.
 * @returns The commands (none if nothing changed), or `null` for a wrong date.
 *
 * @example
 * actualFinishCommands(task, "2027-03-05");
 */
export function actualFinishCommands(task: TaskRow, day: string): Command[] | null {
  const id = task.id;
  const old = task.actualFinish ?? null;
  if (day === dayOf(old)) return [];
  if (day === "") return [{ type: "setTaskActualFinish", id, actualFinish: null }];
  const actualFinish = withDate(old ?? task.finish, day);
  if (actualFinish === null) return null;
  const done: Command[] =
    task.percentComplete === 100 ? [] : [{ type: "setTaskPercentComplete", id, percent: 100 }];
  return [...done, { type: "setTaskActualFinish", id, actualFinish }];
}

/**
 * The commands that set or clear a task's planned finish.
 *
 * @param task - The task as it is now.
 * @param day - "YYYY-MM-DD", or "" for no plan.
 * @returns The commands (none if nothing changed), or `null` for a wrong date.
 *
 * @example
 * plannedFinishCommands(task, ""); // clears the plan
 */
export function plannedFinishCommands(task: TaskRow, day: string): Command[] | null {
  const id = task.id;
  const old = task.plannedFinish ?? null;
  if (day === dayOf(old)) return [];
  if (day === "") return [{ type: "setTaskPlannedFinish", id, plannedFinish: null }];
  const plannedFinish = withDate(old ?? task.finish, day);
  return plannedFinish === null ? null : [{ type: "setTaskPlannedFinish", id, plannedFinish }];
}

/**
 * The commands that set or clear a task's deadline.
 *
 * @param task - The task as it is now.
 * @param day - "YYYY-MM-DD", or "" for no deadline.
 * @returns The commands (none if nothing changed), or `null` for a wrong date.
 *
 * @example
 * deadlineCommands(task, "2027-03-12");
 */
export function deadlineCommands(task: TaskRow, day: string): Command[] | null {
  const id = task.id;
  if (day === dayOf(task.deadline)) return [];
  if (day === "") return [{ type: "setTaskDeadline", id, deadline: null }];
  const deadline = withDate(task.deadline ?? task.finish, day);
  return deadline === null ? null : [{ type: "setTaskDeadline", id, deadline }];
}

/** The dated constraint types that are about the task's *start*. */
const START_TYPES: ReadonlySet<Constraint["type"]> = new Set([
  "startNoEarlierThan",
  "startNoLaterThan",
  "mustStartOn",
] as const);

/**
 * Builds a constraint with a sensible time of day: the old constraint's, or
 * the task's start (start kinds) or finish (finish kinds).
 *
 * @param task - The task as it is now.
 * @param type - The wanted type.
 * @param day - "YYYY-MM-DD"; not used by the two types without a date.
 * @returns The constraint, or `null` if its date is missing or wrong.
 *
 * @example
 * constraintFor(task, "mustStartOn", "2027-03-08");
 */
export function constraintFor(
  task: TaskRow,
  type: Constraint["type"],
  day: string,
): Constraint | null {
  if (type === "asSoonAsPossible" || type === "asLateAsPossible") return { type };
  const old = task.constraint;
  const isStart = START_TYPES.has(type);
  // A time from the other end of the day would be wrong: "start no earlier
  // than 17:00" would push the task to the next morning.
  const keepTime = "date" in old && START_TYPES.has(old.type) === isStart;
  const reference = keepTime ? old.date : isStart ? task.start : task.finish;
  const date = withDate(reference, day);
  return date === null ? null : { type, date };
}

/** What {@link commandsForCellEdit} answers. */
export type CellEditResult =
  /** The commands to send (empty: nothing changed). */
  | { ok: true; commands: Command[] }
  /** The text could not be used; `key` is a translation key, `values` its placeholders. */
  | { ok: false; key: string; values?: Record<string, string | number> };

/** Everything besides the text that a cell edit needs. */
export interface CellEditContext {
  /** Unit sizes, link codes and unit words (from `useUnitContext`). */
  units: PredecessorContext;
  /** All tasks in row order (to turn row numbers into task IDs). */
  tasks: readonly TaskRow[];
  /** The row number (1-based) of the edited task. */
  rowNumber: number;
  /** Makes a UUID for each new link. */
  newId: () => string;
}

/**
 * Turns what was typed into a cell into commands.
 *
 * @param column - The edited column.
 * @param text - The new text (for `mode`: "auto" or "manual"; for dates:
 *   "YYYY-MM-DD").
 * @param task - The task of the row, as it is now.
 * @param context - See {@link CellEditContext}.
 * @returns The commands, or an error key such as `grid.invalid.duration`.
 *
 * @example
 * commandsForCellEdit("duration", "6d", permitTask, ctx);
 * // { ok: true, commands: [{ type: "setTaskDuration", id, duration: { minutes: 2880 } }] }
 */
export function commandsForCellEdit(
  column: GridColumnId,
  text: string,
  task: TaskRow,
  context: CellEditContext,
): CellEditResult {
  const id = task.id;
  switch (column) {
    case "name":
      return {
        ok: true,
        commands: text === task.name ? [] : [{ type: "renameTask", id, name: text }],
      };

    case "duration": {
      const minutes = parseDuration(text, context.units.basis, context.units.unitWords);
      if (minutes === null) return { ok: false, key: "grid.invalid.duration" };
      if (minutes === task.durationMinutes) return { ok: true, commands: [] };
      return { ok: true, commands: [{ type: "setTaskDuration", id, duration: { minutes } }] };
    }

    case "complete": {
      const percent = parsePercent(text);
      if (percent === null) return { ok: false, key: "grid.invalid.complete" };
      if (percent === (task.percentComplete ?? 0)) return { ok: true, commands: [] };
      return { ok: true, commands: percentCommands(task, percent) };
    }

    case "actualFinish": {
      const commands = actualFinishCommands(task, text);
      return commands === null ? { ok: false, key: "grid.invalid.date" } : { ok: true, commands };
    }

    case "plannedFinish":
    case "deadline": {
      const commands =
        column === "deadline" ? deadlineCommands(task, text) : plannedFinishCommands(task, text);
      return commands === null ? { ok: false, key: "grid.invalid.date" } : { ok: true, commands };
    }

    case "constraintType": {
      // The text comes from the list of types (see `editorOptions`).
      const type = text as Constraint["type"];
      const old = task.constraint;
      if (type === old.type) return { ok: true, commands: [] };
      // A type that needs a date starts with the task's own.
      const own = START_TYPES.has(type) ? task.start : task.finish;
      const constraint = constraintFor(task, type, dayOf("date" in old ? old.date : own));
      if (constraint === null) return { ok: false, key: "taskInfo.invalid.constraintDate" };
      return { ok: true, commands: [{ type: "setTaskConstraint", id, constraint }] };
    }

    case "constraintDate": {
      const old = task.constraint;
      if (text === ("date" in old ? dayOf(old.date) : "")) return { ok: true, commands: [] };
      // A date alone means "start no earlier than", as a typed start does.
      const type = "date" in old ? old.type : "startNoEarlierThan";
      const constraint = constraintFor(task, type, text);
      if (constraint === null) return { ok: false, key: "taskInfo.invalid.constraintDate" };
      return { ok: true, commands: [{ type: "setTaskConstraint", id, constraint }] };
    }

    case "calendar": {
      const calendarId = text === "" ? null : text;
      if (calendarId === task.calendarId) return { ok: true, commands: [] };
      return { ok: true, commands: [{ type: "setTaskCalendar", id, calendarId }] };
    }

    case "start": {
      if (text === dayOf(task.start)) return { ok: true, commands: [] };
      const start = withDate(task.start, text);
      if (start === null) return { ok: false, key: "grid.invalid.date" };
      return {
        ok: true,
        commands:
          task.mode === "manual"
            ? [{ type: "setTaskManualStart", id, start }]
            : [
                {
                  type: "setTaskConstraint",
                  id,
                  constraint: { type: "startNoEarlierThan", date: start },
                },
              ],
      };
    }

    case "finish": {
      if (text === dayOf(task.finish)) return { ok: true, commands: [] };
      const finish = withDate(task.finish, text);
      if (finish === null) return { ok: false, key: "grid.invalid.date" };
      return {
        ok: true,
        commands: [
          {
            type: "setTaskConstraint",
            id,
            constraint: { type: "finishNoEarlierThan", date: finish },
          },
        ],
      };
    }

    case "predecessors": {
      const parsed = parsePredecessors(
        text,
        context.units,
        context.tasks.length,
        context.rowNumber,
      );
      if (!parsed.ok) {
        const { code } = parsed.problem;
        const values = "row" in parsed.problem ? { row: parsed.problem.row } : undefined;
        return { ok: false, key: `grid.invalid.${code}`, ...(values ? { values } : {}) };
      }
      const wanted = parsed.entries.flatMap((entry) => {
        const predecessor = context.tasks[entry.row - 1];
        return predecessor === undefined
          ? []
          : [{ predecessor: predecessor.id, kind: entry.kind, lag: entry.lag }];
      });
      return { ok: true, commands: linkCommands(id, task.links, wanted, context.newId) };
    }

    case "mode": {
      const mode: TaskMode = text === "manual" ? "manual" : "auto";
      if (mode === task.mode) return { ok: true, commands: [] };
      // `manualStart: null`: Rust keeps the task where it is when switching
      // to manual (it stores the current start), as MS Project does.
      return { ok: true, commands: [{ type: "setTaskMode", id, mode, manualStart: null }] };
    }

    default:
      return { ok: true, commands: [] };
  }
}
