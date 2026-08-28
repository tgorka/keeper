import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteFolderVm, NoteSpaceVm, NoteTagTreeVm } from "@/lib/ipc/client";

/**
 * Every command the three rail sections and the dialogs they can open reach
 * for. The rail never touches Tauri here; what is under test is the fold.
 */
vi.mock("@/lib/ipc/client", () => ({
  notesSpaces: vi.fn(),
  notesSpacesRestoreDefaults: vi.fn(),
  notesSpaceTerms: vi.fn(),
  notesSpaceSave: vi.fn(),
  notesTagTree: vi.fn(),
  notesTree: vi.fn(),
  notesTemplates: vi.fn(),
  notesDeletePlan: vi.fn(),
  notesDelete: vi.fn(),
}));

import { PhysicalTree } from "@/components/notes/physical-tree";
import {
  RESTORE_DEFAULTS,
  RESTORE_NOTHING_MISSING,
  SpaceList,
} from "@/components/notes/space-list";
import { TagTree } from "@/components/notes/tag-tree";
import { notesSpaces, notesSpacesRestoreDefaults, notesTagTree, notesTree } from "@/lib/ipc/client";
import { resetNotesFiltersStoreForTest } from "@/lib/stores/notes-filters";
import {
  hydrateNotesRailFold,
  NOTES_RAIL_FOLD_COOKIE,
  notesRailFoldStore,
  readNotesRailFold,
  resetNotesRailFoldForTest,
} from "@/lib/stores/notes-rail-fold";

const mockSpaces = vi.mocked(notesSpaces);
const mockRestore = vi.mocked(notesSpacesRestoreDefaults);
const mockTagTree = vi.mocked(notesTagTree);
const mockTree = vi.mocked(notesTree);

const SPACES: NoteSpaceVm[] = [
  {
    id: "s-inbox",
    name: "Inbox",
    query: "is:untagged",
    sort: "modified desc",
    sortEffective: "modified desc",
    limit: 500,
    icon: "inbox",
    order: 0,
    defaultKey: "inbox",
    template: null,
    folder: null,
    error: null,
    warnings: [],
  },
];

const TAGS: NoteTagTreeVm = {
  nodes: [{ name: "client", path: "client", count: 2, children: [] }],
};

const FOLDERS: NoteFolderVm = { relDir: "", dirs: ["projects"], notes: [] };

/** All three sections, in the order and the flex column the rail renders them in. */
function renderRail() {
  return render(
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <SpaceList vaultId="vault-1" />
      <TagTree vaultId="vault-1" />
      <PhysicalTree vaultId="vault-1" />
    </div>,
  );
}

/** The section itself, which survives every fold — it is the way back. */
function section(name: string): HTMLElement {
  return screen.getByRole("region", { name });
}

/**
 * A drift guard, and the measurement it stands in for.
 *
 * jsdom lays out no flexbox, so nothing here can see two captions on top of
 * each other. Measured in a browser against the built stylesheet, in a rail
 * 150px tall: with `min-h-0` the Tags section was laid out **4px** high while
 * the header inside it stayed 20px, so the header hung 20px past the section
 * and painted over the Files caption below. Without it the section's floor is
 * its own min-content height — 100px — and the captions are 76px apart.
 *
 * A section may shrink to its caption. It may not shrink past it.
 */
describe("a rail section never shrinks below its own caption", () => {
  it("gives Tags and Files a height floor of their contents, not of zero", async () => {
    renderRail();
    await waitFor(() => expect(section("Tags")).toBeInTheDocument());

    for (const name of ["Tags", "Files"]) {
      expect(section(name).className).not.toContain("min-h-0");
    }
  });
});

beforeEach(() => {
  mockSpaces.mockReset().mockResolvedValue(SPACES);
  mockRestore.mockReset().mockResolvedValue(0);
  mockTagTree.mockReset().mockResolvedValue(TAGS);
  mockTree.mockReset().mockResolvedValue(FOLDERS);
  resetNotesFiltersStoreForTest();
  resetNotesRailFoldForTest();
});

afterEach(() => {
  resetNotesFiltersStoreForTest();
  resetNotesRailFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
  document.cookie = `${NOTES_RAIL_FOLD_COOKIE}=; path=/; max-age=0`;
});

/**
 * Story 47.3. Before it, exactly one section of this rail folded — Files, in a
 * `useState` that forgot itself — and the two above it had no control at all.
 * Each section gets the same three assertions, because the failure this shape
 * prevents is per-section: a fold that hides its own control, or a fold that
 * forgets, is invisible to a test that only exercises a different section.
 */
describe("the notes rail folds Spaces", () => {
  it("hides the spaces and keeps the header, so there is a way back", async () => {
    renderRail();
    await screen.findByRole("button", { name: "Inbox" });

    fireEvent.click(screen.getByRole("button", { name: "Collapse Spaces" }));

    expect(screen.queryByRole("button", { name: "Inbox" })).not.toBeInTheDocument();
    // The section, its disclosure and the restore control all survive.
    expect(section("Spaces")).toBeInTheDocument();
    const fold = screen.getByRole("button", { name: "Expand Spaces" });
    expect(fold).toHaveAttribute("aria-expanded", "false");
    expect(fold).toHaveAttribute("aria-controls", "notes-rail-spaces");

    fireEvent.click(fold);
    expect(await screen.findByRole("button", { name: "Inbox" })).toBeInTheDocument();
  });

  /**
   * The Story 44.3 objection, answered rather than deleted.
   *
   * "Restore default spaces" is the one control that refills a vault whose owner
   * deleted every default, and the recorded reason not to fold Spaces was that
   * folding would hide it. It does not, because the fold hides the rows and not
   * the header — and pressing it while folded has to REPORT, or the control is
   * reachable and useless, which is the same defect wearing a hat.
   */
  it("still restores the defaults while folded, and still says what happened", async () => {
    renderRail();
    await screen.findByRole("button", { name: "Inbox" });
    fireEvent.click(screen.getByRole("button", { name: "Collapse Spaces" }));

    fireEvent.click(screen.getByRole("button", { name: RESTORE_DEFAULTS }));

    await waitFor(() => expect(mockRestore).toHaveBeenCalledWith("vault-1"));
    expect(await screen.findByText(RESTORE_NOTHING_MISSING)).toBeInTheDocument();
  });
});

describe("the notes rail folds Tags", () => {
  it("hides the tree and keeps the header, so there is a way back", async () => {
    renderRail();
    await screen.findByRole("button", { name: /^Tag client/ });

    fireEvent.click(screen.getByRole("button", { name: "Collapse Tags" }));

    expect(screen.queryByRole("button", { name: /^Tag client/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Expand Tags" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );

    fireEvent.click(screen.getByRole("button", { name: "Expand Tags" }));
    expect(await screen.findByRole("button", { name: /^Tag client/ })).toBeInTheDocument();
  });

  /**
   * The one thing about Tags that is not true of the other two: it owns the
   * column's spare height. `hidden` takes the BODY out of the layout, and a
   * section left at `flex-1` around a hidden body is an empty stripe where the
   * tree used to be — the fold would look like a rendering bug rather than a
   * fold. So the section stops claiming the height, and claims it again on the
   * way back.
   */
  it("gives the column its height back while folded and takes it again when opened", async () => {
    renderRail();
    await screen.findByRole("button", { name: /^Tag client/ });
    expect(section("Tags")).toHaveClass("flex-1");

    fireEvent.click(screen.getByRole("button", { name: "Collapse Tags" }));

    expect(section("Tags")).not.toHaveClass("flex-1");
    expect(section("Tags")).toHaveClass("shrink-0");

    fireEvent.click(screen.getByRole("button", { name: "Expand Tags" }));
    expect(section("Tags")).toHaveClass("flex-1");
  });
});

describe("the notes rail folds Files", () => {
  /**
   * Files arrives folded, and that is load-bearing rather than cosmetic: the
   * tree reads a directory per expansion, so an open default would make every
   * mount of the notes surface walk the vault root.
   */
  it("arrives shut and reads no directory until it is opened", async () => {
    renderRail();
    await screen.findByRole("button", { name: "Inbox" });

    expect(screen.getByRole("button", { name: "Expand Files" })).toBeInTheDocument();
    expect(mockTree).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Expand Files" }));

    await waitFor(() => expect(mockTree).toHaveBeenCalledWith("vault-1", ""));
    expect(await screen.findByRole("treeitem", { name: /projects/ })).toBeInTheDocument();
  });

  it("hides the tree and keeps the header, so there is a way back", async () => {
    renderRail();
    fireEvent.click(await screen.findByRole("button", { name: "Expand Files" }));
    await screen.findByRole("treeitem", { name: /projects/ });

    fireEvent.click(screen.getByRole("button", { name: "Collapse Files" }));

    expect(screen.queryByRole("treeitem", { name: /projects/ })).not.toBeInTheDocument();
    expect(section("Files")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Expand Files" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });
});

/**
 * The remount half of every section's contract, done through the real cookie
 * rather than through the store.
 *
 * An in-process remount would pass on a store that only lives in memory, which
 * is exactly what the Files fold was before this story: a `useState` that
 * forgot itself on every surface switch. So the store is wiped between the two
 * renders — a fresh process, nothing remembered — and the ONLY thing carried
 * across is the cookie the fold wrote.
 */
describe("the notes rail remembers its fold across a remount", () => {
  it.each([
    ["Spaces", "spaces"],
    ["Tags", "tags"],
    ["Files", "files"],
  ] as const)("keeps %s folded", async (label, group) => {
    // Files starts shut, so folding it means opening it first; the assertion is
    // that whatever state was chosen is the state that comes back.
    const first = renderRail();
    await screen.findByRole("button", { name: "Inbox" });
    fireEvent.click(
      screen.getByRole("button", { name: new RegExp(`^(Collapse|Expand) ${label}$`) }),
    );
    const chosen = notesRailFoldStore.getState().groups[group];
    const written = document.cookie;
    first.unmount();

    // A cold start: nothing in memory, only what the browser kept.
    resetNotesRailFoldForTest();
    expect(notesRailFoldStore.getState().groups[group]).not.toBe(chosen);
    hydrateNotesRailFold(written);
    renderRail();
    // The second mount's own read, not a row: the row under test may be folded
    // away, and waiting for one that is hidden would time out on a pass.
    await waitFor(() => expect(mockSpaces).toHaveBeenCalledTimes(2));

    expect(readNotesRailFold(written)[group]).toBe(chosen);
    expect(
      screen.getByRole("button", { name: `${chosen ? "Expand" : "Collapse"} ${label}` }),
    ).toHaveAttribute("aria-expanded", String(!chosen));
  });
});
