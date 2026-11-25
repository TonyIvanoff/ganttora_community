/**
 * tests/frontend/grid/helpers.ts: shared test data for grid, task-info and
 * Gantt-interaction tests.
 *
 * What it is for:
 *   Many tests need one `TaskRow` (a row of the schedule snapshot Rust
 *   sends) with only a few fields changed, and the English unit settings
 *   the grid uses to read "3d" or "2FS+1d". This file builds both. It is
 *   not a test file (its name does not end in `.test.ts`), so Vitest does
 *   not run it; test files import it.
 *
 * Main ideas:
 *   - `taskRow({ name: "Paint" })` starts from a complete, ordinary task
 *     (auto-scheduled, 1 day, ASAP, Mon 1 Mar 2027 08:00–17:00) and changes
 *     only what the test cares about. A reader then sees at a glance what
 *     is special about the task in each test.
 *   - `englishUnits()` is the same `PredecessorContext` the app builds for
 *     English (link codes FS/SS/FF/SF, units d/h/m/w/mo, 480-minute days).
 */

import type { TaskRow } from "@frontend/ipc/types";
import { ENGLISH_LINK_CODES, type PredecessorContext } from "@frontend/features/grid/predecessors";
import { durationBasis } from "@frontend/lib/units";

/**
 * Builds a complete `TaskRow`, then applies `changes`.
 *
 * @param changes - The fields to change (TypeScript's `Partial` means
 *   "any subset of the fields").
 * @returns A new task row. Defaults: id "task-1", name "Task", 480 minutes,
 *   level 1, auto mode, ASAP, start Mon 1 Mar 2027 08:00, finish 17:00.
 *
 * @example
 * const manual = taskRow({ id: "t2", mode: "manual" });
 */
export function taskRow(changes: Partial<TaskRow> = {}): TaskRow {
  return {
    id: "task-1",
    name: "Task",
    durationMinutes: 480,
    outlineLevel: 1,
    outlineNumber: "1",
    parentId: null,
    isSummary: false,
    isMilestone: false,
    mode: "auto",
    constraint: { type: "asSoonAsPossible" },
    deadline: null,
    calendarId: null,
    notes: "",
    hyperlink: "",
    start: "2027-03-01T08:00:00",
    finish: "2027-03-01T17:00:00",
    earlyStart: "2027-03-01T08:00:00",
    earlyFinish: "2027-03-01T17:00:00",
    lateStart: "2027-03-01T08:00:00",
    lateFinish: "2027-03-01T17:00:00",
    totalSlackMinutes: 0,
    freeSlackMinutes: 0,
    isCritical: false,
    missedDeadline: false,
    predecessors: [],
    links: [],
    explanation: null,
    ...changes,
  };
}

/**
 * The English unit and link-code settings, as `useUnitContext` builds them
 * for an English UI and a 480-minute day.
 *
 * @param minutesPerDay - Minutes in one working day (default 480).
 * @returns A `PredecessorContext`. Numbers are formatted with `String`
 *   (so 1.5 prints as "1.5").
 */
export function englishUnits(minutesPerDay = 480): PredecessorContext {
  return {
    basis: durationBasis(minutesPerDay),
    codes: ENGLISH_LINK_CODES,
    unitShort: { minute: "m", hour: "h", day: "d", week: "w", month: "mo" },
    formatNumber: (value) => String(value),
  };
}
