/**
 * fromSnapshot.ts: turns the schedule from Rust into tasks the Gantt can draw.
 *
 * What it is for:
 *   Rust sends a `ScheduleSnapshot` (see `ipc/types.ts`): one `TaskRow` per
 *   task with early and late dates, slack and more. The Gantt renderer only
 *   needs a few of those fields, under the names in `GanttSourceTask`
 *   (`renderer/model.ts`). This file picks and renames them.
 *
 * Where it fits:
 *   The store (`state/store.ts`) calls `snapshotToGanttTasks` every time a
 *   new snapshot arrives from Rust. The mock data for the 10,000-task spike
 *   (`mockSchedule.ts`) already has the `GanttSourceTask` shape, so the
 *   renderer cannot tell the two apart.
 *
 * Main ideas:
 *   - **No scheduling here** (decision D16). Bars are drawn from the
 *     engine's *scheduled* dates (`start` → `finish`), exactly as the
 *     engine calculated them; nothing is computed in the UI.
 *   - If the project could not be scheduled, the dates are `null`. We pass
 *     an empty string instead; the renderer reads it as "no date" (`NaN`)
 *     and skips the bar but keeps the row, so rows stay aligned.
 *   - **Blockers**: a task with blockers also gets its bar
 *     pieces (`segments`), each blocked piece with its blocker's reason,
 *     length and reported day for the tooltip. Tasks without blockers get
 *     no `segments` field at all.
 *   - Pure function: same snapshot in, same tasks out. Easy to test.
 */

import { highestPriority } from "@frontend/features/task-info/risks";
import type { ScheduleSnapshot, TaskRow } from "@frontend/ipc/types";
import type { GanttBlockerInfo, GanttSourceSegment, GanttSourceTask } from "./renderer/model";

/** Text used for a missing date. `parseIsoLocal("")` gives `NaN` (no bar). */
const NO_DATE = "";

/**
 * Converts a schedule snapshot into Gantt tasks, in the same row order.
 *
 * @param snapshot - The snapshot received from Rust.
 * @returns One `GanttSourceTask` per task row. `start`/`finish` are the
 *   scheduled start/finish, or `""` when the project has no valid schedule.
 *   Summary and milestone flags are passed on for their shapes.
 *
 * @example
 * const tasks = snapshotToGanttTasks(response.snapshot);
 * tasks[1].finish; // "2027-03-09T17:00:00" for "Get building permit" (5d)
 *
 * Beginner note: `??` is the "nullish coalescing" operator. `a ?? b` gives
 * `a`, unless `a` is `null` or `undefined`; then it gives `b`.
 */
export function snapshotToGanttTasks(snapshot: ScheduleSnapshot): GanttSourceTask[] {
  return snapshot.tasks.map((row) => ({
    id: row.id,
    name: row.name,
    // `start` / `finish` are the *scheduled* dates (for an ALAP task or a
    // manually scheduled one they differ from the early dates). The early
    // dates are only a fallback for snapshots made before M1, which had no
    // `start` field (the `??` then skips the missing value).
    start: row.start ?? row.earlyStart ?? NO_DATE,
    finish: row.finish ?? row.earlyFinish ?? NO_DATE,
    isCritical: row.isCritical,
    predecessors: row.predecessors,
    isSummary: row.isSummary,
    outlineLevel: row.outlineLevel,
    isMilestone: row.isMilestone,
  }));
}

/**
 * Like {@link snapshotToGanttTasks}, plus the details the chart draws since
 * M1 wave 2: link types (all four are drawn), manual mode (own colour),
 * deadline (marker) and constraint (pin), and the bar pieces of tasks with
 * blockers. The store uses this one.
 *
 * @param snapshot - The snapshot received from Rust.
 * @returns One `GanttSourceTask` per task row, in row order.
 *
 * @example
 * const tasks = snapshotToDetailedGanttTasks(response.snapshot);
 * tasks[4].links; // [{ predecessor: "…", kind: "finishToStart" }, …]
 *
 * Beginner note: `{ ...task, more }` (spread) copies every field of
 * `task` into a new object and adds the fields after it.
 */
export function snapshotToDetailedGanttTasks(snapshot: ScheduleSnapshot): GanttSourceTask[] {
  const basic = snapshotToGanttTasks(snapshot);
  return snapshot.tasks.map((row, i) => ({
    // `basic[i]` always exists (same list, same order); `as` tells TypeScript.
    ...(basic[i] as GanttSourceTask),
    // `?? []` keeps snapshots made before M1 (no `links`) working.
    links: (row.links ?? []).map((link) => ({ predecessor: link.predecessor, kind: link.kind })),
    isManual: row.mode === "manual",
    deadline: row.deadline,
    missedDeadline: row.missedDeadline,
    plannedFinish: row.plannedFinish ?? null,
    actualFinish: row.actualFinish ?? null,
    constraint: row.constraint,
    calendarId: row.calendarId,
    percentComplete: row.percentComplete ?? 0,
    isProject: row.isProject === true,
    riskPriority: highestPriority(row.risks ?? []),
    // Conditional spread: the field exists only for tasks with blockers.
    ...(row.segments !== undefined && row.segments.length > 0 ? { segments: segmentsOf(row) } : {}),
  }));
}

/**
 * The bar pieces of one task with blockers, each blocked piece with the
 * details of its blocker.
 *
 * @param row - A task row whose `segments` list is not empty.
 * @returns The pieces in time order. A blocked piece whose blocker is not
 *   in `row.blockers` (should not happen) gets an empty reason.
 *
 * @example
 * segmentsOf(row)[1]; // { start: "2027-03-05T08:00:00", finish: "…", blocker: { reason: "Wood late", … } }
 */
function segmentsOf(row: TaskRow): GanttSourceSegment[] {
  const byId = new Map((row.blockers ?? []).map((b) => [b.id, b]));
  return (row.segments ?? []).map((seg) => {
    let blocker: GanttBlockerInfo | null = null;
    if (seg.kind.type === "blocked") {
      const found = byId.get(seg.kind.blocker);
      blocker = {
        id: seg.kind.blocker,
        reason: found?.reason ?? "",
        durationMinutes: found?.durationMinutes ?? 0,
        reportedOn: found?.reportedOn ?? null,
      };
    }
    return { start: seg.start, finish: seg.finish, blocker };
  });
}
