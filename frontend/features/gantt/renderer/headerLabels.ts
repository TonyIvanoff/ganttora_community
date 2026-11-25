/**
 * renderer/headerLabels.ts: the text in the timescale header ("January 2027", "4"…).
 *
 * What it is for:
 *   Month and day names differ per language ("January", "Januar", "一月"),
 *   so we never write them ourselves. The browser's built-in `Intl` library
 *   formats dates for any language (`Intl` for dates).
 *
 * Main ideas:
 *   - Creating an `Intl.DateTimeFormat` is slow; using one is fast. So we
 *     create the formatters once per language.
 *   - The same header cell is drawn on every frame while scrolling. We keep
 *     already-formatted labels in a cache (a `Map`), so each label is
 *     formatted only once.
 *   - `timeZone: "UTC"`, because the renderer does all date math in UTC
 *     (see dates.ts).
 */

import type { TickUnit } from "./timeScale";

/** A function that returns the header text for a unit starting at `ms`. */
export type HeaderLabeler = (unit: TickUnit, ms: number, isTopTier: boolean) => string;

/** Above this many cached labels, the cache is emptied (keeps memory small). */
const MAX_CACHE_SIZE = 5_000;

/**
 * Creates a labeler for one language.
 *
 * @param locale - A language tag such as "en", "de" or "zh-CN".
 * @returns A function giving the label for a header cell:
 *   - year: "2027" (top tier of the "months" zoom),
 *   - month: "January 2027" (top tier), "Jan" (bottom tier),
 *   - week: "Jan 4" (the Monday's date),
 *   - day: "4" (day of the month).
 *
 * @example
 * const label = createHeaderLabeler("en");
 * label("month", Date.UTC(2027, 0, 1), true); // "January 2027"
 */
export function createHeaderLabeler(locale: string): HeaderLabeler {
  const monthFormat = new Intl.DateTimeFormat(locale, {
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  });
  const weekFormat = new Intl.DateTimeFormat(locale, {
    month: "short",
    day: "numeric",
    timeZone: "UTC",
  });
  const dayFormat = new Intl.DateTimeFormat(locale, { day: "numeric", timeZone: "UTC" });
  // Short month name for the lower tier of the "months" zoom ("Jan", "Jän.", "1月").
  const shortMonthFormat = new Intl.DateTimeFormat(locale, { month: "short", timeZone: "UTC" });
  const yearFormat = new Intl.DateTimeFormat(locale, { year: "numeric", timeZone: "UTC" });
  const cache = new Map<string, string>();

  return (unit, ms, isTopTier) => {
    const key = `${unit}:${ms}:${isTopTier ? 1 : 0}`;
    const cached = cache.get(key);
    if (cached !== undefined) return cached;

    let text: string;
    if (unit === "year") text = yearFormat.format(ms);
    else if (unit === "month") text = (isTopTier ? monthFormat : shortMonthFormat).format(ms);
    else if (unit === "week") text = weekFormat.format(ms);
    else text = dayFormat.format(ms);

    if (cache.size >= MAX_CACHE_SIZE) cache.clear();
    cache.set(key, text);
    return text;
  };
}
