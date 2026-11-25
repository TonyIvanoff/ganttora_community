/**
 * features/task-info/risks.ts: the risk form and how it becomes document
 * commands.
 *
 * What it is for:
 *   A *risk* is something that could go wrong. The user says what, how
 *   important it is, whether it is approved (known, nothing to do) or
 *   mitigated (with a note how), and its consequence (or ticks that there
 *   is none). Used by `RiskDialog.tsx` (add one) and by the Risks tab of
 *   the Task Information dialog (add, edit, remove).
 *
 * Where it fits:
 *   Pure functions; the commands go to Rust through the store (D16). Rust
 *   checks the same rules again (`riskIncomplete`).
 *
 * Main ideas:
 *   - The form has a field for everything; {@link parseRiskForm} empties the
 *     fields that do not apply (a note on an approved risk, a consequence
 *     when "no consequence" is ticked).
 *   - Untouched risks are not checked, so a risk from an older file that is
 *     still incomplete does not block OK.
 */

import { parseIsoLocal } from "@frontend/features/gantt/renderer/dates";
import { isLateToStart } from "@frontend/features/gantt/renderer/hitTest";
import { subtreeEnd } from "@frontend/features/grid/outline";
import type { Command, Risk, RiskPriority, TaskRow, Uuid } from "@frontend/ipc/types";

/** The fields of the risk form: a risk without its ID. */
export type RiskFormValues = Omit<Risk, "id">;

/** A field of the risk form that can have a problem. */
export type RiskField = "description" | "mitigation" | "consequence";

/** What {@link parseRiskForm} answers. */
export type RiskParse = { ok: true; value: RiskFormValues } | { ok: false; field: RiskField };

/** One risk in the Task Information dialog's draft. */
export interface DraftRisk {
  /** A key for React's list: the risk's ID, or a new random one. */
  key: string;
  /** The risk's ID, or `null` for a risk added in this dialog. */
  id: Uuid | null;
  /** The form fields. */
  values: RiskFormValues;
}

/** What {@link riskCommands} answers. `number` counts from 1. */
export type RiskCommandsResult =
  { ok: true; commands: Command[] } | { ok: false; number: number; field: RiskField };

/** The priorities, lowest first. */
export const RISK_PRIORITIES: readonly RiskPriority[] = ["low", "medium", "high"];

/**
 * The highest priority among some risks.
 *
 * @param risks - The risks of one row.
 * @returns The priority, or `null` when there are no risks.
 *
 * @example
 * highestPriority([lowRisk, highRisk]); // "high"
 */
export function highestPriority(risks: readonly Risk[]): RiskPriority | null {
  let highest = -1;
  for (const risk of risks) highest = Math.max(highest, RISK_PRIORITIES.indexOf(risk.priority));
  return RISK_PRIORITIES[highest] ?? null;
}

/** The empty form: medium priority, to be mitigated. */
export const EMPTY_RISK: RiskFormValues = {
  description: "",
  priority: "medium",
  response: "mitigate",
  mitigation: "",
  noConsequence: false,
  consequence: "",
};

/**
 * Checks a risk form and cleans its values.
 *
 * @param values - The form fields.
 * @returns The values without spaces at the ends and without fields that do
 *   not apply, or the first field that still has to be written.
 *
 * @example
 * parseRiskForm({ ...EMPTY_RISK, description: "Rain" });
 * // { ok: false, field: "mitigation" }
 */
export function parseRiskForm(values: RiskFormValues): RiskParse {
  const value: RiskFormValues = {
    ...values,
    description: values.description.trim(),
    mitigation: values.response === "approved" ? "" : values.mitigation.trim(),
    consequence: values.noConsequence ? "" : values.consequence.trim(),
  };
  if (value.description === "") return { ok: false, field: "description" };
  if (value.response === "mitigate" && value.mitigation === "")
    return { ok: false, field: "mitigation" };
  if (!value.noConsequence && value.consequence === "") return { ok: false, field: "consequence" };
  return { ok: true, value };
}

/**
 * Makes the Risks tab's draft from a task's risks.
 *
 * @param task - The task.
 * @returns One entry per risk, in the task's list order.
 */
export function draftRisksFromTask(task: TaskRow): DraftRisk[] {
  return (task.risks ?? []).map(({ id, ...values }) => ({ key: id, id, values }));
}

/**
 * Turns the Risks tab's draft into commands for what changed.
 *
 * @param task - The task as it is now.
 * @param drafts - The draft entries.
 * @param newId - Makes a UUID for each new risk.
 * @returns The commands (maybe none), or the first problem found.
 *
 * @example
 * riskCommands(task, [], newId); // one removeRisk per risk of the task
 */
export function riskCommands(
  task: TaskRow,
  drafts: readonly DraftRisk[],
  newId: () => string,
): RiskCommandsResult {
  const old = new Map((task.risks ?? []).map((risk) => [risk.id, risk]));
  const kept = new Set(drafts.map((draft) => draft.id));
  const commands: Command[] = [...old.keys()]
    .filter((id) => !kept.has(id))
    .map((id) => ({ type: "removeRisk", task: task.id, id }));

  for (const [index, draft] of drafts.entries()) {
    const before = draft.id === null ? undefined : old.get(draft.id);
    if (before !== undefined && sameRisk(before, draft.values)) continue;
    const parsed = parseRiskForm(draft.values);
    if (!parsed.ok) return { ok: false, number: index + 1, field: parsed.field };
    const risk: Risk = { id: before?.id ?? newId(), ...parsed.value };
    commands.push({ type: before === undefined ? "addRisk" : "editRisk", task: task.id, risk });
  }
  return { ok: true, commands };
}

/** `true` when all fields of two risks are equal. */
function sameRisk(a: RiskFormValues, b: RiskFormValues): boolean {
  return (Object.keys(EMPTY_RISK) as (keyof RiskFormValues)[]).every((key) => a[key] === b[key]);
}

/** One task's risk in a project's list. */
export interface TaskRisk {
  /** The name of the task that has the risk. */
  taskName: string;
  /** The risk. */
  risk: Risk;
}

/**
 * Lists the risks of all tasks in a project. The project's own risks are
 * not part of it.
 *
 * @param rows - All rows, in list order.
 * @param projectId - The project row's ID.
 * @returns The risks in row order (empty for an unknown project).
 *
 * @example
 * taskRisks(snapshot.tasks, garage.id)[0]?.taskName; // "Get the permit"
 */
export function taskRisks(rows: readonly TaskRow[], projectId: Uuid): TaskRisk[] {
  const first = rows.findIndex((row) => row.id === projectId);
  if (first < 0) return [];
  return rows
    .slice(first + 1, subtreeEnd(rows, first))
    .flatMap((row) => (row.risks ?? []).map((risk) => ({ taskName: row.name, risk })));
}

/**
 * The priority of the automatic risk "start date is missed".
 *
 * @param isCritical - A delay of the task delays its project.
 * @returns High on a critical task, else medium.
 */
export function lateStartRisk(isCritical: boolean): RiskPriority {
  return isCritical ? "high" : "medium";
}

/**
 * The higher of two priorities.
 *
 * @param a - One, or `null` for no risk.
 * @param b - The other.
 *
 * @example
 * higherPriority("low", "high"); // "high"
 */
export function higherPriority(
  a: RiskPriority | null,
  b: RiskPriority | null,
): RiskPriority | null {
  if (a === null || b === null) return a ?? b;
  return RISK_PRIORITIES.indexOf(a) > RISK_PRIORITIES.indexOf(b) ? a : b;
}

/**
 * The automatic risk of a task whose start was missed. It is worked out,
 * never saved: it disappears as soon as the task has progress.
 *
 * @param task - The row.
 * @param todayMs - Today at 00:00 (see `localToday`).
 * @returns Its priority, or `null` when the start was not missed.
 */
export function missedStartPriority(task: TaskRow, todayMs: number): RiskPriority | null {
  const late = isLateToStart(
    {
      startMs: parseIsoLocal(task.start ?? ""),
      percentComplete: task.percentComplete ?? 0,
      isSummary: task.isSummary,
      isProject: task.isProject === true,
      isMilestone: task.isMilestone,
    },
    todayMs,
  );
  return late ? lateStartRisk(task.isCritical) : null;
}

/** A task with the automatic risk. */
export interface MissedStart {
  /** The task. */
  task: TaskRow;
  /** Its priority. */
  priority: RiskPriority;
}

/**
 * Lists the tasks of a project whose start was missed.
 *
 * @param rows - All rows, in list order.
 * @param projectId - The project row's ID.
 * @param todayMs - Today at 00:00.
 * @returns The tasks in row order.
 */
export function missedStarts(
  rows: readonly TaskRow[],
  projectId: Uuid,
  todayMs: number,
): MissedStart[] {
  const first = rows.findIndex((row) => row.id === projectId);
  if (first < 0) return [];
  return rows.slice(first + 1, subtreeEnd(rows, first)).flatMap((task) => {
    const priority = missedStartPriority(task, todayMs);
    return priority === null ? [] : [{ task, priority }];
  });
}
