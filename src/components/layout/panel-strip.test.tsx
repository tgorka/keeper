import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FOLD_STRIP_HEAD_SLOT, FOLD_STRIP_NAME_SLOT } from "@/components/layout/fold-strip";
import type { FilesEntryVm, FilesListingVm, NoteVaultVm } from "@/lib/ipc/client";

/** Test id for the stand-in the note panel's own suite explains. */
const EDITOR_STUB_TESTID = "note-editor-stub";

// The note editor COMPONENT, and nothing else in its module: see the
// note-panel describe at the bottom of this file for why this one component is
// a stub here when nothing else below the strip is. The stub renders the
// `frame` prop and nothing else, because the prop boundary is the whole of
// what this file claims about it — while `deriveTitle`, which the strip itself
// calls to name a folded note, stays the real function.
vi.mock(import("@/components/notes/note-editor"), async (importOriginal) => ({
  ...(await importOriginal()),
  NoteEditor: ({ frame }: { frame?: ReactNode }) => (
    <div data-testid={EDITOR_STUB_TESTID}>{frame}</div>
  ),
}));

// The IPC edge, and the one component above. Everything else below the strip —
// the panel store, the viewer registry, the unknown viewer — is the real
// module, because the defect this suite exists to catch is a panel that renders
// nothing, and a mocked body can never render nothing.
const syncBrowse = vi.fn();
const syncOpenEntry = vi.fn();
const revealPath = vi.fn();
const syncReadText = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  syncBrowse: (id: unknown, subpath: unknown) => syncBrowse(id, subpath),
  syncOpenEntry: (id: unknown, subpath: unknown) => syncOpenEntry(id, subpath),
  revealPath: (path: unknown) => revealPath(path),
  // Story 45.8 bound the `document` viewer, so a panel resolving a `.pdf`
  // through the registry now reaches its loader. Held pending: this file
  // asserts that the strip draws whatever the registry hands it, and a read
  // that never settles keeps that assertion about the strip rather than about
  // a document's contents.
  syncReadDocument: vi.fn(() => new Promise(() => undefined)),
  // A folded note panel reads the note once to name its spine. Resolved with
  // an empty body: naming a folded strip is its own story's claim, and this
  // one only needs the call not to explode when a note panel mounts.
  notesBodyRead: vi.fn(async () => ({ text: "", frontmatter: "", rev: "r0", path: null })),
  // Story 50.3's fix reads the listing row's write verdict and hands it to the
  // text viewer, so a `.md` panel now has something worth asserting. Unset by
  // default — `mockReset` leaves it answering `undefined`, which the loader
  // catches into its error sentence, exactly the state every panel test here
  // has always rendered for a `.md`.
  syncReadText: (id: unknown, subpath: unknown) => syncReadText(id, subpath),
  syncWriteEntry: vi.fn(async () => undefined),
  syncReadFrontmatter: vi.fn(async () => ""),
  syncWriteFrontmatter: vi.fn(async () => ""),
  tagsVocabulary: vi.fn(async () => ({ entries: [] })),
}));

import { EXPORT_FILE_LABEL } from "@/components/export/export-file-button";
import {
  PANEL_CLOSE_LABEL,
  PANEL_EMPTY_SENTENCE,
  PANEL_FOLD_LABEL,
  PANEL_NO_VAULT_SENTENCE,
  PANEL_REASON_TESTID,
  PANEL_RESOLVING_SENTENCE,
  PANEL_STRIP_LABEL,
  PANEL_TESTID,
  PANEL_UNFOLD_LABEL,
  PANEL_UNSUPPORTED_SENTENCE,
  PanelStrip,
  panelFileGoneSentence,
} from "@/components/layout/panel-strip";
import { DOCUMENT_VIEWER_TESTID } from "@/components/viewers/document-viewer";
import {
  MEDIA_VIEWER_ELEMENT_TESTID,
  MEDIA_VIEWER_FACTS_TESTID,
} from "@/components/viewers/media-viewer";
import { notesVaultsStore, resetNotesVaultsStoreForTest } from "@/lib/stores/notes-vaults";
import { panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";
import { UNKNOWN_VIEWER_OPEN_LABEL } from "@/lib/viewers";

/** The exact sentence Rust composes for a profile whose drive is out. Verbatim,
 * because the whole point of the state is that it reaches the screen unaltered. */
const DRIVE_IS_OUT =
  "/Volumes/merope/Field is not there. This folder lives on removable media — reattach the volume, then open it again.";

function entry(name: string, relativePath = name): FilesEntryVm {
  return {
    name,
    relativePath,
    absolutePath: `/Users/alice/Vault/${relativePath}`,
    kind: "file",
    sync: { status: "synced", detail: null },
    // Rust sends a write verdict on every row, so a fixture without one is a
    // fixture no listing could produce — and `FilePanelBody` reads it to tell the
    // viewer whether keeper manages this file (Story 46.14).
    write: {
      writable: false,
      reason: "This folder is outside a notes vault.",
      caveat: null,
      caveatShort: null,
    },
  } as FilesEntryVm;
}

function listed(subpath: string, entries: FilesEntryVm[]): FilesListingVm {
  return {
    profileId: "p1",
    subpath,
    // 45.3's create-in-here verdict. These fixtures are panel-rendering
    // fixtures, so the location deliberately refuses: a panel must render a
    // listing identically whether or not the folder happens to be writable.
    write: {
      writable: false,
      reason: "This folder is outside a notes vault.",
      caveat: null,
      caveatShort: null,
    },
    state: "listed",
    entries,
    detail: null,
    truncated: false,
  };
}

/** Render and let the resolution the mount started settle. */
async function mount(): Promise<void> {
  render(<PanelStrip />);
  await act(async () => {
    await Promise.resolve();
  });
}

function clearCookies(): void {
  for (const part of document.cookie.split(";")) {
    const name = part.split("=")[0]?.trim();
    if (name !== undefined && name !== "") {
      // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
      document.cookie = `${name}=; path=/; max-age=0`;
    }
  }
}

beforeEach(() => {
  clearCookies();
  resetPanelsStoreForTest();
  syncBrowse.mockReset();
  syncOpenEntry.mockReset();
  revealPath.mockReset();
  syncReadText.mockReset();
});

describe("the panel strip", () => {
  it("says what it is showing when nothing has been opened", async () => {
    await mount();

    expect(screen.getByLabelText(PANEL_STRIP_LABEL)).toBeInTheDocument();
    expect(screen.getByText(PANEL_EMPTY_SENTENCE)).toBeInTheDocument();
  });

  it("draws a resolved file through the registry rather than deciding itself", async () => {
    syncBrowse.mockResolvedValue(listed("docs", [entry("report.pdf", "docs/report.pdf")]));
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "docs/report.pdf" });

    await mount();

    // It listed the file's OWN FOLDER, not the profile root: `sync_browse` is
    // the one directory reader and it carries the containment rule.
    expect(syncBrowse).toHaveBeenCalledWith("p1", "docs");

    // A `.pdf` resolves to the `document` viewer, which Story 45.8 bound — so
    // the panel draws a document rather than a placeholder. The read is held
    // pending by the mock, so what is asserted is that the STRIP mounted what
    // the registry chose, which is this test's subject; whether the document
    // then draws pages is 45.8's own suite.
    //
    // Before 45.8 this asserted the unknown viewer, and the assertion was
    // right then for the same reason it is right now: the strip renders
    // whatever the registry hands back and holds no opinion of its own.
    const only = panelsStore.getState().panels[0];
    if (only === undefined) {
      throw new Error("expected one panel");
    }
    await waitFor(() => expect(screen.getByTestId(DOCUMENT_VIEWER_TESTID)).toBeInTheDocument());
    const frame = screen.getByTestId(`${PANEL_TESTID}-${only.id}`);
    expect(within(frame).getByTestId(DOCUMENT_VIEWER_TESTID)).toBeInTheDocument();
    // And the frame names the panel for a reader jumping between them.
    expect(frame).toHaveAttribute("aria-label", "report.pdf");
  });

  it("hands the viewer the row's write verdict, so a refused file opens read-only", async () => {
    // **The wire Story 50.3's fix turns on, and the one nothing else can see.**
    // `FilePanelBody` composes the `ViewerFile`, and every viewer test builds
    // its own — so a build where this function dropped `writeRefusal` would
    // leave the frame's own suite green while a session's `workspace/` file got
    // the toolbar, the slash menu and a Save button back. Asserted here in both
    // directions, over one sentence Rust composed.
    const fence =
      "60-sessions/active/2026-08-10-keeper/workspace/notes.md is inside a session's workspace " +
      "— scratch that is not versioned, not synced, and dies with the session. keeper reads it " +
      "but never writes there; promote the file into the session's artifacts instead.";
    syncReadText.mockResolvedValue({
      text: "# Scratch\n",
      sizeBytes: 10,
      sizeLabel: "10 bytes",
      oversize: false,
      binary: false,
      detail: null,
    });
    syncBrowse.mockResolvedValue(
      listed("60-sessions/active/2026-08-10-keeper/workspace", [
        {
          ...entry("notes.md", "60-sessions/active/2026-08-10-keeper/workspace/notes.md"),
          write: { writable: false, reason: fence, caveat: null, caveatShort: null },
        },
      ]),
    );
    panelsStore.getState().setActiveTarget({
      kind: "file",
      profileId: "p1",
      relativePath: "60-sessions/active/2026-08-10-keeper/workspace/notes.md",
    });

    await mount();

    // Rust's sentence, verbatim, and no Save to press.
    expect(await screen.findByText(fence)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
  });

  it("hands the viewer no refusal for a row keeper will write, so Save is there", async () => {
    // The other direction, without which the assertion above passes on a build
    // that hard-codes a refusal for every file.
    syncReadText.mockResolvedValue({
      text: "# Notes\n",
      sizeBytes: 9,
      sizeLabel: "9 bytes",
      oversize: false,
      binary: false,
      detail: null,
    });
    syncBrowse.mockResolvedValue(
      listed("docs", [
        {
          ...entry("notes.md", "docs/notes.md"),
          write: { writable: true, reason: null, caveat: null, caveatShort: null },
        },
      ]),
    );
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "docs/notes.md" });

    await mount();

    expect(await screen.findByRole("button", { name: "Save" })).toBeInTheDocument();
  });

  it("hands a media viewer the path the listing produced, not the file's name", async () => {
    // **The seam nothing pressed.** Every media test in Story 45.7 builds its
    // own `ViewerFile` and asks the registry directly, so all of them would
    // still pass if THIS function put `entry.name` where `relativePath`
    // belongs — measured, not supposed: that mutation survived the whole
    // sweep. The consequence is not cosmetic. A file in a subfolder would be
    // addressed as if it sat at the profile root, `browse::resolve` would find
    // nothing there, and every video, image and audio inside a folder would
    // 404. So the assertion is over the composed URL, which is the only place
    // the two spellings differ.
    syncBrowse.mockResolvedValue(
      listed("2026/08", [
        {
          ...entry("screen-0000.mov", "2026/08/screen-0000.mov"),
          kind: "video",
        },
      ]),
    );
    panelsStore.getState().setActiveTarget({
      kind: "file",
      profileId: "p1",
      relativePath: "2026/08/screen-0000.mov",
    });

    await mount();

    const player = await waitFor(() => screen.getByTestId(MEDIA_VIEWER_ELEMENT_TESTID));
    expect(player.getAttribute("src")).toBe("keeper-file://p1/2026/08/screen-0000.mov");
    // And the absolute path the listing carries reaches no attribute (FR-145).
    // Witnessed first: an absence over a literal is hollow unless something
    // asserts the literal was ever in the input, and `entry()`'s absolute path
    // is a helper this test does not own (Story 45.20's shape).
    expect(entry("x", "2026/08/x").absolutePath).toContain("/Users/alice/Vault");
    expect(document.body.innerHTML).not.toContain("/Users/alice/Vault");
  });

  it("hands the viewer the whole payload it builds, not just a resolvable path", async () => {
    // **Entry-point distribution, not test count.** Story 45.7 has twenty-five
    // media tests and every one of them builds its own `ViewerFile`; the test
    // above was the first to enter through the PANEL, and it pinned only the
    // path. Probed the rest of what this function composes and three more
    // mutations survived: dropping `openWith`, dropping `sizeLabel`, and
    // pointing the opener at `entry.name` instead of the relative path.
    //
    // None is cosmetic. Without `openWith` the placeholder for a file this
    // machine cannot decode says "hand it to the application that owns it" and
    // offers no way to — the remedy named and withheld. Without `sizeLabel`
    // Story 45.5's whole sentence is undone at the panel. And an opener aimed
    // at the name refuses for every file in a subfolder, which is the same
    // works-at-the-root failure as the path bug and reads as a mystery.
    // A promise, because the placeholder does `.catch()` on what the opener
    // returns. The suite's shared `vi.fn()` returns `undefined`, and pressing
    // the button against it throws an unhandled rejection that vitest reports
    // while still passing the test — a green run with an error in it.
    syncOpenEntry.mockResolvedValue(undefined);
    syncBrowse.mockResolvedValue(
      listed("2026/08", [
        {
          ...entry("camera-0000.mkv", "2026/08/camera-0000.mkv"),
          kind: "video",
          size: { bytes: 4300000, label: "4.3 MB" },
        },
      ]),
    );
    panelsStore.getState().setActiveTarget({
      kind: "file",
      profileId: "p1",
      relativePath: "2026/08/camera-0000.mkv",
    });

    await mount();

    // The size Rust formatted reaches the viewer's own facts line.
    const player = await waitFor(() => screen.getByTestId(MEDIA_VIEWER_ELEMENT_TESTID));
    expect(screen.getByTestId(MEDIA_VIEWER_FACTS_TESTID)).toHaveTextContent("4.3 MB");

    // Then press the button. A decode failure is the state where the opener is
    // the entire remedy, so it is the state worth pressing it in.
    Object.defineProperty(player, "error", { configurable: true, value: { code: 3 } });
    fireEvent(player, new Event("error"));
    fireEvent.click(await screen.findByRole("button", { name: UNKNOWN_VIEWER_OPEN_LABEL }));

    // The opener is aimed at the path the listing produced, not the file name.
    expect(syncOpenEntry).toHaveBeenCalledWith("p1", "2026/08/camera-0000.mkv");
  });

  it("renders Rust's own reason when the drive is out, and keeps the panel", async () => {
    syncBrowse.mockResolvedValue({
      profileId: "p1",
      subpath: "docs",
      write: {
        writable: false,
        reason: "This folder is outside a notes vault.",
        caveat: null,
        caveatShort: null,
      },
      state: "mediaAbsent",
      entries: null,
      detail: DRIVE_IS_OUT,
      truncated: false,
    } satisfies FilesListingVm);
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "docs/report.pdf" });

    await mount();

    await waitFor(() =>
      expect(screen.getByTestId(PANEL_REASON_TESTID)).toHaveTextContent(DRIVE_IS_OUT),
    );
    // The panel keeps its place, so it comes back when the drive does. A strip
    // that dropped it would need the user to find the file again.
    expect(panelsStore.getState().panels).toHaveLength(1);
    expect(panelsStore.getState().panels[0]?.target).toEqual({
      kind: "file",
      profileId: "p1",
      relativePath: "docs/report.pdf",
    });
  });

  it("trusts the state over the entry list, so an unreadable folder never reads as empty", async () => {
    // `FilesListingVm` promises `entries` is non-null exactly under `listed`,
    // and the panel checks the STATE as well rather than only unwrapping. An
    // empty array and a null are different values in TypeScript, and a surface
    // that branched on the array alone would tell someone their moved folder
    // is simply missing this file — the exact confusion the VM's own doc says
    // the two fields exist to prevent.
    syncBrowse.mockResolvedValue({
      profileId: "p1",
      subpath: "docs",
      state: "missing",
      entries: [],
      detail: "That folder is not on disk any more.",
      truncated: false,
    } as unknown as FilesListingVm);
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "docs/report.pdf" });

    await mount();

    await waitFor(() =>
      expect(screen.getByTestId(PANEL_REASON_TESTID)).toHaveTextContent(
        "That folder is not on disk any more.",
      ),
    );
  });

  it("names the file when its folder listed and it was not in it", async () => {
    syncBrowse.mockResolvedValue(listed("docs", [entry("other.md", "docs/other.md")]));
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "docs/report.pdf" });

    await mount();

    await waitFor(() =>
      expect(screen.getByTestId(PANEL_REASON_TESTID)).toHaveTextContent(
        panelFileGoneSentence("report.pdf"),
      ),
    );
  });

  it("shows the message Rust composed when the listing call is refused", async () => {
    syncBrowse.mockRejectedValue({ code: "not_found", message: "That folder is not set up." });
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "gone", relativePath: "a.md" });

    await mount();

    await waitFor(() =>
      expect(screen.getByTestId(PANEL_REASON_TESTID)).toHaveTextContent(
        "That folder is not set up.",
      ),
    );
  });

  it("says so for a target no viewer has been built for yet", async () => {
    // Nothing in wave 1 opens a recording, and the vocabulary carries one. A
    // blank pane is the defect DW-172 shipped; a sentence is not.
    panelsStore.getState().setActiveTarget({ kind: "recording", sessionId: "sess-1" });

    await mount();

    expect(screen.getByText(PANEL_UNSUPPORTED_SENTENCE)).toBeInTheDocument();
  });
});

describe("the panel strip's close control", () => {
  it("is absent on the last panel, because closing it is refused", async () => {
    syncBrowse.mockResolvedValue(listed("", [entry("a.md")]));
    panelsStore.getState().setActiveTarget({ kind: "file", profileId: "p1", relativePath: "a.md" });

    await mount();

    expect(screen.queryByRole("button", { name: PANEL_CLOSE_LABEL })).not.toBeInTheDocument();
  });

  it("closes a panel and leaves the rest in place", async () => {
    syncBrowse.mockResolvedValue(listed("", [entry("a.md"), entry("b.md")]));
    const state = panelsStore.getState();
    state.setActiveTarget({ kind: "file", profileId: "p1", relativePath: "a.md" });
    state.openPanel({ kind: "file", profileId: "p1", relativePath: "b.md" });

    await mount();
    const first = panelsStore.getState().panels[0];
    if (first === undefined) {
      throw new Error("expected two panels");
    }
    const frame = screen.getByTestId(`${PANEL_TESTID}-${first.id}`);

    await act(async () => {
      fireEvent.click(within(frame).getByRole("button", { name: PANEL_CLOSE_LABEL }));
      await Promise.resolve();
    });

    expect(panelsStore.getState().panels).toHaveLength(1);
    expect(panelsStore.getState().panels[0]?.target).toEqual({
      kind: "file",
      profileId: "p1",
      relativePath: "b.md",
    });
    expect(screen.queryByTestId(`${PANEL_TESTID}-${first.id}`)).not.toBeInTheDocument();
  });

  it("focuses a panel the pointer lands in, so the next single click replaces that one", async () => {
    syncBrowse.mockResolvedValue(listed("", [entry("a.md"), entry("b.md")]));
    const state = panelsStore.getState();
    state.setActiveTarget({ kind: "file", profileId: "p1", relativePath: "a.md" });
    state.openPanel({ kind: "file", profileId: "p1", relativePath: "b.md" });

    await mount();
    const first = panelsStore.getState().panels[0];
    if (first === undefined) {
      throw new Error("expected two panels");
    }

    await act(async () => {
      fireEvent.mouseDown(screen.getByTestId(`${PANEL_TESTID}-${first.id}`));
      await Promise.resolve();
    });

    expect(panelsStore.getState().activeId).toBe(first.id);
    panelsStore.getState().setActiveTarget({ kind: "file", profileId: "p1", relativePath: "c.md" });
    expect(panelsStore.getState().panels.map((panel) => panel.target?.kind)).toEqual([
      "file",
      "file",
    ]);
    expect(panelsStore.getState().panels[0]?.target).toEqual({
      kind: "file",
      profileId: "p1",
      relativePath: "c.md",
    });
  });

  it("focuses a panel from the pointerdown, which a cancelled press is all there is", async () => {
    // A surface inside a panel may cancel its own `pointerdown` — the task board
    // does, to stop WebKit anchoring a text selection under a drag — and a
    // cancelled `pointerdown` fires NO `mousedown` at all, on any platform. It
    // suppresses the default focus action too, so no focus event arrives either.
    // With the focus taken on `mousedown` alone, pressing a card in an unfocused
    // panel left `activeId` on the panel it was before, and the card's own `open()`
    // retargeted THAT panel: the note opened over whatever the other panel held.
    //
    // jsdom synthesises no compatibility mouse events, so a `pointerdown` alone is
    // exactly the sequence a cancelled press produces in the app.
    syncBrowse.mockResolvedValue(listed("", [entry("a.md"), entry("b.md")]));
    const state = panelsStore.getState();
    state.setActiveTarget({ kind: "file", profileId: "p1", relativePath: "a.md" });
    state.openPanel({ kind: "file", profileId: "p1", relativePath: "b.md" });

    await mount();
    const first = panelsStore.getState().panels[0];
    if (first === undefined) {
      throw new Error("expected two panels");
    }
    expect(panelsStore.getState().activeId).not.toBe(first.id);

    await act(async () => {
      fireEvent.pointerDown(screen.getByTestId(`${PANEL_TESTID}-${first.id}`));
      await Promise.resolve();
    });

    expect(panelsStore.getState().activeId).toBe(first.id);
    // Which is the whole point of the focus: the next single click replaces THIS
    // panel rather than the one that happened to be focused before.
    panelsStore.getState().setActiveTarget({ kind: "file", profileId: "p1", relativePath: "c.md" });
    expect(panelsStore.getState().panels[0]?.target).toEqual({
      kind: "file",
      profileId: "p1",
      relativePath: "c.md",
    });
  });
});

describe("the panel strip's fold control", () => {
  /** Open two file panels and return their ids, left to right. */
  async function twoPanels(): Promise<[string, string]> {
    syncBrowse.mockResolvedValue(listed("", [entry("a.md"), entry("b.md")]));
    const state = panelsStore.getState();
    state.setActiveTarget({ kind: "file", profileId: "p1", relativePath: "a.md" });
    state.openPanel({ kind: "file", profileId: "p1", relativePath: "b.md" });
    await mount();
    const [first, second] = panelsStore.getState().panels;
    if (first === undefined || second === undefined) {
      throw new Error("expected two panels");
    }
    return [first.id, second.id];
  }

  it("folds a panel away, and the body goes with it", async () => {
    const [firstId] = await twoPanels();
    const frame = screen.getByTestId(`${PANEL_TESTID}-${firstId}`);
    // The body really was there first, so its absence below is the fold's doing
    // and not a panel that never resolved. ONE child, since Story 53.3: this is
    // a `.md`, whose viewer draws the panel's row itself, so the panel's only
    // child is the body — and the row inside it is the body's, which is why the
    // count below going to two is the head and the spine rather than a header
    // that survived.
    expect(frame.children).toHaveLength(1);
    expect(within(frame).getByRole("button", { name: PANEL_FOLD_LABEL })).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(within(frame).getByRole("button", { name: PANEL_FOLD_LABEL }));
      await Promise.resolve();
    });

    const folded = screen.getByTestId(`${PANEL_TESTID}-${firstId}`);
    // The body is UNMOUNTED, not hidden. A body kept alive behind `hidden` would
    // keep its listing, its subscription and its editor buffer — the cost the
    // reader was reclaiming — and for a note panel it would hold a document
    // mirror open over a note nobody can see. Asserted as the section's shape
    // rather than by looking for the file's name: a viewer that happened to draw
    // nothing would satisfy the second and not the first.
    //
    // Two children, and neither is a body: the head band, and the panel's name
    // written down the strip (`FoldStripName`). The name is the whole reason a
    // folded panel is not a mystery glyph, so it is asserted by slot here
    // rather than counted away.
    expect(folded.firstElementChild).toHaveAttribute("data-slot", FOLD_STRIP_HEAD_SLOT);
    expect(folded.lastElementChild).toHaveAttribute("data-slot", FOLD_STRIP_NAME_SLOT);
    expect(folded.children).toHaveLength(2);
    expect(folded.lastElementChild?.textContent).toBe("a.md");
    // And it stops taking a share of the strip's width, which is the visible
    // point of folding: the neighbours get it.
    expect(folded).not.toHaveClass("flex-1");
    expect(folded).toHaveAttribute("data-folded", "true");
    // The panel is still named, so a reader moving between panels can still tell
    // which one this is with nothing on screen but a button.
    expect(folded).toHaveAttribute("aria-label", "a.md");
  });

  it("offers the way back in, and takes it", async () => {
    const [firstId] = await twoPanels();
    await act(async () => {
      fireEvent.click(
        within(screen.getByTestId(`${PANEL_TESTID}-${firstId}`)).getByRole("button", {
          name: PANEL_FOLD_LABEL,
        }),
      );
      await Promise.resolve();
    });

    const folded = screen.getByTestId(`${PANEL_TESTID}-${firstId}`);
    // Folded, the control names the panel as well as the act — it is the only
    // thing left on screen for this panel, so a pointer that hovered a bare
    // chevron would learn nothing about WHICH panel it was unfolding.
    const unfold = within(folded).getByRole("button", { name: `${PANEL_UNFOLD_LABEL}: a.md` });
    // The name says which way the control goes; `aria-expanded` says where it is.
    expect(unfold).toHaveAttribute("aria-expanded", "false");
    // The tooltip and the name are the same words, so speech input can say what
    // a pointer reads (WCAG 2.5.3).
    expect(unfold).toHaveAttribute("title", `${PANEL_UNFOLD_LABEL}: a.md`);
    // Nothing else is on screen for this panel — no Close, no Export, no name.
    expect(within(folded).getAllByRole("button")).toHaveLength(1);

    await act(async () => {
      fireEvent.click(unfold);
      await Promise.resolve();
    });

    const open = screen.getByTestId(`${PANEL_TESTID}-${firstId}`);
    expect(open).not.toHaveAttribute("data-folded");
    expect(within(open).getByRole("button", { name: PANEL_FOLD_LABEL })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(within(open).getByText("a.md")).toBeInTheDocument();
  });

  it("offers to fold the last panel, which it refuses to close", async () => {
    syncBrowse.mockResolvedValue(listed("", [entry("a.md")]));
    panelsStore.getState().setActiveTarget({ kind: "file", profileId: "p1", relativePath: "a.md" });

    await mount();

    // The asymmetry, on screen: a control that refuses on activation is worse
    // than no control, so Close is absent — and Fold is not, because the control
    // that undoes it sits exactly where the panel was.
    expect(screen.queryByRole("button", { name: PANEL_CLOSE_LABEL })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: PANEL_FOLD_LABEL })).toBeInTheDocument();
  });

  it("shows a folded panel what it is given, rather than loading it out of sight", async () => {
    const [firstId] = await twoPanels();
    await act(async () => {
      fireEvent.click(
        within(screen.getByTestId(`${PANEL_TESTID}-${firstId}`)).getByRole("button", {
          name: PANEL_FOLD_LABEL,
        }),
      );
      await Promise.resolve();
    });
    // Focus it the way a pointer would, so the next single click in a browser
    // lands here — folded.
    await act(async () => {
      fireEvent.mouseDown(screen.getByTestId(`${PANEL_TESTID}-${firstId}`));
      await Promise.resolve();
    });

    await act(async () => {
      panelsStore
        .getState()
        .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "b.md" });
      await Promise.resolve();
    });

    // The epic's own defect, refused: keeper read the file, put it in this panel,
    // and the reader would have seen nothing at all.
    const frame = screen.getByTestId(`${PANEL_TESTID}-${firstId}`);
    expect(frame).not.toHaveAttribute("data-folded");
    expect(within(frame).getByText("b.md")).toBeInTheDocument();
  });
});

/**
 * A note panel gives up its own header row (Story 50.1).
 *
 * The owner's report is two header bands over one note: this frame's, whose
 * whole content was the word `Note` plus a fold and a close, and the editor's
 * underneath it. The frame now draws nothing and hands its two controls down.
 *
 * **The editor is stubbed here and nowhere else in this file, and the stub is
 * the point rather than a shortcut.** What this file owes is the PROP
 * BOUNDARY, the way `capture-window.test.tsx` owes it for the same component:
 * that the frame stops drawing a row and that its real fold and its real close
 * — composed here, by this file, with this file's labels — arrive at the thing
 * below. That they then land in a group of the editor's header that never
 * overflows into the note's menu is `note-editor.test.tsx`'s claim, asserted
 * over the real editor. Mounting the real one here would drag a second
 * story's IPC surface into a suite whose `@/lib/ipc/client` mock has four
 * functions in it.
 */
describe("a note in a panel", () => {
  beforeEach(() => {
    resetNotesVaultsStoreForTest();
  });

  /** The vault the note lives in, as the store mirrors one. */
  function seedVault(): void {
    notesVaultsStore.getState().setVaults([{ id: "v1" } as NoteVaultVm]);
  }

  async function openNotePanel(): Promise<void> {
    panelsStore.getState().setActiveTarget({ kind: "note", vaultId: "v1", noteId: "n1" });
    await mount();
  }

  it("draws no header of its own, and hands its controls to the editor", async () => {
    seedVault();
    await openNotePanel();

    // The band that carried the word `Note` is gone, and with it the seam
    // under it: one 40px row reclaimed over every note opened in a panel.
    expect(document.querySelectorAll("header")).toHaveLength(0);
    expect(screen.queryByText("Note")).not.toBeInTheDocument();
    // And the panel's two controls are not gone with it — they went down.
    const editor = screen.getByTestId(EDITOR_STUB_TESTID);
    expect(within(editor).getByRole("button", { name: PANEL_FOLD_LABEL })).toBeInTheDocument();
  });

  it("still folds and closes from the controls it handed down", async () => {
    seedVault();
    panelsStore.getState().setActiveTarget({ kind: "note", vaultId: "v1", noteId: "n1" });
    panelsStore.getState().setActiveTarget({ kind: "note", vaultId: "v1", noteId: "n2" });
    panelsStore.getState().openPanel({ kind: "note", vaultId: "v1", noteId: "n2" });
    await mount();

    const frames = screen.getAllByTestId(EDITOR_STUB_TESTID);
    const first = frames[0] as HTMLElement;
    await act(async () => {
      fireEvent.click(within(first).getByRole("button", { name: PANEL_FOLD_LABEL }));
      await Promise.resolve();
    });

    // A control passed through two components is a control that can arrive
    // rendered and dead. This is the press, and the fold is the effect.
    expect(panelsStore.getState().panels[0]?.folded).toBe(true);
  });

  it("keeps its own row for a note whose vault is gone", async () => {
    // No `seedVault`: the store is hydrated with a list this vault is not in,
    // so the body is a sentence rather than an editor — and a sentence cannot
    // carry a fold or a close. This is the case that makes the frame's
    // decision and the body's one rule instead of two.
    notesVaultsStore.getState().setVaults([]);
    await openNotePanel();

    expect(screen.getByTestId(PANEL_REASON_TESTID)).toHaveTextContent(PANEL_NO_VAULT_SENTENCE);
    expect(document.querySelectorAll("header")).toHaveLength(1);
    expect(screen.getByRole("button", { name: PANEL_FOLD_LABEL })).toBeInTheDocument();
  });
});

/**
 * A file panel gives up its own header row too (Story 53.3, FR-317).
 *
 * The owner's report is the same one 50.1 answered for notes, one surface along:
 * the file's name is on this frame's row AND on the save bar the viewer draws
 * under it. So the panel hands its Export, its fold and its close down and draws
 * nothing, exactly as it does for a note.
 *
 * **What is asserted here is the DECISION and the prop boundary**, not what the
 * merged row contains — that is `text-file-frame.test.tsx`'s, over a frame handed
 * a real `frame` node. What only this suite can see is that the panel consults
 * the registry's `ownsHostRow` rather than guessing: the viewer is real here, so
 * a `.pdf` and a listing that has not landed are cases a stub could not produce.
 */
describe("a file in a panel", () => {
  /** One savable markdown file, resolved, with the loader answering. */
  async function openSavableFile(): Promise<void> {
    syncReadText.mockResolvedValue({
      text: "# Notes\n",
      sizeBytes: 9,
      sizeLabel: "9 bytes",
      oversize: false,
      binary: false,
      detail: null,
    });
    syncBrowse.mockResolvedValue(
      listed("docs", [
        {
          ...entry("notes.md", "docs/notes.md"),
          write: { writable: true, reason: null, caveat: null, caveatShort: null },
        },
      ]),
    );
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "docs/notes.md" });
    await mount();
  }

  it("draws no row of its own, and hands Export, fold and close to the viewer", async () => {
    await openSavableFile();

    // ONE header for this panel, and it is the frame's own: the band that
    // carried the file's name a second time is gone.
    await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeInTheDocument());
    expect(document.querySelectorAll("header")).toHaveLength(1);
    const only = panelsStore.getState().panels[0];
    if (only === undefined) {
      throw new Error("expected one panel");
    }
    const panel = screen.getByTestId(`${PANEL_TESTID}-${only.id}`);
    // The panel's own children are the body and nothing else — the row is inside
    // it now, drawn by the frame that draws the Save button.
    expect(panel.children).toHaveLength(1);
    // And all three controls went down. Export travels for a file where it does
    // not for a note: it reads the bytes off the disk, and for a note the editor
    // is the surface that can flush the buffer first.
    const row = document.querySelector("header") as HTMLElement;
    expect(within(row).getByRole("button", { name: EXPORT_FILE_LABEL })).toBeInTheDocument();
    expect(within(row).getByRole("button", { name: PANEL_FOLD_LABEL })).toBeInTheDocument();
    expect(within(row).getByText("notes.md")).toBeInTheDocument();
  });

  it("still folds from the control it handed down", async () => {
    await openSavableFile();
    await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeInTheDocument());

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: PANEL_FOLD_LABEL }));
      await Promise.resolve();
    });

    // A control passed through three components is a control that can arrive
    // rendered and dead. This is the press, and the fold is the effect.
    expect(panelsStore.getState().panels[0]?.folded).toBe(true);
  });

  it("keeps its own row for a file whose viewer draws none", async () => {
    // A `.pdf` resolves to the document viewer, which draws no chrome at all. If
    // the panel gave its row up for every file, this one would have no title, no
    // fold and no close — which is the naive port of 50.1, and the reason the
    // decision reads the registry's promise rather than the target's kind.
    syncBrowse.mockResolvedValue(listed("docs", [entry("report.pdf", "docs/report.pdf")]));
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "docs/report.pdf" });

    await mount();

    await waitFor(() => expect(screen.getByTestId(DOCUMENT_VIEWER_TESTID)).toBeInTheDocument());
    expect(document.querySelectorAll("header")).toHaveLength(1);
    const row = document.querySelector("header") as HTMLElement;
    expect(within(row).getByText("report.pdf")).toBeInTheDocument();
    expect(within(row).getByRole("button", { name: PANEL_FOLD_LABEL })).toBeInTheDocument();
  });

  it("keeps its own row for a file its folder does not have", async () => {
    // The other headerless state a merge could strand: the listing landed and
    // the file was not in it, so the body is a sentence. A sentence carries no
    // fold and no close.
    syncBrowse.mockResolvedValue(listed("docs", [entry("other.md", "docs/other.md")]));
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "docs/notes.md" });

    await mount();

    await waitFor(() =>
      expect(screen.getByTestId(PANEL_REASON_TESTID)).toHaveTextContent(
        panelFileGoneSentence("notes.md"),
      ),
    );
    expect(document.querySelectorAll("header")).toHaveLength(1);
    expect(screen.getByRole("button", { name: PANEL_FOLD_LABEL })).toBeInTheDocument();
  });

  it("keeps its own row while the folder is still answering", async () => {
    // The first frame, before `sync_browse` resolves. Nothing has promised to
    // draw a row yet, so the panel draws it — and a panel with no title for the
    // whole of a pendrive's read is the state this guard exists for.
    //
    // Held open, never settled: a listing that answered would end the state
    // under test. The executor form because this project's `lib` is below
    // es2024, which is why `syncReadDocument`'s mock at the top of this file
    // spells it the same way.
    syncBrowse.mockReturnValue(new Promise<FilesListingVm>(() => undefined));
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "docs/notes.md" });

    await mount();

    expect(screen.getByTestId(PANEL_REASON_TESTID)).toHaveTextContent(PANEL_RESOLVING_SENTENCE);
    expect(document.querySelectorAll("header")).toHaveLength(1);
    expect(screen.getByRole("button", { name: PANEL_FOLD_LABEL })).toBeInTheDocument();
  });

  it("reads no folder for a panel that is folded", async () => {
    // The resolution moved up into the frame, which is above the body the fold
    // unmounts — so the thing that used to stop this by construction no longer
    // does, and the hook has to hold the rule itself. A folded panel that kept
    // listing a directory would spend a pendrive read on a panel nobody can see.
    await openSavableFile();
    await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeInTheDocument());
    syncBrowse.mockClear();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: PANEL_FOLD_LABEL }));
      await Promise.resolve();
    });

    expect(syncBrowse).not.toHaveBeenCalled();
    // And the way back is still there, on the folded strip's own band.
    expect(
      screen.getByRole("button", { name: `${PANEL_UNFOLD_LABEL}: notes.md` }),
    ).toBeInTheDocument();
  });
});
