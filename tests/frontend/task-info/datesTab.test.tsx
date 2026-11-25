// @vitest-environment happy-dom
/**
 * tests/frontend/task-info/datesTab.test.tsx: the Dates tab of the Task and
 * Project Information dialogs, drawn for real.
 *
 * What it checks: which fields a task, a group and a project show and which
 * of them can be edited, and that a typed start reaches Rust on OK.
 *
 * Why it matters: the dates of groups and projects come from their tasks.
 */

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

// Translations: `t(key)` returns the key itself.
vi.mock("react-i18next", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

import { TaskInfoDialog } from "@frontend/features/task-info/TaskInfoDialog";
import { useTaskInfoStore } from "@frontend/features/task-info/taskInfoStore";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import type { TaskRow } from "@frontend/ipc/types";
import { useAppStore } from "@frontend/state/store";
import { shedSnapshot, taskId } from "../gantt/helpers";

/** React's mount point. */
let root: Root;

beforeEach(() => vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true));

afterEach(() => {
  act(() => root.unmount());
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
  useTaskInfoStore.setState(useTaskInfoStore.getInitialState(), true);
  useSettingsStore.setState(useSettingsStore.getInitialState(), true);
});

/** Opens the Dates tab of row 2 with some of its fields changed. */
function openDates(changes: Partial<TaskRow>): void {
  const snapshot = shedSnapshot();
  const tasks = snapshot.tasks.map((task, i) => (i === 1 ? { ...task, ...changes } : task));
  useAppStore.setState({ ...useAppStore.getInitialState(), snapshot: { ...snapshot, tasks } });
  const { settings } = useSettingsStore.getState();
  useSettingsStore.setState({
    settings: { ...settings, general: { ...settings.general, dateFormat: "iso" } },
  });
  root = createRoot(document.body.appendChild(document.createElement("div")));
  act(() => root.render(<TaskInfoDialog />));
  act(() => useTaskInfoStore.getState().open(taskId(2), "dates"));
}

/** The field with a label, or `undefined`. */
function field(label: string): HTMLInputElement | undefined {
  const element = [...document.querySelectorAll("label")].find(
    (l) => l.textContent === `taskInfo.${label}`,
  );
  return element === undefined
    ? undefined
    : (document.getElementById(element.htmlFor) as HTMLInputElement);
}

it("shows a task's dates, editable", () => {
  openDates({ plannedFinish: "2027-03-09T17:00:00" });
  expect(document.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe(
    "taskInfo.tabs.dates",
  );
  expect(field("start")?.value).toMatch(/^2027-03-\d\d$/);
  expect(field("plannedFinish")?.value).toBe("2027-03-09");
  for (const name of ["start", "plannedFinish", "actualFinish", "deadline"]) {
    expect(field(name)?.disabled, name).toBe(false);
  }
  expect(document.querySelector("output")?.textContent).not.toBe("taskInfo.noDate");
});

it("only shows a group's start and plan", () => {
  openDates({ isSummary: true });
  expect(field("start")?.disabled).toBe(true);
  expect(field("plannedFinish")?.disabled).toBe(true);
  expect(field("actualFinish")).toBeUndefined();
  expect(field("deadline")).toBeDefined();
});

it("lets a project's start be edited and shows no constraint", () => {
  openDates({ isSummary: true, isProject: true });
  expect(field("start")?.disabled).toBe(false);
  expect(field("plannedFinish")?.disabled).toBe(true);
  for (const name of ["actualFinish", "constraintType", "deadline", "calendar"]) {
    expect(field(name), name).toBeUndefined();
  }
});

it("shows a typed start in the constraint and sends it on OK", async () => {
  openDates({});
  const sent = vi.fn(() => Promise.resolve(true));
  act(() => useAppStore.setState({ applyCommands: sent }));
  const input = field("start");
  act(() => {
    // React listens to the browser's own setter, so use it directly.
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(
      input,
      "2027-03-15",
    );
    input?.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(field("constraintType")?.value).toBe("startNoEarlierThan");
  expect(field("constraintDate")?.value).toBe("2027-03-15");
  await act(async () => {
    document
      .querySelector("form")
      ?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await Promise.resolve();
  });
  expect(sent).toHaveBeenCalledWith([
    {
      type: "setTaskConstraint",
      id: taskId(2),
      constraint: { type: "startNoEarlierThan", date: "2027-03-15T08:00:00" },
    },
  ]);
});
