/**
 * features/grid/gridShortcuts.ts: recognises the task grid's keyboard
 * shortcuts.
 *
 * What it is for:
 *   Turns a key press inside the task grid into a *grid action* ("move one
 *   row down", "indent", "cut"…) or nothing. The grid component
 *   (`TaskGrid.tsx`) then only has to carry the action out. Keeping the
 *   "which key means what" decision in one pure function means the rules
 *   are written once and can be unit-tested without drawing the grid.
 *
 * Where it fits:
 *   Called by the key handler in `TaskGrid.tsx` while the grid (not a cell
 *   editor) has the focus. The window-wide shortcuts (Undo, Save, zoom…)
 *   are matched separately, in the same style, by `lib/shortcuts.ts`.
 *
 * Main ideas:
 *   - **A pure function.** {@link matchGridShortcut} looks only at the key
 *     press and the platform and returns a plain object. It changes
 *     nothing, so a test can call it with a hand-made object such as
 *     `{ key: "x", code: "KeyX", ctrlKey: true, … }`.
 *   - **Ctrl or Command.** macOS uses the Command key (⌘) where Windows and
 *     Linux use Ctrl. As in `lib/shortcuts.ts`, either one is accepted on
 *     every platform.
 *   - **The keys** (MS Project-like where possible):
 *     | Keys | Action |
 *     |---|---|
 *     | ↑ / ↓ (Shift: extend the selection) | previous / next row |
 *     | ← / → | previous / next column |
 *     | Home / End (with Ctrl/⌘: also first / last row) | first / last column |
 *     | Page Up / Page Down (Shift: extend) | one screen up / down |
 *     | Enter, F2 | edit the active cell |
 *     | a printable character | start editing with that character |
 *     | Shift+F2 | Task Information |
 *     | Shift+F10, the Menu key | the row's context menu |
 *     | Insert | insert a task |
 *     | Delete (macOS also Backspace) | delete the selected tasks |
 *     | Ctrl/⌘+C, X, V | copy, cut, paste rows |
 *     | Alt+Shift+→ / ← | indent / outdent |
 *     | Alt+Shift+↑ / ↓ | move the task up / down |
 *     | Alt+Shift+− / + | collapse / expand summaries |
 *   - **Why the platform matters.** Mac keyboards have no separate forward
 *     Delete key on most laptops; the key labelled "delete" sends
 *     Backspace. So on macOS Backspace also deletes the selected tasks, as
 *     in Finder or Numbers. Elsewhere Backspace does nothing in the grid.
 */

import type { ShortcutKeyEvent } from "@frontend/lib/shortcuts";

/** The operating system family, as far as keyboard habits differ. */
export type GridPlatform = "mac" | "other";

/**
 * The parts of a key press this module looks at. A real React or DOM
 * `KeyboardEvent` has all of them; tests can pass a plain object.
 *
 * Beginner note: `extends` means "everything `ShortcutKeyEvent` has, plus
 * the fields listed here".
 */
export interface GridKeyEvent extends ShortcutKeyEvent {
  /**
   * The *physical* key, e.g. "Minus", "Equal", "NumpadAdd". Used for the
   * collapse / expand keys, because the character they type (`key`)
   * differs between keyboard layouts and with Shift held.
   */
  code: string;
}

/**
 * A grid action found by {@link matchGridShortcut}.
 *
 * It is a *discriminated union*: every variant has an `action` name, and
 * some carry extra details (e.g. `extend` for Shift+arrow). A `switch` on
 * `action` lets TypeScript know which details exist in each case.
 */
export type GridShortcut =
  /** Indent the selected tasks one level (Alt+Shift+→). */
  | { action: "indent" }
  /** Outdent the selected tasks one level (Alt+Shift+←). */
  | { action: "outdent" }
  /** Move the active task above the previous block (Alt+Shift+↑). */
  | { action: "moveTaskUp" }
  /** Move the active task below the next block (Alt+Shift+↓). */
  | { action: "moveTaskDown" }
  /** Hide the subtasks of the selected summaries (Alt+Shift+−). */
  | { action: "collapse" }
  /** Show the subtasks of the selected summaries (Alt+Shift++). */
  | { action: "expand" }
  /** Copy the selected rows (Ctrl/⌘+C). */
  | { action: "copy" }
  /** Copy the selected rows, then delete them (Ctrl/⌘+X). */
  | { action: "cut" }
  /** Paste rows as new tasks (Ctrl/⌘+V). */
  | { action: "paste" }
  /** One row down (↓); `extend` = Shift held, grow the selection. */
  | { action: "rowDown"; extend: boolean }
  /** One row up (↑); `extend` = Shift held, grow the selection. */
  | { action: "rowUp"; extend: boolean }
  /** One column right (→). */
  | { action: "columnRight" }
  /** One column left (←). */
  | { action: "columnLeft" }
  /** First column (Home); `firstRow` = Ctrl/⌘ held, also go to the first row. */
  | { action: "home"; firstRow: boolean }
  /** Last column (End); `lastRow` = Ctrl/⌘ held, also go to the last row. */
  | { action: "end"; lastRow: boolean }
  /** One screen down (Page Down); `extend` = Shift held. */
  | { action: "pageDown"; extend: boolean }
  /** One screen up (Page Up); `extend` = Shift held. */
  | { action: "pageUp"; extend: boolean }
  /** Open Task Information for the active task (Shift+F2). */
  | { action: "taskInfo" }
  /** Open the active row's context menu (Shift+F10 or the Menu key). */
  | { action: "contextMenu" }
  /** Edit the active cell, starting with its current text (Enter, F2). */
  | { action: "edit" }
  /** Start editing the active cell with a typed character. */
  | { action: "type"; text: string }
  /** Insert a new task below the active row (Insert). */
  | { action: "insertTask" }
  /** Delete the selected tasks (Delete; on macOS also Backspace). */
  | { action: "deleteTasks" };

/**
 * Guesses the platform from the browser's description of itself.
 *
 * @returns `"mac"` on macOS (and iPhone / iPad), `"other"` everywhere else,
 *   including test environments without a `navigator`.
 *
 * @example
 * const platform = currentGridPlatform(); // "mac" on a MacBook
 */
export function currentGridPlatform(): GridPlatform {
  return typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.userAgent)
    ? "mac"
    : "other";
}

/**
 * Finds the grid action for a key press.
 *
 * @param event - The key press (see {@link GridKeyEvent}).
 * @param platform - `"mac"` or `"other"` (see {@link currentGridPlatform});
 *   decides whether Backspace deletes tasks.
 * @returns The action, or `null` when the key press means nothing in the
 *   grid (the browser may then handle it, e.g. Tab moves the focus on).
 *   The caller decides what is allowed right now (e.g. nothing that
 *   changes the project in a read-only view).
 *
 * @example
 * const key = { ctrlKey: false, metaKey: false, shiftKey: false, altKey: false };
 * matchGridShortcut({ ...key, key: "ArrowDown", code: "ArrowDown", shiftKey: true }, "other");
 * // { action: "rowDown", extend: true }
 * matchGridShortcut({ ...key, key: "x", code: "KeyX", metaKey: true }, "mac");
 * // { action: "cut" }
 * matchGridShortcut({ ...key, key: "Backspace", code: "Backspace" }, "other");
 * // null
 */
export function matchGridShortcut(
  event: GridKeyEvent,
  platform: GridPlatform,
): GridShortcut | null {
  const command = event.ctrlKey || event.metaKey;
  const { key } = event;

  // Alt+Shift (without Ctrl/⌘): MS Project's outline shortcuts. Any other
  // key with Alt+Shift means nothing here (and must not start typing).
  if (event.altKey && event.shiftKey && !command) {
    if (key === "ArrowRight") return { action: "indent" };
    if (key === "ArrowLeft") return { action: "outdent" };
    if (key === "ArrowUp") return { action: "moveTaskUp" };
    if (key === "ArrowDown") return { action: "moveTaskDown" };
    // `code` names the physical key, so this works on every layout.
    if (event.code === "Minus" || event.code === "NumpadSubtract") return { action: "collapse" };
    if (event.code === "Equal" || event.code === "NumpadAdd") return { action: "expand" };
    return null;
  }

  // Clipboard: Ctrl/⌘ + C, X, V. With Shift held the browser reports a
  // capital letter, so compare in lower case.
  if (command && !event.altKey) {
    const letter = key.toLowerCase();
    if (letter === "c") return { action: "copy" };
    if (letter === "x") return { action: "cut" };
    if (letter === "v") return { action: "paste" };
  }

  switch (key) {
    case "ArrowDown":
      return { action: "rowDown", extend: event.shiftKey };
    case "ArrowUp":
      return { action: "rowUp", extend: event.shiftKey };
    case "ArrowRight":
      return { action: "columnRight" };
    case "ArrowLeft":
      return { action: "columnLeft" };
    case "Home":
      return { action: "home", firstRow: command };
    case "End":
      return { action: "end", lastRow: command };
    case "PageDown":
      return { action: "pageDown", extend: event.shiftKey };
    case "PageUp":
      return { action: "pageUp", extend: event.shiftKey };
    case "F2":
      return event.shiftKey ? { action: "taskInfo" } : { action: "edit" };
    case "F10":
      // Shift+F10 is the usual keyboard way to open a context menu.
      return event.shiftKey && !command && !event.altKey ? { action: "contextMenu" } : null;
    case "ContextMenu":
      // The "Menu" key some keyboards have next to the right Ctrl.
      return { action: "contextMenu" };
    case "Enter":
      return { action: "edit" };
    case "Insert":
      return { action: "insertTask" };
    case "Delete":
      return { action: "deleteTasks" };
    case "Backspace":
      return platform === "mac" && !command && !event.altKey ? { action: "deleteTasks" } : null;
  }

  // A printable character (its `key` is one character long, unlike
  // "Enter" or "Tab"): start typing into the cell, like a spreadsheet.
  // Space is left out: it does nothing in the grid for now.
  if (key.length === 1 && !command && !event.altKey && key !== " ") {
    return { action: "type", text: key };
  }
  return null;
}
