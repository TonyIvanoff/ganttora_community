/**
 * features/grid/predecessors.ts: reads and writes predecessor text such as
 * "2FS+1d, 3SS-20%", and turns a changed list into link commands.
 *
 * What it is for:
 *   In MS Project's Predecessors column you type the *IDs* (row numbers) of
 *   the tasks this task waits for, each optionally followed by a link type
 *   and a lag:
 *
 *   | Text       | Meaning                                                  |
 *   |------------|----------------------------------------------------------|
 *   | `2`        | task 2, finish-to-start (FS), no lag                      |
 *   | `2FS+1d`   | task 2, finish-to-start, start 1 working day after it     |
 *   | `3SS-20%`  | task 3, start-to-start, start 20 % of task 3's duration *before* it |
 *   | `4FF`      | task 4, finish-to-finish                                  |
 *
 *   Entries are separated by "," or ";". This file reads that text into a
 *   list ({@link parsePredecessors}), writes a task's links back as text
 *   ({@link formatPredecessors}), and compares the new list with the
 *   task's current links to make the commands for Rust
 *   ({@link linkCommands}).
 *
 * Where it fits:
 *   Used by the grid's Predecessors cell (`cellEdit.ts`) and the
 *   Predecessors tab of the Task Information dialog
 *   (`features/task-info/taskInfoCommands.ts`).
 *
 * Main ideas:
 *   - **Link type codes are translatable** (German MS Project writes "EA"
 *     for FS). The UI language's codes come from `locales/<lang>.json`
 *     (`link.code.*`); the English codes are always understood too.
 *   - **IDs are row numbers only on screen.** Links in the model use task
 *     UUIDs; the row number is translated to the UUID here.
 *   - **No scheduling** (decision D16): whether a link makes a loop is
 *     checked by Rust; the UI shows Rust's error (`cycleDetected`).
 *   - Every new link gets a fresh UUID made in the UI
 *     (`crypto.randomUUID()`), because the document never invents IDs.
 */

import type { Command, DependencyType, LinkRow } from "@frontend/ipc/types";
import {
  lagParts,
  parseLag,
  type DurationBasis,
  type DurationUnit,
  type ParsedLag,
  type UnitWords,
} from "@frontend/lib/units";

/** All link types, in MS Project's order. */
export const DEPENDENCY_TYPES: readonly DependencyType[] = [
  "finishToStart",
  "startToStart",
  "finishToFinish",
  "startToFinish",
];

/** The English two-letter codes, always understood. */
export const ENGLISH_LINK_CODES: Record<DependencyType, string> = {
  finishToStart: "FS",
  startToStart: "SS",
  finishToFinish: "FF",
  startToFinish: "SF",
};

/** One entry of a predecessor list, as typed. */
export interface ParsedPredecessor {
  /** The row number (ID column, 1 = first task). */
  row: number;
  /** Link type; FS when none was typed. */
  kind: DependencyType;
  /** The lag; `{ kind: "time", minutes: 0 }` when none was typed. */
  lag: ParsedLag;
}

/** Why a predecessor text could not be read. The UI key is `grid.invalid.<code>`. */
export type PredecessorProblem =
  /** The text does not follow the pattern. */
  | { code: "predecessors" }
  /** No task has this row number. */
  | { code: "unknownRow"; row: number }
  /** The task would be its own predecessor. */
  | { code: "selfLinkRow"; row: number }
  /** The same row is listed twice. */
  | { code: "duplicateRow"; row: number };

/** The result of {@link parsePredecessors}: entries, or the first problem. */
export type PredecessorParse =
  { ok: true; entries: ParsedPredecessor[] } | { ok: false; problem: PredecessorProblem };

/** Everything the parser and the formatter need besides the text. */
export interface PredecessorContext {
  /** The project's unit sizes (minutes per day…). */
  basis: DurationBasis;
  /** Link codes of the UI language (e.g. FS → "EA" in German). */
  codes: Record<DependencyType, string>;
  /** Short unit letters of the UI language, for writing lags ("d", "h"…). */
  unitShort: Record<DurationUnit, string>;
  /** Unit words of the UI language, for reading lags. */
  unitWords?: UnitWords;
  /** Formats a number in the user's locale (decimal comma or point). */
  formatNumber: (value: number) => string;
}

/**
 * Finds the link type for a typed code.
 *
 * @param code - Two letters, any case, e.g. "ss".
 * @param codes - The UI language's codes.
 * @returns The type, or `null` if the code is unknown.
 */
function kindOf(code: string, codes: Record<DependencyType, string>): DependencyType | null {
  const upper = code.toUpperCase();
  for (const kind of DEPENDENCY_TYPES) {
    if (codes[kind].toUpperCase() === upper || ENGLISH_LINK_CODES[kind] === upper) return kind;
  }
  return null;
}

/**
 * Reads a predecessor text.
 *
 * @param text - What the user typed, e.g. "2FS+1d, 3SS-20%". Empty text
 *   means "no predecessors".
 * @param context - Unit sizes and translated codes.
 * @param rowCount - How many tasks exist (row numbers 1…rowCount are valid).
 * @param ownRow - The row number of the task being edited (it cannot be
 *   its own predecessor), or `null` if unknown.
 * @returns `{ ok: true, entries }` or `{ ok: false, problem }`.
 *
 * @example
 * parsePredecessors("2FS+1d; 3ss-20%", ctx, 8, 5);
 * // { ok: true, entries: [
 * //   { row: 2, kind: "finishToStart", lag: { kind: "time", minutes: 480 } },
 * //   { row: 3, kind: "startToStart", lag: { kind: "percent", percent: -20 } } ] }
 */
export function parsePredecessors(
  text: string,
  context: PredecessorContext,
  rowCount: number,
  ownRow: number | null,
): PredecessorParse {
  const entries: ParsedPredecessor[] = [];
  const seen = new Set<number>();
  const trimmed = text.trim();
  if (trimmed === "") return { ok: true, entries };

  // One entry: a row number, an optional two-letter code, an optional lag.
  //   (\d+)                          the row number
  //   \s*(\p{L}{2})?                 the link code (letters of any alphabet)
  //   \s*([+-]\s*[\d.,]+\s*(?:%|\p{L}*))?  the lag with its sign and unit
  // The flag "y" ("sticky") makes the pattern match exactly at `lastIndex`,
  // so we can read entry after entry from left to right.
  const entry = /\s*(\d+)\s*(\p{L}{2})?\s*([+-]\s*[\d.,]+\s*(?:%|\p{L}*))?\s*/uy;
  let position = 0;
  while (position < trimmed.length) {
    entry.lastIndex = position;
    const match = entry.exec(trimmed);
    if (match === null) return { ok: false, problem: { code: "predecessors" } };
    position = entry.lastIndex;

    const row = Number(match[1]);
    const kind = match[2] === undefined ? "finishToStart" : kindOf(match[2], context.codes);
    const lag = parseLag(match[3] ?? "", context.basis, context.unitWords);
    if (kind === null || lag === null) return { ok: false, problem: { code: "predecessors" } };
    if (row < 1 || row > rowCount) return { ok: false, problem: { code: "unknownRow", row } };
    if (row === ownRow) return { ok: false, problem: { code: "selfLinkRow", row } };
    if (seen.has(row)) return { ok: false, problem: { code: "duplicateRow", row } };
    seen.add(row);
    entries.push({ row, kind, lag });

    // After an entry: the end, or a separator followed by the next entry.
    if (position < trimmed.length) {
      const separator = trimmed[position];
      if (separator !== "," && separator !== ";") {
        return { ok: false, problem: { code: "predecessors" } };
      }
      position++;
      if (position >= trimmed.length) return { ok: false, problem: { code: "predecessors" } };
    }
  }
  return { ok: true, entries };
}

/**
 * Writes one lag as short text, e.g. "+1d", "-4h", "+50%", or "" for none.
 *
 * @param lagMinutes - The time lag in working minutes.
 * @param lagPercent - The percentage lag, or `null` for a time lag.
 * @param context - Unit letters and number format.
 * @returns The text with its sign, or an empty string for a zero lag.
 *
 * @example
 * formatLag(480, null, ctx); // "+1d"
 * formatLag(0, -20, ctx);    // "-20%"
 */
export function formatLag(
  lagMinutes: number,
  lagPercent: number | null,
  context: PredecessorContext,
): string {
  if (lagPercent !== null) {
    if (lagPercent === 0) return "";
    return `${lagPercent > 0 ? "+" : "-"}${context.formatNumber(Math.abs(lagPercent))}%`;
  }
  if (lagMinutes === 0) return "";
  const parts = lagParts(Math.abs(lagMinutes), context.basis);
  const sign = lagMinutes > 0 ? "+" : "-";
  return `${sign}${context.formatNumber(parts.value)}${context.unitShort[parts.unit]}`;
}

/**
 * Writes a task's incoming links as predecessor text.
 *
 * @param links - The task's links (`TaskRow.links`).
 * @param rowNumberOf - Gives the row number (1-based) of a task ID, or
 *   `undefined` if the task is unknown (such links are left out).
 * @param context - Codes, unit letters and number format.
 * @returns E.g. "2, 3SS+1d". A finish-to-start link without lag is written
 *   as the bare number, as in MS Project.
 *
 * @example
 * formatPredecessors(task.links, (id) => rowById.get(id), ctx); // "2FS+1d, 3"
 */
export function formatPredecessors(
  links: readonly LinkRow[],
  rowNumberOf: (id: string) => number | undefined,
  context: PredecessorContext,
): string {
  const parts: string[] = [];
  for (const link of links) {
    const row = rowNumberOf(link.predecessor);
    if (row === undefined) continue;
    const lag = formatLag(link.lagMinutes, link.lagPercent, context);
    const code = link.kind === "finishToStart" && lag === "" ? "" : context.codes[link.kind];
    parts.push(`${row}${code}${lag}`);
  }
  return parts.join(", ");
}

/** A wanted link, by predecessor *ID* (already translated from row numbers). */
export interface WantedLink {
  /** The predecessor task's ID. */
  predecessor: string;
  /** Link type. */
  kind: DependencyType;
  /** The lag. */
  lag: ParsedLag;
}

/**
 * Compares a task's current links with the wanted ones and makes the
 * commands that turn one into the other.
 *
 * @param successor - The task the links lead to.
 * @param current - Its current incoming links (`TaskRow.links`).
 * @param wanted - The links it should have.
 * @param newId - Makes a UUID for each new link (normally
 *   `() => crypto.randomUUID()`; tests pass a fixed sequence).
 * @returns The commands, in this order: removals, edits, additions. Empty
 *   when nothing changes.
 *
 * Why this order? Removing first means a link that is "moved" (removed
 * and added again) never exists twice, which Rust would refuse
 * (`linkExists`).
 *
 * @example
 * linkCommands(taskId, task.links, [{ predecessor: aId, kind: "finishToStart",
 *   lag: { kind: "time", minutes: 0 } }], () => crypto.randomUUID());
 */
export function linkCommands(
  successor: string,
  current: readonly LinkRow[],
  wanted: readonly WantedLink[],
  newId: () => string,
): Command[] {
  const removals: Command[] = [];
  const edits: Command[] = [];
  const additions: Command[] = [];
  const wantedByPredecessor = new Map(wanted.map((w) => [w.predecessor, w]));
  const currentByPredecessor = new Map(current.map((link) => [link.predecessor, link]));

  for (const link of current) {
    const want = wantedByPredecessor.get(link.predecessor);
    if (want === undefined) {
      removals.push({ type: "removeDependency", id: link.id });
      continue;
    }
    const lagMinutes = want.lag.kind === "time" ? want.lag.minutes : 0;
    const lagPercent = want.lag.kind === "percent" ? want.lag.percent : null;
    // A link has one kind of lag: when a percentage is set, Rust stores 0
    // minutes (and we send 0), so comparing the percentages is enough.
    const sameLag =
      lagPercent === null
        ? link.lagPercent === null && link.lagMinutes === lagMinutes
        : link.lagPercent === lagPercent;
    if (link.kind !== want.kind || !sameLag) {
      edits.push({
        type: "editDependency",
        id: link.id,
        kind: want.kind,
        lag: { minutes: lagMinutes },
        lagPercent,
      });
    }
  }

  for (const want of wanted) {
    if (currentByPredecessor.has(want.predecessor)) continue;
    additions.push({
      type: "addDependency",
      dependency: {
        id: newId(),
        predecessor: want.predecessor,
        successor,
        kind: want.kind,
        lag: { minutes: want.lag.kind === "time" ? want.lag.minutes : 0 },
        lagPercent: want.lag.kind === "percent" ? want.lag.percent : null,
      },
    });
  }
  return [...removals, ...edits, ...additions];
}
