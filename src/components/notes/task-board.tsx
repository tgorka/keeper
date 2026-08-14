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
 * reason to take a dependency that owns pointer input across the whole app. This
 * follows the only precedent, the hand-rolled HTML5 DnD in
 * {@link "@/components/layout/pins-strip"} — with its lesson applied: indices go
 * stale, so every drop is guarded and the *authoritative* answer comes from the
 * re-read afterwards, never from this component's own idea of the order.
 *
 * **Drag is not the only way to move a card.** Every card carries a column menu
 * that does the same write, because a drag needs a pointer, a working hand and a
 * screen tall enough to show two columns at once. The keyboard path is the
 * accessible one and the pointer path is the fast one; they are the same verb.
 */
import { GripVertical, TriangleAlert } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/ui/badge";
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
  const [dragging, setDragging] = useState<string | null>(null);
  const [over, setOver] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const known = new Set(BOARD_COLUMNS.map((column) => column.status));
  const strays = cards.filter((card) => card.status === null || !known.has(card.status));

  const move = async (key: string, status: string, index: number) => {
    setNotice(null);
    setDragging(null);
    setOver(null);
    try {
      await onMove(key, status, index);
    } catch (error) {
      // keeper's own sentence when it sent one — it knows what it refused and
      // this component does not (UX-DR43).
      setNotice(syncErrorMessage(error, moveFailed));
    }
  };

  /**
   * Where a dropped card lands in a column it is dragged within.
   *
   * The index Rust wants counts the column **without** the moved card, so a card
   * dragged downwards inside its own column has to lose the slot it vacated;
   * one dragged from another column does not. Getting this wrong is the classic
   * off-by-one that makes a downward drag land one place short.
   */
  const dropIndex = (column: BoardCard[], key: string, at: number) => {
    const from = column.findIndex((card) => card.key === key);
    return from !== -1 && from < at ? at - 1 : at;
  };

  const draw = (card: BoardCard, status: string, column: BoardCard[], at: number) => (
    <li
      key={card.key}
      draggable
      onDragStart={() => setDragging(card.key)}
      onDragEnd={() => {
        setDragging(null);
        setOver(null);
      }}
      onDragOver={(event) => {
        event.preventDefault();
        setOver(`${status}:${at}`);
      }}
      onDrop={(event) => {
        event.preventDefault();
        event.stopPropagation();
        if (dragging === null) {
          return;
        }
        void move(dragging, status, dropIndex(column, dragging, at));
      }}
      className={cn(
        "rounded-md border border-border bg-card px-2 py-1.5",
        dragging === card.key && "opacity-50",
        over === `${status}:${at}` && dragging !== null && "border-primary border-dashed",
      )}
    >
      <div className="flex items-start gap-1.5">
        <GripVertical
          aria-hidden="true"
          className="mt-0.5 size-3.5 shrink-0 cursor-grab text-muted-foreground"
        />
        <div className="flex min-w-0 flex-col gap-1">
          <button
            type="button"
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
              empty-valued item, and this list is short, closed and known. */}
          <select
            aria-label={`${BOARD_MOVE_LABEL} — ${card.title}`}
            value={card.status ?? ""}
            onChange={(event) => {
              const next = event.target.value;
              // To the end of the column it is joining, which is where a card
              // arrives when nobody said otherwise.
              void move(card.key, next, columnOf(cards, next).length);
            }}
            className="h-7 rounded-md border border-input bg-transparent px-1 text-muted-foreground text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
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
    <section aria-label={heading} className="flex flex-col gap-1">
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
                className={cn(
                  "flex flex-col gap-1 rounded-md border border-border p-2",
                  over === column.status && dragging !== null && "border-primary border-dashed",
                )}
              >
                <h4 className="flex items-baseline justify-between gap-2 font-medium text-xs">
                  <span>{column.label}</span>
                  <span className="figures text-muted-foreground">{inColumn.length}</span>
                </h4>
                {/* The list, not the column box, is what takes a drop: a `ul`
                    already announces itself as a list, where a bare `div` has
                    no role for a screen reader to read out or for a drop to
                    hang off. It keeps a minimum height so an empty column is
                    still a target big enough to hit — the first card of a
                    column has to be droppable somewhere. */}
                <ul
                  aria-label={column.label}
                  onDragOver={(event) => {
                    event.preventDefault();
                    setOver(column.status);
                  }}
                  onDrop={(event) => {
                    event.preventDefault();
                    if (dragging === null) {
                      return;
                    }
                    // A drop on the list itself rather than on a card means the
                    // empty space below the last one: the end.
                    void move(
                      dragging,
                      column.status,
                      dropIndex(inColumn, dragging, inColumn.length),
                    );
                  }}
                  className="flex min-h-16 flex-col gap-1"
                >
                  {inColumn.length === 0 ? (
                    <li className="text-muted-foreground text-xs">{BOARD_EMPTY_COLUMN}</li>
                  ) : (
                    inColumn.map((card, at) => draw(card, column.status, inColumn, at))
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
          <ul className="flex flex-col gap-1">
            {strays.map((card, at) => draw(card, card.status ?? "", strays, at))}
          </ul>
        </div>
      )}
    </section>
  );
}
