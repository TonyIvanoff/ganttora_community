/**
 * i18n/format.ts: writes dates and numbers.
 *
 * What it is for:
 *   A date is written with the *pattern* the user chose ("dd.mm.yyyy") and
 *   the *words* of the UI language ("2026 Mar 23", in Lithuanian
 *   "2026 kovo 23"). Numbers follow the UI language.
 *
 * Where it fits:
 *   Components get a "date style" from the hook
 *   `features/settings/useFormatLocale.ts` and pass it to these helpers.
 *   `dateInput.ts` uses the same patterns for date fields.
 *
 * Main ideas:
 *   - A **date style** is one text with both parts: "en|dd.mm.yyyy". One
 *     text is easy to pass on and to compare.
 *   - A pattern has no language. A country code saved by older versions
 *     ("ru-RU") still works: only its pattern is used.
 *   - Schedule dates have no time zone (see `features/gantt/renderer/dates.ts`).
 *     We read them as UTC and also *write* them in UTC, so the date shown is
 *     exactly the date the engine calculated, wherever the user lives.
 *   - `Intl` (built into the web view) knows the month names of every
 *     language. Its formatters are slow to create, so they are kept.
 */

import { parseIsoLocal } from "@frontend/features/gantt/renderer/dates";

/**
 * `dateFormat` setting value: the pattern of the operating system's region.
 * Must match `DATE_FORMAT_SYSTEM` in `backend/src-tauri/src/settings.rs`.
 */
export const DATE_FORMAT_SYSTEM = "system";

/**
 * `dateFormat` setting value: ISO 8601, the pattern "yyyy-mm-dd".
 * Must match `DATE_FORMAT_ISO` in `backend/src-tauri/src/settings.rs`.
 */
export const DATE_FORMAT_ISO = "iso";

/** The pattern of {@link DATE_FORMAT_ISO}. */
const ISO_PATTERN = "yyyy-mm-dd";

/**
 * The patterns, besides ISO, that can be chosen. "mmm" is the month's name.
 * Must match `DATE_PATTERNS` in `backend/src-tauri/src/settings.rs`.
 */
export const DATE_PATTERNS = [
  "yyyy mmm dd",
  "yyyy/mm/dd",
  "yyyy.mm.dd",
  "dd.mm.yyyy",
  "dd/mm/yyyy",
  "dd-mm-yyyy",
  "dd mmm yyyy",
  "mm/dd/yyyy",
  "mmm/dd/yyyy",
  "mmm dd, yyyy",
] as const;

/** Languages whose short month names are numbers or unclear. */
const LONG_MONTHS: readonly string[] = ["lt"];

/** Cache: language → the 12 month names. */
const monthNames = new Map<string, string[]>();

/**
 * The month names of a language, as used inside a date.
 *
 * @param language - UI language, e.g. "lt".
 * @returns 12 names, January first.
 *
 * @example
 * monthNamesOf("en")[2]; // "Mar"
 * monthNamesOf("lt")[2]; // "kovo"
 */
export function monthNamesOf(language: string): string[] {
  let names = monthNames.get(language);
  if (names === undefined) {
    // With a day, Intl uses the form for dates: "kovo 23", not "kovas".
    const formatter = new Intl.DateTimeFormat(language, {
      month: LONG_MONTHS.includes(language) ? "long" : "short",
      day: "numeric",
      timeZone: "UTC",
    });
    names = Array.from({ length: 12 }, (_, month) => {
      const parts = formatter.formatToParts(Date.UTC(2027, month, 1));
      const name = parts.find((part) => part.type === "month")?.value ?? String(month + 1);
      // "нояб." → "нояб": the dot looks wrong in the middle of a date.
      return name.replace(/\.$/, "");
    });
    monthNames.set(language, names);
  }
  return names;
}

/** Cache: region → its pattern. */
const regionPatterns = new Map<string, string>();

/**
 * The all-numbers pattern of a region.
 *
 * @param region - A locale code such as "ru-RU".
 * @returns The pattern, "yyyy-mm-dd" if it is unknown.
 */
function patternOfRegion(region: string): string {
  let pattern = regionPatterns.get(region);
  if (pattern === undefined) {
    try {
      const written = new Intl.DateTimeFormat(region, {
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
        timeZone: "UTC",
        numberingSystem: "latn",
        calendar: "gregory",
      })
        .format(Date.UTC(2027, 10, 23))
        .replace("2027", "yyyy")
        .replace("11", "mm")
        .replace("23", "dd");
      pattern = (DATE_PATTERNS as readonly string[]).includes(written) ? written : ISO_PATTERN;
    } catch {
      // Not a locale code (a settings file edited by hand).
      pattern = ISO_PATTERN;
    }
    regionPatterns.set(region, pattern);
  }
  return pattern;
}

/**
 * The pattern that a `dateFormat` setting stands for.
 *
 * @param dateFormat - "system", "iso", a pattern, or a locale code saved by
 *   an older version.
 * @returns The pattern.
 *
 * @example
 * patternOf("iso");   // "yyyy-mm-dd"
 * patternOf("ru-RU"); // "dd.mm.yyyy"
 */
export function patternOf(dateFormat: string): string {
  if (dateFormat === DATE_FORMAT_ISO) return ISO_PATTERN;
  if ((DATE_PATTERNS as readonly string[]).includes(dateFormat)) return dateFormat;
  const system = dateFormat === DATE_FORMAT_SYSTEM || dateFormat === "";
  return patternOfRegion(system ? systemLocale() : dateFormat);
}

/**
 * The choice in Settings that matches a `dateFormat` setting.
 *
 * @param dateFormat - The saved setting.
 * @returns "system", "iso" or a pattern.
 *
 * @example
 * dateFormatChoice("ru-RU"); // "dd.mm.yyyy"
 */
export function dateFormatChoice(dateFormat: string): string {
  if (dateFormat === DATE_FORMAT_SYSTEM || dateFormat === "") return DATE_FORMAT_SYSTEM;
  const pattern = patternOf(dateFormat);
  return pattern === ISO_PATTERN ? DATE_FORMAT_ISO : pattern;
}

/**
 * Builds the date style for the helpers below.
 *
 * @param dateFormat - The setting.
 * @param language - UI language.
 * @returns E.g. "en|dd.mm.yyyy".
 */
export function dateStyleOf(dateFormat: string, language: string): string {
  return `${language}|${patternOf(dateFormat)}`;
}

/**
 * Takes a date style apart.
 *
 * @param style - From {@link dateStyleOf}.
 * @returns Its language and pattern.
 */
export function readDateStyle(style: string): { language: string; pattern: string } {
  const [language = "en", pattern = ISO_PATTERN] = style.split("|");
  return { language, pattern };
}

/**
 * Writes a day with a date style.
 *
 * @param day - "YYYY-MM-DD".
 * @param style - From {@link dateStyleOf}.
 * @returns The text.
 *
 * @example
 * writeDay("2026-03-23", "lt|yyyy mmm dd"); // "2026 kovo 23"
 */
export function writeDay(day: string, style: string): string {
  const { language, pattern } = readDateStyle(style);
  const [year = "", month = "", date = ""] = day.split("-");
  // "mmm" stands before "mm" in the search, so it is found first.
  return pattern.replace(/yyyy|mmm|mm|dd/g, (part) => {
    if (part === "yyyy") return year;
    if (part === "dd") return date;
    return part === "mm" ? month : (monthNamesOf(language)[Number(month) - 1] ?? month);
  });
}

/**
 * Writes a schedule date (no time zone).
 *
 * @param iso - ISO date-time text, e.g. "2027-03-24T17:00:00", or `null`.
 * @param style - From {@link dateStyleOf}.
 * @returns The date, or `null` if `iso` is `null` or not a valid date.
 *
 * @example
 * formatScheduleDate("2027-03-24T17:00:00", "en|dd mmm yyyy"); // "24 Mar 2027"
 */
export function formatScheduleDate(iso: string | null, style: string): string | null {
  if (iso === null) return null;
  const ms = parseIsoLocal(iso);
  if (Number.isNaN(ms)) return null;
  return writeDay(new Date(ms).toISOString().slice(0, 10), style);
}

/**
 * The operating system's regional format, as the web view reports it.
 *
 * @returns A locale code such as "en-US", "en-GB" or "de-DE".
 *
 * @example
 * systemLocale(); // "en-GB" on a Mac set to English (UK)
 *
 * Beginner note: `new Intl.DateTimeFormat()` with no language uses the
 * system's default; `resolvedOptions().locale` tells us which one that is.
 */
export function systemLocale(): string {
  return new Intl.DateTimeFormat().resolvedOptions().locale;
}

/** Cache: locale → number formatter. */
const numberFormatters = new Map<string, Intl.NumberFormat>();

/**
 * Formats a plain number in the given locale ("1,234.5" or "1.234,5").
 *
 * @param value - The number.
 * @param locale - Locale code, the UI language.
 * @returns The formatted number, with at most 2 decimals.
 *
 * @example
 * formatNumber(1234.5, "en-US"); // "1,234.5"
 * formatNumber(1234.5, "de-DE"); // "1.234,5"
 */
export function formatNumber(value: number, locale: string): string {
  let formatter = numberFormatters.get(locale);
  if (formatter === undefined) {
    formatter = new Intl.NumberFormat(locale, { maximumFractionDigits: 2 });
    numberFormatters.set(locale, formatter);
  }
  return formatter.format(value);
}
