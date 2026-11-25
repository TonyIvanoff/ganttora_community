/**
 * features/task-info/blockers.ts: the blocker forms and how
 * they become document commands.
 *
 * What it is for:
 *   A *blocker* is an unplanned delay recorded on a task: "the wood
 *   delivery is two weeks late". The user enters four things: the day the
 *   task is stuck from, how long (in working days), why, and the day they
 *   heard about it. Two places use these fields:
 *   - the small "Add blocker" dialog (`BlockerDialog.tsx`), which sends one
 *     `addBlockerAt` command;
 *   - the "Blockers" tab of the Task Information dialog, which edits and
 *     removes the task's blockers and sends the changes with the dialog's
 *     OK, in the same batch (one undo step).
 *   This file holds the checks and the command building for both, as small
 *   pure functions.
 *
 * Where it fits:
 *   Used by `BlockerDialog.tsx`, `TaskInfoDialog.tsx` and
 *   `taskInfoCommands.ts`. The commands go to Rust through the store
 *   (decision D16); Rust refuses blockers on summary tasks
 *   (`blockerOnSummary`) and the UI shows the translated reason.
 *
 * Main ideas:
 *   - **No scheduling here.** The UI sends the *date* the blocker starts
 *     (`addBlockerAt`); the core works out how much of the task's work lies
 *     before that date. The UI never calculates working time itself.
 *   - **Moving a blocker** to another day is "remove it and add it again at
 *     the new date" (same ID), sent together in one batch. Changing only
 *     the length, reason or reported day is one `editBlocker`, which keeps
 *     the blocker's position exactly.
 *   - **Lengths are working time**, read with `lib/units.ts` like every
 *     duration: a bare number is days ("5"), units are allowed ("2w", "4h").
 *   - Dates in the form are "YYYY-MM-DD" (the value of a `DateField`).
 */

import { dayOf } from "@frontend/features/grid/cellEdit";
import type { PredecessorContext } from "@frontend/features/grid/predecessors";
import { subtreeEnd } from "@frontend/features/grid/outline";
import type { BlockerRow, Command, IsoDate, IsoDateTime, TaskRow, Uuid } from "@frontend/ipc/types";
import { minutesToDays, parseDuration } from "@frontend/lib/units";

/** The four fields of a blocker form, as typed. */
export interface BlockerFormValues {
  /** The day the task is blocked from, "YYYY-MM-DD". */
  startDate: string;
  /** The length as typed, e.g. "5", "2w", "3d". */
  lengthText: string;
  /** Why the task is blocked. Required. */
  reason: string;
  /** The day the delay was reported, "YYYY-MM-DD", or "" for none. */
  reportedOn: string;
}

/** A field of the blocker form that can have a problem. */
export type BlockerField = "start" | "length" | "reason" | "reportedOn";

/** A blocker form that passed the checks, ready for a command. */
export interface ParsedBlocker {
  /** The moment the blocker starts: midnight of the chosen day. */
  at: IsoDateTime;
  /** Blocked time in working minutes (more than zero). */
  durationMinutes: number;
  /** The reason, without spaces at the ends. */
  reason: string;
  /** The reported day, or `null`. */
  reportedOn: IsoDate | null;
}

/** What {@link parseBlockerForm} answers. */
export type BlockerParse = { ok: true; value: ParsedBlocker } | { ok: false; field: BlockerField };

/** One blocker in the Task Information dialog's draft. */
export interface DraftBlocker {
  /** A key for React's list: the blocker's ID, or a new random one. */
  key: string;
  /** The blocker's ID, or `null` for a blocker added in this dialog. */
  id: Uuid | null;
  /** The form fields. */
  values: BlockerFormValues;
  /**
   * The fields as the dialog first showed them (`null` for a new blocker).
   * Comparing with them tells which fields the user really changed (a
   * rounded length text must not count as a change).
   */
  initial: BlockerFormValues | null;
  /** The stored position ("after this much work"), kept by `editBlocker`. */
  afterWorkMinutes: number;
}

/** A problem in the Blockers tab: which blocker (counted from 1) and which field. */
export interface DraftBlockerProblem {
  /** The blocker's number in the list, from 1. */
  number: number;
  /** The field. */
  field: BlockerField;
}

/** What {@link blockerCommands} answers. */
export type BlockerCommandsResult =
  { ok: true; commands: Command[] } | ({ ok: false } & DraftBlockerProblem);

/** A valid "YYYY-MM-DD" text (the shape a `DateField` gives). */
const DAY_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

/**
 * Today's date on the user's computer, as "YYYY-MM-DD".
 *
 * @param now - The moment to use (default: now). Tests pass a fixed date.
 * @returns The *local* calendar day (not UTC): at 00:30 in Vilnius it is
 *   already the new day, even though UTC is still on the old one.
 *
 * @example
 * todayIso(new Date(2027, 2, 4, 10, 0)); // "2027-03-04"
 *
 * Beginner note: `padStart(2, "0")` turns "3" into "03", so months and
 * days always have two digits.
 */
export function todayIso(now: Date = new Date()): IsoDate {
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${now.getFullYear()}-${month}-${day}`;
}

/**
 * Checks a blocker form and reads its values.
 *
 * @param values - The form fields.
 * @param units - Unit sizes and words of the project and UI language.
 * @returns `{ ok: true, value }`, or `{ ok: false, field }` for the first
 *   field with a problem, in form order: no valid start day, a length that
 *   cannot be read or is zero, an empty reason, an invalid reported day.
 *
 * @example
 * parseBlockerForm({ startDate: "2027-03-05", lengthText: "5", reason: "Wood late", reportedOn: "" }, units);
 * // { ok: true, value: { at: "2027-03-05T00:00:00", durationMinutes: 2400, reason: "Wood late", reportedOn: null } }
 */
export function parseBlockerForm(
  values: BlockerFormValues,
  units: PredecessorContext,
): BlockerParse {
  if (!DAY_PATTERN.test(values.startDate)) return { ok: false, field: "start" };
  const minutes = parseDuration(values.lengthText, units.basis, units.unitWords);
  if (minutes === null || minutes <= 0) return { ok: false, field: "length" };
  const reason = values.reason.trim();
  if (reason === "") return { ok: false, field: "reason" };
  if (values.reportedOn !== "" && !DAY_PATTERN.test(values.reportedOn)) {
    return { ok: false, field: "reportedOn" };
  }
  return {
    ok: true,
    value: {
      // Midnight: the core counts the task's work *before* this moment, so
      // all work of the chosen day comes after the blocker.
      at: `${values.startDate}T00:00:00`,
      durationMinutes: minutes,
      reason,
      reportedOn: values.reportedOn === "" ? null : values.reportedOn,
    },
  };
}

/**
 * The command that adds a blocker at a date.
 *
 * @param taskId - The blocked task.
 * @param id - The new blocker's UUID (made by the caller).
 * @param blocker - The checked form values.
 * @returns An `addBlockerAt` command.
 *
 * @example
 * addBlockerCommand(task.id, crypto.randomUUID(), parsed.value);
 * // { type: "addBlockerAt", task: "…", id: "…", at: "2027-03-05T00:00:00", duration: { minutes: 2400 }, … }
 */
export function addBlockerCommand(taskId: Uuid, id: Uuid, blocker: ParsedBlocker): Command {
  return {
    type: "addBlockerAt",
    task: taskId,
    id,
    at: blocker.at,
    duration: { minutes: blocker.durationMinutes },
    reason: blocker.reason,
    reportedOn: blocker.reportedOn,
  };
}

/**
 * Writes working minutes as the length field shows them, e.g. "5d".
 *
 * @param minutes - Working minutes.
 * @param units - Unit sizes and the short day letter of the UI language.
 * @returns The number of days (rounded to two decimals, user's number
 *   format) followed by the day unit.
 *
 * @example
 * formatBlockerLength(2400, units); // "5d"
 */
export function formatBlockerLength(minutes: number, units: PredecessorContext): string {
  return `${units.formatNumber(minutesToDays(minutes, units.basis))}${units.unitShort.day}`;
}

/**
 * Makes the Blockers tab's draft from a task's blockers.
 *
 * @param task - The task (its `blockers` may be missing: no blockers).
 * @param units - Unit sizes and words.
 * @returns One draft entry per blocker, in the task's list order. The
 *   start day is the day of the calculated blocked piece ("" when the
 *   schedule has none).
 *
 * @example
 * draftBlockersFromTask(task, units)[0]?.values.lengthText; // "5d"
 */
export function draftBlockersFromTask(task: TaskRow, units: PredecessorContext): DraftBlocker[] {
  return (task.blockers ?? []).map((blocker) => {
    const values: BlockerFormValues = {
      startDate: dayOf(blocker.start),
      lengthText: formatBlockerLength(blocker.durationMinutes, units),
      reason: blocker.reason,
      reportedOn: blocker.reportedOn ?? "",
    };
    return {
      key: blocker.id,
      id: blocker.id,
      values,
      initial: { ...values },
      afterWorkMinutes: blocker.afterWorkMinutes,
    };
  });
}

/**
 * A new, empty entry for the Blockers tab.
 *
 * @param key - A unique key (e.g. `crypto.randomUUID()`).
 * @param today - Today's date "YYYY-MM-DD" (default start and reported day).
 * @returns The entry; its length and reason are empty.
 */
export function newDraftBlocker(key: string, today: IsoDate): DraftBlocker {
  return {
    key,
    id: null,
    values: { startDate: today, lengthText: "", reason: "", reportedOn: today },
    initial: null,
    afterWorkMinutes: 0,
  };
}

/**
 * Turns the Blockers tab's draft into commands for what changed.
 *
 * @param task - The task as it is now.
 * @param drafts - The draft entries.
 * @param units - Unit sizes and words.
 * @param newId - Makes a UUID for each new blocker.
 * @returns The commands (maybe none), or the first problem found:
 *   - a blocker that is no longer in the draft → `removeBlocker`;
 *   - a new entry → `addBlockerAt`;
 *   - a changed start day → `removeBlocker` + `addBlockerAt` with the same
 *     ID (the core works out the new position from the date);
 *   - otherwise a changed length, reason or reported day → `editBlocker`
 *     (same position).
 *   Untouched entries are not checked, so an old blocker with an empty
 *   reason does not block OK.
 *
 * @example
 * blockerCommands(task, [], units, () => crypto.randomUUID());
 * // { ok: true, commands: [{ type: "removeBlocker", task: "…", id: "…" }] } (the task had one)
 */
export function blockerCommands(
  task: TaskRow,
  drafts: readonly DraftBlocker[],
  units: PredecessorContext,
  newId: () => string,
): BlockerCommandsResult {
  const commands: Command[] = [];
  const kept = new Set(drafts.flatMap((d) => (d.id === null ? [] : [d.id])));
  for (const blocker of task.blockers ?? []) {
    if (!kept.has(blocker.id))
      commands.push({ type: "removeBlocker", task: task.id, id: blocker.id });
  }

  for (const [index, draft] of drafts.entries()) {
    const number = index + 1;
    const initial = draft.initial;
    if (draft.id === null || initial === null) {
      // A blocker added in this dialog.
      const parsed = parseBlockerForm(draft.values, units);
      if (!parsed.ok) return { ok: false, number, field: parsed.field };
      commands.push(addBlockerCommand(task.id, newId(), parsed.value));
      continue;
    }
    if (sameValues(draft.values, initial)) continue;

    // An untouched length text keeps the exact old minutes (the shown text
    // is rounded to two decimals of a day, so reading it back could differ).
    // This holds for a moved blocker as well as for an edited one.
    const oldMinutes =
      draft.values.lengthText === initial.lengthText
        ? task.blockers?.find((b) => b.id === draft.id)?.durationMinutes
        : undefined;

    if (draft.values.startDate !== initial.startDate) {
      // Moved to another day: remove, then add again at the new date. Both
      // travel in the dialog's one batch, so this is one undo step.
      const parsed = parseBlockerForm(draft.values, units);
      if (!parsed.ok) return { ok: false, number, field: parsed.field };
      commands.push({ type: "removeBlocker", task: task.id, id: draft.id });
      const durationMinutes = oldMinutes ?? parsed.value.durationMinutes;
      commands.push(addBlockerCommand(task.id, draft.id, { ...parsed.value, durationMinutes }));
      continue;
    }

    // Same day: keep the stored position, change the other fields. The
    // start day is not needed (it may even be empty when the schedule
    // failed), so any valid day stands in for it during the checks.
    const parsed = parseBlockerForm({ ...draft.values, startDate: "2000-01-01" }, units);
    if (!parsed.ok) return { ok: false, number, field: parsed.field };
    const durationMinutes = oldMinutes ?? parsed.value.durationMinutes;
    commands.push({
      type: "editBlocker",
      task: task.id,
      blocker: {
        id: draft.id,
        afterWork: { minutes: draft.afterWorkMinutes },
        duration: { minutes: durationMinutes },
        reason: parsed.value.reason,
        reportedOn: parsed.value.reportedOn,
      },
    });
  }
  return { ok: true, commands };
}

/**
 * Compares two sets of form values field by field.
 *
 * @param a - One set.
 * @param b - The other.
 * @returns `true` when all four fields are equal.
 */
function sameValues(a: BlockerFormValues, b: BlockerFormValues): boolean {
  return (
    a.startDate === b.startDate &&
    a.lengthText === b.lengthText &&
    a.reason === b.reason &&
    a.reportedOn === b.reportedOn
  );
}

/** One blocker in a project's list. */
export interface ProjectBlocker {
  /** The blocked task's name. */
  taskName: string;
  /** The blocker. */
  blocker: BlockerRow;
}

/**
 * Lists the blockers of all tasks in a project.
 *
 * @param rows - All rows, in list order.
 * @param projectId - The project row's ID.
 * @returns The blockers in row order (empty for an unknown project).
 *
 * @example
 * projectBlockers(snapshot.tasks, garage.id)[0]?.taskName; // "Dig the foundation"
 */
export function projectBlockers(rows: readonly TaskRow[], projectId: Uuid): ProjectBlocker[] {
  const first = rows.findIndex((row) => row.id === projectId);
  if (first < 0) return [];
  return rows
    .slice(first + 1, subtreeEnd(rows, first))
    .flatMap((row) => (row.blockers ?? []).map((blocker) => ({ taskName: row.name, blocker })));
}
