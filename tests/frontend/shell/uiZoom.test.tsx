/**
 * tests/frontend/shell/uiZoom.test.tsx: tests for the UI zoom slider
 * (`frontend/shell/UiZoom.tsx`) and `frontend/lib/uiZoom.ts`.
 *
 * Why it matters:
 *   A zoom outside 80–150% could make the app unusable.
 *
 * @vitest-environment happy-dom
 */

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vitest";
import { BROWSER_DEFAULT_SETTINGS } from "@frontend/features/settings/options";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import "@frontend/i18n";
import { missingPart } from "@frontend/lib/uiZoom";
import { UiZoom } from "@frontend/shell/UiZoom";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root;
let host: HTMLElement;

/** Shows the slider at a saved percentage. */
function show(uiZoomPercent: number): void {
  const general = { ...BROWSER_DEFAULT_SETTINGS.general, uiZoomPercent };
  useSettingsStore.setState({ settings: { ...BROWSER_DEFAULT_SETTINGS, general } });
  act(() => root.render(<UiZoom />));
}

/** The saved percentage. */
const saved = (): number => useSettingsStore.getState().settings.general.uiZoomPercent;

/** The buttons: smaller, larger, reset. */
const buttons = (): HTMLButtonElement[] => [...host.querySelectorAll("button")];

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

it("steps with the buttons and goes back to 100%", async () => {
  show(100);
  await act(async () => buttons()[1]?.click());
  expect(saved()).toBe(105);
  await act(async () => buttons()[0]?.click());
  await act(async () => buttons()[0]?.click());
  expect(saved()).toBe(95);
  await act(async () => buttons()[2]?.click());
  expect(saved()).toBe(100);
});

it("stops at 80% and 150%", () => {
  show(80);
  expect(buttons()[0]?.disabled).toBe(true);
  show(150);
  expect(buttons()[1]?.disabled).toBe(true);
});

it("zooms only when the slider is let go", async () => {
  show(100);
  const slider = host.querySelector("input");
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(slider, "130");
    slider?.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(saved()).toBe(100);
  expect(host.textContent).toContain("130%");
  await act(async () => void slider?.dispatchEvent(new Event("pointerup", { bubbles: true })));
  expect(saved()).toBe(130);
});

it("finds the part of the zoom the pixel ratio does not contain", () => {
  expect(missingPart(2, 2, 1.5)).toBe(1.5); // macOS
  expect(missingPart(3, 2, 1.5)).toBe(1); // Windows
  expect(missingPart(2, 2, 1)).toBe(1);
});
