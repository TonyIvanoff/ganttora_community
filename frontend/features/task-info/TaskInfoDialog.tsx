/**
 * features/task-info/TaskInfoDialog.tsx: the Task Information dialog
 * (double-click a task row, Shift+F2, or the toolbar button).
 *
 * What it is for:
 *   Shows every field of one task on six tabs, like MS Project's dialog:
 *   - **General:** name, duration, scheduling mode, milestone mark, hyperlink;
 *   - **Predecessors:** a table of incoming links (task, type, lag);
 *   - **Dates:** start, calculated, planned and actual finish, constraint type
 *     and date, deadline, task calendar;
 *   - **Notes:** free text;
 *   - **Blockers:** unplanned delays on the task (not in MS
 *     Project): each with its dates, length, reason and reported day, to
 *     edit or remove, plus "Add blocker";
 *   - **Risks:** what could go wrong (risks.ts), also for groups and projects.
 *   For a project it is the Project Information dialog: General, Dates
 *   (start, calculated and planned finish), Notes,
 *   Blockers (a list of its tasks' blockers) and Risks (its own, and a list
 *   of its tasks' risks).
 *   OK sends the changes; Cancel (or Escape) throws them away.
 *
 * Where it fits:
 *   Rendered once by `shell/AppShell.tsx`; opened through
 *   `taskInfoStore.ts`. The form is a *draft* (`taskInfoCommands.ts`); on OK
 *   the changed fields become commands, sent with the store's
 *   `applyCommands`. Several commands travel as one `batch`: **one OK = one
 *   undo step**, and if Rust refuses any part (e.g. a link that makes a
 *   loop) nothing is changed and the dialog stays open with the reason.
 *
 * Main ideas:
 *   - **The native `<dialog>` element** (`showModal()`): it keeps the focus
 *     inside, closes with Escape and dims the window behind it, without
 *     extra code. The same as the Settings dialog.
 *   - **Tabs** follow the WAI-ARIA tabs pattern: ←/→ (and Home/End) move
 *     between tabs, Tab moves into the panel.
 *   - **Fresh draft on every open:** the dialog reads the task from the
 *     latest snapshot each time it opens.
 */

import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { DateField } from "@frontend/components/ui/DateField";
import { dayOf, isEditable } from "@frontend/features/grid/cellEdit";
import { CONSTRAINT_KEYS, MODE_KEYS } from "@frontend/features/grid/gridFormat";
import { useFormatLocale } from "@frontend/features/settings/useFormatLocale";
import { formatNumber, formatScheduleDate } from "@frontend/i18n/format";
import { DEPENDENCY_TYPES } from "@frontend/features/grid/predecessors";
import { useUnitContext } from "@frontend/features/grid/useUnitContext";
import type { Constraint, DependencyType, TaskMode, TaskRow } from "@frontend/ipc/types";
import { errorText } from "@frontend/lib/errors";
import { useAppStore } from "@frontend/state/store";
import {
  draftBlockersFromTask,
  projectBlockers,
  newDraftBlocker,
  todayIso,
  type BlockerFormValues,
  type DraftBlocker,
  type ProjectBlocker,
} from "./blockers";
import { localToday } from "@frontend/features/gantt/renderer/dates";
import { RiskFields } from "./RiskFields";
import {
  draftRisksFromTask,
  EMPTY_RISK,
  missedStartPriority,
  missedStarts,
  taskRisks,
  type DraftRisk,
  type MissedStart,
  type TaskRisk,
} from "./risks";
import {
  commandsFromDraft,
  draftFromTask,
  needsDate,
  withMode,
  withStart,
  type DraftField,
  type TaskInfoDraft,
} from "./taskInfoCommands";
import { TABS, useTaskInfoStore, type TabId } from "./taskInfoStore";

/** The tabs of a project: it has no predecessors. */
const PROJECT_TABS: readonly TabId[] = ["general", "dates", "notes", "blockers", "risks"];

/** Translation keys of the tab titles. */
const TAB_KEYS = {
  general: "taskInfo.tabs.general",
  predecessors: "taskInfo.tabs.predecessors",
  dates: "taskInfo.tabs.dates",
  notes: "taskInfo.tabs.notes",
  blockers: "taskInfo.tabs.blockers",
  risks: "taskInfo.tabs.risks",
} as const satisfies Record<TabId, string>;

/** Translation keys of the link types (long names, e.g. "Finish-to-Start (FS)"). */
const LINK_TYPE_KEYS = {
  finishToStart: "link.type.finishToStart",
  startToStart: "link.type.startToStart",
  finishToFinish: "link.type.finishToFinish",
  startToFinish: "link.type.startToFinish",
} as const satisfies Record<DependencyType, string>;

/** The constraint types in MS Project's order. */
const CONSTRAINT_TYPES = Object.keys(CONSTRAINT_KEYS) as Constraint["type"][];

/** The tab that holds each field (to jump to it when it has an error). */
const FIELD_TAB: Record<DraftField, TabId> = {
  duration: "general",
  start: "dates",
  plannedFinish: "dates",
  actualFinish: "dates",
  constraintDate: "dates",
  deadline: "dates",
  links: "predecessors",
  blockers: "blockers",
  risks: "risks",
};

/** Shared look of fields (Tailwind classes). */
const FIELD_CLASS =
  "rounded border border-line bg-surface px-2 py-1 text-sm text-fg disabled:opacity-50";

/** A problem shown in the dialog: where and what. */
interface ShownProblem {
  /** The field (and so the tab), or `null` for an error from Rust. */
  field: DraftField | null;
  /** The translated message. */
  text: string;
}

/**
 * The Task Information dialog. Renders nothing visible while closed.
 *
 * @returns The `<dialog>` element; its form ({@link TaskInfoForm}) exists
 *   only while a task is shown.
 *
 * Why two components? The form's state (draft, tab, error) must start
 * fresh each time the dialog opens. Rendering the form only while open,
 * with the task ID as its `key`, gives exactly that: React creates a new
 * form (with new state) for every opening.
 */
export function TaskInfoDialog() {
  const taskId = useTaskInfoStore((s) => s.taskId);
  const close = useTaskInfoStore((s) => s.close);
  const task = useAppStore((s) =>
    taskId === null ? undefined : s.snapshot?.tasks.find((row) => row.id === taskId),
  );
  const dialogRef = useRef<HTMLDialogElement>(null);
  const baseId = useId();

  // Open / close the native dialog when the store says so.
  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog === null) return;
    if (taskId !== null && task !== undefined && !dialog.open) dialog.showModal();
    else if ((taskId === null || task === undefined) && dialog.open) dialog.close();
  }, [taskId, task]);

  // The task was deleted behind the dialog (e.g. by an undo): close it.
  useEffect(() => {
    if (taskId !== null && task === undefined) close();
  }, [taskId, task, close]);

  return (
    <dialog
      ref={dialogRef}
      aria-labelledby={`${baseId}-title`}
      className="m-auto w-[min(44rem,calc(100vw-2rem))] max-h-[calc(100vh-2rem)] rounded-lg border border-line bg-surface p-0 text-fg shadow-xl backdrop:bg-black/40"
      // Escape, or the window's own close: keep the store in step.
      onClose={close}
    >
      {taskId !== null && task !== undefined && (
        <TaskInfoForm key={taskId} task={task} titleId={`${baseId}-title`} />
      )}
    </dialog>
  );
}

/** Props of {@link TaskInfoForm}. */
interface TaskInfoFormProps {
  /** The task shown (from the latest snapshot). */
  task: TaskRow;
  /** The ID to give the title, which names the dialog for screen readers. */
  titleId: string;
}

/**
 * The dialog's form: tabs, fields, OK and Cancel.
 *
 * @param props - See {@link TaskInfoFormProps}.
 * @returns The form.
 */
function TaskInfoForm({ task, titleId }: TaskInfoFormProps) {
  const { t } = useTranslation();
  const close = useTaskInfoStore((s) => s.close);
  const snapshot = useAppStore((s) => s.snapshot);
  const calendars = useAppStore((s) => s.calendars);
  const units = useUnitContext(snapshot?.minutesPerDay ?? 480);
  const { dateLocale, numberLocale } = useFormatLocale();
  const tabRefs = useRef<Partial<Record<TabId, HTMLButtonElement | null>>>({});
  const baseId = useId();

  const isProject = task.isProject === true;
  const missedPriority = missedStartPriority(task, localToday());
  const tabs: readonly TabId[] = isProject ? PROJECT_TABS : TABS;
  // Read once: the tab asked for when the dialog opened.
  const [tab, setTab] = useState<TabId>(() => {
    const wanted = useTaskInfoStore.getState().tab;
    return tabs.includes(wanted) ? wanted : "general";
  });
  // The draft starts as a copy of the task (the function runs only once).
  const [draft, setDraft] = useState<TaskInfoDraft>(() => ({
    ...draftFromTask(task, units, units.formatNumber),
    blockers: draftBlockersFromTask(task, units),
    risks: draftRisksFromTask(task),
  }));
  const [problem, setProblem] = useState<ShownProblem | null>(null);
  const [busy, setBusy] = useState(false);

  /**
   * Changes some fields of the draft.
   *
   * @param patch - The changed fields.
   */
  function update(patch: Partial<TaskInfoDraft>): void {
    setDraft((d) => ({ ...d, ...patch }));
  }

  /** OK: check, send as one batch, close on success. */
  async function onOk(): Promise<void> {
    const result = commandsFromDraft(task, draft, units, () => crypto.randomUUID());
    if (!result.ok) {
      // Built from a field code, so TypeScript cannot check the key.
      setProblem({
        field: result.field,
        text: t(result.key as "grid.invalid.duration", result.values ?? {}),
      });
      setTab(FIELD_TAB[result.field]);
      return;
    }
    if (result.commands.length === 0) {
      close();
      return;
    }
    setBusy(true);
    const ok = await useAppStore.getState().applyCommands(result.commands);
    setBusy(false);
    if (ok) {
      close();
      return;
    }
    // Show Rust's reason inside the dialog (the banner is behind it).
    const shown = useAppStore.getState().commandError;
    setProblem({
      field: null,
      text:
        shown === null
          ? t("error.unknown")
          : "text" in shown
            ? shown.text
            : errorText(shown.error, t),
    });
  }

  /** ←/→, Home and End move between tabs (WAI-ARIA tabs pattern). */
  function onTabKeyDown(event: KeyboardEvent<HTMLButtonElement>): void {
    const index = tabs.indexOf(tab);
    let next: number;
    if (event.key === "ArrowRight") next = index + 1;
    else if (event.key === "ArrowLeft") next = index - 1;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = tabs.length - 1;
    else return;
    event.preventDefault();
    const target = tabs[(next + tabs.length) % tabs.length];
    if (target === undefined) return;
    setTab(target);
    tabRefs.current[target]?.focus();
  }

  const fieldError = (field: DraftField) => (problem?.field === field ? problem.text : null);
  const otherTasks = snapshot?.tasks ?? [];
  const blockers: DraftBlocker[] = draft.blockers ?? [];
  const risks: DraftRisk[] = draft.risks ?? [];

  /**
   * Changes some fields of one blocker in the draft.
   *
   * @param index - The blocker's place in the list.
   * @param patch - The changed fields.
   */
  function updateBlocker(index: number, patch: Partial<BlockerFormValues>): void {
    setDraft((d) => ({
      ...d,
      blockers: (d.blockers ?? []).map((b, j) =>
        j === index ? { ...b, values: { ...b.values, ...patch } } : b,
      ),
    }));
  }

  /**
   * The calculated dates of a blocker's striped piece, as text.
   *
   * @param id - The blocker's ID, or `null` for one added in this dialog.
   * @returns "start – finish", or `null` when there are no dates yet.
   */
  function blockedDates(id: string | null): string | null {
    const row = id === null ? undefined : task.blockers?.find((b) => b.id === id);
    const start = formatScheduleDate(row?.start ?? null, dateLocale);
    const finish = formatScheduleDate(row?.finish ?? null, dateLocale);
    return start === null || finish === null
      ? null
      : t("taskInfo.blockers.dates", { start, finish });
  }

  /** Total blocked days of the task as the schedule has it, for the summary line. */
  const blockedDays = (task.blockedMinutes ?? 0) / (snapshot?.minutesPerDay ?? 480);

  return (
    <form
      method="dialog"
      className="flex max-h-[calc(100vh-2rem)] flex-col"
      onSubmit={(e) => {
        // We send to Rust first; the dialog closes only on success.
        e.preventDefault();
        void onOk();
      }}
    >
      <h2 id={titleId} className="border-b border-line px-4 py-3 text-base font-semibold">
        {t(isProject ? "taskInfo.projectTitle" : "taskInfo.title", { name: task.name })}
      </h2>

      <div
        role="tablist"
        aria-label={t(isProject ? "taskInfo.projectTabsLabel" : "taskInfo.tabsLabel")}
        className="flex flex-wrap gap-1 border-b border-line px-3 pt-2"
      >
        {tabs.map((id) => (
          <button
            key={id}
            ref={(el) => {
              tabRefs.current[id] = el;
            }}
            type="button"
            role="tab"
            id={`${baseId}-tab-${id}`}
            aria-selected={tab === id}
            aria-controls={`${baseId}-panel`}
            tabIndex={tab === id ? 0 : -1}
            className="rounded-t border border-b-0 border-line px-3 py-1 text-sm whitespace-nowrap aria-selected:bg-app aria-selected:font-medium"
            onClick={() => setTab(id)}
            onKeyDown={onTabKeyDown}
          >
            {t(TAB_KEYS[id])}
          </button>
        ))}
      </div>

      <div
        role="tabpanel"
        id={`${baseId}-panel`}
        aria-labelledby={`${baseId}-tab-${tab}`}
        className="min-h-[16rem] flex-1 overflow-auto px-4 py-3 text-sm"
      >
        {tab === "general" && (
          <div className="grid grid-cols-[max-content_1fr] items-center gap-x-3 gap-y-2">
            <label htmlFor={`${baseId}-name`}>{t("taskInfo.name")}</label>
            <input
              id={`${baseId}-name`}
              className={FIELD_CLASS}
              value={draft.name}
              onChange={(e) => update({ name: e.target.value })}
            />
            <label htmlFor={`${baseId}-duration`}>{t("taskInfo.duration")}</label>
            <div className="flex flex-wrap items-center gap-2">
              <input
                id={`${baseId}-duration`}
                className={`${FIELD_CLASS} w-32`}
                value={draft.durationText}
                disabled={task.isSummary || isProject}
                aria-invalid={fieldError("duration") !== null}
                aria-describedby={`${baseId}-duration-hint`}
                onChange={(e) => update({ durationText: e.target.value })}
              />
              <span id={`${baseId}-duration-hint`} className="text-xs text-fg-muted">
                {isProject
                  ? t("taskInfo.projectDuration")
                  : task.isSummary
                    ? t("taskInfo.summaryDuration")
                    : t("taskInfo.durationHint")}
              </span>
            </div>
            {!isProject && (
              <>
                <label htmlFor={`${baseId}-mode`}>{t("taskInfo.mode")}</label>
                <select
                  id={`${baseId}-mode`}
                  className={`${FIELD_CLASS} w-40`}
                  value={draft.mode}
                  onChange={(e) => update(withMode(task, draft, e.target.value as TaskMode))}
                >
                  <option value="auto">{t(MODE_KEYS.auto)}</option>
                  <option value="manual">{t(MODE_KEYS.manual)}</option>
                </select>
                <span />
                <label className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={draft.milestone}
                    onChange={(e) => update({ milestone: e.target.checked })}
                  />
                  {t("taskInfo.milestone")}
                </label>
              </>
            )}
            <label htmlFor={`${baseId}-link`}>{t("taskInfo.hyperlink")}</label>
            <input
              id={`${baseId}-link`}
              type="text"
              inputMode="url"
              className={FIELD_CLASS}
              value={draft.hyperlink}
              onChange={(e) => update({ hyperlink: e.target.value })}
            />
          </div>
        )}

        {tab === "predecessors" && (
          <div className="flex flex-col gap-2">
            {draft.links.length === 0 ? (
              <p className="text-fg-muted">{t("taskInfo.noPredecessors")}</p>
            ) : (
              <table className="w-full border-collapse">
                <thead>
                  <tr className="text-left text-xs text-fg-muted">
                    <th className="py-1 pr-2 font-medium">{t("taskInfo.linkTask")}</th>
                    <th className="py-1 pr-2 font-medium">{t("taskInfo.linkType")}</th>
                    <th className="py-1 pr-2 font-medium">{t("taskInfo.linkLag")}</th>
                    <th className="py-1">
                      <span className="sr-only">{t("taskInfo.linkActions")}</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {draft.links.map((link, i) => {
                    const setLink = (patch: Partial<typeof link>) =>
                      update({
                        links: draft.links.map((l, j) => (j === i ? { ...l, ...patch } : l)),
                      });
                    const name = otherTasks.find((o) => o.id === link.predecessorId)?.name ?? "";
                    return (
                      <tr key={link.key}>
                        <td className="py-1 pr-2">
                          <select
                            aria-label={t("taskInfo.linkTaskRow", { row: i + 1 })}
                            className={`${FIELD_CLASS} w-full max-w-[18rem]`}
                            value={link.predecessorId}
                            onChange={(e) => setLink({ predecessorId: e.target.value })}
                          >
                            <option value="">{t("taskInfo.chooseTask")}</option>
                            {otherTasks.map((other, index) =>
                              other.id === task.id ? null : (
                                <option key={other.id} value={other.id}>
                                  {t("taskInfo.taskOption", {
                                    id: index + 1,
                                    name: other.name,
                                  })}
                                </option>
                              ),
                            )}
                          </select>
                        </td>
                        <td className="py-1 pr-2">
                          <select
                            aria-label={t("taskInfo.linkTypeRow", { row: i + 1 })}
                            className={FIELD_CLASS}
                            value={link.kind}
                            onChange={(e) => setLink({ kind: e.target.value as DependencyType })}
                          >
                            {DEPENDENCY_TYPES.map((kind) => (
                              <option key={kind} value={kind}>
                                {t(LINK_TYPE_KEYS[kind])}
                              </option>
                            ))}
                          </select>
                        </td>
                        <td className="py-1 pr-2">
                          <input
                            aria-label={t("taskInfo.linkLagRow", { row: i + 1 })}
                            className={`${FIELD_CLASS} w-24`}
                            value={link.lagText}
                            placeholder={t("taskInfo.lagHint")}
                            onChange={(e) => setLink({ lagText: e.target.value })}
                          />
                        </td>
                        <td className="py-1">
                          <button
                            type="button"
                            className="rounded border border-line px-2 py-0.5 whitespace-nowrap hover:bg-app"
                            aria-label={t("taskInfo.removeLink", {
                              name: name || String(i + 1),
                            })}
                            onClick={() => update({ links: draft.links.filter((_, j) => j !== i) })}
                          >
                            {t("taskInfo.remove")}
                          </button>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            )}
            <div>
              <button
                type="button"
                className="rounded border border-line px-3 py-1 whitespace-nowrap hover:bg-app"
                onClick={() =>
                  update({
                    links: [
                      ...draft.links,
                      {
                        key: crypto.randomUUID(),
                        predecessorId: "",
                        kind: "finishToStart",
                        lagText: "",
                      },
                    ],
                  })
                }
              >
                {t("taskInfo.addLink")}
              </button>
            </div>
            <p className="text-xs text-fg-muted">{t("taskInfo.lagExplain")}</p>
          </div>
        )}

        {tab === "dates" && (
          <div className="grid grid-cols-[max-content_1fr] items-center gap-x-3 gap-y-2">
            <label htmlFor={`${baseId}-start`}>{t("taskInfo.start")}</label>
            <DateField
              id={`${baseId}-start`}
              describedBy={`${baseId}-start-hint`}
              className={FIELD_CLASS}
              boxClassName="w-44"
              value={draft.start}
              disabled={!isEditable("start", task)}
              invalid={fieldError("start") !== null}
              onChange={(day) => update(withStart(task, draft, day))}
            />
            <p id={`${baseId}-start-hint`} className="col-start-2 text-xs text-fg-muted">
              {t(
                isProject
                  ? "taskInfo.startHintProject"
                  : task.isSummary
                    ? "taskInfo.startHintSummary"
                    : draft.mode === "manual"
                      ? "taskInfo.startHintManual"
                      : "taskInfo.startHint",
              )}
            </p>
            <span>{t("taskInfo.finish")}</span>
            <output aria-label={t("taskInfo.finish")}>
              {formatScheduleDate(task.finish, dateLocale) ?? t("taskInfo.noDate")}
            </output>
            <label htmlFor={`${baseId}-planned`}>{t("taskInfo.plannedFinish")}</label>
            <DateField
              id={`${baseId}-planned`}
              describedBy={`${baseId}-planned-hint`}
              className={FIELD_CLASS}
              boxClassName="w-44"
              value={draft.plannedFinish}
              disabled={task.isSummary || isProject}
              invalid={fieldError("plannedFinish") !== null}
              onChange={(day) => update({ plannedFinish: day })}
            />
            <p id={`${baseId}-planned-hint`} className="col-start-2 text-xs text-fg-muted">
              {t(
                task.isSummary || isProject
                  ? "taskInfo.plannedFinishHintGroup"
                  : "taskInfo.plannedFinishHint",
              )}
            </p>
            {isEditable("actualFinish", task) && (
              <>
                <label htmlFor={`${baseId}-actual`}>{t("taskInfo.actualFinish")}</label>
                <DateField
                  id={`${baseId}-actual`}
                  describedBy={`${baseId}-actual-hint`}
                  className={FIELD_CLASS}
                  boxClassName="w-44"
                  value={draft.actualFinish}
                  invalid={fieldError("actualFinish") !== null}
                  onChange={(day) => update({ actualFinish: day })}
                />
                <p id={`${baseId}-actual-hint`} className="col-start-2 text-xs text-fg-muted">
                  {t("taskInfo.actualFinishHint")}
                </p>
              </>
            )}
            {!isProject && (
              <>
                <label htmlFor={`${baseId}-ctype`}>{t("taskInfo.constraintType")}</label>
                <select
                  id={`${baseId}-ctype`}
                  aria-describedby={`${baseId}-chint`}
                  className={`${FIELD_CLASS} w-60`}
                  value={draft.constraintType}
                  // Choosing a constraint replaces a typed start on an automatic task.
                  onChange={(e) =>
                    update({
                      constraintType: e.target.value as Constraint["type"],
                      ...(draft.mode === "auto" ? { start: dayOf(task.start) } : {}),
                    })
                  }
                >
                  {CONSTRAINT_TYPES.map((type) => (
                    <option key={type} value={type}>
                      {t(CONSTRAINT_KEYS[type])}
                    </option>
                  ))}
                </select>
                <p id={`${baseId}-chint`} className="col-start-2 text-xs text-fg-muted">
                  {t("taskInfo.constraintHint")}
                </p>
                <label htmlFor={`${baseId}-cdate`}>{t("taskInfo.constraintDate")}</label>
                <DateField
                  id={`${baseId}-cdate`}
                  className={FIELD_CLASS}
                  boxClassName="w-44"
                  value={draft.constraintDate}
                  disabled={!needsDate(draft.constraintType)}
                  invalid={fieldError("constraintDate") !== null}
                  onChange={(day) =>
                    update({
                      constraintDate: day,
                      ...(draft.mode === "auto" ? { start: dayOf(task.start) } : {}),
                    })
                  }
                />
                <label htmlFor={`${baseId}-deadline`}>{t("taskInfo.deadline")}</label>
                <div className="flex flex-wrap items-center gap-2">
                  <DateField
                    id={`${baseId}-deadline`}
                    className={FIELD_CLASS}
                    boxClassName="w-44"
                    value={draft.deadline}
                    invalid={fieldError("deadline") !== null}
                    onChange={(day) => update({ deadline: day })}
                  />
                  {draft.deadline !== "" && (
                    <button
                      type="button"
                      className="rounded border border-line px-2 py-0.5 whitespace-nowrap hover:bg-app"
                      onClick={() => update({ deadline: "" })}
                    >
                      {t("taskInfo.clearDeadline")}
                    </button>
                  )}
                </div>
                <p className="col-start-2 text-xs text-fg-muted">{t("taskInfo.deadlineHint")}</p>
                <label htmlFor={`${baseId}-calendar`}>{t("taskInfo.calendar")}</label>
                <select
                  id={`${baseId}-calendar`}
                  className={`${FIELD_CLASS} w-60`}
                  value={draft.calendarId}
                  onChange={(e) => update({ calendarId: e.target.value })}
                >
                  <option value="">{t("taskInfo.projectCalendar")}</option>
                  {calendars.map((calendar) => (
                    <option key={calendar.id} value={calendar.id}>
                      {calendar.name}
                    </option>
                  ))}
                </select>
              </>
            )}
          </div>
        )}

        {tab === "notes" && (
          <div className="flex h-full flex-col gap-1">
            <label htmlFor={`${baseId}-notes`}>{t("taskInfo.notes")}</label>
            <textarea
              id={`${baseId}-notes`}
              className={`${FIELD_CLASS} min-h-[12rem] flex-1 resize-y`}
              value={draft.notes}
              onChange={(e) => update({ notes: e.target.value })}
            />
          </div>
        )}

        {tab === "blockers" && (
          <div className="flex flex-col gap-3">
            {isProject ? (
              <ProjectBlockers blockers={projectBlockers(otherTasks, task.id)} />
            ) : task.isSummary ? (
              <p className="text-fg-muted">{t("taskInfo.blockers.summary")}</p>
            ) : (
              <>
                <p className="text-xs text-fg-muted">{t("taskInfo.blockers.intro")}</p>
                {blockedDays > 0 && (
                  <p>
                    {t("taskInfo.blockers.total", {
                      days: formatNumber(blockedDays, numberLocale),
                      count: blockedDays,
                    })}
                  </p>
                )}
                {blockers.length === 0 && (
                  <p className="text-fg-muted">{t("taskInfo.blockers.none")}</p>
                )}
                {blockers.map((blocker, i) => {
                  const number = i + 1;
                  const fieldId = (name: string) => `${baseId}-blocker-${blocker.key}-${name}`;
                  const dates = blockedDates(blocker.id);
                  return (
                    <fieldset key={blocker.key} className="rounded border border-line px-3 py-2">
                      <legend className="px-1 font-medium">
                        {t("taskInfo.blockers.heading", { number })}
                      </legend>
                      <div className="mb-2 flex flex-wrap items-center gap-2">
                        <span className="text-xs text-fg-muted">
                          {dates ?? t("taskInfo.blockers.notScheduled")}
                        </span>
                        <button
                          type="button"
                          className="ml-auto rounded border border-line px-2 py-0.5 whitespace-nowrap hover:bg-app"
                          aria-label={t("taskInfo.blockers.removeLabel", { number })}
                          onClick={() => update({ blockers: blockers.filter((_, j) => j !== i) })}
                        >
                          {t("taskInfo.remove")}
                        </button>
                      </div>
                      <div className="grid grid-cols-[max-content_1fr] items-center gap-x-3 gap-y-2">
                        <label htmlFor={fieldId("start")}>{t("blocker.start")}</label>
                        <DateField
                          id={fieldId("start")}
                          className={FIELD_CLASS}
                          boxClassName="w-44"
                          value={blocker.values.startDate}
                          onChange={(day) => updateBlocker(i, { startDate: day })}
                        />
                        <label htmlFor={fieldId("length")}>{t("blocker.length")}</label>
                        <input
                          id={fieldId("length")}
                          className={`${FIELD_CLASS} w-32`}
                          value={blocker.values.lengthText}
                          placeholder={t("blocker.lengthHint")}
                          onChange={(e) => updateBlocker(i, { lengthText: e.target.value })}
                        />
                        <label htmlFor={fieldId("reason")}>{t("blocker.reason")}</label>
                        <input
                          id={fieldId("reason")}
                          className={FIELD_CLASS}
                          value={blocker.values.reason}
                          aria-required="true"
                          placeholder={t("blocker.reasonPlaceholder")}
                          onChange={(e) => updateBlocker(i, { reason: e.target.value })}
                        />
                        <label htmlFor={fieldId("reported")}>{t("blocker.reportedOn")}</label>
                        <DateField
                          id={fieldId("reported")}
                          className={FIELD_CLASS}
                          boxClassName="w-44"
                          value={blocker.values.reportedOn}
                          onChange={(day) => updateBlocker(i, { reportedOn: day })}
                        />
                      </div>
                    </fieldset>
                  );
                })}
                <div>
                  <button
                    type="button"
                    className="rounded border border-line px-3 py-1 whitespace-nowrap hover:bg-app"
                    onClick={() =>
                      // "Today" is read here, in the click handler, not while
                      // drawing (React components must stay pure).
                      update({
                        blockers: [...blockers, newDraftBlocker(crypto.randomUUID(), todayIso())],
                      })
                    }
                  >
                    {t("taskInfo.blockers.add")}
                  </button>
                </div>
              </>
            )}
          </div>
        )}

        {tab === "risks" && (
          <div className="flex flex-col gap-3">
            <p className="text-xs text-fg-muted">{t("risk.intro")}</p>
            {missedPriority !== null && (
              <p className="rounded border border-line bg-app px-3 py-2">
                {t("grid.indicator.riskMissedStart", {
                  priority: t(`risk.priority.${missedPriority}`),
                })}{" "}
                <span className="text-fg-muted">{t("taskInfo.risks.missedStartHint")}</span>
              </p>
            )}
            {isProject && <h3 className="font-semibold">{t("taskInfo.risks.projectHeading")}</h3>}
            {risks.length === 0 && missedPriority === null && (
              <p className="text-fg-muted">
                {t(isProject ? "taskInfo.risks.projectNone" : "taskInfo.risks.none")}
              </p>
            )}
            {risks.map((risk, i) => {
              const number = i + 1;
              return (
                <fieldset key={risk.key} className="rounded border border-line px-3 py-2">
                  <legend className="px-1 font-medium">
                    {t("taskInfo.risks.heading", { number })}
                  </legend>
                  <div className="mb-2 flex justify-end">
                    <button
                      type="button"
                      className="rounded border border-line px-2 py-0.5 whitespace-nowrap hover:bg-app"
                      aria-label={t("taskInfo.risks.removeLabel", { number })}
                      onClick={() => update({ risks: risks.filter((r) => r.key !== risk.key) })}
                    >
                      {t("taskInfo.remove")}
                    </button>
                  </div>
                  <RiskFields
                    values={risk.values}
                    onChange={(patch) =>
                      update({
                        risks: risks.map((r) =>
                          r.key === risk.key ? { ...r, values: { ...r.values, ...patch } } : r,
                        ),
                      })
                    }
                  />
                </fieldset>
              );
            })}
            <div>
              <button
                type="button"
                className="rounded border border-line px-3 py-1 whitespace-nowrap hover:bg-app"
                onClick={() =>
                  update({
                    risks: [...risks, { key: crypto.randomUUID(), id: null, values: EMPTY_RISK }],
                  })
                }
              >
                {t(isProject ? "taskInfo.risks.projectAdd" : "taskInfo.risks.add")}
              </button>
            </div>
            {isProject && (
              <TaskRisks
                risks={taskRisks(otherTasks, task.id)}
                missed={missedStarts(otherTasks, task.id, localToday())}
              />
            )}
          </div>
        )}
      </div>

      {problem !== null && (
        <p
          role="alert"
          className="border-t border-danger-line bg-danger-bg px-4 py-2 text-sm text-danger-fg"
        >
          {problem.text}
        </p>
      )}

      <div className="flex flex-wrap justify-end gap-2 border-t border-line px-4 py-3">
        <button
          type="button"
          className="rounded border border-line px-3 py-1 text-sm whitespace-nowrap hover:bg-app"
          onClick={close}
        >
          {t("taskInfo.cancel")}
        </button>
        <button
          type="submit"
          disabled={busy}
          className="rounded border border-accent bg-accent px-3 py-1 text-sm whitespace-nowrap text-accent-fg disabled:opacity-50"
        >
          {t("taskInfo.ok")}
        </button>
      </div>
    </form>
  );
}

/**
 * The Blockers tab of a project: the blockers of all its tasks. They are
 * edited on the tasks themselves.
 *
 * @param props - `blockers`: the list (see `projectBlockers`).
 * @returns A table, or a note that there are none.
 */
function ProjectBlockers({ blockers }: { blockers: readonly ProjectBlocker[] }) {
  const { t } = useTranslation();
  const { dateLocale, numberLocale } = useFormatLocale();
  const minutesPerDay = useAppStore((s) => s.snapshot?.minutesPerDay ?? 480);
  if (blockers.length === 0) {
    return <p className="text-fg-muted">{t("taskInfo.blockers.projectNone")}</p>;
  }
  return (
    <>
      <p className="text-xs text-fg-muted">{t("taskInfo.blockers.projectIntro")}</p>
      <table className="w-full border-collapse text-left">
        <thead>
          <tr className="text-xs text-fg-muted">
            <th className="py-1 pr-3 font-medium">{t("taskInfo.linkTask")}</th>
            <th className="py-1 pr-3 font-medium">{t("blocker.reason")}</th>
            <th className="py-1 pr-3 font-medium">{t("taskInfo.blockers.when")}</th>
            <th className="py-1 font-medium">{t("blocker.length")}</th>
          </tr>
        </thead>
        <tbody>
          {blockers.map(({ taskName, blocker }) => {
            const start = formatScheduleDate(blocker.start, dateLocale);
            const finish = formatScheduleDate(blocker.finish, dateLocale);
            const days = blocker.durationMinutes / minutesPerDay;
            return (
              <tr key={blocker.id} className="border-t border-line align-top">
                <td className="py-1 pr-3">{taskName}</td>
                <td className="py-1 pr-3">{blocker.reason}</td>
                <td className="py-1 pr-3">
                  {start === null || finish === null
                    ? ""
                    : t("taskInfo.blockers.dates", { start, finish })}
                </td>
                <td className="py-1">
                  {t("taskInfo.blockers.days", {
                    days: formatNumber(days, numberLocale),
                    count: days,
                  })}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </>
  );
}

/**
 * The lower part of a project's Risks tab: the risks of all its tasks, only
 * to read. They are edited on the tasks themselves.
 *
 * @param props - `risks`: the list (see `taskRisks`); `missed`: missed starts.
 * @returns A heading with a table, or with a note that there are none.
 */
function TaskRisks({
  risks,
  missed,
}: {
  risks: readonly TaskRisk[];
  missed: readonly MissedStart[];
}) {
  const { t } = useTranslation();
  return (
    <section className="mt-2 flex flex-col gap-2 border-t border-line pt-3">
      <h3 className="font-semibold">{t("taskInfo.risks.tasksHeading")}</h3>
      {risks.length + missed.length === 0 ? (
        <p className="text-fg-muted">{t("taskInfo.risks.tasksNone")}</p>
      ) : (
        <>
          <p className="text-xs text-fg-muted">{t("taskInfo.risks.tasksIntro")}</p>
          <table className="w-full border-collapse text-left">
            <thead>
              <tr className="text-xs text-fg-muted">
                <th className="py-1 pr-3 font-medium">{t("taskInfo.linkTask")}</th>
                <th className="py-1 pr-3 font-medium">{t("risk.description")}</th>
                <th className="py-1 pr-3 font-medium">{t("risk.priority.label")}</th>
                <th className="py-1 pr-3 font-medium">{t("risk.response.label")}</th>
                <th className="py-1 font-medium">{t("risk.consequence")}</th>
              </tr>
            </thead>
            <tbody>
              {risks.map(({ taskName, risk }) => (
                <tr key={risk.id} className="border-t border-line align-top">
                  <td className="py-1 pr-3">{taskName}</td>
                  <td className="py-1 pr-3">{risk.description}</td>
                  <td className="py-1 pr-3">{t(`risk.priority.${risk.priority}`)}</td>
                  <td className="py-1 pr-3">
                    {risk.response === "approved"
                      ? t("taskInfo.risks.approved")
                      : t("taskInfo.risks.mitigate", { mitigation: risk.mitigation })}
                  </td>
                  <td className="py-1">
                    {risk.noConsequence ? t("risk.noConsequence") : risk.consequence}
                  </td>
                </tr>
              ))}
              {missed.map((m) => (
                <tr key={`missed-${m.task.id}`} className="border-t border-line align-top">
                  <td className="py-1 pr-3">{m.task.name}</td>
                  <td className="py-1 pr-3">{t("risk.missedStart.description")}</td>
                  <td className="py-1 pr-3">{t(`risk.priority.${m.priority}`)}</td>
                  <td className="py-1 pr-3">{t("risk.missedStart.response")}</td>
                  <td className="py-1">{t("risk.missedStart.consequence")}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </section>
  );
}
