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
 *
 * **Right-click is the row's menu, not the WebView's.** Until now this row
 * mounted none, so the macOS WebView answered a right-click with its own
 * default for selected text — Look Up, Translate, Search with Google, Share,
 * Show Writing Tools — a menu about a text selection that knows nothing about
 * the note under the pointer. The owner reported it against 0.8.6 as "right
 * click na notes uzyj analogicznego menu jak w files", and that is exactly the
 * fix: the construction below is `files-pane.tsx`'s, verbatim. One Radix
 * `ContextMenu` whose trigger is the row itself (`asChild`, so the window and
 * the list keep the DOM they always had), paired with `useLongPress` for the
 * phone tier — the same pattern as `chat-row`, `favorites-section`,
 * `networks-group`, `pins-strip` and the Files tree, and not a sixth idiom.
 * Radix's trigger calls `preventDefault()` on the `contextmenu` event, which
 * is the whole of what takes the native menu away.
 *
 * **Every item is a verb the row already had.** The two panel items are the
 * click and the double click this row already answers to; the rest are the
 * keys the list already binds. Nothing is invented, which is also why there is
 * no Copy path beside the Files tree's: a note row carries a VAULT-RELATIVE
 * path, AD-65 forbids this side of the wire from joining it to a vault root,
 * and no command hands back the absolute one — so the string the Files pane
 * puts on the clipboard has nothing behind it here.
 *
 * **Keyboard parity is the list's, and no new keystroke is added here.** A
 * menu reachable only by right-click is unreachable for a keyboard user, so
 * the question is what answers it. The Files tree answers with focusable row
 * controls and nothing else: it binds no `Shift+F10` and mounts no menu key of
 * its own, leaving the platform's own `contextmenu` keystroke as the only way
 * in. This list answers better and already did — every item below is a bare
 * key on the focused row (`p`, `e`, `u`, `Delete`/`⌫`) or a chord (`⌘⇧R`),
 * except "Open in a new panel", which is a pointer gesture in the Files tree
 * too (Story 46.12) and stays one here rather than growing a twin this list
 * would be alone in having. The menu is a second route to verbs that were
 * already reachable, which is the only shape of context menu that is not an
 * accessibility regression.
 */
import { AlertTriangle, Pin } from "lucide-react";
import type { Ref } from "react";
import { NOTE_DELETE_LABEL } from "@/components/notes/note-actions";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { useLongPress } from "@/hooks/use-long-press";
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

/**
 * The menu's two panel verbs, worded exactly as the Files tree words them
 * (`FILES_OPEN_HERE_LABEL`, `FILES_OPEN_BESIDE_LABEL`): the same two panel
 * targets, so the same two sentences. Spelled here rather than imported, which
 * is what every surface in this repo does with a shared label — see
 * `RECORDINGS_REVEAL_LABEL`, `NOTE_REVEAL_LABEL` and `EXPORT_REVEAL_LABEL`,
 * three consts of one string, each naming its twins in a comment.
 */
export const NOTE_ROW_OPEN_HERE_LABEL = "Open in this panel";
export const NOTE_ROW_OPEN_BESIDE_LABEL = "Open in a new panel";

/**
 * The flag verbs, worded as the chat list's row menu words them
 * (`chat-row.tsx`). This list already borrowed that list's keys — `e`
 * archives, `p` pins, `u` acknowledges — so borrowing its wording is one
 * grammar rather than two.
 *
 * `Mark read` has no `Mark unread` twin, unlike the chat row's pair:
 * `notes_mark_read` is the whole of the read-state surface, so the item is
 * offered on an unread row and absent from a read one rather than being drawn
 * as a control that would do nothing.
 */
export const NOTE_ROW_MARK_READ_LABEL = "Mark read";
export const NOTE_ROW_PIN_LABEL = "Pin";
export const NOTE_ROW_UNPIN_LABEL = "Unpin";
export const NOTE_ROW_ARCHIVE_LABEL = "Archive";
export const NOTE_ROW_UNARCHIVE_LABEL = "Unarchive";

/** Reveal, worded identically to every other Reveal in keeper. Absent — never
 * disabled — where the platform has no user-visible file manager, which is the
 * rule the Files tree, the recordings browser and the completion card all
 * follow: a control that fails on activation is worse than no control. */
export const NOTE_ROW_REVEAL_LABEL = "Reveal in Finder";

/** The row verbs the list binds to keys and the menu offers by name. */
export type NoteRowVerb = "e" | "p" | "u" | "r" | "d";

export function NoteRow({
  row,
  selected,
  tabIndex,
  canReveal,
  onSelect,
  onSelectBeside,
  onToggleTag,
  onVerb,
  ref,
}: {
  row: NoteRowVm;
  selected: boolean;
  tabIndex: number;
  /**
   * The platform has a user-visible file manager. Read once by the list rather
   * than per row, and passed down so the menu can leave Reveal out instead of
   * drawing one that would fail on activation.
   */
  canReveal: boolean;
  onSelect: (row: NoteRowVm) => void;
  /** Double click: open this note beside what is open (Story 46.12, AD-90). */
  onSelectBeside: (row: NoteRowVm) => void;
  /** Clicking a tag chip filters by it; it never opens the note. */
  onToggleTag: (tag: string) => void;
  /**
   * The row's verbs, dispatched exactly as the list's keys dispatch them — one
   * handler, so a menu item and its keystroke cannot come to do different
   * things. `d` asks: it opens the confirmation and never deletes.
   */
  onVerb: (row: NoteRowVm, verb: NoteRowVerb) => void;
  ref?: Ref<HTMLButtonElement>;
}) {
  // The phone tier's way into the menu below: a ≥500ms stationary press
  // dispatches the synthetic `contextmenu` the Radix trigger is already
  // listening for. Off the phone tier every handler is a no-op.
  const longPress = useLongPress();
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

  const rowButton = (
    <button
      ref={ref}
      type="button"
      aria-label={label}
      aria-current={selected ? "true" : undefined}
      tabIndex={tabIndex}
      data-slot="note-row"
      data-unread={row.unread ? "true" : undefined}
      data-conflict={row.conflict ? "true" : undefined}
      {...longPress}
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

  // The Files tree's menu, item for item where the two surfaces mean the same
  // thing: the panel pair first, a rule, then the verbs that change the note,
  // then a rule and the destructive one last — the position `NoteActions`
  // already gives Delete, so the item under the cursor when the menu opens is
  // never the one that removes the note.
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{rowButton}</ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onSelect={() => onSelect(row)}>{NOTE_ROW_OPEN_HERE_LABEL}</ContextMenuItem>
        <ContextMenuItem onSelect={() => onSelectBeside(row)}>
          {NOTE_ROW_OPEN_BESIDE_LABEL}
        </ContextMenuItem>
        <ContextMenuSeparator />
        {/* Only where there is something to acknowledge. A row with no commit
            has no revision to mark read against, which is the same condition
            `markNoteRead` refuses on — checked here so the menu never offers a
            verb that would return without doing anything. */}
        {row.unread && row.headRev !== "" && (
          <ContextMenuItem onSelect={() => onVerb(row, "u")}>
            {NOTE_ROW_MARK_READ_LABEL}
          </ContextMenuItem>
        )}
        <ContextMenuItem onSelect={() => onVerb(row, "p")}>
          {row.pinned ? NOTE_ROW_UNPIN_LABEL : NOTE_ROW_PIN_LABEL}
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => onVerb(row, "e")}>
          {row.archived ? NOTE_ROW_UNARCHIVE_LABEL : NOTE_ROW_ARCHIVE_LABEL}
        </ContextMenuItem>
        {canReveal && (
          <ContextMenuItem onSelect={() => onVerb(row, "r")}>
            {NOTE_ROW_REVEAL_LABEL}
          </ContextMenuItem>
        )}
        <ContextMenuSeparator />
        <ContextMenuItem variant="destructive" onSelect={() => onVerb(row, "d")}>
          {NOTE_DELETE_LABEL}
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}
