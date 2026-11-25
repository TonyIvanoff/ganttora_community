/**
 * features/toolbars/toolbarStore.ts: the toolbars as they are now, and their
 * saving to `settings.json`.
 *
 * What it is for:
 *   Holds the layout the screen shows. Every change is shown at once and
 *   then written to the settings in the background.
 *
 * Where it fits:
 *   Read by `ToolbarFrame.tsx`, `CustomizeDialog.tsx` and `menuModel.ts`.
 *   The rules are in `layout.ts`; Rust checks and stores the section
 *   `toolbars` (`backend/src-tauri/src/settings.rs`).
 *
 * Main ideas:
 *   - **Show first, save after.** Two quick changes must not wait for the
 *     disk, nor overwrite each other: saves run one after another and
 *     always write the newest layout.
 *   - **Settings can change too** (loaded at start, "Reset to defaults"):
 *     the store listens and takes over the new layout.
 */

import { create } from "zustand";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import type { ToolbarLayout } from "@frontend/ipc/types";
import { cleanToolbars, defaultToolbars } from "./layout";

/** The store's state and actions. */
export interface ToolbarState {
  /** All toolbars (also hidden ones), in display order. */
  toolbars: readonly ToolbarLayout[];
  /**
   * Changes the layout and saves it.
   *
   * @param change - Gets the current list, returns the new one (a function
   *   from `layout.ts`).
   */
  update: (change: (toolbars: readonly ToolbarLayout[]) => ToolbarLayout[]) => void;
  /** Back to Main, Task and View at the top; custom toolbars are deleted. */
  reset: () => void;
}

/** How many saves are waiting or running. */
let pendingSaves = 0;
/** The queue of saves (see `state/store.ts` for the idea). */
let saveQueue: Promise<unknown> = Promise.resolve();

/** Writes the newest layout into the settings. */
async function save(): Promise<void> {
  const { settings, save: store } = useSettingsStore.getState();
  const layout = [...useToolbarStore.getState().toolbars];
  const error = await store({ ...settings, toolbars: { ...settings.toolbars, layout } });
  // The toolbars stay as the user put them for this session.
  if (error !== null) console.error("Could not save the toolbars", error);
}

/**
 * The toolbar store hook.
 *
 * @example
 * const toolbars = useToolbarStore((s) => s.toolbars);
 * useToolbarStore.getState().update((bars) => moveToolbar(bars, "view", { dock: "left" }));
 */
export const useToolbarStore = create<ToolbarState>()((set, get) => {
  /** Shows a layout and queues its save. */
  function apply(toolbars: readonly ToolbarLayout[]): void {
    set({ toolbars });
    pendingSaves += 1;
    saveQueue = saveQueue.then(save).finally(() => {
      pendingSaves -= 1;
    });
  }
  return {
    toolbars: cleanToolbars(useSettingsStore.getState().settings.toolbars.layout),
    update: (change) => apply(change(get().toolbars)),
    reset: () => apply(defaultToolbars()),
  };
});

// Settings loaded or reset elsewhere: take over their toolbars. Not while
// our own saves run, because their answers may be older than the screen.
useSettingsStore.subscribe((next, previous) => {
  if (pendingSaves > 0 || next.settings.toolbars === previous.settings.toolbars) return;
  useToolbarStore.setState({ toolbars: cleanToolbars(next.settings.toolbars.layout) });
});

/**
 * Whether saves are still running (tests wait for it).
 *
 * @returns A promise that resolves when everything is saved.
 */
export function toolbarsSaved(): Promise<unknown> {
  return saveQueue;
}
