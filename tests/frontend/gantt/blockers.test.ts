/**
 * tests/frontend/gantt/blockers.test.ts: tests for blocked time in the
 * Gantt chart.
 *
 * What it checks:
 *   - `snapshotToDetailedGanttTasks` and `prepareGanttModel` carry the work
 *     and blocked pieces ("segments") of a task to the chart, with the
 *     blocker's reason for the tooltip;
 *   - `barBox` covers the whole bar, blocked time included;
 *   - `hitTestBlocked` finds the blocked piece under the mouse, and
 *     `hitTestBar` still finds the task on every piece.
 *
 * Why it matters:
 *   The striped pieces tell the user which part of a bar is a delay. A
 *   wrong piece or tooltip would blame the wrong days.
 *
 * Main ideas:
 *   Row 0 is a *split* task: work, blocked, work. Row 1 is blocked at its
 *   end. Row 2 has no blockers. Row 3 is a milestone (no work) that a
 *   blocker moved. Positions are calculated with `msToX`, the same function
 *   the chart draws with.
 */

import { describe, expect, it } from "vitest";
import { snapshotToDetailedGanttTasks } from "@frontend/features/gantt/fromSnapshot";
import { parseIsoLocal } from "@frontend/features/gantt/renderer/dates";
import {
  HIT_TOLERANCE,
  MIN_BAR_WIDTH,
  barBox,
  hitTestBar,
  hitTestBlocked,
} from "@frontend/features/gantt/renderer/hitTest";
import { BAR_HEIGHT, HEADER_HEIGHT, ROW_HEIGHT } from "@frontend/features/gantt/renderer/layout";
import { prepareGanttModel, type GanttRow } from "@frontend/features/gantt/renderer/model";
import { ZOOM_LEVELS, createTimeScale, msToX } from "@frontend/features/gantt/renderer/timeScale";
import type { BlockerRow, ScheduleSnapshot, Segment } from "@frontend/ipc/types";
import { taskRow } from "../grid/helpers";
import { shedSnapshot } from "./helpers";

/** A blocker row from `start` to `finish`, one day long. */
function blocker(id: string, start: string, finish: string): BlockerRow {
  return {
    id,
    reason: `Reason ${id}`,
    afterWorkMinutes: 480,
    durationMinutes: 480,
    reportedOn: "2027-03-01",
    start,
    finish,
  };
}

/** A work piece. */
function work(start: string, finish: string): Segment {
  return { start, finish, kind: { type: "work" } };
}

/** A blocked piece that belongs to blocker `id`. */
function blocked(id: string, start: string, finish: string): Segment {
  return { start, finish, kind: { type: "blocked", blocker: id } };
}

const MON = ["2027-03-01T08:00:00", "2027-03-01T17:00:00"] as const;
const TUE = ["2027-03-02T08:00:00", "2027-03-02T17:00:00"] as const;
const WED = ["2027-03-03T08:00:00", "2027-03-03T17:00:00"] as const;

/** The four rows described at the top of the file. */
function snapshot(): ScheduleSnapshot {
  const span = { start: MON[0], finish: WED[1] };
  return {
    ...shedSnapshot(),
    tasks: [
      taskRow({
        id: "split",
        ...span,
        segments: [work(...MON), blocked("b1", ...TUE), work(...WED)],
        blockers: [blocker("b1", ...TUE)],
      }),
      taskRow({
        id: "end",
        ...span,
        segments: [work(MON[0], TUE[1]), blocked("b2", ...WED)],
        blockers: [blocker("b2", ...WED)],
      }),
      taskRow({ id: "plain", ...span, segments: [] }),
      taskRow({
        id: "milestone",
        durationMinutes: 0,
        isMilestone: true,
        start: TUE[0],
        finish: WED[1],
        segments: [blocked("b3", TUE[0], WED[1])],
        blockers: [blocker("b3", TUE[0], WED[1])],
      }),
    ],
  };
}

/** The chart's model and "days" scale, and shortcuts for positions. */
function chart(pxOffset = 0) {
  const model = prepareGanttModel(snapshotToDetailedGanttTasks(snapshot()));
  const scale = createTimeScale(model.minMs, model.maxMs, ZOOM_LEVELS.days, 0);
  /** The x position of a date on screen (scrolled right by `pxOffset`). */
  const x = (iso: string) => msToX(scale, parseIsoLocal(iso)) - pxOffset;
  return { model, scale, x };
}

/** Row `i` of the model, or a test failure. */
function row(rows: readonly GanttRow[], i: number): GanttRow {
  const r = rows[i];
  if (r === undefined) throw new Error(`row ${i} missing`);
  return r;
}

/** The y coordinate in the middle of row `i`. */
function midY(i: number): number {
  return HEADER_HEIGHT + i * ROW_HEIGHT + ROW_HEIGHT / 2;
}

/** The ID of the blocker under a point, or null. */
function blockerAt(c: ReturnType<typeof chart>, x: number, y: number): string | null {
  const hit = hitTestBlocked({ ...c, x, y, scrollLeft: 0, scrollTop: 0 });
  return hit?.segment.blocker?.id ?? null;
}

describe("segments from snapshot to model", () => {
  // Split bar: three pieces without gaps; only the middle one is blocked
  // and carries the tooltip's facts.
  it("keeps work, blocked and work pieces of a split bar", () => {
    const pieces = row(chart().model.rows, 0).segments;
    expect(pieces).toEqual([
      { startMs: parseIsoLocal(MON[0]), finishMs: parseIsoLocal(MON[1]), blocker: null },
      {
        startMs: parseIsoLocal(TUE[0]),
        finishMs: parseIsoLocal(TUE[1]),
        blocker: {
          id: "b1",
          reason: "Reason b1",
          durationMinutes: 480,
          reportedOn: "2027-03-01",
        },
      },
      { startMs: parseIsoLocal(WED[0]), finishMs: parseIsoLocal(WED[1]), blocker: null },
    ]);
  });

  // A blocker at the end: the last piece ends exactly where the bar ends.
  it("keeps a blocked piece at the end of the bar", () => {
    const task = row(chart().model.rows, 1);
    const last = task.segments?.at(-1);
    expect(last?.blocker?.id).toBe("b2");
    expect(last?.finishMs).toBe(task.finishMs);
  });

  // Ordinary tasks (no segments, or an empty list) use the simple bar.
  it("gives null for a task without pieces", () => {
    expect(row(chart().model.rows, 2).segments).toBeNull();
    const source = snapshotToDetailedGanttTasks(shedSnapshot());
    expect(prepareGanttModel(source).rows.every((r) => r.segments === null)).toBe(true);
    const empty = source.map((t) => ({ ...t, segments: [] }));
    expect(prepareGanttModel(empty).rows.every((r) => r.segments === null)).toBe(true);
  });

  // A piece whose blocker is missing must still be drawn as blocked.
  it("keeps a blocked piece whose blocker is unknown", () => {
    const s = snapshot();
    const tasks = s.tasks.map((t) => ({ ...t, blockers: [] }));
    const source = snapshotToDetailedGanttTasks({ ...s, tasks });
    expect(source[0]?.segments?.[1]?.blocker).toEqual({
      id: "b1",
      reason: "",
      durationMinutes: 0,
      reportedOn: null,
    });
  });
});

describe("barBox with blockers", () => {
  // The box is the whole bar, so dragging and links use its real end.
  it("spans split and end-blocked bars from start to finish", () => {
    const c = chart();
    for (const i of [0, 1]) {
      const box = barBox(row(c.model.rows, i), i, c.scale, 0, 0);
      expect(box?.x).toBeCloseTo(c.x(MON[0]), 9);
      expect((box?.x ?? 0) + (box?.width ?? 0)).toBeCloseTo(c.x(WED[1]), 9);
      expect(box?.isMilestone).toBe(false);
    }
  });

  // A blocked milestone happens at its finish, so the diamond is there.
  it("centres a blocked milestone's diamond on its finish", () => {
    const c = chart();
    const box = barBox(row(c.model.rows, 3), 3, c.scale, 0, 0);
    expect(box?.isMilestone).toBe(true);
    expect((box?.x ?? 0) + BAR_HEIGHT / 2).toBeCloseTo(c.x(WED[1]), 9);
  });
});

describe("hitTestBlocked", () => {
  // Split bar: only the middle piece answers; its neighbours are work.
  it("finds the blocked piece inside a split bar only", () => {
    const c = chart();
    expect(blockerAt(c, (c.x(TUE[0]) + c.x(TUE[1])) / 2, midY(0))).toBe("b1");
    expect(blockerAt(c, c.x(TUE[0]) + 0.01, midY(0))).toBe("b1");
    expect(blockerAt(c, c.x(TUE[1]) - 0.01, midY(0))).toBe("b1");
    expect(blockerAt(c, (c.x(MON[0]) + c.x(MON[1])) / 2, midY(0))).toBeNull();
    expect(blockerAt(c, (c.x(WED[0]) + c.x(WED[1])) / 2, midY(0))).toBeNull();
  });

  // Blocker at the end: hit up to the bar's end, not beyond. The row comes
  // back too, so the tooltip knows its task.
  it("finds the blocked piece at the end of a bar", () => {
    const c = chart();
    const x = (c.x(WED[0]) + c.x(WED[1])) / 2;
    const input = { ...c, x, y: midY(1), scrollLeft: 0, scrollTop: 0 };
    expect(hitTestBlocked(input)?.row).toBe(1);
    expect(blockerAt(c, x, midY(1))).toBe("b2");
    expect(blockerAt(c, c.x(TUE[0]), midY(1))).toBeNull();
    expect(blockerAt(c, c.x(WED[1]) + 1, midY(1))).toBeNull();
  });

  // Outside the bar vertically, on the header, on rows without pieces and
  // below the last row there is nothing.
  it("finds nothing outside the blocked pieces", () => {
    const c = chart();
    const x = (c.x(TUE[0]) + c.x(TUE[1])) / 2;
    expect(blockerAt(c, x, midY(0) - BAR_HEIGHT / 2 - 1)).toBeNull();
    expect(blockerAt(c, x, midY(0) + BAR_HEIGHT / 2 + 1)).toBeNull();
    expect(blockerAt(c, x, HEADER_HEIGHT - 1)).toBeNull();
    expect(blockerAt(c, x, midY(2))).toBeNull();
    expect(blockerAt(c, x, midY(50))).toBeNull();
  });

  // Scrolling moves the bars on screen; the hit must move with them.
  it("takes scrolling into account", () => {
    const c = chart(30);
    const x = (c.x(WED[0]) + c.x(WED[1])) / 2;
    const input = { ...c, x, y: midY(0), scrollLeft: 30, scrollTop: ROW_HEIGHT };
    // Scrolled down one row: row 1 is on the first line.
    expect(hitTestBlocked(input)?.segment.blocker?.id).toBe("b2");
  });

  // A one-minute blocker is thinner than a pixel; it still gets the
  // minimum width so that the mouse can find it.
  it("widens a very short blocked piece", () => {
    const s = snapshot();
    const short = [blocked("b1", TUE[0], "2027-03-02T08:01:00")];
    const tasks = s.tasks.map((t, i) => (i === 0 ? { ...t, segments: short } : t));
    const model = prepareGanttModel(snapshotToDetailedGanttTasks({ ...s, tasks }));
    const c = { ...chart(), model };
    expect(blockerAt(c, c.x(TUE[0]) + MIN_BAR_WIDTH - 0.01, midY(0))).toBe("b1");
    expect(blockerAt(c, c.x(TUE[0]) + MIN_BAR_WIDTH + 0.01, midY(0))).toBeNull();
  });
});

describe("hitTestBar with blockers", () => {
  // Clicking any piece, work or blocked, selects the task. For a blocked
  // milestone that includes the striped part left of the diamond.
  it("selects the task on every piece", () => {
    const c = chart();
    const at = (x: number, i: number) =>
      hitTestBar({ ...c, x, y: midY(i), scrollLeft: 0, scrollTop: 0 });
    for (const day of [MON, TUE, WED]) {
      expect(at((c.x(day[0]) + c.x(day[1])) / 2, 0)).toBe(0);
    }
    expect(at(c.x(WED[0]), 1)).toBe(1);
    expect(at(c.x(TUE[0]) + 1, 3)).toBe(3);
    expect(at(c.x(TUE[0]) - HIT_TOLERANCE - 1, 3)).toBeNull();
  });
});
