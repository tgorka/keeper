/**
 * The properties panel over a file that is not a note (Story 50.4, FR-283,
 * AD-120).
 *
 * **Why this is a second suite and not more cases in `properties-panel.test`.**
 * That suite's subject is what a property IS — which control a value's shape
 * implies, what survives a write, which keys are keeper's. Every one of those
 * answers is shared, and story 50.4's whole claim is that they stay shared: it
 * adds a second ADDRESS, not a second panel. So the cases here are about the
 * address and nothing else — what happens when the read refuses, what happens
 * when the write refuses, and that the block a file gets is the block a note
 * would have got. The panel's own suite is untouched, which is matrix row 11.
 *
 * **What jsdom cannot see, and does not pretend to.** Byte preservation is the
 * story, and the bytes are Rust's: rows 1–5, 10 and 12 are asserted over real
 * strings in `keeper_core::file_properties`. What is asserted here is the
 * argument this surface hands that code — the `expect` guard and the block —
 * because handing it the wrong one is the only way this half can lose an edit.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const syncReadFrontmatter = vi.fn<(id: string, subpath: string) => Promise<string>>();
const syncWriteFrontmatter =
  vi.fn<(id: string, subpath: string, expected: string, block: string) => Promise<string>>();
const sessionsFileRename =
  vi.fn<(id: string, subpath: string, expected: string, block: string) => Promise<string>>();
const notesSave = vi.fn();
const recordingNoteTargets = vi.fn<(sessionId: string) => Promise<null>>();
const revealPath = vi.fn();
const recordingOpenPath = vi.fn();
const recordingSessionMeta = vi.fn();
const tagsVocabulary = vi.fn<() => Promise<{ entries: { path: string; count: number }[] }>>();

vi.mock("@/lib/ipc/client", () => ({
  syncReadFrontmatter: (id: string, subpath: string) => syncReadFrontmatter(id, subpath),
  syncWriteFrontmatter: (id: string, subpath: string, expected: string, block: string) =>
    syncWriteFrontmatter(id, subpath, expected, block),
  sessionsFileRename: (id: string, subpath: string, expected: string, block: string) =>
    sessionsFileRename(id, subpath, expected, block),
  notesSave: (...args: unknown[]) => notesSave(...args),
  recordingNoteTargets: (sessionId: string) => recordingNoteTargets(sessionId),
  revealPath: (path: string) => revealPath(path),
  recordingOpenPath: (path: string) => recordingOpenPath(path),
  recordingSessionMeta: (folder: string) => recordingSessionMeta(folder),
  tagsVocabulary: () => tagsVocabulary(),
}));

import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import {
  ADD_NOTE_TAG,
  FileProperties,
  PROPERTIES_LABEL,
  PROPERTIES_REREAD_LABEL,
  readFrontmatter,
} from "./properties-panel";

/** The owner's live session record: no frontmatter, body starts with its title. */
const NO_BLOCK = "";

/** What the panel writes for a first `tags: [about]` — one terminated block. */
const ABOUT_BLOCK = "---\ntags:\n  - about\n---\n";

const PROFILE = "tgdrive";
const REL = "60-sessions/active/weekly/README.md";

function mount(onWritten = vi.fn(), onRenamed?: (next: string) => void) {
  render(
    <FileProperties
      profileId={PROFILE}
      relativePath={REL}
      onWritten={onWritten}
      onRenamed={onRenamed}
    />,
  );
  return onWritten;
}

/** Type a tag into the row's chooser and commit it. */
async function addTag(tag: string): Promise<void> {
  fireEvent.click(await screen.findByRole("button", { name: ADD_NOTE_TAG }));
  await waitFor(() => expect(tagsVocabulary).toHaveBeenCalled());
  const field = await screen.findByRole("combobox", { name: ADD_NOTE_TAG });
  fireEvent.change(field, { target: { value: tag } });
  fireEvent.keyDown(field, { key: "Enter" });
}

beforeEach(() => {
  vi.clearAllMocks();
  syncReadFrontmatter.mockResolvedValue(NO_BLOCK);
  syncWriteFrontmatter.mockResolvedValue(ABOUT_BLOCK);
  sessionsFileRename.mockResolvedValue("60-sessions/active/weekly/README.md");
  recordingNoteTargets.mockResolvedValue(null);
  tagsVocabulary.mockResolvedValue({ entries: [] });
  capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES });
});

describe("a file's properties, over a sync-profile address", () => {
  it("reads the block for the file it was pointed at", async () => {
    mount();

    await waitFor(() => expect(syncReadFrontmatter).toHaveBeenCalledWith(PROFILE, REL));
    expect(await screen.findByRole("region", { name: PROPERTIES_LABEL })).toBeInTheDocument();
  });

  it("offers the tag row to a file with no frontmatter at all, and files it", async () => {
    // The acceptance sentence's frontend half: this is the shape the owner's
    // live `README.md` has, and until 50.4 there was no way to give it one.
    mount();
    await screen.findByRole("region", { name: PROPERTIES_LABEL });

    await addTag("about");

    await waitFor(() =>
      expect(syncWriteFrontmatter).toHaveBeenCalledWith(PROFILE, REL, NO_BLOCK, ABOUT_BLOCK),
    );
  });

  it("guards the write with the block it read, and nothing wider", async () => {
    // The third argument is the clobber guard. It is the BLOCK rather than the
    // file, so a concurrent edit to the body neither refuses nor is lost — the
    // byte-level half of that promise is `keeper_core::file_properties`'s.
    syncReadFrontmatter.mockResolvedValue("---\ntitle: Weekly\n---\n");
    mount();
    await screen.findByRole("region", { name: PROPERTIES_LABEL });

    await addTag("about");

    await waitFor(() => expect(syncWriteFrontmatter).toHaveBeenCalledTimes(1));
    const call = syncWriteFrontmatter.mock.calls[0];
    expect(call[2]).toBe("---\ntitle: Weekly\n---\n");
    // And the new block keeps the key that was already there, in its place.
    expect(call[3]).toBe("---\ntitle: Weekly\ntags:\n  - about\n---\n");
  });

  it("adopts the block Rust hands back, so the next write guards on what landed", async () => {
    // Rust returns the block as it now stands, which is not necessarily the one
    // the panel sent — another key may have been there. The next guard has to
    // be that block, or the second edit refuses against a stale `expect`.
    syncWriteFrontmatter.mockResolvedValue("---\nowner: ada\ntags:\n  - about\n---\n");
    mount();
    await screen.findByRole("region", { name: PROPERTIES_LABEL });

    await addTag("about");
    await waitFor(() => expect(syncWriteFrontmatter).toHaveBeenCalledTimes(1));
    expect(syncWriteFrontmatter.mock.calls[0][2]).toBe(NO_BLOCK);

    await addTag("ref");
    await waitFor(() => expect(syncWriteFrontmatter).toHaveBeenCalledTimes(2));
    // Not the block it sent and not the one it read: the one that is on disk.
    expect(syncWriteFrontmatter.mock.calls[1][2]).toBe("---\nowner: ada\ntags:\n  - about\n---\n");
    expect(syncWriteFrontmatter.mock.calls[1][3]).toBe(
      "---\nowner: ada\ntags:\n  - about\n  - ref\n---\n",
    );
  });

  it("tells its host the file changed, so the buffer over the same bytes re-reads", async () => {
    const onWritten = mount();
    await screen.findByRole("region", { name: PROPERTIES_LABEL });

    await addTag("about");

    // Without this the editor beside the panel is holding the pre-write bytes,
    // and its next Save puts the old block back — the one way this panel could
    // lose the edit it just made.
    await waitFor(() => expect(onWritten).toHaveBeenCalledTimes(1));
  });

  it("writes only the tag that was typed, and never infers one", async () => {
    mount();
    await screen.findByRole("region", { name: PROPERTIES_LABEL });

    await addTag("about");

    await waitFor(() => expect(syncWriteFrontmatter).toHaveBeenCalledTimes(1));
    const block = syncWriteFrontmatter.mock.calls[0][3];
    // No `id`, no `updated`, no kind guessed from where the file sits. A file's
    // kind is what its frontmatter says, and the panel is where a person says
    // it.
    expect(block).toBe(ABOUT_BLOCK);
    for (const stamped of ["id:", "updated:", "keeper:"]) {
      expect(block).not.toContain(stamped);
    }
  });
});

describe("a file whose properties keeper will not serve", () => {
  it("row 8: shows no panel at all when the read refuses", async () => {
    // A `workspace/` file (AD-113). The refusal is Rust's, on the same
    // `WriteScope` the write uses, so the panel is absent rather than present
    // and refusing on the first keystroke.
    syncReadFrontmatter.mockRejectedValue({
      message:
        "active/s/workspace/scratch.md is inside a session's workspace — keeper reads it but never writes there.",
    });
    mount();

    await waitFor(() => expect(syncReadFrontmatter).toHaveBeenCalled());
    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
  });

  it("shows nothing while the read is still out", () => {
    mount();

    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
  });
});

describe("row 10: the write refuses rather than clobbering", () => {
  const REFUSAL =
    "60-sessions/active/weekly/README.md's properties changed on disk while they were being edited; nothing was written — re-read the file and try again";

  it("says what Rust said, word for word", async () => {
    syncWriteFrontmatter.mockRejectedValue({ message: REFUSAL });
    mount();
    await screen.findByRole("region", { name: PROPERTIES_LABEL });

    await addTag("about");

    // Rust's sentence and not one composed here: it names the file and says
    // what to do, and a generic line would replace an answer with a shrug.
    expect(await screen.findByRole("alert")).toHaveTextContent(REFUSAL);
  });

  it("offers the re-read, and takes it", async () => {
    syncWriteFrontmatter.mockRejectedValue({ message: REFUSAL });
    const onWritten = mount();
    await screen.findByRole("region", { name: PROPERTIES_LABEL });
    await addTag("about");
    await screen.findByRole("alert");

    syncReadFrontmatter.mockResolvedValue("---\ntags:\n  - ref\n---\n");
    fireEvent.click(screen.getByRole("button", { name: PROPERTIES_REREAD_LABEL }));

    await waitFor(() => expect(syncReadFrontmatter).toHaveBeenCalledTimes(2));
    // The refusal clears with the stale block it was about, and the host is not
    // told the file changed — because nothing was written.
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    expect(onWritten).not.toHaveBeenCalled();
  });

  it("offers no re-read on a note, which has nothing to re-read", async () => {
    // The note address writes a conflict copy rather than refusing, so there is
    // no stale state for a re-read to resolve. Asserted from this side because
    // it is the asymmetry the shared panel has to carry.
    mount();
    await screen.findByRole("region", { name: PROPERTIES_LABEL });

    expect(screen.queryByRole("button", { name: PROPERTIES_REREAD_LABEL })).toBeNull();
  });
});

/**
 * Row 3's frontend half.
 *
 * Rust keeps a CRLF file's endings outside the block, and proves it over real
 * bytes. What this side owes is that it recognises the block at all: reading
 * `---\r\n` as "this file has no properties" is not cosmetic — the panel would
 * then ADD a second block above the first, and everything in the original would
 * become body.
 */
describe("a file written with CRLF endings", () => {
  const CRLF = "---\r\ntitle: Weekly\r\n---\r\n";

  it("is read as a block rather than as no block", () => {
    const parsed = readFrontmatter(CRLF);

    expect(parsed.block).not.toBeNull();
    expect(parsed.newline).toBe("\r\n");
    expect(parsed.entries.map((entry) => [entry.key, entry.text])).toEqual([["title", "Weekly"]]);
  });

  it("gains a key in its own endings, and never a second block", async () => {
    syncReadFrontmatter.mockResolvedValue(CRLF);
    mount();
    await screen.findByRole("region", { name: PROPERTIES_LABEL });

    await addTag("about");

    await waitFor(() => expect(syncWriteFrontmatter).toHaveBeenCalledTimes(1));
    const block = syncWriteFrontmatter.mock.calls[0][3];
    expect(block).toBe("---\r\ntitle: Weekly\r\ntags:\r\n  - about\r\n---\r\n");
    // One block, and the key that was there is still there.
    expect(block.match(/---/g)).toHaveLength(2);
    expect(readFrontmatter(block).entries.map((entry) => entry.key)).toEqual(["title", "tags"]);
  });
});

/**
 * A title change renames the file (Story 51.6, FR-295; matrix rows 1 and 6).
 *
 * **What this side decides, and it is the whole of it: which command.** Whether
 * the name follows, what it becomes, and what refuses are all
 * `keeper_core::sessions::files`'s, asserted over real strings there. What can
 * only go wrong here is routing a retitle to `sync_write_frontmatter`, which
 * would splice the title and leave the filename — the defect the owner reported.
 */
describe("a session file's title, changed in the panel", () => {
  const TITLED = "---\ntitle: untitled\n---\n";

  /** Change the `title` row's field and commit it, which is a blur or an Enter. */
  async function retitle(next: string): Promise<void> {
    const field = await screen.findByRole("textbox", { name: "title" });
    fireEvent.change(field, { target: { value: next } });
    fireEvent.blur(field);
  }

  it("goes to the rename verb, not to the block writer", async () => {
    syncReadFrontmatter.mockResolvedValue(TITLED);
    mount();
    await screen.findByRole("region", { name: PROPERTIES_LABEL });

    await retitle("Kick Off");

    await waitFor(() =>
      expect(sessionsFileRename).toHaveBeenCalledWith(
        PROFILE,
        REL,
        TITLED,
        "---\ntitle: Kick Off\n---\n",
      ),
    );
    // Never both: two commands over one block would be two journal rows, and the
    // second would be guarding against the first.
    expect(syncWriteFrontmatter).not.toHaveBeenCalled();
  });

  it("leaves every other key on the block writer", async () => {
    syncReadFrontmatter.mockResolvedValue(TITLED);
    mount();
    await screen.findByRole("region", { name: PROPERTIES_LABEL });

    await addTag("about");

    await waitFor(() => expect(syncWriteFrontmatter).toHaveBeenCalledTimes(1));
    expect(sessionsFileRename).not.toHaveBeenCalled();
  });

  /**
   * The two refusals the story exists to make honest, in Rust's own words: a
   * collision names the file it would have overwritten, and a title that folds to
   * nothing says the title was not written either — which is the sentence that
   * stops a reader assuming half of it landed.
   */
  it("prints the refusal verbatim, including the one that says the title did not land", async () => {
    const refusal =
      '"###" has nothing in it a filename can be named after — it needs letters or digits. keeper will not invent a name, and it has not written the title either: a file renamed halfway is worse than one not renamed at all.';
    sessionsFileRename.mockRejectedValue({ message: refusal });
    syncReadFrontmatter.mockResolvedValue(TITLED);
    mount();
    await screen.findByRole("region", { name: PROPERTIES_LABEL });

    await retitle("###");

    expect(await screen.findByRole("alert")).toHaveTextContent(refusal);
  });

  /**
   * A subpath no string surgery on this side could have produced: a different
   * directory, and a filename that is not the title that was typed. So an
   * assertion that finds it can only have got it from the command's answer,
   * which is the whole of what AD-65 asks this side to prove.
   */
  const MOVED = "60-sessions/archive/2026-02/kick-off-notes.md";

  it("tells its host WHERE the file moved to, in the subpath Rust answered with", async () => {
    syncReadFrontmatter.mockResolvedValue(TITLED);
    sessionsFileRename.mockResolvedValue(MOVED);
    const onRenamed = vi.fn<(next: string) => void>();
    const onWritten = mount(vi.fn(), onRenamed);
    await screen.findByRole("region", { name: PROPERTIES_LABEL });

    await retitle("Kick Off");

    await waitFor(() => expect(onRenamed).toHaveBeenCalledWith(MOVED));
    // And NOT the news alone: `onWritten` means "read the address you have
    // again", and after a rename that address is the one the rename emptied —
    // which is what put "is no longer in tgdrive" over a file that had only been
    // renamed. A host told where it went must not also be told to look where it
    // no longer is.
    expect(onWritten).not.toHaveBeenCalled();
  });

  it("still just says the file changed to a host that cannot re-address itself", async () => {
    syncReadFrontmatter.mockResolvedValue(TITLED);
    sessionsFileRename.mockResolvedValue(MOVED);
    const onWritten = mount();
    await screen.findByRole("region", { name: PROPERTIES_LABEL });

    await retitle("Kick Off");

    // Exactly today's behaviour for the note embed and every other host that
    // holds no panel target, so adding the second hook changed no caller.
    await waitFor(() => expect(onWritten).toHaveBeenCalledTimes(1));
  });
});
