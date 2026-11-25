/**
 * renderer/controller.ts: connects the canvas to scrolling, resizing and themes.
 *
 * What it is for:
 *   `draw.ts` knows *how* to paint one frame. This controller decides *when*
 *   to paint: after the user scrolls, after the window is resized, after the
 *   data or zoom changes, after the theme changes. It also keeps the canvas
 *   bitmap the right size for the screen (devicePixelRatio).
 *
 * Where it fits:
 *   `GanttView.tsx` creates one controller when the view appears and destroys
 *   it when the view disappears. The controller is plain TypeScript, not a
 *   React component: drawing at 60 fps must not go through React's render
 *   cycle, which is built for occasional UI updates, not for every frame.
 *
 * Main ideas:
 *   - **requestAnimationFrame (rAF).** The browser calls a function we give
 *     it right before it shows the next frame on screen (usually 60 times a
 *     second). We ask for a rAF only when something changed, and never more
 *     than one at a time. If the user scrolls 5 times within one frame, we
 *     still paint only once. When nothing changes, nothing runs: no wasted
 *     battery.
 *   - **Native scrolling.** A transparent `<div>` with real scrollbars lies
 *     on top of the canvas. Inside it, an empty "spacer" `<div>` as big as the
 *     whole chart makes the scrollbars the right size. The canvas itself never
 *     moves; it just repaints the part the scroll position points at. This
 *     gives us the operating system's own smooth scrolling, trackpad inertia
 *     and keyboard scrolling for free.
 */

import { canvasScale } from "@frontend/lib/uiZoom";
import { ganttPerfStats } from "../perfStats";
import { drawGanttFrame, type GanttOverlay } from "./draw";
import { createHeaderLabeler, type HeaderLabeler } from "./headerLabels";
import { HEADER_HEIGHT, ROW_HEIGHT, backingStoreSize } from "./layout";
import type { GanttModel } from "./model";
import { readGanttTheme, type GanttTheme } from "./theme";
import { msToX, xToMs, type TimeScale } from "./timeScale";

/** What `createGanttController` returns: the controller's public methods. */
export interface GanttController {
  /**
   * Gives the controller new data and/or a new time scale and repaints.
   * When only the zoom changes, the date at the left edge stays in place.
   */
  setData(model: GanttModel, scale: TimeScale): void;
  /**
   * Shows a date in the middle, and keeps it there while the zoom or the
   * window size changes, until the user scrolls.
   */
  centerDate(ms: number): void;
  /**
   * Sets the highlighted day.
   * @param ms - Today (UTC midnight, ms).
   */
  setToday(ms: number): void;
  /** Scrolls horizontally so that the given date is near the left edge. */
  scrollToDate(ms: number): void;
  /**
   * Highlights the selected rows and repaints.
   * @param row - Row of the primary selected task (outlined), or `null`.
   * @param rows - Rows of all selected tasks (highlight band). Default:
   *   just `row`.
   */
  setSelection(row: number | null, rows?: readonly number[]): void;
  /**
   * Sets the rule for shading non-working days (from the project calendar).
   * @param isNonWorkingDay - Gets a day (UTC midnight, ms), answers `true`
   *   for a day off; `undefined` = Saturday and Sunday.
   */
  setNonWorkingDays(isNonWorkingDay: ((dayMs: number) => boolean) | undefined): void;
  /**
   * Shows (or with `null` hides) the drag preview and repaints.
   * @param overlay - See `GanttOverlay` in draw.ts.
   */
  setOverlay(overlay: GanttOverlay | null): void;
  /**
   * Remembers a point that must stay in place during the *next* zoom
   * change (the mouse position for Ctrl+wheel zooming). Without it, the
   * left edge stays in place.
   * @param viewportX - x in the chart area (CSS px from its left edge).
   */
  setZoomAnchor(viewportX: number): void;
  /**
   * Scrolls so that a row is visible and the date is near the left edge.
   * Used for "jump to the driving task".
   * @param row - Row index.
   * @param ms - A date of that task (usually its start), or `NaN` to keep
   *   the horizontal position.
   */
  revealRow(row: number, ms: number): void;
  /** Stops listening to events. Call when the view is removed. */
  destroy(): void;
}

/** What the user does to move the chart (or begins a drag with). */
const USER_INPUT = ["wheel", "pointerdown", "keydown"] as const;

/** Options for {@link createGanttController}. */
export interface GanttControllerOptions {
  /** The canvas to paint on. */
  canvas: HTMLCanvasElement;
  /** The scrolling element lying on top of the canvas. */
  scroller: HTMLElement;
  /** Language for header labels, e.g. "en". */
  locale: string;
}

/**
 * Creates a controller for one Gantt canvas.
 *
 * @param options - Canvas, scroll element and language.
 * @returns The controller. Nothing is painted until `setData` is called.
 * @throws Error if the browser cannot create a 2D canvas context (very old
 *   or broken graphics setups).
 *
 * @example
 * const c = createGanttController({ canvas, scroller, locale: "en" });
 * c.setData(model, scale);
 * // later:
 * c.destroy();
 */
export function createGanttController(options: GanttControllerOptions): GanttController {
  const { canvas, scroller } = options;
  // `alpha: false` tells the browser the canvas is never see-through, which
  // lets it skip blending with whatever is behind: faster.
  const ctx = canvas.getContext("2d", { alpha: false });
  if (ctx === null) throw new Error("Canvas 2D is not available");
  // A const copy that TypeScript knows is never null, for use in functions.
  const context: CanvasRenderingContext2D = ctx;

  let model: GanttModel | null = null;
  let scale: TimeScale | null = null;
  // Row index of the selected task, or null. Set by `setSelection`.
  let selectedRow: number | null = null;
  // Rows of all selected tasks. Set by `setSelection`.
  let selectedRows: readonly number[] = [];
  // Non-working-day rule (undefined = weekends). Set by `setNonWorkingDays`.
  let isNonWorkingDay: ((dayMs: number) => boolean) | undefined;
  // The drag preview, or null. Set by `setOverlay`.
  let overlay: GanttOverlay | null = null;
  // Today (UTC midnight). Set by `setToday`.
  let todayMs: number | undefined;
  // Date kept in the middle, or null. Set by `centerDate`.
  let centredMs: number | null = null;
  // Point to keep in place at the next zoom change, or null (left edge).
  let zoomAnchorX: number | null = null;
  let theme: GanttTheme = readGanttTheme(scroller);
  const label: HeaderLabeler = createHeaderLabeler(options.locale);

  // Size of the canvas as last applied (CSS pixels and pixel ratio).
  let cssWidth = 0;
  let cssHeight = 0;
  let dpr = 0;
  // Id of the pending animation frame, or 0 if none is pending.
  let frameId = 0;
  // Horizontal scroll position at the last paint. Needed for zooming: by the
  // time `setData` runs, the browser may already have clamped `scrollLeft`
  // to the new (smaller) spacer width, so the live value can be wrong.
  let lastScrollLeft = 0;

  /**
   * Makes the canvas bitmap match the visible area and the pixel ratio.
   * Called at the start of every frame; does nothing if nothing changed.
   *
   * Changing `canvas.width` clears the bitmap and is slow, so we only do it
   * when the size or the pixel ratio really changed (e.g. the window was
   * dragged to a screen with a different resolution).
   */
  function syncCanvasSize(): void {
    // clientWidth/Height = the visible area without the scrollbars.
    const w = scroller.clientWidth;
    const h = scroller.clientHeight;
    // `canvasScale`: the UI zoom, where the pixel ratio does not contain it.
    const ratio = (window.devicePixelRatio || 1) * canvasScale();
    if (w === cssWidth && h === cssHeight && ratio === dpr) return;
    cssWidth = w;
    cssHeight = h;
    dpr = ratio;
    const size = backingStoreSize(w, h, ratio);
    canvas.width = size.width;
    canvas.height = size.height;
    canvas.style.width = `${w}px`;
    canvas.style.height = `${h}px`;
    // From now on, drawing at (1, 1) in our code lands on physical pixel
    // (dpr, dpr): we can think in CSS pixels and still get sharp output.
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
  }

  /** Scrolls so that `centredMs` is in the middle (if there is one). */
  function applyCentre(): void {
    if (centredMs === null || scale === null) return;
    const x = msToX(scale, centredMs) + scale.pxPerDay / 2;
    scroller.scrollLeft = Math.max(0, x - scroller.clientWidth / 2);
    lastScrollLeft = scroller.scrollLeft;
  }

  /** The user scrolls (or clicks): nothing is kept in the middle any more. */
  function releaseCentre(): void {
    centredMs = null;
  }

  /** The view changed its size. */
  function onResize(): void {
    applyCentre();
    requestPaint();
  }

  /** Paints one frame now. Called from requestAnimationFrame. */
  function paint(): void {
    frameId = 0;
    if (model === null || scale === null) return;
    syncCanvasSize();
    lastScrollLeft = scroller.scrollLeft;
    const t0 = performance.now();
    drawGanttFrame({
      ctx: context,
      width: cssWidth,
      height: cssHeight,
      dpr,
      scrollLeft: lastScrollLeft,
      scrollTop: scroller.scrollTop,
      model,
      scale,
      theme,
      label,
      selectedRow,
      selectedRows,
      isNonWorkingDay,
      todayMs,
      overlay,
    });
    ganttPerfStats.lastDrawMs = performance.now() - t0;
    ganttPerfStats.framesDrawn++;
  }

  /**
   * Asks for a repaint before the next screen refresh. Many calls within
   * one frame result in a single paint.
   */
  function requestPaint(): void {
    if (frameId !== 0) return;
    frameId = requestAnimationFrame(paint);
  }

  /** Re-reads colours (theme switched) and repaints. */
  function refreshTheme(): void {
    theme = readGanttTheme(scroller);
    requestPaint();
  }

  // --- Event wiring ---------------------------------------------------
  // `passive: true` promises the browser we will not cancel scrolling, so it
  // can scroll immediately without waiting for our code.
  scroller.addEventListener("scroll", requestPaint, { passive: true });

  for (const type of USER_INPUT) scroller.addEventListener(type, releaseCentre, { passive: true });

  // ResizeObserver tells us when the scroller changes size (window resized,
  // side panel opened…).
  const resizeObserver = new ResizeObserver(onResize);
  resizeObserver.observe(scroller);

  // The operating system switched between light and dark mode.
  const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");
  darkQuery.addEventListener("change", refreshTheme);

  // The app forced a theme via <html data-theme="…"> (Settings, M1).
  const themeObserver = new MutationObserver(refreshTheme);
  themeObserver.observe(document.documentElement, {
    attributes: true,
    // "data-ui-zoom": the UI zoom changed (lib/uiZoom.ts), so the size may.
    attributeFilter: ["data-theme", "data-ui-zoom"],
  });

  // devicePixelRatio changes when the window moves to another screen or the
  // user zooms. A media query for the *current* ratio fires when it changes;
  // we then listen again for the new ratio.
  let dprQuery: MediaQueryList | null = null;
  function watchPixelRatio(): void {
    dprQuery?.removeEventListener("change", onPixelRatioChange);
    dprQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    dprQuery.addEventListener("change", onPixelRatioChange);
  }
  function onPixelRatioChange(): void {
    watchPixelRatio();
    requestPaint();
  }
  watchPixelRatio();

  return {
    setData(nextModel, nextScale) {
      // Keep the date at the left edge in place when the zoom changes.
      // (React has already resized the spacer, so the new scroll position
      // is allowed.)
      // With a zoom anchor (Ctrl+wheel), the date under the mouse stays put.
      if (scale !== null && scale.pxPerDay !== nextScale.pxPerDay) {
        const anchor = zoomAnchorX ?? 0;
        const anchorDate = xToMs(scale, lastScrollLeft + anchor);
        scroller.scrollLeft = Math.max(0, msToX(nextScale, anchorDate) - anchor);
        // Two zoom steps can arrive before the next paint.
        lastScrollLeft = scroller.scrollLeft;
      }
      zoomAnchorX = null;
      model = nextModel;
      scale = nextScale;
      applyCentre();
      requestPaint();
    },

    centerDate(ms) {
      centredMs = ms;
      applyCentre();
      requestPaint();
    },

    setToday(ms) {
      todayMs = ms;
      requestPaint();
    },

    scrollToDate(ms) {
      releaseCentre();
      if (scale === null) return;
      // A little room (two days) before the date, so it is not glued to the edge.
      scroller.scrollLeft = Math.max(0, msToX(scale, ms) - 2 * scale.pxPerDay);
      requestPaint();
    },

    setSelection(row, rows) {
      selectedRow = row;
      selectedRows = rows ?? (row === null ? [] : [row]);
      requestPaint();
    },

    setNonWorkingDays(rule) {
      isNonWorkingDay = rule;
      requestPaint();
    },

    setOverlay(next) {
      overlay = next;
      requestPaint();
    },

    setZoomAnchor(viewportX) {
      zoomAnchorX = viewportX;
    },

    revealRow(row, ms) {
      releaseCentre();
      const top = row * ROW_HEIGHT;
      const bodyHeight = scroller.clientHeight - HEADER_HEIGHT;
      // Scroll only if the row is outside the visible rows; then centre it.
      if (top < scroller.scrollTop || top + ROW_HEIGHT > scroller.scrollTop + bodyHeight) {
        scroller.scrollTop = Math.max(0, top - bodyHeight / 2);
      }
      if (scale !== null && !Number.isNaN(ms)) {
        const x = msToX(scale, ms);
        const left = scroller.scrollLeft;
        if (x < left || x > left + scroller.clientWidth - 40) {
          scroller.scrollLeft = Math.max(0, x - 2 * scale.pxPerDay);
        }
      }
      requestPaint();
    },

    destroy() {
      if (frameId !== 0) cancelAnimationFrame(frameId);
      scroller.removeEventListener("scroll", requestPaint);
      for (const type of USER_INPUT) scroller.removeEventListener(type, releaseCentre);
      resizeObserver.disconnect();
      darkQuery.removeEventListener("change", refreshTheme);
      themeObserver.disconnect();
      dprQuery?.removeEventListener("change", onPixelRatioChange);
    },
  };
}
