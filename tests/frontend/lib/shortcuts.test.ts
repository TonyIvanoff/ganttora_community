/**
 * tests/frontend/lib/shortcuts.test.ts: tests for keyboard shortcuts.
 *
 * What it checks:
 *   `frontend/lib/shortcuts.ts`:
 *   - `matchShortcut` recognises Undo (Ctrl+Z / Cmd+Z) and Redo
 *     (Ctrl+Shift+Z / Cmd+Shift+Z / Ctrl+Y) and ignores everything else;
 *   - `isTextEntry` tells whether the key was pressed inside a text field,
 *     where Ctrl+Z must undo the *typing*, not the last project edit.
 *
 * Why it matters:
 *   Undo is the user's safety net. If Ctrl+Z inside a text box undid a
 *   project change instead of the typing, users would lose work without
 *   noticing.
 *
 * Main ideas:
 *   These tests run in Node, which has no web page and so no `HTMLElement`.
 *   To test `isTextEntry` we put small fake classes in their place with
 *   `vi.stubGlobal` ("stub" = a stand-in), and remove them after each test
 *   with `vi.unstubAllGlobals`.
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import { isTextEntry, matchShortcut, type ShortcutKeyEvent } from "@frontend/lib/shortcuts";

/** Builds a key event with no modifier keys, then applies `changes`. */
function key(k: string, changes: Partial<ShortcutKeyEvent> = {}): ShortcutKeyEvent {
  return { key: k, ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...changes };
}

/** Fake page element: only what `isTextEntry` looks at. */
class FakeElement {
  /** True for elements the user can type into directly (contenteditable). */
  isContentEditable = false;
}

/** Fake `<input>`; `type` is "text", "checkbox", "number" ... */
class FakeInput extends FakeElement {
  /** The input's type attribute. */
  type: string;
  constructor(type: string) {
    super();
    this.type = type;
  }
}

/** Fake `<textarea>`. */
class FakeTextArea extends FakeElement {}

/** Replaces the browser element classes with the fakes above. */
function useFakeDom(): void {
  vi.stubGlobal("HTMLElement", FakeElement);
  vi.stubGlobal("HTMLInputElement", FakeInput);
  vi.stubGlobal("HTMLTextAreaElement", FakeTextArea);
}

/** Treats a fake element as an event target (only for these tests). */
function target(element: FakeElement): EventTarget {
  return element as unknown as EventTarget;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("matchShortcut", () => {
  // Windows/Linux use Ctrl, macOS uses Cmd (metaKey).
  it("recognises undo with Ctrl+Z and Cmd+Z", () => {
    expect(matchShortcut(key("z", { ctrlKey: true }))).toBe("undo");
    expect(matchShortcut(key("z", { metaKey: true }))).toBe("undo");
  });

  // With Shift held, browsers report the key as capital "Z".
  it("recognises redo with Ctrl+Shift+Z and Cmd+Shift+Z", () => {
    expect(matchShortcut(key("Z", { ctrlKey: true, shiftKey: true }))).toBe("redo");
    expect(matchShortcut(key("Z", { metaKey: true, shiftKey: true }))).toBe("redo");
    expect(matchShortcut(key("z", { ctrlKey: true, shiftKey: true }))).toBe("redo");
  });

  // Ctrl+Y is the Windows redo; Cmd+Y on macOS means something else
  // (History in many apps), so it must not redo.
  it("recognises Ctrl+Y but not Cmd+Y or Ctrl+Shift+Y", () => {
    expect(matchShortcut(key("y", { ctrlKey: true }))).toBe("redo");
    expect(matchShortcut(key("Y", { ctrlKey: true }))).toBe("redo"); // Caps Lock
    expect(matchShortcut(key("y", { metaKey: true }))).toBeNull();
    expect(matchShortcut(key("Y", { ctrlKey: true, shiftKey: true }))).toBeNull();
  });

  // Without Ctrl/Cmd, with Alt, or with another letter: not a shortcut.
  it("ignores other keys and combinations", () => {
    expect(matchShortcut(key("z"))).toBeNull();
    expect(matchShortcut(key("z", { shiftKey: true }))).toBeNull();
    expect(matchShortcut(key("z", { ctrlKey: true, altKey: true }))).toBeNull();
    expect(matchShortcut(key("a", { ctrlKey: true }))).toBeNull();
    expect(matchShortcut(key("Enter", { ctrlKey: true }))).toBeNull();
  });

  // Inside a text field the browser's own undo must win.
  it("ignores shortcuts typed into a text field", () => {
    useFakeDom();
    const textBox = target(new FakeInput("text"));
    expect(matchShortcut(key("z", { ctrlKey: true, target: textBox }))).toBeNull();
    const checkbox = target(new FakeInput("checkbox"));
    expect(matchShortcut(key("z", { ctrlKey: true, target: checkbox }))).toBe("undo");
  });
});

describe("isTextEntry", () => {
  // No target, or no browser at all (Node): not a text field.
  it("is false without a target or without a DOM", () => {
    expect(isTextEntry(null)).toBe(false);
    expect(isTextEntry(undefined)).toBe(false);
    expect(isTextEntry({} as EventTarget)).toBe(false);
  });

  // Text-like inputs and text areas are text fields.
  it("is true for text inputs and text areas", () => {
    useFakeDom();
    for (const type of ["text", "number", "search", "email", "date", ""]) {
      expect(isTextEntry(target(new FakeInput(type))), `type "${type}"`).toBe(true);
    }
    expect(isTextEntry(target(new FakeTextArea()))).toBe(true);
  });

  // Buttons, checkboxes, sliders ... cannot be typed into.
  it("is false for non-text inputs", () => {
    useFakeDom();
    for (const type of [
      "checkbox",
      "radio",
      "button",
      "submit",
      "reset",
      "range",
      "color",
      "file",
    ]) {
      expect(isTextEntry(target(new FakeInput(type))), `type "${type}"`).toBe(false);
    }
  });

  // Any element with contenteditable is a text field; a plain one is not.
  it("checks contenteditable on other elements", () => {
    useFakeDom();
    const plain = new FakeElement();
    expect(isTextEntry(target(plain))).toBe(false);
    const editable = new FakeElement();
    editable.isContentEditable = true;
    expect(isTextEntry(target(editable))).toBe(true);
  });
});

describe("matchShortcut: Settings (M1)", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  // Ctrl+, (Windows, Linux) and Cmd+, (macOS) open Settings, the usual
  // shortcut in desktop apps.
  it("opens settings with Ctrl+, and Cmd+,", () => {
    expect(matchShortcut(key(",", { ctrlKey: true }))).toBe("openSettings");
    expect(matchShortcut(key(",", { metaKey: true }))).toBe("openSettings");
  });

  // A comma without Ctrl/Cmd is just typing; with Shift or Alt it is
  // another shortcut (or none).
  it("ignores a plain comma and other modifier mixes", () => {
    expect(matchShortcut(key(","))).toBeNull();
    expect(matchShortcut(key(",", { ctrlKey: true, shiftKey: true }))).toBeNull();
    expect(matchShortcut(key(",", { ctrlKey: true, altKey: true }))).toBeNull();
  });

  // Unlike Undo, the Settings shortcut also works while typing in a text
  // field (a text field has no "Ctrl+," of its own).
  it("works inside a text field", () => {
    useFakeDom();
    const field = target(new FakeInput("text"));
    expect(matchShortcut(key(",", { ctrlKey: true, target: field }))).toBe("openSettings");
    expect(matchShortcut(key("z", { ctrlKey: true, target: field }))).toBeNull();
  });
});

describe("matchShortcut: zoom (M1 wave 2)", () => {
  // "=" and "+" share a key on most keyboards ("+" needs Shift), so both
  // zoom in; "-" and "_" (Shift+minus) zoom out. Ctrl on Windows/Linux,
  // Cmd on macOS.
  it("zooms in with Ctrl/Cmd + = or +", () => {
    expect(matchShortcut(key("=", { ctrlKey: true }))).toBe("zoomIn");
    expect(matchShortcut(key("+", { ctrlKey: true, shiftKey: true }))).toBe("zoomIn");
    expect(matchShortcut(key("=", { metaKey: true }))).toBe("zoomIn");
  });

  it("zooms out with Ctrl/Cmd + - or _", () => {
    expect(matchShortcut(key("-", { ctrlKey: true }))).toBe("zoomOut");
    expect(matchShortcut(key("_", { ctrlKey: true, shiftKey: true }))).toBe("zoomOut");
    expect(matchShortcut(key("-", { metaKey: true }))).toBe("zoomOut");
  });

  // Without Ctrl/Cmd it is just typing; with Alt it is not ours.
  it("ignores the keys without Ctrl/Cmd or with Alt", () => {
    expect(matchShortcut(key("="))).toBeNull();
    expect(matchShortcut(key("-"))).toBeNull();
    expect(matchShortcut(key("=", { ctrlKey: true, altKey: true }))).toBeNull();
  });

  // A text field has no Ctrl+= of its own, so zoom works while typing too.
  it("works inside a text field", () => {
    useFakeDom();
    const field = target(new FakeInput("text"));
    expect(matchShortcut(key("=", { ctrlKey: true, target: field }))).toBe("zoomIn");
    expect(matchShortcut(key("-", { ctrlKey: true, target: field }))).toBe("zoomOut");
  });
});

describe("matchShortcut: Add blocker (D23)", () => {
  afterEach(() => vi.unstubAllGlobals());

  // Ctrl+Shift+B on Windows/Linux, Cmd+Shift+B on a Mac. Browsers report a
  // capital "B" with Shift; with Caps Lock on it can be a small "b".
  it("recognises Ctrl/Cmd+Shift+B", () => {
    for (const k of ["B", "b"]) {
      expect(matchShortcut(key(k, { ctrlKey: true, shiftKey: true }))).toBe("addBlocker");
      expect(matchShortcut(key(k, { metaKey: true, shiftKey: true }))).toBe("addBlocker");
    }
  });

  // A missing or extra modifier must not open the dialog by accident.
  it("ignores other combinations with B", () => {
    expect(matchShortcut(key("b"))).toBeNull();
    expect(matchShortcut(key("B", { shiftKey: true }))).toBeNull();
    expect(matchShortcut(key("b", { ctrlKey: true }))).toBeNull();
    expect(matchShortcut(key("B", { ctrlKey: true, shiftKey: true, altKey: true }))).toBeNull();
  });

  // While the user types in a text field, the key belongs to that field.
  it("does nothing inside a text field", () => {
    useFakeDom();
    const event = { ctrlKey: true, shiftKey: true, target: target(new FakeInput("text")) };
    expect(matchShortcut(key("B", event))).toBeNull();
  });
});
