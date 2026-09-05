/**
 * Notes and capture on the phone (Epic 66, Story 66.4, FR-467…FR-469, AD-200).
 *
 * Every test drives the REAL `PhoneShell` — the drawer row, the stack levels,
 * the back bars — and the real `NoteEditor` with its CodeMirror chunk booted,
 * because the claim is not "a phone notes component exists" but that the
 * desktop's readers reach the phone's stack and a save on the phone goes down
 * the same channel a Mac's does. A mocked editor would prove neither.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  NoteBodyBatch,
  NoteCreateReq,
  NoteCreateVm,
  NoteListVm,
  NoteQueryReq,
  NoteRowVm,
  NoteVaultVm,
  NoteWriteVm,
} from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { captureSheetStore } from "@/lib/stores/capture-sheet";
import { detailStore } from "@/lib/stores/detail-ui";
import { leadingDrawerStore } from "@/lib/stores/leading-drawer";
import { resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import { resetNotesFiltersStoreForTest } from "@/lib/stores/notes-filters";
import { resetNotesListStoreForTest } from "@/lib/stores/notes-list";
import { resetNotesVaultsStoreForTest } from "@/lib/stores/notes-vaults";
import { resetPanelsStoreForTest } from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";
import { type ListGeometry, withListGeometry, withRangeRects } from "@/test/layout";
import { settleNoteEditorBoot } from "@/test/note-editor-boot";

const VAULT: NoteVaultVm = {
  id: "v1",
  profileId: "v1",
  name: "Owner's vault",
  subfolder: "notes",
  root: "/container/sync/v1/notes",
  indexed: true,
  noteCount: 2,
  unreadCount: 0,
  captureTemplate: null,
  captureTag: null,
  cadence: { commitIdleMs: 2000, pushIntervalMs: 30000, pushOnBlur: true },
};

function row(id: string, title: string): NoteRowVm {
  return {
    id,
    path: `${id}.md`,
    title,
    snippet: `${title} body`,
    tags: [],
    updatedMs: 1,
    pinned: false,
    archived: false,
    conflict: false,
    origin: "local",
    unread: false,
    headRev: "",
    files: [],
    backlinks: 0,
    forwardlinks: 0,
    order: { value: 0, source: "default" },
    device: "",
  } as unknown as NoteRowVm;
}

const DENTIST = row("01DENTIST", "Ring the dentist");
const GROCERIES = row("01GROCERIES", "Groceries");
const BODIES: Record<string, string> = {
  "01DENTIST": "ring the dentist on monday\n",
  "01GROCERIES": "eggs, milk\n",
  "01CAPTUREPAGE": "",
};

const notesList = vi.fn<(vaultId: string, query: NoteQueryReq) => Promise<NoteListVm>>();
const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();
const notesSave =
  vi.fn<(id: string, text: string, rev: string, block?: string) => Promise<NoteWriteVm>>();
const notesCaptureDraft = vi.fn<(key: string) => Promise<NoteCreateVm>>();
const notesCaptureHide = vi.fn<() => Promise<void>>();
const notesCreate = vi.fn<(vaultId: string, req: NoteCreateReq) => Promise<NoteCreateVm>>();
const notesHistory = vi.fn(async () => []);

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    // The shell's always-mounted chat surfaces (the phone-shell suite's stubs).
    subscribeInbox: vi.fn(async (): Promise<number> => 1),
    unsubscribeInbox: vi.fn(async (): Promise<void> => {}),
    listDrafts: vi.fn(async (): Promise<Array<[string, string]>> => []),
    getFavoritesCollapsed: vi.fn(async (): Promise<boolean> => false),
    subscribeDraftMirror: vi.fn(async (): Promise<number> => 1),
    unsubscribeDraftMirror: vi.fn(async (): Promise<void> => {}),
    couplingCaveats: vi.fn(async () => []),
    loadDraft: vi.fn(async (): Promise<string | null> => null),
    encryptionPosture: vi.fn(() => Promise.resolve(false)),
    paletteQuery: vi.fn(async () => ({ contacts: [], chats: [], actions: [] })),
    searchArchive: vi.fn(async () => []),
    voiceAvailability: vi.fn(() => new Promise<never>(() => {})),
    voiceWakeGet: vi.fn(() => new Promise<never>(() => {})),
    // The notes reads.
    notesVaults: vi.fn(async () => [VAULT]),
    notesVaultActive: vi.fn(async () => VAULT.id),
    notesVaultSetActive: vi.fn(async () => {}),
    notesList: (vaultId: string, query: NoteQueryReq) => notesList(vaultId, query),
    notesSubscribeChanges: vi.fn(async () => "changes-1"),
    notesUnsubscribeChanges: vi.fn(async () => {}),
    notesCreate: (vaultId: string, req: NoteCreateReq) => notesCreate(vaultId, req),
    // The editor.
    notesOpen: (v: string, n: string, on: (b: NoteBodyBatch) => void) => notesOpen(v, n, on),
    notesClose: vi.fn(async () => {}),
    notesSave: (id: string, text: string, rev: string, block?: string) =>
      notesSave(id, text, rev, block),
    notesBufferReport: vi.fn(async () => {}),
    notesTagTree: vi.fn(async () => ({ nodes: [] })),
    tagsVocabulary: vi.fn(async () => ({ entries: [] })),
    notesGallery: vi.fn(async () => ({ folder: "", items: [], notice: null })),
    notesBacklinks: vi.fn(async () => []),
    notesLinkTargets: vi.fn(async () => []),
    notesTemplateUpdatePreview: vi.fn(async () => null),
    notesHistory: () => notesHistory(),
    notesDiff: vi.fn(async () => ({ hunks: [], fromRev: "", toRev: null })),
    // Capture: the page, and the desktop's window verb that must NOT be reached.
    notesCaptureDraft: (key: string) => notesCaptureDraft(key),
    notesCaptureHide: () => notesCaptureHide(),
    listenNotesCaptureShown: vi.fn(async () => () => {}),
    notesCaptureWindows: vi.fn(async () => []),
  };
});

vi.mock("@/hooks/use-sign-out", () => ({
  useSignOut: () => vi.fn(),
}));

vi.mock("@/hooks/use-stale-resume-pill", () => ({
  useStaleResumePill: () => false,
}));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn((_handler?: (e: unknown) => void) => Promise.resolve(() => {})),
  }),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => null),
}));

import { EditorView } from "@codemirror/view";
import { CAPTURE_NOTE_LABEL } from "@/components/capture/capture-note-item";
import {
  CAPTURE_SHEET_DONE_LABEL,
  CAPTURE_SHEET_SLOT,
} from "@/components/capture/capture-phone-sheet";
import { EXPORT_NOTE_LABEL } from "@/components/export/export-note-item";
import { PhoneShell } from "@/components/layout/phone-shell";
import { NOTE_ACTIONS_LABEL } from "@/components/notes/note-actions";
import { NOTE_HISTORY_LABEL } from "@/components/notes/note-history-panel";
import { NEW_NOTE_LABEL, NOTES_COUNT_SLOT } from "@/components/notes/notes-pane";
import {
  NOTES_PHONE_BACK_TO_LIST,
  NOTES_PHONE_CAPTURE_LABEL,
  NOTES_PHONE_NOTE_SLOT,
  NOTES_PHONE_SEARCH_LABEL,
} from "@/components/notes/notes-phone-pane";
import { NOTE_AUTOSAVE_IDLE_MS } from "@/hooks/use-notes-body";
import { phoneRoutesView } from "@/lib/phone-surfaces";

/** The phone once its folder links (Epic 66): notes ride the folder. */
const PHONE = { ...DEFAULT_CAPABILITIES, bots: true, sync: true, notes: true };

const originalMatchMedia = window.matchMedia;
function mockPhoneViewport() {
  window.matchMedia = vi.fn().mockImplementation((query: string) => {
    const match = query.match(/max-width:\s*(\d+)px/);
    const maxWidth = match ? Number(match[1]) : Number.POSITIVE_INFINITY;
    const matches = query.includes("prefers-reduced-motion") ? true : 390 <= maxWidth;
    return {
      matches,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    };
  });
}

/** The stack-level wrapper for the given level, or `null` when unmounted. */
function stackLevel(level: 0 | 1 | 2): HTMLElement | null {
  return document.querySelector<HTMLElement>(`[data-level="${level}"]`);
}

/** Open the drawer and tap the row with this label; resolves once the drawer has gone. */
async function tapDrawerRow(label: RegExp) {
  fireEvent.click(screen.getByRole("button", { name: "Open navigation" }));
  const nav = await screen.findByRole("navigation", { name: "Views" });
  fireEvent.click(within(nav).getByRole("button", { name: label }));
  await waitFor(() => expect(leadingDrawerStore.getState().isOpen).toBe(false));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
}

/** Push the Notes view from the drawer and wait for the list to hold its rows. */
async function openNotes() {
  await tapDrawerRow(/^Notes/);
  await screen.findByRole("button", { name: /Ring the dentist/ });
}

/** The live editor, once its lazy chunk has landed and the reset applied. */
async function liveEditor(body: string): Promise<EditorView> {
  return await waitFor(
    () => {
      const host = document.querySelector<HTMLElement>(".cm-editor");
      expect(host).not.toBeNull();
      const found = EditorView.findFromDOM(host as HTMLElement);
      expect(found).not.toBeNull();
      expect((found as EditorView).state.doc.toString()).toBe(body);
      return found as EditorView;
    },
    { timeout: 4000 },
  );
}

/** Open the note's Actions menu (Story 46.5) and hand back its content. */
async function openNoteActions(): Promise<HTMLElement> {
  const trigger = await screen.findByRole("button", {
    name: new RegExp(`^${NOTE_ACTIONS_LABEL}`),
  });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.pointerUp(trigger, { button: 0 });
  return await screen.findByRole("menu");
}

let restoreRects: () => void;
let geometry: ListGeometry;
beforeAll(() => {
  restoreRects = withRangeRects();
  // The note list is windowed (Story 44.10) and jsdom lays nothing out: with
  // no viewport height no row would mount. A phone's worth of rows.
  geometry = withListGeometry({ viewport: 700, row: 64 });
});

afterAll(() => {
  geometry.undo();
});

beforeEach(() => {
  vi.clearAllMocks();
  mockPhoneViewport();
  accountsStore.getState().clear();
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  detailStore.setState({ open: false });
  leadingDrawerStore.getState().close();
  captureSheetStore.getState().close();
  primaryViewStore.getState().setView("inbox");
  resetNotesVaultsStoreForTest();
  resetNotesFiltersStoreForTest();
  resetNotesListStoreForTest();
  resetPanelsStoreForTest();
  resetNotesEditorStoreForTest();
  capabilitiesStore.getState().applySnapshot(PHONE);
  notesList.mockImplementation(async (_vaultId, query) => {
    const needle = (query.text ?? "").trim().toLowerCase();
    const rows = [DENTIST, GROCERIES].filter((candidate) =>
      candidate.title.toLowerCase().includes(needle),
    );
    return { rows, total: rows.length, matched: rows.length, offset: 0 };
  });
  notesOpen.mockImplementation(async (_vault, noteId, onBatch) => {
    onBatch({
      kind: "reset",
      text: BODIES[noteId] ?? "",
      frontmatter: "",
      rev: `rev-${noteId}`,
      cursor: null,
      path: `${noteId}.md`,
    } as NoteBodyBatch);
    return `sub-${noteId}`;
  });
  notesSave.mockResolvedValue({
    rev: "rev-saved",
    path: "n.md",
    frontmatter: "",
    conflictCopy: null,
  });
  notesCaptureDraft.mockResolvedValue({
    note: {
      vaultId: VAULT.id,
      id: "01CAPTUREPAGE",
      path: "2026-09-05-untitled.md",
      title: "Untitled",
    },
    notices: [],
  });
  notesCaptureHide.mockResolvedValue(undefined);
  notesCreate.mockResolvedValue({
    note: { vaultId: VAULT.id, id: "01GROCERIES", path: "01GROCERIES.md", title: "Groceries" },
    notices: [],
  });
});

afterEach(async () => {
  // Real timers first: the boot settle awaits a promise chain the frozen
  // clock would never let resolve.
  vi.useRealTimers();
  await settleNoteEditorBoot();
  window.matchMedia = originalMatchMedia;
  primaryViewStore.getState().setView("inbox");
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  restoreRects();
  restoreRects = withRangeRects();
});

describe("the Notes view on the phone stack", () => {
  it("is a drawer row that lands on a level 1 with the vault, the search and the list", async () => {
    render(<PhoneShell />);
    expect(phoneRoutesView("notes", PHONE)).toBe(true);
    await openNotes();

    const level = stackLevel(1);
    expect(level).not.toBeNull();
    const notes = within(level as HTMLElement);
    expect(notes.getByRole("button", { name: "Back to Inbox" })).toBeVisible();
    expect(notes.getByRole("button", { name: `Vault ${VAULT.name}` })).toBeInTheDocument();
    expect(notes.getByRole("searchbox", { name: NOTES_PHONE_SEARCH_LABEL })).toBeInTheDocument();
    expect(
      (level as HTMLElement).querySelector(`[data-slot="${NOTES_COUNT_SLOT}"]`),
    ).toHaveTextContent("2");
    expect(notes.getByRole("button", { name: /Groceries/ })).toBeInTheDocument();
    // Both header verbs stand at the phone's minimum target.
    for (const name of [NEW_NOTE_LABEL, NOTES_PHONE_CAPTURE_LABEL]) {
      expect(notes.getByRole("button", { name }).className).toContain("size-11");
    }
    expect(stackLevel(2)).toBeNull();
  });

  it("is absent — no row, no level — where the build has no notes", async () => {
    capabilitiesStore.getState().applySnapshot({ ...PHONE, notes: false });
    primaryViewStore.getState().setView("notes");
    render(<PhoneShell />);
    expect(stackLevel(1)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Open navigation" }));
    const nav = await screen.findByRole("navigation", { name: "Views" });
    expect(within(nav).queryByRole("button", { name: /^Notes/ })).toBeNull();
    // And the Inbox header offers no capture into a vault that cannot exist.
    expect(screen.queryByRole("button", { name: NOTES_PHONE_CAPTURE_LABEL })).toBeNull();
  });

  it("searches through Rust's own list query, never a client-side filter", async () => {
    render(<PhoneShell />);
    await openNotes();
    const field = screen.getByRole("searchbox", { name: NOTES_PHONE_SEARCH_LABEL });
    fireEvent.change(field, { target: { value: "dentist" } });

    await waitFor(() => {
      expect(notesList).toHaveBeenLastCalledWith(
        VAULT.id,
        expect.objectContaining({ text: "dentist" }),
      );
    });
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: /Groceries/ })).toBeNull();
    });
    expect(screen.getByRole("button", { name: /Ring the dentist/ })).toBeInTheDocument();
  });

  it("opens a note rendered in the real editor at level 2, and back pops to the list", async () => {
    render(<PhoneShell />);
    await openNotes();
    fireEvent.click(screen.getByRole("button", { name: /Ring the dentist/ }));

    await screen.findByRole("button", { name: NOTES_PHONE_BACK_TO_LIST });
    const level = stackLevel(2);
    expect(level).not.toBeNull();
    const editor = await liveEditor(BODIES["01DENTIST"] ?? "");
    expect(editor.contentDOM.getAttribute("aria-label")).toBe("Note");
    // The note column ends above the keyboard (Story 13.5's inset), so the
    // caret is never under it; jsdom cannot evaluate the var, so the class
    // plumbing is the assertion.
    const column = document.querySelector<HTMLElement>(`[data-slot="${NOTES_PHONE_NOTE_SLOT}"]`);
    expect(column?.className).toContain("pb-[calc(var(--kb-inset,0px)_+_var(--safe-bottom))]");

    fireEvent.click(screen.getByRole("button", { name: NOTES_PHONE_BACK_TO_LIST }));
    await waitFor(() => expect(stackLevel(2)).toBeNull());
    expect(screen.getByRole("button", { name: /Ring the dentist/ })).toBeInTheDocument();
    expect(primaryViewStore.getState().view).toBe("notes");
  });

  it("re-enters on the list, never on the note left open", async () => {
    render(<PhoneShell />);
    await openNotes();
    fireEvent.click(screen.getByRole("button", { name: /Ring the dentist/ }));
    await screen.findByRole("button", { name: NOTES_PHONE_BACK_TO_LIST });
    act(() => {
      primaryViewStore.getState().setView("inbox");
    });
    await waitFor(() => expect(stackLevel(1)).toBeNull());
    await openNotes();
    expect(stackLevel(2)).toBeNull();
  });

  it("saves an edit down the note's own channel: the existing notes_save, on autosave", async () => {
    render(<PhoneShell />);
    await openNotes();
    fireEvent.click(screen.getByRole("button", { name: /Ring the dentist/ }));
    const editor = await liveEditor(BODIES["01DENTIST"] ?? "");
    vi.useFakeTimers();

    act(() => {
      editor.dispatch({ changes: { from: editor.state.doc.length, insert: "and the vet\n" } });
    });
    await act(async () => {
      vi.advanceTimersByTime(NOTE_AUTOSAVE_IDLE_MS + 1);
    });

    // The subscription argument is the assertion: the same command, the same
    // channel a Mac's editor writes down. What Rust does next — commit, push
    // (`notes_vault::phone_commit_and_push`) — is the Rust test's claim.
    expect(notesSave).toHaveBeenCalledExactlyOnceWith(
      "sub-01DENTIST",
      "ring the dentist on monday\nand the vet\n",
      "rev-01DENTIST",
      undefined,
    );
  });

  it("offers History and no window verb, no export, in the note's actions", async () => {
    render(<PhoneShell />);
    await openNotes();
    fireEvent.click(screen.getByRole("button", { name: /Ring the dentist/ }));
    await liveEditor(BODIES["01DENTIST"] ?? "");
    const menu = await openNoteActions();
    // History is in-process on the phone (AD-198); the menu offers it.
    expect(within(menu).getByRole("menuitem", { name: NOTE_HISTORY_LABEL })).toBeInTheDocument();
    // A window a phone cannot open, and an export with no picker: absent.
    expect(within(menu).queryByRole("menuitem", { name: CAPTURE_NOTE_LABEL })).toBeNull();
    expect(within(menu).queryByRole("menuitem", { name: EXPORT_NOTE_LABEL })).toBeNull();
  });

  it("pushes the note a create lands on, from the header's New note", async () => {
    render(<PhoneShell />);
    await openNotes();
    fireEvent.click(screen.getByRole("button", { name: NEW_NOTE_LABEL }));
    await waitFor(() => expect(notesCreate).toHaveBeenCalledTimes(1));
    await screen.findByRole("button", { name: NOTES_PHONE_BACK_TO_LIST });
    await liveEditor(BODIES["01GROCERIES"] ?? "");
  });
});

describe("quick capture on the phone", () => {
  /** The sheet, mounted and holding a live editor on the page Rust resolved. */
  async function openSheet(): Promise<EditorView> {
    await waitFor(() => expect(notesCaptureDraft).toHaveBeenCalledWith("draft"));
    await screen.findByTestId("capture-phone-sheet");
    return await liveEditor("");
  }

  it("opens as a sheet from the Notes level, on the same command the desktop window calls", async () => {
    render(<PhoneShell />);
    await openNotes();
    fireEvent.click(
      within(stackLevel(1) as HTMLElement).getByRole("button", { name: NOTES_PHONE_CAPTURE_LABEL }),
    );
    await openSheet();
    const sheet = document.querySelector<HTMLElement>(`[data-slot="${CAPTURE_SHEET_SLOT}"]`);
    expect(sheet).not.toBeNull();
    expect(
      within(sheet as HTMLElement).getByRole("button", { name: CAPTURE_SHEET_DONE_LABEL })
        .className,
    ).toContain("h-11");
    // A sheet in the stack, not a window: the desktop's show verb is never
    // reached, and the level underneath is still the Notes list.
    expect(stackLevel(1)).not.toBeNull();
  });

  it("opens from the Inbox header too — one sheet, one store", async () => {
    render(<PhoneShell />);
    fireEvent.click(screen.getByRole("button", { name: NOTES_PHONE_CAPTURE_LABEL }));
    await openSheet();
    expect(captureSheetStore.getState().isOpen).toBe(true);
    expect(stackLevel(1)).toBeNull();
  });

  it("files the thought on Done: saves first, then closes, never the window verb", async () => {
    render(<PhoneShell />);
    fireEvent.click(screen.getByRole("button", { name: NOTES_PHONE_CAPTURE_LABEL }));
    const editor = await openSheet();
    act(() => {
      editor.dispatch({ changes: { from: 0, insert: "buy stamps" } });
    });
    await waitFor(() => expect(notesSave).not.toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: CAPTURE_SHEET_DONE_LABEL }));
    await waitFor(() => expect(captureSheetStore.getState().isOpen).toBe(false));
    // The order is the guarantee (AD-62): the bytes reach Rust before the
    // sheet goes, so the next open finds a page that was written on.
    expect(notesSave).toHaveBeenCalledExactlyOnceWith(
      "sub-01CAPTUREPAGE",
      "buy stamps",
      "rev-01CAPTUREPAGE",
      undefined,
    );
    expect(notesCaptureHide).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByTestId("capture-phone-sheet")).toBeNull());
  });

  it("keeps the sheet and the words when the save is refused", async () => {
    notesSave.mockRejectedValue(new Error("disk full"));
    render(<PhoneShell />);
    fireEvent.click(screen.getByRole("button", { name: NOTES_PHONE_CAPTURE_LABEL }));
    const editor = await openSheet();
    act(() => {
      editor.dispatch({ changes: { from: 0, insert: "not lost" } });
    });
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_SHEET_DONE_LABEL }));
    await waitFor(() => expect(notesSave).toHaveBeenCalledTimes(1));
    expect(captureSheetStore.getState().isOpen).toBe(true);
    expect(editor.state.doc.toString()).toBe("not lost");
  });
});
