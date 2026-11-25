/**
 * features/grid/useUnitContext.ts: the translated unit words and link codes,
 * bundled for the duration and predecessor helpers.
 *
 * What it is for:
 *   `lib/units.ts` and `predecessors.ts` are pure functions: they do not
 *   know the UI language. This hook collects what they need from the
 *   translation file (unit words such as "d, day, days", link codes such as
 *   "FS") and from the settings (number format), and hands it over as one
 *   object.
 *
 * Where it fits:
 *   Called by the grid (`TaskGrid.tsx`) and the Task Information dialog
 *   (`features/task-info/TaskInfoDialog.tsx`).
 *
 * Main ideas:
 *   - `useMemo` rebuilds the object only when the language, the number
 *     format or the project's minutes per day change, so cells that receive
 *     it do not redraw for nothing.
 */

import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useFormatLocale } from "@frontend/features/settings/useFormatLocale";
import { formatNumber } from "@frontend/i18n/format";
import type { DependencyType } from "@frontend/ipc/types";
import {
  durationBasis,
  splitAliases,
  type DurationUnit,
  type UnitWords,
} from "@frontend/lib/units";
import type { PredecessorContext } from "./predecessors";

/**
 * Builds the {@link PredecessorContext} for the current language and project.
 *
 * @param minutesPerDay - Working minutes per day of the open project
 *   (`snapshot.minutesPerDay`).
 * @returns The context object (stable between renders while nothing changes).
 *
 * @example
 * const units = useUnitContext(snapshot.minutesPerDay);
 * parseDuration("3d", units.basis, units.unitWords);
 */
export function useUnitContext(minutesPerDay: number): PredecessorContext {
  const { t } = useTranslation();
  const { numberLocale } = useFormatLocale();
  return useMemo(() => {
    const unitWords: UnitWords = {
      minute: splitAliases(t("units.aliases.minute")),
      hour: splitAliases(t("units.aliases.hour")),
      day: splitAliases(t("units.aliases.day")),
      week: splitAliases(t("units.aliases.week")),
      month: splitAliases(t("units.aliases.month")),
    };
    const unitShort: Record<DurationUnit, string> = {
      minute: t("units.short.minute"),
      hour: t("units.short.hour"),
      day: t("units.short.day"),
      week: t("units.short.week"),
      month: t("units.short.month"),
    };
    const codes: Record<DependencyType, string> = {
      finishToStart: t("link.code.finishToStart"),
      startToStart: t("link.code.startToStart"),
      finishToFinish: t("link.code.finishToFinish"),
      startToFinish: t("link.code.startToFinish"),
    };
    return {
      basis: durationBasis(minutesPerDay),
      codes,
      unitShort,
      unitWords,
      formatNumber: (value: number) => formatNumber(value, numberLocale),
    };
    // react-i18next hands out a new `t` after a language switch, so `t` in
    // the list is enough to re-read the words in the new language.
  }, [t, numberLocale, minutesPerDay]);
}
