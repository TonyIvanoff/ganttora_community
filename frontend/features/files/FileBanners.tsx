/**
 * features/files/FileBanners.tsx: the information strips about the open
 * file: "read-only", "upgraded from an older format" and "notes from
 * opening".
 *
 * What it is for:
 *   - **Read-only banner:** a file made by a newer Ganttora opens read-only
 *     (saving over it could destroy data this version does not know). The
 *     banner says so, and why (`file.readOnlyReason`, shown as
 *     `error.<code>`), and offers "Save As…" to keep changes in a new file.
 *   - **Upgraded banner:** a file from an older Ganttora was upgraded while
 *     opening (only the working copy, not the file on disk). After saving,
 *     older versions can open it only read-only, so the user is told once.
 *   - **Notes banner:** after opening, Rust may report things in the file
 *     that did not fit the program exactly (nothing is lost
 *     silently). Each note becomes one translated sentence. The user can
 *     dismiss the banner.
 *
 * Where it fits:
 *   Rendered by `shell/AppShell.tsx` under the toolbar. Reads `file` and
 *   `readNotes` from the store; the notes are plain data from Rust
 *   (`ReadNoteDto` in `ipc/types.ts`) and get their text from
 *   `features/files/noteText.ts` (keys `file.note.<kind>`).
 *
 * Main ideas:
 *   - `role="status"` lets screen readers announce the banners politely
 *     (after what they are reading now), unlike `role="alert"`, which
 *     interrupts. These are information, not errors.
 *   - Text may be ~30% longer in German, so the strips wrap (`flex-wrap`)
 *     instead of using fixed widths.
 */

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useFormatLocale } from "@frontend/features/settings/useFormatLocale";
import { errorText } from "@frontend/lib/errors";
import { useAppStore } from "@frontend/state/store";
import { noteText } from "./noteText";
import { useFileActions } from "./useFileActions";

/** Shared look of the small buttons inside the banners (Tailwind classes). */
const BANNER_BUTTON_CLASS = "rounded border border-info-line px-2 py-0.5 whitespace-nowrap";

/**
 * The read-only banner and the notes banner (each shown only when needed).
 *
 * @returns Zero, one or two banners.
 */
export function FileBanners() {
  const { t } = useTranslation();
  // Dates in the notes follow the "Date format" setting, like the rest of
  // the app (not the UI language).
  const { dateLocale } = useFormatLocale();
  const file = useAppStore((s) => s.file);
  const readNotes = useAppStore((s) => s.readNotes);
  const snapshot = useAppStore((s) => s.snapshot);
  const dismissReadNotes = useAppStore((s) => s.dismissReadNotes);
  const isDemoData = useAppStore((s) => s.isDemoData);
  const { saveFileAs } = useFileActions();
  /**
   * The upgrade notice is dismissed per file: we remember *which* file it
   * was dismissed for, so opening another upgraded file shows it again.
   *
   * Beginner note: `useState` keeps a value inside this component between
   * renders; calling the setter re-draws the component with the new value.
   */
  const [upgradeDismissedFor, setUpgradeDismissedFor] = useState<string | null>(null);
  const upgradeKey = `${file?.path ?? ""}#${file?.upgradedFrom ?? ""}`;

  // While the 10,000-task demo is shown, the real project is hidden, so
  // banners about its file would be confusing.
  if (isDemoData) return null;

  return (
    <>
      {file?.readOnly === true && (
        <div
          role="status"
          data-testid="read-only-banner"
          className="flex flex-wrap items-center gap-2 border-b border-info-line bg-info-bg px-3 py-2 text-sm text-info-fg"
        >
          <span>
            {t("file.readOnlyMessage")}
            {/* Why, in detail (e.g. "format 3; this version supports up to
                2"), from gtra's code and values. */}
            {file.readOnlyReason !== null && (
              <>
                {" "}
                {errorText(
                  { code: file.readOnlyReason.code, details: file.readOnlyReason.params },
                  t,
                )}
              </>
            )}
          </span>
          <button
            type="button"
            className={`ml-auto ${BANNER_BUTTON_CLASS}`}
            onClick={() => void saveFileAs()}
          >
            {t("file.saveAs")}
          </button>
        </div>
      )}
      {file?.upgradedFrom != null && upgradeDismissedFor !== upgradeKey && (
        <div
          role="status"
          data-testid="upgraded-banner"
          className="flex flex-wrap items-center gap-2 border-b border-info-line bg-info-bg px-3 py-2 text-sm text-info-fg"
        >
          <span className="min-w-0 flex-1">
            {t("file.upgradedMessage", { from: file.upgradedFrom })}
          </span>
          <button
            type="button"
            className={BANNER_BUTTON_CLASS}
            onClick={() => setUpgradeDismissedFor(upgradeKey)}
          >
            {t("file.dismissNotes")}
          </button>
        </div>
      )}
      {readNotes.length > 0 && (
        <div
          role="status"
          data-testid="read-notes-banner"
          className="flex flex-wrap items-start gap-2 border-b border-info-line bg-info-bg px-3 py-2 text-sm text-info-fg"
        >
          <div className="min-w-0 flex-1">
            <p className="font-medium">{t("file.notesTitle", { count: readNotes.length })}</p>
            <ul className="list-disc pl-5">
              {readNotes.map((note, index) => (
                // The notes never change order while shown, so the index is a
                // stable enough key here.
                <li key={index}>{noteText(note, t, snapshot, dateLocale)}</li>
              ))}
            </ul>
          </div>
          <button type="button" className={BANNER_BUTTON_CLASS} onClick={dismissReadNotes}>
            {t("file.dismissNotes")}
          </button>
        </div>
      )}
    </>
  );
}
