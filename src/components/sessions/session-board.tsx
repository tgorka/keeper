/**
 * The session's task board (FR-263): {@link TaskBoard} pointed at one session.
 *
 * **What is left here is what is a session's.** The board — four columns, the
 * drag, the off-by-one, the stray row, the column menu — turned out to be about
 * markdown files rather than about sessions, and the operator asked for the same
 * board *"inside the md file I could use in the notes as well"* (FR-264). So it
 * moved to {@link "@/components/notes/task-board"} and this file kept the three
 * things a note-side board does not have: a root id, a session id, and the
 * sessions plan executor that writes the move.
 *
 * That direction is forced rather than chosen: `sessions/` already imports from
 * `notes/` in half a dozen places and `notes/` imports nothing from `sessions/`.
 * Extracting the board the other way would have created the first edge back.
 *
 * The strings stay exported from here. A session's empty state and a session's
 * refusal both name a session, and a component shared with ordinary notes cannot
 * say "the session may have changed on disk" about a note that has no session.
 */
import type { BoardCard } from "@/components/notes/task-board";
import {
  BOARD_COLUMNS,
  BOARD_EMPTY_COLUMN,
  BOARD_MOVE_LABEL,
  BOARD_ORDER_DEFAULTED,
  BOARD_STRAY_HEADING,
  BOARD_STRAY_HINT,
  BOARD_UNSTABLE_ID,
  TaskBoard,
} from "@/components/notes/task-board";
import type { SessionTaskVm } from "@/lib/ipc/client";
import { sessionsTaskMove } from "@/lib/ipc/client";

/** The section's own heading. */
export const SESSION_BOARD_HEADING = "Tasks";

/** The four columns, in the order work moves through them — the shared set. */
export const SESSION_BOARD_COLUMNS = BOARD_COLUMNS;

/** What an empty column says, so an empty board still reads as a board. */
export const SESSION_BOARD_EMPTY_COLUMN = BOARD_EMPTY_COLUMN;

/** What the section says when the session has no tasks at all. */
export const SESSION_BOARD_EMPTY =
  "No tasks yet. A task is a markdown file tagged `task` — make one from Files above, or write one in Obsidian.";

/** The heading of the row for cards whose status is not one of the four. */
export const SESSION_BOARD_STRAY_HEADING = BOARD_STRAY_HEADING;

/** Why a stray card is shown rather than hidden. */
export const SESSION_BOARD_STRAY_HINT = BOARD_STRAY_HINT;

/** The label of the per-card column menu. */
export const SESSION_BOARD_MOVE_LABEL = BOARD_MOVE_LABEL;

/** What is said when a move is refused and keeper sent no sentence of its own. */
export const SESSION_BOARD_MOVE_FAILED =
  "keeper could not move that card. The session may have changed on disk since the board was drawn — reopen it.";

/** How a card that keeper had to default an `order:` for is described. */
export const SESSION_BOARD_ORDER_DEFAULTED = BOARD_ORDER_DEFAULTED;

/** How a card with no ULID is described. */
export const SESSION_BOARD_UNSTABLE_ID = BOARD_UNSTABLE_ID;

/**
 * A session task in the board's own terms.
 *
 * The card's key is the **session-relative path**, because that is what
 * `sessions_task_move` addresses a task by — a session's files are addressed by
 * path where a vault's notes are addressed by id, and the board never has to
 * know which because it never looks inside the key.
 */
function cardOf(task: SessionTaskVm): BoardCard {
  return {
    key: task.relPath,
    title: task.title,
    status: task.status,
    order: task.order,
    orderIsOwn: task.orderIsOwn,
    tags: task.tags,
    unstableIdentity: task.unstableIdentity,
  };
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
  return (
    <TaskBoard
      heading={SESSION_BOARD_HEADING}
      cards={tasks.map(cardOf)}
      empty={SESSION_BOARD_EMPTY}
      onOpen={onOpen}
      onMove={async (relPath, status, index) => {
        // Rejecting is how the board learns a move was refused: it shows
        // keeper's own sentence and re-reads nothing.
        await sessionsTaskMove(rootId, sessionId, relPath, status, index);
        onChanged();
      }}
      moveFailed={SESSION_BOARD_MOVE_FAILED}
    />
  );
}
