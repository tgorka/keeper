/**
 * A note knows its file, and its links go somewhere (Story 45.18, FR-196,
 * UX-DR79).
 *
 * **Over the real `NoteEditor`, its real boot effect and a real `EditorView`.**
 * Both halves of this story are claims about a surface rather than about a
 * value: whether a control is offered at all, and whether pressing rendered
 * text does anything. `NoteEditor.onFollowLink` was a prop no caller ever
 * passed — declared, wired all the way down to the decoration layer, and dead
 * since 37.6 — which is DW-172's shape exactly, and the only test that can see
 * it is one that presses the thing a person presses.
 *
 * The other host of the same decoration layer is a Files panel showing a `.md`
 * file; that one is covered in `viewers/text-file-viewer.test.tsx`, because a
 * branch reachable only from the second host cannot be reached by tests that
 * all route through the first.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteBodyBatch, NoteRefVm, NoteVaultVm } from "@/lib/ipc/client";

const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();
const notesResolveLink = vi.fn<(vaultId: string, target: string) => Promise<NoteRefVm | null>>();
const openUrl = vi.fn<(url: string) => Promise<void>>();

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: (url: string) => openUrl(url),
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
  notesResolveLink: (vaultId: string, target: string) => notesResolveLink(vaultId, target),
  recordingNoteTargets: vi.fn(async () => null),
  recordingOpenPath: vi.fn(async () => {}),
  revealPath: vi.fn(async () => {}),
  // Reached on the editor's own boot path by 44.8's template-update offer.
  // Added because the boot path reaches it, not "to be safe": a name the boot
  // path never touches, mocked anyway, is a claim about this surface that is
  // false.
  notesTemplateUpdatePreview: vi.fn(async () => null),
  // The editor hydrates the vault mirror now; without these the mirror stays
  // unread, which is the state the "no vault yet" test below asserts against.
  notesVaults: vi.fn(async () => []),
  notesVaultActive: vi.fn(async () => null),
  notesVaultSetActive: vi.fn(async () => {}),
}));

import { ATTACHMENTS_LABEL } from "@/components/notes/attachments-panel";
import { readNoteDocument, resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import { notesVaultsStore, resetNotesVaultsStoreForTest } from "@/lib/stores/notes-vaults";
import { activePanel, panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { SHOW_IN_FILES_LABEL } from "@/lib/vault-link";
import { withRangeRects } from "@/test/layout";
import { NOTE_ACTIONS_LABEL } from "./note-actions";
import { LINK_NOTICE_SLOT, NoteEditor } from "./note-editor";

/**
 * Why 20 s and not `waitFor`'s 5 s default.
 *
 * What is being waited for is the editor's lazily imported CodeMirror chunk,
 * not logic — and under eight concurrent suites that import has been measured
 * past five seconds, so the default turns a red into a measurement of the box.
 * Raised deliberately and named: a failure here should mean the press did
 * nothing, never that the machine was busy.
 */
const CHUNK_TIMEOUT_MS = 20_000;

// Applied at FILE scope rather than as a third argument per test. The
// per-test form was added by a script and it silently missed one, which then
// failed alone under load and looked like a defect in the code it was testing
// — the same class of mistake as a file sliced out of another file. A budget
// that cannot be missed is worth more than one that is precise.
vi.setConfig({ testTimeout: CHUNK_TIMEOUT_MS });

let restoreRects: (() => void) | null = null;
beforeAll(() => {
  restoreRects = withRangeRects();
});
afterAll(() => {
  restoreRects?.();
  restoreRects = null;
});

/**
 * Two vaults on two profiles, always.
 *
 * A one-vault fixture cannot tell a per-profile filter from an unconditional
 * match: `filePathForNote` would compose `v2`'s subfolder for `v1`'s note and
 * every assertion here would still pass. The second vault's subfolder is
 * deliberately different from the first's for the same reason.
 */
function seedVaults(): void {
  const vault = (id: string, profileId: string, subfolder: string): NoteVaultVm =>
    ({
      id,
      profileId,
      name: id,
      subfolder,
      root: `/Volumes/${profileId}/${subfolder}`,
      indexed: true,
      noteCount: 2,
      unreadCount: 0,
      cadence: { commitIdleMs: 1000, pushIntervalMs: 5000, pushOnBlur: true },
    }) as NoteVaultVm;
  notesVaultsStore
    .getState()
    .setVaults([vault("v1", "profile-1", "notes"), vault("v2", "profile-2", "second-brain")]);
}

/**
 * Open the editor on a note with this body at this vault-relative path.
 *
 * **Every fixture keeps its link off the last line, and that is load-bearing.**
 * The editor places the caret at the END of the body when no template hint says
 * otherwise, and `livePreview` gives the caret's own line its source back — so
 * a link on the final line renders as `[[…]]` text with no decoration and
 * nothing to press. Whether that happened used to depend on whether the opening
 * `Reset` had landed before the lazily imported editor chunk did: fast runs
 * constructed the view over an empty document (caret at 0, link decorated),
 * loaded ones over the full text (caret on the link's line, link not
 * decorated). Four of these tests passed for a hundred runs and failed together
 * the first time the box was busy — a fixture whose answer depends on a race is
 * not a fixture.
 */
function openOn(body: string, path: string): void {
  notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
    onBatch({
      kind: "reset",
      text: body,
      frontmatter: "",
      rev: "r0",
      cursor: null,
      path,
    });
    return "sub-1";
  });
}

/** The live editor, once its lazy chunk has landed and the reset has applied. */
async function editor(body: string): Promise<HTMLElement> {
  return await waitFor(() => {
    const host = document.querySelector<HTMLElement>(".cm-content");
    expect(host).not.toBeNull();
    expect(host?.textContent ?? "").toContain(body);
    return host as HTMLElement;
  });
}

/**
 * Press a rendered link, retrying until the outcome is observable.
 *
 * The retry is required rather than polite: the editor is built by an async
 * effect behind a dynamic `import()`, and the decoration set is rebuilt on
 * every document and selection change, so a node captured once is detached by
 * the next rebuild — and a detached node still accepts a `mouseDown` that
 * reaches no handler at all. Re-querying inside the retry is what makes the
 * press land on the view that is on screen.
 *
 * It does NOT need a longer budget. This helper spent a run looking like a
 * timeout and was neither: `src/test/setup.ts`'s bounding-rect shim was telling
 * CodeMirror every line was a screen tall, so the middle of the note — the line
 * with the link in it — was virtualised out of the DOM entirely and no wait
 * could ever find it. The shim now stops at the editor's edge.
 */
async function press(selector: string, outcome: () => void): Promise<void> {
  await waitFor(() => {
    const link = document.querySelector<HTMLElement>(selector);
    expect(link, `nothing matched ${selector}`).not.toBeNull();
    fireEvent.mouseDown(link as HTMLElement);
    outcome();
  });
}

/** The sentence the editor leaves behind when a link went nowhere. */
function noticeText(): string | null {
  return document.querySelector(`[data-slot="${LINK_NOTICE_SLOT}"]`)?.textContent ?? null;
}

/**
 * Open the note's Actions menu and hand back its content.
 *
 * Story 46.5 moved `Show in Files` in here, which changes what an absence
 * means: an item missing from a menu nobody opened is missing for a reason
 * that has nothing to do with `filePathForNote`. Every read of this control —
 * present or absent — goes through this helper, so the three absence tests
 * below are asserting the predicate and not Radix's mounting.
 *
 * `pointerDown`/`pointerUp` rather than `click`: that is the pair Radix's
 * trigger listens for, and the same two lines `note-actions.test.tsx:107` and
 * `export-controls.test.tsx:149` press.
 */
async function openNoteActions(): Promise<HTMLElement> {
  const trigger = await screen.findByRole("button", {
    name: new RegExp(`^${NOTE_ACTIONS_LABEL}`),
  });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.pointerUp(trigger, { button: 0 });
  return await screen.findByRole("menu");
}

/**
 * Press `Show in Files` from the menu.
 *
 * `getByRole("menuitem", …)` and not a bare name query: `SHOW_IN_FILES_LABEL`
 * is a word that could plausibly name a region as well as a verb, and a name
 * query that resolved to the wrong role would fail as "item missing" when the
 * item was fine.
 */
async function pressShowInFiles(): Promise<void> {
  const menu = await openNoteActions();
  fireEvent.click(within(menu).getByRole("menuitem", { name: SHOW_IN_FILES_LABEL }));
}

/**
 * Assert `Show in Files` is not offered, from an OPEN menu — and prove the menu
 * is open by finding a sibling that is unconditional. Without the sibling this
 * reads `null` whether the predicate refused or the menu never mounted, which
 * is an assertion that cannot fail.
 */
async function showInFilesIsNotOffered(): Promise<void> {
  const menu = await openNoteActions();
  // Attachments is the unconditional sibling this proves the menu by. It used to
  // be Properties, until Properties became a leading control that never enters
  // this menu — a witness that is sometimes absent proves nothing.
  expect(
    within(menu).getByRole("menuitemcheckbox", { name: ATTACHMENTS_LABEL }),
  ).toBeInTheDocument();
  expect(within(menu).queryByRole("menuitem", { name: SHOW_IN_FILES_LABEL })).toBeNull();
}

beforeEach(() => {
  vi.clearAllMocks();
  resetPanelsStoreForTest();
  resetNotesVaultsStoreForTest();
  primaryViewStore.getState().setView("notes");
});

afterEach(() => {
  resetNotesEditorStoreForTest();
});

describe("a note knows its file", () => {
  it("opens the note's own file in the Files pane, beside the note", async () => {
    seedVaults();
    openOn("# Meeting\n", "inbox/meeting.md");
    render(<NoteEditor vaultId="v1" noteId="n1" />);

    await pressShowInFiles();
    await pressShowInFiles();

    // The VALUE, not merely that something opened. The profile is the vault's,
    // and the path is the vault's subfolder joined with the note's own path —
    // a composition that has exactly one correct answer and several plausible
    // wrong ones: the note path alone, the other vault's subfolder, or the
    // absolute root that FR-145 forbids ever reaching a surface.
    const targets = panelsStore.getState().panels.map((panel) => panel.target);
    expect(targets).toContainEqual({
      kind: "file",
      profileId: "profile-1",
      relativePath: "notes/inbox/meeting.md",
    });
    expect(primaryViewStore.getState().view).toBe("files");
  });

  it("keeps the note open rather than replacing it with its own file", async () => {
    // `setActiveTarget` would replace the active panel, and the active panel is
    // the note — so pressing this would close the thing you pressed it from,
    // and going back to Notes would find nothing open. `openPanel` is the
    // open-beside gesture and this is what makes that a tested decision.
    seedVaults();
    openOn("# Meeting\n", "inbox/meeting.md");
    panelsStore.getState().setActiveTarget({ kind: "note", vaultId: "v1", noteId: "n1" });
    render(<NoteEditor vaultId="v1" noteId="n1" />);

    await pressShowInFiles();

    const targets = panelsStore.getState().panels.map((panel) => panel.target);
    expect(targets).toContainEqual({ kind: "note", vaultId: "v1", noteId: "n1" });
    expect(targets).toContainEqual({
      kind: "file",
      profileId: "profile-1",
      relativePath: "notes/inbox/meeting.md",
    });
    // And focus followed the thing that was just opened.
    expect(activePanel(panelsStore.getState()).target).toEqual({
      kind: "file",
      profileId: "profile-1",
      relativePath: "notes/inbox/meeting.md",
    });
  });

  it("offers nothing while the vault list has not been read", async () => {
    // `null` is "keeper has not looked", never "you have none". Offering the
    // control here would compose a path against a vault nobody confirmed.
    openOn("# Meeting\n", "inbox/meeting.md");
    render(<NoteEditor vaultId="v1" noteId="n1" />);
    await showInFilesIsNotOffered();
  });

  it("offers nothing for a note whose profile carries no vault subfolder", async () => {
    // What `notes_ipc.rs` projects for an unflagged folder. There is no vault
    // directory to compose against, so there is no file to show — absent, not
    // an action that resolves to the profile root.
    notesVaultsStore.getState().setVaults([
      {
        id: "v1",
        profileId: "profile-1",
        name: "v1",
        subfolder: "",
        root: "/Volumes/profile-1",
        indexed: true,
        noteCount: 0,
        unreadCount: 0,
        cadence: { commitIdleMs: 1000, pushIntervalMs: 5000, pushOnBlur: true },
      } as NoteVaultVm,
    ]);
    openOn("# Meeting\n", "inbox/meeting.md");
    render(<NoteEditor vaultId="v1" noteId="n1" />);
    await showInFilesIsNotOffered();
  });

  it("offers nothing before the note's own path has arrived", async () => {
    // The frame between mounting and the channel's opening `Reset`. It is a
    // real frame — the subscription is a round trip — and Story 45.18 is the
    // reason it is now only ONE frame: before it, `Reset` carried no path at
    // all and this state persisted until the first autosave, which would have
    // made this control absent for every note anyone actually opened.
    seedVaults();
    notesOpen.mockImplementation(async () => "sub-1");
    render(<NoteEditor vaultId="v1" noteId="n1" />);
    await showInFilesIsNotOffered();
    expect(readNoteDocument("v1", "n1").path).toBeNull();
  });
});

describe("a wikilink goes to its note", () => {
  it("resolves through the index's own resolver and opens what it names", async () => {
    seedVaults();
    notesResolveLink.mockResolvedValue({
      vaultId: "v1",
      id: "note-7",
      path: "meeting.md",
      title: "Meeting",
    });
    const opened: string[] = [];
    // Line 2: the caret sits at offset 0 and live preview gives the caret's own
    // line its source back, so a wikilink on line 1 renders as `[[…]]` text
    // with no decoration and no attribute to press.
    openOn(
      "# Index\n\nsee [[Meeting]] for the rest\n\n\nand a last line the caret can sit on, so the link above is\nnever the caret's own line.\n",
      "index.md",
    );
    render(<NoteEditor vaultId="v1" noteId="n1" onOpenNote={(id) => opened.push(id)} />);
    await editor("see");

    await press("[data-keeper-wikilink]", () => {
      // The CALL: this vault, and the link's raw text. A resolver handed the
      // wrong vault would answer for somebody else's notes and the rendering
      // would be identical.
      expect(notesResolveLink).toHaveBeenCalledWith("v1", "Meeting");
      expect(opened).toContain("note-7");
    });
    expect(noticeText()).toBeNull();
  });

  it("says so when nothing in the vault answers to the link", async () => {
    seedVaults();
    notesResolveLink.mockResolvedValue(null);
    const opened: string[] = [];
    openOn(
      "# Index\n\nsee [[Nowhere]] for the rest\n\n\nand a last line the caret can sit on, so the link above is\nnever the caret's own line.\n",
      "index.md",
    );
    render(<NoteEditor vaultId="v1" noteId="n1" onOpenNote={(id) => opened.push(id)} />);
    await editor("see");

    await press("[data-keeper-wikilink]", () => {
      expect(noticeText()).toContain("Nowhere");
    });
    // Nothing was opened, and nothing was invented in its place.
    expect(opened).toEqual([]);
  });
});

describe("an external link goes to the application that owns it", () => {
  it("hands a web link to the OS opener, with the destination it was written with", async () => {
    seedVaults();
    openUrl.mockResolvedValue(undefined);
    openOn(
      "# Index\n\nsee [the docs](https://example.org/a%20b?q=1) today\n\n\nand a last line the caret can sit on, so the link above is\nnever the caret's own line.\n",
      "index.md",
    );
    render(<NoteEditor vaultId="v1" noteId="n1" />);
    await editor("the docs");

    await press("[data-keeper-link]", () => {
      // Verbatim: a destination the surface shortened, escaped again or read
      // out of the visible `title` would open a different page.
      expect(openUrl).toHaveBeenCalledWith("https://example.org/a%20b?q=1");
    });
    expect(noticeText()).toBeNull();
  });

  it("refuses a scheme the opener grant does not carry, and names it", async () => {
    // A note is agent-writable, so a `javascript:` destination is a thing that
    // can genuinely arrive. Tauri's own scope refuses it; this refusal exists
    // so the reader is told which scheme rather than shown the plugin's
    // sentence about a scope they have never seen.
    seedVaults();
    openOn(
      "# Index\n\nsee [click](javascript:alert(1)) today\n\n\nand a last line the caret can sit on, so the link above is\nnever the caret's own line.\n",
      "index.md",
    );
    render(<NoteEditor vaultId="v1" noteId="n1" />);
    await editor("click");

    await press("[data-keeper-link]", () => {
      expect(noticeText()).toContain("javascript:");
    });
    expect(openUrl).not.toHaveBeenCalled();
  });

  it("says so when the OS refuses, rather than swallowing the rejection", async () => {
    // `capabilities/quick-capture.json` grants no opener, so the identical
    // press in a capture window is refused by Tauri. A silent failure there
    // would be this story's own defect one window along.
    seedVaults();
    // `mockImplementation` and not `mockRejectedValue`: the latter builds the
    // rejected promise when the mock is CONFIGURED, so a retry loop that never
    // reaches the call leaves an unhandled rejection behind — a green run with
    // an error in it.
    openUrl.mockImplementation(async () => {
      throw new Error("url not allowed on the configured scope");
    });
    openOn(
      "# Index\n\nsee [the docs](https://example.org/) today\n\n\nand a last line the caret can sit on, so the link above is\nnever the caret's own line.\n",
      "index.md",
    );
    render(<NoteEditor vaultId="v1" noteId="n1" />);
    await editor("the docs");

    await press("[data-keeper-link]", () => {
      expect(noticeText()).toContain("not allowed on the configured scope");
    });
  });
});
