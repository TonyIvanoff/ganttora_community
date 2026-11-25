// @vitest-environment happy-dom
/**
 * tests/frontend/grid/percentEditor.test.tsx: the % complete list in the grid.
 *
 * What it checks: a picked value is saved at once and only once, any number
 * can be typed, and with the arrow keys nothing is saved until Enter.
 */

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@frontend/i18n";
import { CellEditor } from "@frontend/features/grid/GridCell";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root;
const onCommit = vi.fn(() => true);

/** The field, and the offered percentages. */
const field = () => document.querySelector("input") as HTMLInputElement;
const options = () => [...document.querySelectorAll<HTMLElement>("[role=option]")];

/** Types a text. */
function type(value: string): void {
  act(() => {
    // React watches the field's own setter, so use the browser's.
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(field(), value);
    field().dispatchEvent(new Event("input", { bubbles: true }));
  });
}

/** Presses a key. */
function press(key: string): void {
  act(() => {
    field().dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  });
}

beforeEach(() => {
  onCommit.mockClear();
  document.body.innerHTML = "<div></div>";
  root = createRoot(document.body.firstElementChild as Element);
  act(() =>
    root.render(
      <CellEditor
        kind="percent"
        initialText="0"
        selectAll
        label="% complete"
        error={null}
        onCommit={onCommit}
        onCancel={() => undefined}
      />,
    ),
  );
});

afterEach(() => act(() => root.unmount()));

it("offers the seven percentages", () => {
  expect(options().map((o) => parseInt(o.textContent ?? "", 10))).toEqual([
    0, 10, 25, 50, 75, 90, 100,
  ]);
});

it("saves a picked value at once, and only once", () => {
  act(() => options()[4]?.click());
  expect(onCommit).toHaveBeenCalledWith("75", "none");
  act(() => field().blur());
  expect(onCommit).toHaveBeenCalledTimes(1);
});

it("takes any typed number", () => {
  type("37");
  press("Enter");
  expect(onCommit).toHaveBeenCalledWith("37", "down");
});

it("steps with the arrow keys and waits for Enter", () => {
  press("ArrowDown");
  press("ArrowDown");
  press("ArrowUp");
  expect(onCommit).not.toHaveBeenCalled();
  press("Enter");
  expect(onCommit).toHaveBeenCalledWith("10", "down");
});
