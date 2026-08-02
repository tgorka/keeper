/**
 * One note row (Epic 37, Story 37.2, FR-113/FR-114/FR-119, AD-63).
 *
 * 64 px, matching chat-row density, and a pure projection of one
 * {@link NoteRowVm}: nothing here derives a fact, composes a sentence, or
 * decides an order. The one thing the row computes is the relative timestamp,
 * through the same formatter the approval pane uses, because a clock is a
 * rendering concern and Rust has no business shipping "2 hr ago" over IPC.
 *
 * The second line is the interesting decision. On a read row it is the body
 * excerpt; on an UNREAD row it is replaced by the provenance line — "changed by
 * agent · hesperia · 2 h ago", composed in Rust (AD-63) and rendered verbatim.
 * That swap is not decoration: for a row you have not read, the useful fact is
 * who touched it, not what it says, and showing the excerpt would answer the
 * question you did not ask.
 *
 * Bold means unread here exactly as it does in the chat list. Conflict and pin
 * are glyphs rather than colour, because colour alone is not a carrier
 * (UX-DR43) and a conflict has to read on a monochrome panel too.
 */
import { AlertTriangle, Pin } from "lucide-react";
import type { Ref } from "react";
import { formatDraftAge } from "@/lib/format-time";
import type { NoteRowVm } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

/** How many tag chips a row shows before it collapses the rest into `+n`. */
const VISIBLE_TAGS = 3;

export function NoteRow({
  row,
  selected,
  tabIndex,
  onSelect,
  onToggleTag,
  ref,
}: {
  row: NoteRowVm;
  selected: boolean;
  tabIndex: number;
  onSelect: (row: NoteRowVm) => void;
  /** Clicking a tag chip filters by it; it never opens the note. */
  onToggleTag: (tag: string) => void;
  ref?: Ref<HTMLButtonElement>;
}) {
  const shownTags = row.tags.slice(0, VISIBLE_TAGS);
  const overflow = row.tags.length - shownTags.length;
  // The accessible name puts state before content, but only where state changes
  // what the row MEANS — which for an unread, agent-touched note it does.
  const label = [
    "Note",
    row.title,
    row.unread ? "unread" : null,
    row.unread && row.origin !== "" ? row.origin : null,
    row.conflict ? "conflicted" : null,
    row.pinned ? "pinned" : null,
    row.tags.length > 0 ? `${row.tags.length} tags` : null,
  ]
    .filter((part) => part !== null)
    .join(", ");

  return (
    <button
      ref={ref}
      type="button"
      aria-label={label}
      aria-current={selected ? "true" : undefined}
      tabIndex={tabIndex}
      data-slot="note-row"
      data-unread={row.unread ? "true" : undefined}
      data-conflict={row.conflict ? "true" : undefined}
      onClick={() => onSelect(row)}
      className={cn(
        "flex h-16 w-full items-start gap-2 px-3 py-2 text-left outline-none",
        "focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
        selected ? "bg-accent text-accent-foreground" : "hover:bg-accent/50",
        // A conflict is loss in progress, so it gets the destructive edge; a pin
        // is only a preference and gets none.
        row.conflict && "border-destructive border-l-[3px]",
      )}
    >
      {/* The unread dot appears at full opacity and never animates (UX-DR39):
          information that twitches trains people to ignore it. */}
      <span
        aria-hidden="true"
        data-slot="unread-dot"
        className={cn(
          "mt-1.5 size-2 shrink-0 rounded-full",
          row.unread ? "bg-primary" : "bg-transparent",
        )}
      />
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="flex min-w-0 items-center gap-1.5">
          <span className={cn("min-w-0 truncate text-sm", row.unread && "font-semibold")}>
            {row.title}
          </span>
          {row.pinned && (
            <Pin aria-hidden="true" data-slot="pin-glyph" className="size-3 shrink-0" />
          )}
          {row.conflict && (
            <AlertTriangle
              aria-hidden="true"
              data-slot="conflict-glyph"
              className="size-3 shrink-0 text-destructive"
            />
          )}
          <span className="ml-auto shrink-0 text-muted-foreground text-xs">
            {formatDraftAge(row.updatedMs)}
          </span>
        </span>
        <span className="flex min-w-0 items-center gap-1.5">
          <span className="min-w-0 flex-1 truncate text-muted-foreground text-xs">
            {row.unread && row.origin !== "" ? row.origin : row.snippet}
          </span>
          {shownTags.map((tag) => (
            // A real button, not a styled span: it changes what the list shows,
            // so it has to be reachable and announce what it does.
            <button
              key={tag}
              type="button"
              aria-label={`Tag ${tag}, on this note`}
              className="shrink-0 rounded-full bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground leading-none outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
              onClick={(event) => {
                // The chip filters; the row opens. Without this the two fight.
                event.stopPropagation();
                onToggleTag(tag);
              }}
            >
              {tag}
            </button>
          ))}
          {overflow > 0 && (
            <span className="shrink-0 text-[11px] text-muted-foreground leading-none">
              +{overflow}
            </span>
          )}
        </span>
      </span>
    </button>
  );
}
