/**
 * features/toolbars/layout.ts: the rules of where toolbars sit. No screen,
 * no React: only plain data in, plain data out.
 *
 * What it is for:
 *   - which edge a dragged toolbar would dock to ({@link detectDockZone});
 *   - between which neighbours it lands ({@link insertIndexAt});
 *   - keeping a floating toolbar inside the window ({@link clampPosition});
 *   - moving, adding, editing toolbars in the list;
 *   - repairing a saved layout ({@link cleanToolbars}), so a damaged
 *     settings file can never leave the app without usable toolbars.
 *
 * Where it fits:
 *   Used by `toolbarStore.ts`, `ToolbarFrame.tsx` and `CustomizeDialog.tsx`;
 *   tested in `tests/frontend/toolbars/layout.test.ts`.
 *
 * Main ideas:
 *   - The layout is one list. Toolbars on the same edge are shown in list
 *     order, so "order" needs no extra number.
 *   - Functions never change their input; they return a new list.
 */

import type { ToolbarDock, ToolbarLayout } from "@frontend/ipc/types";
import { isActionId, type ActionId } from "./actions";

/** The entry of a toolbar that draws a dividing line. */
export const SEPARATOR = "separator";

/** Most toolbars kept. Must match `MAX_TOOLBARS` in `settings.rs`. */
export const MAX_TOOLBARS = 30;
/** Most entries on one toolbar. Must match `MAX_TOOLBAR_ITEMS` in `settings.rs`. */
export const MAX_TOOLBAR_ITEMS = 60;
/** Longest toolbar name. Must match `MAX_TOOLBAR_NAME` in `settings.rs`. */
export const MAX_TOOLBAR_NAME = 40;

/** Most rows of toolbars at the top and at the bottom. Must match `MAX_TOOLBAR_ROWS` in `settings.rs`. */
export const MAX_TOOLBAR_ROWS = 3;
/** How far past the docked rows (CSS px) a drop still makes a new row. */
export const NEW_ROW_DISTANCE = 40;

/** How near to an edge (CSS px) a drop docks, when nothing is docked there yet. */
export const DOCK_DISTANCE = 48;

/** The toolbars every installation has. They can be hidden, not deleted. */
export const PREDEFINED_IDS = ["main", "task", "view", "legend"] as const;
/** One of {@link PREDEFINED_IDS}. */
export type PredefinedId = (typeof PREDEFINED_IDS)[number];

/** The buttons of the predefined toolbars. */
const PREDEFINED_ITEMS: Record<PredefinedId, readonly (ActionId | typeof SEPARATOR)[]> = {
  main: ["fileSave", SEPARATOR, "undo", "redo"],
  task: [
    "addProject",
    "addTask",
    "addSubtask",
    "addMilestone",
    "linkTasks",
    SEPARATOR,
    "outdent",
    "indent",
    SEPARATOR,
    "addBlocker",
    "addRisk",
    "markOnTrack",
    "setPlan",
    "taskInfo",
  ],
  view: [
    "zoomDays",
    "zoomWeeks",
    "zoomMonths",
    SEPARATOR,
    "zoomOut",
    "zoomReset",
    "zoomIn",
    "goToToday",
    "explain",
  ],
  legend: ["legend"],
};

/**
 * Buttons the predefined toolbars had in earlier versions. A saved toolbar
 * that still has exactly one of these was never customized, so it gets the
 * new buttons; a customized one stays as the user made it.
 */
const EARLIER_ITEMS: Partial<Record<PredefinedId, readonly string[]>> = {
  task: [
    "addTask,addSubtask,separator,outdent,indent,separator,addBlocker,taskInfo",
    "addTask,addSubtask,separator,outdent,indent,separator,linkTasks,addBlocker,taskInfo",
    "addProject,addTask,addSubtask,linkTasks,separator,outdent,indent,separator,addBlocker,taskInfo",
    "addProject,addTask,addSubtask,linkTasks,separator,outdent,indent,separator,addBlocker,addRisk,taskInfo",
    "addProject,addTask,addSubtask,linkTasks,separator,outdent,indent,separator,addBlocker,addRisk,markOnTrack,taskInfo",
    "addProject,addTask,addSubtask,addMilestone,linkTasks,separator,outdent,indent,separator,addBlocker,addRisk,markOnTrack,taskInfo",
  ],
  view: [
    "zoomDays,zoomWeeks,zoomMonths,separator,zoomOut,zoomIn,explain",
    "zoomDays,zoomWeeks,zoomMonths,separator,zoomOut,zoomReset,zoomIn,explain",
  ],
};

/**
 * Gives an untouched predefined toolbar its newer buttons.
 *
 * @param id - The toolbar.
 * @param items - Its saved entries.
 * @returns The entries to use.
 */
function upgradeItems(id: string, items: string[]): string[] {
  if (!isPredefined(id)) return items;
  return EARLIER_ITEMS[id]?.includes(items.join()) === true ? [...PREDEFINED_ITEMS[id]] : items;
}

/** All docks, for checking saved values. */
const DOCKS: readonly ToolbarDock[] = ["top", "bottom", "left", "right", "float"];

/** A point or a toolbar's top-left corner, in CSS px inside the frame. */
export interface Point {
  /** From the left. */
  x: number;
  /** From the top. */
  y: number;
}

/** A width and a height in CSS px. */
export interface Size {
  /** Width. */
  width: number;
  /** Height. */
  height: number;
}

/** A rectangle inside the frame. */
export interface Box extends Point, Size {}

/** How much of each edge is already taken by docked toolbars (CSS px). */
export type Insets = Partial<Record<Exclude<ToolbarDock, "float">, number>>;

/**
 * Whether a toolbar is a predefined one.
 *
 * @param id - The toolbar's ID.
 */
export function isPredefined(id: string): id is PredefinedId {
  return (PREDEFINED_IDS as readonly string[]).includes(id);
}

/**
 * A predefined toolbar as it is after installing.
 *
 * @param id - Which one.
 * @returns A fresh copy (changing it is safe).
 */
export function predefinedToolbar(id: PredefinedId): ToolbarLayout {
  return {
    id,
    name: "",
    items: [...PREDEFINED_ITEMS[id]],
    dock: "top",
    row: 0,
    x: 0,
    y: 0,
    visible: true,
  };
}

/** The toolbars after installing: Main, Task, View and Legend, at the top. */
export function defaultToolbars(): ToolbarLayout[] {
  return PREDEFINED_IDS.map(predefinedToolbar);
}

/**
 * Cleans a toolbar's entries: unknown actions and repeats are left out
 * (separators may repeat).
 *
 * @param items - Any list of texts.
 * @returns The usable entries, in order.
 */
export function cleanItems(items: readonly string[]): string[] {
  const seen = new Set<string>();
  return items
    .filter((item) => {
      if (item === SEPARATOR) return true;
      if (!isActionId(item) || seen.has(item)) return false;
      seen.add(item);
      return true;
    })
    .slice(0, MAX_TOOLBAR_ITEMS);
}

/** A whole, non-negative number, or 0. */
const coordinate = (n: number) => (Number.isFinite(n) ? Math.max(0, Math.round(n)) : 0);

/**
 * Repairs a saved layout.
 *
 * Unknown actions, repeated toolbars and unnamed custom toolbars are left
 * out; a predefined toolbar that is missing comes back. An empty list (a
 * fresh installation) gives the defaults.
 *
 * @param saved - What `settings.json` held (Rust has already checked the types).
 * @returns A layout that always contains the predefined toolbars.
 *
 * @example
 * cleanToolbars([]); // Main, Task, View, Legend at the top
 */
export function cleanToolbars(saved: readonly ToolbarLayout[] | undefined): ToolbarLayout[] {
  const result: ToolbarLayout[] = [];
  const seen = new Set<string>();
  for (const bar of saved ?? []) {
    const name = (bar.name ?? "").trim().slice(0, MAX_TOOLBAR_NAME);
    if (seen.has(bar.id) || (!isPredefined(bar.id) && name === "")) continue;
    if (!isPredefined(bar.id) && result.length >= MAX_TOOLBARS - PREDEFINED_IDS.length) continue;
    seen.add(bar.id);
    result.push({
      id: bar.id,
      name: isPredefined(bar.id) ? "" : name,
      items: upgradeItems(bar.id, cleanItems(bar.items)),
      dock: DOCKS.includes(bar.dock) ? bar.dock : "top",
      row: Math.min(MAX_TOOLBAR_ROWS - 1, coordinate(bar.row ?? 0)),
      x: coordinate(bar.x),
      y: coordinate(bar.y),
      visible: bar.visible,
    });
  }
  for (const id of PREDEFINED_IDS) if (!seen.has(id)) result.push(predefinedToolbar(id));
  return result;
}

/**
 * Finds where a toolbar dropped at `point` would go.
 *
 * Each edge has a landing strip: {@link DOCK_DISTANCE} deep, or a little
 * deeper than the toolbars already docked there (at the top and bottom
 * deep enough for one more row). In a corner the nearest
 * edge wins. Outside the frame counts as "at the edge".
 *
 * @param point - The pointer, inside the frame's coordinates.
 * @param frame - The frame's size.
 * @param insets - Depth of the docked toolbars on each edge.
 * @returns The edge, or `"float"` in the middle.
 *
 * @example
 * detectDockZone({ x: 400, y: 10 }, { width: 800, height: 600 }); // "top"
 * detectDockZone({ x: 400, y: 300 }, { width: 800, height: 600 }); // "float"
 */
export function detectDockZone(point: Point, frame: Size, insets: Insets = {}): ToolbarDock {
  const distances = [
    ["top", point.y],
    ["left", point.x],
    ["right", frame.width - point.x],
    ["bottom", frame.height - point.y],
  ] as const;
  let best: ToolbarDock = "float";
  let bestScore = 1;
  for (const [edge, distance] of distances) {
    // 0 at the edge, 1 at the end of the landing strip.
    const past = hasRows(edge) ? NEW_ROW_DISTANCE : DOCK_DISTANCE / 2;
    const depth = Math.max(DOCK_DISTANCE, (insets[edge] ?? 0) + past);
    const score = Math.max(0, distance) / depth;
    if (score < bestScore) {
      best = edge;
      bestScore = score;
    }
  }
  return best;
}

/**
 * Finds between which docked toolbars a drop lands.
 *
 * @param boxes - The toolbars on that edge (without the dragged one), in order.
 * @param point - The pointer, in the same coordinates.
 * @param dock - The edge: top and bottom read like text (rows, then left
 *   to right), left and right from top to bottom.
 * @returns How many toolbars come before it (0 = first).
 *
 * @example
 * insertIndexAt([{ x: 0, y: 0, width: 100, height: 30 }], { x: 200, y: 10 }, "top"); // 1
 */
export function insertIndexAt(boxes: readonly Box[], point: Point, dock: ToolbarDock): number {
  const isBefore = (box: Box): boolean => {
    if (dock === "left" || dock === "right") return box.y + box.height / 2 < point.y;
    if (box.y + box.height <= point.y) return true; // a row further up
    if (box.y > point.y) return false; // a row further down
    return box.x + box.width / 2 < point.x;
  };
  const index = boxes.findIndex((box) => !isBefore(box));
  return index === -1 ? boxes.length : index;
}

/**
 * Moves a floating toolbar's corner so that the toolbar stays in the frame.
 *
 * @param position - The wanted corner.
 * @param size - The toolbar's size.
 * @param frame - The frame's size.
 * @returns The nearest corner that fits; a toolbar larger than the frame
 *   sits at 0, so its title bar stays reachable.
 *
 * @example
 * clampPosition({ x: 900, y: 10 }, { width: 100, height: 40 }, { width: 800, height: 600 });
 * // { x: 700, y: 10 }
 */
export function clampPosition(position: Point, size: Size, frame: Size): Point {
  return {
    x: Math.round(Math.max(0, Math.min(position.x, frame.width - size.width))),
    y: Math.round(Math.max(0, Math.min(position.y, frame.height - size.height))),
  };
}

/**
 * The visible toolbars of one dock, in order.
 *
 * @param toolbars - The layout.
 * @param dock - Which edge, or `"float"`.
 */
export function toolbarsAt(toolbars: readonly ToolbarLayout[], dock: ToolbarDock): ToolbarLayout[] {
  return toolbars.filter((bar) => bar.visible && bar.dock === dock);
}

/**
 * Whether toolbars at this place stand in rows.
 *
 * @param dock - The place.
 */
export function hasRows(dock: ToolbarDock): dock is "top" | "bottom" {
  return dock === "top" || dock === "bottom";
}

/**
 * The visible toolbars of the top or bottom edge, row by row. Row numbers
 * that no visible toolbar has are skipped, so there are no empty rows.
 *
 * @param toolbars - The layout.
 * @param dock - The edge.
 * @returns The rows from top to bottom.
 *
 * @example
 * rowsAt(defaultToolbars(), "top").length; // 1
 */
export function rowsAt(toolbars: readonly ToolbarLayout[], dock: ToolbarDock): ToolbarLayout[][] {
  const bars = toolbarsAt(toolbars, dock);
  const numbers = [...new Set(bars.map((bar) => bar.row))].sort((a, b) => a - b);
  return numbers.map((row) => bars.filter((bar) => bar.row === row));
}

/** One row on the screen, for {@link rowAt}. */
export interface RowBand {
  /** Top edge. */
  top: number;
  /** Bottom edge. */
  bottom: number;
  /** Only the dragged toolbar is in it, so it disappears when that leaves. */
  vacated: boolean;
}

/**
 * Finds the row a drop lands in.
 *
 * Past the rows (below them at the top edge, above them at the bottom edge)
 * a new row is made, up to {@link MAX_TOOLBAR_ROWS}. Over the row the
 * dragged toolbar has for itself, it keeps that row.
 *
 * @param bands - The rows on the screen, from top to bottom.
 * @param y - The pointer.
 * @param dock - The edge.
 * @returns The row, counted without the vacated one, and whether it is new.
 *
 * @example
 * rowAt([{ top: 0, bottom: 40, vacated: false }], 60, "top"); // { row: 1, newRow: true }
 */
export function rowAt(
  bands: readonly RowBand[],
  y: number,
  dock: "top" | "bottom",
): { row: number; newRow: boolean } {
  const stay = bands.findIndex((band) => band.vacated && y >= band.top && y <= band.bottom);
  if (stay !== -1) return { row: stay, newRow: bands.length > 1 };
  const rows = bands.filter((band) => !band.vacated);
  const first = rows[0];
  const last = rows.at(-1);
  if (first === undefined || last === undefined) return { row: 0, newRow: false };
  if (rows.length < MAX_TOOLBAR_ROWS) {
    if (dock === "top" && y > last.bottom) return { row: rows.length, newRow: true };
    if (dock === "bottom" && y < first.top) return { row: 0, newRow: true };
  }
  const row = rows.findIndex((band) => y <= band.bottom);
  return { row: row === -1 ? rows.length - 1 : row, newRow: false };
}

/** Where a toolbar should go. */
export interface ToolbarPlace {
  /** The edge, or `"float"`. */
  dock: ToolbarDock;
  /** Place among the toolbars of that edge or row (default: last). */
  index?: number;
  /** Row at the top or bottom, counted without the moved toolbar (default: the last). */
  row?: number;
  /** Make a new row there; the rows from it on move down. */
  newRow?: boolean;
  /** Corner, for `"float"`. */
  position?: Point;
}

/**
 * Moves a toolbar (and shows it).
 *
 * @param toolbars - The layout.
 * @param id - The toolbar; an unknown one changes nothing.
 * @param place - Where to.
 * @returns The new layout.
 */
export function moveToolbar(
  toolbars: readonly ToolbarLayout[],
  id: string,
  place: ToolbarPlace,
): ToolbarLayout[] {
  const bar = toolbars.find((b) => b.id === id);
  if (bar === undefined) return [...toolbars];
  let rest = toolbars.filter((b) => b.id !== id);
  let row = 0;
  if (hasRows(place.dock)) {
    const rows = rowsAt(rest, place.dock);
    const newRow = place.newRow === true && rows.length > 0 && rows.length < MAX_TOOLBAR_ROWS;
    const lastRow = newRow ? rows.length : Math.max(0, rows.length - 1);
    row = Math.min(Math.max(0, place.row ?? lastRow), lastRow);
    // Number the visible rows 0, 1, 2 and make room for a new one.
    rest = rest.map((b) => {
      if (b.dock !== place.dock || !b.visible) return b;
      const now = rows.findIndex((bars) => bars.includes(b));
      return { ...b, row: newRow && now >= row ? now + 1 : now };
    });
  }
  const neighbours = toolbarsAt(rest, place.dock).filter(
    (b) => !hasRows(place.dock) || b.row === row,
  );
  const next = neighbours[place.index ?? neighbours.length];
  const last = neighbours.at(-1);
  // Before the toolbar now at that place; else just after the last one.
  const at =
    next !== undefined
      ? rest.indexOf(next)
      : last !== undefined
        ? rest.indexOf(last) + 1
        : rest.length;
  const moved = {
    ...bar,
    dock: place.dock,
    row,
    visible: true,
    x: coordinate(place.position?.x ?? bar.x),
    y: coordinate(place.position?.y ?? bar.y),
  };
  return [...rest.slice(0, at), moved, ...rest.slice(at)];
}

/**
 * Changes some fields of one toolbar.
 *
 * @param toolbars - The layout.
 * @param id - The toolbar.
 * @param patch - The changed fields.
 */
export function patchToolbar(
  toolbars: readonly ToolbarLayout[],
  id: string,
  patch: Partial<Omit<ToolbarLayout, "id">>,
): ToolbarLayout[] {
  return toolbars.map((bar) => (bar.id === id ? { ...bar, ...patch } : bar));
}

/**
 * Adds an empty custom toolbar at the top edge.
 *
 * @param toolbars - The layout.
 * @param id - A new, unused ID.
 * @param name - Its name.
 * @returns The new layout; unchanged if there are already {@link MAX_TOOLBARS}.
 */
export function addToolbar(
  toolbars: readonly ToolbarLayout[],
  id: string,
  name: string,
): ToolbarLayout[] {
  if (toolbars.length >= MAX_TOOLBARS) return [...toolbars];
  return [...toolbars, { id, name, items: [], dock: "top", row: 0, x: 0, y: 0, visible: true }];
}

/**
 * Deletes a custom toolbar (predefined ones stay).
 *
 * @param toolbars - The layout.
 * @param id - The toolbar.
 */
export function removeToolbar(toolbars: readonly ToolbarLayout[], id: string): ToolbarLayout[] {
  return toolbars.filter((bar) => bar.id !== id || isPredefined(id));
}

/**
 * Moves one entry of a list by one place.
 *
 * @param items - The entries.
 * @param index - Which one.
 * @param step - `-1` up, `1` down.
 * @returns A new list; unchanged when the move would leave the list.
 *
 * @example
 * moveItem(["a", "b"], 1, -1); // ["b", "a"]
 */
export function moveItem(items: readonly string[], index: number, step: -1 | 1): string[] {
  const result = [...items];
  const a = result[index];
  const b = result[index + step];
  if (a === undefined || b === undefined) return result;
  result[index] = b;
  result[index + step] = a;
  return result;
}
