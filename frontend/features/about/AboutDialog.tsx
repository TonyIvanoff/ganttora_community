/**
 * features/about/AboutDialog.tsx: the "About Ganttora" dialog.
 *
 * What it is for:
 *   Shows the app's name, what it is, and its version.
 *
 * Where it fits:
 *   Rendered by `shell/AppShell.tsx`; opened from the Help menu (and the
 *   app menu on macOS). The version comes from Rust (`app_info`).
 */

import { useTranslation } from "react-i18next";
import { SimpleDialog } from "@frontend/components/ui/SimpleDialog";
import { useAppStore } from "@frontend/state/store";

/**
 * The dialog.
 *
 * @returns The dialog element, invisible while closed.
 */
export function AboutDialog() {
  const { t } = useTranslation();
  const appInfo = useAppStore((s) => s.appInfo);
  return (
    <SimpleDialog dialog="about" title={t("about.title")} closeLabel={t("about.close")}>
      <p className="text-lg font-semibold">{t("app.name")}</p>
      <p>{t("app.description")}</p>
      <p className="mt-2 text-fg-muted">
        {appInfo === null ? t("about.preview") : t("about.version", { version: appInfo.version })}
      </p>
    </SimpleDialog>
  );
}
