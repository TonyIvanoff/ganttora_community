/**
 * App.tsx: the top-level React component.
 *
 * What it is for:
 *   Runs the start-up work once (load the user's settings and apply theme
 *   and language, ask Rust for app info, offer crash-recovery copies, and
 *   otherwise open the garden-shed demo project in Rust) and then shows the
 *   application frame (`shell/AppShell.tsx`). In a plain browser preview
 *   there is no Rust side, so it shows the 10,000-task mock schedule instead.
 *
 * Where it fits:
 *   `main.tsx` renders `<App />` into the page. Everything visible is inside it.
 */

import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { offerRecovery } from "@frontend/features/files/recovery";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import { useApplySettings } from "@frontend/features/settings/useApplySettings";
import i18next from "@frontend/i18n";
import { dateStyleOf } from "@frontend/i18n/format";
import { getAppInfo, hasDesktopCore } from "@frontend/ipc/client";
import { AppShell } from "@frontend/shell/AppShell";
import { useAppStore } from "@frontend/state/store";

/**
 * `true` once the start-up sequence (settings → recovery → sample) has
 * begun. A module-level variable, not React state: it must survive
 * React's double run of effects in development, and the sequence must
 * never run twice in one app session.
 */
let startUpStarted = false;

/**
 * The root component of the UI.
 *
 * @returns The application frame.
 *
 * Beginner note: a *component* is a function that returns what should be
 * on screen (JSX: HTML-like tags inside TypeScript). React calls it again
 * whenever its data changes and updates only the parts of the page that
 * differ.
 */
export function App() {
  const { t } = useTranslation();
  const setAppInfo = useAppStore((s) => s.setAppInfo);
  const setShowMock = useAppStore((s) => s.setShowMock);
  const openDemoProject = useAppStore((s) => s.openDemoProject);
  const loadSettings = useSettingsStore((s) => s.load);

  // Theme and language follow the settings, from start-up on.
  useApplySettings();

  // The window title (file name, "•" for unsaved changes) is set by
  // AppShell (shell/AppShell.tsx), using the hook in
  // features/files/useDocumentTitle.ts.

  // Start-up, in this order:
  //   1. ask Rust for the saved settings (language, theme…), so the
  //      questions below are already in the user's language;
  //   2. desktop app: offer crash-recovery copies (features/files/recovery.ts);
  //   3. if none was restored, open the garden shed (the real engine
  //      calculates its dates).
  // This effect does not depend on `t`, so a later language switch never
  // re-opens (and so resets) the project. `startUpStarted` keeps the recovery
  // question from being asked twice (development StrictMode runs effects
  // twice).
  useEffect(() => {
    if (startUpStarted) return;
    startUpStarted = true;
    void (async () => {
      await loadSettings();
      if (!hasDesktopCore()) return;
      const dateFormat = useSettingsStore.getState().settings.general.dateFormat;
      const restored = await offerRecovery(i18next.t, dateStyleOf(dateFormat, i18next.language));
      if (!restored) await openDemoProject();
    })();
  }, [loadSettings, openDemoProject]);

  // Start-up, plain browser: there is no Rust side, so show the generated
  // 10,000-task schedule instead.
  useEffect(() => {
    if (!hasDesktopCore()) setShowMock(true, (n) => t("gantt.mockTaskName", { number: n }));
  }, [setShowMock, t]);

  // Start-up: ask the Rust core who it is. In a plain browser this gives null.
  useEffect(() => {
    // `cancelled` protects against a late answer arriving after the
    // component was removed (React may mount effects twice in development).
    let cancelled = false;
    getAppInfo()
      .then((info) => {
        if (!cancelled) setAppInfo(info);
      })
      .catch((error: unknown) => {
        // Not fatal: the app still works; the status bar just shows no version.
        console.error("app_info failed", error);
      });
    return () => {
      cancelled = true;
    };
  }, [setAppInfo]);

  return <AppShell />;
}
