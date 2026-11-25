/**
 * tests/frontend/explain/explainText.test.ts: tests for the "Why this
 * date?" panel's sentences.
 *
 * What it checks:
 *   `explainLines` in `frontend/features/explain/explainText.ts` turns the
 *   engine's explanation of a task into a list of lines, each with a
 *   translation key, the values to fill in, and the task to jump to:
 *   - one "driver" line for each kind of driver (project start, project
 *     finish, predecessor, successor, constraint, deadline, manual start,
 *     subtasks), with lags as days or as a percentage;
 *   - the "end" line (which end was fixed, and when);
 *   - "moved by calendar" (with the calendar name and the actual date) or
 *     "not moved by calendar";
 *   - the "via summary" line when the reason came through a summary task;
 *   - the "blocked" line when blockers made the task longer.
 *
 * Why it matters:
 *   This panel is how a beginner learns why a task is where it is. A wrong
 *   name, row or lag here would teach the user something false about their
 *   plan.
 *
 * Main ideas:
 *   The tests use the shed snapshot and replace the explanation of
 *   "Get building permit" (task 2). Dates are formatted as "D:<iso>" and
 *   numbers as plain text, so the expected values are easy to read.
 */

import { describe, expect, it } from "vitest";
import { explainLines, lagParts } from "@frontend/features/explain/explainText";
import type {
  BlockerRow,
  Driver,
  Explanation,
  ScheduleSnapshot,
  TaskRow,
} from "@frontend/ipc/types";
import { shedSnapshot, taskId } from "../gantt/helpers";

/** Formats a date for the test: "D:" + the ISO text. */
const date = (iso: string) => `D:${iso}`;
/** Formats a number for the test. */
const num = (n: number) => String(n);

/**
 * The shed snapshot where task 2 has the given explanation (and,
 * optionally, other changes to its row).
 */
function withExplanation(
  explanation: Partial<Explanation> & { driver: Driver },
  rowChanges: Partial<TaskRow> = {},
): { snapshot: ScheduleSnapshot; task: TaskRow } {
  const snapshot = shedSnapshot();
  const tasks = snapshot.tasks.map((t, i) =>
    i === 1
      ? {
          ...t,
          ...rowChanges,
          explanation: {
            end: "start" as const,
            drivenDate: "2027-03-03T08:00:00",
            movedByCalendar: false,
            calendarId: "calendar-1",
            viaSummary: null,
            ...explanation,
          },
        }
      : t,
  );
  const result = { ...snapshot, tasks };
  return { snapshot: result, task: tasks[1] as TaskRow };
}

/** The lines for task 2. */
function lines(explanation: Partial<Explanation> & { driver: Driver }, rowChanges = {}) {
  const { snapshot, task } = withExplanation(explanation, rowChanges);
  return explainLines(task, snapshot, date, num);
}

describe("explainLines: drivers", () => {
  // Project start: the project's start date, nothing to jump to.
  it("explains the project start", () => {
    const result = lines({ driver: { type: "projectStart" } });
    expect(result[0]).toEqual({
      key: "explain.driver.projectStart",
      params: { date: "D:2027-03-01T08:00:00" },
      jumpTo: null,
    });
  });

  // Project finish: the fixed finish date (or the driven date if missing).
  it("explains the project finish", () => {
    const { snapshot, task } = withExplanation({ driver: { type: "projectFinish" } });
    const fixed = { ...snapshot, finishDate: "2027-04-30T17:00:00" };
    expect(explainLines(task, fixed, date, num)[0]?.params).toEqual({
      date: "D:2027-04-30T17:00:00",
    });
    expect(explainLines(task, snapshot, date, num)[0]?.params).toEqual({
      date: "D:2027-03-03T08:00:00",
    });
  });

  // Predecessor with a 2-day lag: name, row number, type, "+2" days, and a
  // jump to the predecessor.
  it("explains a predecessor with a lag in days", () => {
    const result = lines(
      {
        driver: { type: "predecessor", dependency: "l1", task: taskId(1), kind: "finishToStart" },
      },
      {
        links: [
          {
            id: "l1",
            predecessor: taskId(1),
            kind: "finishToStart",
            lagMinutes: 960,
            lagPercent: null,
          },
        ],
      },
    );
    expect(result[0]).toEqual({
      key: "explain.driver.predecessor",
      params: {
        name: "Design the shed",
        row: "1",
        kind: "finishToStart",
        lagKind: "days",
        lag: "+2",
        amount: 2,
      },
      jumpTo: taskId(1),
    });
  });

  // A percentage lead: lagKind "percent", value "-20".
  it("explains a predecessor with a percentage lag", () => {
    const result = lines(
      {
        driver: { type: "predecessor", dependency: "l1", task: taskId(1), kind: "startToStart" },
      },
      {
        links: [
          {
            id: "l1",
            predecessor: taskId(1),
            kind: "startToStart",
            lagMinutes: 0,
            lagPercent: -20,
          },
        ],
      },
    );
    expect(result[0]?.params).toMatchObject({ lagKind: "percent", lag: "-20", amount: -20 });
  });

  // No lag at all: lagKind "none".
  it("explains a predecessor without lag", () => {
    const result = lines({
      driver: {
        type: "predecessor",
        dependency: "link-1-2",
        task: taskId(1),
        kind: "finishToStart",
      },
    });
    expect(result[0]?.params).toMatchObject({ lagKind: "none", lag: "", amount: 0 });
  });

  // Successor: the lag is on the *successor's* link (task 4 "Prepare the
  // ground" waits for task 2 in the shed).
  it("explains a successor, reading the lag from the successor's link", () => {
    const snapshot = shedSnapshot();
    const tasks = snapshot.tasks.map((t, i) => {
      if (i === 3) {
        return {
          ...t,
          links: [
            {
              id: "l24",
              predecessor: taskId(2),
              kind: "finishToStart" as const,
              lagMinutes: -480,
              lagPercent: null,
            },
          ],
        };
      }
      if (i === 1) {
        return {
          ...t,
          explanation: {
            driver: {
              type: "successor" as const,
              dependency: "l24",
              task: taskId(4),
              kind: "finishToStart" as const,
            },
            end: "finish" as const,
            drivenDate: "2027-03-09T17:00:00",
            movedByCalendar: false,
            calendarId: "calendar-1",
            viaSummary: null,
          },
        };
      }
      return t;
    });
    const s = { ...snapshot, tasks };
    const result = explainLines(tasks[1] as TaskRow, s, date, num);
    expect(result[0]).toEqual({
      key: "explain.driver.successor",
      params: {
        name: "Prepare the ground",
        row: "4",
        kind: "finishToStart",
        lagKind: "days",
        lag: "-1",
        amount: 1,
      },
      jumpTo: taskId(4),
    });
  });

  // Constraint: its type and date.
  it("explains a constraint", () => {
    const result = lines({
      driver: {
        type: "constraint",
        constraint: { type: "mustStartOn", date: "2027-03-03T08:00:00" },
      },
    });
    expect(result[0]).toEqual({
      key: "explain.driver.constraint",
      params: { constraint: "mustStartOn", date: "D:2027-03-03T08:00:00" },
      jumpTo: null,
    });
    const alap = lines({
      driver: { type: "constraint", constraint: { type: "asLateAsPossible" } },
    });
    expect(alap[0]?.params).toEqual({ constraint: "asLateAsPossible", date: "" });
  });

  // Deadline, manual start and subtasks have simple lines.
  it("explains deadline, manual start and subtasks", () => {
    expect(lines({ driver: { type: "deadline", date: "2027-03-09T17:00:00" } })[0]).toEqual({
      key: "explain.driver.deadline",
      params: { date: "D:2027-03-09T17:00:00" },
      jumpTo: null,
    });
    expect(lines({ driver: { type: "manualStart" } })[0]?.key).toBe("explain.driver.manualStart");
    expect(lines({ driver: { type: "subtasks" } })[0]?.key).toBe("explain.driver.subtasks");
  });
});

describe("explainLines: end, calendar and summary lines", () => {
  // Without a calendar move or summary: driver, end, "not moved".
  it("adds the end line and the not-moved line", () => {
    const result = lines({ driver: { type: "projectStart" } });
    expect(result.map((l) => l.key)).toEqual([
      "explain.driver.projectStart",
      "explain.end",
      "explain.notMovedByCalendar",
    ]);
    expect(result[1]?.params).toEqual({ end: "start", date: "D:2027-03-03T08:00:00" });
  });

  // Moved by the calendar: the calendar's name and the task's real start.
  it("names the calendar that moved the date", () => {
    const result = lines({ driver: { type: "projectStart" }, movedByCalendar: true });
    expect(result[2]).toEqual({
      key: "explain.movedByCalendar",
      params: { calendar: "Standard", end: "start", date: "D:2027-03-03T08:00:00" },
      jumpTo: null,
    });
    // For the finish end, the task's finish is shown.
    const finish = lines({
      driver: { type: "projectStart" },
      movedByCalendar: true,
      end: "finish",
    });
    expect(finish[2]?.params).toMatchObject({ end: "finish", date: "D:2027-03-09T17:00:00" });
  });

  // Through a summary task: a last line naming it, with a jump.
  it("adds the via-summary line", () => {
    const result = lines({ driver: { type: "projectStart" }, viaSummary: taskId(1) });
    expect(result.at(-1)).toEqual({
      key: "explain.viaSummary",
      params: { name: "Design the shed", row: "1" },
      jumpTo: taskId(1),
    });
  });

  // No explanation (failed schedule): no lines.
  it("gives no lines without an explanation", () => {
    const snapshot = shedSnapshot(true);
    expect(explainLines(snapshot.tasks[0] as TaskRow, snapshot, date, num)).toEqual([]);
  });
});

describe("explainLines: blocked time (D23)", () => {
  /** A blocker row with the given ID and reason (one day long). */
  function blocker(id: string, reason: string): BlockerRow {
    return {
      id,
      reason,
      afterWorkMinutes: 480,
      durationMinutes: 480,
      reportedOn: null,
      start: null,
      finish: null,
    };
  }

  // 720 minutes = 1.5 working days; the reasons are listed in order. The
  // line is last, after the other explanations, and has nothing to jump to.
  it("adds a last line with the days and all reasons", () => {
    const result = lines(
      {
        driver: { type: "projectStart" },
        viaSummary: taskId(1),
        blocked: { minutes: 720 },
      },
      { blockers: [blocker("b1", "Late delivery"), blocker("b2", "Rain")] },
    );
    expect(result.at(-2)?.key).toBe("explain.viaSummary");
    expect(result.at(-1)).toEqual({
      key: "explain.blocked",
      params: { days: "1.5", count: 1.5, reasons: "Late delivery; Rain" },
      jumpTo: null,
    });
  });

  // A "day" is the project's own working day: 720 minutes is exactly one
  // day in a project with 12-hour days.
  it("uses the project's minutes per day", () => {
    const { snapshot, task } = withExplanation(
      { driver: { type: "projectStart" }, blocked: { minutes: 720 } },
      { blockers: [blocker("b1", "Rain")] },
    );
    const result = explainLines(task, { ...snapshot, minutesPerDay: 720 }, date, num);
    expect(result.at(-1)?.params).toEqual({ days: "1", count: 1, reasons: "Rain" });
  });

  // No blocked time (missing or zero): no line. "Extended by 0 days" would
  // only confuse.
  it("adds no line without blocked time", () => {
    for (const blocked of [undefined, { minutes: 0 }]) {
      const result = lines(
        { driver: { type: "projectStart" }, ...(blocked ? { blocked } : {}) },
        { blockers: [blocker("b1", "Rain")] },
      );
      expect(result.map((line) => line.key)).not.toContain("explain.blocked");
    }
  });
});

describe("lagParts (explain)", () => {
  // Days with sign, the amount without sign (for plural rules).
  it("splits lags into kind, signed value and amount", () => {
    expect(lagParts(720, null, 480, num)).toEqual({ kind: "days", value: "+1.5", amount: 1.5 });
    expect(lagParts(0, 0, 480, num)).toEqual({ kind: "none", value: "", amount: 0 });
    expect(lagParts(0, 50, 480, num)).toEqual({ kind: "percent", value: "+50", amount: 50 });
  });
});
