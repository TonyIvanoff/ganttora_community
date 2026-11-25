/**
 * tests/frontend/task-info/blockers.test.ts: tests for the logic behind the
 * Add blocker dialog and the Blockers tab.
 *
 * What it checks:
 *   `frontend/features/task-info/blockers.ts`:
 *   - `draftBlockersFromTask` fills the tab's rows from a task;
 *   - `parseBlockerForm` reads a form or names the first wrong field;
 *   - `blockerCommands` compares the rows with the task and sends only what
 *     changed: nothing, add, edit, move (remove + add) or remove, and says
 *     which row and field are wrong when it must refuse.
 *
 * Why it matters:
 *   A blocker records why a plan slipped. A command too many would move
 *   dates the user did not touch; one too few would lose the record.
 *
 * Main ideas:
 *   The task has two blockers, "b1" and "b2". Days have 480 minutes, so a
 *   length of 960 minutes shows as "2d".
 */

import { describe, expect, it } from "vitest";
import {
  blockerCommands,
  draftBlockersFromTask,
  formatBlockerLength,
  newDraftBlocker,
  parseBlockerForm,
  todayIso,
  type BlockerFormValues,
  type DraftBlocker,
} from "@frontend/features/task-info/blockers";
import type { BlockerRow, TaskRow } from "@frontend/ipc/types";
import { englishUnits, taskRow } from "../grid/helpers";

const UNITS = englishUnits();

/** The first blocker: two days, reported. `changes` replaces some fields. */
function firstBlocker(changes: Partial<BlockerRow> = {}): BlockerRow {
  return {
    id: "b1",
    afterWorkMinutes: 480,
    durationMinutes: 960,
    reason: "Late delivery",
    reportedOn: "2027-03-03",
    start: "2027-03-02T08:00:00",
    finish: "2027-03-03T17:00:00",
    ...changes,
  };
}

/** The second blocker: half a day, never reported. */
const SECOND: BlockerRow = {
  id: "b2",
  afterWorkMinutes: 960,
  durationMinutes: 240,
  reason: "Rain",
  reportedOn: null,
  start: "2027-03-05T08:00:00",
  finish: "2027-03-05T12:00:00",
};

/** A three-day task with the given blockers (normally both). */
function task(blockers: BlockerRow[] = [firstBlocker(), SECOND]): TaskRow {
  return taskRow({ id: "task-3", durationMinutes: 1440, blockers });
}

/** Runs `blockerCommands` with new IDs "new-1", "new-2" … */
function run(t: TaskRow, drafts: readonly DraftBlocker[]) {
  let n = 0;
  return blockerCommands(t, drafts, UNITS, () => `new-${++n}`);
}

/** The task's rows, with some values of row `index` changed (as by typing). */
function typed(t: TaskRow, index: number, changes: Partial<BlockerFormValues>): DraftBlocker[] {
  return draftBlockersFromTask(t, UNITS).map((d, i) =>
    i === index ? { ...d, values: { ...d.values, ...changes } } : d,
  );
}

/** A new row as the user filled it in, starting Thu 4 Mar 2027. */
function newRow(key: string, changes: Partial<BlockerFormValues> = {}): DraftBlocker {
  const row = newDraftBlocker(key, "2027-03-04");
  return { ...row, values: { ...row.values, lengthText: "1d", reason: "Strike", ...changes } };
}

describe("draftBlockersFromTask", () => {
  // Every field as the tab shows it; a missing report date is empty text.
  it("fills the rows from the task", () => {
    const first = {
      startDate: "2027-03-02",
      lengthText: "2d",
      reason: "Late delivery",
      reportedOn: "2027-03-03",
    };
    const second = { startDate: "2027-03-05", lengthText: "0.5d", reason: "Rain", reportedOn: "" };
    expect(draftBlockersFromTask(task(), UNITS)).toEqual([
      { key: "b1", id: "b1", values: first, initial: first, afterWorkMinutes: 480 },
      { key: "b2", id: "b2", values: second, initial: second, afterWorkMinutes: 960 },
    ]);
  });

  // `initial` must be a copy. If both were one object, typing would change
  // both and no edit would ever be noticed.
  it("keeps values and initial as separate objects", () => {
    const [row] = draftBlockersFromTask(task(), UNITS);
    expect(row?.initial).not.toBe(row?.values);
  });

  // Rust leaves `blockers` out when there are none; a failed schedule has
  // no dates. Neither may crash the dialog.
  it("accepts missing blockers and missing dates", () => {
    expect(draftBlockersFromTask(taskRow(), UNITS)).toEqual([]);
    const [row] = draftBlockersFromTask(task([firstBlocker({ start: null })]), UNITS);
    expect(row?.values.startDate).toBe("");
  });
});

describe("parseBlockerForm and small helpers", () => {
  /** A form that can be read. */
  const GOOD: BlockerFormValues = {
    startDate: "2027-03-04",
    lengthText: "2d",
    reason: " Strike ",
    reportedOn: "",
  };

  // Spaces around the reason go; an empty report date becomes null.
  it("reads a valid form", () => {
    expect(parseBlockerForm(GOOD, UNITS)).toEqual({
      ok: true,
      value: {
        at: "2027-03-04T00:00:00",
        durationMinutes: 960,
        reason: "Strike",
        reportedOn: null,
      },
    });
  });

  // Each wrong field is named, so the dialog can mark the right box. Zero
  // and negative lengths are refused too.
  it.each([
    ["start", { startDate: "" }],
    ["start", { startDate: "4.3.2027" }],
    ["length", { lengthText: "" }],
    ["length", { lengthText: "soon" }],
    ["length", { lengthText: "0d" }],
    ["length", { lengthText: "-1d" }],
    ["reason", { reason: " " }],
    ["reportedOn", { reportedOn: "3 March" }],
  ] as const)("names the field %s for %o", (field, changes) => {
    expect(parseBlockerForm({ ...GOOD, ...changes }, UNITS)).toEqual({ ok: false, field });
  });

  // With several mistakes the first field from the top is reported.
  it("reports the first wrong field", () => {
    const form = { startDate: "", lengthText: "", reason: "", reportedOn: "x" };
    expect(parseBlockerForm(form, UNITS)).toEqual({ ok: false, field: "start" });
    expect(parseBlockerForm({ ...form, startDate: "2027-03-04" }, UNITS)).toEqual({
      ok: false,
      field: "length",
    });
  });

  // Month and day get a leading zero; a new row starts with today's date.
  it("formats lengths and today", () => {
    expect(formatBlockerLength(720, UNITS)).toBe("1.5d");
    expect(todayIso(new Date(2027, 2, 4))).toBe("2027-03-04");
    expect(newDraftBlocker("k", "2027-03-04")).toEqual({
      key: "k",
      id: null,
      values: { startDate: "2027-03-04", lengthText: "", reason: "", reportedOn: "2027-03-04" },
      initial: null,
      afterWorkMinutes: 0,
    });
  });
});

describe("blockerCommands", () => {
  // Opening the dialog and pressing OK must change nothing.
  it("sends nothing when nothing changed", () => {
    expect(run(task(), draftBlockersFromTask(task(), UNITS))).toEqual({ ok: true, commands: [] });
    expect(run(taskRow(), [])).toEqual({ ok: true, commands: [] });
  });

  // A new row gets a new ID and is placed by date.
  it("adds new rows, each with its own ID", () => {
    const rows = [...draftBlockersFromTask(task(), UNITS), newRow("a"), newRow("b")];
    const add = {
      type: "addBlockerAt",
      task: "task-3",
      at: "2027-03-04T00:00:00",
      duration: { minutes: 480 },
      reason: "Strike",
      reportedOn: "2027-03-04",
    };
    expect(run(task(), rows)).toEqual({
      ok: true,
      commands: [
        { ...add, id: "new-1" },
        { ...add, id: "new-2" },
      ],
    });
  });

  // Length, reason or report date changed: one edit, which keeps the place
  // inside the work (afterWork) exactly.
  it("edits a row without moving it", () => {
    const rows = typed(task(), 0, { lengthText: "3d", reason: "Lost", reportedOn: "" });
    expect(run(task(), rows)).toEqual({
      ok: true,
      commands: [
        {
          type: "editBlocker",
          task: "task-3",
          blocker: {
            id: "b1",
            afterWork: { minutes: 480 },
            duration: { minutes: 1440 },
            reason: "Lost",
            reportedOn: null,
          },
        },
      ],
    });
  });

  // 100 minutes is shown rounded. If the length text was not touched, the
  // exact minutes must survive a change of the reason.
  it("keeps the exact length when only the reason changes", () => {
    const t = task([firstBlocker({ durationMinutes: 100 })]);
    const rounded = draftBlockersFromTask(t, { ...UNITS, formatNumber: (n) => n.toFixed(2) });
    const rows = rounded.map((d) => ({ ...d, values: { ...d.values, reason: "Lost" } }));
    expect(rows[0]?.values.lengthText).toBe("0.21d");
    const result = run(t, rows);
    expect(result.ok && result.commands).toMatchObject([
      { type: "editBlocker", blocker: { duration: { minutes: 100 } } },
    ]);
  });

  // A failed schedule has no start date; the reason can still be edited.
  it("edits a row that has no calculated start", () => {
    const t = task([firstBlocker({ start: null })]);
    const result = run(t, typed(t, 0, { reason: "Lost" }));
    expect(result.ok && result.commands.map((c) => c.type)).toEqual(["editBlocker"]);
  });

  // A new start day: remove, then add again with the same ID, so the core
  // works out the new place in the work.
  it("moves a row with remove + add under the same ID", () => {
    expect(run(task(), typed(task(), 1, { startDate: "2027-03-08" }))).toEqual({
      ok: true,
      commands: [
        { type: "removeBlocker", task: "task-3", id: "b2" },
        {
          type: "addBlockerAt",
          task: "task-3",
          id: "b2",
          at: "2027-03-08T00:00:00",
          duration: { minutes: 240 },
          reason: "Rain",
          reportedOn: null,
        },
      ],
    });
  });

  // Reading the rounded text "0.21d" back would give 101 minutes. Only the
  // day changed, so the length must stay 100 minutes, as it does for an edit.
  it("keeps the exact length when only the start day changes", () => {
    const t = task([firstBlocker({ durationMinutes: 100 })]);
    const rounded = draftBlockersFromTask(t, { ...UNITS, formatNumber: (n) => n.toFixed(2) });
    const rows = rounded.map((d) => ({ ...d, values: { ...d.values, startDate: "2027-03-08" } }));
    const result = run(t, rows);
    expect(result.ok && result.commands).toMatchObject([
      { type: "removeBlocker", id: "b1" },
      { type: "addBlockerAt", id: "b1", duration: { minutes: 100 } },
    ]);
  });

  // Removed rows come first, then the remaining rows from top to bottom.
  it("removes missing rows before the other changes", () => {
    const rows = [...typed(task(), 1, { reason: "Storm" }).slice(1), newRow("a")];
    const result = run(task(), rows);
    expect(result.ok && result.commands).toMatchObject([
      { type: "removeBlocker", id: "b1" },
      { type: "editBlocker", blocker: { id: "b2" } },
      { type: "addBlockerAt", id: "new-1" },
    ]);
  });

  // The number is the row's position on screen, counted from 1, whether the
  // row is edited, moved or new.
  it("refuses with the row number and the field", () => {
    const t = task();
    expect(run(t, typed(t, 1, { lengthText: "0" }))).toEqual({
      ok: false,
      number: 2,
      field: "length",
    });
    expect(run(t, typed(t, 0, { startDate: "" }))).toEqual({
      ok: false,
      number: 1,
      field: "start",
    });
    expect(run(t, typed(t, 1, { reason: "" }))).toEqual({ ok: false, number: 2, field: "reason" });
    const rows = [...draftBlockersFromTask(t, UNITS), newRow("a", { reportedOn: "x" })];
    expect(run(t, rows)).toEqual({ ok: false, number: 3, field: "reportedOn" });
    // After removing the first row, the second one is number 1 on screen.
    expect(run(t, typed(t, 1, { reason: "" }).slice(1))).toEqual({
      ok: false,
      number: 1,
      field: "reason",
    });
  });
});
