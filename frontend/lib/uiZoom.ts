/**
 * lib/uiZoom.ts: makes the whole user interface larger or smaller.
 *
 * What it is for:
 *   Everything inside the window grows or shrinks by one percentage. The web
 *   view does the work, like a browser's own zoom, so clicks, menus and
 *   dialogs fit by themselves.
 *
 * Where it fits:
 *   `useApplySettings.ts` applies the saved percentage; `shell/UiZoom.tsx`
 *   is the slider; the Gantt canvas asks {@link canvasScale}.
 *
 * Main ideas:
 *   - A canvas is a picture. To stay sharp it needs more pixels when zoomed.
 *     Some web views count the zoom in `devicePixelRatio`, others
 *     (macOS) do not; {@link canvasScale} is the part that is missing.
 */

import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { hasDesktopCore } from "@frontend/ipc/client";

/** Smallest zoom in percent (must match `settings.rs`). */
export const UI_ZOOM_MIN = 80;
/** Largest zoom in percent (must match `settings.rs`). */
export const UI_ZOOM_MAX = 150;
/** One step of the slider and its buttons, in percent. */
export const UI_ZOOM_STEP = 5;

/** See {@link canvasScale}. */
let missingScale = 1;

/**
 * The part of the zoom that `devicePixelRatio` does not contain.
 *
 * @returns 1 if it contains all of it.
 */
export function canvasScale(): number {
  return missingScale;
}

/**
 * Works out {@link canvasScale}.
 *
 * @param ratio - `devicePixelRatio`.
 * @param screenScale - The monitor's own scale (Retina: 2).
 * @param zoom - The zoom as a factor (1.5 = 150%).
 * @returns 1 if `ratio` contains the zoom, else `zoom`.
 *
 * @example
 * missingPart(2, 2, 1.5); // 1.5 (macOS)
 * missingPart(3, 2, 1.5); // 1
 */
export function missingPart(ratio: number, screenScale: number, zoom: number): number {
  const withZoom = Math.abs(ratio - screenScale * zoom);
  return withZoom <= Math.abs(ratio - screenScale) ? 1 : zoom;
}

/**
 * Zooms the user interface.
 *
 * @param percent - 80 to 150.
 * @returns A promise that resolves when it is done. In a plain browser
 *   nothing happens (browsers have a zoom of their own).
 */
export async function applyUiZoom(percent: number): Promise<void> {
  if (!hasDesktopCore()) return;
  const zoom = percent / 100;
  try {
    await getCurrentWebview().setZoom(zoom);
    const screenScale = await getCurrentWindow().scaleFactor();
    missingScale = missingPart(window.devicePixelRatio, screenScale, zoom);
  } catch (error: unknown) {
    // Not fatal: the app stays at its size.
    console.error("setZoom failed", error);
  }
  // The canvas checks its size again (renderer/controller.ts).
  document.documentElement.dataset.uiZoom = String(percent);
}
