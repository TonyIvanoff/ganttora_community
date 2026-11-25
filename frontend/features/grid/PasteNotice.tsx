/**
 * features/grid/PasteNotice.tsx: a small notice listing what could not be
 * used from pasted text.
 *
 * What it is for:
 *   A paste goes ahead even when some cells cannot be read (a duration
 *   "3x", a predecessor row that was not pasted…). Rust reports each such
 *   cell as a warning with a code; this notice shows them in the user's
 *   language (`paste.<code>` keys) until the user dismisses it.
 *
 * Where it fits:
 *   Rendered at the top of `TaskGrid.tsx`. The warnings are put into
 *   `gridStore.pasteWarnings` by `useTaskActions.paste`.
 */

import { useTranslation } from "react-i18next";
import type { PasteWarning } from "@frontend/ipc/types";
import { useGridStore } from "./gridStore";

/** Translation keys of the warning codes (typed, so a typo fails the build). */
const WARNING_KEYS = {
  unreadableDuration: "paste.unreadableDuration",
  unreadableOutlineLevel: "paste.unreadableOutlineLevel",
  unreadablePredecessor: "paste.unreadablePredecessor",
  predecessorNotPasted: "paste.predecessorNotPasted",
  selfLink: "paste.selfLink",
  duplicateLink: "paste.duplicateLink",
  linkToOwnSummary: "paste.linkToOwnSummary",
} as const satisfies Record<PasteWarning["code"], string>;

/** At most this many warnings are listed; the rest are counted. */
const MAX_LISTED = 5;

/**
 * The paste warnings notice.
 *
 * @returns The notice, or nothing when there are no warnings.
 */
export function PasteNotice() {
  const { t } = useTranslation();
  const warnings = useGridStore((s) => s.pasteWarnings);
  const setPasteWarnings = useGridStore((s) => s.setPasteWarnings);
  if (warnings.length === 0) return null;
  return (
    <div
      role="status"
      className="flex items-start gap-2 border-b border-line bg-app px-2 py-1 text-xs text-fg"
    >
      <div className="min-w-0 flex-1">
        <p className="font-medium">{t("paste.title", { count: warnings.length })}</p>
        <ul className="list-disc pl-4">
          {warnings.slice(0, MAX_LISTED).map((warning, i) => (
            // The list never changes while shown, so the position is a fine key.
            <li key={i}>{t(WARNING_KEYS[warning.code], warning)}</li>
          ))}
        </ul>
        {warnings.length > MAX_LISTED && (
          <p>{t("paste.more", { count: warnings.length - MAX_LISTED })}</p>
        )}
      </div>
      <button
        type="button"
        className="shrink-0 rounded border border-line px-2 py-0.5 whitespace-nowrap hover:bg-surface"
        onClick={() => setPasteWarnings([])}
      >
        {t("paste.dismiss")}
      </button>
    </div>
  );
}
