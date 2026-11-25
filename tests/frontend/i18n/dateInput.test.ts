/**
 * tests/frontend/i18n/dateInput.test.ts: tests for the short dates of date
 * fields (`frontend/i18n/dateInput.ts`).
 *
 * Why it matters:
 *   03/04/2027 is 4 March for one user and 3 April for another. Reading it
 *   in the wrong order would silently move a task by a month.
 */

import { describe, expect, it } from "vitest";
import {
  addMonths,
  firstWeekdayOf,
  formatDay,
  INVALID_DAY,
  isDay,
  monthGrid,
  parseDay,
} from "@frontend/i18n/dateInput";

describe("formatDay and parseDay", () => {
  it.each([
    ["en|yyyy-mm-dd", "2027-03-04"],
    ["en|mm/dd/yyyy", "03/04/2027"],
    ["en|dd/mm/yyyy", "04/03/2027"],
    ["en|dd mmm yyyy", "04 Mar 2027"],
    ["en|mmm/dd/yyyy", "Mar/04/2027"],
    ["en|mmm dd, yyyy", "Mar 04, 2027"],
    ["lt|yyyy mmm dd", "2027 kovo 04"],
  ])("%s writes %s and reads it back", (style, text) => {
    expect(formatDay("2027-03-04", style)).toBe(text);
    expect(parseDay(text, style)).toBe("2027-03-04");
  });

  // Every month, in every planned language.
  it.each(["en", "de", "fr", "es", "pt", "lt", "pl", "ru", "zh-CN"])(
    "%s reads back",
    (language) => {
      for (const pattern of ["yyyy mmm dd", "dd mmm yyyy", "dd.mm.yyyy"])
        for (let month = 1; month <= 12; month += 1) {
          const day = `2027-${String(month).padStart(2, "0")}-04`;
          const style = `${language}|${pattern}`;
          expect(parseDay(formatDay(day, style), style)).toBe(day);
        }
    },
  );

  it.each(["en|dd.mm.yyyy", "en|mmm/dd/yyyy", "lt|yyyy mmm dd"])("%s reads 20261115", (style) => {
    expect(parseDay("20261115", style)).toBe("2026-11-15");
    expect(parseDay("20261315", style)).toBeNull();
  });

  it("reads short, year-first and full-name dates in any format", () => {
    expect(parseDay("4.3.27", "en|dd.mm.yyyy")).toBe("2027-03-04");
    expect(parseDay("2027-03-04", "en|mm/dd/yyyy")).toBe("2027-03-04");
    expect(parseDay("4 march 2027", "en|mm/dd/yyyy")).toBe("2027-03-04");
  });

  it.each(["", "2027-03", "2027-02-31", "tomorrow", "5d 3 2027", "24/03/2027", "4 ma 2027"])(
    "refuses %j",
    (text) => expect(parseDay(text, "en|mm/dd/yyyy")).toBeNull(),
  );
});

it("tells real days from others", () => {
  expect(isDay("2028-02-29")).toBe(true);
  expect(isDay("2027-02-29")).toBe(false);
  expect(isDay(INVALID_DAY)).toBe(false);
});

it("moves by months and lays out a month", () => {
  expect(addMonths("2027-01-31", 1)).toBe("2027-02-28");
  expect(addMonths("2027-01-31", -1)).toBe("2026-12-31");
  expect(monthGrid("2027-03-24", 1)[0]).toBe("2027-03-01");
  expect(monthGrid("2027-03-24", 0)[0]).toBe("2027-02-28");
  expect(firstWeekdayOf("saturday", "en-US")).toBe(6);
});
