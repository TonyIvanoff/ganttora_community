/**
 * components/ui/Tooltips.tsx: the app's own tooltips.
 *
 * What it is for:
 *   The system draws the tooltips of the `title` attribute in a small, fixed
 *   size. This component shows the same texts in a larger box that also
 *   follows the UI zoom (the zoom scales everything on the page).
 *
 * Where it fits:
 *   `shell/AppShell.tsx` renders it once; every `title` in the app uses it.
 *   The chart has its own tooltip (`features/gantt/GanttView.tsx`).
 *
 * Main ideas:
 *   - Components just write `title="…"`. While the mouse is on an element,
 *     its title (and those around it) is emptied, so the system shows
 *     nothing; it is put back when the mouse leaves.
 *   - The box is a *popover*: above everything, also above dialogs.
 */

import { useEffect, useRef } from "react";

/** How long the mouse rests before the tooltip appears. */
export const TOOLTIP_DELAY_MS = 400;
/** Space between the mouse and the tooltip, and to the window's edge. */
const GAP_PX = 16;
const EDGE_PX = 4;

/**
 * Draws the tooltip of the element under the mouse.
 *
 * @returns The (mostly hidden) box.
 */
export function Tooltips() {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    /** Emptied elements with their texts. */
    const emptied = new Map<Element, string>();
    /** The element the tooltip belongs to. */
    let owner: Element | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let x = 0;
    let y = 0;

    function hide(): void {
      clearTimeout(timer);
      const el = ref.current;
      if (el === null || el.hidden) return;
      (el as Partial<Pick<HTMLElement, "hidePopover">>).hidePopover?.();
      el.hidden = true;
    }

    function show(text: string): void {
      const el = ref.current;
      if (el === null || owner?.isConnected !== true) return;
      el.textContent = text;
      el.hidden = false;
      (el as Partial<Pick<HTMLElement, "showPopover">>).showPopover?.();
      const { offsetWidth: width, offsetHeight: height } = el;
      const left = Math.min(x, window.innerWidth - width - EDGE_PX);
      // Above the mouse when there is no room below.
      const below = y + GAP_PX;
      const top = below + height <= window.innerHeight ? below : y - GAP_PX / 2 - height;
      el.style.left = `${Math.max(EDGE_PX, left)}px`;
      el.style.top = `${Math.max(EDGE_PX, top)}px`;
    }

    /** Gives back the titles of elements the mouse has left. */
    function restore(stillInside: Element | null): void {
      for (const [el, text] of emptied) {
        if (stillInside !== null && el.contains(stillInside)) continue;
        // Not if React has changed or removed the title meanwhile.
        if (el.getAttribute("title") === "") el.setAttribute("title", text);
        emptied.delete(el);
      }
    }

    function onOver(event: MouseEvent): void {
      const target = event.target instanceof Element ? event.target : null;
      x = event.clientX;
      y = event.clientY;
      restore(target);
      let next: Element | null = null;
      for (let el = target; el !== null; el = el.parentElement) {
        const title = el.getAttribute("title");
        if (title === null) {
          // React removed it: forget the old text.
          emptied.delete(el);
          continue;
        }
        if (title !== "") {
          emptied.set(el, title);
          el.setAttribute("title", "");
        }
        if (emptied.has(el)) next ??= el;
      }
      if (next === owner) return;
      hide();
      owner = next;
      const text = next === null ? undefined : emptied.get(next);
      // Not while a mouse button is down (dragging).
      if (text === undefined || event.buttons !== 0) return;
      timer = setTimeout(() => show(text), TOOLTIP_DELAY_MS);
    }

    function onMove(event: MouseEvent): void {
      x = event.clientX;
      y = event.clientY;
    }

    function onLeave(): void {
      hide();
      owner = null;
      restore(null);
    }

    // `true`: hear the events before anything can stop them.
    document.addEventListener("mouseover", onOver, true);
    document.addEventListener("mousemove", onMove, true);
    document.documentElement.addEventListener("mouseleave", onLeave);
    const hiders = ["mousedown", "keydown", "wheel", "scroll", "dragstart"] as const;
    for (const name of hiders) document.addEventListener(name, hide, true);
    return () => {
      onLeave();
      document.removeEventListener("mouseover", onOver, true);
      document.removeEventListener("mousemove", onMove, true);
      document.documentElement.removeEventListener("mouseleave", onLeave);
      for (const name of hiders) document.removeEventListener(name, hide, true);
    };
  }, []);

  return (
    <div
      ref={ref}
      role="tooltip"
      popover="manual"
      hidden
      className="pointer-events-none fixed z-50 m-0 max-w-lg rounded border border-line bg-surface px-3 py-2 text-base whitespace-pre-line text-fg shadow-lg"
      style={{ inset: "auto" }}
    />
  );
}
