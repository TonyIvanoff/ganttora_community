/**
 * features/files/recovery.ts: offers crash-recovery copies at start-up.
 *
 * What it is for:
 *   While a project has unsaved changes, autosave writes a recovery copy
 *   (`useAutosave.ts`). A normal close deletes it. If a copy is still there
 *   when Ganttora starts, the last run ended unexpectedly (a crash, a power
 *   cut). This file asks the user, for each copy: **Restore** (open it,
 *   with its unsaved changes), **Discard** (delete it) or **Later** (keep
 *   it and ask again at the next start).
 *
 * Where it fits:
 *   `App.tsx` calls {@link offerRecovery} once at start-up, before it opens
 *   the sample project; if a copy was restored, the sample is not opened.
 *   Rust lists, restores and deletes the copies
 *   (`backend/src-tauri/src/ipc/recovery.rs`).
 *
 * Main ideas:
 *   - The window shows one project at a time, so at most one copy is
 *     restored per start; any further copies are kept for the next start.
 *   - A damaged copy cannot be restored; the question then only offers to
 *     delete it (or keep it).
 *   - All texts come from `locales/en.json` (`recovery.*`).
 */

import type { TFunction } from "i18next";
import { recoveryDiscard, recoveryList } from "@frontend/ipc/client";
import { writeDay } from "@frontend/i18n/format";
import type { RecoveryCopyDto } from "@frontend/ipc/types";
import { useAppStore } from "@frontend/state/store";
import { askRecovery, confirmAction } from "./dialogs";

/**
 * Formats the time of a copy for the question.
 *
 * @param savedAt - UTC time from Rust, e.g. "2027-02-15T10:42:07Z", or `null`.
 * @param style - The date style (`i18n/format.ts`).
 * @returns The local date and time, e.g. "15 Feb 2027, 11:42", or `null`.
 *
 * @example
 * formatSavedAt("2027-02-15T10:42:07Z", "en|dd mmm yyyy"); // "15 Feb 2027, 11:42" (in Berlin)
 */
export function formatSavedAt(savedAt: string | null, style: string): string | null {
  if (savedAt === null) return null;
  const date = new Date(savedAt);
  if (Number.isNaN(date.getTime())) return null;
  /** Two digits: 7 → "07". */
  const two = (n: number): string => String(n).padStart(2, "0");
  const day = `${date.getFullYear()}-${two(date.getMonth() + 1)}-${two(date.getDate())}`;
  return `${writeDay(day, style)}, ${two(date.getHours())}:${two(date.getMinutes())}`;
}

/**
 * Picks the name to show for a copy: its project name, else its file name,
 * else the translated "Untitled".
 *
 * @param copy - The recovery copy.
 * @param untitled - The translated "Untitled".
 * @returns The display name.
 */
export function copyDisplayName(copy: RecoveryCopyDto, untitled: string): string {
  return copy.projectName ?? copy.sourceFileName ?? untitled;
}

/**
 * Asks about every recovery copy left behind and acts on the answers.
 *
 * @param t - The translate function.
 * @param style - Date style for the copy's time (see {@link formatSavedAt}).
 * @returns `true` if a copy was restored (it is now the open project).
 *
 * @example
 * const restored = await offerRecovery(i18next.t, dateLocale);
 * if (!restored) await openDemoProject();
 */
export async function offerRecovery(t: TFunction, style: string): Promise<boolean> {
  let copies: RecoveryCopyDto[];
  try {
    copies = (await recoveryList()) ?? [];
  } catch (error: unknown) {
    // Not fatal: the app works without recovery. Tell the user once.
    useAppStore.getState().showIpcError(error);
    return false;
  }

  let restored = false;
  for (const copy of copies) {
    // One project per window: the rest wait for the next start.
    if (restored) break;
    const name = copyDisplayName(copy, t("file.untitledName"));
    const time = formatSavedAt(copy.savedAt, style) ?? t("recovery.unknownTime");
    try {
      if (!copy.readable) {
        const remove = await confirmAction({
          title: t("recovery.title"),
          message: t("recovery.damagedMessage", { name, time }),
          okLabel: t("recovery.discard"),
          cancelLabel: t("recovery.later"),
        });
        if (remove) await recoveryDiscard(copy.sessionId);
        continue;
      }
      const choice = await askRecovery({
        title: t("recovery.title"),
        message: t("recovery.message", { name, time }),
        restoreLabel: t("recovery.restore"),
        discardLabel: t("recovery.discard"),
        laterLabel: t("recovery.later"),
      });
      if (choice === "restore") {
        restored = await useAppStore.getState().restoreRecovery(copy.sessionId);
      } else if (choice === "discard") {
        await recoveryDiscard(copy.sessionId);
      }
    } catch (error: unknown) {
      useAppStore.getState().showIpcError(error);
    }
  }
  return restored;
}
