/**
 * components/ui/SimpleDialog.tsx: a dialog with a title, content and one
 * Close button.
 *
 * What it is for:
 *   The shared frame of About and Customize toolbars.
 *
 * Where it fits:
 *   Used by `features/about/AboutDialog.tsx`
 *   and `features/toolbars/CustomizeDialog.tsx`. Opened through
 *   `shell/dialogStore.ts`.
 *
 * Main ideas:
 *   - The native `<dialog>` with `showModal()` keeps the focus inside and
 *     closes on Escape. The content exists only while it is open.
 */

import { useEffect, useId, useRef, type ReactNode } from "react";
import { useDialogStore, type AppDialog } from "@frontend/shell/dialogStore";

/** Props of {@link SimpleDialog}. */
export interface SimpleDialogProps {
  /** Which dialog of the store this is. */
  dialog: AppDialog;
  /** The title (translated). */
  title: string;
  /** Text of the Close button (translated). */
  closeLabel: string;
  /** Widest size, a CSS length. */
  maxWidth?: string;
  /** The content. */
  children: ReactNode;
}

/**
 * The dialog. Renders nothing visible while closed.
 *
 * @param props - See {@link SimpleDialogProps}.
 *
 * @example
 * <SimpleDialog dialog="about" title={t("about.title")} closeLabel={t("about.close")}>…</SimpleDialog>
 */
export function SimpleDialog({
  dialog,
  title,
  closeLabel,
  maxWidth = "28rem",
  children,
}: SimpleDialogProps) {
  const open = useDialogStore((s) => s.open === dialog);
  const close = useDialogStore((s) => s.close);
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();

  useEffect(() => {
    const el = ref.current;
    if (el === null) return;
    if (open && !el.open) el.showModal();
    else if (!open && el.open) el.close();
  }, [open]);

  return (
    <dialog
      ref={ref}
      aria-labelledby={titleId}
      className="m-auto max-h-[calc(100vh-2rem)] w-[calc(100vw-2rem)] rounded-lg border border-line bg-surface p-0 text-fg shadow-xl backdrop:bg-black/40"
      style={{ maxWidth }}
      onClose={close}
    >
      {open && (
        <div className="flex max-h-[calc(100vh-2rem)] flex-col">
          <h2 id={titleId} className="border-b border-line px-4 py-3 text-base font-semibold">
            {title}
          </h2>
          <div className="min-h-0 overflow-auto px-4 py-3 text-sm">{children}</div>
          <div className="flex justify-end border-t border-line px-4 py-3">
            <button
              type="button"
              className="rounded border border-accent bg-accent px-3 py-1 text-sm whitespace-nowrap text-accent-fg"
              onClick={close}
            >
              {closeLabel}
            </button>
          </div>
        </div>
      )}
    </dialog>
  );
}
