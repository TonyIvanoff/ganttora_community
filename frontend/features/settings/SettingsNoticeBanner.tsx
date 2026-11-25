/**
 * features/settings/SettingsNoticeBanner.tsx: tells the user when
 * settings.json could not be used at start-up.
 *
 * What it is for:
 *   If the settings file was damaged, unreadable, or there is no config
 *   folder, Rust starts with the default settings and sends a notice
 *   (`SettingsNotice` in `ipc/types.ts`). This banner explains what
 *   happened, in the user's language, and where the damaged file was kept.
 *
 * Where it fits:
 *   Rendered by `shell/AppShell.tsx` under the toolbar. The notice comes
 *   from `settingsStore.ts`; "Dismiss" hides it for this session.
 */

import { useTranslation } from "react-i18next";
import { useSettingsStore } from "./settingsStore";

/**
 * The settings notice banner; renders nothing when there is no notice.
 *
 * @returns The banner, or `null`.
 */
export function SettingsNoticeBanner() {
  const { t } = useTranslation();
  const notice = useSettingsStore((s) => s.notice);
  const dismiss = useSettingsStore((s) => s.dismissNotice);
  if (notice === null) return null;

  // One translated sentence per kind of notice.
  let text: string;
  switch (notice.kind) {
    case "corrupt":
      text =
        notice.backupPath !== null
          ? t("settings.notice.corrupt", { path: notice.backupPath })
          : t("settings.notice.corruptNoBackup");
      break;
    case "unreadable":
      text = t("settings.notice.unreadable");
      break;
    case "noConfigFolder":
      text = t("settings.notice.noConfigFolder");
      break;
  }

  return (
    <div
      role="status"
      className="flex flex-wrap items-center gap-2 border-b border-info-line bg-info-bg px-3 py-2 text-sm text-info-fg"
    >
      <span className="min-w-0 flex-1 break-words">{text}</span>
      <button
        type="button"
        className="rounded border border-info-line px-2 py-0.5 whitespace-nowrap"
        onClick={dismiss}
      >
        {t("settings.notice.dismiss")}
      </button>
    </div>
  );
}
