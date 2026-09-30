/**
 * A removed recording leaves an open note whichever view it is in — through
 * the real `NoteEditor`, in Source (no media layer) and in Preview (no editor
 * to put one in), where the widget used to stay.
 */
import { cleanup, render, waitFor } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteBodyBatch } from "@/lib/ipc/client";
import { settleNoteEditorBoot } from "@/test/note-editor-boot";

const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();
const notesSave = vi.fn(async (_sub: string, _text: string, _rev: string) => ({
  frontmatter: "",
  rev: "r1",
  path: "n.md",
  conflictCopy: null,
}));
const withoutSession = vi.fn<(text: string, sessionId: string) => Promise<string | null>>();
let heard: ((sessionId: string) => void) | null = null;

vi.mock("@/lib/ipc/client", () => ({
  notesOpen: (v: string, n: string, on: (b: NoteBodyBatch) => void) => notesOpen(v, n, on),
  notesClose: vi.fn(async () => {}),
  notesSave: (sub: string, text: string, rev: string) => notesSave(sub, text, rev),
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
  // A drawn widget stays unclassified: this file is about the buffer, not the panel.
  mediaBlockRecording: () => new Promise(() => {}),
  mediaBlockResolve: () => new Promise(() => {}),
  mediaBlockWithoutSession: (text: string, sessionId: string) => withoutSession(text, sessionId),
  listenRecordingRemoved: async (on: (sessionId: string) => void) => {
    heard = on;
    return () => {
      heard = null;
    };
  },
}));

import { NoteEditor } from "@/components/notes/note-editor";
import { VIEW_MODE_COOKIE } from "@/components/viewers/view-mode";
import { resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import { resetPanelsStoreForTest } from "@/lib/stores/panels";
import { withRangeRects } from "@/test/layout";
import { useRecordingRemoved } from "./use-recording-removed";

let restoreRects: (() => void) | null = null;
beforeAll(() => {
  restoreRects = withRangeRects();
});
afterAll(() => {
  restoreRects?.();
});

const FENCE = '```keeper-media\nsession = "S1"\n```\n';
const BODY = `# Standup\n\nWhat we said.\n\n${FENCE}\nlast line`;
const WITHOUT = "# Standup\n\nWhat we said.\n\n\nlast line";

function Listener() {
  useRecordingRemoved();
  return null;
}

beforeEach(() => {
  notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
    onBatch({ kind: "reset", text: BODY, frontmatter: "", rev: "r0", cursor: null, path: "n.md" });
    return "sub-1";
  });
  // Rust's composition, as the mock tells it: the removed recording's fence goes.
  withoutSession.mockImplementation(async (text, sessionId) =>
    text.includes(`session = "${sessionId}"`) ? text.replace(FENCE, "") : null,
  );
  notesSave.mockClear();
  resetPanelsStoreForTest();
});

afterEach(() => {
  cleanup();
  resetNotesEditorStoreForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: a test resets the one jar the app writes through `writeCookie`
  document.cookie = `${VIEW_MODE_COOKIE}=; max-age=0; path=/`;
});

function openIn(view: "raw" | "rendered"): void {
  // biome-ignore lint/suspicious/noDocumentCookie: the note opens in the view the jar remembers
  document.cookie = `${VIEW_MODE_COOKIE}=${encodeURIComponent(`keeper-note:${view}`)}; path=/`;
  render(
    <>
      <Listener />
      <NoteEditor vaultId="v1" noteId="note-7" />
    </>,
  );
}

function content(slot: "note-editor-host" | "note-preview-host"): string {
  return document.querySelector(`[data-slot="${slot}"] .cm-content`)?.textContent ?? "";
}

describe("a removed recording in an open note", () => {
  it("leaves a note open in Source, keeping every other word, and the note is saved", async () => {
    openIn("raw");
    await settleNoteEditorBoot();
    await waitFor(() => expect(content("note-editor-host")).toContain('session = "S1"'));
    await waitFor(() => expect(heard).not.toBeNull());

    heard?.("S1");

    await waitFor(() => expect(content("note-editor-host")).not.toContain("keeper-media"));
    expect(content("note-editor-host")).toContain("What we said.");
    expect(content("note-editor-host")).toContain("last line");
    await waitFor(() => expect(notesSave).toHaveBeenCalledWith("sub-1", WITHOUT, "r0"));
  });

  it("leaves a note open in Preview, and the note is saved", async () => {
    openIn("rendered");
    await settleNoteEditorBoot();
    await waitFor(() => expect(content("note-preview-host")).toContain("What we said."));
    const widget = () => document.querySelector('[data-slot="note-preview-host"] .cm-media-block');
    await waitFor(() => expect(widget()).not.toBeNull());
    await waitFor(() => expect(heard).not.toBeNull());

    heard?.("S1");

    await waitFor(() => expect(notesSave).toHaveBeenCalledWith("sub-1", WITHOUT, "r0"));
    await waitFor(() => expect(widget()).toBeNull());
    expect(content("note-preview-host")).toContain("What we said.");
  });

  it("leaves a note naming another recording alone", async () => {
    openIn("raw");
    await settleNoteEditorBoot();
    await waitFor(() => expect(heard).not.toBeNull());

    heard?.("OTHER");

    await waitFor(() => expect(withoutSession).not.toHaveBeenCalledWith(BODY, "OTHER"));
    expect(content("note-editor-host")).toContain('session = "S1"');
    expect(notesSave).not.toHaveBeenCalled();
  });
});
