/**
 * features/explain/explainStore.ts: whether the "Why this date?" panel is open.
 *
 * What it is for:
 *   A tiny Zustand store with one flag, so the toolbar button
 *   (`shell/AppShell.tsx`) and the panel (`ExplainPanel.tsx`) agree on it.
 *   Kept separate from the main store (`state/store.ts`) because it is pure
 *   view state of this one feature.
 */

import { create } from "zustand";

/** The panel's state and actions. */
export interface ExplainState {
  /** `true` while the panel is shown beside the Gantt chart. */
  open: boolean;
  /** Shows the panel if hidden, hides it if shown. */
  toggle: () => void;
  /**
   * Shows or hides the panel.
   *
   * @param open - `true` to show.
   */
  setOpen: (open: boolean) => void;
}

/**
 * The hook for the panel state.
 *
 * @example
 * const open = useExplainStore((s) => s.open);
 */
export const useExplainStore = create<ExplainState>()((set) => ({
  open: false,
  toggle: () => set((s) => ({ open: !s.open })),
  setOpen: (open) => set({ open }),
}));
