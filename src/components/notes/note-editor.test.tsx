/**
 * A save does not move the toolbar (Story 46.4).
 *
 * **What this file can and cannot prove, stated up front, because the gap is
 * the whole difficulty of the story.** The defect is a layout shift: the save
 * caption is three strings of three different widths, and in a single
 * non-wrapping flex row every one of those widths is taken out of whatever else
 * in the row can give. jsdom performs no layout at all — every element reports a
 * zero rect, and `src/test/setup.ts`'s shim answers a viewport only for
 * zero-sized elements and deliberately stops at the edge of a CodeMirror editor
 * — so **no test in this repository can measure the reflow**. A test that
 * asserted the caption had moved by N pixels would be asserting the shim.
 *
 * What is observable here is the structural property that causes the shift, and
 * it is observable exactly: whether the caption is a width-variable participant
 * in the same flex row as the buttons. Three claims, each of which fails on the
 * code as it shipped:
 *
 * 1. the row is three groups, and the caption's siblings are groups rather than
 *    controls — so the caption's width is not taken out of a button;
 * 2. the caption's box is `shrink-0` and does not change shape as the word
 *    changes, nor when group 3 gains a control;
 * 3. the box is reserved by strings this machine's own locale produced, not by
 *    a character count someone guessed in `en-GB`.
 *
 * What remains a gate check rather than a test: that the ⋯ menu is visibly
 * still in the same place after a save, in a resized quick-capture window.
 * See `spec-46-4-save-does-not-move-the-toolbar.md`.
 */
import { act, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteBodyBatch, NoteVaultVm, NoteWriteVm } from "@/lib/ipc/client";

const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => {}),
}));

vi.mock("@/lib/ipc/client", () => ({
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

import {
  PANE_HEADER_ACTIONS_SLOT,
  PANE_HEADER_IDENTITY_SLOT,
  PANE_HEADER_STATUS_SLOT,
} from "@/components/layout/pane-header";
import {
  beginSave,
  editBuffer,
  markSaved,
  markSaveFailed,
  resetNotesEditorStoreForTest,
} from "@/lib/stores/notes-editor";
import { notesVaultsStore, resetNotesVaultsStoreForTest } from "@/lib/stores/notes-vaults";
import { resetPanelsStoreForTest } from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { withRangeRects } from "@/test/layout";
import { NoteEditor, SAVE_CAPTION_SIZERS, saveStateWord } from "./note-editor";

/**
 * The editor's CodeMirror chunk is a dynamic `import()`, and under eight
 * concurrent suites it has been measured past five seconds. Nothing in this
 * file waits for it — the header is synchronous — but the mount still pays for
 * it, so the budget is raised at file scope for the same reason
 * `note-file-links.test.tsx` raises it: a red here should mean the header is
 * wrong, never that the box was busy.
 */
vi.setConfig({ testTimeout: 20_000 });

const NOTE_PATH = "inbox/meeting.md";
const BODY = "# Meeting\n\nwhat we said\n";

/** A write as Rust acknowledges one. */
const WRITE: NoteWriteVm = {
  frontmatter: "",
  rev: "r1",
  path: NOTE_PATH,
  conflictCopy: null,
} as NoteWriteVm;

let restoreRects: (() => void) | null = null;

/**
 * A vault whose subfolder resolves, so `Show in Files` is offered.
 *
 * Seeded per test rather than globally: whether group 3 has five children or
 * six is precisely the second thing that used to move this caption, and one
 * test below needs to see the row both ways.
 */
function seedVault(): void {
  const vault = {
    id: "v1",
    profileId: "profile-1",
    name: "v1",
    subfolder: "notes",
    root: "/Volumes/profile-1/notes",
    indexed: true,
    noteCount: 2,
    unreadCount: 0,
    cadence: { commitIdleMs: 1000, pushIntervalMs: 5000, pushOnBlur: true },
  } as NoteVaultVm;
  notesVaultsStore.getState().setVaults([vault]);
}

/** Mount the editor on a note, and let its opening `Reset` land. */
async function openEditor(): Promise<void> {
  notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
    onBatch({
      kind: "reset",
      text: BODY,
      frontmatter: "",
      rev: "r0",
      cursor: null,
      path: NOTE_PATH,
    });
    return "sub-1";
  });
  render(<NoteEditor vaultId="v1" noteId="n1" />);
  await act(async () => {
    await Promise.resolve();
  });
}

function headerRow(): HTMLElement {
  const found = document.querySelector("header");
  if (found === null) {
    throw new Error("the editor drew no header at all");
  }
  return found;
}

function captionSlot(): HTMLElement {
  const found = document.querySelector<HTMLElement>(`[data-slot="${PANE_HEADER_STATUS_SLOT}"]`);
  if (found === null) {
    throw new Error("the header drew no reserved slot for the save caption");
  }
  return found;
}

/**
 * The slot's box, order-insensitively.
 *
 * Sorted because Tailwind class order is the formatter's business and not this
 * test's: what the story claims is that the SET of classes on the caption's
 * container is the same in every save state, not that a string is byte-equal.
 */
function box(element: Element): string {
  return Array.from(element.classList).sort().join(" ");
}

/** The invisible strings that decide how wide the slot is. */
function reserved(): string[] {
  return Array.from(captionSlot().querySelectorAll(":scope > [aria-hidden='true']")).map(
    (sizer) => sizer.textContent ?? "",
  );
}

/** The element carrying the one caption on screen — the sizers beside it are
 *  invisible and `aria-hidden`, and they are the slot's width rather than its
 *  content. */
function shownElement(): Element {
  const shown = captionSlot().querySelector(":scope > :not([aria-hidden='true'])");
  if (shown === null) {
    throw new Error("the slot rendered no caption element");
  }
  return shown;
}

/** What that element says. */
function shownWord(): string {
  return shownElement().textContent ?? "";
}

/**
 * A caption with its digits flattened.
 *
 * Two captions with the same shape are the same width in the slot, because the
 * slot is `tabular-nums` and every digit there is as wide as every other. It is
 * shape, not equality, that a reserved box has to cover: `Saved · 09:15` and
 * `Saved · 23:41` are different strings and identical widths.
 */
function shape(caption: string): string {
  return caption.replace(/\d/g, "0");
}

beforeEach(() => {
  vi.clearAllMocks();
  restoreRects = withRangeRects();
  resetPanelsStoreForTest();
  resetNotesVaultsStoreForTest();
  resetNotesEditorStoreForTest();
  primaryViewStore.getState().setView("notes");
});

afterEach(() => {
  restoreRects?.();
  restoreRects = null;
  resetNotesEditorStoreForTest();
});

describe("the header row is three groups, not nine siblings", () => {
  it("puts no control in the same shrink context as the caption", async () => {
    seedVault();
    await openEditor();

    const row = headerRow();
    // The caption's siblings are the other two groups, and nothing else. This
    // is the property that failed: with the caption, the title, five buttons
    // and a menu trigger all direct children of one non-wrapping flex row, the
    // width the caption gained on a save came out of the buttons beside it.
    expect(captionSlot().parentElement).toBe(row);
    expect(row.children).toHaveLength(3);
    expect(Array.from(row.children).filter((child) => child.tagName === "BUTTON")).toHaveLength(0);

    // And the three are the ones we named, in reading order.
    expect(Array.from(row.children).map((child) => child.getAttribute("data-slot"))).toEqual([
      PANE_HEADER_IDENTITY_SLOT,
      PANE_HEADER_STATUS_SLOT,
      PANE_HEADER_ACTIONS_SLOT,
    ]);
  });

  it("gives the slack to identity and to nothing else", async () => {
    seedVault();
    await openEditor();

    const row = headerRow();
    const identity = row.querySelector(`[data-slot="${PANE_HEADER_IDENTITY_SLOT}"]`);
    // Exactly one member of the row grows and gives ground. Every other member
    // sits where the row's own edge puts it, which is what makes the caption's
    // length something only the title can feel.
    expect(Array.from(row.children).filter((child) => child.classList.contains("flex-1"))).toEqual([
      identity,
    ]);
    expect(identity).toHaveClass("min-w-0");
    // A slot that can be squeezed is not a slot.
    expect(captionSlot()).toHaveClass("shrink-0");
  });
});

describe("the save caption is a box before it is a word", () => {
  it("keeps the same box through dirty, saving and saved", async () => {
    await openEditor();

    // Dirty: the caption is deliberately empty while someone is typing.
    act(() => {
      editBuffer("v1", "n1", `${BODY}more`);
    });
    await waitFor(() => {
      expect(shownWord()).toBe("");
    });
    const dirtyBox = box(captionSlot());
    const dirtyReservation = reserved();

    act(() => {
      beginSave("v1", "n1");
    });
    await waitFor(() => {
      expect(shownWord()).toBe("Saving…");
    });
    expect(box(captionSlot())).toBe(dirtyBox);
    expect(reserved()).toEqual(dirtyReservation);

    act(() => {
      markSaved("v1", "n1", `${BODY}more`, WRITE);
    });
    await waitFor(() => {
      expect(shownWord()).toMatch(/^Saved/);
    });
    expect(box(captionSlot())).toBe(dirtyBox);
    expect(reserved()).toEqual(dirtyReservation);

    // The word really did change three times — otherwise the two assertions
    // above would hold for a caption that never rendered anything.
    expect(shownWord()).not.toBe("Saving…");
  });

  it("keeps the same box while the group beside it changes width", async () => {
    // Identity is the group that gives ground, and its content changes width
    // constantly: the title is derived from the buffer's first heading, so it
    // moves on a keystroke. That movement must not reach the caption's box, and
    // the caption's box must not have been sized off it.
    await openEditor();
    const before = box(captionSlot());
    const reservationBefore = reserved();
    const title = () => document.querySelector("h1")?.textContent ?? "";
    const titleBefore = title();

    act(() => {
      editBuffer("v1", "n1", `# ${"a rather long heading ".repeat(12)}\n\nwhat we said\n`);
    });
    await waitFor(() => {
      expect(title().length).toBeGreaterThan(titleBefore.length * 4);
    });

    expect(box(captionSlot())).toBe(before);
    expect(reserved()).toEqual(reservationBefore);
  });

  it("reserves the box from strings this machine's own clock produced", async () => {
    await openEditor();

    // The reservation is rendered, not described: these are the strings the
    // browser measures to decide how wide the slot is.
    expect(reserved()).toEqual([...SAVE_CAPTION_SIZERS]);

    // Three of them — "Saving…" and both halves of the day — because a locale
    // that appends AM or PM renders one of those two, and which one depends on
    // the hour the person happened to save at.
    expect(SAVE_CAPTION_SIZERS).toHaveLength(3);
    expect(SAVE_CAPTION_SIZERS).toContain(
      saveStateWord({ saving: true, dirty: false, savedAtMs: null, error: null }),
    );

    // And the reservation covers the whole clock. Every hour of the day, in
    // whatever locale this machine has, renders a caption whose shape the slot
    // has already made room for — which is the claim a guessed `w-24` cannot
    // make and this one can.
    const shapes = SAVE_CAPTION_SIZERS.map(shape);
    const midnight = Date.UTC(2024, 5, 17, 0, 0);
    for (let hour = 0; hour < 24; hour += 1) {
      const caption = saveStateWord({
        saving: false,
        dirty: false,
        savedAtMs: midnight + hour * 60 * 60 * 1000,
        error: null,
      });
      expect(shapes, `nothing reserved room for ${caption}`).toContain(shape(caption));
    }
  });

  it("cannot be widened by a save error, and does not swallow one either", async () => {
    await openEditor();
    const before = box(captionSlot());
    const reservationBefore = reserved();

    const REFUSED =
      "the vault is read-only and the write was refused: /Volumes/profile-1/notes/inbox/meeting.md";
    act(() => {
      markSaveFailed("v1", "n1", REFUSED);
    });
    await waitFor(() => {
      expect(shownWord()).toBe(REFUSED);
    });

    // An error is Rust's message verbatim, so it is the one caption that cannot
    // be reserved for. It is taken out of flow instead — it cannot widen the
    // box, and the box is what everything to its right is standing on.
    expect(shownElement()).toHaveClass("absolute");
    expect(box(captionSlot())).toBe(before);
    expect(reserved()).toEqual(reservationBefore);

    // Ellipsised on screen is not the same as thrown away: the whole sentence
    // stays in the DOM for a screen reader and on `title` for a pointer.
    expect(shownElement()).toHaveAttribute("title", REFUSED);
  });
});
