/**
 * features/task-info/riskStore.ts: remembers for which task the "Add risk"
 * dialog is open (or that it is closed).
 *
 * What it is for:
 *   The menu, the toolbar button and the row's context menu call `open`;
 *   `RiskDialog.tsx` watches `taskId` and shows itself.
 *
 * Where it fits:
 *   A tiny Zustand store, like `blockerStore.ts`.
 */

import { create } from "zustand";
import type { Uuid } from "@frontend/ipc/types";

/** The dialog's open state. */
export interface RiskDialogState {
  /** The task that gets the risk, or `null` while the dialog is closed. */
  taskId: Uuid | null;
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
 * The "Add risk" dialog store hook.
 *
 * @example
 * useRiskDialogStore.getState().open(task.id);
 */
export const useRiskDialogStore = create<RiskDialogState>()((set) => ({
  taskId: null,
  open: (id) => set({ taskId: id }),
  close: () => set({ taskId: null }),
}));
