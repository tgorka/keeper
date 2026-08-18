/**
 * The board itself (FR-263, FR-264): four columns of markdown files, dragged
 * between them.
 *
 * **The cards are files, and that is the whole design.** A card's column is its
 * `status:` and its position is its `order:`. There is no board state anywhere —
 * not in this component, not in a database, not in a sidecar. Dropping a card
 * writes two frontmatter keys and the surface re-reads, which is why the same
 * board shows the same thing whether it was Obsidian, an agent or this component
 * that last moved a card (AD-110).
 *
 * **It lives in `notes/` and not in `sessions/` because it is not a session's.**
 * The operator asked for the board *"as the widget inside the md file I could
 * use in the notes as well - not only in the sessions"*, and a component that
 * knew about a session root would have had to be copied to serve that. What is a
 * session's — a root id, a session id, the sessions plan executor — stays in
 * {@link "@/components/sessions/session-board"}, which is now a set of
 * coordinates and one `onMove`. The note-side widget supplies its own.
 *
 * **The columns are Rust's, not this file's.** {@link BOARD_COLUMNS} mirrors the
 * closed `TaskStatus` set, and a card whose status is none of them is shown in
 * its own row rather than silently dropped — a task nobody can see is worse than
 * a task in the wrong place, and the flat shape's one real failure mode is a file
 * no view selects. A widget board over an arbitrary query may hold cards with any
 * word at all in `status:`, so that row is the common case there rather than the
 * exception.
 *
 * **No drag-and-drop library.** The repo has none, and one board is not the
 * reason to take a dependency that owns pointer input across the whole app. The
 * gesture is {@link "@/hooks/use-pointer-drag"}, shared with
 * {@link "@/components/layout/pins-strip"} — with that surface's lesson applied:
 * indices go stale, so every release is guarded and the *authoritative* answer
 * comes from the re-read afterwards, never from this component's own idea of the
 * order.
 *
 * **Drag is not the only way to move a card.** Every card carries a column menu
 * that does the same write, because a drag needs a pointer, a working hand and a
 * screen tall enough to show two columns at once. The keyboard path is the
 * accessible one and the pointer path is the fast one; they are the same verb.
 *
 * **The gesture is pointer events, and HTML5 drag is gone from this file.** Two
 * stories patched `dataTransfer` — correctly — and the board still moved nothing
 * on macOS, because the defect is below the page: Tauri installs a wry drag-drop
 * handler that always claims the event
 * (`tauri-runtime-wry-2.11.4/src/lib.rs:4862-4896`) and wry's macOS WKWebView
 * subclass forwards `performDragOperation:` to `super` only when it does not
 * (`wry-0.55.1/src/wkwebview/drag_drop.rs:88-95`), so `dragstart` and `dragover`
 * fire and the page's `drop` cannot. `draggable`, `onDragStart`, `onDrop` and
 * `dataTransfer` are therefore REMOVED rather than left beside the pointer path:
 * two mechanisms for one verb is how the dead one survived two epics under a
 * suite of green jsdom tests. See {@link "@/hooks/use-pointer-drag"} for the
 * source lines and for why `dragDropEnabled: false` is not the fix.
 *
 * **The whole column box is the target, and the cue is drawn on it.** The `<ul>`
 * used to be what took a drop while the box drew the highlight, and the `<ul>`
 * did not fill the box — so the padding, the header and everything below
 * `min-h-16` were dead while looking live. There is no `dragover` in a pointer
 * gesture, so the target is hit-tested from the pointer's position against the
 * column boxes' own rects, measured on every move (see {@link BoardDrop}).
 *
 * **The menu is demoted, not removed.** Four permanent select boxes are four
 * columns of chrome, so it is revealed on hover and on `focus-within` — the
 * {@link "@/components/sessions/session-tree"} row idiom. It stays in the DOM
 * and in the tab order at all times: opacity is not visibility, and a keyboard
 * user tabbing to it brings it back.
 *
 * **The dragged card follows the pointer, and settles when it lands.** What
 * `draggable="true"` was quietly buying, besides a drag session, was a drag image
 * and a browser that never selected text while one was running; Story 53.1
 * removed the attribute and replaced neither, so the card went half-transparent
 * *in place* while the pointer walked away from it and every drag painted a
 * selection across the app (Story 54.1, FR-323/FR-324). The follow is this repo's
 * own idiom, from {@link "@/components/chat/chat-row"} (`:459-461`): a transform
 * from the live delta, with the transition withheld **while** the gesture is live
 * so the card tracks 1:1, and restored on release so it settles back rather than
 * teleporting. `useReducedMotion` cuts the landing transition and never the
 * follow — direct manipulation is not animation. The selection is stopped where
 * it is anchored: the press cancels its own default, which is
 * {@link "@/components/ui/resizable-columns"} (`:202-203`), and the hook holds a
 * `user-select: none` on the document for as long as the drag lasts. The panel
 * takes focus from the `pointerdown` for the same reason — a cancelled press
 * fires no `mousedown` at all, so `panel-strip.tsx` cannot wait for one.
 *
 * **A transform has two consequences this file pays for by hand.** It moves the
 * box `getBoundingClientRect` reports, so the drop tally SKIPS the lifted card
 * rather than measuring it at the pointer ({@link dropAt}); and it is clipped by
 * the pane the board sits in, so the follow is capped to the board's own box
 * ({@link followTransform}). The settle is withheld from a card whose write is
 * still in flight, because the release commit still paints it in its source
 * column and a restored transition would glide it BACKWARDS from the drop point
 * to the slot it came from, a round trip before the re-read moves it.
 */
import { GripVertical, TriangleAlert } from "lucide-react";
import { useRef, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { type PointerDragDelta, usePointerDrag } from "@/hooks/use-pointer-drag";
import { useReducedMotion } from "@/hooks/use-reduced-motion";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/**
 * The four columns, in the order work moves through them.
 *
 * Left to right is the direction a card travels — prepared, then queued, then
 * finished — with the one that leaves that flow at the end. That ordering is not
 * cosmetic: a board whose columns are alphabetical asks the reader to remember
 * which way is forward, and on a board the answer should be the layout.
 *
 * The keys are the strings Rust parses and writes (`TaskStatus::as_str`), quoted
 * rather than derived, because they are the file format: a card's `status:` is
 * one of these four words and an operator typing one into frontmatter by hand
 * must get the same result as one who dragged.
 */
export const BOARD_COLUMNS: ReadonlyArray<{ status: string; label: string }> = [
  { status: "in-preparation", label: "In preparation" },
  { status: "todo", label: "To do" },
  { status: "done", label: "Done" },
  { status: "deferred", label: "Deferred" },
];

/** What an empty column says, so an empty board still reads as a board. */
export const BOARD_EMPTY_COLUMN = "Nothing here";

/** The heading of the row for cards whose status is not one of the four. */
export const BOARD_STRAY_HEADING = "Not in a column";

/** Why a stray card is shown rather than hidden. */
export const BOARD_STRAY_HINT =
  "These files are tagged `task` but their `status:` is not one of the four columns. Fix the key in the file, or move the card into a column here.";

/** The label of the per-card column menu. */
export const BOARD_MOVE_LABEL = "Move to column";

/** What is said when a move is refused and keeper sent no sentence of its own. */
export const BOARD_MOVE_FAILED =
  "keeper could not move that card. The files may have changed on disk since the board was drawn — reopen it.";

/** How a card that keeper had to default an `order:` for is described. */
export const BOARD_ORDER_DEFAULTED =
  "This file states no order:, so keeper placed the card by title. Drag it once to give it a position of its own.";

/** How a card with no ULID is described. */
export const BOARD_UNSTABLE_ID =
  "This file has no id:, so keeper tracks it by path — renaming it loses pins and marks. keeper does not stamp a file it did not write.";

/**
 * One card, in the terms the board draws rather than in either host's own.
 *
 * `key` is whatever the host's move command addresses a card by — a
 * session-relative path for a session board, a note id for a widget one. The
 * board never composes it, never parses it and never shows it; it hands it back
 * to `onOpen` and `onMove` exactly as it was given. That is the whole of what
 * made this component reusable, and it is the same discipline AD-65 applies to
 * paths and queries.
 */
export interface BoardCard {
  key: string;
  title: string;
  /** The file's own word, or `null` when it states none. Never normalised: a
   *  card in no column keeps the only fact that explains why. */
  status: string | null;
  order: number;
  /** The file stated its position itself, rather than taking the default. */
  orderIsOwn: boolean;
  tags: readonly string[];
  /** keeper tracks this file by path because it has no `id:`. */
  unstableIdentity: boolean;
}

/**
 * The cards of one column, in the order they are rendered.
 *
 * Sorted the way Rust sorts them — `order` ascending, ties broken by folded
 * title — so the index this component sends is the index Rust resolves against.
 * A component that rendered one order and reported another would put cards
 * wherever the difference happened to fall.
 */
function columnOf(cards: readonly BoardCard[], status: string): BoardCard[] {
  return cards
    .filter((card) => card.status === status)
    .sort(
      (a, b) =>
        a.order - b.order ||
        a.title.localeCompare(b.title, undefined, { sensitivity: "base" }) ||
        a.key.localeCompare(b.key),
    );
}

/**
 * Where a release would land: a column, and the slot in it.
 *
 * Resolved by {@link dropAt} from two attributes this file writes and reads and
 * nothing else does: `data-board-column` on each column box — the element a
 * release lands on and the element the cue is drawn on — and `data-card-key` on
 * each card.
 */
interface BoardDrop {
  status: string;
  /** The insertion slot among the column's *rendered* cards. */
  index: number;
}

/**
 * Hit-test a viewport point against the board's columns.
 *
 * A pointer gesture has no `dragover`, so nothing tells this component what the
 * pointer is over: it measures. Rects are read fresh on every call rather than
 * cached at the press, which is what makes a column scrolled — or re-laid-out by
 * the drag's own preview — resolve where it now is instead of where it was.
 * `document.elementFromPoint` would be the shorter spelling and is the wrong one
 * twice: jsdom implements no layout and so does not implement it at all, and it
 * answers with the topmost element, which during a gesture is the pressed card.
 *
 * The slot is the number of cards whose vertical midpoint is above the pointer,
 * counting every card in the column EXCEPT the one being dragged. One rule covers
 * every case the old per-card `onDrop` needed three for: the header (nothing above
 * it, so the top of the column), the gap between two cards, and the empty space
 * below the last one (everything above, so the end).
 *
 * **The lifted card is excluded, and that is not an optimisation.** It carries the
 * follow's `transform`, and `getBoundingClientRect` returns the TRANSFORMED border
 * box — so a card dragged inside its own column measured itself at the pointer,
 * where its contribution reduces to `height / 2 < grabOffsetY`: a constant for the
 * whole gesture, decided by where inside the card the user grabbed it. A card
 * dragged to the bottom of its own column was written to the TOP. Skipping it also
 * makes the answer already an index into the column WITHOUT the moved card, which
 * is the index Rust resolves against — so there is no off-by-one left to
 * compensate for, in either direction, and no second rule for the within-column
 * case to get wrong. The remaining cards are untransformed and stay in flow, so
 * their midpoints are the slots the user can see.
 */
function dropAt(root: HTMLElement | null, clientX: number, clientY: number): BoardDrop | null {
  if (root === null) {
    return null;
  }
  const box = Array.from(root.querySelectorAll<HTMLElement>("[data-board-column]")).find(
    (column) => {
      const rect = column.getBoundingClientRect();
      return (
        clientX >= rect.left && clientX < rect.right && clientY >= rect.top && clientY < rect.bottom
      );
    },
  );
  const status = box?.dataset.boardColumn;
  if (box === undefined || status === undefined) {
    return null;
  }
  const index = Array.from(
    box.querySelectorAll<HTMLElement>('[data-card-key]:not([data-dragging="true"])'),
  ).filter((card) => {
    const rect = card.getBoundingClientRect();
    return rect.top + rect.height / 2 < clientY;
  }).length;
  return { status, index };
}

/**
 * How far the followed card may be translated and still lie inside the board.
 *
 * Measured at the press rather than per move, and that is not a cached-geometry
 * slip of the kind {@link dropAt}'s header warns about: these are the card's
 * offsets from the board's own edges, and the card is INSIDE the board, so a
 * mid-gesture scroll moves both by the same amount and leaves all four
 * unchanged. What a per-move read would have to measure instead is a box that
 * already carries the transform it is being asked to bound.
 */
interface FollowBounds {
  minX: number;
  maxX: number;
  minY: number;
  maxY: number;
}

function followBounds(card: HTMLElement, root: HTMLElement | null): FollowBounds | null {
  if (root === null) {
    return null;
  }
  const box = card.getBoundingClientRect();
  const within = root.getBoundingClientRect();
  return {
    minX: within.left - box.left,
    maxX: within.right - box.right,
    minY: within.top - box.top,
    maxY: within.bottom - box.bottom,
  };
}

/**
 * The follow, capped to the board's own box.
 *
 * **Why capped at all.** The follow is an inline transform on an `<li>` that
 * stays in flow, and both hosts put the board inside a clipping ancestor:
 * `session-detail.tsx:457` wraps the sessions board in `overflow-y-auto`, and
 * `panel-strip.tsx:645` gives every panel `overflow-hidden`. A transformed
 * descendant is clipped by an overflow ancestor and its transformed box joins
 * that ancestor's scrollable overflow — so an uncapped follow made the card
 * vanish behind the pane edge exactly where aiming matters, and made every
 * downward drag in session detail grow the scroll range and shrink the thumb for
 * the duration of the gesture. `opacity-50` makes the card a stacking context,
 * so no `z-index` could have rescued it.
 *
 * **Why capped rather than lifted into a fixed layer.** A `position: fixed`
 * proxy escapes the clip only while no ancestor is a containing block for it,
 * and it costs either a second node carrying this card's own `data-card-key` —
 * which {@link dropAt} counts — or taking the real card out of flow mid-drag,
 * which collapses the hole it left and re-lays the column out under the pointer.
 * The cap keeps one node, one flow and one measurement. What it costs is the
 * last few pixels of 1:1 travel at the board's edge, where there is nothing to
 * drop onto: the pointer still moves freely, the hit-test still follows the
 * pointer, and a release beyond the board is the same no-op it always was.
 *
 * The pointer is always inside the card's transformed box — the press began
 * inside the card and both move by the same delta — so a card held inside the
 * board is a card near a pointer that is itself on screen.
 */
function followTransform(delta: PointerDragDelta, limit: FollowBounds | null): string {
  if (limit === null) {
    return `translate(${delta.x}px, ${delta.y}px)`;
  }
  // A card larger than the board on an axis has no room on it at all, and stays
  // put there rather than being pinned to whichever edge came first.
  const x = limit.maxX < limit.minX ? 0 : Math.min(Math.max(delta.x, limit.minX), limit.maxX);
  const y = limit.maxY < limit.minY ? 0 : Math.min(Math.max(delta.y, limit.minY), limit.maxY);
  return `translate(${x}px, ${y}px)`;
}

export function TaskBoard({
  heading,
  cards,
  empty,
  onOpen,
  onMove,
  moveFailed = BOARD_MOVE_FAILED,
}: {
  /** The section's own heading, which is also its accessible name. */
  heading: string;
  cards: readonly BoardCard[];
  /** What to say when there are no cards at all — how one is made differs by
   *  host, and a sentence that named the wrong way would be worse than none. */
  empty: string;
  /** Open a card's file — a card is a note, and reading it is the point. */
  onOpen: (key: string) => void;
  /** Write the move, and re-read. Rejecting is how a refusal is reported: the
   *  board shows keeper's own sentence and changes nothing. */
  onMove: (key: string, status: string, index: number) => Promise<void>;
  /** What to say when a refusal carries no sentence of its own. */
  moveFailed?: string;
}) {
  const [notice, setNotice] = useState<string | null>(null);
  /**
   * The card whose move keeper is still writing, if any.
   *
   * The settle transition is withheld from it, because the release commit paints
   * it still in its SOURCE column: `forget()` has already zeroed the transform,
   * and restoring `transition-transform` in that same commit interpolates the
   * card from the drop point BACK to the slot it came from — the opposite
   * direction — until the write and the re-read land it a round trip later. The
   * three returning cases (released over nowhere, cancelled, refused) are the
   * ones that want that travel; a successful landing wants the card to sit still.
   * A refusal is only known a round trip later, so it teleports back rather than
   * gliding: the sentence is what reports it, and a wrong animation on the common
   * case is the worse trade.
   */
  const [landing, setLanding] = useState<string | null>(null);
  /** The board's own root, so the hit-test never crosses two mounted boards. */
  const board = useRef<HTMLElement>(null);
  /** The pressed card's room inside the board, measured at the press. */
  const bounds = useRef<FollowBounds | null>(null);
  // The landing transition's gate, and only the landing's: a live follow is the
  // card under the finger, which is the distinction `chat-row.tsx:459` draws.
  const reducedMotion = useReducedMotion();

  const known = new Set(BOARD_COLUMNS.map((column) => column.status));
  const strays = cards.filter((card) => card.status === null || !known.has(card.status));

  const move = async (key: string, status: string, index: number) => {
    setNotice(null);
    try {
      await onMove(key, status, index);
    } catch (error) {
      // keeper's own sentence when it sent one — it knows what it refused and
      // this component does not (UX-DR43).
      setNotice(syncErrorMessage(error, moveFailed));
    }
  };

  /**
   * The gesture: press a card, move it, release it over a column.
   *
   * `onRelease` is handed a null target for a press that never passed the slop,
   * which is the whole of "a press that does not move stays a click" — the title
   * button's own `onClick` runs, untouched. The hook swallows exactly one click
   * after a drag, so the release of a card whose title happened to be under the
   * pointer does not also open the file.
   *
   * The slot is written as {@link dropAt} measured it: that tally already skips
   * the lifted card, so it is already an index into the column WITHOUT it, for a
   * card dragged within its own column and for one arriving from another alike.
   * The off-by-one compensation this release used to apply is retired rather than
   * kept beside it — two corrections for one index is how the wrong one survives.
   */
  const drag = usePointerDrag<string, BoardDrop>({
    resolve: (clientX, clientY) => dropAt(board.current, clientX, clientY),
    // The card translates by the live delta, so the hook has to publish it.
    trackDelta: true,
    onRelease: (key, target) => {
      if (target === null) {
        return;
      }
      // Before the commit that ends the drag, so that commit already knows this
      // card's transform must not be animated back to where it came from.
      setLanding(key);
      void move(key, target.status, target.index).finally(() => {
        // Only if this card's own write is the one still in flight: a second drop
        // landing first must not clear the gate the second one is holding.
        setLanding((current) => (current === key ? null : current));
      });
    },
  });

  /**
   * The pressed card, while the press is a drag.
   *
   * The one card that translates, the one card whose settle transition is
   * withheld, and the one card the cursor changes on. A predicate rather than a
   * value because `draw` is called once per card and only one of them is ever it.
   */
  const lifted = (card: BoardCard) => drag.dragging && drag.item === card.key;

  const draw = (card: BoardCard) => (
    <li
      key={card.key}
      data-card-key={card.key}
      data-dragging={lifted(card) ? "true" : undefined}
      onPointerDown={(event) => {
        // Every press, ahead of the gates below: a press on a card's own menu
        // returns without reaching `begin`, which is the other site that clears
        // the swallowed-click flag, and a touch drag ends with no synthesised
        // click to eat it.
        drag.allowNextClick();
        // Secondary buttons open menus and select text; and a press that begins
        // on the column menu belongs to the menu, which owns its own pointer.
        if (
          event.button !== 0 ||
          (event.target instanceof Element && event.target.closest("select") !== null)
        ) {
          return;
        }
        // The press's own default is the selection, and this is where it is
        // stopped: a cancelled `pointerdown` sets the platform's PREVENT MOUSE
        // EVENT flag, so no `mousedown` is fired and nothing anchors a selection
        // for the following moves to extend — `resizable-columns.tsx:202-203`
        // exactly, which is why a seam drag never leaked one. The spec's own
        // worked sequence still ends in `click`, so the title button below still
        // opens the file; what the cancel does cost is focus-on-mousedown, which
        // is why the `<select>` above returns before reaching here, and why the
        // keyboard path — tab, then Enter — never goes near a pointer.
        event.preventDefault();
        // The room this card has inside the board, before it carries a transform
        // of its own: the follow is capped to it (see {@link followTransform}).
        bounds.current = followBounds(event.currentTarget, board.current);
        drag.begin(card.key, {
          pointerId: event.pointerId,
          clientX: event.clientX,
          clientY: event.clientY,
          target: event.currentTarget,
        });
      }}
      // The move, the release and the cancel are listened for once, on the board
      // (`:461`), and not again here: this card is inside it, so a card-level copy
      // would only run the same hit-test a second time on every move of a live
      // drag. The press stays here, because it is the press that names the card,
      // and so does the click swallow, because that click lands on the card.
      onClickCapture={drag.handlers.onClickCapture}
      // The whole card is the handle, so the whole card says so, and says it
      // twice: `cursor-grab` at rest and `cursor-grabbing` for as long as the
      // gesture is live, off the same `data-dragging` the opacity uses rather
      // than off `:active`, which a plain click would also light. `select-none`
      // is what `draggable` used to buy: without it a mouse drag paints a text
      // selection across the card instead of moving it. `touch-action` is left
      // alone deliberately — claiming it would stop a phone scrolling the board by
      // starting on a card, and a browser that takes a touch gesture over sends
      // `pointercancel`, which returns the card.
      className={cn(
        "group cursor-grab select-none rounded-md border border-border bg-card px-2 py-1.5 data-[dragging=true]:cursor-grabbing data-[dragging=true]:opacity-50",
        // The settle, and only the settle. Withheld while the gesture is live so
        // the card tracks the pointer exactly; withheld again while keeper is
        // writing the drop, so a landed card sits still instead of gliding back
        // to the slot it came from; restored by the render that ends a gesture
        // which returned the card, where the transform goes back to zero, so it
        // travels rather than teleports; and cut altogether under reduced motion
        // (`chat-row.tsx:459`).
        !lifted(card) &&
          landing !== card.key &&
          !reducedMotion &&
          "transition-transform duration-200 ease-out",
      )}
      // The follow, 1:1 until the board's own edge, for the whole gesture. Only
      // on the pressed card and only while it is a drag: a card at rest carries
      // no transform at all, so the other nineteen never pay for a containing
      // block none of them needed.
      style={lifted(card) ? { transform: followTransform(drag.delta, bounds.current) } : undefined}
    >
      <div className="flex items-start gap-1.5">
        <GripVertical
          aria-hidden="true"
          className="mt-0.5 size-3.5 shrink-0 text-muted-foreground"
        />
        <div className="flex min-w-0 flex-col gap-1">
          <button
            type="button"
            // The title is inside the handle, not a hole in it: the press lands
            // on it, the `li` above tracks the pointer, and only a press that
            // travelled far enough to become a drag suppresses this click. That
            // is what a `button` inside a `draggable` card could never do — the
            // old code had to mark the button draggable itself, and paid for it
            // in text selection.
            onClick={() => onOpen(card.key)}
            className="text-left text-sm hover:underline"
          >
            {card.title}
          </button>
          {(card.tags.length > 1 || !card.orderIsOwn || card.unstableIdentity) && (
            <div className="flex flex-wrap items-center gap-1">
              {/* `task` itself is what put the card here — repeating it on every
                  card would be four columns of the same word. */}
              {card.tags
                .filter((tag) => tag !== "task")
                .map((tag) => (
                  <Badge key={tag} variant="outline" className="text-meta">
                    {tag}
                  </Badge>
                ))}
              {!card.orderIsOwn && (
                <span title={BOARD_ORDER_DEFAULTED} className="text-muted-foreground">
                  <TriangleAlert aria-label={BOARD_ORDER_DEFAULTED} className="size-3" />
                </span>
              )}
              {card.unstableIdentity && (
                <span title={BOARD_UNSTABLE_ID} className="text-muted-foreground text-xs">
                  path
                </span>
              )}
            </div>
          )}
          {/* The same move, without a pointer. A native select for the reason
              every other sessions surface uses one: Radix's forbids an
              empty-valued item, and this list is short, closed and known.

              Revealed on hover and on focus rather than drawn on every card at
              all times (`session-tree.tsx:420`). `opacity-0` and not `hidden`:
              it stays in the DOM, in the tab order and in the accessibility
              tree, so tabbing to it both reveals it and reads it out. */}
          <select
            aria-label={`${BOARD_MOVE_LABEL} — ${card.title}`}
            value={card.status ?? ""}
            onChange={(event) => {
              const next = event.target.value;
              // To the end of the column it is joining, which is where a card
              // arrives when nobody said otherwise.
              void move(card.key, next, columnOf(cards, next).length);
            }}
            className="h-7 rounded-md border border-input bg-transparent px-1 text-muted-foreground text-xs opacity-0 outline-none focus-within:opacity-100 focus-visible:ring-2 focus-visible:ring-ring group-hover:opacity-100"
          >
            {card.status !== null && !known.has(card.status) && (
              <option value={card.status}>{card.status}</option>
            )}
            {card.status === null && <option value="">—</option>}
            {BOARD_COLUMNS.map((column) => (
              <option key={column.status} value={column.status}>
                {column.label}
              </option>
            ))}
          </select>
        </div>
      </div>
    </li>
  );

  return (
    <section
      ref={board}
      aria-label={heading}
      // The move, the release and the cancel are listened for here and nowhere
      // else. Before the slop crossing there is no capture, and a press 3 px from
      // the edge of a 28 px card leaves the card before travelling 6: that move
      // lands on the column box, which the card sits below rather than above, so
      // on the card alone nothing would hear it and the drag would silently never
      // start. From here the whole board hears it — including the release of a
      // card whose own `li` an external re-read unmounted mid-drag. Every handler
      // is `pointerId`-guarded and no-ops with no press in flight.
      onPointerMove={drag.handlers.onPointerMove}
      onPointerUp={drag.handlers.onPointerUp}
      onPointerCancel={drag.handlers.onPointerCancel}
      className="flex flex-col gap-1"
    >
      <h3 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
        {heading}
      </h3>
      {notice !== null && (
        <p role="status" className="text-destructive text-xs">
          {notice}
        </p>
      )}
      {cards.length === 0 ? (
        <p className="text-muted-foreground text-xs">{empty}</p>
      ) : (
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-4">
          {BOARD_COLUMNS.map((column) => {
            const inColumn = columnOf(cards, column.status);
            return (
              <div
                key={column.status}
                data-board-column={column.status}
                className={cn(
                  "flex flex-col gap-1 rounded-md border border-border p-2",
                  // Drawn on the element that accepts the release, and only while
                  // one could be accepted: the cue and the target are now the
                  // same box, so the cue cannot claim a region that is dead.
                  drag.dragging &&
                    drag.over?.status === column.status &&
                    "border-primary border-dashed",
                )}
              >
                <h4 className="flex items-baseline justify-between gap-2 font-medium text-xs">
                  <span>{column.label}</span>
                  <span className="figures text-muted-foreground">{inColumn.length}</span>
                </h4>
                {/* The whole box above is the target; this is the list a screen
                    reader reads out. `flex-1` so its own box fills the rest of
                    the column: the list's bounds and the droppable region below
                    the header are then the same rectangle, which is exactly what
                    they were not when the `ul` took the drop and the box drew the
                    highlight. `min-h-16` keeps an empty column tall enough to
                    aim at. */}
                <ul aria-label={column.label} className="flex min-h-16 flex-1 flex-col gap-1">
                  {inColumn.length === 0 ? (
                    <li className="text-muted-foreground text-xs">{BOARD_EMPTY_COLUMN}</li>
                  ) : (
                    inColumn.map((card) => draw(card))
                  )}
                </ul>
              </div>
            );
          })}
        </div>
      )}
      {strays.length > 0 && (
        <div className="mt-1 flex flex-col gap-1 rounded-md border border-border border-dashed p-2">
          <h4 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
            {BOARD_STRAY_HEADING}
          </h4>
          <p className="text-muted-foreground text-xs">{BOARD_STRAY_HINT}</p>
          <ul className="flex flex-col gap-1">{strays.map((card) => draw(card))}</ul>
        </div>
      )}
    </section>
  );
}
