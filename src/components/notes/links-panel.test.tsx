import { render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteRowVm } from "@/lib/ipc/client";
import {
  colourUtilities,
  contrast,
  resolveColour,
  SURFACES,
  TEXT_FLOOR,
  THEMES,
} from "@/test/colour";
import {
  LINK_PREDICATE_SLOT,
  LINK_UNWRITTEN_SLOT,
  LinksPanel,
  UNWRITTEN_MARK,
} from "./links-panel";

const notesBacklinks = vi.fn();
const notesForwardlinks = vi.fn();

vi.mock("@/lib/ipc/client", () => ({
  notesBacklinks: (v: string, n: string) => notesBacklinks(v, n),
  notesForwardlinks: (v: string, n: string) => notesForwardlinks(v, n),
}));

/** A row for a note that exists. `unresolvedTarget` is empty here and on every
 *  ordinary row, which is the whole of the distinction: see {@link unwritten}. */
function row(overrides: Partial<NoteRowVm> = {}): NoteRowVm {
  return {
    id: "n2",
    path: "notes/belief.md",
    title: "Belief statement",
    snippet: "what changes if we are right",
    tags: [],
    updatedMs: 0,
    pinned: false,
    archived: false,
    unread: false,
    conflict: false,
    origin: "local",
    headRev: "",
    predicates: [],
    unresolvedTarget: "",
    order: { value: 0, source: "default" },
    ...overrides,
  } as NoteRowVm;
}

/**
 * A row for a link whose target no note answers to yet.
 *
 * Shaped the way the projection actually sends one: `unresolvedTarget` and
 * `predicates`, and NOTHING else — no id, no path, no title, no snippet. That
 * emptiness is deliberate on the Rust side (a title synthesised from the target
 * would be the target text living in two fields), so a fixture that helpfully
 * filled them in would let the panel read a field that is empty in production
 * and every test here would pass over the defect.
 */
function unwritten(target: string, predicates: string[] = []): NoteRowVm {
  return row({
    id: "",
    path: "",
    title: "",
    snippet: "",
    origin: "",
    unresolvedTarget: target,
    predicates,
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  notesBacklinks.mockResolvedValue([]);
  notesForwardlinks.mockResolvedValue([]);
});

/**
 * A predicate is the author's own word for why two notes are connected, written
 * on the link: `[Belief](belief.md){schema:creator, foaf:knows}`, or in the
 * older spelling `[Belief](belief.md){reference="supports"}`. "What links here"
 * is a weaker question than "what supports this", and the answer is only useful
 * if the list says which it is.
 *
 * One list, not a list beside a legacy single value: the `reference` word folds
 * in as the first entry before the row reaches this panel, so these tests only
 * ever see `predicates`.
 */
describe("a link that says what kind of link it is", () => {
  /** The row as a reader hears it. The button's text content is also its
   *  accessible name, which is where a fact about the edge has to live — not in
   *  a `title` attribute, which is announced to nobody. */
  async function linkRow(): Promise<HTMLElement> {
    return await screen.findByRole("button");
  }

  it("shows the predicate beside the note it connects to", async () => {
    notesForwardlinks.mockResolvedValue([row({ predicates: ["schema:creator"] })]);
    render(<LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={() => {}} />);

    expect(await linkRow()).toHaveTextContent("schema:creator");
  });

  /** The `{reference="supports"}` spelling predates CURIEs and vaults are full
   *  of it. Folded in as the first entry, it has to render as it always has. */
  it("shows the older reference word the same way", async () => {
    notesForwardlinks.mockResolvedValue([row({ predicates: ["supports"] })]);
    render(<LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={() => {}} />);

    expect(await linkRow()).toHaveTextContent("supports");
  });

  /** Order is the author's, never sorted and never deduplicated into a set:
   *  `{dcterms:source, schema:creator}` reads differently the other way round. */
  it("shows every predicate, in the order they were written", async () => {
    notesForwardlinks.mockResolvedValue([
      row({ predicates: ["dcterms:source", "schema:creator", "foaf:knows"] }),
    ]);
    render(<LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={() => {}} />);

    const text = (await linkRow()).textContent ?? "";
    expect(text).toContain("dcterms:source");
    expect(text).toContain("schema:creator");
    expect(text).toContain("foaf:knows");
    expect(text.indexOf("dcterms:source")).toBeLessThan(text.indexOf("schema:creator"));
    expect(text.indexOf("schema:creator")).toBeLessThan(text.indexOf("foaf:knows"));
  });

  /**
   * Nearly every link has none, so the empty case IS the panel and a chip that
   * is always there is furniture.
   *
   * Asserted as the row's exact text and its exact contents, not merely as
   * "the predicate is absent": an empty label, or a separator rendered for an
   * empty list, passes the weaker assertion and is the defect class the sync
   * pane's orphaned separator was. An inventory rather than a count, because
   * a count says "3, expected 2" and an inventory says which node arrived.
   */
  it("shows nothing extra for a link written without one", async () => {
    const only = row();
    notesBacklinks.mockResolvedValue([only]);
    render(<LinksPanel vaultId="v1" noteId="n1" direction="from" onOpen={() => {}} />);

    const button = await linkRow();
    expect(button.textContent).toBe(`${only.title}${only.snippet}`);
    expect(
      Array.from(button.querySelectorAll("span")).map((el) => [el.tagName, el.textContent]),
    ).toEqual([
      ["SPAN", only.title],
      ["SPAN", only.snippet],
    ]);
  });

  /** Both directions are one list rendered twice. A reason that showed only on
   *  the outbound half would answer "what links here" and not "why". */
  it("shows predicates for the notes linking here as well", async () => {
    notesBacklinks.mockResolvedValue([row({ predicates: ["prov:wasDerivedFrom"] })]);
    render(<LinksPanel vaultId="v1" noteId="n1" direction="from" onOpen={() => {}} />);

    expect(await linkRow()).toHaveTextContent("prov:wasDerivedFrom");
  });

  /** Every chip the panel painted, anywhere on the page. */
  function chips(scope: ParentNode = document.body): HTMLElement[] {
    return Array.from(scope.querySelectorAll<HTMLElement>(`[data-slot="${LINK_PREDICATE_SLOT}"]`));
  }

  /**
   * The owner's real syntax is kramdown IAL plus the Semantic Markdown V0
   * property rule, and its commonest token carries no prefix at all:
   * `**[JWT Auth](jwt.md)**{ :depends_on }` reaches this panel as the bare word
   * `depends_on`, `{ :type="Metric" }` as the bare word `type`, and only
   * `{schema:creator}` arrives looking like a CURIE. Three spellings upstream,
   * one spelling here.
   *
   * Asserted as one element SHAPE rather than as three strings that happen to
   * be present, because the defect this guards renders all three: a panel that
   * recognises a colon and treats everything else as something lesser — plain
   * text, a `title` attribute, a different chip — loses the format the owner
   * actually writes while every "the word is on screen" assertion stays green.
   */
  it("renders a bare name, a reduced empty-prefix name and a CURIE identically", async () => {
    notesForwardlinks.mockResolvedValue([
      row({ predicates: ["depends_on", "type", "schema:creator"] }),
    ]);
    render(<LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={() => {}} />);

    const painted = chips(await linkRow());
    expect(painted.map((chip) => chip.textContent)).toEqual([
      "depends_on",
      "type",
      "schema:creator",
    ]);
    // One tag and one class list across all three: no branch on the colon.
    expect(new Set(painted.map((chip) => chip.tagName)).size).toBe(1);
    expect(new Set(painted.map((chip) => chip.className)).size).toBe(1);
  });

  /**
   * The zero case IS the panel — nearly every link carries no predicate — so
   * the row a reader sees a thousand times must still be the row that existed
   * before this field did.
   *
   * The pair of assertions divides like this, and the division is the point.
   * The inventory above catches furniture that is always there: a separator, an
   * empty wrapper around the list, a reserved box. This one catches furniture
   * that is CONDITIONAL on the list being empty — a placeholder chip, a
   * different class on the row, a margin the row only takes when it has nothing
   * to hold — by asserting the empty row against a predicate row with its chips
   * lifted out. Neither alone is the claim; a differential test is blind to
   * anything both sides render, and an inventory is blind to a class.
   */
  it("renders a link written without a predicate as that same row minus its chips", async () => {
    notesForwardlinks.mockResolvedValue([row({ predicates: [] })]);
    const bare = render(<LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={() => {}} />);
    const absent = (await within(bare.container).findByRole("button")).outerHTML;
    bare.unmount();

    notesForwardlinks.mockResolvedValue([row({ predicates: ["cites"] })]);
    const one = render(<LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={() => {}} />);
    const chipped = await within(one.container).findByRole("button");
    const excised = chipped.cloneNode(true) as HTMLElement;
    for (const chip of chips(excised)) {
      chip.remove();
    }

    expect(excised.outerHTML).toBe(absent);
  });

  /**
   * Six predicates on a note column dragged to its narrowest.
   *
   * jsdom lays nothing out, so this measures no pixels and does not pretend to.
   * What it pins are the two declarations the measurement would come out right
   * BECAUSE of, each preventing a different failure: `w-full` takes the row's
   * width from the list rather than from its own text, so six chips cannot
   * widen it; `truncate` makes the button a scroll container, whose min-content
   * contribution is zero, so the surplus is clipped at the row's edge instead
   * of painted across the pane beside it. It also pins the order the clip eats
   * in — chips ahead of the snippet — so a narrow column gives up the body text
   * and never the reasons.
   *
   * The pixel measurement is owed to a browser and is NOT claimed here: at a
   * 180px note column, every chip inside `ul.getBoundingClientRect().right`.
   * It could not be taken in this container, which has no runnable Chromium
   * (`libnss3`, `libX11` and nineteen more are missing from the image).
   */
  it("keeps six predicates inside the row that clips them", async () => {
    const many = [
      "depends_on",
      "owned_by",
      "cites",
      "schema:creator",
      "dcterms:source",
      "prov:wasDerivedFrom",
    ];
    notesForwardlinks.mockResolvedValue([row({ predicates: many })]);
    render(<LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={() => {}} />);

    const button = await linkRow();
    expect(button.className).toContain("w-full");
    expect(button.className).toContain("truncate");

    // All six, and every one of them inside the single box that clips — a chip
    // rendered as a sibling of the row would be clipped by nothing at all.
    expect(chips(button)).toHaveLength(6);
    expect(chips()).toHaveLength(6);

    const snippet = button.lastElementChild;
    expect(snippet?.textContent).toBe("what changes if we are right");
    const order = Array.from(button.children);
    expect(order.indexOf(chips(button)[5])).toBeLessThan(order.indexOf(snippet as Element));
  });

  /**
   * A predicate is written on the SOURCE's link, and the reader is looking at
   * whichever end they opened. The two directions are therefore one row
   * rendered twice, asserted as identical markup rather than as "the word
   * appears in both": a chip the inbound half painted differently would be a
   * second reading of one fact, which is the defect the single `predicates`
   * list was introduced to end.
   */
  it("renders the same row identically whichever end the reader opened", async () => {
    const linked = row({ predicates: ["depends_on", "schema:creator"] });

    notesForwardlinks.mockResolvedValue([linked]);
    const out = render(<LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={() => {}} />);
    const outbound = (await within(out.container).findByRole("button")).outerHTML;
    out.unmount();

    notesBacklinks.mockResolvedValue([linked]);
    const back = render(<LinksPanel vaultId="v1" noteId="n1" direction="from" onOpen={() => {}} />);
    const inbound = (await within(back.container).findByRole("button")).outerHTML;

    expect(inbound).toBe(outbound);
  });
});

/**
 * The owner's report: a note linking to nine targets showed ONE row.
 *
 * The cause was measured and is not in this component — `IndexSnapshot::
 * forwardlinks` collapsed targets into a `BTreeSet` of note ids and skipped
 * every target `resolve_link` did not answer, so eight of the owner's nine
 * occurrences never reached the panel at all. Exactly one of the targets
 * existed on disk, and exactly one row was drawn.
 *
 * These tests are therefore NOT a re-diagnosis. They pin the two things the
 * panel now owes once the projection sends the edges: that it draws every row
 * it is handed, and that a row for a target nobody has written yet is honest
 * about that rather than absent. OKF v0.2 §6.1 — "Consumers MUST tolerate
 * broken links: a link whose target does not exist in the bundle is not
 * malformed; it may simply represent not-yet-written knowledge."
 */
describe("a link to a note that does not exist yet", () => {
  /** Every row the panel drew, written or not. `listitem` and not `button`:
   *  counting buttons is exactly the blindness under test, because an unwritten
   *  row is deliberately not one. */
  function drawn(): HTMLElement[] {
    return screen.getAllByRole("listitem");
  }

  function marks(scope: ParentNode = document.body): HTMLElement[] {
    return Array.from(scope.querySelectorAll<HTMLElement>(`[data-slot="${LINK_UNWRITTEN_SLOT}"]`));
  }

  /**
   * The owner's own note, as the projection sends it after the fix: nine link
   * occurrences, eight distinct edges (`auth-service.md` is written twice and
   * folds through `link_key` into one), one of which resolves.
   *
   * Asserted as an inventory of the labels rather than as a count, because a
   * count of eight is satisfied by eight copies of the one row that used to
   * survive, and the defect was specifically about WHICH edges went missing.
   */
  it("lists every outbound edge, not only the targets that already exist", async () => {
    notesForwardlinks.mockResolvedValue([
      row({ id: "n-log", title: "Company wiki — update log", predicates: ["cites"] }),
      unwritten("2025-method.md"),
      unwritten("auth-service.md", ["depends_on"]),
      unwritten("deck.md"),
      unwritten("jan-kowalski.md"),
      unwritten("nightly-run.md"),
      unwritten("platform-spec.md"),
      unwritten("revenue.md"),
    ]);
    render(<LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={() => {}} />);

    await screen.findByText("Company wiki — update log");
    expect(drawn()).toHaveLength(8);
    for (const target of [
      "2025-method.md",
      "auth-service.md",
      "deck.md",
      "jan-kowalski.md",
      "nightly-run.md",
      "platform-spec.md",
      "revenue.md",
    ]) {
      expect(screen.getByText(target)).toBeTruthy();
    }
    // Seven marked, and the one real note not marked: a panel that marked
    // everything would pass every "the words are on screen" assertion above
    // while calling the owner's existing note unwritten.
    expect(marks()).toHaveLength(7);
  });

  /**
   * Nine rows in, nine rows out, twice over — with two of them naming the SAME
   * note, and with the list CHANGING between the two reads.
   *
   * The panel keyed its rows `key={row.id}`, which two rows for one note share
   * and which is the empty string on every unwritten row, so a note pointing at
   * six unwritten targets handed React six siblings under one key.
   *
   * What that actually does was measured rather than assumed, and it is not the
   * dropped row it looks like: React renders all six on a first mount, and
   * renders them again unchanged if the next read is identical. It goes wrong
   * when the list CHANGES, because that is when reconciliation has to consult
   * the ambiguous key map — and then it INVENTS rows. Writing one of the missing
   * notes produced four rows in and six out: the freshly resolved note appeared
   * as a proper row AND as a stale unwritten row for the same note, with an
   * unrelated row repeated to make up the length, and keyboard focus moved to a
   * row the reader had not chosen.
   *
   * So the change between the reads is the test, not decoration on it. An
   * assertion taken after `render`, or after a refresh returning the identical
   * array, passes with the defect fully present — which is how the first draft
   * of this test passed against the unfixed panel.
   *
   * `revenue.md` becoming a real note is the ordinary way this list changes: a
   * vault is written forwards, so today's unwritten target is tomorrow's row,
   * and every refresh a writer triggers is this shape.
   *
   * Whether the projection ever SENDS two rows for one note is not this test's
   * claim; CoreRust dedupes by `link_key` and currently would not. The claim is
   * narrower and outlives that decision: the panel renders exactly the rows it
   * is handed, and neither drops nor invents one.
   */
  it("renders exactly the rows it is handed when the list changes under a refresh", async () => {
    const linked = [
      row({ id: "n-auth", title: "Auth service", predicates: ["depends_on"] }),
      row({ id: "n-auth", title: "Auth service", predicates: ["owned_by"] }),
      row({ id: "n-log", title: "Company wiki — update log", predicates: ["cites"] }),
      unwritten("2025-method.md"),
      unwritten("deck.md"),
      unwritten("jan-kowalski.md"),
      unwritten("nightly-run.md"),
      unwritten("platform-spec.md"),
      unwritten("revenue.md"),
    ];
    notesForwardlinks.mockResolvedValue(linked);

    const panel = render(
      <LinksPanel vaultId="v1" noteId="n1" direction="to" refreshKey={0} onOpen={() => {}} />,
    );
    await screen.findByText("Company wiki — update log");
    expect(drawn()).toHaveLength(9);
    // Both spellings of the duplicated edge, each with its own reason. Asserted
    // on the predicates because the two rows' titles are identical: a count of
    // nine is also reached with the second row lost and another one repeated.
    expect(marks()).toHaveLength(6);
    expect(document.body.textContent).toContain("depends_on");
    expect(document.body.textContent).toContain("owned_by");

    // The writer writes `revenue.md`. It resolves now, so the projection sends
    // it as an ordinary row and stops sending it as an unwritten one.
    notesForwardlinks.mockResolvedValue([
      ...linked.slice(0, 3),
      row({ id: "n-revenue", title: "Revenue model" }),
      ...linked.slice(3, 8),
    ]);
    panel.rerender(
      <LinksPanel vaultId="v1" noteId="n1" direction="to" refreshKey={1} onOpen={() => {}} />,
    );
    await screen.findByText("Revenue model");

    // Still nine, and — the assertion that catches the invented rows — the
    // written note is no longer ALSO listed as unwritten.
    expect(drawn()).toHaveLength(9);
    expect(marks()).toHaveLength(5);
    expect(screen.queryByText("revenue.md")).toBeNull();
    expect(drawn().map((li) => li.textContent)).toEqual([
      "Auth servicedepends_onwhat changes if we are right",
      "Auth serviceowned_bywhat changes if we are right",
      "Company wiki — update logciteswhat changes if we are right",
      "Revenue modelwhat changes if we are right",
      "2025-method.mdnot written yet",
      "deck.mdnot written yet",
      "jan-kowalski.mdnot written yet",
      "nightly-run.mdnot written yet",
      "platform-spec.mdnot written yet",
    ]);
  });

  /**
   * The mark itself, asserted as an element rather than as words on the page:
   * a note genuinely titled "not written yet" would satisfy a text scan while
   * the row said nothing at all.
   */
  it("says plainly that the target has not been written", async () => {
    notesForwardlinks.mockResolvedValue([unwritten("auth-service.md")]);
    render(<LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={() => {}} />);

    const [only] = await screen.findAllByRole("listitem");
    expect(only.textContent).toBe("auth-service.mdnot written yet");
    expect(marks(only).map((mark) => mark.textContent)).toEqual(["not written yet"]);
  });

  /**
   * Not a control. There is no note to open, and a row that looks live and does
   * nothing is a worse lie than the missing row was — Main ruled out inventing
   * a create-on-click, so there is nothing for a button to do.
   *
   * Asserted on `onOpen` as well as on the absent button, because the real
   * damage is not the affordance: `onOpen(row.id)` on an unwritten row would be
   * called with the empty string, and the surface would try to open a note with
   * no id.
   */
  it("never offers an unwritten target as something to open", async () => {
    const onOpen = vi.fn();
    notesForwardlinks.mockResolvedValue([unwritten("revenue.md"), row({ title: "Real note" })]);
    render(<LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={onOpen} />);

    await screen.findByText("revenue.md");
    // One button on the page, and it belongs to the note that exists.
    const buttons = screen.getAllByRole("button");
    expect(buttons).toHaveLength(1);
    expect(buttons[0].textContent).toContain("Real note");

    buttons[0].click();
    expect(onOpen).toHaveBeenCalledWith("n2");
    expect(onOpen).not.toHaveBeenCalledWith("");
  });

  /**
   * A predicate is a fact about the EDGE, and whether the far end exists yet has
   * nothing to do with it — so the chips on an unwritten row must be the same
   * chips, not a quieter second reading of one fact.
   *
   * Asserted as identical markup lifted from the two kinds of row, which is the
   * same shape as the two-directions test above and catches the same defect
   * class: a chip painted differently here would be a third spelling of
   * `predicates` on one surface.
   *
   * It also pins the mark AHEAD of the chips, which is the opposite of where
   * the snippet sits on a written row and is deliberate. `truncate` eats the
   * row from the right, so whatever trails is what a narrow note column gives
   * up first; the written row can afford to lose its snippet, but "not written
   * yet" is the single fact this row exists to carry and a row that lost it
   * would read as an ordinary link to a note that is perfectly fine.
   */
  it("gives an unwritten row its predicates on exactly the same footing", async () => {
    const both = ["depends_on", "schema:creator"];

    notesForwardlinks.mockResolvedValue([row({ predicates: both })]);
    const written = render(
      <LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={() => {}} />,
    );
    // Awaited, not read straight off the container: the panel fetches in an
    // effect, so an unawaited read collects zero chips from both halves and the
    // comparison passes by matching nothing against nothing.
    await within(written.container).findByText("Belief statement");
    const fromWritten = Array.from(
      written.container.querySelectorAll<HTMLElement>(`[data-slot="${LINK_PREDICATE_SLOT}"]`),
    ).map((chip) => chip.outerHTML);
    written.unmount();

    notesForwardlinks.mockResolvedValue([unwritten("auth-service.md", both)]);
    const missing = render(
      <LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={() => {}} />,
    );
    await within(missing.container).findByText("auth-service.md");
    const fromMissing = Array.from(
      missing.container.querySelectorAll<HTMLElement>(`[data-slot="${LINK_PREDICATE_SLOT}"]`),
    ).map((chip) => chip.outerHTML);

    expect(fromMissing).toEqual(fromWritten);

    const order = Array.from(
      (await within(missing.container).findAllByRole("listitem"))[0].firstElementChild?.children ??
        [],
    );
    const mark = missing.container.querySelector(`[data-slot="${LINK_UNWRITTEN_SLOT}"]`);
    const firstChip = missing.container.querySelector(`[data-slot="${LINK_PREDICATE_SLOT}"]`);
    expect(order.indexOf(mark as Element)).toBeLessThan(order.indexOf(firstChip as Element));
  });

  /**
   * The zero-predicate invariant, carried onto the new row.
   *
   * Nearly every link carries no predicate, so this is what an unwritten row
   * looks like almost every time it is drawn. Asserted as an inventory of the
   * row's elements rather than as "no chips are present", because an empty chip
   * or a container rendered for an empty list passes the weaker assertion — the
   * orphaned-separator defect class the sync pane grew, and the one the written
   * row is already pinned against.
   */
  it("renders an unwritten row without predicates as the target and the mark alone", async () => {
    notesForwardlinks.mockResolvedValue([unwritten("deck.md")]);
    render(<LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={() => {}} />);

    const [only] = await screen.findAllByRole("listitem");
    expect(
      Array.from(only.querySelectorAll("*")).map((el) => [el.tagName, el.textContent]),
    ).toEqual([
      ["DIV", "deck.mdnot written yet"],
      ["SPAN", "deck.md"],
      ["SPAN", "not written yet"],
    ]);
  });

  /**
   * Rows that exist have to be reachable.
   *
   * This list could not run long while a note could only list targets that
   * already existed; carrying unwritten ones makes it as long as the note's
   * link count, and it sits in a `shrink-0` corner of the editor's flex column.
   * Unbounded, a note with forty links pushes the editor off the pane; bounded
   * without a scroller, the rows are drawn and cannot be read — which is the
   * failure the owner's screenshot was first suspected of.
   *
   * jsdom lays nothing out, so this pins the two declarations rather than
   * claiming a measurement: a height ceiling, and overflow that scrolls instead
   * of hiding.
   */
  it("keeps a long list reachable instead of hiding it or pushing the editor away", async () => {
    notesForwardlinks.mockResolvedValue(
      Array.from({ length: 40 }, (_, n) => unwritten(`target-${n}.md`)),
    );
    render(<LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={() => {}} />);

    await screen.findByText("target-39.md");
    expect(drawn()).toHaveLength(40);

    const list = screen.getByRole("list");
    expect(list.className).toContain("max-h-48");
    expect(list.className).toContain("overflow-y-auto");
  });

  /**
   * The mark's ink, recomputed rather than reviewed.
   *
   * "not written yet" is the whole fact this row carries, so it is bound to the
   * 4.5:1 body-text floor and not to the 3:1 that `--faint` is held to — and
   * `index.css` says as much beside `--faint` itself: 3:1, allowed only on
   * section labels and `aria-hidden` glyphs, "may never carry a fact".
   *
   * `scripts/check-design.mjs` cannot catch this one, which is why it is here.
   * The gate measures each TOKEN against its own floor, and `--faint` clears
   * its 3:1 comfortably; swapping this class to `text-faint` would leave the
   * gate green and the sentence at 3.57:1 on light. The arithmetic has to be
   * recomputed where the CHOICE is made, on the tokens as they are actually
   * written in `index.css` today.
   */
  it("says it in ink a reader can read, on every surface and in both themes", () => {
    const ink = colourUtilities(UNWRITTEN_MARK).filter((u) => u.kind === "text");
    // The mark spends exactly one colour, at rest. A second one, or one behind
    // a `dark:`/`hover:` prefix, would be a state this loop never measured.
    expect(ink.map((u) => u.state)).toEqual([""]);

    for (const [themeName, theme] of Object.entries(THEMES)) {
      const colour = resolveColour(ink[0].value, theme);
      expect(colour, `${ink[0].value} is not a token`).not.toBeNull();
      for (const surface of SURFACES) {
        expect(
          contrast(colour as string, theme[surface]),
          `"not written yet" ink ${ink[0].value} on --${surface} (${themeName})`,
        ).toBeGreaterThanOrEqual(TEXT_FLOOR);
      }
    }
  });
});
