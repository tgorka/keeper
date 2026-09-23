/**
 * Story 46.12, the part a store test cannot reach: **two real note editors,
 * mounted at the same time, over two different notes.**
 *
 * `src/lib/stores/notes-editor.test.ts` proves the reducer keeps two documents
 * apart. That is necessary and it is not the claim the owner made, which was
 * about two notes on screen. Between the two sits everything this file exercises
 * and that one does not: two lazy CodeMirror boots racing each other, two
 * `useNotesBody` subscriptions, two sets of autosave and heartbeat timers, and
 * two blur handlers on one document — the lifecycle the panel limit existed to
 * prevent anyone from mounting.
 *
 * So the editors here are the product's own. Every read goes back through
 * `onEdit` into the store, which is the buffer that would reach the file, and
 * every write is asserted on `notesSave`'s SUBSCRIPTION ARGUMENT — because the
 * failure this story removes is not "the wrong text is on screen", it is "the
 * right text went down the wrong channel", and only the argument shows that.
 */
import { EditorView } from "@codemirror/view";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteBodyBatch, NoteReleaseReq } from "@/lib/ipc/client";
import {
  activePanel,
  panelsStore,
  resetPanelsStoreForTest,
  usePanelsStore,
} from "@/lib/stores/panels";
import { withRangeRects } from "@/test/layout";

const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();
const notesClose =
  vi.fn<(subscriptionId: string, release: NoteReleaseReq | null) => Promise<boolean>>();
const notesSave =
  vi.fn<
    (
      subscriptionId: string,
      text: string,
      baseRev: string,
    ) => Promise<{ frontmatter: string; rev: string; path: string; conflictCopy: string | null }>
  >();
const notesBufferReport =
  vi.fn<(subscriptionId: string, text: string, rev: string) => Promise<void>>();

vi.mock("@/lib/ipc/client", () => ({
  notesOpen: (v: string, n: string, on: (b: NoteBodyBatch) => void) => notesOpen(v, n, on),
  notesClose: (id: string, release: NoteReleaseReq | null) => notesClose(id, release),
  notesSave: (id: string, text: string, rev: string) => notesSave(id, text, rev),
  notesBufferReport: (id: string, text: string, rev: string) => notesBufferReport(id, text, rev),
  notesTagTree: vi.fn(async () => ({ nodes: [] })),
  notesBacklinks: vi.fn(async () => []),
  notesResolveConflict: vi.fn(async () => {}),
  notesMarkRead: vi.fn(async () => {}),
  notesDiff: vi.fn(async () => null),
  notesHistory: vi.fn(async () => []),
  recordingNoteTargets: vi.fn(async () => null),
  recordingOpenPath: vi.fn(async () => {}),
  revealPath: vi.fn(async () => {}),
}));

import { NOTE_AUTOSAVE_IDLE_MS } from "@/hooks/use-notes-body";
import { readNoteDocument, resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import { NoteEditor } from "./note-editor";

const VAULT = "v1";
const ONE = "note-one";
const TWO = "note-two";
const ONE_BODY = "the first note\n";
const TWO_BODY = "the second note\n";

/** Which body each note's channel opens with, and which subscription it is. */
const BODIES: Record<string, string> = { [ONE]: ONE_BODY, [TWO]: TWO_BODY };
const SUBSCRIPTIONS: Record<string, string> = { [ONE]: "sub-one", [TWO]: "sub-two" };

// jsdom has no `Range.getClientRects` and CodeMirror's measure pass calls it on
// any animation frame that elapses mid-test. Paired, because `Range.prototype`
// is shared with every test in this file.
let removeRangeRects: (() => void) | null = null;
beforeAll(() => {
  removeRangeRects = withRangeRects();
});
afterAll(() => {
  removeRangeRects?.();
});

beforeEach(() => {
  vi.clearAllMocks();
  notesOpen.mockImplementation(async (_vault, noteId, onBatch) => {
    onBatch({
      kind: "reset",
      text: BODIES[noteId] ?? "",
      frontmatter: "",
      rev: `rev-${noteId}`,
      cursor: null,
      path: `notes/${noteId}.md`,
    });
    return SUBSCRIPTIONS[noteId] ?? "sub-unknown";
  });
  notesClose.mockResolvedValue(false);
  notesSave.mockImplementation(async (_id, _text, _rev) => ({
    frontmatter: "",
    rev: "rev-saved",
    path: "notes/saved.md",
    conflictCopy: null,
  }));
  notesBufferReport.mockResolvedValue(undefined);
});

afterEach(() => {
  // Unconditional, and not in a `finally` inside the two tests that install
  // them: a test that times out never reaches its own `finally`, and fake
  // timers left installed make every test after it hang on `waitFor` — four
  // reds for one cause, none of them pointing at it.
  vi.useRealTimers();
  resetNotesEditorStoreForTest();
  resetPanelsStoreForTest();
});

/**
 * The two panels, as a component, so a test can take one away.
 *
 * `showOne` is how this file spells "the panel was closed or folded": a folded
 * panel renders no body at all (`panel-strip.tsx` does this deliberately, so a
 * fold really does release the document), and a closed one is gone. Both are
 * an unmount of the editor, which is the lifecycle under test.
 */
function Panels({ showOne = true }: { showOne?: boolean }) {
  return (
    <>
      {showOne ? (
        <div data-testid="panel-one">
          <NoteEditor vaultId={VAULT} noteId={ONE} />
        </div>
      ) : null}
      <div data-testid="panel-two">
        <NoteEditor vaultId={VAULT} noteId={TWO} />
      </div>
    </>
  );
}

/** Both editors, side by side, once each has its document and its view. */
async function mountBoth(): Promise<{
  one: EditorView;
  two: EditorView;
  closeOne: () => void;
}> {
  const { rerender } = render(<Panels />);
  const views = await waitFor(() => {
    const one = viewIn("panel-one");
    const two = viewIn("panel-two");
    // Each editor booted over ITS note's body. Asserted before anything is
    // typed, because a shared mirror would already have failed here: the second
    // channel's `reset` would have overwritten the first's document.
    expect(one.state.doc.toString()).toBe(ONE_BODY);
    expect(two.state.doc.toString()).toBe(TWO_BODY);
    return { one, two };
  });
  return { ...views, closeOne: () => rerender(<Panels showOne={false} />) };
}

/** The CodeMirror view the app built inside one panel. `findFromDOM`, so the
 *  view under test is the product's and not one this file configured. */
function viewIn(testId: string): EditorView {
  const host = screen.getByTestId(testId).querySelector<HTMLElement>(".cm-content");
  expect(host).not.toBeNull();
  const view = EditorView.findFromDOM(host as HTMLElement);
  expect(view).not.toBeNull();
  return view as EditorView;
}

/** Type at the end of a view, as the user's own edit — reported through
 *  `onEdit` exactly as a keystroke is. */
function type(view: EditorView, text: string): void {
  act(() => {
    view.dispatch({ changes: { from: view.state.doc.length, insert: text } });
  });
}

describe("two notes open at once", () => {
  it("keeps a separate buffer and a separate dirty flag", async () => {
    const { one, two } = await mountBoth();

    type(one, "typed into the first\n");

    await waitFor(() => {
      expect(readNoteDocument(VAULT, ONE).text).toBe(`${ONE_BODY}typed into the first\n`);
    });
    expect(readNoteDocument(VAULT, ONE).dirty).toBe(true);

    // The other editor still shows its own note, and the store agrees. Both
    // halves matter: the view is what the reader sees and the buffer is what
    // would be written, and the singleton broke them together.
    expect(two.state.doc.toString()).toBe(TWO_BODY);
    expect(readNoteDocument(VAULT, TWO).text).toBe(TWO_BODY);
    expect(readNoteDocument(VAULT, TWO).dirty).toBe(false);
  });

  it("opens one channel per note and gives each editor its own subscription", async () => {
    await mountBoth();

    expect(notesOpen).toHaveBeenCalledTimes(2);
    expect(readNoteDocument(VAULT, ONE).subscriptionId).toBe("sub-one");
    expect(readNoteDocument(VAULT, TWO).subscriptionId).toBe("sub-two");
  });

  it("autosaves only the note that was edited, down only that note's channel", async () => {
    // Mounted on real timers and only then switched: the editor's boot is a
    // dynamic `import()`, and a `waitFor` over a promise chain while the clock
    // is frozen never resolves.
    const { one } = await mountBoth();
    vi.useFakeTimers();

    type(one, "words worth keeping\n");
    await act(async () => {
      vi.advanceTimersByTime(NOTE_AUTOSAVE_IDLE_MS + 1);
    });

    // The subscription argument is the assertion. A save that carried the right
    // text on the wrong channel would put one note's paragraph into the other
    // note's file, which is the data loss the panel limit was holding back —
    // and it would look completely correct on screen.
    expect(notesSave).toHaveBeenCalledExactlyOnceWith(
      "sub-one",
      `${ONE_BODY}words worth keeping\n`,
      `rev-${ONE}`,
    );
    expect(readNoteDocument(VAULT, TWO).dirty).toBe(false);
  });

  it("reports the heartbeat for the note that is being typed in", async () => {
    const { two } = await mountBoth();
    vi.useFakeTimers();

    type(two, "still thinking\n");
    await act(async () => {
      vi.advanceTimersByTime(500);
    });

    expect(notesBufferReport).toHaveBeenCalledExactlyOnceWith(
      "sub-two",
      `${TWO_BODY}still thinking\n`,
      `rev-${TWO}`,
    );
  });

  it("saves the editor that lost focus, not whichever note the store touched last", async () => {
    const { one, two } = await mountBoth();
    // The second note is dirty and stays open. With one mirror, "blur" meant
    // "save the open note" and the open note was whichever editor mounted last,
    // so clicking out of the first would have written the second.
    type(two, "not finished\n");
    type(one, "finished\n");
    await waitFor(() => {
      expect(readNoteDocument(VAULT, ONE).dirty).toBe(true);
    });

    fireEvent.blur(one.contentDOM);

    await waitFor(() => {
      expect(notesSave).toHaveBeenCalledExactlyOnceWith(
        "sub-one",
        `${ONE_BODY}finished\n`,
        `rev-${ONE}`,
      );
    });
    expect(readNoteDocument(VAULT, TWO).dirty).toBe(true);
  });
});

describe("closing one of two panels", () => {
  it("releases that note with its unflushed words in ONE close and leaves the other alone", async () => {
    const { one, closeOne } = await mountBoth();
    type(one, "unflushed\n");
    await waitFor(() => {
      expect(readNoteDocument(VAULT, ONE).dirty).toBe(true);
    });

    act(closeOne);

    // The words that had not reached the disk ride the close itself, on their
    // own channel and against their own revision — a save sent beside it is a
    // second IPC call Rust could run after the close and refuse.
    await waitFor(() => {
      expect(notesClose).toHaveBeenCalledExactlyOnceWith("sub-one", {
        text: `${ONE_BODY}unflushed\n`,
        baseRev: `rev-${ONE}`,
        // No panel targets the note in this harness, so nothing keeps it.
        discard: true,
      });
    });
    expect(notesSave).not.toHaveBeenCalled();
    // And the surviving note kept its document, its channel and its buffer.
    // Before 46.12 the leaving editor's cleanup called `closeNote()`, which
    // emptied the one mirror both editors were reading — so closing a panel
    // blanked the note in the panel beside it.
    expect(readNoteDocument(VAULT, TWO).subscriptionId).toBe("sub-two");
    expect(readNoteDocument(VAULT, TWO).text).toBe(TWO_BODY);
    expect(screen.getByTestId("panel-two")).toBeInTheDocument();
  });

  it("releases a clean editor with no words", async () => {
    const { closeOne } = await mountBoth();
    act(closeOne);
    await waitFor(() => {
      expect(notesClose).toHaveBeenCalledExactlyOnceWith("sub-one", {
        text: null,
        baseRev: `rev-${ONE}`,
        discard: true,
      });
    });
  });
});

const NOTE_ONE = { kind: "note", vaultId: VAULT, noteId: ONE } as const;
const NOTE_TWO = { kind: "note", vaultId: VAULT, noteId: TWO } as const;

/**
 * The panel strip's lifecycle without its chrome: one editor per unfolded note
 * panel, driven by the real panel store. `panel-strip.tsx` renders no body for
 * a folded panel, which is what makes a fold an unmount.
 */
function Strip() {
  const panels = usePanelsStore((state) => state.panels);
  return (
    <>
      {panels.map((panel) =>
        panel.folded || panel.target?.kind !== "note" ? null : (
          <div key={panel.id} data-testid={panel.id}>
            <NoteEditor vaultId={panel.target.vaultId} noteId={panel.target.noteId} />
          </div>
        ),
      )}
    </>
  );
}

/** Rust's side of AD-304: an untouched new note goes only when it may. */
function removeWhenDiscarded(): void {
  notesClose.mockImplementation(async (_id, release) => release?.discard === true);
}

describe("letting go of a new note nobody wrote in (AD-304)", () => {
  it("folding the only panel on it flushes without discarding, and the panel keeps it", async () => {
    removeWhenDiscarded();
    panelsStore.getState().setActiveTarget(NOTE_ONE);
    const [panel] = panelsStore.getState().panels;
    render(<Strip />);
    await waitFor(() => expect(viewIn(panel.id).state.doc.toString()).toBe(ONE_BODY));

    act(() => panelsStore.getState().toggleFold(panel.id));

    await waitFor(() => {
      expect(notesClose).toHaveBeenCalledExactlyOnceWith("sub-one", {
        text: null,
        baseRev: `rev-${ONE}`,
        discard: false,
      });
    });
    await act(async () => {});
    expect(panelsStore.getState().panels).toEqual([
      expect.objectContaining({ id: panel.id, target: NOTE_ONE, folded: true }),
    ]);
  });

  it("moving the panel to another note discards it, and the removal leaves history", async () => {
    removeWhenDiscarded();
    panelsStore.getState().setActiveTarget(NOTE_ONE);
    const [panel] = panelsStore.getState().panels;
    render(<Strip />);
    await waitFor(() => expect(viewIn(panel.id).state.doc.toString()).toBe(ONE_BODY));

    act(() => panelsStore.getState().setActiveTarget(NOTE_TWO));

    await waitFor(() => {
      expect(notesClose).toHaveBeenCalledExactlyOnceWith("sub-one", {
        text: null,
        baseRev: `rev-${ONE}`,
        discard: true,
      });
    });
    // No step back leads onto the note Rust removed.
    await waitFor(() => expect(activePanel(panelsStore.getState()).back).toEqual([]));
    expect(activePanel(panelsStore.getState()).target).toEqual(NOTE_TWO);
  });

  it("left before its open resolved, it is still released and may go", async () => {
    removeWhenDiscarded();
    const opens = notesOpen.getMockImplementation();
    // The executor form: this project's `lib` predates `Promise.withResolvers`.
    let resolveOne: (id: string) => void = () => {};
    notesOpen.mockImplementation(async (vault, noteId, onBatch) => {
      if (noteId === ONE) return await new Promise<string>((resolve) => (resolveOne = resolve));
      if (opens === undefined) throw new Error("expected the default open");
      return await opens(vault, noteId, onBatch);
    });
    panelsStore.getState().setActiveTarget(NOTE_ONE);
    render(<Strip />);
    await waitFor(() => expect(notesOpen).toHaveBeenCalledWith(VAULT, ONE, expect.any(Function)));

    // ⌘⌥N again, before the first note's channel answered.
    act(() => panelsStore.getState().setActiveTarget(NOTE_TWO));
    await act(async () => resolveOne("sub-one"));

    await waitFor(() => {
      expect(notesClose).toHaveBeenCalledExactlyOnceWith("sub-one", {
        text: null,
        baseRev: "",
        discard: true,
      });
    });
    await waitFor(() => expect(activePanel(panelsStore.getState()).back).toEqual([]));
  });
});

describe("a note panel whose note is not there", () => {
  function openFails(error: { code: string; message: string }): void {
    const opens = notesOpen.getMockImplementation();
    notesOpen.mockImplementation(async (vault, noteId, onBatch) => {
      if (noteId === ONE) throw error;
      if (opens === undefined) throw new Error("expected the default open");
      return await opens(vault, noteId, onBatch);
    });
    // TWO beside ONE, so closing ONE's panel is a real close and not a blank.
    panelsStore.getState().setActiveTarget(NOTE_TWO);
    panelsStore.getState().openPanel(NOTE_ONE);
  }
  const targets = () => panelsStore.getState().panels.map((panel) => panel.target);

  it("closes when opening it answers that the note does not exist", async () => {
    // What `notes_error` makes of `NotesError::NotFound` in keeper's notes_ipc.rs.
    openFails({ code: "internal", message: `no such note: ${ONE}` });

    render(<Strip />);

    await waitFor(() => expect(targets()).toEqual([NOTE_TWO]));
  });

  it("stays, showing why, when opening it failed for another reason", async () => {
    openFails({ code: "internal", message: "the disk refused" });

    render(<Strip />);

    await waitFor(() => expect(readNoteDocument(VAULT, ONE).error).toBe("the disk refused"));
    await act(async () => {});
    expect(targets()).toEqual([NOTE_TWO, NOTE_ONE]);
  });
});

/**
 * The panel model lets a single click retarget one panel onto what another
 * already shows, so two panels CAN hold the same note. That must be one
 * document with two views and never two buffers over one file — which would be
 * the singleton's data loss rebuilt one level down, with the two halves fighting
 * over the same path through the conflict machinery instead of over one store.
 */
describe("two panels showing the same note", () => {
  function SameNote({ both = true }: { both?: boolean }) {
    return (
      <>
        <div data-testid="panel-a">
          <NoteEditor vaultId={VAULT} noteId={ONE} />
        </div>
        {both ? (
          <div data-testid="panel-b">
            <NoteEditor vaultId={VAULT} noteId={ONE} />
          </div>
        ) : null}
      </>
    );
  }

  it("open one channel, share one buffer, and close nothing until the last goes", async () => {
    const { rerender } = render(<SameNote />);
    await waitFor(() => {
      expect(viewIn("panel-a").state.doc.toString()).toBe(ONE_BODY);
      expect(viewIn("panel-b").state.doc.toString()).toBe(ONE_BODY);
    });

    // One note, one subscription, however many editors are looking at it.
    expect(notesOpen).toHaveBeenCalledExactlyOnceWith(VAULT, ONE, expect.any(Function));
    expect(readNoteDocument(VAULT, ONE).views).toBe(2);

    type(viewIn("panel-a"), "typed in the left one\n");
    await waitFor(() => {
      expect(readNoteDocument(VAULT, ONE).text).toBe(`${ONE_BODY}typed in the left one\n`);
    });

    // Closing one of them is not closing the note.
    act(() => rerender(<SameNote both={false} />));
    expect(notesClose).not.toHaveBeenCalled();
    expect(readNoteDocument(VAULT, ONE).views).toBe(1);
    expect(readNoteDocument(VAULT, ONE).text).toBe(`${ONE_BODY}typed in the left one\n`);
    expect(readNoteDocument(VAULT, ONE).subscriptionId).toBe("sub-one");
  });

  /** Both views, booted over the note's body. */
  async function mountSame(): Promise<{ a: EditorView; b: EditorView }> {
    render(<SameNote />);
    return await waitFor(() => {
      const a = viewIn("panel-a");
      const b = viewIn("panel-b");
      expect(a.state.doc.toString()).toBe(ONE_BODY);
      expect(b.state.doc.toString()).toBe(ONE_BODY);
      return { a, b };
    });
  }

  it("what one view types and saves the other shows, and typing there keeps it", async () => {
    const { a, b } = await mountSame();

    type(a, "typed in A\n");
    fireEvent.blur(a.contentDOM);
    await waitFor(() => expect(readNoteDocument(VAULT, ONE).savedAtMs).not.toBeNull());

    expect(b.state.doc.toString()).toBe(`${ONE_BODY}typed in A\n`);

    type(b, "typed in B\n");

    // The buffer the next autosave writes: B's keystroke did not report a
    // document without A's words.
    const both = `${ONE_BODY}typed in A\ntyped in B\n`;
    expect(readNoteDocument(VAULT, ONE).text).toBe(both);
    expect(a.state.doc.toString()).toBe(both);
  });

  it("keystrokes typed in one view while its save is in flight survive the ack in both", async () => {
    // The executor form: this project's `lib` predates `Promise.withResolvers`.
    let ack: () => void = () => {};
    notesSave.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          ack = () =>
            resolve({
              frontmatter: "",
              rev: "rev-saved",
              path: "notes/saved.md",
              conflictCopy: null,
            });
        }),
    );
    const { a, b } = await mountSame();
    type(a, "first\n");
    fireEvent.blur(a.contentDOM);
    await waitFor(() => expect(notesSave).toHaveBeenCalledOnce());

    type(a, "second\n");
    await act(async () => ack());

    const both = `${ONE_BODY}first\nsecond\n`;
    await waitFor(() => expect(readNoteDocument(VAULT, ONE).saving).toBe(false));
    expect(a.state.doc.toString()).toBe(both);
    expect(b.state.doc.toString()).toBe(both);
    expect(readNoteDocument(VAULT, ONE)).toMatchObject({ text: both, dirty: true });
  });
});
