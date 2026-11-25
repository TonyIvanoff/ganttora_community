/**
 * tests/frontend/shell/dateFieldEnter.test.tsx: typing a date without
 * separators into a date field (`frontend/components/ui/DateField.tsx`).
 *
 * @vitest-environment happy-dom
 */

import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { DateField } from "@frontend/components/ui/DateField";
import { BROWSER_DEFAULT_SETTINGS } from "@frontend/features/settings/options";
import { useSettingsStore } from "@frontend/features/settings/settingsStore";
import "@frontend/i18n";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** A field that keeps its value. */
function Field() {
  const [value, setValue] = useState("");
  return <DateField value={value} onChange={setValue} />;
}

it("Enter writes 20261115 in the chosen format", () => {
  const general = { ...BROWSER_DEFAULT_SETTINGS.general, dateFormat: "mmm/dd/yyyy" };
  useSettingsStore.setState({ settings: { ...BROWSER_DEFAULT_SETTINGS, general } });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() => root.render(<Field />));
  const input = host.querySelector("input");
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(
      input,
      "20261115",
    );
    input?.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(input?.value).toBe("20261115");
  const enter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
  act(() => void input?.dispatchEvent(enter));
  expect(input?.value).toBe("Nov/15/2026");
  // The dialog around it is not confirmed by this Enter.
  expect(enter.defaultPrevented).toBe(true);
  act(() => root.unmount());
});
