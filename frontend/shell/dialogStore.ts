/**
 * shell/dialogStore.ts: which of the small app dialogs is open, and whether
 * *any* dialog is open.
 *
 * What it is for:
 *   - opens and closes About, Customize toolbars and the legend panel (not modal);
 *   - {@link isModalOpen} answers "is a dialog covering the project?".
 *     While one is, shortcuts and menu entries must not act on the project
 *     behind it.
 *
 * Where it fits:
 *   Used by `features/toolbars/useActions.ts` and `useNativeMenu.ts`.
 *   Settings, Calendars, Task Information and Add blocker have their own
 *   stores; they are only looked at here.
 */

import { create } from "zustand";
import { useCalendarDialogStore } from "@frontend/features/calendars/calendarStore";
import { isNativeDialogOpen } from "@frontend/features/files/dialogs";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import { useBlockerDialogStore } from "@frontend/features/task-info/blockerStore";
import { useRiskDialogStore } from "@frontend/features/task-info/riskStore";
import { useTaskInfoStore } from "@frontend/features/task-info/taskInfoStore";

/** The dialogs this store opens. */
export type AppDialog = "about" | "customize";

/** The store's state and actions. */
export interface DialogState {
  /** The open dialog, or `null`. */
  open: AppDialog | null;
  /** Opens a dialog. */
  show: (dialog: AppDialog) => void;
  /** Closes it. */
  close: () => void;
  /** The colour legend panel is shown. It blocks nothing, so it is not a dialog. */
  legendOpen: boolean;
  /** Shows or hides the colour legend panel. */
  toggleLegend: () => void;
}

/**
 * The dialog store hook.
 *
 * @example
 * useDialogStore.getState().show("about");
 */
export const useDialogStore = create<DialogState>()((set) => ({
  open: null,
  show: (dialog) => set({ open: dialog }),
  close: () => set({ open: null }),
  legendOpen: false,
  toggleLegend: () => set((s) => ({ legendOpen: !s.legendOpen })),
}));

/**
 * Whether a dialog is open right now (ours, or the system's file window).
 *
 * @returns `true` while the project must be left alone.
 */
export function isModalOpen(): boolean {
  return (
    useDialogStore.getState().open !== null ||
    useSettingsStore.getState().dialogOpen ||
    useCalendarDialogStore.getState().dialogOpen ||
    useTaskInfoStore.getState().taskId !== null ||
    useBlockerDialogStore.getState().taskId !== null ||
    useRiskDialogStore.getState().taskId !== null ||
    isNativeDialogOpen()
  );
}

/**
 * Like {@link isModalOpen} for our own dialogs, as a hook: the component
 * redraws when the answer changes.
 *
 * Beginner note: hooks must be called every time, in the same order, so
 * each one is stored first and combined afterwards.
 */
export function useModalOpen(): boolean {
  const app = useDialogStore((s) => s.open !== null);
  const settings = useSettingsStore((s) => s.dialogOpen);
  const calendars = useCalendarDialogStore((s) => s.dialogOpen);
  const taskInfo = useTaskInfoStore((s) => s.taskId !== null);
  const blocker = useBlockerDialogStore((s) => s.taskId !== null);
  const risk = useRiskDialogStore((s) => s.taskId !== null);
  return app || settings || calendars || taskInfo || blocker || risk;
}
