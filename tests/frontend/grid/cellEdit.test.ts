/**
 * tests/frontend/grid/cellEdit.test.ts: tests for turning a grid edit into
 * commands.
 *
 * What it checks:
 *   `frontend/features/grid/cellEdit.ts`:
 *   - `isEditable`: which cells may be edited (summary dates and durations
 *     are calculated, a manual task's finish is not typed);
 *   - `withDate`: a new day keeps the time of day of the old date;
 *   - `commandsForCellEdit`: for each column, the command(s) a finished
 *     edit sends to Rust, no command when nothing changed, and the
 *     translation key of the error when the text cannot be read.
 *
 * Why it matters:
 *   The grid is where most edits happen. Typing a start date must become a
 *   "Start No Earlier Than" constraint for an automatic task (as in MS
 *   Project), but a new manual start for a manual task. Sending a command
 *   when nothing changed would fill the undo history with empty steps.
 */

import { describe, expect, it } from "vitest";
import {
  commandsForCellEdit,
  dayOf,
  isEditable,
  percentCommands,
  withDate,
  type CellEditContext,
  parsePercent,
} from "@frontend/features/grid/cellEdit";
import type { TaskRow } from "@frontend/ipc/types";
import { englishUnits, taskRow } from "./helpers";

/** Three tasks; the edit is on row 3 ("task-3"). */
const TASKS: TaskRow[] = [
  taskRow({ id: "task-1", name: "Design" }),
  taskRow({ id: "task-2", name: "Permit" }),
  taskRow({ id: "task-3", name: "Build" }),
];

/** The edit context for row 3, with new link IDs "new-1", "new-2" … */
function context(): CellEditContext {
  let n = 0;
  return { units: englishUnits(), tasks: TASKS, rowNumber: 3, newId: () => `new-${++n}` };
}

/** The task being edited. */
const BUILD = TASKS[2] as TaskRow;

describe("isEditable", () => {
  // Only the data columns can be edited; the row number, indicators and
  // WBS number are calculated.
  it("allows only the editable columns", () => {
    const task = taskRow();
    for (const column of ["name", "duration", "start", "finish", "predecessors", "mode"] as const) {
      expect(isEditable(column, task), column).toBe(true);
    }
    for (const column of ["rowNumber", "indicators", "outline"] as const) {
      expect(isEditable(column, task), column).toBe(false);
    }
  });

  // A summary's dates and duration come from its subtasks.
  it("locks duration, start and finish of a summary task", () => {
    const summary = taskRow({ isSummary: true });
    expect(isEditable("duration", summary)).toBe(false);
    expect(isEditable("start", summary)).toBe(false);
    expect(isEditable("finish", summary)).toBe(false);
    expect(isEditable("name", summary)).toBe(true);
  });

  // A manual task's finish would need scheduling maths; its start is fine.
  it("locks the finish of a manual task", () => {
    const manual = taskRow({ mode: "manual" });
    expect(isEditable("finish", manual)).toBe(false);
    expect(isEditable("start", manual)).toBe(true);
  });
});

describe("withDate and dayOf", () => {
  // The new day keeps the old time of day (08:00 stays 08:00).
  it("keeps the time of day of the old date", () => {
    expect(withDate("2027-03-01T08:00:00", "2027-03-05")).toBe("2027-03-05T08:00:00");
    expect(withDate("2027-03-01T17:00:00", "2027-03-05")).toBe("2027-03-05T17:00:00");
  });

  // Without an old date, midnight; Rust moves it to the next working time.
  it("uses midnight without an old date", () => {
    expect(withDate(null, "2027-03-05")).toBe("2027-03-05T00:00:00");
  });

  // Only YYYY-MM-DD (what the date input gives) is accepted.
  it("refuses other date formats", () => {
    expect(withDate(null, "5.3.2027")).toBeNull();
    expect(withDate(null, "")).toBeNull();
    expect(withDate(null, "2027-3-5")).toBeNull();
  });

  // dayOf is the inverse: the YYYY-MM-DD part, or "" for no date.
  it("cuts the day out of a date-time", () => {
    expect(dayOf("2027-03-05T08:00:00")).toBe("2027-03-05");
    expect(dayOf(null)).toBe("");
  });
});

describe("commandsForCellEdit", () => {
  // Name: a rename, or nothing if the name is the same.
  it("renames, or sends nothing for the same name", () => {
    expect(commandsForCellEdit("name", "Build", BUILD, context())).toEqual({
      ok: true,
      commands: [],
    });
    expect(commandsForCellEdit("name", "Build shed", BUILD, context())).toEqual({
      ok: true,
      commands: [{ type: "renameTask", id: "task-3", name: "Build shed" }],
    });
  });

  // Duration: "3d" is 1440 minutes; "1d" is what it already is.
  it("sets the duration from text with units", () => {
    expect(commandsForCellEdit("duration", "3d", BUILD, context())).toEqual({
      ok: true,
      commands: [{ type: "setTaskDuration", id: "task-3", duration: { minutes: 1440 } }],
    });
    expect(commandsForCellEdit("duration", "1d", BUILD, context())).toEqual({
      ok: true,
      commands: [],
    });
    expect(commandsForCellEdit("duration", "8h", BUILD, context())).toEqual({
      ok: true,
      commands: [],
    });
  });

  // Unreadable or negative durations give the translated error key.
  it("reports an unreadable duration", () => {
    for (const text of ["abc", "-1d", "3x", ""]) {
      expect(commandsForCellEdit("duration", text, BUILD, context()), text).toEqual({
        ok: false,
        key: "grid.invalid.duration",
      });
    }
  });

  // Start of an automatic task: a Start No Earlier Than constraint, keeping
  // the time of day.
  it("turns a typed start into SNET for an automatic task", () => {
    expect(commandsForCellEdit("start", "2027-03-08", BUILD, context())).toEqual({
      ok: true,
      commands: [
        {
          type: "setTaskConstraint",
          id: "task-3",
          constraint: { type: "startNoEarlierThan", date: "2027-03-08T08:00:00" },
        },
      ],
    });
  });

  // Start of a manual task: the manual start itself.
  it("moves a manual task by its manual start", () => {
    const manual = { ...BUILD, mode: "manual" as const };
    expect(commandsForCellEdit("start", "2027-03-08", manual, context())).toEqual({
      ok: true,
      commands: [{ type: "setTaskManualStart", id: "task-3", start: "2027-03-08T08:00:00" }],
    });
  });

  // The same day, or text that is no date.
  it("sends nothing for the same start and reports bad dates", () => {
    expect(commandsForCellEdit("start", "2027-03-01", BUILD, context())).toEqual({
      ok: true,
      commands: [],
    });
    expect(commandsForCellEdit("start", "next week", BUILD, context())).toEqual({
      ok: false,
      key: "grid.invalid.date",
    });
  });

  // Finish: a Finish No Earlier Than constraint at the old finish time.
  it("turns a typed finish into FNET", () => {
    expect(commandsForCellEdit("finish", "2027-03-04", BUILD, context())).toEqual({
      ok: true,
      commands: [
        {
          type: "setTaskConstraint",
          id: "task-3",
          constraint: { type: "finishNoEarlierThan", date: "2027-03-04T17:00:00" },
        },
      ],
    });
    expect(commandsForCellEdit("finish", "2027-03-01", BUILD, context())).toEqual({
      ok: true,
      commands: [],
    });
  });

  // Predecessors "1, 2SS": two new links, IDs from the counter.
  it("adds links from the predecessors text", () => {
    const result = commandsForCellEdit("predecessors", "1, 2SS", BUILD, context());
    expect(result).toEqual({
      ok: true,
      commands: [
        {
          type: "addDependency",
          dependency: {
            id: "new-1",
            predecessor: "task-1",
            successor: "task-3",
            kind: "finishToStart",
            lag: { minutes: 0 },
            lagPercent: null,
          },
        },
        {
          type: "addDependency",
          dependency: {
            id: "new-2",
            predecessor: "task-2",
            successor: "task-3",
            kind: "startToStart",
            lag: { minutes: 0 },
            lagPercent: null,
          },
        },
      ],
    });
  });

  // Problems become `grid.invalid.<code>` keys, with the row when known.
  it("reports predecessor problems with their keys", () => {
    expect(commandsForCellEdit("predecessors", "9", BUILD, context())).toEqual({
      ok: false,
      key: "grid.invalid.unknownRow",
      values: { row: 9 },
    });
    expect(commandsForCellEdit("predecessors", "3", BUILD, context())).toEqual({
      ok: false,
      key: "grid.invalid.selfLinkRow",
      values: { row: 3 },
    });
    expect(commandsForCellEdit("predecessors", "x", BUILD, context())).toEqual({
      ok: false,
      key: "grid.invalid.predecessors",
    });
  });

  // Mode: switching sends setTaskMode with manualStart null (Rust keeps
  // the current start); the same mode sends nothing.
  it("switches the mode", () => {
    expect(commandsForCellEdit("mode", "manual", BUILD, context())).toEqual({
      ok: true,
      commands: [{ type: "setTaskMode", id: "task-3", mode: "manual", manualStart: null }],
    });
    expect(commandsForCellEdit("mode", "auto", BUILD, context())).toEqual({
      ok: true,
      commands: [],
    });
  });

  // Columns that cannot be edited never produce commands.
  it("sends nothing for read-only columns", () => {
    expect(commandsForCellEdit("rowNumber", "7", BUILD, context())).toEqual({
      ok: true,
      commands: [],
    });
  });
});

describe("parsePercent", () => {
  it("reads whole percentages from 0 to 100", () => {
    expect([parsePercent("50"), parsePercent(" 75 % "), parsePercent("")]).toEqual([50, 75, 0]);
    expect([parsePercent("101"), parsePercent("half"), parsePercent("-1")]).toEqual([
      null,
      null,
      null,
    ]);
  });
});

describe("project rows", () => {
  it("accept their own start, but no duration, finish or percent", () => {
    const project = taskRow({ isProject: true, isSummary: true });
    expect(isEditable("start", project)).toBe(true);
    expect(isEditable("name", project)).toBe(true);
    for (const column of ["duration", "finish", "complete", "mode", "predecessors"] as const) {
      expect(isEditable(column, project)).toBe(false);
    }
  });
});

describe("actual finish", () => {
  const edit = (text: string, extra: Partial<TaskRow>) =>
    commandsForCellEdit("actualFinish", text, taskRow(extra), context());
  const finish = "2027-03-05T17:00:00";

  it("is today when a task reaches 100 %, unless it has one", () => {
    const today = Date.UTC(2027, 2, 8);
    const row = taskRow({ finish });
    expect(percentCommands(row, 100, today)[1]).toEqual({
      type: "setTaskActualFinish",
      id: row.id,
      actualFinish: "2027-03-08T17:00:00",
    });
    expect(percentCommands(row, 90, today)).toHaveLength(1);
    expect(percentCommands({ ...row, actualFinish: finish }, 100, today)).toHaveLength(1);
  });

  it("sets 100 % when typed and can be cleared", () => {
    const typed = edit("2027-03-09", { finish });
    expect(typed.ok && typed.commands.map((c) => c.type)).toEqual([
      "setTaskPercentComplete",
      "setTaskActualFinish",
    ]);
    const cleared = edit("", { finish, actualFinish: finish, percentComplete: 100 });
    expect(cleared.ok && cleared.commands).toMatchObject([{ actualFinish: null }]);
    expect(edit("", { finish })).toEqual({ ok: true, commands: [] });
  });

  it("belongs to tasks only", () => {
    expect(isEditable("actualFinish", taskRow())).toBe(true);
    expect(isEditable("actualFinish", taskRow({ isSummary: true }))).toBe(false);
  });
});

describe("the columns of the Dates tab", () => {
  const edit = (
    column: Parameters<typeof isEditable>[0],
    text: string,
    extra: Partial<TaskRow> = {},
  ) => commandsForCellEdit(column, text, taskRow(extra), context());
  const start = "2027-03-01T08:00:00";
  const finish = "2027-03-02T17:00:00";

  it("can be edited for tasks; a group has no plan to type, a project none of them", () => {
    const group = taskRow({ isSummary: true });
    const project = taskRow({ isSummary: true, isProject: true });
    expect(isEditable("plannedFinish", taskRow())).toBe(true);
    expect(isEditable("plannedFinish", group)).toBe(false);
    expect(isEditable("plannedFinish", project)).toBe(false);
    for (const column of ["constraintType", "constraintDate", "deadline", "calendar"] as const) {
      expect(isEditable(column, taskRow()), column).toBe(true);
      expect(isEditable(column, group), column).toBe(true);
      expect(isEditable(column, project), column).toBe(false);
    }
  });

  it("sets, clears and refuses the planned finish and the deadline", () => {
    expect(edit("plannedFinish", "2027-03-05", { finish })).toMatchObject({
      commands: [{ type: "setTaskPlannedFinish", plannedFinish: "2027-03-05T17:00:00" }],
    });
    expect(edit("deadline", "2027-03-05", { finish })).toMatchObject({
      commands: [{ type: "setTaskDeadline", deadline: "2027-03-05T17:00:00" }],
    });
    expect(edit("plannedFinish", "", { plannedFinish: finish })).toMatchObject({
      commands: [{ plannedFinish: null }],
    });
    expect(edit("deadline", "", { deadline: finish })).toMatchObject({
      commands: [{ deadline: null }],
    });
    expect(edit("deadline", "")).toEqual({ ok: true, commands: [] });
    expect(edit("plannedFinish", "x")).toEqual({ ok: false, key: "grid.invalid.date" });
    expect(edit("deadline", "x")).toEqual({ ok: false, key: "grid.invalid.date" });
  });

  it("gives a chosen constraint the task's own date and keeps an existing day", () => {
    expect(edit("constraintType", "mustFinishOn", { start, finish })).toMatchObject({
      commands: [{ constraint: { type: "mustFinishOn", date: finish } }],
    });
    const constraint = { type: "mustFinishOn", date: "2027-03-20T17:00:00" } as const;
    // The time of day follows the kind: a start is not at 17:00.
    expect(edit("constraintType", "mustStartOn", { start, finish, constraint })).toMatchObject({
      commands: [{ constraint: { type: "mustStartOn", date: "2027-03-20T08:00:00" } }],
    });
    expect(edit("constraintType", "asSoonAsPossible", { constraint })).toMatchObject({
      commands: [{ constraint: { type: "asSoonAsPossible" } }],
    });
    expect(edit("constraintType", "mustFinishOn", { constraint })).toEqual({
      ok: true,
      commands: [],
    });
  });

  it("changes a constraint's date; a date alone means start no earlier than", () => {
    expect(edit("constraintDate", "2027-03-08", { start })).toMatchObject({
      commands: [{ constraint: { type: "startNoEarlierThan", date: "2027-03-08T08:00:00" } }],
    });
    const constraint = { type: "mustFinishOn", date: finish } as const;
    expect(edit("constraintDate", "2027-03-08", { constraint })).toMatchObject({
      commands: [{ constraint: { type: "mustFinishOn", date: "2027-03-08T17:00:00" } }],
    });
    expect(edit("constraintDate", "", { constraint })).toMatchObject({ ok: false });
    expect(edit("constraintDate", "")).toEqual({ ok: true, commands: [] });
  });

  it("sets and clears the task calendar", () => {
    expect(edit("calendar", "cal-night")).toMatchObject({
      commands: [{ type: "setTaskCalendar", calendarId: "cal-night" }],
    });
    expect(edit("calendar", "", { calendarId: "cal-night" })).toMatchObject({
      commands: [{ calendarId: null }],
    });
    expect(edit("calendar", "", { calendarId: null })).toEqual({ ok: true, commands: [] });
  });
});
