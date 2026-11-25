/**
 * renderer/dates.ts: small, pure date helpers for the Gantt renderer.
 *
 * What it is for:
 *   The renderer works with dates as plain numbers: milliseconds since
 *   1 January 1970, in UTC. Numbers are fast to compare and subtract, which
 *   matters when we position thousands of bars. These helpers convert the
 *   ISO text from the schedule into such numbers and round them to days and
 *   weeks.
 *
 * Main ideas:
 *   - Schedule dates have no time zone ("floating" time, like a wall clock).
 *     We *treat* them as UTC for all math. That way a daylight-saving change
 *     can never move a bar by an hour, and every day is exactly 24 h long.
 *   - Every function here is *pure*: same input, same output, no side
 *     effects. Pure functions are easy to unit-test (tests/frontend/gantt).
 */

/** Milliseconds in one day (24 × 60 × 60 × 1000). */
export const DAY_MS = 86_400_000;

/** Milliseconds in one week. */
export const WEEK_MS = 7 * DAY_MS;

/**
 * Parses an ISO 8601 date-time without a time zone, treating it as UTC.
 *
 * @param iso - Text like "2027-01-04T08:00:00" or "2027-01-04".
 * @returns Milliseconds since 1970 (UTC), or `NaN` if the text is not a date.
 *
 * @example
 * parseIsoLocal("1970-01-02T00:00:00"); // 86_400_000
 *
 * Why not `new Date(iso)`? For a date-time *without* "Z", JavaScript
 * assumes the computer's local time zone, so the result would depend on
 * where the user lives. Adding "Z" forces UTC.
 */
export function parseIsoLocal(iso: string): number {
  // Date-only text ("2027-01-04") is already read as UTC by JavaScript.
  const text = iso.length > 10 && !iso.endsWith("Z") ? `${iso}Z` : iso;
  return Date.parse(text);
}

/**
 * Rounds a moment down to midnight of the same day.
 *
 * @param ms - Milliseconds (UTC).
 * @returns Milliseconds of 00:00 on that day.
 *
 * @example
 * startOfDay(Date.UTC(2027, 0, 4, 15, 30)) === Date.UTC(2027, 0, 4);
 */
export function startOfDay(ms: number): number {
  return Math.floor(ms / DAY_MS) * DAY_MS;
}

/**
 * Gives the day of the week, Monday-based.
 *
 * @param ms - Milliseconds (UTC).
 * @returns 0 = Monday, 1 = Tuesday, … 5 = Saturday, 6 = Sunday.
 *
 * Note: JavaScript's own `getUTCDay()` starts with 0 = Sunday. Project
 * planning tools usually start weeks on Monday, so we shift by 6.
 */
export function weekdayMondayBased(ms: number): number {
  return (new Date(ms).getUTCDay() + 6) % 7;
}

/**
 * Finds midnight of the Monday on or before a moment.
 *
 * @param ms - Milliseconds (UTC).
 * @returns Milliseconds of 00:00 on that Monday.
 *
 * @example
 * // Wednesday 6 Jan 2027 → Monday 4 Jan 2027
 * mondayOnOrBefore(Date.UTC(2027, 0, 6)) === Date.UTC(2027, 0, 4);
 */
export function mondayOnOrBefore(ms: number): number {
  const day = startOfDay(ms);
  return day - weekdayMondayBased(day) * DAY_MS;
}

/**
 * Tells whether a day is Saturday or Sunday.
 *
 * In M0 the chart shades weekends as non-working time. From M1 the real
 * project calendar (from Rust) decides which days are non-working.
 *
 * @param ms - Milliseconds (UTC) of any moment in that day.
 * @returns `true` for Saturday and Sunday.
 */
export function isWeekend(ms: number): boolean {
  return weekdayMondayBased(ms) >= 5;
}

/**
 * The user's current day, in the form all chart dates have.
 *
 * @param now - The moment (default: now). Tests pass a fixed one.
 * @returns Midnight of the *local* calendar day, as UTC milliseconds.
 *
 * @example
 * localToday(new Date(2026, 10, 25, 14)) === Date.UTC(2026, 10, 25);
 */
export function localToday(now: Date = new Date()): number {
  return Date.UTC(now.getFullYear(), now.getMonth(), now.getDate());
}
