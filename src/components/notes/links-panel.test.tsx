import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteRowVm } from "@/lib/ipc/client";
import { LinksPanel } from "./links-panel";

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
   * Asserted as the row's exact text and its exact span count, not merely as
   * "the predicate is absent": an empty label, or a separator rendered for an
   * empty list, passes the weaker assertion and is the defect class the sync
   * pane's orphaned separator was.
   */
  it("shows nothing extra for a link written without one", async () => {
    const only = row();
    notesBacklinks.mockResolvedValue([only]);
    render(<LinksPanel vaultId="v1" noteId="n1" direction="from" onOpen={() => {}} />);

    const button = await linkRow();
    expect(button.textContent).toBe(`${only.title}${only.snippet}`);
    expect(button.querySelectorAll("span")).toHaveLength(2);
  });

  /** Both directions are one list rendered twice. A reason that showed only on
   *  the outbound half would answer "what links here" and not "why". */
  it("shows predicates for the notes linking here as well", async () => {
    notesBacklinks.mockResolvedValue([row({ predicates: ["prov:wasDerivedFrom"] })]);
    render(<LinksPanel vaultId="v1" noteId="n1" direction="from" onOpen={() => {}} />);

    expect(await linkRow()).toHaveTextContent("prov:wasDerivedFrom");
  });
});
