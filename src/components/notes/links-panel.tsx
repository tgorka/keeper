/**
 * The two directions of a note's links, as lists (Story 37.7, FR-108).
 *
 * Derived, never stored: both are projections of the core link graph, which
 * resolves through each note's ULID rather than its filename — which is why
 * renaming a note keeps every link working (FR-97).
 *
 * This used to be "Linked from" alone, sitting at the foot of the editor and
 * hiding itself at zero. It is a tab now, and a tab that vanishes is worse than
 * an empty one: the tab strip is what tells a reader the question can be asked
 * at all, and a strip whose contents change shape per note is a strip nobody can
 * learn. So the list renders its own empty sentence and the tab stays.
 */
import { useEffect, useState } from "react";
import { type NoteRowVm, notesBacklinks, notesForwardlinks } from "@/lib/ipc/client";

/** Which way round. `from` is inbound, `to` is outbound. */
export type LinkDirection = "from" | "to";

export interface LinksPanelProps {
  vaultId: string;
  noteId: string;
  direction: LinkDirection;
  /**
   * Bumped by the surface when a change event touches this vault, so a link
   * written by another process shows up without a reload.
   */
  refreshKey?: number;
  /** Open one of the linked notes. */
  onOpen: (noteId: string) => void;
}

/** The sentence an empty list says, per direction. Different sentences because
 *  the two absences mean different things: nothing points here yet, versus this
 *  note points nowhere. */
const NOTHING: Record<LinkDirection, string> = {
  from: "No other note links here yet.",
  to: "This note links to nothing yet.",
};

export function LinksPanel({ vaultId, noteId, direction, refreshKey, onOpen }: LinksPanelProps) {
  const [rows, setRows] = useState<NoteRowVm[]>([]);

  // `refreshKey` is a re-run trigger, not a read: the surface bumps it when a change
  // event touches this vault, and without it a link an agent wrote never appears
  // until the note is reopened.
  // biome-ignore lint/correctness/useExhaustiveDependencies: re-run trigger, not a read
  useEffect(() => {
    let live = true;
    const read = direction === "from" ? notesBacklinks : notesForwardlinks;
    void read(vaultId, noteId)
      .then((found) => {
        if (live) {
          setRows(found);
        }
      })
      .catch(() => {
        // A failed projection is an empty list, never an error card: the note
        // itself is fine and this is the least important thing on screen.
        if (live) {
          setRows([]);
        }
      });
    return () => {
      live = false;
    };
  }, [vaultId, noteId, direction, refreshKey]);

  if (rows.length === 0) {
    return <p className="px-3 py-2 text-muted-foreground text-xs">{NOTHING[direction]}</p>;
  }

  return (
    <ul className="flex flex-col gap-0.5 px-3 py-2">
      {rows.map((row) => (
        <li key={row.id}>
          <button
            type="button"
            className="w-full truncate text-left text-xs hover:underline"
            onClick={() => onOpen(row.id)}
          >
            <span>{row.title}</span>
            {/* The author's own word for the relationship, when they wrote one:
                `[Belief](belief.md){reference="supports"}` makes this row say
                `supports`. Before the title's snippet and in a chip, because it
                is the thing this list is FOR — "what links here" is a weaker
                question than "what supports this", and the answer should not be
                buried in a line of body text. */}
            {row.predicate === null ? null : (
              <span className="ml-2 rounded bg-muted px-1 py-0.5 text-meta text-muted-foreground">
                {row.predicate}
              </span>
            )}
            <span className="ml-2 text-muted-foreground">{row.snippet}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}
