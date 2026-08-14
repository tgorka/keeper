/**
 * The task board (FR-263): four columns of markdown files, dragged between them.
 *
 * **The cards are files, and that is the whole design.** A card is a
 * `task`-tagged file in the session's pool; its column is that file's `status:`
 * and its position is its `order:`. There is no board state anywhere — not in
 * this component, not in a database, not in a sidecar. Dropping a card writes
 * two frontmatter keys and the surface re-reads, which is why the same board
 * shows the same thing whether it was Obsidian, an agent or this component that
 * last moved a card (AD-110).
 *
 * **The columns are Rust's, not this file's.** `SESSION_BOARD_COLUMNS` mirrors
 * the closed `TaskStatus` set, and a card whose status is none of them is shown
 * in its own row rather than silently dropped — a task nobody can see is worse
 * than a task in the wrong place, and the flat shape's one real failure mode is
 * a file no view selects.
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
import type { SessionTaskVm } from "@/lib/ipc/client";
import { sessionsTaskMove } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/** The section's own heading. */
export const SESSION_BOARD_HEADING = "Tasks";

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
export const SESSION_BOARD_COLUMNS: ReadonlyArray<{ status: string; label: string }> = [
  { status: "in-preparation", label: "In preparation" },
  { status: "todo", label: "To do" },
  { status: "done", label: "Done" },
  { status: "deferred", label: "Deferred" },
];

/** What an empty column says, so an empty board still reads as a board. */
export const SESSION_BOARD_EMPTY_COLUMN = "Nothing here";

/** What the section says when the session has no tasks at all. */
export const SESSION_BOARD_EMPTY =
  "No tasks yet. A task is a markdown file tagged `task` — make one from Files above, or write one in Obsidian.";

/** The heading of the row for cards whose status is not one of the four. */
export const SESSION_BOARD_STRAY_HEADING = "Not in a column";

/** Why a stray card is shown rather than hidden. */
export const SESSION_BOARD_STRAY_HINT =
  "These files are tagged `task` but their `status:` is not one of the four columns. Fix the key in the file, or move the card into a column here.";

/** The label of the per-card column menu. */
export const SESSION_BOARD_MOVE_LABEL = "Move to column";

/** What is said when a move is refused and keeper sent no sentence of its own. */
export const SESSION_BOARD_MOVE_FAILED =
  "keeper could not move that card. The session may have changed on disk since the board was drawn — reopen it.";

/** How a card that keeper had to default an `order:` for is described. */
export const SESSION_BOARD_ORDER_DEFAULTED =
  "This file states no order:, so keeper placed the card by title. Drag it once to give it a position of its own.";

/** How a card with no ULID is described. */
export const SESSION_BOARD_UNSTABLE_ID =
  "This file has no id:, so keeper tracks it by path — renaming it loses pins and marks. keeper does not stamp a file it did not write.";

/**
 * The cards of one column, in the order they are rendered.
 *
 * Sorted the way Rust sorts the pool — `order` ascending, ties broken by folded
 * title — so the index this component sends is the index Rust resolves against.
 * A component that rendered one order and reported another would put cards
 * wherever the difference happened to fall.
 */
function columnOf(tasks: readonly SessionTaskVm[], status: string): SessionTaskVm[] {
  return tasks
    .filter((task) => task.status === status)
    .sort(
      (a, b) =>
        a.order - b.order ||
        a.title.localeCompare(b.title, undefined, { sensitivity: "base" }) ||
        a.relPath.localeCompare(b.relPath),
    );
}

export function SessionBoard({
  rootId,
  sessionId,
  tasks,
  onOpen,
  onChanged,
}: {
  rootId: string;
  sessionId: string;
  tasks: readonly SessionTaskVm[];
  /** Open a card's file — a card is a note, and reading it is the point. */
  onOpen: (relPath: string) => void;
  /** Re-read after a write; this component keeps no order of its own. */
  onChanged: () => void;
}) {
  const [dragging, setDragging] = useState<string | null>(null);
  const [over, setOver] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const known = new Set(SESSION_BOARD_COLUMNS.map((column) => column.status));
  const strays = tasks.filter((task) => task.status === null || !known.has(task.status));

  const move = async (relPath: string, status: string, index: number) => {
    setNotice(null);
    setDragging(null);
    setOver(null);
    try {
      await sessionsTaskMove(rootId, sessionId, relPath, status, index);
      onChanged();
    } catch (error) {
      // keeper's own sentence when it sent one — it knows what it refused and
      // this component does not (UX-DR43).
      setNotice(syncErrorMessage(error, SESSION_BOARD_MOVE_FAILED));
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
  const dropIndex = (column: SessionTaskVm[], relPath: string, at: number) => {
    const from = column.findIndex((card) => card.relPath === relPath);
    return from !== -1 && from < at ? at - 1 : at;
  };

  const card = (task: SessionTaskVm, status: string, column: SessionTaskVm[], at: number) => (
    <li
      key={task.relPath}
      draggable
      onDragStart={() => setDragging(task.relPath)}
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
        dragging === task.relPath && "opacity-50",
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
            onClick={() => onOpen(task.relPath)}
            className="text-left text-sm hover:underline"
          >
            {task.title}
          </button>
          {(task.tags.length > 1 || !task.orderIsOwn || task.unstableIdentity) && (
            <div className="flex flex-wrap items-center gap-1">
              {/* `task` itself is what put the card here — repeating it on every
                  card would be four columns of the same word. */}
              {task.tags
                .filter((tag) => tag !== "task")
                .map((tag) => (
                  <Badge key={tag} variant="outline" className="text-meta">
                    {tag}
                  </Badge>
                ))}
              {!task.orderIsOwn && (
                <span title={SESSION_BOARD_ORDER_DEFAULTED} className="text-muted-foreground">
                  <TriangleAlert aria-label={SESSION_BOARD_ORDER_DEFAULTED} className="size-3" />
                </span>
              )}
              {task.unstableIdentity && (
                <span title={SESSION_BOARD_UNSTABLE_ID} className="text-muted-foreground text-xs">
                  path
                </span>
              )}
            </div>
          )}
          {/* The same move, without a pointer. A native select for the reason
              every other sessions surface uses one: Radix's forbids an
              empty-valued item, and this list is short, closed and known. */}
          <select
            aria-label={`${SESSION_BOARD_MOVE_LABEL} — ${task.title}`}
            value={task.status ?? ""}
            onChange={(event) => {
              const next = event.target.value;
              // To the end of the column it is joining, which is where a card
              // arrives when nobody said otherwise.
              void move(task.relPath, next, columnOf(tasks, next).length);
            }}
            className="h-7 rounded-md border border-input bg-transparent px-1 text-muted-foreground text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            {task.status !== null && !known.has(task.status) && (
              <option value={task.status}>{task.status}</option>
            )}
            {task.status === null && <option value="">—</option>}
            {SESSION_BOARD_COLUMNS.map((column) => (
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
    <section aria-label={SESSION_BOARD_HEADING} className="flex flex-col gap-1">
      <h3 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
        {SESSION_BOARD_HEADING}
      </h3>
      {notice !== null && (
        <p role="status" className="text-destructive text-xs">
          {notice}
        </p>
      )}
      {tasks.length === 0 ? (
        <p className="text-muted-foreground text-xs">{SESSION_BOARD_EMPTY}</p>
      ) : (
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-4">
          {SESSION_BOARD_COLUMNS.map((column) => {
            const cards = columnOf(tasks, column.status);
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
                  <span className="figures text-muted-foreground">{cards.length}</span>
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
                    void move(dragging, column.status, dropIndex(cards, dragging, cards.length));
                  }}
                  className="flex min-h-16 flex-col gap-1"
                >
                  {cards.length === 0 ? (
                    <li className="text-muted-foreground text-xs">{SESSION_BOARD_EMPTY_COLUMN}</li>
                  ) : (
                    cards.map((task, at) => card(task, column.status, cards, at))
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
            {SESSION_BOARD_STRAY_HEADING}
          </h4>
          <p className="text-muted-foreground text-xs">{SESSION_BOARD_STRAY_HINT}</p>
          <ul className="flex flex-col gap-1">
            {strays.map((task, at) => card(task, task.status ?? "", strays, at))}
          </ul>
        </div>
      )}
    </section>
  );
}
