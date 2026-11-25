/**
 * i18n/dateInput.ts: writes and reads the short dates of date fields.
 *
 * What it is for:
 *   A date field shows "2027-03-24", "24/03/2027" or "24 Mar 2027",
 *   depending on Settings → General → Date format, and must
 *   understand the same when the user types it.
 *
 * Where it fits:
 *   Used by `components/ui/DateField.tsx`. Pure functions: text in, text out.
 *
 * Main ideas:
 *   - Inside the app a day is always the text "YYYY-MM-DD". Only the field
 *     shows it differently.
 *   - The pattern tells the order of day, month and year.
 */

import { monthNamesOf, readDateStyle, writeDay } from "./format";

/** One part of a date. */
type DatePart = "year" | "month" | "day";

/** What a field reports while its text is not a date. Never a valid day. */
export const INVALID_DAY = "invalid";

/** Milliseconds in a day. */
const DAY_MS = 86_400_000;

/**
 * Builds "YYYY-MM-DD" if the day exists.
 *
 * @param year - Full year.
 * @param month - 1 to 12.
 * @param day - 1 to 31.
 * @returns The day, or `null` (e.g. for 31 February).
 */
function makeDay(year: number, month: number, day: number): string | null {
  if (year < 1900 || year > 9999) return null;
  const date = new Date(Date.UTC(year, month - 1, day));
  // JavaScript turns 31 February into 3 March; then the month differs.
  if (date.getUTCMonth() !== month - 1 || date.getUTCDate() !== day) return null;
  return date.toISOString().slice(0, 10);
}

/**
 * Checks for a real day.
 *
 * @param text - Any text.
 * @returns `true` for an existing "YYYY-MM-DD" day.
 *
 * @example
 * isDay("2027-02-31"); // false
 */
export function isDay(text: string): boolean {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text);
  return match !== null && makeDay(Number(match[1]), Number(match[2]), Number(match[3])) !== null;
}

/**
 * Writes a day for a date field.
 *
 * @param day - "YYYY-MM-DD".
 * @param style - The date style, from `useFormatLocale().dateLocale`.
 * @returns E.g. "24/03/2027"; "" if `day` is not a day.
 *
 * @example
 * formatDay("2027-03-24", "en|dd/mm/yyyy"); // "24/03/2027"
 */
export function formatDay(day: string, style: string): string {
  return isDay(day) ? writeDay(day, style) : "";
}

/**
 * The order in which a pattern writes a date.
 *
 * @param pattern - E.g. "dd.mm.yyyy".
 * @returns E.g. `["day", "month", "year"]`.
 */
function partOrder(pattern: string): DatePart[] {
  return (pattern.match(/yyyy|mmm|mm|dd/g) ?? []).map((part) =>
    part === "yyyy" ? "year" : part === "dd" ? "day" : "month",
  );
}

/**
 * Finds the month a word stands for.
 *
 * @param word - E.g. "mar" or "March".
 * @param language - UI language.
 * @returns 1 to 12, or `null` if no month or several start like that.
 */
function monthOfWord(word: string, language: string): number | null {
  const typed = word.toLocaleLowerCase(language);
  const starts = (name: string, part: string) => name.startsWith(part);
  const found = monthNamesOf(language).flatMap((name, i) => {
    const known = name.toLocaleLowerCase(language);
    // "march" starts with "mar", and "ko" is the start of "kovo".
    const match = known.length >= 3 && starts(typed, known);
    return match || starts(known, typed) ? [i + 1] : [];
  });
  return found.length === 1 ? (found[0] ?? null) : null;
}

/**
 * Reads what the user typed.
 *
 * @param text - E.g. "24.3.2027" or "24 Mar 2027". Any separator works. A
 *   text that starts with a four-digit year is always year, month, day. A
 *   two-digit year means 20xx. Eight digits are year, month, day.
 * @param style - The date style.
 * @returns "YYYY-MM-DD", or `null` if it is not a date.
 *
 * @example
 * parseDay("24.3.27", "en|dd.mm.yyyy"); // "2027-03-24"
 */
export function parseDay(text: string, style: string): string | null {
  // "20261115": quick typing, the same in every pattern.
  const quick = /^\s*(\d{4})(\d{2})(\d{2})\s*$/.exec(text);
  if (quick !== null) return makeDay(Number(quick[1]), Number(quick[2]), Number(quick[3]));
  const { language, pattern } = readDateStyle(style);
  // Chinese writes 年月日 after the numbers; they say nothing more.
  const tokens = text.replace(/[年月日]/g, " ").match(/\d+|\p{L}+/gu);
  if (tokens?.length !== 3) return null;
  let order: DatePart[] =
    tokens[0]?.length === 4 && /^\d/.test(tokens[0])
      ? ["year", "month", "day"]
      : partOrder(pattern);
  // A word is the month, wherever it stands.
  const word = tokens.findIndex((token) => !/^\d/.test(token));
  if (word >= 0) {
    const month = monthOfWord(tokens[word] ?? "", language);
    if (month === null) return null;
    tokens[word] = String(month);
    order = order.filter((part) => part !== "month");
    order.splice(word, 0, "month");
  }
  const found = { year: 0, month: 0, day: 0 };
  for (const [i, part] of order.entries()) {
    const digits = tokens[i] ?? "";
    // "5d": a second word is not a number.
    if (!/^\d+$/.test(digits)) return null;
    if (part === "year" && digits.length !== 4 && digits.length !== 2) return null;
    found[part] = Number(digits) + (part === "year" && digits.length === 2 ? 2000 : 0);
  }
  return makeDay(found.year, found.month, found.day);
}

/**
 * Moves a day.
 *
 * @param day - "YYYY-MM-DD".
 * @param days - How many days later (negative: earlier).
 * @returns The new day.
 *
 * @example
 * addDays("2027-02-28", 1); // "2027-03-01"
 */
export function addDays(day: string, days: number): string {
  return new Date(Date.parse(`${day}T00:00:00Z`) + days * DAY_MS).toISOString().slice(0, 10);
}

/**
 * Moves a day by whole months; a day that does not exist there becomes the
 * last day of the month.
 *
 * @param day - "YYYY-MM-DD".
 * @param months - How many months later (negative: earlier).
 * @returns The new day.
 *
 * @example
 * addMonths("2027-01-31", 1); // "2027-02-28"
 */
export function addMonths(day: string, months: number): string {
  const [year = 0, month = 1, dayOfMonth = 1] = day.split("-").map(Number);
  // Day 0 of a month is the last day of the month before.
  const last = new Date(Date.UTC(year, month + months, 0));
  last.setUTCDate(Math.min(dayOfMonth, last.getUTCDate()));
  return last.toISOString().slice(0, 10);
}

/**
 * The days a month view shows: six whole weeks.
 *
 * @param day - Any day of the month.
 * @param firstWeekday - 0 = Sunday … 6 = Saturday.
 * @returns 42 days, starting on that weekday.
 *
 * @example
 * monthGrid("2027-03-24", 1)[0]; // "2027-03-01", a Monday
 */
export function monthGrid(day: string, firstWeekday: number): string[] {
  const first = `${day.slice(0, 7)}-01`;
  const weekday = new Date(`${first}T00:00:00Z`).getUTCDay();
  const start = addDays(first, -((weekday - firstWeekday + 7) % 7));
  return Array.from({ length: 42 }, (_, i) => addDays(start, i));
}

/**
 * The weekday a week starts on.
 *
 * @param setting - The "First day of the week" setting.
 * @param locale - The system's locale, used for "system".
 * @returns 0 = Sunday … 6 = Saturday.
 *
 * @example
 * firstWeekdayOf("monday", "en-US"); // 1
 */
export function firstWeekdayOf(setting: string, locale: string): number {
  if (setting === "sunday") return 0;
  if (setting === "monday") return 1;
  if (setting === "saturday") return 6;
  // Newer than TypeScript's description of Intl; not every web view has it.
  const info = new Intl.Locale(locale) as Intl.Locale & {
    getWeekInfo?: () => { firstDay: number };
    weekInfo?: { firstDay: number };
  };
  // Intl counts Monday = 1 … Sunday = 7.
  return ((info.getWeekInfo?.() ?? info.weekInfo)?.firstDay ?? 1) % 7;
}
