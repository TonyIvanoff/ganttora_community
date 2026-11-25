/**
 * renderer/dayFill.ts: makes a full working day fill its whole day cell.
 *
 * What it is for:
 *   A one-day task runs from 08:00 to 17:00. Drawn exactly, its bar would
 *   start a third into the day cell and stop before its end. This file maps
 *   the *working hours* of a day onto the *whole* cell: 08:00 becomes the
 *   left edge, 17:00 the right edge, and a four-hour task half a cell.
 *
 * Where it fits:
 *   `GanttView.tsx` makes the mapping; `model.ts` uses it for every date it
 *   prepares for drawing. Only the picture changes: the real dates are kept
 *   beside it (`realStartMs`) and nothing is scheduled here (D16).
 *
 * Main ideas:
 *   - The place in the cell is *working minutes so far ÷ working minutes of
 *     the day*, so the lunch break takes no room.
 *   - A day off has no working minutes: its times stay where they are.
 *   - Each day is computed once per calendar and remembered (10,000 tasks
 *     often share the same few hundred days).
 */

import { blocksOn } from "@frontend/features/calendars/workingTime";
import type { Calendar, WorkingTime } from "@frontend/ipc/types";
import { DAY_MS, startOfDay } from "./dates";

/** Maps a real moment to the moment it is drawn at. */
export type DayFill = (ms: number, calendarId: string | null) => number;

/** The mapping that changes nothing (no calendars known). */
export const noDayFill: DayFill = (ms) => ms;

/**
 * Makes the mapping for a project.
 *
 * @param calendars - All calendars, with their working times.
 * @param projectCalendarId - Used for tasks without their own calendar.
 * @returns The mapping. `NaN` stays `NaN`; an unknown calendar changes
 *   nothing.
 *
 * @example
 * const fill = makeDayFill([standard], standard.id);
 * fill(Date.UTC(2027, 2, 1, 8), null);  // Mon 1 March, 00:00
 * fill(Date.UTC(2027, 2, 1, 17), null); // Tue 2 March, 00:00
 */
export function makeDayFill(
  calendars: readonly Calendar[],
  projectCalendarId: string | null,
): DayFill {
  const byId = new Map(calendars.map((calendar) => [calendar.id, calendar]));
  const days = new Map<string, Map<number, readonly WorkingTime[]>>();

  return (ms, calendarId) => {
    if (Number.isNaN(ms)) return ms;
    const calendar = byId.get(calendarId ?? projectCalendarId ?? "");
    if (calendar === undefined) return ms;
    const day = startOfDay(ms);
    let known = days.get(calendar.id);
    if (known === undefined) days.set(calendar.id, (known = new Map()));
    let blocks = known.get(day);
    if (blocks === undefined) known.set(day, (blocks = blocksOn(calendar, day)));

    const minute = (ms - day) / 60_000;
    let total = 0;
    let before = 0;
    for (const block of blocks) {
      total += block.endMinute - block.startMinute;
      before += Math.max(0, Math.min(minute, block.endMinute) - block.startMinute);
    }
    return total === 0 ? ms : day + (before / total) * DAY_MS;
  };
}
