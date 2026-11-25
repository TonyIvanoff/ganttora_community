/**
 * features/toolbars/ToolbarFrame.tsx: the main frame with toolbars around
 * (and floating over) the view.
 *
 * What it is for:
 *   Lays out the docked toolbars on the four edges, the view in the middle
 *   and floating toolbars on top; draws the drop zone while one is
 *   dragged; opens the grip menus. Also the "Toolbars" button, which is
 *   always visible, so hidden toolbars can always be brought back.
 *
 * Where it fits:
 *   Rendered by `shell/AppShell.tsx` around the view area.
 */

import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { RowContextMenu, type RowMenuEntry } from "@frontend/features/grid/RowContextMenu";
import type { ToolbarDock, ToolbarLayout } from "@frontend/ipc/types";
import { useDialogStore } from "@frontend/shell/dialogStore";
import {
  clampPosition,
  hasRows,
  MAX_TOOLBAR_ROWS,
  moveToolbar,
  NEW_ROW_DISTANCE,
  patchToolbar,
  rowsAt,
  toolbarsAt,
  type Size,
  type ToolbarPlace,
} from "./layout";
import { Toolbar } from "./Toolbar";
import { useToolbarStore } from "./toolbarStore";
import { toolbarName, useActionState, useAnyT, type RunAction } from "./useActions";
import { useToolbarDrag, type ToolbarDrag } from "./useToolbarDrag";

/** Entries of the grip menu that move a toolbar. */
const DOCK_ENTRIES = [
  { dock: "top", key: "toolbars.dockTop" },
  { dock: "left", key: "toolbars.dockLeft" },
  { dock: "right", key: "toolbars.dockRight" },
  { dock: "bottom", key: "toolbars.dockBottom" },
  { dock: "float", key: "toolbars.float" },
] as const satisfies readonly { dock: ToolbarDock; key: string }[];

/** Corner of a toolbar that is floated from the menu for the first time. */
const FIRST_FLOAT = { x: 120, y: 80 };

/** ID of the "Toolbars" button (gets the focus when a toolbar is hidden). */
const MENU_BUTTON_ID = "toolbars-menu-button";

/** Look of each dock. */
const DOCK_CLASS: Record<Exclude<ToolbarDock, "float">, string> = {
  top: "flex flex-col gap-1 border-b border-line bg-app px-3 py-1",
  bottom: "flex flex-col gap-1 border-t border-line bg-app px-3 py-1",
  left: "flex shrink-0 flex-col gap-1 overflow-y-auto border-r border-line bg-app p-1",
  right: "flex shrink-0 flex-col gap-1 overflow-y-auto border-l border-line bg-app p-1",
};

/** Props of {@link ToolbarFrame}. */
export interface ToolbarFrameProps {
  /** Carries an action out (from `useRunAction`). */
  run: RunAction;
  /** The banners and the view. */
  children: ReactNode;
}

/**
 * The frame.
 *
 * @param props - See {@link ToolbarFrameProps}.
 * @returns The frame with its toolbars and the children in the middle.
 */
export function ToolbarFrame({ run, children }: ToolbarFrameProps) {
  const frameRef = useRef<HTMLDivElement>(null);
  const toolbars = useToolbarStore((s) => s.toolbars);
  const update = useToolbarStore((s) => s.update);
  const state = useActionState();
  const { drag, handlersFor, wasDragged } = useToolbarDrag(frameRef);
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  const [frame, setFrame] = useState<Size | null>(null);
  /** The toolbar whose grip gets the focus after the next layout change. */
  const focusNext = useRef<string | null>(null);

  // Know the frame's size, so floating toolbars stay inside it.
  useEffect(() => {
    const el = frameRef.current;
    if (el === null || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() =>
      setFrame({ width: el.clientWidth, height: el.clientHeight }),
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // A toolbar moved with the menu appears somewhere else as a new element:
  // give its grip the focus, so the keyboard user stays on it.
  useEffect(() => {
    const id = focusNext.current;
    if (id === null) return;
    focusNext.current = null;
    const grip = frameRef.current?.querySelector<HTMLElement>(
      `[data-toolbar-id="${id}"] [data-toolbar-item="grip"]`,
    );
    (grip ?? document.getElementById(MENU_BUTTON_ID))?.focus();
  }, [toolbars]);

  /** Hides a toolbar; the focus goes to the "Toolbars" button. */
  function hide(id: string): void {
    focusNext.current = id;
    update((bars) => patchToolbar(bars, id, { visible: false }));
  }

  /** Draws one toolbar. */
  function draw(bar: ToolbarLayout) {
    return (
      <Toolbar
        key={bar.id}
        bar={bar}
        state={state}
        run={run}
        drag={handlersFor(bar.id)}
        onHide={() => hide(bar.id)}
        onMenu={(anchor) => {
          if (wasDragged()) return; // the click at the end of a drag
          const r = anchor.getBoundingClientRect();
          setMenu({ id: bar.id, x: r.left, y: r.bottom });
        }}
      />
    );
  }

  /** Draws one edge, if any toolbar is docked there. */
  function dock(edge: Exclude<ToolbarDock, "float">) {
    const bars = toolbarsAt(toolbars, edge);
    if (bars.length === 0) return null;
    return (
      <div data-dock={edge} className={DOCK_CLASS[edge]}>
        {hasRows(edge)
          ? rowsAt(toolbars, edge).map((row, i) => (
              <div key={i} data-toolbar-row={i} className="flex flex-wrap gap-1">
                {row.map(draw)}
              </div>
            ))
          : bars.map(draw)}
      </div>
    );
  }

  // A side without a toolbar gets a margin, as wide as the title row's, so
  // the view does not touch the window's edge.
  const leftGap = toolbarsAt(toolbars, "left").length === 0;
  const rightGap = toolbarsAt(toolbars, "right").length === 0;

  const menuBar = menu === null ? undefined : toolbars.find((bar) => bar.id === menu.id);

  /** Moves a toolbar with the grip menu and keeps the focus on it. */
  function moveTo(bar: ToolbarLayout, to: ToolbarPlace): void {
    focusNext.current = bar.id;
    const position = bar.x === 0 && bar.y === 0 ? { position: FIRST_FLOAT } : {};
    update((bars) => moveToolbar(bars, bar.id, { ...to, ...position }));
  }

  return (
    <div ref={frameRef} className="relative flex min-h-0 flex-1 flex-col" data-toolbar-frame>
      {dock("top")}
      <div className="flex min-h-0 flex-1">
        {dock("left")}
        <div
          data-view-area=""
          className={`flex min-h-0 min-w-0 flex-1 flex-col ${
            leftGap ? "ml-3 border-l border-line" : ""
          } ${rightGap ? "mr-3 border-r border-line" : ""}`}
        >
          {children}
        </div>
        {dock("right")}
      </div>
      {dock("bottom")}
      {toolbarsAt(toolbars, "float").map((bar) => (
        <Floating key={bar.id} x={bar.x} y={bar.y} frame={frame}>
          {draw(bar)}
        </Floating>
      ))}
      {drag !== null && <DropZone drag={drag} />}
      {menu !== null && menuBar !== undefined && (
        <GripMenu
          x={menu.x}
          y={menu.y}
          bar={menuBar}
          rows={rowsAt(toolbars, menuBar.dock)}
          onMove={(to) => moveTo(menuBar, to)}
          onHide={() => hide(menuBar.id)}
          onClose={() => {
            setMenu(null);
            frameRef.current
              ?.querySelector<HTMLElement>(
                `[data-toolbar-id="${menuBar.id}"] [data-toolbar-item="grip"]`,
              )
              ?.focus();
          }}
        />
      )}
    </div>
  );
}

/** Props of {@link GripMenu}. */
interface GripMenuProps {
  /** Where to open (window coordinates). */
  x: number;
  /** See `x`. */
  y: number;
  /** The toolbar. */
  bar: ToolbarLayout;
  /** The rows of its edge. */
  rows: readonly ToolbarLayout[][];
  /** Docks or floats it. */
  onMove: (to: ToolbarPlace) => void;
  /** Hides it. */
  onHide: () => void;
  /** Closes the menu. */
  onClose: () => void;
}

/**
 * The menu of a toolbar's grip: the keyboard's way to move a toolbar.
 *
 * @param props - See {@link GripMenuProps}.
 */
function GripMenu({ x, y, bar, rows, onMove, onHide, onClose }: GripMenuProps) {
  const t = useAnyT();
  const own = rows.findIndex((row) => row.includes(bar));
  const alone = rows[own]?.length === 1;
  /** Rows to choose from, at the top and bottom (none for a single full row). */
  const rowEntries: RowMenuEntry[] = !hasRows(bar.dock)
    ? []
    : rows.map((_, i) => ({
        key: `row-${i}`,
        label: t("toolbars.row", { number: i + 1 }),
        checked: i === own,
        onSelect: () => {
          if (i === own) return;
          // Its own row disappears when it was alone there.
          onMove({ dock: bar.dock, row: alone && i > own ? i - 1 : i });
        },
      }));
  if (hasRows(bar.dock) && rows.length < MAX_TOOLBAR_ROWS && !alone) {
    const row = bar.dock === "top" ? rows.length : 0;
    rowEntries.push({
      key: "new-row",
      label: t("toolbars.newRow"),
      onSelect: () => onMove({ dock: bar.dock, row, newRow: true }),
    });
  }
  const entries: RowMenuEntry[] = [
    ...DOCK_ENTRIES.map((entry) => ({
      key: entry.dock,
      label: t(entry.key),
      checked: bar.dock === entry.dock,
      onSelect: () => onMove({ dock: entry.dock }),
    })),
    ...(rowEntries.length > 0
      ? [{ key: "row-line", separator: true } as const, ...rowEntries]
      : []),
    { key: "line", separator: true },
    { key: "hide", label: t("toolbars.hide"), onSelect: onHide },
    {
      key: "customize",
      label: t("toolbars.customize"),
      onSelect: () => useDialogStore.getState().show("customize"),
    },
  ];
  return (
    <RowContextMenu
      x={x}
      y={y}
      label={t("toolbars.gripMenuLabel", { name: toolbarName(bar, t) })}
      items={entries}
      onClose={onClose}
    />
  );
}

/** Props of {@link Floating}. */
interface FloatingProps {
  /** Wanted left edge. */
  x: number;
  /** Wanted top edge. */
  y: number;
  /** The frame's size, or `null` before it is measured. */
  frame: Size | null;
  /** The toolbar. */
  children: ReactNode;
}

/**
 * Places a floating toolbar, always inside the frame. The wanted place is
 * kept, so after making the window larger again it returns there.
 *
 * @param props - See {@link FloatingProps}.
 */
function Floating({ x, y, frame, children }: FloatingProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [corner, setCorner] = useState({ x, y });
  // Measured before painting, so it never flashes at the wrong place.
  useLayoutEffect(() => {
    const el = ref.current;
    if (el === null) return;
    const size = { width: el.offsetWidth, height: el.offsetHeight };
    setCorner(frame === null ? { x, y } : clampPosition({ x, y }, size, frame));
  }, [x, y, frame]);
  return (
    <div ref={ref} className="absolute z-30 max-w-full" style={{ left: corner.x, top: corner.y }}>
      {children}
    </div>
  );
}

/** Props of {@link DropZone}. */
interface DropZoneProps {
  /** The drag in progress. */
  drag: ToolbarDrag;
}

/**
 * The highlight that shows where a drop would put the toolbar.
 *
 * @param props - See {@link DropZoneProps}.
 */
function DropZone({ drag }: DropZoneProps) {
  const { zone, insets, ghost, marker, newRow } = drag;
  // A strip as deep as the toolbars already there, at least 40 px.
  const depth = zone === "float" ? 0 : Math.max(40, insets[zone] ?? 0);
  const pastRows = newRow && zone !== "float" ? (insets[zone] ?? 0) : null;
  const style =
    zone === "float"
      ? { left: ghost.x, top: ghost.y, width: ghost.width, height: ghost.height }
      : pastRows !== null
        ? { left: 0, right: 0, height: NEW_ROW_DISTANCE, [zone]: pastRows }
        : zone === "top" || zone === "bottom"
          ? { left: 0, right: 0, height: depth, [zone]: 0 }
          : { top: 0, bottom: 0, width: depth, [zone]: 0 };
  return (
    <>
      <div
        data-drop-zone={zone}
        data-new-row={newRow || undefined}
        className="pointer-events-none absolute z-40 rounded border-2 border-dashed border-accent bg-accent/20"
        style={style}
      />
      {marker !== null && (
        <div
          className="pointer-events-none absolute z-40 bg-accent"
          style={{ left: marker.x, top: marker.y, width: marker.width, height: marker.height }}
        />
      )}
    </>
  );
}

/**
 * The always-visible "Toolbars" button: show, hide, customize, reset.
 *
 * @returns The button, with its menu when open.
 */
export function ToolbarsMenuButton() {
  const t = useAnyT();
  const toolbars = useToolbarStore((s) => s.toolbars);
  const update = useToolbarStore((s) => s.update);
  const reset = useToolbarStore((s) => s.reset);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [at, setAt] = useState<{ x: number; y: number } | null>(null);
  const entries: RowMenuEntry[] = [
    ...toolbars.map((bar) => ({
      key: bar.id,
      label: toolbarName(bar, t),
      checked: bar.visible,
      onSelect: () => update((bars) => patchToolbar(bars, bar.id, { visible: !bar.visible })),
    })),
    { key: "line", separator: true },
    {
      key: "customize",
      label: t("toolbars.customize"),
      onSelect: () => useDialogStore.getState().show("customize"),
    },
    { key: "reset", label: t("toolbars.reset"), onSelect: reset },
  ];
  return (
    <>
      <button
        ref={buttonRef}
        id={MENU_BUTTON_ID}
        type="button"
        aria-haspopup="menu"
        aria-expanded={at !== null}
        className="rounded border border-line px-2 py-0.5 text-sm whitespace-nowrap hover:bg-app"
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setAt({ x: r.left, y: r.bottom });
        }}
      >
        {t("toolbars.menuButton")} <span aria-hidden="true">▾</span>
      </button>
      {at !== null && (
        <RowContextMenu
          x={at.x}
          y={at.y}
          label={t("toolbars.menuLabel")}
          items={entries}
          onClose={() => {
            setAt(null);
            buttonRef.current?.focus();
          }}
        />
      )}
    </>
  );
}
