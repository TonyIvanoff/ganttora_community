/**
 * features/task-info/RiskDialog.tsx: the small "Add risk" dialog.
 *
 * What it is for:
 *   Records something that could go wrong with the selected task, group or
 *   project. "Add risk" sends one `addRisk` command; Cancel or Escape
 *   closes without changes.
 *
 * Where it fits:
 *   Rendered once by `shell/AppShell.tsx`; opened through `riskStore.ts`.
 *   Built like `BlockerDialog.tsx`; the fields are `RiskFields.tsx`, the
 *   checks are in `risks.ts`. To edit or remove a risk, the user opens the
 *   Risks tab of Task information.
 */

import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TaskRow } from "@frontend/ipc/types";
import { errorText } from "@frontend/lib/errors";
import { useAppStore } from "@frontend/state/store";
import { RiskFields } from "./RiskFields";
import { EMPTY_RISK, parseRiskForm, type RiskField, type RiskFormValues } from "./risks";
import { useRiskDialogStore } from "./riskStore";

/** Translation keys of the problems, per field. */
const PROBLEM_KEYS = {
  description: "risk.invalid.description",
  mitigation: "risk.invalid.mitigation",
  consequence: "risk.invalid.consequence",
} as const satisfies Record<RiskField, string>;

/**
 * The "Add risk" dialog. Renders nothing visible while closed.
 *
 * @returns The `<dialog>` element; its form exists only while it is open.
 */
export function RiskDialog() {
  const taskId = useRiskDialogStore((s) => s.taskId);
  const close = useRiskDialogStore((s) => s.close);
  const task = useAppStore((s) =>
    taskId === null ? undefined : s.snapshot?.tasks.find((row) => row.id === taskId),
  );
  const dialogRef = useRef<HTMLDialogElement>(null);
  const titleId = useId();

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
      aria-labelledby={titleId}
      className="m-auto max-h-[calc(100vh-2rem)] w-[min(34rem,calc(100vw-2rem))] rounded-lg border border-line bg-surface p-0 text-fg shadow-xl backdrop:bg-black/40"
      onClose={close}
    >
      {taskId !== null && task !== undefined && (
        <RiskForm key={taskId} task={task} titleId={titleId} />
      )}
    </dialog>
  );
}

/**
 * The dialog's form: the fields, Cancel and "Add risk".
 *
 * @param props - `task`: the row that gets the risk; `titleId`: the ID to
 *   give the title (names the dialog for screen readers).
 * @returns The form.
 */
function RiskForm({ task, titleId }: { task: TaskRow; titleId: string }) {
  const { t } = useTranslation();
  const close = useRiskDialogStore((s) => s.close);
  const [values, setValues] = useState<RiskFormValues>(EMPTY_RISK);
  /** The field with a problem (`null` for an error from Rust) and its message. */
  const [problem, setProblem] = useState<{ field: RiskField | null; text: string } | null>(null);
  const [busy, setBusy] = useState(false);

  /** "Add risk": check, send one command, close on success. */
  async function onOk(): Promise<void> {
    const parsed = parseRiskForm(values);
    if (!parsed.ok) {
      setProblem({ field: parsed.field, text: t(PROBLEM_KEYS[parsed.field]) });
      return;
    }
    setBusy(true);
    const ok = await useAppStore.getState().applyCommand({
      type: "addRisk",
      task: task.id,
      risk: { id: crypto.randomUUID(), ...parsed.value },
    });
    setBusy(false);
    if (ok) {
      close();
      return;
    }
    // Rust refused: show why here, the window's banner is behind the dialog.
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
        {t("risk.dialogTitle", { name: task.name })}
      </h2>
      <div className="flex flex-col gap-3 overflow-auto px-4 py-3 text-sm">
        <p className="text-fg-muted">{t("risk.intro")}</p>
        <RiskFields
          values={values}
          invalid={problem?.field ?? null}
          onChange={(patch) => setValues((v) => ({ ...v, ...patch }))}
        />
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
          {t("risk.cancel")}
        </button>
        <button
          type="submit"
          disabled={busy}
          className="rounded border border-accent bg-accent px-3 py-1 text-sm whitespace-nowrap text-accent-fg disabled:opacity-50"
        >
          {t("risk.ok")}
        </button>
      </div>
    </form>
  );
}
