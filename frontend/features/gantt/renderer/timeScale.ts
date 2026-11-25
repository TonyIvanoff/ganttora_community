/**
 * renderer/timeScale.ts: the maths that links dates to horizontal pixels.
 *
 * What it is for:
 *   A Gantt chart is a graph with time running left to right. To draw a bar
 *   we must know at which x position (in pixels) a date lies, and to draw the
 *   header we must know which days, weeks and months are on screen. This
 *   file answers exactly those questions, with pure functions.
 *
 * Main ideas:
 *   - A *time scale* is defined by an origin (the date at x = 0) and a zoom
 *     (pixels per day). Then: x = (date − origin) / one day × pixels per day.
 *   - The origin is a Monday a week before the first task, so the chart has
 *     some empty space on the left and weeks line up with the grid.
 *   - "Ticks" are the boundaries drawn in the header (each day, week or month).
 *     We only create the ticks that are visible: never 3 years of days.
 */

import { DAY_MS, WEEK_MS, mondayOnOrBefore, startOfDay } from "./dates";

/** The time units the header can show. */
export type TickUnit = "day" | "week" | "month" | "year";

/**
 * Names of the timescale levels offered in the toolbar. They choose the
 * header's units; how wide everything is drawn is the zoom *percentage*.
 */
export type ZoomLevelId = "days" | "weeks" | "months";

/** How one zoom level looks. */
export interface ZoomLevel {
  /** Width of one day in CSS pixels at 100 %. */
  pxPerDay: number;
  /** Unit of the upper header row (the bigger unit). */
  topUnit: TickUnit;
  /** Unit of the lower header row (the smaller unit). */
  bottomUnit: TickUnit;
}

/**
 * The available zoom levels.
 *
 * - days: 28 px per day: room for a day number in each header cell.
 * - weeks: 7 px per day = 49 px per week: room for a short date per week.
 * - months: 2 px per day ≈ 60 px per month: room for a short month name;
 *   the upper tier shows years. Good for projects that run for years.
 */
export const ZOOM_LEVELS: Readonly<Record<ZoomLevelId, ZoomLevel>> = {
  days: { pxPerDay: 28, topUnit: "month", bottomUnit: "day" },
  weeks: { pxPerDay: 7, topUnit: "month", bottomUnit: "week" },
  months: { pxPerDay: 2, topUnit: "year", bottomUnit: "month" },
};

/** Smallest zoom, in percent. */
export const MIN_ZOOM_PERCENT = 10;
/** Largest zoom, in percent. */
export const MAX_ZOOM_PERCENT = 400;
/** How much one press of Zoom in / Zoom out changes, in percentage points. */
export const ZOOM_STEP_PERCENT = 10;

/**
 * Keeps a zoom percentage inside the limits and makes it a whole number.
 *
 * @param percent - Any number.
 * @returns A whole number from 10 to 400; 100 for "not a number".
 *
 * @example
 * clampZoomPercent(1000); // 400
 */
export function clampZoomPercent(percent: number): number {
  if (Number.isNaN(percent)) return 100;
  return Math.min(MAX_ZOOM_PERCENT, Math.max(MIN_ZOOM_PERCENT, Math.round(percent)));
}

/**
 * The next zoom percentage for Zoom in / Zoom out: the next multiple of 10.
 *
 * @param percent - The zoom now.
 * @param direction - `"in"` (bigger) or `"out"` (smaller).
 * @returns The new zoom; unchanged at the limits.
 *
 * @example
 * stepZoomPercent(100, "in");  // 110
 * stepZoomPercent(117, "out"); // 110 (first back to a round number)
 */
export function stepZoomPercent(percent: number, direction: "in" | "out"): number {
  const steps = percent / ZOOM_STEP_PERCENT;
  const next = direction === "in" ? Math.floor(steps) + 1 : Math.ceil(steps) - 1;
  return clampZoomPercent(next * ZOOM_STEP_PERCENT);
}

/**
 * The zoom after turning the mouse wheel (or pinching a trackpad).
 *
 * @param percent - The zoom now (may have decimals).
 * @param deltaY - The wheel movement; negative = away from the user = in.
 * @returns The new zoom, *not* rounded: a trackpad sends many tiny
 *   movements, and rounding each one would lose them.
 *
 * @example
 * wheelZoomPercent(100, -100); // about 122 (one notch of a mouse wheel)
 *
 * Beginner note: `Math.exp` makes every notch change the zoom by the same
 * *proportion*, so zooming feels equally fast at 20 % and at 300 %.
 */
export function wheelZoomPercent(percent: number, deltaY: number): number {
  // Fast wheels send huge values: limit one event to a comfortable step.
  const delta = Math.min(50, Math.max(-50, deltaY));
  const next = percent * Math.exp(-delta * 0.004);
  return Math.min(MAX_ZOOM_PERCENT, Math.max(MIN_ZOOM_PERCENT, next));
}

/**
 * How many header cells share one label, so labels stay readable when the
 * chart is zoomed far out.
 *
 * @param unit - The unit of the header row.
 * @param unitWidth - Width of one cell in CSS pixels.
 * @param isTopTier - Upper row ("January 2027") or lower row ("Jan").
 * @returns 1 = every cell has a label, 7 = every seventh…
 *
 * @example
 * labelStride("day", 28, false); // 1
 * labelStride("day", 5, false);  // 7: one label per week
 */
export function labelStride(unit: TickUnit, unitWidth: number, isTopTier: boolean): number {
  const needed =
    unit === "day" ? 18 : unit === "week" ? 44 : unit === "year" ? 36 : isTopTier ? 90 : 28;
  // Steps that fall on natural borders: weeks, quarters, half years, decades.
  const steps =
    unit === "day"
      ? [1, 7, 14, 28, 56]
      : unit === "week"
        ? [1, 2, 4, 8, 16, 32, 64]
        : unit === "month"
          ? [1, 3, 6, 12]
          : [1, 2, 5, 10, 50];
  return steps.find((n) => n * unitWidth >= needed) ?? (steps[steps.length - 1] as number);
}

/**
 * The position of a header cell in its natural count (days and weeks from
 * the start of the scale, months in the year, the year number). Cells whose
 * index divides by the {@link labelStride} carry the label.
 *
 * @param scale - The time scale.
 * @param unit - The unit of the cell.
 * @param ms - The start of the cell.
 * @returns A whole number, 0 or more.
 */
export function tickIndex(scale: TimeScale, unit: TickUnit, ms: number): number {
  if (unit === "day") return Math.round((ms - scale.originMs) / DAY_MS);
  if (unit === "week") return Math.round((ms - scale.originMs) / WEEK_MS);
  const date = new Date(ms);
  return unit === "month" ? date.getUTCMonth() : date.getUTCFullYear();
}

/** Empty days shown before the first task. */
const PADDING_BEFORE_DAYS = 7;
/** Empty days shown after the last task. */
const PADDING_AFTER_DAYS = 21;

/** A complete time scale: where x = 0 is, how wide a day is, total width. */
export interface TimeScale {
  /** The date (ms, a Monday at 00:00) drawn at x = 0. */
  originMs: number;
  /** Width of one day in CSS pixels. */
  pxPerDay: number;
  /** Width of the whole chart in CSS pixels. */
  totalWidth: number;
  /** The zoom level this scale was built from. */
  zoom: ZoomLevel;
}

/**
 * Builds a time scale that covers a date range.
 *
 * @param minMs - Earliest date that must be visible (ms). `NaN` = no tasks.
 * @param maxMs - Latest date that must be visible (ms).
 * @param zoom - The zoom level (Days, Weeks or Months).
 * @param fallbackMs - Date to show when there are no tasks (usually "today",
 *   passed in by the caller so that this function stays pure).
 * @param percent - The zoom in percent; 100 = the level's normal width.
 * @returns The scale. With no tasks (`NaN`), it shows 8 weeks starting
 *   around `fallbackMs`.
 *
 * @example
 * const scale = createTimeScale(Date.UTC(2027, 0, 6), Date.UTC(2027, 1, 1), ZOOM_LEVELS.days, Date.now());
 * scale.originMs === Date.UTC(2026, 11, 28); // Monday, one week before
 */
export function createTimeScale(
  minMs: number,
  maxMs: number,
  zoom: ZoomLevel,
  fallbackMs: number,
  percent = 100,
): TimeScale {
  const hasRange = Number.isFinite(minMs) && Number.isFinite(maxMs);
  const from = hasRange ? minMs : fallbackMs;
  const to = hasRange ? maxMs : fallbackMs + 8 * WEEK_MS;
  const originMs = mondayOnOrBefore(from - PADDING_BEFORE_DAYS * DAY_MS);
  const endMs = startOfDay(to) + PADDING_AFTER_DAYS * DAY_MS;
  const days = Math.ceil((endMs - originMs) / DAY_MS);
  const pxPerDay = (zoom.pxPerDay * percent) / 100;
  return { originMs, pxPerDay, totalWidth: days * pxPerDay, zoom };
}

/**
 * Converts a date to an x position.
 *
 * @param scale - The time scale.
 * @param ms - The date (ms).
 * @returns x in CSS pixels from the left edge of the whole chart (not the
 *   screen; subtract the horizontal scroll position for that).
 *
 * @example
 * msToX(scale, scale.originMs + DAY_MS) === scale.pxPerDay;
 */
export function msToX(scale: TimeScale, ms: number): number {
  return ((ms - scale.originMs) / DAY_MS) * scale.pxPerDay;
}

/**
 * Converts an x position back to a date: the inverse of {@link msToX}.
 *
 * @param scale - The time scale.
 * @param x - CSS pixels from the left edge of the whole chart.
 * @returns The date (ms) at that position.
 */
export function xToMs(scale: TimeScale, x: number): number {
  return scale.originMs + (x / scale.pxPerDay) * DAY_MS;
}

/** One header cell: a time unit and where it is drawn. */
export interface Tick {
  /** Start of the unit (ms), e.g. midnight of a day or the 1st of a month. */
  ms: number;
  /** Left edge in chart pixels. */
  x: number;
  /** Width in pixels (until the start of the next unit). */
  width: number;
}

/**
 * Lists the header cells of one unit that overlap a horizontal range.
 *
 * @param scale - The time scale.
 * @param unit - "day", "week" (Monday-based), "month" or "year".
 * @param x0 - Left edge of the range, chart pixels (usually the scroll position).
 * @param x1 - Right edge of the range, chart pixels.
 * @returns Ticks from left to right. The first may start before `x0` and the
 *   last may end after `x1` (they are partly visible).
 *
 * @example
 * visibleTicks(scale, "day", 0, 100).length; // 4 days at 28 px per day
 */
export function visibleTicks(scale: TimeScale, unit: TickUnit, x0: number, x1: number): Tick[] {
  const ticks: Tick[] = [];
  if (x1 <= x0) return ticks;

  if (unit === "day" || unit === "week") {
    // Days and weeks have a fixed length, so we can jump straight to the
    // first visible one with a division, then step forward.
    const stepDays = unit === "day" ? 1 : 7;
    const stepPx = stepDays * scale.pxPerDay;
    // The origin is a Monday, so week boundaries are multiples of stepPx.
    const first = Math.max(0, Math.floor(x0 / stepPx));
    const last = Math.ceil(x1 / stepPx);
    for (let i = first; i < last; i++) {
      ticks.push({ ms: scale.originMs + i * stepDays * DAY_MS, x: i * stepPx, width: stepPx });
    }
    return ticks;
  }

  // Months and years have different lengths (28 to 31 days, 365 or 366),
  // so we walk step by step with Date.UTC, which handles the varying
  // lengths and leap years. A year is simply a step of 12 months.
  const monthStep = unit === "year" ? 12 : 1;
  const startDate = new Date(xToMs(scale, Math.max(0, x0)));
  let year = startDate.getUTCFullYear();
  let month = unit === "year" ? 0 : startDate.getUTCMonth();
  for (;;) {
    const ms = Date.UTC(year, month, 1);
    const nextMs = Date.UTC(year, month + monthStep, 1);
    const x = msToX(scale, ms);
    if (x >= x1) break;
    ticks.push({ ms, x, width: msToX(scale, nextMs) - x });
    month += monthStep;
    if (month >= 12) {
      month -= 12;
      year++;
    }
  }
  return ticks;
}
