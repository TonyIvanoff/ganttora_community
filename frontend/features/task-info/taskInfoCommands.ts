/**
 * features/task-info/taskInfoCommands.ts: the Task Information dialog's
 * form data, and how it becomes document commands.
 *
 * What it is for:
 *   The dialog edits a *draft*: a plain copy of the task's fields as the
 *   form shows them (text for durations and lags, "YYYY-MM-DD" for dates).
 *   Nothing is sent while the user types. On OK, {@link commandsFromDraft}
 *   compares the draft with the task, checks every field, and makes the
 *   commands for exactly the fields that changed.
 *
 * Where it fits:
 *   Used by `TaskInfoDialog.tsx`. The commands are sent with the store's
 *   `applyCommands`, which wraps several of them in one `batch` command:
 *   one OK is one undo step, and if Rust refuses one change (e.g. a link
 *   that makes a loop) none of them is applied.
 *
 * Main ideas:
 *   - **Draft first, commands on OK** (like MS Project's dialog): Cancel
 *     simply throws the draft away.
 *   - **Only changed fields** become commands, so the change journal lists
 *     what the user really changed.
 *   - **Dates keep their time of day** (`withDate` in `features/grid/cellEdit.ts`):
 *     a constraint date keeps the old constraint's time, else the task's
 *     start (for start constraints) or finish (for finish constraints).
 *   - **Blockers** are edited on their own tab; their
 *     commands are built by `blockers.ts` and added to the same batch.
 *   - **A typed start** works as in the grid: on an automatic task it becomes
 *     a "Start No Earlier Than" constraint (shown in the form at once, see
 *     {@link withStart}); on a manual task it is stored as typed.
 *   - Pure functions: easy to test.
 */

import {
  actualFinishCommands,
  constraintFor,
  dayOf,
  deadlineCommands,
  isEditable,
  plannedFinishCommands,
  withDate,
} from "@frontend/features/grid/cellEdit";
import {
  formatLag,
  linkCommands,
  type PredecessorContext,
  type WantedLink,
} from "@frontend/features/grid/predecessors";
import type { Command, Constraint, DependencyType, TaskMode, TaskRow } from "@frontend/ipc/types";
import { minutesToDays, parseDuration, parseLag } from "@frontend/lib/units";
import { blockerCommands, type DraftBlocker } from "./blockers";
import { riskCommands, type DraftRisk } from "./risks";

/** One row of the Predecessors table in the dialog. */
export interface DraftLink {
  /** A key for React's list (the link ID, or a new random one). */
  key: string;
  /** The predecessor task's ID, or "" while none is chosen. */
  predecessorId: string;
  /** Link type. */
  kind: DependencyType;
  /** The lag as typed, e.g. "+1d", "-20%", or "" for none. */
  lagText: string;
}

/** The dialog's form data. */
export interface TaskInfoDraft {
  /** Task name. */
  name: string;
  /** Duration as typed, e.g. "5" or "2w". */
  durationText: string;
  /**
   * The duration text exactly as the dialog first showed it. Why keep it:
   * the shown text is rounded (60 minutes shows as "0.13d", which reads
   * back as 62 minutes), so comparing *minutes* would send a change the
   * user never made. If the text is untouched, the duration is not sent.
   * Optional so hand-built drafts still work; then minutes are compared.
   */
  initialDurationText?: string;
  /** Auto or manual. */
  mode: TaskMode;
  /** The milestone mark. */
  milestone: boolean;
  /** Web or file link. */
  hyperlink: string;
  /** Notes text. */
  notes: string;
  /** Start day "YYYY-MM-DD" ("" if the task has no calculated start). */
  start: string;
  /** Planned finish day, or "" for no plan (see "Set plan"). */
  plannedFinish: string;
  /** Actual finish day, or "" while not finished. */
  actualFinish: string;
  /** Constraint type. */
  constraintType: Constraint["type"];
  /** Constraint day "YYYY-MM-DD" ("" for the two kinds without a date). */
  constraintDate: string;
  /** Deadline day "YYYY-MM-DD", or "" for none. */
  deadline: string;
  /** Task calendar ID, or "" for the project calendar. */
  calendarId: string;
  /** The incoming links. */
  links: DraftLink[];
  /**
   * The task's blockers, edited on the Blockers tab.
   * Optional: when missing, blockers are left as they are. The dialog
   * fills it with `draftBlockersFromTask` (blockers.ts).
   */
  blockers?: DraftBlocker[];
  /** The task's risks (Risks tab). Optional, like `blockers`. */
  risks?: DraftRisk[];
}

/** The fields of the dialog an error can point at. */
export type DraftField =
  | "duration"
  | "start"
  | "plannedFinish"
  | "actualFinish"
  | "constraintDate"
  | "deadline"
  | "links"
  | "blockers"
  | "risks";

/** What {@link commandsFromDraft} answers. */
export type DraftResult =
  | { ok: true; commands: Command[] }
  | {
      ok: false;
      /** The field to show the error at. */
      field: DraftField;
      /** Translation key of the message. */
      key: string;
      /** Its placeholder values. */
      values?: Record<string, string | number>;
    };

/** The constraint types that carry a date. */
const DATED: ReadonlySet<Constraint["type"]> = new Set([
  "startNoEarlierThan",
  "startNoLaterThan",
  "finishNoEarlierThan",
  "finishNoLaterThan",
  "mustStartOn",
  "mustFinishOn",
]);

/**
 * Tells whether a constraint type needs a date.
 *
 * @param type - The constraint type.
 * @returns `true` for the six dated kinds, `false` for ASAP and ALAP.
 */
export function needsDate(type: Constraint["type"]): boolean {
  return DATED.has(type);
}

/**
 * Makes the draft for a task, as the dialog first shows it.
 *
 * @param task - The task.
 * @param units - Unit sizes and words (for the duration and lag text).
 * @param formatDays - Writes a number of days for the duration field
 *   (in the user's number format, without unit), e.g. `(d) => "2,5"`.
 * @returns The draft.
 *
 * @example
 * const draft = draftFromTask(task, units, (d) => String(d));
 * draft.durationText; // "5"
 */
export function draftFromTask(
  task: TaskRow,
  units: PredecessorContext,
  formatDays: (days: number) => string,
): TaskInfoDraft {
  const constraint = task.constraint;
  const durationText = `${formatDays(minutesToDays(task.durationMinutes, units.basis))}${units.unitShort.day}`;
  return {
    name: task.name,
    durationText,
    initialDurationText: durationText,
    mode: task.mode,
    // A zero-duration task is drawn as a milestone without the mark; the
    // checkbox shows the *mark*, which only this flag tells us… but the
    // snapshot only has `isMilestone` (mark or zero duration). Treat a
    // zero-duration task as unmarked, so OK does not add a mark by itself.
    milestone: task.isMilestone && task.durationMinutes > 0,
    hyperlink: task.hyperlink,
    notes: task.notes,
    start: dayOf(task.start),
    plannedFinish: dayOf(task.plannedFinish ?? null),
    actualFinish: dayOf(task.actualFinish ?? null),
    constraintType: constraint.type,
    constraintDate: "date" in constraint ? dayOf(constraint.date) : "",
    deadline: dayOf(task.deadline),
    calendarId: task.calendarId ?? "",
    links: task.links.map((link) => ({
      key: link.id,
      predecessorId: link.predecessor,
      kind: link.kind,
      lagText: formatLag(link.lagMinutes, link.lagPercent, units),
    })),
  };
}

/**
 * The form after the user typed a start date. On an automatic task the
 * constraint fields change with it, so the user sees what the date means.
 *
 * @param task - The task as it is now.
 * @param draft - The draft.
 * @param start - The typed day; the task's own day undoes the change.
 * @returns The changed fields.
 *
 * @example
 * withStart(task, draft, "2027-03-08");
 * // { start: "2027-03-08", constraintType: "startNoEarlierThan", constraintDate: "2027-03-08" }
 */
export function withStart(
  task: TaskRow,
  draft: TaskInfoDraft,
  start: string,
): Pick<TaskInfoDraft, "start" | "constraintType" | "constraintDate"> {
  const own = dayOf(task.start);
  if (draft.mode === "auto" && start !== own) {
    return { start, constraintType: "startNoEarlierThan", constraintDate: start };
  }
  // Typed back to the task's own day, or a manual task: if an earlier typed
  // start had set the constraint, the old constraint returns. A constraint
  // the user chose in the form stays.
  const { constraint } = task;
  const setByStart =
    draft.start !== own &&
    draft.constraintType === "startNoEarlierThan" &&
    draft.constraintDate === draft.start;
  if (!setByStart) {
    return { start, constraintType: draft.constraintType, constraintDate: draft.constraintDate };
  }
  return {
    start,
    constraintType: constraint.type,
    constraintDate: "date" in constraint ? dayOf(constraint.date) : "",
  };
}

/**
 * The form after the user chose another scheduling mode: a typed start then
 * means something else, so it is applied again in the new mode.
 *
 * @param task - The task as it is now.
 * @param draft - The draft.
 * @param mode - The chosen mode.
 * @returns The changed fields.
 */
export function withMode(
  task: TaskRow,
  draft: TaskInfoDraft,
  mode: TaskMode,
): Partial<TaskInfoDraft> {
  return { mode, ...withStart(task, { ...draft, mode }, draft.start) };
}

/**
 * Builds the constraint from the draft, keeping a sensible time of day.
 *
 * @param task - The task as it is now.
 * @param draft - The draft.
 * @returns The constraint, or `null` if a date is needed but invalid.
 */
function draftConstraint(task: TaskRow, draft: TaskInfoDraft): Constraint | null {
  const type = draft.constraintType;
  if (type === "asSoonAsPossible" || type === "asLateAsPossible") return { type };
  // Set by a typed start: keeps the start's time of day, as in the grid.
  if (type === "startNoEarlierThan" && draft.start !== dayOf(task.start)) {
    const date = withDate(task.start, draft.constraintDate);
    return date === null ? null : { type, date };
  }
  return constraintFor(task, type, draft.constraintDate);
}

/**
 * Compares two constraints.
 *
 * @param a - One constraint.
 * @param b - The other.
 * @returns `true` if type and date are equal.
 */
function sameConstraint(a: Constraint, b: Constraint): boolean {
  return a.type === b.type && ("date" in a ? a.date : null) === ("date" in b ? b.date : null);
}

/**
 * Turns the draft into the commands for the fields that changed.
 *
 * @param task - The task as it is now.
 * @param draft - The dialog's draft.
 * @param units - Unit sizes and words.
 * @param newId - Makes a UUID for each new link.
 * @returns The commands (maybe none), or the first problem found.
 *
 * @example
 * const result = commandsFromDraft(task, { ...draft, name: "Permit" }, units, () => crypto.randomUUID());
 * // { ok: true, commands: [{ type: "renameTask", id, name: "Permit" }] }
 */
export function commandsFromDraft(
  task: TaskRow,
  draft: TaskInfoDraft,
  units: PredecessorContext,
  newId: () => string,
): DraftResult {
  const id = task.id;
  const commands: Command[] = [];

  if (draft.name !== task.name) commands.push({ type: "renameTask", id, name: draft.name });

  // A summary's duration is rolled up from its subtasks: never sent.
  // An untouched duration text is never sent either (see initialDurationText).
  if (!task.isSummary && draft.durationText !== draft.initialDurationText) {
    const minutes = parseDuration(draft.durationText, units.basis, units.unitWords);
    if (minutes === null) return { ok: false, field: "duration", key: "grid.invalid.duration" };
    if (minutes !== task.durationMinutes) {
      commands.push({ type: "setTaskDuration", id, duration: { minutes } });
    }
  }

  if (draft.mode !== task.mode) {
    commands.push({ type: "setTaskMode", id, mode: draft.mode, manualStart: null });
  }

  const wasMarked = task.isMilestone && task.durationMinutes > 0;
  if (draft.milestone !== wasMarked) {
    commands.push({ type: "setTaskMilestone", id, milestone: draft.milestone });
  }

  if (draft.start !== dayOf(task.start) && isEditable("start", task)) {
    const start = withDate(task.start, draft.start);
    if (start === null) return { ok: false, field: "start", key: "grid.invalid.date" };
    // An automatic task gets the constraint below instead (see withStart).
    if (draft.mode === "manual") commands.push({ type: "setTaskManualStart", id, start });
  }

  // Groups and projects show their plan; only "Set plan" changes it.
  if (isEditable("plannedFinish", task)) {
    const planned = plannedFinishCommands(task, draft.plannedFinish);
    if (planned === null) return { ok: false, field: "plannedFinish", key: "grid.invalid.date" };
    commands.push(...planned);
  }

  if (isEditable("actualFinish", task)) {
    const actual = actualFinishCommands(task, draft.actualFinish);
    if (actual === null) return { ok: false, field: "actualFinish", key: "grid.invalid.date" };
    commands.push(...actual);
  }

  const constraint = draftConstraint(task, draft);
  if (constraint === null) {
    return { ok: false, field: "constraintDate", key: "taskInfo.invalid.constraintDate" };
  }
  if (!sameConstraint(constraint, task.constraint)) {
    commands.push({ type: "setTaskConstraint", id, constraint });
  }

  const deadline = deadlineCommands(task, draft.deadline);
  if (deadline === null) return { ok: false, field: "deadline", key: "grid.invalid.date" };
  commands.push(...deadline);

  const calendarId = draft.calendarId === "" ? null : draft.calendarId;
  if (calendarId !== task.calendarId) commands.push({ type: "setTaskCalendar", id, calendarId });
  if (draft.notes !== task.notes) commands.push({ type: "setTaskNotes", id, notes: draft.notes });
  if (draft.hyperlink !== task.hyperlink) {
    commands.push({ type: "setTaskHyperlink", id, hyperlink: draft.hyperlink });
  }

  // Links: rows without a chosen task are ignored (an empty new row).
  const wanted: WantedLink[] = [];
  const seen = new Set<string>();
  for (const [i, link] of draft.links.entries()) {
    if (link.predecessorId === "") continue;
    const row = i + 1;
    if (link.predecessorId === id) {
      return { ok: false, field: "links", key: "taskInfo.invalid.selfLink", values: { row } };
    }
    if (seen.has(link.predecessorId)) {
      return { ok: false, field: "links", key: "taskInfo.invalid.duplicateLink", values: { row } };
    }
    seen.add(link.predecessorId);
    const lag = parseLag(link.lagText, units.basis, units.unitWords);
    if (lag === null)
      return { ok: false, field: "links", key: "taskInfo.invalid.lag", values: { row } };
    wanted.push({ predecessor: link.predecessorId, kind: link.kind, lag });
  }
  commands.push(...linkCommands(id, task.links, wanted, newId));

  // Blockers: removed, added, moved or edited ones (see blockers.ts).
  if (draft.blockers !== undefined) {
    const result = blockerCommands(task, draft.blockers, units, newId);
    if (!result.ok) {
      return {
        ok: false,
        field: "blockers",
        key: `taskInfo.invalid.blocker.${result.field}`,
        values: { number: result.number },
      };
    }
    commands.push(...result.commands);
  }

  // Risks: removed, added or edited ones (see risks.ts).
  if (draft.risks !== undefined) {
    const result = riskCommands(task, draft.risks, newId);
    if (!result.ok) {
      return {
        ok: false,
        field: "risks",
        key: `taskInfo.invalid.risk.${result.field}`,
        values: { number: result.number },
      };
    }
    commands.push(...result.commands);
  }

  return { ok: true, commands };
}
