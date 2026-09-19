/**
 * A capture window's chrome (Story 45.15, FR-191, FR-192, UX-DR77, AD-260).
 *
 * Story 75.3 moved this chrome out of a strip of its own and into the editor
 * header's frame group. Four of the tests below were written against the strip
 * and are kept rather than replaced, because what they assert is not where the
 * controls are but what travels with them: the close button stays last, the
 * drag region is conditional on the lock, and DW-199's corner inset is still
 * the number Rust measured. Each says so where it differs from what it used to
 * say.
 *
 * Two things this file is careful about, both from wave 2's audit:
 *
 * - **Assert the call, not only the render.** Every control here ends in an IPC
 *   call carrying a key, and a mock resolves the same value whatever key it is
 *   handed. A test that presses a button and checks the button is checking the
 *   button.
 * - **Two windows in the fixture, always.** The story's headline is *several*
 *   capture windows, and a mutation that hands every control the first
 *   window's key passes every single-window test while making the second
 *   window's close button close somebody else's window.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CaptureWindowVm } from "@/lib/ipc/client";
import { settleNoteEditorBoot } from "@/test/note-editor-boot";

const notesCaptureWindows = vi.fn<() => Promise<CaptureWindowVm[]>>();
const notesCaptureOpen = vi.fn<(target: unknown) => Promise<void>>();
const notesCaptureClose = vi.fn<(key: string) => Promise<void>>();
const notesCaptureSetLocked = vi.fn<(key: string, locked: boolean) => Promise<void>>();
const notesCaptureSetAlwaysOnTop = vi.fn<(key: string, alwaysOnTop: boolean) => Promise<void>>();
const listenNotesCaptureWindows = vi.fn<(onChanged: () => void) => Promise<() => void>>();

vi.mock("@/lib/ipc/client", () => ({
  notesCaptureWindows: () => notesCaptureWindows(),
  notesCaptureOpen: (target: unknown) => notesCaptureOpen(target),
  notesCaptureClose: (key: string) => notesCaptureClose(key),
  notesCaptureSetLocked: (key: string, locked: boolean) => notesCaptureSetLocked(key, locked),
  notesCaptureSetAlwaysOnTop: (key: string, alwaysOnTop: boolean) =>
    notesCaptureSetAlwaysOnTop(key, alwaysOnTop),
  listenNotesCaptureWindows: (onChanged: () => void) => listenNotesCaptureWindows(onChanged),
}));

/**
 * Story 46.12: the save is addressed to a note. The spy takes the pair, so this
 * file can assert that the window closing is the one whose note was written —
 * which is the prop boundary it exists to guard, one level down.
 */
const saveNote = vi.fn<(vaultId: string, noteId: string) => Promise<boolean>>();
vi.mock("@/hooks/use-notes-body", () => ({
  saveNote: (vaultId: string, noteId: string) => saveNote(vaultId, noteId),
}));

/**
 * The document is somebody else's component (Story 45.14) and mounting the real
 * `NoteEditor` here would test their story with this story's fixtures. What
 * this file owes is the PROP BOUNDARY: that each window hands its own note
 * down, which is the half a rendered editor would hide rather than reveal.
 *
 * Since Story 75.3 the boundary carries two more things — the chrome, as the
 * editor header's frame group, and the title-bar flag that goes with it — so
 * the stub renders `frame` rather than dropping it. Dropping it would take the
 * close button off screen and turn every dismissal test in this file into a
 * test of `getByRole` throwing.
 */
const documentProps =
  vi.fn<
    (props: { vaultId: string; noteId: string; titleBar?: unknown; frame?: ReactNode }) => void
  >();
vi.mock("@/components/capture/capture-document", () => ({
  CaptureDocument: (props: {
    vaultId: string;
    noteId: string;
    titleBar?: unknown;
    frame?: ReactNode;
  }) => {
    documentProps(props);
    return <div data-testid={`document-${props.vaultId}-${props.noteId}`}>{props.frame}</div>;
  },
}));

import {
  CAPTURE_CLOSE_LABEL,
  CAPTURE_LOCK_LABEL,
  CAPTURE_PIN_LABEL,
  CAPTURE_UNLOCK_LABEL,
  CAPTURE_UNPIN_LABEL,
  CaptureNoteWindow,
  CaptureWindowChrome,
  useCaptureDismissKeys,
} from "@/components/capture/capture-window";
import { resetCaptureWindowsStoreForTest } from "@/lib/stores/capture-windows";

const FIRST: CaptureWindowVm = {
  key: "note:v1/n1",
  target: { kind: "note", vaultId: "v1", noteId: "n1" },
  locked: true,
  visible: true,
  // On top, which every capture window was before Story 48.4 and which this
  // fixture keeps so the toggle's two states are both live in one list.
  alwaysOnTop: true,
  // Locked, so tao hit-tests no resize edge and there is no border to dodge.
  chromeInset: 0,
};

const SECOND: CaptureWindowVm = {
  key: "note:v1/n2",
  target: { kind: "note", vaultId: "v1", noteId: "n2" },
  locked: false,
  visible: true,
  alwaysOnTop: false,
  // Unlocked on a 2x GTK display: `scale_factor() * 5`.
  chromeInset: 10,
};

/**
 * A third window on a 3x display, which exists only because Story 75.3 made
 * the right-hand inset a SUBTRACTION.
 *
 * The chrome now sits at the end of a header with 12px of gutter, so at 1x (5)
 * and at 2x (10) the buttons are already clear of the resize border and the
 * padding is zero. 15 is the first scale where any of it is left over, and
 * without this fixture "subtract the gutter" and "never pad at all" would be
 * the same two passing tests.
 */
const THIRD: CaptureWindowVm = {
  key: "note:v1/n3",
  target: { kind: "note", vaultId: "v1", noteId: "n3" },
  locked: false,
  visible: true,
  alwaysOnTop: false,
  chromeInset: 15,
};

beforeEach(() => {
  vi.clearAllMocks();
  resetCaptureWindowsStoreForTest();
  notesCaptureWindows.mockResolvedValue([FIRST, SECOND, THIRD]);
  notesCaptureClose.mockResolvedValue(undefined);
  notesCaptureSetLocked.mockResolvedValue(undefined);
  notesCaptureSetAlwaysOnTop.mockResolvedValue(undefined);
  saveNote.mockResolvedValue(true);
  listenNotesCaptureWindows.mockResolvedValue(() => {});
});

// The capture panel mounts the real note editor, whose boot outlives a test
// that only needed the chrome; see the helper for the teardown race that costs.
afterEach(settleNoteEditorBoot);

describe("CaptureWindowChrome", () => {
  it("dismisses through the act it was handed, not through one of its own", async () => {
    // The close button and Escape must be the same act, and the only way to
    // guarantee that is for the strip not to own one. Rendered with a real
    // callback and pressed, rather than checked for existence.
    const onClose = vi.fn();
    render(<CaptureWindowChrome captureKey="note:v1/n1" onClose={onClose} />);
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_CLOSE_LABEL }));
    expect(onClose).toHaveBeenCalledTimes(1);
    // And it decides nothing about what dismissal means.
    expect(notesCaptureClose).not.toHaveBeenCalled();
  });

  it("locks and unlocks the window it belongs to, with the value it is toggling to", async () => {
    render(<CaptureWindowChrome captureKey="note:v1/n1" onClose={() => {}} />);
    // FIRST is locked, so the control offers to unlock — the label is the state
    // it moves to, which is what a person reads before pressing.
    await screen.findByRole("button", { name: CAPTURE_UNLOCK_LABEL });
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_UNLOCK_LABEL }));
    // Both arguments asserted: passing the current value instead of the next
    // one is a lock button that presses and changes nothing.
    expect(notesCaptureSetLocked).toHaveBeenCalledWith("note:v1/n1", false);
  });

  it("reads its own row out of a list holding several windows", async () => {
    // Two windows in two states. A chrome that read the first row regardless of
    // its key would show this window as locked and offer the wrong verb.
    render(<CaptureWindowChrome captureKey="note:v1/n2" onClose={() => {}} />);
    await screen.findByRole("button", { name: CAPTURE_LOCK_LABEL });
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_LOCK_LABEL }));
    expect(notesCaptureSetLocked).toHaveBeenCalledWith("note:v1/n2", true);
  });

  it("names the always-on-top state it moves to, in both states", async () => {
    // Story 48.4. The accessible name IS the affordance here: the icon alone
    // cannot say which way the toggle goes, and a name that stated the CURRENT
    // state would tell a screen-reader user the opposite of what pressing does.
    // Both states out of one fixture list, so a component that hard-coded
    // either label fails on the other window rather than passing everywhere.
    const { rerender } = render(<CaptureWindowChrome captureKey="note:v1/n1" onClose={() => {}} />);
    // FIRST is on top, so the control offers to stop it.
    const pinned = await screen.findByRole("button", { name: CAPTURE_UNPIN_LABEL });
    expect(pinned).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByRole("button", { name: CAPTURE_PIN_LABEL })).toBeNull();
    // The icon too, and not only the name. The report asked for an *ikonke* —
    // an icon — and a sighted user reads nothing else on this button. Because
    // the glyph is `aria-hidden`, every accessible-name assertion above passes
    // just as happily with the two icons swapped, which is a real defect for
    // everyone who is not using a screen reader. `classList.contains` and not
    // a substring match: "lucide-pin-off" contains "lucide-pin".
    expect(pinned.querySelector("svg")?.classList.contains("lucide-pin")).toBe(true);

    rerender(<CaptureWindowChrome captureKey="note:v1/n2" onClose={() => {}} />);
    const unpinned = await screen.findByRole("button", { name: CAPTURE_PIN_LABEL });
    expect(unpinned).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryByRole("button", { name: CAPTURE_UNPIN_LABEL })).toBeNull();
    expect(unpinned.querySelector("svg")?.classList.contains("lucide-pin-off")).toBe(true);
  });

  it("pins the window it belongs to, with the value it is toggling to", async () => {
    // Both arguments asserted, following the lock's test: a toggle that passes
    // the CURRENT value presses and changes nothing, and one that passes the
    // first row's key pins somebody else's window.
    const { rerender } = render(<CaptureWindowChrome captureKey="note:v1/n1" onClose={() => {}} />);
    await screen.findByRole("button", { name: CAPTURE_UNPIN_LABEL });
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_UNPIN_LABEL }));
    expect(notesCaptureSetAlwaysOnTop).toHaveBeenCalledWith("note:v1/n1", false);

    rerender(<CaptureWindowChrome captureKey="note:v1/n2" onClose={() => {}} />);
    await screen.findByRole("button", { name: CAPTURE_PIN_LABEL });
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_PIN_LABEL }));
    expect(notesCaptureSetAlwaysOnTop).toHaveBeenLastCalledWith("note:v1/n2", true);
    expect(notesCaptureSetAlwaysOnTop).toHaveBeenCalledTimes(2);
    // And pinning is not locking: the two controls are independent, and a
    // handler wired to the wrong store action would be invisible here without
    // this line.
    expect(notesCaptureSetLocked).not.toHaveBeenCalled();
  });

  it("keeps the close button last, so DW-199's corner inset still protects it", async () => {
    // 47.5 inset the chrome's top and right edges because GTK hit-tests an
    // undecorated resizable window's resize border INSIDE the surface, and the
    // close button sits where the top and right strips overlap. A third button
    // is safe only while it does not take that corner.
    //
    // Story 75.3 moved this cluster into the editor header's frame group, which
    // is itself last in the row — so the corner geometry is unchanged and this
    // test still means what it meant. Scoped to the cluster rather than to the
    // document, because the row it now lives in holds the note's own verbs too
    // and a bare `getAllByRole` would be asserting their order as well.
    render(<CaptureWindowChrome captureKey="note:v1/n2" onClose={() => {}} />);
    await screen.findByRole("button", { name: CAPTURE_PIN_LABEL });
    const names = within(screen.getByTestId("capture-window-chrome"))
      .getAllByRole("button")
      .map((button) => button.getAttribute("aria-label"));
    expect(names).toEqual([CAPTURE_PIN_LABEL, CAPTURE_LOCK_LABEL, CAPTURE_CLOSE_LABEL]);
  });

  it("sizes the window controls to fit a 40px row with the resize border on top", async () => {
    // The arithmetic in the component's comment, asserted rather than trusted.
    // The row is 40px and the cluster carries a top padding of the DW-199
    // inset — 10 on this fixture's 2x display, 15 at 3x — so a 32px control
    // (`icon-sm`, which every other button in this header is) overflows it by
    // two pixels and by seven. `icon-xs` is 24, and 24 + 15 = 39.
    //
    // jsdom performs no layout, so this reads the class the size maps to: that
    // is the decision this test exists to pin, and the pixels it stands for are
    // measured in a real browser as part of the story's acceptance.
    render(<CaptureWindowChrome captureKey="note:v1/n3" onClose={() => {}} />);
    await screen.findByRole("button", { name: CAPTURE_PIN_LABEL });
    for (const button of within(screen.getByTestId("capture-window-chrome")).getAllByRole(
      "button",
    )) {
      expect(button.className).toContain("size-6");
    }
  });

  it("behaves as an ordinary window before Rust has answered", async () => {
    // This test asserted the OPPOSITE until Epic 75, and the reasoning behind
    // it — assume what the window already is — is unchanged. What changed is
    // what the window already is: AD-259 makes `alwaysOnTop: false` the shipped
    // default, in `Placement::default()` and in the draft window's birth state,
    // so an unanswered read is now a window that is almost certainly NOT
    // floating. Drawing a lit pin over it would offer to undo something nobody
    // asked for.
    notesCaptureWindows.mockReturnValue(new Promise<CaptureWindowVm[]>(() => {}));
    render(<CaptureWindowChrome captureKey="note:v1/n1" onClose={() => {}} />);
    expect(screen.getByRole("button", { name: CAPTURE_PIN_LABEL })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: CAPTURE_UNPIN_LABEL })).toBeNull();
  });

  it("makes the chrome a drag region only while the window is unlocked", async () => {
    // The drag region IS the unlocked window's mechanism — an undecorated
    // window has no title bar of the platform's — so its presence is the
    // feature and its absence is the lock. Chrome that dragged while locked
    // would move the window when the user aimed at the close button.
    //
    // Still asserted on this box after Story 75.3, and that is not redundant
    // with the header's own marking: Tauri's shim matches the exact element the
    // press landed on and never walks up, and this box covers the header's
    // frame wrapper completely, so a marking only on the wrapper would have no
    // area a pointer could reach.
    const { rerender } = render(<CaptureWindowChrome captureKey="note:v1/n1" onClose={() => {}} />);
    await screen.findByRole("button", { name: CAPTURE_UNLOCK_LABEL });
    expect(screen.getByTestId("capture-window-chrome")).not.toHaveAttribute(
      "data-tauri-drag-region",
    );
    rerender(<CaptureWindowChrome captureKey="note:v1/n2" onClose={() => {}} />);
    await screen.findByRole("button", { name: CAPTURE_LOCK_LABEL });
    expect(screen.getByTestId("capture-window-chrome")).toHaveAttribute("data-tauri-drag-region");
  });

  it("keeps the buttons out of the window's own resize border, by the number Rust measured", async () => {
    // DW-199. On GTK an unlocked undecorated window's resize edges are
    // hit-tested inside the surface, and the close button is flush into the
    // corner where two of those strips overlap — so aiming at close starts a
    // resize. The chrome is inset by exactly what the shell measured, and by
    // nothing when there is no border there.
    const { rerender } = render(<CaptureWindowChrome captureKey="note:v1/n1" onClose={() => {}} />);
    await screen.findByRole("button", { name: CAPTURE_UNLOCK_LABEL });
    // Locked: no inset at all, not "an inset of zero pixels" — a gutter on a
    // window with no resize border is a control moved for nothing.
    expect(screen.getByTestId("capture-window-chrome")).not.toHaveAttribute("style");

    rerender(<CaptureWindowChrome captureKey="note:v1/n2" onClose={() => {}} />);
    await screen.findByRole("button", { name: CAPTURE_LOCK_LABEL });
    const chrome = screen.getByTestId("capture-window-chrome");
    // 10, not 5: the number is `scale_factor() * 5` and this fixture is a 2x
    // display. A component that hard-coded the constant would pass a 1x test
    // and leave half the border over the close button on the owner's hardware.
    expect(chrome).toHaveStyle({ paddingTop: "10px" });
    // And NO right-hand gutter at 2x, where the old strip added the inset to
    // its own `px-1`. Story 75.3 put this cluster at the end of a header that
    // already keeps CAPTURE_HEADER_GUTTER_PX of gutter to the window's right
    // edge, which is 12 — more than the border is at 1x (5) or 2x (10). Paying
    // it twice would cost the row ten to fourteen pixels of title for a
    // clearance it already has, and the row's width is the whole difficulty in
    // this story.
    expect(chrome).toHaveStyle({ paddingRight: "0px" });

    // 3x is the first scale where anything is left over, and it is exactly what
    // the gutter does not cover: 15 − 12.
    rerender(<CaptureWindowChrome captureKey="note:v1/n3" onClose={() => {}} />);
    await waitFor(() => {
      expect(screen.getByTestId("capture-window-chrome")).toHaveStyle({ paddingRight: "3px" });
    });
    expect(screen.getByTestId("capture-window-chrome")).toHaveStyle({ paddingTop: "15px" });
  });

  it("behaves as UNLOCKED before Rust has answered", async () => {
    // Flipped by Epic 75, and the flip is the point rather than a detail.
    //
    // This test used to assert the opposite, on the reasoning that an unknown
    // window should be assumed to behave the way it always had — and the way it
    // always had was locked, because locked was the shipped default and it
    // persisted. AD-258 rescinds that: the open path forces `locked: false`, at
    // show and not only at create, so a window that arrives on screen is a
    // window that is unlocked. Assuming locked would now draw a closed padlock
    // over a window nobody locked and offer to unlock something that is not
    // locked — and would withhold the drag region for a frame on a window whose
    // whole affordance it is.
    //
    // A read that never resolves, so the store stays at `null` for the whole
    // test rather than for one tick. The executor form rather than
    // `Promise.withResolvers`: this tsconfig's `lib` predates it, and the
    // resolvers would go unused anyway — the point is that nothing settles.
    notesCaptureWindows.mockReturnValue(new Promise<CaptureWindowVm[]>(() => {}));
    render(<CaptureWindowChrome captureKey="note:v1/n1" onClose={() => {}} />);
    expect(screen.getByRole("button", { name: CAPTURE_LOCK_LABEL })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: CAPTURE_UNLOCK_LABEL })).toBeNull();
    expect(screen.getByTestId("capture-window-chrome")).toHaveAttribute("data-tauri-drag-region");
  });
});

describe("useCaptureDismissKeys", () => {
  function Harness({ onDismiss }: { onDismiss: () => void }) {
    useCaptureDismissKeys(onDismiss);
    return <input aria-label="field" />;
  }

  it("dismisses on Escape and on Ctrl/Cmd+W", () => {
    const onDismiss = vi.fn();
    render(<Harness onDismiss={onDismiss} />);
    fireEvent.keyDown(window, { key: "Escape" });
    // `ctrlKey`, not `metaKey`: jsdom matches nothing on the latter, and this
    // app reads the platform nowhere.
    fireEvent.keyDown(window, { key: "w", ctrlKey: true });
    expect(onDismiss).toHaveBeenCalledTimes(2);
  });

  it("leaves an Escape somebody else has handled alone", () => {
    // CodeMirror marks the event handled when Escape closes the `/` menu, the
    // tag chooser or the emoji chooser. Without this guard, dismissing a
    // completion popup destroys the window the person is working in.
    const onDismiss = vi.fn();
    render(<Harness onDismiss={onDismiss} />);
    const handled = new KeyboardEvent("keydown", { key: "Escape", cancelable: true });
    handled.preventDefault();
    window.dispatchEvent(handled);
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("ignores every other key", () => {
    const onDismiss = vi.fn();
    render(<Harness onDismiss={onDismiss} />);
    fireEvent.keyDown(window, { key: "w" });
    fireEvent.keyDown(window, { key: "q", ctrlKey: true });
    fireEvent.keyDown(window, { key: "Enter" });
    expect(onDismiss).not.toHaveBeenCalled();
  });
});

describe("CaptureNoteWindow", () => {
  it("hands its own note to the document, for each of two windows at once", async () => {
    // The acceptance criterion in the form one realm can express: two capture
    // windows, two different notes, neither reading the other's. Asserted on
    // both, because a component that always renders the first would satisfy a
    // single-window test.
    render(
      <>
        <CaptureNoteWindow vaultId="v1" noteId="n1" />
        <CaptureNoteWindow vaultId="v1" noteId="n2" />
      </>,
    );
    expect(screen.getByTestId("document-v1-n1")).toBeInTheDocument();
    expect(screen.getByTestId("document-v1-n2")).toBeInTheDocument();
    // `objectContaining`, because the boundary carries a frame node and a
    // title-bar flag as well now and neither is what this test is about — see
    // the two below, which are.
    expect(documentProps).toHaveBeenCalledWith(
      expect.objectContaining({ vaultId: "v1", noteId: "n1" }),
    );
    expect(documentProps).toHaveBeenCalledWith(
      expect.objectContaining({ vaultId: "v1", noteId: "n2" }),
    );
  });

  it("draws no chrome row of its own — the controls go to the document's header", async () => {
    // Story 75.3's headline, in the one realm that can state it. The document
    // is stubbed here, and the stub renders whatever `frame` it is handed: so
    // if the window still drew a strip beside the document there would be two
    // clusters on screen, and if it drew one INSTEAD of passing the frame down
    // there would be one that is not inside the document. Exactly one, inside
    // the document, is the merged shape.
    render(<CaptureNoteWindow vaultId="v1" noteId="n1" />);
    const document_ = await screen.findByTestId("document-v1-n1");
    const chrome = screen.getAllByTestId("capture-window-chrome");
    expect(chrome).toHaveLength(1);
    expect(document_).toContainElement(chrome[0] ?? null);
  });

  it("tells the document whether ITS window may be dragged, per window", async () => {
    // The title-bar flag is what makes the editor's header a drag region, and
    // it is per-window: FIRST is locked and SECOND is not. A host that computed
    // it once, or read the first row whatever key it held, would make a locked
    // window draggable the moment a second window opened unlocked — which is
    // the whole class of defect this file's two-window fixture exists for.
    render(
      <>
        <CaptureNoteWindow vaultId="v1" noteId="n1" />
        <CaptureNoteWindow vaultId="v1" noteId="n2" />
      </>,
    );
    await waitFor(() => {
      expect(documentProps).toHaveBeenCalledWith(
        expect.objectContaining({ noteId: "n1", titleBar: { draggable: false } }),
      );
    });
    expect(documentProps).toHaveBeenCalledWith(
      expect.objectContaining({ noteId: "n2", titleBar: { draggable: true } }),
    );
  });

  it("waits for the save to LAND before it closes", async () => {
    // Invocation order is not the contract and asserting it was a hole: `void
    // saveNote()` starts the save first and still lets the close fire while
    // the write is in flight, which passed an order assertion and survived a
    // mutation. This window is DESTROYED rather than hidden, so a write still
    // travelling when the webview goes away is the last 1.5 s of typing lost
    // (AD-62). What has to be true is that the save has RESOLVED.
    // Held in a one-slot box rather than a `let`: TypeScript narrows a `let`
    // initialised to `null` to `null` at the call site below, because the
    // assignment happens inside a callback it cannot order.
    const landed: { resolve: ((ok: boolean) => void) | null } = { resolve: null };
    saveNote.mockImplementation(
      () =>
        new Promise<boolean>((resolve) => {
          landed.resolve = resolve;
        }),
    );
    render(<CaptureNoteWindow vaultId="v1" noteId="n2" />);
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_CLOSE_LABEL }));
    await waitFor(() => {
      // Story 46.12: the write that gates this close is THIS window's note.
      expect(saveNote).toHaveBeenCalledExactlyOnceWith("v1", "n2");
    });
    expect(notesCaptureClose).not.toHaveBeenCalled();
    landed.resolve?.(true);
    await waitFor(() => {
      expect(notesCaptureClose).toHaveBeenCalledWith("note:v1/n2");
    });
  });

  it("does NOT close when the write was refused", async () => {
    // W3NoteFile's shape, and this window is where it bites hardest: the
    // prewarmed window merely hides on a refused write, so the words survive in
    // a buffer on a page that is handed back. This one is DESTROYED. Closing it
    // over a write Rust refused takes the webview, the buffer and the unsaved
    // text with it, and says nothing — because the only surface that could have
    // said anything is the one that just vanished.
    saveNote.mockResolvedValue(false);
    render(<CaptureNoteWindow vaultId="v1" noteId="n2" />);
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_CLOSE_LABEL }));
    await waitFor(() => {
      expect(saveNote).toHaveBeenCalledTimes(1);
    });
    expect(notesCaptureClose).not.toHaveBeenCalled();
    // Still on screen, with the words still in it. The reason is already
    // rendered by the editor, which reads the same store `markSaveFailed` wrote.
    expect(screen.getByTestId("document-v1-n2")).toBeInTheDocument();
    // And Escape does not get a second bite at throwing them away either.
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => {
      expect(saveNote).toHaveBeenCalledTimes(2);
    });
    expect(notesCaptureClose).not.toHaveBeenCalled();
  });

  it("closes the window Escape was pressed in, not the other one", async () => {
    render(<CaptureNoteWindow vaultId="v1" noteId="n2" />);
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => {
      expect(notesCaptureClose).toHaveBeenCalledWith("note:v1/n2");
    });
    expect(notesCaptureClose).toHaveBeenCalledTimes(1);
  });

  it("keys itself the way Rust does, including an id with a slash in it", async () => {
    // A note id is derived from a path, so a slash in one is ordinary — and an
    // unescaped one would make this window ask about a different note's
    // placement.
    render(<CaptureNoteWindow vaultId="v1" noteId="sub/dir/n3" />);
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_CLOSE_LABEL }));
    await waitFor(() => {
      expect(notesCaptureClose).toHaveBeenCalledWith("note:v1/sub%2Fdir%2Fn3");
    });
  });
});
