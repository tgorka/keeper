/**
 * Story 45.14: **quick capture IS the note editor** (FR-190, AD-93).
 *
 * The claim is not "quick capture can now do markdown". It is that the capture
 * window mounts the *same component* the notes pane does, so the format
 * toolbar, the `/` menu, the tag chooser and the attachment picker are in it by
 * construction rather than by five separate pieces of work. A test that mocked
 * `NoteEditor` and asserted it was rendered would prove precisely nothing about
 * that — it would prove that a stub was rendered (DW-172). So every test here
 * mounts the **real** editor: its boot effect, its dynamic imports, a live
 * `EditorView` read back through `EditorView.findFromDOM`.
 *
 * # Doors
 *
 * Counted deliberately, because a behaviour with many tests that all enter
 * through one door has untested doors no matter how many tests there are:
 *
 * | Door | What it is | Tests |
 * |---|---|---|
 * | `CaptureDraftDocument` | the hotkey window — the door a real user comes through | 11 |
 * | `CaptureDocument` | a window opened on an existing note (Story 45.15) | 2 |
 * | `CapturePanel` | the webview root, which is what `capture.html` actually boots | 2 |
 *
 * The third row is the DW-172 row. Rendering `CaptureDraftDocument` in a test
 * can never observe that `capture-main.tsx` does not mount it, and "the tray
 * listener was declared and never mounted" is a defect this project has already
 * shipped twice.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  NoteAttachSourceVm,
  NoteBodyBatch,
  NoteCreateVm,
  NoteWriteVm,
} from "@/lib/ipc/client";
import { withRangeRects } from "@/test/layout";
import { settleNoteEditorBoot } from "@/test/note-editor-boot";

const notesCaptureDraft = vi.fn<(key: string) => Promise<NoteCreateVm>>();
const notesCaptureHide = vi.fn<() => Promise<void>>();
const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();
const notesClose = vi.fn<(id: string) => Promise<void>>();
const notesSave =
  vi.fn<(id: string, text: string, rev: string, block?: string) => Promise<NoteWriteVm>>();
const notesAttachSources = vi.fn<(v: string, s: string[]) => Promise<NoteAttachSourceVm[]>>();
const pickFiles = vi.fn<() => Promise<string[] | string | null>>();

/** Handed the hook's re-resolve callback, so a test can act as `show` does. */
let onCaptureShown: (() => void) | null = null;
const listenNotesCaptureShown = vi.fn(async (onShown: () => void) => {
  onCaptureShown = onShown;
  return () => {
    onCaptureShown = null;
  };
});

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: () => pickFiles(),
}));

vi.mock("@/lib/ipc/client", () => ({
  notesCaptureDraft: (key: string) => notesCaptureDraft(key),
  notesCaptureHide: () => notesCaptureHide(),
  listenNotesCaptureShown: (onShown: () => void) => listenNotesCaptureShown(onShown),
  notesOpen: (v: string, n: string, on: (b: NoteBodyBatch) => void) => notesOpen(v, n, on),
  notesClose: (id: string) => notesClose(id),
  notesSave: (id: string, text: string, rev: string, block?: string) =>
    notesSave(id, text, rev, block),
  notesAttachSources: (v: string, s: string[]) => notesAttachSources(v, s),
  notesBufferReport: vi.fn(async () => {}),
  notesTagTree: vi.fn(async () => ({ nodes: [{ path: "inbox", count: 1, children: [] }] })),
  tagsVocabulary: vi.fn(async () => ({ entries: [{ path: "errand", count: 3 }] })),
  notesGallery: vi.fn(async () => ({ folder: "", items: [], notice: null })),
  notesBacklinks: vi.fn(async () => []),
  notesResolveConflict: vi.fn(async () => {}),
  notesMarkRead: vi.fn(async () => {}),
  notesDiff: vi.fn(async () => null),
  notesHistory: vi.fn(async () => []),
  notesLinkTargets: vi.fn(async () => []),
  recordingNoteTargets: vi.fn(async () => null),
  recordingOpenPath: vi.fn(async () => {}),
  revealPath: vi.fn(async () => {}),
  // Story 45.15's chrome mounts inside `CapturePanel`, so this file's boot
  // path reaches these three the moment the root is rendered. Added here
  // rather than by mocking the chrome away, because the root's job IS to
  // assemble a window out of a document and a strip of buttons.
  notesCaptureWindows: vi.fn(async () => []),
  notesCaptureSetLocked: vi.fn(async () => {}),
  listenNotesCaptureWindows: vi.fn(async () => () => {}),
  // Reached only on a SLOW run, and therefore easy to mistake for unreached:
  // `NoteEditor` mounts `TemplateUpdateOffer`, which calls this after four
  // seconds of idle. A test that finishes sooner never gets here; the timeout
  // test below does, and without this the missing export throws an unhandled
  // rejection that surfaces as a 5 s timeout in a test about something else.
  notesTemplateUpdatePreview: vi.fn(async () => null),
}));

import { EditorView } from "@codemirror/view";
import { CapturePanel } from "@/capture-main";
import {
  ATTACH_FILE_LABEL,
  ATTACH_FROM_COMPUTER_LABEL,
} from "@/components/notes/attach-file-button";
import { ATTACHMENTS_LABEL } from "@/components/notes/attachments-panel";
import { NOTE_ACTIONS_LABEL } from "@/components/notes/note-actions";
import { ADD_NOTE_TAG, PROPERTIES_LABEL } from "@/components/notes/properties-panel";
import { resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import { CAPTURE_OPENING_LABEL, CaptureDocument, CaptureDraftDocument } from "./capture-document";

/**
 * jsdom does no layout, so CodeMirror's measure pass — which runs on any
 * animation frame that elapses while these tests hold a real `EditorView` —
 * would throw outside every `try` a test can write and take the run's exit code
 * while the summary still printed passes. Never hand-rolled: the shim this
 * replaces returned an empty rect list and threw anyway.
 */
let restoreRects: () => void;
beforeEach(() => {
  restoreRects = withRangeRects();
});
afterEach(() => {
  restoreRects();
  resetNotesEditorStoreForTest();
});

/** The key `capture-main.tsx` names the prewarmed hotkey window with. */
const DRAFT_KEY = "draft";

/** A capture note as `create_note` writes one, with a tag key to edit. */
const BLOCK = [
  "---",
  "id: 01CAPTUREPAGE",
  "tags:",
  "  - inbox",
  "keeper:",
  "  capture: true",
  "---",
  "",
].join("\n");

/** The page a fresh capture starts on: what a template scaffold looks like. */
const SCAFFOLD = "";

function page(id: string, notices: string[] = []): NoteCreateVm {
  return {
    note: { vaultId: "v1", id, path: `2026-08-10-untitled.md`, title: "Untitled" },
    notices,
  };
}

/** Open every `notes_open` on `body`, as Rust's reset snapshot delivers it. */
function serveBody(body: string): void {
  notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
    onBatch({
      kind: "reset",
      text: body,
      frontmatter: BLOCK,
      rev: "r0",
      cursor: body.length,
      path: "2026-08-10-untitled.md",
    } as NoteBodyBatch);
    return "sub-1";
  });
}

/** The live editor, once its lazy chunk has landed and the reset applied. */
async function liveEditor(): Promise<EditorView> {
  return await waitFor(
    () => {
      const host = document.querySelector<HTMLElement>(".cm-editor");
      expect(host).not.toBeNull();
      const found = EditorView.findFromDOM(host as HTMLElement);
      expect(found).not.toBeNull();
      return found as EditorView;
    },
    { timeout: 4000 },
  );
}

/**
 * Open the note's Actions menu (Story 46.5) and hand back its content.
 *
 * This window is 560px wide (`notes_window.rs:91`) and is the surface the
 * defect was filed against: the header used to carry six controls, the row
 * does not wrap, and the last of them — the one holding Delete — was off the
 * screen. Four of them live in this menu now, so the two the capture window
 * presses are reached through it.
 */
async function openNoteActions(): Promise<HTMLElement> {
  const trigger = await screen.findByRole("button", {
    name: new RegExp(`^${NOTE_ACTIONS_LABEL}`),
  });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.pointerUp(trigger, { button: 0 });
  return await screen.findByRole("menu");
}

beforeEach(() => {
  vi.clearAllMocks();
  onCaptureShown = null;
  notesCaptureDraft.mockResolvedValue(page("01CAPTUREPAGE"));
  notesCaptureHide.mockResolvedValue(undefined);
  notesClose.mockResolvedValue(undefined);
  notesSave.mockResolvedValue({
    rev: "r1",
    path: "2026-08-10-untitled.md",
    frontmatter: BLOCK,
    conflictCopy: null,
  });
  notesAttachSources.mockResolvedValue([]);
  pickFiles.mockResolvedValue(null);
  serveBody(SCAFFOLD);
});

// ---------------------------------------------------------------------------
// Door 1 — the hotkey window
// ---------------------------------------------------------------------------

// The capture panel mounts the real note editor, whose boot outlives a test
// that only needed the chrome; see the helper for the teardown race that costs.
afterEach(settleNoteEditorBoot);

describe("the quick-capture draft window", () => {
  it("mounts the real note editor on the note Rust resolved", async () => {
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);

    // Asserting the CALL, not only the result: `notesCaptureDraft` is a
    // `mockResolvedValue` and answers the same page whatever key it is handed,
    // so without this a capture window asking for the wrong window's draft —
    // two windows sharing one page, the Story 45.15 defect — would pass.
    await waitFor(() => {
      expect(notesCaptureDraft).toHaveBeenCalledWith(DRAFT_KEY);
    });

    // The real component, proved by a real `EditorView` in the DOM. A mocked
    // NoteEditor cannot produce one, and neither can a textarea.
    const editor = await liveEditor();
    expect(editor.state.doc.toString()).toBe(SCAFFOLD);

    // And it opened THAT note, not a plausible one.
    expect(notesOpen).toHaveBeenCalledWith("v1", "01CAPTUREPAGE", expect.any(Function));

    // The editor's own vocabulary arrived with it rather than being rebuilt:
    // these are `NoteEditor`'s controls, and nothing in this story renders them.
    expect(screen.getByLabelText("Bold")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: ATTACH_FILE_LABEL })).toBeInTheDocument();
    // Story 46.5: the note's panel verbs live in its Actions menu now, so the
    // header carries the menu and the menu carries Properties — as a
    // `menuitemcheckbox` since Story 49, because it discloses a panel and now
    // reports whether that panel is open. By role and not by bare name:
    // `PropertiesPanel`'s own `<section>` answers to the same word, and a name
    // query that resolved to the region would fail as "the item is missing"
    // when the item was fine.
    const menu = await openNoteActions();
    // Attachments, not Properties: Properties is a leading control now and is
    // never in this menu, so it can no longer stand for "the editor's own menu
    // is here".
    expect(
      within(menu).getByRole("menuitemcheckbox", { name: ATTACHMENTS_LABEL }),
    ).toBeInTheDocument();
  });

  it("renders every notice the create had to say", async () => {
    // Two, because a mutation keeping only the first would pass any one-item
    // fixture — and 44.6 states outright that two notices can arrive together.
    notesCaptureDraft.mockResolvedValue(
      page("01CAPTUREPAGE", [
        "keeper couldn't read the template inbox.md, so this note is plain.",
        "A new note can't satisfy is:recording, so this note won't appear in Recordings.",
      ]),
    );
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);

    const notices = await screen.findAllByRole("status");
    expect(notices.map((node) => node.textContent)).toEqual([
      "keeper couldn't read the template inbox.md, so this note is plain.",
      "A new note can't satisfy is:recording, so this note won't appear in Recordings.",
    ]);
  });

  it("keeps a notice while the page it belongs to is still on screen", async () => {
    // The re-resolve answers the SAME page with no notices, because no create
    // happened. Replacing the held page on every resolve would wipe the
    // sentence off the screen a second after the window was shown — and the
    // sentence is the only thing that says a template could not be read.
    notesCaptureDraft.mockResolvedValue(
      page("01CAPTUREPAGE", ["keeper couldn't read the template inbox.md, so this note is plain."]),
    );
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);
    await screen.findByRole("status");
    await waitFor(() => {
      expect(onCaptureShown).not.toBeNull();
    });

    notesCaptureDraft.mockResolvedValue(page("01CAPTUREPAGE"));
    onCaptureShown?.();
    await waitFor(() => {
      expect(notesCaptureDraft).toHaveBeenCalledTimes(2);
    });

    expect(screen.getByRole("status")).toHaveTextContent("couldn't read the template inbox.md");
  });

  it("drops the old page's notice when the page is torn off", async () => {
    notesCaptureDraft.mockResolvedValue(
      page("01CAPTUREPAGE", ["keeper couldn't read the template inbox.md, so this note is plain."]),
    );
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);
    await screen.findByRole("status");

    // A fresh page is a different note, so it must not inherit the sentence
    // that was said about the last one.
    notesCaptureDraft.mockResolvedValue(page("01FRESHPAGE"));
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("status")).toBeNull();
    });
  });

  it("carries a mark applied in capture into the bytes that are saved", async () => {
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);
    const editor = await liveEditor();

    // Type a thought and select it, exactly as a person would before pressing
    // a toolbar button.
    editor.dispatch({ changes: { from: 0, insert: "ring the dentist" } });
    editor.dispatch({ selection: { anchor: 0, head: "ring the dentist".length } });
    fireEvent.click(screen.getByLabelText("Bold"));

    await waitFor(() => {
      expect(editor.state.doc.toString()).toBe("**ring the dentist**");
    });

    // The mark is only real if it reaches the file. Escape is the dismissal,
    // and dismissal force-flushes before it hides (AD-62).
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => {
      expect(notesSave).toHaveBeenCalledWith("sub-1", "**ring the dentist**", "r0", undefined);
    });
  });

  it("puts a tag chosen in capture into the block that is written", async () => {
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);
    await liveEditor();

    // The header control, not the menu: Properties never enters the menu now.
    fireEvent.click(screen.getByRole("button", { name: PROPERTIES_LABEL }));
    fireEvent.click(await screen.findByRole("button", { name: ADD_NOTE_TAG }));
    const field = await screen.findByLabelText(ADD_NOTE_TAG);
    fireEvent.change(field, { target: { value: "errand" } });
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => {
      expect(notesSave).toHaveBeenCalled();
    });
    // The fourth argument is the frontmatter block; the tag has to be in it,
    // beside the one the note already had rather than instead of it.
    // Indexed rather than `.at(-1)`: this project's `lib` target predates
    // ES2022. And no `?? ""` — the assertion above proves there is a call, so
    // a fallback here could only turn a missing write into an empty string and
    // let the two `toContain`s below fail for the wrong reason.
    const calls = notesSave.mock.calls;
    const block = calls[calls.length - 1][3];
    expect(block).toContain("- inbox");
    expect(block).toContain("- errand");
  });

  it("inserts an attachment with the one spelling, at the caret", async () => {
    // Two files and not one: a mutation attaching only the first of a
    // selection reports one file that genuinely did go in, so a single-item
    // fixture cannot see it.
    const first = "/Users/alice/Movies/standup.mov";
    const second = "/Users/alice/Desktop/holiday.png";
    pickFiles.mockResolvedValue([first, second]);
    notesAttachSources.mockResolvedValue([
      { name: "standup.mov", relPath: "recordings/standup.mov", copied: false, refusal: null },
      { name: "holiday.png", relPath: "attachments/holiday.png", copied: true, refusal: null },
    ]);
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);
    const editor = await liveEditor();

    // Story 46.11: "Attach a file" is a dropdown trigger now, because the header
    // gained a second source and AD-104 leaves its action group at two controls.
    // The quick-capture window mounts the same header, so it gets the same two
    // doors — including the in-vault one, which is the point of putting them in
    // the control rather than in the 560px row.
    const trigger = screen.getByRole("button", { name: ATTACH_FILE_LABEL });
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.pointerUp(trigger, { button: 0 });
    const attachMenu = await screen.findByRole("menu");
    fireEvent.click(within(attachMenu).getByRole("menuitem", { name: ATTACH_FROM_COMPUTER_LABEL }));

    // The call, because `notesAttachSources` answers the same list whatever it
    // is handed: the vault the capture note lives in, and every picked path.
    await waitFor(() => {
      expect(notesAttachSources).toHaveBeenCalledWith("v1", [first, second]);
    });
    await waitFor(() => {
      expect(editor.state.doc.toString()).toBe(
        "![[recordings/standup.mov]]\n![[attachments/holiday.png]]",
      );
    });
    // FR-145: an absolute path may never reach a note.
    expect(editor.state.doc.toString()).not.toContain("/Users/");
  });

  it("saves, then hides, then arms the next page — in that order", async () => {
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);
    const editor = await liveEditor();
    editor.dispatch({ changes: { from: 0, insert: "ring the dentist" } });

    // The second resolve answers a DIFFERENT note: the page was written on, so
    // Rust tore it off.
    notesCaptureDraft.mockResolvedValue(page("01FRESHPAGE"));
    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => {
      expect(notesCaptureDraft).toHaveBeenCalledTimes(2);
    });
    // Ordering is the guarantee. Rust decides "was this page written on?" from
    // the bytes on disk, so the last 1.5 s of typing must be there before it is
    // asked — otherwise the page reads as untouched, is handed back, and the
    // next thought lands underneath this one.
    const saved = notesSave.mock.invocationCallOrder[0];
    const hid = notesCaptureHide.mock.invocationCallOrder[0];
    const rearmed = notesCaptureDraft.mock.invocationCallOrder[1];
    expect(saved).toBeLessThan(hid);
    expect(hid).toBeLessThan(rearmed);
    expect(notesSave).toHaveBeenCalledWith("sub-1", "ring the dentist", "r0", undefined);
  });

  it("swaps to the fresh page and closes the old subscription", async () => {
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);
    const editor = await liveEditor();
    editor.dispatch({ changes: { from: 0, insert: "filed" } });

    notesCaptureDraft.mockResolvedValue(page("01FRESHPAGE"));
    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => {
      expect(notesOpen).toHaveBeenCalledWith("v1", "01FRESHPAGE", expect.any(Function));
    });
    // The old page's subscription must not outlive the page: a channel still
    // pushing into a store pointed at another note is how one note's revision
    // gets stamped onto another.
    expect(notesClose).toHaveBeenCalledWith("sub-1");

    // Exactly one write for that page, and this is the assertion the
    // `saveOpenNote` doc comment claims and nothing enforced. `useNotesBody`'s
    // unmount flush fires when the note swaps; if the forced save had not
    // adopted its own acknowledgement, `dirty` would still be true and the
    // same text would go out again against the revision the first write has
    // already superseded — which Rust reads as somebody else's edit and
    // answers with a conflict copy, on a note the person just filed.
    expect(notesSave.mock.calls.filter((call) => call[0] === "sub-1")).toHaveLength(1);
  });

  it("keeps the same page, and the same editor, when nobody wrote on it", async () => {
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);
    const editor = await liveEditor();

    // Dismissed without typing: Rust hands the same untouched page back.
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => {
      expect(notesCaptureDraft).toHaveBeenCalledTimes(2);
    });

    // One open, not two. A remount here would throw away the caret and the
    // undo stack of a page the person is still holding — and, over a day of
    // idle hotkey presses, would leave one empty note behind per press.
    expect(notesOpen).toHaveBeenCalledTimes(1);
    expect(await liveEditor()).toBe(editor);
  });

  it("re-checks the page when the window is shown by something other than a dismissal", async () => {
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);
    await liveEditor();
    await waitFor(() => {
      expect(onCaptureShown).not.toBeNull();
    });

    // `listenNotesCaptureShown` was declared in Epic 36 and called from
    // nowhere until this story — the third of DW-172's dead listeners. It is
    // the belt for a show keeper's own dismissal did not precede.
    notesCaptureDraft.mockResolvedValue(page("01FRESHPAGE"));
    onCaptureShown?.();

    await waitFor(() => {
      expect(notesOpen).toHaveBeenCalledWith("v1", "01FRESHPAGE", expect.any(Function));
    });
  });

  it("does not dismiss when Escape was the editor's, not the window's", async () => {
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);
    await liveEditor();

    // What a completion popup does with Escape: handles it and marks the event
    // handled. Without the `defaultPrevented` guard, closing the `/` menu would
    // also throw the window away — a keystroke that destroys the surface the
    // person is in the middle of using.
    const handled = new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    });
    handled.preventDefault();
    window.dispatchEvent(handled);

    expect(notesCaptureHide).not.toHaveBeenCalled();

    // The very next unhandled Escape still dismisses, so the guard narrows the
    // chord rather than breaking it.
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => {
      expect(notesCaptureHide).toHaveBeenCalledTimes(1);
    });
  });

  it("dismisses on the close chord as well as on Escape", async () => {
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);
    await liveEditor();

    // `ctrlKey`, because `metaKey` matches nothing under jsdom — and the
    // handler reads both rather than the platform, which this app never does.
    fireEvent.keyDown(window, { key: "w", ctrlKey: true });
    await waitFor(() => {
      expect(notesCaptureHide).toHaveBeenCalledTimes(1);
    });
  });

  it("says a hide failed, and keeps the page rather than throwing the words away", async () => {
    notesCaptureHide.mockRejectedValue({
      code: "internal",
      message: "the capture window isn't there any more",
      accountId: null,
      retriable: false,
    });
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);
    const editor = await liveEditor();
    editor.dispatch({ changes: { from: 0, insert: "do not lose me" } });

    fireEvent.keyDown(window, { key: "Escape" });

    // Said out loud. A window that would not go away while the app looked like
    // it had filed the thought is the silent failure this epic is about.
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "the capture window isn't there any more",
    );
    // And the words are still on screen, in the same editor: a hide is the
    // window's business, and losing a note over it would be capture's one
    // unforgivable act.
    expect(editor.state.doc.toString()).toBe("do not lose me");
    expect(notesSave).toHaveBeenCalledWith("sub-1", "do not lose me", "r0", undefined);
  });

  it("stops saying the window would not hide once it does", async () => {
    // `mockImplementation`, not `mockRejectedValue`: the latter builds its
    // rejected promise when the mock is CONFIGURED rather than when it is
    // called, so a rejection nothing has reached yet is an unhandled rejection
    // sitting in the run — a green with an error in it.
    notesCaptureHide.mockImplementation(async () => {
      throw { code: "internal", message: "stuck", accountId: null, retriable: false };
    });
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);
    await liveEditor();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(await screen.findByRole("alert")).toHaveTextContent("stuck");

    // It hides this time. A sentence about a failure that has stopped
    // happening is a window permanently accusing itself of something, and the
    // next real one would be invisible underneath it.
    notesCaptureHide.mockImplementation(async () => {});
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("alert")).toBeNull();
    });
  });

  it("hands the window's chrome the same dismissal Escape performs", async () => {
    // Story 45.15's close button. A slot that arranged its own hide would be a
    // second spelling of one sentence — it would skip the force-flush that
    // makes Rust see the page as written on, so the next summon would land the
    // next thought underneath this one.
    render(
      <CaptureDraftDocument
        captureKey={DRAFT_KEY}
        chrome={(dismiss) => (
          <button type="button" onClick={dismiss}>
            Close
          </button>
        )}
      />,
    );
    const editor = await liveEditor();
    editor.dispatch({ changes: { from: 0, insert: "filed by the close button" } });

    notesCaptureDraft.mockResolvedValue(page("01FRESHPAGE"));
    fireEvent.click(screen.getByRole("button", { name: "Close" }));

    await waitFor(() => {
      expect(notesCaptureDraft).toHaveBeenCalledTimes(2);
    });
    // Byte-for-byte the Escape path: save, then hide, then re-arm.
    expect(notesSave).toHaveBeenCalledWith("sub-1", "filed by the close button", "r0", undefined);
    expect(notesSave.mock.invocationCallOrder[0]).toBeLessThan(
      notesCaptureHide.mock.invocationCallOrder[0],
    );
    expect(notesCaptureHide.mock.invocationCallOrder[0]).toBeLessThan(
      notesCaptureDraft.mock.invocationCallOrder[1],
    );
  });

  it("says it is opening a page, and stops saying it once there is one", async () => {
    // The positive witness for the absence asserted in the no-vault test
    // below. Without it that assertion is one half of a pair with nothing on
    // the other side: delete the loading branch entirely and it still passes,
    // and the label would be a string nothing on earth renders.
    // A holder rather than a bare `let`: the only assignment is inside the
    // executor, so TypeScript narrows a plain binding to `null` at the call.
    const handOver: { settle?: (resolved: NoteCreateVm) => void } = {};
    notesCaptureDraft.mockImplementation(
      () =>
        new Promise<NoteCreateVm>((settle) => {
          handOver.settle = settle;
        }),
    );
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);

    expect(await screen.findByText(CAPTURE_OPENING_LABEL)).toBeInTheDocument();
    expect(document.querySelector(".cm-editor")).toBeNull();

    handOver.settle?.(page("01CAPTUREPAGE"));
    await liveEditor();
    expect(screen.queryByText(CAPTURE_OPENING_LABEL)).toBeNull();
  });

  it("does not hide when the write was refused, and says why where the person is", async () => {
    // `saveOpenNote` catches its own failure, so `await`ing it is not a success
    // check — the caller has to read the answer. Without that, a refused write
    // takes the panel away with the reason legible only inside the window that
    // just disappeared, and then asks Rust "was this page written on?" about
    // bytes that never reached the disk — which answers no, hands the same page
    // back, and leaves the person told nothing. UX-DR35's error branch.
    notesSave.mockImplementation(async () => {
      throw {
        code: "internal",
        message: "vault folder isn't there any more",
        accountId: null,
        retriable: false,
      };
    });
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);
    const editor = await liveEditor();
    editor.dispatch({ changes: { from: 0, insert: "do not lose me" } });

    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => {
      expect(notesSave).toHaveBeenCalledWith("sub-1", "do not lose me", "r0", undefined);
    });
    // The panel stays, the words stay, and the page is not torn off — the note
    // Rust would hand back is the one whose bytes never changed.
    expect(notesCaptureHide).not.toHaveBeenCalled();
    expect(notesCaptureDraft).toHaveBeenCalledTimes(1);
    expect(editor.state.doc.toString()).toBe("do not lose me");
    // And the reason is on screen, in the window that is still open. It is the
    // editor's own caption, because one write must not grow two error channels.
    expect(await screen.findByText(/vault folder isn't there any more/)).toBeInTheDocument();
  });

  it("gives the typing surface a name, which the textarea it replaced had", async () => {
    // The contract the deleted panel kept and nobody wrote down. Its textarea
    // carried `aria-label="Quick capture"`; CodeMirror's content is
    // `role="textbox"` with no name at all, so porting the panel to the real
    // editor would have left a screen-reader user summoning the window into an
    // unlabelled text box. There was no failing test and no diff line to
    // review — the promise existed only as an attribute on code that was
    // deleted wholesale.
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);
    await liveEditor();

    // Queried by role and name, which is what a screen reader does, rather
    // than by reading the attribute off the node this file put it on.
    expect(screen.getByRole("textbox", { name: "Note" })).toHaveAttribute(
      "contenteditable",
      "true",
    );
  });

  it("says there is nowhere to put a thought instead of taking keystrokes", async () => {
    notesCaptureDraft.mockRejectedValue({
      code: "unsupported",
      message: "no notes vault yet — flag a folder you already sync and it becomes one",
      accountId: null,
      retriable: false,
    });
    render(<CaptureDraftDocument captureKey={DRAFT_KEY} />);

    expect(await screen.findByRole("alert")).toHaveTextContent("no notes vault yet");
    // No editor, because there is no note. A window that accepted words it
    // could not keep would be the failure capture exists to prevent.
    expect(document.querySelector(".cm-editor")).toBeNull();
    expect(screen.queryByText(CAPTURE_OPENING_LABEL)).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Door 2 — a window opened on a note that already exists (Story 45.15)
// ---------------------------------------------------------------------------

describe("a capture window opened on an existing note", () => {
  it("shows that note in the same editor, resolving nothing", async () => {
    serveBody("alpha\n");
    render(<CaptureDocument vaultId="v2" noteId="01OLDNOTE" />);

    const editor = await liveEditor();
    expect(editor.state.doc.toString()).toBe("alpha\n");
    expect(notesOpen).toHaveBeenCalledWith("v2", "01OLDNOTE", expect.any(Function));
    // A note that already exists was not created, so nothing asks Rust for a
    // page — that is what makes "any note openable as a capture window" a prop
    // rather than a second surface.
    expect(notesCaptureDraft).not.toHaveBeenCalled();
  });

  it("renders no notice strip when there was no create to have anything to say", async () => {
    render(<CaptureDocument vaultId="v2" noteId="01OLDNOTE" />);
    await liveEditor();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("is the same editor, with the same vocabulary, on the note it was opened on", async () => {
    // Door count, honestly read: every behaviour test above enters through the
    // draft window, so "the editor works in a capture window" was a claim about
    // ONE of the two windows this file exports. A branch reachable only from a
    // second host cannot be reached by tests that all route through the first.
    serveBody("alpha\n");
    render(<CaptureDocument vaultId="v2" noteId="01OLDNOTE" />);
    const editor = await liveEditor();

    editor.dispatch({ selection: { anchor: 0, head: 5 } });
    fireEvent.click(screen.getByLabelText("Bold"));
    await waitFor(() => {
      expect(editor.state.doc.toString()).toBe("**alpha**\n");
    });

    // And it reaches this note's file, not the draft's: the write carries the
    // subscription `notesOpen` handed back for `01OLDNOTE`.
    fireEvent.keyDown(editor.contentDOM, { key: "s", ctrlKey: true });
    await waitFor(() => {
      expect(notesSave).toHaveBeenCalledWith("sub-1", "**alpha**\n", "r0", undefined);
    });
  });
});

// ---------------------------------------------------------------------------
// Door 3 — the webview root, which is the one `capture.html` actually boots
// ---------------------------------------------------------------------------

describe("the capture webview's root", () => {
  it("mounts the draft document, so the window is the editor and not a textarea", async () => {
    // No query string: the prewarmed hotkey window, which is what
    // `capture.html` boots with. Story 45.15's `?vault=&note=` branch is that
    // story's own test.
    render(<CapturePanel search="" />);

    // DW-172: the two tests above render `CaptureDraftDocument` themselves and
    // could never see that the root does not. This one can.
    await waitFor(() => {
      expect(notesCaptureDraft).toHaveBeenCalledWith(DRAFT_KEY);
    });
    await liveEditor();
    // The surface this story replaced, asserted absent: a textarea left mounted
    // beside the editor would be two editors again, which is the whole defect.
    expect(document.querySelector("textarea")).toBeNull();
  });

  it("asks for the same window key the draft document does", async () => {
    render(<CapturePanel search="" />);
    await waitFor(() => {
      expect(notesCaptureDraft).toHaveBeenCalledTimes(1);
    });
    // Spelled out rather than compared to a constant this file imports from the
    // component under test: a root that asked for `""` or for `undefined` would
    // resolve some other window's page, and a test that read the key from the
    // same place the code does could not tell.
    expect(notesCaptureDraft.mock.calls[0]?.[0]).toBe("draft");
  });
});
