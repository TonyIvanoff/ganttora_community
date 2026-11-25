/**
 * tests/frontend/gantt/zoomAndLabels.test.ts: tests for zooming and the
 * timescale header text.
 *
 * What it checks:
 *   - the zoom percentage (renderer/timeScale.ts): steps, limits, wheel,
 *     and header labels that stay readable when zoomed out;
 *   - `visibleTicks` with the "year" unit (top tier of the Months zoom):
 *     one column per calendar year, with real year lengths (leap years);
 *   - `createHeaderLabeler` (renderer/headerLabels.ts): the header text for
 *     each unit, in English and German, and the same text from its cache.
 *
 * Why it matters:
 *   Ctrl+= / Ctrl+− must never leave the limits. The
 *   header text comes from the browser's `Intl` library, so it is
 *   automatically in the user's language; a mistake would show wrong month
 *   names or years.
 *
 * Main ideas:
 *   All dates are UTC; `Date.UTC(2027, 0, 1)` is 1 January 2027 (months are
 *   counted from 0 in JavaScript).
 */

import { describe, expect, it } from "vitest";
import { createHeaderLabeler } from "@frontend/features/gantt/renderer/headerLabels";
import {
  ZOOM_LEVELS,
  clampZoomPercent,
  createTimeScale,
  labelStride,
  stepZoomPercent,
  tickIndex,
  visibleTicks,
  wheelZoomPercent,
} from "@frontend/features/gantt/renderer/timeScale";

describe("zoom percentage", () => {
  it("steps to the next multiple of ten and stops at the limits", () => {
    expect(stepZoomPercent(100, "in")).toBe(110);
    expect(stepZoomPercent(100, "out")).toBe(90);
    expect(stepZoomPercent(117, "in")).toBe(120);
    expect(stepZoomPercent(117, "out")).toBe(110);
    expect(stepZoomPercent(400, "in")).toBe(400);
    expect(stepZoomPercent(10, "out")).toBe(10);
  });

  it("cleans any number", () => {
    expect(clampZoomPercent(99.6)).toBe(100);
    expect(clampZoomPercent(0)).toBe(10);
    expect(clampZoomPercent(9999)).toBe(400);
    expect(clampZoomPercent(Number.NaN)).toBe(100);
  });

  // Out and back in by the same movement ends where it began, and small
  // trackpad movements count too.
  it("follows the wheel", () => {
    expect(wheelZoomPercent(100, -100)).toBeGreaterThan(115);
    expect(wheelZoomPercent(100, 100)).toBeLessThan(90);
    expect(wheelZoomPercent(wheelZoomPercent(100, 30), -30)).toBeCloseTo(100);
    expect(wheelZoomPercent(100, -1)).toBeGreaterThan(100);
    expect(wheelZoomPercent(400, -100)).toBe(400);
  });

  // The percentage changes the width only; the header units stay.
  it("is independent of days, weeks and months", () => {
    for (const level of Object.values(ZOOM_LEVELS)) {
      const scale = createTimeScale(Date.UTC(2027, 0, 1), Date.UTC(2027, 1, 1), level, 0, 150);
      expect(scale.pxPerDay).toBe(level.pxPerDay * 1.5);
      expect(scale.zoom).toBe(level);
    }
  });
});

describe("labelStride", () => {
  it("labels every cell at 100 %", () => {
    expect(labelStride("day", 28, false)).toBe(1);
    expect(labelStride("week", 49, false)).toBe(1);
    expect(labelStride("month", 60, false)).toBe(1);
    expect(labelStride("month", 196, true)).toBe(1);
  });

  it("shares labels when zoomed out", () => {
    expect(labelStride("day", 5, false)).toBe(7);
    expect(labelStride("month", 6, false)).toBe(6);
  });

  // Mondays, and January, April, July, October.
  it("puts shared labels on natural borders", () => {
    const scale = createTimeScale(Date.UTC(2027, 0, 1), Date.UTC(2027, 1, 1), ZOOM_LEVELS.days, 0);
    const monday = Date.UTC(2027, 0, 4);
    expect(tickIndex(scale, "day", monday) % 7).toBe(0);
    expect(tickIndex(scale, "month", Date.UTC(2027, 3, 1)) % 3).toBe(0);
  });
});

describe("visibleTicks: years", () => {
  // A plan from March 2027 to February 2028 at the Months zoom: columns
  // for 2027 (365 days) and 2028 (a leap year, 366 days).
  it("gives one column per year with real lengths", () => {
    const scale = createTimeScale(
      Date.UTC(2027, 2, 1),
      Date.UTC(2028, 1, 15),
      ZOOM_LEVELS.months,
      0,
    );
    const ticks = visibleTicks(scale, "year", 0, scale.totalWidth);
    expect(ticks.map((t) => new Date(t.ms).getUTCFullYear())).toEqual([2027, 2028]);
    expect(ticks[0]?.width).toBe(365 * 2);
    expect(ticks[1]?.width).toBe(366 * 2);
    // The 2027 column starts before the chart's origin (late February), so
    // its x is negative; the 2028 column starts exactly 365 days later.
    expect(ticks[0]?.x).toBeLessThan(0);
    expect((ticks[1]?.x ?? 0) - (ticks[0]?.x ?? 0)).toBe(365 * 2);
  });
});

describe("createHeaderLabeler", () => {
  const jan4 = Date.UTC(2027, 0, 4);

  // English labels for every unit and tier.
  it("labels every unit in English", () => {
    const label = createHeaderLabeler("en");
    expect(label("year", jan4, true)).toBe("2027");
    expect(label("month", jan4, true)).toBe("January 2027");
    expect(label("month", jan4, false)).toBe("Jan");
    expect(label("week", jan4, false)).toBe("Jan 4");
    expect(label("day", jan4, false)).toBe("4");
  });

  // Another language gives that language's month names.
  it("uses the language it was made for", () => {
    const label = createHeaderLabeler("de");
    expect(label("month", jan4, true)).toBe("Januar 2027");
    expect(label("year", jan4, true)).toBe("2027");
  });

  // Asking twice gives the same text (the second time from the cache).
  it("gives the same label again from its cache", () => {
    const label = createHeaderLabeler("en");
    const first = label("week", jan4, false);
    expect(label("week", jan4, false)).toBe(first);
    // Different tier, different text: the cache key includes the tier.
    expect(label("month", jan4, false)).not.toBe(label("month", jan4, true));
  });
});
