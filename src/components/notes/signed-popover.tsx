import { Check, Minus, Plus } from "lucide-react";
import { type ComponentProps, useEffect, useId, useRef } from "react";
import { Input } from "@/components/ui/input";
import { PopoverContent } from "@/components/ui/popover";
import type { NoteTagTerm } from "@/lib/ipc/client";
import type { TagChip } from "@/lib/stores/notes-filters";
import { cn } from "@/lib/utils";

export const TAG_TERM_PAINT = {
  include: "bg-accent text-accent-foreground",
  exclude: "bg-destructive/15 text-destructive",
} as const;

export interface SignedPopoverProps {
  matches: readonly string[];
  chips: readonly TagChip[];
  onChoose: (tag: string, term: NoteTagTerm) => void;
  phone?: boolean;
  id?: string;
  active?: number;
  loading?: boolean;
  error?: string | null;
  query?: { value: string; onChange: (value: string) => void };
  onCloseAutoFocus?: ComponentProps<typeof PopoverContent>["onCloseAutoFocus"];
}

/** The same signed choices for the caret, explicit browser, and note-row tag. */
export function SignedPopover({
  matches,
  chips,
  onChoose,
  phone = false,
  id: providedId,
  active,
  loading = false,
  error = null,
  query,
  onCloseAutoFocus,
}: SignedPopoverProps) {
  const generatedId = useId();
  const id = providedId ?? generatedId;
  const list = useRef<HTMLDivElement | HTMLFieldSetElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const caret = active !== undefined;
  useEffect(() => {
    if (active !== undefined)
      document.getElementById(`${id}-${active}`)?.scrollIntoView?.({ block: "nearest" });
  }, [active, id]);
  const choices = loading ? (
    <p className="p-2 text-xs">Loading tags…</p>
  ) : error ? (
    <p role="status" className="p-2 text-xs">
      {error}
    </p>
  ) : matches.length === 0 ? (
    <p className="p-2 text-xs">
      {query?.value ? "No matching tags." : "This vault has no tags yet."}
    </p>
  ) : (
    matches.flatMap((tag, index) =>
      (["include", "exclude"] as const).map((term, sign) => {
        const Sign = term === "include" ? Plus : Minus;
        const selection = caret
          ? ({ role: "option", "aria-selected": active === index * 2 + sign } as const)
          : {};
        return (
          <button
            key={`${tag}-${term}`}
            id={`${id}-${index * 2 + sign}`}
            type="button"
            {...selection}
            tabIndex={caret ? -1 : 0}
            aria-label={`${term === "include" ? "Include" : "Exclude"} tag ${tag}`}
            onPointerDown={caret ? (event) => event.preventDefault() : undefined}
            onClick={() => onChoose(tag, term)}
            onKeyDown={(event) => {
              if (caret || !["ArrowDown", "ArrowUp"].includes(event.key)) return;
              event.preventDefault();
              const buttons = list.current?.querySelectorAll<HTMLButtonElement>("button");
              if (!buttons?.length) return;
              buttons[
                (index * 2 + sign + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) %
                  buttons.length
              ]?.focus();
            }}
            className={cn(
              "flex w-full items-center gap-2 rounded-[7px] px-2 text-left text-sm font-medium outline-none hover:ring-2 hover:ring-inset hover:ring-ring focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring aria-selected:ring-2 aria-selected:ring-inset aria-selected:ring-ring",
              TAG_TERM_PAINT[term],
              phone ? "min-h-11" : "min-h-8",
            )}
          >
            <span className="flex w-6 shrink-0 justify-center">
              <Sign aria-hidden="true" className="size-3.5" />
            </span>
            <span
              className={cn(
                "min-w-0 flex-1 truncate",
                term === "exclude" && "line-through decoration-destructive/60",
              )}
            >
              {tag}
            </span>
            <span className="size-4 shrink-0">
              {chips.some((chip) => chip.tag === tag && chip.term === term) && (
                <Check aria-label="Already applied" className="size-4" />
              )}
            </span>
          </button>
        );
      }),
    )
  );
  const listClassName = cn("space-y-0 overflow-y-auto", phone ? "max-h-[264px]" : "max-h-48");
  return (
    <PopoverContent
      align="start"
      sideOffset={4}
      collisionPadding={8}
      // Portalled choices still bubble through the owning note row in React.
      onClick={(event) => event.stopPropagation()}
      onDoubleClick={(event) => event.stopPropagation()}
      className={cn(
        "max-w-[calc(100vw-16px)] gap-1 rounded-[10px] p-1",
        caret ? "w-[var(--radix-popover-trigger-width)]" : "w-[280px]",
      )}
      onOpenAutoFocus={(event) => {
        event.preventDefault();
        if (query) search.current?.focus();
        else if (!caret) list.current?.querySelector<HTMLButtonElement>("button")?.focus();
      }}
      onCloseAutoFocus={onCloseAutoFocus ?? (caret ? (event) => event.preventDefault() : undefined)}
    >
      {query && (
        <Input
          ref={search}
          aria-label="Find a tag"
          value={query.value}
          className={phone ? "h-11" : "h-8"}
          onChange={(event) => query.onChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown" && !event.nativeEvent.isComposing) {
              event.preventDefault();
              list.current?.querySelector<HTMLButtonElement>("button")?.focus();
            } else if (event.key === "Enter" && !event.nativeEvent.isComposing && matches[0]) {
              event.preventDefault();
              onChoose(matches[0], "include");
            }
          }}
        />
      )}
      {caret ? (
        <div
          ref={(element) => {
            list.current = element;
          }}
          id={id}
          role="listbox"
          aria-label="Tag suggestions"
          className={listClassName}
        >
          {choices}
        </div>
      ) : (
        <fieldset
          ref={(element) => {
            list.current = element;
          }}
          id={id}
          aria-label="Tag suggestions"
          className={cn("min-w-0", listClassName)}
        >
          {choices}
        </fieldset>
      )}
    </PopoverContent>
  );
}
