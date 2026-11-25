/**
 * Tests of the risk form (`risks.ts`).
 *
 * Why it matters: a risk must say how it is mitigated and what its
 * consequence is, unless the user explicitly chose otherwise.
 */

import { describe, expect, it } from "vitest";
import {
  draftRisksFromTask,
  EMPTY_RISK,
  parseRiskForm,
  riskCommands,
  RISK_PRIORITIES,
} from "@frontend/features/task-info/risks";
import type { Risk, TaskRow } from "@frontend/ipc/types";

const FULL = {
  ...EMPTY_RISK,
  description: "Late wood",
  mitigation: "Order early",
  consequence: "Wall later",
};
const RISK: Risk = { id: "r1", ...FULL };
const task = (risks: Risk[]) => ({ id: "t1", risks }) as unknown as TaskRow;
const newId = () => "new";

describe("parseRiskForm", () => {
  it("asks for the description, the mitigation and the consequence", () => {
    expect(parseRiskForm({ ...FULL, description: " " })).toEqual({
      ok: false,
      field: "description",
    });
    expect(parseRiskForm({ ...FULL, mitigation: "" })).toEqual({ ok: false, field: "mitigation" });
    expect(parseRiskForm({ ...FULL, consequence: "" })).toEqual({
      ok: false,
      field: "consequence",
    });
  });

  it("needs no mitigation when approved and no consequence when ticked", () => {
    const result = parseRiskForm({ ...FULL, response: "approved", noConsequence: true });
    expect(result).toMatchObject({ ok: true, value: { mitigation: "", consequence: "" } });
  });

  it("lists the priorities from low to high (the icon shows the last match)", () => {
    expect(RISK_PRIORITIES).toEqual(["low", "medium", "high"]);
  });
});

describe("riskCommands", () => {
  it("sends nothing for untouched risks, even incomplete ones", () => {
    const row = task([{ ...RISK, consequence: "" }]);
    expect(riskCommands(row, draftRisksFromTask(row), newId)).toEqual({ ok: true, commands: [] });
  });

  it("adds, edits and removes", () => {
    const row = task([RISK, { ...RISK, id: "r2" }]);
    const drafts = draftRisksFromTask(row).slice(0, 1);
    const first = drafts[0];
    if (first === undefined) throw new Error("no draft");
    const result = riskCommands(
      row,
      [
        { ...first, values: { ...first.values, priority: "high" } },
        { key: "k", id: null, values: FULL },
      ],
      newId,
    );
    expect(result).toEqual({
      ok: true,
      commands: [
        { type: "removeRisk", task: "t1", id: "r2" },
        { type: "editRisk", task: "t1", risk: { ...RISK, priority: "high" } },
        { type: "addRisk", task: "t1", risk: { ...FULL, id: "new" } },
      ],
    });
  });

  it("names the incomplete risk", () => {
    const result = riskCommands(task([]), [{ key: "k", id: null, values: EMPTY_RISK }], newId);
    expect(result).toEqual({ ok: false, number: 1, field: "description" });
  });
});
