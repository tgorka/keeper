/**
 * The registry's `text` viewer, mounted the way a panel mounts it
 * (Story 45.4, AD-87, AD-88).
 *
 * **Every test here goes through `viewerComponentFor`.** Importing
 * `TextFileViewer` directly would prove the component works and prove nothing
 * about the binding — and "declared and never mounted" is DW-172, which shipped
 * green in epic 44 because `renderHook` mounts the hook itself and can never
 * see that `App` does not. A viewer bound in a table nobody exercises is the
 * same defect wearing a different hat.
 *
 * The IPC surface is mocked because these are the states a real vault produces
 * on demand and cannot produce on request; everything below the IPC line —
 * 45.6's loading hook, 45.6's real CodeMirror, the toggle, the structure view —
 * is the real thing.
 */
import {
  acceptCompletion,
  completionStatus,
  currentCompletions,
  startCompletion,
} from "@codemirror/autocomplete";
import { EditorView } from "@codemirror/view";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  NoteCsvVm,
  NoteFolderVm,
  NoteRefVm,
  NoteRowVm,
  PanelTargetVm,
  TextFileVm,
} from "@/lib/ipc/client";
import { withRangeRects } from "@/test/layout";

const syncReadText = vi.fn<(profileId: string, subpath: string) => Promise<TextFileVm>>();
const syncWriteEntry = vi.fn<(profileId: string, subpath: string, text: string) => Promise<void>>();
const notesCsvRead = vi.fn<(vaultId: string, target: string) => Promise<NoteCsvVm>>();
const notesTree = vi.fn<(vaultId: string, relDir: string) => Promise<NoteFolderVm>>();
const notesResolveLink = vi.fn<(vaultId: string, target: string) => Promise<NoteRefVm | null>>();
const notesVaultSetActive = vi.fn<(vaultId: string) => Promise<void>>();
const syncReadFrontmatter = vi.fn<(profileId: string, subpath: string) => Promise<string>>();
const sessionsFileRename =
  vi.fn<(profileId: string, subpath: string, expected: string, block: string) => Promise<string>>();

vi.mock("@/lib/ipc/client", () => ({
  syncReadText: (profileId: string, subpath: string) => syncReadText(profileId, subpath),
  syncWriteEntry: (profileId: string, subpath: string, text: string) =>
    syncWriteEntry(profileId, subpath, text),
  notesCsvRead: (vaultId: string, target: string) => notesCsvRead(vaultId, target),
  notesCsvSetCell: vi.fn(),
  notesTree: (vaultId: string, relDir: string) => notesTree(vaultId, relDir),
  notesResolveLink: (vaultId: string, target: string) => notesResolveLink(vaultId, target),
  notesVaultSetActive: (vaultId: string) => notesVaultSetActive(vaultId),
  // The mirror hydrates itself from this surface now (Story 45.18); a vault
  // list it cannot read leaves the mirror unhydrated, which is the state every
  // test below that does NOT seed one is asserting against.
  notesVaults: vi.fn(async () => []),
  notesVaultActive: vi.fn(async () => null),
  revealPath: vi.fn(async () => undefined),
  syncOpenEntry: vi.fn(async () => undefined),
  // Story 50.4: a markdown file in a profile now mounts the properties panel,
  // which reaches the client through this factory. A mock that omits an export
  // throws where the panel READS it, so these are needed even by the tests that
  // never touch a property. `syncReadFrontmatter` resolving to `""` is a file
  // with no frontmatter, which is what every fixture here is.
  //
  // Story 52.2 made both of these controllable, because a rename's answer is the
  // panel target the viewer re-addresses itself with, and only a test can say
  // what that answer was.
  syncReadFrontmatter: (profileId: string, subpath: string) =>
    syncReadFrontmatter(profileId, subpath),
  sessionsFileRename: (profileId: string, subpath: string, expected: string, block: string) =>
    sessionsFileRename(profileId, subpath, expected, block),
  syncWriteFrontmatter: vi.fn(async () => ""),
  notesSave: vi.fn(),
  recordingNoteTargets: vi.fn(async () => null),
  recordingOpenPath: vi.fn(async () => undefined),
  recordingSessionMeta: vi.fn(),
  tagsVocabulary: vi.fn(async () => ({ entries: [] })),
}));

import { SLASH_COMMANDS } from "@/components/notes/editor/slash-menu";
import { PROPERTIES_LABEL } from "@/components/notes/properties-panel";
import { matchEmoji } from "@/lib/emoji/match";
import type { NoteVaultVm } from "@/lib/ipc/client";
import { notesVaultsStore, resetNotesVaultsStoreForTest } from "@/lib/stores/notes-vaults";
import { panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { type ViewerFile, viewerComponentFor } from "@/lib/viewers";
import { FILE_SAVE_LABEL, TEXT_FILE_CAVEAT_TESTID } from "./text-file-frame";
import { TEXT_FILE_NOTICE_SLOT, TextFileViewer } from "./text-file-viewer";

/**
 * The configured world for a test that needs one (Story 45.18).
 *
 * **Two vaults, on two profiles, always.** A one-vault fixture cannot tell a
 * per-profile filter from an unconditional match — `notePathForFile` would
 * resolve `profile-2`'s file into `profile-1`'s vault and every assertion here
 * would still pass.
 */
function seedVaults(): void {
  const vault = (id: string, profileId: string, subfolder: string): NoteVaultVm =>
    ({
      id,
      profileId,
      name: id,
      subfolder,
      root: `/Volumes/${profileId}/${subfolder}`,
      indexed: true,
      noteCount: 2,
      unreadCount: 0,
      cadence: { commitIdleMs: 1000, pushIntervalMs: 5000, pushOnBlur: true },
    }) as NoteVaultVm;
  notesVaultsStore
    .getState()
    .setVaults([vault("vault-1", "profile-1", "inbox"), vault("vault-2", "profile-2", "inbox")]);
  notesVaultsStore.getState().setActiveVaultId("vault-2");
}

/** A folder listing with more than one note in it, so a mutation that keeps
 *  only the first — or matches the wrong one — has something to fail against. */
function folderWith(...notes: Array<{ id: string; path: string }>): NoteFolderVm {
  return {
    relDir: "",
    dirs: [],
    notes: notes.map(
      ({ id, path }) =>
        ({
          id,
          path,
          title: path,
          snippet: "",
          tags: [],
          updatedMs: 0,
          pinned: false,
          archived: false,
          unread: false,
          conflict: false,
        }) as unknown as NoteRowVm,
    ),
  };
}

/**
 * Press the rendered wikilink, retrying until the outcome is observable.
 *
 * The retry is not politeness about timing, it is required: the preview is mounted by an
 * async `import()`, and the decoration that draws the link is rebuilt whenever the
 * document it decorates changes — the loaded text arriving after the first paint is
 * exactly that. A node captured before either happens is detached, and a `mouseDown` on
 * it reaches no handler at all, silently, because a detached node still accepts events.
 * Re-querying inside the retry is what makes the press land on the view on screen.
 *
 * Story 51.5 narrowed the first half of that and not the second: the pane no longer
 * remounts on every text change — it adopts the text through `setContent` — but the
 * adoption is still a document change, so the decorations are still rebuilt under it.
 */
async function pressWikilink(outcome: () => void): Promise<void> {
  await waitFor(() => {
    const link = document.querySelector<HTMLElement>("[data-keeper-wikilink]");
    expect(link, "the preview drew no wikilink").not.toBeNull();
    fireEvent.mouseDown(link as HTMLElement);
    outcome();
  });
}

/** The notice this surface leaves behind. A `data-slot`, like every other
 *  named region in this app, so it is read with a selector rather than
 *  `getByTestId` — which looks for `data-testid` and finds nothing. */
function noticeText(): string | null {
  return document.querySelector(`[data-slot="${TEXT_FILE_NOTICE_SLOT}"]`)?.textContent ?? null;
}

/**
 * Why 20 s and not `waitFor`'s 5 s default.
 *
 * What is being waited for is the editor's lazily imported CodeMirror chunk,
 * not logic — and under eight concurrent suites that import has been measured
 * past five seconds, so the default turns a red into a measurement of the box.
 * Raised deliberately and named: a failure here should mean the press did
 * nothing, never that the machine was busy.
 */
const CHUNK_TIMEOUT_MS = 20_000;

// Applied at FILE scope rather than as a third argument per test. The
// per-test form was added by a script and it silently missed one, which then
// failed alone under load and looked like a defect in the code it was testing
// — the same class of mistake as a file sliced out of another file. A budget
// that cannot be missed is worth more than one that is precise.
vi.setConfig({ testTimeout: CHUNK_TIMEOUT_MS });

function target(overrides: Partial<ViewerFile> = {}): ViewerFile {
  return {
    name: "config.json",
    kind: "file",
    relativePath: "inbox/config.json",
    profileId: "profile-1",
    absolutePath: "/Volumes/merope/inbox/config.json",
    sizeLabel: "412 bytes",
    openWith: null,
    writeCaveat: null,
    writeRefusal: null,
    ...overrides,
  };
}

function vm(overrides: Partial<TextFileVm> = {}): TextFileVm {
  return {
    text: '{"port": 8080}',
    sizeBytes: 14,
    sizeLabel: "14 bytes",
    oversize: false,
    binary: false,
    detail: null,
    ...overrides,
  };
}

/** Mount exactly as a panel host does: ask the registry, render what it says. */
function openThroughTheRegistry(file: ViewerFile) {
  const { entry, Component } = viewerComponentFor(file);
  return { entry, ...render(<Component file={file} entry={entry} />) };
}

/** Drain microtasks without letting a frame run — see `raw-rendered-view.test.tsx`. */
async function settle(): Promise<void> {
  await act(async () => {
    for (let tick = 0; tick < 10; tick += 1) {
      await Promise.resolve();
    }
  });
}

/**
 * The real CodeMirror the raw view mounts, once its chunks have landed.
 *
 * 45.6's editor loads its grammar through `import()`, so the editor is not in
 * the DOM on the first tick. Waiting on a timer is safe here because
 * `withRangeRects` above gives the measure pass something to measure.
 */
async function editorHost(): Promise<HTMLElement> {
  await waitFor(() => expect(document.querySelector(".cm-content")).not.toBeNull());
  return document.querySelector(".cm-content") as HTMLElement;
}

let removeRangeRects: (() => void) | null = null;
beforeAll(() => {
  removeRangeRects = withRangeRects();
});
afterAll(() => {
  removeRangeRects?.();
});

beforeEach(() => {
  syncReadText.mockReset();
  syncWriteEntry.mockReset();
  notesCsvRead.mockReset();
  notesTree.mockReset();
  notesResolveLink.mockReset();
  notesVaultSetActive.mockReset();
  notesVaultSetActive.mockResolvedValue(undefined);
  syncReadFrontmatter.mockReset();
  // No frontmatter, which is what the panel's own suite calls an unblocked file
  // and what every fixture here was before Story 52.2 made this controllable.
  syncReadFrontmatter.mockResolvedValue("");
  sessionsFileRename.mockReset();
  resetNotesVaultsStoreForTest();
  resetPanelsStoreForTest();
  primaryViewStore.getState().setView("files");
  // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
  document.cookie = "keeper_viewer_modes=; path=/; max-age=0";
});

describe("the registry's `text` id really mounts this viewer", () => {
  it("resolves a .json file to a component that draws its structure", async () => {
    syncReadText.mockResolvedValue(vm());
    const { entry } = openThroughTheRegistry(target());

    // The row the table chose, so a failure here says which half broke.
    expect(entry.viewer).toBe("text");
    expect(entry.rendered).toBe("structure");

    await settle();
    expect(screen.getByRole("tab", { name: "Structure" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("port")).toBeInTheDocument();
    expect(screen.getByText("8080")).toBeInTheDocument();
    // The path arrived as the listing produced it, and nothing was joined.
    expect(syncReadText).toHaveBeenCalledWith("profile-1", "inbox/config.json");
  });

  it("says it is opening rather than flashing an empty editor", () => {
    // Never settles, so the viewer is held in the state under test. The
    // executor form because this project's `lib: ES2020` has no
    // `Promise.withResolvers`, and there is nothing to resolve regardless.
    syncReadText.mockReturnValue(new Promise<TextFileVm>(() => undefined));
    openThroughTheRegistry(target());
    expect(screen.getByRole("status")).toHaveTextContent("opening config.json");
  });
});

describe("the states a real vault produces", () => {
  it("refuses bytes that are not text, in Rust's own words, with no editor", async () => {
    syncReadText.mockResolvedValue(
      vm({ text: null, binary: true, detail: "config.json is not text keeper can edit" }),
    );
    openThroughTheRegistry(target());
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent("is not text keeper can edit");
    // Rendering `text ?? ""` would put an empty editable pane over a binary
    // file and offer to save it, which is how an editor overwrites a `.png`.
    expect(screen.queryByRole("tablist")).toBeNull();
    expect(document.querySelector(".cm-content")).toBeNull();
  });

  it("shows Rust's sentence when the file cannot be read at all", async () => {
    syncReadText.mockRejectedValue({ message: "inbox/config.json: no such file or directory" });
    openThroughTheRegistry(target());
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent("no such file or directory");
  });

  it("says a file outside every profile can be shown but not written", async () => {
    openThroughTheRegistry(target({ profileId: null }));
    await settle();

    // The hook never calls a command it cannot scope — reading through
    // `absolutePath` would go around browse.rs's containment (AD-65).
    expect(syncReadText).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("not inside a synced folder");
  });
});

/** The modifier CodeMirror's `Mod-s` resolves to **in this test environment**.
 *
 *  A constant, not a browser check. `src/test/no-user-agent-gating.test.ts`
 *  forbids asking the browser which platform it is anywhere under `src/`,
 *  because in this app that answer comes from the Rust capabilities handshake
 *  and a client-side guess is how the rule rots. Here the constant is also the
 *  honest value: jsdom presents itself as something other than a Mac, so
 *  CodeMirror binds `Mod` to Ctrl, and a Cmd-flagged event would match nothing,
 *  assert nothing, and still pass. */
const MOD = { ctrlKey: true };

/** Type into the real editor, the way an edit actually arrives. */
async function retype(editor: HTMLElement, text: string): Promise<void> {
  await act(async () => {
    const view = EditorView.findFromDOM(editor);
    view?.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
  });
  await settle();
}

describe("saving goes through Story 45.3's one write path", () => {
  it("writes the exact buffer to the profile and subpath it was given", async () => {
    syncReadText.mockResolvedValue(vm({ text: "hello\n" }));
    syncWriteEntry.mockResolvedValue(undefined);
    openThroughTheRegistry(target({ name: "notes.txt", relativePath: "inbox/notes.txt" }));
    const editor = await editorHost();

    // Through the real CodeMirror the raw view mounts, not through a stand-in:
    // the claim is that the characters the reader produced are the characters
    // the write command receives, tabs and trailing newline included.
    await retype(editor, "goodbye\n\tindented\n");
    fireEvent.keyDown(editor, { key: "s", ...MOD });
    await settle();

    expect(syncWriteEntry).toHaveBeenCalledWith(
      "profile-1",
      "inbox/notes.txt",
      "goodbye\n\tindented\n",
    );
  });

  it("declines out loud, and does not write, when nothing changed", async () => {
    const info = vi.spyOn(console, "info").mockImplementation(() => {});
    syncReadText.mockResolvedValue(vm({ text: "hello\n" }));
    openThroughTheRegistry(target({ name: "notes.txt", relativePath: "inbox/notes.txt" }));
    const editor = await editorHost();

    fireEvent.keyDown(editor, { key: "s", ...MOD });
    await settle();

    expect(syncWriteEntry).not.toHaveBeenCalled();
    // DW-162: a save that silently does nothing looks like a save that worked.
    expect(info).toHaveBeenCalledWith(expect.stringContaining("nothing changed"));
    info.mockRestore();
  });

  it("keeps a Windows file's line endings when one word is edited", async () => {
    syncReadText.mockResolvedValue(vm({ text: "alpha\r\nbeta\r\ngamma\r\n" }));
    syncWriteEntry.mockResolvedValue(undefined);
    openThroughTheRegistry(target({ name: "notes.txt", relativePath: "inbox/notes.txt" }));
    const editor = await editorHost();

    // Edited IN PLACE, by position, deliberately: replacing the whole document
    // with a CRLF string would re-introduce the terminators as ordinary
    // characters and hide the thing being asserted. What has to survive is the
    // text the editor was CONSTRUCTED with, because that is where a normalising
    // buffer does its damage — one word retyped, every line in the file
    // changed, and a whole-file diff on the next sync of a file the reader
    // believes they barely touched.
    //
    // `TextFileVm`'s own doc comment promises the opposite in its own words:
    // "no line-ending normalisation ... a file opened and saved untouched is
    // the same file, which is the only thing that makes an editor over synced
    // content safe to use at all". Rust keeps that promise; this asserts the
    // editor above it does too.
    await act(async () => {
      const view = EditorView.findFromDOM(editor);
      const at = view?.state.doc.toString().indexOf("beta") ?? -1;
      view?.dispatch({ changes: { from: at, to: at + 4, insert: "BETA" } });
    });
    await settle();
    fireEvent.keyDown(editor, { key: "s", ...MOD });
    await settle();

    expect(syncWriteEntry).toHaveBeenCalledWith(
      "profile-1",
      "inbox/notes.txt",
      "alpha\r\nBETA\r\ngamma\r\n",
    );
  });

  it("puts Rust's refusal of a save where the reader is looking", async () => {
    syncReadText.mockResolvedValue(vm({ text: "hello\n" }));
    syncWriteEntry.mockRejectedValue({
      message: "inbox/notes.txt is on a read-only volume, so keeper did not write it",
    });
    openThroughTheRegistry(target({ name: "notes.txt", relativePath: "inbox/notes.txt" }));
    const editor = await editorHost();

    await retype(editor, "goodbye\n");
    fireEvent.keyDown(editor, { key: "s", ...MOD });
    await settle();

    // Whether a LOCATION can be written is Rust's answer and it arrives here,
    // as a sentence. A viewer that swallowed it would leave the reader
    // believing a file was saved that was not.
    expect(screen.getByRole("alert")).toHaveTextContent("read-only volume");
    // And the buffer is not rolled back: losing what somebody typed is worse
    // than showing text the disk does not have yet.
    expect(EditorView.findFromDOM(editor)?.state.doc.toString()).toBe("goodbye\n");
  });

  it("refuses a format keeper must not rewrite, by name, before an edit is possible", async () => {
    syncReadText.mockResolvedValue(vm({ text: "hello\n" }));
    const { entry } = viewerComponentFor(target({ name: "notes.txt" }));

    // Built by hand on purpose. No `viewer: "text"` row is non-writable today,
    // so the registry cannot produce this input — and a guard that only runs on
    // inputs the current table cannot produce is precisely the guard that rots
    // unnoticed until the row that needs it is added.
    render(
      <TextFileViewer
        file={target({ name: "notes.txt", relativePath: "inbox/notes.txt" })}
        entry={{ ...entry, writable: false, label: "Locked" }}
      />,
    );
    await editorHost();

    expect(screen.getByRole("status")).toHaveTextContent("keeper does not write Locked files");
  });

  it("shows AD-102's caveat before the first keystroke, and only when there is one", async () => {
    // Story 46.14. A file no vault holds is now writable — keeper saves it
    // through a second, plain writer — and the whole of what makes that
    // honest is that the reader is told what is missing BEFORE editing, not
    // after saving. Rust composes the sentence; this surface owes only that
    // it is on screen with the editor, not instead of it.
    const caveat =
      "AGENTS.md is not one of keeper's notes — it is outside tgdrive's notes vault " +
      "(10-notes). keeper saves it straight to the file and sends a delete to this " +
      "computer's trash: no note history, no search index and no conflict copy. Nothing " +
      "about how tgdrive syncs this folder changes.";
    syncReadText.mockResolvedValue(vm({ text: "hello\n" }));

    openThroughTheRegistry(
      target({ name: "AGENTS.md", relativePath: "AGENTS.md", writeCaveat: caveat }),
    );
    const editor = await editorHost();

    // Verbatim, never paraphrased — the same rule `reason` and `detail` follow.
    expect(screen.getByTestId(TEXT_FILE_CAVEAT_TESTID)).toHaveTextContent(caveat);
    // And the editor is there: this is a caveat, not a refusal.
    expect(editor).toBeInTheDocument();
  });

  it("says nothing standing about a file keeper does manage", async () => {
    syncReadText.mockResolvedValue(vm({ text: "hello\n" }));

    openThroughTheRegistry(target({ relativePath: "10-notes/config.json" }));
    await editorHost();

    expect(screen.queryByTestId(TEXT_FILE_CAVEAT_TESTID)).toBeNull();
  });
});

describe("the CSV table, now that a panel can resolve its vault (Story 45.18)", () => {
  it("tables a CSV inside the notes vault, addressed by the vault it resolved to", async () => {
    // THE INHERITED ASSERTION, CHANGED. Until 45.18 this test pinned the
    // opposite — a CSV opened from a panel showed its source and a sentence
    // saying keeper could only table one inside a notes vault — because a panel
    // held a sync profile id and 44.16's commands want a notes vault id. That
    // resolution now exists in Rust and is mirrored here, so the same bytes
    // draw the same table in a panel and in a note.
    seedVaults();
    syncReadText.mockResolvedValue(vm({ text: "name,qty\nwidget,3\nsprocket,5\n" }));
    notesCsvRead.mockResolvedValue({
      relPath: "rows.csv",
      rev: "rev-1",
      columns: 2,
      totalRows: 3,
      rows: [
        { index: 0, line: 1, cells: ["name", "qty"], ragged: false },
        { index: 1, line: 2, cells: ["widget", "3"], ragged: false },
        { index: 2, line: 3, cells: ["sprocket", "5"], ragged: false },
      ],
      notices: [],
    });
    openThroughTheRegistry(target({ name: "rows.csv", relativePath: "inbox/rows.csv" }));
    await settle();

    expect(screen.getByRole("tab", { name: "Table" })).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("alert")).toBeNull();
    // The CALL, not only the result. A mock that ignores its arguments would
    // draw this table for `vault-2` and for the whole profile path, and the
    // rendering would be identical: the file would be read from the wrong
    // vault, or not found at all, only on a real machine.
    expect(notesCsvRead).toHaveBeenCalledWith("vault-1", "rows.csv");
  });

  it("still opens a CSV outside every vault as its source, and says why", async () => {
    // The other half of the same rule, and the reason the assertion above is
    // not simply "the table always draws now": a profile that is not a vault,
    // and a file beside the vault in one that is, both still have no vault
    // coordinates and must say so rather than draw an empty table.
    seedVaults();
    syncReadText.mockResolvedValue(vm({ text: "name,qty\nwidget,3\n" }));
    openThroughTheRegistry(target({ name: "rows.csv", relativePath: "archive/rows.csv" }));
    const editor = await editorHost();

    expect(screen.getByRole("alert")).toHaveTextContent("inside a notes vault");
    expect(editor.textContent).toContain("widget");
    expect(notesCsvRead).not.toHaveBeenCalled();
  });

  it("does not claim a vault before the vault list has been read", async () => {
    // `null` is "keeper has not looked", never "you have none". Guessing a
    // vault here would address 44.16's commands with an id nobody confirmed.
    syncReadText.mockResolvedValue(vm({ text: "name,qty\nwidget,3\n" }));
    openThroughTheRegistry(target({ name: "rows.csv", relativePath: "inbox/rows.csv" }));
    await editorHost();

    expect(screen.getByRole("alert")).toHaveTextContent("inside a notes vault");
    expect(notesCsvRead).not.toHaveBeenCalled();
  });
});

describe("a file knows its note (Story 45.18, FR-196)", () => {
  it("offers Open in Notes for a markdown file in the vault, and opens the note it names", async () => {
    seedVaults();
    syncReadText.mockResolvedValue(vm({ text: "# Meeting\n" }));
    // Two notes in the listing, so a match on the wrong row — or one that keeps
    // only the first — has something to fail against.
    notesTree.mockResolvedValue(
      folderWith(
        { id: "note-other", path: "daily/other.md" },
        { id: "note-1", path: "meeting.md" },
      ),
    );
    openThroughTheRegistry(target({ name: "meeting.md", relativePath: "inbox/meeting.md" }));
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "Open in Notes" }));
    await settle();

    // The CALL: the vault it resolved to, and the file's own directory INSIDE
    // that vault — not the profile-relative one. Passing `inbox/` here would
    // list a folder that does not exist in the vault and find no note.
    expect(notesTree).toHaveBeenCalledWith("vault-1", "");
    // The vault is made active before the target is set, or the notes pane
    // shows nothing: it only renders the open note while its vault is active.
    expect(notesVaultSetActive).toHaveBeenCalledWith("vault-1");
    // And the state: the note is open, BESIDE the file rather than replacing
    // it, and the Notes tab is showing.
    const targets = panelsStore.getState().panels.map((panel) => panel.target);
    expect(targets).toContainEqual({ kind: "note", vaultId: "vault-1", noteId: "note-1" });
    expect(primaryViewStore.getState().view).toBe("notes");
  });

  it("offers nothing for a markdown file outside the vault", async () => {
    // The story's own sentence: absent rather than present-and-failing. A
    // disabled button, or one that reported "no note" on click, would be the
    // shape this is written to prevent.
    seedVaults();
    syncReadText.mockResolvedValue(vm({ text: "# Loose\n" }));
    openThroughTheRegistry(target({ name: "loose.md", relativePath: "archive/loose.md" }));
    await settle();

    expect(screen.queryByRole("button", { name: "Open in Notes" })).toBeNull();
  });

  it("offers nothing for a file in the vault that is not markdown", async () => {
    // An attachment in the vault is not a note, and the registry's own format
    // is what says so — never the extension (AD-87).
    seedVaults();
    syncReadText.mockResolvedValue(vm({ text: "a,b\n1,2\n" }));
    notesCsvRead.mockResolvedValue({
      relPath: "rows.csv",
      rev: "r",
      columns: 2,
      totalRows: 1,
      rows: [{ index: 0, line: 1, cells: ["a", "b"], ragged: false }],
      notices: [],
    });
    openThroughTheRegistry(target({ name: "rows.csv", relativePath: "inbox/rows.csv" }));
    await settle();

    expect(screen.queryByRole("button", { name: "Open in Notes" })).toBeNull();
  });

  it("says the index has not caught up rather than opening nothing", async () => {
    // The file is on screen and the vault holds it, so "not found" without a
    // reason reads as a fault in keeper. It usually is a cold scan in flight.
    seedVaults();
    syncReadText.mockResolvedValue(vm({ text: "# New\n" }));
    notesTree.mockResolvedValue(folderWith({ id: "note-other", path: "somethingelse.md" }));
    openThroughTheRegistry(target({ name: "fresh.md", relativePath: "inbox/fresh.md" }));
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "Open in Notes" }));
    await settle();

    expect(noticeText()).toContain("fresh.md");
    expect(primaryViewStore.getState().view).toBe("files");
  });

  it("refuses to navigate when the vault switch was refused, and says why", async () => {
    // W2Media's shape: two producers that run one after the other cannot share
    // one view of the world. `setActiveVault` swallows a rejected
    // `notes_vault_set_active` into the mirror's error slot and returns
    // normally, so the later producer — this action reporting success — used to
    // win. The reader arrived in Notes with no note open, because the pane only
    // shows one while its vault is active, and with nothing saying why.
    seedVaults();
    syncReadText.mockResolvedValue(vm({ text: "# Meeting\n" }));
    notesTree.mockResolvedValue(
      folderWith(
        { id: "note-other", path: "daily/other.md" },
        { id: "note-1", path: "meeting.md" },
      ),
    );
    notesVaultSetActive.mockImplementation(async () => {
      throw { message: "that vault is not open on this device" };
    });
    openThroughTheRegistry(target({ name: "meeting.md", relativePath: "inbox/meeting.md" }));
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "Open in Notes" }));
    await settle();

    expect(noticeText()).toContain("that vault is not open on this device");
    // And nothing moved: being left where you are with a reason beats being
    // moved somewhere empty.
    expect(primaryViewStore.getState().view).toBe("files");
    expect(panelsStore.getState().panels.map((panel) => panel.target)).not.toContainEqual({
      kind: "note",
      vaultId: "vault-1",
      noteId: "note-1",
    });
  });

  it("shows Rust's own words when the vault listing is refused", async () => {
    seedVaults();
    syncReadText.mockResolvedValue(vm({ text: "# New\n" }));
    notesTree.mockRejectedValue({ message: "that vault is not open" });
    openThroughTheRegistry(target({ name: "fresh.md", relativePath: "inbox/fresh.md" }));
    await settle();

    fireEvent.click(screen.getByRole("button", { name: "Open in Notes" }));
    await settle();

    expect(noticeText()).toContain("that vault is not open");
  });

  /**
   * The SECOND host of the decoration layer, which is the whole reason these
   * tests are here and not only in the note editor's suite.
   *
   * A `.md` file opened from Files renders through the same `livePreview` the
   * editor mounts, so every link in it had the same `cursor: pointer` over the
   * same dead text — and a branch reachable only from the second host cannot be
   * reached by tests that all route through the first.
   */
  it("follows a wikilink written in a vault file, opening the note beside it", async () => {
    seedVaults();
    // Line 2, not line 1: the caret sits at offset 0 and `livePreview` gives
    // the caret's own line its source back, so a wikilink on the first line
    // renders as `[[Meeting]]` text with no decoration and no attribute.
    syncReadText.mockResolvedValue(
      vm({
        text: "# Index\n\nsee [[Meeting]] for the rest\n\n\nand a last line the caret can sit on, so the link above is\nnever the caret's own line.\n",
      }),
    );
    notesResolveLink.mockResolvedValue({
      vaultId: "vault-1",
      id: "note-7",
      path: "meeting.md",
      title: "Meeting",
    });
    // The panel the real host would already be holding. Without it the store
    // has one empty panel, `openPanel` fills it rather than appending, and the
    // test would prove the opposite of "beside".
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "profile-1", relativePath: "inbox/index.md" });
    openThroughTheRegistry(target({ name: "index.md", relativePath: "inbox/index.md" }));
    await settle();

    // The CALL: the vault this FILE resolved to, and the link's raw text. A
    // resolver handed the empty vault id would answer for no vault at all, and
    // the rendering would be identical.
    await pressWikilink(() => {
      expect(notesResolveLink).toHaveBeenCalledWith("vault-1", "Meeting");
    });
    await settle();
    const targets = panelsStore.getState().panels.map((panel) => panel.target);
    expect(targets).toContainEqual({ kind: "note", vaultId: "vault-1", noteId: "note-7" });
    // Beside, not instead: the file panel is still open behind it.
    expect(targets).toContainEqual({
      kind: "file",
      profileId: "profile-1",
      relativePath: "inbox/index.md",
    });
  });

  it("says a wikilink in a file outside every vault cannot be looked up", async () => {
    // Not silence, and not a lookup against an empty vault id: the file has no
    // vault, and that is the fact the reader needs.
    seedVaults();
    syncReadText.mockResolvedValue(
      vm({
        text: "# Loose\n\nsee [[Meeting]] for the rest\n\n\nand a last line the caret can sit on, so the link above is\nnever the caret's own line.\n",
      }),
    );
    openThroughTheRegistry(target({ name: "loose.md", relativePath: "archive/loose.md" }));
    await settle();

    await pressWikilink(() => {
      expect(noticeText()).toContain("not inside a notes vault");
    });

    expect(notesResolveLink).not.toHaveBeenCalled();
  });
});

/**
 * The writing tools a session log now has (Story 50.3, FR-233, rows 1–5 and
 * 8–10).
 *
 * **Why here.** The claim is not "these extensions work" — `slash-menu.test.ts`,
 * `emoji-complete.test.ts` and `format-commands.test.ts` each prove that over a
 * stack they build themselves, and every one of them stayed green through Story
 * 43.9, in which the `/` menu had never opened for anybody. The claim is that a
 * FILE opened the way a panel opens one has them: through the registry, through
 * the frame's own markdown-and-writable verdict, in the real editor, on the tab
 * a person writes in. Only a test that starts at `viewerComponentFor` can see
 * that, which is what this file has always been for.
 *
 * The fixture is the owner's own layout: a folder-shaped session, whose zone can
 * never be inside a notes vault, so `inVault` is null throughout and none of
 * this depends on a vault resolving.
 */
const SESSION_DIR = "60-sessions/active/2026-08-10-keeper";
const SESSION_README = `${SESSION_DIR}/README.md`;

/** Type at the caret, one transaction per character.
 *
 *  A completion only opens for a transaction that says it came from a keystroke,
 *  and the emoji filter is written against exactly the shape one person typing
 *  one character produces — so a whole-string dispatch would prove neither. */
async function typeAtCaret(view: EditorView, text: string): Promise<void> {
  await act(async () => {
    for (const character of text) {
      const at = view.state.selection.main.head;
      view.dispatch({
        changes: { from: at, insert: character },
        selection: { anchor: at + character.length },
        userEvent: "input.type",
      });
    }
  });
}

/** The live view behind a content DOM, asserted rather than assumed. */
function liveView(editor: HTMLElement): EditorView {
  const view = EditorView.findFromDOM(editor);
  expect(view, "no EditorView is mounted in that content DOM").not.toBeNull();
  return view as EditorView;
}

/**
 * Open a session log and put the caret at the end of its Source tab.
 *
 * A savable markdown file opens in NOTE since story 52.3 (`defaultViewMode`), so
 * the press below is what makes these tests about the SOURCE tab's tools rather
 * than about whichever pane happened to be selected. Both panes have the toolbar
 * now; the Source tab is the one 50.3 wired and the one these tests own.
 */
async function openSessionLog(text: string): Promise<{ editor: HTMLElement; view: EditorView }> {
  syncReadText.mockResolvedValue(vm({ text }));
  syncWriteEntry.mockResolvedValue(undefined);
  openThroughTheRegistry(target({ name: "README.md", relativePath: SESSION_README }));
  await settle();
  fireEvent.click(screen.getByRole("tab", { name: "Source" }));
  const editor = await editorHost();
  const view = liveView(editor);
  await act(async () => {
    view.dispatch({ selection: { anchor: view.state.doc.length } });
  });
  return { editor, view };
}

describe("a session log is markdown a person writes into (Story 50.3)", () => {
  it("has the format toolbar, and Bold wraps the selection", async () => {
    const { view } = await openSessionLog("# Session\n\nalpha\n");

    const at = view.state.doc.toString().indexOf("alpha");
    await act(async () => {
      view.dispatch({ selection: { anchor: at, head: at + "alpha".length } });
    });
    fireEvent.click(screen.getByRole("button", { name: "Bold" }));
    await settle();

    // The characters, in the buffer the save path holds — not a class on a
    // button. This is the same command the note editor's toolbar runs, from the
    // one module both import.
    expect(view.state.doc.toString()).toBe("# Session\n\n**alpha**\n");
  });

  it("saves what the toolbar wrote, through the file's own write path", async () => {
    const { editor, view } = await openSessionLog("# Session\n\nalpha\n");

    const at = view.state.doc.toString().indexOf("alpha");
    await act(async () => {
      view.dispatch({ selection: { anchor: at, head: at + "alpha".length } });
    });
    fireEvent.click(screen.getByRole("button", { name: "Bold" }));
    await settle();
    fireEvent.keyDown(editor, { key: "s", ...MOD });
    await settle();

    // Row 9. A toolbar that edited a buffer the save path could not see would
    // look identical on screen and write nothing — which is the failure mode a
    // file surface with no autosave cannot afford.
    expect(syncWriteEntry).toHaveBeenCalledWith(
      "profile-1",
      SESSION_README,
      "# Session\n\n**alpha**\n",
    );
  });

  it("opens the slash menu, offering the commands a note offers", async () => {
    const { view } = await openSessionLog("# Session\n\n");

    await typeAtCaret(view, "/");
    await vi.waitFor(() => expect(completionStatus(view.state)).toBe("active"));

    // As a set: with an empty pattern CodeMirror orders the rows itself. The
    // rows are the shared table's own, and `writing-tools.test.ts` proves there
    // is only one such table in the repository — which together is the whole of
    // "the same items the note editor offers".
    const offered = currentCompletions(view.state).map((option) => option.label);
    expect(offered.sort()).toEqual(SLASH_COMMANDS.map((command) => command.label).sort());
  });

  it("inserts exactly the text the same command inserts in a note", async () => {
    const { view } = await openSessionLog("# Session\n\n");

    await typeAtCaret(view, "/tas");
    await vi.waitFor(() => expect(completionStatus(view.state)).toBe("active"));
    // Polled rather than asserted once: production keeps CodeMirror's 75 ms
    // `interactionDelay`, so an accept that lands under a still-moving hand
    // refuses — returning false and changing nothing, which is safe to retry.
    await vi.waitFor(() => expect(acceptCompletion(view)).toBe(true));
    await settle();

    const task = SLASH_COMMANDS.find((command) => command.label === "Task");
    if (task === undefined) {
      throw new Error("the shared slash table no longer has a Task row");
    }
    // Row 10, and asserted against the table's own function rather than a
    // literal: `slash-menu.test.ts` pins `- [ ] ` for this row over the note's
    // stack, so a change to the text has to change both surfaces at once.
    expect(view.state.doc.toString()).toBe(`# Session\n\n${task.text(new Date())}`);
    // And no slash survived, which is the whole of 43.9's defect.
    expect(view.state.doc.toString()).not.toContain("/");
  });

  it("completes an emoji shortcode, and commits one typed in full", async () => {
    const { view } = await openSessionLog("# Session\n\n");

    await typeAtCaret(view, ":sm");
    await vi.waitFor(() => expect(completionStatus(view.state)).toBe("active"));

    // The same list `matchEmoji` gives a note, in the same order — the source
    // hands narrowing to keeper's matcher (`filter: false`), so what the menu
    // offers IS the matcher's answer.
    const matches = matchEmoji("sm");
    expect(matches.length).toBeGreaterThan(0);
    const offered = currentCompletions(view.state).map((option) => option.label);
    expect(offered).toEqual(matches.map((hit) => hit.shortcode));

    // The other half of Story 45.11, which moved with the menu: a shortcode
    // typed straight through becomes its character.
    await typeAtCaret(view, "\n:tada:");
    expect(view.state.doc.toString()).toBe("# Session\n\n:sm\n🎉");
  });
});

describe("a file that is not prose keeps the editor it had (Story 50.3)", () => {
  it("gives a workspace source file no toolbar, no menu and no shortcodes", async () => {
    syncReadText.mockResolvedValue(vm({ text: "fn main() {}\n" }));
    openThroughTheRegistry(
      target({ name: "main.rs", relativePath: `${SESSION_DIR}/workspace/main.rs` }),
    );
    const editor = await editorHost();
    const view = liveView(editor);

    // Row 4. `startCompletion` returns false only when the extension is absent
    // from the view, which is a stronger claim than "no menu opened for the keys
    // this test happened to press".
    expect(screen.queryByRole("button", { name: "Bold" })).toBeNull();
    expect(startCompletion(view)).toBe(false);
    // And the code editor is the one it always was: a gutter, because a config
    // file is something people are told about by line.
    expect(document.querySelector(".cm-gutters")).not.toBeNull();

    await act(async () => {
      view.dispatch({ selection: { anchor: view.state.doc.length } });
    });
    await typeAtCaret(view, ":tada:");

    // Six characters, unchanged. A `.rs` file is not prose, and a shortcode in
    // one is a string literal somebody meant.
    expect(view.state.doc.toString()).toBe("fn main() {}\n:tada:");
  });

  it("keeps the rendered tab read-only, with no toolbar over it (AD-88)", async () => {
    syncReadText.mockResolvedValue(vm({ text: "# Session\n\nalpha\n" }));
    openThroughTheRegistry(target({ name: "README.md", relativePath: SESSION_README }));
    await settle();

    // Row 5, re-anchored by story 52.3: a savable markdown file now LANDS in
    // Note, so the read-only half is reached by pressing Preview — which is what
    // this test is about and always was. What must not change is what Preview is:
    // a drawing of the document that nothing can type into (AD-88).
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settle();
    expect(screen.getByRole("tab", { name: "Preview" })).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("button", { name: "Bold" })).toBeNull();

    const view = liveView(await editorHost());
    expect(view.state.readOnly).toBe(true);
    expect(startCompletion(view)).toBe(false);
  });

  it("offers no tools over a markdown format keeper will not write", async () => {
    syncReadText.mockResolvedValue(vm({ text: "# Session\n\nalpha\n" }));
    const { entry } = viewerComponentFor(target({ name: "README.md" }));

    // Built by hand for the reason the refusal test above states: no
    // `viewer: "text"` row is non-writable today, and a guard that only runs on
    // inputs the current table can produce is the guard that rots unnoticed.
    render(
      <TextFileViewer
        file={target({ name: "README.md", relativePath: SESSION_README })}
        entry={{ ...entry, writable: false, label: "Locked" }}
      />,
    );
    await settle();
    fireEvent.click(screen.getByRole("tab", { name: "Source" }));
    const view = liveView(await editorHost());

    // Row 8. Absent, not present-and-failing: the tools are ways of writing
    // text, and this buffer is one nothing can be written into.
    expect(screen.queryByRole("button", { name: "Bold" })).toBeNull();
    expect(startCompletion(view)).toBe(false);
  });

  it("offers no tools over a file only the first part of which was read", async () => {
    syncReadText.mockResolvedValue(
      vm({ text: "# Session\n\nalpha\n", oversize: true, sizeLabel: "40 MB" }),
    );
    openThroughTheRegistry(target({ name: "README.md", relativePath: SESSION_README }));
    await settle();
    fireEvent.click(screen.getByRole("tab", { name: "Source" }));
    const view = liveView(await editorHost());

    // The other half of row 8, and the reason the frame decides the tools from
    // the same flag that decides Save: the loader refuses a save that would
    // truncate the rest of the file, so there is no save for a toolbar edit to
    // land through.
    expect(screen.queryByRole("button", { name: FILE_SAVE_LABEL })).toBeNull();
    expect(screen.queryByRole("button", { name: "Bold" })).toBeNull();
    expect(startCompletion(view)).toBe(false);
  });
});

/**
 * The hole Story 50.3 shipped, and the fixture that would have found it.
 *
 * The story's own justification named a guard that did not exist: "a
 * `workspace/` source file is refused by Rust rather than by the registry …
 * what keeps the tools off it here is that it is not markdown". The test above
 * it uses `main.rs`, so it proved only that a `.rs` is not markdown and stayed
 * green over the case the sentence was about. `workspace/` markdown is the
 * documented normal case — `docs/sessions.md` says `workspace/` files open
 * read-only, and this repo's own canonical fixture for one is
 * `workspace/iter-3.md` — and before the fix it got the full toolbar, the slash
 * menu, emoji completion and a Save button over a buffer every write refuses,
 * and was not marked read-only either.
 *
 * The refusal is Rust's own: `keeper_sync::files_write::WriteRefusal::
 * SessionWorkspace`'s Display, which `sync_browse` puts on the listing row this
 * panel opened the file from.
 */
const WORKSPACE_REFUSAL =
  `${SESSION_DIR}/workspace/notes.md is inside a session's workspace — scratch that is not ` +
  "versioned, not synced, and dies with the session. keeper reads it but never writes there; " +
  "promote the file into the session's artifacts instead.";
const WORKSPACE_NOTE = `${SESSION_DIR}/workspace/notes.md`;

describe("markdown keeper will not write gets no writing tools (Story 50.3)", () => {
  it("gives a workspace markdown file no toolbar, no menu, no Save, and says why", async () => {
    syncReadText.mockResolvedValue(vm({ text: "# Scratch\n\nalpha\n" }));
    openThroughTheRegistry(
      target({ name: "notes.md", relativePath: WORKSPACE_NOTE, writeRefusal: WORKSPACE_REFUSAL }),
    );
    await settle();
    fireEvent.click(screen.getByRole("tab", { name: "Source" }));
    const view = liveView(await editorHost());

    // Every control that assumes a save can follow, absent — and the reason on
    // screen in Rust's words rather than nothing.
    expect(screen.queryByRole("button", { name: "Bold" })).toBeNull();
    expect(startCompletion(view)).toBe(false);
    expect(screen.queryByRole("button", { name: FILE_SAVE_LABEL })).toBeNull();
    expect(view.state.readOnly).toBe(true);
    expect(screen.getByText(WORKSPACE_REFUSAL)).toBeInTheDocument();

    // And the buffer is genuinely inert to the tools: a shortcode typed into
    // scratch stays six characters, as it does in a `.rs`.
    await act(async () => {
      view.dispatch({ selection: { anchor: view.state.doc.length } });
    });
    await typeAtCaret(view, ":tada:");
    expect(view.state.doc.toString()).toContain(":tada:");
  });

  it("withholds them for the refusal and not for the word workspace in the path", async () => {
    // The same file at the same path, with keeper's verdict flipped and nothing
    // else changed. This is what makes the test above a claim about the fence
    // rather than about a path segment — and it is the assertion that would
    // fail on the tempting wrong fix, a `relativePath.includes("/workspace/")`
    // in the webview, which is the frontend deciding which folders are scratch
    // (AD-65).
    syncReadText.mockResolvedValue(vm({ text: "# Scratch\n\nalpha\n" }));
    openThroughTheRegistry(
      target({ name: "notes.md", relativePath: WORKSPACE_NOTE, writeRefusal: null }),
    );
    await settle();
    fireEvent.click(screen.getByRole("tab", { name: "Source" }));
    const view = liveView(await editorHost());

    expect(await screen.findByRole("button", { name: "Bold" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: FILE_SAVE_LABEL })).toBeInTheDocument();
    expect(view.state.readOnly).toBe(false);
  });
});

/**
 * The owner's own sentence, end to end (Story 51.5, FR-294).
 *
 * *"nie jest to prawdziwy note edytor jak w notes (chcialem preview, source,
 * note)"* — so this opens a session log the way the panel opens one, presses the
 * third tab, writes in it, and asserts the bytes that reach Rust. Everything
 * below the IPC line is real: the registry binding, the frame's predicate, the
 * loader, the decoration layer and the one write path.
 */
describe("a session log opens in three modes (Story 51.5)", () => {
  it("opens in Note, writes there, and saves only when asked", async () => {
    syncReadText.mockResolvedValue(vm({ text: "# Session\n\nalpha\n" }));
    syncWriteEntry.mockResolvedValue(undefined);
    openThroughTheRegistry(target({ name: "README.md", relativePath: SESSION_README }));
    await settle();

    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "Preview",
      "Source",
      "Note",
    ]);
    // Story 52.3, end to end: he lands where he writes, with no press at all —
    // and the pane he lands in is the editable one, not the Preview that used to
    // be selected here.
    expect(screen.getByRole("tab", { name: "Note" })).toHaveAttribute("aria-selected", "true");

    const editor = await editorHost();
    const view = liveView(editor);
    expect(view.state.readOnly).toBe(false);

    await act(async () => {
      view.dispatch({ selection: { anchor: view.state.doc.length } });
    });
    await typeAtCaret(view, "beta\n");

    // Rendered as he types, through the note editor's own decorations — the
    // half of the old refusal that never had teeth.
    expect(document.querySelector(".cm-lp-h1")).not.toBeNull();
    // And nothing has been written. There is no autosave for a file, in this
    // mode least of all: the write path is last-write-wins.
    expect(syncWriteEntry).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: FILE_SAVE_LABEL })).toBeEnabled();

    fireEvent.keyDown(editor, { key: "s", ...MOD });
    await settle();

    // The same command, the same profile and the same subpath the Source tab
    // saves through — which is the whole of "Note mode adds no write path".
    expect(syncWriteEntry).toHaveBeenCalledWith(
      "profile-1",
      SESSION_README,
      "# Session\n\nalpha\nbeta\n",
    );
  });

  it("offers no third tab over a file keeper will not write", async () => {
    syncReadText.mockResolvedValue(vm({ text: "# Scratch\n\nalpha\n" }));
    openThroughTheRegistry(
      target({ name: "notes.md", relativePath: WORKSPACE_NOTE, writeRefusal: WORKSPACE_REFUSAL }),
    );
    await settle();

    // Rust's fence, arriving on the listing row (AD-113). Preview and Source
    // are unchanged and the refusal is still on screen in Rust's words.
    const tabs = screen.getAllByRole("tab").map((tab) => tab.textContent);
    expect(tabs).toEqual(["Preview", "Source"]);
    expect(screen.getByText(WORKSPACE_REFUSAL)).toBeInTheDocument();
  });
});

/**
 * The owner's report, closed at the surface that showed it (Story 52.2, FR-302).
 *
 * *"zmiana title property zmienia nazwe teraz ale tez wyswietla '<path> is no
 * longer in tgdrive…' zamiast przeladowac plik z nowa nazwa"* — the rename
 * landed and the pane reported the file missing, because the panel told its host
 * only that something had changed and the host re-read the address the rename had
 * just emptied.
 *
 * **The assertion is the panel's target, not a rendered sentence.** What was
 * broken is WHERE this surface points after a rename, and the target is that,
 * exactly. The missing-file sentence is `panel-strip.tsx`'s and is asserted in
 * its own suite; reproducing it here would need the strip mounted around a
 * viewer this suite deliberately renders on its own (`openThroughTheRegistry`).
 */
describe("a rename in the properties panel takes the open pane with it (Story 52.2)", () => {
  /** A block with a title, which is the field a rename is committed from. */
  const TITLED = "---\ntitle: untitled\n---\n";

  /**
   * What Rust answers the rename with — and the point of the fixture: a
   * different DIRECTORY, and a filename that is not the title that was typed.
   * No string surgery on this side could compose it from `SESSION_README` plus
   * "Kick Off", so a panel target holding it can only have got it from the
   * command's return value, which is the whole of what AD-65 asks this half to
   * prove.
   */
  const MOVED = "60-sessions/archive/2026-02/kick-off-notes.md";

  /** Commit the title row, which is a blur — the panel's own suite's gesture. */
  async function retitle(next: string): Promise<void> {
    const field = await screen.findByRole("textbox", { name: "title" });
    fireEvent.change(field, { target: { value: next } });
    fireEvent.blur(field);
  }

  it("re-points the active panel at the subpath the rename answered with", async () => {
    syncReadText.mockResolvedValue(vm({ text: "# Session\n\nalpha\n" }));
    syncReadFrontmatter.mockResolvedValue(TITLED);
    sessionsFileRename.mockResolvedValue(MOVED);
    // The pane the reader is already in, showing the file about to be renamed —
    // without it there is no active panel to follow anything.
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "profile-1", relativePath: SESSION_README });
    openThroughTheRegistry(target({ name: "README.md", relativePath: SESSION_README }));
    await settle();

    await retitle("Kick Off");

    await waitFor(() => {
      const { panels, activeId } = panelsStore.getState();
      const active = panels.find((panel) => panel.id === activeId);
      expect(active?.target).toEqual({
        kind: "file",
        profileId: "profile-1",
        relativePath: MOVED,
      });
    });
  });

  it("moves the pane it was in rather than opening a second one", async () => {
    syncReadText.mockResolvedValue(vm({ text: "# Session\n\nalpha\n" }));
    syncReadFrontmatter.mockResolvedValue(TITLED);
    sessionsFileRename.mockResolvedValue(MOVED);
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "profile-1", relativePath: SESSION_README });
    const before = panelsStore.getState().panels.length;
    openThroughTheRegistry(target({ name: "README.md", relativePath: SESSION_README }));
    await settle();

    await retitle("Kick Off");

    await waitFor(() =>
      expect(panelsStore.getState().panels.map((panel) => panel.target)).toContainEqual({
        kind: "file",
        profileId: "profile-1",
        relativePath: MOVED,
      }),
    );
    // Following a file is a retarget, not an open: a second panel would leave the
    // emptied address on screen beside the file, which is the banner this story
    // exists to remove.
    expect(panelsStore.getState().panels).toHaveLength(before);
    expect(panelsStore.getState().panels.map((panel) => panel.target)).not.toContainEqual({
      kind: "file",
      profileId: "profile-1",
      relativePath: SESSION_README,
    });
  });

  /** Somewhere else in the same profile, for the pane that must not move. */
  const OTHER: PanelTargetVm = {
    kind: "file",
    profileId: "profile-1",
    relativePath: `${SESSION_DIR}/notes.md`,
  };

  /** The README as a panel target — what the pane showing this file holds. */
  const README: PanelTargetVm = {
    kind: "file",
    profileId: "profile-1",
    relativePath: SESSION_README,
  };

  /**
   * The sequence the owner will actually perform, and the one that made the
   * first cut of this feature a worse defect than the banner it removed.
   *
   * The title field commits on BLUR and a pane takes focus on `onMouseDown`
   * (`panel-strip.tsx`), so "type the new title, then click into the other pane"
   * runs the focus change FIRST and the commit second. Re-pointing "the active
   * panel" then moves the pane the reader has just clicked into — destroying what
   * it was showing — and leaves the pane they renamed from on the emptied
   * address, still rendering "is no longer in tgdrive". The rename is held open
   * across the focus change here rather than assumed to be, because that is what
   * the real round trip does.
   */
  it("moves the pane holding the file, not the one that took focus while the title was committing", async () => {
    syncReadText.mockResolvedValue(vm({ text: "# Session\n\nalpha\n" }));
    syncReadFrontmatter.mockResolvedValue(TITLED);
    let answer: (subpath: string) => void = () => {};
    sessionsFileRename.mockReturnValue(
      new Promise<string>((resolve) => {
        answer = resolve;
      }),
    );
    // Two panes: the left one on another file, the right one on the README the
    // reader is renaming from.
    panelsStore.getState().setActiveTarget(OTHER);
    panelsStore.getState().openPanel(README);
    const [left, right] = panelsStore.getState().panels;
    if (left === undefined || right === undefined) {
      throw new Error("expected two panels");
    }
    openThroughTheRegistry(target({ name: "README.md", relativePath: SESSION_README }));
    await settle();

    await retitle("Kick Off");
    // The click into the other pane, which is what a blur means when it is a
    // click and not a Tab.
    await act(async () => {
      panelsStore.getState().focusPanel(left.id);
    });
    await act(async () => {
      answer(MOVED);
    });

    await waitFor(() =>
      expect(panelsStore.getState().panels.map((panel) => panel.target)).toEqual([
        OTHER,
        { kind: "file", profileId: "profile-1", relativePath: MOVED },
      ]),
    );
    // And the pane the reader clicked into keeps focus as well as its document: a
    // rename is not a navigation.
    expect(panelsStore.getState().activeId).toBe(left.id);
  });

  /**
   * The same file in two panes — the shape a panel strip exists for, and the one
   * where "move the active panel" leaves a dead path behind with no second click
   * involved at all. The pane that did not follow persists that path to the
   * panels cookie, so the banner comes back after a restart too.
   */
  it("moves every pane holding the file, not only the focused one", async () => {
    syncReadText.mockResolvedValue(vm({ text: "# Session\n\nalpha\n" }));
    syncReadFrontmatter.mockResolvedValue(TITLED);
    sessionsFileRename.mockResolvedValue(MOVED);
    panelsStore.getState().setActiveTarget(README);
    panelsStore.getState().openPanel(OTHER);
    panelsStore.getState().setActiveTarget(README);
    expect(panelsStore.getState().panels.map((panel) => panel.target)).toEqual([README, README]);
    openThroughTheRegistry(target({ name: "README.md", relativePath: SESSION_README }));
    await settle();

    await retitle("Kick Off");

    const moved = { kind: "file", profileId: "profile-1", relativePath: MOVED };
    await waitFor(() =>
      expect(panelsStore.getState().panels.map((panel) => panel.target)).toEqual([moved, moved]),
    );
  });

  /**
   * The re-point is not a PREVIEW, and this is the gesture that proves it costs
   * something to pretend otherwise.
   *
   * `setActiveTarget` records {@link Panel.replaced} so the second click of a
   * double click can put back what the first displaced. Used for a rename it
   * records `was:` the path the rename just emptied — and `openPanel`'s restore
   * branch is live, so double-clicking the renamed file in the tree puts that dead
   * path back and opens the file beside it. The reader gets the banner they were
   * just spared, plus a second panel.
   */
  it("leaves no preview memory of the emptied path for a later double click to restore", async () => {
    syncReadText.mockResolvedValue(vm({ text: "# Session\n\nalpha\n" }));
    syncReadFrontmatter.mockResolvedValue(TITLED);
    sessionsFileRename.mockResolvedValue(MOVED);
    // Pinned, not previewed: this pane really holds the README.
    panelsStore.getState().openPanel(README);
    openThroughTheRegistry(target({ name: "README.md", relativePath: SESSION_README }));
    await settle();

    await retitle("Kick Off");
    const moved: PanelTargetVm = { kind: "file", profileId: "profile-1", relativePath: MOVED };
    await waitFor(() =>
      expect(panelsStore.getState().panels.map((panel) => panel.target)).toEqual([moved]),
    );

    // The double click, on the renamed row.
    panelsStore.getState().openPanel(moved);

    expect(panelsStore.getState().panels.map((panel) => panel.target)).toEqual([moved]);
  });

  it("offers no properties panel for a file in no profile, so no rename can strand it", async () => {
    syncReadFrontmatter.mockResolvedValue(TITLED);
    openThroughTheRegistry(
      target({ name: "loose.md", relativePath: "archive/loose.md", profileId: null }),
    );
    await settle();

    // The spec's one Block-if: with no profile there is no address to re-point,
    // and this surface answers it by having no rename to offer at all — not by
    // offering one whose success would re-read a path that had moved.
    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();
    expect(syncReadFrontmatter).not.toHaveBeenCalled();
    expect(sessionsFileRename).not.toHaveBeenCalled();
  });
});

/**
 * The block is drawn once, and saved whole (Story 52.3, FR-304).
 *
 * *"note tab w sesions nie musi renderowac czesci properties jak juz jest powyzej
 * formularz"* — on a file the buffer IS the whole file, so the YAML block was on
 * screen twice: once as the panel's controls, once as `---` lines in the reader's
 * own document.
 *
 * End to end here rather than in `text-file-frame.test.tsx`, deliberately: what
 * has to hold is that the frame's verdict REACHES the pane and that a save still
 * writes the block, and both need the real panel above a real editor over real
 * bytes. A frame test could only assert the prop it just passed.
 */
describe("the properties block is drawn once, not twice (Story 52.3)", () => {
  /** A file with properties, as `file_properties` writes them, and its body. */
  const BLOCK = "---\ntitle: Weekly\ntags:\n  - about\n---\n";
  const BODY = "# Weekly\n\nalpha\n";

  it("keeps the block out of Note mode and puts it back in the bytes a save writes", async () => {
    syncReadFrontmatter.mockResolvedValue(BLOCK);
    syncReadText.mockResolvedValue(vm({ text: BLOCK + BODY }));
    syncWriteEntry.mockResolvedValue(undefined);
    openThroughTheRegistry(target({ name: "README.md", relativePath: SESSION_README }));
    await settle();

    // The form is genuinely above the pane. Without this the test would pass over
    // a surface that had no panel at all, which is the case where the block SHOULD
    // be document text.
    expect(await screen.findByRole("region", { name: PROPERTIES_LABEL })).toBeInTheDocument();

    const editor = await editorHost();
    const view = liveView(editor);
    // Story 52.3's default put the reader here with no press, and what he is
    // looking at is his document — not his document with its own metadata pasted
    // at the top of it.
    expect(screen.getByRole("tab", { name: "Note" })).toHaveAttribute("aria-selected", "true");
    expect(view.state.doc.toString()).toBe(BODY);

    await act(async () => {
      view.dispatch({ selection: { anchor: view.state.doc.length } });
    });
    await typeAtCaret(view, "beta\n");
    fireEvent.keyDown(editor, { key: "s", ...MOD });
    await settle();

    // Byte for byte, through the same one write path: the block the reader was
    // never shown is still the first thing in the file. A save that wrote what the
    // pane was holding would delete the properties the form above it is editing.
    expect(syncWriteEntry).toHaveBeenCalledWith(
      "profile-1",
      SESSION_README,
      `${BLOCK}# Weekly\n\nalpha\nbeta\n`,
    );
  });

  it("still shows every byte on the Source tab, which is the file's characters", async () => {
    syncReadFrontmatter.mockResolvedValue(BLOCK);
    syncReadText.mockResolvedValue(vm({ text: BLOCK + BODY }));
    openThroughTheRegistry(target({ name: "README.md", relativePath: SESSION_README }));
    await settle();
    fireEvent.click(screen.getByRole("tab", { name: "Source" }));
    const view = liveView(await editorHost());

    // AD-88's one buffer, visible in full in the one view that is always the
    // source. Hiding anything here would be a lie about what Save writes.
    expect(view.state.doc.toString()).toBe(BLOCK + BODY);
  });
});
