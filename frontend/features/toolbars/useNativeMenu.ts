/**
 * features/toolbars/useNativeMenu.ts: keeps the native menu bar in step
 * with the app.
 *
 * What it is for:
 *   Sends the menu description (`menuModel.ts`) to Rust when it changes,
 *   and carries out the entry the user chose.
 *
 * Where it fits:
 *   Called once by `shell/AppShell.tsx`. Rust side: `menu.rs`
 *   (`menu_set`, `menu_update`, event `app://menu`). Does nothing in the
 *   browser preview.
 *
 * Main ideas:
 *   - The whole menu is sent only when its structure or texts change
 *     (language, toolbars, text field focus on macOS). Otherwise only
 *     "enabled" and "ticked" are sent.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { hasDesktopCore, menuSet, menuUpdate } from "@frontend/ipc/client";
import type { MenuStateUpdate } from "@frontend/ipc/types";
import { isModalOpen, useModalOpen } from "@frontend/shell/dialogStore";
import { isActionId } from "./actions";
import { patchToolbar } from "./layout";
import { buildMenu, menuStates, menuStructure, TOOLBAR_PREFIX } from "./menuModel";
import { useToolbarStore } from "./toolbarStore";
import { IS_MAC, isTextMode, useActionState, useAnyT, type RunAction } from "./useActions";

/** Event from Rust: the chosen entry's ID. Must equal `MENU_EVENT` in `menu.rs`. */
export const MENU_EVENT = "app://menu";

/** Logs a failed menu call: the app works without the menu. */
const logError = (error: unknown) => console.error("menu", error);

/**
 * Follows whether text is being edited (see `isTextMode`).
 *
 * @returns `true` while a text field has the focus or text is selected.
 */
function useTextMode(): boolean {
  const [textMode, setTextMode] = useState(false);
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    // When the focus moves, "out" comes before "in": wait until it settled.
    function onChange(): void {
      clearTimeout(timer);
      timer = setTimeout(() => setTextMode(isTextMode()), 0);
    }
    const events = ["focusin", "focusout", "selectionchange"] as const;
    for (const name of events) document.addEventListener(name, onChange);
    return () => {
      clearTimeout(timer);
      for (const name of events) document.removeEventListener(name, onChange);
    };
  }, []);
  return textMode;
}

/**
 * Connects the native menu.
 *
 * @param run - Carries an action out (from `useRunAction`).
 */
export function useNativeMenu(run: RunAction): void {
  const t = useAnyT();
  const state = useActionState();
  const toolbars = useToolbarStore((s) => s.toolbars);
  const modal = useModalOpen();
  const textMode = useTextMode();
  const model = useMemo(
    () => buildMenu({ mac: IS_MAC, t, state, toolbars, textMode, modal }),
    [t, state, toolbars, textMode, modal],
  );

  /** What Rust has now. */
  const sent = useRef<{ structure: string; states: readonly MenuStateUpdate[] } | null>(null);
  useEffect(() => {
    if (!hasDesktopCore()) return;
    const structure = menuStructure(model);
    const states = menuStates(model);
    if (sent.current?.structure !== structure) {
      menuSet(model).catch(logError);
    } else {
      const before = new Map(sent.current.states.map((s) => [s.id, s]));
      const changed = states.filter((s) => {
        const old = before.get(s.id);
        return old?.enabled !== s.enabled || old.checked !== s.checked;
      });
      if (changed.length > 0) menuUpdate(changed).catch(logError);
    }
    sent.current = { structure, states };
  }, [model]);

  // The newest `run`, for the listener that is registered only once.
  const runRef = useRef(run);
  useEffect(() => {
    runRef.current = run;
  });

  useEffect(() => {
    if (!hasDesktopCore()) return;
    let stop: (() => void) | undefined;
    let disposed = false;
    void listen<string>(MENU_EVENT, ({ payload: id }) => {
      if (isActionId(id)) runRef.current(id, "menu");
      else if (id.startsWith(TOOLBAR_PREFIX) && !isModalOpen()) {
        const bar = id.slice(TOOLBAR_PREFIX.length);
        useToolbarStore.getState().update((bars) => {
          const now = bars.find((b) => b.id === bar);
          return patchToolbar(bars, bar, { visible: !(now?.visible ?? true) });
        });
      }
      // The system ticks an entry by itself when it is chosen. If the app
      // did not follow (e.g. "Days" was already on), put the tick right.
      setTimeout(() => {
        const ticks = sent.current?.states.filter((s) => s.checked !== null) ?? [];
        if (ticks.length > 0) menuUpdate(ticks).catch(logError);
      }, 50);
    }).then((unlisten) => {
      if (disposed) unlisten();
      else stop = unlisten;
    });
    return () => {
      disposed = true;
      stop?.();
    };
  }, []);
}
