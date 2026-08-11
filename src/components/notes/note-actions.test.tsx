/**
 * Story 45.17: deleting a note, from the door a person actually uses.
 *
 * **The first two tests mount the real `NoteEditor`**, and that is the point
 * rather than thoroughness. Epic 44 shipped three tray listeners that were
 * declared and never mounted, because `renderHook` mounts the hook itself and
 * can never see that nothing else does. A `NoteActions` test that rendered
 * `NoteActions` would have exactly that shape: green over a menu no surface
 * puts on screen. So the delete path is driven from the editor's own header,
 * through its own boot, and what is asserted is that Rust received the ids the
 * editor was opened on.
 *
 * The rest drive the component directly, for the cases the editor cannot
 * produce — a child item from another story, and a plan that fails.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteBodyBatch, NoteDeletePlanVm } from "@/lib/ipc/client";

const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();
const notesDeletePlan = vi.fn<(v: string, n: string) => Promise<NoteDeletePlanVm>>();
const notesDelete = vi.fn<(v: string, n: string) => Promise<void>>();

vi.mock("@/lib/ipc/client", () => ({
  notesOpen: (v: string, n: string, on: (b: NoteBodyBatch) => void) => notesOpen(v, n, on),
  notesDeletePlan: (v: string, n: string) => notesDeletePlan(v, n),
  notesDelete: (v: string, n: string) => notesDelete(v, n),
  notesClose: vi.fn(async () => {}),
  notesSave: vi.fn(async () => ({ frontmatter: "", rev: "r1", path: "n.md", conflictCopy: null })),
  notesBufferReport: vi.fn(async () => {}),
  notesTagTree: vi.fn(async () => ({ nodes: [] })),
  tagsVocabulary: vi.fn(async () => ({ entries: [] })),
  notesAttachSources: vi.fn(async () => []),
  notesBacklinks: vi.fn(async () => []),
  notesResolveConflict: vi.fn(async () => {}),
  notesTemplateUpdatePreview: vi.fn(async () => null),
  notesMarkRead: vi.fn(async () => {}),
  notesDiff: vi.fn(async () => null),
  notesHistory: vi.fn(async () => []),
  recordingNoteTargets: vi.fn(async () => null),
  recordingOpenPath: vi.fn(async () => {}),
  revealPath: vi.fn(async () => {}),
}));

import { EXPORT_NOTE_LABEL } from "@/components/export/export-note-item";
import { PANE_HEADER_ACTIONS_SLOT } from "@/components/layout/pane-header";
import { DropdownMenuItem } from "@/components/ui/dropdown-menu";
import { resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import { panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";
import { withRangeRects } from "@/test/layout";
import { ATTACH_FILE_LABEL } from "./attach-file-button";
import { ATTACHMENTS_LABEL } from "./attachments-panel";
import {
  NOTE_ACTIONS_LABEL,
  NOTE_ACTIONS_TEXT,
  NOTE_DELETE_LABEL,
  NoteActions,
} from "./note-actions";
import {
  NOTE_DELETE_CANCEL,
  NOTE_DELETE_CONFIRM,
  NOTE_DELETE_NO_PLAN,
  NOTE_DELETE_TESTID,
} from "./note-delete-dialog";
import { NoteEditor } from "./note-editor";
import { NOTE_HISTORY_LABEL } from "./note-history-panel";
import { PROPERTIES_LABEL } from "./properties-panel";

// jsdom does no layout, so CodeMirror's measure pass throws outside every `try`
// a test can write and takes the run's exit code while the summary still prints
// passes. Never hand-rolled — `src/test/layout.ts` owns the shim.
let restoreRects: (() => void) | null = null;

beforeAll(() => {
  restoreRects = withRangeRects();
});

afterAll(() => {
  restoreRects?.();
  restoreRects = null;
});

/** The frontmatter the body channel carries beside the buffer. */
const BLOCK = "---\nid: 01SEEDNOTE\ncreated: 2026-08-09T10:00:00+02:00\n---\n";

/** A plan whose every field is distinguishable from anything a paraphrase would build. */
const PLAN: NoteDeletePlanVm = {
  path: "meetings/2026-08-09-standup.md",
  question: 'Delete "Standup"?',
  consequence: "keeper removes meetings/2026-08-09-standup.md from this vault.",
  recovery: "keeper moves it into the vault's trash.",
};

function openOn(body: string): void {
  notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
    onBatch({
      kind: "reset",
      text: body,
      frontmatter: BLOCK,
      rev: "r0",
      cursor: null,
      path: "meetings/2026-08-09-standup.md",
    });
    return "sub-1";
  });
}

/**
 * Open the actions menu and hand back the menu element.
 *
 * Radix opens on POINTER-DOWN, not on `click` — a `fireEvent.click` on the
 * trigger does nothing under jsdom and the test then times out looking for a
 * menu that was never asked for. Same helper shape as `account-footer.test`.
 */
async function openActions(name: string | RegExp): Promise<HTMLElement> {
  const trigger = screen.getByRole("button", { name });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.pointerUp(trigger, { button: 0 });
  return await screen.findByRole("menu");
}

beforeEach(() => {
  vi.clearAllMocks();
  notesDeletePlan.mockResolvedValue(PLAN);
  notesDelete.mockResolvedValue(undefined);
  resetPanelsStoreForTest();
});

afterEach(() => {
  resetNotesEditorStoreForTest();
  resetPanelsStoreForTest();
});

describe("deleting the open note, from the editor's own header", () => {
  /**
   * The menu is on screen in the real editor, named for the note, and the
   * delete item asks Rust about **this** note.
   *
   * The ids are asserted, not merely that a command was called: a menu that
   * rendered and handed on the wrong note is the shape that shipped twice in
   * wave 2, and it looks identical from the outside.
   */
  it("asks Rust what deleting this note would remove", async () => {
    openOn("# Standup\n");
    render(<NoteEditor vaultId="v1" noteId="note-7" />);

    const menu = await openActions(new RegExp(`^${NOTE_ACTIONS_LABEL}`));
    fireEvent.click(within(menu).getByRole("menuitem", { name: NOTE_DELETE_LABEL }));

    await waitFor(() => expect(notesDeletePlan).toHaveBeenCalledWith("v1", "note-7"));
    // Rust's sentences, verbatim, not a paraphrase assembled here.
    expect(await screen.findByText(PLAN.question)).toBeInTheDocument();
    const body = await screen.findByTestId(NOTE_DELETE_TESTID);
    expect(body).toHaveTextContent(PLAN.consequence);
    // Scoped to the dialog: the editor header now shows the path too (45.18),
    // and an unscoped query would pass on the header while the confirmation
    // said nothing about which file it is about to move.
    expect(within(await screen.findByRole("alertdialog")).getByText(PLAN.path)).toBeInTheDocument();

    // **The two sentences reach a screen reader only through the dialog's
    // `aria-describedby`, and a dangling reference renders byte-identically.**
    // Every assertion above finds the text by testid, which passes just as well
    // if the relationship is broken and the consequence and recovery are
    // announced to nobody — and those two sentences are the entire reason this
    // confirmation is safe to press. So resolve the attribute to an element and
    // read ITS text. W3Recording's shape: does this thing name something, and
    // does anything check the thing it names exists?
    const dialog = await screen.findByRole("alertdialog");
    const describedBy = dialog.getAttribute("aria-describedby");
    expect(describedBy).not.toBeNull();
    const described = document.getElementById(describedBy as string);
    expect(described).not.toBeNull();
    expect(described).toHaveTextContent(PLAN.consequence);
    expect(described).toHaveTextContent(PLAN.recovery);
    // And nothing has been deleted by asking.
    expect(notesDelete).not.toHaveBeenCalled();
  });

  /**
   * Confirming deletes THAT note and closes the panel showing it.
   *
   * The panel assertion is the one a rendered-text test misses: `deleteNote`
   * is the only path that closes the target, and a dialog calling `notesDelete`
   * directly would pass every assertion about the command and leave a panel
   * pointed at a note that is gone.
   */
  it("deletes the note it named, and the panel showing it stops showing it", async () => {
    panelsStore.getState().setActiveTarget({ kind: "note", vaultId: "v1", noteId: "note-7" });
    openOn("# Standup\n");
    render(<NoteEditor vaultId="v1" noteId="note-7" />);

    const menu = await openActions(new RegExp(`^${NOTE_ACTIONS_LABEL}`));
    fireEvent.click(within(menu).getByRole("menuitem", { name: NOTE_DELETE_LABEL }));
    fireEvent.click(await screen.findByRole("button", { name: NOTE_DELETE_CONFIRM }));

    await waitFor(() => expect(notesDelete).toHaveBeenCalledWith("v1", "note-7"));
    await waitFor(() => {
      const targets = panelsStore.getState().panels.map((panel) => panel.target);
      expect(targets.some((target) => target?.kind === "note" && target.noteId === "note-7")).toBe(
        false,
      );
    });
  });
});

/**
 * Story 46.5 — the verb is findable, not merely reachable.
 *
 * Every test above passed on the morning the owner wrote "I still see no way
 * to delete notes", and they passed honestly: the menu was mounted, the item
 * was in it, and Rust got the right ids. They were blind to the defect because
 * they find the trigger by `role` and an `aria-label` no eye reads, in a jsdom
 * that performs no layout — and the defect was in exactly those two gaps. The
 * trigger was a bare `⋯` among five text buttons, and the row it ends does not
 * wrap and was wider than the 560px quick-capture window
 * (`notes_window.rs:91`), so it was off the screen as well as unlabelled.
 *
 * Neither pixel fact can be measured here. What these assert is the structure
 * the measurement follows from — a word on the trigger, and two controls in
 * the row instead of six. The pixels are gate checks in the spec, because a
 * test that claimed to measure them would be lying about jsdom.
 */
describe("finding the destructive verb", () => {
  it("puts the trigger's word in the name it answers to and on its tooltip", async () => {
    openOn("# Standup\n");
    render(<NoteEditor vaultId="v1" noteId="note-7" />);

    const trigger = await screen.findByRole("button", {
      name: new RegExp(`^${NOTE_ACTIONS_LABEL}`),
    });
    // 46.5's report was that an icon among five WORDS reads as decoration. The
    // row is icons now, so the trigger is one too (48.9) — and the word it used
    // to render is still the word it answers to. The eye reads the tooltip, a
    // screen reader reads the name, speech input says either: all three are the
    // same words (WCAG 2.5.3).
    expect(trigger).toHaveAttribute("title", NOTE_ACTIONS_TEXT);
    const spoken = trigger.getAttribute("aria-label") ?? "";
    expect(spoken.startsWith(NOTE_ACTIONS_TEXT)).toBe(true);
    // The name carries the note as well as the act, so a workspace with two
    // note panels open does not announce one control twice.
    expect(spoken).toContain("Standup");
  });

  it("leaves two controls in the header, so its last one is not the one off the screen", async () => {
    openOn("# Standup\n");
    render(<NoteEditor vaultId="v1" noteId="note-7" />);
    await screen.findByRole("button", { name: new RegExp(`^${NOTE_ACTIONS_LABEL}`) });

    // jsdom lays nothing out, so this cannot assert a width. It asserts what
    // the width followed from: six controls plus two spans in a non-wrapping
    // 560px row overflowed, and this menu is the group's LAST child, so it was
    // the first thing past the edge. The identities are asserted and not just
    // the count — a collapse that kept two of the wrong two would fit and
    // still lose the verb.
    const actions = document.querySelector<HTMLElement>(
      `[data-slot="${PANE_HEADER_ACTIONS_SLOT}"]`,
    );
    expect(actions).not.toBeNull();
    // By accessible name, not by text: both controls are glyphs since 48.9, so
    // their text content is empty and the identities this asserts live in the
    // names. `within(...).getByRole` would not preserve the row's ORDER, which
    // is the half of this that says the menu is still last.
    const labels = Array.from((actions as HTMLElement).querySelectorAll("button"), (button) =>
      button.getAttribute("aria-label"),
    );
    expect(labels).toEqual([ATTACH_FILE_LABEL, `${NOTE_ACTIONS_LABEL} Standup`]);
  });

  it("still offers every verb the header used to carry, by name, from that one menu", async () => {
    // The move is only safe if nothing was dropped on the way in — and the
    // order still has to put the destructive one last, which was 45.17's
    // contract and is the only guard left once six items share a list.
    //
    // `Show in Files` is deliberately not here: this file seeds no vaults, so
    // 45.18's predicate refuses it. `note-file-links.test.tsx` owns that one,
    // present AND absent, with the menu open in both directions.
    openOn("# Standup\n");
    render(<NoteEditor vaultId="v1" noteId="note-7" />);

    const menu = await openActions(new RegExp(`^${NOTE_ACTIONS_LABEL}`));
    const items = within(menu).getAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual([
      ATTACHMENTS_LABEL,
      PROPERTIES_LABEL,
      NOTE_HISTORY_LABEL,
      EXPORT_NOTE_LABEL,
      NOTE_DELETE_LABEL,
    ]);
  });
});

describe("NoteActions", () => {
  /** Declining calls no delete. Asserted on the command, not on the dialog. */
  it("removes nothing when the confirmation is declined", async () => {
    render(<NoteActions vaultId="v1" noteId="note-7" title="Standup" />);

    const menu = await openActions(`${NOTE_ACTIONS_LABEL} Standup`);
    fireEvent.click(within(menu).getByRole("menuitem", { name: NOTE_DELETE_LABEL }));
    fireEvent.click(await screen.findByRole("button", { name: NOTE_DELETE_CANCEL }));

    await waitFor(() => expect(screen.queryByText(PLAN.question)).not.toBeInTheDocument());
    expect(notesDelete).not.toHaveBeenCalled();
  });

  /**
   * Another story's item renders, and renders ABOVE Delete. The order is the
   * contract two stories were given: destructive last, so nothing has to reason
   * about position and the item under the cursor is never the destructive one.
   */
  it("renders another story's item above the destructive one", async () => {
    render(
      <NoteActions vaultId="v1" noteId="note-7" title="Standup">
        <DropdownMenuItem>{"Export\u2026"}</DropdownMenuItem>
      </NoteActions>,
    );

    const menu = await openActions(`${NOTE_ACTIONS_LABEL} Standup`);
    const items = within(menu).getAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual(["Export\u2026", NOTE_DELETE_LABEL]);
  });

  /**
   * A confirmation keeper cannot compose is a confirmation with no Delete
   * button. Offering one beside "keeper couldn't work out what this would
   * remove" invites a press at the one thing keeper has just declined to
   * describe.
   */
  it("offers no Delete when the plan could not be composed", async () => {
    notesDeletePlan.mockRejectedValue(new Error("no such note"));
    render(<NoteActions vaultId="v1" noteId="note-7" title="Standup" />);

    const menu = await openActions(`${NOTE_ACTIONS_LABEL} Standup`);
    fireEvent.click(within(menu).getByRole("menuitem", { name: NOTE_DELETE_LABEL }));

    expect(await screen.findByText(NOTE_DELETE_NO_PLAN)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: NOTE_DELETE_CONFIRM })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: NOTE_DELETE_CANCEL })).toBeInTheDocument();
  });

  /**
   * The delete was refused. The dialog stays and says so — `preventDefault` on
   * the action is what keeps it mounted, and without it the sentence would be
   * composed into a dialog that had already gone.
   */
  it("stays open and says why when the delete is refused", async () => {
    notesDelete.mockRejectedValue({ message: "the drive is read-only" });
    const onDeleted = vi.fn();
    render(<NoteActions vaultId="v1" noteId="note-7" title="Standup" onDeleted={onDeleted} />);

    const menu = await openActions(`${NOTE_ACTIONS_LABEL} Standup`);
    fireEvent.click(within(menu).getByRole("menuitem", { name: NOTE_DELETE_LABEL }));
    fireEvent.click(await screen.findByRole("button", { name: NOTE_DELETE_CONFIRM }));

    expect(await screen.findByRole("alert")).toHaveTextContent("the drive is read-only");
    expect(screen.getByText(PLAN.question)).toBeInTheDocument();
    expect(onDeleted).not.toHaveBeenCalled();
  });
});
