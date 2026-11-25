/**
 * lib/projectTint.ts: the light background colour of each project.
 *
 * What it is for:
 *   One file can hold several projects. Each gets its own
 *   light colour behind all its rows, so they can be told apart at once.
 *
 * Where it fits:
 *   Used by the task list (`TaskGrid.tsx`) and the chart (`draw.ts`), so
 *   both always agree.
 *
 * Main ideas:
 *   - **Any number of projects.** The hue (the place on the colour wheel)
 *     turns by the *golden angle*, 137.5°, from project to project. That
 *     way neighbours always differ a lot, however many there are.
 *   - **Always light.** The colour is RGBA with a low alpha (A). It is the
 *     theme's `--project-tint`: 0.1 in the light theme, less in the dark one.
 */

/** Hue of the first project (green), in degrees. */
const FIRST_HUE = 140;
/** The golden angle in degrees. */
const GOLDEN_ANGLE = 137.5;

/**
 * The background colour of a project's rows.
 *
 * @param index - The project (0 = first in the list).
 * @param alpha - How strong: 0 = invisible, 1 = solid. As CSS text: "0.1"
 *   or `var(--project-tint)`.
 * @returns A CSS colour.
 *
 * @example
 * projectTint(0, "0.1"); // "rgba(32, 223, 96, 0.1)": light green
 */
export function projectTint(index: number, alpha: string): string {
  const hue = (FIRST_HUE + index * GOLDEN_ANGLE) % 360;
  // Hue → red, green, blue, at 75 % saturation and 50 % lightness: the
  // three channels are the same wave, shifted by 120° each.
  const channel = (shift: number): number => {
    const k = ((hue + shift) / 30) % 12;
    const value = 0.5 - 0.375 * Math.max(-1, Math.min(k - 3, 9 - k, 1));
    return Math.round(value * 255);
  };
  return `rgba(${channel(0)}, ${channel(240)}, ${channel(120)}, ${alpha})`;
}
