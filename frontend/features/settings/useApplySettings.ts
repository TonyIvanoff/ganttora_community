/**
 * features/settings/useApplySettings.ts: makes the theme, language and
 * UI zoom settings take effect, immediately.
 *
 * What it is for:
 *   Storing "theme: dark" in settings.json does nothing on its own. This
 *   hook watches the settings store and, whenever the theme or language
 *   changes (at start-up or after Save in the Settings dialog), applies it:
 *   - **theme:** sets `<html data-theme="light|dark">`, or removes the
 *     attribute for "system". The CSS in `styles/themes.css` does the rest,
 *     and the Gantt canvas notices the attribute change and repaints
 *     (`features/gantt/renderer/controller.ts`).
 *   - **language:** calls `setLanguage` in `i18n/index.ts`; every label
 *     re-renders in the new language with no restart.
 *   - **UI zoom:** calls `applyUiZoom` in `lib/uiZoom.ts`.
 *
 * Where it fits:
 *   Called once, by `App.tsx`.
 */

import { useEffect } from "react";
import { setLanguage } from "@frontend/i18n";
import type { Theme } from "@frontend/ipc/types";
import { applyUiZoom } from "@frontend/lib/uiZoom";
import { useSettingsStore } from "./settingsStore";

/**
 * Puts a theme on the page.
 *
 * @param theme - "light", "dark" or "system".
 * @param root - The element that carries the attribute; the `<html>`
 *   element by default (tests can pass another one).
 *
 * @example
 * applyTheme("dark");   // <html data-theme="dark">
 * applyTheme("system"); // <html> (follows the OS again)
 */
export function applyTheme(theme: Theme, root: HTMLElement = document.documentElement): void {
  if (theme === "system") {
    // No attribute: themes.css then follows `prefers-color-scheme`.
    delete root.dataset.theme;
  } else {
    root.dataset.theme = theme;
  }
}

/**
 * Applies the theme, language and UI zoom from the settings store whenever they change.
 *
 * @example
 * function App() {
 *   useApplySettings();
 *   return <AppShell />;
 * }
 *
 * Beginner note: `useEffect(fn, [a, b])` runs `fn` after the component is
 * drawn, and again only when `a` or `b` changed. Selecting just the
 * values we need means unrelated setting changes (e.g. the autosave
 * interval) do not re-run the effects.
 */
export function useApplySettings(): void {
  const theme = useSettingsStore((s) => s.settings.general.theme);
  const language = useSettingsStore((s) => s.settings.general.language);

  const uiZoom = useSettingsStore((s) => s.settings.general.uiZoomPercent);

  useEffect(() => {
    void applyUiZoom(uiZoom);
  }, [uiZoom]);

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  useEffect(() => {
    void setLanguage(language);
  }, [language]);
}
