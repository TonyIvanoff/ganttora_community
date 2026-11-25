/**
 * ipc/client.ts: typed functions for talking to the Rust side of the app.
 *
 * What it is for:
 *   The window (React) and the core (Rust) run in different worlds. They talk
 *   through IPC ("inter-process communication"): the UI sends a *command* by
 *   name, Rust runs it and sends back a result as JSON. Tauri's `invoke()`
 *   does the sending. This file wraps each command in a small function with
 *   proper TypeScript types, so the rest of the UI never calls `invoke()`
 *   with a raw string.
 *
 * Where it fits:
 *   Rust side: `backend/src-tauri/src/ipc/mod.rs` (`app_info`) and
 *   `backend/src-tauri/src/ipc/project.rs` (`project_*`) and
 *   `backend/src-tauri/src/ipc/calendar.rs` (`calendar_*`) and
 *   `backend/src-tauri/src/ipc/file.rs` (`file_*`) and
 *   `backend/src-tauri/src/ipc/recovery.rs` (`recovery_*`) and
 *   `backend/src-tauri/src/ipc/app.rs` (`app_close_project`, `app_quit`) and
 *   `backend/src-tauri/src/ipc/settings.rs` (`settings_*`) and
 *   `backend/src-tauri/src/ipc/clipboard.rs` (`copy_tasks`, `paste_tasks`)
 *   define the commands.
 *   UI side: components and the store (`state/store.ts`) call these wrappers.
 *   Decision D16: every edit is a command sent to Rust; the UI only shows
 *   the result. No scheduling logic lives in the UI.
 *
 * Main ideas:
 *   - When the UI runs in a normal browser (`pnpm dev` without Tauri) there
 *     is no Rust side. The wrappers then return `null` instead of failing,
 *     so the UI can still be developed and tested in a browser.
 *   - The JSON shapes live in `ipc/types.ts`. That file is written by
 *     hand for now; later it is *generated* from the Rust types
 *     (scripts/gen-ipc-types.sh). `AppInfo` below is still declared here and
 *     moves there with the generator.
 *   - Project and file commands answer with a `ProjectResponse` (the new
 *     snapshot plus canUndo / canRedo plus the file status).
 *   - If Rust refuses a command, the promise is *rejected* with an
 *     `IpcError` object (a code, an English detail, sometimes `details`);
 *     callers `catch` it and show it with `lib/errors.ts`, which picks the
 *     translated message.
 */

import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  ColumnLabels,
  Command,
  IsoDateTime,
  MenuNode,
  MenuStateUpdate,
  PasteResponse,
  Uuid,
  ProjectResponse,
  RecoveryCopyDto,
  Settings,
  SettingsResponse,
} from "./types";

/**
 * Basic facts about the running app, returned by the Rust command `app_info`.
 *
 * Must match the Rust struct `AppInfo` in `backend/src-tauri/src/ipc/mod.rs`
 * field by field. (Temporary: this type moves to the generated `types.ts`.)
 */
export interface AppInfo {
  /** Product name, e.g. "Ganttora". Not translated: it is a name. */
  name: string;
  /** App version from Cargo.toml, e.g. "0.1.0". */
  version: string;
  /** Operating system family reported by Rust: "macos", "windows" or "linux". */
  os: string;
}

/**
 * Tells whether the UI is running inside the Tauri desktop window.
 *
 * @returns `true` in the desktop app, `false` in a plain browser tab.
 *
 * @example
 * if (!hasDesktopCore()) console.info("Browser preview mode");
 */
export function hasDesktopCore(): boolean {
  return isTauri();
}

/**
 * Asks Rust for basic app information (name, version, operating system).
 *
 * @returns A promise of the `AppInfo`, or of `null` when there is no Rust
 *   side (plain browser).
 * @throws Rejects if the Rust command itself fails (it cannot in M0, but
 *   real commands can; callers should `catch`).
 *
 * @example
 * const info = await getAppInfo();
 * if (info) console.log(info.version);
 *
 * Beginner note: `async` functions return a *Promise*: a value that arrives
 * later. `await` waits for it without freezing the window. Talking to Rust
 * is asynchronous because the answer comes back from another thread.
 */
export async function getAppInfo(): Promise<AppInfo | null> {
  if (!hasDesktopCore()) return null;
  // The string must equal the Rust function name `app_info`.
  return invoke<AppInfo>("app_info");
}

/**
 * Opens the garden-shed demo project in this window (Rust: `project_new_demo`).
 *
 * @returns A promise of the new project's `ProjectResponse`, or of `null`
 *   in a plain browser (no Rust side).
 * @throws Rejects with an `IpcError` if the shell state is unusable.
 *
 * @example
 * const response = await projectNewDemo();
 * response?.snapshot.projectFinish; // "2027-03-23T17:00:00"
 */
export async function projectNewDemo(): Promise<ProjectResponse | null> {
  if (!hasDesktopCore()) return null;
  return invoke<ProjectResponse>("project_new_demo");
}

/**
 * Reads the current schedule of this window's project without changing it
 * (Rust: `project_snapshot`).
 *
 * @returns A promise of the current `ProjectResponse`, or `null` in a browser.
 * @throws Rejects with an `IpcError` if no project is open.
 *
 * @example
 * const current = await projectSnapshot();
 */
export async function projectSnapshot(): Promise<ProjectResponse | null> {
  if (!hasDesktopCore()) return null;
  return invoke<ProjectResponse>("project_snapshot");
}

/**
 * Sends one edit command to Rust (Rust: `project_apply`).
 *
 * @param command - The edit, e.g.
 *   `{ type: "setTaskDuration", id, duration: { minutes: 2880 } }`.
 * @returns A promise of the `ProjectResponse` after the edit, or `null` in a browser.
 * @throws Rejects with an `IpcError` if the edit is refused (unknown
 *   task, negative duration, circular link…). The project is then unchanged.
 *
 * @example
 * const after = await projectApply({
 *   type: "setTaskDuration",
 *   id: permitId,
 *   duration: { minutes: 6 * 480 },
 * });
 *
 * Beginner note: the second argument of `invoke` is an object whose keys
 * are the Rust parameter names. `{ command }` is short for
 * `{ command: command }`.
 */
export async function projectApply(command: Command): Promise<ProjectResponse | null> {
  if (!hasDesktopCore()) return null;
  return invoke<ProjectResponse>("project_apply", { command });
}

/**
 * Undoes the last edit (Rust: `project_undo`).
 *
 * @returns A promise of the `ProjectResponse` after the undo, or `null` in a browser.
 * @throws Rejects with an `IpcError` (`nothingToUndo`) if there is nothing to undo.
 *
 * @example
 * await projectUndo();
 */
export async function projectUndo(): Promise<ProjectResponse | null> {
  if (!hasDesktopCore()) return null;
  return invoke<ProjectResponse>("project_undo");
}

/**
 * Redoes the last undone edit (Rust: `project_redo`).
 *
 * @returns A promise of the `ProjectResponse` after the redo, or `null` in a browser.
 * @throws Rejects with an `IpcError` (`nothingToRedo`) if there is nothing to redo.
 *
 * @example
 * await projectRedo();
 */
export async function projectRedo(): Promise<ProjectResponse | null> {
  if (!hasDesktopCore()) return null;
  return invoke<ProjectResponse>("project_redo");
}

/**
 * Counts the working minutes between two moments on a calendar of the open
 * project (Rust: `calendar_working_minutes`). Read only. The Gantt chart
 * uses it when a bar's right edge is dragged, to turn the new finish date
 * into a duration (calendar maths stays in Rust, decision D16).
 *
 * @param calendarId - The calendar, or `null` for the project calendar.
 * @param start - First moment, e.g. "2027-03-01T08:00:00".
 * @param finish - Second moment (before `start` gives a negative number).
 * @returns A promise of the working minutes, or `null` in a browser.
 * @throws Rejects with an `IpcError` (`noProjectOpen`, `calendarNotFound`).
 *
 * @example
 * await calendarWorkingMinutes(null, "2027-03-01T08:00:00", "2027-03-05T17:00:00"); // 2400
 */
export async function calendarWorkingMinutes(
  calendarId: Uuid | null,
  start: IsoDateTime,
  finish: IsoDateTime,
): Promise<number | null> {
  if (!hasDesktopCore()) return null;
  return invoke<number>("calendar_working_minutes", { calendarId, start, finish });
}

/**
 * Finds the moment reached after some working time on a calendar of the
 * open project (Rust: `calendar_add_working_time`). `start` is first moved
 * to the next working moment. Read only. The Gantt chart uses it for the
 * finish date shown in the tooltip while a bar is dragged.
 *
 * @param calendarId - The calendar, or `null` for the project calendar.
 * @param start - Where to start counting.
 * @param minutes - Working minutes to add (0 = a milestone: the snapped start).
 * @returns A promise of the finish moment (ISO text), or `null` in a browser.
 * @throws Rejects with an `IpcError` (`noProjectOpen`, `calendarNotFound`,
 *   `noWorkingTime`, `dateOutOfRange`).
 *
 * @example
 * await calendarAddWorkingTime(null, "2027-03-01T08:00:00", 2400); // "2027-03-05T17:00:00"
 */
export async function calendarAddWorkingTime(
  calendarId: Uuid | null,
  start: IsoDateTime,
  minutes: number,
): Promise<IsoDateTime | null> {
  if (!hasDesktopCore()) return null;
  return invoke<IsoDateTime>("calendar_add_working_time", { calendarId, start, minutes });
}

/**
 * Replaces this window's project with a new, empty one (Rust: `file_new`).
 *
 * @param name - The project name, normally the translated "Untitled"
 *   (`t("file.untitledName")`). Rust uses "Untitled" if it is blank.
 * @returns A promise of the new project's `ProjectResponse`, or `null` in a browser.
 * @throws Rejects with an `IpcError` if the shell state is unusable.
 *
 * @example
 * await fileNew(t("file.untitledName"));
 */
export async function fileNew(name: string): Promise<ProjectResponse | null> {
  if (!hasDesktopCore()) return null;
  return invoke<ProjectResponse>("file_new", { name });
}

/**
 * Opens a `.gtra` file in this window (Rust: `file_open`).
 *
 * @param path - Full path of the file, usually from the Open dialog
 *   (`features/files/dialogs.ts`).
 * @returns A promise of the opened project's `ProjectResponse` (with
 *   `file.notes` filled in), or `null` in a browser.
 * @throws Rejects with an `IpcError` if the file cannot be opened. The
 *   project that was open stays open.
 *
 * @example
 * const opened = await fileOpen("/Users/sam/shed.gtra");
 * opened?.file.fileName; // "shed.gtra"
 */
export async function fileOpen(path: string): Promise<ProjectResponse | null> {
  if (!hasDesktopCore()) return null;
  return invoke<ProjectResponse>("file_open", { path });
}

/**
 * Saves this window's project to its current file (Rust: `file_save`).
 *
 * @returns A promise of the `ProjectResponse` after saving (`file.dirty`
 *   false), or `null` in a browser.
 * @throws Rejects if the project has no file yet or the file is read-only
 *   (call {@link fileSaveAs} instead), or if writing fails.
 *
 * @example
 * await fileSave();
 */
export async function fileSave(): Promise<ProjectResponse | null> {
  if (!hasDesktopCore()) return null;
  return invoke<ProjectResponse>("file_save");
}

/**
 * Saves this window's project to a new file (Rust: `file_save_as`). From
 * then on that file is the project's file.
 *
 * @param path - Where to save, usually from the Save As dialog. Rust adds
 *   ".gtra" if it is missing.
 * @returns A promise of the `ProjectResponse` after saving, or `null` in a browser.
 * @throws Rejects with an `IpcError` if writing fails; nothing changes then.
 *
 * @example
 * await fileSaveAs("/Users/sam/shed.gtra");
 */
export async function fileSaveAs(path: string): Promise<ProjectResponse | null> {
  if (!hasDesktopCore()) return null;
  return invoke<ProjectResponse>("file_save_as", { path });
}

/**
 * Reads the app settings and the start-up notice (Rust: `settings_get`).
 *
 * @returns A promise of the `SettingsResponse`, or `null` in a browser
 *   (the settings store then keeps its built-in defaults).
 * @throws Rejects with an `IpcError` (`settingsUnavailable`) if the shell
 *   state is broken.
 *
 * @example
 * const response = await settingsGet();
 * response?.settings.general.language; // "en"
 */
export async function settingsGet(): Promise<SettingsResponse | null> {
  if (!hasDesktopCore()) return null;
  return invoke<SettingsResponse>("settings_get");
}

/**
 * Stores new settings (Rust: `settings_set`). Rust cleans invalid values
 * and answers with the settings exactly as stored.
 *
 * @param settings - The whole settings object.
 * @returns A promise of the stored settings, or `null` in a browser.
 * @throws Rejects with an `IpcError` (`settingsSaveFailed`,
 *   `settingsNotStored`, `settingsUnavailable`).
 *
 * @example
 * const stored = await settingsSet({ ...settings, general: { ...settings.general, theme: "dark" } });
 */
export async function settingsSet(settings: Settings): Promise<Settings | null> {
  if (!hasDesktopCore()) return null;
  return invoke<Settings>("settings_set", { settings });
}

/**
 * Stores and returns the default settings (Rust: `settings_reset`).
 *
 * @returns A promise of the default settings, or `null` in a browser.
 * @throws Rejects with an `IpcError`, as {@link settingsSet}.
 *
 * @example
 * const defaults = await settingsReset();
 */
export async function settingsReset(): Promise<Settings | null> {
  if (!hasDesktopCore()) return null;
  return invoke<Settings>("settings_reset");
}

/**
 * Writes a recovery copy if the project has unsaved changes (Rust:
 * `file_autosave`). Called by the autosave timer
 * (`features/files/useAutosave.ts`). The user's file is never touched.
 *
 * @returns A promise of `true` if a copy was written, `false` if there was
 *   nothing to do, or `null` in a browser.
 * @throws Rejects with an `IpcError` (`gtra.*`) if the copy cannot be written.
 *
 * @example
 * await fileAutosave();
 */
export async function fileAutosave(): Promise<boolean | null> {
  if (!hasDesktopCore()) return null;
  return invoke<boolean>("file_autosave");
}

/**
 * Lists the recovery copies a crash left behind, newest first (Rust:
 * `recovery_list`).
 *
 * @returns A promise of the list (empty if none), or `null` in a browser.
 * @throws Rejects with an `IpcError` if the recovery folder cannot be read.
 *
 * @example
 * const copies = (await recoveryList()) ?? [];
 */
export async function recoveryList(): Promise<RecoveryCopyDto[] | null> {
  if (!hasDesktopCore()) return null;
  return invoke<RecoveryCopyDto[]>("recovery_list");
}

/**
 * Opens a recovery copy as this window's project (Rust: `recovery_restore`).
 * The restored project has unsaved changes (`file.dirty` is true).
 *
 * @param sessionId - The copy's `sessionId` from {@link recoveryList}.
 * @returns A promise of the restored `ProjectResponse`, or `null` in a browser.
 * @throws Rejects with an `IpcError` (`recoveryNotFound`, `gtra.*`).
 *
 * @example
 * await recoveryRestore(copies[0].sessionId);
 */
export async function recoveryRestore(sessionId: string): Promise<ProjectResponse | null> {
  if (!hasDesktopCore()) return null;
  return invoke<ProjectResponse>("recovery_restore", { sessionId });
}

/**
 * Deletes a recovery copy (Rust: `recovery_discard`).
 *
 * @param sessionId - The copy's `sessionId` from {@link recoveryList}.
 * @returns A promise that resolves when done (also in a browser).
 * @throws Rejects with an `IpcError` if the file cannot be deleted.
 *
 * @example
 * await recoveryDiscard(copy.sessionId);
 */
export async function recoveryDiscard(sessionId: string): Promise<void> {
  if (!hasDesktopCore()) return;
  await invoke("recovery_discard", { sessionId });
}

/**
 * Closes this window's project cleanly (Rust: `app_close_project`): its
 * working copy and recovery copy are deleted. Called right before the
 * window closes, after the unsaved-changes question.
 *
 * @returns A promise that resolves when done (also in a browser).
 * @throws Rejects with an `IpcError` (`projectUnavailable`).
 *
 * @example
 * await appCloseProject();
 * await getCurrentWindow().destroy();
 */
export async function appCloseProject(): Promise<void> {
  if (!hasDesktopCore()) return;
  await invoke("app_close_project");
}

/**
 * Closes every project cleanly and ends the app (Rust: `app_quit`). Called
 * after the quit question was answered with Save or Don't save.
 *
 * @returns A promise that normally never resolves (the app ends).
 *
 * @example
 * if (await resolveUnsavedChanges()) await appQuit();
 */
export async function appQuit(): Promise<void> {
  if (!hasDesktopCore()) return;
  await invoke("app_quit");
}

/**
 * Copies tasks to the system clipboard as tab-separated text (Rust:
 * `copy_tasks`), which spreadsheets and MS Project can paste.
 *
 * @param ids - The tasks to copy (written in row order).
 * @param labels - Column headings in the UI language for the header line.
 * @returns A promise that resolves when done (also in a browser, where
 *   nothing happens).
 * @throws Rejects with an `IpcError` (`noProjectOpen`, `clipboardUnavailable`).
 *
 * @example
 * await copyTasks([taskId], labels);
 */
export async function copyTasks(ids: readonly string[], labels: ColumnLabels): Promise<void> {
  if (!hasDesktopCore()) return;
  await invoke("copy_tasks", { ids, labels });
}

/**
 * Inserts the rows on the system clipboard as new tasks, as one undo step
 * (Rust: `paste_tasks`).
 *
 * @param atIndex - The row the first new task goes to (0 = first row).
 * @param level - The outline level of the first new task.
 * @param labels - Column headings in the UI language (English ones are
 *   always recognised too).
 * @returns A promise of the {@link PasteResponse}, or `null` in a browser.
 * @throws Rejects with an `IpcError` (`clipboardUnavailable`, or a document
 *   error such as `rejected` → `cycleDetected`; nothing is pasted then).
 *
 * @example
 * const pasted = await pasteTasks(3, 2, labels);
 * pasted?.warnings; // [{ code: "unreadableDuration", row: 2, text: "3x" }]
 */
export async function pasteTasks(
  atIndex: number,
  level: number,
  labels: ColumnLabels,
): Promise<PasteResponse | null> {
  if (!hasDesktopCore()) return null;
  return invoke<PasteResponse>("paste_tasks", { atIndex, level, labels });
}

/**
 * Replaces the native menu bar (`menu.rs`). Does nothing in a browser.
 *
 * @param menu - The menus, left to right (`features/toolbars/menuModel.ts`).
 * @throws Rejects with code `menuFailed` if the system refuses.
 */
export async function menuSet(menu: MenuNode[]): Promise<void> {
  if (hasDesktopCore()) await invoke("menu_set", { menu });
}

/**
 * Changes which menu entries are greyed out or ticked. Does nothing in a browser.
 *
 * @param states - The changed entries.
 * @throws Rejects with code `menuFailed` if the system refuses.
 */
export async function menuUpdate(states: MenuStateUpdate[]): Promise<void> {
  if (hasDesktopCore()) await invoke("menu_update", { states });
}
