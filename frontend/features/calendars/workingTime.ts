/**
 * features/calendars/workingTime.ts: small, pure helpers for reading a calendar.
 *
 * What it is for:
 *   The UI needs to *look at* a calendar in a few places: the Gantt chart
 *   shades non-working days and snaps a dragged bar to a working day; the
 *   calendar editor shows working times as "08:00" text and checks what the
 *   user typed. These helpers answer simple questions like "which working
 *   blocks does this day have?" and "is 13:00–12:00 a valid block?".
 *
 * Where it fits:
 *   - `features/gantt/GanttView.tsx` and `features/gantt/interactions.ts`
 *     use {@link isWorkingDay} and {@link snapToWorkingDay}.
 *   - `features/calendars/CalendarDialog.tsx` uses the text helpers and
 *     {@link blocksProblem}.
 *
 * Main ideas:
 *   - **No scheduling here** (decision D16). Counting working minutes or
 *     adding working time is done by Rust (`calendarWorkingMinutes`,
 *     `calendarAddWorkingTime` in `ipc/client.ts`). This file only reads
 *     which days and hours are working, with the *same* rule as the engine
 *     (`blocks_on` in `backend/crates/engine/src/calendar_math.rs`):
 *     an exception covering the day wins (the first one, if several do),
 *     then a holiday (no work), then the normal week.
 *   - Times of day are *minutes after midnight*: 480 = 08:00, 1020 = 17:00.
 *   - Days are UTC milliseconds at 00:00, like everywhere in the Gantt
 *     renderer (`features/gantt/renderer/dates.ts`).
 */

import { DAY_MS, startOfDay, weekdayMondayBased } from "@frontend/features/gantt/renderer/dates";
import type { Calendar, IsoDate, IsoDateTime, WorkWeek, WorkingTime } from "@frontend/ipc/types";

/** Minutes in a day on the clock (24 × 60). A block may end at 1440 (midnight). */
export const MINUTES_PER_CLOCK_DAY = 1440;

/**
 * The weekday keys of a {@link WorkWeek}, Monday first (index 0 = Monday,
 * matching `weekdayMondayBased`).
 *
 * `as const` keeps the exact strings, so `week[WEEKDAYS[i]]` is type-checked.
 */
export const WEEKDAYS = [
  "monday",
  "tuesday",
  "wednesday",
  "thursday",
  "friday",
  "saturday",
  "sunday",
] as const satisfies readonly (keyof WorkWeek)[];

/** One weekday key, e.g. "monday". */
export type WeekdayKey = (typeof WEEKDAYS)[number];

/**
 * Turns a UTC-midnight millisecond value into an ISO date ("2027-03-01").
 *
 * @param dayMs - Any moment of the day (ms, UTC).
 * @returns The date text, 10 characters.
 *
 * @example
 * isoDate(Date.UTC(2027, 2, 1)); // "2027-03-01"
 */
export function isoDate(dayMs: number): IsoDate {
  return new Date(dayMs).toISOString().slice(0, 10);
}

/**
 * Turns milliseconds into an ISO date-time without time zone, the form Rust
 * expects ("2027-03-01T08:00:00").
 *
 * @param ms - The moment (ms, UTC).
 * @returns The text, 19 characters (seconds included, no milliseconds).
 *
 * @example
 * isoDateTime(Date.UTC(2027, 2, 1, 8)); // "2027-03-01T08:00:00"
 */
export function isoDateTime(ms: number): IsoDateTime {
  return new Date(ms).toISOString().slice(0, 19);
}

/**
 * Gives the working blocks of one day, with the engine's rule (see the
 * file header: exception, then holiday, then the normal week).
 *
 * @param calendar - The calendar.
 * @param dayMs - Any moment of the day (ms, UTC).
 * @returns The blocks (empty = a day off). Do not change the returned array:
 *   it is the calendar's own.
 *
 * @example
 * blocksOn(standard, Date.UTC(2027, 2, 1)); // Monday: [{480,720},{780,1020}]
 */
export function blocksOn(calendar: Calendar, dayMs: number): readonly WorkingTime[] {
  const date = isoDate(dayMs);
  // ISO dates compare correctly as text: "2027-03-01" < "2027-03-15".
  const exception = (calendar.exceptions ?? []).find((e) => e.start <= date && date <= e.finish);
  if (exception !== undefined) return exception.workingTimes;
  if (calendar.holidays.includes(date)) return [];
  const key = WEEKDAYS[weekdayMondayBased(dayMs)];
  return key === undefined ? [] : calendar.week[key];
}

/**
 * Tells whether a day has any working time.
 *
 * @param calendar - The calendar.
 * @param dayMs - Any moment of the day (ms, UTC).
 * @returns `true` for a working day.
 */
export function isWorkingDay(calendar: Calendar, dayMs: number): boolean {
  return blocksOn(calendar, dayMs).length > 0;
}

/** How far {@link snapToWorkingDay} looks for a working day (about 3 years). */
const MAX_SNAP_DAYS = 1100;

/**
 * Moves a day to the nearest working day in one direction and gives the
 * first working minute of it (for a start) or the last (for a finish).
 *
 * @param calendar - The calendar.
 * @param dayMs - Any moment of the day the user pointed at (ms, UTC).
 * @param edge - `"start"`: look forward and return the day's first working
 *   moment (e.g. Monday 08:00). `"finish"`: look backward and return the
 *   day's last working moment (e.g. Friday 17:00).
 * @returns The moment (ms), or `NaN` if no working day was found nearby
 *   (a calendar without working time).
 *
 * @example
 * // Saturday 6 March 2027 on the standard calendar:
 * snapToWorkingDay(standard, Date.UTC(2027, 2, 6), "start");  // Mon 8 Mar 08:00
 * snapToWorkingDay(standard, Date.UTC(2027, 2, 6), "finish"); // Fri 5 Mar 17:00
 */
export function snapToWorkingDay(
  calendar: Calendar,
  dayMs: number,
  edge: "start" | "finish",
): number {
  const step = edge === "start" ? DAY_MS : -DAY_MS;
  let day = startOfDay(dayMs);
  for (let i = 0; i < MAX_SNAP_DAYS; i++, day += step) {
    const blocks = blocksOn(calendar, day);
    const first = blocks[0];
    const last = blocks[blocks.length - 1];
    if (first === undefined || last === undefined) continue;
    const minute = edge === "start" ? first.startMinute : last.endMinute;
    return day + minute * 60_000;
  }
  return Number.NaN;
}

/**
 * Writes minutes after midnight as "HH:MM" (the value of an
 * `<input type="time">`).
 *
 * @param minutes - 0 to 1440.
 * @returns E.g. "08:00"; 1440 is written "24:00".
 *
 * @example
 * minutesToTime(780); // "13:00"
 */
export function minutesToTime(minutes: number): string {
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  // `padStart(2, "0")` turns "8" into "08".
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
}

/**
 * Reads "HH:MM" text into minutes after midnight.
 *
 * @param text - E.g. "08:00" or "8:30"; "24:00" is allowed (end of day).
 * @returns The minutes, or `null` if the text is not a valid time.
 *
 * @example
 * timeToMinutes("13:30"); // 810
 * timeToMinutes("25:00"); // null
 */
export function timeToMinutes(text: string): number | null {
  // A *regular expression*: one or two digits, a colon, two digits.
  const match = /^(\d{1,2}):(\d{2})$/.exec(text.trim());
  if (match === null) return null;
  const h = Number(match[1]);
  const m = Number(match[2]);
  if (m > 59) return null;
  const total = h * 60 + m;
  return total <= MINUTES_PER_CLOCK_DAY ? total : null;
}

/**
 * Checks a list of working blocks with the engine's rules: each block ends
 * after it starts, ends at 24:00 at the latest, and starts at or after the
 * end of the block before it.
 *
 * @param blocks - The blocks in order.
 * @returns `null` if they are fine, otherwise the index of the first bad
 *   block.
 *
 * @example
 * blocksProblem([{ startMinute: 480, endMinute: 720 }]);  // null
 * blocksProblem([{ startMinute: 720, endMinute: 480 }]);  // 0
 */
export function blocksProblem(blocks: readonly WorkingTime[]): number | null {
  let previousEnd = 0;
  for (let i = 0; i < blocks.length; i++) {
    const block = blocks[i];
    if (block === undefined) continue;
    const broken =
      block.startMinute >= block.endMinute ||
      block.endMinute > MINUTES_PER_CLOCK_DAY ||
      block.startMinute < previousEnd;
    if (broken) return i;
    previousEnd = block.endMinute;
  }
  return null;
}

/**
 * Adds up the working minutes of a list of blocks (for "8 h" labels).
 *
 * @param blocks - The blocks.
 * @returns The total minutes, e.g. 480 for 08:00–12:00 + 13:00–17:00.
 */
export function blocksMinutes(blocks: readonly WorkingTime[]): number {
  return blocks.reduce((sum, b) => sum + Math.max(0, b.endMinute - b.startMinute), 0);
}

/**
 * The usual working day: 08:00–12:00 and 13:00–17:00 (as MS Project's
 * "Standard" calendar). Used for new calendars and new working days.
 *
 * @returns A fresh array (safe to change).
 */
export function standardDay(): WorkingTime[] {
  return [
    { startMinute: 480, endMinute: 720 },
    { startMinute: 780, endMinute: 1020 },
  ];
}

/**
 * The usual week: Monday to Friday {@link standardDay}, weekend off.
 *
 * @returns A fresh week (safe to change).
 */
export function standardWeek(): WorkWeek {
  return {
    monday: standardDay(),
    tuesday: standardDay(),
    wednesday: standardDay(),
    thursday: standardDay(),
    friday: standardDay(),
    saturday: [],
    sunday: [],
  };
}

/**
 * How much of a task should be done by today, for a warning only (nothing
 * is scheduled with it). Whole working days before today count as done;
 * today itself does not, because its work is still going on.
 *
 * @param calendar - The task's calendar.
 * @param startMs - The task's start.
 * @param finishMs - The task's finish.
 * @param todayMs - Today at 00:00.
 * @param blocked - Blocked pieces: their full days are not work.
 * @returns 0–100, rounded.
 *
 * @example
 * // Tue 17 – Wed 25 Nov 2026 (7 working days), today Mon 23 Nov: 4 of 7.
 * expectedPercent(standard, start, finish, Date.UTC(2026, 10, 23)); // 57
 */
export function expectedPercent(
  calendar: Calendar,
  startMs: number,
  finishMs: number,
  todayMs: number,
  blocked: readonly { startMs: number; finishMs: number }[] = [],
): number {
  let total = 0;
  let done = 0;
  for (let day = startOfDay(startMs); day < finishMs; day += DAY_MS) {
    const blocks = blocksOn(calendar, day);
    const first = blocks[0];
    const last = blocks.at(-1);
    if (first === undefined || last === undefined) continue;
    const from = Math.max(startMs, day + first.startMinute * 60_000);
    const to = Math.min(finishMs, day + last.endMinute * 60_000);
    if (blocked.some((b) => b.startMs <= from && b.finishMs >= to)) continue;
    total++;
    if (day < todayMs) done++;
  }
  return total === 0 ? 0 : Math.round((done / total) * 100);
}
