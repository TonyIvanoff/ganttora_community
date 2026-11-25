/**
 * tests/frontend/files/noteText.test.ts: tests for the "read note"
 * sentences shown after opening a file.
 *
 * What it checks:
 *   `noteText` in `frontend/features/files/noteText.ts` picks the right
 *   translation key (`file.note.<kind>`) for every kind of note and fills
 *   in its details:
 *   - the start date follows the "Date format" setting: "en|yyyy-mm-dd" gives
 *     2027-03-24, "en|dd mmm yyyy" gives a British-style date with weekday;
 *   - a missing duration names the task by its name when the task is in
 *     the snapshot, by its ID otherwise.
 *
 * Why it matters:
 *   These notes tell the user what Ganttora changed while opening their
 *   file. A wrong date or an unreadable ID instead of a task name would
 *   make the user distrust the file, or miss a real problem.
 *
 * Main ideas:
 *   Instead of the real translation library we pass a fake `t` that returns
 *   the key and the values as text, so we can check exactly what
 *   `noteText` asked for, independent of the English wording.
 */

import { describe, expect, it } from "vitest";
import type { TFunction } from "i18next";
import { noteText } from "@frontend/features/files/noteText";
import { shedSnapshot, taskId } from "../gantt/helpers";

/**
 * Fake translation function: "key {json of values}".
 *
 * Beginner note: `as unknown as TFunction` tells TypeScript "trust me, use
 * this as a TFunction". The real type is very detailed; our fake only
 * needs the (key, values) form `noteText` uses.
 */
const t = ((key: string, values?: Record<string, unknown>) =>
  values === undefined ? key : `${key} ${JSON.stringify(values)}`) as unknown as TFunction;

/** Reads the values back out of the fake's text. */
function valuesOf(text: string): Record<string, unknown> {
  return JSON.parse(text.slice(text.indexOf("{"))) as Record<string, unknown>;
}

describe("noteText: start date", () => {
  const note = { kind: "startDateMissing", used: "2027-03-24T08:00:00" } as const;

  // "en|yyyy-mm-dd" writes the date as YYYY-MM-DD, the same in every language.
  it("writes the date in ISO style", () => {
    const text = noteText(note, t, null, "en|yyyy-mm-dd");
    expect(text.startsWith("file.note.startDateMissing ")).toBe(true);
    expect(valuesOf(text).date).toBe("2027-03-24");
  });

  it("writes the date with a month name", () => {
    expect(valuesOf(noteText(note, t, null, "en|dd mmm yyyy")).date).toBe("24 Mar 2027");
  });

  // A date the program cannot read is shown as it came, not as "null".
  it("falls back to the raw text for an unreadable date", () => {
    const text = noteText(
      { kind: "startDateMissing", used: "not a date" },
      t,
      null,
      "en|yyyy-mm-dd",
    );
    expect(valuesOf(text).date).toBe("not a date");
  });
});

describe("noteText: other kinds", () => {
  // The task's name is friendlier than its UUID.
  it("names a task with a missing duration by its name", () => {
    const text = noteText(
      { kind: "durationMissing", taskId: taskId(3) },
      t,
      shedSnapshot(),
      "en|yyyy-mm-dd",
    );
    expect(valuesOf(text)).toEqual({ task: "Buy materials" });
  });

  // Without a snapshot, or for an unknown task, the ID is shown.
  it("falls back to the task ID", () => {
    expect(
      valuesOf(noteText({ kind: "durationMissing", taskId: "t-x" }, t, null, "en|yyyy-mm-dd")),
    ).toEqual({
      task: "t-x",
    });
    expect(
      valuesOf(
        noteText({ kind: "durationMissing", taskId: "t-x" }, t, shedSnapshot(), "en|yyyy-mm-dd"),
      ),
    ).toEqual({ task: "t-x" });
  });

  // Notes without details use just their key.
  it("uses the plain key for notes without details", () => {
    expect(noteText({ kind: "calendarMissing", calendarId: "c" }, t, null, "en|yyyy-mm-dd")).toBe(
      "file.note.calendarMissing",
    );
    expect(
      noteText(
        { kind: "baseCalendarFlattened", calendarId: "c", baseId: "b" },
        t,
        null,
        "en|yyyy-mm-dd",
      ),
    ).toBe("file.note.baseCalendarFlattened");
  });

  // Table, column and count are passed on for the sentence.
  it("passes table, column and count", () => {
    const value = noteText(
      { kind: "valueNotLoaded", table: "tasks", rowId: "r", column: "cost", value: "9" },
      t,
      null,
      "en|yyyy-mm-dd",
    );
    expect(valuesOf(value)).toEqual({ table: "tasks", column: "cost" });
    const rows = noteText(
      { kind: "rowsNotLoaded", table: "resources", count: 3 },
      t,
      null,
      "en|yyyy-mm-dd",
    );
    expect(valuesOf(rows)).toEqual({ table: "resources", count: 3 });
  });
});
