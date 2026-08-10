/**
 * Story 45.21 — the two doors into an export, each driven through its real host.
 *
 * There are exactly two ways a person reaches this feature: the note editor's
 * Actions menu, and a file panel's header. Both are counted here on purpose —
 * wave 2 lost two headline defects to a suite whose tests all entered through
 * one surface, and a component tested only through a hand-built mount has never
 * been checked for what its host actually hands it.
 *
 * The file door is driven through the real `PanelStrip`, not through
 * `ExportFileButton` directly, because the interesting question is what the
 * PANEL composes: the profile id and the listing's own relative path. A button
 * handed a hand-built pair could not fail the way a panel handing it the file's
 * bare name would — which is how every file in a subfolder 404'd in 45.7.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ExportReceiptVm, FilesEntryVm, FilesListingVm } from "@/lib/ipc/client";

const openFolder = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => openFolder(...args),
}));

const notesExport = vi.fn();
const syncExportEntry = vi.fn();
const syncBrowse = vi.fn();
const syncOpenEntry = vi.fn();
const revealPath = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  notesExport: (vaultId: unknown, noteId: unknown, destination: unknown) =>
    notesExport(vaultId, noteId, destination),
  syncExportEntry: (id: unknown, subpath: unknown, destination: unknown) =>
    syncExportEntry(id, subpath, destination),
  syncBrowse: (id: unknown, subpath: unknown) => syncBrowse(id, subpath),
  syncOpenEntry: (id: unknown, subpath: unknown) => syncOpenEntry(id, subpath),
  revealPath: (path: unknown) => revealPath(path),
  // The registry reaches this for a `.pdf`. Held pending: this suite is about
  // the header, and a document that never resolves keeps it that way.
  // `new Promise` rather than `Promise.withResolvers`: this project's `lib`
  // target predates ES2024 and `tsc` rejects the newer spelling.
  syncReadDocument: vi.fn(() => new Promise<never>(() => undefined)),
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccess(...args),
    error: (...args: unknown[]) => toastError(...args),
  },
}));

import { EXPORT_REVEAL_LABEL } from "@/components/export/export-announce";
import { EXPORT_FILE_LABEL } from "@/components/export/export-file-button";
import { EXPORT_NOTE_LABEL, ExportNoteItem } from "@/components/export/export-note-item";
import { PanelStrip } from "@/components/layout/panel-strip";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { notesEditorStore, resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import { panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";

/** A note receipt with two written entries and one missing embed — enough that
 *  a surface reporting only the first, or dropping the caveat, is visible. */
const NOTE_RECEIPT: ExportReceiptVm = {
  path: "/Users/alice/Desktop/Meeting",
  written: ["Meeting/Meeting.md", "Meeting/attachments/photo.png"],
  missing: ["gone.png"],
  notes: [],
  summary:
    "Exported Meeting.md and 1 attachment to /Users/alice/Desktop/Meeting. keeper could not find 1 file this note embeds, so it was not carried: gone.png.",
};

const FILE_RECEIPT: ExportReceiptVm = {
  path: "/Users/alice/Desktop/report.pdf",
  written: ["report.pdf"],
  missing: [],
  notes: [],
  summary: "Exported report.pdf to /Users/alice/Desktop.",
};

/** Two files in a subfolder. Two, because a panel that offered Export for only
 *  the first row would pass a one-entry listing. */
function entry(name: string, relativePath: string): FilesEntryVm {
  return {
    name,
    relativePath,
    absolutePath: `/Users/alice/Vault/${relativePath}`,
    kind: "file",
    sync: { status: "synced", detail: null },
    // Spelled out rather than cast past. Dropping `as FilesEntryVm` showed
    // this fixture was missing three fields the real listing always sends
    // (W3NoteFile's shape: a cast asserts a literal instead of checking it).
    // `size` is what a real `stat` produced; `write` refuses because these are
    // panel-rendering fixtures and a panel must draw a listing identically
    // whether or not the folder happens to be writable.
    size: { bytes: 4300000, label: "4.3 MB" },
    folderRole: null,
    write: { writable: false, reason: "This folder is outside a notes vault." },
  };
}

function listed(subpath: string, entries: FilesEntryVm[]): FilesListingVm {
  return {
    profileId: "p1",
    subpath,
    write: { writable: false, reason: "This folder is outside a notes vault." },
    state: "listed",
    entries,
    detail: null,
    truncated: false,
  };
}

beforeEach(() => {
  resetPanelsStoreForTest();
  resetNotesEditorStoreForTest();
  openFolder.mockReset().mockResolvedValue("/Users/alice/Desktop");
  notesExport.mockReset().mockResolvedValue(NOTE_RECEIPT);
  syncExportEntry.mockReset().mockResolvedValue(FILE_RECEIPT);
  syncBrowse.mockReset();
  syncOpenEntry.mockReset();
  revealPath.mockReset().mockResolvedValue(undefined);
  toastSuccess.mockReset();
  toastError.mockReset();
  capabilitiesStore
    .getState()
    .applySnapshot({ ...DEFAULT_CAPABILITIES, sync: true, revealInFileManager: true });
});

/** The note door: the item as it is mounted, inside a real Radix menu. */
function renderNoteMenu(vaultId = "v1", noteId = "n1") {
  return render(
    <DropdownMenu>
      <DropdownMenuTrigger>Actions for Meeting</DropdownMenuTrigger>
      <DropdownMenuContent>
        <ExportNoteItem vaultId={vaultId} noteId={noteId} />
      </DropdownMenuContent>
    </DropdownMenu>,
  );
}

async function openNoteMenu(): Promise<HTMLElement> {
  const trigger = await screen.findByRole("button", { name: "Actions for Meeting" });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.pointerUp(trigger, { button: 0 });
  return await screen.findByRole("menu");
}

/** The file door: a panel already pointed at a file two folders deep. */
async function mountFilePanel(relativePath = "docs/reports/report.pdf"): Promise<void> {
  const folder = relativePath.slice(0, relativePath.lastIndexOf("/"));
  const name = relativePath.slice(relativePath.lastIndexOf("/") + 1);
  syncBrowse.mockResolvedValue(
    listed(folder, [entry("other.pdf", `${folder}/other.pdf`), entry(name, relativePath)]),
  );
  panelsStore.getState().setActiveTarget({ kind: "file", profileId: "p1", relativePath });
  render(<PanelStrip />);
  await act(async () => {
    await Promise.resolve();
  });
}

describe("the note door — the editor's Actions menu", () => {
  it("exports the note the item was mounted for, to the folder that was picked", async () => {
    renderNoteMenu("vault-7", "note-9");
    const menu = await openNoteMenu();

    fireEvent.click(within(menu).getByRole("menuitem", { name: EXPORT_NOTE_LABEL }));

    await waitFor(() =>
      expect(notesExport).toHaveBeenCalledWith("vault-7", "note-9", "/Users/alice/Desktop"),
    );
    expect(syncExportEntry).not.toHaveBeenCalled();
  });

  it("says what Rust said, caveat included, and offers Reveal at the exported path", async () => {
    renderNoteMenu();
    const menu = await openNoteMenu();
    fireEvent.click(within(menu).getByRole("menuitem", { name: EXPORT_NOTE_LABEL }));

    await waitFor(() => expect(toastSuccess).toHaveBeenCalled());
    const [message, options] = toastSuccess.mock.calls[0] ?? [];
    // Verbatim: the count, the destination and the missing embed are all Rust's
    // words, and a surface that re-worded any of them would drift from them.
    expect(message).toBe(NOTE_RECEIPT.summary);
    expect(options.action.label).toBe(EXPORT_REVEAL_LABEL);

    options.action.onClick();
    expect(revealPath).toHaveBeenCalledWith("/Users/alice/Desktop/Meeting");
  });

  it("says nothing at all when the dialog is cancelled", async () => {
    openFolder.mockResolvedValue(null);
    renderNoteMenu();
    const menu = await openNoteMenu();
    fireEvent.click(within(menu).getByRole("menuitem", { name: EXPORT_NOTE_LABEL }));

    await waitFor(() => expect(openFolder).toHaveBeenCalled());
    expect(notesExport).not.toHaveBeenCalled();
    expect(toastSuccess).not.toHaveBeenCalled();
    expect(toastError).not.toHaveBeenCalled();
  });

  it("shows the refusal keeper worded when the destination cannot be written", async () => {
    const refusal =
      'keeper could not make a folder called "Meeting" there: Permission denied (os error 13).';
    notesExport.mockImplementation(async () => {
      throw { code: "internal", message: refusal };
    });
    renderNoteMenu();
    const menu = await openNoteMenu();
    fireEvent.click(within(menu).getByRole("menuitem", { name: EXPORT_NOTE_LABEL }));

    await waitFor(() => expect(toastError).toHaveBeenCalledWith(refusal));
    expect(toastSuccess).not.toHaveBeenCalled();
  });
});

describe("the file door — a panel's header", () => {
  it("exports the path the listing produced, not the file's bare name", async () => {
    await mountFilePanel("docs/reports/report.pdf");

    fireEvent.click(await screen.findByRole("button", { name: EXPORT_FILE_LABEL }));

    await waitFor(() =>
      expect(syncExportEntry).toHaveBeenCalledWith(
        "p1",
        "docs/reports/report.pdf",
        "/Users/alice/Desktop",
      ),
    );
    expect(notesExport).not.toHaveBeenCalled();
  });

  it("says what Rust said and reveals what was written", async () => {
    await mountFilePanel();
    fireEvent.click(await screen.findByRole("button", { name: EXPORT_FILE_LABEL }));

    await waitFor(() => expect(toastSuccess).toHaveBeenCalled());
    const [message, options] = toastSuccess.mock.calls[0] ?? [];
    expect(message).toBe(FILE_RECEIPT.summary);
    options.action.onClick();
    expect(revealPath).toHaveBeenCalledWith("/Users/alice/Desktop/report.pdf");
  });

  it("offers no Reveal where the platform has no file manager, and still exports", async () => {
    capabilitiesStore
      .getState()
      .applySnapshot({ ...DEFAULT_CAPABILITIES, sync: true, revealInFileManager: false });
    await mountFilePanel();
    fireEvent.click(await screen.findByRole("button", { name: EXPORT_FILE_LABEL }));

    await waitFor(() => expect(toastSuccess).toHaveBeenCalled());
    const [, options] = toastSuccess.mock.calls[0] ?? [];
    expect(options.action).toBeUndefined();
    expect(syncExportEntry).toHaveBeenCalledTimes(1);
  });

  /** The placement rule, asserted rather than described: a note panel must not
   *  grow a second Export beside the editor's, because the panel's would export
   *  the last autosave rather than what is on screen. */
  it("offers no Export on a note panel — that one lives in the editor's menu", async () => {
    panelsStore.getState().setActiveTarget({ kind: "note", vaultId: "v1", noteId: "n1" });
    render(<PanelStrip />);
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.queryByRole("button", { name: EXPORT_FILE_LABEL })).toBeNull();
  });

  it("offers no Export on an empty panel", async () => {
    render(<PanelStrip />);
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.queryByRole("button", { name: EXPORT_FILE_LABEL })).toBeNull();
  });
});

describe("both doors", () => {
  /** One act, one word. A person who learns Export on a PDF finds it on a note.
   *  Two labels would also mean two things to search a menu for. */
  it("use the same label", () => {
    expect(EXPORT_FILE_LABEL).toBe(EXPORT_NOTE_LABEL);
  });

  it("do not flush somebody else's unsaved note when a file is exported", async () => {
    notesEditorStore.setState({
      vaultId: "v1",
      noteId: "n1",
      subscriptionId: "sub-1",
      text: "unsaved",
      base: "",
      dirty: true,
    });
    await mountFilePanel();
    fireEvent.click(await screen.findByRole("button", { name: EXPORT_FILE_LABEL }));

    await waitFor(() => expect(syncExportEntry).toHaveBeenCalledTimes(1));
    expect(toastError).not.toHaveBeenCalled();
  });
});
