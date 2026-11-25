/**
 * features/toolbars/actionClaim.ts: makes sure one key press acts once.
 *
 * What it is for:
 *   On macOS, ⌘S can arrive twice: the page's key listener hears it, and the
 *   menu bar's "Save" may also report it. The first one to arrive *claims*
 *   the action; the copy from the other source is dropped.
 *
 * Where it fits:
 *   Called by `useActions.ts` for every action, and by `grid/TaskGrid.tsx`
 *   for copy, cut and paste.
 */

/** Where an action was started. */
export type ActionSource = "key" | "menu" | "button";

/** Two reports of one key press arrive within this time, in milliseconds. */
export const CLAIM_WINDOW_MS = 250;

/** The last claim of each action: who, and when. */
const lastClaim = new Map<string, { source: ActionSource; at: number }>();

/**
 * Claims an action for one source.
 *
 * Holding ⌘Z repeats the key: key, menu, key, menu… Only reports from the
 * *other* source are dropped, so every repeat still counts once. Buttons
 * are never dropped: a button press has no second report.
 *
 * @param id - The action.
 * @param source - Who reports it.
 * @param now - The time in milliseconds (tests pass their own).
 * @returns `true` to go on, `false` if it is a copy.
 *
 * @example
 * claimAction("undo", "key", 0);  // true
 * claimAction("undo", "menu", 5); // false: the same press
 */
export function claimAction(id: string, source: ActionSource, now = performance.now()): boolean {
  if (source === "button") return true;
  const last = lastClaim.get(id);
  if (last !== undefined && last.source !== source && now - last.at < CLAIM_WINDOW_MS) return false;
  lastClaim.set(id, { source, at: now });
  return true;
}
