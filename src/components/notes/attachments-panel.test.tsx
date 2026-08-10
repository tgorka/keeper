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
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  NoteAttachSourceVm,
  NoteBodyBatch,
  NoteWriteVm,
  RecordingNoteTargetVm,
} from "@/lib/ipc/client";
import { withRangeRects } from "@/test/layout";

const recordingNoteTargets =
  vi.fn<(sessionId: string) => Promise<RecordingNoteTargetVm[] | null>>();
const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();
/** A body save returns the block unchanged, because a body save does not touch
 *  it. The editor adopts what this returns, so a `""` here would silently take
 *  the note's `session:` away the first time the panel's click blurs the
 *  editor — and every assertion after that would be about the wrong note. */
const notesSave = vi.fn<(id: string, text: string, rev: string) => Promise<NoteWriteVm>>();
/** Story 46.2: the panel has to agree with the picker's own receipt, so the
 *  picker has to be pressable here. Reached only from a click, which is why the
 *  suites that merely mount the editor correctly have no stub for it. */
const notesAttachSources = vi.fn<(v: string, s: string[]) => Promise<NoteAttachSourceVm[]>>();
/** Story 46.11: the panel asks the vault which of the note's embeds it holds.
 *  Defaulted in `beforeEach` to "every target resolves where it is written",
 *  because that is the ordinary vault and every 46.2 case is about a file that
 *  is there. */
const notesEmbedPaths = vi.fn<(v: string, targets: string[]) => Promise<(string | null)[]>>();
const pickFiles = vi.fn<() => Promise<string[] | null>>();

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: () => pickFiles(),
}));

vi.mock("@/lib/ipc/client", () => ({
  recordingNoteTargets: (sessionId: string) => recordingNoteTargets(sessionId),
  notesOpen: (v: string, n: string, on: (b: NoteBodyBatch) => void) => notesOpen(v, n, on),
  notesClose: vi.fn(async () => {}),
  notesSave: (id: string, text: string, rev: string) => notesSave(id, text, rev),
  notesAttachSources: (v: string, s: string[]) => notesAttachSources(v, s),
  notesEmbedPaths: (v: string, targets: string[]) => notesEmbedPaths(v, targets),
  notesBufferReport: vi.fn(async () => {}),
  notesTagTree: vi.fn(async () => ({ nodes: [] })),
  notesBacklinks: vi.fn(async () => []),
  // Reached, but only on a SLOW run, which is what made it a latent flake
  // rather than a deterministic red. `NoteEditor` mounts `TemplateUpdateOffer`,
  // which calls this after `TEMPLATE_OFFER_IDLE_MS` (4 s) of idle. A test that
  // finishes inside four seconds never gets there; under eight-agent load these
  // do not, the call fires, and the missing export throws an unhandled
  // rejection that times the test out 5 s later. Seen once in three repeats.
  notesTemplateUpdatePreview: vi.fn(async () => null),
  notesResolveConflict: vi.fn(async () => {}),
  notesMarkRead: vi.fn(async () => {}),
  notesDiff: vi.fn(async () => null),
  notesHistory: vi.fn(async () => []),
  notesCreate: vi.fn(async () => ""),
  notesLinkTargets: vi.fn(async () => []),
  recordingOpenPath: vi.fn(async () => {}),
  revealPath: vi.fn(async () => {}),
}));

import { readNoteDocument, resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import { ATTACH_FILE_LABEL, ATTACH_FROM_COMPUTER_LABEL } from "./attach-file-button";
import { ATTACHMENTS_LABEL, AttachmentsPanel } from "./attachments-panel";
import { NOTE_ACTIONS_LABEL } from "./note-actions";
import { NoteEditor } from "./note-editor";

/**
 * jsdom does no layout, so CodeMirror's measure pass throws on any frame that
 * elapses mid-test — and a jsdom throw escaping a measure pass takes the run's
 * exit code while the summary line still prints passes.
 *
 * This file used to carry its own shim. It was wrong in the way that matters:
 * it installed an EMPTY `DOMRectList`, so a measure that DID run read
 * `rects[0]` as undefined and threw anyway. That is a permanent fault which
 * only ever SHOWED as an occasional red, because whether a frame elapses at all
 * depends on how busy the machine is. It was never an ordering problem —
 * vitest isolates per test file (measured by W2Marks and W2Emoji with a
 * two-file probe, one of them checked for sensitivity by inverting it;
 * `isolate` and `pool` are unset in `vitest.config.ts` and default to
 * isolated), so every file starts with a clean `Range.prototype` and the old
 * `if (!…)` guard was always true.
 *
 * `withRangeRects` returns real rects and its undo is mandatory and paired.
 */
let restoreRects: (() => void) | undefined;
beforeAll(() => {
  restoreRects = withRangeRects();
});
afterAll(() => {
  restoreRects?.();
});

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
  // The ordinary vault: every target resolves at the path it is written at.
  // Rust's own resolution can answer a different path for a bare name — the
  // cases that care set their own answer.
  notesEmbedPaths.mockImplementation(async (_vault, targets) => targets);
  notesSave.mockResolvedValue({
    rev: "r1",
    path: "n.md",
    frontmatter: RECORDING_BLOCK,
    conflictCopy: null,
  });
});

describe("the attachment panel", () => {
  /** Rendered, with both of its resolves settled. */
  async function panel(frontmatter: string, body: string, onInsert = vi.fn()) {
    render(
      <AttachmentsPanel vaultId="v1" frontmatter={frontmatter} body={body} onInsert={onInsert} />,
    );
    await screen.findByLabelText("Attachments");
    // The kind arrives from Rust one microtask later, and so does the answer to
    // "which of these does the vault hold" (Story 46.11); every assertion below
    // wants the settled panel, not the one mid-resolve. `findBy*` on the rows
    // would not do it — a test asserting an ABSENCE has nothing to wait for.
    await waitFor(() => {
      expect(recordingNoteTargets).toHaveBeenCalledTimes(frontmatter.includes("session:") ? 1 : 0);
      expect(screen.queryByText("Looking…")).toBeNull();
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

  it("tells an ordinary note it has no attachments, and says nothing about recordings", async () => {
    await panel("---\ntitle: Groceries\nfiles:\n  - list.txt\n---\n", "milk\n");

    // The panel is called "Attachments", so the sentence in it is about
    // attachments. It used to announce that the note is not a recording note —
    // true, a different fact, and the answer to a question nobody asked.
    expect(screen.getByText(/no attachments/)).toBeInTheDocument();
    expect(screen.queryByText(/recording note/)).toBeNull();
    // And it says how one gets here, because "no attachments" with no next
    // step is where the owner stopped.
    expect(screen.getByText(/Attaching one adds it here/)).toBeInTheDocument();
    // A `files:` key in somebody else's note is somebody else's list: it is
    // relative to the recordings destination root, and this note has no root.
    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
    expect(screen.queryByText("list.txt")).toBeNull();
    expect(recordingNoteTargets).not.toHaveBeenCalled();
  });

  /**
   * The reported defect, at the surface that reported it (Story 46.2).
   *
   * The owner attached a file from outside the vault, keeper copied it into
   * `attachments/` and wrote `![[attachments/…]]` into the body, and the panel
   * named "Attachments" said the note is not a recording note. It read `files:`
   * and returned before it had looked at the body at all.
   */
  it("lists an attachment an ordinary note embeds in its body", async () => {
    await panel("---\ntitle: Groceries\n---\n", "milk\n\n![[attachments/receipt.png]]\n");

    const rows = screen.getAllByRole("listitem");
    expect(rows).toHaveLength(1);
    expect(within(rows[0]).getByTitle("attachments/receipt.png").textContent).toBe("receipt.png");
    // Not the dead end it used to be.
    expect(screen.queryByText(/no attachments/)).toBeNull();
    expect(screen.queryByText(/recording note/)).toBeNull();
    // No session, so nothing was asked of the index — and therefore no kind is
    // claimed. `kindOf` matches by NAME, so a session holding its own
    // `receipt.png` would label this row from a different file entirely.
    expect(recordingNoteTargets).not.toHaveBeenCalled();
    expect(within(rows[0]).queryByText(/^(video|image|audio|file|folder)$/)).toBeNull();
    // And no Insert: the row exists BECAUSE the body embeds it, so the one
    // label a session row wears once inserted is the only one this can wear.
    // No new verb was invented — reveal and open both need an absolute path,
    // and FR-145 is the reason this panel holds none.
    expect(within(rows[0]).queryByRole("button")).toBeNull();
    expect(within(rows[0]).getByText("In the note")).toBeInTheDocument();
  });

  it("lists both sources for a recording note, without changing the session rows", async () => {
    await panel(RECORDING_BLOCK, `notes\n\n![[attachments/receipt.png]]\n`);

    const rows = screen.getAllByRole("listitem");
    // The session's two, in the order the note lists them, then the body's —
    // and the session rows still offer Insert, still say what each file is.
    expect(rows.map((row) => within(row).getByTitle(/./).textContent)).toEqual([
      "screen-0000.mov",
      "manifest.json",
      "receipt.png",
    ]);
    expect(screen.getByRole("button", { name: `Insert ${SCREEN}` })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: `Insert ${MANIFEST}` })).toBeInTheDocument();
    expect(within(rows[0]).getByText("video")).toBeInTheDocument();
    // Two lists in two frames — `files:` is relative to the recordings
    // destination root, a body embed to the vault — so each says which it is.
    expect(screen.getByText("From this note's properties")).toBeInTheDocument();
    expect(screen.getByText("In this note's body")).toBeInTheDocument();
  });

  /**
   * The ruling epic 46's spine made for this story: `attachments/` stops being
   * the test (Story 46.11).
   *
   * An in-vault attach names the file where it already lives, so it never
   * acquires the prefix 46.2 read. The reason this note has a row is that it
   * embeds a file **and the vault holds it** — and the vault is what says so, in
   * one call, in `embed::candidates` order.
   */
  it("lists a file the note embeds from anywhere in the vault, not only from attachments/", async () => {
    await panel("---\ntitle: Trip\n---\n", "the beach\n\n![[photos/2026/holiday.png]]\n");

    const rows = screen.getAllByRole("listitem");
    expect(rows).toHaveLength(1);
    expect(within(rows[0]).getByTitle("photos/2026/holiday.png").textContent).toBe("holiday.png");
    // Asked, rather than assumed: the panel is pure about the text and asks Rust
    // about the disk, through the same resolver the embed viewer uses.
    expect(notesEmbedPaths).toHaveBeenCalledWith("v1", ["photos/2026/holiday.png"]);
    expect(screen.queryByText(/no attachments/)).toBeNull();
  });

  it("lists both shapes at once — the copy in attachments/ and the file left where it was", async () => {
    await panel(
      "---\ntitle: Trip\n---\n",
      "![[attachments/receipt.png]]\n\n![[photos/holiday.png]]\n",
    );

    // One list, in document order, and no distinction drawn between them:
    // "keeper copied this one in" is a fact about how the file got here, not
    // about what the note has now.
    expect(
      screen
        .getAllByRole("listitem")
        .map((row) => within(row).getByTitle(/./).getAttribute("title")),
    ).toEqual(["attachments/receipt.png", "photos/holiday.png"]);
  });

  it("shows the path the vault resolved a bare name to, not the bare name", async () => {
    // `embed::candidates` tries the target where it is written first and in the
    // attachments folder second, so which file `![[photo.png]]` means is a
    // question only Rust can answer — which is exactly why 46.2 declined to list
    // it and why this story can.
    notesEmbedPaths.mockResolvedValue(["attachments/photo.png"]);
    await panel("---\ntitle: Trip\n---\n", "![[photo.png]]\n");

    const row = screen.getByRole("listitem");
    expect(within(row).getByTitle("attachments/photo.png")).toBeInTheDocument();
  });

  it("names an embed whose file is not in the vault instead of dropping the row in silence", async () => {
    notesEmbedPaths.mockResolvedValue([null, "attachments/here.png"]);
    await panel("---\ntitle: Trip\n---\n", "![[photos/deleted.png]]\n![[attachments/here.png]]\n");

    // The one that is there is a row; the one that is not is a sentence naming
    // it. A row means "the vault has it", so the missing file does not get one —
    // and it does not get silence either, which is the failure this epic is
    // about.
    const rows = screen.getAllByRole("listitem");
    expect(rows).toHaveLength(1);
    expect(within(rows[0]).getByTitle("attachments/here.png")).toBeInTheDocument();
    expect(
      screen.getByText(/embeds photos\/deleted\.png, which is not in this vault/),
    ).toBeInTheDocument();
    // And it is not the empty state: a note with a broken embed is not a note
    // with no attachments.
    expect(screen.queryByText(/no attachments/)).toBeNull();
  });

  it("makes no claim about the vault while the probe is in flight", async () => {
    // The executor form, not `Promise.withResolvers`: the project compiles
    // against `lib: ES2020`, where that constructor method does not exist.
    let answer!: (paths: (string | null)[]) => void;
    notesEmbedPaths.mockReturnValue(
      new Promise<(string | null)[]>((resolve) => {
        answer = resolve;
      }),
    );
    render(
      <AttachmentsPanel
        vaultId="v1"
        frontmatter="---\ntitle: Trip\n---\n"
        body="![[photos/holiday.png]]\n"
        onInsert={vi.fn()}
      />,
    );
    await screen.findByLabelText(ATTACHMENTS_LABEL);

    // Neither claim: not "no attachments" (the note plainly embeds something)
    // and not "not in this vault" (keeper has not finished looking). Both would
    // be sentences a user learns to disbelieve.
    expect(screen.queryByText(/no attachments/)).toBeNull();
    expect(screen.queryByText(/not in this vault/)).toBeNull();
    expect(screen.getByText("Looking…")).toBeInTheDocument();

    answer(["photos/holiday.png"]);
    expect(await screen.findByTitle("photos/holiday.png")).toBeInTheDocument();
    expect(screen.queryByText("Looking…")).toBeNull();
  });

  it("blames itself, not the note, when it cannot ask the vault at all", async () => {
    notesEmbedPaths.mockRejectedValue(new Error("that vault is not open"));
    await panel("---\ntitle: Trip\n---\n", "![[photos/holiday.png]]\n");

    // keeper did not find out. Saying "not in this vault" would blame the note
    // for keeper's own outage, and saying "no attachments" would be worse.
    expect(screen.getByText(/could not check which of this note's files/)).toBeInTheDocument();
    expect(screen.queryByText(/not in this vault/)).toBeNull();
    expect(screen.queryByText(/no attachments/)).toBeNull();
    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
  });

  /**
   * 46.2's residual edge case, which this story makes reachable and therefore
   * has to answer.
   *
   * A recording note's `files:` are relative to the recordings destination root,
   * which may be anywhere — the fixture's is `~/Movies/keeper`. So a session
   * embed resolves to nothing IN THE VAULT, and it renders anyway, because
   * `recording-embed.ts` resolves it against the session index. The session list
   * is the authority on those files; the body list neither duplicates them nor
   * accuses the note of having lost them.
   */
  it("leaves a session file to the session list, listing it once and calling it missing never", async () => {
    notesEmbedPaths.mockResolvedValue([null]);
    await panel(RECORDING_BLOCK, `notes\n\n![[${SCREEN}]]\n`);

    // The session's two rows and nothing else: no second row for the embed, and
    // no accusation about it either.
    expect(
      screen
        .getAllByRole("listitem")
        .map((row) => within(row).getByTitle(/./).getAttribute("title")),
    ).toEqual([SCREEN, MANIFEST]);
    expect(screen.queryByText(/not in this vault/)).toBeNull();
    // The session row is the one that reports it, in the panel's own word for
    // the fact.
    expect(screen.queryByRole("button", { name: `Insert ${SCREEN}` })).toBeNull();
    expect(screen.getByText("In the note")).toBeInTheDocument();
    // And no captions: there is only one list on screen.
    expect(screen.queryByText("In this note's body")).toBeNull();
  });

  it("asks the vault nothing about a note that embeds nothing", async () => {
    await panel("---\ntitle: Groceries\n---\n", "milk\n");

    // No embeds, no question — and the empty state arrives immediately rather
    // than after a round trip that had nothing to ask.
    expect(notesEmbedPaths).not.toHaveBeenCalled();
    expect(screen.getByText(/no attachments/)).toBeInTheDocument();
  });

  it("re-asks the vault when an embed is added, and not on every keystroke", async () => {
    const { rerender } = render(
      <AttachmentsPanel
        vaultId="v1"
        frontmatter="---\ntitle: Trip\n---\n"
        body="![[photos/holiday.png]]\n"
        onInsert={vi.fn()}
      />,
    );
    await screen.findByTitle("photos/holiday.png");
    expect(notesEmbedPaths).toHaveBeenCalledTimes(1);

    // Typing prose changes the body on every keystroke and changes the TARGETS
    // on none of them. A probe per keystroke would be a `stat` per keystroke.
    rerender(
      <AttachmentsPanel
        vaultId="v1"
        frontmatter="---\ntitle: Trip\n---\n"
        body="![[photos/holiday.png]]\nthe beach was\n"
        onInsert={vi.fn()}
      />,
    );
    expect(notesEmbedPaths).toHaveBeenCalledTimes(1);

    // A second embed is a new question.
    rerender(
      <AttachmentsPanel
        vaultId="v1"
        frontmatter="---\ntitle: Trip\n---\n"
        body="![[photos/holiday.png]]\n![[photos/sunset.png]]\n"
        onInsert={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(notesEmbedPaths).toHaveBeenCalledTimes(2);
    });
    expect(notesEmbedPaths).toHaveBeenLastCalledWith("v1", [
      "photos/holiday.png",
      "photos/sunset.png",
    ]);
  });

  it("captions neither list when only one of them is on screen", async () => {
    await panel(RECORDING_BLOCK, "notes\n");

    // A heading over a single list is a word that carries nothing, and there is
    // no second list for it to distinguish this one from.
    expect(screen.queryByText("From this note's properties")).toBeNull();
    expect(screen.queryByText("In this note's body")).toBeNull();
  });

  it("says something different again when a recording note lists no files", async () => {
    await panel("---\nsession: 01KY-01KZ\nrecording: rec/2026/x\n---\n", "notes\n");

    expect(screen.getByText(/list no files/)).toBeInTheDocument();
    // The two empty states are two sentences, because they are two facts: a
    // recording note whose properties list nothing is not the same as a note
    // with no attachments.
    expect(screen.queryByText(/no attachments/)).toBeNull();
  });

  it("shows a recording note's body attachment even when its properties list no files", async () => {
    await panel(
      "---\nsession: 01KY-01KZ\nrecording: rec/2026/x\n---\n",
      "![[attachments/receipt.png]]\n",
    );

    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    // The empty state is for an empty panel, and this one is not empty.
    expect(screen.queryByText(/list no files/)).toBeNull();
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
    render(
      <AttachmentsPanel
        vaultId="v1"
        frontmatter={RECORDING_BLOCK}
        body="notes\n"
        onInsert={vi.fn()}
      />,
    );
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
      <AttachmentsPanel
        vaultId="v1"
        frontmatter={RECORDING_BLOCK}
        body="notes\n"
        onInsert={vi.fn()}
      />,
    );
    expect(await screen.findByText("video")).toBeInTheDocument();

    // Held open for good: what matters is the window before the second answer
    // arrives, which in a real app is a round trip to Rust.
    recordingNoteTargets.mockReturnValue(new Promise(() => {}));
    const next = RECORDING_BLOCK.replace(
      "01KZHS7EJB5QKR8T9CHXQ46RNS",
      "01KZOTHERSESSIONXXXXXXXXXX",
    );
    rerender(
      <AttachmentsPanel vaultId="v1" frontmatter={next} body="notes\n" onInsert={vi.fn()} />,
    );

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
      });
      return "sub-1";
    });
  });

  afterEach(() => {
    resetNotesEditorStoreForTest();
  });

  /** Mount the editor, wait for its lazy chunk, and open the panel. */
  async function editorWithPanel(): Promise<void> {
    render(<NoteEditor vaultId="v1" noteId="n1" />);
    await waitFor(() => {
      expect(document.querySelector(".cm-content")).not.toBeNull();
      expect(readNoteDocument("v1", "n1").text).toBe(OPENED);
    });
    // Story 46.5: the control that opens this panel is a menu item now. Read as
    // a `menuitem`, because `ATTACHMENTS_LABEL` also names the panel's own
    // `<section>` — a bare name query would resolve to the thing being opened
    // and the failure would read as "the item is missing".
    const trigger = await screen.findByRole("button", {
      name: new RegExp(`^${NOTE_ACTIONS_LABEL}`),
    });
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.pointerUp(trigger, { button: 0 });
    const menu = await screen.findByRole("menu");
    fireEvent.click(within(menu).getByRole("menuitem", { name: ATTACHMENTS_LABEL }));
    await screen.findByLabelText(ATTACHMENTS_LABEL);
  }

  /**
   * Reach the OS picker, which is a menu item now (Story 46.11).
   *
   * "Attach a file" is a dropdown trigger since the header gained a second
   * source: AD-104 leaves the action group at two controls, so the two doors are
   * two items rather than two buttons. The trigger keeps its name, so what
   * changed for a test is one interaction and not a query.
   */
  async function pickFromComputer(): Promise<void> {
    const trigger = screen.getByRole("button", { name: ATTACH_FILE_LABEL });
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.pointerUp(trigger, { button: 0 });
    const menu = await screen.findByRole("menu");
    fireEvent.click(within(menu).getByRole("menuitem", { name: ATTACH_FROM_COMPUTER_LABEL }));
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
      expect(readNoteDocument("v1", "n1").text).toBe(`alpha![[${SCREEN}]]\nbeta\n`);
    });
    // Not appended, not at offset zero: where the caret was. And the editor has
    // the focus the click took off it, so the next keystroke is still typing.
    expect(document.activeElement?.classList.contains("cm-content")).toBe(true);
  });

  it("cannot write the same attachment twice", async () => {
    await editorWithPanel();

    await pressInsert(SCREEN);
    await waitFor(() => {
      expect(readNoteDocument("v1", "n1").text).toContain("![[");
    });

    // There is no second press to make: the control is gone the moment the
    // body holds the embed, so duplication is not guarded against, it is
    // unavailable.
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: `Insert ${SCREEN}` })).toBeNull();
    });
    expect(screen.getByText("In the note")).toBeInTheDocument();
    const occurrences = readNoteDocument("v1", "n1").text.split("![[").length - 1;
    expect(occurrences).toBe(1);
  });

  it("leaves the other attachment insertable, at the caret the first one left", async () => {
    await editorWithPanel();

    await pressInsert(SCREEN);
    await waitFor(() => {
      expect(readNoteDocument("v1", "n1").text).toContain("screen-0000.mov");
    });
    await pressInsert(MANIFEST);

    // The caret ended after the first embed, so the second lands against it —
    // exactly as it would for a person who typed both without moving.
    await waitFor(() => {
      expect(readNoteDocument("v1", "n1").text).toBe(`alpha![[${SCREEN}]]![[${MANIFEST}]]\nbeta\n`);
    });
  });

  /** Reopen the note as an ordinary one — no `session:`, so no `files:` list and
   *  nothing but the body for the panel to read. */
  function ordinaryNote(): void {
    notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
      onBatch({
        kind: "reset",
        text: OPENED,
        frontmatter: "---\ntitle: Groceries\n---\n",
        rev: "r0",
        cursor: 0,
        path: "n.md",
      });
      return "sub-1";
    });
    // And the block a save gives back, for the same reason the recording one is
    // mocked at all: the editor adopts what `notesSave` returns, so leaving the
    // recording block here would hand this ordinary note a `session:` the first
    // time it saved — and every assertion after that would be about a recording
    // note. Reachable since Story 46.11: opening the attach dropdown takes focus
    // off the editor, which is what fires the blur save.
    notesSave.mockResolvedValue({
      rev: "r1",
      path: "n.md",
      frontmatter: "---\ntitle: Groceries\n---\n",
      conflictCopy: null,
    });
  }

  it("tells an ordinary note it has no attachments", async () => {
    ordinaryNote();
    await editorWithPanel();

    expect(screen.getByText(/no attachments/)).toBeInTheDocument();
    expect(screen.queryByText(/recording note/)).toBeNull();
    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
  });

  /**
   * The defect, end to end, through the two surfaces that disagreed (Story
   * 46.2, AD-103).
   *
   * The owner pressed "Attach a file", the banner said keeper had copied it
   * into `attachments/`, and the panel underneath said the note is not a
   * recording note. One gesture, two surfaces, opposite answers — and the panel
   * was the one that was wrong. This asserts them together, because a banner
   * saying 1 over a panel showing 0 is the same defect one layer along.
   *
   * The panel reads the BUFFER, so the row is there on the keystroke after the
   * insert rather than after the next write to disk — asserted by the buffer and
   * the row agreeing while `notesEmbedPaths` was asked about the buffer's own
   * target. A save may well happen in between now: opening the attach dropdown
   * moves focus out of the editor, which is what the blur save is for.
   */
  it("lists what the picker just attached, agreeing with the picker's own receipt", async () => {
    ordinaryNote();
    pickFiles.mockResolvedValue(["/Users/alice/Desktop/receipt.png"]);
    notesAttachSources.mockResolvedValue([
      { name: "receipt.png", relPath: "attachments/receipt.png", copied: true, refusal: null },
    ]);
    await editorWithPanel();

    // Nothing yet, and the panel says so about attachments.
    expect(screen.getByText(/no attachments/)).toBeInTheDocument();

    await pickFromComputer();

    // The receipt, from the button.
    expect(await screen.findByText(/copied it into attachments\//)).toBeInTheDocument();
    // And the same file, in the panel named after it.
    const row = await screen.findByRole("listitem");
    expect(within(row).getByTitle("attachments/receipt.png").textContent).toBe("receipt.png");
    // Straight from the buffer: spliced at the caret this note opened with, and
    // the target the panel asked the vault about is the one in that buffer —
    // never a path read back off disk.
    expect(readNoteDocument("v1", "n1").text).toBe(`![[attachments/receipt.png]]${OPENED}`);
    expect(notesEmbedPaths).toHaveBeenCalledWith("v1", ["attachments/receipt.png"]);
  });
});
