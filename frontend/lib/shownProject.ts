/**
 * lib/shownProject.ts: which project the top of the window describes.
 *
 * What it is for:
 *   The window's top row shows the project of the clicked row; before any
 *   click, the first project in the list.
 *
 * Where it fits:
 *   Used by `ProjectInfo.tsx`. A pure function, easy to test.
 */

import type { TaskRow, Uuid } from "@frontend/ipc/types";

/**
 * Finds the project to show.
 *
 * @param tasks - All rows, in list order.
 * @param selectedId - The clicked row, or `null`.
 * @returns The project that holds the row (or is the row). Without a
 *   clicked row: the first project. `undefined` if there are none.
 *
 * @example
 * shownProject(snapshot.tasks, null)?.name; // "House"
 */
export function shownProject(
  tasks: readonly TaskRow[],
  selectedId: Uuid | null,
): TaskRow | undefined {
  // A project comes before its tasks, so the last project seen before the
  // clicked row is the one that holds it.
  let project: TaskRow | undefined;
  for (const task of tasks) {
    if (task.isProject === true) project = task;
    if (task.id === selectedId && project !== undefined) return project;
  }
  return tasks.find((task) => task.isProject === true);
}
