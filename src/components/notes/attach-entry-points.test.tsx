/**
 * Story 45.13: one insertion path, three entry points.
 *
 * The claim this story makes is not "each of these three surfaces can attach a
 * file". It is that they are **the same act**, so the file lands as the same
 * bytes whichever one you used. A test per surface asserting its own expected
 * string would let all three drift together and still pass, which is exactly
 * how this app came to have two attachment inserters that disagreed. So the
 * central test here runs all three and asserts their results against *each
 * other*, byte for byte, with no expected literal in the middle.
 *
 * Two of the three go through the real `NoteEditor` — its boot effect, its
 * dynamic imports, a real `EditorView` — because "the text reaches the note"
 * is a claim about the seam between React and a CodeMirror caret, and a spy on
 * `onInsert` proves nothing about it. The third writes to a note nobody has
 * open, so its result is read off the write command.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  NoteAttachSourceVm,
  NoteAttachTargetVm,
  NoteBodyBatch,
  NoteBodyVm,
  NoteWriteVm,
  RecordingNoteTargetVm,
} from "@/lib/ipc/client";
import { withRangeRects } from "@/test/layout";

const recordingNoteTargets =
  vi.fn<(sessionId: string) => Promise<RecordingNoteTargetVm[] | null>>();
const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();
const notesSave = vi.fn<(id: string, text: string, rev: string) => Promise<NoteWriteVm>>();
const notesAttachSources = vi.fn<(v: string, s: string[]) => Promise<NoteAttachSourceVm[]>>();
const notesAttachTargets =
  vi.fn<(v: string, q: string, n: string[]) => Promise<NoteAttachTargetVm[]>>();
const notesBodyRead = vi.fn<(v: string, n: string) => Promise<NoteBodyVm>>();
const notesBodyWrite =
  vi.fn<(v: string, n: string, text: string, rev: string) => Promise<NoteWriteVm>>();
const pickFiles = vi.fn<() => Promise<string[] | string | null>>();

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: () => pickFiles(),
}));

/**
 * The mock factory is the minimum the boot path actually reaches, and the
 * NAMES THAT ARE ABSENT ARE PART OF THE ASSERTION.
 *
 * A `vi.mock` factory replaces the whole module, and vitest throws
 * `No "x" export is defined on the mock` on ACCESS — not on import. So a name
 * listed here is a claim that mounting this surface reaches it, and a name
 * left out is a claim that it does not. Twenty were listed when this file was
 * written, copied from `attachments-panel.test.tsx` and never questioned. **Ten
 * of them were never reached:** `notesTagTree`, `notesGallery`,
 * `notesLinkTargets`, `notesResolveConflict`, `notesMarkRead`, `notesDiff`,
 * `notesHistory`, `notesCreate`, `recordingOpenPath`, `revealPath`. Removed one
 * group at a time, re-running between, and the suite is still green — so each
 * was quietly telling the next reader something false about what `NoteEditor`
 * does on mount.
 *
 * `notesBacklinks` is the one that failed that experiment and came back:
 * `BacklinksPanel` mounts in edit mode and asks for them. That is the shape of
 * evidence this comment rests on — every name here was kept because removing it
 * broke something, not because it looked plausible.
 *
 * `notesAttachSources` earns its place for the opposite reason, and it is worth
 * stating: the picker calls it only inside a click handler, so the two suites
 * that merely MOUNT the editor (`emoji-wiring`, `new-note-caret`) need no stub
 * for it and correctly have none. It is here because this file clicks.
 *
 * A future test that drives a path needing one of the removed names fails
 * loudly with the name in the message. That is the intended behaviour: add it
 * back then, knowing something reaches it.
 */
vi.mock("@/lib/ipc/client", () => ({
  recordingNoteTargets: (sessionId: string) => recordingNoteTargets(sessionId),
  notesOpen: (v: string, n: string, on: (b: NoteBodyBatch) => void) => notesOpen(v, n, on),
  notesClose: vi.fn(async () => {}),
  notesSave: (id: string, text: string, rev: string) => notesSave(id, text, rev),
  notesAttachSources: (v: string, s: string[]) => notesAttachSources(v, s),
  notesAttachTargets: (v: string, q: string, n: string[]) => notesAttachTargets(v, q, n),
  notesBodyRead: (v: string, n: string) => notesBodyRead(v, n),
  notesBodyWrite: (v: string, n: string, text: string, rev: string) =>
    notesBodyWrite(v, n, text, rev),
  notesBufferReport: vi.fn(async () => {}),
  notesBacklinks: vi.fn(async () => []),
}));

import { notesEditorStore } from "@/lib/stores/notes-editor";
import { ATTACH_FILE_LABEL } from "./attach-file-button";
import {
  ATTACH_ACTION_LABEL,
  ATTACH_HOLDS_PREFIX,
  ATTACH_OUTCOME_TESTID,
  ATTACH_SEARCH_LABEL,
  AttachToNoteDialog,
} from "./attach-to-note-dialog";
import { NoteEditor } from "./note-editor";

/**
 * jsdom has no `Range.getClientRects`, and CodeMirror's measure pass throws on
 * the first frame that elapses mid-test — taking the run's exit code with it.
 */
let restoreRects: () => void;
beforeEach(() => {
  restoreRects = withRangeRects();
});
afterEach(() => {
  restoreRects();
});

/** The one file all three entry points attach, in the three frames it exists in. */
const ABSOLUTE = "/Users/alice/Movies/keeper/recordings/2026/standup/screen-0000.mov";
const RELATIVE = "recordings/2026/standup/screen-0000.mov";

/**
 * The body every entry point starts from, and the caret at its end.
 *
 * The caret matters to the parity claim and is the only thing that had to be
 * arranged for it: an insertion at the caret and an append to a closed note are
 * the same write exactly when the caret is where the append would go. Anywhere
 * else the two produce different notes, correctly — this test is about the
 * bytes the attachment contributes, not about where a person left their cursor.
 */
const OPENED = "alpha\nbeta\n";

const RECORDING_BLOCK = [
  "---",
  "title: Standup",
  "session: 01KYH5DXGP1XQRHTME8CJFVEJ6-01KZHS7EJB5QKR8T9CHXQ46RNS",
  "recording: recordings/2026/standup",
  "files:",
  `  - ${RELATIVE}`,
  "---",
  "",
].join("\n");

const NOTE: NoteAttachTargetVm = {
  id: "n1",
  title: "Standup",
  path: "notes/standup.md",
  holds: [],
};

beforeEach(() => {
  vi.clearAllMocks();
  recordingNoteTargets.mockResolvedValue([
    {
      relativePath: RELATIVE,
      absolutePath: ABSOLUTE,
      kind: "video",
    },
  ]);
  notesSave.mockResolvedValue({
    rev: "r1",
    path: "notes/standup.md",
    frontmatter: RECORDING_BLOCK,
    conflictCopy: null,
  });
  notesBodyWrite.mockResolvedValue({
    rev: "r1",
    path: "notes/standup.md",
    frontmatter: RECORDING_BLOCK,
    conflictCopy: null,
  });
  notesAttachSources.mockResolvedValue([
    { name: "screen-0000.mov", relPath: RELATIVE, copied: false, refusal: null },
  ]);
  notesAttachTargets.mockResolvedValue([NOTE]);
  notesBodyRead.mockResolvedValue({ rev: "r0", text: OPENED });
  pickFiles.mockResolvedValue([ABSOLUTE]);
  notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
    onBatch({
      kind: "reset",
      text: OPENED,
      frontmatter: RECORDING_BLOCK,
      rev: "r0",
      cursor: OPENED.length,
      path: "notes/standup.md",
    });
    return "sub-1";
  });
});

afterEach(() => {
  notesEditorStore.setState({ text: "", base: "", subscriptionId: null, frontmatter: "" });
});

/** Mount the real editor and wait for its lazy chunk and its first document. */
async function mountEditor(): Promise<void> {
  render(<NoteEditor vaultId="v1" noteId="n1" />);
  await waitFor(() => {
    expect(document.querySelector(".cm-content")).not.toBeNull();
    expect(notesEditorStore.getState().text).toBe(OPENED);
  });
}

/** Entry point one: the attachments panel Story 43.7 built. */
async function throughThePanel(): Promise<string> {
  await mountEditor();
  fireEvent.click(screen.getByRole("button", { name: "Attachments" }));
  const insert = await screen.findByRole("button", { name: `Insert ${RELATIVE}` });
  // A real button takes focus on the way down, off the editor, which fires the
  // blur save — the moment the caret could be lost.
  insert.focus();
  fireEvent.click(insert);
  await waitFor(() => {
    expect(notesEditorStore.getState().text).toContain("![[");
  });
  return notesEditorStore.getState().text;
}

/** Entry point two: a file picked off the drive, into the open note. */
async function throughThePicker(): Promise<string> {
  await mountEditor();
  fireEvent.click(screen.getByRole("button", { name: ATTACH_FILE_LABEL }));
  await waitFor(() => {
    expect(notesEditorStore.getState().text).toContain("![[");
  });
  return notesEditorStore.getState().text;
}

/** Entry point three: a Files-pane selection, into a note nobody has open. */
async function throughTheChooser(sources: string[] = [ABSOLUTE]): Promise<string> {
  render(<AttachToNoteDialog vaultId="v1" sources={sources} onClose={vi.fn()} />);
  const attach = await screen.findByRole("button", {
    name: `${ATTACH_ACTION_LABEL} to ${NOTE.title}`,
  });
  fireEvent.click(attach);
  await waitFor(() => {
    expect(notesBodyWrite).toHaveBeenCalled();
  });
  return notesBodyWrite.mock.calls[0][2];
}

describe("one insertion path", () => {
  /**
   * The story, in one assertion. No expected literal: each result is compared
   * with the others, so a change to the embed spelling can only pass here by
   * changing all three at once — which is what having one spelling means.
   */
  it("puts the same file into the note as the same bytes from all three entry points", async () => {
    const fromPanel = await throughThePanel();
    document.body.innerHTML = "";
    notesEditorStore.setState({ text: "", base: "", subscriptionId: null, frontmatter: "" });

    const fromPicker = await throughThePicker();
    document.body.innerHTML = "";
    notesEditorStore.setState({ text: "", base: "", subscriptionId: null, frontmatter: "" });

    const fromChooser = await throughTheChooser();

    expect(fromPanel).toBe(fromPicker);
    expect(fromPicker).toBe(fromChooser);
    // And it is the note plus one embed, not the note plus something that
    // merely matched between three broken implementations.
    expect(fromPanel).toBe(`${OPENED}![[${RELATIVE}]]`);
  });

  /**
   * FR-145, at the surface that is most likely to break it: this is the only
   * entry point that starts from an absolute path, and the absolute path must
   * not survive into the note. The webview sends it to Rust and writes back
   * only what Rust returned.
   */
  it("never writes the absolute path it was handed", async () => {
    const written = await throughTheChooser();

    expect(notesAttachSources).toHaveBeenCalledWith("v1", [ABSOLUTE]);
    expect(written).not.toContain(ABSOLUTE);
    expect(written).not.toContain("/Users/");
    expect(written).toContain(`![[${RELATIVE}]]`);
  });
});

describe("a file that is not in the vault", () => {
  /**
   * The branch: copy it in. A link would be an absolute path (FR-145 forbids
   * it, and the vault syncs to machines where it names nothing) and a refusal
   * would make "attachments from anywhere" mean "attachments from the vault".
   *
   * Rust does the copying; what is asserted here is that the surface writes the
   * path of the COPY and says out loud that a copy was made.
   */
  it("attaches the copy keeper made, and says that it made one", async () => {
    const outside = "/Users/alice/Desktop/holiday.png";
    notesAttachSources.mockResolvedValue([
      { name: "holiday.png", relPath: "attachments/holiday.png", copied: true, refusal: null },
    ]);
    notesAttachTargets.mockResolvedValue([NOTE]);

    const written = await throughTheChooser([outside]);

    expect(notesAttachSources).toHaveBeenCalledWith("v1", [outside]);
    expect(written).toBe(`${OPENED}![[attachments/holiday.png]]`);
    expect(screen.getByTestId(ATTACH_OUTCOME_TESTID).textContent).toContain(
      "outside the vault, so keeper copied it into attachments/",
    );
  });

  /** A folder is not a file. Rust refuses it and the sentence is Rust's. */
  it("passes on Rust's refusal rather than inventing one", async () => {
    notesAttachSources.mockResolvedValue([
      {
        name: "Trip",
        relPath: null,
        copied: false,
        refusal:
          "Trip is a folder. A note can embed a file, but there is nothing to show for a directory.",
      },
    ]);
    render(
      <AttachToNoteDialog
        vaultId="v1"
        sources={["/Users/alice/Pictures/Trip"]}
        onClose={vi.fn()}
      />,
    );
    fireEvent.click(
      await screen.findByRole("button", { name: `${ATTACH_ACTION_LABEL} to ${NOTE.title}` }),
    );

    await waitFor(() => {
      expect(screen.getByTestId(ATTACH_OUTCOME_TESTID).textContent).toContain("Trip is a folder.");
    });
    // Nothing to write, so nothing was written — not an empty save.
    expect(notesBodyWrite).not.toHaveBeenCalled();
  });

  /**
   * The invariant `NoteAttachSourceVm` states in prose and enforces nowhere:
   * exactly one of `relPath` and `refusal` is set.
   *
   * If the surface derived its refusals by filtering `refusal !== null`, a
   * source that came back with NEITHER would land in no list and vanish
   * without a word — this story's original bug, reachable through a VM shape
   * rather than through the duplicate rule. Partitioning on `relPath === null`
   * instead makes that structurally impossible, and this is the test that says
   * so: keeper composing a worse sentence than Rust's is a far better answer
   * than silence.
   */
  it("says something about a source that came back with neither a path nor a reason", async () => {
    notesAttachSources.mockResolvedValue([
      { name: "mystery.bin", relPath: null, copied: false, refusal: null },
    ]);

    render(
      <AttachToNoteDialog vaultId="v1" sources={["/Users/alice/mystery.bin"]} onClose={vi.fn()} />,
    );
    fireEvent.click(
      await screen.findByRole("button", { name: `${ATTACH_ACTION_LABEL} to ${NOTE.title}` }),
    );

    await waitFor(() => {
      expect(screen.getByTestId(ATTACH_OUTCOME_TESTID).textContent).toContain("mystery.bin");
    });
    expect(notesBodyWrite).not.toHaveBeenCalled();
  });
});

describe("a multiselection", () => {
  const SECOND = "/Users/alice/Movies/keeper/recordings/2026/standup/camera-0000.mov";
  const SECOND_REL = "recordings/2026/standup/camera-0000.mov";

  /**
   * The stated order is the order offered, one embed per line.
   *
   * Asserts the CALL as well as the result. `notesAttachSources` is mocked with
   * `mockResolvedValue`, which answers the same list whatever it is handed — so
   * without the first expectation, sending Rust only the first of the two
   * sources would still produce both embeds and this test would still pass. It
   * did: that mutation survived until this line existed.
   */
  it("writes every file, in the order the selection offered them", async () => {
    notesAttachSources.mockResolvedValue([
      { name: "screen-0000.mov", relPath: RELATIVE, copied: false, refusal: null },
      { name: "camera-0000.mov", relPath: SECOND_REL, copied: false, refusal: null },
    ]);

    const written = await throughTheChooser([ABSOLUTE, SECOND]);

    expect(notesAttachSources).toHaveBeenCalledWith("v1", [ABSOLUTE, SECOND]);
    expect(written).toBe(`${OPENED}![[${RELATIVE}]]\n![[${SECOND_REL}]]`);
  });

  /** Order is a property of the request, not of the alphabet. */
  it("keeps the offered order even when it is not sorted", async () => {
    notesAttachSources.mockResolvedValue([
      { name: "camera-0000.mov", relPath: SECOND_REL, copied: false, refusal: null },
      { name: "screen-0000.mov", relPath: RELATIVE, copied: false, refusal: null },
    ]);

    const written = await throughTheChooser([SECOND, ABSOLUTE]);

    expect(written).toBe(`${OPENED}![[${SECOND_REL}]]\n![[${RELATIVE}]]`);
  });
});

describe("a duplicate", () => {
  /**
   * The failure this story exists to end: doing nothing and not saying so.
   * The note is not written and the person is told which file and why.
   */
  it("is refused with a sentence, and nothing is written", async () => {
    notesBodyRead.mockResolvedValue({ rev: "r0", text: `${OPENED}![[${RELATIVE}]]\n` });

    render(<AttachToNoteDialog vaultId="v1" sources={[ABSOLUTE]} onClose={vi.fn()} />);
    fireEvent.click(
      await screen.findByRole("button", { name: `${ATTACH_ACTION_LABEL} to ${NOTE.title}` }),
    );

    await waitFor(() => {
      expect(screen.getByTestId(ATTACH_OUTCOME_TESTID).textContent).toBe(
        "screen-0000.mov is already in this note, so keeper left it out.",
      );
    });
    expect(notesBodyWrite).not.toHaveBeenCalled();
  });

  /**
   * Matched by name across a folder rename (Story 40.4), which is the reason
   * the key is the name and not the path. The note points at the old folder;
   * the selection points at the new one; it is one file.
   */
  it("is still a duplicate when the folder around the file was renamed", async () => {
    notesBodyRead.mockResolvedValue({
      rev: "r0",
      text: "![[recordings/2026/standup renamed/screen-0000.mov]]\n",
    });

    render(<AttachToNoteDialog vaultId="v1" sources={[ABSOLUTE]} onClose={vi.fn()} />);
    fireEvent.click(
      await screen.findByRole("button", { name: `${ATTACH_ACTION_LABEL} to ${NOTE.title}` }),
    );

    await waitFor(() => {
      expect(screen.getByTestId(ATTACH_OUTCOME_TESTID).textContent).toContain(
        "already in this note",
      );
    });
    expect(notesBodyWrite).not.toHaveBeenCalled();
  });
});

describe("the note search", () => {
  const OTHER: NoteAttachTargetVm = {
    id: "n2",
    title: "Holiday plans",
    path: "notes/holiday.md",
    holds: [],
  };

  it("asks Rust for the typed query, with the names being attached", async () => {
    render(<AttachToNoteDialog vaultId="v1" sources={[ABSOLUTE]} onClose={vi.fn()} />);
    await screen.findByRole("button", { name: `${ATTACH_ACTION_LABEL} to ${NOTE.title}` });

    fireEvent.change(screen.getByLabelText(ATTACH_SEARCH_LABEL), { target: { value: "stand" } });

    await waitFor(() => {
      expect(notesAttachTargets).toHaveBeenLastCalledWith("v1", "stand", ["screen-0000.mov"]);
    });
  });

  it("finds a note by title", async () => {
    notesAttachTargets.mockImplementation(async (_v, query) =>
      [NOTE, OTHER].filter((note) => note.title.toLowerCase().includes(query.toLowerCase())),
    );

    render(<AttachToNoteDialog vaultId="v1" sources={[ABSOLUTE]} onClose={vi.fn()} />);
    fireEvent.change(screen.getByLabelText(ATTACH_SEARCH_LABEL), { target: { value: "holiday" } });

    expect(
      await screen.findByRole("button", { name: `${ATTACH_ACTION_LABEL} to ${OTHER.title}` }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: `${ATTACH_ACTION_LABEL} to ${NOTE.title}` }),
    ).toBeNull();
  });

  /**
   * The other half of "refuse to write the same attachment twice": a note that
   * already has the file is not offered as somewhere to put it. It stays on
   * screen saying so, because a note that vanished from a search reads as
   * "keeper cannot find my note" — a different and more alarming fact.
   */
  it("does not offer a note that already holds the attachment, and says why", async () => {
    notesAttachTargets.mockResolvedValue([{ ...NOTE, holds: ["screen-0000.mov"] }, OTHER]);

    render(<AttachToNoteDialog vaultId="v1" sources={[ABSOLUTE]} onClose={vi.fn()} />);

    expect(
      await screen.findByRole("button", { name: `${ATTACH_ACTION_LABEL} to ${OTHER.title}` }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: `${ATTACH_ACTION_LABEL} to ${NOTE.title}` }),
    ).toBeNull();
    expect(screen.getByText(`${ATTACH_HOLDS_PREFIX} screen-0000.mov`)).toBeInTheDocument();
  });
});

/**
 * The picker's three answers, each named rather than one being the remainder.
 *
 * W2Media's finding this wave: a mutation sweep only covers lines you already
 * thought about, so it cannot find a branch you never wrote because a ternary
 * wrote it for you. This file's picker used to say
 * `picked === null ? [] : Array.isArray(picked) ? picked : [picked]`, whose
 * last arm is unreachable under the plugin's own declared type and was
 * explained by a shape that type excludes. The cases are named now, and these
 * are the tests for the two that write nothing.
 */
describe("the file picker's answers", () => {
  it("says nothing at all when the dialog is cancelled", async () => {
    pickFiles.mockResolvedValue(null);
    await mountEditor();

    fireEvent.click(screen.getByRole("button", { name: ATTACH_FILE_LABEL }));
    await waitFor(() => {
      expect(pickFiles).toHaveBeenCalled();
    });

    // A cancel is not an outcome: no note text, and no banner claiming one.
    expect(notesEditorStore.getState().text).toBe(OPENED);
    expect(notesAttachSources).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).toBeNull();
  });

  /**
   * The case the type says cannot happen, which is exactly why it needs a
   * sentence rather than a fall-through: handing a bare string to
   * `notesAttachSources` would send Rust a shape it cannot read, and the person
   * would watch the picker close and nothing happen.
   */
  it("says so when the picker breaks its own contract, rather than attaching nothing in silence", async () => {
    pickFiles.mockResolvedValue(ABSOLUTE as unknown as string[]);
    await mountEditor();

    fireEvent.click(screen.getByRole("button", { name: ATTACH_FILE_LABEL }));

    expect(await screen.findByRole("status")).toHaveTextContent(
      "keeper could not read what the file picker returned",
    );
    expect(notesAttachSources).not.toHaveBeenCalled();
    expect(notesEditorStore.getState().text).toBe(OPENED);
  });
});
describe("what the picker hands Rust", () => {
  /**
   * The seam nothing else covers, and the mocks are why.
   *
   * `notesAttachSources.mockResolvedValue(...)` ignores its arguments, so every
   * test above passes on the RESULT while saying nothing about the CALL. Two
   * mutations proved it: sending `[]` instead of the picked paths, and sending
   * a different vault id, both survived the whole suite. Neither is cosmetic —
   * an empty selection makes the picker silently do nothing, which is this
   * story's headline failure at the entry point in its own title, and the wrong
   * vault copies the file somewhere the note cannot resolve it, so the embed
   * renders as "not found" on a file the person just watched keeper accept.
   *
   * Two paths rather than one, because a mutation that keeps only the first
   * would otherwise pass on a single-file fixture.
   */
  it("sends the vault and every picked path, not a subset", async () => {
    const second = "/Users/alice/Desktop/holiday.png";
    pickFiles.mockResolvedValue([ABSOLUTE, second]);
    notesAttachSources.mockResolvedValue([
      { name: "screen-0000.mov", relPath: RELATIVE, copied: false, refusal: null },
      { name: "holiday.png", relPath: "attachments/holiday.png", copied: true, refusal: null },
    ]);
    await mountEditor();

    fireEvent.click(screen.getByRole("button", { name: ATTACH_FILE_LABEL }));

    await waitFor(() => {
      expect(notesAttachSources).toHaveBeenCalledWith("v1", [ABSOLUTE, second]);
    });
    // And the note gets both, in the order picked.
    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe(
        `${OPENED}![[${RELATIVE}]]\n![[attachments/holiday.png]]`,
      );
    });
  });
});

/**
 * The duplicate rule, at the entry point that had no test for it.
 *
 * Every duplicate test in this file drives the CHOOSER, which reads the note's
 * body from disk. The picker reads the live buffer instead, through a `body`
 * prop `NoteEditor` hands it — and a prop is a boundary like any other. Two
 * mutations proved nothing was checking it: `body=""` and `body={base}` both
 * survived the entire suite.
 *
 * Neither is cosmetic. `body=""` blinds the duplicate check completely, so the
 * picker writes a second embed of a file the note already shows — the exact
 * thing this story's title says it refuses, at one of its three entry points.
 * `body={base}` is the subtler one and it is what the prop's own doc comment
 * claims to prevent: `base` is what Rust last acknowledged, so a file attached
 * and attached again before the autosave fires would slip through.
 */
describe("the picker's duplicate rule", () => {
  it("refuses a file the note already holds, and says so", async () => {
    await mountEditor();

    // First press writes it.
    fireEvent.click(screen.getByRole("button", { name: ATTACH_FILE_LABEL }));
    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe(`${OPENED}![[${RELATIVE}]]`);
    });

    // Second press, same file, against the buffer the first one just changed —
    // no save has happened in between, which is the point.
    fireEvent.click(screen.getByRole("button", { name: ATTACH_FILE_LABEL }));

    expect(await screen.findByRole("status")).toHaveTextContent(
      "screen-0000.mov is already in this note, so keeper left it out.",
    );
    // Written once, not twice.
    expect(notesEditorStore.getState().text).toBe(`${OPENED}![[${RELATIVE}]]`);
  });
});

/**
 * What the picker SAYS, which nothing tested.
 *
 * Found by Main's count-your-tests-per-entry-point shape rather than by any
 * mutation of a line: this story has three doors, and every test of the
 * outcome sentence went through the chooser. The picker composes its own
 * clauses from its own code, and all three of these mutations survived —
 * never saying a file was copied, dropping a source with neither path nor
 * reason, and never passing on Rust's refusal at all.
 *
 * All three are the same failure wearing three hats: **the picker says
 * nothing**, at the entry point in this story's title, about something that
 * happened to the person's disk.
 */
describe("what the picker says", () => {
  it("says a file from outside the vault was copied in", async () => {
    const outside = "/Users/alice/Desktop/holiday.png";
    pickFiles.mockResolvedValue([outside]);
    notesAttachSources.mockResolvedValue([
      { name: "holiday.png", relPath: "attachments/holiday.png", copied: true, refusal: null },
    ]);
    await mountEditor();

    fireEvent.click(screen.getByRole("button", { name: ATTACH_FILE_LABEL }));

    // A copy is a change to their disk and gets a receipt, not silence.
    expect(await screen.findByRole("status")).toHaveTextContent(
      "outside the vault, so keeper copied it into attachments/",
    );
    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe(`${OPENED}![[attachments/holiday.png]]`);
    });
  });

  it("passes on Rust's refusal rather than swallowing it", async () => {
    pickFiles.mockResolvedValue(["/Users/alice/Pictures/Trip"]);
    notesAttachSources.mockResolvedValue([
      {
        name: "Trip",
        relPath: null,
        copied: false,
        refusal:
          "Trip is a folder. A note can embed a file, but there is nothing to show for a directory.",
      },
    ]);
    await mountEditor();

    fireEvent.click(screen.getByRole("button", { name: ATTACH_FILE_LABEL }));

    expect(await screen.findByRole("status")).toHaveTextContent("Trip is a folder.");
    expect(notesEditorStore.getState().text).toBe(OPENED);
  });

  /** The same invariant the chooser has: partitioned on "produced no path", so
   *  a source with neither cannot be dropped in silence. */
  it("says something about a source that came back with neither a path nor a reason", async () => {
    pickFiles.mockResolvedValue(["/Users/alice/mystery.bin"]);
    notesAttachSources.mockResolvedValue([
      { name: "mystery.bin", relPath: null, copied: false, refusal: null },
    ]);
    await mountEditor();

    fireEvent.click(screen.getByRole("button", { name: ATTACH_FILE_LABEL }));

    expect(await screen.findByRole("status")).toHaveTextContent("mystery.bin");
    expect(notesEditorStore.getState().text).toBe(OPENED);
  });
});
