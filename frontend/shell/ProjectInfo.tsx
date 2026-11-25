/**
 * shell/ProjectInfo.tsx: the project in the window's top row.
 *
 * What it is for:
 *   Shows "Selected Project: House | Mon 1 Mar – Fri 26 Mar | Duration: 20 days" for
 *   the project of the clicked row (see `lib/shownProject.ts`).
 *
 * Where it fits:
 *   Rendered by `AppShell.tsx`. Dates and duration come from Rust; nothing
 *   is calculated here. The duration is the same as in the project's
 *   Duration cell.
 */

import { useTranslation } from "react-i18next";
import { useFormatLocale } from "@frontend/features/settings/useFormatLocale";
import { formatNumber, formatScheduleDate } from "@frontend/i18n/format";
import { useAppStore } from "@frontend/state/store";
import { shownProject } from "@frontend/lib/shownProject";

/**
 * The project's name, dates and duration.
 *
 * @returns The line, or `null` when there is no project. An empty project
 *   shows only its name, because it has no work to schedule yet.
 */
export function ProjectInfo() {
  const { t } = useTranslation();
  const { dateLocale, numberLocale } = useFormatLocale();
  const snapshot = useAppStore((s) => s.snapshot);
  const selectedId = useAppStore((s) => s.selectedTaskId);
  if (snapshot === null) return null;
  const project = shownProject(snapshot.tasks, selectedId);
  if (project === undefined) return null;

  const parts = [t("project.name", { name: project.name })];
  if (project.isSummary) {
    const start = formatScheduleDate(project.start, dateLocale);
    const finish = formatScheduleDate(project.finish, dateLocale);
    if (start !== null && finish !== null) parts.push(t("project.dates", { start, finish }));
    const days = project.durationMinutes / snapshot.minutesPerDay;
    parts.push(t("project.duration", { days: formatNumber(days, numberLocale), count: days }));
  }
  return (
    <span className="flex min-w-0 flex-wrap items-center" data-testid="project-info">
      {parts.map((part, i) => (
        <span key={i} className={i === 0 ? "font-medium" : "ml-3 border-l border-line pl-3"}>
          {part}
        </span>
      ))}
    </span>
  );
}
