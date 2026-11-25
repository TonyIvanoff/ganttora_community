/**
 * features/files/useCloseGuard.ts: asks "Save / Don't save / Cancel"
 * before the window closes or the app quits with unsaved changes.
 *
 * What it is for:
 *   Clicking the window's close button (or Alt+F4, or Cmd+W), or quitting
 *   the app (Cmd+Q), would normally end at once, and unsaved changes would
 *   be lost. This hook holds the close or quit back while the project has
 *   unsaved changes, asks the user, and then goes on only if the user chose
 *   Don't save, or chose Save and the save worked.
 *
 * Where it fits:
 *   Called once by `shell/AppShell.tsx`, with `resolveUnsavedChanges` from
 *   `useFileActions.ts` (the same question New and Open ask).
 *   - **Close:** Tauri's close-requested event. Closing the window from the
 *     UI needs `core:window:allow-destroy` in
 *     `backend/src-tauri/capabilities/default.json`.
 *   - **Quit:** Rust holds the exit back (`ipc/app.rs`,
 *     `on_exit_requested`) and sends the event `app://quit-requested`; we
 *     ask, then call `appQuit()`.
 *   Before the window or the app goes away we tell Rust to close the
 *   project *cleanly* (`appCloseProject` / `appQuit`), which deletes its
 *   working copy and its crash-recovery copy: the user has decided.
 *
 * Main ideas:
 *   - A listener may call `event.preventDefault()` to keep the window open;
 *     it then closes it later itself with `window.destroy()`.
 *   - Without unsaved changes nothing is prevented, so the window closes as
 *     usual (Rust then closes the project cleanly when the app ends).
 *   - A second request while the question is showing is ignored (it would
 *     open a second question).
 */

import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { appCloseProject, appQuit, hasDesktopCore } from "@frontend/ipc/client";
import { useAppStore } from "@frontend/state/store";

/**
 * Name of the event Rust sends when the user quits with unsaved changes.
 * Must equal `QUIT_REQUESTED_EVENT` in `backend/src-tauri/src/ipc/app.rs`.
 */
export const QUIT_REQUESTED_EVENT = "app://quit-requested";

/**
 * Guards the window's close button and the app's Quit against losing
 * unsaved changes.
 *
 * @param resolveUnsavedChanges - Asks the question (and saves if asked);
 *   resolves `true` if the window may close. From {@link useFileActions}.
 *
 * @example
 * const { resolveUnsavedChanges } = useFileActions();
 * useCloseGuard(resolveUnsavedChanges);
 *
 * Beginner note: `useRef` gives a box whose content survives re-renders
 * without causing one. We put the newest `resolveUnsavedChanges` in it on
 * every render, so the listeners (registered only once) always call the
 * current version, which knows the current file name and language.
 */
export function useCloseGuard(resolveUnsavedChanges: () => Promise<boolean>): void {
  const resolveRef = useRef(resolveUnsavedChanges);
  // No dependency list: runs after every render, keeping the box current.
  useEffect(() => {
    resolveRef.current = resolveUnsavedChanges;
  });

  useEffect(() => {
    // In a plain browser there is no desktop window to guard.
    if (!hasDesktopCore()) return;
    const appWindow = getCurrentWindow();
    /** `true` while the question is on screen (shared by close and quit). */
    let asking = false;
    /** Functions that remove the listeners; filled in once registered. */
    const unlisteners: (() => void)[] = [];
    /** Set if the component went away before registration finished. */
    let disposed = false;

    /**
     * Asks the question if needed, then runs `proceed`.
     *
     * @param proceed - What to do when the user did not cancel.
     */
    async function guard(proceed: () => Promise<void>): Promise<void> {
      if (asking) return;
      asking = true;
      try {
        if (await resolveRef.current()) await proceed();
      } catch (error: unknown) {
        // A failed dialog or IPC call: show it, keep the window open.
        useAppStore.getState().showIpcError(error);
      } finally {
        asking = false;
      }
    }

    /** Keeps the remover, or calls it at once if we were already cleaned up. */
    function keep(unlisten: () => void): void {
      if (disposed) unlisten();
      else unlisteners.push(unlisten);
    }

    void appWindow
      .onCloseRequested(async (event) => {
        const file = useAppStore.getState().file;
        // Nothing to lose: let Tauri close the window normally.
        if (file === null || !file.dirty) return;
        event.preventDefault();
        await guard(async () => {
          await appCloseProject();
          await appWindow.destroy();
        });
      })
      .then(keep);

    // Rust only sends this event while a project has unsaved changes.
    void listen(QUIT_REQUESTED_EVENT, () => {
      void guard(appQuit);
    }).then(keep);

    return () => {
      disposed = true;
      for (const unlisten of unlisteners) unlisten();
    };
  }, []);
}
