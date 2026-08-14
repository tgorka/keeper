/**
 * The React panel a `> [!board]` / `> [!log]` / `> [!refs]` callout becomes
 * (FR-264).
 *
 * # A loader and a mount, and nothing else
 *
 * `file-embed-host.tsx`'s shape, for its reason: `note-widget.ts` is reached
 * from `live-preview.ts`, which `note-editor.tsx` imports lazily to keep the
 * editor's chunk free of React (NFR-27). The React boundary therefore lives in
 * this file and arrives through a dynamic `import()`, so the widget module can
 * be statically imported by the renderer without dragging React in behind it.
 *
 * # The three views are three renderers over one query
 *
 * Rust selects and orders the rows — `notes/widget.rs` owns the default query
 * per kind, what a stated query replaces, and the sort (a board by `order`, a
 * log by filename descending, references by folded title). Nothing here filters,
 * re-sorts or composes a query (AD-65), which is what makes the board in a note
 * and a session's own board agree about what a task is.
 *
 * The board is {@link TaskBoard} — the same component `session-board.tsx` binds,
 * which is the operator's requirement stated as an import rather than as a
 * promise: *"the trello like task view should be the widget inside the md file I
 * could use in the notes as well"*.
 *
 * # One read, re-run after a write
 *
 * A move writes two frontmatter keys through Rust and this panel re-reads. It
 * keeps no order of its own, so a card that Obsidian moved while the note was
 * open comes back in its new column on the next read rather than fighting a
 * local copy (AD-110).
 */
import { useCallback, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import type { BoardCard } from "@/components/notes/task-board";
import { TaskBoard } from "@/components/notes/task-board";
import type { WidgetKind, WidgetRow } from "@/lib/ipc/client";
import { notesWidget, notesWidgetMove } from "@/lib/ipc/client";
import { panelsStore } from "@/lib/stores/panels";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The heading each kind draws above itself, which is also its accessible name.
 *  A widget is a section of somebody's note and has to say what it is: the
 *  callout's marker is gone once the block renders. */
export const WIDGET_HEADINGS: Record<WidgetKind, string> = {
  board: "Tasks",
  log: "Log",
  refs: "References",
};

/** What each kind says when its query selected nothing. Per kind, because the
 *  way to make one differs and a sentence naming the wrong way is worse than a
 *  vague one (UX-DR44 — never an empty box). */
export const WIDGET_EMPTY: Record<WidgetKind, string> = {
  board:
    "No notes match this board. A card is a note tagged `task` — write one, and it appears here.",
  log: "No log entries yet. An entry is a note tagged `log`.",
  refs: "Nothing referenced yet. A reference is a note tagged `ref`.",
};

/** What a widget says while Rust is answering. Said rather than left blank: a
 *  block that renders empty and then fills is indistinguishable from a query
 *  that selected nothing. */
export const WIDGET_LOADING = "Reading…";

/** What is said when the read is refused and keeper sent no sentence of its
 *  own. The query is the likely cause and the block is right there, so the
 *  sentence points at it rather than at a support page. */
export const WIDGET_READ_FAILED =
  "keeper could not read this widget. Check the query on the callout's first line.";

/** What is said when a card cannot be moved and keeper sent no sentence. */
export const WIDGET_MOVE_FAILED =
  "keeper could not move that card. The notes may have changed since the board was drawn — click outside the block and back in to redraw it.";

/** How a row's time is drawn: the day, because a log's own filename already
 *  carries the minute and a second precision would be two clocks. */
function day(updatedMs: number): string {
  if (!Number.isFinite(updatedMs) || updatedMs <= 0) {
    return "";
  }
  return new Date(updatedMs).toISOString().slice(0, 10);
}

/**
 * A selected note in the board's own terms.
 *
 * The card's key is the **note id**, because that is what `notes_widget_move`
 * addresses a note by — a vault's notes are addressed by id where a session's
 * files are addressed by path, and the board never has to know which because it
 * never looks inside the key.
 */
function cardOf(row: WidgetRow): BoardCard {
  return {
    key: row.id,
    title: row.title,
    status: row.status,
    order: row.order,
    orderIsOwn: row.orderIsOwn,
    tags: row.tags,
    // A note in a vault always has an id — the vault index assigns one — so a
    // widget board never draws the `path` chip a session board can.
    unstableIdentity: false,
  };
}

export interface NoteWidgetProps {
  /** The vault the note is in. The editor is built per vault, so this is always
   *  a real id. */
  vaultId: string;
  kind: WidgetKind;
  /** The callout's own text, verbatim. Empty means "the kind's default query",
   *  which is Rust's decision and not this component's. */
  argument: string;
}

export function NoteWidget({ vaultId, kind, argument }: NoteWidgetProps): React.ReactElement {
  const [rows, setRows] = useState<readonly WidgetRow[] | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const read = useCallback(async () => {
    try {
      setRows(await notesWidget(vaultId, kind, argument));
      setNotice(null);
    } catch (error) {
      // keeper's own sentence when it sent one: it knows which term of the
      // query it refused and this component does not (UX-DR43).
      setNotice(syncErrorMessage(error, WIDGET_READ_FAILED));
    }
  }, [vaultId, kind, argument]);

  useEffect(() => {
    void read();
  }, [read]);

  const open = (noteId: string) => {
    panelsStore.getState().setActiveTarget({ kind: "note", vaultId, noteId });
  };

  const heading = WIDGET_HEADINGS[kind];

  if (notice !== null) {
    return (
      <section aria-label={heading} className="flex flex-col gap-1 p-2">
        <p role="status" className="text-destructive text-xs">
          {notice}
        </p>
      </section>
    );
  }

  if (rows === null) {
    return (
      <section aria-label={heading} className="flex flex-col gap-1 p-2">
        <p className="text-muted-foreground text-xs">{WIDGET_LOADING}</p>
      </section>
    );
  }

  if (kind === "board") {
    return (
      <div className="p-2">
        <TaskBoard
          heading={heading}
          cards={rows.map(cardOf)}
          empty={WIDGET_EMPTY.board}
          onOpen={open}
          onMove={async (noteId, status, index) => {
            await notesWidgetMove(vaultId, kind, argument, noteId, status, index);
            await read();
          }}
          moveFailed={WIDGET_MOVE_FAILED}
        />
      </div>
    );
  }

  // A log and a list of references are the same shape — a title, a date, the
  // first line — drawn by the same code because they differ only in what Rust
  // selected and how it sorted. Two components here would be one component
  // twice.
  return (
    <section aria-label={heading} className="flex flex-col gap-1 p-2">
      <h3 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
        {heading}
      </h3>
      {rows.length === 0 ? (
        <p className="text-muted-foreground text-xs">{WIDGET_EMPTY[kind]}</p>
      ) : (
        <ol className="flex flex-col gap-1">
          {rows.map((row) => (
            <li key={row.id} className="rounded-md border border-border px-2 py-1.5">
              <button
                type="button"
                onClick={() => {
                  open(row.id);
                }}
                className="flex w-full items-baseline gap-2 text-left"
              >
                <span className="min-w-0 flex-1 truncate text-sm hover:underline">{row.title}</span>
                <span className="figures shrink-0 text-meta text-muted-foreground">
                  {day(row.updatedMs)}
                </span>
              </button>
              {row.snippet !== "" && (
                <p className="mt-0.5 line-clamp-2 text-muted-foreground text-xs">{row.snippet}</p>
              )}
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

/**
 * Mount the panel into a plain DOM node, for a CodeMirror widget to own.
 *
 * `mountNoteFileEmbed`'s shape and its reason: the React boundary lives here so
 * that `note-widget.ts` — which `live-preview.ts` imports statically — contains
 * no React import at all.
 */
export function mountNoteWidget(
  container: HTMLElement,
  args: NoteWidgetProps,
): { unmount: () => void } {
  const root = createRoot(container);
  root.render(<NoteWidget vaultId={args.vaultId} kind={args.kind} argument={args.argument} />);
  return {
    unmount: () => {
      root.unmount();
    },
  };
}
