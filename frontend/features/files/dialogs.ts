/**
 * features/files/dialogs.ts: shows the system's "Open" and "Save As" windows.
 *
 * What it is for:
 *   To open or save a project, the user picks a file in the operating
 *   system's own file window (the one every app uses). This file shows that
 *   window and gives back the chosen path, or `null` if the user cancelled.
 *   It also shows the small questions around files: "Save / Don't save /
 *   Cancel" for unsaved changes, a "Continue / Cancel" confirmation, and
 *   "Restore / Discard / Later" for crash-recovery copies.
 *   Reading and writing the file is *not* done here: the path is sent to
 *   Rust (`ipc/client.ts` → `backend/src-tauri/src/ipc/file.rs`).
 *
 * Where it fits:
 *   Called by the file actions in `features/files/useFileActions.ts`,
 *   which then hand the chosen path to the store, and by the close guard
 *   (`features/files/useCloseGuard.ts`). Uses the official Tauri dialog plugin
 *   (`@tauri-apps/plugin-dialog`, licence MIT OR Apache-2.0). Which dialogs
 *   the UI may show is allowed in
 *   `backend/src-tauri/capabilities/default.json` (`dialog:allow-open`,
 *   `dialog:allow-save`, and `dialog:allow-message` for the question).
 *
 * Main ideas:
 *   - Why the UI opens the dialog and not Rust: the Rust commands then only
 *     take a path, so they can be tested without any window, and waiting
 *     for the user does not block a Rust thread.
 *   - A *filter* limits the window to `.gtra` files. Its label ("Ganttora
 *     project") is user-visible text, so the caller passes it in from the
 *     translation file; this module contains no UI text.
 */

import {
  message as pluginMessage,
  open as pluginOpen,
  save as pluginSave,
} from "@tauri-apps/plugin-dialog";

/** How many system windows (Open, Save As, questions) are showing. */
let openDialogs = 0;

/**
 * Whether a system window is waiting for an answer. Menu entries then must
 * not act on the project behind it (`shell/dialogStore.ts`).
 *
 * @returns `true` while one is showing.
 */
export function isNativeDialogOpen(): boolean {
  return openDialogs > 0;
}

/**
 * Counts a dialog as open until its answer arrives.
 *
 * Beginner note: `<A extends unknown[], R>` makes this work for any function:
 * the result takes the same parameters (`A`) and gives the same answer (`R`).
 *
 * @param show - The plugin function that shows the window.
 * @returns A function that does the same and keeps the count.
 */
function counted<A extends unknown[], R>(show: (...args: A) => Promise<R>) {
  return async (...args: A): Promise<R> => {
    openDialogs += 1;
    try {
      return await show(...args);
    } finally {
      openDialogs -= 1;
    }
  };
}

/** The plugin's message window, counted. */
const message = counted(pluginMessage);
/** The plugin's Save As window, counted. */
const save = counted(pluginSave);

/** The extension of Ganttora project files, without the dot. */
export const GTRA_EXTENSION = "gtra";

/**
 * Shows the system "Open" window for one `.gtra` file.
 *
 * @param filterLabel - Translated name of the file type, e.g.
 *   `t("file.filterName")` = "Ganttora project".
 * @returns A promise of the chosen full path, or `null` if the user cancelled.
 * @throws Rejects if the dialog cannot be shown (e.g. permission missing).
 *
 * @example
 * const path = await pickFileToOpen(t("file.filterName"));
 * if (path !== null) await fileOpen(path);
 *
 * Beginner note: `multiple: false` and `directory: false` tell the plugin
 * we want exactly one file, so TypeScript knows the answer is
 * `string | null` (not a list).
 */
export async function pickFileToOpen(filterLabel: string): Promise<string | null> {
  // Wrapped here (not once above) so TypeScript keeps the exact answer type.
  return counted(() =>
    pluginOpen({
      multiple: false,
      directory: false,
      filters: [{ name: filterLabel, extensions: [GTRA_EXTENSION] }],
    }),
  )();
}

/**
 * Shows the system "Save As" window.
 *
 * @param filterLabel - Translated name of the file type (see {@link pickFileToOpen}).
 * @param suggestedName - File name proposed in the window, e.g. the
 *   current file name or "Garden shed.gtra".
 * @returns A promise of the chosen full path, or `null` if the user cancelled.
 *   (Rust adds ".gtra" if the path has no such extension.)
 * @throws Rejects if the dialog cannot be shown.
 *
 * @example
 * const path = await pickFileToSave(t("file.filterName"), "Garden shed.gtra");
 */
export async function pickFileToSave(
  filterLabel: string,
  suggestedName: string,
): Promise<string | null> {
  return save({
    defaultPath: suggestedName,
    filters: [{ name: filterLabel, extensions: [GTRA_EXTENSION] }],
  });
}

/** The translated texts of the "Save changes?" question. */
export interface UnsavedChangesTexts {
  /** Window title, e.g. "Unsaved changes". */
  title: string;
  /** The question, e.g. "Do you want to save the changes to “shed.gtra”?". */
  message: string;
  /** Label of the button that saves first, e.g. "Save". */
  saveLabel: string;
  /** Label of the button that throws the changes away, e.g. "Don't save". */
  dontSaveLabel: string;
  /** Label of the button that goes back to the project, e.g. "Cancel". */
  cancelLabel: string;
}

/**
 * The user's answer to the "Save changes?" question:
 * - `"save"`: save first, then continue;
 * - `"discard"`: continue without saving (the changes are lost);
 * - `"cancel"`: stop, keep working on the project.
 */
export type UnsavedChoice = "save" | "discard" | "cancel";

/**
 * Turns the button the dialog reports into an {@link UnsavedChoice}.
 *
 * The dialog plugin answers with "Yes" / "No" / "Cancel" on some systems
 * and with the *label* of the pressed button on others, so both are
 * accepted. Anything else (e.g. the window was closed) means "cancel",
 * the safe choice: nothing is lost.
 *
 * @param result - What `message()` resolved with.
 * @param texts - The labels that were shown.
 * @returns The choice.
 *
 * @example
 * toUnsavedChoice("Yes", texts);           // "save"
 * toUnsavedChoice("Don't save", texts);    // "discard" (label = texts.dontSaveLabel)
 * toUnsavedChoice("Cancel", texts);        // "cancel"
 */
export function toUnsavedChoice(result: string, texts: UnsavedChangesTexts): UnsavedChoice {
  if (result === "Yes" || result === texts.saveLabel) return "save";
  if (result === "No" || result === texts.dontSaveLabel) return "discard";
  return "cancel";
}

/**
 * Asks "Save the changes?" with three buttons (Save / Don't save / Cancel),
 * before a project with unsaved changes is closed or replaced (New, Open,
 * Open sample, closing the window).
 *
 * @param texts - The translated texts (see {@link UnsavedChangesTexts}).
 * @returns A promise of the user's {@link UnsavedChoice}.
 * @throws Rejects if the dialog cannot be shown.
 *
 * @example
 * const choice = await askSaveChanges(texts);
 * if (choice === "cancel") return;
 * if (choice === "save") await save();
 *
 * Beginner note: the three buttons map to the plugin's "Yes", "No" and
 * "Cancel" buttons with our own labels. The order and position of the
 * buttons follow each operating system's own rules.
 */
export async function askSaveChanges(texts: UnsavedChangesTexts): Promise<UnsavedChoice> {
  const result = await message(texts.message, {
    title: texts.title,
    kind: "warning",
    buttons: { yes: texts.saveLabel, no: texts.dontSaveLabel, cancel: texts.cancelLabel },
  });
  return toUnsavedChoice(result, texts);
}

/** The texts of a two-button "Continue / Cancel" question (all translated). */
export interface ConfirmTexts {
  /** Window title. */
  title: string;
  /** The question. */
  message: string;
  /** The button that goes ahead. */
  okLabel: string;
  /** The button that stops. */
  cancelLabel: string;
}

/**
 * Asks a yes-or-no question with two buttons, e.g. before a Save As that
 * leaves data out.
 *
 * @param texts - The translated texts.
 * @returns `true` if the user pressed the OK button; `false` for Cancel or
 *   when the window was closed (the safe choice).
 *
 * @example
 * if (await confirmAction({ title, message, okLabel, cancelLabel })) { … }
 */
export async function confirmAction(texts: ConfirmTexts): Promise<boolean> {
  const result = await message(texts.message, {
    title: texts.title,
    kind: "warning",
    buttons: { ok: texts.okLabel, cancel: texts.cancelLabel },
  });
  // As with `toUnsavedChoice`: some systems answer "Ok", others the label.
  return result === "Ok" || result === texts.okLabel;
}

/** The texts of the crash-recovery question (all translated). */
export interface RecoveryTexts {
  /** Window title. */
  title: string;
  /** The question, naming the project and the time of the copy. */
  message: string;
  /** Button: open the copy. */
  restoreLabel: string;
  /** Button: delete the copy. */
  discardLabel: string;
  /** Button: decide at the next start. */
  laterLabel: string;
}

/** The answer to the recovery question. */
export type RecoveryChoice = "restore" | "discard" | "later";

/**
 * Asks what to do with one recovery copy: Restore, Discard or Later.
 *
 * @param texts - The translated texts.
 * @returns The choice; closing the window counts as "later" (nothing is
 *   deleted, the question comes back at the next start).
 *
 * @example
 * const choice = await askRecovery(texts);
 */
export async function askRecovery(texts: RecoveryTexts): Promise<RecoveryChoice> {
  const result = await message(texts.message, {
    title: texts.title,
    kind: "info",
    buttons: { yes: texts.restoreLabel, no: texts.discardLabel, cancel: texts.laterLabel },
  });
  if (result === "Yes" || result === texts.restoreLabel) return "restore";
  if (result === "No" || result === texts.discardLabel) return "discard";
  return "later";
}

/**
 * Builds a file name to suggest in the Save As window.
 *
 * @param currentFileName - The current file name, if the project was saved
 *   before (e.g. "shed.gtra"), or `null`.
 * @param projectName - The project's name, e.g. "Garden shed".
 * @param fallbackName - Used when the project name is empty: the
 *   translated "Untitled" (`t("file.untitledName")`).
 * @returns The current file name if there is one, else the project name
 *   with characters that are not allowed in file names replaced by "_",
 *   plus ".gtra".
 *
 * @example
 * suggestFileName(null, "Garden shed", "Untitled");        // "Garden shed.gtra"
 * suggestFileName("shed.gtra", "Garden shed", "Untitled"); // "shed.gtra"
 * suggestFileName(null, "A/B: plan", "Untitled");          // "A_B_ plan.gtra"
 * suggestFileName(null, "  ", "Untitled");                 // "Untitled.gtra"
 */
export function suggestFileName(
  currentFileName: string | null,
  projectName: string,
  fallbackName: string,
): string {
  if (currentFileName !== null) return currentFileName;
  // These characters are forbidden in file names on Windows (and "/" on
  // every system). A *regular expression* (the /…/g pattern) finds them
  // all; `g` means "every match", not only the first.
  const safe = (name: string) => name.replace(/[\\/:*?"<>|]/g, "_").trim();
  const base = safe(projectName) === "" ? safe(fallbackName) : safe(projectName);
  return `${base}.${GTRA_EXTENSION}`;
}
