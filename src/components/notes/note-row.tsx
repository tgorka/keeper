/**
 * One note row (Epic 37, Story 37.2, FR-113/FR-114/FR-119, AD-63).
 *
 * 64 px, matching chat-row density, and a pure projection of one
 * {@link NoteRowVm}: nothing here derives a fact or decides an order. What the
 * row does compute is presentation of facts Rust already settled — the relative
 * timestamp, through the same formatter the approval pane uses, because a clock
 * is a rendering concern and Rust has no business shipping "2 hr ago" over IPC.
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
 *
 * The `+n` beside the chips is a truncation, so AD-83 applies to it (Story
 * 44.12): it opens the tags it is hiding rather than merely counting them. A
 * row that says `+2` and cannot say which two is the same failure as a property
 * cut with nowhere to read the rest, in a smaller box.
 *
 * The order (Story 44.5, AD-81) is shown beside the note because an ordering the
 * reader cannot account for reads as randomness. It is the note's own frontmatter
 * value, and the row never invents one: a note that stated no position shows the
 * default it was given, dimmer, and a note whose `order` is not a number shows
 * the default it fell back to plus a mark saying so. All three cases are named in
 * the accessible label, because `aria-label` overrides the row's contents and a
 * number only drawn is a number a screen reader user never receives.
 */
import { AlertTriangle, Pin } from "lucide-react";
import type { Ref } from "react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { formatDraftAge } from "@/lib/format-time";
import type { NoteOrder, NoteRowVm } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

/** How many tag chips a row shows before it collapses the rest into `+n`. */
const VISIBLE_TAGS = 3;

/** The accessible name of the `+n` chip, suffixed with the count. Named so a
 * test and a screen reader agree on what the affordance is. */
export const NOTE_MORE_TAGS_LABEL = "More tags on this note:";

/**
 * The mark an order the note itself could not state carries, so the fallback has
 * a carrier that is not colour (UX-DR43). `order: soon` is not a number; the note
 * still sorts at the default, and the row has to say the file and the list
 * disagree rather than showing a bare `0` nobody can account for.
 */
export const NOTE_ORDER_UNREADABLE_MARK = "?";

/** The displayed order: the number, plus {@link NOTE_ORDER_UNREADABLE_MARK}
 * where the note's own value could not be read. */
export function formatNoteOrder(order: NoteOrder): string {
  const value = `${order.value}`;
  return order.source === "unreadable" ? `${value}${NOTE_ORDER_UNREADABLE_MARK}` : value;
}

/**
 * How the order is announced. The row's `aria-label` overrides its contents for
 * name computation, so a number rendered and not named here is a number a screen
 * reader user never receives — and then the ordering is exactly as unaccountable
 * for them as it was for everyone before this story.
 */
export function noteOrderLabel(order: NoteOrder): string {
  switch (order.source) {
    case "own":
      return `order ${order.value}`;
    case "default":
      return `order ${order.value}, the default`;
    default:
      return `order ${order.value}, the default; this note's own order is not a number`;
  }
}

export function NoteRow({
  row,
  selected,
  tabIndex,
  onSelect,
  onSelectBeside,
  onToggleTag,
  ref,
}: {
  row: NoteRowVm;
  selected: boolean;
  tabIndex: number;
  onSelect: (row: NoteRowVm) => void;
  /** Double click: open this note beside what is open (Story 46.12, AD-90). */
  onSelectBeside: (row: NoteRowVm) => void;
  /** Clicking a tag chip filters by it; it never opens the note. */
  onToggleTag: (tag: string) => void;
  ref?: Ref<HTMLButtonElement>;
}) {
  const shownTags = row.tags.slice(0, VISIBLE_TAGS);
  const hiddenTags = row.tags.slice(VISIBLE_TAGS);
  const overflow = hiddenTags.length;
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
    noteOrderLabel(row.order),
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
      // Story 46.12: the Files tree's pair. The single click that necessarily
      // preceded this one is undone by the panel store, so the note that was
      // showing comes back rather than being replaced by a second copy of this
      // one — which is why no timer swallows the first click here either.
      onDoubleClick={() => onSelectBeside(row)}
      className={cn(
        // `items-center`, not `items-start`. Top-aligned content in a 64px box
        // pooled all of a row's slack UNDER its text, so the gutter between two
        // rows was 18px of one row plus 8px of the next and the boundary sat
        // nowhere in particular. Centred, the slack is equal above and below
        // and the boundary falls in the middle of a measurable gap — which is
        // how the chat list, the recordings list and the Files tree all read.
        "flex h-16 w-full items-center gap-2 px-3 py-2 text-left outline-none",
        "focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
        selected ? "bg-accent text-accent-foreground" : "hover:bg-accent/50",
        // A conflict is loss in progress, so it gets the destructive edge; a pin
        // is only a preference and gets none.
        row.conflict && "border-destructive border-l-[3px]",
      )}
    >
      {/* The unread dot appears at full opacity and never animates (UX-DR39):
          information that twitches trains people to ignore it.

          Filled for unread, HOLLOW for read, and never absent — it used to be
          `bg-transparent` once read, which cost two things. It carried its one
          state in colour alone, where DESIGN.md asks for a filled/hollow pair
          and never a bare dot. And it left this list with no anchor: every
          other list in the app draws no row rule and gets its rhythm from a
          mark repeating at a constant x down the column — the chat list's
          avatar and account bar, a recording row's card edge, a tree row's file
          icon. A list of read notes had an empty lane and nothing to repeat,
          which is the sense in which its row boundary went missing. The answer
          is the app's answer, not a hairline per row: one rule here would make
          the notes list the only ruled list in keeper, which is heavier than
          the app rather than more legible. */}
      <span
        aria-hidden="true"
        data-slot="unread-dot"
        className={cn(
          "size-2 shrink-0 rounded-full",
          row.unread ? "bg-primary" : "border border-border",
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
          {/* The number the sort actually used, beside the note it placed. An
              ordering the reader cannot account for reads as randomness, and
              this is the cheapest possible account of it (Story 44.5, AD-81).
              Set in the register's mono face: this is a column of figures read
              down rather than across, and it only reads as a column if the
              digits are the same width in every row. */}
          <span
            data-slot="note-order"
            data-order-source={row.order.source}
            className={cn(
              "ml-auto shrink-0 font-mono text-xs",
              row.order.source === "own" && "text-foreground",
              // A note that never stated a position is quieter than one that
              // did: a column of identical defaults should read as "nobody
              // ordered these", not as data. Quieter is a step down the text
              // ramp, not a step through it — this number is still the fact the
              // sort used, so it stays at the 4.5:1 metadata tone rather than
              // being faded below it.
              row.order.source === "default" && "text-muted-foreground",
              row.order.source === "unreadable" && "text-destructive",
            )}
          >
            {formatNoteOrder(row.order)}
          </span>
          <span className="figures shrink-0 text-muted-foreground text-xs">
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
              className="shrink-0 rounded-full bg-muted px-1.5 py-0.5 text-meta text-muted-foreground leading-none outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
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
            <Popover>
              <PopoverTrigger asChild>
                <button
                  type="button"
                  aria-label={`${NOTE_MORE_TAGS_LABEL} ${hiddenTags.join(", ")}`}
                  className="shrink-0 rounded-full px-1 py-0.5 text-meta text-muted-foreground leading-none outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
                  onClick={(event) => {
                    // Same rule as a chip: this opens the tags, not the note.
                    event.stopPropagation();
                  }}
                >
                  +{overflow}
                </button>
              </PopoverTrigger>
              <PopoverContent
                align="end"
                className="w-56 gap-1"
                // The trigger lives inside the row button, so a click landing on
                // the panel must not travel back out and open the note.
                onClick={(event) => event.stopPropagation()}
              >
                <p className="font-medium text-muted-foreground text-xs">{NOTE_MORE_TAGS_LABEL}</p>
                <span className="flex max-h-40 flex-wrap gap-1 overflow-y-auto">
                  {hiddenTags.map((tag) => (
                    <button
                      key={tag}
                      type="button"
                      aria-label={`Tag ${tag}, on this note`}
                      className="rounded-full bg-muted px-1.5 py-0.5 text-meta text-muted-foreground leading-none outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
                      onClick={(event) => {
                        event.stopPropagation();
                        onToggleTag(tag);
                      }}
                    >
                      {tag}
                    </button>
                  ))}
                </span>
              </PopoverContent>
            </Popover>
          )}
        </span>
      </span>
    </button>
  );
}
