/**
 * Story 48.3 — "Open in a capture window" is really in a note's Actions menu.
 *
 * **This file exists because its absence is the whole defect.** Story 45.15
 * built `CaptureNoteItem`, `openNoteAsCapture`, `notesCaptureOpen`,
 * `notes_capture_open` and `notes_window::open`, tested the component in
 * `capture-note-item.test.tsx`, and rendered the component **nowhere**. Its only
 * importer in the repository, for three epics, was that test. Every assertion
 * that test makes is true and always was: the item does open the note it was
 * given, it does carry its own props, it is absent without `capabilities.notes`.
 * None of them can see that no production tree mounts it.
 *
 * So the owner of 0.8.1 had exactly one capture window — the prewarmed draft —
 * and reported it twice, as "nie mozna miec wiecej niz jednej notatki" and "nie
 * widze tez mozliwosci otworzenia istniejacych notatek jak quick capture".
 * Two reports, one unrendered child. And 45.15's gate check 2 — "open a note's
 * actions menu → Open in a capture window" — could not have passed on any build
 * it produced, and was signed off anyway.
 *
 * The fix is one import and one line. The value of this file is that the line
 * cannot go missing again quietly: every test here mounts the **real**
 * `NoteEditor`, finds the **real** Actions trigger by the name a person reads,
 * and presses the item. `export-in-the-note-editor.test.tsx` is the same file
 * for Story 45.21, written for exactly this reason and one epic earlier — this
 * story is what happens when a deliverable does not get one.
 *
 * # What is asserted at the IPC edge and why
 *
 * Every assertion lands on `notesCaptureOpen`'s ARGUMENT, and where two windows
 * are in question, on `captureKey` of that argument. The key is what Rust hashes
 * into a window label, so "two notes give two windows" is decidable here without
 * a compositor: two distinct keys are two labels are two windows, and one key is
 * one window however many times it is asked for. A test that counted calls
 * instead would pass just as happily while both presses named one note.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { CaptureTargetVm, NoteBodyBatch, NoteVaultVm } from "@/lib/ipc/client";

const notesCaptureOpen = vi.fn<(target: CaptureTargetVm) => Promise<void>>();
const notesCaptureWindows = vi.fn<() => Promise<unknown[]>>();
const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => {}),
}));

// The editor's own boot path, as `note-editor.test.tsx` establishes it, plus the
// two commands this story's press reaches. Nothing here is mocked "to be safe":
// the vault pair is `ensureNotesVaultsHydrated`'s `loadSnapshot`, and
// `notesTemplateUpdatePreview` is reached only on a slow run, four seconds in,
// where omitting it is an unhandled rejection inside a passing test.
vi.mock("@/lib/ipc/client", () => ({
  notesCaptureOpen: (target: CaptureTargetVm) => notesCaptureOpen(target),
  notesCaptureWindows: () => notesCaptureWindows(),
  notesOpen: (v: string, n: string, on: (b: NoteBodyBatch) => void) => notesOpen(v, n, on),
  notesClose: vi.fn(async () => {}),
  notesSave: vi.fn(async () => ({ frontmatter: "", rev: "r1", path: "n.md", conflictCopy: null })),
  notesBufferReport: vi.fn(async () => {}),
  notesTagTree: vi.fn(async () => ({ nodes: [] })),
  notesBacklinks: vi.fn(async () => []),
  notesResolveConflict: vi.fn(async () => {}),
  notesMarkRead: vi.fn(async () => {}),
  notesDiff: vi.fn(async () => null),
  notesHistory: vi.fn(async () => []),
  notesResolveLink: vi.fn(async () => null),
  recordingNoteTargets: vi.fn(async () => null),
  recordingOpenPath: vi.fn(async () => {}),
  revealPath: vi.fn(async () => {}),
  notesTemplateUpdatePreview: vi.fn(async () => null),
  notesVaults: vi.fn(async () => []),
  notesVaultActive: vi.fn(async () => null),
  notesVaultSetActive: vi.fn(async () => {}),
}));

import { CAPTURE_NOTE_LABEL } from "@/components/capture/capture-note-item";
import { EXPORT_NOTE_LABEL } from "@/components/export/export-note-item";
import { NOTE_ACTIONS_LABEL, NOTE_DELETE_LABEL } from "@/components/notes/note-actions";
import { NoteEditor } from "@/components/notes/note-editor";
import { NOTE_HISTORY_LABEL } from "@/components/notes/note-history-panel";
import { captureKey } from "@/lib/capture-target";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { resetCaptureWindowsStoreForTest } from "@/lib/stores/capture-windows";
import { readNoteDocument, resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import { notesVaultsStore, resetNotesVaultsStoreForTest } from "@/lib/stores/notes-vaults";
import { resetPanelsStoreForTest } from "@/lib/stores/panels";
import { withRangeRects } from "@/test/layout";

/**
 * The editor's CodeMirror chunk is behind a dynamic `import()` and has been
 * measured past five seconds under load. Nothing here waits for it — the header
 * is synchronous — but the mount still pays for it, so the budget is raised at
 * FILE scope: a red in this file should mean the menu item is gone, never that
 * the box was busy.
 */
vi.setConfig({ testTimeout: 20_000 });

// jsdom does no layout, so CodeMirror's measure pass — which runs on any
// animation frame that elapses while the real `NoteEditor` is mounted — would
// throw outside every `try` a test can write and take the run's exit code while
// the summary still printed passes. Never hand-rolled; `src/test/layout.ts` owns
// the shim and its undo.
let restoreRects: (() => void) | null = null;

beforeAll(() => {
  restoreRects = withRangeRects();
});

afterAll(() => {
  restoreRects?.();
  restoreRects = null;
});

/** Per-note bodies, so two editors on two notes have two different titles — and
 *  therefore two distinguishable Actions triggers, since the trigger's
 *  accessible name carries the note's title for exactly that reason. */
const BODIES: Record<string, string> = {
  "note-9": "# Standing meeting\n\nwhat we said\n",
  "note-11": "# Grocery list\n\nolives\n",
};

/** A vault whose subfolder resolves, so the menu under test is the full one
 *  Story 46.5 assembled rather than a short version this file arranged. */
function seedVault(): void {
  notesVaultsStore.getState().setVaults([
    {
      id: "vault-7",
      profileId: "profile-1",
      name: "vault-7",
      subfolder: "notes",
      root: "/Volumes/profile-1/notes",
      indexed: true,
      noteCount: 2,
      unreadCount: 0,
      cadence: { commitIdleMs: 1000, pushIntervalMs: 5000, pushOnBlur: true },
    } as NoteVaultVm,
  ]);
}

/**
 * Open one editor's Actions menu, by the name a person reads on the trigger.
 *
 * `pointerDown`/`pointerUp` and not `click`: that is the pair Radix's trigger
 * listens for. `title` selects between two mounted editors — a bare
 * `findByRole("button")` would find whichever came first in the document, which
 * is the mutation the two-windows test below exists to catch.
 */
async function openActions(title: string): Promise<HTMLElement> {
  const trigger = await screen.findByRole("button", {
    name: `${NOTE_ACTIONS_LABEL} ${title}`,
  });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.pointerUp(trigger, { button: 0 });
  return await screen.findByRole("menu");
}

/** Open one editor's menu and press this story's item in it. */
async function pressCaptureItem(title: string): Promise<void> {
  const menu = await openActions(title);
  fireEvent.click(within(menu).getByRole("menuitem", { name: CAPTURE_NOTE_LABEL }));
}

beforeEach(() => {
  vi.clearAllMocks();
  resetNotesEditorStoreForTest();
  resetNotesVaultsStoreForTest();
  resetPanelsStoreForTest();
  resetCaptureWindowsStoreForTest();
  capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, notes: true });
  notesCaptureOpen.mockResolvedValue(undefined);
  notesCaptureWindows.mockResolvedValue([]);
  notesOpen.mockImplementation(async (_vaultId, noteId, onBatch) => {
    onBatch({
      kind: "reset",
      text: BODIES[noteId] ?? "# Untitled\n",
      frontmatter: "",
      rev: "r0",
      path: `notes/${noteId}.md`,
      cursor: null,
    });
    return `sub-${noteId}`;
  });
  seedVault();
});

describe("the Actions menu a person actually opens", () => {
  it("offers Open in a capture window, and opens the note the header is showing", async () => {
    render(<NoteEditor vaultId="vault-7" noteId="note-9" />);

    await pressCaptureItem("Standing meeting");

    // The ids the HEADER composed, whole. Not a call count: a menu item that
    // fires and hands on the wrong note is a person watching keeper open
    // somebody else's note in a window they asked for on this one, and it
    // resolves the same `undefined` either way.
    await waitFor(() =>
      expect(notesCaptureOpen).toHaveBeenCalledWith({
        kind: "note",
        vaultId: "vault-7",
        noteId: "note-9",
      }),
    );
  });

  it("gives two notes two windows, from two menus", async () => {
    // The owner's first sentence, as a test: more than one note in a capture
    // window. Two editors mounted at once, which is what Story 46.12's
    // multi-panel notes surface really produces, and each pressed through its
    // own trigger.
    render(
      <>
        <NoteEditor vaultId="vault-7" noteId="note-9" />
        <NoteEditor vaultId="vault-7" noteId="note-11" />
      </>,
    );

    await pressCaptureItem("Standing meeting");
    await pressCaptureItem("Grocery list");

    await waitFor(() => expect(notesCaptureOpen).toHaveBeenCalledTimes(2));
    const asked = notesCaptureOpen.mock.calls.map(([target]) => target);
    expect(asked).toEqual([
      { kind: "note", vaultId: "vault-7", noteId: "note-9" },
      { kind: "note", vaultId: "vault-7", noteId: "note-11" },
    ]);
    // Two DISTINCT keys, which is the part that decides whether the owner gets
    // two windows: `notes_window::open` hashes this key into the window label
    // and reuses a window only for an exact label match. Two keys are two
    // labels are two windows. Asserted through the real `captureKey` — the same
    // function pinned to Rust's by `capture-key-vectors.json` — rather than by
    // eyeballing the ids, so a store or a helper that collapsed two targets
    // into one addressable window could not hide behind distinct arguments.
    const keys = asked.map(captureKey);
    expect(new Set(keys).size).toBe(2);
    expect(keys).toEqual(["note:vault-7/note-9", "note:vault-7/note-11"]);
  });

  it("keeps one note one window, however many panels are showing it", async () => {
    // The other half of "two windows", and the half a count of calls cannot
    // tell apart from it. Story 46.12 made the notes surface multi-panel, so a
    // note can be open twice at once and now also be opened as a capture window
    // from either copy.
    render(
      <>
        <NoteEditor vaultId="vault-7" noteId="note-9" />
        <NoteEditor vaultId="vault-7" noteId="note-9" />
      </>,
    );
    // Confirmed rather than assumed: 46.12 refcounts a document by VIEWS, and
    // two panels on one note join one buffer instead of blanking each other.
    // If that ever regressed to two documents, the ids below would still match
    // and this story would still look fine while the two panels disagreed about
    // the note's text.
    await waitFor(() => expect(readNoteDocument("vault-7", "note-9").views).toBe(2));

    const triggers = await screen.findAllByRole("button", {
      name: `${NOTE_ACTIONS_LABEL} Standing meeting`,
    });
    expect(triggers).toHaveLength(2);
    for (const trigger of triggers) {
      fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
      fireEvent.pointerUp(trigger, { button: 0 });
      const menu = await screen.findByRole("menu");
      fireEvent.click(within(menu).getByRole("menuitem", { name: CAPTURE_NOTE_LABEL }));
      // Radix unmounts on its own schedule, and a second `findByRole("menu")`
      // can otherwise resolve to the one that is closing — which reads as the
      // second panel's item firing when it was the first panel's, twice.
      await waitFor(() => expect(screen.queryAllByRole("menu")).toHaveLength(0));
    }

    await waitFor(() => expect(notesCaptureOpen).toHaveBeenCalledTimes(2));
    // ONE key from both panels. `notes_window::open` derives the window label
    // from it, so the second panel raises the window the first one opened rather
    // than putting a rival webview over the same buffer — two capture windows
    // editing one note is the one outcome this story must not produce.
    const fromBothPanels = notesCaptureOpen.mock.calls.map(([target]) => captureKey(target));
    expect(fromBothPanels).toEqual(["note:vault-7/note-9", "note:vault-7/note-9"]);
  });

  it("asks again for a note whose window is already open, under the one key", async () => {
    // The already-open case, made deliberate rather than left to chance.
    //
    // Rust answers it: one label per note, so `open` raises and focuses the
    // window that is there. The UI's whole job is therefore to KEEP ASKING —
    // and that is worth a test, because the obvious "improvement" is to read
    // the mirror and refuse or relabel the second press. That would replace a
    // raise-and-focus with silence, and it would do so off a mirror the main
    // window does not even keep (see `capture-windows.ts`).
    //
    // So the mirror is seeded as if the window were open, and the press must
    // still reach Rust with the same target.
    //
    // The row is deliberately NOT annotated `CaptureWindowVm`. Only `key` is
    // load-bearing — `captureWindowFor` looks a row up by that and nothing else
    // — and annotating it would tie this story's own PR to whichever epic-48
    // story next adds a field to the generated type, which is how a stack ends
    // up with a commit that only compiles at the tip.
    notesCaptureWindows.mockResolvedValue([
      {
        key: "note:vault-7/note-9",
        target: { kind: "note", vaultId: "vault-7", noteId: "note-9" },
        locked: false,
        visible: true,
        chromeInset: 0,
      },
    ]);
    render(<NoteEditor vaultId="vault-7" noteId="note-9" />);

    await pressCaptureItem("Standing meeting");
    await waitFor(() => expect(notesCaptureOpen).toHaveBeenCalledTimes(1));
    await pressCaptureItem("Standing meeting");
    await waitFor(() => expect(notesCaptureOpen).toHaveBeenCalledTimes(2));

    const keys = notesCaptureOpen.mock.calls.map(([target]) => captureKey(target));
    // One key, twice. Never a second window for one note, and never a press
    // that went nowhere.
    expect(keys).toEqual(["note:vault-7/note-9", "note:vault-7/note-9"]);
  });

  it("is absent where a capture window cannot exist, proved from an open menu", async () => {
    // `capabilities.notes` is `sync && desktop`, computed in Rust (AD-27) — the
    // same flag the in-app capture chord reads, so the item is offered exactly
    // where ⌘⌥K is.
    //
    // Read from an OPEN menu with a sibling asserted present, following
    // `note-file-links.test.tsx`: an item missing from a menu nobody opened is
    // missing for a reason that has nothing to do with the gate, and an
    // assertion that cannot fail is worse than no assertion.
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, notes: false });
    render(<NoteEditor vaultId="vault-7" noteId="note-9" />);

    const menu = await openActions("Standing meeting");
    expect(within(menu).getByRole("menuitem", { name: NOTE_HISTORY_LABEL })).toBeInTheDocument();
    expect(within(menu).queryByRole("menuitem", { name: CAPTURE_NOTE_LABEL })).toBeNull();
    expect(notesCaptureOpen).not.toHaveBeenCalled();
  });

  it("sits with what shows the note, above what acts on it", async () => {
    // Story 46.5 gave this menu's separator a meaning: above it changes what is
    // SHOWN, below it acts on the note itself. A capture window is a way of
    // looking at a note, so the item belongs above — which is why Story 48.3
    // did not follow 45.15's own note ("beside Export"), written when the menu
    // held Export and Delete and nothing else.
    //
    // Relative order and not an index: what the decision claims is which side
    // of the rule the item is on, not that it is the fourth child. An index
    // would go red for a header restructure that kept the invariant.
    render(<NoteEditor vaultId="vault-7" noteId="note-9" />);

    const menu = await openActions("Standing meeting");
    const labels = within(menu)
      .getAllByRole("menuitem")
      .map((item) => item.textContent ?? "");
    const at = (label: string): number => {
      const index = labels.indexOf(label);
      expect(index, `${label} is not in the menu at all`).toBeGreaterThanOrEqual(0);
      return index;
    };
    expect(at(NOTE_HISTORY_LABEL)).toBeLessThan(at(CAPTURE_NOTE_LABEL));
    expect(at(CAPTURE_NOTE_LABEL)).toBeLessThan(at(EXPORT_NOTE_LABEL));
    expect(at(EXPORT_NOTE_LABEL)).toBeLessThan(at(NOTE_DELETE_LABEL));
  });
});
