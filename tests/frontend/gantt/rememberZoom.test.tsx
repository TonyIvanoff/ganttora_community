// @vitest-environment happy-dom
/**
 * tests/frontend/gantt/rememberZoom.test.tsx: the zoom survives a restart.
 *
 * What it checks (`frontend/features/gantt/useRememberZoom.ts`): the saved
 * zoom is used only once the settings are loaded, many quick changes are
 * saved once, and "Reset to defaults" resets the zoom.
 *
 * Why it matters:
 *   Saving too early would overwrite the remembered zoom with 100 %.
 *
 * Main ideas:
 *   Fake timers let the test jump forward instead of waiting.
 */

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { SAVE_DELAY_MS, useRememberZoom } from "@frontend/features/gantt/useRememberZoom";
import { BROWSER_DEFAULT_SETTINGS } from "@frontend/features/settings/options";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import type { ViewSettings } from "@frontend/ipc/types";
import { useAppStore } from "@frontend/state/store";

/** React's mount point. */
let root: Root;

/** A component that only uses the hook. */
function Probe() {
  useRememberZoom();
  return null;
}

/** The default settings with another zoom. */
const withView = (view: ViewSettings) => ({ ...BROWSER_DEFAULT_SETTINGS, view });

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers();
  useAppStore.setState(useAppStore.getInitialState(), true);
  useSettingsStore.setState(useSettingsStore.getInitialState(), true);
  root = createRoot(document.body.appendChild(document.createElement("div")));
  act(() => root.render(<Probe />));
});

afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

/** Pretends the settings file was read. */
function load(view: ViewSettings): void {
  act(() => useSettingsStore.setState({ settings: withView(view), loaded: true }));
}

it("shows the saved zoom and does not save it again", async () => {
  const save = vi.fn().mockResolvedValue(null);
  useSettingsStore.setState({ save });
  load({ timescale: "weeks", zoomPercent: 150 });
  expect(useAppStore.getState()).toMatchObject({ zoom: "weeks", zoomPercent: 150 });
  await vi.advanceTimersByTimeAsync(SAVE_DELAY_MS * 2);
  expect(save).not.toHaveBeenCalled();
});

it("saves quick changes once, and nothing before loading", async () => {
  const save = vi.fn().mockResolvedValue(null);
  useSettingsStore.setState({ save });
  useAppStore.getState().setZoomPercent(50);
  await vi.advanceTimersByTimeAsync(SAVE_DELAY_MS * 2);
  expect(save).not.toHaveBeenCalled();

  load({ timescale: "days", zoomPercent: 100 });
  for (const percent of [110, 120, 130]) useAppStore.getState().setZoomPercent(percent);
  useAppStore.getState().setZoom("months");
  await vi.advanceTimersByTimeAsync(SAVE_DELAY_MS * 2);
  expect(save).toHaveBeenCalledTimes(1);
  expect(save.mock.calls[0]?.[0]).toMatchObject({
    view: { timescale: "months", zoomPercent: 130 },
  });
});

it("follows a reset of the settings", () => {
  load({ timescale: "weeks", zoomPercent: 200 });
  vi.advanceTimersByTime(SAVE_DELAY_MS * 2);
  act(() => useSettingsStore.setState({ settings: BROWSER_DEFAULT_SETTINGS }));
  expect(useAppStore.getState()).toMatchObject({ zoom: "days", zoomPercent: 100 });
});
