/**
 * tests/frontend/grid/cutSelected.test.ts: tests for "Cut" (Ctrl/Cmd+X)
 * and "Add sub-task" in the task grid.
 *
 * What it checks:
 *   `cutSelected` from `useTaskActions` (`frontend/features/grid/useTaskActions.ts`):
 *   - it copies the selected tasks *with the subtasks* of a selected
 *     summary, in row order, and then deletes only the top-level tasks;
 *   - all deletions travel to Rust as ONE batch (one Ctrl+Z restores them);
 *   - if copying fails, nothing is deleted and the error is shown;
 *   - if the user answers "No" to "Delete these tasks and their
 *     subtasks?", nothing is copied or deleted.
 *
 * Why it matters:
 *   Cut removes tasks from the plan. If the copy failed but the delete
 *   still happened, the tasks would be gone and not on the clipboard: the
 *   user would lose work. And pasting must bring back exactly what
 *   disappeared, subtasks included.
 *
 * Main ideas:
 *   - `useTaskActions` is a React *hook* (a function that may only run
 *     inside a component). Here it is called outside React, so we replace
 *     the two React helpers it uses: `useMemo` just runs its function, and
 *     `useTranslation` gives a `t` that returns the key.
 *   - `vi.mock` replaces the bridge to Rust (`@frontend/ipc/client`) and
 *     the native dialog (`confirmAction`) with fakes we control, like in
 *     `tests/frontend/state/store.test.ts`.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProjectResponse, ScheduleSnapshot } from "@frontend/ipc/types";
import { shedSnapshot, taskId } from "../gantt/helpers";

// React: `useMemo(fn)` → just `fn()`. Everything else stays real.
vi.mock("react", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react")>()),
  useMemo: <T>(fn: () => T) => fn(),
}));

// Translations: `useTranslation().t(key)` returns the key itself. The
// rest of the library stays real (the app's i18n setup needs it).
vi.mock("react-i18next", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: (key: string) => key }),
}));

// The bridge to Rust: the real module, with the calls we need faked.
vi.mock("@frontend/ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@frontend/ipc/client")>()),
  copyTasks: vi.fn(),
  projectApply: vi.fn(),
  pasteTasks: vi.fn(() => Promise.resolve(null)),
  projectNewDemo: vi.fn(),
}));

// The "Delete subtasks too?" question: we decide the answer.
vi.mock("@frontend/features/files/dialogs", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@frontend/features/files/dialogs")>()),
  confirmAction: vi.fn(),
}));

import { confirmAction } from "@frontend/features/files/dialogs";
import { useTaskActions } from "@frontend/features/grid/useTaskActions";
import { copyTasks, pasteTasks, projectApply, projectNewDemo } from "@frontend/ipc/client";
import { useAppStore } from "@frontend/state/store";

/**
 * The shed with task 1 as a summary over tasks 2 and 3. Not marked as
 * demo data, so the actions are allowed to edit it.
 */
function outlinedShed(): ScheduleSnapshot {
  const snapshot = shedSnapshot();
  const tasks = snapshot.tasks.map((task, i) => {
    if (i === 0) return { ...task, isSummary: true };
    if (i === 1 || i === 2) return { ...task, outlineLevel: 2 };
    return task;
  });
  return { ...snapshot, tasks };
}

/** Wraps a snapshot in the answer Rust sends after an edit. */
function response(snapshot: ScheduleSnapshot): ProjectResponse {
  return {
    snapshot,
    canUndo: true,
    canRedo: false,
    file: {
      path: null,
      fileName: null,
      readOnly: false,
      dirty: true,
      notes: [],
      upgradedFrom: null,
      readOnlyReason: null,
    },
    calendars: [],
  };
}

/**
 * Loads `snapshot` into the store as an editable project and selects
 * `selected` (row numbers, 1-based).
 */
async function openAndSelect(snapshot: ScheduleSnapshot, selected: number[]): Promise<void> {
  vi.mocked(projectNewDemo).mockResolvedValueOnce(response(snapshot));
  await useAppStore.getState().openDemoProject();
  // `openDemoProject` marks the data as a demo; real projects are editable.
  useAppStore.setState({ isDemoData: false });
  const ids = selected.map(taskId);
  useAppStore.getState().selectTasks(ids, ids[0] ?? null);
}

beforeEach(() => {
  vi.clearAllMocks();
  useAppStore.setState(useAppStore.getInitialState(), true);
  vi.mocked(projectApply).mockResolvedValue(response(shedSnapshot()));
  vi.mocked(copyTasks).mockResolvedValue(undefined);
  vi.mocked(confirmAction).mockResolvedValue(true);
});

describe("cutSelected", () => {
  // Cutting the summary (row 1) copies rows 1, 2, 3 (its subtasks go with
  // it) and sends one removeTask for the summary only: Rust removes the
  // subtasks together with it.
  it("copies the summary with its subtasks and deletes the summary", async () => {
    await openAndSelect(outlinedShed(), [1]);
    await useTaskActions().cutSelected();

    expect(confirmAction).toHaveBeenCalledTimes(1);
    expect(copyTasks).toHaveBeenCalledTimes(1);
    expect(vi.mocked(copyTasks).mock.calls[0]?.[0]).toEqual([taskId(1), taskId(2), taskId(3)]);
    expect(projectApply).toHaveBeenCalledTimes(1);
    expect(projectApply).toHaveBeenCalledWith({ type: "removeTask", id: taskId(1) });
  });

  // Several plain tasks: copied and deleted together, as ONE batch, so one
  // Ctrl+Z restores them all. A subtask whose summary is also selected is
  // not removed separately.
  it("deletes several tasks in one batch", async () => {
    await openAndSelect(outlinedShed(), [1, 2, 5, 7]);
    await useTaskActions().cutSelected();

    expect(vi.mocked(copyTasks).mock.calls[0]?.[0]).toEqual([
      taskId(1),
      taskId(2),
      taskId(3),
      taskId(5),
      taskId(7),
    ]);
    expect(projectApply).toHaveBeenCalledTimes(1);
    expect(projectApply).toHaveBeenCalledWith({
      type: "batch",
      commands: [
        { type: "removeTask", id: taskId(1) },
        { type: "removeTask", id: taskId(5) },
        { type: "removeTask", id: taskId(7) },
      ],
    });
  });

  // No summary selected: no question is asked.
  it("does not ask when no summary is cut", async () => {
    await openAndSelect(outlinedShed(), [5]);
    await useTaskActions().cutSelected();
    expect(confirmAction).not.toHaveBeenCalled();
    expect(projectApply).toHaveBeenCalledWith({ type: "removeTask", id: taskId(5) });
  });

  // The copy fails (e.g. the clipboard is busy): nothing may be deleted,
  // otherwise the tasks would be lost. The error is shown instead.
  it("deletes nothing when copying fails", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    vi.mocked(copyTasks).mockRejectedValueOnce({ code: "clipboardUnavailable" });
    await openAndSelect(outlinedShed(), [5]);
    await useTaskActions().cutSelected();

    expect(copyTasks).toHaveBeenCalledTimes(1);
    expect(projectApply).not.toHaveBeenCalled();
    expect(useAppStore.getState().commandError).not.toBeNull();
  });

  // The user says "No" to deleting subtasks: nothing is copied or deleted.
  it("does nothing when the user cancels", async () => {
    vi.mocked(confirmAction).mockResolvedValueOnce(false);
    await openAndSelect(outlinedShed(), [1]);
    await useTaskActions().cutSelected();
    expect(copyTasks).not.toHaveBeenCalled();
    expect(projectApply).not.toHaveBeenCalled();
  });

  // Nothing selected, or the demo project shown: nothing happens.
  it("does nothing without a selection or on demo data", async () => {
    await openAndSelect(outlinedShed(), []);
    await useTaskActions().cutSelected();
    useAppStore.setState({ isDemoData: true, selectedTaskIds: [taskId(5)] });
    await useTaskActions().cutSelected();
    expect(copyTasks).not.toHaveBeenCalled();
    expect(projectApply).not.toHaveBeenCalled();
  });
});

describe("addSubtask", () => {
  // Adding to the group in row 1 puts the new row after its last sub-task
  // (row 3), one level deeper. Index 3 counts from 0.
  it("adds the new task at the end of the group, one level deeper", async () => {
    await openAndSelect(outlinedShed(), [1]);
    useAppStore.getState().setCollapsed([taskId(1)], true);
    await useTaskActions().addSubtask();

    expect(projectApply).toHaveBeenCalledTimes(1);
    expect(vi.mocked(projectApply).mock.calls[0]?.[0]).toMatchObject({
      type: "addTask",
      index: 3,
      task: { name: "task.newSubtaskName", outlineLevel: 2 },
    });
    // A collapsed group opens, so the new row is visible.
    expect(useAppStore.getState().collapsedTaskIds.has(taskId(1))).toBe(false);
  });

  it("turns a plain task into a group", async () => {
    await openAndSelect(outlinedShed(), [4]);
    await useTaskActions().addSubtask();
    expect(vi.mocked(projectApply).mock.calls[0]?.[0]).toMatchObject({
      index: 4,
      task: { outlineLevel: 2 },
    });
  });

  // As a group, the task's blocker would silently stop counting.
  it("refuses a task with a blocker", async () => {
    const snapshot = outlinedShed();
    const blocker = {
      id: "b1",
      afterWorkMinutes: 0,
      durationMinutes: 480,
      reason: "Late",
      reportedOn: null,
      start: null,
      finish: null,
    };
    const tasks = snapshot.tasks.map((t, i) => (i === 3 ? { ...t, blockers: [blocker] } : t));
    await openAndSelect({ ...snapshot, tasks }, [4]);
    await useTaskActions().addSubtask();
    expect(projectApply).not.toHaveBeenCalled();
    expect(useAppStore.getState().commandError).toMatchObject({
      error: { code: "subtaskOnBlocked" },
    });
  });
});

describe("toggleLinks", () => {
  // Rows 1 and 2 are linked in the shed already; 2 is inside group 1.
  it("links in row order as one batch and skips existing links", async () => {
    await openAndSelect(shedSnapshot(), [8, 1, 2]);
    await useTaskActions().toggleLinks();
    expect(projectApply).toHaveBeenCalledTimes(1);
    expect(vi.mocked(projectApply).mock.calls[0]?.[0]).toMatchObject({
      type: "addDependency",
      dependency: { predecessor: taskId(2), successor: taskId(8), kind: "finishToStart" },
    });
  });

  it("links a selected group instead of its sub-tasks", async () => {
    await openAndSelect(outlinedShed(), [1, 2, 8]);
    await useTaskActions().toggleLinks();
    expect(vi.mocked(projectApply).mock.calls[0]?.[0]).toMatchObject({
      dependency: { predecessor: taskId(1), successor: taskId(8) },
    });
  });

  it("does nothing with one task", async () => {
    await openAndSelect(shedSnapshot(), [1]);
    await useTaskActions().toggleLinks();
    expect(projectApply).not.toHaveBeenCalled();
  });

  it("removes the links when all selected tasks are linked", async () => {
    const snapshot = shedSnapshot();
    const link = snapshot.tasks[1]?.links[0]?.id;
    await openAndSelect(snapshot, [1, 2]);
    await useTaskActions().toggleLinks();
    expect(vi.mocked(projectApply).mock.calls[0]?.[0]).toEqual({
      type: "removeDependency",
      id: link,
    });
  });
});

describe("projects", () => {
  it("adds a project row at the end", async () => {
    await openAndSelect(shedSnapshot(), [1]);
    await useTaskActions().addProject();
    expect(vi.mocked(projectApply).mock.calls[0]?.[0]).toMatchObject({
      type: "addTask",
      index: 8,
      task: { isProject: true, outlineLevel: 1 },
    });
  });

  it("inserts a task inside the selected project", async () => {
    const snapshot = outlinedShed();
    const tasks = snapshot.tasks.map((t, i) => (i === 0 ? { ...t, isProject: true } : t));
    await openAndSelect({ ...snapshot, tasks }, [1]);
    await useTaskActions().insertTask();
    expect(vi.mocked(projectApply).mock.calls[0]?.[0]).toMatchObject({
      type: "addTask",
      index: 1,
      task: { outlineLevel: 2 },
    });
  });
});

describe("Add task and paste", () => {
  /** Opens the rows and selects one (counted from 1). */
  async function select(selected: number) {
    const snapshot = shedSnapshot();
    // Row 2 is a group with the sub-tasks 3 and 4.
    const tasks = snapshot.tasks.map((t, i) => ({
      ...t,
      isSummary: i === 1,
      outlineLevel: i === 2 || i === 3 ? 3 : 2,
    }));
    await openAndSelect({ ...snapshot, tasks }, [selected]);
  }
  it("adds below the selected task", async () => {
    await select(1);
    await useTaskActions().insertTask();
    expect(vi.mocked(projectApply).mock.calls[0]?.[0]).toMatchObject({
      index: 1,
      task: { outlineLevel: 2 },
    });
  });
  it("adds a sub-task below the selected sub-task", async () => {
    await select(3);
    await useTaskActions().insertTask();
    expect(vi.mocked(projectApply).mock.calls[0]?.[0]).toMatchObject({
      index: 3,
      task: { outlineLevel: 3 },
    });
  });
  it("adds below the sub-tasks of a selected group", async () => {
    await select(2);
    await useTaskActions().insertTask();
    expect(vi.mocked(projectApply).mock.calls[0]?.[0]).toMatchObject({
      index: 4,
      task: { outlineLevel: 2 },
    });
  });
  it("pastes below the selected sub-task, as a sub-task", async () => {
    await select(4);
    await useTaskActions().paste();
    expect(pasteTasks).toHaveBeenLastCalledWith(4, 3, expect.anything());
  });
  it("pastes below the sub-tasks of a selected group", async () => {
    await select(2);
    await useTaskActions().paste();
    expect(pasteTasks).toHaveBeenLastCalledWith(4, 2, expect.anything());
  });
});

describe("markOnTrack", () => {
  // Rows 2 and 3 are in group 1. All dates are long past, so 100 % is due.
  it("sets the sub-tasks of a group in one batch and never lowers progress", async () => {
    const snapshot = outlinedShed();
    const past = { start: "2020-01-06T08:00:00", finish: "2020-01-07T17:00:00" };
    const tasks = snapshot.tasks.map((t, i) => ({
      ...t,
      ...past,
      percentComplete: i === 2 ? 100 : 20,
    }));
    await openAndSelect({ ...snapshot, tasks }, [1, 4]);
    await useTaskActions().markOnTrack();
    expect(projectApply).toHaveBeenCalledTimes(1);
    expect(projectApply).toHaveBeenCalledWith({
      type: "batch",
      commands: [
        { type: "setTaskPercentComplete", id: taskId(2), percent: 100 },
        expect.objectContaining({ type: "setTaskActualFinish", id: taskId(2) }),
        { type: "setTaskPercentComplete", id: taskId(4), percent: 100 },
        expect.objectContaining({ type: "setTaskActualFinish", id: taskId(4) }),
      ],
    });
  });

  it("does nothing for a task in the future", async () => {
    const snapshot = shedSnapshot();
    const future = { start: "2090-01-02T08:00:00", finish: "2090-01-03T17:00:00" };
    const tasks = snapshot.tasks.map((t) => ({ ...t, ...future }));
    await openAndSelect({ ...snapshot, tasks }, [4]);
    await useTaskActions().markOnTrack();
    expect(projectApply).not.toHaveBeenCalled();
  });
});

describe("addMilestone", () => {
  it("adds a row without duration below the selected task, linked to it", async () => {
    await openAndSelect(outlinedShed(), [2]);
    await useTaskActions().addMilestone();
    expect(vi.mocked(projectApply).mock.calls[0]?.[0]).toMatchObject({
      type: "batch",
      commands: [
        {
          type: "addTask",
          index: 2,
          task: {
            name: "task.newMilestoneName",
            duration: { minutes: 0 },
            outlineLevel: 2,
            mode: "auto",
          },
        },
        { type: "addDependency", dependency: { predecessor: taskId(2), kind: "finishToStart" } },
      ],
    });
  });

  it("is not linked to a project", async () => {
    const snapshot = outlinedShed();
    const tasks = snapshot.tasks.map((t, i) => (i === 0 ? { ...t, isProject: true } : t));
    await openAndSelect({ ...snapshot, tasks }, [1]);
    await useTaskActions().addMilestone();
    expect(vi.mocked(projectApply).mock.calls[0]?.[0]).toMatchObject({ type: "addTask", index: 1 });
  });
});

describe("setPlan", () => {
  it("saves every finish that is not the plan yet, in one batch", async () => {
    const snapshot = shedSnapshot();
    const tasks = snapshot.tasks.map((t, i) =>
      i === 0 && t.finish !== null ? { ...t, plannedFinish: t.finish } : t,
    );
    await openAndSelect({ ...snapshot, tasks }, [1]);
    await useTaskActions().setPlan();
    expect(projectApply).toHaveBeenCalledTimes(1);
    expect(vi.mocked(projectApply).mock.calls[0]?.[0]).toMatchObject({
      type: "batch",
      commands: tasks
        .slice(1)
        .map((t) => ({ type: "setTaskPlannedFinish", id: t.id, plannedFinish: t.finish })),
    });
  });
});
