/**
 * features/task-info/blockerStore.ts: remembers for which task the
 * "Add blocker" dialog is open (or that it is closed).
 *
 * What it is for:
 *   The dialog can be opened from the toolbar button, the grid row's
 *   context menu and the shortcut Ctrl/⌘+Shift+B. They all call
 *   {@link BlockerDialogState.open}; the dialog (`BlockerDialog.tsx`)
 *   watches `taskId` and shows itself.
 *
 * Where it fits:
 *   A tiny Zustand store next to the dialog, like `taskInfoStore.ts`.
 *   `shell/AppShell.tsx` reads it so window shortcuts (Ctrl+Z…) do not act
 *   on the project behind the open dialog.
 *
 * Main ideas:
 *   - **Today is read when the dialog opens**, here in the store action,
 *     not while React draws the dialog. React components must be *pure*
 *     (the same input always gives the same picture), and "today" changes
 *     by itself, so it is passed in as data.
 */

import { create } from "zustand";
import type { IsoDate, Uuid } from "@frontend/ipc/types";
import { todayIso } from "./blockers";

/** The dialog's open state. */
export interface BlockerDialogState {
  /** The task that gets the blocker, or `null` while the dialog is closed. */
  taskId: Uuid | null;
  /** Today's date ("YYYY-MM-DD") when the dialog was opened: the default dates. */
  today: IsoDate;
  /**
   * Opens the dialog for a task.
   *
   * @param id - The task's ID.
   */
  open: (id: Uuid) => void;
  /** Closes the dialog. */
  close: () => void;
}

/**
 * The "Add blocker" dialog store hook.
 *
 * @example
 * const open = useBlockerDialogStore((s) => s.open);
 * open(task.id);
 */
export const useBlockerDialogStore = create<BlockerDialogState>()((set) => ({
  taskId: null,
  today: todayIso(),
  open: (id) => set({ taskId: id, today: todayIso() }),
  close: () => set({ taskId: null }),
}));
