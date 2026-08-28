import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionEntryVm } from "@/lib/ipc/client";

const syncOpenEntry = vi.fn();
const revealPath = vi.fn();
const sessionsFileDelete = vi.fn();
// Story 52.8 pulled `SpaceRowMenu` onto every row, so this suite now loads its
// three commands too. Declared rather than left to the proxy: a verb that reads
// an undefined key off a mocked module fails with vitest's own message instead
// of the assertion that would say which wire is loose.
const sessionsFilePath = vi.fn();
const sessionsFileRename = vi.fn();
const syncReadFrontmatter = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  syncOpenEntry: (id: unknown, subpath: unknown) => syncOpenEntry(id, subpath),
  revealPath: (path: unknown) => revealPath(path),
  sessionsFileDelete: (root: unknown, session: unknown, rel: unknown) =>
    sessionsFileDelete(root, session, rel),
  sessionsFilePath: (root: unknown, subpath: unknown) => sessionsFilePath(root, subpath),
  sessionsFileRename: (root: unknown, subpath: unknown, block: unknown, next: unknown) =>
    sessionsFileRename(root, subpath, block, next),
  syncReadFrontmatter: (root: unknown, subpath: unknown) => syncReadFrontmatter(root, subpath),
}));

import { FILES_SYNC_MARK_TESTID } from "@/components/layout/sync-status-mark";
import {
  initialOpenFolders,
  SESSION_TREE_DELETE_FAILED,
  SESSION_TREE_DELETE_LABEL,
  SESSION_TREE_EMPTY,
  SESSION_TREE_LABEL,
  SESSION_TREE_OPEN_EXTERNAL_LABEL,
  SESSION_TREE_REVEAL_LABEL,
  SESSION_TREE_TRUNCATED,
  SessionTree,
} from "@/components/sessions/session-tree";
import {
  SPACE_ROW_COPY_PATH_LABEL,
  SPACE_ROW_DELETE_LABEL,
  SPACE_ROW_OPEN_BESIDE_LABEL,
  SPACE_ROW_OPEN_HERE_LABEL,
  SPACE_ROW_OPEN_LABEL,
  SPACE_ROW_RENAME_KEEPS_NAME,
  SPACE_ROW_RENAME_LABEL,
  SPACE_ROW_REVEAL_LABEL,
} from "@/components/sessions/space-row-menu";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

const NOW = Date.now();

/** The fence's own words, as Rust composes them (AD-113). */
const LOCK_SENTENCE =
  "60-sessions/active/2026-08-10-keeper/workspace is inside a session's workspace — scratch that is not versioned, not synced, and dies with the session. keeper reads it but never writes there; promote the file into the session's artifacts instead.";

/**
 * `check_deletable`'s sentence for the two files that decide the shape, as the
 * tree receives it (FR-262). Quoted rather than re-derived, because quoting is
 * what the component does too — the row's job is to say Rust's answer, and a
 * fixture that invented its own would be testing a rule that does not exist.
 */
const UNDELETABLE_SENTENCE =
  "about.md is what tells keeper this session is a flat one: deleting it would silently turn the session back into the old folder shape.";

/** Every directory, always — a folder delete is a verb this tree does not offer. */
const DIR_UNDELETABLE =
  "keeper deletes one file at a time. Removing a folder takes everything inside it with it, which is a bigger promise than this tree makes — do it in Finder.";

function entry(over: Partial<SessionEntryVm> & Pick<SessionEntryVm, "name">): SessionEntryVm {
  const relPath = over.relPath ?? over.name;
  return {
    relPath,
    parent: "",
    depth: 1,
    isDir: false,
    subpath: `60-sessions/active/2026-08-10-keeper/${relPath}`,
    absolutePath: `/Users/tgorka/tgdrive/60-sessions/active/2026-08-10-keeper/${relPath}`,
    size: { bytes: 2048, label: "2.0 kB" },
    mtimeMs: NOW - 60_000,
    sync: { status: "synced", detail: null },
    locked: null,
    undeletable: over.isDir === true ? DIR_UNDELETABLE : null,
    ...over,
  };
}

/**
 * The zone's own shape: four sections, a file under two of them, and one file
 * two levels down — the case a flat list could not show at all.
 */
function zone(): SessionEntryVm[] {
  return [
    entry({ name: "artifacts", isDir: true, size: null }),
    entry({
      name: "release-notes.md",
      relPath: "artifacts/release-notes.md",
      parent: "artifacts",
      depth: 2,
    }),
    entry({
      name: "shots",
      relPath: "artifacts/shots",
      parent: "artifacts",
      depth: 2,
      isDir: true,
      size: null,
    }),
    entry({
      name: "board.png",
      relPath: "artifacts/shots/board.png",
      parent: "artifacts/shots",
      depth: 3,
    }),
    entry({ name: "workspace", isDir: true, size: null, locked: LOCK_SENTENCE }),
    entry({
      name: "iter-3.md",
      relPath: "workspace/iter-3.md",
      parent: "workspace",
      depth: 2,
      locked: LOCK_SENTENCE,
      // Rust says both things about a scratch file: the fence locks it and
      // `check_deletable` refuses it for the same reason. The row picks one.
      undeletable: LOCK_SENTENCE,
      sync: { status: "excluded", detail: "workspace/ is excluded by the zone's own pattern." },
    }),
    // The file whose refusal is the surprising one: it looks like any other
    // note in the pool, and deleting it silently changes what the session IS.
    entry({ name: "about.md", undeletable: UNDELETABLE_SENTENCE }),
    entry({ name: "README.md" }),
  ];
}

/**
 * The case the scratch exception exists for: a folder inside `workspace/`,
 * which is what a package manager leaves behind.
 */
function deepScratch(): SessionEntryVm[] {
  return [
    entry({ name: "workspace", isDir: true, size: null, locked: LOCK_SENTENCE }),
    entry({
      name: "node_modules",
      relPath: "workspace/node_modules",
      parent: "workspace",
      depth: 2,
      isDir: true,
      size: null,
      locked: LOCK_SENTENCE,
    }),
  ];
}

function mount(over: Partial<React.ComponentProps<typeof SessionTree>> = {}) {
  const onOpen = vi.fn();
  const onChanged = vi.fn();
  const result = render(
    <SessionTree
      rootId="tgdrive"
      sessionId="active/2026-08-10-keeper"
      entries={zone()}
      truncated={false}
      onOpen={onOpen}
      onChanged={onChanged}
      {...over}
    />,
  );
  return { ...result, onOpen, onChanged };
}

/**
 * Right-click a row, which is the gesture story 52.8 exists for.
 *
 * Returns the menu rather than asserting inside it, because Radix's menu is
 * modal: with one open the tree behind it is `aria-hidden`, so every row a case
 * needs must be held before this is called.
 */
async function openMenu(row: HTMLElement): Promise<HTMLElement> {
  fireEvent.contextMenu(row);
  return await screen.findByRole("menu");
}

/**
 * What the menu offers, by accessible name and in order.
 *
 * `aria-label` first because two items carry a sentence beside their label — a
 * refusal, or what a rename of the record does instead — and the label is the
 * name while the sentence is the description (`attach-file-button.tsx:211-218`).
 * Reading `textContent` alone would compare against "Deletekeeper deletes one
 * file at a time…", which is the string nobody can say.
 */
function verbs(menu: HTMLElement): string[] {
  return within(menu)
    .getAllByRole("menuitem")
    .map((item) => item.getAttribute("aria-label") ?? item.textContent ?? "");
}

beforeEach(() => {
  syncOpenEntry.mockResolvedValue(undefined);
  revealPath.mockResolvedValue(undefined);
  sessionsFileDelete.mockResolvedValue(null);
  sessionsFilePath.mockResolvedValue(
    "/Users/tgorka/tgdrive/60-sessions/active/2026-08-10-keeper/artifacts/release-notes.md",
  );
  syncReadFrontmatter.mockResolvedValue("---\ntitle: notes\n---\n");
  sessionsFileRename.mockResolvedValue(
    "60-sessions/active/2026-08-10-keeper/artifacts/release-notes.md",
  );
  capabilitiesStore.setState({
    capabilities: { ...DEFAULT_CAPABILITIES, revealInFileManager: true },
    hydrated: true,
  });
});

afterEach(() => {
  vi.clearAllMocks();
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

describe("SessionTree", () => {
  it("opens every folder on arrival, however deep", () => {
    mount();
    const tree = screen.getByRole("tree", { name: SESSION_TREE_LABEL });
    // A section's children render...
    expect(within(tree).getByRole("treeitem", { name: "release-notes.md" })).toBeInTheDocument();
    // ...and so does what a folder INSIDE a section holds. This is the whole
    // change: the operator asked for the structure preloaded, and in a flat
    // session `artifacts/` is where the only real nesting is left.
    expect(within(tree).getByRole("treeitem", { name: "shots" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(within(tree).getByRole("treeitem", { name: "board.png" })).toBeInTheDocument();
  });

  it("opens workspace/ itself but not the depth below it", () => {
    mount();
    const tree = screen.getByRole("tree", { name: SESSION_TREE_LABEL });
    // Scratch is the one directory with no contract about its size — an agent
    // pointing a package manager at it is the case the truncation notice names.
    // So its own row opens (its contents are never hidden) and the subtree
    // below stays closed, which is the one-level rule surviving exactly where
    // it was earning its keep.
    expect(within(tree).getByRole("treeitem", { name: "workspace" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(within(tree).getByRole("treeitem", { name: "iter-3.md" })).toBeInTheDocument();
    expect(initialOpenFolders(zone()).has("workspace")).toBe(true);
    expect(initialOpenFolders(deepScratch()).has("workspace/node_modules")).toBe(false);
  });

  it("renders nesting as aria-level, one level per folder", () => {
    mount();
    expect(screen.getByRole("treeitem", { name: "artifacts" })).toHaveAttribute("aria-level", "1");
    expect(screen.getByRole("treeitem", { name: "release-notes.md" })).toHaveAttribute(
      "aria-level",
      "2",
    );
  });

  it("opens a closed folder and reveals what is under it", () => {
    mount();
    screen.getByRole("treeitem", { name: "shots" }).focus();
    fireEvent.keyDown(document.activeElement as Element, { key: "ArrowRight" });
    expect(screen.getByRole("treeitem", { name: "shots" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(screen.getByRole("treeitem", { name: "board.png" })).toHaveAttribute("aria-level", "3");
  });

  it("closes a folder and hides its whole subtree, however deep", () => {
    mount();
    // Open `shots` first, so there is a grandchild to hide.
    screen.getByRole("treeitem", { name: "shots" }).focus();
    fireEvent.keyDown(document.activeElement as Element, { key: "ArrowRight" });
    expect(screen.getByRole("treeitem", { name: "board.png" })).toBeInTheDocument();
    // Now close the SECTION — the grandchild goes with it, not just the child.
    screen.getByRole("treeitem", { name: "artifacts" }).focus();
    fireEvent.keyDown(document.activeElement as Element, { key: "ArrowLeft" });
    expect(screen.queryByRole("treeitem", { name: "shots" })).not.toBeInTheDocument();
    expect(screen.queryByRole("treeitem", { name: "board.png" })).not.toBeInTheDocument();
  });

  it("walks with the arrows and keeps exactly one row in the tab order", () => {
    mount();
    const first = screen.getByRole("treeitem", { name: "artifacts" });
    expect(first).toHaveAttribute("tabindex", "0");
    first.focus();
    fireEvent.keyDown(document.activeElement as Element, { key: "ArrowDown" });
    expect(document.activeElement).toBe(screen.getByRole("treeitem", { name: "release-notes.md" }));
    expect(first).toHaveAttribute("tabindex", "-1");
    fireEvent.keyDown(document.activeElement as Element, { key: "End" });
    expect(document.activeElement).toBe(screen.getByRole("treeitem", { name: "README.md" }));
    fireEvent.keyDown(document.activeElement as Element, { key: "Home" });
    expect(document.activeElement).toBe(first);
  });

  it("opens a file on Enter and toggles a folder on Enter", () => {
    const { onOpen } = mount();
    const file = screen.getByRole("treeitem", { name: "release-notes.md" });
    file.focus();
    fireEvent.keyDown(file, { key: "Enter" });
    expect(onOpen).toHaveBeenCalledWith(
      expect.objectContaining({ relPath: "artifacts/release-notes.md" }),
    );
    const folder = screen.getByRole("treeitem", { name: "artifacts" });
    folder.focus();
    fireEvent.keyDown(folder, { key: "Enter" });
    expect(folder).toHaveAttribute("aria-expanded", "false");
  });

  it("says why a locked row is locked, in the fence's own words", () => {
    mount();
    const locked = screen.getByRole("treeitem", { name: "iter-3.md" });
    expect(locked).toHaveAccessibleDescription(expect.stringContaining("never writes there"));
    // The unlocked row says nothing about writing — a lock everywhere is a
    // lock nowhere.
    expect(
      screen.getByRole("treeitem", { name: "release-notes.md" }),
    ).not.toHaveAccessibleDescription(expect.stringContaining("never writes there"));
  });

  it("carries the Files tab's own sync mark and sentence", () => {
    mount();
    const locked = screen.getByRole("treeitem", { name: "iter-3.md" });
    const mark = within(locked).getByTestId(FILES_SYNC_MARK_TESTID);
    expect(mark).toHaveAttribute("data-sync-status", "excluded");
    expect(mark).toHaveAccessibleName("workspace/ is excluded by the zone's own pattern.");
  });

  it("describes a row by its size and age without putting either in its name", () => {
    mount();
    const row = screen.getByRole("treeitem", { name: "release-notes.md" });
    expect(row).toHaveAccessibleName("release-notes.md");
    expect(row).toHaveAccessibleDescription(expect.stringContaining("2.0 kB"));
  });

  it("opens externally and reveals through the profile-relative and absolute paths", async () => {
    mount();
    const row = screen.getByRole("treeitem", { name: "release-notes.md" });
    within(row).getByLabelText(SESSION_TREE_OPEN_EXTERNAL_LABEL).click();
    await waitFor(() => {
      expect(syncOpenEntry).toHaveBeenCalledWith(
        "tgdrive",
        "60-sessions/active/2026-08-10-keeper/artifacts/release-notes.md",
      );
    });
    within(row).getByLabelText(SESSION_TREE_REVEAL_LABEL).click();
    await waitFor(() => {
      expect(revealPath).toHaveBeenCalledWith(
        "/Users/tgorka/tgdrive/60-sessions/active/2026-08-10-keeper/artifacts/release-notes.md",
      );
    });
  });

  it("offers no reveal where the platform has none", () => {
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: true });
    mount();
    const row = screen.getByRole("treeitem", { name: "release-notes.md" });
    expect(within(row).queryByLabelText(SESSION_TREE_REVEAL_LABEL)).not.toBeInTheDocument();
    // The other verb is not platform-conditional.
    expect(within(row).getByLabelText(SESSION_TREE_OPEN_EXTERNAL_LABEL)).toBeInTheDocument();
  });

  it("says a truncated walk was truncated, and why", () => {
    mount({ truncated: true });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_TREE_TRUNCATED);
  });

  it("says an empty session is empty rather than rendering nothing", () => {
    mount({ entries: [] });
    expect(screen.getByText(SESSION_TREE_EMPTY)).toBeInTheDocument();
    expect(screen.queryByRole("tree")).not.toBeInTheDocument();
  });

  it("deletes a file only after the confirmation names it", async () => {
    const { onChanged } = mount();
    const row = screen.getByRole("treeitem", { name: "README.md" });
    within(row).getByLabelText(`${SESSION_TREE_DELETE_LABEL} README.md`).click();
    // Nothing has been asked of Rust yet — the dialog is the whole point.
    expect(sessionsFileDelete).not.toHaveBeenCalled();
    const dialog = await screen.findByRole("alertdialog");
    // Which file, in a tree of forty rows.
    expect(dialog).toHaveTextContent("README.md");
    within(dialog).getByRole("button", { name: SESSION_TREE_DELETE_LABEL }).click();
    await waitFor(() => {
      expect(sessionsFileDelete).toHaveBeenCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "README.md",
      );
    });
    // The surface re-reads rather than trusting its own idea of the pool.
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("asks and then does nothing when the answer is Cancel", async () => {
    mount();
    const row = screen.getByRole("treeitem", { name: "README.md" });
    within(row).getByLabelText(`${SESSION_TREE_DELETE_LABEL} README.md`).click();
    const dialog = await screen.findByRole("alertdialog");
    within(dialog).getByRole("button", { name: "Cancel" }).click();
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(sessionsFileDelete).not.toHaveBeenCalled();
  });

  it("disables Delete on a file Rust refuses, and says the refusal", () => {
    mount();
    const row = screen.getByRole("treeitem", { name: "about.md" });
    // Labelled by the refusal itself, so the reason is what a screen reader
    // reads and what the tooltip shows — a disabled button with no sentence
    // teaches nothing about why this one file is different.
    const button = within(row).getByLabelText(UNDELETABLE_SENTENCE);
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("title", UNDELETABLE_SENTENCE);
  });

  it("offers no Delete at all on a scratch row, which already says why", () => {
    mount();
    const row = screen.getByRole("treeitem", { name: "iter-3.md" });
    expect(within(row).queryByLabelText(LOCK_SENTENCE)).not.toBeInTheDocument();
    expect(
      within(row).queryByLabelText(`${SESSION_TREE_DELETE_LABEL} iter-3.md`),
    ).not.toBeInTheDocument();
    // The lock's own sentence is still on the row — this is UX-DR43, not silence.
    expect(row).toHaveAccessibleDescription(expect.stringContaining("never writes there"));
  });

  it("offers no Delete on a folder, whatever it holds", () => {
    mount();
    const row = screen.getByRole("treeitem", { name: "artifacts" });
    const button = within(row).queryByLabelText(DIR_UNDELETABLE);
    expect(button === null || button.hasAttribute("disabled")).toBe(true);
    expect(
      within(row).queryByLabelText(`${SESSION_TREE_DELETE_LABEL} artifacts`),
    ).not.toBeInTheDocument();
  });

  it("says keeper's own refusal when the delete is refused, and changes nothing", async () => {
    const { onChanged } = mount();
    sessionsFileDelete.mockRejectedValue({ message: "That file is outside the session." });
    const row = screen.getByRole("treeitem", { name: "README.md" });
    within(row).getByLabelText(`${SESSION_TREE_DELETE_LABEL} README.md`).click();
    const dialog = await screen.findByRole("alertdialog");
    within(dialog).getByRole("button", { name: SESSION_TREE_DELETE_LABEL }).click();
    // Rust's sentence, not the fallback — the fallback exists for a refusal
    // that arrived without one.
    expect(await screen.findByRole("status")).toHaveTextContent(
      "That file is outside the session.",
    );
    expect(screen.queryByText(SESSION_TREE_DELETE_FAILED)).not.toBeInTheDocument();
    expect(onChanged).not.toHaveBeenCalled();
  });
});

/**
 * Story 52.8, FR-312 — *"prawy klawisz myszy wciaz nie dziala w session files"*.
 *
 * Story 51.6 built one `ContextMenu` for this app's session surface and scoped
 * itself to a SPACE row; the rows the owner was right-clicking are these. Every
 * case below opens the menu on a row **`SessionTree` rendered**, which is the
 * whole point of the suite: `space-row-menu.test.tsx` mounts the component around
 * a synthetic `<button>` and proves the component works, which says nothing about
 * which rows have it.
 */
describe("SessionTree — the row's context menu", () => {
  /** Row 1. The space row's seven verbs, on a file in the tree. */
  it("answers a right-click on a file row with the seven verbs a space row has", async () => {
    mount();
    const row = screen.getByRole("treeitem", { name: "release-notes.md" });

    expect(verbs(await openMenu(row))).toEqual([
      SPACE_ROW_OPEN_HERE_LABEL,
      SPACE_ROW_OPEN_BESIDE_LABEL,
      SPACE_ROW_OPEN_LABEL,
      SPACE_ROW_REVEAL_LABEL,
      SPACE_ROW_COPY_PATH_LABEL,
      SPACE_ROW_RENAME_LABEL,
      SPACE_ROW_DELETE_LABEL,
    ]);
  });

  /**
   * Row 2. A folder is not a panel target and has no title to write, so the three
   * verbs that address a file and the rename are gone — `files-pane.tsx`'s own
   * answer for this row (`files-pane.test.tsx:1938-1942`), plus the Delete this
   * tree's rows carry. Delete is the one it keeps and cannot use: Rust refuses
   * every directory, so it is there with the refusal rather than absent.
   */
  it("answers a right-click on a folder row with the verbs a folder actually has", async () => {
    mount();
    const row = screen.getByRole("treeitem", { name: "artifacts" });

    const menu = await openMenu(row);
    expect(verbs(menu)).toEqual([
      SPACE_ROW_REVEAL_LABEL,
      SPACE_ROW_COPY_PATH_LABEL,
      SPACE_ROW_DELETE_LABEL,
    ]);
    const remove = within(menu).getByRole("menuitem", { name: SPACE_ROW_DELETE_LABEL });
    expect(remove.closest("[data-disabled]")).not.toBeNull();
    expect(remove).toHaveAccessibleDescription(DIR_UNDELETABLE);
  });

  /**
   * Row 3. The menu never offers a verb that would fail afterwards, and the
   * reason is Rust's sentence rather than a re-wording: `undeletable` is
   * `check_deletable`'s own refusal and is `Some` for everything under
   * `workspace/`, which is what the fence says about this file.
   */
  it("disables Delete on a locked row and describes it in the fence's own words", async () => {
    mount();
    const row = screen.getByRole("treeitem", { name: "iter-3.md" });

    const remove = within(await openMenu(row)).getByRole("menuitem", {
      name: SPACE_ROW_DELETE_LABEL,
    });
    expect(remove.closest("[data-disabled]")).not.toBeNull();
    expect(remove).toHaveAccessibleDescription(expect.stringContaining("never writes there"));
  });

  /**
   * Row 4. `files::renames` answers *false* for the record, and what that means
   * is that the title is written and the file does not move (`files.rs:405-416`)
   * — so the item stays live and says what it will do. This tree labels its rows
   * with the FILENAME, which is exactly why the sentence is needed here: without
   * it the verb's whole effect is off screen.
   */
  it("says the record keeps its filename, rather than looking like it did nothing", async () => {
    mount();
    const record = screen.getByRole("treeitem", { name: "README.md" });

    const rename = within(await openMenu(record)).getByRole("menuitem", {
      name: SPACE_ROW_RENAME_LABEL,
    });
    expect(rename).toHaveAccessibleDescription(SPACE_ROW_RENAME_KEEPS_NAME);

    // And the confirmation says it too, because the body it would otherwise show
    // promises a file that follows its title — the one thing this rename does not
    // do.
    fireEvent.click(rename);
    expect(await screen.findByRole("alertdialog")).toHaveTextContent(SPACE_ROW_RENAME_KEEPS_NAME);
  });

  /** And an ordinary pool file says nothing of the kind — the row's name follows. */
  it("promises nothing of the sort on a file whose name does follow its title", async () => {
    mount();
    const row = screen.getByRole("treeitem", { name: "release-notes.md" });

    expect(
      within(await openMenu(row)).getByRole("menuitem", { name: SPACE_ROW_RENAME_LABEL }),
    ).toHaveAccessibleDescription("");
  });

  /**
   * A rename writes a frontmatter `title:`, so it is a verb only markdown has —
   * and the registry is what says which files those are. The row is still a panel
   * target, which is the difference between this case and the folder above.
   */
  it("offers no Rename on a file with no frontmatter to write", async () => {
    mount();
    const row = screen.getByRole("treeitem", { name: "board.png" });

    const menu = await openMenu(row);
    expect(within(menu).queryByRole("menuitem", { name: SPACE_ROW_RENAME_LABEL })).toBeNull();
    expect(
      within(menu).getByRole("menuitem", { name: SPACE_ROW_OPEN_HERE_LABEL }),
    ).toBeInTheDocument();
  });

  /**
   * Row 5. Five of these verbs have no other route from this row — the three
   * hover buttons are open-externally, reveal and delete — so a menu only a mouse
   * can open would be five verbs behind a pointer. The keystroke lives in the
   * component; this asserts the tree's row is where it arrives.
   */
  it("opens the menu on the focused row from Shift+F10", async () => {
    mount();
    const row = screen.getByRole("treeitem", { name: "release-notes.md" });
    row.focus();

    fireEvent.keyDown(row, { key: "F10", shiftKey: true });

    expect(
      within(await screen.findByRole("menu")).getByRole("menuitem", {
        name: SPACE_ROW_OPEN_HERE_LABEL,
      }),
    ).toBeInTheDocument();
  });

  /**
   * The event collision, asserted rather than reasoned about. The row now has two
   * `onKeyDown` handlers — the tree's own and the trigger's summon key — and Radix
   * composes them child-first, so the tree keeps every key it had and the menu
   * takes only the two it answers to.
   */
  it("keeps the tree's arrows and Enter while the row is also a menu trigger", () => {
    const { onOpen } = mount();
    screen.getByRole("treeitem", { name: "artifacts" }).focus();

    fireEvent.keyDown(document.activeElement as Element, { key: "ArrowDown" });
    expect(document.activeElement).toBe(screen.getByRole("treeitem", { name: "release-notes.md" }));
    fireEvent.keyDown(document.activeElement as Element, { key: "Enter" });

    expect(onOpen).toHaveBeenCalledWith(
      expect.objectContaining({ relPath: "artifacts/release-notes.md" }),
    );
    // Enter is the row's, not the menu's: a tree whose Enter opened a menu would
    // have lost the gesture that opens the file.
    expect(screen.queryByRole("menu")).toBeNull();
  });

  /**
   * Row 6. Through the tree's own `onOpen` — the surface already has exactly one
   * function that opens a session file, and a `setActiveTarget` in the menu would
   * be a second implementation of the row's click.
   */
  it("opens the file the row names in this panel", async () => {
    const { onOpen } = mount();
    const row = screen.getByRole("treeitem", { name: "release-notes.md" });

    const menu = await openMenu(row);
    fireEvent.click(within(menu).getByRole("menuitem", { name: SPACE_ROW_OPEN_HERE_LABEL }));

    await waitFor(() =>
      expect(onOpen).toHaveBeenCalledWith(
        expect.objectContaining({
          relPath: "artifacts/release-notes.md",
          subpath: "60-sessions/active/2026-08-10-keeper/artifacts/release-notes.md",
        }),
      ),
    );
  });

  /**
   * The delete verb, end to end from the menu, because the session id is the one
   * argument this tree supplies and a menu wired with the wrong one would refuse
   * every delete at the far end.
   */
  it("deletes through the menu with the session the tree was given", async () => {
    const { onChanged } = mount();
    const row = screen.getByRole("treeitem", { name: "README.md" });

    const menu = await openMenu(row);
    fireEvent.click(within(menu).getByRole("menuitem", { name: SPACE_ROW_DELETE_LABEL }));
    const dialog = await screen.findByRole("alertdialog");
    expect(sessionsFileDelete).not.toHaveBeenCalled();
    within(dialog).getByRole("button", { name: SPACE_ROW_DELETE_LABEL }).click();

    await waitFor(() =>
      expect(sessionsFileDelete).toHaveBeenCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "README.md",
      ),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  /**
   * The house pattern's other half (`files-pane.test.tsx:1959`). A ≥500ms
   * stationary press dispatches the synthetic `contextmenu` the Radix trigger
   * already listens for, so the phone gets the same menu rather than a second
   * one — and this exists to prove the bridge is wired to the row rather than
   * merely imported.
   */
  it("opens the same menu on a phone-tier long press", async () => {
    const originalMatchMedia = window.matchMedia;
    window.matchMedia = vi.fn().mockImplementation((query: string) => {
      const match = query.match(/max-width:\s*(\d+)px/);
      const maxWidth = match ? Number(match[1]) : Number.POSITIVE_INFINITY;
      return {
        matches: query.includes("prefers-reduced-motion") ? false : 390 <= maxWidth,
        media: query,
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: vi.fn(),
      };
    });
    try {
      mount();
      const row = screen.getByRole("treeitem", { name: "release-notes.md" });

      // Fake timers only around the hold: the query below polls on real ones.
      vi.useFakeTimers();
      fireEvent.pointerDown(row, { pointerId: 1, clientX: 30, clientY: 30 });
      act(() => {
        vi.advanceTimersByTime(500);
      });
      vi.useRealTimers();

      expect(verbs(await screen.findByRole("menu"))).toEqual([
        SPACE_ROW_OPEN_HERE_LABEL,
        SPACE_ROW_OPEN_BESIDE_LABEL,
        SPACE_ROW_OPEN_LABEL,
        SPACE_ROW_REVEAL_LABEL,
        SPACE_ROW_COPY_PATH_LABEL,
        SPACE_ROW_RENAME_LABEL,
        SPACE_ROW_DELETE_LABEL,
      ]);
    } finally {
      vi.useRealTimers();
      window.matchMedia = originalMatchMedia;
    }
  });
});
