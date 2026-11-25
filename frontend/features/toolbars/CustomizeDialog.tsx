/**
 * features/toolbars/CustomizeDialog.tsx: "Customize toolbars".
 *
 * What it is for:
 *   Show and hide toolbars; create, rename and delete your own; choose and
 *   order the buttons and lines of any toolbar; reset one or all.
 *
 * Where it fits:
 *   Rendered by `shell/AppShell.tsx`; opened from View → Toolbars, the
 *   "Toolbars" button or a grip menu. Every change goes through
 *   `toolbarStore.ts` and is saved at once, so there is no Cancel.
 */

import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { SimpleDialog } from "@frontend/components/ui/SimpleDialog";
import type { ToolbarLayout } from "@frontend/ipc/types";
import { ACTION_IDS, isActionId } from "./actions";
import {
  addToolbar,
  isPredefined,
  MAX_TOOLBAR_ITEMS,
  MAX_TOOLBAR_NAME,
  MAX_TOOLBARS,
  moveItem,
  patchToolbar,
  predefinedToolbar,
  removeToolbar,
  SEPARATOR,
} from "./layout";
import { useToolbarStore } from "./toolbarStore";
import { actionTexts, toolbarName, useAnyT } from "./useActions";

/** Look of the dialog's buttons. */
const BUTTON =
  "rounded border border-line px-2 py-1 whitespace-nowrap hover:bg-app disabled:opacity-50";
/** Look of its lists and fields. */
const FIELD = "rounded border border-line bg-surface px-2 py-1 text-fg";

/**
 * The dialog.
 *
 * @returns The dialog element, invisible while closed.
 */
export function CustomizeDialog() {
  const { t } = useTranslation();
  return (
    <SimpleDialog
      dialog="customize"
      title={t("customize.title")}
      closeLabel={t("customize.close")}
      maxWidth="52rem"
    >
      <CustomizeForm />
    </SimpleDialog>
  );
}

/**
 * The dialog's content. It exists only while the dialog is open, so every
 * opening starts fresh.
 */
function CustomizeForm() {
  const t = useAnyT();
  const toolbars = useToolbarStore((s) => s.toolbars);
  const update = useToolbarStore((s) => s.update);
  const reset = useToolbarStore((s) => s.reset);
  const [chosen, setChosen] = useState("main");
  // The chosen one may have been deleted: fall back to the first.
  const bar = toolbars.find((b) => b.id === chosen) ?? toolbars[0];

  /** Creates a toolbar called "Toolbar 4" and chooses it. */
  function create(): void {
    const id = `custom-${crypto.randomUUID()}`;
    update((bars) => addToolbar(bars, id, t("customize.newName", { number: bars.length + 1 })));
    setChosen(id);
  }

  return (
    <div className="flex flex-col gap-3">
      <p className="text-fg-muted">{t("customize.intro")}</p>
      <div className="flex flex-wrap gap-4">
        <fieldset className="flex min-w-44 flex-col gap-1">
          <legend className="mb-1 font-medium">{t("customize.toolbars")}</legend>
          {toolbars.map((b) => {
            const name = toolbarName(b, t);
            return (
              <div key={b.id} className="flex items-center gap-2">
                <input
                  type="checkbox"
                  aria-label={t("customize.shown", { name })}
                  checked={b.visible}
                  onChange={(e) =>
                    update((bars) => patchToolbar(bars, b.id, { visible: e.target.checked }))
                  }
                />
                <button
                  type="button"
                  aria-pressed={b.id === bar?.id}
                  className="min-w-0 flex-1 truncate rounded px-2 py-1 text-left hover:bg-app aria-pressed:bg-accent aria-pressed:text-accent-fg"
                  onClick={() => setChosen(b.id)}
                >
                  {name}
                </button>
              </div>
            );
          })}
          <button
            type="button"
            className={`${BUTTON} mt-1`}
            disabled={toolbars.length >= MAX_TOOLBARS}
            onClick={create}
          >
            {t("customize.new")}
          </button>
          <button type="button" className={BUTTON} onClick={reset}>
            {t("customize.resetAll")}
          </button>
        </fieldset>
        {bar !== undefined && <ItemsEditor key={bar.id} bar={bar} />}
      </div>
    </div>
  );
}

/** Props of {@link ItemsEditor}. */
interface ItemsEditorProps {
  /** The toolbar being edited. */
  bar: ToolbarLayout;
}

/**
 * The right part: name, available buttons, buttons on the toolbar.
 *
 * @param props - See {@link ItemsEditorProps}.
 */
function ItemsEditor({ bar }: ItemsEditorProps) {
  const t = useAnyT();
  const baseId = useId();
  const update = useToolbarStore((s) => s.update);
  const [name, setName] = useState(bar.name);
  const [available, setAvailable] = useState<string>(SEPARATOR);
  /** Index of the chosen entry on the toolbar; -1 = none. */
  const [at, setAt] = useState(-1);
  const predefined = isPredefined(bar.id);

  /** Stores new entries and chooses one of them. */
  function setItems(items: string[], index: number): void {
    update((bars) => patchToolbar(bars, bar.id, { items }));
    setAt(Math.min(index, items.length - 1));
  }

  /** Text of an entry. */
  const label = (item: string) =>
    isActionId(item) ? actionTexts(item, t).label : t("customize.separator");

  /** Adds the chosen available entry after the chosen one (or at the end). */
  function add(): void {
    if (!choices.includes(available) || bar.items.length >= MAX_TOOLBAR_ITEMS) return;
    const place = at === -1 ? bar.items.length : at + 1;
    setItems([...bar.items.slice(0, place), available, ...bar.items.slice(place)], place);
  }

  /** Saves the typed name; an empty one goes back to the old name. */
  function rename(): void {
    const next = name.trim();
    if (next === "") setName(bar.name);
    else if (next !== bar.name) update((bars) => patchToolbar(bars, bar.id, { name: next }));
  }

  // A line may appear many times, a button once.
  const choices: string[] = [SEPARATOR, ...ACTION_IDS.filter((id) => !bar.items.includes(id))];

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        {predefined ? (
          <>
            <span className="font-medium">{toolbarName(bar, t)}</span>
            <button
              type="button"
              className={BUTTON}
              onClick={() => {
                if (isPredefined(bar.id)) setItems(predefinedToolbar(bar.id).items, -1);
              }}
            >
              {t("customize.resetOne")}
            </button>
          </>
        ) : (
          <>
            <label htmlFor={`${baseId}-name`}>{t("customize.rename")}</label>
            <input
              id={`${baseId}-name`}
              className={`${FIELD} min-w-0`}
              value={name}
              maxLength={MAX_TOOLBAR_NAME}
              onChange={(e) => setName(e.target.value)}
              onBlur={rename}
              onKeyDown={(e) => {
                if (e.key === "Enter") rename();
              }}
            />
            <button
              type="button"
              className={BUTTON}
              onClick={() => update((bars) => removeToolbar(bars, bar.id))}
            >
              {t("customize.delete")}
            </button>
          </>
        )}
      </div>
      <div className="flex flex-wrap items-start gap-2">
        <div className="flex min-w-44 flex-1 flex-col gap-1">
          <label htmlFor={`${baseId}-available`}>{t("customize.available")}</label>
          <select
            id={`${baseId}-available`}
            size={12}
            className={FIELD}
            value={available}
            onChange={(e) => setAvailable(e.target.value)}
            onDoubleClick={add}
          >
            {choices.map((item) => (
              <option key={item} value={item}>
                {label(item)}
              </option>
            ))}
          </select>
        </div>
        <div className="flex flex-col gap-1 pt-6">
          <button type="button" className={BUTTON} onClick={add}>
            {t("customize.add")}
          </button>
          <button
            type="button"
            className={BUTTON}
            disabled={at < 0}
            onClick={() =>
              setItems(
                bar.items.filter((_, i) => i !== at),
                at,
              )
            }
          >
            {t("customize.remove")}
          </button>
          <button
            type="button"
            className={BUTTON}
            disabled={at <= 0}
            onClick={() => setItems(moveItem(bar.items, at, -1), at - 1)}
          >
            {t("customize.up")}
          </button>
          <button
            type="button"
            className={BUTTON}
            disabled={at < 0 || at >= bar.items.length - 1}
            onClick={() => setItems(moveItem(bar.items, at, 1), at + 1)}
          >
            {t("customize.down")}
          </button>
        </div>
        <div className="flex min-w-44 flex-1 flex-col gap-1">
          <label htmlFor={`${baseId}-current`}>{t("customize.current")}</label>
          <select
            id={`${baseId}-current`}
            size={12}
            className={FIELD}
            value={at}
            onChange={(e) => setAt(Number(e.target.value))}
          >
            {bar.items.map((item, i) => (
              <option key={`${i}-${item}`} value={i}>
                {item === SEPARATOR ? "———" : label(item)}
              </option>
            ))}
          </select>
        </div>
      </div>
    </div>
  );
}
