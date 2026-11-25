/**
 * tests/frontend/lib/errors.test.ts: tests for turning error codes into messages.
 *
 * What it checks:
 *   `frontend/lib/errors.ts`:
 *   - `isIpcError` recognises `{ code: "…" }` objects and nothing else;
 *   - `errorChain` follows nested errors (`rejected` → `cycleDetected`);
 *   - `errorParams` turns `details` into message values (lists joined
 *     with arrows);
 *   - `errorKey` picks the most specific translated key (`error.<code>`),
 *     including the file errors `error.gtra.*`, and falls back to
 *     `error.unknown`;
 *   - `errorText` fills the values into the English message.
 *
 * Why it matters:
 *   Since M1, Rust never sends English sentences, only codes such as
 *   `{ code: "rejected", details: { code: "cycleDetected", … } }`. If the
 *   code-to-message step breaks, the user sees "Something went wrong"
 *   instead of "These tasks would depend on each other in a loop: A → B".
 *
 * Main ideas:
 *   The English messages come from `locales/en.json`, which `@frontend/i18n`
 *   loads when it is imported. Tests that need a specific set of known keys
 *   pass their own `exists` function instead.
 */

import { describe, expect, it } from "vitest";
import i18next from "@frontend/i18n";
import { errorChain, errorKey, errorParams, errorText, isIpcError } from "@frontend/lib/errors";

/** The error the document sends when an edit would create a loop. */
const rejectedCycle = {
  code: "rejected",
  details: {
    code: "cycleDetected",
    details: { tasks: ["id-a", "id-b"], names: ["Design", "Permit"] },
  },
};

describe("isIpcError", () => {
  // Only objects with a text `code` count; strings, null, numbers and
  // objects with a non-text code do not.
  it("accepts only objects with a text code", () => {
    expect(isIpcError({ code: "taskNotFound" })).toBe(true);
    expect(isIpcError({ code: "gtra.io", detail: "disk full" })).toBe(true);
    expect(isIpcError("taskNotFound")).toBe(false);
    expect(isIpcError(null)).toBe(false);
    expect(isIpcError(undefined)).toBe(false);
    expect(isIpcError(42)).toBe(false);
    expect(isIpcError({ code: 7 })).toBe(false);
    expect(isIpcError({ message: "x" })).toBe(false);
  });
});

describe("errorChain", () => {
  // `rejected` wraps the engine's `cycleDetected`; the chain lists both,
  // outermost first.
  it("follows nested codes", () => {
    expect(errorChain(rejectedCycle).map((e) => e.code)).toEqual(["rejected", "cycleDetected"]);
  });

  // Two levels: calendar → noWorkingTime (ScheduleError::Calendar).
  it("follows two levels below rejected", () => {
    const error = {
      code: "rejected",
      details: { code: "calendar", details: { code: "noWorkingTime", details: { calendar: "c" } } },
    };
    expect(errorChain(error).map((e) => e.code)).toEqual(["rejected", "calendar", "noWorkingTime"]);
  });

  // Something that is not a coded error gives an empty chain.
  it("is empty for non-errors", () => {
    expect(errorChain("boom")).toEqual([]);
    expect(errorChain(null)).toEqual([]);
  });

  // A loop of self-references must not hang: the chain stops at 8.
  it("stops after 8 levels", () => {
    const loop: { code: string; details?: unknown } = { code: "rejected" };
    loop.details = loop;
    expect(errorChain(loop)).toHaveLength(8);
  });
});

describe("errorParams", () => {
  // Text and numbers are kept; lists are joined with " → " (a cycle reads
  // "Design → Permit"); other values are dropped.
  it("keeps text, numbers and lists", () => {
    expect(
      errorParams({ names: ["Design", "Permit"], found: 3, path: "/x.gtra", nested: { a: 1 } }),
    ).toEqual({ names: "Design → Permit", found: 3, path: "/x.gtra" });
  });

  // A bare value (e.g. the task ID of `taskNotFound`) becomes `{ value }`.
  it("wraps a bare value", () => {
    expect(errorParams("0b7d")).toEqual({ value: "0b7d" });
    expect(errorParams(5)).toEqual({ value: 5 });
  });

  // Nothing usable gives no values.
  it("ignores missing or list details", () => {
    expect(errorParams(undefined)).toEqual({});
    expect(errorParams(null)).toEqual({});
    expect(errorParams(["a"])).toEqual({});
  });
});

describe("errorKey", () => {
  // The innermost code with a message wins: the user learns *why* the edit
  // was refused (a loop), not only that it was refused.
  it("prefers the most specific known code", () => {
    expect(errorKey(rejectedCycle)).toBe("error.cycleDetected");
  });

  // If the inner code has no message, the outer one is used.
  it("falls back to the outer code", () => {
    const exists = (key: string) => key === "error.rejected";
    expect(errorKey(rejectedCycle, exists)).toBe("error.rejected");
  });

  // File errors use dotted codes; they map to the nested `error.gtra.*` keys.
  it("maps file error codes to error.gtra.* keys", () => {
    expect(errorKey({ code: "gtra.newer_format", detail: "" })).toBe("error.gtra.newer_format");
    expect(errorKey({ code: "gtra.file_not_found", detail: "" })).toBe("error.gtra.file_not_found");
    expect(errorKey({ code: "gtra.no_save_path", detail: "" })).toBe("error.gtra.no_save_path");
  });

  // Every Rust GtraError code has an English message (none falls back to
  // "unknown"), so a file problem is always explained.
  it("has a message for every gtra code", () => {
    const codes = [
      "file_not_found",
      "permission_denied",
      "io",
      "database",
      "not_a_gtra_file",
      "file_exists",
      "invalid_path",
      "newer_format",
      "read_only",
      "migration_failed",
      "invalid_data",
      "invalid_project",
      "invalid_record",
      "no_save_path",
      "upgraded",
    ];
    for (const code of codes) {
      expect(errorKey({ code: `gtra.${code}` })).toBe(`error.gtra.${code}`);
    }
  });

  // Unknown codes and non-errors give the generic message key.
  it("falls back to error.unknown", () => {
    expect(errorKey({ code: "somethingNew" })).toBe("error.unknown");
    expect(errorKey("plain text")).toBe("error.unknown");
    expect(errorKey(undefined)).toBe("error.unknown");
  });
});

describe("errorText", () => {
  // The values from `details` are filled into the English message.
  it("fills in the names of a cycle", () => {
    const text = errorText(rejectedCycle, i18next.t);
    expect(text).toContain("Design → Permit");
  });

  // The newer-format message names both versions.
  it("fills in the versions of a newer file", () => {
    const text = errorText(
      { code: "gtra.newer_format", detail: "", details: { found: 3, supported: 2 } },
      i18next.t,
    );
    expect(text).toContain("3");
    expect(text).toContain("2");
  });

  // Anything unknown gives the generic sentence, never an empty string.
  it("never returns empty text", () => {
    expect(errorText({ code: "somethingNew" }, i18next.t)).toBe(i18next.t("error.unknown"));
  });
});
