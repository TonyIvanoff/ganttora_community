/**
 * shell/SettingsButton.tsx: the gear button in the title row.
 *
 * What it is for:
 *   One click opens the Settings window (also in the menu: Settings…).
 *
 * Where it fits:
 *   Shown by `AppShell.tsx` after the theme buttons.
 */

import { useTranslation } from "react-i18next";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";

/** The drawing: a ring with eight teeth. */
const GEAR_PATH =
  "M8 5.7a2.3 2.3 0 1 0 .01 0z M8 3.4a4.6 4.6 0 1 0 .01 0z M8 1v2.4 M8 12.6V15 M1 8h2.4 M12.6 8H15 " +
  "M3.05 3.05l1.7 1.7 M11.25 11.25l1.7 1.7 M12.95 3.05l-1.7 1.7 M4.75 11.25l-1.7 1.7";

/**
 * The gear button.
 *
 * @returns The button.
 */
export function SettingsButton() {
  const { t } = useTranslation();
  const openSettings = useSettingsStore((s) => s.openDialog);
  return (
    <button
      type="button"
      aria-label={t("settings.title")}
      title={t("settings.title")}
      className="rounded border border-line p-1 hover:bg-app"
      onClick={openSettings}
    >
      <svg
        aria-hidden="true"
        width="16"
        height="16"
        viewBox="0 0 16 16"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
      >
        <path d={GEAR_PATH} />
      </svg>
    </button>
  );
}
