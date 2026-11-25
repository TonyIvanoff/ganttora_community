// @vitest-environment happy-dom
/**
 * tests/frontend/components/Tooltips.test.tsx: the app's own tooltips.
 *
 * What it checks: the text appears after a moment, the system's tooltip is
 * switched off meanwhile, and the title comes back afterwards.
 */

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { TOOLTIP_DELAY_MS, Tooltips } from "@frontend/components/ui/Tooltips";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root;
const get = (id: string) => document.getElementById(id) as HTMLElement;
const tip = () => document.querySelector("[role=tooltip]") as HTMLElement;
const over = (id: string) =>
  get(id).dispatchEvent(new MouseEvent("mouseover", { bubbles: true, clientX: 5, clientY: 5 }));

beforeEach(() => {
  vi.useFakeTimers();
  document.body.innerHTML =
    '<div id="row" title="Row"><i id="icon" title="Icon">x</i></div><b id="other">y</b><div id="app"></div>';
  root = createRoot(get("app"));
  act(() => root.render(<Tooltips />));
});

afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
});

it("shows the innermost title after a moment and silences the system's", () => {
  over("icon");
  expect(tip().hidden).toBe(true);
  vi.advanceTimersByTime(TOOLTIP_DELAY_MS);
  expect(tip().hidden).toBe(false);
  expect(tip().textContent).toBe("Icon");
  expect(get("icon").title + get("row").title).toBe("");
});

it("gives the titles back when the mouse leaves", () => {
  over("icon");
  vi.advanceTimersByTime(TOOLTIP_DELAY_MS);
  over("row");
  expect(tip().hidden).toBe(true);
  expect(get("icon").title).toBe("Icon");
  vi.advanceTimersByTime(TOOLTIP_DELAY_MS);
  expect(tip().textContent).toBe("Row");
  over("other");
  expect(tip().hidden).toBe(true);
  expect(get("row").title).toBe("Row");
});

it("keeps a title that was changed meanwhile", () => {
  over("icon");
  get("icon").title = "New";
  over("other");
  expect(get("icon").title).toBe("New");
});

it("hides on a click", () => {
  over("icon");
  vi.advanceTimersByTime(TOOLTIP_DELAY_MS);
  get("icon").dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
  expect(tip().hidden).toBe(true);
});
