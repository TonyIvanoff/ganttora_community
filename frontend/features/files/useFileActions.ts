/**
 * features/files/useFileActions.ts: the File actions (New, Open, Save,
 * Save As, Open sample) as one React hook.
 *
 * What it is for:
 *   Each File action is a small sequence of steps: maybe ask "save
 *   unsaved changes?", maybe show a file window, then send the command to
 *   Rust. The toolbar buttons and the keyboard shortcuts both need these
 *   sequences, so they live here once.
 *
 * Where it fits:
 *   Used by `features/toolbars/useActions.ts` (menu, toolbars, shortcuts) and
 *   `shell/AppShell.tsx` (close guard). Uses `features/files/dialogs.ts` for
 *   the system windows and the store (`state/store.ts`) for the IPC calls.
 *   No file is read or written here; Rust does that (`ipc/file.rs`).
 *
 * Main ideas:
 *   - **Save falls back to Save As.** A project that was never saved has no
 *     path, and a read-only file must not be saved over. In both cases
 *     "Save" shows the Save As window instead, like other desktop apps.
 *   - **Unsaved changes are protected.** Before New, Open or Open sample
 *     replaces a project with unsaved changes, the user is asked "Save /
 *     Don't save / Cancel" (the same question guards closing the window,
 *     see `useCloseGuard.ts`).
 *   - **Save As on a read-only file** writes a new file with only what this
 *     version understands (Rust: `save_as_model_only`), so the user is
 *     asked to confirm first.
 *   - All texts come from `locales/en.json` through `t(…)`.
 */

import { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { hasDesktopCore } from "@frontend/ipc/client";
import { useAppStore } from "@frontend/state/store";
import {
  askSaveChanges,
  confirmAction,
  pickFileToOpen,
  pickFileToSave,
  suggestFileName,
} from "./dialogs";

/** The File actions returned by {@link useFileActions}. Each one returns a promise. */
export interface FileActions {
  /** Starts a new, empty project (asks first if there are unsaved changes). */
  newFile: () => Promise<void>;
  /** Shows the Open window and opens the chosen `.gtra` file. */
  openFile: () => Promise<void>;
  /** Saves to the current file, or shows Save As if there is none / it is read-only. */
  saveFile: () => Promise<void>;
  /** Shows the Save As window and saves to the chosen path. */
  saveFileAs: () => Promise<void>;
  /** Opens the built-in garden-shed sample project. */
  openSample: () => Promise<void>;
  /**
   * Asks "Save / Don't save / Cancel" if there are unsaved changes (and
   * saves if asked). Resolves `true` if the project may now be closed or
   * replaced, `false` if the user cancelled or saving did not finish.
   * Used by the close-window guard (`useCloseGuard.ts`).
   */
  resolveUnsavedChanges: () => Promise<boolean>;
  /** `false` in the browser preview (no Rust side): the buttons are then disabled. */
  available: boolean;
}

/**
 * Turns anything a rejected promise may carry into readable text.
 *
 * @param error - An `Error`, a string, or anything else.
 * @returns The message text.
 */
function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

/**
 * Gives the File actions, ready to call from buttons and shortcuts.
 *
 * @returns A {@link FileActions} object. The functions keep their identity
 *   between renders unless their inputs change, so they are safe to use in
 *   `useEffect` dependency lists.
 *
 * @example
 * const files = useFileActions();
 * <button onClick={() => void files.saveFile()}>{t("file.save")}</button>
 *
 * Beginner note: `useCallback(fn, [deps])` is a React hook that returns the
 * *same* function object on every render until something in `deps`
 * changes. Without it, a new function would be made on every render, and
 * effects that depend on it would re-run each time. `useMemo(() => value,
 * [deps])` does the same for any value; we use it where the value is a
 * function *built by* another function (`guarded(…)`).
 */
export function useFileActions(): FileActions {
  const { t } = useTranslation();
  const file = useAppStore((s) => s.file);
  const projectName = useAppStore((s) => s.snapshot?.projectName ?? "");
  const storeNewFile = useAppStore((s) => s.newFile);
  const storeOpenFile = useAppStore((s) => s.openFile);
  const storeSaveFile = useAppStore((s) => s.saveFile);
  const storeSaveFileAs = useAppStore((s) => s.saveFileAs);
  const openDemoProject = useAppStore((s) => s.openDemoProject);
  const showError = useAppStore((s) => s.showError);
  const available = hasDesktopCore();

  /**
   * Runs one action and shows any error from a dialog in the error banner.
   * (Errors from Rust are already shown by the store.)
   */
  const guarded = useCallback(
    (action: () => Promise<void>) => async () => {
      if (!available) return;
      try {
        await action();
      } catch (error: unknown) {
        showError(t("file.dialogError", { message: errorText(error) }));
      }
    },
    [available, showError, t],
  );

  const saveFileAs = useMemo(
    () =>
      guarded(async () => {
        const suggested = suggestFileName(
          file?.fileName ?? null,
          projectName,
          t("file.untitledName"),
        );
        // A read-only file (from a newer Ganttora) can only be saved as a
        // new file holding what this version understands; say so first.
        if (file?.readOnly === true) {
          const goAhead = await confirmAction({
            title: t("file.saveAsModelOnlyTitle"),
            message: t("file.saveAsModelOnlyMessage"),
            okLabel: t("file.saveAsModelOnlyContinue"),
            cancelLabel: t("file.unsavedCancel"),
          });
          if (!goAhead) return;
        }
        const path = await pickFileToSave(t("file.filterName"), suggested);
        if (path !== null) await storeSaveFileAs(path);
      }),
    [guarded, file, projectName, t, storeSaveFileAs],
  );

  const saveFile = useMemo(
    () =>
      guarded(async () => {
        if (file === null || file.path === null || file.readOnly) {
          await saveFileAs();
        } else {
          await storeSaveFile();
        }
      }),
    [guarded, file, saveFileAs, storeSaveFile],
  );

  /**
   * Resolves `true` if the open project may be closed or replaced: either
   * it has no unsaved changes, or the user answered the "Save / Don't save
   * / Cancel" question with Don't save, or with Save and the save worked.
   *
   * Save uses {@link saveFile}, so a never-saved project shows the Save As
   * window first. If the user cancels that window, or saving fails (the
   * reason is then in the error banner), the project still has unsaved
   * changes and the answer is `false`: nothing is lost.
   */
  const resolveUnsavedChanges = useCallback(async (): Promise<boolean> => {
    if (file === null || !file.dirty) return true;
    const choice = await askSaveChanges({
      title: t("file.unsavedTitle"),
      message: t("file.unsavedMessage", { name: file.fileName ?? projectName }),
      saveLabel: t("file.unsavedSave"),
      dontSaveLabel: t("file.unsavedDontSave"),
      cancelLabel: t("file.unsavedCancel"),
    });
    if (choice === "cancel") return false;
    if (choice === "discard") return true;
    await saveFile();
    // Read the store *now* (not the `file` from this render): the save
    // above has just replaced it with Rust's answer.
    return useAppStore.getState().file?.dirty === false;
  }, [file, projectName, t, saveFile]);

  /** Shorter name used by New / Open / Open sample below. */
  const mayReplace = resolveUnsavedChanges;

  const newFile = useMemo(
    () =>
      guarded(async () => {
        if (await mayReplace()) await storeNewFile(t("file.untitledName"));
      }),
    [guarded, mayReplace, storeNewFile, t],
  );

  const openFile = useMemo(
    () =>
      guarded(async () => {
        if (!(await mayReplace())) return;
        const path = await pickFileToOpen(t("file.filterName"));
        if (path !== null) await storeOpenFile(path);
      }),
    [guarded, mayReplace, storeOpenFile, t],
  );

  const openSample = useMemo(
    () =>
      guarded(async () => {
        if (await mayReplace()) await openDemoProject();
      }),
    [guarded, mayReplace, openDemoProject],
  );

  return { newFile, openFile, saveFile, saveFileAs, openSample, resolveUnsavedChanges, available };
}
