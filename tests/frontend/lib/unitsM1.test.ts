/**
 * tests/frontend/lib/unitsM1.test.ts: tests for durations and lags with
 * units (M1).
 *
 * What it checks:
 *   The M1 part of `frontend/lib/units.ts` (the M0 `parseDays` is tested in
 *   `units.test.ts`):
 *   - `durationBasis`: how many minutes a day, week and month have;
 *   - `parseDuration`: "3d", "2w", "4h", "30m", "1mo", "1,5d", "5" …;
 *   - `parseLag`: "+2d", "-1d", "+50%", "" (no lag) …;
 *   - `lagParts`: the largest unit that writes a lag exactly;
 *   - `minutesToDays`: minutes as days, rounded to 2 decimals.
 *
 * Why it matters:
 *   Every duration and lag the user types goes through these functions.
 *   "1,5d" (decimal comma) must mean one and a half days for German users;
 *   "-0%" must not show up as "-0"; a negative duration must be refused.
 */

import { describe, expect, it } from "vitest";
import {
  durationBasis,
  lagParts,
  minutesToDays,
  parseDuration,
  parseLag,
  type UnitWords,
} from "@frontend/lib/units";

const BASIS = durationBasis(480);

describe("durationBasis", () => {
  // MS Project's defaults: 5 days a week, 20 days a month.
  it("derives week and month from the day length", () => {
    expect(durationBasis(480)).toEqual({
      minutesPerDay: 480,
      minutesPerWeek: 2400,
      minutesPerMonth: 9600,
    });
    expect(durationBasis(420).minutesPerWeek).toBe(2100);
  });

  // A broken day length (0, negative, NaN) falls back to 8 hours.
  it("falls back to 480 minutes for a bad day length", () => {
    expect(durationBasis(0).minutesPerDay).toBe(480);
    expect(durationBasis(-5).minutesPerDay).toBe(480);
    expect(durationBasis(Number.NaN).minutesPerDay).toBe(480);
  });
});

describe("parseDuration", () => {
  // Every unit, short and long, any letter case.
  it("reads every unit", () => {
    expect(parseDuration("30m", BASIS)).toBe(30);
    expect(parseDuration("4h", BASIS)).toBe(240);
    expect(parseDuration("4 hours", BASIS)).toBe(240);
    expect(parseDuration("3d", BASIS)).toBe(1440);
    expect(parseDuration("3D", BASIS)).toBe(1440);
    expect(parseDuration("2w", BASIS)).toBe(4800);
    expect(parseDuration("1mo", BASIS)).toBe(9600);
  });

  // Decimal comma or point; no unit means the default unit (days).
  it("reads decimals and a missing unit", () => {
    expect(parseDuration("1,5d", BASIS)).toBe(720);
    expect(parseDuration("1.5d", BASIS)).toBe(720);
    expect(parseDuration("5", BASIS)).toBe(2400);
    expect(parseDuration("5", BASIS, undefined, "hour")).toBe(300);
    expect(parseDuration(" 2d? ", BASIS)).toBe(960); // "?" = estimated
  });

  // Text that is not a duration, and negative durations, give null.
  it("refuses what is not a duration", () => {
    for (const text of ["", "3x", "-1d", "1e3", "1,2,3", "d", "abc", "2 d d"]) {
      expect(parseDuration(text, BASIS), text).toBeNull();
    }
  });

  // A translated UI adds its own unit words; English keeps working.
  it("accepts translated unit words", () => {
    const german: UnitWords = {
      minute: ["min"],
      hour: ["std"],
      day: ["t", "tag", "tage"],
      week: ["wo"],
      month: ["mon"],
    };
    expect(parseDuration("3 Tage", BASIS, german)).toBe(1440);
    expect(parseDuration("2std", BASIS, german)).toBe(120);
    expect(parseDuration("3d", BASIS, german)).toBe(1440);
  });
});

describe("parseLag", () => {
  // Empty means no lag at all.
  it("reads an empty lag as zero minutes", () => {
    expect(parseLag("", BASIS)).toEqual({ kind: "time", minutes: 0 });
    expect(parseLag("  ", BASIS)).toEqual({ kind: "time", minutes: 0 });
  });

  // Signed time lags; a missing sign means "+".
  it("reads signed time lags", () => {
    expect(parseLag("+2d", BASIS)).toEqual({ kind: "time", minutes: 960 });
    expect(parseLag("-1d", BASIS)).toEqual({ kind: "time", minutes: -480 });
    expect(parseLag("2d", BASIS)).toEqual({ kind: "time", minutes: 960 });
    expect(parseLag("- 4h", BASIS)).toEqual({ kind: "time", minutes: -240 });
  });

  // Percentages, rounded to whole numbers.
  it("reads percentages", () => {
    expect(parseLag("+50%", BASIS)).toEqual({ kind: "percent", percent: 50 });
    expect(parseLag("-20 %", BASIS)).toEqual({ kind: "percent", percent: -20 });
    expect(parseLag("12,6%", BASIS)).toEqual({ kind: "percent", percent: 13 });
  });

  // "-0" must become a plain 0 (else it would print as "-0").
  it("never gives negative zero", () => {
    const percent = parseLag("-0%", BASIS);
    expect(percent?.kind === "percent" && Object.is(percent.percent, 0)).toBe(true);
    const time = parseLag("-0d", BASIS);
    expect(time?.kind === "time" && Object.is(time.minutes, 0)).toBe(true);
  });

  // Unreadable lags give null.
  it("refuses text that is not a lag", () => {
    for (const text of ["+", "+%", "+x", "++1d", "+1x", "abc%"]) {
      expect(parseLag(text, BASIS), text).toBeNull();
    }
  });
});

describe("lagParts", () => {
  // The largest unit that divides the minutes exactly.
  it("chooses days, then hours, then minutes", () => {
    expect(lagParts(960, BASIS)).toEqual({ value: 2, unit: "day" });
    expect(lagParts(720, BASIS)).toEqual({ value: 12, unit: "hour" });
    expect(lagParts(90, BASIS)).toEqual({ value: 90, unit: "minute" });
    expect(lagParts(0, BASIS)).toEqual({ value: 0, unit: "day" });
  });
});

describe("minutesToDays", () => {
  // Days with at most two decimals.
  it("converts minutes to days with two decimals", () => {
    expect(minutesToDays(720, BASIS)).toBe(1.5);
    expect(minutesToDays(2400, BASIS)).toBe(5);
    expect(minutesToDays(100, BASIS)).toBe(0.21);
    expect(minutesToDays(0, BASIS)).toBe(0);
  });
});
