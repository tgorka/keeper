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
 *
 * The same argument, one level down, is why a link whose target no note answers
 * to is a ROW here and not an absence. The owner's example note pointed at nine
 * targets, eight of them not yet written, and the tab showed one line — read,
 * correctly, as the feature being broken. OKF v0.2 §6.1 settles it: "Consumers
 * MUST tolerate broken links: a link whose target does not exist in the bundle
 * is not malformed; it may simply represent not-yet-written knowledge." A vault
 * is written forwards, so those are the edges a writer most wants to see.
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

/**
 * The row's own box, shared by both kinds of row.
 *
 * Named because the not-yet-written row is NOT a button and so cannot inherit
 * these by sitting in the same element: `w-full` takes the width from the list
 * rather than from the text, and `truncate`'s `overflow: hidden` makes the row a
 * scroll container whose min-content contribution is zero, so a long target and
 * six predicates clip at the row's edge instead of spilling across the pane
 * beside it. Two rows that clipped differently would be one list with two
 * layouts, and the narrow-column defect would come back on whichever half was
 * forgotten.
 */
const LINK_ROW = "w-full truncate text-left text-xs";

/**
 * Says a link points at a note nobody has written yet.
 *
 * `text-muted-foreground` and never `text-faint`. `--faint`'s own comment in
 * `index.css` says it is held to 3:1 rather than 4.5:1 and may therefore carry
 * no fact — and whether the note on the other end of this link exists is
 * precisely a fact. Measured, faint is 3.57:1 light and 3.96:1 dark against the
 * background, and muted-foreground is 5.39:1 and 7.31:1.
 *
 * Exported so the test recomputes that arithmetic rather than trusting this
 * paragraph. `scripts/check-design.mjs` cannot: it checks each TOKEN against
 * its own floor, and `--faint` passes its 3:1 comfortably, so swapping this
 * class to `text-faint` is invisible to the gate while dropping the sentence
 * below AA.
 *
 * Plain text rather than the chip style, because a chip here would be read as
 * one more predicate — the author's word for the relationship — and this is
 * keeper's word about the target instead.
 */
export const UNWRITTEN_MARK = "ml-2 text-muted-foreground";

/** What that mark says. Lower case and unadorned: this is the ordinary state of
 *  a vault written forwards, not a warning and not an error. */
const UNWRITTEN = "not written yet";

/**
 * Names one predicate chip in the DOM.
 *
 * The zero-predicate row is asserted as this row MINUS its chips, so a test has
 * to be able to lift a chip out without knowing what a chip looks like: a query
 * written against `bg-muted` would go on passing the day the chip is restyled,
 * and the assertion it carries — that a link written without a predicate emits
 * no furniture at all — is the one that must not quietly stop being checked.
 */
export const LINK_PREDICATE_SLOT = "link-predicate";

/**
 * Names the not-yet-written mark in the DOM, on the same grounds as
 * {@link LINK_PREDICATE_SLOT}: the claim under test is that the row is HONEST
 * about a missing target, and a test that looked for the words by scanning text
 * content could not tell the mark apart from a note actually titled "not written
 * yet".
 */
export const LINK_UNWRITTEN_SLOT = "link-unwritten";

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

  // Bounded and scrollable, which it did not need to be while a note could only
  // ever list the targets that already existed. Carrying unwritten targets is
  // what makes this list as long as the note's link count, and this region sits
  // in a `shrink-0` corner of a flex column: unbounded, a note with forty links
  // would push the editor it belongs to off the pane.

  return (
    <ul className="flex max-h-48 flex-col gap-0.5 overflow-y-auto px-3 py-2">
      {rows.map((row, index) => (
        // Keyed by position, which is this row's only stable identity here.
        //
        // `row.id` was the key and cannot be: it is the empty string on every
        // not-yet-written row, so a note pointing at seven unwritten targets
        // gives seven siblings one key. React does not drop them — measured, it
        // renders all seven — but it reconciles the NEXT read against that
        // ambiguous map, and then it invents rows: write one of the missing
        // notes and the refreshed list keeps the old unwritten row for the note
        // that now exists, beside the resolved row for the same note, and
        // repeats another row to make up the length. Four rows in, six out, and
        // keyboard focus lands on a different row than it was on.
        //
        // Position is sound because the list is replaced whole on every read
        // and is never reordered or spliced in place.
        // biome-ignore lint/suspicious/noArrayIndexKey: the row's only stable identity; `row.id` is empty on every unwritten row
        <li key={index}>
          {row.unresolvedTarget ? (
            // A link to a note nobody has written yet. Shown, because OKF v0.2
            // §6.1 requires consumers to tolerate broken links — "not
            // malformed; it may simply represent not-yet-written knowledge" —
            // and because a vault is written forwards: you link the note you
            // are about to write, so the dropped edges were exactly the ones
            // the writer most wanted to see. A silently absent row is the one
            // outcome that teaches the owner the feature is broken.
            //
            // Not a button and carrying no click. There is nothing to open, and
            // a control that looks live and does nothing is a worse lie than
            // the missing row was; creating the note on click is a different
            // question that nobody has asked.
            <div className={LINK_ROW}>
              {/* The target exactly as the author typed it. It is all there is
                  to show — there is no note to have a title — and the words
                  they wrote are what lets them find the link in the body. */}
              <span>{row.unresolvedTarget}</span>
              {/* Ahead of the predicates, unlike the snippet which trails them.
                  `truncate` eats this row from the right, and this mark is the
                  single fact the row exists to carry: clipped away first, the
                  row would read as an ordinary link to a note that is fine. */}
              <span data-slot={LINK_UNWRITTEN_SLOT} className={UNWRITTEN_MARK}>
                {UNWRITTEN}
              </span>
              <Predicates predicates={row.predicates} />
            </div>
          ) : (
            <button
              type="button"
              className={`${LINK_ROW} hover:underline`}
              onClick={() => onOpen(row.id)}
            >
              <span>{row.title}</span>
              <Predicates predicates={row.predicates} />
              <span className="ml-2 text-muted-foreground">{row.snippet}</span>
            </button>
          )}
        </li>
      ))}
    </ul>
  );
}

/**
 * Why these two notes are connected, in the author's own vocabulary:
 * `**[JWT Auth](jwt.md)**{ :depends_on }` makes the row say both.
 *
 * One component rather than the same map written into each branch of the row,
 * because a not-yet-written target carries its predicates on exactly the same
 * footing as a written one — the author's reason for the link is a fact about
 * the EDGE, and whether the far end exists yet has nothing to do with it. Two
 * copies of this map would be two chances for the unwritten half to drift into
 * a second reading of one fact, which is the defect the single `predicates`
 * list was introduced to end.
 *
 * Before the snippet, because it is the thing this list is FOR — "what links
 * here" is a weaker question than "what supports this", and the answer should
 * not be buried in a line of body text. Ahead of the snippet also decides what
 * a narrow column loses first: the reasons are the last thing the clip takes
 * rather than the first.
 *
 * One list, not a list beside a single legacy value. The older
 * `{reference="supports"}` spelling folds into this list before the row is
 * built, so a vault written last month renders exactly as it did; two spellings
 * of one fact arriving at one surface is the defect this replaced.
 *
 * Printed as given, and never read: a bare `depends_on`, an empty-prefix
 * `:type` already reduced to `type` upstream, and a CURIE `schema:creator` are
 * one kind of fact and take one code path. A branch on the colon — a chip for a
 * CURIE, something else for the rest — would drop the owner's commonest
 * spelling on the floor, and it is this panel's job to say what the author
 * wrote rather than to hold an opinion about which vocabulary they wrote it in.
 *
 * Verbatim and in the written order: keeper neither invents a predicate,
 * translates one, nor sorts them. A wrong predicate in a graph somebody queries
 * is worse than an absent one, and `{dcterms:source, schema:creator}` is a
 * different reading from the same two the other way round.
 *
 * Real text inside the row, so a predicate lands in the row's accessible name.
 * Never a `title` attribute alone: `title` is not reliably announced and cannot
 * be reached from a keyboard, the rule `lamp.tsx` states — and a predicate is a
 * fact about the edge rather than decoration on it.
 *
 * A fragment with no wrapping element on purpose. Nearly every link carries no
 * predicate, and a container rendered for an empty list would leave every
 * ordinary row carrying a margin it does not carry today — the orphaned-
 * separator defect class the sync pane grew. Zero predicates emit zero nodes.
 */
function Predicates({ predicates }: { predicates: string[] }) {
  return (
    <>
      {predicates.map((predicate) => (
        // Keyed by the predicate itself: exact duplicates are dropped where the
        // attribute block is parsed, so the list cannot repeat.
        <span key={predicate} data-slot={LINK_PREDICATE_SLOT} className={LINK_LABEL}>
          {predicate}
        </span>
      ))}
    </>
  );
}
