import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteWriteVm } from "@/lib/ipc/client";
import { PropertiesPanel, readFrontmatter } from "./properties-panel";

const notesSave = vi.fn<(id: string, text: string, rev: string) => Promise<NoteWriteVm>>();

vi.mock("@/lib/ipc/client", () => ({
  notesSave: (id: string, text: string, rev: string) => notesSave(id, text, rev),
}));

/** A note whose frontmatter holds a key keeper has never heard of, written in
 *  a style keeper would not have chosen. Both must survive an unrelated edit. */
const NOTE = [
  "---",
  "id: 01ARZ3NDEKTSV4RRFFQ69G5FAV",
  "tags:",
  "  - work",
  "  - clients/acme",
  "pinned: false",
  "mood:   'pensive, mostly'",
  "---",
  "",
  "# Standing meeting",
  "",
].join("\n");

beforeEach(() => {
  vi.clearAllMocks();
  notesSave.mockResolvedValue({ rev: "rev-2", path: "notes/standing.md", conflictCopy: null });
});

function renderPanel(text: string = NOTE) {
  return render(
    <PropertiesPanel text={text} subscriptionId="sub-1" baseRev="rev-1" onSaved={() => {}} />,
  );
}

describe("readFrontmatter", () => {
  it("infers a control from each value's shape", () => {
    const parsed = readFrontmatter(NOTE);
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

  it("reports a block it will not touch rather than rewriting it", () => {
    const parsed = readFrontmatter("---\nweird: !!str [a\n---\nbody\n");
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
    const written = notesSave.mock.calls[0][1];
    expect(notesSave.mock.calls[0][0]).toBe("sub-1");
    expect(notesSave.mock.calls[0][2]).toBe("rev-1");
    // The one key that changed, changed.
    expect(written).toContain("pinned: true");
    // Everything else — including a key keeper does not know, its odd spacing
    // and its single quotes — is exactly the bytes that came in (FR-121).
    expect(written).toContain("mood:   'pensive, mostly'");
    expect(written).toContain("id: 01ARZ3NDEKTSV4RRFFQ69G5FAV");
    expect(written).toContain("tags:\n  - work\n  - clients/acme\n");
    expect(written).toContain("# Standing meeting");
    expect(written.replace("pinned: true", "pinned: false")).toBe(NOTE);
  });

  it("writes a list edit in the style the note already used", async () => {
    renderPanel();

    fireEvent.click(screen.getByRole("button", { name: "Remove work from tags" }));

    await waitFor(() => {
      expect(notesSave).toHaveBeenCalledTimes(1);
    });
    const written = notesSave.mock.calls[0][1];
    expect(written).toContain("tags:\n  - clients/acme\npinned: false");
    expect(written).toContain("mood:   'pensive, mostly'");
  });

  it("never edits the ULID, because links resolve through it", () => {
    renderPanel();
    expect(screen.queryByRole("textbox", { name: "id" })).toBeNull();
  });
});
