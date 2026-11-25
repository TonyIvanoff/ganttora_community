/**
 * tests/frontend/gantt/hitTest.test.ts: tests for bar boxes and clicking.
 *
 * What it checks:
 *   `frontend/features/gantt/renderer/hitTest.ts`:
 *   - `barBox` computes the rectangle of a task bar on screen (or of a
 *     milestone diamond), taking scrolling into account;
 *   - `hitTestBar` finds which bar is under the mouse, with a tolerance of
 *     3 px on each side so thin bars are easy to click.
 *
 * Why it matters:
 *   Clicking a bar selects the task (and later drags it). If the box is off,
 *   the user clicks a bar and nothing happens, or the wrong task is picked.
 *
 * Main ideas:
 *   - The header is 48 px tall; rows are 28 px; bars are 14 px tall and
 *     centred in their row, so a bar's top is 7 px below the row's top.
 *   - Milestones (start = finish, e.g. "Shed finished") are drawn as a
 *     14 × 14 px diamond centred on their date.
 */

import { describe, expect, it } from "vitest";
import {
  HIT_TOLERANCE,
  MIN_BAR_WIDTH,
  barBox,
  hitTestBar,
} from "@frontend/features/gantt/renderer/hitTest";
import { BAR_HEIGHT, HEADER_HEIGHT, ROW_HEIGHT } from "@frontend/features/gantt/renderer/layout";
import { prepareGanttModel, type GanttModel } from "@frontend/features/gantt/renderer/model";
import {
  ZOOM_LEVELS,
  createTimeScale,
  msToX,
  type TimeScale,
} from "@frontend/features/gantt/renderer/timeScale";
import { snapshotToGanttTasks } from "@frontend/features/gantt/fromSnapshot";
import { shedSnapshot, shedTasks } from "./helpers";

/** The shed model and its "days" time scale, as the chart builds them. */
function shed(): { model: GanttModel; scale: TimeScale } {
  const model = prepareGanttModel(shedTasks());
  const scale = createTimeScale(model.minMs, model.maxMs, ZOOM_LEVELS.days, 0);
  return { model, scale };
}

/** Returns row `i` of the model, or fails the test if it does not exist. */
function row(model: GanttModel, i: number) {
  const r = model.rows[i];
  if (r === undefined) throw new Error(`row ${i} missing`);
  return r;
}

/** The y coordinate in the middle of row `i` (no vertical scroll). */
function midY(i: number): number {
  return HEADER_HEIGHT + i * ROW_HEIGHT + ROW_HEIGHT / 2;
}

describe("barBox", () => {
  // A normal bar starts at its start date and ends at its finish date,
  // 7 px below the top of its row.
  it("places a task bar from start to finish", () => {
    const { model, scale } = shed();
    const task = row(model, 0);
    const box = barBox(task, 0, scale, 0, 0);
    expect(box).not.toBeNull();
    expect(box?.isMilestone).toBe(false);
    expect(box?.x).toBeCloseTo(msToX(scale, task.startMs), 9);
    expect(box?.width).toBeCloseTo(msToX(scale, task.finishMs) - msToX(scale, task.startMs), 9);
    expect(box?.y).toBe(HEADER_HEIGHT + (ROW_HEIGHT - BAR_HEIGHT) / 2);
    expect(box?.height).toBe(BAR_HEIGHT);
  });

  // Scrolling moves the box left/up by the scroll amounts.
  it("follows the scroll position", () => {
    const { model, scale } = shed();
    const still = barBox(row(model, 2), 2, scale, 0, 0);
    const scrolled = barBox(row(model, 2), 2, scale, 100, 28);
    expect(scrolled?.x).toBeCloseTo((still?.x ?? 0) - 100, 9);
    expect(scrolled?.y).toBe((still?.y ?? 0) - 28);
  });

  // A milestone becomes a 14 × 14 diamond box centred on its date.
  it("draws a milestone as a diamond centred on its date", () => {
    const { model, scale } = shed();
    const milestone = row(model, 7); // "Shed finished"
    const box = barBox(milestone, 7, scale, 0, 0);
    expect(box?.isMilestone).toBe(true);
    expect(box?.width).toBe(BAR_HEIGHT);
    expect(box?.height).toBe(BAR_HEIGHT);
    expect(box?.x).toBeCloseTo(msToX(scale, milestone.startMs) - BAR_HEIGHT / 2, 9);
  });

  // A very short task (one minute) is still at least 2 px wide, so it stays
  // visible and clickable.
  it("gives very short tasks a minimum width", () => {
    const start = "2027-03-01T08:00:00";
    const model = prepareGanttModel([
      {
        id: "a",
        name: "Tiny",
        start,
        finish: "2027-03-01T08:01:00",
        isCritical: false,
        predecessors: [],
      },
    ]);
    const scale = createTimeScale(model.minMs, model.maxMs, ZOOM_LEVELS.weeks, 0);
    expect(barBox(row(model, 0), 0, scale, 0, 0)?.width).toBe(MIN_BAR_WIDTH);
  });

  // No dates (failed schedule): no box at all, so nothing is drawn.
  it("returns null for a task without dates", () => {
    const model = prepareGanttModel(snapshotToGanttTasks(shedSnapshot(true)));
    const scale = createTimeScale(model.minMs, model.maxMs, ZOOM_LEVELS.days, 0);
    expect(barBox(row(model, 0), 0, scale, 0, 0)).toBeNull();
  });
});

describe("hitTestBar", () => {
  // Clicking in the middle of a bar finds its row.
  it("finds the bar under the mouse", () => {
    const { model, scale } = shed();
    const box = barBox(row(model, 1), 1, scale, 0, 0);
    const x = (box?.x ?? 0) + (box?.width ?? 0) / 2;
    expect(hitTestBar({ model, scale, x, y: midY(1), scrollLeft: 0, scrollTop: 0 })).toBe(1);
  });

  // Up to 3 px beside the bar still counts; 3.5 px does not. This checks
  // both edges of the tolerance zone.
  it("accepts clicks up to 3 px beside the bar", () => {
    const { model, scale } = shed();
    const box = barBox(row(model, 3), 3, scale, 0, 0);
    if (box === null) throw new Error("no box");
    const hit = (x: number) =>
      hitTestBar({ model, scale, x, y: midY(3), scrollLeft: 0, scrollTop: 0 });
    expect(HIT_TOLERANCE).toBe(3);
    expect(hit(box.x - 3)).toBe(3);
    expect(hit(box.x - 3.5)).toBeNull();
    expect(hit(box.x + box.width + 3)).toBe(3);
    expect(hit(box.x + box.width + 3.5)).toBeNull();
  });

  // The milestone diamond also has the 3 px tolerance around its box.
  it("hits a milestone diamond with tolerance", () => {
    const { model, scale } = shed();
    const box = barBox(row(model, 7), 7, scale, 0, 0);
    if (box === null) throw new Error("no box");
    const hit = (x: number) =>
      hitTestBar({ model, scale, x, y: midY(7), scrollLeft: 0, scrollTop: 0 });
    expect(hit(box.x + BAR_HEIGHT / 2)).toBe(7); // centre
    expect(hit(box.x - 3)).toBe(7);
    expect(hit(box.x + BAR_HEIGHT + 3)).toBe(7);
    expect(hit(box.x - 4)).toBeNull();
  });

  // The whole row height counts (by design, to make thin bars easy to hit),
  // so the very top pixel of the row still hits; the next row starts at +28.
  it("uses the whole row height", () => {
    const { model, scale } = shed();
    const box = barBox(row(model, 0), 0, scale, 0, 0);
    const x = (box?.x ?? 0) + 1;
    expect(hitTestBar({ model, scale, x, y: HEADER_HEIGHT, scrollLeft: 0, scrollTop: 0 })).toBe(0);
    expect(
      hitTestBar({
        model,
        scale,
        x,
        y: HEADER_HEIGHT + ROW_HEIGHT - 0.01,
        scrollLeft: 0,
        scrollTop: 0,
      }),
    ).toBe(0);
  });

  // Clicks on the header, below the last row, or on a row whose task has
  // no dates hit nothing.
  it("returns null on the header, below the rows and without dates", () => {
    const { model, scale } = shed();
    const box = barBox(row(model, 0), 0, scale, 0, 0);
    const x = (box?.x ?? 0) + 1;
    expect(
      hitTestBar({ model, scale, x, y: HEADER_HEIGHT - 1, scrollLeft: 0, scrollTop: 0 }),
    ).toBeNull();
    expect(hitTestBar({ model, scale, x, y: midY(8), scrollLeft: 0, scrollTop: 0 })).toBeNull();

    const failed = prepareGanttModel(snapshotToGanttTasks(shedSnapshot(true)));
    expect(
      hitTestBar({ model: failed, scale, x, y: midY(0), scrollLeft: 0, scrollTop: 0 }),
    ).toBeNull();
  });

  // With scrolling, the same screen point finds a different row/bar: scroll
  // down 2 rows and the first visible row is row 2.
  it("accounts for scrolling", () => {
    const { model, scale } = shed();
    const scrollTop = 2 * ROW_HEIGHT;
    const scrollLeft = 150;
    const box = barBox(row(model, 2), 2, scale, scrollLeft, scrollTop);
    if (box === null) throw new Error("no box");
    expect(box.y).toBe(HEADER_HEIGHT + (ROW_HEIGHT - BAR_HEIGHT) / 2);
    const x = box.x + box.width / 2;
    expect(hitTestBar({ model, scale, x, y: midY(0), scrollLeft, scrollTop })).toBe(2);
  });
});
