/**
 * lib/shortcuts.ts: recognises keyboard shortcuts.
 *
 * What it is for:
 *   Turns a key press into an *action name* ("undo", "fileSave", "zoomIn"…)
 *   or nothing. Keeping this decision in one pure function means the rules
 *   are written once, are easy to test, and the component that listens to
 *   the keyboard (`shell/AppShell.tsx`) stays short.
 *
 * Where it fits:
 *   These are the *window-wide* shortcuts: Undo, Redo, the four file
 *   shortcuts, Settings and timescale zoom. `shell/AppShell.tsx` listens on
 *   the whole window and calls {@link matchShortcut}. Shortcuts that only
 *   work while the task grid has the focus (arrows, Delete, copy, cut,
 *   paste, indent…) are matched by `features/grid/gridShortcuts.ts` in the
 *   same style. A full, MS Project-compatible list and a command palette
 *   that reads it come later.
 *
 * Main ideas:
 *   - macOS uses the Command key (⌘) where Windows and Linux use Ctrl.
 *     The browser reports them as `metaKey` and `ctrlKey`. We accept either,
 *     so the same code works everywhere.
 *   - Undo: Ctrl/Cmd+Z. Redo: Ctrl/Cmd+Shift+Z, and also Ctrl+Y (the usual
 *     redo key on Windows).
 *   - File: New Ctrl/Cmd+N, Open Ctrl/Cmd+O, Save Ctrl/Cmd+S,
 *     Save As Ctrl/Cmd+Shift+S.
 *   - Settings: Ctrl/Cmd+, (comma), the usual "Preferences" key on macOS
 *     and in many Windows/Linux apps.
 *   - Add blocker: Ctrl/Cmd+Shift+B ("B" for blocker) opens
 *     the "Add blocker" dialog for the selected task. Not while typing.
 *   - Timescale zoom: Ctrl/Cmd+= (or +) zooms in, Ctrl/Cmd+- zooms out.
 *     (In a browser these zoom the page; the app uses them for the Gantt
 *     timescale, as many planning tools do.)
 *   - When the user is typing in a text field, Ctrl/Cmd+Z must undo the
 *     *typing* (the field's own undo), not the last project edit. So Undo
 *     and Redo key presses inside text fields are ignored here.
 */

/** Actions that a shortcut can trigger. */
export type ShortcutAction =
  | "undo"
  | "redo"
  | "fileNew"
  | "fileOpen"
  | "fileSave"
  | "fileSaveAs"
  | "openSettings"
  | "zoomIn"
  | "zoomOut"
  | "addBlocker";

/**
 * The parts of a keyboard event this module looks at. A real
 * `KeyboardEvent` has all of them, and tests can pass a plain object.
 */
export interface ShortcutKeyEvent {
  /** The key, e.g. "z", "Z", "y". */
  key: string;
  /** Ctrl held down. */
  ctrlKey: boolean;
  /** Command (⌘, macOS) or Windows key held down. */
  metaKey: boolean;
  /** Shift held down. */
  shiftKey: boolean;
  /** Alt / Option held down. */
  altKey: boolean;
  /** Where the key press happened (the focused element), if known. */
  target?: EventTarget | null;
}

/**
 * Tells whether an element is a place where the user types text.
 *
 * @param target - The focused element (event target), or `null`.
 * @returns `true` for text inputs, number inputs, text areas and editable
 *   content; `false` for buttons, selects, the chart and everything else.
 *
 * @example
 * isTextEntry(document.querySelector("input[type=number]")); // true
 */
export function isTextEntry(target: EventTarget | null | undefined): boolean {
  // `instanceof` checks what kind of object something is. `typeof
  // HTMLElement` guards against environments without a DOM (tests in Node).
  if (typeof HTMLElement === "undefined" || !(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target instanceof HTMLTextAreaElement) return true;
  if (target instanceof HTMLInputElement) {
    // Checkboxes, radio buttons and buttons have no text to undo.
    const nonText = ["checkbox", "radio", "button", "submit", "reset", "range", "color", "file"];
    return !nonText.includes(target.type);
  }
  return false;
}

/**
 * Finds the action for a key press.
 *
 * @param event - The key press (see {@link ShortcutKeyEvent}).
 * @returns The action (`"undo"`, `"redo"`, `"fileNew"`, `"fileOpen"`,
 *   `"fileSave"`, `"fileSaveAs"`, `"openSettings"`, `"zoomIn"`,
 *   `"zoomOut"`, `"addBlocker"`) or `null` when the key press is not a
 *   shortcut. Undo, Redo and Add blocker are ignored inside text fields; the file, Settings and zoom
 *   shortcuts work everywhere (Ctrl+S while typing should still save).
 *   Alt held down never matches (Alt+letter types special characters on
 *   many keyboards).
 *
 * @example
 * matchShortcut({ key: "z", ctrlKey: true, metaKey: false, shiftKey: false, altKey: false });
 * // "undo"
 * matchShortcut({ key: "Z", ctrlKey: false, metaKey: true, shiftKey: true, altKey: false });
 * // "redo"
 * matchShortcut({ key: "S", ctrlKey: true, metaKey: false, shiftKey: true, altKey: false });
 * // "fileSaveAs"
 * matchShortcut({ key: "=", ctrlKey: false, metaKey: true, shiftKey: false, altKey: false });
 * // "zoomIn"
 */
export function matchShortcut(event: ShortcutKeyEvent): ShortcutAction | null {
  const command = event.ctrlKey || event.metaKey;
  if (!command || event.altKey) return null;
  // With Shift held, browsers report "Z" (capital); compare in lower case.
  const key = event.key.toLowerCase();
  // File shortcuts: Ctrl/Cmd+N, O, S and Shift+S.
  if (key === "n" && !event.shiftKey) return "fileNew";
  if (key === "o" && !event.shiftKey) return "fileOpen";
  if (key === "s") return event.shiftKey ? "fileSaveAs" : "fileSave";
  // Settings: Ctrl/Cmd+, (works while typing too, like the file shortcuts).
  if (key === "," && !event.shiftKey) return "openSettings";
  // Zoom: "=" and "+" share a key on most keyboards ("+" needs Shift).
  if (key === "=" || key === "+") return "zoomIn";
  if (key === "-" || key === "_") return "zoomOut";
  // From here on: Undo / Redo, which belong to the text field while typing.
  if (isTextEntry(event.target)) return null;
  if (key === "z") return event.shiftKey ? "redo" : "undo";
  if (key === "y" && event.ctrlKey && !event.shiftKey) return "redo";
  if (key === "b" && event.shiftKey) return "addBlocker";
  return null;
}
