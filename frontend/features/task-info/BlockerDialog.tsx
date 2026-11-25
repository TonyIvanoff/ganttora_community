/**
 * features/task-info/BlockerDialog.tsx: the small "Add blocker" dialog
 *
 * What it is for:
 *   Records an unplanned delay on the selected task, e.g. "the wood
 *   delivery is two weeks late". Four fields:
 *   - **Blocked from:** the day the task is stuck from (default today);
 *   - **Length:** how long, in working days ("5", "2w", "4h" also work);
 *   - **Reason:** why (required, so everyone sees it later);
 *   - **Reported on:** the day the delay became known (default today).
 *   "Add blocker" sends one `addBlockerAt` command; Cancel or Escape closes
 *   without changes. The task gets longer by the blocked time, its
 *   successors move later, and the Gantt chart draws the blocked time
 *   striped in red.
 *
 * Where it fits:
 *   Rendered once by `shell/AppShell.tsx`; opened through
 *   `blockerStore.ts` (toolbar button, grid context menu, Ctrl/⌘+Shift+B).
 *   The checks and the command come from `blockers.ts`. Rust refuses a
 *   blocker on a summary task (`blockerOnSummary`); the dialog then stays
 *   open and shows the translated reason.
 *
 * Main ideas:
 *   - The native `<dialog>` element with `showModal()`, like the Task
 *     Information dialog: focus stays inside, Escape closes.
 *   - The form is rendered only while the dialog is open, with the task ID
 *     as `key`, so every opening starts with fresh fields.
 */

import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { DateField } from "@frontend/components/ui/DateField";
import { useUnitContext } from "@frontend/features/grid/useUnitContext";
import type { TaskRow } from "@frontend/ipc/types";
import { errorText } from "@frontend/lib/errors";
import { useAppStore } from "@frontend/state/store";
import {
  addBlockerCommand,
  parseBlockerForm,
  type BlockerField,
  type BlockerFormValues,
} from "./blockers";
import { useBlockerDialogStore } from "./blockerStore";

/** Shared look of fields (same as the Task Information dialog). */
const FIELD_CLASS =
  "rounded border border-line bg-surface px-2 py-1 text-sm text-fg disabled:opacity-50";

/** Translation keys of the problems, per field. */
const PROBLEM_KEYS = {
  start: "blocker.invalid.start",
  length: "blocker.invalid.length",
  reason: "blocker.invalid.reason",
  reportedOn: "blocker.invalid.reportedOn",
} as const satisfies Record<BlockerField, string>;

/**
 * The "Add blocker" dialog. Renders nothing visible while closed.
 *
 * @returns The `<dialog>` element; its form exists only while it is open.
 */
export function BlockerDialog() {
  const taskId = useBlockerDialogStore((s) => s.taskId);
  const close = useBlockerDialogStore((s) => s.close);
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

  // The task disappeared behind the dialog (e.g. an undo): close it.
  useEffect(() => {
    if (taskId !== null && task === undefined) close();
  }, [taskId, task, close]);

  return (
    <dialog
      ref={dialogRef}
      aria-labelledby={`${baseId}-title`}
      className="m-auto w-[min(32rem,calc(100vw-2rem))] max-h-[calc(100vh-2rem)] rounded-lg border border-line bg-surface p-0 text-fg shadow-xl backdrop:bg-black/40"
      onClose={close}
    >
      {taskId !== null && task !== undefined && (
        <BlockerForm key={taskId} task={task} titleId={`${baseId}-title`} />
      )}
    </dialog>
  );
}

/** Props of {@link BlockerForm}. */
interface BlockerFormProps {
  /** The task that gets the blocker. */
  task: TaskRow;
  /** The ID to give the title (names the dialog for screen readers). */
  titleId: string;
}

/**
 * The dialog's form: four fields, Cancel and "Add blocker".
 *
 * @param props - See {@link BlockerFormProps}.
 * @returns The form.
 */
function BlockerForm({ task, titleId }: BlockerFormProps) {
  const { t } = useTranslation();
  const close = useBlockerDialogStore((s) => s.close);
  const today = useBlockerDialogStore((s) => s.today);
  const snapshot = useAppStore((s) => s.snapshot);
  const units = useUnitContext(snapshot?.minutesPerDay ?? 480);
  const baseId = useId();

  const [values, setValues] = useState<BlockerFormValues>({
    startDate: today,
    lengthText: "",
    reason: "",
    reportedOn: today,
  });
  /** The field with a problem (`null` for an error from Rust) and its message. */
  const [problem, setProblem] = useState<{ field: BlockerField | null; text: string } | null>(null);
  const [busy, setBusy] = useState(false);

  /**
   * Changes some fields.
   *
   * @param patch - The changed fields.
   */
  function update(patch: Partial<BlockerFormValues>): void {
    setValues((v) => ({ ...v, ...patch }));
  }

  /** "Add blocker": check, send one command, close on success. */
  async function onOk(): Promise<void> {
    const parsed = parseBlockerForm(values, units);
    if (!parsed.ok) {
      setProblem({ field: parsed.field, text: t(PROBLEM_KEYS[parsed.field]) });
      return;
    }
    setBusy(true);
    const ok = await useAppStore
      .getState()
      .applyCommand(addBlockerCommand(task.id, crypto.randomUUID(), parsed.value));
    setBusy(false);
    if (ok) {
      close();
      return;
    }
    // Rust refused (e.g. a summary task): show the reason in the dialog,
    // because the window's error banner is hidden behind it.
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

  /** `true` when `field` has the current problem (for `aria-invalid`). */
  const invalid = (field: BlockerField) => problem?.field === field;

  return (
    <form
      method="dialog"
      className="flex max-h-[calc(100vh-2rem)] flex-col"
      onSubmit={(e) => {
        // Rust first; the dialog closes only on success.
        e.preventDefault();
        void onOk();
      }}
    >
      <h2 id={titleId} className="border-b border-line px-4 py-3 text-base font-semibold">
        {t("blocker.dialogTitle", { name: task.name })}
      </h2>
      <div className="flex flex-col gap-3 overflow-auto px-4 py-3 text-sm">
        <p className="text-fg-muted">{t("blocker.intro")}</p>
        <div className="grid grid-cols-[max-content_1fr] items-center gap-x-3 gap-y-2">
          <label htmlFor={`${baseId}-start`}>{t("blocker.start")}</label>
          <div className="flex flex-col gap-0.5">
            <DateField
              id={`${baseId}-start`}
              className={FIELD_CLASS}
              boxClassName="w-44"
              value={values.startDate}
              invalid={invalid("start")}
              describedBy={`${baseId}-start-hint`}
              onChange={(day) => update({ startDate: day })}
            />
            <span id={`${baseId}-start-hint`} className="text-xs text-fg-muted">
              {t("blocker.startHint")}
            </span>
          </div>
          <label htmlFor={`${baseId}-length`}>{t("blocker.length")}</label>
          <div className="flex flex-col gap-0.5">
            <input
              id={`${baseId}-length`}
              className={`${FIELD_CLASS} w-32`}
              value={values.lengthText}
              aria-invalid={invalid("length")}
              aria-describedby={`${baseId}-length-hint`}
              // The first field to type into: the date already has a default.
              autoFocus
              onChange={(e) => update({ lengthText: e.target.value })}
            />
            <span id={`${baseId}-length-hint`} className="text-xs text-fg-muted">
              {t("blocker.lengthHint")}
            </span>
          </div>
          <label htmlFor={`${baseId}-reason`}>{t("blocker.reason")}</label>
          <input
            id={`${baseId}-reason`}
            className={FIELD_CLASS}
            value={values.reason}
            placeholder={t("blocker.reasonPlaceholder")}
            aria-invalid={invalid("reason")}
            aria-required="true"
            onChange={(e) => update({ reason: e.target.value })}
          />
          <label htmlFor={`${baseId}-reported`}>{t("blocker.reportedOn")}</label>
          <DateField
            id={`${baseId}-reported`}
            className={FIELD_CLASS}
            boxClassName="w-44"
            value={values.reportedOn}
            invalid={invalid("reportedOn")}
            onChange={(day) => update({ reportedOn: day })}
          />
        </div>
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
          {t("blocker.cancel")}
        </button>
        <button
          type="submit"
          disabled={busy}
          className="rounded border border-accent bg-accent px-3 py-1 text-sm whitespace-nowrap text-accent-fg disabled:opacity-50"
        >
          {t("blocker.ok")}
        </button>
      </div>
    </form>
  );
}
