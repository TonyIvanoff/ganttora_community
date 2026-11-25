/**
 * tests/frontend/grid/columnWidths.test.ts: tests for the grid's column
 * widths.
 *
 * What it checks:
 *   `frontend/features/grid/columnWidths.ts`:
 *   - `clampColumnWidth` keeps a width inside 24–1200 pixels, rounds it to
 *     a whole number, and uses the fallback for "not a number";
 *   - `resolveColumnWidths` picks, for every column, a drag in progress
 *     ("live") over the saved setting over the default, and ignores saved
 *     keys that are not columns;
 *   - `columnLefts` adds the widths up into left edges and a total.
 *
 * Why it matters:
 *   Saved widths come from `settings.json`, which may be from an older
 *   version or edited by hand. A broken value (text, a negative number, a
 *   huge number) must never make a column vanish or the grid unusable; the
 *   grid must always be drawable.
 *
 * Main ideas:
 *   These are pure functions (no window, no stores), so they are called
 *   directly with plain objects.
 */

import { describe, expect, it } from "vitest";
import { GRID_COLUMN_IDS } from "@frontend/features/grid/cellEdit";
import {
  DEFAULT_COLUMN_WIDTHS,
  MAX_COLUMN_WIDTH,
  MIN_COLUMN_WIDTH,
  clampColumnWidth,
  columnLefts,
  resolveColumnWidths,
} from "@frontend/features/grid/columnWidths";

describe("clampColumnWidth", () => {
  // A normal width passes through unchanged.
  it("keeps a width inside the range", () => {
    expect(clampColumnWidth(150, 84)).toBe(150);
  });

  // Fractions come from mouse positions; widths are whole pixels.
  it("rounds fractions to whole pixels", () => {
    expect(clampColumnWidth(300.6, 84)).toBe(301);
    expect(clampColumnWidth(300.4, 84)).toBe(300);
  });

  // Too narrow or negative → the minimum; too wide → the maximum.
  it("clamps widths outside the range", () => {
    expect(clampColumnWidth(10.4, 84)).toBe(MIN_COLUMN_WIDTH);
    expect(clampColumnWidth(-50, 84)).toBe(MIN_COLUMN_WIDTH);
    expect(clampColumnWidth(0, 84)).toBe(MIN_COLUMN_WIDTH);
    expect(clampColumnWidth(5000, 84)).toBe(MAX_COLUMN_WIDTH);
  });

  // The limits themselves are allowed.
  it("accepts the exact limits", () => {
    expect(clampColumnWidth(MIN_COLUMN_WIDTH, 84)).toBe(MIN_COLUMN_WIDTH);
    expect(clampColumnWidth(MAX_COLUMN_WIDTH, 84)).toBe(MAX_COLUMN_WIDTH);
  });

  // NaN and infinities are not usable widths: the fallback is returned
  // (not clamped to the maximum, which would make a giant column).
  it("returns the fallback for NaN and infinity", () => {
    expect(clampColumnWidth(Number.NaN, 84)).toBe(84);
    expect(clampColumnWidth(Number.POSITIVE_INFINITY, 84)).toBe(84);
    expect(clampColumnWidth(Number.NEGATIVE_INFINITY, 84)).toBe(84);
  });
});

describe("resolveColumnWidths", () => {
  // Nothing saved, nothing dragged: every column has its default.
  it("uses the defaults when nothing is saved", () => {
    expect(resolveColumnWidths({}, {})).toEqual(DEFAULT_COLUMN_WIDTHS);
  });

  // A saved width wins over the default; the others keep their defaults.
  it("prefers a saved width over the default", () => {
    const widths = resolveColumnWidths({ name: 300 }, {});
    expect(widths.name).toBe(300);
    expect(widths.duration).toBe(DEFAULT_COLUMN_WIDTHS.duration);
  });

  // While the user drags, the live width wins over the saved one.
  it("prefers a live (dragging) width over the saved one", () => {
    expect(resolveColumnWidths({ name: 300 }, { name: 320 }).name).toBe(320);
    expect(resolveColumnWidths({}, { start: 200 }).start).toBe(200);
  });

  // Keys that are not columns (e.g. from a newer or hand-edited file) are
  // ignored: the result has exactly the grid's columns.
  it("ignores unknown keys", () => {
    const widths = resolveColumnWidths({ name: 300, unknown: 50, resources: 99 }, {});
    expect(Object.keys(widths).sort()).toEqual([...GRID_COLUMN_IDS].sort());
    expect(widths).not.toHaveProperty("unknown");
  });

  // Odd saved values are cleaned: out of range → clamped, NaN → default.
  it("cleans saved values that are out of range or not numbers", () => {
    const widths = resolveColumnWidths({ name: 5, duration: Number.NaN, start: 99999 }, {});
    expect(widths.name).toBe(MIN_COLUMN_WIDTH);
    expect(widths.duration).toBe(DEFAULT_COLUMN_WIDTHS.duration);
    expect(widths.start).toBe(MAX_COLUMN_WIDTH);
  });

  // A live drag past the limits is clamped too.
  it("clamps live widths", () => {
    expect(resolveColumnWidths({}, { name: 3 }).name).toBe(MIN_COLUMN_WIDTH);
  });
});

describe("columnLefts", () => {
  // Each column starts where the previous one ends; total = sum of widths.
  it("adds the default widths into left edges and a total", () => {
    const { left, total } = columnLefts(resolveColumnWidths({}, {}));
    expect(left.rowNumber).toBe(0);
    expect(left.indicators).toBe(DEFAULT_COLUMN_WIDTHS.rowNumber);
    expect(left.outline).toBe(DEFAULT_COLUMN_WIDTHS.rowNumber + DEFAULT_COLUMN_WIDTHS.indicators);
    const sum = GRID_COLUMN_IDS.reduce((acc, id) => acc + DEFAULT_COLUMN_WIDTHS[id], 0);
    expect(total).toBe(sum);
    // The last column ends exactly at the total.
    expect(left.mode + DEFAULT_COLUMN_WIDTHS.mode).toBe(total);
  });

  // Widening one column moves every column to its right by the same amount.
  it("shifts later columns when one column is wider", () => {
    const base = columnLefts(resolveColumnWidths({}, {}));
    const wider = columnLefts(resolveColumnWidths({ name: DEFAULT_COLUMN_WIDTHS.name + 60 }, {}));
    expect(wider.left.name).toBe(base.left.name);
    expect(wider.left.duration).toBe(base.left.duration + 60);
    expect(wider.left.mode).toBe(base.left.mode + 60);
    expect(wider.total).toBe(base.total + 60);
  });
});
