import { render, screen, waitFor } from "@testing-library/react";
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
    predicate: null,
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
 * The predicate is the author's own word for a relationship, written on the
 * link: `[Belief](belief.md){reference="supports"}`. "What links here" is a
 * weaker question than "what supports this", and the answer is only useful if
 * the list says which it is.
 */
describe("a link that says what kind of link it is", () => {
  it("shows the predicate beside the note it connects to", async () => {
    notesForwardlinks.mockResolvedValue([row({ predicate: "supports" })]);
    render(<LinksPanel vaultId="v1" noteId="n1" direction="to" onOpen={() => {}} />);

    await waitFor(() => expect(screen.getByText("supports")).toBeInTheDocument());
  });

  /** Nearly every link has none, and a chip that is always there is furniture. */
  it("shows nothing extra for a link written without one", async () => {
    notesBacklinks.mockResolvedValue([row()]);
    render(<LinksPanel vaultId="v1" noteId="n1" direction="from" onOpen={() => {}} />);

    await waitFor(() => expect(screen.getByText("Belief statement")).toBeInTheDocument());
    expect(screen.queryByText("supports")).not.toBeInTheDocument();
  });
});
