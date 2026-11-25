/**
 * features/grid/useTaskActions.ts: the outline actions (insert, add sub-task, delete,
 * indent, outdent, move, collapse, task information, add blocker, copy,
 * cut, paste)
 * shared by the grid's keyboard shortcuts and the toolbar buttons.
 *
 * What it is for:
 *   "Indent" can be pressed in the toolbar or typed as Alt+Shift+→ in the
 *   grid. Both must do exactly the same, so the work lives here once.
 *
 * Where it fits:
 *   Called by `TaskGrid.tsx` (keys) and `features/toolbars/useActions.ts` (menu, buttons).
 *   Every change is a document command sent through the store
 *   (`applyCommand` / `applyCommands`, decision D16). Rust checks it; when
 *   it refuses (e.g. `cannotIndent` on the first row) the error banner shows
 *   the translated reason (`lib/errors.ts`).
 *
 * Main ideas:
 *   - **Selection.** Actions work on the shared selection
 *     (`selectedTaskIds` in `state/store.ts`). For delete, indent, outdent and move, a task whose
 *     summary is also selected is skipped: the summary takes its subtasks
 *     along ({@link topLevelSelection}).
 *   - **Fresh values.** The actions read the stores with `getState()` at the
 *     moment they run, instead of values captured at render time, so a fast
 *     key press never acts on an old selection.
 *   - **Undo steps.** Several commands are sent as one `batch` (see
 *     `applyCommands` in `state/store.ts`): indenting three selected tasks
 *     is one Ctrl+Z, and if Rust refuses one of them nothing changes.
 *   - **Cut = copy + delete.** Ctrl/Cmd+X copies the selected tasks (with
 *     the subtasks of selected summaries, since deleting a summary deletes
 *     them too) and then deletes them in one batch, so one Ctrl+Z brings
 *     them all back. Copying does not change the project, so it is not an
 *     undo step of its own. If copying fails, nothing is deleted.
 */

import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { percentCommands } from "./cellEdit";
import { confirmAction } from "@frontend/features/files/dialogs";
import { localToday } from "@frontend/features/gantt/renderer/dates";
import { linkCommand } from "@frontend/features/gantt/interactions";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import { useBlockerDialogStore } from "@frontend/features/task-info/blockerStore";
import { useRiskDialogStore } from "@frontend/features/task-info/riskStore";
import { useTaskInfoStore } from "@frontend/features/task-info/taskInfoStore";
import { copyTasks } from "@frontend/ipc/client";
import type { ColumnLabels, Command, Task, TaskRow } from "@frontend/ipc/types";
import { useAppStore } from "@frontend/state/store";
import { expectedProgress } from "./gridFormat";
import { useGridStore } from "./gridStore";
import { linksInside, selectionIsLinked, subtreeEnd, topLevelSelection } from "./outline";

/** The actions returned by {@link useTaskActions}. */
export interface TaskActions {
  /** Adds a new project row at the end and opens its name. */
  addProject: () => Promise<void>;
  /** Inserts a new task below the active row (or at the end) and opens its name. */
  insertTask: () => Promise<void>;
  /**
   * Adds a new task as the last sub-task of the active task and opens its
   * name. The active task becomes a group (summary) if it was not one yet.
   * A task with blockers is refused (`subtaskOnBlocked`): as a group its
   * blockers would stop having any effect.
   */
  addSubtask: () => Promise<void>;
  /** Adds a milestone (a task without duration) below the active row, linked to it, and opens its name. */
  addMilestone: () => Promise<void>;
  /** Deletes the selected tasks; asks first if any of them has subtasks. */
  deleteSelected: () => Promise<void>;
  /** Indents the selected tasks one level. */
  indentSelected: () => Promise<void>;
  /** Outdents the selected tasks one level. */
  outdentSelected: () => Promise<void>;
  /** Moves the active task (with its subtasks) above the previous block or below the next one. */
  moveActive: (direction: "up" | "down") => Promise<void>;
  /** Hides (`true`) or shows (`false`) the subtasks of the selected summaries. */
  collapseSelected: (collapsed: boolean) => void;
  /**
   * Sets the selected tasks (and the sub-tasks of selected groups) to the
   * progress expected by today. Never lowers a task's progress.
   */
  markOnTrack: () => Promise<void>;
  /** Saves every row's current finish as its planned finish. */
  setPlan: () => Promise<void>;
  /** Opens the Task Information dialog for the active task. */
  openTaskInfo: () => void;
  /**
   * Opens the "Add blocker" dialog for the active task. On a
   * summary task it shows the translated `blockerOnSummary` error instead
   * (Rust would refuse it anyway; this saves filling in the form first).
   */
  addBlocker: () => void;
  /** Opens the "Add risk" dialog for the active row (any kind of row). */
  addRisk: () => void;
  /**
   * Links the selected tasks one after another, in row order
   * (finish-to-start). Pairs that are linked already are left alone. A
   * sub-task whose group is also selected is skipped: the group is linked.
   * If all of them are linked already, removes the links between them.
   */
  toggleLinks: () => Promise<void>;
  /** Copies the selected tasks to the system clipboard (Ctrl/Cmd+C). */
  copySelected: () => Promise<void>;
  /**
   * Cuts the selected tasks (Ctrl/Cmd+X): copies them, with the subtasks
   * of selected summaries, then deletes them as one undo step. Asks first
   * if a selected task has subtasks, like delete.
   */
  cutSelected: () => Promise<void>;
  /** Pastes the clipboard's rows as new tasks below the active row (Ctrl/Cmd+V). */
  paste: () => Promise<void>;
}

/**
 * The selected tasks, as the actions see them right now.
 *
 * @returns The snapshot's rows, the selected IDs and the active ID; or
 *   `null` when nothing can be edited (no project, or the mock is shown).
 */
function current(): {
  rows: readonly TaskRow[];
  selected: Set<string>;
  activeId: string | null;
} | null {
  const app = useAppStore.getState();
  if (app.snapshot === null || app.isDemoData) return null;
  return {
    rows: app.snapshot.tasks,
    selected: new Set(app.selectedTaskIds),
    activeId: app.selectedTaskId,
  };
}

/**
 * The tasks that deleting (or cutting) the selection would remove, as
 * top-level IDs: a task whose summary is also selected is left out,
 * because the summary takes it along.
 *
 * @param state - The selection as {@link current} returned it.
 * @returns The IDs to send `removeTask` for, in row order.
 */
function removalIds(state: NonNullable<ReturnType<typeof current>>): string[] {
  return topLevelSelection(state.rows, state.selected);
}

/**
 * Every row that removing `ids` takes away: each task plus, for a summary,
 * all its subtasks. Used by cut, so that what is pasted later is exactly
 * what disappeared.
 *
 * @param rows - All tasks in row order.
 * @param ids - Top-level IDs (see {@link removalIds}).
 * @returns The IDs of the removed rows, in row order.
 *
 * @example
 * // Rows: 1 Summary, 2 └ Sub, 3 Other. Cutting "Summary" removes 1 and 2.
 * withSubtasks(rows, ["summary-id"]); // ["summary-id", "sub-id"]
 */
function withSubtasks(rows: readonly TaskRow[], ids: readonly string[]): string[] {
  const result: string[] = [];
  for (const id of ids) {
    const start = rows.findIndex((row) => row.id === id);
    if (start === -1) continue;
    // `subtreeEnd` gives the index just after the task's last subtask.
    for (const row of rows.slice(start, subtreeEnd(rows, start))) result.push(row.id);
  }
  return result;
}

/**
 * Where a new or pasted task goes.
 *
 * - On a project: the first place inside it.
 * - Else below the active row (below a group's sub-tasks, so that it does
 *   not become one of them), on the same level.
 * - Without an active row: at the end.
 *
 * @param rows - All rows in list order.
 * @param activeIndex - The active row, or -1.
 * @returns The row number, the outline level (at least 2: tasks are always
 *   inside a project; never deeper than "row above + 1") and whether the
 *   active row is a project.
 *
 * @example
 * placeBelow(rows, 1); // { index: 2, level: 2, onProject: false }
 */
function placeBelow(
  rows: readonly TaskRow[],
  activeIndex: number,
): { index: number; level: number; onProject: boolean } {
  const active = rows[activeIndex];
  const onProject = active?.isProject === true;
  const index =
    active === undefined
      ? rows.length
      : onProject
        ? activeIndex + 1
        : subtreeEnd(rows, activeIndex);
  const level = Math.max(2, (active ?? rows[rows.length - 1])?.outlineLevel ?? 1);
  return { index, level, onProject };
}

/**
 * A new project row (Rust: `Task::project_row`).
 *
 * @param id - Its UUID.
 * @param name - Its name.
 */
function projectRow(id: string, name: string): Task {
  return { id, name, duration: { minutes: 0 }, outlineLevel: 1, isProject: true };
}

/**
 * Gives the outline actions, ready to be called from buttons or keys.
 *
 * @returns A {@link TaskActions} object (the same object between renders
 *   while the language does not change).
 *
 * @example
 * const actions = useTaskActions();
 * <button onClick={() => void actions.indentSelected()}>…</button>
 */
export function useTaskActions(): TaskActions {
  const { t } = useTranslation();

  return useMemo<TaskActions>(() => {
    /** Sends commands as one change (one batch, one undo step). */
    const send = (commands: Command[]) => useAppStore.getState().applyCommands(commands);

    /** Selects a new row and opens its name for typing. */
    const openName = (id: string): void => {
      useAppStore.getState().selectTask(id);
      useGridStore.getState().setAnchorId(id);
      useGridStore.getState().setPendingEditId(id);
    };

    /** Column headings for the clipboard, in the UI language. */
    const labels = (): ColumnLabels => ({
      name: t("grid.clipboard.name"),
      duration: t("grid.clipboard.duration"),
      start: t("grid.clipboard.start"),
      finish: t("grid.clipboard.finish"),
      predecessors: t("grid.clipboard.predecessors"),
      outlineLevel: t("grid.clipboard.outlineLevel"),
    });

    /**
     * Asks "Delete these tasks and their subtasks?" if one of them is a
     * summary.
     *
     * @returns `true` to go on (no summary, or the user said yes).
     */
    const confirmSubtasks = async (rows: readonly TaskRow[], ids: readonly string[]) => {
      const hasSubtasks = rows.some((row) => ids.includes(row.id) && row.isSummary);
      if (!hasSubtasks) return true;
      return confirmAction({
        title: t("task.deleteConfirmTitle"),
        message: t("task.deleteConfirmMessage", { count: ids.length }),
        okLabel: t("task.deleteConfirmOk"),
        cancelLabel: t("task.deleteConfirmCancel"),
      });
    };

    /**
     * Deletes tasks in one batch (one undo step) and selects the row that
     * moves up into the place of the first one.
     *
     * @param rows - All tasks in row order (before the delete).
     * @param ids - Top-level IDs to delete.
     */
    const removeTasks = async (rows: readonly TaskRow[], ids: readonly string[]) => {
      const first = rows.findIndex((row) => row.id === ids[0]);
      const after = rows
        .slice(first)
        .find(
          (row) => !ids.includes(row.id) && row.outlineLevel <= (rows[first]?.outlineLevel ?? 1),
        );
      await send(ids.map((id) => ({ type: "removeTask", id })));
      useAppStore.getState().selectTask(after?.id ?? null);
      useGridStore.getState().setAnchorId(after?.id ?? null);
    };

    /**
     * Adds a row below the active one (or at the end) and opens its name.
     *
     * @param name - Its name.
     * @param minutes - Its duration; 0 makes a milestone. A milestone is
     *   linked to the active task, so it sits at that task's end (and is
     *   scheduled automatically, else the link would not place it).
     */
    const insertBelow = async (name: string, minutes: number) => {
      const app = useAppStore.getState();
      const state = current();
      if (state === null || app.snapshot === null) return;
      const { rows, activeId } = state;
      const activeIndex = rows.findIndex((row) => row.id === activeId);
      const { index, level, onProject } = placeBelow(rows, activeIndex);
      const id = crypto.randomUUID();
      const link =
        minutes === 0 && activeId !== null && activeIndex >= 0 && !onProject
          ? linkCommand(activeId, id, crypto.randomUUID())
          : null;
      const mode =
        link === null ? useSettingsStore.getState().settings.scheduling.newTaskMode : "auto";
      // The last project was deleted: the task needs a new one around it.
      const needsProject = rows.length === 0;
      const projectTask = projectRow(crypto.randomUUID(), app.snapshot.projectName);
      const ok = await send([
        ...(needsProject ? [{ type: "addTask" as const, index: 0, task: projectTask }] : []),
        {
          type: "addTask",
          index: needsProject ? 1 : index,
          task: {
            id,
            name,
            duration: { minutes },
            outlineLevel: level,
            mode,
          },
        },
        ...(link === null ? [] : [link]),
      ]);
      if (!ok) return;
      // A collapsed project would hide the new row.
      if (onProject && activeId !== null) app.setCollapsed([activeId], false);
      openName(id);
    };

    return {
      addProject: async () => {
        const state = current();
        if (state === null) return;
        const id = crypto.randomUUID();
        const task = projectRow(id, t("task.newProjectName"));
        const ok = await send([{ type: "addTask", index: state.rows.length, task }]);
        if (ok) openName(id);
      },

      insertTask: async () => {
        // One working day, like MS Project's "1 day?" for new tasks.
        const minutes = useAppStore.getState().snapshot?.minutesPerDay;
        if (minutes !== undefined) await insertBelow(t("task.newTaskName"), minutes);
      },

      addMilestone: () => insertBelow(t("task.newMilestoneName"), 0),

      addSubtask: async () => {
        const app = useAppStore.getState();
        const state = current();
        if (state === null || app.snapshot === null) return;
        const { rows, activeId } = state;
        const parentIndex = rows.findIndex((row) => row.id === activeId);
        const parent = rows[parentIndex];
        if (parent === undefined) return;
        // The engine ignores blockers on a group, so a recorded delay would
        // silently vanish from the schedule. Say so instead.
        if ((parent.blockers ?? []).length > 0) {
          app.showIpcError({ code: "subtaskOnBlocked" });
          return;
        }
        const id = crypto.randomUUID();
        const ok = await app.applyCommand({
          type: "addTask",
          // Just after the task's last sub-task.
          index: subtreeEnd(rows, parentIndex),
          task: {
            id,
            // Directly inside a project it is a task.
            name: t(parent.isProject === true ? "task.newTaskName" : "task.newSubtaskName"),
            duration: { minutes: app.snapshot.minutesPerDay },
            outlineLevel: parent.outlineLevel + 1,
            mode: useSettingsStore.getState().settings.scheduling.newTaskMode,
          },
        });
        if (!ok) return;
        // A collapsed group would hide the new row.
        useAppStore.getState().setCollapsed([parent.id], false);
        openName(id);
      },

      deleteSelected: async () => {
        const state = current();
        if (state === null) return;
        const ids = removalIds(state);
        if (ids.length === 0 || !(await confirmSubtasks(state.rows, ids))) return;
        await removeTasks(state.rows, ids);
      },

      indentSelected: async () => {
        const state = current();
        if (state === null) return;
        const ids = topLevelSelection(state.rows, state.selected);
        await send(ids.map((id) => ({ type: "indentTask", id })));
      },

      outdentSelected: async () => {
        const state = current();
        if (state === null) return;
        const ids = topLevelSelection(state.rows, state.selected);
        await send(ids.map((id) => ({ type: "outdentTask", id })));
      },

      moveActive: async (direction) => {
        const state = current();
        if (state === null || state.activeId === null) return;
        const { rows } = state;
        const source = rows.findIndex((row) => row.id === state.activeId);
        const task = rows[source];
        if (task === undefined) return;
        const end = subtreeEnd(rows, source);
        const blockSize = end - source;
        let toIndex: number;
        if (direction === "down") {
          if (end >= rows.length) return;
          // Jump over the whole next block (a summary with its subtasks).
          toIndex = subtreeEnd(rows, end) - blockSize;
        } else {
          // The nearest row above at the same or a smaller level starts the
          // block we jump over (a sibling, or our own summary).
          let above = source - 1;
          while (above >= 0 && (rows[above]?.outlineLevel ?? 0) > task.outlineLevel) above--;
          if (above < 0) return;
          toIndex = above;
        }
        // The task keeps its level; Rust refuses the move if the outline
        // would break there (`invalidOutline`).
        await send([{ type: "moveTask", id: task.id, toIndex, level: task.outlineLevel }]);
      },

      collapseSelected: (collapsed) => {
        const state = current();
        if (state === null) return;
        const ids = state.rows
          .filter((row) => row.isSummary && state.selected.has(row.id))
          .map((r) => r.id);
        useAppStore.getState().setCollapsed(ids, collapsed);
      },

      markOnTrack: async () => {
        const state = current();
        if (state === null) return;
        const { calendars, snapshot } = useAppStore.getState();
        const ids = new Set(
          withSubtasks(state.rows, topLevelSelection(state.rows, state.selected)),
        );
        const commands = state.rows.flatMap((row): Command[] => {
          if (!ids.has(row.id)) return [];
          const percent = expectedProgress(
            row,
            calendars,
            snapshot?.calendarId ?? null,
            localToday(),
          );
          if (percent === null || percent <= (row.percentComplete ?? 0)) return [];
          return percentCommands(row, percent);
        });
        if (commands.length > 0) await send(commands);
      },

      setPlan: async () => {
        const rows = useAppStore.getState().snapshot?.tasks ?? [];
        const commands = rows.flatMap((row): Command[] =>
          row.finish === null || row.finish === row.plannedFinish
            ? []
            : [{ type: "setTaskPlannedFinish", id: row.id, plannedFinish: row.finish }],
        );
        if (commands.length > 0) await send(commands);
      },

      openTaskInfo: () => {
        const state = current();
        if (state === null || state.activeId === null) return;
        useTaskInfoStore.getState().open(state.activeId);
      },

      addBlocker: () => {
        const state = current();
        if (state === null || state.activeId === null) return;
        const task = state.rows.find((row) => row.id === state.activeId);
        if (task === undefined) return;
        if (task.isSummary || task.isProject === true) {
          // Same code as Rust's refusal, so the same translated text shows.
          useAppStore.getState().showIpcError({ code: "blockerOnSummary" });
          return;
        }
        useBlockerDialogStore.getState().open(task.id);
      },

      addRisk: () => {
        const state = current();
        if (state === null || state.activeId === null) return;
        useRiskDialogStore.getState().open(state.activeId);
      },

      toggleLinks: async () => {
        const state = current();
        if (state === null) return;
        const ids = topLevelSelection(state.rows, state.selected);
        if (selectionIsLinked(state.rows, ids)) {
          const links = linksInside(state.rows, new Set(ids));
          await send(links.map((id) => ({ type: "removeDependency", id })));
          return;
        }
        const commands = ids.flatMap((successor, i): Command[] => {
          const predecessor = ids[i - 1];
          if (predecessor === undefined) return [];
          const row = state.rows.find((r) => r.id === successor);
          const before = state.rows.find((r) => r.id === predecessor);
          // Linked already, in either direction: a second link is refused.
          if (row?.links.some((link) => link.predecessor === predecessor)) return [];
          if (before?.links.some((link) => link.predecessor === successor)) return [];
          const command = linkCommand(predecessor, successor, crypto.randomUUID());
          return command === null ? [] : [command];
        });
        if (commands.length > 0) await send(commands);
      },

      copySelected: async () => {
        const state = current();
        if (state === null) return;
        // In row order, so the copied text reads like the grid.
        const ids = state.rows.filter((row) => state.selected.has(row.id)).map((row) => row.id);
        if (ids.length === 0) return;
        try {
          await copyTasks(ids, labels());
        } catch (error: unknown) {
          useAppStore.getState().showIpcError(error);
        }
      },

      cutSelected: async () => {
        const state = current();
        if (state === null) return;
        const ids = removalIds(state);
        if (ids.length === 0 || !(await confirmSubtasks(state.rows, ids))) return;
        try {
          await copyTasks(withSubtasks(state.rows, ids), labels());
        } catch (error: unknown) {
          // Not copied, so do not delete: the tasks would be lost.
          useAppStore.getState().showIpcError(error);
          return;
        }
        await removeTasks(state.rows, ids);
      },

      paste: async () => {
        const state = current();
        if (state === null) return;
        const activeIndex = state.rows.findIndex((row) => row.id === state.activeId);
        // The last project was deleted: the tasks need a new one around them.
        if (state.rows.length === 0) {
          const name = useAppStore.getState().snapshot?.projectName ?? t("task.newProjectName");
          const task = projectRow(crypto.randomUUID(), name);
          if (!(await send([{ type: "addTask", index: 0, task }]))) return;
        }
        const place = placeBelow(state.rows, activeIndex);
        // In an empty list the new project row comes first.
        const atIndex = Math.max(1, place.index);
        const pasted = await useAppStore.getState().pasteTasks(atIndex, place.level, labels());
        if (pasted === null) return;
        // A collapsed project would hide the new rows.
        if (place.onProject && state.activeId !== null) {
          useAppStore.getState().setCollapsed([state.activeId], false);
        }
        const [first] = pasted.taskIds;
        if (first !== undefined) {
          useAppStore.getState().selectTasks(pasted.taskIds, first);
          useGridStore.getState().setAnchorId(first);
        }
        useGridStore.getState().setPasteWarnings(pasted.warnings);
      },
    };
  }, [t]);
}
