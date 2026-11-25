/**
 * tests/frontend/gantt/summaryMilestone.test.ts: summary brackets, milestone
 * diamonds and the snapshot's scheduled dates (M1).
 *
 * What it checks:
 *   - `snapshotToGanttTasks` (`frontend/features/gantt/fromSnapshot.ts`)
 *     uses the scheduled `start` / `finish` (M1: ALAP, manual and summary
 *     tasks can differ from the early dates) and falls back to the early
 *     dates, then to empty text; it passes the summary and milestone flags.
 *   - `prepareGanttModel` keeps those flags on each row.
 *   - `barBox` / `hitTestBar` (`frontend/features/gantt/renderer/hitTest.ts`),
 *     the shapes the renderer draws and clicks on: a summary task is a
 *     bracket as wide as its dates; a milestone (flagged, or zero duration)
 *     is a square diamond box centred on its start; a summary is never a
 *     diamond, even with zero length.
 *
 * Why it matters:
 *   The renderer draws exactly these boxes (`draw.ts` uses `barBox`). A
 *   summary drawn as a diamond, or a manual task drawn at its early date,
 *   would show the user a different plan than the one Rust calculated.
 *
 * Main ideas:
 *   The draw functions themselves need a real canvas; the boxes they draw
 *   are tested here instead, without one.
 */

import { describe, expect, it } from "vitest";
import { snapshotToGanttTasks } from "@frontend/features/gantt/fromSnapshot";
import { barBox, hitTestBar } from "@frontend/features/gantt/renderer/hitTest";
import { BAR_HEIGHT, HEADER_HEIGHT, ROW_HEIGHT } from "@frontend/features/gantt/renderer/layout";
import {
  prepareGanttModel,
  type GanttRow,
  type GanttSourceTask,
} from "@frontend/features/gantt/renderer/model";
import { ZOOM_LEVELS, createTimeScale, msToX } from "@frontend/features/gantt/renderer/timeScale";
import { shedSnapshot } from "./helpers";

/** A small outline: a summary, two subtasks, a flagged milestone with a duration. */
const TASKS: GanttSourceTask[] = [
  {
    id: "phase",
    name: "Phase 1",
    start: "2027-03-01T08:00:00",
    finish: "2027-03-09T17:00:00",
    isCritical: true,
    predecessors: [],
    isSummary: true,
  },
  {
    id: "design",
    name: "Design",
    start: "2027-03-01T08:00:00",
    finish: "2027-03-02T17:00:00",
    isCritical: true,
    predecessors: [],
  },
  {
    id: "review",
    name: "Review (marked as milestone)",
    start: "2027-03-03T08:00:00",
    finish: "2027-03-04T17:00:00",
    isCritical: false,
    predecessors: ["design"],
    isMilestone: true,
  },
  {
    id: "empty-phase",
    name: "Empty phase (zero length summary)",
    start: "2027-03-05T08:00:00",
    finish: "2027-03-05T08:00:00",
    isCritical: false,
    predecessors: [],
    isSummary: true,
  },
];

/** The model and a "days" time scale for {@link TASKS}. */
function setup() {
  const model = prepareGanttModel(TASKS);
  const scale = createTimeScale(model.minMs, model.maxMs, ZOOM_LEVELS.days, 0);
  return { model, scale };
}

/** Row `i` of the model, or a failed test. */
function row(rows: readonly GanttRow[], i: number): GanttRow {
  const r = rows[i];
  if (r === undefined) throw new Error(`row ${i} missing`);
  return r;
}

describe("snapshotToGanttTasks (M1 dates and flags)", () => {
  // The scheduled dates win over the early dates (an ALAP task is drawn
  // where it is scheduled, not where it could start).
  it("uses start/finish before the early dates", () => {
    const snapshot = shedSnapshot();
    const alap = snapshot.tasks[2];
    if (alap === undefined) throw new Error("no row 3");
    alap.start = "2027-03-09T08:00:00";
    alap.finish = "2027-03-11T17:00:00";
    const tasks = snapshotToGanttTasks(snapshot);
    expect(tasks[2]?.start).toBe("2027-03-09T08:00:00");
    expect(tasks[2]?.finish).toBe("2027-03-11T17:00:00");
  });

  // Without scheduled dates (an older shell) the early dates are used.
  it("falls back to the early dates", () => {
    const snapshot = shedSnapshot();
    const first = snapshot.tasks[0];
    if (first === undefined) throw new Error("no row 1");
    first.start = null;
    first.finish = null;
    const tasks = snapshotToGanttTasks(snapshot);
    expect(tasks[0]?.start).toBe("2027-03-01T08:00:00");
    expect(tasks[0]?.finish).toBe("2027-03-02T17:00:00");
  });

  // Summary and milestone flags are passed through.
  it("passes the summary and milestone flags", () => {
    const snapshot = shedSnapshot();
    const first = snapshot.tasks[0];
    if (first === undefined) throw new Error("no row 1");
    first.isSummary = true;
    const tasks = snapshotToGanttTasks(snapshot);
    expect(tasks[0]?.isSummary).toBe(true);
    expect(tasks[7]?.isMilestone).toBe(true);
    expect(tasks[6]?.isMilestone).toBe(false);
  });
});

describe("prepareGanttModel flags", () => {
  // Missing flags count as false.
  it("keeps the flags, defaulting to false", () => {
    const { model } = setup();
    expect(model.rows.map((r) => [r.isSummary, r.isMilestone])).toEqual([
      [true, false],
      [false, false],
      [false, true],
      [true, false],
    ]);
  });
});

describe("barBox for summaries and milestones", () => {
  // A summary is a bracket box from its start to its finish.
  it("draws a summary as wide as its dates", () => {
    const { model, scale } = setup();
    const phase = row(model.rows, 0);
    const box = barBox(phase, 0, scale, 0, 0);
    expect(box).not.toBeNull();
    expect(box?.isSummary).toBe(true);
    expect(box?.isMilestone).toBe(false);
    expect(box?.x).toBeCloseTo(msToX(scale, phase.startMs));
    expect(box?.width).toBeCloseTo(msToX(scale, phase.finishMs) - msToX(scale, phase.startMs));
  });

  // A task flagged as a milestone is a diamond at its start, even though
  // it lasts two days: a square box of bar height centred on the start.
  it("draws a flagged milestone as a diamond at its start", () => {
    const { model, scale } = setup();
    const review = row(model.rows, 2);
    const box = barBox(review, 2, scale, 0, 0);
    expect(box?.isMilestone).toBe(true);
    expect(box?.width).toBe(BAR_HEIGHT);
    expect(box?.height).toBe(BAR_HEIGHT);
    expect(box?.x).toBeCloseTo(msToX(scale, review.startMs) - BAR_HEIGHT / 2);
  });

  // A zero-length summary is still a summary, never a diamond.
  it("never draws a summary as a diamond", () => {
    const { model, scale } = setup();
    const box = barBox(row(model.rows, 3), 3, scale, 0, 0);
    expect(box?.isSummary).toBe(true);
    expect(box?.isMilestone).toBe(false);
    expect(box?.width).toBeGreaterThan(0);
  });
});

describe("hitTestBar on summaries and milestones", () => {
  /** The y coordinate in the middle of row `i`. */
  const midY = (i: number) => HEADER_HEIGHT + i * ROW_HEIGHT + ROW_HEIGHT / 2;

  // Clicking inside the summary bracket selects the summary row.
  it("hits the summary bracket", () => {
    const { model, scale } = setup();
    const phase = row(model.rows, 0);
    const x = (msToX(scale, phase.startMs) + msToX(scale, phase.finishMs)) / 2;
    expect(hitTestBar({ model, scale, x, y: midY(0), scrollLeft: 0, scrollTop: 0 })).toBe(0);
  });

  // The diamond is clickable at its start, but not at the task's finish
  // (the diamond does not stretch to the finish date).
  it("hits the diamond only near its start", () => {
    const { model, scale } = setup();
    const review = row(model.rows, 2);
    const atStart = msToX(scale, review.startMs);
    const atFinish = msToX(scale, review.finishMs);
    expect(hitTestBar({ model, scale, x: atStart, y: midY(2), scrollLeft: 0, scrollTop: 0 })).toBe(
      2,
    );
    expect(
      hitTestBar({ model, scale, x: atFinish, y: midY(2), scrollLeft: 0, scrollTop: 0 }),
    ).toBeNull();
  });
});
