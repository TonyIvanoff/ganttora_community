/**
 * renderer/layout.ts: vertical layout and "what is visible?" maths.
 *
 * What it is for:
 *   Answers: at which y position is row N drawn? Which rows are on screen
 *   right now? How big must the canvas bitmap be on this screen? All as pure
 *   functions, so they can be unit-tested (tests/frontend/gantt).
 *
 * Main ideas:
 *   - **Virtualisation.** The chart is 10,000 rows × 28 px = 280,000 px tall,
 *     but the window shows maybe 30 rows. Drawing only those 30 (plus the
 *     links that cross the screen) keeps each frame cheap, no matter how big
 *     the project is. `visibleRowRange` finds those rows with one division
 *     instead of looking at every row.
 *   - **devicePixelRatio.** On a "retina" or high-DPI screen one CSS pixel is
 *     made of 2×2 (or 1.5×1.5…) physical pixels. A canvas whose bitmap has
 *     only one pixel per CSS pixel looks blurry there. So the bitmap must be
 *     `cssSize × devicePixelRatio` pixels big; see `backingStoreSize`.
 */

/** Height of one task row in CSS pixels. */
export const ROW_HEIGHT = 28;

/** Height of a task bar inside its row. */
export const BAR_HEIGHT = 14;

/** Height of one header tier (the header has two: e.g. months over days). */
export const HEADER_TIER_HEIGHT = 24;

/** Total header height: two tiers. */
export const HEADER_HEIGHT = 2 * HEADER_TIER_HEIGHT;

/** A range of rows: `first` is included, `end` is not (like `slice`). */
export interface RowRange {
  /** Index of the first visible row. */
  first: number;
  /** One past the last visible row. `first === end` means "no rows". */
  end: number;
}

/**
 * Finds the rows that are at least partly visible.
 *
 * @param scrollTop - How far the chart is scrolled down, in CSS pixels.
 * @param viewportHeight - Height of the visible area, including the header.
 * @param rowCount - Number of rows in the chart.
 * @param rowHeight - Height of one row (default {@link ROW_HEIGHT}).
 * @param headerHeight - Height of the fixed header (default {@link HEADER_HEIGHT}).
 * @returns The visible rows. Empty if the viewport has no room below the header.
 *
 * @example
 * visibleRowRange(0, 48 + 280, 10_000); // { first: 0, end: 10 }
 * visibleRowRange(14, 48 + 280, 10_000); // { first: 0, end: 11 }: row 10 peeks in
 */
export function visibleRowRange(
  scrollTop: number,
  viewportHeight: number,
  rowCount: number,
  rowHeight: number = ROW_HEIGHT,
  headerHeight: number = HEADER_HEIGHT,
): RowRange {
  const bodyHeight = viewportHeight - headerHeight;
  if (bodyHeight <= 0 || rowCount <= 0) return { first: 0, end: 0 };
  const first = Math.max(0, Math.floor(scrollTop / rowHeight));
  const end = Math.min(rowCount, Math.ceil((scrollTop + bodyHeight) / rowHeight));
  return { first: Math.min(first, end), end };
}

/**
 * Gives the y position of the top of a row on the screen.
 *
 * @param row - Row index (0 = first task).
 * @param scrollTop - Vertical scroll position in CSS pixels.
 * @returns y in CSS pixels from the top of the canvas (header included).
 *
 * @example
 * rowTop(0, 0) === HEADER_HEIGHT; // first row starts right under the header
 */
export function rowTop(row: number, scrollTop: number): number {
  return HEADER_HEIGHT + row * ROW_HEIGHT - scrollTop;
}

/**
 * Total height of the scrollable content: header plus all rows.
 *
 * @param rowCount - Number of rows.
 * @returns Height in CSS pixels.
 */
export function contentHeight(rowCount: number): number {
  return HEADER_HEIGHT + rowCount * ROW_HEIGHT;
}

/**
 * Computes the canvas bitmap size for a given CSS size and pixel ratio.
 *
 * @param cssWidth - Width of the canvas on the page, in CSS pixels.
 * @param cssHeight - Height in CSS pixels.
 * @param dpr - `window.devicePixelRatio` (1 on normal screens, 2 on retina).
 * @returns Whole numbers of physical pixels for `canvas.width` / `canvas.height`.
 *
 * @example
 * backingStoreSize(800, 600, 2); // { width: 1600, height: 1200 }
 */
export function backingStoreSize(
  cssWidth: number,
  cssHeight: number,
  dpr: number,
): { width: number; height: number } {
  const ratio = dpr > 0 && Number.isFinite(dpr) ? dpr : 1;
  return {
    width: Math.max(1, Math.round(cssWidth * ratio)),
    height: Math.max(1, Math.round(cssHeight * ratio)),
  };
}

/**
 * Moves a coordinate so a thin line lands exactly on physical pixels.
 *
 * @param v - Coordinate in CSS pixels.
 * @param dpr - Device pixel ratio.
 * @param physicalWidth - Width of the line in *physical* pixels (default 1).
 * @returns The snapped coordinate in CSS pixels.
 *
 * Why: canvas lines are centred on their coordinate. A 1-pixel line drawn
 * on the *border* between two pixels is smeared over both and looks grey
 * and blurry. A line with an odd physical width must sit on the centre of a
 * pixel (+0.5); a line with an even width must sit on a border.
 *
 * @example
 * crisp(10, 1) === 10.5;
 * crisp(10, 2) === 10.25;    // 20.5 physical pixels ÷ 2
 * crisp(10, 2, 2) === 10;    // 2-pixel line: on the border
 */
export function crisp(v: number, dpr: number, physicalWidth: number = 1): number {
  const offset = physicalWidth % 2 === 1 ? 0.5 : 0;
  return (Math.round(v * dpr) + offset) / dpr;
}

/**
 * Width, in physical pixels, of a "1 CSS pixel" line on this screen.
 *
 * @param dpr - Device pixel ratio.
 * @returns At least 1; 2 on a retina screen (dpr 2), 1 on dpr 1 or 1.25.
 */
export function physicalLineWidth(dpr: number): number {
  return Math.max(1, Math.round(dpr));
}
