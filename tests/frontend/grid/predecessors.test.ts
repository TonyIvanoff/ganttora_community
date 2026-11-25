/**
 * tests/frontend/grid/predecessors.test.ts: tests for the Predecessors cell.
 *
 * What it checks:
 *   `frontend/features/grid/predecessors.ts`:
 *   - `parsePredecessors` reads text such as "2FS+1d; 3ss-20%" into links,
 *     and reports unknown rows, links to the task itself and duplicates;
 *   - `formatLag` / `formatPredecessors` write links back as text, and the
 *     text reads back into the same links (a "round trip");
 *   - `linkCommands` turns "the links the user wants" into the fewest
 *     commands, always in the order remove → edit → add.
 *
 * Why it matters:
 *   The Predecessors column is how most users create links. Misreading
 *   "3SS-20%" would silently give the wrong schedule. The command order
 *   matters because Rust refuses a second link between the same two tasks:
 *   an old link must be removed before a new one is added.
 *
 * Main ideas:
 *   Row numbers in the text are grid row numbers (1 = first row). The
 *   tests use a 5-row project and say which row is being edited (`ownRow`).
 */

import { describe, expect, it } from "vitest";
import type { LinkRow } from "@frontend/ipc/types";
import {
  ENGLISH_LINK_CODES,
  formatLag,
  formatPredecessors,
  linkCommands,
  parsePredecessors,
  type PredecessorContext,
} from "@frontend/features/grid/predecessors";
import { englishUnits } from "./helpers";

const UNITS = englishUnits();

/** Parses with 5 rows, editing row 1, and fails the test on a problem. */
function parseOk(text: string, context: PredecessorContext = UNITS) {
  const result = parsePredecessors(text, context, 5, 1);
  if (!result.ok) throw new Error(`unexpected problem ${JSON.stringify(result.problem)}`);
  return result.entries;
}

/** A link row coming from row `row` ("task-<row>"). */
function link(row: number, changes: Partial<LinkRow> = {}): LinkRow {
  return {
    id: `link-${row}`,
    predecessor: `task-${row}`,
    kind: "finishToStart",
    lagMinutes: 0,
    lagPercent: null,
    ...changes,
  };
}

/** Row number of "task-N" is N. */
function rowOf(id: string): number | undefined {
  const n = Number(id.replace("task-", ""));
  return Number.isFinite(n) && n >= 1 && n <= 5 ? n : undefined;
}

describe("parsePredecessors", () => {
  // The example from the module: a time lag and a percentage lead, with
  // lower-case code and ";" as separator.
  it('reads "2FS+1d; 3ss-20%"', () => {
    expect(parseOk("2FS+1d; 3ss-20%")).toEqual([
      { row: 2, kind: "finishToStart", lag: { kind: "time", minutes: 480 } },
      { row: 3, kind: "startToStart", lag: { kind: "percent", percent: -20 } },
    ]);
  });

  // A plain number is a finish-to-start link without lag; spaces and ","
  // separators are fine.
  it("reads plain row numbers and the four codes", () => {
    expect(parseOk(" 2 , 3FF , 4SF, 5 SS +2h ")).toEqual([
      { row: 2, kind: "finishToStart", lag: { kind: "time", minutes: 0 } },
      { row: 3, kind: "finishToFinish", lag: { kind: "time", minutes: 0 } },
      { row: 4, kind: "startToFinish", lag: { kind: "time", minutes: 0 } },
      { row: 5, kind: "startToStart", lag: { kind: "time", minutes: 120 } },
    ]);
  });

  // An empty cell means "no predecessors" (all links removed).
  it("reads an empty cell as no links", () => {
    expect(parseOk("")).toEqual([]);
    expect(parseOk("   ")).toEqual([]);
  });

  // Row 0 and rows past the end do not exist.
  it("reports unknown rows", () => {
    expect(parsePredecessors("7", UNITS, 5, 1)).toEqual({
      ok: false,
      problem: { code: "unknownRow", row: 7 },
    });
    expect(parsePredecessors("0", UNITS, 5, 1)).toEqual({
      ok: false,
      problem: { code: "unknownRow", row: 0 },
    });
  });

  // A task cannot wait for itself.
  it("reports a link to the task's own row", () => {
    expect(parsePredecessors("2, 1", UNITS, 5, 1)).toEqual({
      ok: false,
      problem: { code: "selfLinkRow", row: 1 },
    });
  });

  // Only one link per pair of tasks is allowed.
  it("reports the same row twice", () => {
    expect(parsePredecessors("2, 2SS", UNITS, 5, 1)).toEqual({
      ok: false,
      problem: { code: "duplicateRow", row: 2 },
    });
  });

  // Unknown codes, missing separators, a trailing comma and plain words are
  // all "cannot read this" problems.
  it("reports text it cannot read", () => {
    for (const text of ["2XX", "2 3", "2,", "abc", "2FS+", "2FS+1x", "2FS 1d"]) {
      expect(parsePredecessors(text, UNITS, 5, 1), text).toEqual({
        ok: false,
        problem: { code: "predecessors" },
      });
    }
  });

  // A translated UI has its own link codes (German: AA = Anfang-Anfang);
  // the English codes keep working too.
  it("accepts translated link codes as well as the English ones", () => {
    const german: PredecessorContext = {
      ...UNITS,
      codes: { finishToStart: "EA", startToStart: "AA", finishToFinish: "EE", startToFinish: "AE" },
    };
    expect(parseOk("2AA, 3SS", german).map((e) => e.kind)).toEqual([
      "startToStart",
      "startToStart",
    ]);
  });
});

describe("formatLag", () => {
  // Zero lags are not written at all; others get their sign and the
  // largest unit that divides them exactly.
  it("writes lags with sign and unit", () => {
    expect(formatLag(0, null, UNITS)).toBe("");
    expect(formatLag(999, 0, UNITS)).toBe(""); // percent wins over minutes
    expect(formatLag(960, null, UNITS)).toBe("+2d");
    expect(formatLag(720, null, UNITS)).toBe("+12h");
    expect(formatLag(-90, null, UNITS)).toBe("-90m");
    expect(formatLag(0, 50, UNITS)).toBe("+50%");
    expect(formatLag(0, -20, UNITS)).toBe("-20%");
  });
});

describe("formatPredecessors", () => {
  // FS without lag is written as just the row; everything else with code.
  it('writes "2FS+1d, 3SS-20%" style text', () => {
    const links = [
      link(2, { lagMinutes: 480 }),
      link(3, { kind: "startToStart", lagPercent: -20 }),
      link(4),
    ];
    expect(formatPredecessors(links, rowOf, UNITS)).toBe("2FS+1d, 3SS-20%, 4");
  });

  // A link from a row the grid cannot number (e.g. hidden data) is skipped.
  it("skips predecessors without a row number", () => {
    expect(formatPredecessors([link(9), link(2)], rowOf, UNITS)).toBe("2");
  });

  // Text written by format reads back into the same links.
  it("round-trips through parsePredecessors", () => {
    const links = [
      link(2, { lagMinutes: 480 }),
      link(3, { kind: "startToStart", lagPercent: -20 }),
      link(4, { kind: "finishToFinish", lagMinutes: -90 }),
      link(5, { kind: "startToFinish" }),
    ];
    const text = formatPredecessors(links, rowOf, UNITS);
    expect(parseOk(text)).toEqual([
      { row: 2, kind: "finishToStart", lag: { kind: "time", minutes: 480 } },
      { row: 3, kind: "startToStart", lag: { kind: "percent", percent: -20 } },
      { row: 4, kind: "finishToFinish", lag: { kind: "time", minutes: -90 } },
      { row: 5, kind: "startToFinish", lag: { kind: "time", minutes: 0 } },
    ]);
  });
});

describe("linkCommands", () => {
  /** A counter for new link IDs: "new-1", "new-2" … */
  function counter(): () => string {
    let n = 0;
    return () => `new-${++n}`;
  }

  // Current: from 2, 3, 4. Wanted: 3 changed to SS, 4 unchanged, 5 new;
  // 2 is gone. The commands come out as remove(2), edit(3), add(5).
  it("orders the commands remove → edit → add", () => {
    const current = [link(2), link(3), link(4, { kind: "startToStart", lagPercent: 50 })];
    const commands = linkCommands(
      "task-1",
      current,
      [
        { predecessor: "task-5", kind: "finishToStart", lag: { kind: "time", minutes: 480 } },
        { predecessor: "task-3", kind: "startToStart", lag: { kind: "time", minutes: 0 } },
        { predecessor: "task-4", kind: "startToStart", lag: { kind: "percent", percent: 50 } },
      ],
      counter(),
    );
    expect(commands).toEqual([
      { type: "removeDependency", id: "link-2" },
      {
        type: "editDependency",
        id: "link-3",
        kind: "startToStart",
        lag: { minutes: 0 },
        lagPercent: null,
      },
      {
        type: "addDependency",
        dependency: {
          id: "new-1",
          predecessor: "task-5",
          successor: "task-1",
          kind: "finishToStart",
          lag: { minutes: 480 },
          lagPercent: null,
        },
      },
    ]);
  });

  // Nothing changed: no commands at all (no empty undo step).
  it("gives no commands when nothing changed", () => {
    const current = [link(2, { lagMinutes: 480 })];
    const wanted = [
      {
        predecessor: "task-2",
        kind: "finishToStart" as const,
        lag: { kind: "time" as const, minutes: 480 },
      },
    ];
    expect(linkCommands("task-1", current, wanted, counter())).toEqual([]);
  });

  // A percent link with leftover minutes still counts as unchanged when the
  // percentage is the same (Rust ignores the minutes then).
  it("ignores stored minutes on an unchanged percent link", () => {
    const current = [link(2, { lagMinutes: 999, lagPercent: 50 })];
    const wanted = [
      {
        predecessor: "task-2",
        kind: "finishToStart" as const,
        lag: { kind: "percent" as const, percent: 50 },
      },
    ];
    expect(linkCommands("task-1", current, wanted, counter())).toEqual([]);
  });

  // Switching a percent link to a time lag is an edit with lagPercent null.
  it("edits a percent link that becomes a time lag", () => {
    const current = [link(2, { lagPercent: 50 })];
    const wanted = [
      {
        predecessor: "task-2",
        kind: "finishToStart" as const,
        lag: { kind: "time" as const, minutes: 0 },
      },
    ];
    expect(linkCommands("task-1", current, wanted, counter())).toEqual([
      {
        type: "editDependency",
        id: "link-2",
        kind: "finishToStart",
        lag: { minutes: 0 },
        lagPercent: null,
      },
    ]);
  });

  // The English codes table is complete (used as the fallback everywhere).
  it("has an English code for every link type", () => {
    expect(ENGLISH_LINK_CODES).toEqual({
      finishToStart: "FS",
      startToStart: "SS",
      finishToFinish: "FF",
      startToFinish: "SF",
    });
  });
});
