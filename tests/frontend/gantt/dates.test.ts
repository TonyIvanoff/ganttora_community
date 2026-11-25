/**
 * tests/frontend/gantt/dates.test.ts: tests for the Gantt date helpers.
 *
 * What it checks:
 *   `frontend/features/gantt/renderer/dates.ts` turns schedule dates such as
 *   "2027-03-01T08:00:00" into numbers (milliseconds) and answers simple
 *   calendar questions: start of the day, day of the week, the Monday of a
 *   week, weekend or not.
 *
 * Why it matters:
 *   Every bar position in the chart starts from these numbers. Schedule dates
 *   have no time zone, so they must be read as UTC; otherwise a user in
 *   Tokyo and a user in New York would see the bars on different days.
 *
 * Main ideas:
 *   - A test file is a list of `it("does something", () => { ... })` blocks.
 *     `describe` groups them. `expect(a).toBe(b)` fails the test if a ≠ b.
 *   - 1 March 2027 is a Monday; the garden shed project starts that day.
 */

import { describe, expect, it } from "vitest";
import {
  DAY_MS,
  WEEK_MS,
  isWeekend,
  mondayOnOrBefore,
  parseIsoLocal,
  startOfDay,
  weekdayMondayBased,
} from "@frontend/features/gantt/renderer/dates";

/** Monday 1 March 2027, 00:00 UTC, in milliseconds. `Date.UTC` months start at 0. */
const MON_1_MAR = Date.UTC(2027, 2, 1);

describe("parseIsoLocal", () => {
  // The engine sends dates without a time zone; they must be read as UTC.
  it("reads a date-time without a time zone as UTC", () => {
    expect(parseIsoLocal("2027-03-01T08:00:00")).toBe(Date.UTC(2027, 2, 1, 8));
  });

  // A text that already ends in "Z" (UTC) must not get a second "Z".
  it("accepts a date-time that already ends in Z", () => {
    expect(parseIsoLocal("2027-03-01T08:00:00Z")).toBe(Date.UTC(2027, 2, 1, 8));
  });

  // A bare date (10 characters) is midnight UTC by the JavaScript rules.
  it("reads a bare date as midnight UTC", () => {
    expect(parseIsoLocal("2027-03-01")).toBe(MON_1_MAR);
  });

  // Empty or broken text gives NaN ("not a number"); callers check for it.
  it("returns NaN for empty or invalid text", () => {
    expect(parseIsoLocal("")).toBeNaN();
    expect(parseIsoLocal("not a date")).toBeNaN();
  });
});

describe("startOfDay / weekdayMondayBased / mondayOnOrBefore / isWeekend", () => {
  // Cutting the time off must give midnight of the same day.
  it("startOfDay drops the time of day", () => {
    expect(startOfDay(Date.UTC(2027, 2, 1, 17, 30))).toBe(MON_1_MAR);
    expect(startOfDay(MON_1_MAR)).toBe(MON_1_MAR);
  });

  // Monday = 0 ... Sunday = 6 (the European week, as in the chart header).
  it("numbers weekdays from Monday = 0", () => {
    const days = [0, 1, 2, 3, 4, 5, 6].map((i) => weekdayMondayBased(MON_1_MAR + i * DAY_MS));
    expect(days).toEqual([0, 1, 2, 3, 4, 5, 6]);
  });

  // Week columns start on Mondays; any day of a week maps to its Monday.
  it("finds the Monday on or before a day", () => {
    expect(mondayOnOrBefore(MON_1_MAR)).toBe(MON_1_MAR);
    expect(mondayOnOrBefore(Date.UTC(2027, 2, 7, 15))).toBe(MON_1_MAR); // Sunday afternoon
    expect(mondayOnOrBefore(Date.UTC(2027, 2, 8))).toBe(MON_1_MAR + WEEK_MS); // next Monday
  });

  // Weekends are shaded in the chart.
  it("marks Saturday and Sunday as weekend", () => {
    expect(isWeekend(Date.UTC(2027, 2, 5))).toBe(false); // Friday
    expect(isWeekend(Date.UTC(2027, 2, 6))).toBe(true); // Saturday
    expect(isWeekend(Date.UTC(2027, 2, 7, 23, 59))).toBe(true); // Sunday, late
    expect(isWeekend(Date.UTC(2027, 2, 8))).toBe(false); // Monday
  });
});
