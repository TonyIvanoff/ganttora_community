/**
 * perfStats.ts: a tiny shared notebook for performance numbers.
 *
 * What it is for:
 *   The Gantt controller writes how long the last frame took to draw; the
 *   developer FPS counter (`FpsCounter.tsx`) reads it. Storing this in React
 *   state would re-render components on every frame and slow everything
 *   down, so we use a plain object that nobody "watches".
 */

/** Performance numbers of the Gantt renderer. Mutated in place. */
export const ganttPerfStats = {
  /** Time the last frame took to draw, in milliseconds. */
  lastDrawMs: 0,
  /** Number of frames drawn since the app started. */
  framesDrawn: 0,
};
