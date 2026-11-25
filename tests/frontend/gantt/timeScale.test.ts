/**
 * tests/frontend/gantt/timeScale.test.ts: tests for the time scale.
 *
 * What it checks:
 *   `frontend/features/gantt/renderer/timeScale.ts` maps dates to horizontal
 *   pixel positions ("date → x") and back ("x → date"), decides how wide the
 *   chart is, and lists the header columns ("ticks": days, weeks, months)
 *   that are visible between two x positions.
 *
 * Why it matters:
 *   If date → x is off by even one day, every bar is drawn in the wrong
 *   place. If x → date is not the exact inverse, clicking a bar would pick
 *   the wrong date. The tick list decides which columns are drawn when the
 *   user scrolls, so it must cover the visible area and nothing far beyond
 *   it (10,000-task projects must stay fast).
 *
 * Main ideas:
 *   - The garden shed runs from Mon 1 Mar 2027 08:00 to Tue 23 Mar 2027 17:00.
 *   - The scale adds 7 days before (rounded back to a Monday) and 21 days
 *     after, so the chart starts on Mon 22 Feb 2027 and ends on 13 Apr 2027:
 *     50 days. At the "days" zoom one day is 28 px, so the chart is 1400 px.
 */

import { describe, expect, it } from "vitest";
import { DAY_MS, weekdayMondayBased } from "@frontend/features/gantt/renderer/dates";
import {
  ZOOM_LEVELS,
  createTimeScale,
  msToX,
  visibleTicks,
  xToMs,
} from "@frontend/features/gantt/renderer/timeScale";

const SHED_START = Date.UTC(2027, 2, 1, 8); // Mon 1 Mar 2027 08:00
const SHED_FINISH = Date.UTC(2027, 2, 23, 17); // Tue 23 Mar 2027 17:00
const ORIGIN = Date.UTC(2027, 1, 22); // Mon 22 Feb 2027 00:00

/** The scale the shed gets at the "days" zoom (28 px per day). */
function shedScale() {
  return createTimeScale(SHED_START, SHED_FINISH, ZOOM_LEVELS.days, 0);
}

describe("createTimeScale", () => {
  // The origin is a Monday at least 7 days before the first task; the width
  // covers 21 more days after the last one.
  it("pads the project range and starts on a Monday", () => {
    const scale = shedScale();
    expect(scale.originMs).toBe(ORIGIN);
    expect(weekdayMondayBased(scale.originMs)).toBe(0);
    expect(scale.pxPerDay).toBe(28);
    expect(scale.totalWidth).toBe(50 * 28);
  });

  // The "weeks" zoom only changes the pixels per day, not the dates covered.
  it("uses the zoom level's pixels per day", () => {
    const scale = createTimeScale(SHED_START, SHED_FINISH, ZOOM_LEVELS.weeks, 0);
    expect(scale.originMs).toBe(ORIGIN);
    expect(scale.totalWidth).toBe(50 * 7);
  });

  // A project without dates (empty, or the schedule failed) gives NaN as its
  // range. The chart then shows 8 weeks around a fallback date (today).
  it("falls back to a default range when there are no dates", () => {
    const today = Date.UTC(2027, 2, 3, 12); // a Wednesday
    const scale = createTimeScale(Number.NaN, Number.NaN, ZOOM_LEVELS.days, today);
    expect(scale.originMs).toBe(ORIGIN); // Monday on/before 24 Feb
    expect(Number.isFinite(scale.totalWidth)).toBe(true);
    expect(scale.totalWidth).toBeGreaterThan(8 * 7 * 28);
  });
});

describe("msToX and xToMs (date → x, x → date)", () => {
  // Day 7 after the origin at 08:00 is 7⅓ days × 28 px.
  it("maps the shed start to the right pixel", () => {
    const scale = shedScale();
    expect(msToX(scale, ORIGIN)).toBe(0);
    expect(msToX(scale, SHED_START)).toBeCloseTo((7 + 8 / 24) * 28, 9);
    expect(msToX(scale, ORIGIN + DAY_MS)).toBe(28);
  });

  // Dates before the origin give negative x (off-screen to the left).
  it("gives negative x before the origin", () => {
    expect(msToX(shedScale(), ORIGIN - DAY_MS)).toBe(-28);
  });

  // x → date must undo date → x exactly, for any date, at both zooms.
  it("x → date is the inverse of date → x", () => {
    for (const zoom of [ZOOM_LEVELS.days, ZOOM_LEVELS.weeks]) {
      const scale = createTimeScale(SHED_START, SHED_FINISH, zoom, 0);
      for (const ms of [ORIGIN, SHED_START, SHED_FINISH, ORIGIN + 12.5 * DAY_MS]) {
        expect(xToMs(scale, msToX(scale, ms))).toBeCloseTo(ms, 0);
      }
      for (const x of [0, 1, 27.5, 700, 1399]) {
        expect(msToX(scale, xToMs(scale, x))).toBeCloseTo(x, 9);
      }
    }
  });
});

describe("visibleTicks (visible column range)", () => {
  // Days, 0–100 px: columns 0, 1, 2, 3 (the 4th starts at 84 px and reaches
  // into view); nothing beyond.
  it("lists the day columns that touch the visible range", () => {
    const ticks = visibleTicks(shedScale(), "day", 0, 100);
    expect(ticks.map((t) => t.x)).toEqual([0, 28, 56, 84]);
    expect(ticks.every((t) => t.width === 28)).toBe(true);
    expect(ticks[1]?.ms).toBe(ORIGIN + DAY_MS);
  });

  // Scrolled to 30–90 px: the first visible column is the one that starts
  // at 28 px (it is partly hidden on the left).
  it("starts at the partly visible column when scrolled", () => {
    const ticks = visibleTicks(shedScale(), "day", 30, 90);
    expect(ticks.map((t) => t.x)).toEqual([28, 56, 84]);
  });

  // Negative x (overscroll) is clamped: no columns before the origin.
  it("never lists columns before the origin", () => {
    const ticks = visibleTicks(shedScale(), "day", -100, 30);
    expect(ticks[0]?.x).toBe(0);
  });

  // An empty or reversed range has no columns.
  it("returns nothing for an empty range", () => {
    expect(visibleTicks(shedScale(), "day", 50, 50)).toEqual([]);
    expect(visibleTicks(shedScale(), "day", 80, 10)).toEqual([]);
  });

  // Week columns are 7 days wide and each starts on a Monday.
  it("lists week columns starting on Mondays", () => {
    const ticks = visibleTicks(shedScale(), "week", 0, 400);
    expect(ticks.map((t) => t.x)).toEqual([0, 196, 392]);
    expect(ticks.every((t) => t.width === 7 * 28)).toBe(true);
    expect(ticks.every((t) => weekdayMondayBased(t.ms) === 0)).toBe(true);
  });

  // Month columns follow the real month lengths (Feb 28, Mar 31 days). The
  // first month starts before the origin, so its x is negative.
  it("lists month columns with real month lengths", () => {
    const ticks = visibleTicks(shedScale(), "month", 0, 1400);
    expect(ticks.map((t) => t.ms)).toEqual([
      Date.UTC(2027, 1, 1),
      Date.UTC(2027, 2, 1),
      Date.UTC(2027, 3, 1),
    ]);
    expect(ticks[0]?.x).toBe(-21 * 28);
    expect(ticks[0]?.width).toBe(28 * 28);
    expect(ticks[1]?.x).toBe(7 * 28);
    expect(ticks[1]?.width).toBe(31 * 28);
  });

  // Month columns across New Year: December is followed by January of the
  // next year (the month counter must wrap from 11 to 0).
  it("wraps months across the new year", () => {
    const scale = createTimeScale(
      Date.UTC(2027, 11, 20),
      Date.UTC(2028, 0, 20),
      ZOOM_LEVELS.days,
      0,
    );
    const ticks = visibleTicks(scale, "month", 0, scale.totalWidth);
    expect(ticks.map((t) => new Date(t.ms).toISOString().slice(0, 7))).toEqual([
      "2027-12",
      "2028-01",
      "2028-02",
    ]);
  });
});
