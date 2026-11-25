/**
 * features/grid/outline.ts: pure helpers for the task outline (WBS) in the UI.
 *
 * What it is for:
 *   The task list is stored as rows in order, each with an *outline level*
 *   (1 = top level, 2 = subtask of the row above, …), exactly like MS
 *   Project. The UI needs a few questions answered about that list:
 *   which rows does a summary task contain? which rows are visible when
 *   some summaries are collapsed? where does a dragged row land?
 *
 * Where it fits:
 *   - `state/store.ts` uses {@link filterCollapsed} so the Gantt chart draws
 *     the same rows as the grid (rows must line up side by side).
 *   - `features/grid/TaskGrid.tsx` and `useTaskActions.ts` use the others.
 *
 * Main ideas:
 *   - **No scheduling here** (decision D16). These helpers only *look* at
 *     the row order and levels Rust sent; every change is still a command
 *     (`indentTask`, `moveTask`…) that Rust checks and applies.
 *   - A summary task's *subtree* (the summary and all rows below it) is
 *     the summary row plus every following row with a *deeper* level. The
 *     first row at the same or a smaller level ends it.
 *
 *   ```
 *   row  level  name
 *   0    1      Prepare          ← subtree of row 0 = rows 0..2
 *   1    2        Permit
 *   2    2        Buy materials
 *   3    1      Build            ← ends the subtree of row 0
 *   ```
 *   - Pure functions: same input, same output. Easy to test.
 */

import type { ScheduleSnapshot } from "@frontend/ipc/types";

/** The only things these helpers need to know about a row. */
export interface OutlineRow {
  /** Task ID. */
  id: string;
  /** Indent level, 1 = top level. */
  outlineLevel: number;
}

/**
 * Finds where a task's subtree ends.
 *
 * @param rows - All rows, in order.
 * @param index - The row of the task.
 * @returns The index *after* the last row of the subtree (like `slice`'s
 *   end). For a task without subtasks this is `index + 1`. For an index out
 *   of range it is `index`.
 *
 * @example
 * // levels [1, 2, 2, 1]
 * subtreeEnd(rows, 0); // 3
 * subtreeEnd(rows, 1); // 2
 */
export function subtreeEnd(rows: readonly OutlineRow[], index: number): number {
  const row = rows[index];
  if (row === undefined) return index;
  let end = index + 1;
  // Walk down while the rows are deeper than the task itself.
  while (end < rows.length && (rows[end]?.outlineLevel ?? 0) > row.outlineLevel) end++;
  return end;
}

/**
 * Lists the rows that are visible when some summary tasks are collapsed.
 *
 * @param rows - All rows, in order.
 * @param collapsed - IDs of the collapsed summary tasks. IDs that are not
 *   in the list, or not summaries, do nothing.
 * @returns The indexes (into `rows`) of the visible rows, in order. The
 *   collapsed summary itself stays visible; only the rows inside it hide.
 *
 * @example
 * // levels [1, 2, 2, 1], row 0 collapsed
 * visibleRowIndices(rows, new Set([rows[0].id])); // [0, 3]
 *
 * Beginner note: a `Set` is a collection without duplicates that answers
 * "is this value in it?" (`has`) very quickly, even with 10,000 entries.
 */
export function visibleRowIndices(
  rows: readonly OutlineRow[],
  collapsed: ReadonlySet<string>,
): number[] {
  const visible: number[] = [];
  let index = 0;
  while (index < rows.length) {
    visible.push(index);
    const row = rows[index];
    // A collapsed row: jump over its whole subtree (one pass in total).
    index = row !== undefined && collapsed.has(row.id) ? subtreeEnd(rows, index) : index + 1;
  }
  return visible;
}

/**
 * Gives a copy of the snapshot that contains only the visible rows.
 *
 * @param snapshot - The snapshot from Rust.
 * @param collapsed - IDs of collapsed summary tasks.
 * @returns The same snapshot object when nothing is collapsed (cheap),
 *   otherwise a copy whose `tasks` holds only the visible rows.
 *
 * @example
 * const shown = filterCollapsed(snapshot, new Set([prepareId]));
 */
export function filterCollapsed(
  snapshot: ScheduleSnapshot,
  collapsed: ReadonlySet<string>,
): ScheduleSnapshot {
  if (collapsed.size === 0) return snapshot;
  const tasks = visibleRowIndices(snapshot.tasks, collapsed).flatMap((i) => {
    const task = snapshot.tasks[i];
    return task === undefined ? [] : [task];
  });
  return { ...snapshot, tasks };
}

/**
 * Keeps only the "top" tasks of a selection: a task whose summary is also
 * selected is dropped, because commands on the summary (delete, indent,
 * move) already take its subtasks along.
 *
 * @param rows - All rows, in order.
 * @param selected - IDs of the selected tasks (any order).
 * @returns The IDs that remain, in row order.
 *
 * @example
 * // levels [1, 2, 2, 1]; rows 0 and 1 selected → only row 0
 * topLevelSelection(rows, new Set([rows[0].id, rows[1].id])); // [rows[0].id]
 */
export function topLevelSelection(
  rows: readonly OutlineRow[],
  selected: ReadonlySet<string>,
): string[] {
  const result: string[] = [];
  let index = 0;
  while (index < rows.length) {
    const row = rows[index];
    if (row !== undefined && selected.has(row.id)) {
      result.push(row.id);
      index = subtreeEnd(rows, index); // skip everything inside it
    } else {
      index++;
    }
  }
  return result;
}

/**
 * Computes the `toIndex` of a `moveTask` command for a drag and drop.
 *
 * @param rows - All rows, in order.
 * @param sourceIndex - The row being dragged.
 * @param dropBeforeIndex - The row it is dropped *above* (`rows.length` =
 *   below the last row).
 * @returns The `toIndex` for Rust (counted in the list *without* the moved
 *   block, see `MoveTask` in `backend/crates/document/src/command.rs`), or
 *   `null` when the drop would not move anything or would drop a summary
 *   inside itself.
 *
 * @example
 * // 5 single rows; drag row 0 above row 3 → it becomes row 2
 * moveTargetIndex(rows, 0, 3); // 2
 * moveTargetIndex(rows, 3, 0); // 0
 */
export function moveTargetIndex(
  rows: readonly OutlineRow[],
  sourceIndex: number,
  dropBeforeIndex: number,
): number | null {
  const end = subtreeEnd(rows, sourceIndex);
  if (end === sourceIndex) return null; // source does not exist
  // Dropping on the block itself or just below it: nothing moves.
  if (dropBeforeIndex >= sourceIndex && dropBeforeIndex <= end) return null;
  const blockSize = end - sourceIndex;
  // Rows below the block shift up by its size once it is taken out.
  return dropBeforeIndex > sourceIndex ? dropBeforeIndex - blockSize : dropBeforeIndex;
}

/**
 * Finds the row index of the summary task that contains a row.
 *
 * @param rows - All rows, in order.
 * @param index - The row.
 * @returns The index of the nearest row above with a smaller level, or
 *   `null` for a top-level row.
 *
 * @example
 * // levels [1, 2, 2, 1]
 * parentIndex(rows, 2); // 0
 */
export function parentIndex(rows: readonly OutlineRow[], index: number): number | null {
  const level = rows[index]?.outlineLevel;
  if (level === undefined) return null;
  for (let i = index - 1; i >= 0; i--) {
    if ((rows[i]?.outlineLevel ?? 0) < level) return i;
  }
  return null;
}

/**
 * Whether all selected tasks can become sub-tasks of the row above them.
 *
 * @param rows - All rows, in order.
 * @param selectedIds - IDs of the selected tasks.
 * @returns `false` for projects, and for the first task of its group.
 */
export function canBecomeSubtasks(
  rows: readonly OutlineRow[],
  selectedIds: readonly string[],
): boolean {
  const ids = new Set(topLevelSelection(rows, new Set(selectedIds)));
  return (
    ids.size > 0 &&
    rows.every((row, i) => {
      if (!ids.has(row.id)) return true;
      const above = rows[i - 1];
      return above !== undefined && row.outlineLevel > 1 && above.outlineLevel >= row.outlineLevel;
    })
  );
}

/**
 * Whether all selected rows are sub-tasks, so that they can move one level
 * up (a task cannot leave its project).
 *
 * @param rows - All rows, in order.
 * @param selectedIds - IDs of the selected tasks.
 */
export function canBecomeTasks(
  rows: readonly OutlineRow[],
  selectedIds: readonly string[],
): boolean {
  const ids = new Set(topLevelSelection(rows, new Set(selectedIds)));
  return ids.size > 0 && rows.every((row) => !ids.has(row.id) || row.outlineLevel > 2);
}

/** What {@link selectionIsLinked} needs to know about a row. */
export interface LinkedRow extends OutlineRow {
  /** The links into this row. */
  links?: readonly { id: string; predecessor: string }[];
}

/**
 * Whether the selected tasks are linked one after another, in row order
 * (in either direction). Then the Link button becomes Unlink.
 *
 * @param rows - All rows, in order.
 * @param selectedIds - IDs of the selected tasks.
 * @returns `true` if there are at least two and each pair of neighbours is
 *   linked.
 *
 * @example
 * selectionIsLinked(rows, [design.id, permit.id]); // true in the shed
 */
export function selectionIsLinked(
  rows: readonly LinkedRow[],
  selectedIds: readonly string[],
): boolean {
  if (selectedIds.length < 2) return false;
  const ids = topLevelSelection(rows, new Set(selectedIds));
  const chain = new Set(ids);
  const linked = new Set<string>();
  for (const row of rows) {
    if (!chain.has(row.id)) continue;
    for (const link of row.links ?? []) {
      linked.add(`${link.predecessor}>${row.id}`).add(`${row.id}>${link.predecessor}`);
    }
  }
  return ids.length > 1 && ids.every((id, i) => i === 0 || linked.has(`${ids[i - 1]}>${id}`));
}

/**
 * The links between the selected tasks.
 *
 * @param rows - All rows, in order.
 * @param selectedIds - IDs of the selected tasks.
 * @returns The IDs of the links whose two ends are both selected.
 */
export function linksInside(
  rows: readonly LinkedRow[],
  selectedIds: ReadonlySet<string>,
): string[] {
  return rows.flatMap((row) =>
    selectedIds.has(row.id)
      ? (row.links ?? []).filter((l) => selectedIds.has(l.predecessor)).map((l) => l.id)
      : [],
  );
}
