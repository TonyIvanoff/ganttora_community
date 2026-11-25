/**
 * tests/frontend/gantt/layout.test.ts: tests for rows and pixel layout.
 *
 * What it checks:
 *   `frontend/features/gantt/renderer/layout.ts`: which rows are visible for
 *   a scroll position ("visible row range"), where a row is drawn, how tall
 *   the content is, and the canvas sizes for high-resolution screens.
 *
 * Why it matters:
 *   The chart only draws the visible rows, which is what keeps 10,000 tasks
 *   fast. If the range is one row too short, the last row flickers or is
 *   missing; if it is too long, we waste time drawing hidden rows.
 *
 * Main ideas:
 *   Rows are 28 px tall. The header (two tiers of 24 px) is 48 px tall and
 *   does not scroll.
 */

import { describe, expect, it } from "vitest";
import {
  HEADER_HEIGHT,
  ROW_HEIGHT,
  backingStoreSize,
  contentHeight,
  crisp,
  physicalLineWidth,
  rowTop,
  visibleRowRange,
} from "@frontend/features/gantt/renderer/layout";

describe("visibleRowRange", () => {
  // Header 48 + 10 rows × 28 = 328 px: exactly rows 0–9 are visible.
  it("shows the rows that fit below the header", () => {
    expect(visibleRowRange(0, HEADER_HEIGHT + 10 * ROW_HEIGHT, 100)).toEqual({ first: 0, end: 10 });
  });

  // Scrolled half a row down: row 0 is half hidden, row 10 half visible, so
  // the range grows to 11 rows (end is exclusive).
  it("includes partly visible rows at both edges", () => {
    expect(visibleRowRange(14, HEADER_HEIGHT + 10 * ROW_HEIGHT, 100)).toEqual({
      first: 0,
      end: 11,
    });
    expect(visibleRowRange(28 * 5 + 1, HEADER_HEIGHT + 10 * ROW_HEIGHT, 100)).toEqual({
      first: 5,
      end: 16,
    });
  });

  // Fewer rows than space: the range stops at the last row.
  it("never goes past the last row", () => {
    expect(visibleRowRange(0, 1000, 5)).toEqual({ first: 0, end: 5 });
  });

  // Scrolled far beyond the content (e.g. after deleting many rows): the
  // range is empty, never "first > end".
  it("is empty when scrolled past the end", () => {
    expect(visibleRowRange(10_000, 400, 10)).toEqual({ first: 10, end: 10 });
  });

  // No rows, or a window no taller than the header: nothing to draw.
  it("is empty with no rows or no body height", () => {
    expect(visibleRowRange(0, 400, 0)).toEqual({ first: 0, end: 0 });
    expect(visibleRowRange(0, HEADER_HEIGHT, 50)).toEqual({ first: 0, end: 0 });
    expect(visibleRowRange(0, 10, 50)).toEqual({ first: 0, end: 0 });
  });

  // A 10,000-row project only ever draws a screenful.
  it("keeps the range small for huge projects", () => {
    const range = visibleRowRange(28 * 5000, 1000, 10_000);
    expect(range.first).toBe(5000);
    expect(range.end - range.first).toBeLessThanOrEqual(Math.ceil((1000 - HEADER_HEIGHT) / 28) + 1);
  });
});

describe("rowTop / contentHeight", () => {
  // Row n starts n rows below the header, moved up by the scroll position.
  it("places rows below the header", () => {
    expect(rowTop(0, 0)).toBe(48);
    expect(rowTop(2, 10)).toBe(48 + 56 - 10);
  });

  // The scrollable height is header plus all rows.
  it("adds the header to the rows' height", () => {
    expect(contentHeight(0)).toBe(48);
    expect(contentHeight(10)).toBe(48 + 280);
  });
});

describe("backingStoreSize / crisp / physicalLineWidth (sharp lines on any screen)", () => {
  // On a "retina" screen (2 device pixels per CSS pixel) the canvas needs
  // twice as many pixels; bad ratios fall back to 1; sizes are never 0.
  it("scales the canvas by the device pixel ratio", () => {
    expect(backingStoreSize(300, 200, 2)).toEqual({ width: 600, height: 400 });
    expect(backingStoreSize(300, 200, 1.5)).toEqual({ width: 450, height: 300 });
    expect(backingStoreSize(300, 200, 0)).toEqual({ width: 300, height: 200 });
    expect(backingStoreSize(300, 200, Number.NaN)).toEqual({ width: 300, height: 200 });
    expect(backingStoreSize(0, 0, 2)).toEqual({ width: 1, height: 1 });
  });

  // A 1-pixel line is drawn on a half pixel so it covers exactly one row of
  // device pixels instead of blurring over two.
  it("snaps odd-width lines to half pixels", () => {
    expect(crisp(10.2, 1)).toBe(10.5);
    expect(crisp(10.2, 2)).toBe(10.25); // (round(20.4) + 0.5) / 2
    expect(crisp(10.2, 1, 2)).toBe(10); // even width: whole pixels
  });

  // Lines are at least one device pixel wide.
  it("rounds line width to whole device pixels", () => {
    expect(physicalLineWidth(1)).toBe(1);
    expect(physicalLineWidth(2)).toBe(2);
    expect(physicalLineWidth(0.5)).toBe(1);
  });
});
