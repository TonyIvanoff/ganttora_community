/**
 * state/store.ts: the UI's shared memory (a Zustand store).
 *
 * What it is for:
 *   Several parts of the window need the same information: the toolbar
 *   changes the zoom, the Gantt chart uses it; the task grid sends a new
 *   duration, the Gantt chart and the toolbar show the new finish date.
 *   Instead of passing values from component to component, they all read
 *   from and write to one *store*.
 *
 * Where it fits:
 *   - The **source of truth is Rust** (decision D16). The real project, its
 *     schedule and its undo history live in the core (`document` crate).
 *     Every edit is a command sent there through `ipc/client.ts`; Rust
 *     answers with a `ProjectResponse` (new snapshot + canUndo/canRedo +
 *     file status) and this store simply *replaces* its copy with the
 *     answer. The store never calculates a date itself.
 *   - File actions (New, Open, Save, Save As) are IPC calls too. The
 *     dialogs that pick a path are shown by `features/files/useFileActions.ts`,
 *     which then calls `openFile` / `saveFileAs` here with the path.
 *   - Besides that copy it holds pure view state: zoom, the selection
 *     (`selectedTaskIds`, shared by the Gantt chart and the task grid, with
 *     `selectedTaskId` as the primary one), scroll requests, whether the
 *     10,000-task demo is shown, and which summary tasks are collapsed.
 *     Collapsed rows are left out of `tasks`, so the Gantt chart and the
 *     grid show the same rows side by side.
 *   - View state that only one feature needs lives in that feature's own
 *     store (e.g. `features/grid/gridStore.ts`: grid width, Shift anchor).
 *     the project documentation plans separate `state/selection.ts` and
 *     `state/viewState.ts` files; for now selection and zoom stay here,
 *     which keeps every shared value in one place.
 *
 * Two data sources for the Gantt chart:
 *   1. **The real project** from Rust (in the desktop app): the garden shed.
 *   2. **The mock schedule** (`features/gantt/mockSchedule.ts`): 10,000
 *      generated tasks for the scrolling performance test, switched on with
 *      the toolbar button "Demo: 10,000 tasks". In a plain browser (no Rust)
 *      it is the only data there is.
 *   `tasks` always holds what the Gantt chart should draw right now.
 *
 * Beginner note: a *hook* is a React function whose name starts with `use`.
 * `useAppStore` is a hook: a component calls
 * `const zoom = useAppStore((s) => s.zoom)` and React re-draws that
 * component automatically whenever `zoom` changes, and only then. The
 * function `(s) => s.zoom` is a *selector*: it picks the part of the store
 * the component cares about.
 */

import { create } from "zustand";
import { snapshotToDetailedGanttTasks } from "@frontend/features/gantt/fromSnapshot";
import { filterCollapsed } from "@frontend/features/grid/outline";
import { generateMockSchedule } from "@frontend/features/gantt/mockSchedule";
import type { GanttSourceTask } from "@frontend/features/gantt/renderer/model";
import { clampZoomPercent, type ZoomLevelId } from "@frontend/features/gantt/renderer/timeScale";
import {
  fileNew,
  fileOpen,
  fileSave,
  fileSaveAs,
  pasteTasks as ipcPasteTasks,
  projectApply,
  projectNewDemo,
  projectRedo,
  projectUndo,
  recoveryRestore,
  type AppInfo,
} from "@frontend/ipc/client";
import type {
  Calendar,
  ColumnLabels,
  Command,
  FileStatus,
  PasteResponse,
  ProjectResponse,
  ReadNoteDto,
  ScheduleSnapshot,
} from "@frontend/ipc/types";

/** Number of mock tasks for the M0 performance spike. */
export const MOCK_TASK_COUNT = 10_000;

/** Seed for the mock data; fixed so every run shows the same project. */
export const MOCK_SEED = 20_270_104;

/** Gives the display name of mock task number n (from the translation file). */
export type MockNameFor = (n: number) => string;

/** Everything the store holds, plus the functions that change it. */
export interface AppState {
  /** Info from the Rust core; `null` until loaded or when running in a browser. */
  appInfo: AppInfo | null;
  /** What the Gantt chart draws now: the real project or the mock tasks. */
  tasks: readonly GanttSourceTask[];
  /** `true` while the mock tasks are shown. Shown in the status bar. */
  isDemoData: boolean;
  /** The latest schedule received from Rust, or `null` before the first one. */
  snapshot: ScheduleSnapshot | null;
  /** Rust says Undo is possible (enables the Undo button). */
  canUndo: boolean;
  /** Rust says Redo is possible (enables the Redo button). */
  canRedo: boolean;
  /**
   * The file behind the project (name, read-only, unsaved changes), from
   * the latest Rust answer; `null` before the first one.
   */
  file: FileStatus | null;
  /**
   * Notes from the last opened file (things that did not fit the model),
   * shown in an info banner until dismissed or until another project is
   * opened. Rust sends them only once, in the `file_open` answer, so the
   * store keeps them here.
   */
  readNotes: readonly ReadNoteDto[];
  /**
   * The last refused command or failed IPC call, shown in the error banner
   * until dismissed or until the next successful command. (Scheduling
   * errors of the project itself are in `snapshot.error`.) See
   * {@link ShownError}: the store keeps the raw error, and the banner
   * translates it, so a language switch also changes the shown text.
   */
  commandError: ShownError | null;
  /**
   * ID of the selected task, or `null`. Clicking a bar selects it. With
   * several tasks selected, this is the *primary* one (the last clicked):
   * the "Why this date?" panel and the edit bar show it.
   */
  selectedTaskId: string | null;
  /**
   * IDs of all selected tasks, in the order they were selected (the
   * primary one, {@link AppState.selectedTaskId}, is last). Shared by the
   * Gantt chart and the task grid. Empty when nothing is selected.
   */
  selectedTaskIds: readonly string[];
  /**
   * Every calendar of the project with its working times, from the latest
   * Rust answer (`ProjectResponse.calendars`); empty before the first one.
   */
  calendars: readonly Calendar[];
  /**
   * A request to scroll a task into view (e.g. "jump to the driving task"
   * in the "Why this date?" panel). `seq` grows with every request, so
   * asking for the same task twice still triggers the scroll. `null` until
   * the first request.
   */
  revealTaskRequest: { id: string; seq: number } | null;
  /** Units of the timescale header: Days, Weeks or Months. */
  zoom: ZoomLevelId;
  /** Zoom of the timescale in percent (100 = normal), independent of `zoom`. */
  zoomPercent: number;
  /**
   * Increases by one each time someone asks the Gantt chart to scroll to the
   * project start. The chart reacts to the *change*, so the same request can
   * be made twice in a row. (A "request counter", sometimes called a nonce.)
   */
  scrollToStartRequest: number;
  /** The same kind of counter for "Go to today". */
  scrollToTodayRequest: number;
  /**
   * IDs of the summary tasks whose subtasks are hidden ("collapsed") in the
   * grid and the Gantt chart. View state only: not saved in the file and
   * not an undo step (as in MS Project).
   */
  collapsedTaskIds: ReadonlySet<string>;

  /** Stores the info received from Rust. */
  setAppInfo: (info: AppInfo | null) => void;
  /** Changes the zoom level. */
  setZoom: (zoom: ZoomLevelId) => void;
  /** Changes the zoom percentage (kept inside 10–400). */
  setZoomPercent: (percent: number) => void;
  /** Asks the Gantt chart to scroll to the first task. */
  requestScrollToStart: () => void;
  /** Asks the Gantt chart to show today. */
  requestScrollToToday: () => void;
  /** Selects only this task (by ID), or clears the selection with `null`. */
  selectTask: (id: string | null) => void;
  /**
   * Adds a task to the selection, or removes it if it is already selected
   * (Ctrl/Cmd+click). An added task becomes the primary selection.
   *
   * @param id - The task ID.
   */
  toggleTaskSelection: (id: string) => void;
  /**
   * Selects a task and asks the views to scroll it into view.
   *
   * @param id - The task ID.
   */
  revealTask: (id: string) => void;
  /**
   * Replaces the selection with several tasks at once (e.g. a Shift+click
   * range in the task grid).
   *
   * @param ids - The tasks, in the order to keep; the primary one should be last.
   * @param primaryId - The primary task (default: the last of `ids`, or
   *   `null` when `ids` is empty).
   */
  selectTasks: (ids: readonly string[], primaryId?: string | null) => void;
  /** Hides the error banner. */
  dismissError: () => void;
  /**
   * Shows a message in the error banner (e.g. when a file dialog could not
   * be opened).
   *
   * @param message - The text to show (already translated).
   */
  showError: (message: string) => void;
  /**
   * Shows a raw error (e.g. an `IpcError` from Rust) in the error banner;
   * the banner translates it with `lib/errors.ts`.
   *
   * @param error - What a rejected promise carried.
   */
  showIpcError: (error: unknown) => void;
  /** Hides the read-notes banner. */
  dismissReadNotes: () => void;
  /**
   * Shows or hides the 10,000-task mock schedule.
   *
   * @param show - `true` to show the mock tasks, `false` for the real project.
   * @param nameFor - Gives the display name of mock task number n (from the
   *   translation file, so this store contains no UI text).
   * @param count - How many mock tasks (default {@link MOCK_TASK_COUNT}).
   */
  setShowMock: (show: boolean, nameFor: MockNameFor, count?: number) => void;
  /** Asks Rust to open the garden-shed demo project and shows it. */
  openDemoProject: () => Promise<void>;
  /**
   * Sends one edit command to Rust and shows the result.
   *
   * @returns `true` if Rust accepted the command, `false` if it was refused
   *   (the reason is then in `commandError`).
   */
  applyCommand: (command: Command) => Promise<boolean>;
  /**
   * Sends several edit commands to Rust as **one** change: a `batch`
   * command, all-or-nothing and one undo step. One command is sent as it
   * is; an empty list does nothing.
   *
   * @param commands - The commands, in order.
   * @returns `true` if Rust accepted them (or the list was empty), `false`
   *   if it refused (then nothing changed and the reason is in `commandError`).
   */
  applyCommands: (commands: readonly Command[]) => Promise<boolean>;
  /**
   * Pastes the system clipboard's rows as new tasks (one undo step) and
   * shows the result.
   *
   * @param atIndex - The row the first new task goes to.
   * @param level - The outline level of the first new task.
   * @param labels - Column headings in the UI language.
   * @returns Rust's answer (new task IDs and warnings), or `null` if the
   *   paste failed (reason in `commandError`) or there is no Rust side.
   */
  pasteTasks: (
    atIndex: number,
    level: number,
    labels: ColumnLabels,
  ) => Promise<PasteResponse | null>;
  /**
   * Collapses or expands summary tasks (hides or shows their subtasks).
   *
   * @param ids - The summary tasks.
   * @param collapsed - `true` to hide the subtasks, `false` to show them.
   */
  setCollapsed: (ids: readonly string[], collapsed: boolean) => void;
  /**
   * Collapses an expanded summary task, or expands a collapsed one.
   *
   * @param id - The summary task.
   */
  toggleCollapsed: (id: string) => void;
  /** Asks Rust to undo the last edit. Does nothing if `canUndo` is false or the mock is shown. */
  undo: () => Promise<void>;
  /** Asks Rust to redo the last undone edit. Does nothing if `canRedo` is false or the mock is shown. */
  redo: () => Promise<void>;
  /**
   * Asks Rust for a new, empty project (replaces the open one).
   *
   * @param name - The new project's name (the translated "Untitled").
   * @returns `true` on success.
   */
  newFile: (name: string) => Promise<boolean>;
  /**
   * Asks Rust to open the `.gtra` file at `path` (replaces the open project).
   *
   * @param path - Full path, from the Open dialog.
   * @returns `true` on success; on failure the old project stays and the
   *   reason is in `commandError`.
   */
  openFile: (path: string) => Promise<boolean>;
  /**
   * Asks Rust to save to the project's current file. The caller must use
   * {@link AppState.saveFileAs} instead when `file.path` is `null` or
   * `file.readOnly` is true (`features/files/useFileActions.ts` does).
   *
   * @returns `true` on success.
   */
  saveFile: () => Promise<boolean>;
  /**
   * Asks Rust to save the project to `path`, which becomes its file.
   *
   * @param path - Full path, from the Save As dialog.
   * @returns `true` on success.
   */
  saveFileAs: (path: string) => Promise<boolean>;
  /**
   * Asks Rust to open a crash-recovery copy (replaces the open project).
   * The restored project has unsaved changes.
   *
   * @param sessionId - The copy's id from `recoveryList()`.
   * @returns `true` on success.
   */
  restoreRecovery: (sessionId: string) => Promise<boolean>;
}

/**
 * What the error banner shows: either a text that is already translated
 * (`{ text }`, e.g. "The file window could not be shown"), or a raw error
 * from Rust (`{ error }`, an `IpcError`) that the banner translates with
 * `lib/errors.ts`.
 *
 * Beginner note: `|` makes a *union type*: the value is one shape or the
 * other. `"text" in shown` tells TypeScript which one it is.
 */
export type ShownError = { text: string } | { error: unknown };

/**
 * The queue of IPC calls that change the project, so they run strictly one
 * after another. Without it, pressing Ctrl+Z three times quickly could send
 * three requests whose answers arrive in a different order, and the screen
 * would show an older snapshot last.
 *
 * Beginner note: a resolved promise is a value that is "already done".
 * `queue = queue.then(next)` means "run `next` when everything before it
 * has finished", like people standing in line.
 */
let queue: Promise<unknown> = Promise.resolve();

/**
 * Adds a task to the {@link queue}.
 *
 * @param task - The async work to run after everything already queued.
 * @returns A promise of the task's own result.
 */
function enqueue<T>(task: () => Promise<T>): Promise<T> {
  const run = queue.then(task);
  // The next task waits for this one, whether it succeeded or failed.
  queue = run.catch(() => undefined);
  return run;
}

/**
 * The app store hook. See the beginner note in the file header.
 *
 * @example
 * const zoom = useAppStore((s) => s.zoom);
 * const setZoom = useAppStore((s) => s.setZoom);
 * setZoom("weeks");
 *
 * Beginner note: `create` receives a function `(set, get) => ({ … })`.
 * `set` merges new values into the store, e.g. `set({ zoom: "weeks" })`,
 * and tells every component that uses `zoom` to re-draw. `get` reads the
 * current values (needed inside actions).
 */
export const useAppStore = create<AppState>()((set, get) => {
  /** Cached mock tasks: generated once, the first time they are shown. */
  let mockTasks: readonly GanttSourceTask[] | null = null;

  /**
   * Takes a `ProjectResponse` from Rust into the store.
   *
   * @param response - The answer of a project or file command.
   * @param replacesProject - `true` when the answer is a *different*
   *   project (New, Open, Open sample). Then the old read notes are replaced
   *   by the new ones, and the mock data is switched off so the user sees
   *   the project they just opened.
   */
  function receive(response: ProjectResponse, replacesProject: boolean): void {
    const { snapshot } = response;
    const { selectedTaskId, selectedTaskIds } = get();
    const isDemoData = replacesProject ? false : get().isDemoData;
    // Another project: the old collapsed IDs mean nothing there.
    const collapsed = replacesProject ? new Set<string>() : get().collapsedTaskIds;
    // Keep the selection only for tasks that still exist in the new
    // snapshot. A Set answers "is this ID there?" in one step.
    const ids = new Set(snapshot.tasks.map((task) => task.id));
    const stillThere = selectedTaskId !== null && ids.has(selectedTaskId);
    set({
      selectedTaskIds: selectedTaskIds.filter((id) => ids.has(id)),
      // `?? []` keeps older answers without calendars working.
      calendars: response.calendars ?? [],
      snapshot,
      canUndo: response.canUndo,
      canRedo: response.canRedo,
      file: response.file,
      commandError: null,
      selectedTaskId: stillThere ? selectedTaskId : null,
      isDemoData,
      // While the mock is shown, the chart keeps showing it.
      ...(isDemoData ? {} : { tasks: ganttTasks(snapshot, collapsed) }),
      ...(replacesProject ? { readNotes: response.file.notes, collapsedTaskIds: collapsed } : {}),
    });
  }

  /**
   * The rows the Gantt chart draws: the snapshot without the subtasks of
   * collapsed summary tasks (so the chart lines up with the grid).
   *
   * @param snapshot - The latest snapshot.
   * @param collapsed - The collapsed summary IDs.
   * @returns The Gantt tasks in row order.
   */
  function ganttTasks(
    snapshot: ScheduleSnapshot,
    collapsed: ReadonlySet<string>,
  ): GanttSourceTask[] {
    return snapshotToDetailedGanttTasks(filterCollapsed(snapshot, collapsed));
  }

  /**
   * Stores a new set of collapsed IDs and redraws the chart rows from it.
   *
   * @param collapsed - The new set.
   */
  function applyCollapsed(collapsed: ReadonlySet<string>): void {
    const { snapshot, isDemoData } = get();
    set({
      collapsedTaskIds: collapsed,
      ...(snapshot !== null && !isDemoData ? { tasks: ganttTasks(snapshot, collapsed) } : {}),
    });
  }

  /**
   * Runs one project IPC call in the queue and takes in its answer.
   *
   * @param call - The client function to call (returns `null` in a browser).
   * @param replacesProject - See {@link receive}. Default `false`.
   * @returns `true` on success, `false` if Rust refused or IPC failed.
   */
  function run(
    call: () => Promise<ProjectResponse | null>,
    replacesProject = false,
  ): Promise<boolean> {
    return enqueue(async () => {
      try {
        const response = await call();
        if (response === null) return false; // plain browser: no Rust side
        receive(response, replacesProject);
        return true;
      } catch (error: unknown) {
        // The English detail is for developers (console); the banner shows
        // the translated message (shell/AppShell.tsx, lib/errors.ts).
        console.warn("IPC error", error);
        set({ commandError: { error } });
        return false;
      }
    });
  }

  return {
    appInfo: null,
    tasks: [],
    isDemoData: false,
    snapshot: null,
    canUndo: false,
    canRedo: false,
    file: null,
    readNotes: [],
    commandError: null,
    selectedTaskId: null,
    selectedTaskIds: [],
    calendars: [],
    revealTaskRequest: null,
    zoom: "days",
    zoomPercent: 100,
    scrollToStartRequest: 0,
    scrollToTodayRequest: 0,
    collapsedTaskIds: new Set<string>(),

    setAppInfo: (info) => set({ appInfo: info }),
    setZoom: (zoom) => set({ zoom }),
    setZoomPercent: (percent) => set({ zoomPercent: clampZoomPercent(percent) }),
    requestScrollToStart: () => set((s) => ({ scrollToStartRequest: s.scrollToStartRequest + 1 })),
    requestScrollToToday: () => set((s) => ({ scrollToTodayRequest: s.scrollToTodayRequest + 1 })),
    selectTask: (id) => set({ selectedTaskId: id, selectedTaskIds: id === null ? [] : [id] }),
    toggleTaskSelection: (id) =>
      set((s) => {
        if (s.selectedTaskIds.includes(id)) {
          const rest = s.selectedTaskIds.filter((other) => other !== id);
          // The last remaining one becomes primary (or nothing is selected).
          return { selectedTaskIds: rest, selectedTaskId: rest.at(-1) ?? null };
        }
        return { selectedTaskIds: [...s.selectedTaskIds, id], selectedTaskId: id };
      }),
    revealTask: (id) =>
      set((s) => ({
        selectedTaskId: id,
        selectedTaskIds: [id],
        revealTaskRequest: { id, seq: (s.revealTaskRequest?.seq ?? 0) + 1 },
      })),
    selectTasks: (ids, primaryId) =>
      set({ selectedTaskIds: [...ids], selectedTaskId: primaryId ?? ids.at(-1) ?? null }),
    dismissError: () => set({ commandError: null }),
    showError: (message) => set({ commandError: { text: message } }),
    showIpcError: (error) => set({ commandError: { error } }),
    dismissReadNotes: () => set({ readNotes: [] }),

    setShowMock: (show, nameFor, count = MOCK_TASK_COUNT) => {
      if (show) {
        mockTasks ??= generateMockSchedule({ count, seed: MOCK_SEED, nameFor });
        set({ tasks: mockTasks, isDemoData: true, selectedTaskId: null, selectedTaskIds: [] });
      } else {
        const { snapshot } = get();
        set({
          tasks: snapshot === null ? [] : ganttTasks(snapshot, get().collapsedTaskIds),
          isDemoData: false,
          selectedTaskId: null,
          selectedTaskIds: [],
        });
      }
    },

    openDemoProject: async () => {
      await run(projectNewDemo, true);
    },

    applyCommand: (command) => run(() => projectApply(command)),

    applyCommands: async (commands) => {
      const [first] = commands;
      if (first === undefined) return true;
      // One command needs no wrapper; several become one batch, so one
      // Ctrl+Z undoes them all and a refusal leaves nothing half done.
      const command: Command =
        commands.length === 1 ? first : { type: "batch", commands: [...commands] };
      return run(() => projectApply(command));
    },

    pasteTasks: (atIndex, level, labels) =>
      enqueue(async () => {
        try {
          const pasted = await ipcPasteTasks(atIndex, level, labels);
          if (pasted === null) return null;
          receive(pasted.response, false);
          return pasted;
        } catch (error: unknown) {
          console.warn("IPC error", error);
          set({ commandError: { error } });
          return null;
        }
      }),

    setCollapsed: (ids, collapsed) => {
      // A new Set (never change the old one): Zustand compares by identity.
      const next = new Set(get().collapsedTaskIds);
      for (const id of ids) {
        if (collapsed) next.add(id);
        else next.delete(id);
      }
      applyCollapsed(next);
    },

    toggleCollapsed: (id) => {
      const next = new Set(get().collapsedTaskIds);
      if (!next.delete(id)) next.add(id);
      applyCollapsed(next);
    },

    undo: async () => {
      // While the mock is shown the project is hidden, so undo would change
      // something the user cannot see: do nothing.
      if (!get().canUndo || get().isDemoData) return;
      await run(projectUndo);
    },

    redo: async () => {
      if (!get().canRedo || get().isDemoData) return;
      await run(projectRedo);
    },

    newFile: (name) => run(() => fileNew(name), true),
    openFile: (path) => run(() => fileOpen(path), true),
    saveFile: () => run(fileSave),
    saveFileAs: (path) => run(() => fileSaveAs(path)),
    restoreRecovery: (sessionId) => run(() => recoveryRestore(sessionId), true),
  };
});
