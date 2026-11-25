/**
 * features/grid/GridSplitter.tsx: the thin bar between the grid and the
 * Gantt chart that changes how wide the grid is.
 *
 * What it is for:
 *   Drag it with the mouse, or focus it with Tab and press ← / → (Shift for
 *   bigger steps), Home (narrowest) or End (all columns visible).
 *
 * Where it fits:
 *   Rendered by `shell/AppShell.tsx` between `TaskGrid` and `GanttView`.
 *   The width is stored in `gridStore.width`.
 *
 * Main ideas:
 *   - `role="separator"` with `aria-valuenow` is the accessible pattern for
 *     a "window splitter": screen readers announce it and its position.
 *   - **Pointer capture.** `setPointerCapture` keeps sending pointer moves
 *     to the bar even when the mouse leaves it during a fast drag.
 */

import { useRef, type KeyboardEvent, type PointerEvent } from "react";
import { useTranslation } from "react-i18next";
import { MIN_GRID_WIDTH, useGridStore } from "./gridStore";

/** Pixels per arrow key press (Shift: ×5). */
const KEY_STEP = 16;
/** Width at which every column is visible (End key). */
const FULL_WIDTH = 940;

/**
 * The splitter bar.
 *
 * @returns A focusable vertical bar.
 */
export function GridSplitter() {
  const { t } = useTranslation();
  const width = useGridStore((s) => s.width);
  const setWidth = useGridStore((s) => s.setWidth);
  /** Pointer x and grid width when the drag started, or `null`. */
  const drag = useRef<{ x: number; width: number } | null>(null);

  /** Starts a drag. */
  function onPointerDown(event: PointerEvent<HTMLDivElement>): void {
    drag.current = { x: event.clientX, width };
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  /** Follows the pointer while dragging. */
  function onPointerMove(event: PointerEvent<HTMLDivElement>): void {
    if (drag.current === null) return;
    setWidth(drag.current.width + event.clientX - drag.current.x);
  }

  /** Arrow keys, Home and End. */
  function onKeyDown(event: KeyboardEvent<HTMLDivElement>): void {
    const step = event.shiftKey ? KEY_STEP * 5 : KEY_STEP;
    if (event.key === "ArrowLeft") setWidth(width - step);
    else if (event.key === "ArrowRight") setWidth(width + step);
    else if (event.key === "Home") setWidth(MIN_GRID_WIDTH);
    else if (event.key === "End") setWidth(FULL_WIDTH);
    else return;
    event.preventDefault();
  }

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={t("grid.splitter")}
      aria-valuenow={width}
      aria-valuemin={MIN_GRID_WIDTH}
      tabIndex={0}
      className="w-1 shrink-0 cursor-col-resize bg-line hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={() => (drag.current = null)}
      onPointerCancel={() => (drag.current = null)}
      onKeyDown={onKeyDown}
    />
  );
}
