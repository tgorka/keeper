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

const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();
const notesRename = vi.fn<(v: string, n: string, title: string) => Promise<unknown>>();

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

/** Mount the editor on a note, and let its opening `Reset` land. `frame` is
 *  the holding surface's own controls, which only a panel has. */
async function openEditor(frame?: ReactNode, frontmatter = ""): Promise<void> {
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
  render(<NoteEditor vaultId="v1" noteId="n1" frame={frame} />);
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
    expect(names()).toEqual([
      ATTACHMENTS_LABEL,
      PROPERTIES_LABEL,
      NOTE_HISTORY_LABEL,
      SHOW_IN_FILES_LABEL,
    ]);

    resize(800);
    expect(names()).toEqual([ATTACHMENTS_LABEL, PROPERTIES_LABEL, NOTE_HISTORY_LABEL]);

    resize(700);
    expect(names()).toEqual([ATTACHMENTS_LABEL, PROPERTIES_LABEL]);

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
        PROPERTIES_LABEL,
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

    expect(names()).toEqual([]);
    expect(menuItems()).toEqual([
      ATTACHMENTS_LABEL,
      PROPERTIES_LABEL,
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
    expect(before).toContain(PROPERTIES_LABEL);
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

  /** The same geometry as the suite above, plus the frame group: two 32px
   *  controls and the 8px between them. */
  const FRAMED_WIDTHS: Record<string, number> = { ...WIDTHS, frame: 72 };

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
    await openEditor(PANEL_CONTROLS);
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

    // 800px unframed promotes three — the suite above asserts exactly that at
    // exactly this width. Framed, group 3 is owed 80px more (72 for the two
    // controls, 8 for the seam beside them): 800 - 160 - 8 - 90 - 8 - 72 - 8 =
    // 454, and 454 less the 198 the leading control and the trigger reserve
    // buys the 108 and the 100 but not the 74 behind them.
    resize(800);
    expect(names()).toEqual([ATTACHMENTS_LABEL, PROPERTIES_LABEL]);

    // And the row is still a row that grows: the frame group is a constant
    // subtraction, not a cap.
    resize(1400);
    expect(names()).toEqual([
      ATTACHMENTS_LABEL,
      PROPERTIES_LABEL,
      NOTE_HISTORY_LABEL,
      SHOW_IN_FILES_LABEL,
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

  /** Press an item in the note's actions menu, whichever kind of item it is. */
  function pick(label: string): void {
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
  /** Press an item in the note's actions menu, whichever kind of item it is. */
  function openProperties(): void {
    openNoteActions();
    const item =
      screen.queryByRole("menuitem", { name: PROPERTIES_LABEL }) ??
      screen.getByRole("menuitemcheckbox", { name: PROPERTIES_LABEL });
    fireEvent.click(item);
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
