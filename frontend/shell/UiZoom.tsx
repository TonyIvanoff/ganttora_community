/**
 * shell/UiZoom.tsx: the slider that zooms the user interface.
 *
 * What it is for:
 *   − and + buttons, a slider from 80% to 150%, and the percentage (click:
 *   back to 100%). The choice is saved in the settings.
 *
 * Where it fits:
 *   Shown by `AppShell.tsx`, left of the theme buttons. `useApplySettings.ts`
 *   does the zooming (`lib/uiZoom.ts`).
 *
 * Main ideas:
 *   - While the slider is dragged only the number changes; the zoom comes
 *     when it is let go. Otherwise the slider would grow and move away under
 *     the mouse.
 */

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import { UI_ZOOM_MAX, UI_ZOOM_MIN, UI_ZOOM_STEP } from "@frontend/lib/uiZoom";
import { useAppStore } from "@frontend/state/store";

/** Classes of the three buttons. */
const BUTTON_CLASS = "rounded px-1 text-sm hover:bg-app disabled:opacity-40";

/**
 * The zoom slider.
 *
 * @returns The group of slider and buttons.
 */
export function UiZoom() {
  const { t } = useTranslation();
  const saved = useSettingsStore((s) => s.settings.general.uiZoomPercent);
  // The value while dragging, or null.
  const [draft, setDraft] = useState<number | null>(null);
  const shown = draft ?? saved;

  /** Saves a percentage; `useApplySettings` then shows it. */
  async function choose(percent: number): Promise<void> {
    const { settings, save } = useSettingsStore.getState();
    const uiZoomPercent = Math.min(UI_ZOOM_MAX, Math.max(UI_ZOOM_MIN, percent));
    if (uiZoomPercent !== settings.general.uiZoomPercent) {
      const error = await save({ ...settings, general: { ...settings.general, uiZoomPercent } });
      if (error !== null) useAppStore.getState().showIpcError(error);
    }
    setDraft(null);
  }

  /** Takes over the dragged value. */
  function release(): void {
    if (draft !== null) void choose(draft);
  }

  return (
    <span role="group" aria-label={t("uiZoom.label")} className="flex items-center gap-1">
      <button
        type="button"
        className={BUTTON_CLASS}
        aria-label={t("uiZoom.out")}
        title={t("uiZoom.out")}
        disabled={shown <= UI_ZOOM_MIN}
        onClick={() => void choose(saved - UI_ZOOM_STEP)}
      >
        −
      </button>
      <input
        type="range"
        className="w-24 accent-accent"
        aria-label={t("uiZoom.label")}
        title={t("uiZoom.label")}
        aria-valuetext={t("uiZoom.percent", { percent: shown })}
        min={UI_ZOOM_MIN}
        max={UI_ZOOM_MAX}
        step={UI_ZOOM_STEP}
        value={shown}
        onChange={(e) => setDraft(Number(e.target.value))}
        onPointerUp={release}
        onKeyUp={release}
        onBlur={release}
      />
      <button
        type="button"
        className={BUTTON_CLASS}
        aria-label={t("uiZoom.in")}
        title={t("uiZoom.in")}
        disabled={shown >= UI_ZOOM_MAX}
        onClick={() => void choose(saved + UI_ZOOM_STEP)}
      >
        +
      </button>
      <button
        type="button"
        className={`${BUTTON_CLASS} min-w-12 text-right tabular-nums`}
        title={t("uiZoom.reset")}
        onClick={() => void choose(100)}
      >
        {t("uiZoom.percent", { percent: shown })}
      </button>
    </span>
  );
}
