/**
 * tests/frontend/i18n/format.test.ts: tests for date formatting
 * (`frontend/i18n/format.ts`).
 *
 * Why it matters:
 *   The setting chooses only the pattern; a Russian pattern in an English
 *   app must not bring Russian words. Schedule dates have no time zone; a
 *   task finishing on Wednesday must not be shown as Thursday.
 */

import { describe, expect, it } from "vitest";
import {
  dateFormatChoice,
  DATE_PATTERNS,
  dateStyleOf,
  formatScheduleDate,
  patternOf,
} from "@frontend/i18n/format";

describe("formatScheduleDate", () => {
  it.each([
    ["iso", "2026-03-23"],
    ["yyyy mmm dd", "2026 Mar 23"],
    ["dd.mm.yyyy", "23.03.2026"],
    ["mm/dd/yyyy", "03/23/2026"],
    ["mmm/dd/yyyy", "Mar/23/2026"],
    ["mmm dd, yyyy", "Mar 23, 2026"],
  ])("writes %s", (format, text) => {
    expect(formatScheduleDate("2026-03-23T17:00:00", dateStyleOf(format, "en"))).toBe(text);
  });

  // The user's example.
  it("takes the words from the app language", () => {
    const style = dateStyleOf("yyyy mmm dd", "lt");
    expect(formatScheduleDate("2026-03-23T08:00:00", style)).toBe("2026 kovo 23");
  });

  it("uses no foreign words for formats saved by older versions", () => {
    const style = dateStyleOf("ru-RU", "en");
    expect(formatScheduleDate("2026-11-17T08:00:00", style)).toBe("17.11.2026");
  });

  it("does not shift the day near midnight", () => {
    const style = dateStyleOf("iso", "en");
    expect(formatScheduleDate("2027-03-24T23:59:00", style)).toBe("2027-03-24");
    expect(formatScheduleDate("2027-03-24T00:00:00", style)).toBe("2027-03-24");
  });

  it("returns null for missing or invalid dates", () => {
    expect(formatScheduleDate(null, "en|dd.mm.yyyy")).toBeNull();
    expect(formatScheduleDate("", "en|dd.mm.yyyy")).toBeNull();
    expect(formatScheduleDate("tomorrow", "en|dd.mm.yyyy")).toBeNull();
  });
});

it("older settings match a choice in Settings", () => {
  expect(dateFormatChoice("ru-RU")).toBe("dd.mm.yyyy");
  expect(dateFormatChoice("lt-LT")).toBe("iso");
  expect(dateFormatChoice("en-US")).toBe("mm/dd/yyyy");
  expect(dateFormatChoice("system")).toBe("system");
  for (const pattern of DATE_PATTERNS) expect(dateFormatChoice(pattern)).toBe(pattern);
});

it("an invalid setting falls back to ISO", () => {
  expect(patternOf("not a locale!!")).toBe("yyyy-mm-dd");
});
