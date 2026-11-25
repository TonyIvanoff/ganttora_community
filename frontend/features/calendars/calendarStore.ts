/**
 * features/calendars/calendarStore.ts: whether the calendar editor is open.
 *
 * What it is for:
 *   A tiny Zustand store so the toolbar button (`shell/AppShell.tsx`) can
 *   open the dialog and the dialog (`CalendarDialog.tsx`) can close itself.
 *   The calendars themselves live in the main store (`state/store.ts`,
 *   `calendars`), which always holds what Rust last sent.
 */

import { create } from "zustand";

/** The dialog's state and actions. */
export interface CalendarDialogState {
  /** `true` while the calendar editor is shown. */
  dialogOpen: boolean;
  /** Opens the editor. */
  openDialog: () => void;
  /** Closes the editor. */
  closeDialog: () => void;
}

/**
 * The hook for the calendar editor's open state.
 *
 * @example
 * const openCalendars = useCalendarDialogStore((s) => s.openDialog);
 */
export const useCalendarDialogStore = create<CalendarDialogState>()((set) => ({
  dialogOpen: false,
  openDialog: () => set({ dialogOpen: true }),
  closeDialog: () => set({ dialogOpen: false }),
}));
