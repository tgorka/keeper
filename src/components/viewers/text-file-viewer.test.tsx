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
import { EditorView } from "@codemirror/view";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteCsvVm, NoteFolderVm, NoteRefVm, NoteRowVm, TextFileVm } from "@/lib/ipc/client";
import { withRangeRects } from "@/test/layout";

const syncReadText = vi.fn<(profileId: string, subpath: string) => Promise<TextFileVm>>();
const syncWriteEntry = vi.fn<(profileId: string, subpath: string, text: string) => Promise<void>>();
const notesCsvRead = vi.fn<(vaultId: string, target: string) => Promise<NoteCsvVm>>();
const notesTree = vi.fn<(vaultId: string, relDir: string) => Promise<NoteFolderVm>>();
const notesResolveLink = vi.fn<(vaultId: string, target: string) => Promise<NoteRefVm | null>>();
const notesVaultSetActive = vi.fn<(vaultId: string) => Promise<void>>();

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
}));

import type { NoteVaultVm } from "@/lib/ipc/client";
import { notesVaultsStore, resetNotesVaultsStoreForTest } from "@/lib/stores/notes-vaults";
import { panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { type ViewerFile, viewerComponentFor } from "@/lib/viewers";
import { TEXT_FILE_CAVEAT_TESTID } from "./text-file-frame";
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
 * The retry is not politeness about timing, it is required: the preview is
 * mounted by an async `import()` and is torn down and rebuilt when the loaded
 * text arrives, so a node captured from the first mount is detached by the
 * second and a `mouseDown` on it reaches no handler at all — silently, because
 * a detached node still accepts events. Re-querying inside the retry is what
 * makes the press land on the view that is actually on screen.
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
