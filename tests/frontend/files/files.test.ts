/**
 * tests/frontend/files/files.test.ts: tests for the file helpers (M1).
 *
 * What it checks:
 *   - `formatSavedAt` (`frontend/features/files/recovery.ts`): the time of a
 *     recovery copy, shown in the "Recover unsaved projects?" dialog, in the
 *     user's date pattern; missing or broken times give `null`.
 *   - `copyDisplayName`: which name the dialog shows for a recovery copy.
 *   - `toUnsavedChoice` (`frontend/features/files/dialogs.ts`): turns the
 *     answer of the "Save changes?" dialog into save / discard / cancel.
 *
 * Why it matters:
 *   The "Save / Don't save / Cancel" guard protects unsaved work. If an
 *   unexpected answer (the dialog closed with Esc) were read as "Don't
 *   save", the user would lose their changes. Cancel must be the default.
 *
 * Main ideas:
 *   These are plain functions, so they can be tested without a window. The
 *   Tauri plugins they sit next to are only imported, never called.
 */

import { describe, expect, it } from "vitest";
import { copyDisplayName, formatSavedAt } from "@frontend/features/files/recovery";
import { toUnsavedChoice, type UnsavedChangesTexts } from "@frontend/features/files/dialogs";
import type { RecoveryCopyDto } from "@frontend/ipc/types";

/** The dialog texts as the app shows them in English. */
const TEXTS: UnsavedChangesTexts = {
  title: "Unsaved changes",
  message: "Save the changes to Garden shed?",
  saveLabel: "Save",
  dontSaveLabel: "Don't save",
  cancelLabel: "Cancel",
};

describe("formatSavedAt", () => {
  // The computer's own time zone; the test uses a local time to be the same
  // everywhere.
  it("writes the local time with the date pattern", () => {
    const savedAt = new Date(2027, 1, 15, 9, 5).toISOString();
    expect(formatSavedAt(savedAt, "en|yyyy-mm-dd")).toBe("2027-02-15, 09:05");
    expect(formatSavedAt(savedAt, "en|dd mmm yyyy")).toBe("15 Feb 2027, 09:05");
  });

  // No time, or text that is not a time, gives null (the dialog then says
  // "unknown time").
  it("returns null for missing or invalid times", () => {
    expect(formatSavedAt(null, "en|yyyy-mm-dd")).toBeNull();
    expect(formatSavedAt("yesterday", "en|yyyy-mm-dd")).toBeNull();
  });
});

describe("copyDisplayName", () => {
  /** A recovery copy with every name field filled; tests remove some. */
  const copy: RecoveryCopyDto = {
    sessionId: "s-1",
    sourcePath: "/home/sam/shed.gtra",
    sourceFileName: "shed.gtra",
    savedAt: "2027-02-15T10:42:07Z",
    projectName: "Garden shed",
    readable: true,
  };

  // Project name first, then the file name, then "Untitled".
  it("prefers project name, then file name, then untitled", () => {
    expect(copyDisplayName(copy, "Untitled")).toBe("Garden shed");
    expect(copyDisplayName({ ...copy, projectName: null }, "Untitled")).toBe("shed.gtra");
    expect(copyDisplayName({ ...copy, projectName: null, sourceFileName: null }, "Untitled")).toBe(
      "Untitled",
    );
  });
});

describe("toUnsavedChoice", () => {
  // Some systems report "Yes"/"No"/"Cancel", others the button label.
  it("reads Yes/No and the button labels", () => {
    expect(toUnsavedChoice("Yes", TEXTS)).toBe("save");
    expect(toUnsavedChoice("Save", TEXTS)).toBe("save");
    expect(toUnsavedChoice("No", TEXTS)).toBe("discard");
    expect(toUnsavedChoice("Don't save", TEXTS)).toBe("discard");
    expect(toUnsavedChoice("Cancel", TEXTS)).toBe("cancel");
  });

  // Anything else (dialog closed, unknown answer) is "cancel": nothing is
  // lost.
  it("treats unknown answers as cancel", () => {
    expect(toUnsavedChoice("", TEXTS)).toBe("cancel");
    expect(toUnsavedChoice("Ok", TEXTS)).toBe("cancel");
    expect(toUnsavedChoice("save", TEXTS)).toBe("cancel");
  });

  // Translated labels work too (German buttons).
  it("works with translated labels", () => {
    const german = { ...TEXTS, saveLabel: "Speichern", dontSaveLabel: "Nicht speichern" };
    expect(toUnsavedChoice("Speichern", german)).toBe("save");
    expect(toUnsavedChoice("Nicht speichern", german)).toBe("discard");
  });
});
