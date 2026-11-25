// @vitest-environment happy-dom
/**
 * tests/frontend/grid/RowContextMenu.test.tsx: the row menu on a (fake) page.
 *
 * What it checks (`frontend/features/grid/RowContextMenu.tsx`): the first
 * usable entry gets the focus, the arrow keys skip greyed-out entries and
 * wrap around, choosing closes the menu and acts, Escape and a press
 * outside only close it.
 *
 * Why it matters:
 *   The menu must work fully without a mouse.
 *
 * Main ideas:
 *   The comment on line 1 runs this file in *happy-dom*, a small imitation
 *   of a browser page. `act` lets React finish its work before we look.
 */

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Translations: `t(key)` returns the key itself.
vi.mock("react-i18next", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: (key: string) => key }),
}));

import { RowContextMenu } from "@frontend/features/grid/RowContextMenu";

/** Records calls: an entry's key when chosen, "close" when closing. */
let calls: string[];
/** React's mount point. */
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  calls = [];
  const host = document.body.appendChild(document.createElement("div"));
  root = createRoot(host);
  act(() =>
    root.render(
      <RowContextMenu
        x={10}
        y={10}
        items={[
          { key: "off", label: "Off", disabled: true, onSelect: () => calls.push("off") },
          { key: "one", label: "One", onSelect: () => calls.push("one") },
          { key: "two", label: "Two", onSelect: () => calls.push("two") },
        ]}
        onClose={() => calls.push("close")}
      />,
    ),
  );
});

afterEach(() => {
  act(() => root.unmount());
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
});

/** Presses a key on the focused element. */
function press(key: string): void {
  act(() => {
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
  });
}

/** The text of the focused entry. */
const focused = () => document.activeElement?.textContent;

describe("RowContextMenu", () => {
  it("moves the focus over usable entries only, and wraps", () => {
    expect(focused()).toBe("One");
    press("ArrowDown");
    expect(focused()).toBe("Two");
    press("ArrowDown");
    expect(focused()).toBe("One");
    press("ArrowUp");
    expect(focused()).toBe("Two");
    press("Home");
    expect(focused()).toBe("One");
  });

  it("closes first, then runs the chosen entry", () => {
    act(() => (document.activeElement as HTMLElement).click());
    expect(calls).toEqual(["close", "one"]);
  });

  it("only closes on Escape and on a press outside", () => {
    press("Escape");
    act(() => {
      document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    });
    expect(calls).toEqual(["close", "close"]);
  });
});
