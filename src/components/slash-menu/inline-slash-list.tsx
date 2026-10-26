/**
 * The mid-text `/` menu (Story 101.3, R418), drawn on the same cmdk surface as
 * the whole-line menu ({@link SlashMenuList}) and anchored the same way, above
 * the composer. The keyboard stays in the textarea ({@link useInlineSlash});
 * a pointer over a row moves the highlight and a click inserts it.
 */
import { AtSign, FileText, Smile } from "lucide-react";
import { useLayoutEffect, useRef } from "react";
import { INLINE_MENU_LABEL } from "@/components/slash-menu/inline-slash";
import { Command, CommandItem, CommandList } from "@/components/ui/command";
import type { InlineSlashState } from "@/hooks/use-inline-slash";
import type { ChatInlineKindVm } from "@/lib/ipc/gen/ChatInlineKindVm";

const KIND_ICON: Record<ChatInlineKindVm, typeof AtSign> = {
  mention: AtSign,
  file: FileText,
  shrug: Smile,
};

export function InlineSlashList({
  menu,
  label = INLINE_MENU_LABEL,
}: {
  menu: InlineSlashState;
  /** The menu's accessible name: the mid-text menu's unless a composer names its own. */
  label?: string;
}) {
  const listRef = useRef<HTMLDivElement>(null);
  const { setIds, open, rows } = menu;
  useLayoutEffect(() => {
    const list = listRef.current;
    if (!open || list === null || rows.length === 0) {
      setIds(null);
      return;
    }
    const options: Record<string, string> = {};
    for (const option of list.querySelectorAll<HTMLElement>("[data-inline-row]")) {
      options[option.dataset.inlineRow ?? ""] = option.id;
    }
    setIds({ list: list.id, options });
  }, [open, rows, setIds]);
  if (!open) {
    return null;
  }
  const active = rows[menu.highlight];
  return (
    <div
      className="absolute right-0 bottom-full left-0 z-20 mb-2 overflow-hidden rounded-md border border-border bg-popover text-popover-foreground shadow-md"
      // The textarea keeps the focus: a click on a row must not blur it.
      onPointerDown={(event) => event.preventDefault()}
    >
      <Command
        shouldFilter={false}
        value={active?.key ?? ""}
        onValueChange={(key) => {
          const index = rows.findIndex((row) => row.key === key);
          if (index !== -1) {
            menu.setHighlight(index);
          }
        }}
        className="bg-transparent"
      >
        <CommandList ref={listRef} label={label} className="max-h-64 p-1">
          {rows.map((row) => {
            const Icon = KIND_ICON[row.kind];
            const key = row.key;
            return (
              <CommandItem
                key={key}
                value={key}
                data-inline-row={key}
                onSelect={() => menu.accept(row)}
              >
                <Icon aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate font-mono">{row.label}</span>
                  <span className="truncate text-muted-foreground text-xs">{row.detail}</span>
                </span>
              </CommandItem>
            );
          })}
        </CommandList>
      </Command>
      {menu.note !== null && (
        <p
          role="status"
          className="border-border border-t px-3 py-1.5 text-muted-foreground text-xs"
        >
          {menu.note}
        </p>
      )}
    </div>
  );
}
