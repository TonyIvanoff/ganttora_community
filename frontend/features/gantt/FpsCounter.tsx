/**
 * FpsCounter.tsx: a small "frames per second" display for developers.
 *
 * What it is for:
 *   The M0 goal is smooth scrolling (60 fps) with 10,000 tasks. This counter
 *   shows how many frames the browser shows per second and how long the last
 *   Gantt frame took to paint. It is shown only in development builds
 *   (`pnpm dev` / `pnpm tauri dev`), never to users.
 *
 * How it measures:
 *   A `requestAnimationFrame` loop counts screen refreshes. Twice a second
 *   it divides the count by the time passed. The paint time comes from
 *   `perfStats.ts`, written by the Gantt controller.
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ganttPerfStats } from "./perfStats";

/** How often the display updates, in milliseconds. */
const UPDATE_INTERVAL_MS = 500;

/**
 * The FPS display.
 *
 * @returns A small text element like "60 fps · draw 0.8 ms".
 *
 * Note: while nothing moves, the browser may slow the loop down, so a low
 * number at rest is normal. Watch the number *while scrolling*.
 *
 * Beginner note: `useState` gives a value plus a function to change it.
 * Changing it makes React re-draw this component with the new value.
 */
export function FpsCounter() {
  const { t } = useTranslation();
  const [stats, setStats] = useState({ fps: 0, drawMs: 0 });

  useEffect(() => {
    let frames = 0;
    let last = performance.now();
    let id = 0;
    /** Called once per screen refresh. */
    const loop = (now: number) => {
      frames++;
      if (now - last >= UPDATE_INTERVAL_MS) {
        setStats({
          fps: Math.round((frames * 1000) / (now - last)),
          drawMs: Math.round(ganttPerfStats.lastDrawMs * 10) / 10,
        });
        frames = 0;
        last = now;
      }
      id = requestAnimationFrame(loop);
    };
    id = requestAnimationFrame(loop);
    // Clean-up: stop the loop when the component disappears.
    return () => cancelAnimationFrame(id);
  }, []);

  return (
    <span className="tabular-nums" data-testid="fps-counter">
      {t("dev.fps", { fps: stats.fps, drawMs: stats.drawMs })}
    </span>
  );
}
