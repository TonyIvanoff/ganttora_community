/**
 * tests/frontend/gantt/model.test.ts: tests for the Gantt model and the
 * snapshot conversion.
 *
 * What it checks:
 *   - `snapshotToGanttTasks` (`frontend/features/gantt/fromSnapshot.ts`)
 *     turns the `ScheduleSnapshot` from Rust into the renderer's input.
 *   - `prepareGanttModel` (`frontend/features/gantt/renderer/model.ts`)
 *     turns that input into rows with millisecond dates, links as row
 *     numbers, the date range and the number of critical tasks.
 *
 * Why it matters:
 *   This is the bridge from the Rust schedule to the picture. A field read
 *   under the wrong name, or a link to the wrong row, would draw a wrong
 *   plan without any error message.
 */

import { describe, expect, it } from "vitest";
import { snapshotToGanttTasks } from "@frontend/features/gantt/fromSnapshot";
import { prepareGanttModel } from "@frontend/features/gantt/renderer/model";
import { shedSnapshot, shedTasks, taskId } from "./helpers";

describe("snapshotToGanttTasks", () => {
  // Every row keeps its ID, name, critical flag, predecessors, summary and
  // milestone flags, and uses the *scheduled* dates (`start` / `finish`).
  it("maps each snapshot row to a Gantt task, in order", () => {
    const tasks = snapshotToGanttTasks(shedSnapshot());
    expect(tasks).toHaveLength(8);
    expect(tasks).toEqual(shedTasks());
    expect(tasks[4]).toEqual({
      id: taskId(5),
      name: "Build the floor",
      start: "2027-03-12T08:00:00",
      finish: "2027-03-15T17:00:00",
      isCritical: true,
      predecessors: [taskId(3), taskId(4)],
      isSummary: false,
      outlineLevel: 1,
      isMilestone: false,
    });
  });

  // When the schedule failed, dates are null in the snapshot; they become
  // empty text, so the renderer can skip the bars (empty text reads as NaN).
  it("turns missing dates into empty text", () => {
    const tasks = snapshotToGanttTasks(shedSnapshot(true));
    expect(tasks).toHaveLength(8);
    expect(tasks.every((t) => t.start === "" && t.finish === "")).toBe(true);
    expect(tasks.every((t) => !t.isCritical)).toBe(true);
    expect(tasks[1]?.predecessors).toEqual([taskId(1)]);
  });

  // No tasks, no rows.
  it("handles an empty project", () => {
    expect(snapshotToGanttTasks({ ...shedSnapshot(), tasks: [] })).toEqual([]);
  });
});

describe("prepareGanttModel", () => {
  // Dates become milliseconds (UTC), links become row numbers.
  it("converts dates and links", () => {
    const model = prepareGanttModel(shedTasks());
    expect(model.rows).toHaveLength(8);
    expect(model.rows[0]?.startMs).toBe(Date.UTC(2027, 2, 1, 8));
    expect(model.rows[4]?.predecessorRows).toEqual([2, 3]);
  });

  // The range spans the first start and the last finish; 7 of 8 tasks are
  // critical; the longest link spans 2 rows (row 3 → row 5).
  it("computes range, critical count and longest link", () => {
    const model = prepareGanttModel(shedTasks());
    expect(model.minMs).toBe(Date.UTC(2027, 2, 1, 8));
    expect(model.maxMs).toBe(Date.UTC(2027, 2, 23, 17));
    expect(model.criticalCount).toBe(7);
    expect(model.maxLinkRowSpan).toBe(2);
  });

  // A predecessor ID that is not in the list (e.g. filtered out) is
  // skipped instead of crashing.
  it("ignores links to unknown tasks", () => {
    const tasks = shedTasks();
    const first = tasks[0];
    if (first === undefined) throw new Error("no first task");
    tasks[0] = { ...first, predecessors: ["not-there"] };
    expect(prepareGanttModel(tasks).rows[0]?.predecessorRows).toEqual([]);
  });

  // A project without dates (empty, or the schedule failed) has NaN as its
  // range, which tells the time scale to use its fallback.
  it("has a NaN range without dates", () => {
    const empty = prepareGanttModel([]);
    expect(empty.minMs).toBeNaN();
    expect(empty.maxMs).toBeNaN();
    const failed = prepareGanttModel(snapshotToGanttTasks(shedSnapshot(true)));
    expect(failed.minMs).toBeNaN();
    expect(failed.rows[0]?.startMs).toBeNaN();
  });
});
