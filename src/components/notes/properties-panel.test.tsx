import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteWriteVm } from "@/lib/ipc/client";
import { PropertiesPanel, readFrontmatter } from "./properties-panel";

const notesSave =
  vi.fn<
    (id: string, text: string, rev: string, frontmatter: string | null) => Promise<NoteWriteVm>
  >();

vi.mock("@/lib/ipc/client", () => ({
  notesSave: (id: string, text: string, rev: string, frontmatter: string | null) =>
    notesSave(id, text, rev, frontmatter),
}));

/** A block holding a key keeper has never heard of, written in a style keeper
 *  would not have chosen. Both must survive an unrelated edit. */
const BLOCK = [
  "---",
  "id: 01ARZ3NDEKTSV4RRFFQ69G5FAV",
  "tags:",
  "  - work",
  "  - clients/acme",
  "pinned: false",
  "mood:   'pensive, mostly'",
  "---",
  "",
].join("\n");

/** The buffer, which the panel never edits but always writes. */
const BODY = "\n# Standing meeting\n\nunsaved keystrokes\n";

beforeEach(() => {
  vi.clearAllMocks();
  notesSave.mockResolvedValue({
    rev: "rev-2",
    path: "notes/standing.md",
    frontmatter: BLOCK,
    conflictCopy: null,
  });
});

function renderPanel(frontmatter: string = BLOCK) {
  return render(
    <PropertiesPanel
      frontmatter={frontmatter}
      body={BODY}
      subscriptionId="sub-1"
      baseRev="rev-1"
      onSaved={() => {}}
    />,
  );
}

describe("readFrontmatter", () => {
  it("infers a control from each value's shape", () => {
    const parsed = readFrontmatter(BLOCK);
    expect(parsed.unparsed).toBe(false);
    expect(parsed.entries.map((entry) => [entry.key, entry.kind])).toEqual([
      ["id", "text"],
      ["tags", "list"],
      ["pinned", "boolean"],
      ["mood", "text"],
    ]);
    expect(parsed.entries[1].items).toEqual(["work", "clients/acme"]);
    expect(parsed.entries[3].text).toBe("pensive, mostly");
  });

  it("reads a block delivered on its own, exactly as it reads one at the head of a note", () => {
    const alone = readFrontmatter(BLOCK);
    const inDocument = readFrontmatter(`${BLOCK}\n# body\n`);
    expect(inDocument.entries.map((entry) => entry.valueFrom)).toEqual(
      alone.entries.map((entry) => entry.valueFrom),
    );
  });

  it("reports a block it will not touch rather than rewriting it", () => {
    const parsed = readFrontmatter("---\nweird: !!str [a\n---\n");
    expect(parsed.unparsed).toBe(true);
  });
});

describe("PropertiesPanel", () => {
  it("preserves every other key byte-for-byte when one key is edited", async () => {
    renderPanel();

    fireEvent.click(screen.getByRole("switch", { name: "pinned" }));

    await waitFor(() => {
      expect(notesSave).toHaveBeenCalledTimes(1);
    });
    const [subscription, body, baseRev, written] = notesSave.mock.calls[0];
    expect(subscription).toBe("sub-1");
    expect(baseRev).toBe("rev-1");
    // The block is the fourth argument, because the block is what this panel owns.
    expect(written).not.toBeNull();
    const block = written ?? "";
    // The one key that changed, changed.
    expect(block).toContain("pinned: true");
    // Everything else — including a key keeper does not know, its odd spacing
    // and its single quotes — is exactly the bytes that came in (FR-121).
    expect(block).toContain("mood:   'pensive, mostly'");
    expect(block).toContain("id: 01ARZ3NDEKTSV4RRFFQ69G5FAV");
    expect(block).toContain("tags:\n  - work\n  - clients/acme\n");
    expect(block.replace("pinned: true", "pinned: false")).toBe(BLOCK);
    // And the body goes along untouched: one write covers the whole note, so a
    // property edit must not discard what the user has typed since the last save.
    expect(body).toBe(BODY);
    expect(block).not.toContain("Standing meeting");
  });

  it("writes a list edit in the style the note already used", async () => {
    renderPanel();

    fireEvent.click(screen.getByRole("button", { name: "Remove work from tags" }));

    await waitFor(() => {
      expect(notesSave).toHaveBeenCalledTimes(1);
    });
    const block = notesSave.mock.calls[0][3] ?? "";
    expect(block).toContain("tags:\n  - clients/acme\npinned: false");
    expect(block).toContain("mood:   'pensive, mostly'");
  });

  it("creates a block for a note that has none, without touching the body", async () => {
    renderPanel("");

    fireEvent.change(screen.getByRole("textbox", { name: "New property name" }), {
      target: { value: "pinned" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    await waitFor(() => {
      expect(notesSave).toHaveBeenCalledTimes(1);
    });
    const [, body, , written] = notesSave.mock.calls[0];
    expect(written).toBe('---\npinned: ""\n---\n');
    expect(body).toBe(BODY);
  });

  it("never edits the ULID, because links resolve through it", () => {
    renderPanel();
    expect(screen.queryByRole("textbox", { name: "id" })).toBeNull();
  });
});
