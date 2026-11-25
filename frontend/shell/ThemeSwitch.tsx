/**
 * shell/ThemeSwitch.tsx: the sun and moon buttons in the title row.
 *
 * What it is for:
 *   One click switches to the light or the dark theme and saves the choice,
 *   exactly like Settings → General → Theme ("System" is chosen there).
 *
 * Where it fits:
 *   Shown by `AppShell.tsx` next to the "Toolbars" button.
 */

import { useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import { useAppStore } from "@frontend/state/store";

/** Asks the operating system for its theme. */
const DARK_QUERY = "(prefers-color-scheme: dark)";

/**
 * Calls back when the operating system's theme changes.
 *
 * @param onChange - What to call.
 * @returns A function that stops it.
 */
function subscribe(onChange: () => void): () => void {
  const query = window.matchMedia(DARK_QUERY);
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
}

/** The drawings: a sun and a moon. */
const PATHS = {
  light:
    "M8 5a3 3 0 1 0 .01 0z M8 1v1.5 M8 13.5V15 M1 8h1.5 M13.5 8H15 M3 3l1.1 1.1 M11.9 11.9L13 13 M13 3l-1.1 1.1 M4.1 11.9L3 13",
  dark: "M13.5 9.8A6 6 0 1 1 6.2 2.5a5 5 0 0 0 7.3 7.3z",
} as const;

/**
 * The two theme buttons.
 *
 * @returns The buttons; the one of the theme on screen is pressed.
 *
 * Beginner note: `useSyncExternalStore` reads a value that lives outside
 * React (here: the system's theme) and redraws when it changes.
 */
export function ThemeSwitch() {
  const { t } = useTranslation();
  const chosen = useSettingsStore((s) => s.settings.general.theme);
  const systemDark = useSyncExternalStore(subscribe, () => window.matchMedia(DARK_QUERY).matches);
  const shown = chosen === "system" ? (systemDark ? "dark" : "light") : chosen;

  /** Saves the choice; `useApplySettings` then shows it. */
  async function choose(theme: "light" | "dark"): Promise<void> {
    const { settings, save } = useSettingsStore.getState();
    const error = await save({ ...settings, general: { ...settings.general, theme } });
    if (error !== null) useAppStore.getState().showIpcError(error);
  }

  return (
    <span role="group" aria-label={t("theme.label")} className="flex">
      {(["light", "dark"] as const).map((theme) => (
        <button
          key={theme}
          type="button"
          aria-pressed={shown === theme}
          aria-label={t(`theme.${theme}`)}
          title={t(`theme.${theme}`)}
          className="border border-line p-1 first:rounded-l last:-ml-px last:rounded-r hover:bg-app aria-pressed:bg-accent aria-pressed:text-accent-fg"
          onClick={() => void choose(theme)}
        >
          <svg
            aria-hidden="true"
            width="16"
            height="16"
            viewBox="0 0 16 16"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.4"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d={PATHS[theme]} />
          </svg>
        </button>
      ))}
    </span>
  );
}
