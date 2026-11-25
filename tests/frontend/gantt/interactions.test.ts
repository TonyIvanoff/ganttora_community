/**
 * tests/frontend/gantt/interactions.test.ts: tests for dragging bars in the
 * Gantt chart.
 *
 * What it checks:
 *   `frontend/features/gantt/interactions.ts`, the pure logic behind the
 *   mouse gestures (the canvas code only calls these functions):
 *   - `hitTestPart`: did the pointer hit a bar's body or its right edge?
 *   - `classifyDrag`: after how many pixels a press becomes a drag (4 px),
 *     and whether it is a move, a resize or a link;
 *   - `daysFromPixels`, `snapMoveStart`, `snapResizeFinish`: whole days,
 *     snapped to working time (weekends, holidays, exceptions), and a
 *     finish that is never before the start;
 *   - `moveCommand`, `resizeCommand`, `linkCommand`: the one command each
 *     drag sends, or `null` when nothing would change.
 *
 * Why it matters:
 *   Dragging is the most direct way to edit a plan. Dropping a task on a
 *   Saturday must land it on Monday 08:00, like MS Project; a tiny shake of
 *   the mouse while clicking must not move anything.
 *
 * Main ideas:
 *   Times are milliseconds in UTC "wall clock" (see renderer/dates.ts).
 *   `at("2027-03-01T08:00:00")` turns ISO text into that number. 1 Mar 2027
 *   is a Monday.
 */

import { describe, expect, it } from "vitest";
import {
  DRAG_THRESHOLD,
  RESIZE_ZONE,
  classifyDrag,
  daysFromPixels,
  hitTestPart,
  linkCommand,
  moveCommand,
  resizeCommand,
  snapMoveStart,
  snapResizeFinish,
} from "@frontend/features/gantt/interactions";
import { standardWeek } from "@frontend/features/calendars/workingTime";
import { parseIsoLocal } from "@frontend/features/gantt/renderer/dates";
import { barBox, HIT_TOLERANCE } from "@frontend/features/gantt/renderer/hitTest";
import { HEADER_HEIGHT, ROW_HEIGHT } from "@frontend/features/gantt/renderer/layout";
import { prepareGanttModel, type GanttModel } from "@frontend/features/gantt/renderer/model";
import {
  ZOOM_LEVELS,
  createTimeScale,
  type TimeScale,
} from "@frontend/features/gantt/renderer/timeScale";
import type { Calendar } from "@frontend/ipc/types";
import { shedTasks } from "./helpers";
import { taskRow } from "../grid/helpers";

/** ISO text → milliseconds. */
const at = parseIsoLocal;

/** The standard calendar (Mon–Fri 08:00–12:00, 13:00–17:00), plus extras. */
function calendar(changes: Partial<Calendar> = {}): Calendar {
  return { id: "cal", name: "Standard", week: standardWeek(), holidays: [], ...changes };
}

/** The shed model and a scale at the given zoom. */
function shed(zoom: keyof typeof ZOOM_LEVELS = "days"): { model: GanttModel; scale: TimeScale } {
  const model = prepareGanttModel(shedTasks());
  return { model, scale: createTimeScale(model.minMs, model.maxMs, ZOOM_LEVELS[zoom], 0) };
}

/** The y coordinate in the middle of row `i`. */
function midY(i: number): number {
  return HEADER_HEIGHT + i * ROW_HEIGHT + ROW_HEIGHT / 2;
}

describe("hitTestPart", () => {
  // Row 0 ("Design the shed", 2 days) is 38.5 px wide at 28 px a day: wide
  // enough for a resize zone of 6 px at its right end.
  it("finds the body and the right edge of a bar", () => {
    const { model, scale } = shed();
    const row = model.rows[0];
    if (row === undefined) throw new Error("row 0 missing");
    const box = barBox(row, 0, scale, 0, 0);
    if (box === null) throw new Error("no box");
    const right = box.x + box.width;
    expect(hitTestPart(model, scale, box.x + 5, midY(0), 0, 0)).toEqual({ row: 0, part: "body" });
    expect(hitTestPart(model, scale, right - RESIZE_ZONE - 1, midY(0), 0, 0)?.part).toBe("body");
    expect(hitTestPart(model, scale, right - 2, midY(0), 0, 0)?.part).toBe("finishEdge");
    // The edge zone reaches the click tolerance past the bar.
    expect(hitTestPart(model, scale, right + HIT_TOLERANCE, midY(0), 0, 0)?.part).toBe(
      "finishEdge",
    );
    expect(hitTestPart(model, scale, right + HIT_TOLERANCE + 1, midY(0), 0, 0)).toBeNull();
  });

  // Milestones have no edge to drag; neither have bars narrower than three
  // resize zones (here: the "weeks" zoom, 7 px a day).
  it("has no edge on milestones and on narrow bars", () => {
    const { model, scale } = shed();
    const milestone = model.rows[7];
    if (milestone === undefined) throw new Error("row 7 missing");
    const mBox = barBox(milestone, 7, scale, 0, 0);
    if (mBox === null) throw new Error("no box");
    expect(hitTestPart(model, scale, mBox.x + mBox.width - 1, midY(7), 0, 0)?.part).toBe("body");

    const narrow = shed("weeks");
    const row = narrow.model.rows[0];
    if (row === undefined) throw new Error("row 0 missing");
    const box = barBox(row, 0, narrow.scale, 0, 0);
    if (box === null) throw new Error("no box");
    expect(box.width).toBeLessThan(3 * RESIZE_ZONE);
    expect(
      hitTestPart(narrow.model, narrow.scale, box.x + box.width - 1, midY(0), 0, 0)?.part,
    ).toBe("body");
  });

  // In the header or far from any bar: nothing.
  it("returns null away from bars", () => {
    const { model, scale } = shed();
    expect(hitTestPart(model, scale, 300, 10, 0, 0)).toBeNull();
    expect(hitTestPart(model, scale, 5000, midY(0), 0, 0)).toBeNull();
  });
});

describe("classifyDrag", () => {
  // Below 4 px in both directions it is still a click.
  it("waits for the 4 px threshold", () => {
    expect(DRAG_THRESHOLD).toBe(4);
    expect(classifyDrag("body", 3, 0, true)).toBeNull();
    expect(classifyDrag("body", -3, 3, true)).toBeNull();
    expect(classifyDrag("body", 4, 0, true)).toBe("move");
    expect(classifyDrag("body", 0, -4, true)).toBe("link");
  });

  // Mostly sideways = move; mostly up/down = link; a tie counts as sideways.
  it("tells a move from a link by the direction", () => {
    expect(classifyDrag("body", 20, 5, true)).toBe("move");
    expect(classifyDrag("body", 5, 20, true)).toBe("link");
    expect(classifyDrag("body", 10, 10, true)).toBe("move");
  });

  // The right edge resizes, whatever the direction.
  it("resizes from the right edge", () => {
    expect(classifyDrag("finishEdge", 20, 0, true)).toBe("resize");
    expect(classifyDrag("finishEdge", 0, 20, true)).toBe("resize");
  });

  // A bar whose dates cannot change (e.g. a summary) can only be linked.
  it("only links when dates cannot change", () => {
    expect(classifyDrag("body", 20, 0, false)).toBe("link");
    expect(classifyDrag("finishEdge", 20, 0, false)).toBe("link");
  });
});

describe("daysFromPixels", () => {
  // Rounded to whole days, and never negative zero.
  it("rounds to whole days", () => {
    expect(daysFromPixels(42, { pxPerDay: 28 })).toBe(2);
    expect(daysFromPixels(41, { pxPerDay: 28 })).toBe(1);
    expect(Object.is(daysFromPixels(-10, { pxPerDay: 28 }), 0)).toBe(true);
    expect(daysFromPixels(-28, { pxPerDay: 28 })).toBe(-1);
  });
});

describe("snapMoveStart", () => {
  const monday = at("2027-03-01T08:00:00");

  // Mon + 4 days = Fri 08:00 (a working day: no snapping needed).
  it("moves by whole days to the first working minute", () => {
    expect(snapMoveStart(monday, 4, calendar())).toBe(at("2027-03-05T08:00:00"));
  });

  // Mon + 5 days = Saturday → the next working day, Mon 8 Mar 08:00.
  it("skips the weekend forward", () => {
    expect(snapMoveStart(monday, 5, calendar())).toBe(at("2027-03-08T08:00:00"));
    expect(snapMoveStart(monday, -1, calendar())).toBe(at("2027-03-01T08:00:00"));
  });

  // A holiday on Mon 8 Mar pushes it on to Tuesday.
  it("skips holidays", () => {
    const withHoliday = calendar({ holidays: ["2027-03-08"] });
    expect(snapMoveStart(monday, 5, withHoliday)).toBe(at("2027-03-09T08:00:00"));
  });

  // Without a calendar: midnight of the target day, no snapping.
  it("uses midnight without a calendar", () => {
    expect(snapMoveStart(monday, 5, null)).toBe(at("2027-03-06T00:00:00"));
  });
});

describe("snapResizeFinish", () => {
  const start = at("2027-03-01T08:00:00");
  const finish = at("2027-03-02T17:00:00");

  // Tue 17:00 + 1 day = Wed 17:00.
  it("moves the finish by whole days to the last working minute", () => {
    expect(snapResizeFinish(start, finish, 1, calendar())).toBe(at("2027-03-03T17:00:00"));
  });

  // Tue + 4 = Saturday → back to Fri 17:00; with a holiday on Friday → Thu.
  it("snaps back over weekends and holidays", () => {
    expect(snapResizeFinish(start, finish, 4, calendar())).toBe(at("2027-03-05T17:00:00"));
    const withHoliday = calendar({ holidays: ["2027-03-05"] });
    expect(snapResizeFinish(start, finish, 4, withHoliday)).toBe(at("2027-03-04T17:00:00"));
  });

  // A calendar exception makes Saturday a short working day (09:00–13:00).
  it("respects working exceptions", () => {
    const saturdayWork = calendar({
      exceptions: [
        {
          name: "Catch-up",
          start: "2027-03-06",
          finish: "2027-03-06",
          workingTimes: [{ startMinute: 540, endMinute: 780 }],
        },
      ],
    });
    expect(snapResizeFinish(start, finish, 4, saturdayWork)).toBe(at("2027-03-06T13:00:00"));
  });

  // Dragging far to the left stops at the end of the start's own day.
  it("never puts the finish before the start", () => {
    expect(snapResizeFinish(start, finish, -10, calendar())).toBe(at("2027-03-01T17:00:00"));
    // A manual task that starts on a Saturday: the finish is the end of
    // the next working day, still after the start.
    const saturday = at("2027-03-06T08:00:00");
    const result = snapResizeFinish(saturday, at("2027-03-06T17:00:00"), 0, calendar());
    expect(result).toBeGreaterThan(saturday);
    expect(result).toBe(at("2027-03-08T17:00:00"));
  });

  // Without a calendar: the end of the target day (midnight after it).
  it("uses the end of the day without a calendar", () => {
    expect(snapResizeFinish(start, finish, 1, null)).toBe(at("2027-03-04T00:00:00"));
    expect(snapResizeFinish(start, finish, -5, null)).toBe(at("2027-03-02T00:00:00"));
  });
});

describe("moveCommand", () => {
  // An automatic task gets a Start No Earlier Than constraint.
  it("pins an automatic task with SNET", () => {
    expect(moveCommand(taskRow(), at("2027-03-08T08:00:00"))).toEqual({
      type: "setTaskConstraint",
      id: "task-1",
      constraint: { type: "startNoEarlierThan", date: "2027-03-08T08:00:00" },
    });
  });

  // A manual task simply gets its new manual start.
  it("moves a manual task by its manual start", () => {
    expect(moveCommand(taskRow({ mode: "manual" }), at("2027-03-08T08:00:00"))).toEqual({
      type: "setTaskManualStart",
      id: "task-1",
      start: "2027-03-08T08:00:00",
    });
  });

  // Dropped where it already was: no command (no empty undo step).
  it("returns null when the start does not change", () => {
    expect(moveCommand(taskRow(), at("2027-03-01T08:00:00"))).toBeNull();
  });

  // A task without a start (failed schedule) still gets a command.
  it("moves a task that has no start yet", () => {
    expect(moveCommand(taskRow({ start: null }), at("2027-03-01T08:00:00"))).not.toBeNull();
  });
});

describe("resizeCommand", () => {
  // Whole minutes, never negative, null when unchanged.
  it("sets a rounded, non-negative duration", () => {
    expect(resizeCommand(taskRow(), 959.6)).toEqual({
      type: "setTaskDuration",
      id: "task-1",
      duration: { minutes: 960 },
    });
    expect(resizeCommand(taskRow(), -30)).toEqual({
      type: "setTaskDuration",
      id: "task-1",
      duration: { minutes: 0 },
    });
    expect(resizeCommand(taskRow(), 480)).toBeNull();
  });
});

describe("linkCommand", () => {
  // Dropping on another bar adds a finish-to-start link without lag.
  it("adds an FS link", () => {
    expect(linkCommand("task-1", "task-2", "link-9")).toEqual({
      type: "addDependency",
      dependency: {
        id: "link-9",
        predecessor: "task-1",
        successor: "task-2",
        kind: "finishToStart",
        lag: { minutes: 0 },
        lagPercent: null,
      },
    });
  });

  // Dropping on the same bar does nothing.
  it("refuses a link from a task to itself", () => {
    expect(linkCommand("task-1", "task-1", "link-9")).toBeNull();
  });
});
