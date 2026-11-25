/**
 * features/gantt/useRememberZoom.ts: remembers the chart's zoom.
 *
 * What it is for:
 *   Days/Weeks/Months and the zoom percentage are kept in `settings.json`,
 *   so the chart looks the same after a restart.
 *
 * Where it fits:
 *   Used once, by `shell/AppShell.tsx`. The live values stay in
 *   `state/store.ts`; the saved ones are the settings' `view` group
 *   (`backend/src-tauri/src/settings.rs`).
 *
 * Main ideas:
 *   - **Wait before saving.** Turning the wheel changes the zoom many times
 *     a second; only the last value is written, a moment later.
 *   - **Nothing is saved before the settings are loaded**, or the start
 *     values would overwrite the remembered ones.
 */

import { useEffect } from "react";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import { useAppStore } from "@frontend/state/store";

/** How long the zoom must stay unchanged before it is saved, in ms. */
export const SAVE_DELAY_MS = 600;

/**
 * Takes over the saved zoom, and saves later changes.
 *
 * @example
 * useRememberZoom(); // once, in AppShell
 */
export function useRememberZoom(): void {
  const loaded = useSettingsStore((s) => s.loaded);

  useEffect(() => {
    if (!loaded) return;
    let timer: ReturnType<typeof setTimeout> | undefined;

    /** Shows the saved zoom. */
    function apply(): void {
      const { timescale, zoomPercent } = useSettingsStore.getState().settings.view;
      const app = useAppStore.getState();
      app.setZoom(timescale);
      app.setZoomPercent(zoomPercent);
    }

    /** Writes the zoom, if it differs from the saved one. */
    async function save(): Promise<void> {
      timer = undefined;
      const { settings, save: store } = useSettingsStore.getState();
      const { zoom: timescale, zoomPercent } = useAppStore.getState();
      const { view } = settings;
      if (view.timescale === timescale && view.zoomPercent === zoomPercent) return;
      const error = await store({ ...settings, view: { timescale, zoomPercent } });
      // The zoom stays as it is for this session.
      if (error !== null) console.error("Could not save the zoom", error);
    }

    apply();
    const stopApp = useAppStore.subscribe((next, previous) => {
      if (next.zoom === previous.zoom && next.zoomPercent === previous.zoomPercent) return;
      clearTimeout(timer);
      timer = setTimeout(() => void save(), SAVE_DELAY_MS);
    });
    // "Reset to defaults" in Settings resets the zoom too. The answer to a
    // save is a new object with the same values: nothing to do then. A
    // waiting change (timer) is newer than any saved value.
    const stopSettings = useSettingsStore.subscribe((next, previous) => {
      const a = next.settings.view;
      const b = previous.settings.view;
      const same = a.timescale === b.timescale && a.zoomPercent === b.zoomPercent;
      if (same || timer !== undefined) return;
      apply();
    });
    return () => {
      clearTimeout(timer);
      stopApp();
      stopSettings();
    };
  }, [loaded]);
}
