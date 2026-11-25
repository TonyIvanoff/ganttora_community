/**
 * renderer/draw.ts: paints one frame of the Gantt chart onto a canvas.
 *
 * What it is for:
 *   Given the data (model), the time scale, the scroll position and the
 *   colours, `drawGanttFrame` paints everything that is visible: the
 *   selected-row highlight, non-working-day shading, grid lines, task bars
 *   (milestone diamonds, summary brackets, manual tasks in their own
 *   colour, blocked time in striped red), deadline markers, constraint pins, dependency
 *   links of all four types, the drag preview (a ghost bar or a rubber-band
 *   link line while the user drags) and the two-tier timescale header.
 *
 * Where it fits:
 *   Called by `controller.ts` inside `requestAnimationFrame`, at most once
 *   per screen refresh. It does not know about React.
 *
 * Main ideas for speed (goal: 60 frames per second = 16.7 ms per frame,
 * with 10,000 tasks):
 *   1. **Draw only what is visible** (virtualisation): rows come from
 *      `visibleRowRange`, header cells from `visibleTicks`.
 *   2. **Batch drawing by colour.** Every `fill()` or `stroke()` call has a
 *      fixed cost. Instead of filling each bar separately, we add all normal
 *      bars to one *path* and fill it once, then all critical bars in a
 *      second path. Same for lines.
 *   3. **No allocations in hot loops where avoidable**, so the garbage
 *      collector rarely has to pause the app.
 *   4. **Blockers take the slow path only where they exist**.
 *      A task with blockers is drawn piece by piece (work, blocked, work);
 *      every other task stays one rectangle. The check is a single
 *      `segments !== null` per visible row.
 *
 * Beginner note on Canvas 2D: a `<canvas>` is a grid of pixels. The
 * "context" (`ctx`) is the paint brush: `ctx.fillStyle = "red"` picks a
 * colour, `ctx.fillRect(x, y, w, h)` paints a rectangle. A *path* is a shape
 * you describe first (`beginPath`, `rect`, `moveTo`, `lineTo`) and paint
 * afterwards in one go (`fill` or `stroke`). Nothing on a canvas is an
 * object you can move later: to change the picture, you paint it again.
 */

import { isWeekend } from "./dates";
import type { HeaderLabeler } from "./headerLabels";
import {
  barBox,
  deadlineBox,
  FINISH_ARROW,
  finishArrowBox,
  isLateToStart,
  isMissedMilestone,
  lateStartBox,
  type BarBox,
} from "./hitTest";
import {
  BAR_HEIGHT,
  HEADER_HEIGHT,
  HEADER_TIER_HEIGHT,
  ROW_HEIGHT,
  crisp,
  physicalLineWidth,
  rowTop,
  visibleRowRange,
} from "./layout";
import { linkPath, type BarEnds, type LinkPath } from "./links";
import type { GanttModel } from "./model";
import { higherPriority, lateStartRisk } from "@frontend/features/task-info/risks";
import { projectTint } from "@frontend/lib/projectTint";
import type { GanttTheme } from "./theme";
import { labelStride, msToX, tickIndex, type TimeScale, visibleTicks } from "./timeScale";

/** Everything one frame needs. */
export interface FrameInput {
  /** The canvas paint brush, already scaled for devicePixelRatio. */
  ctx: CanvasRenderingContext2D;
  /** Canvas width in CSS pixels. */
  width: number;
  /** Canvas height in CSS pixels. */
  height: number;
  /** devicePixelRatio, for crisp 1-pixel lines. */
  dpr: number;
  /** Horizontal scroll position (CSS pixels). */
  scrollLeft: number;
  /** Vertical scroll position (CSS pixels). */
  scrollTop: number;
  /** Prepared task data. */
  model: GanttModel;
  /** Date ↔ x conversion. */
  scale: TimeScale;
  /** Colours and font. */
  theme: GanttTheme;
  /** Formats header texts. */
  label: HeaderLabeler;
  /** Row index of the primary selected task (outlined), or `null` for none. */
  selectedRow: number | null;
  /**
   * Rows of all selected tasks (highlight band). Optional: missing means
   * only `selectedRow` is highlighted.
   */
  selectedRows?: readonly number[];
  /**
   * Tells whether a day (UTC midnight, ms) is non-working, for the shading.
   * Optional: missing means "Saturday and Sunday" (the M0 behaviour). The
   * Gantt view passes the project calendar's rule here.
   */
  isNonWorkingDay?: ((dayMs: number) => boolean) | undefined;
  /** Today (UTC midnight, ms): its column is highlighted. Optional. */
  todayMs?: number | undefined;
  /** What the user is dragging right now (drawn on top), or `null`. */
  overlay?: GanttOverlay | null;
}

/**
 * The drag preview drawn on top of the chart (see `interactions.ts`).
 *
 * - `bar`: a ghost of the bar at its new place (move) or size (resize).
 * - `link`: a dashed line from the source bar to the mouse, and an outline
 *   around the bar the link would go to (`targetRow`), if any.
 *
 * Beginner note: like `Command` in `ipc/types.ts`, this is a discriminated
 * union: the `kind` field tells which shape the object has.
 */
export type GanttOverlay =
  | { kind: "bar"; row: number; startMs: number; finishMs: number }
  | { kind: "link"; fromRow: number; pointerX: number; pointerY: number; targetRow: number | null };

/** Numbers about a drawn frame (for the dev FPS display and tests). */
export interface FrameStats {
  /** Rows painted. */
  rows: number;
  /** Links painted. */
  links: number;
}

/** Arrowhead size (half-width) in CSS pixels. */
const ARROW_SIZE = 4;
/** Below this many pixels per day, single days are too narrow to shade. */
const MIN_PX_PER_DAY_FOR_SHADING = 4;
/** Thickness of the border around a bar with a risk, in CSS pixels. */
const RISK_BORDER = 4;
/** Thickness of the line between two projects, in CSS pixels. */
const PROJECT_SEPARATOR = 2;
/** Today's column stays this wide when zoomed out, in CSS pixels. */
const MIN_TODAY_WIDTH = 3;
/** Height of today's mark in a header without single days, in CSS pixels. */
const TODAY_MARK_HEIGHT = 4;
/** Thickness of the line from a bar to its deadline, in CSS pixels. */
const DEADLINE_LINE = 1.5;
/** Its dashes and gaps, in CSS pixels. */
const DEADLINE_DASH = [4, 3];
/** Thickness of the outline around a deadline arrow, in CSS pixels. */
const DEADLINE_OUTLINE = 3;
/** Size of the constraint pin, in CSS pixels. */
const MARKER_SIZE = 5;
/** Distance between the diagonal stripes of a blocked piece, in CSS pixels. */
const HATCH_GAP = 5;

/**
 * Paints one complete frame.
 *
 * @param input - See {@link FrameInput}.
 * @returns How many rows and links were painted.
 *
 * @example
 * drawGanttFrame({ ctx, width: 800, height: 600, dpr: 2, scrollLeft: 0,
 *   scrollTop: 0, model, scale, theme, label });
 */
export function drawGanttFrame(input: FrameInput): FrameStats {
  const { ctx, width, height, theme } = input;

  // 1. Clear everything with the background colour.
  ctx.fillStyle = theme.background;
  ctx.fillRect(0, 0, width, height);

  const range = visibleRowRange(input.scrollTop, height, input.model.rows.length);

  // 2. Body: selection band, shading and grid first (behind), then links,
  //    then bars on top.
  drawSelectionBand(input, range.first, range.end);
  drawGrid(input, range.first, range.end);
  const links = drawLinks(input, range.first, range.end);
  drawBars(input, range.first, range.end);
  drawMarkers(input, range.first, range.end);
  if (input.overlay != null) drawOverlay(input, input.overlay);

  // 3. Header last, so rows scrolled up "slide under" it.
  drawHeader(input);

  return { rows: range.end - range.first, links };
}

/**
 * Paints weekend shading, vertical grid lines and horizontal row lines.
 *
 * @param input - The frame input.
 * @param first - First visible row.
 * @param end - One past the last visible row.
 */
function drawGrid(input: FrameInput, first: number, end: number): void {
  const { ctx, width, height, dpr, scale, scrollLeft, scrollTop, theme, model } = input;
  const x0 = scrollLeft;
  const x1 = scrollLeft + width;

  // Non-working-day shading: one path for all shaded days, filled once.
  // Zoomed out, a day is only 2 px wide: shading would only flicker.
  if (scale.pxPerDay >= MIN_PX_PER_DAY_FOR_SHADING) {
    const nonWorking = input.isNonWorkingDay ?? isWeekend;
    const days = visibleTicks(scale, "day", x0, x1);
    ctx.beginPath();
    for (const day of days) {
      if (nonWorking(day.ms)) ctx.rect(day.x - scrollLeft, HEADER_HEIGHT, day.width, height);
    }
    ctx.fillStyle = theme.nonWorking;
    ctx.fill();
  }

  drawProjectTint(input, first, end);
  drawToday(input);

  // Vertical lines at each grid cell of the lower header tier.
  const unit = scale.zoom.bottomUnit;
  const cells = visibleTicks(scale, unit, x0, x1);
  ctx.beginPath();
  for (const cell of cells) {
    // Zoomed far out, lines for every cell would melt into a grey area.
    const stride = labelStride(unit, cell.width, false);
    if (tickIndex(scale, unit, cell.ms) % stride !== 0) continue;
    const x = crisp(cell.x - scrollLeft, dpr);
    ctx.moveTo(x, HEADER_HEIGHT);
    ctx.lineTo(x, height);
  }
  // Horizontal line under each visible row.
  for (let row = first; row < end; row++) {
    const y = crisp(rowTop(row + 1, scrollTop), dpr);
    ctx.moveTo(0, y);
    ctx.lineTo(width, y);
  }
  ctx.lineWidth = 1 / dpr;
  ctx.strokeStyle = theme.gridLine;
  ctx.stroke();

  // The line between two projects: above every project but the first.
  ctx.fillStyle = theme.projectSeparator;
  for (let row = first; row < end; row++) {
    const task = model.rows[row];
    if (task?.isProject !== true || (task.projectIndex ?? 0) === 0) continue;
    ctx.fillRect(0, rowTop(row, scrollTop), width, PROJECT_SEPARATOR);
  }
}

/**
 * Paints the light background of each project's rows. Selected rows keep
 * their own highlight.
 *
 * @param input - The frame input.
 * @param first - First visible row.
 * @param end - One past the last visible row.
 */
function drawProjectTint(input: FrameInput, first: number, end: number): void {
  const { ctx, width, scrollTop, model, theme, selectedRow } = input;
  const selected = new Set(input.selectedRows ?? (selectedRow === null ? [] : [selectedRow]));
  for (let row = first; row < end; row++) {
    const index = model.rows[row]?.projectIndex ?? null;
    if (index === null || selected.has(row)) continue;
    ctx.fillStyle = projectTint(index, theme.projectTint);
    ctx.fillRect(0, rowTop(row, scrollTop), width, ROW_HEIGHT);
  }
}

/**
 * Paints today's column, behind the grid lines and the bars.
 *
 * @param input - The frame input.
 */
function drawToday(input: FrameInput): void {
  const { ctx, height, scale, scrollLeft, theme, todayMs } = input;
  if (todayMs === undefined) return;
  ctx.fillStyle = theme.today;
  const width = Math.max(scale.pxPerDay, MIN_TODAY_WIDTH);
  ctx.fillRect(msToX(scale, todayMs) - scrollLeft, HEADER_HEIGHT, width, height);
}

/**
 * Paints the selected row's highlight band (behind everything in the row).
 *
 * @param input - The frame input.
 * @param first - First visible row.
 * @param end - One past the last visible row.
 */
function drawSelectionBand(input: FrameInput, first: number, end: number): void {
  const { ctx, width, scrollTop, theme, selectedRow } = input;
  const rows = input.selectedRows ?? (selectedRow === null ? [] : [selectedRow]);
  ctx.fillStyle = theme.selection;
  for (const row of rows) {
    if (row < first || row >= end) continue;
    ctx.fillRect(0, rowTop(row, scrollTop), width, ROW_HEIGHT);
  }
}

/**
 * Paints the task bars and milestone diamonds. Task names are not drawn:
 * they appear in a tooltip on hover (`GanttView.tsx`).
 *
 * @param input - The frame input.
 * @param first - First visible row.
 * @param end - One past the last visible row.
 *
 * The geometry comes from `barBox` (hitTest.ts), the same function that
 * decides what a click hits, so drawing and clicking always agree.
 */
function drawBars(input: FrameInput, first: number, end: number): void {
  const { ctx, width, dpr, scale, scrollLeft, scrollTop, theme, model, selectedRow } = input;

  // Two paths per colour. Bars are added in the loop, painted after it:
  // `box` is the outline (its inside shows the chart background, so links
  // and grid lines do not show through), `done` the finished part.
  const group = (color: string): BarPaths => ({
    color,
    box: new Path2D(),
    done: new Path2D(),
  });
  const project = group(theme.barProject);
  const normal = group(theme.bar);
  // Sub-tasks (inside another task) are cyan.
  const subtask = group(theme.barSubtask);
  // Manually scheduled tasks get their own colour (dark green).
  const manual = group(theme.barManual);
  // Milestones whose date has passed without being reached are orange.
  const missed = group(theme.lateStart);
  // Bars with a risk get a thick border around them.
  const risks = { low: new Path2D(), medium: new Path2D(), high: new Path2D() };
  // Blocked pieces: filled, then striped and outlined.
  const blocked = new Path2D();
  const hatch = new Path2D();
  let hasBlocked = false;
  // The selected bar gets an outline, drawn after the fills.
  let selectedBox: BarBox | null = null;

  for (let row = first; row < end; row++) {
    const task = model.rows[row];
    if (task === undefined) continue;
    const box = barBox(task, row, scale, scrollLeft, scrollTop);
    if (box === null) continue;
    if (row === selectedRow) selectedBox = box;

    // Skip bars completely outside the screen.
    // (A milestone with blockers also has blocked pieces left of its box.)
    const left = task.segments === null ? box.x : msToX(scale, task.startMs) - scrollLeft;
    if (box.x + box.width < 0 || left > width) continue;

    const missedMilestone =
      box.isMilestone && input.todayMs !== undefined && isMissedMilestone(task, input.todayMs);
    const target = task.isProject
      ? project
      : missedMilestone
        ? missed
        : task.isManual && !box.isSummary
          ? manual
          : task.isSubtask
            ? subtask
            : normal;
    const share = Math.min(1, Math.max(0, task.percentComplete / 100));

    // Not around a project: one small task's risk must not mark all of it.
    // A missed start is a risk too (higher on a critical task).
    const isLate = input.todayMs !== undefined && isLateToStart(task, input.todayMs);
    const priority = isLate
      ? higherPriority(task.riskPriority, lateStartRisk(task.isCritical))
      : task.riskPriority;
    if (priority !== null && !task.isProject) {
      const border = risks[priority];
      const around = grow(box, RISK_BORDER / 2);
      // Around the whole bar, also when blockers split it.
      const right = box.x + box.width + RISK_BORDER / 2;
      const x = Math.min(left, box.x) - RISK_BORDER / 2;
      if (box.isMilestone && task.segments === null) {
        addDiamond(border, grow(box, RISK_BORDER));
      } else border.rect(x, around.y, right - x, around.height);
    }

    if (box.isSummary) {
      addSummaryBracket(target.box, box);
      // Finished part: along the thin bar of the bracket.
      target.done.rect(box.x, box.y, box.width * share, box.height * 0.4);
      continue;
    }
    if (task.segments !== null) {
      // A task with blockers: each piece on its own. A milestone shows only
      // its blocked pieces, then its diamond. The finished part fills the
      // work pieces from the left.
      const work = task.segments.filter((seg) => seg.blocker === null);
      let remaining = share * work.reduce((sum, seg) => sum + seg.finishMs - seg.startMs, 0);
      for (const seg of task.segments) {
        const x = msToX(scale, seg.startMs) - scrollLeft;
        const w = Math.max(1, msToX(scale, seg.finishMs) - scrollLeft - x);
        if (seg.blocker !== null) {
          blocked.rect(x, box.y, w, box.height);
          // Stripes only for the visible part: a long blocker at a
          // close zoom would otherwise add thousands of unseen lines.
          const hx = Math.max(x, -box.height);
          const hw = Math.min(x + w, width + box.height) - hx;
          if (hw > 0) addHatch(hatch, hx, box.y, hw, box.height);
          hasBlocked = true;
        } else if (!box.isMilestone) {
          addBar(target.box, x, box.y, w, box.height);
          const length = seg.finishMs - seg.startMs;
          const done = length <= 0 ? 0 : Math.min(1, remaining / length);
          remaining -= done * length;
          if (done > 0) target.done.rect(x, box.y, w * done, box.height);
        }
      }
    }
    if (box.isMilestone) {
      addDiamond(target.box, box);
      // A milestone is reached or not: filled only at 100 %.
      if (share === 1) addDiamond(target.done, box);
    } else if (task.segments === null) {
      addBar(target.box, box.x, box.y, box.width, box.height);
      if (share > 0) target.done.rect(box.x, box.y, box.width * share, box.height);
    }
  }

  const line = (2 * physicalLineWidth(dpr)) / dpr;
  ctx.lineWidth = line;
  for (const { color, box, done } of [project, normal, subtask, manual, missed]) {
    ctx.fillStyle = theme.background;
    ctx.fill(box);
    ctx.fillStyle = color;
    ctx.fill(done);
    ctx.lineWidth = line;
    ctx.strokeStyle = color;
    ctx.stroke(box);
  }
  if (hasBlocked) {
    ctx.fillStyle = theme.barBlocked;
    ctx.fill(blocked);
    // Stripes only inside the blocked pieces: `clip` limits painting to
    // the path. `save`/`restore` remove the clip again afterwards.
    ctx.save();
    ctx.clip(blocked);
    ctx.lineWidth = physicalLineWidth(dpr) / dpr;
    ctx.strokeStyle = theme.barBlockedLine;
    ctx.stroke(hatch);
    ctx.restore();
    // A thin dark outline separates the piece from the work around it.
    ctx.lineWidth = physicalLineWidth(dpr) / dpr;
    ctx.strokeStyle = theme.barBlockedLine;
    ctx.stroke(blocked);
  }

  // Risk borders come after all bars and blocked pieces, so they lie on top
  // of them. The bar's own thin outline stays visible inside the border.
  ctx.lineWidth = RISK_BORDER;
  for (const [border, color] of [
    [risks.low, theme.riskLow],
    [risks.medium, theme.riskMedium],
    [risks.high, theme.riskHigh],
  ] as const) {
    ctx.strokeStyle = color;
    ctx.stroke(border);
  }

  if (selectedBox !== null) {
    // An outline a little outside the bar, so its own one stays visible.
    const outline = new Path2D();
    const around = grow(selectedBox, RISK_BORDER + 1);
    if (selectedBox.isMilestone) addDiamond(outline, around);
    else outline.rect(around.x, around.y, around.width, around.height);
    ctx.lineWidth = line;
    ctx.strokeStyle = theme.selectionOutline;
    ctx.stroke(outline);
  }
}

/** The paths of all bars of one colour. */
interface BarPaths {
  /** Colour of the outline and of the finished part. */
  color: string;
  /** The outlines. */
  box: Path2D;
  /** The finished parts. */
  done: Path2D;
}

/**
 * Adds a bar's outline, drawn inside its edges, so that two bars that touch
 * do not overlap.
 *
 * @param target - The path to extend.
 * @param x - Left edge.
 * @param y - Top edge.
 * @param w - Width.
 * @param h - Height.
 */
function addBar(target: Path2D, x: number, y: number, w: number, h: number): void {
  // A very narrow bar has no room for the inset.
  const inset = w > 4 ? 1 : 0;
  target.rect(x + inset, y + 1, w - 2 * inset, h - 2);
}

/**
 * A box that is larger on every side.
 *
 * @param box - The box.
 * @param by - How much, in CSS px.
 * @returns The larger box.
 */
function grow(box: BarBox, by: number): BarBox {
  return {
    ...box,
    x: box.x - by,
    y: box.y - by,
    width: box.width + 2 * by,
    height: box.height + 2 * by,
  };
}

/**
 * Adds a summary-task bracket: a thin bar along the top of `box` with a
 * small downward point at each end, showing the span of the subtasks.
 *
 * ```text
 *   ████████████████████   ← thin bar (40 % of the bar height)
 *   ▼                  ▼   ← end points
 * ```
 *
 * @param target - The path to extend.
 * @param box - The summary's box (full bar height).
 */
function addSummaryBracket(target: Path2D, box: BarBox): void {
  const barHeight = box.height * 0.4;
  // End points, at most a third of the bar wide so short summaries stay readable.
  const point = Math.min(box.height * 0.5, box.width / 3);
  const left = box.x;
  const right = box.x + box.width;
  const top = box.y;
  const bottom = box.y + box.height * 0.8;
  target.moveTo(left, top);
  target.lineTo(right, top);
  target.lineTo(right, bottom);
  target.lineTo(right - point, top + barHeight);
  target.lineTo(left + point, top + barHeight);
  target.lineTo(left, bottom);
  target.closePath();
}

/**
 * Adds diagonal stripes ("hatching") covering a rectangle. They are
 * painted clipped to the blocked pieces, so lines may run past the edges.
 *
 * ```text
 *   ╱╱╱╱╱╱╱╱   one line every HATCH_GAP pixels, bottom-left to top-right
 * ```
 *
 * @param target - The path to extend.
 * @param x - Left edge.
 * @param y - Top edge.
 * @param w - Width.
 * @param h - Height.
 */
function addHatch(target: Path2D, x: number, y: number, w: number, h: number): void {
  // Starting `h` pixels left of the piece makes the first line reach its
  // bottom-left corner. Lines start on multiples of HATCH_GAP (measured
  // from the left edge of the chart), so stripes line up across pieces.
  const first = Math.floor((x - h) / HATCH_GAP) * HATCH_GAP;
  for (let sx = first; sx < x + w; sx += HATCH_GAP) {
    target.moveTo(sx, y + h);
    target.lineTo(sx + h, y);
  }
}

/**
 * Adds a diamond (a square standing on its corner) that fills `box`.
 *
 * @param target - The path to extend.
 * @param box - The square the diamond fits in.
 */
function addDiamond(target: Path2D, box: BarBox): void {
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  target.moveTo(cx, box.y);
  target.lineTo(box.x + box.width, cy);
  target.lineTo(cx, box.y + box.height);
  target.lineTo(box.x, cy);
  target.closePath();
}

/**
 * Paints the links (all four types) that cross the visible area.
 *
 * @param input - The frame input.
 * @param first - First visible row.
 * @param end - One past the last visible row.
 * @returns How many links were painted.
 *
 * Which links can be visible? A link that crosses the screen joins a row
 * at or above the bottom of the screen with a row at or below its top. The
 * successor row of such a link is at most `maxLinkRowSpan` rows away from
 * the visible rows. So we look at rows from `first − span` to `end + span`
 * only: the cost does not grow with the size of the project.
 * (If a project has a very long link, this window grows; acceptable for M0,
 * to revisit with real data in M1.)
 */
function drawLinks(input: FrameInput, first: number, end: number): number {
  const { ctx, width, dpr, theme, model } = input;
  if (end <= first) return 0;
  const span = model.maxLinkRowSpan;
  const from = Math.max(0, first - span);
  const to = Math.min(model.rows.length, end + span);

  const normalLines = new Path2D();
  const normalHeads = new Path2D();
  let count = 0;

  for (let row = from; row < to; row++) {
    const succ = model.rows[row];
    if (succ === undefined || Number.isNaN(succ.startMs)) continue;
    for (const link of succ.links) {
      const predRow = link.row;
      // Vertical culling: does the link touch the visible rows at all?
      const top = Math.min(row, predRow);
      const bottom = Math.max(row, predRow);
      if (bottom < first || top >= end) continue;
      const pred = model.rows[predRow];
      if (pred === undefined || Number.isNaN(pred.finishMs)) continue;

      const predEnds = barEnds(input, pred.startMs, pred.finishMs, predRow);
      const succEnds = barEnds(input, succ.startMs, succ.finishMs, row);
      // Horizontal culling: entirely left or right of the screen?
      const minX = Math.min(predEnds.startX, succEnds.startX) - 2 * ARROW_SIZE - 16;
      const maxX = Math.max(predEnds.finishX, succEnds.finishX) + 16;
      if (maxX < 0 || minX > width) continue;

      const path = linkPath(link.kind, predEnds, succEnds, BAR_HEIGHT, ROW_HEIGHT);
      addPolyline(normalLines, path, dpr);
      addArrowHead(normalHeads, path);
      count++;
    }
  }

  // About 1 CSS pixel wide, rounded to whole physical pixels (sharp lines).
  ctx.lineWidth = physicalLineWidth(dpr) / dpr;
  ctx.strokeStyle = theme.link;
  ctx.stroke(normalLines);
  ctx.fillStyle = theme.link;
  ctx.fill(normalHeads);
  return count;
}

/**
 * Gives the x of both ends of a bar and the y of its middle, in viewport
 * pixels, for routing links.
 *
 * @param input - The frame input (scale and scroll position).
 * @param startMs - The task's start.
 * @param finishMs - The task's finish.
 * @param row - The task's row.
 * @returns The ends. A milestone (start = finish) has both ends at its
 *   diamond's tips, so links touch the diamond and not its centre.
 */
function barEnds(input: FrameInput, startMs: number, finishMs: number, row: number): BarEnds {
  const { scale, scrollLeft, scrollTop } = input;
  let startX = msToX(scale, startMs) - scrollLeft;
  let finishX = msToX(scale, finishMs) - scrollLeft;
  if (finishMs === startMs) {
    startX -= BAR_HEIGHT / 2;
    finishX += BAR_HEIGHT / 2;
  }
  return { startX, finishX, y: rowTop(row, scrollTop) + ROW_HEIGHT / 2 };
}

/**
 * Adds a link's lines to a path, snapped to physical pixels.
 *
 * @param target - The path to extend.
 * @param link - The computed link.
 * @param dpr - Device pixel ratio.
 */
function addPolyline(target: Path2D, link: LinkPath, dpr: number): void {
  const w = physicalLineWidth(dpr);
  link.points.forEach((p, i) => {
    const x = crisp(p.x, dpr, w);
    const y = crisp(p.y, dpr, w);
    if (i === 0) target.moveTo(x, y);
    else target.lineTo(x, y);
  });
}

/**
 * Adds a small triangle (the arrowhead) at the end of a link.
 *
 * @param target - The path to extend.
 * @param link - The computed link; the tip is its last point.
 */
function addArrowHead(target: Path2D, link: LinkPath): void {
  const tip = link.points[link.points.length - 1];
  if (tip === undefined) return;
  const s = ARROW_SIZE;
  target.moveTo(tip.x, tip.y);
  if (link.arrow === "down") {
    target.lineTo(tip.x - s, tip.y - s * 1.5);
    target.lineTo(tip.x + s, tip.y - s * 1.5);
  } else if (link.arrow === "up") {
    target.lineTo(tip.x - s, tip.y + s * 1.5);
    target.lineTo(tip.x + s, tip.y + s * 1.5);
  } else if (link.arrow === "left") {
    target.lineTo(tip.x + s * 1.5, tip.y - s);
    target.lineTo(tip.x + s * 1.5, tip.y + s);
  } else {
    target.lineTo(tip.x - s * 1.5, tip.y - s);
    target.lineTo(tip.x - s * 1.5, tip.y + s);
  }
  target.closePath();
}

/**
 * Adds a deadline arrow, pointing down, to a path.
 *
 * @param path - The path to extend.
 * @param box - Where it is drawn.
 */
function addDeadlineArrow(path: Path2D, box: Pick<BarBox, "x" | "y" | "width" | "height">): void {
  const { x, y, width, height } = box;
  const head = y + height / 2;
  path.moveTo(x + width / 2, y + height);
  path.lineTo(x, head);
  path.lineTo(x + width / 4, head);
  path.lineTo(x + width / 4, y);
  path.lineTo(x + (width * 3) / 4, y);
  path.lineTo(x + (width * 3) / 4, head);
  path.lineTo(x + width, head);
  path.closePath();
}

/**
 * Adds a "late to start" arrow, pointing right, to a path.
 *
 * @param path - The path to extend.
 * @param box - Where it is drawn.
 */
function addLateStartArrow(path: Path2D, box: Pick<BarBox, "x" | "y" | "width" | "height">): void {
  const { x, y, width, height } = box;
  const head = x + width / 2;
  path.moveTo(x + width, y + height / 2);
  path.lineTo(head, y);
  path.lineTo(head, y + height / 4);
  path.lineTo(x, y + height / 4);
  path.lineTo(x, y + (height * 3) / 4);
  path.lineTo(head, y + (height * 3) / 4);
  path.lineTo(head, y + height);
  path.closePath();
}

/**
 * Adds a finish arrow to a path: a thin shaft and a head.
 *
 * @param path - The path to extend.
 * @param box - Where it is drawn.
 * @param right - Points right (late) or left (early).
 */
function addFinishArrow(
  path: Path2D,
  box: Pick<BarBox, "x" | "y" | "width" | "height">,
  right: boolean,
): void {
  const { headLength, shaftHeight } = FINISH_ARROW;
  const tip = right ? box.x + box.width : box.x;
  const tail = right ? box.x : box.x + box.width;
  const neck = right ? tip - headLength : tip + headLength;
  const middle = box.y + box.height / 2;
  path.moveTo(tip, middle);
  path.lineTo(neck, box.y);
  path.lineTo(neck, middle - shaftHeight / 2);
  path.lineTo(tail, middle - shaftHeight / 2);
  path.lineTo(tail, middle + shaftHeight / 2);
  path.lineTo(neck, middle + shaftHeight / 2);
  path.lineTo(neck, box.y + box.height);
  path.closePath();
}

/**
 * Paints the deadline markers and constraint pins of the visible rows.
 *
 * @param input - The frame input.
 * @param first - First visible row.
 * @param end - One past the last visible row.
 *
 * - **Deadline**: a large arrow in the task's row, pointing down at the
 *   deadline date. A dashed line joins it to its bar. Green-ish
 *   normally, red when the task finishes after it (missed deadline).
 * - **Late to start**: an orange arrow left of the bar, pointing right at
 *   a start that has passed while the task is still at 0 %.
 * - **Finish arrow**: from the planned to the actual finish, orange and to
 *   the right when late, green and to the left when early. Only an outline
 *   while the task is not finished (its calculated finish).
 * - **Constraint pin**: a short vertical line with a dot at the date of a
 *   dated constraint (Start No Earlier Than, Must Finish On…), above the
 *   bar, so the user sees *why* a bar cannot move earlier or later.
 */
function drawMarkers(input: FrameInput, first: number, end: number): void {
  const { ctx, width, dpr, scale, scrollLeft, scrollTop, theme, model, todayMs } = input;
  const lateStarts = new Path2D();
  const deadlines = new Path2D();
  const missed = new Path2D();
  const deadlineLines = new Path2D();
  const missedLines = new Path2D();
  const finishArrows = {
    lateDone: new Path2D(),
    lateOpen: new Path2D(),
    earlyDone: new Path2D(),
    earlyOpen: new Path2D(),
  };
  const pinLines = new Path2D();
  const pinHeads = new Path2D();
  const s = MARKER_SIZE;

  for (let row = first; row < end; row++) {
    const task = model.rows[row];
    if (task === undefined) continue;
    const top = rowTop(row, scrollTop);
    const barTop = top + (ROW_HEIGHT - BAR_HEIGHT) / 2;
    const late = lateStartBox(task, row, scale, scrollLeft, scrollTop, todayMs);
    if (late !== null && late.x + late.width > 0 && late.x < width) {
      addLateStartArrow(lateStarts, late);
    }
    const arrow = finishArrowBox(task, row, scale, scrollLeft, scrollTop);
    if (task.variance !== null && arrow !== null && arrow.x + arrow.width > 0 && arrow.x < width) {
      const { late: isLate, done } = task.variance;
      const path = isLate
        ? finishArrows[done ? "lateDone" : "lateOpen"]
        : finishArrows[done ? "earlyDone" : "earlyOpen"];
      addFinishArrow(path, arrow, isLate);
    }
    if (!Number.isNaN(task.deadlineMs)) {
      const box = deadlineBox(task, row, scale, scrollLeft, scrollTop);
      const path = task.missedDeadline ? missed : deadlines;
      if (box !== null && box.x + box.width > 0 && box.x < width) addDeadlineArrow(path, box);
      // A dashed line across the gap to the bar shows whose deadline it is.
      const bar = barBox(task, row, scale, scrollLeft, scrollTop);
      if (box !== null && bar !== null) {
        const from = Math.max(Math.min(bar.x + bar.width, box.x + box.width), 0);
        const to = Math.min(Math.max(bar.x, box.x), width);
        const y = crisp(top + ROW_HEIGHT / 2, dpr);
        if (to > from) {
          const lines = task.missedDeadline ? missedLines : deadlineLines;
          lines.moveTo(from, y);
          lines.lineTo(to, y);
        }
      }
    }
    if (!Number.isNaN(task.constraintMs)) {
      const x = crisp(msToX(scale, task.constraintMs) - scrollLeft, dpr);
      if (x > -s && x < width + s) {
        pinLines.moveTo(x, top + 2);
        pinLines.lineTo(x, barTop);
        pinHeads.moveTo(x + 2, top + 3);
        pinHeads.arc(x, top + 3, 2, 0, 2 * Math.PI);
      }
    }
  }
  ctx.lineWidth = DEADLINE_LINE;
  ctx.strokeStyle = theme.deadline;
  ctx.setLineDash(DEADLINE_DASH);
  ctx.stroke(deadlineLines);
  ctx.strokeStyle = theme.deadlineMissed;
  ctx.stroke(missedLines);
  ctx.setLineDash([]);
  // An outline in the background colour keeps the arrow clear on a bar.
  ctx.strokeStyle = theme.background;
  ctx.lineWidth = DEADLINE_OUTLINE;
  ctx.stroke(deadlines);
  ctx.stroke(missed);
  ctx.stroke(lateStarts);
  ctx.fillStyle = theme.lateStart;
  ctx.fill(lateStarts);
  ctx.fillStyle = theme.deadline;
  ctx.fill(deadlines);
  ctx.fillStyle = theme.deadlineMissed;
  ctx.fill(missed);
  for (const [path, color, done] of [
    [finishArrows.lateDone, theme.lateStart, true],
    [finishArrows.lateOpen, theme.lateStart, false],
    [finishArrows.earlyDone, theme.deadline, true],
    [finishArrows.earlyOpen, theme.deadline, false],
  ] as const) {
    ctx.strokeStyle = theme.background;
    ctx.lineWidth = DEADLINE_OUTLINE;
    ctx.stroke(path);
    ctx.fillStyle = done ? color : theme.background;
    ctx.fill(path);
    ctx.strokeStyle = color;
    ctx.lineWidth = 1;
    ctx.stroke(path);
  }
  ctx.lineWidth = physicalLineWidth(dpr) / dpr;
  ctx.strokeStyle = theme.constraint;
  ctx.stroke(pinLines);
  ctx.fillStyle = theme.constraint;
  ctx.fill(pinHeads);
}

/**
 * Paints the drag preview on top of the bars.
 *
 * @param input - The frame input.
 * @param overlay - What is being dragged (see {@link GanttOverlay}).
 */
function drawOverlay(input: FrameInput, overlay: GanttOverlay): void {
  const { ctx, dpr, scale, scrollLeft, scrollTop, theme, model } = input;
  ctx.save();
  ctx.lineWidth = (2 * physicalLineWidth(dpr)) / dpr;
  ctx.strokeStyle = theme.dragPreview;
  if (overlay.kind === "bar") {
    const task = model.rows[overlay.row];
    if (task !== undefined) {
      // The ghost uses the same geometry as a real bar (barBox), with the
      // new dates.
      const box = barBox(
        { ...task, startMs: overlay.startMs, finishMs: overlay.finishMs },
        overlay.row,
        scale,
        scrollLeft,
        scrollTop,
      );
      if (box !== null) {
        const ghost = new Path2D();
        if (box.isMilestone) addDiamond(ghost, box);
        else ghost.rect(box.x, box.y, box.width, box.height);
        ctx.setLineDash([4, 3]);
        ctx.stroke(ghost);
      }
    }
  } else {
    const source = model.rows[overlay.fromRow];
    if (source !== undefined) {
      const from = barEnds(input, source.startMs, source.finishMs, overlay.fromRow);
      ctx.setLineDash([4, 3]);
      ctx.beginPath();
      ctx.moveTo(from.finishX, from.y);
      ctx.lineTo(overlay.pointerX, overlay.pointerY);
      ctx.stroke();
    }
    const target = overlay.targetRow === null ? undefined : model.rows[overlay.targetRow];
    if (target !== undefined && overlay.targetRow !== null) {
      const box = barBox(target, overlay.targetRow, scale, scrollLeft, scrollTop);
      if (box !== null) {
        ctx.setLineDash([]);
        ctx.strokeRect(box.x - 3, box.y - 3, box.width + 6, box.height + 6);
      }
    }
  }
  ctx.restore();
}

/**
 * Paints the two-tier timescale header (e.g. months above days).
 *
 * @param input - The frame input.
 */
function drawHeader(input: FrameInput): void {
  const { ctx, width, dpr, scale, scrollLeft, theme, label } = input;
  const x0 = scrollLeft;
  const x1 = scrollLeft + width;

  ctx.fillStyle = theme.headerBackground;
  ctx.fillRect(0, 0, width, HEADER_HEIGHT);

  // Today: the whole day cell, or a mark where a cell is more than one day.
  const { todayMs } = input;
  const dayCells =
    scale.zoom.bottomUnit === "day" && labelStride("day", scale.pxPerDay, false) === 1;
  if (todayMs !== undefined) {
    const markHeight = dayCells ? HEADER_TIER_HEIGHT : TODAY_MARK_HEIGHT;
    ctx.fillStyle = theme.todayHeader;
    ctx.fillRect(
      msToX(scale, todayMs) - scrollLeft,
      HEADER_HEIGHT - markHeight,
      Math.max(scale.pxPerDay, MIN_TODAY_WIDTH),
      markHeight,
    );
  }

  ctx.textBaseline = "middle";
  const lines = new Path2D();

  const tiers = [
    { unit: scale.zoom.topUnit, y: 0, isTop: true },
    { unit: scale.zoom.bottomUnit, y: HEADER_TIER_HEIGHT, isTop: false },
  ];
  for (const tier of tiers) {
    // Zoomed far out, several cells share one label (see `labelStride`).
    // A shared label may start left of the screen, so look that far back.
    const days = { day: 1, week: 7, month: 30, year: 365 }[tier.unit];
    const stride = labelStride(tier.unit, days * scale.pxPerDay, tier.isTop);
    const from = Math.max(0, x0 - (stride - 1) * days * scale.pxPerDay * 1.1);
    for (const tick of visibleTicks(scale, tier.unit, from, x1)) {
      if (tickIndex(scale, tier.unit, tick.ms) % stride !== 0) continue;
      const left = tick.x - scrollLeft;
      const cellWidth = tick.width * stride;
      const lineX = crisp(left, dpr);
      lines.moveTo(lineX, tier.y);
      lines.lineTo(lineX, tier.y + HEADER_TIER_HEIGHT);

      // Keep the label of a partly visible cell on screen ("sticky" label),
      // but never let it run past its own cell.
      const text = label(tier.unit, tick.ms, tier.isTop);
      const textX = Math.max(left, 0) + 4;
      if (textX < left + cellWidth - 4) {
        ctx.save();
        ctx.beginPath();
        ctx.rect(left, tier.y, cellWidth, HEADER_TIER_HEIGHT);
        ctx.clip();
        const isToday = dayCells && !tier.isTop && tick.ms === todayMs;
        ctx.font = `${isToday ? "bold " : ""}12px ${theme.fontFamily}`;
        ctx.fillStyle = isToday ? theme.todayHeaderText : theme.headerText;
        ctx.fillText(text, textX, tier.y + HEADER_TIER_HEIGHT / 2);
        ctx.restore();
      }
    }
  }
  // Horizontal lines: between the tiers and under the header.
  const mid = crisp(HEADER_TIER_HEIGHT, dpr);
  const bottom = crisp(HEADER_HEIGHT - 1, dpr);
  lines.moveTo(0, mid);
  lines.lineTo(width, mid);
  lines.moveTo(0, bottom);
  lines.lineTo(width, bottom);

  ctx.lineWidth = 1 / dpr;
  ctx.strokeStyle = theme.headerLine;
  ctx.stroke(lines);
}
