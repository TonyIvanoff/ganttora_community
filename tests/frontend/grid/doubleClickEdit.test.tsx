// @vitest-environment happy-dom
/**
 * tests/frontend/grid/doubleClickEdit.test.tsx: editing a cell with a
 * Ctrl/⌘+click, and the double-click that opens the information dialog.
 *
 * What it checks (`frontend/features/grid/TaskGrid.tsx`):
 *   - Ctrl/⌘+clicking a sub-task's Start cell opens a date field, and the
 *     chosen day is sent to Rust as "start no earlier than";
 *   - double-clicking any cell opens Task Information.
 *
 * Why it matters:
 *   Users look for editing where they see the value: in the cell.
 *
 * Main ideas:
 *   happy-dom does not lay out the page (every size is 0), so the test says
 *   how tall the grid is; otherwise the grid would draw no rows.
 */

import { act, createRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

// Translations: `t(key)` returns the key itself.
vi.mock("react-i18next", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

// The bridge to Rust: only the calls we need are faked.
vi.mock("@frontend/ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@frontend/ipc/client")>()),
  projectApply: vi.fn(() => new Promise(() => undefined)),
}));

import { TaskGrid } from "@frontend/features/grid/TaskGrid";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import * as taskInfoStore from "@frontend/features/task-info/taskInfoStore";
import { IS_MAC } from "@frontend/features/toolbars/useActions";
import { projectApply } from "@frontend/ipc/client";
import { useAppStore } from "@frontend/state/store";
import { shedSnapshot, taskId } from "../gantt/helpers";

const { useTaskInfoStore } = taskInfoStore;

/** React's mount point. */
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(1200);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(
    new DOMRect(0, 0, 1200, 600),
  );
  // Row 2 is a sub-task of row 1.
  const snapshot = shedSnapshot();
  const tasks = snapshot.tasks.map((task, i) =>
    i === 0 ? { ...task, isSummary: true } : i === 1 ? { ...task, outlineLevel: 2 } : task,
  );
  useAppStore.setState({ ...useAppStore.getInitialState(), snapshot: { ...snapshot, tasks } });
  useAppStore.setState({ isDemoData: false });
  root = createRoot(document.body.appendChild(document.createElement("div")));
  act(() => root.render(<TaskGrid ganttContainerRef={createRef<HTMLDivElement>()} />));
});

afterEach(() => {
  act(() => root.unmount());
  document.body.innerHTML = "";
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  useTaskInfoStore.setState(useTaskInfoStore.getInitialState(), true);
});

/** Double-clicks a cell as a mouse does: press, then the double-click. */
function doubleClick(row: number, column: string): void {
  const cell = document.querySelector(`[id$="-r${row - 1}-${column}"]`);
  act(() => void cell?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true })));
  act(() => void cell?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })));
}

/** Ctrl+clicks (a Mac: ⌘+clicks) a cell: press, then click. */
function editClick(row: number, column: string): void {
  const cell = document.querySelector(`[id$="-r${row - 1}-${column}"]`);
  const init = { bubbles: true, ctrlKey: !IS_MAC, metaKey: IS_MAC };
  act(() => void cell?.dispatchEvent(new MouseEvent("mousedown", init)));
  act(() => void cell?.dispatchEvent(new MouseEvent("click", init)));
}

it("sets a sub-task's start date from its cell", async () => {
  editClick(2, "start");
  const field = document.querySelector<HTMLInputElement>("input[data-date-field]");
  expect(field).not.toBeNull();
  expect(useTaskInfoStore.getState().taskId).toBeNull();
  if (field === null) return;
  act(() => {
    // React listens to the browser's own setter, so use it directly.
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(
      field,
      "2027-03-15",
    );
    field.dispatchEvent(new Event("input", { bubbles: true }));
    field.dispatchEvent(new Event("change", { bubbles: true }));
  });
  act(
    () => void field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })),
  );
  // The store queues commands, so the call comes a moment later.
  await vi.waitFor(() =>
    expect(projectApply).toHaveBeenCalledWith({
      type: "setTaskConstraint",
      id: taskId(2),
      constraint: { type: "startNoEarlierThan", date: "2027-03-15T08:00:00" },
    }),
  );
});

it("shows the date in the format of the settings and takes a day from the calendar", () => {
  const sent = vi.fn(() => Promise.resolve(true));
  act(() => useAppStore.setState({ applyCommands: sent }));
  const { settings } = useSettingsStore.getState();
  useSettingsStore.setState({
    settings: { ...settings, general: { ...settings.general, dateFormat: "iso" } },
  });
  editClick(2, "start");
  const field = document.querySelector<HTMLInputElement>("input[data-date-field]");
  expect(field?.value).toMatch(/^2027-03-\d\d$/);
  act(() => field?.click());
  act(() => document.querySelector<HTMLButtonElement>('[data-day="2027-03-16"]')?.click());
  expect(sent).toHaveBeenCalledWith([
    {
      type: "setTaskConstraint",
      id: taskId(2),
      constraint: { type: "startNoEarlierThan", date: "2027-03-16T08:00:00" },
    },
  ]);
  useSettingsStore.setState({ settings });
});

it("adds a row to the selection with the same click on its ID", () => {
  editClick(2, "rowNumber");
  editClick(3, "rowNumber");
  expect(document.querySelector("input")).toBeNull();
  expect(useAppStore.getState().selectedTaskIds).toEqual([taskId(2), taskId(3)]);
});

it.each(["rowNumber", "name", "start"])("opens Task Information from the %s cell", (column) => {
  doubleClick(2, column);
  expect(document.querySelector("input")).toBeNull();
  expect(useTaskInfoStore.getState().taskId).toBe(taskId(2));
});

it.each([
  ["name", "general"],
  ["start", "advanced"],
  ["finish", "advanced"],
  ["predecessors", "predecessors"],
  ["duration", "general"],
])("opens the tab of the %s column: %s", (column, tab) => {
  doubleClick(2, column);
  expect(useTaskInfoStore.getState().tab).toBe(tab);
});

it("opens the tab of what the indicators show", () => {
  const { infoTabOf } = taskInfoStore;
  const task = shedSnapshot().tasks[1]!;
  const tab = (changes: object) =>
    infoTabOf("indicators", { ...task, ...changes }, [], Date.UTC(2027, 1, 1));
  const plain = { blockers: [], risks: [], notes: "", missedDeadline: false };
  expect(tab({ ...plain, constraint: { type: "asSoonAsPossible" } })).toBe("general");
  expect(tab({ ...plain, constraint: { type: "asSoonAsPossible" }, notes: "x" })).toBe("notes");
  expect(tab({ ...plain, missedDeadline: true })).toBe("advanced");
  expect(tab({ ...plain, risks: [{}] })).toBe("risks");
  expect(tab({ ...plain, risks: [{}], blockers: [{}] })).toBe("blockers");
});
