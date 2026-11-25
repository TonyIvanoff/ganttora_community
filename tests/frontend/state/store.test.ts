/**
 * tests/frontend/state/store.test.ts: tests for the app's central state
 * (the Zustand store).
 *
 * What it checks:
 *   `frontend/state/store.ts`, with the Rust side replaced by a fake:
 *   - `applyCommands` sends one command as it is, several commands as ONE
 *     `batch` command, and nothing at all for an empty list;
 *   - collapsing a summary task (`toggleCollapsed`, `setCollapsed`) also
 *     hides its subtasks in the Gantt chart's rows (`tasks`);
 *   - multi-selection: `toggleTaskSelection`, `selectTasks`, `revealTask`;
 *   - after a new snapshot arrives, the selection keeps only tasks that
 *     still exist.
 *
 * Why it matters:
 *   One user action must be one undo step: the task dialog and paste send
 *   several commands, so they must travel as one batch. A selection that
 *   points at a deleted task would make the next action fail.
 *
 * Main ideas:
 *   - `vi.mock` replaces the module `@frontend/ipc/client` (the bridge to
 *     Rust) with fake functions, so the store runs without the desktop app.
 *     `vi.mocked(fn)` lets us set what a fake returns and check how it was
 *     called.
 *   - The store is a single shared object; `beforeEach` resets it to its
 *     first state so the tests do not affect each other.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Command, ProjectResponse, ScheduleSnapshot } from "@frontend/ipc/types";
import { shedSnapshot, taskId } from "../gantt/helpers";

// Replace the IPC client before the store imports it. Every function the
// store uses must exist on the fake.
vi.mock("@frontend/ipc/client", () => ({
  fileNew: vi.fn(),
  fileOpen: vi.fn(),
  fileSave: vi.fn(),
  fileSaveAs: vi.fn(),
  pasteTasks: vi.fn(),
  projectApply: vi.fn(),
  projectNewDemo: vi.fn(),
  projectRedo: vi.fn(),
  projectUndo: vi.fn(),
  recoveryRestore: vi.fn(),
}));

// Imported after `vi.mock` (Vitest moves the mock to the top anyway).
import { projectApply, projectNewDemo } from "@frontend/ipc/client";
import { useAppStore } from "@frontend/state/store";

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

/** The shed with tasks 2 and 3 indented under task 1 (a summary). */
function outlinedShed(): ScheduleSnapshot {
  const snapshot = shedSnapshot();
  const tasks = snapshot.tasks.map((task, i) =>
    i === 1 || i === 2 ? { ...task, outlineLevel: 2 } : task,
  );
  return { ...snapshot, tasks };
}

/** Opens `snapshot` in the store, as "New demo project" would. */
async function open(snapshot: ScheduleSnapshot): Promise<void> {
  vi.mocked(projectNewDemo).mockResolvedValueOnce(response(snapshot));
  await useAppStore.getState().openDemoProject();
}

/** Two simple commands. */
const RENAME: Command = { type: "renameTask", id: taskId(1), name: "Design" };
const DURATION: Command = { type: "setTaskDuration", id: taskId(1), duration: { minutes: 960 } };

beforeEach(() => {
  vi.clearAllMocks();
  // `true` = replace the whole state, not merge into it.
  useAppStore.setState(useAppStore.getInitialState(), true);
});

describe("applyCommands", () => {
  // Several commands → one batch, so one Ctrl+Z undoes them all.
  it("sends several commands as one batch", async () => {
    vi.mocked(projectApply).mockResolvedValue(response(shedSnapshot()));
    const ok = await useAppStore.getState().applyCommands([RENAME, DURATION]);
    expect(ok).toBe(true);
    expect(projectApply).toHaveBeenCalledTimes(1);
    expect(projectApply).toHaveBeenCalledWith({ type: "batch", commands: [RENAME, DURATION] });
  });

  // One command needs no wrapper.
  it("sends a single command unwrapped", async () => {
    vi.mocked(projectApply).mockResolvedValue(response(shedSnapshot()));
    await useAppStore.getState().applyCommands([RENAME]);
    expect(projectApply).toHaveBeenCalledWith(RENAME);
  });

  // Nothing to do: no call to Rust, and it counts as success.
  it("sends nothing for an empty list", async () => {
    expect(await useAppStore.getState().applyCommands([])).toBe(true);
    expect(projectApply).not.toHaveBeenCalled();
  });

  // A refusal from Rust is shown as an error and reported as failure.
  it("reports a refused batch", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    vi.mocked(projectApply).mockRejectedValue({ code: "rejected" });
    expect(await useAppStore.getState().applyCommands([RENAME, DURATION])).toBe(false);
    expect(useAppStore.getState().commandError).toEqual({ error: { code: "rejected" } });
  });
});

describe("collapsing", () => {
  // Collapsing task 1 hides tasks 2 and 3 in the chart rows; expanding
  // brings them back.
  it("filters the Gantt rows of collapsed summaries", async () => {
    await open(outlinedShed());
    expect(useAppStore.getState().tasks).toHaveLength(8);

    useAppStore.getState().toggleCollapsed(taskId(1));
    expect(useAppStore.getState().tasks.map((t) => t.id)).toEqual([1, 4, 5, 6, 7, 8].map(taskId));

    useAppStore.getState().toggleCollapsed(taskId(1));
    expect(useAppStore.getState().tasks).toHaveLength(8);
  });

  // setCollapsed works on a list ("collapse all" / "expand all").
  it("collapses and expands a list of tasks", async () => {
    await open(outlinedShed());
    useAppStore.getState().setCollapsed([taskId(1)], true);
    expect(useAppStore.getState().collapsedTaskIds.has(taskId(1))).toBe(true);
    expect(useAppStore.getState().tasks).toHaveLength(6);
    useAppStore.getState().setCollapsed([taskId(1)], false);
    expect(useAppStore.getState().tasks).toHaveLength(8);
  });

  // The collapsed set survives an edit (same project) …
  it("keeps collapsed rows hidden after an edit", async () => {
    await open(outlinedShed());
    useAppStore.getState().toggleCollapsed(taskId(1));
    vi.mocked(projectApply).mockResolvedValue(response(outlinedShed()));
    await useAppStore.getState().applyCommand(RENAME);
    expect(useAppStore.getState().tasks).toHaveLength(6);
  });

  // … but is cleared when another project is opened.
  it("forgets collapsed rows when a new project opens", async () => {
    await open(outlinedShed());
    useAppStore.getState().toggleCollapsed(taskId(1));
    await open(outlinedShed());
    expect(useAppStore.getState().collapsedTaskIds.size).toBe(0);
    expect(useAppStore.getState().tasks).toHaveLength(8);
  });
});

describe("selection", () => {
  // Ctrl+click adds a task (it becomes the primary one); Ctrl+click again
  // removes it, and the last remaining one becomes primary.
  it("toggles tasks in and out of the selection", () => {
    const store = useAppStore.getState();
    store.toggleTaskSelection("a");
    store.toggleTaskSelection("b");
    expect(useAppStore.getState().selectedTaskIds).toEqual(["a", "b"]);
    expect(useAppStore.getState().selectedTaskId).toBe("b");
    useAppStore.getState().toggleTaskSelection("b");
    expect(useAppStore.getState().selectedTaskIds).toEqual(["a"]);
    expect(useAppStore.getState().selectedTaskId).toBe("a");
    useAppStore.getState().toggleTaskSelection("a");
    expect(useAppStore.getState().selectedTaskIds).toEqual([]);
    expect(useAppStore.getState().selectedTaskId).toBeNull();
  });

  // Shift+click selects a range; the primary is the given one or the last.
  it("selects a list of tasks", () => {
    useAppStore.getState().selectTasks(["a", "b", "c"]);
    expect(useAppStore.getState().selectedTaskIds).toEqual(["a", "b", "c"]);
    expect(useAppStore.getState().selectedTaskId).toBe("c");
    useAppStore.getState().selectTasks(["a", "b"], "a");
    expect(useAppStore.getState().selectedTaskId).toBe("a");
    useAppStore.getState().selectTasks([]);
    expect(useAppStore.getState().selectedTaskId).toBeNull();
  });

  // "Go to task" selects it alone and asks the views to scroll to it; each
  // request gets a new number, so asking twice for the same task works.
  it("reveals a task with a new request number each time", () => {
    useAppStore.getState().selectTasks(["a", "b"]);
    useAppStore.getState().revealTask("c");
    expect(useAppStore.getState().selectedTaskIds).toEqual(["c"]);
    expect(useAppStore.getState().selectedTaskId).toBe("c");
    expect(useAppStore.getState().revealTaskRequest).toEqual({ id: "c", seq: 1 });
    useAppStore.getState().revealTask("c");
    expect(useAppStore.getState().revealTaskRequest).toEqual({ id: "c", seq: 2 });
  });

  // After a delete, the snapshot no longer has task 8: it leaves the
  // selection, and the primary selection (task 8) is cleared.
  it("drops deleted tasks from the selection after a new snapshot", async () => {
    await open(shedSnapshot());
    useAppStore.getState().selectTasks([taskId(1), taskId(8)]);
    const without8 = shedSnapshot();
    without8.tasks = without8.tasks.slice(0, 7);
    vi.mocked(projectApply).mockResolvedValue(response(without8));
    await useAppStore.getState().applyCommand({ type: "removeTask", id: taskId(8) });
    expect(useAppStore.getState().selectedTaskIds).toEqual([taskId(1)]);
    expect(useAppStore.getState().selectedTaskId).toBeNull();
  });
});
