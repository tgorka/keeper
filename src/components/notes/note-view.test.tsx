/**
 * The note's three views (Preview | Note | Source), through the real
 * `NoteEditor`: what each one draws, that only Note and Source take a caret,
 * and that the choice outlives the editor it was made in.
 */
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteBodyBatch } from "@/lib/ipc/client";
import { settleNoteEditorBoot } from "@/test/note-editor-boot";

const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();

vi.mock("@/lib/ipc/client", () => ({
  notesOpen: (v: string, n: string, on: (b: NoteBodyBatch) => void) => notesOpen(v, n, on),
  notesClose: vi.fn(async () => {}),
  notesSave: vi.fn(async () => ({ frontmatter: "", rev: "r1", path: "n.md", conflictCopy: null })),
  notesBufferReport: vi.fn(async () => {}),
  notesTagTree: vi.fn(async () => ({ nodes: [] })),
  tagsVocabulary: vi.fn(async () => ({ entries: [] })),
  notesAttachSources: vi.fn(async () => []),
  notesBacklinks: vi.fn(async () => []),
  notesTemplateUpdatePreview: vi.fn(async () => null),
  notesMarkRead: vi.fn(async () => {}),
  notesNoteMarks: vi.fn(async () => ({ rev: "r0", ranges: [] })),
  notesGallery: vi.fn(async () => ({ entries: [] })),
  recordingNoteTargets: vi.fn(async () => null),
}));

import { VIEW_MODE_COOKIE } from "@/components/viewers/view-mode";
import { resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import { resetPanelsStoreForTest } from "@/lib/stores/panels";
import { withRangeRects } from "@/test/layout";
import { NOTE_VIEW_LABEL, NOTE_VIEW_LABELS, NoteEditor } from "./note-editor";

// jsdom does no layout; `src/test/layout.ts` owns the shim CodeMirror needs.
let restoreRects: (() => void) | null = null;
beforeAll(() => {
  restoreRects = withRangeRects();
});
afterAll(() => {
  restoreRects?.();
});

/** Bold on a line the caret (at the end) is not on, so Note draws it. */
const BODY = "# Standup\n\n**Decided** to ship.\n\nlast line";

beforeEach(() => {
  notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
    onBatch({ kind: "reset", text: BODY, frontmatter: "", rev: "r0", cursor: null, path: "n.md" });
    return "sub-1";
  });
  resetPanelsStoreForTest();
});

afterEach(async () => {
  cleanup();
  resetNotesEditorStoreForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: a test resets the one jar the app writes through `writeCookie`
  document.cookie = `${VIEW_MODE_COOKIE}=; max-age=0; path=/`;
});

async function mounted(): Promise<void> {
  render(<NoteEditor vaultId="v1" noteId="note-7" />);
  await settleNoteEditorBoot();
  await waitFor(() => expect(editorContent()?.textContent).toContain("ship"));
}

function editorContent(): HTMLElement | null {
  return document.querySelector('[data-slot="note-editor-host"] .cm-content');
}

function previewContent(): HTMLElement | null {
  return document.querySelector('[data-slot="note-preview-host"] .cm-content');
}

async function choose(view: keyof typeof NOTE_VIEW_LABELS): Promise<void> {
  const trigger = screen.getByRole("button", { name: new RegExp(`^${NOTE_VIEW_LABEL}`) });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.click(await screen.findByRole("menuitemradio", { name: NOTE_VIEW_LABELS[view] }));
}

describe("the note's views", () => {
  it("opens in Note: the live preview, marks drawn, a caret to type with", async () => {
    await mounted();

    expect(
      screen.getByRole("button", { name: `${NOTE_VIEW_LABEL}: ${NOTE_VIEW_LABELS.note}` }),
    ).toBeInTheDocument();
    expect(editorContent()?.textContent).not.toContain("**Decided**");
    expect(editorContent()?.getAttribute("contenteditable")).toBe("true");
  });

  it("shows the raw markdown in Source, in the same editor, still editable", async () => {
    await mounted();
    const editor = editorContent();

    await choose("raw");

    await waitFor(() => expect(editorContent()?.textContent).toContain("**Decided**"));
    expect(editorContent()).toBe(editor);
    expect(editorContent()?.getAttribute("contenteditable")).toBe("true");

    await choose("note");
    await waitFor(() => expect(editorContent()?.textContent).not.toContain("**Decided**"));
  });

  it("draws Preview read-only in its own pane, with the editor hidden behind it", async () => {
    await mounted();

    await choose("rendered");

    await waitFor(() => expect(previewContent()?.textContent).toContain("ship"));
    expect(previewContent()?.getAttribute("contenteditable")).toBe("false");
    expect(document.querySelector('[data-slot="note-editor-host"]')).toHaveClass("hidden");
    // No toolbar where there is no caret for it to act on.
    expect(screen.queryByRole("toolbar")).toBeNull();
  });

  it("opens the next note in the view chosen last", async () => {
    await mounted();
    await choose("raw");
    cleanup();

    await mounted();

    expect(
      screen.getByRole("button", { name: `${NOTE_VIEW_LABEL}: ${NOTE_VIEW_LABELS.raw}` }),
    ).toBeInTheDocument();
    expect(editorContent()?.textContent).toContain("**Decided**");
  });
});
