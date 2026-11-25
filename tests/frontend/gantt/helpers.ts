/**
 * tests/frontend/gantt/helpers.ts: shared test data for the Gantt tests.
 *
 * What it is for:
 *   Several test files need the garden shed as Gantt input,
 *   and as a `ScheduleSnapshot` like the one Rust sends. This file builds
 *   both once. It is not a test file itself (its name does not end in
 *   `.test.ts`), so Vitest does not run it; test files import it.
 *
 * Main ideas:
 *   Dates are the ones the engine calculates for the shed,
 *   written as ISO text without a time zone, exactly as Rust sends them.
 */

import type { ScheduleSnapshot, TaskRow } from "@frontend/ipc/types";
import type { GanttSourceTask } from "@frontend/features/gantt/renderer/model";

/** One shed row: name, early start, early finish, critical, predecessor rows (1-based). */
type ShedRow = [string, string, string, boolean, number[]];

/** The garden shed, as calculated by the engine. */
const SHED: ShedRow[] = [
  ["Design the shed", "2027-03-01T08:00:00", "2027-03-02T17:00:00", true, []],
  ["Get building permit", "2027-03-03T08:00:00", "2027-03-09T17:00:00", true, [1]],
  ["Buy materials", "2027-03-03T08:00:00", "2027-03-05T17:00:00", false, [1]],
  ["Prepare the ground", "2027-03-10T08:00:00", "2027-03-11T17:00:00", true, [2]],
  ["Build the floor", "2027-03-12T08:00:00", "2027-03-15T17:00:00", true, [3, 4]],
  ["Build walls and roof", "2027-03-16T08:00:00", "2027-03-19T17:00:00", true, [5]],
  ["Paint", "2027-03-22T08:00:00", "2027-03-23T17:00:00", true, [6]],
  ["Shed finished", "2027-03-23T17:00:00", "2027-03-23T17:00:00", true, [7]],
];

/** A readable fake task ID for a 1-based row number, e.g. "task-3". */
export function taskId(row: number): string {
  return `task-${row}`;
}

/** The shed as Gantt input (what `snapshotToGanttTasks` produces). */
export function shedTasks(): GanttSourceTask[] {
  return SHED.map(([name, start, finish, isCritical, preds], i) => ({
    id: taskId(i + 1),
    name,
    start,
    finish,
    isCritical,
    predecessors: preds.map(taskId),
    isSummary: false,
    outlineLevel: 1,
    // Only "Shed finished" (zero duration) is a milestone.
    isMilestone: start === finish,
  }));
}

/**
 * The shed as a `ScheduleSnapshot`, the JSON object Rust sends to the UI.
 * Every row is a complete M1 `TaskRow` (outline, mode, constraint, scheduled
 * start/finish, links, "Why this date?" explanation), so the type checker
 * catches any field the Rust side adds or renames.
 *
 * @param failed - When true, imitate a failed schedule: every date, slack
 *   and explanation is `null`, nothing is critical, and `error` holds a
 *   coded error `{ code: "cycleDetected", details: { tasks, names } }`, the
 *   shape Rust sends since M1 (the UI translates the code).
 */
export function shedSnapshot(failed = false): ScheduleSnapshot {
  const tasks: TaskRow[] = SHED.map(([name, start, finish, isCritical, preds], i) => ({
    id: taskId(i + 1),
    name,
    durationMinutes: 480,
    outlineLevel: 1,
    outlineNumber: String(i + 1),
    parentId: null,
    isSummary: false,
    isMilestone: start === finish,
    mode: "auto",
    constraint: { type: "asSoonAsPossible" },
    deadline: null,
    calendarId: null,
    notes: "",
    hyperlink: "",
    start: failed ? null : start,
    finish: failed ? null : finish,
    earlyStart: failed ? null : start,
    earlyFinish: failed ? null : finish,
    lateStart: failed ? null : start,
    lateFinish: failed ? null : finish,
    totalSlackMinutes: failed ? null : 0,
    freeSlackMinutes: failed ? null : 0,
    isCritical: failed ? false : isCritical,
    missedDeadline: false,
    predecessors: preds.map(taskId),
    links: preds.map((p) => ({
      id: `link-${p}-${i + 1}`,
      predecessor: taskId(p),
      kind: "finishToStart",
      lagMinutes: 0,
      lagPercent: null,
    })),
    explanation: failed
      ? null
      : {
          driver: { type: "projectStart" },
          end: "start",
          drivenDate: start,
          movedByCalendar: false,
          calendarId: "calendar-1",
          viaSummary: null,
        },
  }));
  return {
    projectId: "project-1",
    projectName: "Garden shed",
    minutesPerDay: 480,
    scheduleFrom: "start",
    finishDate: null,
    calendarId: "calendar-1",
    calendars: [{ id: "calendar-1", name: "Standard" }],
    projectStart: failed ? null : "2027-03-01T08:00:00",
    projectFinish: failed ? null : "2027-03-23T17:00:00",
    error: failed
      ? { code: "cycleDetected", details: { tasks: [taskId(1), taskId(2)], names: ["A", "B"] } }
      : null,
    tasks,
  };
}
