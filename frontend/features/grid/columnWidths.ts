/**
 * features/grid/columnWidths.ts: the task grid's column widths: defaults,
 * limits, and the widths to draw with.
 *
 * What it is for:
 *   The user can drag the right edge of a column header to make the column
 *   wider or narrower. The new width is remembered in the app settings
 *   (`settings.json`, field `grid.columnWidths`), so the grid looks the
 *   same at the next start. This file holds the plain rules for that: the
 *   default width of each column, the narrowest and widest allowed width,
 *   and how saved widths and a drag in progress combine.
 *
 * Where it fits:
 *   - `TaskGrid.tsx` calls {@link resolveColumnWidths} and
 *     {@link columnLefts} on every render and {@link clampColumnWidth}
 *     while a column edge is dragged.
 *   - `features/settings/options.ts` uses {@link DEFAULT_COLUMN_WIDTHS} for
 *     the browser preview's default settings.
 *   - Rust keeps the same defaults and limits (`DEFAULT_COLUMN_WIDTHS`,
 *     `MIN_COLUMN_WIDTH`, `MAX_COLUMN_WIDTH` in
 *     `backend/src-tauri/src/settings.rs`) and cleans the saved values.
 *
 * Main ideas:
 *   - **Saved widths may be incomplete or odd.** The file can come from an
 *     older version (fewer columns) or be edited by hand. A missing or
 *     unusable width falls back to the default; numbers are kept inside
 *     the limits. So the grid can always be drawn.
 *   - **Pure functions.** Nothing here touches the page or the stores, so
 *     the rules are easy to test.
 */

import { GRID_COLUMN_IDS, type GridColumnId } from "./cellEdit";

/** Narrowest a column may be dragged, in CSS pixels (same as Rust). */
export const MIN_COLUMN_WIDTH = 24;

/** Widest a column may be dragged, in CSS pixels (same as Rust). */
export const MAX_COLUMN_WIDTH = 1200;

/**
 * Default width of each column in CSS pixels. Long German titles wrap to
 * two lines in the header. Must match `DEFAULT_COLUMN_WIDTHS` in
 * `backend/src-tauri/src/settings.rs`.
 */
export const DEFAULT_COLUMN_WIDTHS: Readonly<Record<GridColumnId, number>> = {
  rowNumber: 44,
  indicators: 164,
  outline: 56,
  name: 240,
  duration: 84,
  complete: 80,
  start: 136,
  finish: 136,
  plannedFinish: 136,
  actualFinish: 136,
  predecessors: 120,
  mode: 84,
};

/**
 * Default widths of earlier versions. Rust stores *all* widths, so a saved
 * old default was not chosen by the user: the new default replaces it.
 */
const EARLIER_WIDTHS: Readonly<Partial<Record<GridColumnId, readonly number[]>>> = {
  indicators: [40, 124],
};

/**
 * Keeps a width inside the allowed range and makes it a whole number.
 *
 * @param width - A width in CSS pixels (may be a fraction, negative, or
 *   not a number at all).
 * @param fallback - Used when `width` is not a finite number.
 * @returns A whole number between {@link MIN_COLUMN_WIDTH} and
 *   {@link MAX_COLUMN_WIDTH}.
 *
 * @example
 * clampColumnWidth(10.4, 84);  // 24 (too narrow)
 * clampColumnWidth(300.6, 84); // 301
 * clampColumnWidth(NaN, 84);   // 84
 */
export function clampColumnWidth(width: number, fallback: number): number {
  if (!Number.isFinite(width)) return fallback;
  return Math.min(MAX_COLUMN_WIDTH, Math.max(MIN_COLUMN_WIDTH, Math.round(width)));
}

/**
 * The width to draw each column with.
 *
 * @param saved - The widths from the settings (`grid.columnWidths`); keys
 *   that are not columns are ignored, missing columns use the default.
 * @param live - Widths of a drag in progress, which win over `saved`
 *   (empty when nothing is dragged).
 * @returns A width for every column, inside the allowed range.
 *
 * @example
 * resolveColumnWidths({ name: 300, unknown: 50 }, {}).name;        // 300
 * resolveColumnWidths({ name: 300 }, { name: 320 }).name;          // 320
 * resolveColumnWidths({}, {}).duration;                            // 84 (default)
 */
export function resolveColumnWidths(
  saved: Readonly<Record<string, number>>,
  live: Readonly<Partial<Record<GridColumnId, number>>>,
): Record<GridColumnId, number> {
  // `{} as Record<…>`: we fill every column in the loop below, so the
  // object is complete when it is returned.
  const result = {} as Record<GridColumnId, number>;
  for (const id of GRID_COLUMN_IDS) {
    const fallback = DEFAULT_COLUMN_WIDTHS[id];
    // `??` takes the right side only when the left is `undefined`/`null`.
    const stored = EARLIER_WIDTHS[id]?.includes(saved[id] ?? NaN) === true ? undefined : saved[id];
    result[id] = clampColumnWidth(live[id] ?? stored ?? fallback, fallback);
  }
  return result;
}

/**
 * The left edge of each column, counted from the grid's left side.
 *
 * @param widths - A width for every column (see {@link resolveColumnWidths}).
 * @returns The left edges, plus `total`: the width of all columns together.
 *
 * @example
 * const { left, total } = columnLefts(resolveColumnWidths({}, {}));
 * left.indicators; // 44 (right after the 44-pixel row-number column)
 */
export function columnLefts(widths: Readonly<Record<GridColumnId, number>>): {
  left: Record<GridColumnId, number>;
  total: number;
} {
  const left = {} as Record<GridColumnId, number>;
  let x = 0;
  for (const id of GRID_COLUMN_IDS) {
    left[id] = x;
    x += widths[id];
  }
  return { left, total: x };
}
