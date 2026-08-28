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
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  NoteAttachSourceVm,
  NoteAttachTargetVm,
  NoteBodyBatch,
  NoteBodyVm,
  NoteGalleryVm,
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
/** Story 46.11: the Attachments panel asks which of the note's embeds the vault
 *  holds, and the in-vault chooser lists a vault folder. Both are reached by
 *  mounting the panel or opening the chooser, which is why 45.13's mock factory
 *  had neither. */
const notesEmbedPaths = vi.fn<(v: string, targets: string[]) => Promise<(string | null)[]>>();
const notesGallery = vi.fn<(v: string, folder: string, scope?: string) => Promise<NoteGalleryVm>>();

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
  notesEmbedPaths: (v: string, targets: string[]) => notesEmbedPaths(v, targets),
  notesGallery: (v: string, folder: string, scope?: string) => notesGallery(v, folder, scope),
}));

import { readNoteDocument, resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import {
  ATTACH_FILE_LABEL,
  ATTACH_FROM_COMPUTER_HINT,
  ATTACH_FROM_COMPUTER_LABEL,
  ATTACH_FROM_VAULT_HINT,
} from "./attach-file-button";
import {
  ATTACH_FROM_VAULT_CAPPED,
  ATTACH_FROM_VAULT_FILTER_LABEL,
  ATTACH_FROM_VAULT_LABEL,
  ATTACH_FROM_VAULT_LIST_TESTID,
  ATTACH_FROM_VAULT_OUTCOME_TESTID,
  ATTACH_FROM_VAULT_PROMISE,
  ATTACH_FROM_VAULT_ROW_CAP,
  ATTACH_FROM_VAULT_TRUNCATED,
  ATTACH_FROM_VAULT_UP_LABEL,
  REASON_OUTSIDE_VAULT,
  SYNCED_ROOT_LABEL,
} from "./attach-from-vault-dialog";
import {
  ATTACH_ACTION_LABEL,
  ATTACH_HOLDS_PREFIX,
  ATTACH_OUTCOME_TESTID,
  ATTACH_SEARCH_LABEL,
  AttachToNoteDialog,
} from "./attach-to-note-dialog";
import { ATTACHMENTS_LABEL } from "./attachments-panel";
import { NOTE_ACTIONS_LABEL } from "./note-actions";
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

/**
 * The synced folder the in-vault chooser browses, one folder per key (Story
 * 46.11; widened for item 10).
 *
 * A real walk down to the same file the other three entry points attach, rather
 * than a root listing that implausibly holds a nested path: the chooser hands
 * `notes_gallery` the folder it is looking at, and a fixture that skipped the
 * navigation would not exercise the one thing the dialog does with a folder row.
 *
 * This profile's notes subfolder is empty — the vault IS the synced folder — so
 * every entry's `vaultRelPath` equals its `relPath`, which is what Rust promises
 * for that configuration. {@link SYNCED} is the other one, where the two frames
 * differ and files exist above the vault root.
 */
const VAULT: Record<string, NoteGalleryVm["items"]> = {
  "": [
    {
      name: "recordings",
      relPath: "recordings",
      vaultRelPath: "recordings",
      kind: "folder",
      url: null,
    },
  ],
  recordings: [
    {
      name: "2026",
      relPath: "recordings/2026",
      vaultRelPath: "recordings/2026",
      kind: "folder",
      url: null,
    },
  ],
  "recordings/2026": [
    {
      name: "standup",
      relPath: "recordings/2026/standup",
      vaultRelPath: "recordings/2026/standup",
      kind: "folder",
      url: null,
    },
  ],
  "recordings/2026/standup": [
    {
      name: "screen-0000.mov",
      relPath: "recordings/2026/standup/screen-0000.mov",
      vaultRelPath: "recordings/2026/standup/screen-0000.mov",
      kind: "video",
      url: "keeper-note://v1/recordings/2026/standup/screen-0000.mov",
    },
  ],
};

/**
 * The other configuration, and the one item 10 is about: a profile synced at
 * `~/tgdrive` whose notes subfolder is `notes`, so the synced folder root holds
 * the vault **and** two siblings the vault has never seen.
 *
 * Every path here is synced-folder relative, which is what `notes_gallery`
 * answers with under `"syncedFolder"`. `vaultRelPath` is Rust's second frame: the
 * same file's vault-relative path, or `null` for a file above the vault root.
 * Both come from Rust so the webview does no path arithmetic (AD-65) — note that
 * `notes/attachments/holiday.png` and `photos/holiday.png` share a file name and
 * differ only in that frame, which is exactly the confusion the second field
 * exists to prevent.
 */
const SYNCED: Record<string, NoteGalleryVm["items"]> = {
  "": [
    { name: "notes", relPath: "notes", vaultRelPath: "", kind: "folder", url: null },
    { name: "photos", relPath: "photos", vaultRelPath: null, kind: "folder", url: null },
    {
      name: "tax-return.pdf",
      relPath: "tax-return.pdf",
      vaultRelPath: null,
      kind: "file",
      url: null,
    },
  ],
  notes: [
    {
      name: "attachments",
      relPath: "notes/attachments",
      vaultRelPath: "attachments",
      kind: "folder",
      url: null,
    },
  ],
  "notes/attachments": [
    {
      name: "holiday.png",
      relPath: "notes/attachments/holiday.png",
      vaultRelPath: "attachments/holiday.png",
      kind: "image",
      url: "keeper-note://v1/attachments/holiday.png",
    },
  ],
  photos: [
    {
      name: "holiday.png",
      relPath: "photos/holiday.png",
      // Above the vault root. `keeper-note://` resolves against the vault root
      // and refuses a climbing path, so no embed can ever name this file — which
      // is why Rust sends no URL for it either.
      vaultRelPath: null,
      kind: "image",
      url: null,
    },
  ],
};

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
  // The vault the chooser walks, and the vault the panel asks about (Story
  // 46.11). A folder nothing put in `VAULT` is a folder that is not there, and
  // `notes_gallery` answers that with a sentence rather than a rejection.
  notesGallery.mockImplementation(async (_vault, folder) => ({
    folder,
    items: VAULT[folder] ?? [],
    truncated: false,
    problem: folder in VAULT ? null : "this folder is not in the vault",
  }));
  notesEmbedPaths.mockImplementation(async (_vault, targets) => targets);
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
  resetNotesEditorStoreForTest();
});

/** Mount the real editor and wait for its lazy chunk and its first document. */
async function mountEditor(): Promise<void> {
  render(<NoteEditor vaultId="v1" noteId="n1" />);
  await waitFor(() => {
    expect(document.querySelector(".cm-content")).not.toBeNull();
    expect(readNoteDocument("v1", "n1").text).toBe(OPENED);
  });
}

/** Open the note's Actions menu the way Radix's trigger listens for. */
async function openNoteActions(): Promise<HTMLElement> {
  const trigger = await screen.findByRole("button", {
    name: new RegExp(`^${NOTE_ACTIONS_LABEL}`),
  });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.pointerUp(trigger, { button: 0 });
  return await screen.findByRole("menu");
}

/** Entry point one: the attachments panel Story 43.7 built. */
async function throughThePanel(): Promise<string> {
  await mountEditor();
  // Story 46.5: the panel's own control is a menu item now — the 560px capture
  // window could not hold six header buttons — and a `menuitemcheckbox` since
  // Story 49, because it discloses a panel and now says whether it is open.
  // By role and not a bare name query, because `ATTACHMENTS_LABEL` is also the
  // panel section's accessible name and a name query would happily resolve to
  // the thing being opened.
  const menu = await openNoteActions();
  fireEvent.click(within(menu).getByRole("menuitemcheckbox", { name: ATTACHMENTS_LABEL }));
  const insert = await screen.findByRole("button", { name: `Insert ${RELATIVE}` });
  // A real button takes focus on the way down, off the editor, which fires the
  // blur save — the moment the caret could be lost.
  insert.focus();
  fireEvent.click(insert);
  await waitFor(() => {
    expect(readNoteDocument("v1", "n1").text).toContain("![[");
  });
  return readNoteDocument("v1", "n1").text;
}

/**
 * Open the attach control's own menu (Story 46.11).
 *
 * "Attach a file" is a dropdown trigger since it acquired a second source. The
 * trigger keeps its name and the header keeps its two controls (AD-104), so what
 * changed for a test is one interaction, not a query.
 */
async function openAttachMenu(): Promise<HTMLElement> {
  const trigger = screen.getByRole("button", { name: ATTACH_FILE_LABEL });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.pointerUp(trigger, { button: 0 });
  return await screen.findByRole("menu");
}

/** Reach the OS picker, which is one of the menu's two items now. */
async function pickFromComputer(): Promise<void> {
  const menu = await openAttachMenu();
  fireEvent.click(within(menu).getByRole("menuitem", { name: ATTACH_FROM_COMPUTER_LABEL }));
}

/** Entry point two: a file picked off the drive, into the open note. */
async function throughThePicker(): Promise<string> {
  await mountEditor();
  await pickFromComputer();
  await waitFor(() => {
    expect(readNoteDocument("v1", "n1").text).toContain("![[");
  });
  return readNoteDocument("v1", "n1").text;
}

/**
 * Entry point four: a file the vault already holds, browsed to from the note
 * (Story 46.11).
 *
 * Walks the fixture vault down to the same file the other three attach, so the
 * parity assertion covers a door that never calls `notes_attach_sources` at all.
 */
async function throughTheVaultChooser(): Promise<string> {
  await mountEditor();
  const menu = await openAttachMenu();
  fireEvent.click(within(menu).getByRole("menuitem", { name: ATTACH_FROM_VAULT_LABEL }));
  for (const folder of ["recordings", "2026", "standup"]) {
    fireEvent.click(await screen.findByRole("button", { name: `${folder}/` }));
  }
  fireEvent.click(await screen.findByRole("button", { name: `Attach ${RELATIVE}` }));
  await waitFor(() => {
    expect(readNoteDocument("v1", "n1").text).toContain("![[");
  });
  // Closed the way a person closes it, and not left open: this dialog renders in
  // a portal under `document.body`, and the parity test blanks `innerHTML`
  // between doors — which tears a live portal's nodes out from under React and
  // makes the unmount throw `NotFoundError` rather than failing an assertion.
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  await waitFor(() => {
    expect(screen.queryByText(ATTACH_FROM_VAULT_PROMISE)).toBeNull();
  });
  return readNoteDocument("v1", "n1").text;
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
   * changing all of them at once — which is what having one spelling means.
   *
   * Four since Story 46.11, and the fourth is the interesting one: it is the
   * only door that resolves nothing in Rust, because the path it starts from is
   * already the path a note names. Same bytes anyway, because all four go
   * through `planAttachments`.
   */
  it("puts the same file into the note as the same bytes from all four entry points", async () => {
    const fromPanel = await throughThePanel();
    document.body.innerHTML = "";
    resetNotesEditorStoreForTest();

    const fromPicker = await throughThePicker();
    document.body.innerHTML = "";
    resetNotesEditorStoreForTest();

    const fromVault = await throughTheVaultChooser();
    document.body.innerHTML = "";
    resetNotesEditorStoreForTest();

    const fromChooser = await throughTheChooser();

    expect(fromPanel).toBe(fromPicker);
    expect(fromPicker).toBe(fromVault);
    expect(fromVault).toBe(fromChooser);
    // And it is the note plus one embed, not the note plus something that
    // merely matched between four broken implementations.
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

    await pickFromComputer();
    await waitFor(() => {
      expect(pickFiles).toHaveBeenCalled();
    });

    // A cancel is not an outcome: no note text, and no banner claiming one.
    expect(readNoteDocument("v1", "n1").text).toBe(OPENED);
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

    await pickFromComputer();

    expect(await screen.findByRole("status")).toHaveTextContent(
      "keeper could not read what the file picker returned",
    );
    expect(notesAttachSources).not.toHaveBeenCalled();
    expect(readNoteDocument("v1", "n1").text).toBe(OPENED);
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

    await pickFromComputer();

    await waitFor(() => {
      expect(notesAttachSources).toHaveBeenCalledWith("v1", [ABSOLUTE, second]);
    });
    // And the note gets both, in the order picked.
    await waitFor(() => {
      expect(readNoteDocument("v1", "n1").text).toBe(
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
    await pickFromComputer();
    await waitFor(() => {
      expect(readNoteDocument("v1", "n1").text).toBe(`${OPENED}![[${RELATIVE}]]`);
    });

    // Second press, same file, against the buffer the first one just changed —
    // no save has happened in between, which is the point.
    await pickFromComputer();

    expect(await screen.findByRole("status")).toHaveTextContent(
      "screen-0000.mov is already in this note, so keeper left it out.",
    );
    // Written once, not twice.
    expect(readNoteDocument("v1", "n1").text).toBe(`${OPENED}![[${RELATIVE}]]`);
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

    await pickFromComputer();

    // A copy is a change to their disk and gets a receipt, not silence.
    expect(await screen.findByRole("status")).toHaveTextContent(
      "outside the vault, so keeper copied it into attachments/",
    );
    await waitFor(() => {
      expect(readNoteDocument("v1", "n1").text).toBe(`${OPENED}![[attachments/holiday.png]]`);
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

    await pickFromComputer();

    expect(await screen.findByRole("status")).toHaveTextContent("Trip is a folder.");
    expect(readNoteDocument("v1", "n1").text).toBe(OPENED);
  });

  /** The same invariant the chooser has: partitioned on "produced no path", so
   *  a source with neither cannot be dropped in silence. */
  it("says something about a source that came back with neither a path nor a reason", async () => {
    pickFiles.mockResolvedValue(["/Users/alice/mystery.bin"]);
    notesAttachSources.mockResolvedValue([
      { name: "mystery.bin", relPath: null, copied: false, refusal: null },
    ]);
    await mountEditor();

    await pickFromComputer();

    expect(await screen.findByRole("status")).toHaveTextContent("mystery.bin");
    expect(readNoteDocument("v1", "n1").text).toBe(OPENED);
  });
});

/**
 * The two doors, and the promise each one makes (Story 46.11).
 *
 * The owner's ask was "offer attach from a SYNC FOLDER in the dropdown too, not
 * only from outside", and the ruling epic 46's spine made for it is that the
 * in-vault door must **not** copy. So the load-bearing assertions here are about
 * what does NOT happen: `notes_attach_sources` is the only thing in this app
 * that copies a file into a vault, and this door never calls it.
 */
describe("the two doors on the attach control", () => {
  it("offers both sources from one control, so the header keeps its two", async () => {
    await mountEditor();
    const menu = await openAttachMenu();

    // Two items, the OS picker first because that is what this control has
    // always done.
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.getAttribute("aria-label")),
    ).toEqual([ATTACH_FROM_COMPUTER_LABEL, ATTACH_FROM_VAULT_LABEL]);
    // And each says what it will do to the disk BEFORE it is opened, which is
    // the whole reason they are two items with two hints rather than one verb
    // with a folder chooser.
    expect(within(menu).getByText(ATTACH_FROM_COMPUTER_HINT)).toBeInTheDocument();
    expect(within(menu).getByText(ATTACH_FROM_VAULT_HINT)).toBeInTheDocument();
  });

  /**
   * The ruling, asserted as an absence.
   *
   * The vault holds the file and the sync engine already carries it, so a copy
   * would duplicate bytes that are on every machine this vault reaches and leave
   * two files to drift. The note points at the file where it lives.
   */
  it("inserts a reference to the file where it already lives, and copies nothing", async () => {
    const written = await throughTheVaultChooser();

    expect(written).toBe(`${OPENED}![[${RELATIVE}]]`);
    // The one copier in this app, never called — not called with a different
    // argument, not called at all. This door has no absolute path to give it.
    expect(notesAttachSources).not.toHaveBeenCalled();
    // Nor did anything write to the vault: the only write is the note itself,
    // through the editor's own save path.
    expect(notesBodyWrite).not.toHaveBeenCalled();
    // And nothing on screen or in the note is an absolute path (FR-145): every
    // path this door ever held came from `notes_gallery` vault-relative.
    expect(written).not.toContain("/Users/");
  });

  it("says which folder it is showing, and never where that folder is", async () => {
    await mountEditor();
    const menu = await openAttachMenu();
    fireEvent.click(within(menu).getByRole("menuitem", { name: ATTACH_FROM_VAULT_LABEL }));

    // The promise, before anything is chosen.
    expect(await screen.findByText(ATTACH_FROM_VAULT_PROMISE)).toBeInTheDocument();
    // A phrase for the root, not a path — the webview is never told where the
    // synced folder is (AD-65) and must not put one on screen (FR-145).
    expect(screen.getByText(SYNCED_ROOT_LABEL)).toBeInTheDocument();
    // No way up from the root, because there is nothing above it in this frame.
    expect(screen.queryByRole("button", { name: ATTACH_FROM_VAULT_UP_LABEL })).toBeNull();

    fireEvent.click(await screen.findByRole("button", { name: "recordings/" }));

    expect(await screen.findByText(`${SYNCED_ROOT_LABEL} / recordings`)).toBeInTheDocument();
    // The scope, said out loud on every call (item 10). Without the third
    // argument this door lists the notes subfolder and the whole widening is
    // gone, so it is asserted here and not left implicit.
    expect(notesGallery).toHaveBeenLastCalledWith("v1", "recordings", "syncedFolder");

    // And back out again, to the folder above and not to the root.
    fireEvent.click(await screen.findByRole("button", { name: "2026/" }));
    await screen.findByText(`${SYNCED_ROOT_LABEL} / recordings / 2026`);
    fireEvent.click(screen.getByRole("button", { name: ATTACH_FROM_VAULT_UP_LABEL }));
    expect(await screen.findByText(`${SYNCED_ROOT_LABEL} / recordings`)).toBeInTheDocument();
  });

  /**
   * A vault folder is mostly notes, so "why can I not attach this" is the first
   * question this surface would raise if it stayed silent. The reason goes where
   * the button would have been — the shape the attachments panel and the note
   * chooser already use for a row they will not offer.
   */
  it("declines to offer what an embed cannot name, and says why instead of refusing after the click", async () => {
    notesGallery.mockResolvedValue({
      folder: "",
      items: [
        {
          name: "holiday.png",
          relPath: "holiday.png",
          vaultRelPath: "holiday.png",
          kind: "image",
          url: "keeper-note://x",
        },
        {
          name: "daily.md",
          relPath: "daily.md",
          vaultRelPath: "daily.md",
          kind: "file",
          url: null,
        },
        {
          name: "why#not.png",
          relPath: "why#not.png",
          vaultRelPath: "why#not.png",
          kind: "image",
          url: null,
        },
      ],
      truncated: false,
      problem: null,
    });
    await mountEditor();
    const menu = await openAttachMenu();
    fireEvent.click(within(menu).getByRole("menuitem", { name: ATTACH_FROM_VAULT_LABEL }));

    // The one that can be attached is the only one with a button.
    expect(await screen.findByRole("button", { name: "Attach holiday.png" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Attach daily.md" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Attach why#not.png" })).toBeNull();
    // Each with its own reason, because they are two different facts: one is a
    // transclusion by `export::names_a_note`, the other is a name no wikilink
    // can spell.
    expect(screen.getByText("a note, not a file")).toBeInTheDocument();
    expect(screen.getByText("keeper cannot name this in a note")).toBeInTheDocument();
  });

  it("will not attach the same file twice, and says so where the person is looking", async () => {
    await mountEditor();
    const menu = await openAttachMenu();
    fireEvent.click(within(menu).getByRole("menuitem", { name: ATTACH_FROM_VAULT_LABEL }));
    for (const folder of ["recordings", "2026", "standup"]) {
      fireEvent.click(await screen.findByRole("button", { name: `${folder}/` }));
    }

    fireEvent.click(await screen.findByRole("button", { name: `Attach ${RELATIVE}` }));
    await waitFor(() => {
      expect(readNoteDocument("v1", "n1").text).toBe(`${OPENED}![[${RELATIVE}]]`);
    });

    // The receipt is in the dialog, because the dialog is covering the editor's
    // banner and the person is looking at the dialog.
    expect(screen.getByTestId(ATTACH_FROM_VAULT_OUTCOME_TESTID)).toHaveTextContent("no copy");
    // And there is no second press to make: the row wears the panel's own word
    // for the fact, so duplication is unavailable rather than guarded against.
    expect(screen.queryByRole("button", { name: `Attach ${RELATIVE}` })).toBeNull();
    expect(screen.getByText("In the note")).toBeInTheDocument();
  });

  it("renders Rust's sentence for a folder it cannot list, rather than calling it empty", async () => {
    // A missing folder, an unreadable one and a path that escapes the vault all
    // come back the same way from `notes_gallery`: a normal reply carrying a
    // finished sentence, because a surface has to show something and a rejected
    // promise gives it nothing to show. Which of the three it was is Rust's to
    // know. "This folder is empty" would be a different fact, and untrue.
    notesGallery.mockResolvedValue({
      folder: "",
      items: [],
      truncated: false,
      problem: "this folder is not in the vault, so there is nothing to show",
    });
    await mountEditor();
    const menu = await openAttachMenu();
    fireEvent.click(within(menu).getByRole("menuitem", { name: ATTACH_FROM_VAULT_LABEL }));

    expect(
      await screen.findByText(/not in the vault, so there is nothing to show/),
    ).toBeInTheDocument();
    expect(screen.queryByText("This folder is empty.")).toBeNull();
  });
});

/**
 * Item 10: *"when you attach a file from a folder, offer the WHOLE folder — not
 * only the notes part."*
 *
 * The door used to list the vault root, which for a profile with a notes
 * subfolder is a strict subset of the folder the person syncs. It now lists the
 * synced folder, so these are the two facts that were not true before and the
 * one that must not stop being true:
 *
 * - a file beside the vault is REACHED, so the listing really is the whole
 *   folder;
 * - it is not OFFERED, because `keeper-note://` resolves against the vault root
 *   and refuses a climbing path, so its embed would render nothing anywhere;
 * - the path that lands in the note is the vault's, never the one the listing
 *   was keyed on — the two differ for every file under the subfolder, and no
 *   arithmetic in the webview turns one into the other (AD-65).
 */
describe("the whole synced folder, not only the notes part", () => {
  /** Point the chooser at {@link SYNCED} instead of the default {@link VAULT}. */
  function browsingTheSyncedFolder(): void {
    notesGallery.mockImplementation(async (_vault, folder) => ({
      folder,
      items: SYNCED[folder] ?? [],
      truncated: false,
      // Rust's sentence is scope-aware now: "not in the vault" would be a false
      // claim about a folder that is not in the SYNCED FOLDER.
      problem: folder in SYNCED ? null : "that folder is not in the folder you sync",
    }));
  }

  /** Mount the editor and open the in-vault door, which is all four of these
   *  tests do before they diverge. */
  async function openTheChooser(): Promise<void> {
    await mountEditor();
    const menu = await openAttachMenu();
    fireEvent.click(within(menu).getByRole("menuitem", { name: ATTACH_FROM_VAULT_LABEL }));
    await screen.findByText(ATTACH_FROM_VAULT_PROMISE);
  }

  it("reaches a file that lives beside the vault, and says why it cannot be attached", async () => {
    browsingTheSyncedFolder();
    await openTheChooser();

    // The vault's own folder is now one row among the synced folder's, which is
    // the widening: before this, `photos/` was not on screen at all. Both are
    // browsable — a folder is opened, never embedded, so being above the vault
    // root costs it nothing.
    expect(await screen.findByRole("button", { name: "notes/" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "photos/" })).toBeInTheDocument();

    // A file at the synced root, above the vault: shown, and shown with the
    // reason where its button would be. Hiding it would answer "offer the whole
    // folder" with a listing that is once again not the whole folder.
    expect(screen.getByTitle("tax-return.pdf")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Attach tax-return.pdf" })).toBeNull();
    // Exactly one, and on the file: the reason belongs to the row that would
    // otherwise carry a button, and `photos/` must not wear it.
    expect(screen.getAllByText(REASON_OUTSIDE_VAULT)).toHaveLength(1);

    // The copier, still never called — the invariant `attach-file-button.tsx`
    // documents. Widening the listing widened what this door can SHOW, and must
    // not have quietly given it a way to copy the files it cannot embed.
    expect(notesAttachSources).not.toHaveBeenCalled();
    expect(readNoteDocument("v1", "n1").text).toBe(OPENED);
  });

  /**
   * The name collision, which is the whole reason Rust sends two paths.
   *
   * `photos/holiday.png` and `notes/attachments/holiday.png` have the same file
   * name and the same listing-frame shape. Only the second one has a
   * vault-relative path, and a surface that guessed by stripping a prefix would
   * offer both and embed the wrong bytes for one of them.
   */
  it("offers the one of two same-named files that the vault can actually name", async () => {
    browsingTheSyncedFolder();
    await openTheChooser();

    fireEvent.click(await screen.findByRole("button", { name: "photos/" }));
    // Reached, listed, and refused: this is the file above the vault root.
    expect(await screen.findByTitle("photos/holiday.png")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Attach / })).toBeNull();
    expect(screen.getByText(REASON_OUTSIDE_VAULT)).toBeInTheDocument();

    // The same name, under the vault this time.
    fireEvent.click(screen.getByRole("button", { name: ATTACH_FROM_VAULT_UP_LABEL }));
    fireEvent.click(await screen.findByRole("button", { name: "notes/" }));
    fireEvent.click(await screen.findByRole("button", { name: "attachments/" }));

    // Named by the path the NOTE will hold, not by the path the listing was
    // keyed on: `attachments/holiday.png` and not `notes/attachments/holiday.png`.
    const attach = await screen.findByRole("button", { name: "Attach attachments/holiday.png" });
    expect(screen.getByTitle("notes/attachments/holiday.png")).toBeInTheDocument();

    fireEvent.click(attach);
    await waitFor(() => {
      expect(readNoteDocument("v1", "n1").text).toBe(`${OPENED}![[attachments/holiday.png]]`);
    });
    // The synced-folder frame must never reach the note: `![[notes/…]]` would be
    // a path `keeper-note://` resolves under the vault root and does not find.
    expect(readNoteDocument("v1", "n1").text).not.toContain("notes/attachments");
    expect(notesAttachSources).not.toHaveBeenCalled();
  });

  /**
   * The synced folder is not vault-sized — the owner's holds 155,662 files — so
   * `browse`'s thousand-entry cap is the listing this dialog actually gets. A
   * thousand mounted rows in a scroller eight rows tall is the defect widening
   * the scope would otherwise have shipped.
   */
  it("mounts a bounded number of rows for a huge folder, and keeps the rest reachable", async () => {
    const many = Array.from({ length: 1000 }, (_unused, index) => {
      const name = `f${String(index).padStart(4, "0")}.png`;
      return { name, relPath: name, vaultRelPath: name, kind: "image" as const, url: null };
    });
    notesGallery.mockResolvedValue({
      folder: "",
      items: many,
      // Rust cut the folder short as well, which is a different fact from the
      // dialog's own cap and gets its own sentence.
      truncated: true,
      problem: null,
    });
    await openTheChooser();

    const list = await screen.findByTestId(ATTACH_FROM_VAULT_LIST_TESTID);
    await waitFor(() => {
      expect(within(list).getAllByRole("listitem")).toHaveLength(ATTACH_FROM_VAULT_ROW_CAP);
    });
    // Both limits said, separately: one is Rust's and cannot be undone here, the
    // other is this dialog's and typing gets past it.
    expect(screen.getByText(ATTACH_FROM_VAULT_TRUNCATED)).toBeInTheDocument();
    expect(screen.getByText(ATTACH_FROM_VAULT_CAPPED)).toBeInTheDocument();

    // The last entry is far beyond the cap, so it is not mounted…
    expect(screen.queryByRole("button", { name: "Attach f0999.png" })).toBeNull();
    // …and the filter is what reaches it. A cap without this would make 800
    // files unreachable rather than merely unmounted.
    fireEvent.change(screen.getByLabelText(ATTACH_FROM_VAULT_FILTER_LABEL), {
      target: { value: "f0999" },
    });

    expect(await screen.findByRole("button", { name: "Attach f0999.png" })).toBeInTheDocument();
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    // Narrowed to one row, so the dialog's own cap is no longer in force — while
    // Rust's cut still is, and still says so.
    expect(screen.queryByText(ATTACH_FROM_VAULT_CAPPED)).toBeNull();
    expect(screen.getByText(ATTACH_FROM_VAULT_TRUNCATED)).toBeInTheDocument();
  });

  it("clears the filter when the folder changes, so the next folder is not silently narrowed", async () => {
    browsingTheSyncedFolder();
    await openTheChooser();

    const filter = await screen.findByLabelText(ATTACH_FROM_VAULT_FILTER_LABEL);
    fireEvent.change(filter, { target: { value: "photos" } });
    // Only the folder whose name matches survives the needle.
    expect(screen.queryByRole("button", { name: "notes/" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "photos/" }));

    // `photos/` holds no file called "photos", so a needle carried across would
    // render an empty folder and be read as one.
    expect(await screen.findByTitle("photos/holiday.png")).toBeInTheDocument();
    expect(screen.getByLabelText(ATTACH_FROM_VAULT_FILTER_LABEL)).toHaveValue("");
  });
});
