import { Check } from "lucide-react";
import { useEffect, useRef } from "react";
import { PopoverContent } from "@/components/ui/popover";
import type { NoteTagTerm } from "@/lib/ipc/client";
import type { TagChip } from "@/lib/stores/notes-filters";
import { cn } from "@/lib/utils";

export function TagSuggest({
  id,
  matches,
  active,
  chips,
  phone,
  loading,
  error,
  onChoose,
}: {
  id: string;
  matches: readonly string[];
  active: number;
  chips: readonly TagChip[];
  phone: boolean;
  loading: boolean;
  error: string | null;
  onChoose: (tag: string, term: NoteTagTerm) => void;
}) {
  const list = useRef<HTMLDivElement>(null);
  useEffect(() => {
    list.current?.ownerDocument
      .getElementById(`${id}-${active}`)
      ?.scrollIntoView?.({ block: "nearest" });
  }, [active, id]);
  return (
    <PopoverContent
      align="start"
      sideOffset={4}
      collisionPadding={8}
      className="w-[var(--radix-popover-trigger-width)] max-w-[calc(100vw-16px)] gap-0 rounded-[10px] p-1"
      onOpenAutoFocus={(event) => event.preventDefault()}
      onCloseAutoFocus={(event) => event.preventDefault()}
    >
      <div
        ref={list}
        id={id}
        role="listbox"
        aria-label="Tag suggestions"
        className={cn("overflow-y-auto", phone ? "max-h-[264px]" : "max-h-48")}
      >
        {loading ? (
          <p className="p-2 text-xs">Loading tags…</p>
        ) : error ? (
          <p role="status" className="p-2 text-xs">
            {error}
          </p>
        ) : (
          matches.flatMap((tag, index) =>
            (["include", "exclude"] as const).map((term, sign) => (
              // biome-ignore lint/a11y/useKeyWithClickEvents: the field owns Arrow/Enter navigation via aria-activedescendant, matching TagCombobox.
              <div
                key={`${tag}-${term}`}
                id={`${id}-${index * 2 + sign}`}
                role="option"
                tabIndex={-1}
                aria-label={`${term === "include" ? "Include" : "Exclude"} tag ${tag}`}
                aria-selected={active === index * 2 + sign}
                onPointerDown={(event) => event.preventDefault()}
                onClick={() => onChoose(tag, term)}
                className={cn(
                  "flex cursor-default items-center gap-1 rounded-[7px] px-2 text-sm aria-selected:bg-accent",
                  phone ? "min-h-11" : "min-h-8",
                )}
              >
                <span aria-hidden="true">{term === "include" ? "+" : "−"}</span>
                <span className="min-w-0 flex-1 truncate">{tag}</span>
                {chips.some((chip) => chip.tag === tag && chip.term === term) && (
                  <Check aria-label="Already applied" className="size-3 shrink-0" />
                )}
              </div>
            )),
          )
        )}
      </div>
    </PopoverContent>
  );
}
