/**
 * features/grid/gridRows.ts: the list of rows the grid shows.
 *
 * What it is for:
 *   The grid can show two kinds of data: the real project from Rust (one
 *   `TaskRow` per task, editable) or the 10,000-task mock schedule for the
 *   performance test (`GanttSourceTask`, read-only). This file turns either
 *   into one simple list of {@link GridRow}s, leaving out the subtasks of
 *   collapsed summaries, so the grid component only has one shape to draw.
 *
 * Where it fits:
 *   Called by `TaskGrid.tsx` inside `useMemo` (once per new snapshot,
 *   collapse or mock switch, not on every scroll).
 *
 * Main ideas:
 *   - The visible rows are exactly the rows the Gantt chart draws
 *     (`state/store.ts` filters with the same `filterCollapsed` rule), so
 *     row N of the grid sits next to row N of the chart.
 *   - `index` is the row's place in the *full* list (also when rows above
 *     it are hidden): the ID column shows `index + 1`, as MS Project shows
 *     the task ID.
 */

import type { GanttSourceTask } from "@frontend/features/gantt/renderer/model";
import type { ScheduleSnapshot, TaskRow } from "@frontend/ipc/types";
import { visibleRowIndices } from "./outline";

/** One visible row of the grid. */
export interface GridRow {
  /** Task ID (UUID). */
  id: string;
  /** Place in the full task list (0 = first). The ID column shows `index + 1`. */
  index: number;
  /** The real task, or `null` for a mock row (read-only). */
  task: TaskRow | null;
  /** Task name. */
  name: string;
  /** Indent level, 1 = top level. */
  level: number;
  /** Outline number, e.g. "1.2". */
  outlineNumber: string;
  /** Which project it belongs to (0 = first), for the background; `null` without projects. */
  projectIndex: number | null;
  /** Has subtasks: drawn bold, with a collapse button. */
  isSummary: boolean;
  /** Its subtasks are hidden. */
  collapsed: boolean;
  /** Scheduled start, or `null`. */
  start: string | null;
  /** Scheduled finish, or `null`. */
  finish: string | null;
}

/**
 * Builds the visible rows from the real project.
 *
 * @param snapshot - The latest snapshot from Rust.
 * @param collapsed - IDs of collapsed summary tasks.
 * @returns The visible rows, in order.
 *
 * @example
 * const rows = rowsFromSnapshot(snapshot, new Set());
 * rows[1].outlineNumber; // "2"
 */
export function rowsFromSnapshot(
  snapshot: ScheduleSnapshot,
  collapsed: ReadonlySet<string>,
): GridRow[] {
  // Project rows are top level, so they are never hidden: counting the
  // visible ones is enough.
  let projectIndex: number | null = null;
  return visibleRowIndices(snapshot.tasks, collapsed).flatMap((index) => {
    const task = snapshot.tasks[index];
    if (task === undefined) return [];
    if (task.isProject === true) projectIndex = (projectIndex ?? -1) + 1;
    return [
      {
        id: task.id,
        index,
        task,
        name: task.name,
        level: task.outlineLevel,
        projectIndex,
        outlineNumber: task.outlineNumber,
        isSummary: task.isSummary,
        collapsed: task.isSummary && collapsed.has(task.id),
        start: task.start,
        finish: task.finish,
      },
    ];
  });
}

/**
 * Builds read-only rows from the mock schedule.
 *
 * @param tasks - The mock tasks (no outline: all at level 1).
 * @returns One row per task; the outline number is the row number.
 *
 * @example
 * rowsFromMock(mockTasks)[9999].outlineNumber; // "10000"
 */
export function rowsFromMock(tasks: readonly GanttSourceTask[]): GridRow[] {
  return tasks.map((task, index) => ({
    id: task.id,
    index,
    task: null,
    name: task.name,
    level: 1,
    projectIndex: null,
    outlineNumber: String(index + 1),
    isSummary: task.isSummary === true,
    collapsed: false,
    // The mock writes "" for "no date"; the grid wants `null`.
    start: task.start === "" ? null : task.start,
    finish: task.finish === "" ? null : task.finish,
  }));
}
