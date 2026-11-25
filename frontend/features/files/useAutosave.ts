/**
 * features/files/useAutosave.ts: the autosave timer.
 *
 * What it is for:
 *   If Ganttora crashes or the computer loses power, unsaved work should
 *   not be lost. Every few minutes (Settings → Files → "Autosave every N
 *   minutes"; 0 = off) this timer asks Rust to write a *recovery copy* of
 *   the project, but only while there are unsaved changes and the file is
 *   not read-only. The user's own file is never touched by autosave; that
 *   stays the user's decision (Save).
 *
 * Where it fits:
 *   Called once by `shell/AppShell.tsx`. Rust does the work
 *   (`file_autosave` in `backend/src-tauri/src/ipc/file.rs`); the copies
 *   are offered at the next start by `recovery.ts`.
 *
 * Main ideas:
 *   - `setInterval(fn, ms)` runs `fn` every `ms` milliseconds until
 *     `clearInterval` stops it. The effect restarts the timer whenever the
 *     interval setting changes.
 *   - The check "dirty and not read-only" is done when the timer fires
 *     (reading the store *then*), so the timer does not restart on every
 *     edit. Rust checks again anyway.
 *   - A failed autosave is logged, not shown: it must never interrupt the
 *     user's work. The next tick simply tries again.
 */

import { useEffect } from "react";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import { fileAutosave, hasDesktopCore } from "@frontend/ipc/client";
import { useAppStore } from "@frontend/state/store";

/** Milliseconds in one minute. */
const MS_PER_MINUTE = 60_000;

/**
 * Starts the autosave timer for the window's project.
 *
 * @example
 * useAutosave(); // in AppShell
 */
export function useAutosave(): void {
  const minutes = useSettingsStore((s) => s.settings.files.autosaveMinutes);

  useEffect(() => {
    // Off in a plain browser (no Rust) and when the setting is 0.
    if (!hasDesktopCore() || minutes <= 0) return;
    const timer = setInterval(() => {
      const { file } = useAppStore.getState();
      if (file === null || !file.dirty || file.readOnly) return;
      fileAutosave().catch((error: unknown) => console.warn("autosave failed", error));
    }, minutes * MS_PER_MINUTE);
    // The clean-up function stops the old timer before a new one starts.
    return () => clearInterval(timer);
  }, [minutes]);
}
