/**
 * features/task-info/RiskFields.tsx: the fields of one risk.
 *
 * What it is for:
 *   Description, priority, response (mitigate or approved), mitigation note
 *   and consequence. The note is shown only for "mitigate"; the consequence
 *   is switched off when "No consequence" is ticked.
 *
 * Where it fits:
 *   Used by `RiskDialog.tsx` and by the Risks tab of `TaskInfoDialog.tsx`,
 *   so both always look and work the same. The checks are in `risks.ts`.
 */

import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { RiskPriority, RiskResponse } from "@frontend/ipc/types";
import { RISK_PRIORITIES, type RiskField, type RiskFormValues } from "./risks";

/** Shared look of fields (same as the Task Information dialog). */
const FIELD_CLASS =
  "rounded border border-line bg-surface px-2 py-1 text-sm text-fg disabled:opacity-50";

/** Translation keys of the priorities. */
const PRIORITY_KEYS = {
  low: "risk.priority.low",
  medium: "risk.priority.medium",
  high: "risk.priority.high",
} as const satisfies Record<RiskPriority, string>;

/** The responses, in order, with their translation keys. */
const RESPONSES = [
  ["mitigate", "risk.response.mitigate"],
  ["approved", "risk.response.approved"],
] as const satisfies readonly (readonly [RiskResponse, string])[];

/** Props of {@link RiskFields}. */
interface RiskFieldsProps {
  /** The values shown. */
  values: RiskFormValues;
  /** The field with a problem, if any. */
  invalid?: RiskField | null;
  /** Called with the changed fields. */
  onChange: (patch: Partial<RiskFormValues>) => void;
}

/**
 * The fields of one risk, as two columns (label, field).
 *
 * @param props - See {@link RiskFieldsProps}.
 * @returns The fields.
 */
export function RiskFields({ values, invalid = null, onChange }: RiskFieldsProps) {
  const { t } = useTranslation();
  const id = useId();
  return (
    <div className="grid grid-cols-[max-content_1fr] items-center gap-x-3 gap-y-2">
      <label htmlFor={`${id}-description`}>{t("risk.description")}</label>
      <input
        id={`${id}-description`}
        className={FIELD_CLASS}
        value={values.description}
        placeholder={t("risk.descriptionPlaceholder")}
        aria-required="true"
        aria-invalid={invalid === "description"}
        onChange={(e) => onChange({ description: e.target.value })}
      />
      <label htmlFor={`${id}-priority`}>{t("risk.priority.label")}</label>
      <select
        id={`${id}-priority`}
        className={`${FIELD_CLASS} w-44`}
        value={values.priority}
        onChange={(e) => onChange({ priority: e.target.value as RiskPriority })}
      >
        {RISK_PRIORITIES.map((priority) => (
          <option key={priority} value={priority}>
            {t(PRIORITY_KEYS[priority])}
          </option>
        ))}
      </select>
      <span id={`${id}-response`}>{t("risk.response.label")}</span>
      <div role="radiogroup" aria-labelledby={`${id}-response`} className="flex flex-col gap-1">
        {RESPONSES.map(([response, key]) => (
          <label key={response} className="flex items-center gap-2">
            <input
              type="radio"
              name={`${id}-response`}
              checked={values.response === response}
              onChange={() => onChange({ response })}
            />
            {t(key)}
          </label>
        ))}
      </div>
      {values.response === "mitigate" && (
        <>
          <label htmlFor={`${id}-mitigation`}>{t("risk.mitigation")}</label>
          <textarea
            id={`${id}-mitigation`}
            rows={2}
            className={FIELD_CLASS}
            value={values.mitigation}
            placeholder={t("risk.mitigationPlaceholder")}
            aria-required="true"
            aria-invalid={invalid === "mitigation"}
            onChange={(e) => onChange({ mitigation: e.target.value })}
          />
        </>
      )}
      <label htmlFor={`${id}-consequence`}>{t("risk.consequence")}</label>
      <div className="flex flex-col gap-1">
        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={values.noConsequence}
            onChange={(e) => onChange({ noConsequence: e.target.checked })}
          />
          {t("risk.noConsequence")}
        </label>
        <textarea
          id={`${id}-consequence`}
          rows={2}
          className={FIELD_CLASS}
          value={values.noConsequence ? "" : values.consequence}
          placeholder={values.noConsequence ? "" : t("risk.consequencePlaceholder")}
          disabled={values.noConsequence}
          aria-required={!values.noConsequence}
          aria-invalid={invalid === "consequence"}
          onChange={(e) => onChange({ consequence: e.target.value })}
        />
      </div>
    </div>
  );
}
