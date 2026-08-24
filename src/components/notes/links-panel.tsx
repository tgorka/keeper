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

/**
 * One quiet label on a link row.
 *
 * Named rather than repeated because the row now paints as many of these as the
 * link was written with, and a chip style that drifted between the first and the
 * rest would read as two kinds of fact when there is only one.
 */
const LINK_LABEL = "ml-2 rounded bg-muted px-1 py-0.5 text-meta text-muted-foreground";

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
            {/* Why these two notes are connected, in the author's own
                vocabulary: `[Belief](belief.md){schema:creator, foaf:knows}`
                makes this row say both. Before the snippet and in chips,
                because it is the thing this list is FOR — "what links here" is
                a weaker question than "what supports this", and the answer
                should not be buried in a line of body text.

                One list, not a list beside a single legacy value. The older
                `{reference="supports"}` spelling folds into this list as its
                first entry before the row is built, so a vault written last
                month renders exactly as it did; two spellings of one fact
                arriving at one surface is the defect this replaced.

                Verbatim and in the written order: keeper neither invents a
                predicate, translates one, nor sorts them. A wrong predicate in
                a graph somebody queries is worse than an absent one, and
                `{dcterms:source, schema:creator}` is a different reading from
                the same two the other way round.

                Real text inside the row's button, so a predicate lands in the
                row's accessible name. Never a `title` attribute alone: `title`
                is not reliably announced and cannot be reached from a keyboard,
                the rule `lamp.tsx` states — and a predicate is a fact about the
                edge rather than decoration on it.

                Mapped with no wrapping element on purpose. Nearly every link
                carries none, and a container rendered for an empty list would
                leave every ordinary row carrying a margin it does not carry
                today — the orphaned-separator defect class the sync pane grew.
                Zero predicates emit zero nodes. */}
            {row.predicates.map((predicate) => (
              // Keyed by the predicate itself: exact duplicates are dropped
              // where the attribute block is parsed, so the list cannot repeat.
              <span key={predicate} className={LINK_LABEL}>
                {predicate}
              </span>
            ))}
            <span className="ml-2 text-muted-foreground">{row.snippet}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}
