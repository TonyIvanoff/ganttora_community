// @vitest-environment happy-dom
/**
 * tests/frontend/toolbars/ToolbarFrame.test.tsx: toolbars on a (fake) page.
 *
 * What it checks: dragging a toolbar by its grip docks or floats it, Escape
 * cancels, the grip menu docks with the keyboard and keeps the focus, the
 * arrow keys move inside a toolbar, and the Customize dialog edits toolbars.
 *
 * Why it matters:
 *   Every mouse action needs a keyboard way, and a drag must never leave
 *   things half moved.
 *
 * Main ideas:
 *   The imitation page has no sizes, so every element gets a made-up
 *   rectangle: the frame 800×600, everything else 100×30.
 */

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Translations: `t(key)` returns the key itself.
vi.mock("react-i18next", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: (key: string) => key }),
}));

import { CustomizeDialog } from "@frontend/features/toolbars/CustomizeDialog";
import { LegendPanel } from "@frontend/features/gantt/LegendDialog";
import { defaultToolbars } from "@frontend/features/toolbars/layout";
import { ToolbarFrame, ToolbarsMenuButton } from "@frontend/features/toolbars/ToolbarFrame";
import { toolbarsSaved, useToolbarStore } from "@frontend/features/toolbars/toolbarStore";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import { isModalOpen, useDialogStore } from "@frontend/shell/dialogStore";

/** React's mount point. */
let root: Root;
/** Actions that were run. */
let runs: string[];

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const frame = this.hasAttribute("data-toolbar-frame");
    return new DOMRect(0, 0, frame ? 800 : 100, frame ? 600 : 30);
  });
  useToolbarStore.setState({ toolbars: defaultToolbars() });
  useDialogStore.setState({ open: null, legendOpen: false });
  runs = [];
  root = createRoot(document.body.appendChild(document.createElement("div")));
  act(() =>
    root.render(
      <>
        <ToolbarsMenuButton />
        <ToolbarFrame run={(id) => runs.push(id)}>view</ToolbarFrame>
        <CustomizeDialog />
        <LegendPanel />
      </>,
    ),
  );
});

afterEach(async () => {
  act(() => root.unmount());
  await toolbarsSaved();
  document.body.innerHTML = "";
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** Finds an element or fails. */
function find(selector: string): HTMLElement {
  const el = document.querySelector<HTMLElement>(selector);
  if (el === null) throw new Error(`not found: ${selector}`);
  return el;
}

/** The grip of a toolbar. */
const grip = (id: string) => find(`[data-toolbar-id="${id}"] [data-toolbar-item="grip"]`);

/** Finds a button by its text. */
function button(text: string): HTMLElement {
  const el = [...document.querySelectorAll<HTMLElement>("button")].find((b) =>
    b.textContent?.includes(text),
  );
  if (el === undefined) throw new Error(`no button: ${text}`);
  return el;
}

/** Sends a pointer event. */
function pointer(el: Element, type: string, x: number, y: number): void {
  act(() => {
    el.dispatchEvent(
      new PointerEvent(type, { bubbles: true, clientX: x, clientY: y, button: 0, pointerId: 1 }),
    );
  });
}

/** Presses a key on the focused element. */
function press(key: string): void {
  act(() => {
    (document.activeElement ?? document.body).dispatchEvent(
      new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }),
    );
  });
}

/** Clicks. */
const click = (el: HTMLElement) => act(() => el.click());

/** One toolbar of the store. */
const bar = (id: string) => useToolbarStore.getState().toolbars.find((b) => b.id === id);

describe("dragging", () => {
  it("docks at an edge, showing the drop zone on the way", async () => {
    const g = grip("view");
    pointer(g, "pointerdown", 10, 10);
    pointer(g, "pointermove", 10, 300);
    expect(find("[data-drop-zone]").dataset.dropZone).toBe("left");
    pointer(g, "pointerup", 10, 300);
    expect(document.querySelector("[data-drop-zone]")).toBeNull();
    expect(bar("view")?.dock).toBe("left");
    expect(
      find('[data-dock="left"] [data-toolbar-id="view"]').getAttribute("aria-orientation"),
    ).toBe("vertical");
    await toolbarsSaved();
    const saved = useSettingsStore.getState().settings.toolbars.layout;
    expect(saved.find((b) => b.id === "view")?.dock).toBe("left");
  });

  it("floats in the middle, inside the frame", () => {
    const g = grip("task");
    pointer(g, "pointerdown", 10, 10);
    pointer(g, "pointermove", 400, 300);
    pointer(g, "pointerup", 400, 300);
    expect(bar("task")).toMatchObject({ dock: "float", x: 390, y: 290 });
    expect(find('[data-toolbar-id="task"] [data-titlebar]')).toBeTruthy();
    click(find('[data-toolbar-id="task"] [data-toolbar-item="close"]'));
    expect(bar("task")?.visible).toBe(false);
  });

  it("changes nothing after Escape or a tiny move", () => {
    const before = useToolbarStore.getState().toolbars;
    const g = grip("main");
    pointer(g, "pointerdown", 10, 10);
    pointer(g, "pointermove", 10, 300);
    press("Escape");
    pointer(g, "pointerup", 10, 300);
    pointer(g, "pointerdown", 10, 10);
    pointer(g, "pointermove", 12, 10);
    pointer(g, "pointerup", 12, 10);
    expect(useToolbarStore.getState().toolbars).toBe(before);
  });
});

describe("legend", () => {
  it("is a panel that blocks nothing and closes with ×", () => {
    act(() => useDialogStore.getState().toggleLegend());
    expect(isModalOpen()).toBe(false);
    const title = find("[data-legend-title]");
    expect(title.parentElement?.style.right).toBe("2rem");
    pointer(title, "pointerdown", 10, 10);
    pointer(title, "pointermove", 60, 40);
    expect(title.parentElement?.style.right).toBe(""); // left its corner
    click(find('[aria-label="legend.close"]'));
    expect(document.querySelector("[data-legend-title]")).toBeNull();
  });
});

describe("keyboard", () => {
  it("makes a new row with the grip menu and goes back to the first", () => {
    click(grip("legend"));
    click(button("toolbars.newRow"));
    expect(find('[data-toolbar-row="1"]').querySelectorAll("[data-toolbar-id]")).toHaveLength(1);
    expect(bar("legend")?.row).toBe(1);
    click(grip("legend"));
    click(
      [...document.querySelectorAll<HTMLElement>('[role^="menuitem"]')].find((el) =>
        el.textContent?.includes("toolbars.row"),
      )!,
    );
    expect(bar("legend")?.row).toBe(0);
    expect(document.querySelector('[data-toolbar-row="1"]')).toBeNull();
  });

  it("docks with the grip menu and keeps the focus on the grip", () => {
    grip("main").focus();
    click(grip("main"));
    expect(find('[role="menu"]').contains(document.activeElement)).toBe(true);
    click(button("toolbars.dockRight"));
    expect(bar("main")?.dock).toBe("right");
    expect(document.activeElement).toBe(grip("main"));
  });

  it("hides from the menu and comes back with the Toolbars button", () => {
    click(grip("main"));
    click(button("toolbars.hide"));
    expect(bar("main")?.visible).toBe(false);
    expect(document.activeElement?.id).toBe("toolbars-menu-button");
    click(find("#toolbars-menu-button"));
    const entry = button("toolbars.name.main");
    expect(entry.getAttribute("aria-checked")).toBe("false");
    click(entry);
    expect(bar("main")?.visible).toBe(true);
  });

  it("moves with the arrow keys; Tab stops once", () => {
    grip("view").focus();
    press("ArrowRight");
    expect(document.activeElement?.getAttribute("data-toolbar-item")).toBe("zoomDays");
    press("End");
    expect(document.activeElement?.getAttribute("data-toolbar-item")).toBe("explain");
    press("ArrowRight");
    expect(document.activeElement).toBe(grip("view"));
    expect(find('[data-toolbar-id="view"]').querySelectorAll('[tabindex="0"]')).toHaveLength(1);
  });

  it("runs enabled buttons only", () => {
    click(find('[data-toolbar-item="zoomWeeks"]'));
    const undo = find('[data-toolbar-item="undo"]'); // nothing to undo
    expect(undo.getAttribute("aria-disabled")).toBe("true");
    click(undo);
    expect(runs).toEqual(["zoomWeeks"]);
  });
});

describe("customize dialog", () => {
  beforeEach(() => act(() => useDialogStore.getState().show("customize")));

  /** Chooses an option of a list. */
  function choose(list: Element | undefined, value: string): void {
    act(() => {
      (list as HTMLSelectElement).value = value;
      list?.dispatchEvent(new Event("change", { bubbles: true }));
    });
  }

  it("creates a toolbar, adds and removes buttons, deletes it", () => {
    click(button("customize.new"));
    const id = useToolbarStore.getState().toolbars.at(-1)?.id ?? "";
    expect(id).toMatch(/^custom-/);
    const [available] = document.querySelectorAll("dialog select");
    choose(available, "fileSave");
    click(button("customize.add"));
    choose(available, "separator");
    click(button("customize.add"));
    expect(bar(id)?.items).toEqual(["fileSave", "separator"]);
    click(button("customize.up"));
    expect(bar(id)?.items).toEqual(["separator", "fileSave"]);
    click(button("customize.remove"));
    expect(bar(id)?.items).toEqual(["fileSave"]);
    click(button("customize.delete"));
    expect(bar(id)).toBeUndefined();
  });

  it("resets one and all", () => {
    act(() =>
      useToolbarStore
        .getState()
        .update((bars) => bars.map((b) => ({ ...b, items: [], visible: false }))),
    );
    click(button("customize.resetOne"));
    expect(bar("main")?.items).toEqual(defaultToolbars()[0]?.items);
    click(button("customize.resetAll"));
    expect(useToolbarStore.getState().toolbars).toEqual(defaultToolbars());
  });
});

it("leaves a margin only on a side without a toolbar", () => {
  const view = find("[data-view-area]");
  expect(view.className).toContain("ml-3");
  expect(view.className).toContain("mr-3");
  const bars = defaultToolbars().map((b) =>
    b.id === "task" ? { ...b, dock: "left" as const } : b,
  );
  act(() => useToolbarStore.setState({ toolbars: bars }));
  expect(view.className).not.toContain("ml-3");
  expect(view.className).toContain("mr-3");
});
