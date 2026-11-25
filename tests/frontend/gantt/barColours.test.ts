/**
 * tests/frontend/gantt/barColours.test.ts: which colour each kind of bar
 * gets in the Gantt chart.
 *
 * What it checks (`frontend/features/gantt/renderer/draw.ts`):
 *   - bars are outlined in their colour and filled only where finished;
 *   - a task is blue, a sub-task cyan, a project has its own;
 *   - a risk gives a border, being critical does not;
 *   - blocked time uses the blocker colour.
 *
 * Why it matters:
 *   The colours carry meaning: red must only ever mean "blocked".
 *
 * Main ideas:
 *   A real canvas needs a browser. Here a fake `Path2D` counts the shapes
 *   added to it, and a fake brush remembers which colour painted how many.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { localToday } from "@frontend/features/gantt/renderer/dates";
import { drawGanttFrame, type FrameInput } from "@frontend/features/gantt/renderer/draw";
import {
  deadlineBox,
  hitTestDeadline,
  finishArrowBox,
  hitTestFinishArrow,
  hitTestLateStart,
  isLateToStart,
  lateStartBox,
} from "@frontend/features/gantt/renderer/hitTest";
import { prepareGanttModel, type GanttSourceTask } from "@frontend/features/gantt/renderer/model";
import type { GanttTheme } from "@frontend/features/gantt/renderer/theme";
import { createTimeScale, msToX, ZOOM_LEVELS } from "@frontend/features/gantt/renderer/timeScale";

/** A fake path: counts rectangles and closed shapes (brackets, diamonds). */
class FakePath {
  /** Number of shapes added. */
  shapes = 0;
  /** Adds a rectangle. */
  rect(): void {
    this.shapes++;
  }
  /** Ends a shape drawn line by line. */
  closePath(): void {
    this.shapes++;
  }
  /** Moves the pen (not counted). */
  moveTo(): void {}
  /** Draws a line (not counted). */
  lineTo(): void {}
  /** Draws an arc (not counted). */
  arc(): void {}
}

/** A theme in which every colour is its own field name, e.g. bar = "bar". */
const THEME = new Proxy({}, { get: (_target, name) => String(name) }) as GanttTheme;

beforeEach(() => vi.stubGlobal("Path2D", FakePath));
afterEach(() => vi.unstubAllGlobals());

/**
 * One task on Mon 1 – Wed 3 March 2027.
 *
 * @param extra - Fields that differ.
 */
function task(extra: Partial<GanttSourceTask> = {}): GanttSourceTask {
  return {
    id: "a",
    name: "A",
    start: "2027-03-01T08:00:00",
    finish: "2027-03-03T17:00:00",
    isCritical: false,
    predecessors: [],
    ...extra,
  };
}

/**
 * Draws one task and reports the colours that painted at least one shape.
 *
 * @param source - The task.
 * @param todayMs - Today, if it matters.
 * @returns Entries such as "fill bar" and "stroke riskLow", sorted.
 */
function painted(source: GanttSourceTask, todayMs?: number): string[] {
  const seen = new Set<string>();
  const state: Record<string, unknown> = {};
  /** Remembers a paint operation if its path has shapes. */
  const note = (kind: "fill" | "stroke") => (path?: FakePath) => {
    if (path !== undefined && path.shapes > 0) seen.add(`${kind} ${String(state[`${kind}Style`])}`);
  };
  const ctx = new Proxy(state, {
    get: (target, name) => {
      if (name === "fill" || name === "stroke") return note(name);
      if (name === "measureText") return () => ({ width: 10 });
      return name in target ? target[String(name)] : () => undefined;
    },
  });
  const model = prepareGanttModel([source]);
  const input: FrameInput = {
    ctx: ctx as unknown as CanvasRenderingContext2D,
    width: 2000,
    height: 400,
    dpr: 1,
    scrollLeft: 0,
    scrollTop: 0,
    model,
    scale: createTimeScale(model.minMs, model.maxMs, ZOOM_LEVELS.days, 0),
    theme: THEME,
    label: () => "",
    todayMs,
    selectedRow: null,
  };
  drawGanttFrame(input);
  return [...seen].sort();
}

/** An empty bar of one colour. */
const empty = (color: string) => ["fill background", `stroke ${color}`];

describe("bar colours", () => {
  it("outlines a task blue and a sub-task cyan", () => {
    expect(painted(task())).toEqual(empty("bar"));
    expect(painted(task({ outlineLevel: 1 }))).toEqual(empty("bar"));
    expect(painted(task({ outlineLevel: 2 }))).toEqual(empty("barSubtask"));
    expect(painted(task({ outlineLevel: 3 }))).toEqual(empty("barSubtask"));
  });

  it("fills the finished part", () => {
    expect(painted(task({ percentComplete: 40 }))).toEqual([
      "fill background",
      "fill bar",
      "stroke bar",
    ]);
  });

  it("fills a milestone only when it is reached", () => {
    const milestone = { finish: "2027-03-01T08:00:00" };
    expect(painted(task({ ...milestone, percentComplete: 99 }))).toEqual(empty("bar"));
    expect(painted(task({ ...milestone, percentComplete: 100 }))).toContain("fill bar");
  });

  it("draws a missed milestone orange, without arrow or risk", () => {
    const at = "2027-03-01T17:00:00";
    const milestone = { start: at, finish: at, isMilestone: true };
    const nextDay = Date.UTC(2027, 2, 2);
    expect(painted(task(milestone), nextDay)).toContain("stroke lateStart");
    const all = painted(task(milestone), nextDay).join();
    expect(all).not.toContain("fill lateStart");
    expect(all).not.toContain("risk");
    expect(painted(task({ ...milestone, percentComplete: 100 }), nextDay)).toContain("stroke bar");
    expect(painted(task(milestone), Date.UTC(2027, 2, 1))).toContain("stroke bar"); // due today
  });

  it("gives projects their own colour, and nothing to an empty one", () => {
    const project = { isProject: true, outlineLevel: 1 };
    expect(painted(task({ ...project, isSummary: true }))).toContain("stroke barProject");
    expect(painted(task(project))).toEqual([]);
  });

  it("does not highlight critical bars", () => {
    expect(painted(task({ isCritical: true }))).toEqual(empty("bar"));
  });

  it("gives a bar with a risk a border in the colour of its priority", () => {
    expect(painted(task({ riskPriority: "low" }))).toContain("stroke riskLow");
    expect(painted(task({ riskPriority: "medium" }))).toContain("stroke riskMedium");
    expect(painted(task({ riskPriority: "high" }))).toContain("stroke riskHigh");
    expect(painted(task({ riskPriority: "high", isProject: true })).join()).not.toContain("risk");
    expect(painted(task({ riskPriority: null })).join()).not.toContain("risk");
  });

  it("keeps the manual colour on every level", () => {
    expect(painted(task({ isManual: true, outlineLevel: 2 }))).toEqual(empty("barManual"));
  });

  it("paints blocked time in the blocker colour", () => {
    const blocker = { id: "b", reason: "Late", durationMinutes: 480, reportedOn: null };
    const segments = [
      { start: "2027-03-01T08:00:00", finish: "2027-03-02T08:00:00", blocker: null },
      { start: "2027-03-02T08:00:00", finish: "2027-03-03T17:00:00", blocker },
    ];
    expect(painted(task({ segments }))).toEqual([
      "fill background",
      "fill barBlocked",
      "stroke bar",
      "stroke barBlockedLine",
    ]);
  });

  it("counts levels from the project when there is one", () => {
    const rows = prepareGanttModel([
      task({ id: "p", isProject: true, outlineLevel: 1 }),
      task({ id: "t", outlineLevel: 2 }),
      task({ id: "s", outlineLevel: 3 }),
    ]).rows;
    expect(rows.map((r) => r.isSubtask)).toEqual([false, false, true]);
  });
});

it("highlights today in the chart and in the header", () => {
  const rects: string[] = [];
  const state: Record<string, unknown> = {};
  const ctx = new Proxy(state, {
    get: (target, name) => {
      if (name === "fillRect") {
        return (x: number, y: number, w: number) =>
          rects.push(`${String(state.fillStyle)} ${x} ${y > 0 ? "below" : "top"} ${w}`);
      }
      return name in target ? target[String(name)] : () => undefined;
    },
  });
  const model = prepareGanttModel([task()]);
  const scale = createTimeScale(model.minMs, model.maxMs, ZOOM_LEVELS.days, 0);
  const todayMs = Date.UTC(2027, 2, 2);
  drawGanttFrame({
    ctx: ctx as unknown as CanvasRenderingContext2D,
    width: 2000,
    height: 400,
    dpr: 1,
    scrollLeft: 0,
    scrollTop: 0,
    model,
    scale,
    theme: THEME,
    label: () => "",
    selectedRow: null,
    todayMs,
  });
  const x = msToX(scale, todayMs);
  expect(rects).toContain(`today ${x} below ${scale.pxPerDay}`);
  expect(rects).toContain(`todayHeader ${x} below ${scale.pxPerDay}`);
});

it("takes today from the user's own clock", () => {
  expect(localToday(new Date(2026, 10, 25, 0, 30))).toBe(Date.UTC(2026, 10, 25));
  expect(localToday(new Date(2026, 10, 25, 23, 30))).toBe(Date.UTC(2026, 10, 25));
});

it("draws the deadline as a large arrow that can be pointed at", () => {
  const source = { ...task(), deadline: "2027-03-20T17:00:00" };
  const model = prepareGanttModel([source]);
  const scale = createTimeScale(model.minMs, model.maxMs, ZOOM_LEVELS.days, 0);
  const row = model.rows[0]!;
  const box = deadlineBox(row, 0, scale, 0, 0)!;
  expect([box.width, box.height]).toEqual([14, 20]);
  expect(box.x + 7).toBeCloseTo(msToX(scale, row.deadlineMs));
  const at = (x: number) =>
    hitTestDeadline({ model, scale, x, y: box.y + 5, scrollLeft: 0, scrollTop: 0 });
  expect(at(box.x + 1)).toBe(0);
  expect(at(box.x - 1)).toBeNull();
  // The arrow and its outline.
  expect(painted(source)).toEqual(expect.arrayContaining(["fill deadline", "stroke background"]));
  expect(painted({ ...source, missedDeadline: true })).toContain("fill deadlineMissed");
});

it("marks a task that should have started with an arrow to its left", () => {
  const today = Date.UTC(2027, 2, 2);
  const late = (extra: Partial<GanttSourceTask>, day = today) => {
    const row = prepareGanttModel([task(extra)]).rows[0]!;
    return isLateToStart(row, day);
  };
  expect(late({})).toBe(true);
  expect(late({}, Date.UTC(2027, 2, 1))).toBe(false); // starts today
  expect(late({ percentComplete: 10 })).toBe(false);
  expect(late({ isProject: true })).toBe(false);
  expect(late({ isSummary: true })).toBe(false);

  const model = prepareGanttModel([task()]);
  const scale = createTimeScale(model.minMs, model.maxMs, ZOOM_LEVELS.days, 0);
  const row = model.rows[0]!;
  const box = lateStartBox(row, 0, scale, 0, 0, today)!;
  expect(box.x + box.width).toBeLessThan(msToX(scale, row.startMs));
  const input = { model, scale, x: box.x + 2, y: box.y + 2, scrollLeft: 0, scrollTop: 0 };
  expect(hitTestLateStart(input, today)).toBe(0);
  expect(hitTestLateStart({ ...input, x: box.x - 2 }, today)).toBeNull();
  expect(lateStartBox(row, 0, scale, 0, 0, undefined)).toBeNull();
  expect(painted(task(), today)).toContain("fill lateStart");
  expect(painted(task())).not.toContain("fill lateStart");
});

it("draws an arrow from the planned to the actual or calculated finish", () => {
  const row = (extra: Partial<GanttSourceTask>) => prepareGanttModel([task(extra)]).rows[0]!;
  const finish = row({}).realFinishMs;
  const iso = (days: number) => new Date(finish + days * 86_400_000).toISOString().slice(0, 19);
  // No plan, or on the planned day: none.
  expect(row({}).variance).toBeNull();
  expect(row({ plannedFinish: iso(0) }).variance).toBeNull();
  expect(row({ plannedFinish: iso(-2) }).variance).toMatchObject({ late: true, done: false });
  expect(row({ plannedFinish: iso(2) }).variance).toMatchObject({ late: false, done: false });
  // Finished: only the actual finish counts.
  const done = { percentComplete: 100, plannedFinish: iso(-2) };
  expect(row(done).variance).toBeNull();
  expect(row({ ...done, actualFinish: iso(-3) }).variance).toMatchObject({
    late: false,
    done: true,
  });
  expect(row({ plannedFinish: iso(-2), isSummary: true }).variance).toBeNull();

  const model = prepareGanttModel([task({ plannedFinish: iso(-2) })]);
  const scale = createTimeScale(model.minMs, model.maxMs, ZOOM_LEVELS.days, 0);
  const box = finishArrowBox(model.rows[0]!, 0, scale, 0, 0)!;
  expect(box.x + box.width).toBeCloseTo(msToX(scale, model.rows[0]!.finishMs));
  const input = { model, scale, x: box.x + 2, y: box.y + 2, scrollLeft: 0, scrollTop: 0 };
  expect(hitTestFinishArrow(input)).toBe(0);
  expect(hitTestFinishArrow({ ...input, x: box.x - 2 })).toBeNull();
  expect(painted(task({ plannedFinish: iso(-2) }))).toContain("stroke lateStart");
  expect(painted(task({ ...done, actualFinish: iso(-3) }))).toContain("fill deadline");
});
