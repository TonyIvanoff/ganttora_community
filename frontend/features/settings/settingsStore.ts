/**
 * features/settings/settingsStore.ts: the UI's copy of the app settings.
 *
 * What it is for:
 *   Many parts of the UI depend on settings: the theme, the language, the
 *   date format. They all read them from this small Zustand store. The
 *   *real* settings live in Rust (`backend/src-tauri/src/settings.rs`),
 *   which owns `settings.json`; this store only keeps the copy Rust sent.
 *
 * Where it fits:
 *   - `App.tsx` calls `load()` once at start-up.
 *   - `useApplySettings.ts` applies theme and language whenever they change.
 *   - `SettingsDialog.tsx` edits a draft and calls `save()` / `reset()`.
 *
 * Main ideas:
 *   - **Rust answers, the store takes the answer.** `save()` sends the
 *     whole settings object; Rust cleans it, writes the file and answers
 *     with what it stored. Only then does the store change. If saving
 *     fails, nothing changes and the error is returned to the caller.
 *   - In the browser preview (no Rust side) changes are kept in memory only.
 *   - It is a separate store from `state/store.ts` because settings belong
 *     to the user, not to the open project.
 */

import { create } from "zustand";
import { settingsGet, settingsReset, settingsSet } from "@frontend/ipc/client";
import type { Settings, SettingsNotice } from "@frontend/ipc/types";
import { isIpcError } from "@frontend/lib/errors";
import { BROWSER_DEFAULT_SETTINGS } from "./options";

/**
 * Error code Rust sends when settings are kept in memory but cannot be
 * written (no config folder). Matches `ERR_SETTINGS_NOT_STORED` in
 * `backend/src-tauri/src/ipc/settings.rs`.
 */
const NOT_STORED_CODE = "settingsNotStored";

/** Everything the settings store holds and can do. */
export interface SettingsStoreState {
  /** The current settings (Rust's answer, or the browser defaults). */
  settings: Settings;
  /** A start-up problem with settings.json, until dismissed; else `null`. */
  notice: SettingsNotice | null;
  /** `true` once the first `load()` has finished (successfully or not). */
  loaded: boolean;
  /** Whether the Settings dialog is open. */
  dialogOpen: boolean;
  /**
   * Asks Rust for the settings. Errors are logged; the defaults stay.
   *
   * @returns A promise that resolves when done.
   */
  load: () => Promise<void>;
  /**
   * Stores new settings in Rust and, on success, in this store.
   *
   * @param next - The whole new settings object.
   * @returns A promise of `null` on success, or of the error (an
   *   `IpcError` or other value) if saving failed.
   */
  save: (next: Settings) => Promise<unknown>;
  /**
   * Restores the default settings (in Rust, then here).
   *
   * @returns Like {@link SettingsStoreState.save}.
   */
  reset: () => Promise<unknown>;
  /** Opens the Settings dialog. */
  openDialog: () => void;
  /** Closes the Settings dialog. */
  closeDialog: () => void;
  /** Hides the start-up notice. */
  dismissNotice: () => void;
}

/**
 * The settings store hook.
 *
 * @example
 * const theme = useSettingsStore((s) => s.settings.general.theme);
 * const openSettings = useSettingsStore((s) => s.openDialog);
 *
 * Beginner note: see `state/store.ts` for how Zustand stores and selectors
 * work.
 */
export const useSettingsStore = create<SettingsStoreState>()((set) => ({
  settings: BROWSER_DEFAULT_SETTINGS,
  notice: null,
  loaded: false,
  dialogOpen: false,

  load: async () => {
    try {
      const response = await settingsGet();
      if (response !== null) set({ settings: response.settings, notice: response.notice });
    } catch (error: unknown) {
      // Not fatal: the app runs with the defaults.
      console.error("settings_get failed", error);
    } finally {
      set({ loaded: true });
    }
  },

  save: async (next) => {
    try {
      const stored = await settingsSet(next);
      // `null` = browser preview: keep the change in memory.
      set({ settings: stored ?? next });
      return null;
    } catch (error: unknown) {
      // "Not stored" (no config folder): Rust kept the change for this
      // session, so the UI uses it too, and still reports the problem.
      if (isIpcError(error) && error.code === NOT_STORED_CODE) set({ settings: next });
      return error;
    }
  },

  reset: async () => {
    try {
      const stored = await settingsReset();
      set({ settings: stored ?? BROWSER_DEFAULT_SETTINGS });
      return null;
    } catch (error: unknown) {
      return error;
    }
  },

  openDialog: () => set({ dialogOpen: true }),
  closeDialog: () => set({ dialogOpen: false }),
  dismissNotice: () => set({ notice: null }),
}));
