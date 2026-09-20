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
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteBodyBatch, NoteVaultVm, NoteWriteVm } from "@/lib/ipc/client";
import { settleNoteEditorBoot } from "@/test/note-editor-boot";

const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();
const notesRename = vi.fn<(v: string, n: string, title: string) => Promise<unknown>>();
const notesNoteMarks = vi.fn(async () => ({ rev: "r0", ranges: [[2, 9]] as [number, number][] }));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => {}),
}));

vi.mock("@/lib/ipc/client", () => ({
  notesOpen: (v: string, n: string, on: (b: NoteBodyBatch) => void) => notesOpen(v, n, on),
  notesNoteMarks: () => notesNoteMarks(),
  notesClose: vi.fn(async () => {}),
  notesSave: vi.fn(async () => ({ frontmatter: "", rev: "r1", path: "n.md", conflictCopy: null })),
  notesBufferReport: vi.fn(async () => {}),
  notesTagTree: vi.fn(async () => ({ nodes: [] })),
  notesBacklinks: vi.fn(async () => []),
  notesResolveConflict: vi.fn(async () => {}),
  notesMarkRead: vi.fn(async () => {}),
  notesRename: (v: string, n: string, title: string) => notesRename(v, n, title),
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
  PANE_HEADER_FRAME_SLOT,
  PANE_HEADER_IDENTITY_SLOT,
  PANE_HEADER_LEADING_SLOT,
  PANE_HEADER_STATUS_SLOT,
  type PaneHeaderTitleBar,
} from "@/components/layout/pane-header";
import {
  beginSave,
  editBuffer,
  markSaved,
  markSaveFailed,
  resetNotesEditorStoreForTest,
} from "@/lib/stores/notes-editor";
import { notesFiltersStore } from "@/lib/stores/notes-filters";
import { notesVaultsStore, resetNotesVaultsStoreForTest } from "@/lib/stores/notes-vaults";
import { panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";
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

/** Mount the editor on a note, and let its opening `Reset` land.
 *
 *  `frame` is the holding surface's own controls — a panel's fold and close
 *  since Story 50.1, a capture window's pin, lock and close since 75.3.
 *  `titleBar` is the second half of that second host: the claim that this row
 *  is not a band inside somebody else's window but the window's own title bar.
 *  Both default to the shape the notes pane passes, which is nothing. */
async function openEditor(
  frame?: ReactNode,
  frontmatter = "",
  titleBar: PaneHeaderTitleBar | null = null,
  panelId?: string,
): Promise<void> {
  notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
    onBatch({
      kind: "reset",
      text: BODY,
      frontmatter,
      rev: "r0",
      cursor: null,
      path: NOTE_PATH,
    });
    return "sub-1";
  });
  render(
    <NoteEditor vaultId="v1" noteId="n1" frame={frame} titleBar={titleBar} panelId={panelId} />,
  );
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
 * slot carries `figures` and every digit there is as wide as every other. It is
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

// The editor's boot outlives a test that only needed the header; see the helper
// for the teardown race that costs.
afterEach(settleNoteEditorBoot);

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
    // stays in the DOM for a screen reader, and on the SLOT's `title` for a
    // pointer. It rides on the slot rather than on the word because in a
    // capture window's title bar the word is inert — the press has to reach
    // the box so it can drag the window — and a hover has to reach the same
    // box so the sentence is still readable when it does not fit.
    expect(captionSlot()).toHaveAttribute("title", REFUSED);
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

/**
 * Every verb the row is showing, by the name it answers to, in the row's order.
 *
 * The name and not the text: since 48.9 a promoted control is a glyph, so its
 * text content is empty and the word a user reaches it by is its accessible
 * name. Order is read off the DOM because the order is what these tests are
 * about — the row degrades from the end, and a set would not notice if it
 * stopped doing that.
 */
function names(): string[] {
  return Array.from(document.querySelectorAll("[data-priority-action]")).map((control) => {
    const name = control.getAttribute("aria-label") ?? "";
    // The attribute finds the control; this proves the name is one a screen
    // reader would compute, rather than an attribute nobody consumes.
    expect(screen.getByRole("button", { name })).toBe(control);
    return name;
  });
}

/**
 * Every item in the note's actions menu, in DOM order.
 *
 * Two roles, not one. Since Story 49 the two verbs that open a panel are
 * `menuitemcheckbox` down here rather than `menuitem`: the state the promoted
 * control carries as `aria-expanded` has to survive the demotion, and a menu's
 * word for "this one is on" is a checkbox item. A query for the single role
 * would have quietly stopped seeing half of this menu.
 */
const MENU_ITEM_SELECTOR = '[role="menuitem"],[role="menuitemcheckbox"]';

/** Open the note's own actions menu. */
function openNoteActions(): void {
  const trigger = screen.getByRole("button", { name: new RegExp(`^${NOTE_ACTIONS_LABEL}`) });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.click(trigger);
}

/** Open the note's own actions menu, and hand back what is in it. */
function menuItems(): string[] {
  openNoteActions();
  return Array.from(document.querySelectorAll(MENU_ITEM_SELECTOR)).map(
    (item) => item.textContent ?? "",
  );
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
    expect(names()).toEqual([ATTACHMENTS_LABEL, NOTE_HISTORY_LABEL, SHOW_IN_FILES_LABEL]);

    resize(800);
    expect(names()).toEqual([ATTACHMENTS_LABEL, NOTE_HISTORY_LABEL, SHOW_IN_FILES_LABEL]);

    resize(700);
    expect(names()).toEqual([ATTACHMENTS_LABEL, NOTE_HISTORY_LABEL]);

    // What the 560px capture window is near: the two panels the owner reported
    // as missing are the last things to go, and Attachments is the last of all.
    resize(600);
    expect(names()).toEqual([ATTACHMENTS_LABEL]);

    resize(500);
    expect(names()).toEqual([]);
  });

  it("moves the first verb into the menu at exactly one width", async () => {
    const resize = await openAtWidths();

    resize(572);
    expect(names()).toEqual([ATTACHMENTS_LABEL]);
    resize(571);
    expect(names()).toEqual([]);
  });

  it("keeps Delete in the menu, and never in the row, at every width", async () => {
    const resize = await openAtWidths();

    for (const width of [1400, 800, 700, 600, 500, 0]) {
      resize(width);
      expect(names()).not.toContain(NOTE_DELETE_LABEL);
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
        NOTE_HISTORY_LABEL,
        SHOW_IN_FILES_LABEL,
        NOTE_DELETE_LABEL,
      ]) {
        // The row and the menu partition the verbs; neither gets a copy of the
        // other's. A promoted control still in the menu is what the predicate
        // exists to prevent.
        //
        // Counted across BOTH roles by name (48.9): promoted, a verb is a glyph
        // with a name and no text; in the menu it is a word. A text query sees
        // only the second and would pass over a duplicated control. `hidden`
        // because Radix marks everything outside the open menu `aria-hidden`.
        const asControl = screen.queryAllByRole("button", { hidden: true, name: label });
        const asItem = [
          ...screen.queryAllByRole("menuitem", { hidden: true, name: label }),
          ...screen.queryAllByRole("menuitemcheckbox", { hidden: true, name: label }),
        ];
        expect(asControl.length + asItem.length).toBe(1);
      }
      fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    }
  });

  it("gives back what it promoted, in priority order, above what never promotes", async () => {
    const resize = await openAtWidths();

    resize(700);
    // Attachments is a word in the row; Show in Files came back to the menu, in
    // the order the row would have shown it, and above Export, which never
    // leaves. Properties is in neither list — it is a leading control, so it is
    // in the row at every width and can never come back to the menu.
    // Asserted before the menu is opened: an open Radix menu hides the rest of
    // the header from the accessibility tree, so a control checked afterwards
    // would look absent whether it was there or not.
    expect(screen.getByRole("button", { name: PROPERTIES_LABEL })).toBeVisible();
    expect(menuItems()).toEqual([SHOW_IN_FILES_LABEL, "Export…", NOTE_DELETE_LABEL]);
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

    expect(names()).toEqual([]);
    expect(menuItems()).toEqual([
      ATTACHMENTS_LABEL,
      NOTE_HISTORY_LABEL,
      SHOW_IN_FILES_LABEL,
      "Export…",
      NOTE_DELETE_LABEL,
    ]);
  });

  /**
   * Story 49: the two panel verbs say whether their panel is open.
   *
   * `showProperties` and `showAttachments` have been `useState` booleans with
   * toggle handlers since 45.x, rendered as plain actions with no state on them
   * in either direction — so the only way to learn whether Properties was
   * already open was to look down the pane and recognise the panel. Asserted
   * through `expanded` on a role+name query rather than through a class,
   * because the claim is about what the control reports, not about how it is
   * painted.
   */
  it("says whether the panel it opens is open, and does not resize the row saying it", async () => {
    const resize = await openAtWidths();
    resize(1400);

    const before = names();
    // Not among the candidates, and that is the claim: a leading control is in
    // the row whatever the budget says, so it is never in `names()` and never
    // in the menu either.
    expect(before).not.toContain(PROPERTIES_LABEL);
    // Closed, and saying so.
    expect(screen.getByRole("button", { name: PROPERTIES_LABEL, expanded: false })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: PROPERTIES_LABEL }));

    const open = screen.getByRole("button", { name: PROPERTIES_LABEL, expanded: true });
    // And it names what it opened, rather than leaving a screen reader to guess
    // which of the strips below this header appeared because of this press.
    const region = document.getElementById(open.getAttribute("aria-controls") ?? "");
    expect(region).not.toBeNull();
    expect(
      within(region as HTMLElement).getByRole("region", { name: PROPERTIES_LABEL }),
    ).toBeInTheDocument();

    // The row did not change shape. Promotion is decided from one measurement
    // per candidate, so a pressed treatment with a width — a border, a ring, a
    // longer name — would make how many verbs are on screen a function of which
    // panels are open, and the header would reflow when somebody opened one.
    expect(names()).toEqual(before);

    // The same control closes it. It was a toggle all along; now it says so.
    fireEvent.click(open);
    expect(screen.getByRole("button", { name: PROPERTIES_LABEL, expanded: false })).toBeVisible();
  });

  it("keeps that state when the verb is too narrow to promote and falls into the menu", async () => {
    // The zero-budget shape: everything is in the menu, which is exactly where
    // a state carried only by the promoted control would have disappeared.
    seedVault();
    restoreWidths = withActionWidths(WIDTHS);
    await openEditor();

    openNoteActions();
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: ATTACHMENTS_LABEL }));

    openNoteActions();
    expect(
      screen.getByRole("menuitemcheckbox", { name: ATTACHMENTS_LABEL, checked: true }),
    ).toBeInTheDocument();
    // And the verb that discloses nothing is still a plain item, so the menu
    // does not grow a column of empty tick-boxes beside History and Export.
    expect(screen.getByRole("menuitem", { name: NOTE_HISTORY_LABEL })).toBeInTheDocument();
  });
});

/**
 * One row for a note in a panel, and the panel's controls in it (Story 50.1).
 *
 * The owner's report is "merge 2 pierwsze linijki note w jedna". A note open in
 * a panel drew TWO 40px bands: the panel's, whose entire content was the word
 * `Note` and its fold and close, and this header underneath it. The word says
 * nothing the note's own title does not say better, so the panel gives up its
 * row and hands its two controls down here.
 *
 * What the merge can break is the arithmetic. Group 3 decides how many verbs
 * are on screen from the pixels the row can spare, and two controls that were
 * not in this row before are 80px it can no longer spare. `panel-strip.test.
 * tsx` proves the panel stopped drawing a row; these prove that the row it
 * stopped drawing arrived here intact, and that group 3 was told.
 */
describe("a note in a panel: one row, carrying the panel's own controls", () => {
  const FOLD_LABEL = "Fold panel";
  const CLOSE_LABEL = "Close panel";

  /** What a panel hands down. Plain buttons, because what these are is
   *  `panel-strip.tsx`'s decision and this file's claim is only about where
   *  the header puts whatever it is given. */
  const PANEL_CONTROLS = (
    <>
      <button type="button">{FOLD_LABEL}</button>
      <button type="button">{CLOSE_LABEL}</button>
    </>
  );

  /** Frame controls and leading navigation each cost two 32px targets plus an 8px gap. */
  const FRAMED_WIDTHS: Record<string, number> = { ...WIDTHS, frame: 72, navigation: 72 };

  let restoreWidths: (() => void) | null = null;
  let observer: { resize: (width: number) => void; undo: () => void } | null = null;

  afterEach(() => {
    restoreWidths?.();
    restoreWidths = null;
    observer?.undo();
    observer = null;
  });

  async function openFramed(): Promise<(width: number) => void> {
    seedVault();
    restoreWidths = withActionWidths(FRAMED_WIDTHS);
    observer = withHandFiredResize();
    await openEditor(PANEL_CONTROLS, "", null, panelsStore.getState().activeId);
    const { resize } = observer;
    return (width) => {
      act(() => resize(width));
    };
  }

  it("draws one header, with the panel's controls last and outside the verbs", async () => {
    await openFramed();

    const row = headerRow();
    // One row and not two: the whole point of the merge. The editor's header
    // is the only `<header>` this mount produces, and the panel's controls are
    // in it rather than in a band above it.
    expect(document.querySelectorAll("header")).toHaveLength(1);
    expect(Array.from(row.children).map((child) => child.getAttribute("data-slot"))).toEqual([
      PANE_HEADER_LEADING_SLOT,
      PANE_HEADER_IDENTITY_SLOT,
      PANE_HEADER_STATUS_SLOT,
      PANE_HEADER_ACTIONS_SLOT,
      PANE_HEADER_FRAME_SLOT,
    ]);
    const frame = row.querySelector<HTMLElement>(`[data-slot="${PANE_HEADER_FRAME_SLOT}"]`);
    expect(within(frame as HTMLElement).getByRole("button", { name: FOLD_LABEL })).toBeVisible();
    expect(within(frame as HTMLElement).getByRole("button", { name: CLOSE_LABEL })).toBeVisible();
  });

  it("keeps the way out of the panel out of the note's overflow at every width", async () => {
    const resize = await openFramed();

    for (const width of [1400, 800, 600, 400, 0]) {
      resize(width);
      // Fold and close are the panel's, not the note's, and a verb that acts on
      // the frame must not be findable only by opening the surface's menu — the
      // 0.8.1 reports behind Story 48.5 are what that costs. They are controls
      // at every width, including the one where the note has promoted nothing.
      expect(screen.getByRole("button", { name: FOLD_LABEL })).toBeInTheDocument();
      expect(screen.getByRole("button", { name: CLOSE_LABEL })).toBeInTheDocument();
      const promoted = names();
      expect(promoted).not.toContain(FOLD_LABEL);
      expect(promoted).not.toContain(CLOSE_LABEL);
      // Opened once and read once: the trigger goes `aria-hidden` while the
      // menu is up, so a second `menuItems()` in the same breath cannot find
      // the control it needs to press.
      const inMenu = menuItems();
      expect(inMenu).not.toContain(FOLD_LABEL);
      expect(inMenu).not.toContain(CLOSE_LABEL);
      fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    }
  });

  it("charges the row for them, so one fewer verb promotes at the same width", async () => {
    const resize = await openFramed();

    // The panel's frame and navigation each charge 72px plus an 8px seam.
    // 800 - 160 - 8 - 90 - 8 - 72 - 8 - 72 - 8 = 374; after the
    // actions' 198px reservation, only the first 108px verb fits.
    resize(800);
    expect(names()).toEqual([ATTACHMENTS_LABEL]);

    // And the row is still a row that grows: the frame group is a constant
    // subtraction, not a cap.
    resize(1400);
    expect(names()).toEqual([ATTACHMENTS_LABEL, NOTE_HISTORY_LABEL, SHOW_IN_FILES_LABEL]);
  });
});

/**
 * A note in the quick-capture window: the row IS the window (Story 75.3).
 *
 * The suite above is the same merge for a panel, and stops where the panel
 * does: a panel is a band inside a window somebody else draws, so its header
 * can be a header and nothing more. The capture window has nothing above this
 * row. It is the title bar — the thing you grab to move the window, the only
 * place the platform's close button can be, and the row that has to hold six
 * controls inside 560px and then inside the minimum.
 *
 * `PaneHeader` learns that as ONE prop (`titleBar`), because it is one fact
 * about the host with three consequences, and a reviewer's first question
 * about a shared component gaining a prop is what it did to the hosts that do
 * not pass it. The first two tests here answer exactly that and nothing else.
 *
 * What these cannot prove, for the usual reason: jsdom performs no layout, so
 * no test here measures a pixel. The widths come from `withActionWidths`, the
 * arithmetic is keeper's own, and whether the row FITS at 560 and at the
 * minimum is a browser fact checked on the gate.
 */
describe("a note in the quick-capture window: the row is the window's title bar", () => {
  /** What the capture window hands down — pin, lock and close. Plain buttons
   *  with this file's own strings, for the same reason `PANEL_CONTROLS` is:
   *  what these controls ARE is `capture-window.tsx`'s decision and is
   *  asserted there. The claim here is only about where the row puts them and
   *  what it refuses to do to them. */
  const PIN_LABEL = "Float above other windows";
  const LOCK_LABEL = "Lock this window's position and size";
  const WINDOW_CLOSE_LABEL = "Close this capture window";
  const WINDOW_CONTROLS = (
    <>
      <button type="button">{PIN_LABEL}</button>
      <button type="button">{LOCK_LABEL}</button>
      <button type="button">{WINDOW_CLOSE_LABEL}</button>
    </>
  );

  /** The same geometry as the suites above, plus a frame group of three
   *  `icon-xs` window controls: 3 × 24 + 2 × 8 = 88. Sixteen more than the
   *  panel's 72 for one more control, because the window's controls are
   *  24px where a panel's are 32 — the whole reason 88 fits where 104 would
   *  not have. */
  const CAPTURE_WIDTHS: Record<string, number> = { ...WIDTHS, frame: 88 };

  /** Every element the drag shim would match, in DOM order. Tauri's
   *  `data-tauri-drag-region` is a document-level `mousedown` listener that
   *  compares against the EXACT element the pointer went down on and never
   *  walks up to an ancestor, so "which elements carry it" is the whole of
   *  "where can this window be dragged from". */
  function dragRegions(): string[] {
    return Array.from(document.querySelectorAll("[data-tauri-drag-region]")).map(
      (element) => element.getAttribute("data-slot") ?? element.tagName.toLowerCase(),
    );
  }

  function identitySlot(): HTMLElement {
    const found = document.querySelector<HTMLElement>(`[data-slot="${PANE_HEADER_IDENTITY_SLOT}"]`);
    if (found === null) {
      throw new Error("the header drew no identity group");
    }
    return found;
  }

  function actionsSlot(): HTMLElement {
    const found = document.querySelector<HTMLElement>(`[data-slot="${PANE_HEADER_ACTIONS_SLOT}"]`);
    if (found === null) {
      throw new Error("the header drew no actions group");
    }
    return found;
  }

  let restoreWidths: (() => void) | null = null;
  let observer: { resize: (width: number) => void; undo: () => void } | null = null;

  afterEach(() => {
    restoreWidths?.();
    restoreWidths = null;
    observer?.undo();
    observer = null;
  });

  /** Mount the editor as the capture window mounts it: the three window
   *  controls as the frame group, and the claim that this row is the title
   *  bar. `draggable` is the live lock — a locked window does not move, so its
   *  title bar is not a drag handle. */
  async function openTitleBar(draggable = true): Promise<(width: number) => void> {
    seedVault();
    restoreWidths = withActionWidths(CAPTURE_WIDTHS);
    observer = withHandFiredResize();
    await openEditor(WINDOW_CONTROLS, "", { draggable });
    const { resize } = observer;
    return (width) => {
      act(() => resize(width));
    };
  }

  it("changes nothing for the notes pane, which is not in a window of its own", async () => {
    seedVault();
    await openEditor();

    // The blast radius, asserted rather than asserted-about. Three claims,
    // one per consequence the prop carries: a pane that passes no `titleBar`
    // marks nothing draggable, puts no CSS floor under its title, and keeps
    // the caption slot unsqueezable — which is 46.4's contract and is what the
    // first suite in this file exists to protect.
    expect(dragRegions()).toEqual([]);
    expect(identitySlot()).not.toHaveClass("min-w-40");
    expect(captionSlot()).toHaveClass("shrink-0");
  });

  it("changes nothing for a note in a panel, which has a frame but not a window", async () => {
    seedVault();
    restoreWidths = withActionWidths({ ...WIDTHS, frame: 72 });
    await openEditor(
      <>
        <button type="button">Fold panel</button>
        <button type="button">Close panel</button>
      </>,
    );

    // The harder half of the same question: this host DOES pass a frame
    // group, so it exercises every line the new prop touches except the prop
    // itself. A panel's header must not become draggable because a window's
    // did, and dragging it would move the whole application window.
    expect(dragRegions()).toEqual([]);
    expect(identitySlot()).not.toHaveClass("min-w-40");
    expect(captionSlot()).toHaveClass("shrink-0");
  });

  it("marks the row, the title and the window's controls as the drag handle", async () => {
    await openTitleBar();

    // The 32px strip this replaces was marked as one box across the window's
    // full width. The merged row is marked in four: the header itself (its
    // padding and the gaps between the groups), the identity group (the
    // title, and every pixel of slack in the row, which is most of it), the
    // status slot (the save caption's own box, which sits in the middle of the
    // row and would otherwise be a dead zone the width of `Saved · 12:34`),
    // and the frame wrapper (the seams around pin, lock and close).
    // Contiguous, 40px tall rather than 32, and larger than what it replaces.
    expect(dragRegions()).toEqual([
      "header",
      PANE_HEADER_IDENTITY_SLOT,
      PANE_HEADER_STATUS_SLOT,
      PANE_HEADER_FRAME_SLOT,
    ]);

    // And NOT the actions group: a press that missed Attach by two pixels
    // must not move the window instead of opening the panel.
    expect(actionsSlot()).not.toHaveAttribute("data-tauri-drag-region");
    // The title, the path and the caption are inside their marked boxes rather
    // than being them, and the shim does not walk up — so they are made inert,
    // which turns a press on the title or on the caption into a press on the
    // box that holds it.
    expect(identitySlot()).toHaveClass("[&>*]:pointer-events-none");
    expect(captionSlot()).toHaveClass("[&>*]:pointer-events-none");
  });

  it("marks nothing when the window is locked, because a locked window does not move", async () => {
    await openTitleBar(false);

    // The lock's whole point. This is the old `capture-window.test.tsx`
    // assertion — the drag region is conditional — moved to where the
    // attribute now lives: the strip that used to carry it is gone, and the
    // condition survived the move.
    expect(dragRegions()).toEqual([]);
    // The controls are still there and still findable: locking freezes the
    // geometry, it does not take the way out away.
    expect(screen.getByRole("button", { name: WINDOW_CLOSE_LABEL })).toBeVisible();
  });

  it("makes the caption the one thing that gives, and the title the one thing that truncates", async () => {
    await openTitleBar();

    const row = headerRow();
    // AD-260's order, read off the row. Every member but the caption refuses
    // to shrink — identity because a title bar's drag handle may not collapse
    // (`flex-1` off a zero basis has a scaled shrink factor of zero, so
    // without a floor it surrenders all 160px), actions and frame because
    // neither may clip a control. So the caption is the only box a deficit
    // can come out of, which is precisely AD-260's "the status caption gives
    // first".
    const squeezable = Array.from(row.children).filter(
      (child) => !child.classList.contains("shrink-0"),
    );
    expect(squeezable.map((child) => child.getAttribute("data-slot"))).toEqual([
      PANE_HEADER_IDENTITY_SLOT,
      PANE_HEADER_STATUS_SLOT,
    ]);
    expect(captionSlot()).toHaveClass("min-w-0", "overflow-hidden");

    // Identity is in that list only because `flex-1` implies `shrink`; what
    // stops it is the floor, and the floor is 160px twice — once in the
    // arithmetic as PANE_HEADER_IDENTITY_MIN_PX, once in CSS as `min-w-40`,
    // because the arithmetic decides how many verbs promote and only the CSS
    // decides what a real browser does with negative free space.
    expect(identitySlot()).toHaveClass("min-w-40", "flex-1");
    // Second, and only second: once the caption is gone the title is what
    // gives, and it gives by truncating rather than by pushing anything out.
    expect(within(identitySlot()).getByRole("heading")).toHaveClass("truncate");
  });

  it("stops squeezing the caption when it is the reason a write was refused", async () => {
    await openTitleBar();

    const REFUSED = "the vault is read-only and the write was refused";
    act(() => {
      markSaveFailed("v1", "n1", REFUSED);
    });
    await waitFor(() => {
      expect(shownWord()).toBe(REFUSED);
    });

    // AD-260's order reverses for this one caption, because in a capture
    // window it is the ONLY place a refused write says why (UX-DR35) — and
    // the window does not close while it is refused, so a person told nothing
    // is a person pressing Escape at a window that ignores them. The caption
    // keeps its box and identity gives up its floor instead: at the 400px
    // minimum the row then asks for 330px rather than 400, so the sentence is
    // on screen instead of clipped to nothing.
    expect(captionSlot()).toHaveClass("shrink-0");
    expect(captionSlot()).not.toHaveClass("overflow-hidden");
    expect(identitySlot()).not.toHaveClass("min-w-40");

    // What gives instead is the note's name, and it gives the way it always
    // does — by truncating, never by pushing a control out of the row. The
    // whole sentence stays readable on the slot's tooltip.
    expect(within(identitySlot()).getByRole("heading")).toHaveClass("truncate");
    expect(captionSlot()).toHaveAttribute("title", REFUSED);
  });

  it("keeps the way out of the WINDOW on screen at every width, including the minimum", async () => {
    const resize = await openTitleBar();

    // Content widths, because that is what a `ResizeObserver` reports: 1400
    // is a window nobody has, 800 a stretched one, 536 the default 560 less
    // its gutters, 376 the 400px floor this story derives less the same, and 0
    // a row no observer has answered for yet. AD-260 forbids dropping an
    // action to make room at any of them: a control that disappears at a width
    // is a control nobody can rely on, and these three are the pin, the lock
    // and the way to shut the window.
    for (const width of [1400, 800, 536, 376, 0]) {
      resize(width);
      for (const label of [PIN_LABEL, LOCK_LABEL, WINDOW_CLOSE_LABEL]) {
        expect(screen.getByRole("button", { name: label })).toBeInTheDocument();
        expect(names()).not.toContain(label);
      }
      // Nor are they hiding in the note's menu — a verb that acts on the
      // window must never be reachable only by opening the document's `…`.
      const inMenu = menuItems();
      for (const label of [PIN_LABEL, LOCK_LABEL, WINDOW_CLOSE_LABEL]) {
        expect(inMenu).not.toContain(label);
      }
      fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    }
  });

  it("charges the row for them, and demotes two verbs at the default 560px width", async () => {
    const resize = await openTitleBar();

    // Capture has window controls but no navigation stack: 88px of frame and
    // its seam, unlike the panel's two separate 72px groups. Its budget is
    // 800 - 160 - 8 - 90 - 8 - 88 - 8 = 438, less
    // the 198 the leading control and the trigger reserve, buys the 108 and
    // the 100 but not the 74.
    resize(800);
    expect(names()).toEqual([ATTACHMENTS_LABEL, NOTE_HISTORY_LABEL]);

    // 536 is `CAPTURE_DEFAULT_SIZE`'s 560 less the row's own 12px gutters —
    // a `ResizeObserver` reports CONTENT width, which is what this helper
    // delivers and what the budget is computed from. 536 - 160 - 8 - 90 - 8 -
    // 88 - 8 = 174, and the leading Properties control plus the `…` trigger
    // reserve 198 before a single candidate is considered.
    //
    // So nothing promotes, and that is the behaviour change AD-260 accepts
    // rather than the defect it fixes: at the default width two verbs that
    // used to be glyphs in the row are now words in the menu. The price of the
    // alternative is dropping a control, which AD-260 forbids, or a fourth
    // overflow mechanism, which nobody asked for. Attach and Properties are
    // the group's own always-out controls and are unaffected.
    resize(536);
    expect(names()).toEqual([]);
    const demoted = menuItems();
    expect(demoted).toContain(ATTACHMENTS_LABEL);
    expect(demoted).toContain(NOTE_HISTORY_LABEL);
    expect(demoted).toContain(SHOW_IN_FILES_LABEL);
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });

    // And the row still grows: the frame group is a constant subtraction, not
    // a cap, so a window someone stretched gets its verbs back.
    resize(1400);
    expect(names()).toEqual([ATTACHMENTS_LABEL, NOTE_HISTORY_LABEL, SHOW_IN_FILES_LABEL]);
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

  /** Press a verb, wherever it lives: Properties is a leading control that never
   *  enters the menu, and everything else is a menu item of one kind or another. */
  function pick(label: string): void {
    const control = screen.queryByRole("button", { name: label });
    if (control !== null) {
      fireEvent.click(control);
      return;
    }
    openNoteActions();
    const item =
      screen.queryByRole("menuitem", { name: label }) ??
      screen.getByRole("menuitemcheckbox", { name: label });
    fireEvent.click(item);
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

/**
 * A retitle renames the note's file (Story 51.6, FR-97; matrix row 12).
 *
 * **The story is reachability, so the test is a call site.** `notes_rename` has
 * been built, registered and wrapped since FR-97 and had no caller anywhere in
 * `src/` — the command worked and nothing asked it anything, so every note has
 * been carrying whatever filename it was created with however often its title
 * changed. Nothing in the repo would have caught that: a suite over the command
 * would have passed, and a suite over the panel would not have known the command
 * existed. What fails without the wiring is this: press the field, and see
 * whether the verb runs.
 *
 * The vault and note ids matter as much as the title. `notes_rename` resolves the
 * note by ULID and derives the filename itself, which is why a note needs no
 * pointer rewriting where a session file needs a journaled plan — and why passing
 * a path here would be the wrong argument for a command whose whole premise is
 * that the path is not the identity.
 */
describe("a note's title, changed in the properties panel", () => {
  /** Press the disclosure control, which is in the header at every width and
   *  never in the menu — that is the point of it being a leading control. */
  function openProperties(): void {
    fireEvent.click(screen.getByRole("button", { name: PROPERTIES_LABEL }));
  }

  it("renames the file, through the command FR-97 shipped and nobody called", async () => {
    await openEditor(undefined, "---\ntitle: Meeting\n---\n");
    openProperties();

    const field = await screen.findByRole("textbox", { name: "title" });
    fireEvent.change(field, { target: { value: "Kick Off" } });
    fireEvent.blur(field);

    await waitFor(() => expect(notesRename).toHaveBeenCalledWith("v1", "n1", "Kick Off"));
  });

  it("leaves every other property alone, so nothing else moves a file", async () => {
    await openEditor(undefined, "---\ntitle: Meeting\nowner: ada\n---\n");
    openProperties();

    const field = await screen.findByRole("textbox", { name: "owner" });
    fireEvent.change(field, { target: { value: "grace" } });
    fireEvent.blur(field);

    // The write happened; the rename did not. A panel that renamed on any write
    // would move a file because somebody corrected a typo in `owner:`.
    await waitFor(() => expect(field).toHaveValue("grace"));
    expect(notesRename).not.toHaveBeenCalled();
  });
});

describe("persistent list marks", () => {
  afterEach(() => {
    notesFiltersStore.getState().setText("");
  });
  it("paints a current reply and clears when the query clears", async () => {
    notesNoteMarks.mockResolvedValue({ rev: "r0", ranges: [[2, 9]] });
    notesFiltersStore.getState().setText("Meeting");
    await openEditor();
    await settleNoteEditorBoot();
    await waitFor(() =>
      expect(document.querySelector(".cm-search-mark")).toHaveTextContent("Meeting"),
    );
    act(() => notesFiltersStore.getState().setText(""));
    expect(document.querySelector(".cm-search-mark")).toBeNull();
  });
  it("does not paint a stale revision", async () => {
    notesNoteMarks.mockResolvedValue({ rev: "older", ranges: [[2, 9]] });
    notesFiltersStore.getState().setText("Meeting");
    await openEditor();
    await settleNoteEditorBoot();
    await waitFor(() => expect(notesNoteMarks).toHaveBeenCalled());
    expect(document.querySelector(".cm-search-mark")).toBeNull();
  });
  it("clears the first note and paints only the second note's ranges", async () => {
    notesFiltersStore.getState().setText("word");
    notesNoteMarks.mockResolvedValue({ rev: "r0", ranges: [[0, 3]] });
    notesOpen.mockImplementation(async (_vault, note, onBatch) => {
      onBatch({
        kind: "reset",
        text: note === "n1" ? "one word" : "two word",
        frontmatter: "",
        rev: "r0",
        cursor: null,
        path: `${note}.md`,
      });
      return `sub-${note}`;
    });
    const { rerender } = render(<NoteEditor vaultId="v1" noteId="n1" />);
    await settleNoteEditorBoot();
    await waitFor(() => expect(document.querySelector(".cm-search-mark")).toHaveTextContent("one"));
    rerender(<NoteEditor vaultId="v1" noteId="n2" />);
    await waitFor(() => expect(document.querySelector(".cm-search-mark")).toHaveTextContent("two"));
    expect(document.querySelector(".cm-content")).not.toHaveTextContent("one");
  });
  it("keeps Escape-dismissed marks hidden through a refreshed body and trailing whitespace", async () => {
    notesNoteMarks.mockResolvedValue({ rev: "r0", ranges: [[2, 9]] });
    notesFiltersStore.getState().setText("Meeting");
    await openEditor();
    await settleNoteEditorBoot();
    await waitFor(() =>
      expect(document.querySelector(".cm-search-mark")).toHaveTextContent("Meeting"),
    );
    const content = document.querySelector(".cm-content");
    if (!content) throw new Error("Editor missing");
    fireEvent.keyDown(content, { key: "Escape" });
    expect(document.querySelector(".cm-search-mark")).toBeNull();
    const calls = notesNoteMarks.mock.calls.length;
    await act(async () => {
      notesFiltersStore.getState().setText("Meeting ");
      notesOpen.mock.calls[notesOpen.mock.calls.length - 1]?.[2]({
        kind: "reset",
        text: `${BODY}\nAnother line`,
        frontmatter: "",
        rev: "r1",
        cursor: null,
        path: "n.md",
      });
    });
    expect(notesNoteMarks).toHaveBeenCalledTimes(calls);
    expect(document.querySelector(".cm-search-mark")).toBeNull();
    notesNoteMarks.mockResolvedValue({ rev: "r1", ranges: [[2, 9]] });
    act(() => notesFiltersStore.getState().setText("Meet"));
    await waitFor(() =>
      expect(document.querySelector(".cm-search-mark")).toHaveTextContent("Meeting"),
    );
  });
});
