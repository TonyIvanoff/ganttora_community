/**
 * features/settings/useFormatLocale.ts: how to write dates and numbers, as a
 * React hook.
 *
 * What it is for:
 *   Combines the "Date format" setting (the pattern) with the UI language
 *   (the words) for `i18n/format.ts`.
 *
 * Where it fits:
 *   Used by every component that shows a date or a number. Pure logic lives
 *   in `i18n/format.ts`.
 */

import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { dateStyleOf } from "@frontend/i18n/format";
import { useSettingsStore } from "./settingsStore";

/** The values returned by {@link useFormatLocale}. */
export interface FormatLocales {
  /** The date style for `formatScheduleDate` and date fields: "en|dd.mm.yyyy". */
  dateLocale: string;
  /** For `formatNumber` and currency: the UI language. */
  numberLocale: string;
}

/**
 * Gives the date style and the number language.
 *
 * @returns A {@link FormatLocales} object; it changes only when the setting
 *   or the UI language changes.
 *
 * @example
 * const { dateLocale } = useFormatLocale();
 * formatScheduleDate(snapshot.projectFinish, dateLocale);
 */
export function useFormatLocale(): FormatLocales {
  const { i18n } = useTranslation();
  const language = i18n.language;
  const dateFormat = useSettingsStore((s) => s.settings.general.dateFormat);
  return useMemo(
    () => ({ dateLocale: dateStyleOf(dateFormat, language), numberLocale: language }),
    [dateFormat, language],
  );
}
