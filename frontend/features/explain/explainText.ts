/**
 * features/explain/explainText.ts: turns the engine's "Why this date?" answer
 * into sentences the panel can translate.
 *
 * What it is for:
 *   For every task, Rust sends an `Explanation` (see `ipc/types.ts`): the
 *   *driver* (the reason that won: a predecessor, a constraint, the project
 *   start…), which end of the task it fixed, the date it asked for and
 *   whether the calendar then moved that date. This file turns that data
 *   into a list of *lines*: a translation key from `locales/en.json`
 *   (`explain.*`) plus the values for its placeholders, and, where useful,
 *   the ID of a task the user can jump to.
 *
 * Where it fits:
 *   `ExplainPanel.tsx` calls {@link explainLines} for the selected task and
 *   shows each line with `t(line.key, line.params)`.
 *
 * Main ideas:
 *   - **Pure function, no UI text.** It returns keys, never English words,
 *     so every language works and the function is easy to test.
 *   - **No scheduling here** (decision D16). The engine already decided
 *     the driver; we only describe it.
 *   - Dates are passed in as formatted text by a `formatDate` function, so
 *     this file does not care about the user's date format.
 *   - A last line says how much **blocked time** made the
 *     task longer, with the blockers' reasons.
 */

import type { Constraint, ScheduleSnapshot, TaskRow, Uuid } from "@frontend/ipc/types";

/**
 * Every translation key a line can have. Listing them (instead of `string`)
 * lets TypeScript check that each one exists in `locales/en.json`.
 */
export type ExplainKey =
  | `explain.driver.${
      | "projectStart"
      | "projectFinish"
      | "predecessor"
      | "successor"
      | "constraint"
      | "deadline"
      | "manualStart"
      | "subtasks"}`
  | "explain.end"
  | "explain.movedByCalendar"
  | "explain.notMovedByCalendar"
  | "explain.viaSummary"
  | "explain.blocked";

/** One line of the explanation. */
export interface ExplainLine {
  /** Translation key, e.g. "explain.driver.predecessor". */
  key: ExplainKey;
  /** Values for the placeholders in the translated text. */
  params: Record<string, string | number>;
  /** A task the user can jump to from this line, or `null`. */
  jumpTo: Uuid | null;
}

/**
 * Formats the lag of a link for the text: "+2 days", "-1 day", "+50%" or "".
 *
 * @param lagMinutes - Lag in working minutes (negative = lead).
 * @param lagPercent - Lag in percent, or `null` for a time lag.
 * @param minutesPerDay - Working minutes in a day (480 = 8 hours).
 * @param formatNumber - Formats a number for the user's region.
 * @returns `{ kind, value }`: `kind` is "none", "days" or "percent" (for an
 *   ICU `select` in the text) and `value` the number as text, with sign.
 *
 * @example
 * lagParts(960, null, 480, String); // { kind: "days", value: "+2", amount: 2 }
 */
export function lagParts(
  lagMinutes: number,
  lagPercent: number | null,
  minutesPerDay: number,
  formatNumber: (n: number) => string,
): { kind: "none" | "days" | "percent"; value: string; amount: number } {
  if (lagPercent !== null && lagPercent !== 0) {
    return { kind: "percent", value: signed(lagPercent, formatNumber), amount: lagPercent };
  }
  if (lagMinutes === 0) return { kind: "none", value: "", amount: 0 };
  const days = lagMinutes / minutesPerDay;
  return { kind: "days", value: signed(days, formatNumber), amount: Math.abs(days) };
}

/** Writes a number with an explicit "+" for positive values. */
function signed(n: number, formatNumber: (n: number) => string): string {
  return n > 0 ? `+${formatNumber(n)}` : formatNumber(n);
}

/**
 * The date of a constraint, or `null` for ASAP / ALAP.
 *
 * @param constraint - The constraint.
 * @returns Its ISO date-time, or `null`.
 */
export function constraintDate(constraint: Constraint): string | null {
  return "date" in constraint ? constraint.date : null;
}

/**
 * Builds the explanation lines for one task.
 *
 * @param task - The task's row from the snapshot.
 * @param snapshot - The whole snapshot (to find other tasks' names, the
 *   project start/finish and the calendar names).
 * @param formatDate - Formats an ISO date-time for the user (e.g.
 *   `formatScheduleDate`); `null` input never happens here.
 * @param formatNumber - Formats a number for the user's region.
 * @returns The lines, in reading order. Empty when the task has no
 *   explanation (the project could not be scheduled).
 *
 * @example
 * explainLines(row, snapshot, (iso) => iso.slice(0, 10), String)[0];
 * // { key: "explain.driver.predecessor", params: { name: "Get building permit", … }, jumpTo: "…" }
 */
export function explainLines(
  task: TaskRow,
  snapshot: ScheduleSnapshot,
  formatDate: (iso: string) => string,
  formatNumber: (n: number) => string,
): ExplainLine[] {
  const explanation = task.explanation;
  if (explanation === null) return [];
  const byId = new Map(snapshot.tasks.map((t) => [t.id, t]));
  const nameOf = (id: Uuid) => byId.get(id)?.name ?? "";
  const rowOf = (id: Uuid) => byId.get(id)?.outlineNumber ?? "";
  const lines: ExplainLine[] = [];
  const driver = explanation.driver;

  switch (driver.type) {
    case "projectStart":
      lines.push({
        key: "explain.driver.projectStart",
        params: { date: formatDate(snapshot.projectStart ?? explanation.drivenDate) },
        jumpTo: null,
      });
      break;
    case "projectFinish":
      lines.push({
        key: "explain.driver.projectFinish",
        params: { date: formatDate(snapshot.finishDate ?? explanation.drivenDate) },
        jumpTo: null,
      });
      break;
    case "predecessor":
    case "successor": {
      // The lag is on the link. For a predecessor the link is one of this
      // task's incoming links; for a successor it is on the other task.
      const owner = driver.type === "predecessor" ? task : byId.get(driver.task);
      const link = owner?.links.find((l) => l.id === driver.dependency);
      const lag = lagParts(
        link?.lagMinutes ?? 0,
        link?.lagPercent ?? null,
        snapshot.minutesPerDay,
        formatNumber,
      );
      lines.push({
        key: `explain.driver.${driver.type}`,
        params: {
          name: nameOf(driver.task),
          row: rowOf(driver.task),
          kind: driver.kind,
          lagKind: lag.kind,
          lag: lag.value,
          amount: lag.amount,
        },
        jumpTo: driver.task,
      });
      break;
    }
    case "constraint": {
      const date = constraintDate(driver.constraint);
      lines.push({
        key: "explain.driver.constraint",
        params: {
          constraint: driver.constraint.type,
          date: date === null ? "" : formatDate(date),
        },
        jumpTo: null,
      });
      break;
    }
    case "deadline":
      lines.push({
        key: "explain.driver.deadline",
        params: { date: formatDate(driver.date) },
        jumpTo: null,
      });
      break;
    case "manualStart":
      lines.push({ key: "explain.driver.manualStart", params: {}, jumpTo: null });
      break;
    case "subtasks":
      lines.push({ key: "explain.driver.subtasks", params: {}, jumpTo: null });
      break;
  }

  // Which end the reason fixed, and the date it asked for.
  lines.push({
    key: "explain.end",
    params: { end: explanation.end, date: formatDate(explanation.drivenDate) },
    jumpTo: null,
  });

  // Did the calendar move it? Then say which calendar and to when.
  if (explanation.movedByCalendar) {
    const calendar = snapshot.calendars.find((c) => c.id === explanation.calendarId);
    const actual = explanation.end === "start" ? task.start : task.finish;
    lines.push({
      key: "explain.movedByCalendar",
      params: {
        calendar: calendar?.name ?? "",
        end: explanation.end,
        date: actual === null ? "" : formatDate(actual),
      },
      jumpTo: null,
    });
  } else {
    lines.push({ key: "explain.notMovedByCalendar", params: {}, jumpTo: null });
  }

  // The reason came through a summary task (e.g. a link to the summary).
  if (explanation.viaSummary !== null) {
    lines.push({
      key: "explain.viaSummary",
      params: { name: nameOf(explanation.viaSummary), row: rowOf(explanation.viaSummary) },
      jumpTo: explanation.viaSummary,
    });
  }

  // Blockers made the task longer than its work alone.
  const blockedMinutes = explanation.blocked?.minutes ?? 0;
  if (blockedMinutes > 0) {
    const days = blockedMinutes / snapshot.minutesPerDay;
    lines.push({
      key: "explain.blocked",
      params: {
        days: formatNumber(days),
        count: days,
        reasons: (task.blockers ?? []).map((b) => b.reason).join("; "),
      },
      jumpTo: null,
    });
  }
  return lines;
}
