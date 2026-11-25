/**
 * tests/frontend/toolbars/layout.test.ts: the pure toolbar rules.
 *
 * What it checks (`frontend/features/toolbars/layout.ts`): which edge a
 * drop docks to, the order of toolbars, staying inside the window, and
 * repairing a saved layout.
 *
 * Why it matters:
 *   A damaged settings file or a small window must never leave the user
 *   without reachable toolbars.
 */

import { describe, expect, it } from "vitest";
import type { ToolbarLayout } from "@frontend/ipc/types";
import {
  addToolbar,
  clampPosition,
  cleanItems,
  cleanToolbars,
  defaultToolbars,
  detectDockZone,
  insertIndexAt,
  MAX_TOOLBARS,
  moveItem,
  moveToolbar,
  patchToolbar,
  removeToolbar,
  rowAt,
  rowsAt,
  toolbarsAt,
} from "@frontend/features/toolbars/layout";

/** A frame for the drop tests. */
const FRAME = { width: 800, height: 600 };

/** A custom toolbar with some fields changed. */
function custom(changes: Partial<ToolbarLayout> = {}): ToolbarLayout {
  return {
    id: "mine",
    name: "Mine",
    items: [],
    dock: "top",
    row: 0,
    x: 0,
    y: 0,
    visible: true,
    ...changes,
  };
}

/** The IDs of a list. */
const ids = (bars: readonly ToolbarLayout[]) => bars.map((bar) => bar.id);

describe("detectDockZone", () => {
  it("docks near an edge and floats in the middle", () => {
    expect(detectDockZone({ x: 400, y: 10 }, FRAME)).toBe("top");
    expect(detectDockZone({ x: 10, y: 300 }, FRAME)).toBe("left");
    expect(detectDockZone({ x: 790, y: 300 }, FRAME)).toBe("right");
    expect(detectDockZone({ x: 400, y: 590 }, FRAME)).toBe("bottom");
    expect(detectDockZone({ x: 400, y: 300 }, FRAME)).toBe("float");
  });

  it("takes the nearest edge in a corner, and counts outside as the edge", () => {
    expect(detectDockZone({ x: 5, y: 20 }, FRAME)).toBe("left");
    expect(detectDockZone({ x: 20, y: 5 }, FRAME)).toBe("top");
    expect(detectDockZone({ x: 400, y: -50 }, FRAME)).toBe("top");
  });

  it("makes the strip deeper where toolbars are docked", () => {
    expect(detectDockZone({ x: 400, y: 80 }, FRAME)).toBe("float");
    expect(detectDockZone({ x: 400, y: 80 }, FRAME, { top: 80 })).toBe("top");
  });
});

describe("insertIndexAt", () => {
  const row = [
    { x: 0, y: 0, width: 100, height: 30 },
    { x: 100, y: 0, width: 100, height: 30 },
    { x: 0, y: 30, width: 100, height: 30 },
  ];
  it("reads rows like text", () => {
    expect(insertIndexAt(row, { x: 20, y: 10 }, "top")).toBe(0);
    expect(insertIndexAt(row, { x: 120, y: 10 }, "top")).toBe(1);
    expect(insertIndexAt(row, { x: 300, y: 10 }, "top")).toBe(2);
    expect(insertIndexAt(row, { x: 300, y: 40 }, "top")).toBe(3);
  });
  it("reads columns from top to bottom", () => {
    const column = [
      { x: 0, y: 0, width: 80, height: 100 },
      { x: 0, y: 100, width: 80, height: 100 },
    ];
    expect(insertIndexAt(column, { x: 40, y: 40 }, "left")).toBe(0);
    expect(insertIndexAt(column, { x: 40, y: 120 }, "right")).toBe(1);
    expect(insertIndexAt([], { x: 0, y: 0 }, "left")).toBe(0);
  });
});

describe("clampPosition", () => {
  it("keeps a floating toolbar inside, also when it is too large", () => {
    const size = { width: 100, height: 40 };
    expect(clampPosition({ x: 900, y: -5 }, size, FRAME)).toEqual({ x: 700, y: 0 });
    expect(clampPosition({ x: 50, y: 60 }, size, FRAME)).toEqual({ x: 50, y: 60 });
    expect(clampPosition({ x: 50, y: 60 }, size, { width: 60, height: 20 })).toEqual({
      x: 0,
      y: 0,
    });
  });
});

describe("changing the list", () => {
  it("moves between neighbours", () => {
    const bars = defaultToolbars();
    expect(ids(moveToolbar(bars, "view", { dock: "top", index: 0 }))).toEqual([
      "view",
      "main",
      "task",
      "legend",
    ]);
    expect(ids(moveToolbar(bars, "main", { dock: "top" }))).toEqual([
      "task",
      "view",
      "legend",
      "main",
    ]);
    const left = moveToolbar(bars, "task", { dock: "left" });
    expect(ids(toolbarsAt(left, "left"))).toEqual(["task"]);
    expect(ids(toolbarsAt(left, "top"))).toEqual(["main", "view", "legend"]);
  });

  it("ignores hidden neighbours, shows a moved toolbar, keeps the input", () => {
    const bars = patchToolbar(defaultToolbars(), "main", { visible: false });
    const moved = moveToolbar(bars, "view", { dock: "top", index: 0 });
    expect(ids(toolbarsAt(moved, "top"))).toEqual(["view", "task", "legend"]);
    expect(
      moveToolbar(bars, "main", { dock: "float", position: { x: 5.4, y: 7 } }).find(
        (b) => b.id === "main",
      ),
    ).toMatchObject({ visible: true, dock: "float", x: 5, y: 7 });
    expect(bars[0]?.visible).toBe(false);
    expect(moveToolbar(bars, "nothing", { dock: "left" })).toEqual(bars);
  });

  it("adds and deletes custom toolbars only", () => {
    const bars = addToolbar(defaultToolbars(), "mine", "Mine");
    expect(ids(bars)).toContain("mine");
    expect(ids(removeToolbar(bars, "mine"))).not.toContain("mine");
    expect(ids(removeToolbar(bars, "main"))).toContain("main");
  });

  it("moves an entry by one place", () => {
    expect(moveItem(["a", "b", "c"], 2, -1)).toEqual(["a", "c", "b"]);
    expect(moveItem(["a", "b"], 1, 1)).toEqual(["a", "b"]);
  });
});

describe("cleaning a saved layout", () => {
  it("gives the predefined toolbars for an empty or missing one", () => {
    expect(cleanToolbars([])).toEqual(defaultToolbars());
    expect(cleanToolbars(undefined)).toEqual(defaultToolbars());
  });

  it("drops what is unknown and brings back what is missing", () => {
    const clean = cleanToolbars([
      custom({
        items: ["fileSave", "makeTea", "fileSave", "separator", "separator"],
        x: NaN,
        y: -3,
      }),
      custom(), // the same ID again
      custom({ id: "unnamed", name: "  " }),
      { ...custom({ id: "view", name: "Hacked" }), dock: "moon" as never },
    ]);
    expect(ids(clean)).toEqual(["mine", "view", "main", "task", "legend"]);
    expect(clean[0]).toMatchObject({ items: ["fileSave", "separator", "separator"], x: 0, y: 0 });
    expect(clean[1]).toMatchObject({ name: "", dock: "top" });
  });

  it("keeps a predefined toolbar hidden or empty if the user chose so", () => {
    const clean = cleanToolbars([{ ...custom({ id: "main", name: "" }), visible: false }]);
    expect(clean[0]).toMatchObject({ id: "main", visible: false, items: [] });
  });

  it("limits the number of toolbars but never loses a predefined one", () => {
    const many = Array.from({ length: 100 }, (_, i) => custom({ id: `c${i}` }));
    const clean = cleanToolbars(many);
    expect(clean).toHaveLength(MAX_TOOLBARS);
    expect(ids(clean)).toEqual(expect.arrayContaining(["main", "task", "view"]));
    expect(addToolbar(clean, "more", "More")).toHaveLength(MAX_TOOLBARS);
  });

  it("is not fooled by names every object has", () => {
    expect(cleanItems(["toString", "constructor", "undo"])).toEqual(["undo"]);
  });
});

describe("rows", () => {
  /** The IDs of every row at the top. */
  const rows = (bars: readonly ToolbarLayout[]) => rowsAt(bars, "top").map(ids);

  it("makes new rows, at most three, and leaves no empty one", () => {
    let bars = moveToolbar(defaultToolbars(), "legend", { dock: "top", row: 1, newRow: true });
    expect(rows(bars)).toEqual([["main", "task", "view"], ["legend"]]);
    bars = moveToolbar(bars, "view", { dock: "top", row: 2, newRow: true });
    expect(rows(bars)).toEqual([["main", "task"], ["legend"], ["view"]]);
    const fourth = moveToolbar(bars, "main", { dock: "top", row: 3, newRow: true });
    expect(rows(fourth)).toEqual([["task"], ["legend"], ["view", "main"]]);
    const joined = moveToolbar(bars, "legend", { dock: "top", row: 0, index: 1 });
    expect(rows(joined)).toEqual([["main", "legend", "task"], ["view"]]);
  });

  it("puts a new row above the others", () => {
    const bars = moveToolbar(defaultToolbars(), "view", { dock: "top", row: 0, newRow: true });
    expect(rows(bars)).toEqual([["view"], ["main", "task", "legend"]]);
  });

  it("forgets the row at the sides and repairs a bad one", () => {
    const bars = moveToolbar(defaultToolbars(), "view", { dock: "top", row: 1, newRow: true });
    expect(moveToolbar(bars, "view", { dock: "left" }).find((b) => b.id === "view")?.row).toBe(0);
    expect(cleanToolbars([custom({ row: 9 })])[0]?.row).toBe(2);
    // Saved before rows existed.
    expect(cleanToolbars([custom({ row: undefined as never })])[0]?.row).toBe(0);
  });

  it("finds the row under the pointer", () => {
    const band = (top: number, vacated = false) => ({ top, bottom: top + 40, vacated });
    expect(rowAt([], 10, "top")).toEqual({ row: 0, newRow: false });
    expect(rowAt([band(0)], 20, "top")).toEqual({ row: 0, newRow: false });
    expect(rowAt([band(0)], 60, "top")).toEqual({ row: 1, newRow: true });
    expect(rowAt([band(50)], 20, "bottom")).toEqual({ row: 0, newRow: true });
    const full = [band(0), band(40), band(80)];
    expect(rowAt(full, 130, "top")).toEqual({ row: 2, newRow: false });
    // Over its own row it stays; elsewhere that row no longer counts.
    const own = [band(0, true), band(44)];
    expect(rowAt(own, 20, "top")).toEqual({ row: 0, newRow: true });
    expect(rowAt(own, 60, "top")).toEqual({ row: 0, newRow: false });
  });
});
