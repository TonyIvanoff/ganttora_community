/**
 * features/toolbars/useToolbarDrag.ts: dragging a toolbar with the pointer.
 *
 * What it is for:
 *   Follows a press on a toolbar's grip, works out where a drop would put
 *   the toolbar, and on release puts it there. Escape (or losing the
 *   pointer) cancels and changes nothing.
 *
 * Where it fits:
 *   Used by `ToolbarFrame.tsx`, which draws the highlighted drop zone. The
 *   rules (which edge, which place) are the pure functions of `layout.ts`.
 *
 * Main ideas:
 *   - **Pointer events** cover mouse, pen and touch. *Pointer capture* sends
 *     all further moves to the grip, even when the pointer leaves it.
 *   - A press only becomes a drag after a few pixels, so a simple click
 *     still opens the grip menu.
 */

import { useCallback, useEffect, useRef, useState, type PointerEvent, type RefObject } from "react";
import type { ToolbarDock } from "@frontend/ipc/types";
import {
  clampPosition,
  detectDockZone,
  hasRows,
  insertIndexAt,
  moveToolbar,
  rowAt,
  type Box,
  type Insets,
  type Point,
  type RowBand,
} from "./layout";
import { useToolbarStore } from "./toolbarStore";

/** How far the pointer must move before a press becomes a drag (CSS px). */
export const DRAG_THRESHOLD = 4;

/** What the frame draws during a drag. */
export interface ToolbarDrag {
  /** The dragged toolbar. */
  id: string;
  /** Where a drop now would put it. */
  zone: ToolbarDock;
  /** Its place among the toolbars there (of that row). */
  index: number;
  /** The row, at the top or bottom. */
  row: number;
  /** A drop makes a new row. */
  newRow: boolean;
  /** Where a floating drop would land. */
  ghost: Box;
  /** Depth of the docked toolbars per edge (size of the highlight). */
  insets: Insets;
  /** The insertion line inside an occupied dock, or `null`. */
  marker: Box | null;
}

/** The handlers a grip or title bar needs. */
export interface DragHandlers {
  /** Press. */
  onPointerDown: (event: PointerEvent<HTMLElement>) => void;
  /** Move. */
  onPointerMove: (event: PointerEvent<HTMLElement>) => void;
  /** Release. */
  onPointerUp: (event: PointerEvent<HTMLElement>) => void;
  /** The system took the pointer away. */
  onPointerCancel: () => void;
  /** Same, or our own release. */
  onLostPointerCapture: () => void;
}

/** A press in progress. */
interface Press {
  /** The pressed toolbar. */
  id: string;
  /** Where it was pressed, in window coordinates. */
  start: Point;
  /** Pointer minus the toolbar's corner, so it does not jump when floating. */
  offset: Point;
  /** The toolbar's size. */
  size: { width: number; height: number };
}

/** An element's rectangle inside the frame. */
function boxIn(frame: DOMRect, element: Element): Box {
  const r = element.getBoundingClientRect();
  return { x: r.left - frame.left, y: r.top - frame.top, width: r.width, height: r.height };
}

/** The line drawn where a drop would insert the toolbar. */
function markerAt(boxes: readonly Box[], index: number, zone: ToolbarDock): Box | null {
  const next = boxes[index];
  const box = next ?? boxes.at(-1);
  if (box === undefined) return null;
  if (zone === "left" || zone === "right") {
    return { x: box.x, y: next ? box.y : box.y + box.height, width: box.width, height: 3 };
  }
  return { x: next ? box.x : box.x + box.width, y: box.y, width: 3, height: box.height };
}

/**
 * Toolbar dragging.
 *
 * @param frameRef - The main frame. Docks inside it carry `data-dock`,
 *   toolbars carry `data-toolbar-id`.
 * @returns The current drag (or `null`), a function giving the handlers for
 *   one toolbar's grip, and `wasDragged()`: `true` when the click that
 *   follows a drag should be ignored.
 */
export function useToolbarDrag(frameRef: RefObject<HTMLDivElement | null>): {
  drag: ToolbarDrag | null;
  handlersFor: (id: string) => DragHandlers;
  wasDragged: () => boolean;
} {
  const [drag, setDrag] = useState<ToolbarDrag | null>(null);
  const press = useRef<Press | null>(null);
  /** The same as `drag`, readable at once inside event handlers. */
  const live = useRef<ToolbarDrag | null>(null);
  const ignoreClick = useRef(false);

  /** Shows (or with `null` ends) the drag. */
  const show = useCallback((next: ToolbarDrag | null): void => {
    live.current = next;
    setDrag(next);
  }, []);

  /** Ends the press without moving anything. */
  const cancel = useCallback((): void => {
    if (press.current === null) return;
    if (live.current !== null) ignoreClick.current = true;
    press.current = null;
    show(null);
    setTimeout(() => (ignoreClick.current = false), 0);
  }, [show]);

  // Escape cancels. `true` = capture phase: we hear it before anyone else.
  const dragging = drag !== null;
  useEffect(() => {
    if (!dragging) return;
    function onKeyDown(event: KeyboardEvent): void {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      cancel();
    }
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [dragging, cancel]);

  /** Works out what a drop at the pointer would do. */
  function measure(event: PointerEvent<HTMLElement>, p: Press): ToolbarDrag | null {
    const frameEl = frameRef.current;
    if (frameEl === null) return null;
    const frame = frameEl.getBoundingClientRect();
    const point = { x: event.clientX - frame.left, y: event.clientY - frame.top };
    const insets: Insets = {};
    for (const dock of ["top", "bottom", "left", "right"] as const) {
      const el = frameEl.querySelector(`[data-dock="${dock}"]`);
      if (el === null) continue;
      const r = el.getBoundingClientRect();
      insets[dock] = dock === "top" || dock === "bottom" ? r.height : r.width;
    }
    const zone = detectDockZone(point, frame, insets);
    const others = (parent: Element | null) =>
      [...(parent?.querySelectorAll("[data-toolbar-id]") ?? [])]
        .filter((el) => el.getAttribute("data-toolbar-id") !== p.id)
        .map((el) => boxIn(frame, el));
    const dockEl = frameEl.querySelector(`[data-dock="${zone}"]`);
    let place = { row: 0, newRow: false };
    let boxes: Box[];
    if (hasRows(zone)) {
      const rowEls = [...(dockEl?.querySelectorAll("[data-toolbar-row]") ?? [])];
      const bands = rowEls.map((el): RowBand => {
        const r = boxIn(frame, el);
        return { top: r.y, bottom: r.y + r.height, vacated: others(el).length === 0 };
      });
      place = rowAt(bands, point.y, zone);
      const kept = rowEls.filter((_, i) => bands[i]?.vacated === false);
      boxes = place.newRow ? [] : others(kept[place.row] ?? null);
    } else {
      boxes = others(dockEl);
    }
    const index = zone === "float" ? 0 : insertIndexAt(boxes, point, zone);
    const corner = clampPosition(
      { x: point.x - p.offset.x, y: point.y - p.offset.y },
      p.size,
      frame,
    );
    return {
      id: p.id,
      zone,
      index,
      ...place,
      insets,
      ghost: { ...corner, ...p.size },
      marker: zone === "float" ? null : markerAt(boxes, index, zone),
    };
  }

  /** The handlers of one toolbar. */
  function handlersFor(id: string): DragHandlers {
    return {
      onPointerDown: (event) => {
        if (event.button !== 0) return; // left button, touch or pen only
        const bar = event.currentTarget.closest("[data-toolbar-id]");
        if (bar === null) return;
        const r = bar.getBoundingClientRect();
        // `?.()`: the method is missing in the tests' imitation browser.
        event.currentTarget.setPointerCapture?.(event.pointerId);
        press.current = {
          id,
          start: { x: event.clientX, y: event.clientY },
          // A long toolbar wraps when it floats: keep the corner near.
          offset: {
            x: Math.min(event.clientX - r.left, 60),
            y: Math.min(event.clientY - r.top, 12),
          },
          size: { width: r.width, height: r.height },
        };
      },
      onPointerMove: (event) => {
        const p = press.current;
        if (p === null) return;
        const moved = Math.hypot(event.clientX - p.start.x, event.clientY - p.start.y);
        if (live.current === null && moved < DRAG_THRESHOLD) return;
        show(measure(event, p));
      },
      onPointerUp: () => {
        const drop = live.current;
        press.current = null;
        show(null);
        if (drop === null) return; // a click, not a drag
        ignoreClick.current = true;
        setTimeout(() => (ignoreClick.current = false), 0);
        useToolbarStore
          .getState()
          .update((bars) =>
            moveToolbar(
              bars,
              drop.id,
              drop.zone === "float"
                ? { dock: "float", position: { x: drop.ghost.x, y: drop.ghost.y } }
                : { dock: drop.zone, index: drop.index, row: drop.row, newRow: drop.newRow },
            ),
          );
      },
      onPointerCancel: cancel,
      onLostPointerCapture: cancel,
    };
  }

  return { drag, handlersFor, wasDragged: () => ignoreClick.current };
}
