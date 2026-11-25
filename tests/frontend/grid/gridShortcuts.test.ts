/**
 * tests/frontend/grid/gridShortcuts.test.ts: tests for the task grid's
 * keyboard shortcuts.
 *
 * What it checks:
 *   `matchGridShortcut` in `frontend/features/grid/gridShortcuts.ts` turns a
 *   key press into a grid action ("row down", "indent", "cut"…) or `null`.
 *   There is one test per action, plus the tricky cases:
 *   - Alt+Shift with a key that is not an outline key gives `null` (and
 *     must not start typing a letter into the cell);
 *   - Ctrl+Shift+C still copies (the browser reports a capital "C");
 *   - Space and Ctrl+letter do not start typing;
 *   - Backspace deletes tasks on macOS only;
 *   - Shift+F2 opens Task Information, plain F2 edits the cell;
 *   - Shift+F10 and the Menu key open the row menu, plain F10 does not;
 *   - Ctrl+Home / Ctrl+End also jump to the first / last row.
 *
 * Why it matters:
 *   Keyboard users work fast and trust the keys. If Backspace deleted tasks
 *   on Windows (where people press it to fix a typo), or Alt+Shift+K typed a
 *   "K" into a task name, users would damage their plan without noticing.
 *
 * Main ideas:
 *   `matchGridShortcut` is a *pure function*: it only looks at the key press
 *   we give it. So we can pass small hand-made objects instead of real
 *   keyboard events, and no window or grid is needed.
 */

import { describe, expect, it } from "vitest";
import { matchGridShortcut, type GridKeyEvent } from "@frontend/features/grid/gridShortcuts";

/**
 * Builds a key press with no modifier keys held, then applies `changes`.
 *
 * @param key - What the key types or its name, e.g. "a", "ArrowDown".
 * @param changes - Fields to override, e.g. `{ shiftKey: true }`.
 * @returns A plain object with every field `matchGridShortcut` reads.
 *   `code` (the physical key) defaults to `key`; tests that care about it
 *   set it explicitly.
 */
function press(key: string, changes: Partial<GridKeyEvent> = {}): GridKeyEvent {
  return {
    key,
    code: key,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    ...changes,
  };
}

/** Alt and Shift held together: the outline shortcuts. */
const ALT_SHIFT = { altKey: true, shiftKey: true } as const;

describe("matchGridShortcut: moving around", () => {
  // Plain arrows move one row; with Shift the selection grows instead.
  it("maps up and down arrows, with and without Shift", () => {
    expect(matchGridShortcut(press("ArrowDown"), "other")).toEqual({
      action: "rowDown",
      extend: false,
    });
    expect(matchGridShortcut(press("ArrowDown", { shiftKey: true }), "other")).toEqual({
      action: "rowDown",
      extend: true,
    });
    expect(matchGridShortcut(press("ArrowUp"), "other")).toEqual({
      action: "rowUp",
      extend: false,
    });
    expect(matchGridShortcut(press("ArrowUp", { shiftKey: true }), "other")).toEqual({
      action: "rowUp",
      extend: true,
    });
  });

  // Left and right move between columns.
  it("maps left and right arrows to columns", () => {
    expect(matchGridShortcut(press("ArrowRight"), "other")).toEqual({ action: "columnRight" });
    expect(matchGridShortcut(press("ArrowLeft"), "other")).toEqual({ action: "columnLeft" });
  });

  // Home / End go to the first / last column; with Ctrl or Cmd also to the
  // first / last row (like a spreadsheet).
  it("maps Home and End, and Ctrl/Cmd+Home/End to the first/last row", () => {
    expect(matchGridShortcut(press("Home"), "other")).toEqual({ action: "home", firstRow: false });
    expect(matchGridShortcut(press("Home", { ctrlKey: true }), "other")).toEqual({
      action: "home",
      firstRow: true,
    });
    expect(matchGridShortcut(press("Home", { metaKey: true }), "mac")).toEqual({
      action: "home",
      firstRow: true,
    });
    expect(matchGridShortcut(press("End"), "other")).toEqual({ action: "end", lastRow: false });
    expect(matchGridShortcut(press("End", { ctrlKey: true }), "other")).toEqual({
      action: "end",
      lastRow: true,
    });
  });

  // Page Up / Page Down move one screen; Shift extends the selection.
  it("maps Page Up and Page Down, with and without Shift", () => {
    expect(matchGridShortcut(press("PageDown"), "other")).toEqual({
      action: "pageDown",
      extend: false,
    });
    expect(matchGridShortcut(press("PageDown", { shiftKey: true }), "other")).toEqual({
      action: "pageDown",
      extend: true,
    });
    expect(matchGridShortcut(press("PageUp"), "other")).toEqual({
      action: "pageUp",
      extend: false,
    });
    expect(matchGridShortcut(press("PageUp", { shiftKey: true }), "other")).toEqual({
      action: "pageUp",
      extend: true,
    });
  });
});

describe("matchGridShortcut: editing", () => {
  // Enter and F2 open the cell editor with the cell's current text.
  it("maps Enter and F2 to edit", () => {
    expect(matchGridShortcut(press("Enter"), "other")).toEqual({ action: "edit" });
    expect(matchGridShortcut(press("F2"), "other")).toEqual({ action: "edit" });
  });

  // Shift+F2 is MS Project's "Task Information", not "edit the cell".
  it("maps Shift+F2 to Task Information", () => {
    expect(matchGridShortcut(press("F2", { shiftKey: true }), "other")).toEqual({
      action: "taskInfo",
    });
  });

  // A printable character starts editing with that character, including
  // capitals typed with Shift and digits.
  it("starts typing on a printable character", () => {
    expect(matchGridShortcut(press("a"), "other")).toEqual({ action: "type", text: "a" });
    expect(matchGridShortcut(press("A", { shiftKey: true }), "other")).toEqual({
      action: "type",
      text: "A",
    });
    expect(matchGridShortcut(press("5"), "mac")).toEqual({ action: "type", text: "5" });
  });

  // Space does nothing in the grid for now; it must not open the editor.
  it("does not start typing on Space", () => {
    expect(matchGridShortcut(press(" ", { code: "Space" }), "other")).toBeNull();
  });

  // Ctrl/Cmd + a letter is a command (e.g. Ctrl+B), never typing.
  it("does not start typing on Ctrl/Cmd+letter", () => {
    expect(matchGridShortcut(press("b", { ctrlKey: true }), "other")).toBeNull();
    expect(matchGridShortcut(press("b", { metaKey: true }), "mac")).toBeNull();
  });

  // Alt + a letter (menu accelerators on Windows) is not typing either.
  it("does not start typing on Alt+letter", () => {
    expect(matchGridShortcut(press("b", { altKey: true }), "other")).toBeNull();
  });

  // Named keys longer than one character (Tab, Escape) mean nothing here,
  // so the browser can handle them (Tab moves the focus on).
  it("returns null for Tab and Escape", () => {
    expect(matchGridShortcut(press("Tab"), "other")).toBeNull();
    expect(matchGridShortcut(press("Escape"), "other")).toBeNull();
  });
});

describe("matchGridShortcut: tasks", () => {
  // Insert adds a task below the active row.
  it("maps Insert to insert a task", () => {
    expect(matchGridShortcut(press("Insert"), "other")).toEqual({ action: "insertTask" });
  });

  // Delete removes the selected tasks on every platform.
  it("maps Delete to delete tasks on both platforms", () => {
    expect(matchGridShortcut(press("Delete"), "other")).toEqual({ action: "deleteTasks" });
    expect(matchGridShortcut(press("Delete"), "mac")).toEqual({ action: "deleteTasks" });
  });

  // On a Mac the key labelled "delete" sends Backspace, so it deletes tasks.
  // On Windows/Linux Backspace does nothing in the grid (people press it to
  // fix typing, and it must not delete a task by accident).
  it("maps Backspace to delete tasks on mac only", () => {
    expect(matchGridShortcut(press("Backspace"), "mac")).toEqual({ action: "deleteTasks" });
    expect(matchGridShortcut(press("Backspace"), "other")).toBeNull();
  });

  // Cmd+Backspace / Alt+Backspace are text shortcuts on a Mac, not delete.
  it("ignores Backspace with Cmd or Alt on mac", () => {
    expect(matchGridShortcut(press("Backspace", { metaKey: true }), "mac")).toBeNull();
    expect(matchGridShortcut(press("Backspace", { altKey: true }), "mac")).toBeNull();
  });
});

describe("matchGridShortcut: clipboard", () => {
  // Ctrl on Windows/Linux, Cmd on macOS; both are accepted everywhere.
  it("maps Ctrl/Cmd+C, X and V to copy, cut and paste", () => {
    expect(matchGridShortcut(press("c", { ctrlKey: true }), "other")).toEqual({ action: "copy" });
    expect(matchGridShortcut(press("c", { metaKey: true }), "mac")).toEqual({ action: "copy" });
    expect(matchGridShortcut(press("x", { ctrlKey: true }), "other")).toEqual({ action: "cut" });
    expect(matchGridShortcut(press("x", { metaKey: true }), "mac")).toEqual({ action: "cut" });
    expect(matchGridShortcut(press("v", { ctrlKey: true }), "other")).toEqual({ action: "paste" });
    expect(matchGridShortcut(press("v", { metaKey: true }), "mac")).toEqual({ action: "paste" });
  });

  // With Shift held the browser reports "C", not "c". Ctrl+Shift+C must
  // still copy (the comparison is in lower case).
  it("still copies with Ctrl+Shift+C", () => {
    expect(matchGridShortcut(press("C", { ctrlKey: true, shiftKey: true }), "other")).toEqual({
      action: "copy",
    });
  });

  // A plain "x" without Ctrl/Cmd is typing, not cut.
  it("treats a plain x as typing, not cut", () => {
    expect(matchGridShortcut(press("x"), "other")).toEqual({ action: "type", text: "x" });
  });
});

describe("matchGridShortcut: outline (Alt+Shift)", () => {
  // MS Project's outline keys: Alt+Shift+arrows indent/outdent/move.
  it("maps Alt+Shift+arrows to indent, outdent and move", () => {
    expect(matchGridShortcut(press("ArrowRight", ALT_SHIFT), "other")).toEqual({
      action: "indent",
    });
    expect(matchGridShortcut(press("ArrowLeft", ALT_SHIFT), "other")).toEqual({
      action: "outdent",
    });
    expect(matchGridShortcut(press("ArrowUp", ALT_SHIFT), "other")).toEqual({
      action: "moveTaskUp",
    });
    expect(matchGridShortcut(press("ArrowDown", ALT_SHIFT), "other")).toEqual({
      action: "moveTaskDown",
    });
  });

  // Collapse / expand use the *physical* key (`code`), because the
  // character typed differs by keyboard layout and with Shift held (on a US
  // keyboard Shift+"=" types "+", on a Mac Alt+Shift+"-" types "—").
  it("maps Alt+Shift+minus / plus by physical key to collapse / expand", () => {
    expect(matchGridShortcut(press("_", { ...ALT_SHIFT, code: "Minus" }), "other")).toEqual({
      action: "collapse",
    });
    expect(matchGridShortcut(press("—", { ...ALT_SHIFT, code: "Minus" }), "mac")).toEqual({
      action: "collapse",
    });
    expect(
      matchGridShortcut(press("-", { ...ALT_SHIFT, code: "NumpadSubtract" }), "other"),
    ).toEqual({ action: "collapse" });
    expect(matchGridShortcut(press("+", { ...ALT_SHIFT, code: "Equal" }), "other")).toEqual({
      action: "expand",
    });
    expect(matchGridShortcut(press("+", { ...ALT_SHIFT, code: "NumpadAdd" }), "other")).toEqual({
      action: "expand",
    });
  });

  // Any other key with Alt+Shift means nothing, and in particular must not
  // start typing the character into the cell.
  it("returns null for Alt+Shift with another key", () => {
    expect(matchGridShortcut(press("K", { ...ALT_SHIFT, code: "KeyK" }), "other")).toBeNull();
    expect(matchGridShortcut(press("Home", ALT_SHIFT), "other")).toBeNull();
    expect(matchGridShortcut(press("Delete", ALT_SHIFT), "other")).toBeNull();
  });

  // Ctrl+Alt+Shift+→ is not the outline shortcut (Ctrl turns it off); it
  // falls through to the plain arrow rule.
  it("does not indent when Ctrl is also held", () => {
    expect(
      matchGridShortcut(press("ArrowRight", { ...ALT_SHIFT, ctrlKey: true }), "other"),
    ).toEqual({ action: "columnRight" });
  });
});

describe("matchGridShortcut: context menu (blockers, D23)", () => {
  // Shift+F10 is the usual keyboard way to open a context menu. Plain F10
  // (the menu bar on Windows) and F10 with other modifiers must do nothing.
  it("maps Shift+F10 and only Shift+F10", () => {
    for (const platform of ["mac", "other"] as const) {
      expect(matchGridShortcut(press("F10", { shiftKey: true }), platform)).toEqual({
        action: "contextMenu",
      });
      expect(matchGridShortcut(press("F10"), platform)).toBeNull();
      for (const modifier of ["ctrlKey", "metaKey", "altKey"] as const) {
        expect(
          matchGridShortcut(press("F10", { shiftKey: true, [modifier]: true }), platform),
        ).toBeNull();
      }
    }
  });

  // The Menu key (between AltGr and Ctrl on many keyboards) does the same.
  it("maps the Menu key", () => {
    expect(matchGridShortcut(press("ContextMenu"), "other")).toEqual({ action: "contextMenu" });
    expect(matchGridShortcut(press("ContextMenu"), "mac")).toEqual({ action: "contextMenu" });
  });
});
