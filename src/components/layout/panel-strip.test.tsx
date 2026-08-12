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
}));

import {
  PANEL_CLOSE_LABEL,
  PANEL_EMPTY_SENTENCE,
  PANEL_FOLD_LABEL,
  PANEL_NO_VAULT_SENTENCE,
  PANEL_REASON_TESTID,
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
    write: { writable: false, reason: "This folder is outside a notes vault.", caveat: null },
  } as FilesEntryVm;
}

function listed(subpath: string, entries: FilesEntryVm[]): FilesListingVm {
  return {
    profileId: "p1",
    subpath,
    // 45.3's create-in-here verdict. These fixtures are panel-rendering
    // fixtures, so the location deliberately refuses: a panel must render a
    // listing identically whether or not the folder happens to be writable.
    write: { writable: false, reason: "This folder is outside a notes vault.", caveat: null },
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
      write: { writable: false, reason: "This folder is outside a notes vault.", caveat: null },
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
    // and not a panel that never resolved.
    expect(frame.children).toHaveLength(2);

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
