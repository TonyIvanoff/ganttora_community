/**
 * features/files/noteText.ts: turns a "read note" from Rust into a
 * translated sentence.
 *
 * What it is for:
 *   After opening a `.gtra` file, Rust may report things that did not fit
 *   the program exactly (`ReadNoteDto` in `ipc/types.ts`). Rust sends only
 *   data (the kind of note and its details); all user-visible text lives in
 *   `locales/en.json` under `file.note.<kind>`. This function joins the two.
 *
 * Where it fits:
 *   Used by `features/files/FileBanners.tsx` (the notes banner). Kept in its
 *   own file because it is a plain function, not a component: plain
 *   functions are easy to test (in `tests/frontend/`), and React's fast
 *   refresh works best when component files export only components.
 */

import type { TFunction } from "i18next";
import { formatScheduleDate } from "@frontend/i18n/format";
import type { ReadNoteDto, ScheduleSnapshot } from "@frontend/ipc/types";

/**
 * Turns one read note into a translated sentence.
 *
 * @param note - The note from Rust.
 * @param t - The translation function (from `useTranslation()`).
 * @param snapshot - The current schedule, used to show a task's *name*
 *   instead of its ID; `null` if there is none.
 * @param locale - How to write dates: the `dateLocale` from
 *   `useFormatLocale()` (the "Date format" setting), i.e. a locale code
 *   such as "en-GB", or "iso" for 2027-03-24. Not the UI language: dates
 *   follow the Date format setting everywhere in the app.
 * @returns The sentence to show.
 *
 * @example
 * noteText({ kind: "rowsNotLoaded", table: "resources", count: 3 }, t, null, "en");
 * // "3 rows in “resources” cannot be shown yet; they stay in the file."
 *
 * Beginner note: `switch (note.kind)` on a discriminated union lets
 * TypeScript know the exact shape in each `case`, so `note.taskId` is only
 * allowed in the `"durationMissing"` case. Because every kind has a
 * `case` that returns, TypeScript also checks that the function always
 * returns a string: a new kind added to `types.ts` without a `case` here
 * becomes a compile error.
 */
export function noteText(
  note: ReadNoteDto,
  t: TFunction,
  snapshot: ScheduleSnapshot | null,
  locale: string,
): string {
  switch (note.kind) {
    case "startDateMissing":
      return t("file.note.startDateMissing", {
        date: formatScheduleDate(note.used, locale) ?? note.used,
      });
    case "calendarMissing":
      return t("file.note.calendarMissing");
    case "durationMissing": {
      // Show the task's name if we can find it; its ID otherwise.
      const task = snapshot?.tasks.find((row) => row.id === note.taskId);
      return t("file.note.durationMissing", { task: task?.name ?? note.taskId });
    }
    case "baseCalendarFlattened":
      return t("file.note.baseCalendarFlattened");
    case "valueNotLoaded":
      return t("file.note.valueNotLoaded", { table: note.table, column: note.column });
    case "rowsNotLoaded":
      return t("file.note.rowsNotLoaded", { table: note.table, count: note.count });
  }
}
