/**
 * tests/frontend/i18n/settingsFormat.test.ts: tests for the settings helpers (M1).
 *
 * What it checks:
 *   - `formatNumber`: numbers in the user's style (1,234.5 or 1.234,5).
 *   - `applyTheme` (`frontend/features/settings/useApplySettings.ts`): the
 *     Light / Dark / System setting becomes the `data-theme` attribute.
 *
 * Why it matters:
 *   A wrong theme attribute leaves the app in the
 *   wrong colours.
 *
 * Main ideas:
 *   Tests run in Node without a web page, so `applyTheme` gets a small fake
 *   element that only has a `dataset` (the object behind `data-*`
 *   attributes).
 */

import { describe, expect, it } from "vitest";
import { formatNumber } from "@frontend/i18n/format";
import { applyTheme } from "@frontend/features/settings/useApplySettings";

describe("formatNumber", () => {
  // English uses a comma for thousands and a point for decimals; German
  // the other way round; at most 2 decimals.
  it("writes numbers in the locale's style", () => {
    expect(formatNumber(1234.5, "en-US")).toBe("1,234.5");
    expect(formatNumber(1234.5, "de-DE")).toBe("1.234,5");
    expect(formatNumber(2.3456, "en-US")).toBe("2.35");
  });

  // Calling twice with the same locale gives the same text (the formatter
  // is cached).
  it("is stable across calls", () => {
    expect(formatNumber(7, "en-US")).toBe(formatNumber(7, "en-US"));
  });
});

describe("applyTheme", () => {
  /** A fake element with only a `dataset`, standing in for `<html>`. */
  function fakeRoot(): HTMLElement {
    return { dataset: {} } as unknown as HTMLElement;
  }

  // Light and dark set the attribute; system removes it so the CSS follows
  // the operating system.
  it("sets and clears data-theme", () => {
    const root = fakeRoot();
    applyTheme("dark", root);
    expect(root.dataset.theme).toBe("dark");
    applyTheme("light", root);
    expect(root.dataset.theme).toBe("light");
    applyTheme("system", root);
    expect(root.dataset.theme).toBeUndefined();
  });
});
