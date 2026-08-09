/**
 * Story 43.7, in two halves.
 *
 * The first half renders the panel and asserts what it lists and what it hands
 * out. The second mounts the real `NoteEditor` — its own boot effect, its own
 * dynamic imports, a real `EditorView` — and presses the panel's button, then
 * reads the document back out of the notes store. That second half is the one
 * that matters: this story's whole risk is in the seam between a React panel
 * and a CodeMirror caret, and a test that asserted `onInsert` was called with
 * the right string would prove nothing about whether the string reaches the
 * note, whether it lands where the caret is, or whether it survives the blur
 * the click itself causes.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteBodyBatch, NoteWriteVm, RecordingNoteTargetVm } from "@/lib/ipc/client";

const recordingNoteTargets =
  vi.fn<(sessionId: string) => Promise<RecordingNoteTargetVm[] | null>>();
const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();
/** A body save returns the block unchanged, because a body save does not touch
 *  it. The editor adopts what this returns, so a `""` here would silently take
 *  the note's `session:` away the first time the panel's click blurs the
 *  editor — and every assertion after that would be about the wrong note. */
const notesSave = vi.fn<(id: string, text: string, rev: string) => Promise<NoteWriteVm>>();

vi.mock("@/lib/ipc/client", () => ({
  recordingNoteTargets: (sessionId: string) => recordingNoteTargets(sessionId),
  notesOpen: (v: string, n: string, on: (b: NoteBodyBatch) => void) => notesOpen(v, n, on),
  notesClose: vi.fn(async () => {}),
  notesSave: (id: string, text: string, rev: string) => notesSave(id, text, rev),
  notesBufferReport: vi.fn(async () => {}),
  notesTagTree: vi.fn(async () => ({ nodes: [] })),
  notesBacklinks: vi.fn(async () => []),
  notesResolveConflict: vi.fn(async () => {}),
  notesMarkRead: vi.fn(async () => {}),
  notesDiff: vi.fn(async () => null),
  notesHistory: vi.fn(async () => []),
  notesCreate: vi.fn(async () => ""),
  notesLinkTargets: vi.fn(async () => []),
  recordingOpenPath: vi.fn(async () => {}),
  revealPath: vi.fn(async () => {}),
}));

import { notesEditorStore } from "@/lib/stores/notes-editor";
import { AttachmentsPanel } from "./attachments-panel";
import { NoteEditor } from "./note-editor";

// Same shim, same reason, as `tab-wiring.test.tsx`: jsdom does no layout, so
// CodeMirror's measure pass would throw out of the test on the first frame.
if (!Range.prototype.getClientRects) {
  Range.prototype.getClientRects = () =>
    Object.assign([] as DOMRect[], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
}

/** The folder as the note was written: Story 42.4's stub, before a retitle. */
const WRITTEN = "recordings/2026/2026-08-08 1552 standup";
const SCREEN = `${WRITTEN}/screen-0000.mov`;
const MANIFEST = `${WRITTEN}/manifest.json`;

const RECORDING_BLOCK = [
  "---",
  "title: Standup",
  "session: 01KYH5DXGP1XQRHTME8CJFVEJ6-01KZHS7EJB5QKR8T9CHXQ46RNS",
  `recording: ${WRITTEN}`,
  "files:",
  `  - ${SCREEN}`,
  `  - ${MANIFEST}`,
  "---",
  "",
].join("\n");

/**
 * Where Rust says the session is NOW: Story 40.4 renamed the folder after the
 * note was written, and the index holds a file the note does not list. Both
 * disagreements are deliberate — one proves the panel inserts the note's own
 * text, the other proves it lists the note's files and not the folder's.
 */
const FOUND = "recordings/2026/2026-08-08 1552 standup retitled";
const ROOT = "/Users/alice/Movies/keeper";
const TARGETS: RecordingNoteTargetVm[] = [
  { relativePath: FOUND, absolutePath: `${ROOT}/${FOUND}`, kind: "folder" },
  {
    relativePath: `${FOUND}/screen-0000.mov`,
    absolutePath: `${ROOT}/${FOUND}/screen-0000.mov`,
    kind: "video",
  },
  {
    relativePath: `${FOUND}/camera-0000.mov`,
    absolutePath: `${ROOT}/${FOUND}/camera-0000.mov`,
    kind: "video",
  },
  {
    relativePath: `${FOUND}/manifest.json`,
    absolutePath: `${ROOT}/${FOUND}/manifest.json`,
    kind: "file",
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  recordingNoteTargets.mockResolvedValue(TARGETS);
  notesSave.mockResolvedValue({
    rev: "r1",
    path: "n.md",
    frontmatter: RECORDING_BLOCK,
    conflictCopy: null,
  });
});

describe("the attachment panel", () => {
  /** Rendered, with its resolve settled. */
  async function panel(frontmatter: string, body: string, onInsert = vi.fn()) {
    render(<AttachmentsPanel frontmatter={frontmatter} body={body} onInsert={onInsert} />);
    await screen.findByLabelText("Attachments");
    // The kind arrives from Rust one microtask later; every assertion below
    // wants the settled panel, not the one mid-resolve.
    await waitFor(() => {
      expect(recordingNoteTargets).toHaveBeenCalledTimes(frontmatter.includes("session:") ? 1 : 0);
    });
    return onInsert;
  }

  it("lists exactly the note's own attachments — not the session folder's, and not the folder", async () => {
    await panel(RECORDING_BLOCK, "notes\n");

    const rows = screen.getAllByRole("listitem");
    expect(rows.map((row) => within(row).getByTitle(/./).textContent)).toEqual([
      "screen-0000.mov",
      "manifest.json",
    ]);
    // The index holds a camera track this note never listed. It is the
    // session's file; it is not this note's attachment.
    expect(screen.queryByText("camera-0000.mov")).toBeNull();
    // And the session folder is not an attachment at all: there is no element
    // for a directory, so there is nothing for an embed of one to become.
    expect(screen.queryByText(/standup$/)).toBeNull();
  });

  it("says what each file is, in the vocabulary Rust decided", async () => {
    await panel(RECORDING_BLOCK, "notes\n");

    const [screenRow, manifestRow] = screen.getAllByRole("listitem");
    expect(within(screenRow).getByText("video")).toBeInTheDocument();
    expect(within(manifestRow).getByText("file")).toBeInTheDocument();
  });

  it("hands out the text a user would type — the note's own path, never the index's", async () => {
    const onInsert = await panel(RECORDING_BLOCK, "notes\n");

    fireEvent.click(screen.getByRole("button", { name: `Insert ${SCREEN}` }));

    // Byte for byte: `!`, the wikilink, and the path the note itself carries.
    // The index says the folder has been renamed; writing THAT path in would
    // give the note two frames for one session and a line no hand typed.
    expect(onInsert).toHaveBeenCalledTimes(1);
    expect(onInsert).toHaveBeenCalledWith(
      "![[recordings/2026/2026-08-08 1552 standup/screen-0000.mov]]",
    );
  });

  it("offers no insert for an attachment the body already embeds", async () => {
    await panel(RECORDING_BLOCK, `intro\n\n![[${SCREEN}]]\n`);

    expect(screen.queryByRole("button", { name: `Insert ${SCREEN}` })).toBeNull();
    const [screenRow] = screen.getAllByRole("listitem");
    expect(within(screenRow).getByText("In the note")).toBeInTheDocument();
    // The other one is untouched: this is a fact about one file, not a mode.
    expect(screen.getByRole("button", { name: `Insert ${MANIFEST}` })).toBeInTheDocument();
  });

  it("counts an embed written under the folder's old name as the same attachment", async () => {
    // The note lists the pre-retitle path and the body embeds the post-retitle
    // one. Two paths, one file, and inserting again would show the reader the
    // same video twice.
    await panel(RECORDING_BLOCK, `![[${FOUND}/screen-0000.mov]]\n`);

    expect(screen.queryByRole("button", { name: `Insert ${SCREEN}` })).toBeNull();
  });

  it("does not mistake a plain wikilink for an embed", async () => {
    await panel(RECORDING_BLOCK, `see [[${SCREEN}]]\n`);

    // `[[…]]` mentions the file; `![[…]]` shows it. The panel only ever writes
    // the second, so only the second is already there.
    expect(screen.getByRole("button", { name: `Insert ${SCREEN}` })).toBeInTheDocument();
  });

  it("tells a note with no session that it is not a recording note", async () => {
    await panel("---\ntitle: Groceries\nfiles:\n  - list.txt\n---\n", "milk\n");

    expect(screen.getByText(/isn't a recording note/)).toBeInTheDocument();
    // Not an empty list, which would read as "this note has no attachments"
    // when the truth is a different fact about the note. And a `files:` key in
    // somebody else's note is somebody else's list.
    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
    expect(screen.queryByText("list.txt")).toBeNull();
    expect(recordingNoteTargets).not.toHaveBeenCalled();
  });

  it("says something different again when a recording note lists no files", async () => {
    await panel("---\nsession: 01KY-01KZ\nrecording: rec/2026/x\n---\n", "notes\n");

    expect(screen.getByText(/list no files/)).toBeInTheDocument();
    // The two empty states are two sentences, because they are two facts.
    expect(screen.queryByText(/isn't a recording note/)).toBeNull();
  });

  it("still lists and still inserts when keeper cannot locate the session", async () => {
    recordingNoteTargets.mockResolvedValue(null);
    const onInsert = await panel(RECORDING_BLOCK, "notes\n");

    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByText(/can't locate this session/)).toBeInTheDocument();
    // No kind is claimed for a file keeper cannot find — the classifier is
    // Rust's one answer, and guessing from the extension here would be the
    // second table Story 43.5 exists to prevent.
    expect(screen.queryByText("video")).toBeNull();
    // The text it would write does not depend on the index at all.
    fireEvent.click(screen.getByRole("button", { name: `Insert ${SCREEN}` }));
    expect(onInsert).toHaveBeenCalledWith(`![[${SCREEN}]]`);
  });

  it("makes no claim about the session while the answer is still in flight", async () => {
    // The executor form, not `Promise.withResolvers`: the project compiles
    // against `lib: ES2020`, where that constructor method does not exist.
    // `recording-summary-card.test.tsx` holds a call open the same way.
    let answer!: (targets: RecordingNoteTargetVm[] | null) => void;
    recordingNoteTargets.mockReturnValue(
      new Promise<RecordingNoteTargetVm[] | null>((resolve) => {
        answer = resolve;
      }),
    );
    render(<AttachmentsPanel frontmatter={RECORDING_BLOCK} body="notes\n" onInsert={vi.fn()} />);
    await screen.findByLabelText("Attachments");

    // Listed and insertable immediately — the list comes from the note, which
    // is already here. What is missing is only the kind.
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    // And no accusation: "keeper can't locate this session" is a claim, and
    // keeper has not finished looking. Rendered on the first frame of every
    // note, it would be a sentence the user learns to disbelieve.
    expect(screen.queryByText(/can't locate this session/)).toBeNull();

    answer(null);
    expect(await screen.findByText(/can't locate this session/)).toBeInTheDocument();
  });

  it("does not label the next note's files with this note's kinds", async () => {
    // The panel outlives the note in it: it stays mounted while the user moves
    // from one recording note to another. A second session whose file happens
    // to be called `screen-0000.mov` — every session's is — would wear the
    // first session's `video` label for as long as its own resolve took.
    const { rerender } = render(
      <AttachmentsPanel frontmatter={RECORDING_BLOCK} body="notes\n" onInsert={vi.fn()} />,
    );
    expect(await screen.findByText("video")).toBeInTheDocument();

    // Held open for good: what matters is the window before the second answer
    // arrives, which in a real app is a round trip to Rust.
    recordingNoteTargets.mockReturnValue(new Promise(() => {}));
    const next = RECORDING_BLOCK.replace(
      "01KZHS7EJB5QKR8T9CHXQ46RNS",
      "01KZOTHERSESSIONXXXXXXXXXX",
    );
    rerender(<AttachmentsPanel frontmatter={next} body="notes\n" onInsert={vi.fn()} />);

    expect(recordingNoteTargets).toHaveBeenCalledTimes(2);
    expect(screen.queryByText("video")).toBeNull();
    expect(screen.queryByText("file")).toBeNull();
  });

  it("claims no kind for a file the index cannot place, and offers it anyway", async () => {
    const gone = `${WRITTEN}/notes-i-deleted.txt`;
    const block = RECORDING_BLOCK.replace(`  - ${MANIFEST}\n`, `  - ${MANIFEST}\n  - ${gone}\n`);
    await panel(block, "notes\n");

    const row = screen.getAllByRole("listitem")[2];
    expect(within(row).getByTitle(gone)).toBeInTheDocument();
    // Every kind word on screen belongs to the two files Rust did place. This
    // row says nothing about what its file is, because keeper does not know —
    // the extension is right there and reading it here would be the second
    // classifier Story 43.5 exists to prevent.
    expect(within(row).queryByText(/^(video|image|audio|file)$/)).toBeNull();
    // Offered regardless: the note says the file is its own, and the text the
    // panel would write is the note's, not the index's.
    expect(within(row).getByRole("button", { name: `Insert ${gone}` })).toBeInTheDocument();
  });

  it("reads a hand-written scalar files: as the one attachment it plainly is", async () => {
    await panel(`---\nsession: 01KY-01KZ\nfiles: ${SCREEN}\n---\n`, "notes\n");

    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    expect(screen.getByRole("button", { name: `Insert ${SCREEN}` })).toBeInTheDocument();
  });
});

/**
 * The half a component test cannot reach.
 *
 * Everything above hands the panel an `onInsert` spy. Here the panel is wired
 * to the real editor, and the assertion is on the buffer the notes store holds
 * — which the store only learns about because CodeMirror's update listener
 * called `onEdit`. If that reads right, the text went through the surface a
 * user types into and is on its way to disk.
 */
describe("inserting, through the editor the user types into", () => {
  const OPENED = "alpha\nbeta\n";

  beforeEach(() => {
    notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
      onBatch({
        kind: "reset",
        text: OPENED,
        frontmatter: RECORDING_BLOCK,
        rev: "r0",
        // Pinned mid-document on purpose: "at the cursor" is only observable
        // when the cursor is somewhere the end of the buffer is not.
        cursor: 5,
        path: "n.md",
      } as NoteBodyBatch);
      return "sub-1";
    });
  });

  afterEach(() => {
    notesEditorStore.setState({ text: "", base: "", subscriptionId: null, frontmatter: "" });
  });

  /** Mount the editor, wait for its lazy chunk, and open the panel. */
  async function editorWithPanel(): Promise<void> {
    render(<NoteEditor vaultId="v1" noteId="n1" />);
    await waitFor(() => {
      expect(document.querySelector(".cm-content")).not.toBeNull();
      expect(notesEditorStore.getState().text).toBe(OPENED);
    });
    fireEvent.click(screen.getByRole("button", { name: "Attachments" }));
    await screen.findByLabelText("Attachments");
  }

  /**
   * Press one attachment's Insert the way a pointer does.
   *
   * `fireEvent.click` alone moves no focus in jsdom, which would have made
   * "the editor gets its focus back" pass whether or not anything handed it
   * back. A real button takes focus on the way down — and takes it OFF the
   * editor, which fires the blur save and is the moment the caret could be
   * lost — so the test does that first and the assertion means something.
   */
  async function pressInsert(relativePath: string): Promise<void> {
    const button = await screen.findByRole("button", { name: `Insert ${relativePath}` });
    button.focus();
    expect(document.activeElement).toBe(button);
    fireEvent.click(button);
  }

  it("writes the embed at the caret, and hands focus back", async () => {
    await editorWithPanel();

    await pressInsert(SCREEN);

    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe(`alpha![[${SCREEN}]]\nbeta\n`);
    });
    // Not appended, not at offset zero: where the caret was. And the editor has
    // the focus the click took off it, so the next keystroke is still typing.
    expect(document.activeElement?.classList.contains("cm-content")).toBe(true);
  });

  it("cannot write the same attachment twice", async () => {
    await editorWithPanel();

    await pressInsert(SCREEN);
    await waitFor(() => {
      expect(notesEditorStore.getState().text).toContain("![[");
    });

    // There is no second press to make: the control is gone the moment the
    // body holds the embed, so duplication is not guarded against, it is
    // unavailable.
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: `Insert ${SCREEN}` })).toBeNull();
    });
    expect(screen.getByText("In the note")).toBeInTheDocument();
    const occurrences = notesEditorStore.getState().text.split("![[").length - 1;
    expect(occurrences).toBe(1);
  });

  it("leaves the other attachment insertable, at the caret the first one left", async () => {
    await editorWithPanel();

    await pressInsert(SCREEN);
    await waitFor(() => {
      expect(notesEditorStore.getState().text).toContain("screen-0000.mov");
    });
    await pressInsert(MANIFEST);

    // The caret ended after the first embed, so the second lands against it —
    // exactly as it would for a person who typed both without moving.
    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe(`alpha![[${SCREEN}]]![[${MANIFEST}]]\nbeta\n`);
    });
  });

  it("says the note is not a recording note when the note has no session", async () => {
    notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
      onBatch({
        kind: "reset",
        text: OPENED,
        frontmatter: "---\ntitle: Groceries\n---\n",
        rev: "r0",
        cursor: 0,
        path: "n.md",
      } as NoteBodyBatch);
      return "sub-1";
    });
    await editorWithPanel();

    expect(screen.getByText(/isn't a recording note/)).toBeInTheDocument();
    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
  });
});
