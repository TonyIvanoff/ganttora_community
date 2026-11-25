/**
 * features/grid/gridStore.ts: view state that belongs only to the task grid.
 *
 * What it is for:
 *   The grid needs a little memory of its own: where a Shift range starts,
 *   which new task should open its name for typing, how wide the grid is
 *   next to the Gantt chart, and the warnings of the last paste.
 *
 * Where it fits:
 *   - The *selection* itself is shared with the Gantt chart and lives in the
 *     app store (`selectedTaskIds`, `selectedTaskId` = primary, in
 *     `state/store.ts`): clicking a row selects its bar and the other way
 *     round. This store only adds what the grid alone needs.
 *   - Kept apart from `state/store.ts` so the grid and the Gantt chart can
 *     be developed side by side without touching the same file.
 */

import { create } from "zustand";
import type { PasteWarning } from "@frontend/ipc/types";

/** Default width of the grid next to the Gantt chart, in CSS pixels. */
export const DEFAULT_GRID_WIDTH = 560;

/** Narrowest the grid may be made, in CSS pixels. */
export const MIN_GRID_WIDTH = 160;

/** The grid's own state and the functions that change it. */
export interface GridState {
  /** Where a Shift+click or Shift+arrow range starts, or `null`. */
  anchorId: string | null;
  /**
   * A task whose name cell should open for typing as soon as it appears
   * (set after "Insert task"; the new row arrives with the next snapshot).
   */
  pendingEditId: string | null;
  /** Width of the grid in CSS pixels (dragged with the splitter). */
  width: number;
  /** Warnings of the last paste, shown in a notice until dismissed. */
  pasteWarnings: readonly PasteWarning[];

  /**
   * Sets where a later Shift range starts.
   *
   * @param id - The anchor task, or `null`.
   */
  setAnchorId: (id: string | null) => void;
  /** Asks the grid to open the name of task `id` for typing (or `null`). */
  setPendingEditId: (id: string | null) => void;
  /** Sets the grid width (limited to at least {@link MIN_GRID_WIDTH}). */
  setWidth: (width: number) => void;
  /** Shows (or with `[]` hides) the paste warnings notice. */
  setPasteWarnings: (warnings: readonly PasteWarning[]) => void;
}

/**
 * The grid store hook.
 *
 * @example
 * const width = useGridStore((s) => s.width);
 */
export const useGridStore = create<GridState>()((set) => ({
  anchorId: null,
  pendingEditId: null,
  width: DEFAULT_GRID_WIDTH,
  pasteWarnings: [],

  setAnchorId: (id) => set({ anchorId: id }),
  setPendingEditId: (id) => set({ pendingEditId: id }),
  setWidth: (width) => set({ width: Math.max(MIN_GRID_WIDTH, Math.round(width)) }),
  setPasteWarnings: (warnings) => set({ pasteWarnings: warnings }),
}));
