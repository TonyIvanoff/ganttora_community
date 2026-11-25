/**
 * tests/frontend/task-info/taskInfoCommands.test.ts: tests for the Task
 * Information dialog's logic.
 *
 * What it checks:
 *   `frontend/features/task-info/taskInfoCommands.ts`:
 *   - `draftFromTask` fills the dialog's fields from a task (duration as
 *     "2d", constraint date as a day, lags as "+1d", empty strings for "no
 *     deadline" / "project calendar");
 *   - `commandsFromDraft` sends commands **only for the fields that
 *     changed**, in a fixed order, and refuses with the field and the
 *     translation key when something cannot be read (duration, constraint
 *     date, deadline, a link to itself, the same predecessor twice, a lag);
 *   - blockers join the same batch, or are left alone.
 *
 * Why it matters:
 *   OK in the dialog sends one batch (one undo step). If unchanged fields
 *   were sent too, every OK would add pointless change records to the
 *   journal and could even move the task.
 *
 * Main ideas:
 *   `formatDays` is `String`, so 2 days prints as "2" and the draft shows
 *   "2d". The task is "task-3" in a project with tasks 1–3.
 */

import { describe, expect, it } from "vitest";
import {
  commandsFromDraft,
  draftFromTask,
  needsDate,
  type TaskInfoDraft,
} from "@frontend/features/task-info/taskInfoCommands";
import { draftBlockersFromTask, newDraftBlocker } from "@frontend/features/task-info/blockers";
import type { TaskRow } from "@frontend/ipc/types";
import { englishUnits, taskRow } from "../grid/helpers";
import en from "../../../locales/en.json";

const UNITS = englishUnits();

/** The task in the dialog: 2 days, Mon 1 Mar 08:00 → Tue 2 Mar 17:00, one link. */
function task(changes: Partial<TaskRow> = {}): TaskRow {
  return taskRow({
    id: "task-3",
    name: "Build",
    durationMinutes: 960,
    finish: "2027-03-02T17:00:00",
    links: [
      {
        id: "link-1",
        predecessor: "task-1",
        kind: "finishToStart",
        lagMinutes: 480,
        lagPercent: null,
      },
    ],
    ...changes,
  });
}

/** The draft the dialog shows for `t`. */
function draft(t: TaskRow): TaskInfoDraft {
  return draftFromTask(t, UNITS, String);
}

/** Runs `commandsFromDraft` with new link IDs "new-1", "new-2" … */
function commands(t: TaskRow, d: TaskInfoDraft) {
  let n = 0;
  return commandsFromDraft(t, d, UNITS, () => `new-${++n}`);
}

describe("draftFromTask", () => {
  // Every field of the dialog, from an ordinary task.
  it("fills the fields from the task", () => {
    expect(draft(task())).toEqual({
      name: "Build",
      durationText: "2d",
      initialDurationText: "2d",
      mode: "auto",
      milestone: false,
      hyperlink: "",
      notes: "",
      constraintType: "asSoonAsPossible",
      constraintDate: "",
      deadline: "",
      calendarId: "",
      links: [{ key: "link-1", predecessorId: "task-1", kind: "finishToStart", lagText: "+1d" }],
    });
  });

  // Dated constraint, deadline and task calendar show as days and IDs.
  it("shows constraint date, deadline and calendar", () => {
    const d = draft(
      task({
        constraint: { type: "startNoEarlierThan", date: "2027-03-08T08:00:00" },
        deadline: "2027-03-12T17:00:00",
        calendarId: "cal-night",
      }),
    );
    expect(d.constraintType).toBe("startNoEarlierThan");
    expect(d.constraintDate).toBe("2027-03-08");
    expect(d.deadline).toBe("2027-03-12");
    expect(d.calendarId).toBe("cal-night");
  });

  // The milestone box shows the *mark*: a zero-duration task is drawn as a
  // milestone anyway, so its box starts unticked.
  it("ticks the milestone box only for a marked task with duration", () => {
    expect(draft(task({ isMilestone: true, durationMinutes: 960 })).milestone).toBe(true);
    expect(draft(task({ isMilestone: true, durationMinutes: 0 })).milestone).toBe(false);
  });

  // needsDate: only the six dated constraint kinds need a date field.
  it("knows which constraints need a date", () => {
    expect(needsDate("asSoonAsPossible")).toBe(false);
    expect(needsDate("asLateAsPossible")).toBe(false);
    expect(needsDate("mustFinishOn")).toBe(true);
    expect(needsDate("startNoEarlierThan")).toBe(true);
  });
});

describe("commandsFromDraft: only changed fields", () => {
  // Opening the dialog and pressing OK changes nothing.
  it("sends nothing for an unchanged draft", () => {
    const t = task({
      constraint: { type: "finishNoLaterThan", date: "2027-03-12T17:00:00" },
      deadline: "2027-03-15T17:00:00",
      calendarId: "cal-night",
      notes: "Two coats",
      hyperlink: "https://example.com",
    });
    expect(commands(t, draft(t))).toEqual({ ok: true, commands: [] });
  });

  // Only the name changed: only a rename.
  it("sends only the changed field", () => {
    const t = task();
    expect(commands(t, { ...draft(t), name: "Build walls" })).toEqual({
      ok: true,
      commands: [{ type: "renameTask", id: "task-3", name: "Build walls" }],
    });
  });

  // Everything changed: the commands come in the documented order.
  it("sends every changed field in a fixed order", () => {
    const t = task();
    const d: TaskInfoDraft = {
      ...draft(t),
      name: "Build walls",
      durationText: "3d",
      mode: "manual",
      milestone: true,
      constraintType: "startNoEarlierThan",
      constraintDate: "2027-03-08",
      deadline: "2027-03-12",
      calendarId: "cal-night",
      notes: "Two coats",
      hyperlink: "https://example.com",
      links: [],
    };
    const result = commands(t, d);
    expect(result.ok).toBe(true);
    expect(result.ok ? result.commands.map((c) => c.type) : []).toEqual([
      "renameTask",
      "setTaskDuration",
      "setTaskMode",
      "setTaskMilestone",
      "setTaskConstraint",
      "setTaskDeadline",
      "setTaskCalendar",
      "setTaskNotes",
      "setTaskHyperlink",
      "removeDependency",
    ]);
  });

  // A new start constraint takes the time of day of the task's start;
  // a new finish constraint the time of its finish.
  it("gives new constraint dates the start or finish time of day", () => {
    const t = task();
    expect(
      commands(t, { ...draft(t), constraintType: "mustStartOn", constraintDate: "2027-03-08" }),
    ).toEqual({
      ok: true,
      commands: [
        {
          type: "setTaskConstraint",
          id: "task-3",
          constraint: { type: "mustStartOn", date: "2027-03-08T08:00:00" },
        },
      ],
    });
    expect(
      commands(t, {
        ...draft(t),
        constraintType: "finishNoLaterThan",
        constraintDate: "2027-03-08",
      }),
    ).toEqual({
      ok: true,
      commands: [
        {
          type: "setTaskConstraint",
          id: "task-3",
          constraint: { type: "finishNoLaterThan", date: "2027-03-08T17:00:00" },
        },
      ],
    });
  });

  // Switching back to "as soon as possible" needs no date.
  it("clears a dated constraint", () => {
    const t = task({ constraint: { type: "mustStartOn", date: "2027-03-08T08:00:00" } });
    expect(commands(t, { ...draft(t), constraintType: "asSoonAsPossible" })).toEqual({
      ok: true,
      commands: [
        { type: "setTaskConstraint", id: "task-3", constraint: { type: "asSoonAsPossible" } },
      ],
    });
  });

  // Emptying the deadline removes it; a calendar "" means the project
  // calendar (null).
  it("clears the deadline and the task calendar", () => {
    const t = task({ deadline: "2027-03-15T17:00:00", calendarId: "cal-night" });
    expect(commands(t, { ...draft(t), deadline: "", calendarId: "" })).toEqual({
      ok: true,
      commands: [
        { type: "setTaskDeadline", id: "task-3", deadline: null },
        { type: "setTaskCalendar", id: "task-3", calendarId: null },
      ],
    });
  });

  // A summary task's duration is never sent, even if the text is garbage.
  it("never sends a summary's duration", () => {
    const t = task({ isSummary: true });
    expect(commands(t, { ...draft(t), durationText: "nonsense" })).toEqual({
      ok: true,
      commands: [],
    });
  });

  // Link rows: an empty row is ignored; a changed lag is an edit; a new
  // predecessor is an add.
  it("turns link rows into link commands", () => {
    const t = task();
    const d: TaskInfoDraft = {
      ...draft(t),
      links: [
        { key: "link-1", predecessorId: "task-1", kind: "finishToStart", lagText: "+2d" },
        { key: "row-2", predecessorId: "", kind: "finishToStart", lagText: "" },
        { key: "row-3", predecessorId: "task-2", kind: "startToStart", lagText: "-20%" },
      ],
    };
    expect(commands(t, d)).toEqual({
      ok: true,
      commands: [
        {
          type: "editDependency",
          id: "link-1",
          kind: "finishToStart",
          lag: { minutes: 960 },
          lagPercent: null,
        },
        {
          type: "addDependency",
          dependency: {
            id: "new-1",
            predecessor: "task-2",
            successor: "task-3",
            kind: "startToStart",
            lag: { minutes: 0 },
            lagPercent: -20,
          },
        },
      ],
    });
  });

  // BUG (reported to frontend-dev): a 1-hour task shows "0.13d" (2-decimal
  // rounding), which reads back as 62 minutes, so pressing OK without any
  // change sends setTaskDuration 62. Expected: no commands.
  it("an unchanged 1-hour task sends no duration change (fixed bug)", () => {
    const t = task({ durationMinutes: 60, finish: "2027-03-01T09:00:00" });
    expect(commands(t, draft(t))).toEqual({ ok: true, commands: [] });
  });
});

describe("commandsFromDraft: validation", () => {
  // Each problem names the field to highlight and the message key.
  it("refuses an unreadable duration", () => {
    const t = task();
    expect(commands(t, { ...draft(t), durationText: "3x" })).toEqual({
      ok: false,
      field: "duration",
      key: "grid.invalid.duration",
    });
  });

  it("refuses a dated constraint without a valid date", () => {
    const t = task();
    expect(
      commands(t, { ...draft(t), constraintType: "startNoEarlierThan", constraintDate: "" }),
    ).toEqual({ ok: false, field: "constraintDate", key: "taskInfo.invalid.constraintDate" });
  });

  it("refuses an unreadable deadline", () => {
    const t = task();
    expect(commands(t, { ...draft(t), deadline: "soon" })).toEqual({
      ok: false,
      field: "deadline",
      key: "grid.invalid.date",
    });
  });

  // Link problems give the 1-based row of the dialog's link table.
  it("refuses a link to the task itself, duplicates and bad lags", () => {
    const t = task();
    const row = (predecessorId: string, lagText = "") => ({
      key: predecessorId,
      predecessorId,
      kind: "finishToStart" as const,
      lagText,
    });
    expect(commands(t, { ...draft(t), links: [row("task-1"), row("task-3")] })).toEqual({
      ok: false,
      field: "links",
      key: "taskInfo.invalid.selfLink",
      values: { row: 2 },
    });
    expect(commands(t, { ...draft(t), links: [row("task-1"), row("task-1")] })).toEqual({
      ok: false,
      field: "links",
      key: "taskInfo.invalid.duplicateLink",
      values: { row: 2 },
    });
    expect(commands(t, { ...draft(t), links: [row("task-2", "+x")] })).toEqual({
      ok: false,
      field: "links",
      key: "taskInfo.invalid.lag",
      values: { row: 1 },
    });
  });
});

describe("commandsFromDraft: blockers (D23)", () => {
  /** The dialog's task with one blocker of one day. */
  function blockedTask(): TaskRow {
    return task({
      blockers: [
        {
          id: "b1",
          afterWorkMinutes: 480,
          durationMinutes: 480,
          reason: "Late delivery",
          reportedOn: null,
          start: "2027-03-02T08:00:00",
          finish: "2027-03-02T17:00:00",
        },
      ],
    });
  }

  // `undefined` means "this draft does not know about blockers". It must
  // not be read as "no blockers", which would delete them all.
  it("leaves the blockers alone when the draft has none set", () => {
    const t = blockedTask();
    const d = draft(t);
    expect(d.blockers).toBeUndefined();
    expect(commands(t, d)).toEqual({ ok: true, commands: [] });
  });

  // An empty list, on the other hand, is the user's wish to remove them.
  it("removes the blockers when the list is empty", () => {
    const t = blockedTask();
    expect(commands(t, { ...draft(t), blockers: [] })).toEqual({
      ok: true,
      commands: [{ type: "removeBlocker", task: "task-3", id: "b1" }],
    });
  });

  // One OK is one undo step, so blocker commands join the same list, after
  // the other fields and links.
  it("appends blocker commands after the other commands", () => {
    const t = blockedTask();
    const d: TaskInfoDraft = { ...draft(t), name: "Build it", links: [], blockers: [] };
    const result = commands(t, d);
    expect(result.ok && result.commands.map((c) => c.type)).toEqual([
      "renameTask",
      "removeDependency",
      "removeBlocker",
    ]);
  });

  // A bad field refuses the whole OK. The key names the field, the number
  // says which blocker (counted from 1), and each key has an English text.
  it.each([
    ["start", { startDate: "" }],
    ["length", { lengthText: "soon" }],
    ["reason", { reason: "  " }],
    ["reportedOn", { reportedOn: "yesterday" }],
  ] as const)("refuses an invalid %s", (field, bad) => {
    const t = blockedTask();
    const good = draftBlockersFromTask(t, UNITS);
    const added = newDraftBlocker("new", "2027-03-02");
    added.values = { ...added.values, lengthText: "1d", reason: "Rain", ...bad };
    const result = commands(t, { ...draft(t), name: "Other", blockers: [...good, added] });
    expect(result).toEqual({
      ok: false,
      field: "blockers",
      key: `taskInfo.invalid.blocker.${field}`,
      values: { number: 2 },
    });
    expect(en.taskInfo.invalid.blocker[field]).toContain("{number}");
  });
});
