/**
 * features/files/useDocumentTitle.ts: the name of the open document, with
 * "•" for unsaved changes, and the window title that goes with it.
 *
 * What it is for:
 *   Desktop apps show which file is open and whether it has unsaved
 *   changes. This hook builds that text ("shed.gtra •") and
 *   keeps the window's title bar in sync ("shed.gtra • — Ganttora").
 *
 * Where it fits:
 *   Called once by `shell/AppShell.tsx`. Reads `file` and `snapshot` from
 *   the store (`state/store.ts`), which Rust fills after every command.
 *
 * Main ideas:
 *   - A saved project shows its **file name**; a project that was never
 *     saved (a new one, the sample) shows its **project name**.
 *   - All texts come from `locales/en.json` (`file.displayName`,
 *     `file.displayNameDirty`, `file.windowTitle`), so translators can move
 *     the dot or the dash if their language prefers.
 */

import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { hasDesktopCore } from "@frontend/ipc/client";
import { useAppStore } from "@frontend/state/store";

/**
 * Gives the name of the open document for titles, with " •" added while
 * there are unsaved changes, and keeps the window title in sync.
 *
 * @returns The display name (e.g. "shed.gtra •"), or `null` before a
 *   project is loaded or while the 10,000-task demo is shown (the window
 *   title is then just "Ganttora").
 *
 * @example
 * const title = useDocumentTitle();
 * <span>{title}</span>
 *
 * Beginner note: the page's `document.title` is not automatically the
 * native window title in Tauri, so we also call `setTitle` on the current
 * window (allowed by `core:window:allow-set-title` in
 * `backend/src-tauri/capabilities/default.json`). `useEffect` runs that
 * code *after* React has drawn the screen, and again only when `name`
 * changes.
 */
export function useDocumentTitle(): string | null {
  const { t } = useTranslation();
  const fileName = useAppStore((s) => s.file?.fileName ?? null);
  const dirty = useAppStore((s) => s.file?.dirty ?? false);
  const projectName = useAppStore((s) => s.snapshot?.projectName ?? null);
  const isDemoData = useAppStore((s) => s.isDemoData);

  // A saved project shows its file name; a never-saved one its project name.
  const base = fileName ?? projectName;
  const name =
    base === null || isDemoData
      ? null
      : t(dirty ? "file.displayNameDirty" : "file.displayName", { name: base });

  useEffect(() => {
    const title =
      name === null ? t("app.name") : t("file.windowTitle", { document: name, app: t("app.name") });
    document.title = title;
    if (hasDesktopCore()) {
      // Not fatal if it fails (e.g. permission missing): log and move on.
      getCurrentWindow()
        .setTitle(title)
        .catch((error: unknown) => console.error("setTitle failed", error));
    }
  }, [name, t]);

  return name;
}
