/**
 * features/task-info/taskInfoStore.ts: remembers which task the Task
 * Information dialog shows (or that it is closed), and on which tab it opens.
 *
 * What it is for:
 *   The dialog can be opened from several places: a double-click on a grid
 *   row, Shift+F2 in the grid, or the toolbar's "Task information" button.
 *   They all call {@link TaskInfoState.open}; the dialog
 *   (`TaskInfoDialog.tsx`) watches `taskId` and shows itself.
 *
 * Where it fits:
 *   A tiny Zustand store next to the dialog. `shell/AppShell.tsx` also reads
 *   it, so project shortcuts (Ctrl+Z…) do not act on the project behind an
 *   open dialog.
 */

import { create } from "zustand";
import type { GridColumnId } from "@frontend/features/grid/cellEdit";
import type { TaskRow } from "@frontend/ipc/types";
import { localToday } from "@frontend/features/gantt/renderer/dates";
import { projectBlockers } from "./blockers";
import { missedStartPriority, missedStarts, taskRisks } from "./risks";

/** The six tabs, in order. */
export const TABS = ["general", "predecessors", "dates", "notes", "blockers", "risks"] as const;
/** One tab's ID. */
export type TabId = (typeof TABS)[number];

/** The dialog's open state. */
export interface TaskInfoState {
  /** The task shown, or `null` while the dialog is closed. */
  taskId: string | null;
  /** The tab shown first. */
  tab: TabId;
  /**
   * Opens the dialog for a task.
   *
   * @param id - The task's ID.
   * @param tab - The tab to show first (default: General). A project has no
   *   Predecessors tab; it shows General instead.
   */
  open: (id: string, tab?: TabId) => void;
  /** Closes the dialog. */
  close: () => void;
}

/**
 * The Task Information dialog store hook.
 *
 * @example
 * const open = useTaskInfoStore((s) => s.open);
 * open(task.id);
 */
export const useTaskInfoStore = create<TaskInfoState>()((set) => ({
  taskId: null,
  tab: "general",
  open: (id, tab = "general") => set({ taskId: id, tab }),
  close: () => set({ taskId: null }),
}));

/** The columns whose field is on the Dates tab. */
const DATE_COLUMNS: ReadonlySet<GridColumnId | undefined> = new Set([
  "start",
  "finish",
  "plannedFinish",
  "actualFinish",
  "constraintType",
  "constraintDate",
  "deadline",
  "calendar",
] as const);

/**
 * The tab to open for a double-clicked cell.
 *
 * @param column - The column, or `undefined` beside the cells.
 * @param task - The row's task. Its indicators can mean several things; the
 *   first that applies wins: blocker, risk, missed deadline or constraint, note.
 * @param rows - All rows, to look into a project.
 * @param todayMs - Today at 00:00, for missed starts.
 * @returns The tab. A project has no Predecessors tab; the dialog then
 *   shows General.
 *
 * @example
 * infoTabOf("finish", task); // "dates"
 */
export function infoTabOf(
  column: GridColumnId | undefined,
  task: TaskRow,
  rows: readonly TaskRow[] = [],
  todayMs: number = localToday(),
): TabId {
  if (DATE_COLUMNS.has(column)) return "dates";
  if (column === "predecessors") return "predecessors";
  if (column !== "indicators") return "general";
  // A project also shows the blockers and risks of its tasks.
  const isProject = task.isProject === true;
  if ((task.blockers ?? []).length > 0) return "blockers";
  if (isProject && projectBlockers(rows, task.id).length > 0) return "blockers";
  if ((task.risks ?? []).length > 0 || missedStartPriority(task, todayMs) !== null) return "risks";
  if (isProject && taskRisks(rows, task.id).length > 0) return "risks";
  if (isProject && missedStarts(rows, task.id, todayMs).length > 0) return "risks";
  if (task.missedDeadline || task.constraint.type !== "asSoonAsPossible") return "dates";
  return task.notes.trim() === "" ? "general" : "notes";
}
