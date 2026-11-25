/**
 * GanttView.tsx: the React component that shows the Gantt chart.
 *
 * What it is for:
 *   Puts a `<canvas>` and a scrolling layer on the page and hands them to
 *   the renderer controller (`renderer/controller.ts`), which does the actual
 *   painting. React's job here is to *set up* and *clean up*, and to turn
 *   mouse actions into commands; it does not take part in drawing frames.
 *
 * Where it fits:
 *   Rendered by `shell/AppShell.tsx` in the main view area. Reads the tasks,
 *   the zoom, the selection and the calendars from the store
 *   (`state/store.ts`).
 *   - A click on a bar selects that task (Ctrl/Cmd+click adds it to the
 *     selection); a click beside every bar clears the selection.
 *   - Dragging a bar moves it, dragging its right edge resizes it, dragging
 *     up or down onto another bar links the two (`interactions.ts`). Each
 *     drag sends one command to Rust; Esc cancels the drag.
 *   - Ctrl + mouse wheel (Command on macOS; or a trackpad pinch) changes
 *     the zoom percentage, keeping the date under the mouse in place.
 *   - Hovering over the striped, blocked piece of a bar shows
 *     its blocker: reason, length and the day it was reported. For a task
 *     with blockers, a drag still changes only the planned work: the
 *     blocked time is taken out of a resize and added to a move preview.
 *
 * The layers, from bottom to top:
 *
 * ```
 *  ┌──────────────────────────────┐  wrapper  (position: relative)
 *  │ canvas    (absolute, top-left; paints only the visible part)
 *  │ scroller  (absolute, fills the wrapper, transparent, has scrollbars;
 *  │            marked `data-gantt-scroller` so the task grid can find it)
 *  │   └ spacer (as big as the whole chart: 10,000 rows × 28 px tall)
 *  │ tooltip   (absolute; visible while dragging or over a bar)
 *  └──────────────────────────────┘
 * ```
 */

import { useEffect, useMemo, useRef, type PointerEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { isWorkingDay, isoDateTime } from "@frontend/features/calendars/workingTime";
import { useFormatLocale } from "@frontend/features/settings/useFormatLocale";
import { formatNumber, formatScheduleDate } from "@frontend/i18n/format";
import {
  calendarAddWorkingTime,
  calendarWorkingMinutes,
  hasDesktopCore,
} from "@frontend/ipc/client";
import type { Calendar, TaskRow } from "@frontend/ipc/types";
import { useAppStore } from "@frontend/state/store";
import {
  classifyDrag,
  daysFromPixels,
  hitTestPart,
  linkCommand,
  moveCommand,
  resizeCommand,
  rowAtY,
  snapMoveStart,
  snapResizeFinish,
  type DragKind,
  type PartHit,
} from "./interactions";
import { createGanttController, type GanttController } from "./renderer/controller";
import { localToday, parseIsoLocal } from "./renderer/dates";
import { makeDayFill, noDayFill } from "./renderer/dayFill";
import {
  hitTestBar,
  hitTestBlocked,
  hitTestDeadline,
  hitTestFinishArrow,
  hitTestLateStart,
  isLateToStart,
  isMissedMilestone,
} from "./renderer/hitTest";
import { contentHeight } from "./renderer/layout";
import { prepareGanttModel } from "./renderer/model";
import { ZOOM_LEVELS, createTimeScale, wheelZoomPercent } from "./renderer/timeScale";

/** How often the view checks whether a new day has begun, in ms. */
const TODAY_CHECK_MS = 60_000;

/** `true` on macOS, where Command (not Ctrl) + wheel zooms. */
const IS_MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.userAgent);

/**
 * Everything known about one drag, from the mouse press to the release.
 * Kept in a ref (not React state): it changes on every mouse move, and the
 * canvas, not React, draws the preview.
 */
interface DragState {
  /** The pointer that pressed (the mouse, or one finger / pen). */
  pointerId: number;
  /** Where the press happened, viewport CSS px. */
  startX: number;
  /** Where the press happened, viewport CSS px. */
  startY: number;
  /** The bar and part that were pressed. */
  hit: PartHit;
  /** The task's row from Rust, or `null` for mock data (no editing). */
  task: TaskRow | null;
  /** The task's calendar (for snapping), or `null` if unknown. */
  calendar: Calendar | null;
  /** The kind of drag, or `null` while it is still a click. */
  kind: DragKind | null;
  /** The previewed start (ms) for move / resize. */
  previewStart: number;
  /** The previewed finish (ms) for move / resize. */
  previewFinish: number;
  /** The row under the mouse for a link drag, or `null`. */
  targetRow: number | null;
  /** Counts preview requests to Rust, so late answers are ignored. */
  previewSeq: number;
  /** Set once the drag was cancelled (Esc): the release then does nothing. */
  cancelled: boolean;
}

/**
 * The Gantt chart view.
 *
 * @returns The JSX for the chart area. It fills its parent, so the parent
 *   must have a size (AppShell gives it the remaining window space).
 *
 * Keyboard: the scrolling layer can receive focus (Tab). The browser then
 * scrolls it with the arrow keys, Page Up / Page Down, Home and End. Dates,
 * durations and links can also be changed with the keyboard in the task
 * grid and the task information dialog; the mouse drags here are shortcuts.
 *
 * Beginner notes on the hooks used here:
 * - `useRef` gives a box (`ref.current`) that survives re-renders. Passing
 *   it as `ref={…}` to an element makes React put the real DOM element in it.
 * - `useMemo(fn, [deps])` remembers the result of `fn` and recomputes it only
 *   when one of the dependencies changes. Preparing 10,000 tasks is not free,
 *   so we do it only when the tasks change, not on every render.
 * - `useEffect(fn, [deps])` runs `fn` *after* React has updated the page,
 *   whenever a dependency changed. If `fn` returns a function, React calls
 *   that "clean-up" before the next run and when the component disappears.
 */
export function GanttView() {
  const { t, i18n } = useTranslation();
  const tasks = useAppStore((s) => s.tasks);
  const zoom = useAppStore((s) => s.zoom);
  const zoomPercent = useAppStore((s) => s.zoomPercent);
  const scrollToStartRequest = useAppStore((s) => s.scrollToStartRequest);
  const scrollToTodayRequest = useAppStore((s) => s.scrollToTodayRequest);
  const selectedTaskId = useAppStore((s) => s.selectedTaskId);
  const selectedTaskIds = useAppStore((s) => s.selectedTaskIds);
  const revealTaskRequest = useAppStore((s) => s.revealTaskRequest);
  const snapshot = useAppStore((s) => s.snapshot);
  const calendars = useAppStore((s) => s.calendars);
  const isDemoData = useAppStore((s) => s.isDemoData);
  const { dateLocale, numberLocale } = useFormatLocale();

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const controllerRef = useRef<GanttController | null>(null);
  const dragRef = useRef<DragState | null>(null);
  /** `true` while the tooltip shows a task or blocker (hover), not a drag. */
  const hoverTipRef = useRef(false);

  // Today; checked every minute, because the app may stay open overnight.
  const [today, setToday] = useState(() => localToday());
  useEffect(() => {
    const timer = setInterval(() => setToday(localToday()), TODAY_CHECK_MS);
    return () => clearInterval(timer);
  }, []);

  // Convert the tasks once per change (dates → numbers, ids → rows).
  const projectCalendarId = snapshot?.calendarId ?? null;
  // Where a moment is drawn: a full working day fills its whole cell.
  const fill = useMemo(
    () => (isDemoData ? noDayFill : makeDayFill(calendars, projectCalendarId)),
    [calendars, projectCalendarId, isDemoData],
  );
  const model = useMemo(() => prepareGanttModel(tasks, fill), [tasks, fill]);
  // Build the date ↔ pixel mapping for the current zoom.
  // The chart always reaches today, even if the project does not.
  const scale = useMemo(
    () =>
      createTimeScale(
        Math.min(model.minMs, today),
        Math.max(model.maxMs, today),
        ZOOM_LEVELS[zoom],
        today,
        zoomPercent,
      ),
    [model, today, zoom, zoomPercent],
  );

  // The project calendar (full working times), or null for the mock data.
  const projectCalendar = useMemo(
    () =>
      isDemoData || snapshot === null
        ? null
        : (calendars.find((c) => c.id === snapshot.calendarId) ?? null),
    [calendars, snapshot, isDemoData],
  );

  // Rows of the selected tasks; the primary one last. A Map from ID to row
  // finds each row in one step.
  const selection = useMemo(() => {
    const rowById = new Map(model.rows.map((r, i) => [r.id, i]));
    const rows: number[] = [];
    for (const id of selectedTaskIds) {
      const row = rowById.get(id);
      if (row !== undefined) rows.push(row);
    }
    const primary = selectedTaskId === null ? undefined : rowById.get(selectedTaskId);
    return { rows, primary: primary ?? null };
  }, [model, selectedTaskId, selectedTaskIds]);

  // Create the controller when the view appears; destroy it when it goes.
  // It is re-created only if the language changes (header month names).
  useEffect(() => {
    const canvas = canvasRef.current;
    const scroller = scrollerRef.current;
    if (canvas === null || scroller === null) return;
    const controller = createGanttController({ canvas, scroller, locale: i18n.language });
    controllerRef.current = controller;
    return () => {
      controller.destroy();
      controllerRef.current = null;
    };
  }, [i18n.language]);

  // Hand new data or a new scale to the controller.
  // This effect is declared after the one above, so on the first render the
  // controller already exists when it runs (effects run in order).
  useEffect(() => {
    controllerRef.current?.setData(model, scale);
  }, [model, scale, i18n.language]);

  // Tell the controller which rows to highlight. Also re-sent when the
  // controller is re-created (language change), hence `i18n.language`.
  useEffect(() => {
    controllerRef.current?.setSelection(selection.primary, selection.rows);
  }, [selection, i18n.language]);

  // Shade the project calendar's days off (holidays too), not just weekends.
  // A small cache keeps the per-frame cost tiny: each day is looked up once.
  useEffect(() => {
    if (projectCalendar === null) {
      controllerRef.current?.setNonWorkingDays(undefined);
      return;
    }
    const cache = new Map<number, boolean>();
    controllerRef.current?.setNonWorkingDays((dayMs) => {
      let off = cache.get(dayMs);
      if (off === undefined) {
        off = !isWorkingDay(projectCalendar, dayMs);
        cache.set(dayMs, off);
      }
      return off;
    });
  }, [projectCalendar, i18n.language]);

  // Highlight today's column.
  useEffect(() => {
    controllerRef.current?.setToday(today);
  }, [today, i18n.language]);

  // When the app starts, another project is opened, or "Go to today" is
  // pressed: show today.
  const projectId = snapshot?.projectId ?? null;
  useEffect(() => {
    controllerRef.current?.centerDate(localToday());
  }, [projectId, scrollToTodayRequest]);

  // React to "Go to project start" (the request counter changed).
  useEffect(() => {
    if (scrollToStartRequest === 0 || Number.isNaN(model.minMs)) return;
    controllerRef.current?.scrollToDate(model.minMs);
  }, [scrollToStartRequest, model.minMs]);

  // React to "jump to this task" (e.g. from the "Why this date?" panel).
  useEffect(() => {
    if (revealTaskRequest === null) return;
    const row = model.rows.findIndex((r) => r.id === revealTaskRequest.id);
    const task = model.rows[row];
    if (task !== undefined) controllerRef.current?.revealRow(row, task.startMs);
    // Only a *new* request should scroll, not every model change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [revealTaskRequest]);

  // Ctrl/Cmd + wheel zooms. React's own `onWheel` is "passive" (it cannot
  // stop the browser from scrolling), so we add the listener ourselves with
  // `passive: false`, which allows `preventDefault()`.
  useEffect(() => {
    const scroller = scrollerRef.current;
    if (scroller === null) return;
    // The zoom with decimals, so tiny trackpad movements add up.
    let exact = useAppStore.getState().zoomPercent;
    function onWheel(event: WheelEvent): void {
      // Ctrl on Windows and Linux, Command on macOS. Ctrl is accepted on
      // macOS too: a trackpad pinch arrives as "Ctrl + wheel".
      if (!event.ctrlKey && !(IS_MAC && event.metaKey)) return;
      // Stop the web view from zooming the whole page.
      event.preventDefault();
      const { zoomPercent: current, setZoomPercent } = useAppStore.getState();
      // Changed by a button in the meantime: continue from there.
      if (Math.round(exact) !== current) exact = current;
      // Some mice report lines (mode 1) instead of pixels; a line is ~16 px.
      // Shift turns a wheel sideways on some systems: then use deltaX.
      const delta = event.deltaY !== 0 ? event.deltaY : event.deltaX;
      exact = wheelZoomPercent(exact, event.deltaMode === 1 ? delta * 16 : delta);
      if (Math.round(exact) === current || scroller === null) return;
      const rect = scroller.getBoundingClientRect();
      controllerRef.current?.setZoomAnchor(event.clientX - rect.left);
      setZoomPercent(exact);
    }
    scroller.addEventListener("wheel", onWheel, { passive: false });
    return () => scroller.removeEventListener("wheel", onWheel);
  }, []);

  // ---- Drag helpers (called from the pointer handlers below) -------------

  /**
   * Can bars be dragged right now? Not for the mock data (it is not a real
   * project) and not in a plain browser (no Rust to send commands to).
   */
  const editable = !isDemoData && snapshot !== null && hasDesktopCore();

  /**
   * Shows the tooltip with a text near the mouse, or hides it with `null`.
   *
   * @param text - The text (already translated), or `null`.
   * @param x - Mouse x in the chart area (CSS px).
   * @param y - Mouse y in the chart area (CSS px).
   *
   * Writes to the DOM element directly instead of through React state:
   * this runs on every mouse move, and a React re-render each time would
   * be wasted work.
   */
  function showTooltip(text: string | null, x = 0, y = 0): void {
    const tip = tooltipRef.current;
    if (tip === null) return;
    if (text === null) {
      tip.hidden = true;
      return;
    }
    tip.textContent = text;
    tip.hidden = false;
    // Keep it inside the chart: shift it left, or put it above the mouse.
    const area = tip.parentElement;
    const maxLeft = (area?.clientWidth ?? Infinity) - tip.offsetWidth - 4;
    const below = y + 18;
    const fits = below + tip.offsetHeight <= (area?.clientHeight ?? Infinity);
    tip.style.left = `${Math.max(0, Math.min(x + 14, maxLeft))}px`;
    tip.style.top = `${fits ? below : Math.max(0, y - 8 - tip.offsetHeight)}px`;
  }

  /** Formats a date (ms) for the tooltip, in the user's date format. */
  function dateText(ms: number): string {
    return formatScheduleDate(isoDateTime(ms), dateLocale) ?? "";
  }

  /** Formats working minutes as days ("2.5"), with the project's hours per day. */
  function daysText(minutes: number): string {
    const perDay = snapshot?.minutesPerDay ?? 480;
    return formatNumber(minutes / perDay, numberLocale);
  }

  /**
   * Updates the preview (ghost bar or link line) and the tooltip for the
   * current mouse position.
   *
   * @param drag - The drag in progress (its `kind` is set).
   * @param x - Mouse x in the chart area.
   * @param y - Mouse y in the chart area.
   */
  function updatePreview(drag: DragState, x: number, y: number): void {
    const scroller = scrollerRef.current;
    const controller = controllerRef.current;
    const row = model.rows[drag.hit.row];
    if (scroller === null || controller === null || row === undefined) return;
    const task = drag.task;

    if (drag.kind === "link") {
      const target = rowAtY(y, scroller.scrollTop, model.rows.length);
      const targetTask = target === null ? undefined : model.rows[target];
      drag.targetRow =
        target !== null && target !== drag.hit.row && targetTask !== undefined ? target : null;
      controller.setOverlay({
        kind: "link",
        fromRow: drag.hit.row,
        pointerX: x,
        pointerY: y,
        targetRow: drag.targetRow,
      });
      const to = drag.targetRow === null ? undefined : model.rows[drag.targetRow];
      showTooltip(
        to === undefined
          ? t("gantt.drag.linkHint", { from: row.name })
          : t("gantt.drag.link", { from: row.name, to: to.name }),
        x,
        y,
      );
      return;
    }
    if (task === null || task.start === null || task.finish === null) return;

    const days = daysFromPixels(x - drag.startX, scale);
    // Drags work with the real dates; only the preview is drawn `filled`.
    const startMs = row.realStartMs;
    const finishMs = row.realFinishMs;
    const filled = (ms: number): number => fill(ms, row.calendarId);
    let nextStart: number;
    let nextFinish: number;
    if (drag.kind === "move") {
      nextStart = snapMoveStart(startMs, days, drag.calendar);
      // Until Rust answers, keep the bar's length in calendar days.
      nextFinish = nextStart + (finishMs - startMs);
    } else {
      nextStart = startMs;
      nextFinish = snapResizeFinish(startMs, finishMs, days, drag.calendar);
    }
    // Ask Rust only when the snapped date changed (not on every pixel).
    // For a move only the start counts: the finish shown may already be
    // Rust's exact answer, which must not be replaced by the estimate.
    const changed =
      drag.kind === "move" ? nextStart !== drag.previewStart : nextFinish !== drag.previewFinish;
    if (drag.kind === "move" && !changed) nextFinish = drag.previewFinish;
    drag.previewStart = nextStart;
    drag.previewFinish = nextFinish;
    controller.setOverlay({
      kind: "bar",
      row: drag.hit.row,
      startMs: filled(nextStart),
      finishMs: filled(nextFinish),
    });

    const calendarId = task.calendarId;
    if (drag.kind === "move") {
      showTooltip(
        t("gantt.drag.move", { start: dateText(nextStart), finish: dateText(nextFinish) }),
        x,
        y,
      );
      if (!changed) return;
      const seq = ++drag.previewSeq;
      // The real finish depends on the calendar: ask Rust.
      // With blockers the bar spans work + blocked time (both on the task
      // calendar), so the preview adds them up.
      calendarAddWorkingTime(
        calendarId,
        isoDateTime(nextStart),
        task.durationMinutes + (task.blockedMinutes ?? 0),
      )
        .then((finishIso) => {
          if (finishIso === null || dragRef.current !== drag || drag.previewSeq !== seq) return;
          drag.previewFinish = parseIsoLocal(finishIso);
          controller.setOverlay({
            kind: "bar",
            row: drag.hit.row,
            startMs: filled(drag.previewStart),
            finishMs: filled(drag.previewFinish),
          });
          showTooltip(
            t("gantt.drag.move", {
              start: dateText(drag.previewStart),
              finish: dateText(drag.previewFinish),
            }),
            x,
            y,
          );
        })
        .catch(() => undefined); // Only a preview: the drop reports errors.
    } else {
      showTooltip(t("gantt.drag.resizePending", { finish: dateText(nextFinish) }), x, y);
      if (!changed) return;
      const seq = ++drag.previewSeq;
      calendarWorkingMinutes(calendarId, task.start, isoDateTime(nextFinish))
        .then((spanMinutes) => {
          if (spanMinutes === null || dragRef.current !== drag || drag.previewSeq !== seq) return;
          const minutes = workMinutes(task, spanMinutes);
          showTooltip(
            t("gantt.drag.resize", {
              finish: dateText(nextFinish),
              days: daysText(minutes),
              count: minutes / (snapshot?.minutesPerDay ?? 480),
            }),
            x,
            y,
          );
        })
        .catch(() => undefined);
    }
  }

  /**
   * The planned work inside a bar span: the span minus the task's blocked
   * time, never below zero. Without blockers: the span.
   *
   * @param task - The task.
   * @param spanMinutes - Working minutes from the bar's start to a finish.
   * @returns The work minutes the new duration should have.
   */
  function workMinutes(task: TaskRow, spanMinutes: number): number {
    return Math.max(0, spanMinutes - (task.blockedMinutes ?? 0));
  }

  /**
   * Shows the name of the task under the mouse (and, on a blocked piece,
   * its blocker), or hides the hover tooltip when the mouse is on no bar.
   *
   * @param x - Mouse x in the chart area.
   * @param y - Mouse y in the chart area.
   * @param scroller - The scrolling layer (for the scroll position).
   */
  function updateHoverTooltip(x: number, y: number, scroller: HTMLDivElement): void {
    const input = {
      model,
      scale,
      x,
      y,
      scrollLeft: scroller.scrollLeft,
      scrollTop: scroller.scrollTop,
    };
    const onDeadline = hitTestDeadline(input);
    const row =
      hitTestBar(input) ??
      onDeadline ??
      hitTestLateStart(input, today) ??
      hitTestFinishArrow(input);
    const name = row === null ? undefined : model.rows[row]?.name;
    if (name === undefined) {
      if (hoverTipRef.current) {
        hoverTipRef.current = false;
        showTooltip(null);
      }
      return;
    }
    const lines = [name];
    const percent = row === null ? 0 : (model.rows[row]?.percentComplete ?? 0);
    if (percent > 0) lines.push(t("gantt.complete", { percent }));
    const hovered = row === null ? undefined : model.rows[row];
    if (hovered !== undefined && isLateToStart(hovered, today)) {
      lines.push(t("gantt.lateStart", { date: dateText(hovered.startMs) }));
    }
    if (hovered !== undefined && isMissedMilestone(hovered, today)) {
      const date = dateText(hovered.realFinishMs);
      lines.push(t("gantt.milestoneMissed", { date }));
    }
    const variance = hovered?.variance ?? null;
    if (variance !== null) {
      const { late: isLate, done } = variance;
      const key = done
        ? isLate
          ? "gantt.finishedLate"
          : "gantt.finishedEarly"
        : isLate
          ? "gantt.willFinishLate"
          : "gantt.willFinishEarly";
      lines.push(
        t("gantt.plannedFinish", { date: dateText(variance.plannedMs) }),
        t(key, { date: dateText(variance.finishMs) }),
      );
    }
    const late = onDeadline === null ? undefined : model.rows[onDeadline];
    if (late !== undefined) {
      const date = dateText(late.deadlineMs);
      lines.push(t(late.missedDeadline ? "gantt.deadlineMissed" : "gantt.deadline", { date }));
    }
    const blocker = hitTestBlocked(input)?.segment.blocker ?? null;
    if (blocker !== null) {
      const days = blocker.durationMinutes / (snapshot?.minutesPerDay ?? 480);
      lines.push(
        t("gantt.blocker.reason", { reason: blocker.reason }),
        t("gantt.blocker.length", { days: daysText(blocker.durationMinutes), count: days }),
      );
      if (blocker.reportedOn !== null) {
        const date = formatScheduleDate(`${blocker.reportedOn}T00:00:00`, dateLocale);
        if (date !== null) lines.push(t("gantt.blocker.reported", { date }));
      }
    }
    hoverTipRef.current = true;
    showTooltip(lines.join("\n"), x, y);
  }

  /** Ends the drag: hides the preview and the tooltip, forgets the state. */
  function endDrag(): void {
    dragRef.current = null;
    hoverTipRef.current = false;
    controllerRef.current?.setOverlay(null);
    showTooltip(null);
    window.removeEventListener("keydown", onDragKeyDown, true);
  }

  /**
   * Esc during a drag cancels it. Registered on `window` while dragging,
   * in the *capture* phase (`true`), so it runs before other key handlers.
   *
   * Beginner note: this function is declared with `function`, so it can be
   * used above its own line (functions are "hoisted").
   */
  function onDragKeyDown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || dragRef.current === null) return;
    event.preventDefault();
    event.stopPropagation();
    dragRef.current.cancelled = true;
    endDrag();
  }

  /**
   * Sends the one command a finished drag stands for.
   *
   * @param drag - The finished drag (its `kind` is set).
   */
  async function commitDrag(drag: DragState): Promise<void> {
    const { applyCommand, showIpcError } = useAppStore.getState();
    const task = drag.task;
    if (task === null) return;
    try {
      if (drag.kind === "link") {
        // Find the target by its ID, not by row number: when a summary is
        // collapsed, chart rows skip hidden tasks, so row N of the chart is
        // not task N of the snapshot.
        const targetId = drag.targetRow === null ? undefined : model.rows[drag.targetRow]?.id;
        const target = snapshot?.tasks.find((t) => t.id === targetId);
        if (target === undefined) return;
        const command = linkCommand(task.id, target.id, crypto.randomUUID());
        if (command !== null) await applyCommand(command);
      } else if (drag.kind === "move") {
        const command = moveCommand(task, drag.previewStart);
        if (command !== null) await applyCommand(command);
      } else if (drag.kind === "resize" && task.start !== null) {
        // The new duration is the working time from the start to the new
        // finish, on the task's calendar: Rust counts it.
        const minutes = await calendarWorkingMinutes(
          task.calendarId,
          task.start,
          isoDateTime(drag.previewFinish),
        );
        if (minutes === null) return;
        const command = resizeCommand(task, workMinutes(task, minutes));
        if (command !== null) await applyCommand(command);
      }
    } catch (error: unknown) {
      showIpcError(error);
    }
  }

  // ---- Pointer handlers ----------------------------------------------------

  /**
   * Gives the mouse position inside the chart area (viewport CSS px).
   *
   * @param event - A pointer event on the scrolling layer.
   * @returns `{ x, y }`; `getBoundingClientRect` tells where the layer is
   *   in the window, so `clientX - rect.left` is the position inside it.
   */
  function pointerPosition(event: PointerEvent<HTMLDivElement>): { x: number; y: number } {
    const rect = event.currentTarget.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  }

  /** Mouse button pressed: remember what was pressed; a drag may follow. */
  function onPointerDown(event: PointerEvent<HTMLDivElement>): void {
    if (event.button !== 0) return; // only the main (left) button
    const scroller = event.currentTarget;
    const { x, y } = pointerPosition(event);
    // Presses on the scroll bars belong to the browser.
    if (x >= scroller.clientWidth || y >= scroller.clientHeight) return;
    const hit = hitTestPart(model, scale, x, y, scroller.scrollLeft, scroller.scrollTop);
    if (hit === null) {
      if (!event.ctrlKey && !event.metaKey) useAppStore.getState().selectTask(null);
      return;
    }
    const row = model.rows[hit.row];
    // By ID, not by row number (collapsed summaries hide rows; see above).
    const task = editable ? (snapshot?.tasks.find((t) => t.id === row?.id) ?? null) : null;
    const calendarId = task?.calendarId ?? snapshot?.calendarId ?? null;
    dragRef.current = {
      pointerId: event.pointerId,
      startX: x,
      startY: y,
      hit,
      task,
      calendar: calendars.find((c) => c.id === calendarId) ?? null,
      kind: null,
      previewStart: row?.realStartMs ?? Number.NaN,
      previewFinish: row?.realFinishMs ?? Number.NaN,
      targetRow: null,
      previewSeq: 0,
      cancelled: false,
    };
    // "Pointer capture": all following moves and the release come to this
    // element, even when the mouse leaves it during the drag.
    scroller.setPointerCapture(event.pointerId);
  }

  /** Mouse moved: update the drag, or show the right cursor when hovering. */
  function onPointerMove(event: PointerEvent<HTMLDivElement>): void {
    const scroller = event.currentTarget;
    const { x, y } = pointerPosition(event);
    const drag = dragRef.current;
    if (drag === null || drag.pointerId !== event.pointerId) {
      // Hovering over a bar: show its name and blocker (read-only too).
      updateHoverTooltip(x, y, scroller);
      // Hovering: a resize cursor on right edges, a move cursor on bars.
      if (!editable) return;
      const hit = hitTestPart(model, scale, x, y, scroller.scrollLeft, scroller.scrollTop);
      const summary = hit !== null && model.rows[hit.row]?.isSummary === true;
      scroller.style.cursor =
        hit === null ? "" : hit.part === "finishEdge" ? "ew-resize" : summary ? "pointer" : "grab";
      return;
    }
    if (drag.task === null) return; // mock data: clicks only
    if (drag.kind === null) {
      // A drag replaces the hover tooltip with its own.
      hoverTipRef.current = false;
      const canChangeDates = !drag.task.isSummary && drag.task.start !== null;
      drag.kind = classifyDrag(drag.hit.part, x - drag.startX, y - drag.startY, canChangeDates);
      if (drag.kind === null) return;
      window.addEventListener("keydown", onDragKeyDown, true);
      scroller.style.cursor =
        drag.kind === "resize" ? "ew-resize" : drag.kind === "link" ? "crosshair" : "grabbing";
    }
    updatePreview(drag, x, y);
  }

  /** Mouse released: a click selects; a drag sends its command. */
  function onPointerUp(event: PointerEvent<HTMLDivElement>): void {
    const drag = dragRef.current;
    if (drag === null || drag.pointerId !== event.pointerId) return;
    event.currentTarget.style.cursor = "";
    if (drag.kind === null && !drag.cancelled) {
      // It was a click.
      const id = model.rows[drag.hit.row]?.id;
      if (id !== undefined) {
        const store = useAppStore.getState();
        if (event.ctrlKey || event.metaKey) store.toggleTaskSelection(id);
        else store.selectTask(id);
      }
      endDrag();
      return;
    }
    endDrag();
    if (!drag.cancelled) void commitDrag(drag);
  }

  /** The mouse left the chart: hide a hover tooltip (not a drag's). */
  function onPointerLeave(): void {
    if (dragRef.current === null && hoverTipRef.current) {
      hoverTipRef.current = false;
      showTooltip(null);
    }
  }

  /** The system took the pointer away (e.g. a window switch): cancel. */
  function onPointerCancel(): void {
    if (dragRef.current !== null) endDrag();
  }

  return (
    <div className="relative h-full w-full overflow-hidden bg-surface">
      {/* The canvas is decoration for screen readers (aria-hidden); the
          scroller below carries the accessible label. pointer-events-none
          lets mouse and trackpad events pass through to the scroller. */}
      <canvas
        ref={canvasRef}
        aria-hidden="true"
        className="pointer-events-none absolute top-0 left-0 block"
      />
      <div
        ref={scrollerRef}
        // A marker for other features: the task grid finds this element by
        // the attribute to keep both scrolled to the same rows
        // (`features/grid/useScrollSync.ts`).
        data-gantt-scroller=""
        tabIndex={0}
        role="region"
        aria-label={t("gantt.label")}
        className="visible-scrollbars absolute inset-0 touch-none overflow-x-scroll overflow-y-auto"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerCancel}
        onPointerLeave={onPointerLeave}
        onLostPointerCapture={onPointerCancel}
      >
        <div style={{ width: scale.totalWidth, height: contentHeight(model.rows.length) }} />
      </div>
      {/* Drag and hover tooltip: text set directly by showTooltip().
          role="status" lets screen readers announce the new dates while
          dragging. `whitespace-pre-line` starts a new line at each line
          break (the blocker tooltip has several) and wraps a long reason. */}
      <div
        ref={tooltipRef}
        role="status"
        hidden
        className="pointer-events-none absolute z-10 w-max max-w-lg rounded border border-line bg-surface px-3 py-2 text-base whitespace-pre-line text-fg shadow"
      />
    </div>
  );
}
