/**
 * tests/frontend/grid/outline.test.ts: tests for the outline helpers of the
 * task grid.
 *
 * What it checks:
 *   `frontend/features/grid/outline.ts`, the small functions that read the
 *   indent structure (WBS) of the task list:
 *   - `subtreeEnd`: where a task's block (the task and its subtasks) ends;
 *   - `visibleRowIndices` / `filterCollapsed`: which rows stay visible when
 *     some summary tasks are collapsed (the grid and the Gantt both use it);
 *   - `topLevelSelection`: the selected tasks without the ones already
 *     inside another selected task (for delete, indent, copy);
 *   - `moveTargetIndex`: where a dragged block lands (for `moveTask`);
 *   - `parentIndex`: the summary task a row belongs to.
 *
 * Why it matters:
 *   Every outline action (collapse, delete, drag to move) depends on these
 *   answers. An off-by-one here would hide the wrong rows or move a block
 *   to the wrong place.
 *
 * Main ideas:
 *   The test outline (level in brackets):
 *     0 A(1)
 *     1   B(2)
 *     2     C(3)
 *     3   D(2)
 *     4 E(1)
 *     5 F(1)
 */

import { describe, expect, it } from "vitest";
import {
  filterCollapsed,
  moveTargetIndex,
  parentIndex,
  subtreeEnd,
  topLevelSelection,
  visibleRowIndices,
  type OutlineRow,
} from "@frontend/features/grid/outline";
import { shedSnapshot, taskId } from "../gantt/helpers";

/** The test outline described in the file header. */
const ROWS: OutlineRow[] = [
  { id: "A", outlineLevel: 1 },
  { id: "B", outlineLevel: 2 },
  { id: "C", outlineLevel: 3 },
  { id: "D", outlineLevel: 2 },
  { id: "E", outlineLevel: 1 },
  { id: "F", outlineLevel: 1 },
];

describe("subtreeEnd", () => {
  // A's block is A, B, C, D (rows 0–3), so it ends before row 4.
  it("finds the end of a summary task's block", () => {
    expect(subtreeEnd(ROWS, 0)).toBe(4);
    expect(subtreeEnd(ROWS, 1)).toBe(3);
  });

  // A task without subtasks is a block of one row.
  it("gives index + 1 for a leaf and for the last row", () => {
    expect(subtreeEnd(ROWS, 2)).toBe(3);
    expect(subtreeEnd(ROWS, 4)).toBe(5);
    expect(subtreeEnd(ROWS, 5)).toBe(6);
  });

  // A row that does not exist: the index itself (an empty block).
  it("returns the index for a row that does not exist", () => {
    expect(subtreeEnd(ROWS, 9)).toBe(9);
    expect(subtreeEnd([], 0)).toBe(0);
  });
});

describe("visibleRowIndices", () => {
  // Nothing collapsed: every row is visible.
  it("shows every row when nothing is collapsed", () => {
    expect(visibleRowIndices(ROWS, new Set())).toEqual([0, 1, 2, 3, 4, 5]);
  });

  // Collapsing A hides B, C and D, but A itself stays visible.
  it("hides the subtasks of a collapsed summary", () => {
    expect(visibleRowIndices(ROWS, new Set(["A"]))).toEqual([0, 4, 5]);
    expect(visibleRowIndices(ROWS, new Set(["B"]))).toEqual([0, 1, 3, 4, 5]);
  });

  // A collapsed row inside a collapsed row changes nothing more.
  it("handles nested collapsed rows and collapsed leaves", () => {
    expect(visibleRowIndices(ROWS, new Set(["A", "B"]))).toEqual([0, 4, 5]);
    expect(visibleRowIndices(ROWS, new Set(["C", "F"]))).toEqual([0, 1, 2, 3, 4, 5]);
  });
});

describe("filterCollapsed", () => {
  /** The shed snapshot, with rows 2–3 indented under row 1. */
  function outlinedShed() {
    const snapshot = shedSnapshot();
    const tasks = snapshot.tasks.map((task, i) =>
      i === 1 || i === 2 ? { ...task, outlineLevel: 2 } : task,
    );
    return { ...snapshot, tasks };
  }

  // Nothing collapsed: the very same object comes back (no copy), which
  // lets React skip work.
  it("returns the same snapshot when nothing is collapsed", () => {
    const snapshot = outlinedShed();
    expect(filterCollapsed(snapshot, new Set())).toBe(snapshot);
  });

  // Collapsing task 1 removes tasks 2 and 3 from the snapshot's task list;
  // everything else about the snapshot stays the same.
  it("drops the rows hidden by collapsed summaries", () => {
    const snapshot = outlinedShed();
    const filtered = filterCollapsed(snapshot, new Set([taskId(1)]));
    expect(filtered.tasks.map((t) => t.id)).toEqual([1, 4, 5, 6, 7, 8].map(taskId));
    expect(filtered.projectName).toBe(snapshot.projectName);
    expect(snapshot.tasks).toHaveLength(8); // the original is not changed
  });
});

describe("topLevelSelection", () => {
  // A selected summary already includes its subtasks: C is dropped.
  it("drops selected rows that are inside another selected row", () => {
    expect(topLevelSelection(ROWS, new Set(["A", "C", "E"]))).toEqual(["A", "E"]);
  });

  // Siblings and unrelated rows all stay, in row order.
  it("keeps rows that are not inside each other, in row order", () => {
    expect(topLevelSelection(ROWS, new Set(["D", "C"]))).toEqual(["C", "D"]);
    expect(topLevelSelection(ROWS, new Set())).toEqual([]);
    expect(topLevelSelection(ROWS, new Set(["unknown"]))).toEqual([]);
  });
});

describe("moveTargetIndex", () => {
  // B's block is rows 1–2 (2 rows). Dropping it above A puts it at row 0.
  it("moves a block up", () => {
    expect(moveTargetIndex(ROWS, 1, 0)).toBe(0);
  });

  // Dropping B's block above E (row 4): after taking out its 2 rows, E is
  // at row 2, so the block's new index is 2.
  it("moves a block down, counting without the block", () => {
    expect(moveTargetIndex(ROWS, 1, 4)).toBe(2);
    expect(moveTargetIndex(ROWS, 1, 6)).toBe(4); // to the very end
  });

  // Dropping on the block itself, or right below it, moves nothing.
  it("returns null when nothing would move", () => {
    expect(moveTargetIndex(ROWS, 1, 1)).toBeNull();
    expect(moveTargetIndex(ROWS, 1, 2)).toBeNull();
    expect(moveTargetIndex(ROWS, 1, 3)).toBeNull();
  });

  // A source row that does not exist cannot be moved.
  it("returns null for an unknown source row", () => {
    expect(moveTargetIndex(ROWS, 9, 0)).toBeNull();
  });
});

describe("parentIndex", () => {
  // C's parent is B, D's parent is A.
  it("finds the nearest row above with a smaller level", () => {
    expect(parentIndex(ROWS, 2)).toBe(1);
    expect(parentIndex(ROWS, 3)).toBe(0);
    expect(parentIndex(ROWS, 1)).toBe(0);
  });

  // Top-level rows and rows that do not exist have no parent.
  it("returns null for top-level and unknown rows", () => {
    expect(parentIndex(ROWS, 0)).toBeNull();
    expect(parentIndex(ROWS, 4)).toBeNull();
    expect(parentIndex(ROWS, 42)).toBeNull();
  });
});
