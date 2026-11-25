/**
 * tests/frontend/gantt/dayFill.test.ts: a full working day fills its cell.
 *
 * What it checks (`frontend/features/gantt/renderer/dayFill.ts`): the
 * borders of a working day, part days, days off and unknown calendars.
 *
 * Why it matters:
 *   The picture must not change the real dates: only where they are drawn.
 *
 * Main ideas:
 *   1 March 2027 is a Monday; hours are UTC, like everywhere in the chart.
 */

import { describe, expect, it } from "vitest";
import { standardWeek } from "@frontend/features/calendars/workingTime";
import { makeDayFill } from "@frontend/features/gantt/renderer/dayFill";
import { prepareGanttModel } from "@frontend/features/gantt/renderer/model";
import type { Calendar } from "@frontend/ipc/types";

const standard: Calendar = { id: "std", name: "Standard", week: standardWeek(), holidays: [] };
const fill = makeDayFill([standard], "std");
/** A moment in March 2027. */
const at = (day: number, hour = 0) => Date.UTC(2027, 2, day, hour);

describe("makeDayFill", () => {
  it("stretches 08:00–17:00 over the whole cell", () => {
    expect(fill(at(1, 8), null)).toBe(at(1));
    expect(fill(at(1, 17), null)).toBe(at(2));
  });

  it("draws four hours as half a cell, without the lunch break", () => {
    expect(fill(at(1, 12), null)).toBe(at(1, 12));
    expect(fill(at(1, 13), null)).toBe(at(1, 12));
    expect(fill(at(1, 10), null)).toBe(at(1, 6));
  });

  it("leaves days off and unknown calendars alone", () => {
    expect(fill(at(6, 9), null)).toBe(at(6, 9)); // Saturday
    expect(fill(at(1, 9), "other")).toBe(at(1, 9));
    expect(fill(Number.NaN, null)).toBeNaN();
  });
});

it("keeps the real dates beside the drawn ones", () => {
  const row = prepareGanttModel(
    [
      {
        id: "a",
        name: "A",
        start: "2027-03-01T08:00:00",
        finish: "2027-03-01T17:00:00",
        isCritical: false,
        predecessors: [],
      },
    ],
    fill,
  ).rows[0];
  expect([row?.startMs, row?.finishMs]).toEqual([at(1), at(2)]);
  expect([row?.realStartMs, row?.realFinishMs]).toEqual([at(1, 8), at(1, 17)]);
});
