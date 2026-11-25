/**
 * tests/frontend/grid/projectRows.test.ts: what tells rows apart: the
 * projects' background colours, the percentages to choose from, and which
 * rows can become tasks or sub-tasks.
 */

import { standardWeek } from "@frontend/features/calendars/workingTime";
import { progressProblem } from "@frontend/features/grid/gridFormat";
import {
  higherPriority,
  missedStartPriority,
  missedStarts,
} from "@frontend/features/task-info/risks";
import type { Calendar } from "@frontend/ipc/types";
import { taskRow } from "./helpers";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import "@frontend/i18n";
import { IndicatorsCell } from "@frontend/features/grid/GridCell";
import { infoTabOf } from "@frontend/features/task-info/taskInfoStore";
import { describe, expect, it } from "vitest";
import { DEFAULT_COLUMN_WIDTHS, resolveColumnWidths } from "@frontend/features/grid/columnWidths";
import { percentChoices } from "@frontend/features/grid/gridFormat";
import { rowsFromSnapshot } from "@frontend/features/grid/gridRows";
import { canBecomeSubtasks, canBecomeTasks } from "@frontend/features/grid/outline";
import { projectTint } from "@frontend/lib/projectTint";
import { projectBlockers } from "@frontend/features/task-info/blockers";
import { EMPTY_RISK, taskRisks } from "@frontend/features/task-info/risks";
import { shownProject } from "@frontend/lib/shownProject";
import type { TaskRow } from "@frontend/ipc/types";
import { shedSnapshot } from "../gantt/helpers";

/** Two projects: House (two tasks, the second with a sub-task) and Garage (one task). */
function twoProjects() {
  const snapshot = shedSnapshot();
  const base = snapshot.tasks[0] as TaskRow;
  const row = (id: string, outlineLevel: number, isProject = false): TaskRow => ({
    ...base,
    id,
    outlineLevel,
    isProject,
    isSummary: isProject,
  });
  const tasks = [
    row("house", 1, true),
    row("a", 2),
    row("b", 2),
    row("b1", 3),
    row("garage", 1, true),
    row("c", 2),
  ];
  return { ...snapshot, tasks };
}

describe("project colours", () => {
  it("give every row its project's number, also when a project is collapsed", () => {
    const numbers = (collapsed: string[]) =>
      rowsFromSnapshot(twoProjects(), new Set(collapsed)).map((r) => r.projectIndex);
    expect(numbers([])).toEqual([0, 0, 0, 0, 1, 1]);
    expect(numbers(["house"])).toEqual([0, 1, 1]);
  });

  it("are different for many projects", () => {
    const colors = Array.from({ length: 50 }, (_, i) => projectTint(i, "0.1"));
    expect(new Set(colors).size).toBe(50);
    expect(colors[0]).toBe("rgba(32, 223, 96, 0.1)");
  });
});

describe("the project at the top of the window", () => {
  const name = (id: string | null) => shownProject(twoProjects().tasks, id)?.id;
  it("is the first one before any click", () => {
    expect(name(null)).toBe("house");
    expect(name("deleted")).toBe("house");
  });
  it("is the one that holds the clicked row", () => {
    expect(name("b1")).toBe("house");
    expect(name("garage")).toBe("garage");
    expect(name("c")).toBe("garage");
  });
});

describe("project blockers", () => {
  it("are those of the project's own tasks", () => {
    const blocker = {
      id: "x",
      afterWorkMinutes: 0,
      durationMinutes: 480,
      reason: "Rain",
      reportedOn: null,
      start: null,
      finish: null,
    };
    const tasks = twoProjects().tasks.map((t) =>
      t.id === "b1" ? { ...t, name: "Roof", blockers: [blocker] } : t,
    );
    expect(projectBlockers(tasks, "house")).toEqual([{ taskName: "Roof", blocker }]);
    expect(projectBlockers(tasks, "garage")).toEqual([]);
  });
});

describe("task risks of a project", () => {
  it("are those of its tasks, without the project's own", () => {
    const risk = (id: string) => ({ id, ...EMPTY_RISK });
    const tasks = twoProjects().tasks.map((t) =>
      t.id === "house" || t.id === "b1" ? { ...t, name: t.id, risks: [risk(t.id)] } : t,
    );
    expect(taskRisks(tasks, "house")).toEqual([{ taskName: "b1", risk: risk("b1") }]);
    expect(taskRisks(tasks, "garage")).toEqual([]);
  });
});

describe("percentages", () => {
  it("are the fixed choices", () => {
    expect(percentChoices("30")).toEqual([0, 10, 25, 30, 50, 75, 90, 100]);
  });

  it("keep another value that is already set", () => {
    expect(percentChoices("40")).toEqual([0, 10, 25, 40, 50, 75, 90, 100]);
  });
});

describe("converting rows", () => {
  const { tasks } = twoProjects();

  it("makes sub-tasks only of tasks with a task above", () => {
    expect(canBecomeSubtasks(tasks, ["b"])).toBe(true);
    expect(canBecomeSubtasks(tasks, ["a"])).toBe(false); // first in its project
    expect(canBecomeSubtasks(tasks, ["garage"])).toBe(false);
    expect(canBecomeSubtasks(tasks, [])).toBe(false);
  });

  it("makes tasks only of sub-tasks", () => {
    expect(canBecomeTasks(tasks, ["b1"])).toBe(true);
    expect(canBecomeTasks(tasks, ["b"])).toBe(false);
    expect(canBecomeTasks(tasks, ["b1", "a"])).toBe(false);
  });
});

describe("indicator column width", () => {
  it("replaces the old default but keeps the user's own width", () => {
    const width = (saved: number) => resolveColumnWidths({ indicators: saved }, {}).indicators;
    expect(width(40)).toBe(DEFAULT_COLUMN_WIDTHS.indicators);
    expect(width(60)).toBe(60);
  });
});

describe("the indicators of a project", () => {
  const rows = [
    taskRow({ id: "house", name: "House", isProject: true, isSummary: true, notes: "Call Alex" }),
    taskRow({
      id: "paint",
      name: "Paint",
      outlineLevel: 2,
      risks: [
        {
          id: "r",
          description: "Rain",
          priority: "high",
          response: "approved",
          mitigation: "",
          consequence: "",
          noConsequence: true,
        },
      ],
      blockers: [
        {
          id: "b",
          startDate: "2027-03-01",
          durationMinutes: 480,
          reason: "No paint",
          reportedOn: null,
        } as never,
      ],
    }),
  ];
  const html = (index: number) =>
    renderToStaticMarkup(
      createElement(IndicatorsCell, {
        row: { task: rows[index] } as never,
        rows,
        dateLocale: "en",
        today: Date.UTC(2027, 2, 2),
      }),
    );

  it("show the risks and blockers of its tasks and its own note", () => {
    expect(html(0)).toMatch(/Tasks:\n• Paint – Risk \([^]*Rain/);
    expect(html(0)).toMatch(/Paint – Approved risk/);
    expect(html(0)).toMatch(/Paint – Blocker.*No paint/);
    expect(html(0)).toContain("Note: Call Alex");
    // The task itself does not repeat its name.
    expect(html(1)).toContain("Rain");
    expect(html(1)).not.toContain("Paint –");
  });

  it("do not say late to start; the task does", () => {
    expect(html(0)).not.toContain("Should have started");
    expect(html(1)).toContain("Should have started");
  });

  it("separate the project's issues from those of its tasks", () => {
    const risk = rows[1]!.risks![0]!;
    const project = { ...rows[0]!, risks: [{ ...risk, id: "p", description: "Budget" }] };
    const text = renderToStaticMarkup(
      createElement(IndicatorsCell, {
        row: { task: project } as never,
        rows: [project, rows[1]!],
        dateLocale: "en",
        today: Date.UTC(2027, 1, 1),
      }),
    );
    expect(text).toMatch(
      /Project:\n• Risk[^\n]*Budget[^\n]*\n─+\nTasks:\n• Paint – Risk[^\n]*Rain/,
    );
    expect(text).toMatch(/Project:\n• Approved[^\n]*Budget[^\n]*\n─+\nTasks:\n• Paint – Approved/);
  });

  it("count a missed start as a risk, high on a critical task", () => {
    expect(html(0)).toContain("Paint – Risk (Medium): the start date is missed");
    const critical = { ...rows[1]!, isCritical: true };
    expect(missedStartPriority(critical, Date.UTC(2027, 2, 2))).toBe("high");
    expect(
      missedStartPriority({ ...critical, percentComplete: 10 }, Date.UTC(2027, 2, 2)),
    ).toBeNull();
    expect(missedStarts(rows, "house", Date.UTC(2027, 2, 2)).map((m) => m.task.name)).toEqual([
      "Paint",
    ]);
    expect(higherPriority("low", "high")).toBe("high");
    expect(higherPriority(null, "low")).toBe("low");
  });

  it("lead to the tab with the tasks' blockers", () => {
    expect(infoTabOf("indicators", rows[0]!, rows)).toBe("blockers");
    expect(infoTabOf("indicators", rows[0]!, rows.slice(0, 1), Date.UTC(2027, 1, 1))).toBe("notes");
  });
});

describe("progress compared with the dates", () => {
  const calendar: Calendar = {
    id: "cal",
    name: "Standard",
    week: standardWeek(),
    holidays: [],
  } as never;
  const day = (d: number) => Date.UTC(2026, 10, d);
  // Tue 17 – Wed 25 Nov 2026: seven working days.
  const problem = (percentComplete: number, today: number, changes = {}) =>
    progressProblem(
      taskRow({
        start: "2026-11-17T08:00:00",
        finish: "2026-11-25T17:00:00",
        percentComplete,
        ...changes,
      }),
      [calendar],
      "cal",
      day(today),
    );

  it("finds a task that is behind", () => {
    expect(problem(10, 23)).toEqual({ kind: "behind", actual: 10, expected: 57 });
    expect(problem(50, 23)).toBeNull(); // within the tolerance
    expect(problem(10, 17)).toBeNull(); // today's work still goes on
    expect(problem(0, 23)).toBeNull(); // "late to start" says it
  });

  it("finds an overdue task", () => {
    expect(problem(90, 26)?.kind).toBe("overdue");
    expect(problem(0, 26)?.kind).toBe("overdue");
    expect(problem(100, 26)).toBeNull();
  });

  it("finds a milestone that was not reached", () => {
    const milestone = { isMilestone: true, start: "2026-11-25T17:00:00" };
    expect(problem(0, 26, milestone)?.kind).toBe("milestone");
    expect(problem(0, 25, milestone)).toBeNull();
    expect(problem(100, 26, milestone)).toBeNull();
    const late = taskRow({ ...milestone, isMilestone: true, percentComplete: 0 });
    expect(missedStartPriority(late, day(30))).toBeNull();
  });

  it("leaves out blocked days, projects and groups", () => {
    const segments = [
      {
        kind: { type: "blocked", blocker: "b" },
        start: "2026-11-18T08:00:00",
        finish: "2026-11-20T17:00:00",
      },
    ];
    expect(problem(10, 23, { segments })?.expected).toBe(25); // 1 of 4 days
    expect(problem(10, 23, { isSummary: true })).toBeNull();
    expect(problem(10, 23, { isProject: true })).toBeNull();
  });
});
