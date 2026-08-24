import { render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteRowVm } from "@/lib/ipc/client";
import { LINK_PREDICATE_SLOT, LinksPanel } from "./links-panel";

const notesBacklinks = vi.fn();
const notesForwardlinks = vi.fn();

vi.mock("@/lib/ipc/client", () => ({
  notesBacklinks: (v: string, n: string) => notesBacklinks(v, n),
  notesForwardlinks: (v: string, n: string) => notesForwardlinks(v, n),
}));

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
    order: { value: 0, source: "default" },
    ...overrides,
  } as NoteRowVm;
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
