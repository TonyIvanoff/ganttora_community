/**
 * lib/errors.ts: turns errors from Rust into translated messages.
 *
 * What it is for:
 *   Rust only speaks English, but the UI is shown in nine languages. So a
 *   Rust command that fails sends an *error code* (an `IpcError`, e.g.
 *   `{ code: "settingsSaveFailed", detail: "…" }`), and this helper picks
 *   the matching sentence from the translation file: the key
 *   `error.settingsSaveFailed` in `locales/en.json`. The same works for the
 *   schedule's own error (`snapshot.error`, `{ code, details }`).
 *
 * Where it fits:
 *   Called wherever a rejected IPC promise or a schedule error is shown to
 *   the user: the store's error banner (`state/store.ts`, `shell/AppShell.tsx`),
 *   the Settings dialog, the file banners. The Rust side of the codes is
 *   `IpcError` in `backend/src-tauri/src/ipc/mod.rs`.
 *
 * Main ideas:
 *   - **Never show Rust's English text as the message.** The `detail` is
 *     written to the developer console instead.
 *   - **Key scheme.** The key is always `error.<code>`:
 *       - shell, settings, document and engine codes are camelCase:
 *         `error.noProjectOpen`, `error.cycleDetected`;
 *       - file-format codes from `gtra` contain a dot:
 *         `error.gtra.newer_format`. i18next reads dots as *nesting*, so in
 *         `en.json` they live in an `error.gtra` object. This keeps the
 *         codes exactly as Rust sends them.
 *   - **Nested codes.** A refused edit arrives as
 *     `{ code: "rejected", details: { code: "cycleDetected", details: {…} } }`.
 *     We walk down the `details` chain and use the deepest code that has a
 *     translation, so the user reads "These tasks depend on each other in a
 *     loop" instead of a vague "The change was refused".
 *   - **Message values.** The innermost `details` object fills the
 *     message's placeholders (`{names}`, `{found}`…). Lists are joined with
 *     arrows ("A → B → A").
 *   - An unknown code, or a non-coded error (a JavaScript `Error`), gets the
 *     general message `error.unknown`.
 */

import type { TFunction } from "i18next";
import i18next from "@frontend/i18n";
import type { CodedError } from "@frontend/ipc/types";

/** Joins list values in a message, e.g. the task names of a loop. */
const LIST_SEPARATOR = " → ";

/**
 * Checks whether a value carries an error code: an `IpcError` from a
 * command or a `CodedError` from the core (an object with a text `code`).
 *
 * @param value - Anything a rejected promise may carry.
 * @returns `true` if `value` has a string `code` field.
 *
 * @example
 * isIpcError({ code: "settingsSaveFailed", detail: "" }); // true
 * isIpcError("no project is open");                       // false
 *
 * Beginner note: the return type `value is CodedError` is a *type guard*.
 * After `if (isIpcError(x))`, TypeScript knows that `x` has a `code`
 * inside the `if` block.
 */
export function isIpcError(value: unknown): value is CodedError {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    typeof (value as { code: unknown }).code === "string"
  );
}

/**
 * Lists the chain of coded errors from the outside in.
 *
 * @param error - What a rejected promise carried, or a `snapshot.error`.
 * @returns The coded errors, outermost first; empty if `error` has no code.
 *
 * @example
 * errorChain({ code: "rejected", details: { code: "cycleDetected", details: {} } })
 *   .map((e) => e.code); // ["rejected", "cycleDetected"]
 */
export function errorChain(error: unknown): CodedError[] {
  const chain: CodedError[] = [];
  let current: unknown = error;
  // A small limit protects against a (theoretical) endless chain.
  while (isIpcError(current) && chain.length < 8) {
    chain.push(current);
    current = current.details;
  }
  return chain;
}

/**
 * Turns an error's `details` into values for an ICU message.
 *
 * @param details - The innermost `details` (an object, a text, or nothing).
 * @returns Name → value pairs: text and numbers as they are, lists of text
 *   joined with arrows; a plain text `details` becomes `{ value }`.
 *
 * @example
 * errorParams({ names: ["A", "B"], found: 3 }); // { names: "A → B", found: 3 }
 */
export function errorParams(details: unknown): Record<string, string | number> {
  if (typeof details === "string" || typeof details === "number") return { value: details };
  if (typeof details !== "object" || details === null || Array.isArray(details)) return {};
  const params: Record<string, string | number> = {};
  for (const [name, value] of Object.entries(details)) {
    if (typeof value === "string" || typeof value === "number") params[name] = value;
    else if (Array.isArray(value)) params[name] = value.map(String).join(LIST_SEPARATOR);
  }
  return params;
}

/**
 * Gives the translation key for an error, plus the coded error it belongs to.
 *
 * @param error - What a rejected IPC promise carried.
 * @param exists - Tells whether a key exists in the translation files.
 *   Defaults to i18next's own check; tests can pass their own.
 * @returns `"error.<code>"` for the deepest code in the chain that has a
 *   translation, otherwise `"error.unknown"`.
 *
 * @example
 * errorKey({ code: "settingsSaveFailed", detail: "" }); // "error.settingsSaveFailed"
 * errorKey({ code: "gtra.newer_format", detail: "" });  // "error.gtra.newer_format"
 * errorKey("some English text");                        // "error.unknown"
 */
export function errorKey(
  error: unknown,
  exists: (key: string) => boolean = (key) => i18next.exists(key),
): string {
  return resolve(error, exists).key;
}

/**
 * Finds the key and the values for an error (shared by the helpers above).
 *
 * @param error - The error.
 * @param exists - Key check, as in {@link errorKey}.
 * @returns The key and the message values.
 */
function resolve(
  error: unknown,
  exists: (key: string) => boolean,
): { key: string; params: Record<string, string | number> } {
  // Deepest first: the most specific explanation wins.
  const chain = errorChain(error).reverse();
  for (const coded of chain) {
    const key = `error.${coded.code}`;
    if (exists(key)) return { key, params: errorParams(coded.details) };
  }
  return { key: "error.unknown", params: {} };
}

/**
 * Turns an error into a message in the user's language, without logging.
 * Use it for errors shown again and again (e.g. `snapshot.error` on every
 * render).
 *
 * @param error - An `IpcError`, a `CodedError`, or anything else.
 * @param t - The translate function from `useTranslation()`.
 * @returns The translated message.
 *
 * @example
 * errorText(snapshot.error, t); // "These tasks depend on each other in a loop: A → B."
 */
export function errorText(error: unknown, t: TFunction): string {
  const { key, params } = resolve(error, (k) => i18next.exists(k));
  // The key is built at run time, so TypeScript cannot check it against
  // en.json; the `as "error.unknown"` tells it to treat the key as a valid
  // one. `resolve` only returns keys that exist.
  return t(key as "error.unknown", params);
}

/**
 * Turns an error into a message in the user's language, and logs the
 * technical detail for developers.
 *
 * @param error - What a rejected IPC promise carried (an `IpcError`, a
 *   string, an `Error`…).
 * @param t - The translate function from `useTranslation()`.
 * @returns The translated message, e.g. "Your settings could not be saved."
 *
 * @example
 * try { await settingsSet(next); }
 * catch (error) { setMessage(errorMessage(error, t)); }
 */
export function errorMessage(error: unknown, t: TFunction): string {
  // The English detail is useful in the developer console, never on screen.
  console.warn("IPC error", error);
  return errorText(error, t);
}
