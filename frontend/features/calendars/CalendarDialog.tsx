/**
 * features/calendars/CalendarDialog.tsx: the calendar editor ("Change Working Time").
 *
 * What it is for:
 *   A calendar says when work can happen: the working hours of each
 *   weekday, plus exceptions such as company holidays. This dialog lets the
 *   user:
 *   - see all calendars of the project, create a new one, copy, rename or
 *     delete one (a calendar still used by the project or a task cannot be
 *     deleted: Rust answers `calendarInUse` and the dialog shows why);
 *   - set the **work week**: working times per weekday (e.g. 08:00–12:00
 *     and 13:00–17:00), or a day off;
 *   - add **exceptions**: a name, a date range, and either "not working"
 *     or their own working times; and single **holidays**;
 *   - make a calendar the **project calendar**.
 *
 * Where it fits:
 *   Opened from the toolbar (`shell/AppShell.tsx`, `calendarStore.ts`).
 *   Reads the calendars from the main store (`state/store.ts`); every change
 *   is a command sent to Rust (`addCalendar`, `updateCalendar`,
 *   `removeCalendar`, `setProjectCalendar`; decision D16), so each one can
 *   be undone.
 *
 * Main ideas:
 *   - **A draft.** Week and exception edits change a local copy (`draft`)
 *     first. "Apply" sends the whole calendar in one `updateCalendar`
 *     command: one undo step for one round of editing. Choosing another
 *     calendar throws away unapplied edits (the button says so by being
 *     enabled).
 *   - **Checks before sending.** Times must be in order and not overlap
 *     (`blocksProblem` in `workingTime.ts`, the same rules as the engine);
 *     the problem is shown next to the field and Apply is disabled.
 *   - The native `<dialog>` element (`showModal()`): it traps the focus,
 *     closes with Esc and darkens the window behind it, with no extra code.
 */

import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { DateField } from "@frontend/components/ui/DateField";
import { isDay } from "@frontend/i18n/dateInput";
import type { Calendar, CalendarException, Command, WorkingTime } from "@frontend/ipc/types";
import { errorText } from "@frontend/lib/errors";
import { useAppStore } from "@frontend/state/store";
import { useCalendarDialogStore } from "./calendarStore";
import {
  WEEKDAYS,
  blocksMinutes,
  blocksProblem,
  minutesToTime,
  standardDay,
  standardWeek,
  timeToMinutes,
} from "./workingTime";

/** Look of text and date fields (Tailwind classes). */
const FIELD_CLASS =
  "min-w-0 rounded border border-line bg-surface px-2 py-1 text-sm text-fg " +
  "focus:outline-2 focus:outline-focus";

/** Look of small buttons. */
const BUTTON_CLASS =
  "rounded border border-line px-2 py-1 text-sm whitespace-nowrap hover:bg-app " +
  "disabled:opacity-50 disabled:hover:bg-transparent";

/** Look of the main (accent) button. */
const PRIMARY_CLASS =
  "rounded border border-accent bg-accent px-3 py-1 text-sm whitespace-nowrap text-accent-fg " +
  "disabled:opacity-50";

/**
 * Makes a deep copy of a calendar, so the draft can be changed without
 * touching the store's copy.
 *
 * @param calendar - The calendar.
 * @returns An independent copy.
 *
 * Beginner note: `structuredClone` copies an object *and everything inside
 * it*. A plain `{ ...calendar }` would copy only the top level: the week
 * arrays would still be shared with the store.
 */
function copyCalendar(calendar: Calendar): Calendar {
  return structuredClone({ ...calendar, exceptions: calendar.exceptions ?? [] });
}

/**
 * Checks a whole calendar draft.
 *
 * @param calendar - The draft.
 * @returns `true` if every weekday and exception has valid times, every
 *   exception has a start on or before its finish, and the week has some
 *   working time.
 */
function draftIsValid(calendar: Calendar): boolean {
  const weekOk = WEEKDAYS.every((day) => blocksProblem(calendar.week[day]) === null);
  const hasWork = WEEKDAYS.some((day) => calendar.week[day].length > 0);
  const exceptionsOk = (calendar.exceptions ?? []).every(
    (e) =>
      e.name.trim() !== "" &&
      isDay(e.start) &&
      isDay(e.finish) &&
      e.start <= e.finish &&
      blocksProblem(e.workingTimes) === null,
  );
  return weekOk && hasWork && exceptionsOk && calendar.name.trim() !== "";
}

/** Props of {@link BlocksEditor}. */
interface BlocksEditorProps {
  /** Accessible name of the group, e.g. "Monday". */
  label: string;
  /** The working blocks. */
  blocks: readonly WorkingTime[];
  /** Called with the new blocks after every change. */
  onChange: (blocks: WorkingTime[]) => void;
}

/**
 * Edits a list of working blocks: one "from – to" pair of time fields per
 * block, a button to add a block and one to remove each.
 *
 * @param props - See {@link BlocksEditorProps}.
 * @returns The fields, plus a warning if the times are not valid.
 */
function BlocksEditor({ label, blocks, onChange }: BlocksEditorProps) {
  const { t } = useTranslation();
  const problem = blocksProblem(blocks);

  /** Replaces one end of one block, if the typed time is valid. */
  function setTime(index: number, end: "startMinute" | "endMinute", text: string): void {
    const minutes = timeToMinutes(text);
    if (minutes === null) return;
    onChange(blocks.map((b, i) => (i === index ? { ...b, [end]: minutes } : b)));
  }

  /** Adds a block one hour after the last one (or a standard day if empty). */
  function addBlock(): void {
    const last = blocks[blocks.length - 1];
    if (last === undefined) {
      onChange(standardDay());
      return;
    }
    const start = Math.min(last.endMinute + 60, 1380);
    onChange([...blocks, { startMinute: start, endMinute: Math.min(start + 60, 1440) }]);
  }

  return (
    <div role="group" aria-label={label} className="flex flex-col gap-1">
      {blocks.length === 0 && <span className="text-fg-muted">{t("calendars.dayOff")}</span>}
      {blocks.map((block, i) => (
        // The index is a fine key here: blocks have no ID and are only
        // added or removed at known places.
        <div key={i} className="flex flex-wrap items-center gap-1">
          <input
            type="time"
            aria-label={t("calendars.from")}
            className={FIELD_CLASS}
            value={minutesToTime(block.startMinute)}
            onChange={(e) => setTime(i, "startMinute", e.target.value)}
          />
          <span aria-hidden="true">–</span>
          <input
            type="time"
            aria-label={t("calendars.to")}
            className={FIELD_CLASS}
            // `<input type="time">` cannot show 24:00; midnight is 00:00 there.
            value={minutesToTime(block.endMinute === 1440 ? 0 : block.endMinute)}
            onChange={(e) =>
              setTime(i, "endMinute", e.target.value === "00:00" ? "24:00" : e.target.value)
            }
          />
          <button
            type="button"
            className={BUTTON_CLASS}
            aria-label={t("calendars.removeBlock")}
            title={t("calendars.removeBlock")}
            onClick={() => onChange(blocks.filter((_, j) => j !== i))}
          >
            ×
          </button>
        </div>
      ))}
      <div className="flex flex-wrap gap-1">
        <button type="button" className={BUTTON_CLASS} onClick={addBlock}>
          {t("calendars.addBlock")}
        </button>
        {blocks.length > 0 && (
          <button type="button" className={BUTTON_CLASS} onClick={() => onChange([])}>
            {t("calendars.makeDayOff")}
          </button>
        )}
      </div>
      {problem !== null && (
        <span role="alert" className="text-danger-fg">
          {t("calendars.invalidTimes")}
        </span>
      )}
    </div>
  );
}

/**
 * The calendar editor dialog. Renders nothing visible while closed.
 *
 * @returns The `<dialog>` element.
 */
export function CalendarDialog() {
  const { t } = useTranslation();
  const dialogOpen = useCalendarDialogStore((s) => s.dialogOpen);
  const closeDialog = useCalendarDialogStore((s) => s.closeDialog);
  const calendars = useAppStore((s) => s.calendars);
  const snapshot = useAppStore((s) => s.snapshot);
  const applyCommand = useAppStore((s) => s.applyCommand);
  const baseId = useId();
  const dialogRef = useRef<HTMLDialogElement>(null);

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Calendar | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [newHoliday, setNewHoliday] = useState("");

  // Open / close the native dialog when the store says so.
  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog === null) return;
    if (dialogOpen && !dialog.open) {
      const { calendars: all, snapshot: snap } = useAppStore.getState();
      const first = all.find((c) => c.id === snap?.calendarId) ?? all[0] ?? null;
      setSelectedId(first?.id ?? null);
      setDraft(first === null ? null : copyCalendar(first));
      setError(null);
      dialog.showModal();
    } else if (!dialogOpen && dialog.open) {
      dialog.close();
    }
  }, [dialogOpen]);

  const stored = calendars.find((c) => c.id === selectedId) ?? null;
  const isProjectCalendar = snapshot !== null && snapshot.calendarId === selectedId;
  // Unapplied edits? Compare as JSON text: simple and exact for plain data.
  const dirty =
    stored !== null &&
    draft !== null &&
    JSON.stringify(copyCalendar(stored)) !== JSON.stringify(draft);

  /**
   * Picks a calendar in the list; its stored version becomes the draft.
   *
   * @param calendar - The calendar to edit.
   */
  function select(calendar: Calendar): void {
    setSelectedId(calendar.id);
    setDraft(copyCalendar(calendar));
    setError(null);
  }

  /**
   * Sends one command and shows Rust's refusal (e.g. `calendarInUse`) in
   * the dialog. The error banner of the main window would be hidden behind
   * the dialog, so the error is taken from the store and shown here.
   *
   * @param command - The command.
   * @returns `true` if Rust accepted it.
   */
  async function send(command: Command): Promise<boolean> {
    setBusy(true);
    setError(null);
    const ok = await applyCommand(command);
    setBusy(false);
    if (!ok) {
      const shown = useAppStore.getState().commandError;
      if (shown !== null) {
        setError("text" in shown ? shown.text : errorText(shown.error, t));
        // Shown here, so the main banner does not repeat it after closing.
        useAppStore.getState().dismissError();
      }
    }
    return ok;
  }

  /**
   * Adds a calendar (a standard week, or a copy of the selected one) and
   * selects it.
   *
   * @param copyOf - The calendar to copy, or `null` for a new standard one.
   */
  async function addCalendar(copyOf: Calendar | null): Promise<void> {
    const calendar: Calendar =
      copyOf === null
        ? {
            id: crypto.randomUUID(),
            name: t("calendars.newName"),
            week: standardWeek(),
            holidays: [],
            exceptions: [],
          }
        : {
            ...copyCalendar(copyOf),
            id: crypto.randomUUID(),
            name: t("calendars.copyName", { name: copyOf.name }),
          };
    if (await send({ type: "addCalendar", calendar })) select(calendar);
  }

  /** Deletes the selected calendar (Rust refuses if it is in use). */
  async function removeSelected(): Promise<void> {
    if (stored === null) return;
    if (await send({ type: "removeCalendar", id: stored.id })) {
      const next = useAppStore.getState().calendars[0] ?? null;
      setSelectedId(next?.id ?? null);
      setDraft(next === null ? null : copyCalendar(next));
    }
  }

  /** Sends the draft as one `updateCalendar` command. */
  async function applyDraft(): Promise<void> {
    if (draft === null) return;
    await send({ type: "updateCalendar", calendar: { ...draft, name: draft.name.trim() } });
  }

  /**
   * Changes the draft.
   *
   * @param patch - The fields to replace.
   */
  function patchDraft(patch: Partial<Calendar>): void {
    setDraft((d) => (d === null ? d : { ...d, ...patch }));
  }

  /**
   * Changes one exception of the draft.
   *
   * @param index - Its position in the list.
   * @param patch - The fields to replace.
   */
  function patchException(index: number, patch: Partial<CalendarException>): void {
    setDraft((d) =>
      d === null
        ? d
        : {
            ...d,
            exceptions: (d.exceptions ?? []).map((e, i) => (i === index ? { ...e, ...patch } : e)),
          },
    );
  }

  const exceptions = draft?.exceptions ?? [];

  return (
    <dialog
      ref={dialogRef}
      aria-labelledby={`${baseId}-title`}
      className="m-auto max-h-[calc(100vh-2rem)] w-[min(60rem,calc(100vw-2rem))] rounded-lg border border-line bg-surface p-0 text-fg shadow-xl backdrop:bg-black/40"
      onClose={closeDialog}
    >
      <div className="flex max-h-[calc(100vh-2rem)] flex-col">
        <h2
          id={`${baseId}-title`}
          className="border-b border-line px-4 py-3 text-base font-semibold"
        >
          {t("calendars.title")}
        </h2>
        {error !== null && (
          <p
            role="alert"
            className="border-b border-danger-line bg-danger-bg px-4 py-2 text-sm text-danger-fg"
          >
            {error}
          </p>
        )}

        <div className="flex min-h-0 flex-1 flex-col sm:flex-row">
          {/* ---- Calendar list and its buttons ---- */}
          <div className="flex shrink-0 flex-col gap-2 border-b border-line p-3 sm:w-60 sm:border-r sm:border-b-0">
            <ul aria-label={t("calendars.listLabel")} className="flex flex-col gap-1">
              {calendars.map((calendar) => (
                <li key={calendar.id}>
                  <button
                    type="button"
                    aria-pressed={calendar.id === selectedId}
                    className="w-full rounded px-2 py-1 text-left text-sm break-words hover:bg-app aria-pressed:bg-accent aria-pressed:text-accent-fg"
                    onClick={() => select(calendar)}
                  >
                    {snapshot?.calendarId === calendar.id
                      ? t("calendars.projectCalendarName", { name: calendar.name })
                      : calendar.name}
                  </button>
                </li>
              ))}
            </ul>
            <div className="flex flex-wrap gap-1">
              <button
                type="button"
                className={BUTTON_CLASS}
                disabled={busy}
                onClick={() => void addCalendar(null)}
              >
                {t("calendars.new")}
              </button>
              <button
                type="button"
                className={BUTTON_CLASS}
                disabled={busy || stored === null}
                onClick={() => void addCalendar(stored)}
              >
                {t("calendars.copy")}
              </button>
              <button
                type="button"
                className={BUTTON_CLASS}
                disabled={busy || stored === null}
                onClick={() => void removeSelected()}
              >
                {t("calendars.delete")}
              </button>
            </div>
          </div>

          {/* ---- The selected calendar ---- */}
          {draft === null ? (
            <p className="p-4 text-sm text-fg-muted">{t("calendars.none")}</p>
          ) : (
            <div className="flex min-w-0 flex-1 flex-col gap-4 overflow-y-auto p-4 text-sm">
              <label className="flex flex-col gap-1">
                <span className="font-medium">{t("calendars.name")}</span>
                <input
                  className={FIELD_CLASS}
                  value={draft.name}
                  onChange={(e) => patchDraft({ name: e.target.value })}
                />
              </label>
              <div className="flex flex-wrap items-center gap-2">
                {isProjectCalendar ? (
                  <span className="text-fg-muted">{t("calendars.isProjectCalendar")}</span>
                ) : (
                  <button
                    type="button"
                    className={BUTTON_CLASS}
                    disabled={busy || stored === null}
                    onClick={() =>
                      stored !== null &&
                      void send({ type: "setProjectCalendar", calendarId: stored.id })
                    }
                  >
                    {t("calendars.setProjectCalendar")}
                  </button>
                )}
              </div>

              {/* Work week: one row per weekday. */}
              <fieldset className="flex flex-col gap-2">
                <legend className="mb-1 font-medium">{t("calendars.workWeek")}</legend>
                {WEEKDAYS.map((day) => (
                  <div
                    key={day}
                    className="flex flex-col gap-1 border-b border-line pb-2 sm:flex-row sm:gap-3"
                  >
                    <span className="sm:w-32 sm:shrink-0">
                      {t(`calendars.weekday.${day}`)}
                      <span className="block text-xs text-fg-muted">
                        {t("calendars.hours", { hours: blocksMinutes(draft.week[day]) / 60 })}
                      </span>
                    </span>
                    <BlocksEditor
                      label={t(`calendars.weekday.${day}`)}
                      blocks={draft.week[day]}
                      onChange={(blocks) => patchDraft({ week: { ...draft.week, [day]: blocks } })}
                    />
                  </div>
                ))}
                {WEEKDAYS.every((day) => draft.week[day].length === 0) && (
                  <span role="alert" className="text-danger-fg">
                    {t("calendars.noWorkingTime")}
                  </span>
                )}
              </fieldset>

              {/* Exceptions: date ranges with their own working times. */}
              <fieldset className="flex flex-col gap-2">
                <legend className="mb-1 font-medium">{t("calendars.exceptions")}</legend>
                {exceptions.length === 0 && (
                  <span className="text-fg-muted">{t("calendars.noExceptions")}</span>
                )}
                {exceptions.map((exception, i) => (
                  <div key={i} className="flex flex-col gap-2 rounded border border-line p-2">
                    <div className="flex flex-wrap items-end gap-2">
                      <label className="flex min-w-40 flex-1 flex-col gap-1">
                        <span>{t("calendars.exceptionName")}</span>
                        <input
                          className={FIELD_CLASS}
                          value={exception.name}
                          onChange={(e) => patchException(i, { name: e.target.value })}
                        />
                      </label>
                      <label className="flex flex-col gap-1">
                        <span>{t("calendars.exceptionStart")}</span>
                        <DateField
                          className={FIELD_CLASS}
                          boxClassName="w-36"
                          value={exception.start}
                          onChange={(day) => patchException(i, { start: day })}
                        />
                      </label>
                      <label className="flex flex-col gap-1">
                        <span>{t("calendars.exceptionFinish")}</span>
                        <DateField
                          className={FIELD_CLASS}
                          boxClassName="w-36"
                          value={exception.finish}
                          onChange={(day) => patchException(i, { finish: day })}
                        />
                      </label>
                      <label className="flex items-center gap-1">
                        <input
                          type="checkbox"
                          checked={exception.workingTimes.length > 0}
                          onChange={(e) =>
                            patchException(i, {
                              workingTimes: e.target.checked ? standardDay() : [],
                            })
                          }
                        />
                        <span>{t("calendars.exceptionWorking")}</span>
                      </label>
                      <button
                        type="button"
                        className={BUTTON_CLASS}
                        onClick={() =>
                          patchDraft({ exceptions: exceptions.filter((_, j) => j !== i) })
                        }
                      >
                        {t("calendars.removeException")}
                      </button>
                    </div>
                    {exception.start > exception.finish && exception.finish !== "" && (
                      <span role="alert" className="text-danger-fg">
                        {t("calendars.invalidRange")}
                      </span>
                    )}
                    {exception.workingTimes.length > 0 && (
                      <BlocksEditor
                        label={exception.name}
                        blocks={exception.workingTimes}
                        onChange={(blocks) => patchException(i, { workingTimes: blocks })}
                      />
                    )}
                  </div>
                ))}
                <div>
                  <button
                    type="button"
                    className={BUTTON_CLASS}
                    onClick={() =>
                      patchDraft({
                        exceptions: [
                          ...exceptions,
                          {
                            name: t("calendars.newExceptionName"),
                            start: newHoliday || (snapshot?.projectStart ?? "").slice(0, 10),
                            finish: newHoliday || (snapshot?.projectStart ?? "").slice(0, 10),
                            workingTimes: [],
                          },
                        ],
                      })
                    }
                  >
                    {t("calendars.addException")}
                  </button>
                </div>
              </fieldset>

              {/* Holidays: single days off. */}
              <fieldset className="flex flex-col gap-2">
                <legend className="mb-1 font-medium">{t("calendars.holidays")}</legend>
                <ul className="flex flex-wrap gap-2">
                  {draft.holidays.map((day) => (
                    <li
                      key={day}
                      className="flex items-center gap-1 rounded border border-line px-2 py-0.5"
                    >
                      <span>{day}</span>
                      <button
                        type="button"
                        className="px-1"
                        aria-label={t("calendars.removeHoliday", { date: day })}
                        title={t("calendars.removeHoliday", { date: day })}
                        onClick={() =>
                          patchDraft({ holidays: draft.holidays.filter((h) => h !== day) })
                        }
                      >
                        ×
                      </button>
                    </li>
                  ))}
                </ul>
                <div className="flex flex-wrap items-center gap-2">
                  <DateField
                    label={t("calendars.holidayDate")}
                    className={FIELD_CLASS}
                    boxClassName="w-36"
                    value={newHoliday}
                    onChange={(day) => setNewHoliday(day)}
                  />
                  <button
                    type="button"
                    className={BUTTON_CLASS}
                    disabled={!isDay(newHoliday) || draft.holidays.includes(newHoliday)}
                    onClick={() => {
                      patchDraft({ holidays: [...draft.holidays, newHoliday].sort() });
                      setNewHoliday("");
                    }}
                  >
                    {t("calendars.addHoliday")}
                  </button>
                </div>
              </fieldset>
            </div>
          )}
        </div>

        <div className="flex flex-wrap justify-end gap-2 border-t border-line px-4 py-3">
          <button
            type="button"
            className={BUTTON_CLASS}
            disabled={!dirty || busy}
            onClick={() => stored !== null && setDraft(copyCalendar(stored))}
          >
            {t("calendars.revert")}
          </button>
          <button
            type="button"
            className={PRIMARY_CLASS}
            disabled={!dirty || busy || draft === null || !draftIsValid(draft)}
            onClick={() => void applyDraft()}
          >
            {t("calendars.apply")}
          </button>
          <button type="button" className={BUTTON_CLASS} onClick={closeDialog}>
            {t("calendars.close")}
          </button>
        </div>
      </div>
    </dialog>
  );
}
