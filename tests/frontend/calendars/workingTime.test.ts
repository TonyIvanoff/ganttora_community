/**
 * tests/frontend/calendars/workingTime.test.ts: tests for the calendar
 * helpers used by the calendar editor and by Gantt snapping.
 *
 * What it checks:
 *   `frontend/features/calendars/workingTime.ts`:
 *   - `blocksOn`: the working hours of one day, with the priority
 *     exception → holiday → work week;
 *   - `snapToWorkingDay`: the first working minute on or after a day, or
 *     the last one on or before it;
 *   - `timeToMinutes` / `minutesToTime`: "08:30" ↔ 510, including "24:00"
 *     for the end of the day;
 *   - `blocksProblem`: the first broken working-time block (start after
 *     end, overlap, past midnight).
 *
 * Why it matters:
 *   These answers must match the Rust engine's calendar rules. If the UI
 *   thought a holiday was a working day, a dragged bar would snap onto it
 *   and the engine would then move it again: a confusing jump.
 *
 * Main ideas:
 *   Days are midnight-UTC milliseconds; `day("2027-03-06")` is Saturday
 *   6 March 2027. Working times are minutes after midnight (480 = 08:00).
 */

import { describe, expect, it } from "vitest";
import {
  blocksMinutes,
  blocksOn,
  blocksProblem,
  isWorkingDay,
  minutesToTime,
  snapToWorkingDay,
  standardDay,
  standardWeek,
  timeToMinutes,
} from "@frontend/features/calendars/workingTime";
import { parseIsoLocal } from "@frontend/features/gantt/renderer/dates";
import type { Calendar } from "@frontend/ipc/types";

/** Midnight of an ISO day, as milliseconds. */
const day = (iso: string) => parseIsoLocal(iso);

/** A standard calendar with a holiday on Fri 5 Mar and two exceptions. */
function calendar(): Calendar {
  return {
    id: "cal",
    name: "Standard",
    week: standardWeek(),
    holidays: ["2027-03-05", "2027-03-10"],
    exceptions: [
      // Saturday made a short working day.
      {
        name: "Catch-up",
        start: "2027-03-06",
        finish: "2027-03-06",
        workingTimes: [{ startMinute: 540, endMinute: 780 }],
      },
      // Wednesday 10 Mar is both a holiday and an exception: the exception wins.
      {
        name: "Half day",
        start: "2027-03-10",
        finish: "2027-03-10",
        workingTimes: [{ startMinute: 480, endMinute: 720 }],
      },
    ],
  };
}

describe("blocksOn", () => {
  // An ordinary Monday: the work week's two blocks.
  it("uses the work week on ordinary days", () => {
    expect(blocksOn(calendar(), day("2027-03-01"))).toEqual(standardDay());
    expect(blocksOn(calendar(), day("2027-03-07"))).toEqual([]); // Sunday
  });

  // A holiday has no working time.
  it("gives no working time on a holiday", () => {
    expect(blocksOn(calendar(), day("2027-03-05"))).toEqual([]);
    expect(isWorkingDay(calendar(), day("2027-03-05"))).toBe(false);
  });

  // An exception beats both the work week (Saturday) and a holiday.
  it("lets an exception win over the week and over a holiday", () => {
    expect(blocksOn(calendar(), day("2027-03-06"))).toEqual([{ startMinute: 540, endMinute: 780 }]);
    expect(blocksOn(calendar(), day("2027-03-10"))).toEqual([{ startMinute: 480, endMinute: 720 }]);
  });

  // A calendar without the optional `exceptions` list still works.
  it("works without an exceptions list", () => {
    const plain: Calendar = { id: "c", name: "Plain", week: standardWeek(), holidays: [] };
    expect(blocksOn(plain, day("2027-03-06"))).toEqual([]);
  });
});

describe("snapToWorkingDay", () => {
  // Thursday is a working day: start 08:00, finish 17:00 the same day.
  it("keeps a working day", () => {
    expect(snapToWorkingDay(calendar(), day("2027-03-04"), "start")).toBe(
      parseIsoLocal("2027-03-04T08:00:00"),
    );
    expect(snapToWorkingDay(calendar(), day("2027-03-04"), "finish")).toBe(
      parseIsoLocal("2027-03-04T17:00:00"),
    );
  });

  // Sunday → forward to Monday 08:00 (start) or back to Saturday's
  // exception end 13:00 (finish).
  it("walks forward for a start and back for a finish", () => {
    expect(snapToWorkingDay(calendar(), day("2027-03-07"), "start")).toBe(
      parseIsoLocal("2027-03-08T08:00:00"),
    );
    expect(snapToWorkingDay(calendar(), day("2027-03-07"), "finish")).toBe(
      parseIsoLocal("2027-03-06T13:00:00"),
    );
  });

  // Any time of day is treated as that whole day.
  it("ignores the time of day of the input", () => {
    expect(snapToWorkingDay(calendar(), parseIsoLocal("2027-03-04T15:30:00"), "start")).toBe(
      parseIsoLocal("2027-03-04T08:00:00"),
    );
  });

  // A calendar with no working time at all gives NaN instead of looping.
  it("gives NaN when there is no working day", () => {
    const never: Calendar = {
      id: "n",
      name: "Never",
      week: { ...standardWeek(), monday: [], tuesday: [], wednesday: [], thursday: [], friday: [] },
      holidays: [],
    };
    expect(snapToWorkingDay(never, day("2027-03-01"), "start")).toBeNaN();
  });
});

describe("timeToMinutes and minutesToTime", () => {
  // Readable times, with one- or two-digit hours and spaces around.
  it("reads clock times", () => {
    expect(timeToMinutes("08:00")).toBe(480);
    expect(timeToMinutes("8:30")).toBe(510);
    expect(timeToMinutes(" 13:15 ")).toBe(795);
    expect(timeToMinutes("00:00")).toBe(0);
    expect(timeToMinutes("24:00")).toBe(1440); // end of the day
  });

  // Past 24:00, bad minutes and other text are refused.
  it("refuses impossible times", () => {
    for (const text of ["24:01", "25:00", "7:60", "8", "8.30", "abc", ""]) {
      expect(timeToMinutes(text), text).toBeNull();
    }
  });

  // Writing back: always two digits each, "24:00" for 1440.
  it("writes minutes as HH:MM", () => {
    expect(minutesToTime(0)).toBe("00:00");
    expect(minutesToTime(510)).toBe("08:30");
    expect(minutesToTime(1440)).toBe("24:00");
    expect(timeToMinutes(minutesToTime(1035))).toBe(1035);
  });
});

describe("blocksProblem", () => {
  // The standard day and an empty day are fine; blocks may touch.
  it("accepts valid blocks", () => {
    expect(blocksProblem(standardDay())).toBeNull();
    expect(blocksProblem([])).toBeNull();
    expect(
      blocksProblem([
        { startMinute: 480, endMinute: 720 },
        { startMinute: 720, endMinute: 1440 },
      ]),
    ).toBeNull();
  });

  // The index of the first broken block.
  it("finds the first broken block", () => {
    expect(blocksProblem([{ startMinute: 600, endMinute: 600 }])).toBe(0); // empty
    expect(blocksProblem([{ startMinute: 700, endMinute: 600 }])).toBe(0); // backwards
    expect(
      blocksProblem([
        { startMinute: 480, endMinute: 720 },
        { startMinute: 700, endMinute: 900 }, // overlaps the first
      ]),
    ).toBe(1);
    expect(
      blocksProblem([
        { startMinute: 480, endMinute: 720 },
        { startMinute: 780, endMinute: 1500 }, // past midnight
      ]),
    ).toBe(1);
  });

  // blocksMinutes adds up the working minutes (8 hours for the standard day).
  it("counts working minutes", () => {
    expect(blocksMinutes(standardDay())).toBe(480);
    expect(blocksMinutes([])).toBe(0);
  });
});
