/**
 * tests/frontend/toolbars/actions.test.ts: the action registry and the menu
 * built from it.
 *
 * What it checks: every text key exists in `locales/en.json`, every action
 * can be reached from the menu, every ID on a toolbar or in the menu is
 * registered, the rules for "possible now", the two-reports-of-one-key
 * protection, and the macOS text-editing rules of the menu.
 *
 * Why it matters:
 *   Registry keys are plain text that TypeScript cannot check, and the
 *   menu must never steal Delete or Ctrl+C from a text field.
 */

import { describe, expect, it } from "vitest";
import en from "@locales/en.json";
import { claimAction, CLAIM_WINDOW_MS } from "@frontend/features/toolbars/actionClaim";
import {
  ACTION_IDS,
  actionOf,
  isActionEnabled,
  isActionId,
  isActionPressed,
  type ActionContext,
  type ActionState,
} from "@frontend/features/toolbars/actions";
import { cleanToolbars, defaultToolbars, SEPARATOR } from "@frontend/features/toolbars/layout";
import {
  buildMenu,
  menuStates,
  menuStructure,
  QUIT_ID,
  TOOLBAR_PREFIX,
  type MenuInput,
} from "@frontend/features/toolbars/menuModel";
import type { MenuNode } from "@frontend/ipc/types";
import { useAppStore } from "@frontend/state/store";

/** Looks a dotted key up in en.json. */
function text(key: string): unknown {
  return key
    .split(".")
    .reduce<unknown>((node, part) => (node as Record<string, unknown> | undefined)?.[part], en);
}

/** An open project with a selected task. */
const READY: ActionState = {
  hasCore: true,
  isDemoData: false,
  hasProject: true,
  hasTask: true,
  hasGroups: true,
  canUndo: true,
  canRedo: true,
  zoom: "weeks",
  zoomPercent: 100,
  hasSeveralTasks: true,
  onProject: false,
  canBecomeSubtasks: true,
  canBecomeTasks: true,
  selectionLinked: false,
  explainOpen: false,
  legendOpen: false,
};

/** Menu input; `t` fails on a key that is not in en.json. */
function input(changes: Partial<MenuInput> = {}): MenuInput {
  return {
    mac: true,
    t: (key) => {
      expect(typeof text(key), key).toBe("string");
      return key;
    },
    state: READY,
    toolbars: defaultToolbars(),
    textMode: false,
    modal: false,
    ...changes,
  };
}

/** All nodes of a menu, also inside submenus. */
function flat(nodes: readonly MenuNode[]): MenuNode[] {
  return nodes.flatMap((n) => (n.kind === "submenu" ? [n, ...flat(n.items)] : [n]));
}

/** The entries with an ID. */
function items(changes: Partial<MenuInput> = {}) {
  return flat(buildMenu(input(changes))).filter((n) => n.kind === "item");
}

describe("registry", () => {
  it("has every text in en.json", () => {
    for (const id of ACTION_IDS) {
      const a = actionOf(id);
      // Texts that depend on the state: check both states of the selection.
      const both = [a.labelKey, a.shortKey, a.titleKey].flatMap((key) =>
        typeof key === "function" ? [key(READY), key({ ...READY, selectionLinked: true })] : [key],
      );
      const keys = [...both, a.shortcut?.mac, a.shortcut?.other];
      for (const key of keys) {
        if (key !== undefined) expect(typeof text(key), `${id}: ${key}`).toBe("string");
      }
    }
    for (const bar of defaultToolbars()) {
      expect(typeof text(`toolbars.name.${bar.id}`)).toBe("string");
    }
  });

  it("knows every button of the predefined toolbars", () => {
    for (const bar of defaultToolbars()) {
      for (const item of bar.items) expect(item === SEPARATOR || isActionId(item), item).toBe(true);
    }
  });

  it("says what is possible", () => {
    expect(ACTION_IDS.filter((id) => !isActionEnabled(id, READY))).toEqual([]);
    const demo = { ...READY, isDemoData: true };
    for (const id of ["undo", "addTask", "cut", "paste", "deleteTasks", "calendars"] as const) {
      expect(isActionEnabled(id, demo), id).toBe(false);
    }
    expect(isActionEnabled("addTask", { ...READY, hasTask: false })).toBe(true);
    expect(isActionEnabled("indent", { ...READY, canBecomeSubtasks: false })).toBe(false);
    expect(isActionEnabled("outdent", { ...READY, canBecomeTasks: false })).toBe(false);
    expect(isActionEnabled("fileSave", { ...READY, hasCore: false })).toBe(false);
    expect(isActionPressed("zoomWeeks", READY)).toBe(true);
    expect(isActionPressed("fileSave", READY)).toBeUndefined();
  });
});

describe("claimAction", () => {
  it("drops the second report of one press, but not key repeats", () => {
    expect(claimAction("x", "key", 1000)).toBe(true);
    expect(claimAction("x", "menu", 1005)).toBe(false);
    expect(claimAction("x", "key", 1030)).toBe(true);
    expect(claimAction("x", "menu", 1035)).toBe(false);
    expect(claimAction("x", "menu", 1030 + CLAIM_WINDOW_MS)).toBe(true);
    expect(claimAction("x", "button", 1031 + CLAIM_WINDOW_MS)).toBe(true);
  });
});

describe("menu", () => {
  it("reaches every action, and only known ones, on every system", () => {
    for (const mac of [true, false]) {
      const found = new Set(items({ mac }).map((n) => n.id));
      for (const id of ACTION_IDS) expect(found.has(id), id).toBe(true);
      for (const id of found) {
        expect(isActionId(id) || id === QUIT_ID || id.startsWith(TOOLBAR_PREFIX), id).toBe(true);
      }
      for (const bar of defaultToolbars())
        expect(found.has(`${TOOLBAR_PREFIX}${bar.id}`)).toBe(true);
    }
  });

  it("owns shortcuts on macOS only, and never text-editing keys", () => {
    expect(items({ mac: false }).filter((n) => n.accelerator !== undefined)).toEqual([]);
    expect(items({ mac: false }).find((n) => n.id === "fileSave")?.label).toContain("\t");
    for (const n of items()) {
      if (n.accelerator !== undefined) expect(n.accelerator).toContain("CmdOrCtrl");
    }
    for (const id of ["deleteTasks", "addTask", "indent", "moveUp", "taskInfo"]) {
      expect(items().find((n) => n.id === id)?.accelerator).toBeUndefined();
    }
  });

  it("gives Edit back to text fields while typing", () => {
    const typing = flat(buildMenu(input({ textMode: true })));
    const system = typing.flatMap((n) => (n.kind === "predefined" ? [n.name] : []));
    expect(system).toEqual(expect.arrayContaining(["undo", "cut", "copy", "paste", "selectAll"]));
    expect(items({ textMode: true }).some((n) => n.accelerator === "CmdOrCtrl+C")).toBe(false);
    const other = items({ textMode: true, mac: false });
    for (const id of ["undo", "copy", "deleteTasks"]) {
      expect(other.find((n) => n.id === id)?.enabled, id).toBe(false);
    }
  });

  it("greys out everything but Quit behind a dialog", () => {
    const enabled = items({ modal: true }).filter((n) => n.enabled);
    expect(enabled.map((n) => n.id)).toEqual([QUIT_ID]);
  });

  it("rebuilds only when texts or structure change", () => {
    const a = buildMenu(input());
    const b = buildMenu(input({ state: { ...READY, canUndo: false, zoom: "days" } }));
    expect(menuStructure(a)).toBe(menuStructure(b));
    expect(menuStates(a)).not.toEqual(menuStates(b));
    expect(menuStructure(buildMenu(input({ textMode: true })))).not.toBe(menuStructure(a));
  });

  it("doubles & in names", () => {
    const bars = defaultToolbars().concat({
      id: "mine",
      name: "A & B",
      items: [],
      dock: "top",
      row: 0,
      x: 0,
      y: 0,
      visible: true,
    });
    const entry = items({ toolbars: bars }).find((n) => n.id === "toolbar:mine");
    expect(entry?.label).toBe("A && B");
  });
});

describe("newer buttons on saved toolbars", () => {
  it("reach an untouched toolbar, but not a customized one", () => {
    const [main, task, view] = defaultToolbars();
    if (main === undefined || task === undefined || view === undefined) throw new Error("missing");
    const old = {
      ...task,
      dock: "left" as const,
      // As saved by the version before "Add project".
      items: [
        "addTask",
        "addSubtask",
        SEPARATOR,
        "outdent",
        "indent",
        SEPARATOR,
        "linkTasks",
        "addBlocker",
        "taskInfo",
      ],
    };
    const custom = { ...view, items: ["zoomIn"] };
    const [, upgraded, kept] = cleanToolbars([main, old, custom]);
    expect(upgraded).toEqual({ ...task, dock: "left" });
    expect(kept?.items).toEqual(["zoomIn"]);
  });

  it("gives a saved View toolbar the Go to today button", () => {
    const view = defaultToolbars().find((bar) => bar.id === "view");
    if (view === undefined) throw new Error("missing");
    const saved = { ...view, items: view.items.filter((item) => item !== "goToToday") };
    expect(cleanToolbars([saved]).find((bar) => bar.id === "view")?.items).toContain("goToToday");
  });
});

describe("link and unlink", () => {
  const label = (state: ActionState) => {
    const entry = items({ state }).find((n) => n.id === "linkTasks");
    return entry?.kind === "item" ? [entry.label, entry.enabled] : [];
  };
  it("is greyed out with one task and turns into Unlink", () => {
    expect(label({ ...READY, hasSeveralTasks: false })).toEqual(["task.link", false]);
    expect(label(READY)).toEqual(["task.link", true]);
    expect(label({ ...READY, selectionLinked: true })).toEqual(["task.unlink", true]);
  });
});

it("offers Project info for a project and Task info for a task", () => {
  const label = (onProject: boolean) =>
    items({ state: { ...READY, onProject } }).find((n) => n.id === "taskInfo");
  expect(label(true)).toMatchObject({ label: "task.projectInfo" });
  expect(label(false)).toMatchObject({ label: "task.info" });
});

it("Go to today asks the chart, every time it is pressed", () => {
  const before = useAppStore.getState().scrollToTodayRequest;
  const goToToday = useAppStore.getState().requestScrollToToday;
  const context = { goToToday } as unknown as ActionContext;
  actionOf("goToToday").run(context);
  actionOf("goToToday").run(context);
  expect(useAppStore.getState().scrollToTodayRequest).toBe(before + 2);
});
