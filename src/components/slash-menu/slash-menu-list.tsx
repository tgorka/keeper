/**
 * The one slash menu every composer draws (Story 61.9; the messenger composer
 * since Story 100.5, R385) and the `/help` panel beside it.
 *
 * Rendered on the shared cmdk surface (`ui/command.tsx`) with
 * `shouldFilter={false}`: the rows arrive filtered and ordered by Rust, and cmdk
 * only draws them and their highlight — the palette's contract (AD-20). The
 * keyboard stays in the composer's textarea ({@link useSlashMenu}); cmdk's own
 * keyboard handling never sees a key, so it cannot disagree with it. A pointer
 * over a row moves the highlight and a click accepts it.
 *
 * cmdk mints the listbox's and each option's id itself and lets nobody
 * override them, so the drawn menu reads them back after each render and hands
 * them to the hook ({@link SlashMenuState.setIds}); the textarea then names
 * the listbox it controls and the highlighted option as its active descendant
 * (R393). The ids are React's, so two composers on one screen never collide.
 *
 * Anchored above the composer: the parent is `relative` and the menu sits at
 * its top edge, over the timeline, so opening it moves nothing on screen.
 */

import { useLayoutEffect, useRef } from "react";
import { argumentHint, type SlashPreview, type SlashRow } from "@/components/slash-menu/slash-menu";
import { Button } from "@/components/ui/button";
import { Command, CommandItem, CommandList } from "@/components/ui/command";
import type { SlashMenuState } from "@/hooks/use-slash-menu";

/** The menu's accessible name, in every composer. */
export const SLASH_MENU_LABEL = "Commands";

/** How a row names its other spellings. */
export const slashAliases = (aliases: readonly string[]) =>
  `also ${aliases.map((alias) => `/${alias}`).join(", ")}`;

export function SlashMenuList<P extends SlashPreview>({ menu }: { menu: SlashMenuState<P> }) {
  const listRef = useRef<HTMLDivElement>(null);
  const { setIds, open, rows: drawnRows } = menu;
  // What cmdk called the list and each row, read whenever the drawn rows
  // change — or nothing, once the menu closes.
  useLayoutEffect(() => {
    const list = listRef.current;
    if (!open || list === null || drawnRows.length === 0) {
      setIds(null);
      return;
    }
    const options: Record<string, string> = {};
    for (const option of list.querySelectorAll<HTMLElement>("[data-slash-row]")) {
      options[option.dataset.slashRow ?? ""] = option.id;
    }
    setIds({ list: list.id, options });
  }, [open, drawnRows, setIds]);
  if (!menu.open || menu.preview === null) {
    return null;
  }
  const { preview, rows, active } = menu;
  return (
    <div
      className="absolute right-0 bottom-full left-0 z-20 mb-2 overflow-hidden rounded-md border border-border bg-popover text-popover-foreground shadow-md"
      // The textarea keeps the focus: a click on a row must not blur it.
      onPointerDown={(event) => event.preventDefault()}
    >
      <Command
        shouldFilter={false}
        value={active?.name ?? ""}
        onValueChange={(name) => {
          const index = rows.findIndex((row) => row.name === name);
          if (index !== -1) {
            menu.setHighlight(index);
          }
        }}
        className="bg-transparent"
      >
        <CommandList ref={listRef} label={SLASH_MENU_LABEL} className="max-h-64 p-1">
          {rows.map((row) => (
            <CommandItem
              key={row.name}
              value={row.name}
              data-slash-row={row.name}
              onSelect={() => menu.accept(row)}
            >
              <SlashRowBody row={row} />
            </CommandItem>
          ))}
        </CommandList>
        {preview.note != null && (
          <p className="border-border border-t px-3 py-1.5 text-muted-foreground text-xs">
            {preview.note}
          </p>
        )}
        {/* Taught where it is relevant: a leading slash on screen is the moment
            somebody may have meant a path. */}
        <p className="border-border border-t px-3 py-1.5 text-muted-foreground text-xs">
          {preview.escapeHint}
        </p>
      </Command>
    </div>
  );
}

/** One row: name, hint, aliases, the registry's sentence, and why not. */
function SlashRowBody({ row }: { row: SlashRow }) {
  const hint = argumentHint(row);
  return (
    <span className="flex min-w-0 flex-1 flex-col">
      <span className="flex min-w-0 flex-wrap items-baseline gap-x-2">
        <span className="font-mono">/{row.name}</span>
        {hint !== null && <span className="font-mono text-muted-foreground text-xs">{hint}</span>}
        {row.aliases.length > 0 && (
          <span className="text-muted-foreground text-xs">{slashAliases(row.aliases)}</span>
        )}
      </span>
      <span className="min-w-0 text-muted-foreground text-xs">{row.description}</span>
      {/* A reason says why Enter will not run it; a warning says what to expect
          when Enter does. Both are the registry's words. */}
      {row.reason !== null && <span className="text-destructive text-xs">{row.reason}</span>}
      {row.warning != null && <span className="text-muted-foreground text-xs">{row.warning}</span>}
    </span>
  );
}

/** The `/help` panel's accessible name, and its close verb. */
export const SLASH_HELP_LABEL = "Commands keeper knows";
export const SLASH_HELP_CLOSE = "Close";

/**
 * Every command the composer's registry offers here, as `/help` shows it: the
 * rows of a bare `/`, and the escape, which is the one thing nobody can guess
 * from the menu.
 */
export function SlashHelp({
  rows,
  escapeHint,
  onClose,
}: {
  rows: readonly SlashRow[];
  escapeHint: string;
  onClose: () => void;
}) {
  return (
    <section
      aria-label={SLASH_HELP_LABEL}
      className="flex flex-col gap-2 rounded-md border border-border p-3"
    >
      <ul className="flex flex-col gap-1 text-sm">
        {rows.map((row) => {
          const hint = argumentHint(row);
          return (
            <li key={row.name} className="flex min-w-0 flex-wrap gap-x-2">
              <span className="font-mono">
                /{row.name}
                {hint === null ? "" : ` ${hint}`}
              </span>
              <span className="min-w-0 flex-1 text-muted-foreground">{row.description}</span>
            </li>
          );
        })}
      </ul>
      <p className="text-muted-foreground text-xs">{escapeHint}</p>
      <div className="flex">
        <Button type="button" variant="outline" size="sm" onClick={onClose}>
          {SLASH_HELP_CLOSE}
        </Button>
      </div>
    </section>
  );
}
