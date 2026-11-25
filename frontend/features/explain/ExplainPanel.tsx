/**
 * features/explain/ExplainPanel.tsx: the "Why this date?" side panel.
 *
 * What it is for:
 *   Project plans surprise people: "why does this task start on the 12th?"
 *   The panel answers for the selected task in plain sentences: what drove
 *   its date (a predecessor and its link type and lag, a constraint, the
 *   project start, a deadline, a manual start, its subtasks), whether the
 *   calendar then moved the date to the next working time, and a button to
 *   jump to the task that drives it.
 *
 * Where it fits:
 *   Shown by `shell/AppShell.tsx` beside the Gantt chart when the toolbar
 *   button "Why this date?" is pressed (`explainStore.ts`). Reads the
 *   selected task from the main store; the sentences come from
 *   `explainText.ts` and `locales/en.json` (`explain.*`).
 *
 * Main ideas:
 *   - The panel only *shows* what the engine calculated (decision D16).
 *   - `aria-live="polite"` lets screen readers read the new explanation
 *     when the selection changes, without interrupting the user.
 *   - Width: `w-80` with wrapping text, so longer German sentences and
 *     Chinese text fit.
 */

import { useTranslation } from "react-i18next";
import { useFormatLocale } from "@frontend/features/settings/useFormatLocale";
import { formatNumber, formatScheduleDate } from "@frontend/i18n/format";
import { useAppStore } from "@frontend/state/store";
import { explainLines } from "./explainText";
import { useExplainStore } from "./explainStore";

/**
 * The "Why this date?" panel.
 *
 * @returns An `<aside>` with the explanation of the selected task.
 */
export function ExplainPanel() {
  const { t } = useTranslation();
  const snapshot = useAppStore((s) => s.snapshot);
  const isDemoData = useAppStore((s) => s.isDemoData);
  const selectedTaskId = useAppStore((s) => s.selectedTaskId);
  const revealTask = useAppStore((s) => s.revealTask);
  const setOpen = useExplainStore((s) => s.setOpen);
  const { dateLocale, numberLocale } = useFormatLocale();

  const task =
    snapshot === null || isDemoData || selectedTaskId === null
      ? undefined
      : snapshot.tasks.find((row) => row.id === selectedTaskId);

  const lines =
    task === undefined || snapshot === null
      ? []
      : explainLines(
          task,
          snapshot,
          (iso) => formatScheduleDate(iso, dateLocale) ?? iso,
          (n) => formatNumber(n, numberLocale),
        );

  return (
    <aside
      aria-labelledby="explain-title"
      className="flex w-80 shrink-0 flex-col gap-3 overflow-y-auto border-l border-line bg-surface p-3 text-sm"
    >
      <div className="flex items-start gap-2">
        <h2 id="explain-title" className="font-semibold">
          {t("explain.title")}
        </h2>
        <button
          type="button"
          className="ml-auto rounded border border-line px-2 py-0.5 text-xs whitespace-nowrap hover:bg-app"
          onClick={() => setOpen(false)}
        >
          {t("explain.close")}
        </button>
      </div>
      <div aria-live="polite" className="flex flex-col gap-2">
        {task === undefined ? (
          <p className="text-fg-muted">{t("explain.noSelection")}</p>
        ) : (
          <>
            <p className="font-medium break-words">
              {t("explain.taskHeading", { row: task.outlineNumber, name: task.name })}
            </p>
            {lines.length === 0 ? (
              <p className="text-fg-muted">{t("explain.notScheduled")}</p>
            ) : (
              <ul className="flex list-disc flex-col gap-2 pl-5">
                {lines.map((line) => (
                  <li key={line.key} className="break-words">
                    {t(line.key, line.params)}
                    {line.jumpTo !== null && (
                      <>
                        {" "}
                        <button
                          type="button"
                          className="text-accent underline hover:no-underline"
                          onClick={() => {
                            if (line.jumpTo !== null) revealTask(line.jumpTo);
                          }}
                        >
                          {t("explain.jump")}
                        </button>
                      </>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </>
        )}
      </div>
    </aside>
  );
}
