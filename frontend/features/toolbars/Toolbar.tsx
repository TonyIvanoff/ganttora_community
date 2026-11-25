/**
 * features/toolbars/Toolbar.tsx: draws one toolbar, docked or floating.
 *
 * What it is for:
 *   A grip to drag it, then buttons (icon + short text) and dividing lines.
 *   Docked left or right, the buttons stand below each other with small
 *   text under the icon. Floating, it has a small title bar.
 *
 * Where it fits:
 *   Rendered by `ToolbarFrame.tsx`. Texts and rules come from the registry
 *   (`actions.ts`, `useActions.ts`).
 *
 * Main ideas:
 *   - **WAI-ARIA toolbar pattern:** Tab enters the toolbar once; the arrow
 *     keys (and Home / End) move between its buttons. Only one button has
 *     `tabIndex 0` ("roving tab index").
 *   - Buttons that cannot be used are marked `aria-disabled` but stay
 *     focusable, so the focus is not lost when, say, Undo runs out.
 */

import { useState, type KeyboardEvent } from "react";
import type { ToolbarLayout } from "@frontend/ipc/types";
import {
  actionOf,
  isActionEnabled,
  isActionId,
  isActionPressed,
  type ActionState,
} from "./actions";
import { ActionIcon } from "./icons";
import { SEPARATOR } from "./layout";
import { actionTexts, toolbarName, useAnyT, type RunAction } from "./useActions";
import type { DragHandlers } from "./useToolbarDrag";

/** Props of {@link Toolbar}. */
export interface ToolbarProps {
  /** The toolbar. */
  bar: ToolbarLayout;
  /** The live state, for enabled and pressed. */
  state: ActionState;
  /** Carries an action out. */
  run: RunAction;
  /** Drag handlers for the grip or title bar. */
  drag: DragHandlers;
  /** Opens the grip menu below an element. */
  onMenu: (anchor: HTMLElement) => void;
  /** Hides the toolbar (close button of a floating one). */
  onHide: () => void;
}

/**
 * One toolbar.
 *
 * @param props - See {@link ToolbarProps}.
 * @returns The toolbar element (`data-toolbar-id` names it for dragging).
 */
export function Toolbar({ bar, state, run, drag, onMenu, onHide }: ToolbarProps) {
  const t = useAnyT();
  const name = toolbarName(bar, t);
  const floating = bar.dock === "float";
  const vertical = bar.dock === "left" || bar.dock === "right";
  /** Which item Tab lands on: an action ID or "grip". */
  const [stop, setStop] = useState("grip");
  // The button may have been customized away: then the grip takes over.
  const tabStop = bar.items.includes(stop) ? stop : "grip";

  /** Arrow keys, Home and End move the focus inside the toolbar. */
  function onKeyDown(event: KeyboardEvent<HTMLElement>): void {
    const items = [...event.currentTarget.querySelectorAll<HTMLElement>("[data-toolbar-item]")];
    const at = items.findIndex((el) => el === document.activeElement);
    if (at === -1) return;
    const [forward, back] = vertical ? ["ArrowDown", "ArrowUp"] : ["ArrowRight", "ArrowLeft"];
    let next: number;
    if (event.key === forward) next = (at + 1) % items.length;
    else if (event.key === back) next = (at - 1 + items.length) % items.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = items.length - 1;
    else return;
    event.preventDefault();
    items[next]?.focus();
  }

  const grip = (
    <button
      type="button"
      data-toolbar-item="grip"
      tabIndex={tabStop === "grip" ? 0 : -1}
      aria-haspopup="menu"
      aria-label={t("toolbars.grip", { name })}
      title={t("toolbars.gripTitle")}
      className={`flex shrink-0 cursor-move touch-none items-center justify-center rounded text-fg-muted select-none hover:bg-app ${
        vertical ? "h-4 w-full" : "w-4 self-stretch"
      }`}
      {...(floating ? {} : drag)}
      onFocus={() => setStop("grip")}
      onClick={(e) => onMenu(e.currentTarget)}
    >
      <span aria-hidden="true">{vertical ? "⋯" : "⋮"}</span>
    </button>
  );

  return (
    <div
      role="toolbar"
      aria-label={t("toolbars.label", { name })}
      aria-orientation={vertical ? "vertical" : "horizontal"}
      data-toolbar-id={bar.id}
      className={
        floating
          ? "w-max max-w-full rounded border border-line bg-surface text-sm shadow-lg"
          : vertical
            ? "flex w-20 shrink-0 flex-col gap-0.5 rounded border border-line bg-surface p-0.5 text-[0.7rem]"
            : "flex max-w-full gap-0.5 rounded border border-line bg-surface p-0.5 text-sm"
      }
      onKeyDown={onKeyDown}
    >
      {floating ? (
        <div
          className="flex cursor-move touch-none items-stretch gap-1 border-b border-line bg-app px-1 text-xs select-none"
          data-titlebar
          {...drag}
        >
          {grip}
          <span className="min-w-0 truncate py-1">{name}</span>
          <button
            type="button"
            data-toolbar-item="close"
            tabIndex={-1}
            aria-label={t("toolbars.close", { name })}
            title={t("toolbars.close", { name })}
            className="ml-auto rounded px-1.5 hover:bg-surface"
            // Pressing × must not start a drag of the title bar around it.
            onPointerDown={(e) => e.stopPropagation()}
            onClick={onHide}
          >
            <span aria-hidden="true">×</span>
          </button>
        </div>
      ) : (
        grip
      )}
      <div className={vertical ? "flex flex-col gap-0.5" : "flex flex-wrap gap-0.5"}>
        {bar.items.length === 0 && (
          <span className="px-2 py-1 text-fg-muted italic">{t("toolbars.empty")}</span>
        )}
        {bar.items.map((item, i) => {
          if (item === SEPARATOR) {
            return (
              <div
                key={`separator-${i}`}
                role="separator"
                aria-orientation={vertical ? "horizontal" : "vertical"}
                className={vertical ? "my-0.5 border-t border-line" : "mx-0.5 border-l border-line"}
              />
            );
          }
          if (!isActionId(item)) return null;
          const texts = actionTexts(item, t, undefined, state);
          const enabled = isActionEnabled(item, state);
          const action = actionOf(item);
          return (
            <button
              key={item}
              type="button"
              data-toolbar-item={item}
              tabIndex={tabStop === item ? 0 : -1}
              aria-disabled={!enabled}
              aria-pressed={isActionPressed(item, state)}
              aria-keyshortcuts={action.aria}
              aria-haspopup={action.opensDialog ? "dialog" : undefined}
              aria-label={texts.label}
              title={texts.title}
              className={`flex items-center rounded border border-transparent hover:bg-app aria-disabled:opacity-40 aria-disabled:hover:bg-transparent aria-pressed:border-accent aria-pressed:bg-accent aria-pressed:text-accent-fg ${
                vertical
                  ? "flex-col gap-0.5 px-0.5 py-1 text-center leading-tight break-words"
                  : "gap-1 px-2 py-1 whitespace-nowrap"
              }`}
              onFocus={() => setStop(item)}
              onClick={() => {
                if (enabled) run(item, "button");
              }}
            >
              <ActionIcon id={item} />
              <span className={vertical ? "w-full" : ""}>{texts.short}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
