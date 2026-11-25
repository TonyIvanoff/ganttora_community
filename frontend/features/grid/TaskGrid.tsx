/**
 * features/grid/TaskGrid.tsx: the task sheet, the spreadsheet-like table
 * left of the Gantt chart.
 *
 * What it is for:
 *   Shows one row per task (ID, indicators, WBS number, name, duration,
 *   start, finish, critical, predecessors, mode) and lets the user edit them in
 *   place, select rows, change the outline (indent, outdent, collapse,
 *   insert, delete, drag to reorder), copy and paste, and open the Task
 *   Information dialog, all with mouse *and* keyboard.
 *
 * Where it fits:
 *   Rendered by `shell/AppShell.tsx` next to the Gantt chart. Reads the
 *   snapshot from the store (`state/store.ts`); every edit becomes document
 *   commands (`cellEdit.ts`, `useTaskActions.ts`) sent to Rust (decision
 *   D16). The rows scroll together with the chart (`useScrollSync.ts`).
 *
 * Main ideas:
 *   - **Virtual rows (TanStack Virtual).** With 10,000 tasks only the
 *     ~40 rows on screen exist in the page; the others are just empty
 *     space of the right height. Scrolling stays smooth at any size.
 *   - **Columns (TanStack Table).** The table library keeps the column
 *     list and header cells; we draw the cells ourselves (`GridCell.tsx`).
 *   - **Same geometry as the chart.** Header `HEADER_HEIGHT` and rows
 *     `ROW_HEIGHT` pixels, read from `features/gantt/renderer/layout.ts`,
 *     so row N sits next to bar N.
 *   - **Active cell and focus.** The scrolling element itself has the
 *     keyboard focus (`tabIndex={0}`); the active cell is only drawn with a
 *     frame and announced with `aria-activedescendant`. While a cell is
 *     edited, the focus is in its editor.
 *   - **Keys** (when the grid has focus): which key means what is decided
 *     by the pure function `matchGridShortcut` (`gridShortcuts.ts`, where
 *     the full list is); this component only carries the action out:
 *     arrows, Home/End, Page Up/Down move; Enter, F2 or typing edits;
 *     Insert / Delete add and delete tasks; Ctrl/Cmd+C, X, V copy, cut and
 *     paste rows; Alt+Shift+arrows indent, outdent and move; Shift+F2
 *     opens Task (or Project) Information; Shift+F10 (or the Menu key) opens the row's
 *     context menu.
 *   - **Mouse.** A double-click anywhere on a row opens its information
 *     dialog; Ctrl+click (⌘+click on a Mac) edits the clicked cell. The
 *     row's tooltip says so.
 *   - **Context menu** (`RowContextMenu.tsx`): a right-click on a row
 *     selects it and offers "Task information…" and "Add blocker…".
 *   - **Column widths.** Drag the right edge of a column header (or focus
 *     it and press ← / →) to resize the column; double-click it for the
 *     default width. Widths are saved in the settings
 *     (`grid.columnWidths`, see `columnWidths.ts`). While dragging, the
 *     new width is kept in this component only (`liveWidths`) and written
 *     to the settings once, when the mouse button is released.
 */

import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type KeyboardEvent,
  type MouseEvent,
  type PointerEvent,
  type RefObject,
} from "react";
import { useTranslation } from "react-i18next";
import { createColumnHelper, tableFeatures, useTable } from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { HEADER_HEIGHT, ROW_HEIGHT } from "@frontend/features/gantt/renderer/layout";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import { useFormatLocale } from "@frontend/features/settings/useFormatLocale";
import { infoTabOf, useTaskInfoStore } from "@frontend/features/task-info/taskInfoStore";
import { claimAction } from "@frontend/features/toolbars/actionClaim";
import { isActionEnabled, type ActionId } from "@frontend/features/toolbars/actions";
import {
  actionTexts,
  IS_MAC,
  useActionState,
  useAnyT,
  useRunAction,
} from "@frontend/features/toolbars/useActions";
import { useAppStore } from "@frontend/state/store";
import { commandsForCellEdit, GRID_COLUMN_IDS, isEditable, type GridColumnId } from "./cellEdit";
import {
  clampColumnWidth,
  columnLefts,
  DEFAULT_COLUMN_WIDTHS,
  MAX_COLUMN_WIDTH,
  MIN_COLUMN_WIDTH,
  resolveColumnWidths,
} from "./columnWidths";
import { projectTint } from "@frontend/lib/projectTint";
import { CellEditor, IndicatorsCell, NameCell } from "./GridCell";
import {
  cellText,
  editorKind,
  editorStartText,
  type CellFormat,
  type EditMove,
} from "./gridFormat";
import { rowsFromMock, rowsFromSnapshot, type GridRow } from "./gridRows";
import { currentGridPlatform, matchGridShortcut } from "./gridShortcuts";
import { useGridStore } from "./gridStore";
import { moveTargetIndex, subtreeEnd } from "./outline";
import { PasteNotice } from "./PasteNotice";
import { RowContextMenu, type RowMenuEntry } from "./RowContextMenu";
import { useScrollSync } from "./useScrollSync";
import { useTaskActions } from "./useTaskActions";
import { useUnitContext } from "./useUnitContext";

/** The row context menu: actions in groups. */
const ROW_MENU: readonly (ActionId | "separator")[] = [
  "addTask",
  "addSubtask",
  "addMilestone",
  "linkTasks",
  "separator",
  "indent",
  "outdent",
  "separator",
  "addBlocker",
  "addRisk",
  "markOnTrack",
  "taskInfo",
  "separator",
  "cut",
  "copy",
  "paste",
  "separator",
  "deleteTasks",
];

/** Translation keys of the column titles. */
const COLUMN_TITLE_KEYS = {
  rowNumber: "grid.column.rowNumber",
  indicators: "grid.column.indicators",
  outline: "grid.column.outline",
  name: "grid.column.name",
  duration: "grid.column.duration",
  start: "grid.column.start",
  finish: "grid.column.finish",
  plannedFinish: "grid.column.plannedFinish",
  actualFinish: "grid.column.actualFinish",
  complete: "grid.column.complete",
  predecessors: "grid.column.predecessors",
  mode: "grid.column.mode",
} as const satisfies Record<GridColumnId, string>;

/** The platform, for the grid's shortcuts (Backspace deletes on macOS). */
const PLATFORM = currentGridPlatform();

/** `true` on macOS: only used to show the right shortcut text in the menu. */

/** How many pixels one ← / → key press resizes a column by. */
const RESIZE_KEY_STEP = 8;

/**
 * The table's feature set: only the core (columns, header groups, rows).
 * Kept at module level: TanStack Table wants it stable between renders.
 */
const features = tableFeatures({});
/** Builds typed column definitions for {@link GridRow} data. */
const columnHelper = createColumnHelper<typeof features, GridRow>();

/** The cell being edited. */
interface EditState {
  /** The task of the edited row. */
  rowId: string;
  /** The edited column. */
  column: GridColumnId;
  /** The text the editor starts with. */
  initialText: string;
  /** Select all text at the start (Enter/F2) or not (typing). */
  selectAll: boolean;
  /** A translated error from the last save attempt, or `null`. */
  error: string | null;
}

/** Where a dragged row would land, for drawing the drop line. */
interface DropTarget {
  /** Visible row index the line is drawn at. */
  visibleIndex: number;
  /** Above or below that row. */
  after: boolean;
}

/** A column-edge drag in progress. */
interface ColumnResize {
  /** The column being resized. */
  column: GridColumnId;
  /** Pointer x position when the drag started (CSS pixels). */
  startX: number;
  /** The column's width when the drag started. */
  startWidth: number;
}

/** Props of {@link TaskGrid}. */
export interface TaskGridProps {
  /** The element around the Gantt chart, for scroll syncing. */
  ganttContainerRef: RefObject<HTMLElement | null>;
}

/**
 * Whether a click means "edit this cell": ⌘+click on a Mac (where Ctrl+click
 * is the right-click), Ctrl+click elsewhere.
 *
 * @param event - The mouse event.
 * @returns `true` for an edit click.
 */
function isEditClick(event: MouseEvent): boolean {
  return !event.shiftKey && (IS_MAC ? event.metaKey : event.ctrlKey);
}

/**
 * The task grid.
 *
 * @param props - See {@link TaskGridProps}.
 * @returns The grid, filling the height of its parent; its width comes from
 *   `gridStore.width`.
 */
export function TaskGrid({ ganttContainerRef }: TaskGridProps) {
  const { t } = useTranslation();
  const snapshot = useAppStore((s) => s.snapshot);
  const isDemoData = useAppStore((s) => s.isDemoData);
  const mockTasks = useAppStore((s) => s.tasks);
  const collapsed = useAppStore((s) => s.collapsedTaskIds);
  const activeId = useAppStore((s) => s.selectedTaskId);
  const selectTask = useAppStore((s) => s.selectTask);
  const toggleCollapsed = useAppStore((s) => s.toggleCollapsed);
  const applyCommands = useAppStore((s) => s.applyCommands);
  const selectedTaskIds = useAppStore((s) => s.selectedTaskIds);
  const selectTasks = useAppStore((s) => s.selectTasks);
  const toggleTaskSelection = useAppStore((s) => s.toggleTaskSelection);
  const anchorId = useGridStore((s) => s.anchorId);
  const setAnchorId = useGridStore((s) => s.setAnchorId);
  const pendingEditId = useGridStore((s) => s.pendingEditId);
  const setPendingEditId = useGridStore((s) => s.setPendingEditId);
  const width = useGridStore((s) => s.width);
  const calendars = useAppStore((s) => s.calendars);
  const openTaskInfo = useTaskInfoStore((s) => s.open);
  const actions = useTaskActions();
  const runAction = useRunAction();
  const actionState = useActionState();
  const anyT = useAnyT();
  const { dateLocale, numberLocale } = useFormatLocale();
  const units = useUnitContext(snapshot?.minutesPerDay ?? 480);
  const baseId = useId();

  const scrollerRef = useRef<HTMLDivElement>(null);
  const [activeColumn, setActiveColumn] = useState<GridColumnId>("name");
  const [editing, setEditing] = useState<EditState | null>(null);
  const [dropTarget, setDropTarget] = useState<DropTarget | null>(null);
  /** Widths of a column drag in progress (win over the saved widths). */
  const [liveWidths, setLiveWidths] = useState<Partial<Record<GridColumnId, number>>>({});
  /** The column-edge drag in progress, or `null`. */
  const resizeRef = useRef<ColumnResize | null>(null);
  const savedWidths = useSettingsStore((s) => s.settings.grid.columnWidths);
  /** The row being dragged (full-list index), or `null`. */
  const dragSource = useRef<number | null>(null);
  /** Where the row context menu is open (window coordinates), or `null`. */
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);

  // The width of every column, and where each one starts.
  const columnWidth = useMemo(
    () => resolveColumnWidths(savedWidths, liveWidths),
    [savedWidths, liveWidths],
  );
  const { left: columnLeft, total: totalWidth } = useMemo(
    () => columnLefts(columnWidth),
    [columnWidth],
  );

  const readOnly = isDemoData || snapshot === null;
  const allTasks = useMemo(() => snapshot?.tasks ?? [], [snapshot]);

  // The visible rows (rebuilt only when the data or the collapsing changes).
  const rows = useMemo(
    () =>
      isDemoData
        ? rowsFromMock(mockTasks)
        : snapshot === null
          ? []
          : rowsFromSnapshot(snapshot, collapsed),
    [isDemoData, mockTasks, snapshot, collapsed],
  );
  // Task ID → visible row index, and → row number (for predecessor text).
  const visibleIndexById = useMemo(() => new Map(rows.map((r, i) => [r.id, i])), [rows]);
  const rowNumberById = useMemo(
    () => new Map(allTasks.map((task, i) => [task.id, i + 1])),
    [allTasks],
  );
  const format: CellFormat = useMemo(
    () => ({ units, dateLocale, rowNumberOf: (id) => rowNumberById.get(id) }),
    [units, dateLocale, rowNumberById],
  );

  // The selection is shared with the Gantt chart (store), so a click on a
  // bar selects the row and a click on a row selects the bar.
  const selection = useMemo(() => new Set(selectedTaskIds), [selectedTaskIds]);
  const activeIndex = activeId === null ? -1 : (visibleIndexById.get(activeId) ?? -1);

  // Columns for TanStack Table: an ID and a title each. `display` columns
  // read no data field; the cells are drawn by this component.
  const columns = useMemo(
    () =>
      columnHelper.columns(
        GRID_COLUMN_IDS.map((id) =>
          columnHelper.display({ id, header: () => t(COLUMN_TITLE_KEYS[id]) }),
        ),
      ),
    [t],
  );
  const table = useTable({ features, columns, data: rows });
  const headers = table.getHeaderGroups()[0]?.headers ?? [];

  // The virtualizer works out which rows are on screen. `scrollMargin`
  // tells it the rows start below the header.
  // The lint rule warns that TanStack Virtual's functions cannot be cached
  // by the React Compiler; that is expected (it then simply skips caching
  // this component), and we never pass those functions to cached children.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollerRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
    scrollMargin: HEADER_HEIGHT,
    getItemKey: (index) => rows[index]?.id ?? index,
  });

  useScrollSync(scrollerRef, ganttContainerRef, isDemoData);

  // Keep the active cell on screen (after keys, Gantt clicks, inserts).
  useEffect(() => {
    const scroller = scrollerRef.current;
    if (scroller === null || activeIndex < 0) return;
    const top = activeIndex * ROW_HEIGHT; // in the rows area, below the header
    const bodyHeight = scroller.clientHeight - HEADER_HEIGHT;
    if (top < scroller.scrollTop) scroller.scrollTop = top;
    else if (top + ROW_HEIGHT > scroller.scrollTop + bodyHeight) {
      scroller.scrollTop = top + ROW_HEIGHT - bodyHeight;
    }
    const left = columnLeft[activeColumn];
    const right = left + columnWidth[activeColumn];
    if (left < scroller.scrollLeft) scroller.scrollLeft = left;
    else if (right > scroller.scrollLeft + scroller.clientWidth) {
      scroller.scrollLeft = right - scroller.clientWidth;
    }
    // Only keys and clicks should scroll, not a column resize: the widths
    // are read when the active cell changes but are not a reason to run.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeIndex, activeColumn]);

  // After "Insert task": open the new task's name as soon as its row exists.
  useEffect(() => {
    if (pendingEditId === null) return;
    const index = visibleIndexById.get(pendingEditId);
    const row = index === undefined ? undefined : rows[index];
    if (row === undefined) return;
    setPendingEditId(null);
    setActiveColumn("name");
    setEditing({
      rowId: row.id,
      column: "name",
      initialText: row.name,
      selectAll: true,
      error: null,
    });
  }, [pendingEditId, visibleIndexById, rows, setPendingEditId]);

  /** Gives the focus back to the grid (after an edit, a click). */
  function focusGrid(): void {
    scrollerRef.current?.focus({ preventScroll: true });
  }

  /**
   * Makes a row active, with a selection change.
   *
   * @param index - Visible row index (clamped to the list).
   * @param mode - "single" replaces the selection; "extend" selects from
   *   the anchor to here (Shift); "toggle" adds/removes this row (Ctrl/⌘).
   */
  function activateRow(index: number, mode: "single" | "extend" | "toggle" = "single"): void {
    const row = rows[Math.max(0, Math.min(rows.length - 1, index))];
    if (row === undefined) return;
    if (mode === "extend") {
      const anchorIndex = visibleIndexById.get(anchorId ?? activeId ?? row.id) ?? 0;
      const [from, to] = anchorIndex <= index ? [anchorIndex, index] : [index, anchorIndex];
      const range = rows.slice(from, to + 1).map((r) => r.id);
      // The clicked row is the primary one: put it last.
      selectTasks([...range.filter((id) => id !== row.id), row.id], row.id);
    } else if (mode === "toggle") {
      toggleTaskSelection(row.id);
      setAnchorId(row.id);
    } else {
      selectTask(row.id);
      setAnchorId(row.id);
    }
  }

  /**
   * Starts editing the active cell, if it can be edited.
   *
   * @param typed - A typed character that replaces the text, or `null` to
   *   start with the current value.
   */
  function startEditing(typed: string | null): void {
    const row = rows[activeIndex];
    if (readOnly || row?.task == null || !isEditable(activeColumn, row.task)) return;
    const kind = editorKind(activeColumn);
    const initialText =
      typed !== null && (kind === "text" || (kind === "percent" && /\d/.test(typed)))
        ? typed
        : editorStartText(activeColumn, row, format, t);
    setEditing({
      rowId: row.id,
      column: activeColumn,
      initialText,
      // A letter typed into a percentage is ignored: the old number is kept.
      selectAll: typed === null || (kind === "percent" && !/\d/.test(typed)),
      error: null,
    });
  }

  /**
   * Moves the active cell after an edit or a key.
   *
   * @param move - The direction.
   */
  function moveActive(move: EditMove): void {
    const col = GRID_COLUMN_IDS.indexOf(activeColumn);
    if (move === "down") activateRow(activeIndex + 1);
    else if (move === "up") activateRow(activeIndex - 1);
    else if (move === "right") setActiveColumn(GRID_COLUMN_IDS[col + 1] ?? activeColumn);
    else if (move === "left") setActiveColumn(GRID_COLUMN_IDS[col - 1] ?? activeColumn);
  }

  /**
   * Saves the editor's text: checks it, sends the commands, moves on.
   *
   * @param text - The editor's text.
   * @param move - Where to go afterwards.
   * @returns `false` if the text was refused (the editor stays open).
   */
  function commit(text: string, move: EditMove): boolean {
    if (editing === null) return true;
    const index = visibleIndexById.get(editing.rowId);
    const row = index === undefined ? undefined : rows[index];
    if (row?.task == null || snapshot === null) {
      setEditing(null);
      return true;
    }
    const result = commandsForCellEdit(editing.column, text, row.task, {
      units,
      tasks: snapshot.tasks,
      rowNumber: row.index + 1,
      newId: () => crypto.randomUUID(),
    });
    if (!result.ok) {
      // The key is built from a problem code, so TypeScript cannot check it.
      setEditing({
        ...editing,
        error: t(result.key as "grid.invalid.duration", result.values ?? {}),
      });
      return false;
    }
    setEditing(null);
    focusGrid();
    moveActive(move);
    // Several commands (e.g. predecessor changes) go as one batch: one undo step.
    if (result.commands.length > 0) void applyCommands(result.commands);
    return true;
  }

  /** Cancels editing (Escape). */
  function cancel(): void {
    setEditing(null);
    focusGrid();
  }

  /**
   * The grid's keyboard handling. `matchGridShortcut` (gridShortcuts.ts)
   * says which action a key press means; here we check whether it is
   * allowed right now and carry it out.
   *
   * @param event - A key press inside the grid (it may come from a cell
   *   editor, whose own key presses *bubble up* to here).
   */
  function onKeyDown(event: KeyboardEvent<HTMLDivElement>): void {
    const shortcut = matchGridShortcut(event, PLATFORM);
    if (shortcut === null) return;
    // While a cell is edited, its editor handles the keys.
    if (editing !== null) return;
    if (rows.length === 0) {
      // An empty project: only "insert a task" makes sense.
      if (shortcut.action === "insertTask" && !readOnly) {
        event.preventDefault();
        void actions.insertTask();
      }
      return;
    }
    const page = Math.max(
      1,
      Math.floor(((scrollerRef.current?.clientHeight ?? 0) - HEADER_HEIGHT) / ROW_HEIGHT) - 1,
    );

    switch (shortcut.action) {
      // Outline changes: in a read-only view the key is left alone.
      case "indent":
      case "outdent":
      case "moveTaskUp":
      case "moveTaskDown":
      case "collapse":
      case "expand":
        if (readOnly) return;
        if (shortcut.action === "indent") void actions.indentSelected();
        else if (shortcut.action === "outdent") void actions.outdentSelected();
        else if (shortcut.action === "moveTaskUp") void actions.moveActive("up");
        else if (shortcut.action === "moveTaskDown") void actions.moveActive("down");
        else actions.collapseSelected(shortcut.action === "collapse");
        break;
      // `claimAction`: on macOS the menu bar may report the same key press.
      case "copy":
        if (claimAction("copy", "key")) void actions.copySelected();
        break;
      case "cut":
        if (!readOnly && claimAction("cut", "key")) void actions.cutSelected();
        break;
      case "paste":
        if (!readOnly && claimAction("paste", "key")) void actions.paste();
        break;
      case "rowDown":
      case "rowUp": {
        const next = activeIndex + (shortcut.action === "rowDown" ? 1 : -1);
        activateRow(activeIndex < 0 ? 0 : next, shortcut.extend ? "extend" : "single");
        break;
      }
      case "columnRight":
        moveActive("right");
        break;
      case "columnLeft":
        moveActive("left");
        break;
      case "home":
        if (shortcut.firstRow) activateRow(0);
        setActiveColumn(GRID_COLUMN_IDS[0]);
        break;
      case "end":
        if (shortcut.lastRow) activateRow(rows.length - 1);
        setActiveColumn(GRID_COLUMN_IDS[GRID_COLUMN_IDS.length - 1] ?? "mode");
        break;
      case "pageDown":
        activateRow(activeIndex + page, shortcut.extend ? "extend" : "single");
        break;
      case "pageUp":
        activateRow(activeIndex - page, shortcut.extend ? "extend" : "single");
        break;
      case "taskInfo":
        actions.openTaskInfo();
        break;
      case "contextMenu": {
        // Open the menu under the active cell (there is no mouse position).
        const cell =
          activeIndex >= 0
            ? document.getElementById(`${baseId}-r${activeIndex}-${activeColumn}`)
            : null;
        if (cell === null) break;
        const rect = cell.getBoundingClientRect();
        setMenu({ x: rect.left, y: rect.bottom });
        break;
      }
      case "edit":
        startEditing(null);
        break;
      case "type":
        // A printable character: start typing into the cell, like a spreadsheet.
        startEditing(shortcut.text);
        break;
      case "insertTask":
        if (!readOnly) void actions.insertTask();
        break;
      case "deleteTasks":
        if (!readOnly) void actions.deleteSelected();
        break;
    }
    // The key was ours: stop the web view's own reaction (scrolling the
    // page with the arrows, its own copy…).
    event.preventDefault();
  }

  /**
   * Writes one column's width into the settings (settings.json), then
   * drops the drag's temporary width.
   *
   * @param column - The column.
   * @param width - The new width in CSS pixels (already clamped).
   */
  function saveColumnWidth(column: GridColumnId, width: number): void {
    const store = useSettingsStore.getState();
    const { settings } = store;
    const next = {
      ...settings,
      grid: { ...settings.grid, columnWidths: { ...settings.grid.columnWidths, [column]: width } },
    };
    // `save` resolves with `null` on success or the error. On an error the
    // column goes back to its saved width: better than showing a width
    // that will be forgotten.
    void store.save(next).then((error) => {
      if (error !== null) console.error("Could not save the column width", error);
      setLiveWidths({});
    });
  }

  /**
   * Starts dragging a column's right edge.
   *
   * @param event - The pointer press on the resize handle.
   * @param column - The column whose edge it is.
   */
  function onResizePointerDown(event: PointerEvent<HTMLDivElement>, column: GridColumnId): void {
    if (event.button !== 0) return; // left button (or a touch / pen) only
    event.preventDefault();
    event.stopPropagation();
    // *Pointer capture*: all further moves go to this handle, even when the
    // pointer leaves it, until the button is released.
    event.currentTarget.setPointerCapture(event.pointerId);
    resizeRef.current = { column, startX: event.clientX, startWidth: columnWidth[column] };
  }

  /**
   * Follows the pointer while a column edge is dragged.
   *
   * @param event - The pointer move.
   */
  function onResizePointerMove(event: PointerEvent<HTMLDivElement>): void {
    const resize = resizeRef.current;
    if (resize === null) return;
    const width = clampColumnWidth(
      resize.startWidth + event.clientX - resize.startX,
      resize.startWidth,
    );
    setLiveWidths({ [resize.column]: width });
  }

  /**
   * Ends a column-edge drag and saves the new width.
   *
   * @param event - The pointer release.
   */
  function onResizePointerUp(event: PointerEvent<HTMLDivElement>): void {
    const resize = resizeRef.current;
    if (resize === null) return;
    resizeRef.current = null;
    const width = clampColumnWidth(
      resize.startWidth + event.clientX - resize.startX,
      resize.startWidth,
    );
    if (width === resize.startWidth) setLiveWidths({});
    else saveColumnWidth(resize.column, width);
  }

  /** Cancels a column-edge drag (pointer lost, e.g. the window lost focus). */
  function onResizeCancel(): void {
    if (resizeRef.current === null) return;
    resizeRef.current = null;
    setLiveWidths({});
  }

  /**
   * Resizes a column with the keyboard while its handle has the focus.
   *
   * @param event - The key press (← narrower, → wider).
   * @param column - The column.
   */
  function onResizeKeyDown(event: KeyboardEvent<HTMLDivElement>, column: GridColumnId): void {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    // Keep the grid's own arrow-key handling (moving the active cell) out.
    event.stopPropagation();
    const step = event.key === "ArrowRight" ? RESIZE_KEY_STEP : -RESIZE_KEY_STEP;
    const width = clampColumnWidth(columnWidth[column] + step, columnWidth[column]);
    if (width === columnWidth[column]) return;
    // Show it at once; the settings follow.
    setLiveWidths({ [column]: width });
    saveColumnWidth(column, width);
  }

  /**
   * A mouse press on a cell: selects the row (Shift = range; Ctrl/⌘ on a
   * cell that cannot be edited = add/remove) and makes the cell active.
   * Ctrl/⌘ on an editable cell edits it (see the cell's `onClick`).
   *
   * @param event - The mouse event.
   * @param index - Visible row index.
   * @param column - The column.
   */
  function onCellMouseDown(event: MouseEvent, index: number, column: GridColumnId): void {
    if (editing !== null && editing.rowId === rows[index]?.id && editing.column === column) return;
    const row = rows[index];
    const edits =
      isEditClick(event) && !readOnly && row?.task != null && isEditable(column, row.task);
    const mode = event.shiftKey
      ? "extend"
      : !edits && (event.ctrlKey || event.metaKey)
        ? "toggle"
        : "single";
    activateRow(index, mode);
    setActiveColumn(column);
  }

  /**
   * Starts dragging a row by its ID cell.
   *
   * @param event - The drag event.
   * @param row - The dragged row.
   */
  function onDragStart(event: DragEvent, row: GridRow): void {
    dragSource.current = row.index;
    event.dataTransfer.effectAllowed = "move";
    // Some systems only start a drag when data is set.
    event.dataTransfer.setData("text/plain", row.name);
  }

  /**
   * Shows where a dragged row would land.
   *
   * @param event - The drag-over event.
   * @param visibleIndex - The row under the pointer.
   */
  function onDragOver(event: DragEvent<HTMLDivElement>, visibleIndex: number): void {
    if (dragSource.current === null) return;
    event.preventDefault(); // allows the drop
    const rect = event.currentTarget.getBoundingClientRect();
    const after = event.clientY > rect.top + rect.height / 2;
    if (dropTarget?.visibleIndex !== visibleIndex || dropTarget.after !== after) {
      setDropTarget({ visibleIndex, after });
    }
  }

  /** Drops the dragged row: sends one `moveTask` command. */
  function onDrop(event: DragEvent): void {
    event.preventDefault();
    const source = dragSource.current;
    const target = dropTarget;
    dragSource.current = null;
    setDropTarget(null);
    if (source === null || target === null || snapshot === null) return;
    const row = rows[target.visibleIndex];
    const task = snapshot.tasks[source];
    if (row === undefined || task === undefined) return;
    // "Below a row" = above the next visible row (after a collapsed summary:
    // after its hidden subtasks).
    const before = !target.after
      ? row.index
      : row.collapsed
        ? subtreeEnd(snapshot.tasks, row.index)
        : row.index + 1;
    const toIndex = moveTargetIndex(snapshot.tasks, source, before);
    if (toIndex === null) return;
    void applyCommands([{ type: "moveTask", id: task.id, toIndex, level: null }]);
  }

  /**
   * A right-click on a row: select it (unless it is already part of the
   * selection) and open the context menu at the mouse.
   *
   * @param event - The mouse event.
   * @param index - Visible row index.
   */
  function onRowContextMenu(event: MouseEvent, index: number): void {
    const row = rows[index];
    if (row === undefined || row.task === null) return; // mock rows: browser menu
    event.preventDefault();
    if (!selection.has(row.id)) activateRow(index);
    else if (row.id !== activeId) selectTasks([...selectedTaskIds], row.id);
    setMenu({ x: event.clientX, y: event.clientY });
  }

  /** The context menu's entries, from the action registry. */
  const menuItems: RowMenuEntry[] = ROW_MENU.map((id, i) =>
    id === "separator"
      ? { key: `separator-${i}`, separator: true }
      : {
          key: id,
          label: actionTexts(id, anyT, undefined, actionState).label,
          shortcut: actionTexts(id, anyT).shortcut,
          disabled: !isActionEnabled(id, actionState),
          onSelect: () => runAction(id, "button"),
        },
  );

  const activeCellId =
    activeIndex >= 0 && editing === null ? `${baseId}-r${activeIndex}-${activeColumn}` : undefined;

  return (
    <div
      className="flex h-full shrink-0 flex-col border-r border-line bg-surface"
      style={{ width }}
    >
      <PasteNotice />
      <div
        ref={scrollerRef}
        role="treegrid"
        tabIndex={0}
        aria-label={t("grid.label")}
        aria-rowcount={rows.length + 1}
        aria-colcount={GRID_COLUMN_IDS.length}
        aria-multiselectable="true"
        aria-readonly={readOnly}
        aria-activedescendant={activeCellId}
        className="visible-scrollbars relative min-h-0 flex-1 overflow-x-scroll overflow-y-auto text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset"
        onKeyDown={onKeyDown}
        onDragLeave={(e) => {
          if (e.currentTarget === e.target) setDropTarget(null);
        }}
        onDragEnd={() => {
          dragSource.current = null;
          setDropTarget(null);
        }}
      >
        {/* Header: sticky, exactly as tall as the Gantt chart's header. */}
        <div
          role="row"
          aria-rowindex={1}
          className="sticky top-0 z-10 flex border-b border-line bg-surface"
          style={{ height: HEADER_HEIGHT, width: totalWidth }}
        >
          {headers.map((header) => {
            const id = header.column.id as GridColumnId;
            const title = t(COLUMN_TITLE_KEYS[id]);
            return (
              <div
                key={header.id}
                role="columnheader"
                title={title}
                className="relative flex items-end border-r border-line px-1 pb-1 text-xs leading-tight font-medium text-fg-muted"
                style={{ width: columnWidth[id] }}
              >
                <span className="line-clamp-2 overflow-hidden break-words">
                  <table.FlexRender header={header} />
                </span>
                {/* Resize handle: a thin strip over the right edge. A
                    *separator* with a value is how screen readers know a
                    draggable divider; aria-valuenow says the width. */}
                <div
                  role="separator"
                  aria-orientation="vertical"
                  aria-label={t("grid.resizeColumn", { column: title })}
                  aria-valuenow={columnWidth[id]}
                  aria-valuemin={MIN_COLUMN_WIDTH}
                  aria-valuemax={MAX_COLUMN_WIDTH}
                  title={t("grid.resizeColumnTitle")}
                  tabIndex={0}
                  className="absolute top-0 right-0 z-10 h-full w-2 cursor-col-resize touch-none outline-none hover:bg-accent/40 focus-visible:bg-accent"
                  onPointerDown={(e) => onResizePointerDown(e, id)}
                  onPointerMove={onResizePointerMove}
                  onPointerUp={onResizePointerUp}
                  onPointerCancel={onResizeCancel}
                  onLostPointerCapture={onResizeCancel}
                  onDoubleClick={() => {
                    setLiveWidths({ [id]: DEFAULT_COLUMN_WIDTHS[id] });
                    saveColumnWidth(id, DEFAULT_COLUMN_WIDTHS[id]);
                  }}
                  onKeyDown={(e) => onResizeKeyDown(e, id)}
                />
              </div>
            );
          })}
        </div>

        {rows.length === 0 && snapshot !== null && !isDemoData && (
          <p className="p-3 text-fg-muted">{t("grid.empty")}</p>
        )}

        {/* The rows area: as tall as all rows; only visible rows exist. */}
        <div className="relative" style={{ height: rows.length * ROW_HEIGHT, width: totalWidth }}>
          {virtualizer.getVirtualItems().map((item) => {
            const row = rows[item.index];
            if (row === undefined) return null;
            const selected = selection.has(row.id);
            const isActiveRow = item.index === activeIndex;
            return (
              <div
                key={row.id}
                role="row"
                aria-rowindex={item.index + 2}
                aria-selected={selected}
                aria-level={row.level}
                aria-expanded={row.isSummary ? !row.collapsed : undefined}
                className={`absolute left-0 flex border-b border-line select-none ${
                  row.task?.isProject === true ? "text-base font-bold" : ""
                } ${selected ? "bg-accent/25" : "bg-surface"}`}
                style={{
                  height: ROW_HEIGHT,
                  width: totalWidth,
                  // Each project's light colour; a selected row keeps its highlight.
                  ...(row.projectIndex !== null && !selected
                    ? { backgroundColor: projectTint(row.projectIndex, "var(--project-tint)") }
                    : {}),
                  // `item.start` includes the header margin; the rows area
                  // already starts below the header.
                  transform: `translateY(${item.start - HEADER_HEIGHT}px)`,
                }}
                title={
                  row.task === null || readOnly
                    ? undefined
                    : t(row.task.isProject === true ? "grid.rowHintProject" : "grid.rowHint", {
                        modifier: t(IS_MAC ? "grid.editModifierMac" : "grid.editModifierOther"),
                      })
                }
                onDoubleClick={(e) => {
                  if (e.ctrlKey || e.metaKey || e.shiftKey) return;
                  if (row.task === null) return;
                  // The cell's ID ends with its column.
                  const cell = (e.target as Element).closest('[role="gridcell"]');
                  const column = GRID_COLUMN_IDS.find((id) => cell?.id.endsWith(`-${id}`));
                  openTaskInfo(row.id, infoTabOf(column, row.task, snapshot?.tasks));
                }}
                onContextMenu={(e) => onRowContextMenu(e, item.index)}
                onDragOver={(e) => onDragOver(e, item.index)}
                onDrop={onDrop}
              >
                {/* The line between two projects. */}
                {row.task?.isProject === true && (row.projectIndex ?? 0) > 0 && (
                  <div
                    aria-hidden="true"
                    className="pointer-events-none absolute top-0 right-0 left-0 z-[1] h-0.5"
                    style={{ background: "var(--project-separator)" }}
                  />
                )}
                {dropTarget?.visibleIndex === item.index && (
                  <div
                    aria-hidden="true"
                    className={`pointer-events-none absolute right-0 left-0 z-10 h-0.5 bg-accent ${
                      dropTarget.after ? "bottom-0" : "top-0"
                    }`}
                  />
                )}
                {GRID_COLUMN_IDS.map((column, colIndex) => {
                  const isActive = isActiveRow && column === activeColumn;
                  const isEditing =
                    editing !== null && editing.rowId === row.id && editing.column === column;
                  const editable = row.task !== null && !readOnly && isEditable(column, row.task);
                  return (
                    <div
                      key={column}
                      id={`${baseId}-r${item.index}-${column}`}
                      role="gridcell"
                      aria-colindex={colIndex + 1}
                      aria-readonly={!editable}
                      draggable={column === "rowNumber" && !readOnly}
                      onDragStart={column === "rowNumber" ? (e) => onDragStart(e, row) : undefined}
                      title={
                        column === "rowNumber" && !readOnly
                          ? t("grid.dragHandle", { name: row.name })
                          : undefined
                      }
                      className={`relative flex items-center overflow-visible border-r border-line ${
                        isEditing ? "" : "px-1"
                      } ${column === "rowNumber" ? "cursor-grab justify-end text-fg-muted" : ""} ${
                        column === "duration" || column === "complete" ? "justify-end" : ""
                      } ${isActive ? "outline-2 -outline-offset-2 outline-accent" : ""}`}
                      style={{ width: columnWidth[column] }}
                      onMouseDown={(e) => onCellMouseDown(e, item.index, column)}
                      onDoubleClick={(e) => {
                        // Inside the open editor, a double-click selects text.
                        if (isEditing) e.stopPropagation();
                      }}
                      // On "click", not on the press: by then an editor that
                      // was open elsewhere has saved. If it refused its text
                      // it is still open (`editing`) and stays.
                      onClick={(e) => {
                        if (isEditClick(e) && editable && editing === null) startEditing(null);
                      }}
                    >
                      {isEditing ? (
                        <CellEditor
                          key={`${row.id}:${column}`}
                          kind={editorKind(column)}
                          initialText={editing.initialText}
                          selectAll={editing.selectAll}
                          label={t(COLUMN_TITLE_KEYS[column])}
                          error={editing.error}
                          onCommit={commit}
                          onCancel={cancel}
                        />
                      ) : column === "name" ? (
                        <NameCell row={row} onToggle={() => toggleCollapsed(row.id)} />
                      ) : column === "indicators" ? (
                        <IndicatorsCell
                          row={row}
                          rows={snapshot?.tasks ?? []}
                          calendars={calendars}
                          projectCalendarId={snapshot?.calendarId ?? null}
                          dateLocale={dateLocale}
                          numberLocale={numberLocale}
                          minutesPerDay={snapshot?.minutesPerDay ?? 480}
                        />
                      ) : (
                        <>
                          <span className="truncate">{cellText(column, row, format, t)}</span>
                          {column === "complete" && editable && (
                            <button
                              type="button"
                              // The keyboard opens the list with Enter or F2.
                              tabIndex={-1}
                              aria-label={t("grid.choosePercent", { name: row.name })}
                              className="ml-0.5 shrink-0 rounded px-0.5 text-fg-muted hover:bg-app"
                              // The press already made this cell the active one.
                              onClick={(e) => {
                                if (!e.shiftKey && !e.ctrlKey && !e.metaKey) startEditing(null);
                              }}
                              onDoubleClick={(e) => e.stopPropagation()}
                            >
                              <span aria-hidden="true">▾</span>
                            </button>
                          )}
                        </>
                      )}
                    </div>
                  );
                })}
              </div>
            );
          })}
        </div>
      </div>
      {menu !== null && (
        <RowContextMenu
          x={menu.x}
          y={menu.y}
          items={menuItems}
          onClose={() => {
            setMenu(null);
            focusGrid();
          }}
        />
      )}
    </div>
  );
}
