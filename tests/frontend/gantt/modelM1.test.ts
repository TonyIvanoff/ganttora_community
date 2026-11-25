/**
 * tests/frontend/gantt/modelM1.test.ts: tests for the M1 fields of the
 * Gantt model (link types, deadlines, constraints, manual bars).
 *
 * What it checks:
 *   - `snapshotToDetailedGanttTasks` (fromSnapshot.ts) copies the M1 fields
 *     of each snapshot row into the chart's input: typed links, manual
 *     mode, deadline, missed deadline, constraint; an older snapshot
 *     without `links` still works;
 *   - `prepareGanttModel` (renderer/model.ts) resolves typed links to row
 *     numbers, turns the deadline and constraint dates into milliseconds
 *     (`NaN` when there is none), and widens the chart's range (`maxMs`) to
 *     include a deadline after every bar.
 *
 * Why it matters:
 *   The deadline marker and constraint pin are drawn from these numbers. If
 *   `maxMs` ignored a late deadline, the user could not scroll to it.
 *
 * Main ideas:
 *   `NaN` ("not a number") is how the model says "no date"; the drawing
 *   code skips `NaN` dates.
 */

import { describe, expect, it } from "vitest";
import { snapshotToDetailedGanttTasks } from "@frontend/features/gantt/fromSnapshot";
import { parseIsoLocal } from "@frontend/features/gantt/renderer/dates";
import { prepareGanttModel, type GanttSourceTask } from "@frontend/features/gantt/renderer/model";
import type { ScheduleSnapshot, TaskRow } from "@frontend/ipc/types";
import { shedSnapshot, taskId } from "./helpers";

/** A minimal chart task, changed by `changes`. */
function source(id: string, changes: Partial<GanttSourceTask> = {}): GanttSourceTask {
  return {
    id,
    name: id,
    start: "2027-03-01T08:00:00",
    finish: "2027-03-02T17:00:00",
    isCritical: false,
    predecessors: [],
    ...changes,
  };
}

describe("snapshotToDetailedGanttTasks", () => {
  /** The shed snapshot with M1 details on rows 2 and 3. */
  function detailed(): ScheduleSnapshot {
    const snapshot = shedSnapshot();
    const tasks = snapshot.tasks.map((task, i): TaskRow => {
      if (i === 1) {
        return {
          ...task,
          mode: "manual",
          deadline: "2027-03-08T17:00:00",
          missedDeadline: true,
          constraint: { type: "startNoEarlierThan", date: "2027-03-03T08:00:00" },
          links: [
            {
              id: "l",
              predecessor: taskId(1),
              kind: "startToStart",
              lagMinutes: 480,
              lagPercent: null,
            },
          ],
        };
      }
      return task;
    });
    return { ...snapshot, tasks };
  }

  // Row 2 carries every M1 field into the chart input.
  it("copies links, manual mode, deadline and constraint", () => {
    const tasks = snapshotToDetailedGanttTasks(detailed());
    expect(tasks).toHaveLength(8);
    const permit = tasks[1];
    expect(permit?.links).toEqual([{ predecessor: taskId(1), kind: "startToStart" }]);
    expect(permit?.isManual).toBe(true);
    expect(permit?.deadline).toBe("2027-03-08T17:00:00");
    expect(permit?.missedDeadline).toBe(true);
    expect(permit?.constraint).toEqual({
      type: "startNoEarlierThan",
      date: "2027-03-03T08:00:00",
    });
    // The basic fields are still there.
    expect(permit?.name).toBe("Get building permit");
    expect(permit?.start).toBe("2027-03-03T08:00:00");
  });

  // An ordinary row: auto mode, no deadline, ASAP.
  it("gives defaults for ordinary rows", () => {
    const first = snapshotToDetailedGanttTasks(detailed())[0];
    expect(first?.isManual).toBe(false);
    expect(first?.deadline).toBeNull();
    expect(first?.links).toEqual([]);
    expect(first?.constraint).toEqual({ type: "asSoonAsPossible" });
  });

  // A snapshot from before M1 has no `links`: the chart gets an empty list.
  it("copes with snapshots without links", () => {
    const snapshot = shedSnapshot();
    const old = {
      ...snapshot,
      // Remove the `links` field (as if Rust had never sent it). `delete`
      // works on a copy, so the shared snapshot is not changed.
      tasks: snapshot.tasks.map((task) => {
        const copy: Partial<TaskRow> = { ...task };
        delete copy.links;
        return copy as TaskRow;
      }),
    };
    expect(snapshotToDetailedGanttTasks(old).every((t) => t.links?.length === 0)).toBe(true);
  });
});

describe("prepareGanttModel: M1 fields", () => {
  // Typed links resolve to rows and keep their type; plain predecessors
  // are only used when there are no typed links.
  it("resolves typed links", () => {
    const model = prepareGanttModel([
      source("a"),
      source("b", { links: [{ predecessor: "a", kind: "finishToFinish" }], predecessors: ["x"] }),
      source("c", { predecessors: ["a"] }),
    ]);
    expect(model.rows[1]?.links).toEqual([{ row: 0, kind: "finishToFinish" }]);
    expect(model.rows[1]?.predecessorRows).toEqual([0]);
    expect(model.rows[2]?.links).toEqual([{ row: 0, kind: "finishToStart" }]);
  });

  // Deadline and constraint dates in milliseconds; NaN when absent.
  it("converts deadline and constraint dates", () => {
    const model = prepareGanttModel([
      source("a", {
        deadline: "2027-03-05T17:00:00",
        constraint: { type: "mustStartOn", date: "2027-03-01T08:00:00" },
        isManual: true,
        missedDeadline: true,
      }),
      source("b", { deadline: null, constraint: { type: "asSoonAsPossible" } }),
      source("c"),
    ]);
    const [a, b, c] = model.rows;
    expect(a?.deadlineMs).toBe(parseIsoLocal("2027-03-05T17:00:00"));
    expect(a?.constraintMs).toBe(parseIsoLocal("2027-03-01T08:00:00"));
    expect(a?.isManual).toBe(true);
    expect(a?.missedDeadline).toBe(true);
    expect(b?.deadlineMs).toBeNaN();
    expect(b?.constraintMs).toBeNaN();
    expect(c?.deadlineMs).toBeNaN();
    expect(c?.isManual).toBe(false);
  });

  // A deadline after the last bar widens the range; an earlier one does not.
  it("includes a late deadline in maxMs", () => {
    const late = prepareGanttModel([source("a", { deadline: "2027-04-30T17:00:00" })]);
    expect(late.maxMs).toBe(parseIsoLocal("2027-04-30T17:00:00"));
    const early = prepareGanttModel([source("a", { deadline: "2027-03-01T17:00:00" })]);
    expect(early.maxMs).toBe(parseIsoLocal("2027-03-02T17:00:00"));
  });
});
