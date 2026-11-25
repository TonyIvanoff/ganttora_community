/**
 * features/grid/RowContextMenu.tsx: the small menu that opens on a task
 * row with a right-click, Shift+F10 or the keyboard's Menu key.
 *
 * What it is for:
 *   Shows a list of entries, with dividing lines between groups, at the
 *   mouse. The task grid fills it with actions of the selected row; the
 *   toolbars use it for their grip menus.
 *
 * Where it fits:
 *   Rendered by `TaskGrid.tsx` and `features/toolbars/ToolbarFrame.tsx`. It
 *   knows nothing about tasks: the caller hands in the entries.
 *
 * Main ideas:
 *   - **WAI-ARIA menu pattern:** the list has `role="menu"`, each entry
 *     `role="menuitem"`. The first entry gets the focus; ↑ / ↓ (and Home /
 *     End) move between entries; Enter or Space chooses; Escape or Tab
 *     closes and gives the focus back to the grid.
 *   - **`position: fixed`** at the mouse (or at the active row for the
 *     keyboard), kept inside the window so it never opens off-screen.
 *   - A press anywhere outside the menu closes it.
 */

import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";

/** A dividing line between groups of entries. */
export interface RowMenuSeparator {
  /** A stable key for React. */
  key: string;
  /** Marks the entry as a line. */
  separator: true;
}

/** One line of the menu: an entry or a divider. */
export type RowMenuEntry = RowMenuItem | RowMenuSeparator;

/** One entry of the menu. */
export interface RowMenuItem {
  /** Never set here: tells an entry from a {@link RowMenuSeparator}. */
  separator?: undefined;
  /** A stable key for React. */
  key: string;
  /** The entry's text (already translated). */
  label: string;
  /** The shortcut shown on the right (already translated), if any. */
  shortcut?: string | undefined;
  /** Draws a tick (`true`) or room for one (`false`); for on/off entries. */
  checked?: boolean;
  /** Greyed out and not choosable. */
  disabled?: boolean;
  /** What choosing the entry does. */
  onSelect: () => void;
}

/** Props of {@link RowContextMenu}. */
export interface RowContextMenuProps {
  /** Where to open: window coordinates (CSS px) of the top-left corner. */
  x: number;
  /** Where to open (see `x`). */
  y: number;
  /** The entries and dividers. */
  items: readonly RowMenuEntry[];
  /** Name for screen readers (default: "Task actions"). */
  label?: string;
  /** Called when the menu should close (after a choice, Escape, a click outside). */
  onClose: () => void;
}

/** Space kept between the menu and the window edge, in CSS pixels. */
const EDGE_GAP = 4;

/**
 * The row context menu.
 *
 * @param props - See {@link RowContextMenuProps}.
 * @returns The menu, positioned over everything else.
 *
 * @example
 * <RowContextMenu x={120} y={300} items={[{ key: "info", label: "Task information…", onSelect }]}
 *   onClose={() => setMenu(null)} />
 */
export function RowContextMenu({ x, y, items, label, onClose }: RowContextMenuProps) {
  const { t } = useTranslation();
  const menuRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  // Where the menu really goes, after keeping it inside the window.
  const [position, setPosition] = useState({ left: x, top: y });

  // Measure the menu and move it back inside the window if needed.
  // `useLayoutEffect` runs before the browser paints, so the menu never
  // flashes at the wrong place.
  useLayoutEffect(() => {
    const menu = menuRef.current;
    if (menu === null) return;
    const rect = menu.getBoundingClientRect();
    setPosition({
      left: Math.max(EDGE_GAP, Math.min(x, window.innerWidth - rect.width - EDGE_GAP)),
      top: Math.max(EDGE_GAP, Math.min(y, window.innerHeight - rect.height - EDGE_GAP)),
    });
  }, [x, y]);

  // Focus the first entry that can be chosen, so the keyboard works at once.
  // Only when the menu opens: the parent hands in a new `items` list on
  // every render, which must not pull the focus back to the first entry.
  useEffect(() => {
    const first = items.findIndex((item) => !item.separator && item.disabled !== true);
    // Nothing usable: the menu itself takes the focus, so Escape still works.
    (itemRefs.current[first] ?? menuRef.current)?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // A press outside the menu closes it. Registered in the *capture* phase
  // (`true`), so it runs before the press reaches anything else.
  useEffect(() => {
    function onPointerDown(event: PointerEvent): void {
      if (menuRef.current?.contains(event.target as Node) === true) return;
      onClose();
    }
    window.addEventListener("pointerdown", onPointerDown, true);
    return () => window.removeEventListener("pointerdown", onPointerDown, true);
  }, [onClose]);

  /** ↑ / ↓ / Home / End move the focus; Escape and Tab close. */
  function onKeyDown(event: KeyboardEvent<HTMLDivElement>): void {
    // Keep the grid's own key handling (it is the parent) out of the menu.
    event.stopPropagation();
    if (event.key === "Escape" || event.key === "Tab") {
      event.preventDefault();
      onClose();
      return;
    }
    const enabled = items.flatMap((item, i) =>
      item.separator || item.disabled === true ? [] : [i],
    );
    if (enabled.length === 0) return;
    const current = itemRefs.current.findIndex((el) => el === document.activeElement);
    const at = enabled.indexOf(current);
    let next: number | undefined;
    if (event.key === "ArrowDown") next = enabled[(at + 1) % enabled.length];
    else if (event.key === "ArrowUp") next = enabled[(at - 1 + enabled.length) % enabled.length];
    else if (event.key === "Home") next = enabled[0];
    else if (event.key === "End") next = enabled[enabled.length - 1];
    else return;
    event.preventDefault();
    if (next !== undefined) itemRefs.current[next]?.focus();
  }

  return (
    <div
      ref={menuRef}
      role="menu"
      tabIndex={-1}
      aria-label={label ?? t("grid.menu.label")}
      className="fixed z-50 flex max-h-[calc(100vh-8px)] overflow-auto outline-none min-w-48 flex-col rounded border border-line bg-surface py-1 text-sm text-fg shadow-lg"
      style={{ left: position.left, top: position.top }}
      onKeyDown={onKeyDown}
      // The browser's own context menu must not open on top of ours.
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((item, i) =>
        item.separator ? (
          <div key={item.key} role="separator" className="my-1 border-t border-line" />
        ) : (
          <button
            key={item.key}
            ref={(el) => {
              itemRefs.current[i] = el;
            }}
            type="button"
            role={item.checked === undefined ? "menuitem" : "menuitemcheckbox"}
            aria-checked={item.checked}
            tabIndex={-1}
            disabled={item.disabled}
            className="flex shrink-0 items-center gap-4 px-3 py-1 text-left whitespace-nowrap outline-none hover:bg-app focus:bg-accent focus:text-accent-fg disabled:opacity-50"
            onClick={() => {
              onClose();
              item.onSelect();
            }}
          >
            <span className="flex gap-2">
              {item.checked !== undefined && (
                <span aria-hidden="true" className="w-3">
                  {item.checked ? "✓" : ""}
                </span>
              )}
              {item.label}
            </span>
            {item.shortcut !== undefined && (
              <span className="ml-auto text-xs opacity-70">{item.shortcut}</span>
            )}
          </button>
        ),
      )}
    </div>
  );
}
