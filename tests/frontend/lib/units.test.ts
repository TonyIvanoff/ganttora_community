/**
 * tests/frontend/lib/units.test.ts: tests for reading typed durations.
 *
 * What it checks:
 *   `parseDays` in `frontend/lib/units.ts` turns what the user types into
 *   the duration field ("6", "2,5", "2.5") into a number of days, or `null`
 *   when the text cannot be a duration.
 *
 * Why it matters:
 *   Users in Germany, Lithuania, Russia and many other countries write a
 *   decimal comma ("2,5"). If we only understood "2.5", half our users
 *   could not enter half days. Invalid input must give `null`, so the field
 *   can show an error instead of sending nonsense to the engine.
 */

import { describe, expect, it } from "vitest";
import { parseDays } from "@frontend/lib/units";

describe("parseDays", () => {
  // Whole numbers, as typed in the shed example (task 2: 5 → 6 days).
  it("reads whole numbers", () => {
    expect(parseDays("6")).toBe(6);
    expect(parseDays("0")).toBe(0);
    expect(parseDays("365")).toBe(365);
  });

  // Decimal comma and decimal point give the same result.
  it("accepts a decimal comma and a decimal point", () => {
    expect(parseDays("2,5")).toBe(2.5);
    expect(parseDays("2.5")).toBe(2.5);
    expect(parseDays("0,25")).toBe(0.25);
  });

  // Spaces around the number (e.g. pasted text) are ignored.
  it("ignores surrounding spaces", () => {
    expect(parseDays("  3 ")).toBe(3);
    expect(parseDays("\t2,5\n")).toBe(2.5);
  });

  // Empty text, words, negative numbers and "infinity" are not durations.
  it("returns null for invalid input", () => {
    expect(parseDays("")).toBeNull();
    expect(parseDays("   ")).toBeNull();
    expect(parseDays("abc")).toBeNull();
    expect(parseDays("6 days")).toBeNull(); // units come in M1
    expect(parseDays("-1")).toBeNull();
    expect(parseDays("-0,5")).toBeNull();
    expect(parseDays("Infinity")).toBeNull();
    expect(parseDays("NaN")).toBeNull();
  });

  // Two separators ("1,2,3", "1.2,3") are ambiguous and refused.
  it("refuses more than one decimal separator", () => {
    expect(parseDays("1,2,3")).toBeNull();
    expect(parseDays("1.2,3")).toBeNull();
  });
});
