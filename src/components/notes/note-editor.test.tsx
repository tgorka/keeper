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
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
import { SHOW_IN_FILES_LABEL } from "@/lib/vault-link";
import { withActionWidths, withHandFiredResize, withRangeRects } from "@/test/layout";
import { ATTACHMENTS_LABEL } from "./attachments-panel";
import { NOTE_ACTIONS_LABEL, NOTE_DELETE_LABEL } from "./note-actions";
import {
  NoteEditor,
  PANEL_BACK_LABEL,
  PANEL_UNAVAILABLE_SLOT,
  SAVE_CAPTION_SIZERS,
  saveStateWord,
} from "./note-editor";
import { NOTE_HISTORY_LABEL } from "./note-history-panel";
import { PROPERTIES_LABEL } from "./properties-panel";

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

/**
 * The header shows what fits and menus what does not (Story 48.5).
 *
 * **The widths below are this file's and not a browser's**, for the reason
 * stated at the top: jsdom performs no layout, `src/test/setup.ts` answers one
 * viewport for every zero-sized element, and a suite that measured a real
 * reflow here would be measuring the shim. `withActionWidths` declares a width
 * for exactly the elements this mechanism measures and `withHandFiredResize`
 * delivers the observation the shimmed `ResizeObserver` never does. The policy
 * itself — which item moves at which width — is proved to the pixel and
 * without a DOM in `priority-actions.test.tsx`. What these tests add is that
 * the real editor's real header is wired to it: its own four verbs, its own
 * priority order, its own menu.
 *
 * With these numbers the group owes 464px before the first candidate (160
 * identity, 90 status, two 8px seams, a 110 leading control, an 80 trigger and
 * its seam), and the four candidates cost 108, 100, 74 and 108.
 */
const WIDTHS: Record<string, number> = {
  attachments: 100,
  properties: 92,
  history: 66,
  "show-in-files": 100,
  leading: 110,
  menu: 80,
  status: 90,
};

/** Every verb the row is showing as a word, in the row's order. */
function words(): string[] {
  return Array.from(document.querySelectorAll("[data-priority-action]")).map(
    (control) => control.textContent ?? "",
  );
}

/** Open the note's own actions menu, and hand back what is in it. */
function menuItems(): string[] {
  const trigger = screen.getByRole("button", { name: new RegExp(`^${NOTE_ACTIONS_LABEL}`) });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.click(trigger);
  return screen.getAllByRole("menuitem").map((item) => item.textContent ?? "");
}

describe("the header shows the verbs it has room for", () => {
  let restoreWidths: (() => void) | null = null;
  let observer: { resize: (width: number) => void; undo: () => void } | null = null;

  afterEach(() => {
    restoreWidths?.();
    restoreWidths = null;
    observer?.undo();
    observer = null;
  });

  async function openAtWidths(): Promise<(width: number) => void> {
    seedVault();
    restoreWidths = withActionWidths(WIDTHS);
    observer = withHandFiredResize();
    await openEditor();
    const { resize } = observer;
    return (width) => {
      act(() => resize(width));
    };
  }

  it("degrades one verb at a time, in the order the editor declared", async () => {
    const resize = await openAtWidths();

    resize(1400);
    expect(words()).toEqual([
      ATTACHMENTS_LABEL,
      PROPERTIES_LABEL,
      NOTE_HISTORY_LABEL,
      SHOW_IN_FILES_LABEL,
    ]);

    resize(800);
    expect(words()).toEqual([ATTACHMENTS_LABEL, PROPERTIES_LABEL, NOTE_HISTORY_LABEL]);

    resize(700);
    expect(words()).toEqual([ATTACHMENTS_LABEL, PROPERTIES_LABEL]);

    // What the 560px capture window is near: the two panels the owner reported
    // as missing are the last things to go, and Attachments is the last of all.
    resize(600);
    expect(words()).toEqual([ATTACHMENTS_LABEL]);

    resize(500);
    expect(words()).toEqual([]);
  });

  it("moves the first verb into the menu at exactly one width", async () => {
    const resize = await openAtWidths();

    resize(572);
    expect(words()).toEqual([ATTACHMENTS_LABEL]);
    resize(571);
    expect(words()).toEqual([]);
  });

  it("keeps Delete in the menu, and never in the row, at every width", async () => {
    const resize = await openAtWidths();

    for (const width of [1400, 800, 700, 600, 500, 0]) {
      resize(width);
      expect(words()).not.toContain(NOTE_DELETE_LABEL);
      expect(menuItems()).toContain(NOTE_DELETE_LABEL);
      fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    }
  });

  it("renders no verb twice at any width", async () => {
    const resize = await openAtWidths();

    for (const width of [1400, 800, 700, 600, 500]) {
      resize(width);
      menuItems();
      for (const label of [
        ATTACHMENTS_LABEL,
        PROPERTIES_LABEL,
        NOTE_HISTORY_LABEL,
        SHOW_IN_FILES_LABEL,
        NOTE_DELETE_LABEL,
      ]) {
        // The row and the menu partition the verbs; neither gets a copy of the
        // other's. A promoted control still in the menu is what the predicate
        // exists to prevent.
        expect(screen.getAllByText(label)).toHaveLength(1);
      }
      fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    }
  });

  it("gives back what it promoted, in priority order, above what never promotes", async () => {
    const resize = await openAtWidths();

    resize(700);
    // Attachments and Properties are words in the row; History and Show in
    // Files came back to the menu, in the order the row would have shown them,
    // and above Export, which never leaves.
    expect(menuItems()).toEqual([
      NOTE_HISTORY_LABEL,
      SHOW_IN_FILES_LABEL,
      "Export…",
      NOTE_DELETE_LABEL,
    ]);
  });

  it("stays at 46.5's shape on a machine that never delivers an observation", async () => {
    // No hand-fired resize: `src/test/setup.ts`'s ResizeObserver records
    // nothing and delivers nothing, which is what every other suite in this
    // repository sees. The budget stays zero and the header is the one control
    // and the menu that 46.5 shipped — so a missing observation degrades to
    // the old shape rather than to a broken one.
    seedVault();
    restoreWidths = withActionWidths(WIDTHS);
    await openEditor();

    expect(words()).toEqual([]);
    expect(menuItems()).toEqual([
      ATTACHMENTS_LABEL,
      PROPERTIES_LABEL,
      NOTE_HISTORY_LABEL,
      SHOW_IN_FILES_LABEL,
      "Export…",
      NOTE_DELETE_LABEL,
    ]);
  });
});

/**
 * A panel that cannot open says so (Story 48.5).
 *
 * Both panels were `showX && mode === "edit"`, so pressing Properties while
 * reading an older version produced nothing whatsoever — and half of a 0.8.1
 * report ("editing tags on a recording note") is that silence. The other half
 * was that the control was buried in a menu, which the tests above address; a
 * control that is easy to find and then does nothing is the same report with a
 * shorter path to it.
 */
describe("a panel that will not open in this mode", () => {
  /** The sentence the pane leaves where the panel would have been. */
  function notice(): HTMLElement | null {
    return document.querySelector<HTMLElement>(`[data-slot="${PANEL_UNAVAILABLE_SLOT}"]`);
  }

  /** Press an item in the note's actions menu. */
  function pick(label: string): void {
    const trigger = screen.getByRole("button", { name: new RegExp(`^${NOTE_ACTIONS_LABEL}`) });
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("menuitem", { name: label }));
  }

  it("explains itself in history mode, and comes back with one press", async () => {
    await openEditor();

    pick(PROPERTIES_LABEL);
    expect(screen.getByRole("region", { name: PROPERTIES_LABEL })).toBeInTheDocument();
    expect(notice()).toBeNull();

    pick(NOTE_HISTORY_LABEL);
    await waitFor(() => {
      expect(notice()).not.toBeNull();
    });
    // Not merely absent: it says which of the pane's modes is the reason, and
    // names the panel it is standing in for.
    expect(notice()?.textContent).toContain(PROPERTIES_LABEL);
    expect(notice()?.textContent).toContain("older version");
    expect(screen.queryByRole("region", { name: PROPERTIES_LABEL })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: PANEL_BACK_LABEL }));
    await waitFor(() => {
      expect(screen.getByRole("region", { name: PROPERTIES_LABEL })).toBeInTheDocument();
    });
    expect(notice()).toBeNull();
  });

  it("says the same thing for the other panel the header opens", async () => {
    await openEditor();

    pick(ATTACHMENTS_LABEL);
    pick(NOTE_HISTORY_LABEL);
    await waitFor(() => {
      expect(notice()).not.toBeNull();
    });
    expect(notice()?.textContent).toContain(ATTACHMENTS_LABEL);
  });
});
