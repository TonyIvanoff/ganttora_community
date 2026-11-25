/**
 * lib/units.ts: reads and writes durations and lags typed by the user.
 *
 * What it is for:
 *   The user types durations as text ("6", "2.5", "3d", "2w", "4h") and
 *   lags as text with a sign ("+1d", "-20%"). The core works in working
 *   *minutes* (and whole percents for percentage lags). This file turns the
 *   text into those numbers, and numbers back into short text for the grid.
 *
 * Where it fits:
 *   Used by the task grid (`features/grid/`: Duration and Predecessors
 *   columns) and the Task Information dialog (`features/task-info/`).
 *   M0's `parseDays` is kept: it is the simplest case (a plain number of
 *   days).
 *
 * Main ideas:
 *   - **Units are a change of scale, not scheduling.** "2w" means
 *     2 × (minutes in a working week). How many minutes a day or week has
 *     comes from the project (`snapshot.minutesPerDay`), see
 *     {@link DurationBasis}. Which *dates* that covers is decided by the
 *     engine in Rust (decision D16).
 *   - **Unit words are translatable.** English letters (m, h, d, w, mo) are
 *     always understood; the words of the UI language are added from
 *     `locales/<lang>.json` (`units.aliases.*`), so a German user can type
 *     "3t" (Tage) once that translation exists.
 *   - Many languages write a decimal *comma* ("2,5"), English a decimal
 *     *point* ("2.5"). Both are accepted.
 *   - **No elapsed durations ("3ed").** The model has no elapsed time yet, so
 *     they are not accepted (they would silently mean something else).
 *   - Pure functions: text in, number (or `null`) out. Easy to test.
 */

/** The units a duration may be typed in. */
export type DurationUnit = "minute" | "hour" | "day" | "week" | "month";

/** All units, from smallest to largest. */
export const DURATION_UNITS: readonly DurationUnit[] = ["minute", "hour", "day", "week", "month"];

/**
 * How many working minutes one unit has in this project.
 *
 * Why not fixed numbers? A "day" is 8 hours on a standard calendar but may
 * be 7.5 or 10 hours in another project. The values come from the project
 * (`snapshot.minutesPerDay`), see {@link durationBasis}.
 */
export interface DurationBasis {
  /** Working minutes in one day, e.g. 480 (8 h). */
  minutesPerDay: number;
  /** Working minutes in one week, e.g. 2400 (5 days × 8 h). */
  minutesPerWeek: number;
  /** Working minutes in one month, e.g. 9600 (20 days × 8 h). */
  minutesPerMonth: number;
}

/**
 * Words the user may type after a number, per unit, all in lower case.
 *
 * @example
 * const words: UnitWords = { minute: ["m"], hour: ["h"], day: ["d", "day", "days"], week: ["w"], month: ["mo"] };
 */
export type UnitWords = Record<DurationUnit, readonly string[]>;

/**
 * English unit words, always understood (also in other UI languages, as in
 * MS Project, because users meet them in help pages and imported files).
 */
export const ENGLISH_UNIT_WORDS: UnitWords = {
  minute: ["m", "min", "mins", "minute", "minutes"],
  hour: ["h", "hr", "hrs", "hour", "hours"],
  day: ["d", "dy", "day", "days"],
  week: ["w", "wk", "wks", "week", "weeks"],
  month: ["mo", "mon", "mons", "month", "months"],
};

/** Days in a working week and a working month, as MS Project's defaults. */
const DAYS_PER_WEEK = 5;
const DAYS_PER_MONTH = 20;

/**
 * Builds the {@link DurationBasis} of a project.
 *
 * @param minutesPerDay - Working minutes per day (`snapshot.minutesPerDay`).
 *   Values of 0 or below are replaced by 480 (8 hours), so a broken value
 *   can never make a division by zero.
 * @returns The basis; a week is 5 days and a month 20 days (MS Project's
 *   defaults).
 *
 * @example
 * durationBasis(480); // { minutesPerDay: 480, minutesPerWeek: 2400, minutesPerMonth: 9600 }
 */
export function durationBasis(minutesPerDay: number): DurationBasis {
  const day = minutesPerDay > 0 && Number.isFinite(minutesPerDay) ? minutesPerDay : 480;
  return {
    minutesPerDay: day,
    minutesPerWeek: day * DAYS_PER_WEEK,
    minutesPerMonth: day * DAYS_PER_MONTH,
  };
}

/**
 * Minutes in one unit.
 *
 * @param unit - The unit.
 * @param basis - The project's basis.
 * @returns Working minutes in one `unit`.
 */
function minutesPerUnit(unit: DurationUnit, basis: DurationBasis): number {
  switch (unit) {
    case "minute":
      return 1;
    case "hour":
      return 60;
    case "day":
      return basis.minutesPerDay;
    case "week":
      return basis.minutesPerWeek;
    case "month":
      return basis.minutesPerMonth;
  }
}

/**
 * Splits a comma-separated list from the translation file into unit words.
 *
 * @param text - E.g. "d, day, days" (from `units.aliases.day`).
 * @returns The words in lower case without spaces; empty entries are dropped.
 *
 * @example
 * splitAliases("d, Day,days"); // ["d", "day", "days"]
 */
export function splitAliases(text: string): string[] {
  return text
    .split(",")
    .map((word) => word.trim().toLowerCase())
    .filter((word) => word !== "");
}

/**
 * Finds the unit a typed word stands for.
 *
 * @param word - What followed the number, already lower case, e.g. "d".
 * @param words - The UI language's words; English is always checked too.
 * @returns The unit, or `null` if the word is unknown.
 */
function unitOf(word: string, words: UnitWords | undefined): DurationUnit | null {
  for (const unit of DURATION_UNITS) {
    if (words?.[unit].includes(word) === true || ENGLISH_UNIT_WORDS[unit].includes(word)) {
      return unit;
    }
  }
  return null;
}

/**
 * Reads a number written with a decimal point or comma.
 *
 * @param text - E.g. "2.5" or "2,5".
 * @returns The number, or `NaN` if the text is not a plain number.
 */
function readNumber(text: string): number {
  // Only digits with at most one decimal mark: "1e3" or "0x10" are refused.
  if (!/^\d+(?:[.,]\d+)?$|^[.,]\d+$/.test(text)) return Number.NaN;
  return Number(text.replace(",", "."));
}

/**
 * Reads a number of days typed by the user (M0; still used where only days
 * make sense).
 *
 * @param text - What the user typed, e.g. "6", "2.5" or "2,5". Spaces
 *   around the number are ignored.
 * @returns The number of days, or `null` if the text is empty, not a
 *   number, or below 0.
 *
 * @example
 * parseDays("6");   // 6
 * parseDays("2,5"); // 2.5
 * parseDays("-1");  // null
 * parseDays("abc"); // null
 */
export function parseDays(text: string): number | null {
  const trimmed = text.trim().replace(",", ".");
  if (trimmed === "") return null;
  // `Number("1e3")` would be 1000; that is fine, just unusual.
  const days = Number(trimmed);
  return Number.isFinite(days) && days >= 0 ? days : null;
}

/**
 * Reads a duration such as "3", "3d", "2.5 days", "2w", "4h" or "30m".
 *
 * @param text - What the user typed. A number without a unit is read in
 *   `defaultUnit`. Spaces are allowed between number and unit. A trailing
 *   "?" (MS Project's "estimated" mark, e.g. "1 day?") is ignored.
 * @param basis - The project's {@link DurationBasis}.
 * @param words - Unit words of the UI language (English is always known).
 * @param defaultUnit - The unit of a bare number (default `"day"`).
 * @returns Whole working minutes (rounded), or `null` if the text is empty,
 *   negative, has an unknown unit, or is not a number.
 *
 * @example
 * const basis = durationBasis(480);
 * parseDuration("3d", basis);       // 1440
 * parseDuration("2,5", basis);      // 1200 (2.5 days)
 * parseDuration("1w", basis);       // 2400
 * parseDuration("90 min", basis);   // 90
 * parseDuration("3ed", basis);      // null (elapsed days are not supported)
 */
export function parseDuration(
  text: string,
  basis: DurationBasis,
  words?: UnitWords,
  defaultUnit: DurationUnit = "day",
): number | null {
  const amount = parseAmount(text.trim().replace(/\?$/, ""), basis, words, defaultUnit);
  return amount === null || amount < 0 ? null : amount;
}

/**
 * Reads a number with an optional unit word (the shared part of
 * {@link parseDuration} and {@link parseLag}).
 *
 * @param text - Trimmed text, e.g. "3d" or "2.5 days" (no sign).
 * @param basis - The project's basis.
 * @param words - Unit words of the UI language.
 * @param defaultUnit - The unit of a bare number.
 * @returns Whole minutes, or `null` if unreadable.
 */
function parseAmount(
  text: string,
  basis: DurationBasis,
  words: UnitWords | undefined,
  defaultUnit: DurationUnit,
): number | null {
  // A regular expression ("regex") describes a text pattern:
  //   ([\d.,]+)  one or more digits / decimal marks: the number
  //   \s*        optional spaces
  //   (\p{L}*)   optional letters (any alphabet, flag "u"): the unit word
  const match = /^([\d.,]+)\s*(\p{L}*)$/u.exec(text);
  if (match === null) return null;
  const value = readNumber(match[1] ?? "");
  if (!Number.isFinite(value)) return null;
  const word = (match[2] ?? "").toLowerCase();
  const unit = word === "" ? defaultUnit : unitOf(word, words);
  if (unit === null) return null;
  return Math.round(value * minutesPerUnit(unit, basis));
}

/** A lag read from text: a time in minutes or a percentage. */
export type ParsedLag =
  /** A time lag (negative = lead). */
  | { kind: "time"; minutes: number }
  /** A percentage of the predecessor's duration (negative = lead). */
  | { kind: "percent"; percent: number };

/**
 * Reads a lag such as "1d", "+2d", "-3h", "50%" or "-20%".
 *
 * @param text - What the user typed. Empty text means "no lag". A number
 *   without unit is read in days.
 * @param basis - The project's {@link DurationBasis}.
 * @param words - Unit words of the UI language.
 * @returns The lag, or `null` if the text cannot be read. Percentages are
 *   rounded to whole numbers, because the model stores whole percents
 *   (`Dependency.lagPercent` is an integer in Rust).
 *
 * @example
 * const basis = durationBasis(480);
 * parseLag("", basis);     // { kind: "time", minutes: 0 }
 * parseLag("+1d", basis);  // { kind: "time", minutes: 480 }
 * parseLag("-20%", basis); // { kind: "percent", percent: -20 }
 */
export function parseLag(text: string, basis: DurationBasis, words?: UnitWords): ParsedLag | null {
  let rest = text.trim();
  if (rest === "") return { kind: "time", minutes: 0 };
  let sign = 1;
  if (rest.startsWith("+") || rest.startsWith("-")) {
    sign = rest.startsWith("-") ? -1 : 1;
    rest = rest.slice(1).trim();
  }
  if (rest.endsWith("%")) {
    const value = readNumber(rest.slice(0, -1).trim());
    if (!Number.isFinite(value)) return null;
    // `|| 0` turns -0 into 0, so "-0%" does not print as "-0".
    return { kind: "percent", percent: sign * Math.round(value) || 0 };
  }
  const minutes = parseAmount(rest, basis, words, "day");
  return minutes === null ? null : { kind: "time", minutes: sign * minutes || 0 };
}

/** A duration split into a number and a unit, ready to be shown. */
export interface DurationParts {
  /** The amount in `unit`, rounded to two decimals. */
  value: number;
  /** The unit. */
  unit: DurationUnit;
}

/**
 * Picks the unit to show a time lag in: the largest of day, hour, minute
 * that divides it exactly (so "+1d", "+4h", "+90m" stay short and exact).
 *
 * @param minutes - Working minutes (may be negative).
 * @param basis - The project's basis.
 * @returns The amount and unit.
 *
 * @example
 * lagParts(480, durationBasis(480)); // { value: 1, unit: "day" }
 * lagParts(240, durationBasis(480)); // { value: 4, unit: "hour" }
 */
export function lagParts(minutes: number, basis: DurationBasis): DurationParts {
  if (minutes % basis.minutesPerDay === 0) {
    return { value: minutes / basis.minutesPerDay, unit: "day" };
  }
  if (minutes % 60 === 0) return { value: minutes / 60, unit: "hour" };
  return { value: minutes, unit: "minute" };
}

/**
 * Converts working minutes into days for display (the grid's Duration
 * column shows days, like MS Project's default).
 *
 * @param minutes - Working minutes.
 * @param basis - The project's basis.
 * @returns Days, rounded to two decimals.
 *
 * @example
 * minutesToDays(1200, durationBasis(480)); // 2.5
 */
export function minutesToDays(minutes: number, basis: DurationBasis): number {
  return Math.round((minutes / basis.minutesPerDay) * 100) / 100;
}
