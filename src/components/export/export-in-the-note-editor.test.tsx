/**
 * Story 45.21 — Export is really in the note editor's header.
 *
 * `export-controls.test.tsx` drives the menu item inside a hand-built Radix
 * menu, which proves the item works and proves nothing about whether anything
 * mounts it. Epic 44 shipped three tray listeners that were declared and never
 * mounted, because `renderHook` mounts the hook itself and can never see that
 * `App` does not — this file exists so that cannot happen to Export.
 *
 * So the whole real `NoteEditor` is mounted, its own Actions menu is opened
 * through its own trigger, and the item is pressed. If somebody removes the
 * child from the header, or the menu stops rendering children, this fails.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteBodyBatch } from "@/lib/ipc/client";

const openFolder = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => openFolder(...args),
}));

const notesExport = vi.fn();
const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();

vi.mock("@/lib/ipc/client", () => ({
  notesExport: (vaultId: unknown, noteId: unknown, destination: unknown) =>
    notesExport(vaultId, noteId, destination),
  notesOpen: (v: string, n: string, on: (b: NoteBodyBatch) => void) => notesOpen(v, n, on),
  notesClose: vi.fn(async () => {}),
  notesSave: vi.fn(async () => ({ frontmatter: "", rev: "r1", path: "n.md", conflictCopy: null })),
  notesBufferReport: vi.fn(async () => {}),
  notesTagTree: vi.fn(async () => ({ nodes: [] })),
  notesGallery: vi.fn(async () => ({ items: [] })),
  notesBacklinks: vi.fn(async () => []),
  notesResolveConflict: vi.fn(async () => {}),
  notesMarkRead: vi.fn(async () => {}),
  notesDiff: vi.fn(async () => null),
  notesHistory: vi.fn(async () => []),
  notesVaults: vi.fn(async () => []),
  // Reached only on a SLOW run: `TemplateUpdateOffer` asks for this after a
  // four-second idle timer, so a test that finishes sooner never touches it
  // and a busy box does. Omitting it is an unhandled rejection inside a
  // passing test (W2Attach, 45.13). Not unreached — do not trim it.
  notesTemplateUpdatePreview: vi.fn(async () => null),
  recordingNoteTargets: vi.fn(async () => null),
  recordingOpenPath: vi.fn(async () => {}),
  revealPath: vi.fn(async () => {}),
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccess(...args),
    error: (...args: unknown[]) => toastError(...args),
  },
}));

import { EXPORT_NOTE_LABEL } from "@/components/export/export-note-item";
import { NOTE_ACTIONS_LABEL } from "@/components/notes/note-actions";
import { NoteEditor } from "@/components/notes/note-editor";
import { resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import { withRangeRects } from "@/test/layout";

// jsdom does no layout, so CodeMirror's measure pass — which runs on any
// animation frame that elapses while the real `NoteEditor` is mounted — would
// throw outside every `try` a test can write and take the run's exit code while
// the summary still printed passes. Never hand-rolled; `src/test/layout.ts`
// owns the shim and its undo.
let restoreRects: (() => void) | null = null;

beforeAll(() => {
  restoreRects = withRangeRects();
});

afterAll(() => {
  restoreRects?.();
  restoreRects = null;
});

const BODY = "# Standing meeting\n\n![[attachments/photo.png]]\n";

beforeEach(() => {
  resetNotesEditorStoreForTest();
  openFolder.mockReset().mockResolvedValue("/Users/alice/Desktop");
  notesExport.mockReset().mockResolvedValue({
    path: "/Users/alice/Desktop/Standing meeting",
    written: ["Standing meeting/standing.md", "Standing meeting/attachments/photo.png"],
    missing: [],
    notes: [],
    summary: "Exported standing.md and 1 attachment to /Users/alice/Desktop/Standing meeting.",
  });
  notesOpen.mockReset().mockImplementation(async (_v, _n, onBatch) => {
    // `reset` is the kind `notes_open` really sends. Spelled as the batch
    // union spells it rather than invented: an unknown kind leaves the editor
    // store `undefined`, which fails as a null read three frames later and
    // reads like a bug in the surface.
    onBatch({
      kind: "reset",
      text: BODY,
      rev: "r1",
      frontmatter: "",
      path: "notes/standing.md",
      cursor: null,
    });
    return "sub-1";
  });
  toastSuccess.mockReset();
  toastError.mockReset();
});

describe("the header a person actually sees", () => {
  it("offers Export in the note's Actions menu and exports the note it is showing", async () => {
    render(<NoteEditor vaultId="vault-7" noteId="note-9" />);

    const trigger = await screen.findByRole("button", {
      name: new RegExp(`^${NOTE_ACTIONS_LABEL}`),
    });
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.pointerUp(trigger, { button: 0 });

    const menu = await screen.findByRole("menu");
    fireEvent.click(within(menu).getByRole("menuitem", { name: EXPORT_NOTE_LABEL }));

    // The ids the HEADER composed, not ones this test handed a component.
    await waitFor(() =>
      expect(notesExport).toHaveBeenCalledWith("vault-7", "note-9", "/Users/alice/Desktop"),
    );
    await waitFor(() => expect(toastSuccess).toHaveBeenCalled());
  });
});
