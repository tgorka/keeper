import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  FilesEntrySyncVm,
  FilesEntryVm,
  FilesListingVm,
  SyncProfileVm,
} from "@/lib/ipc/client";

// Mock the typed IPC client so the pane never touches Tauri.
const syncProfiles = vi.fn();
const syncBrowse = vi.fn();
const syncOpenEntry = vi.fn();
const syncDeletePlan = vi.fn();
const syncDeleteEntries = vi.fn();
const syncCreateEntry = vi.fn();
const revealPath = vi.fn();
// Story 45.13's chooser is mounted by this pane and imports these four. They
// answer for the two tests at the bottom of this file; every other test never
// opens the dialog and never reaches them.
const notesAttachTargets = vi.fn();
const notesAttachSources = vi.fn();
const notesBodyRead = vi.fn();
const notesBodyWrite = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  syncProfiles: () => syncProfiles(),
  syncBrowse: (id: unknown, subpath: unknown) => syncBrowse(id, subpath),
  syncOpenEntry: (id: unknown, subpath: unknown) => syncOpenEntry(id, subpath),
  syncDeletePlan: (id: unknown, subpaths: unknown) => syncDeletePlan(id, subpaths),
  syncDeleteEntries: (id: unknown, subpaths: unknown) => syncDeleteEntries(id, subpaths),
  syncCreateEntry: (id: unknown, subpath: unknown, name: unknown) =>
    syncCreateEntry(id, subpath, name),
  revealPath: (path: unknown) => revealPath(path),
  notesAttachTargets: (v: unknown, q: unknown, n: unknown) => notesAttachTargets(v, q, n),
  notesAttachSources: (v: unknown, s: unknown) => notesAttachSources(v, s),
  notesBodyRead: (v: unknown, n: unknown) => notesBodyRead(v, n),
  notesBodyWrite: (v: unknown, n: unknown, t: unknown, r: unknown) => notesBodyWrite(v, n, t, r),
}));

import {
  FILES_ALL_PAUSED_SENTENCE,
  FILES_CONFIRM_TESTID,
  FILES_COPY_PATH_LABEL,
  FILES_COUNT_SLOT,
  FILES_CREATE_LABEL,
  FILES_DELETE_LABEL,
  FILES_EMPTY_FOLDER_SENTENCE,
  FILES_NAME_FLOOR_PX,
  FILES_NAME_LABEL,
  FILES_NEW_FILE_LABEL,
  FILES_NEW_FILE_NAME_LABEL,
  FILES_NO_PROFILES_SENTENCE,
  FILES_OPEN_BESIDE_LABEL,
  FILES_OPEN_HERE_LABEL,
  FILES_OPEN_LABEL,
  FILES_PANE_TITLE,
  FILES_REFRESH_LABEL,
  FILES_REVEAL_LABEL,
  FILES_ROLE_SLOT,
  FILES_SELECTED_TESTID,
  FILES_SELECTION_LABEL,
  FILES_SIZE_BASE_NOTE,
  FILES_SIZE_SLOT,
  FILES_STATE_DETAIL_TESTID,
  FILES_TREE_LABEL,
  FILES_UNBUILT_CONTROL_LABELS,
  FILES_WRITE_ERROR_TESTID,
  FilesPane,
  filesRowActionsBudget,
} from "@/components/layout/files-pane";
import {
  COLUMN_COLLAPSE_PREFIX,
  COLUMN_EXPAND_PREFIX,
  COLUMN_RAIL_CONTROL_SLOT,
} from "@/components/layout/surface-column";
import {
  FILES_SYNC_MARK_LABEL,
  FILES_SYNC_MARK_TESTID,
} from "@/components/layout/sync-status-mark";
import { ATTACH_TO_NOTE_LABEL } from "@/components/notes/attach-to-note-dialog";
import { OVERFLOW_PANEL_LABEL, OVERFLOW_TRIGGER_LABEL } from "@/components/ui/overflow-value";
import { COLUMN_RESIZER_LABEL } from "@/components/ui/resizable-columns";
import { WINDOW_ROW_ATTR, WINDOW_VIEWPORT_ATTR } from "@/components/ui/window-list";
import {
  COLUMN_KEY_STEP,
  COLUMN_WIDTH_COOKIE,
  readColumnWidths,
  SURFACE_COLUMNS,
} from "@/lib/column-widths";
import { formatFileSize } from "@/lib/file-size";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { COLUMN_FOLD_COOKIE, resetColumnFoldForTest } from "@/lib/stores/column-fold";
import {
  FILES_TREE_COOKIE,
  filesTreeCookie,
  filesTreeStore,
  hydrateFilesTree,
  nodeKey,
  readFilesTree,
  resetFilesTreeForTest,
} from "@/lib/stores/files-tree";
import { notesVaultsStore } from "@/lib/stores/notes-vaults";
import { activePanel, panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";
import { type ListGeometry, withListGeometry, withTextLayout } from "@/test/layout";

/** The exact sentence Rust composes for an unplugged profile. Verbatim, because
 * the whole point of the state is that this reaches the screen unaltered. */
/** The exact sentence `keeper_sync::files_write` composes for a path outside a
 * vault. Verbatim, because the whole point of carrying it on the listing is
 * that it reaches the screen unaltered. */
const OUTSIDE_VAULT =
  "outside.txt is outside Vault's notes vault (10-notes), and keeper only writes inside the vault it manages. You can open and reveal this file here; changing it is your file manager's job.";

const DRIVE_IS_OUT =
  "/Volumes/merope/Field is not there. This folder lives on removable media — reattach the volume, then open it again.";

/**
 * Click, then let the promise the click started settle.
 *
 * Every action in this pane fires an IPC call whose `.then` sets state, so a
 * bare `fireEvent.click` leaves a React `act(...)` warning and a render the
 * assertion below has not seen. Flushing the microtask queue inside `act` is
 * what makes the next `expect` read the DOM after the answer landed.
 */
/**
 * A row's expand/collapse control.
 *
 * The folder toggle is the FIRST button in a row, and it was the only one until
 * Story 45.3 put New file beside it on an open, writable folder. Every call
 * site here used to be `within(row).getByRole("button")`, which was exact while
 * a row had one button and is ambiguous now; naming the concept once beats
 * threading an accessible name through twenty-seven call sites, and it says
 * which button is meant rather than relying on there being only one.
 */
function expander(row: HTMLElement): HTMLElement {
  return within(row).getAllByRole("button")[0] as HTMLElement;
}

async function click(element: HTMLElement): Promise<void> {
  await act(async () => {
    fireEvent.click(element);
    await Promise.resolve();
  });
}

function profile(p: Partial<SyncProfileVm> & Pick<SyncProfileVm, "id" | "name">): SyncProfileVm {
  return {
    localPath: `/Users/alice/${p.name}`,
    remoteUrl: "https://git.invalid/r.git",
    branch: "main",
    direction: "bidirectional",
    lane: "main",
    subpaths: [],
    excludes: [],
    removable: false,
    lfsMode: "materialize",
    lfsThresholdBytes: 100_000_000,
    settleMs: null,
    effectiveSettleMs: 5000,
    pollIntervalMs: null,
    effectivePollIntervalMs: 30_000,
    tags: [],
    commitSubjectTemplate: "",
    authorOverride: null,
    enabled: true,
    notes: false,
    notesSubfolder: null,
    recordings: false,
    recordingsSubfolder: "recordings",
    ...p,
  } as SyncProfileVm;
}

/** A synced entry — the state a row is in when nothing is wrong with it, so
 * every test that is not about the mark keeps reading as before.
 *
 * `extra` carries the fields a story added that most tests do not care about
 * (Story 45.5's `size` and `folderRole`), defaulted to the absences Rust sends
 * for an ordinary file: no size known, no configured role. A test that is about
 * one of them says so and overrides it. */
function entry(
  name: string,
  kind: FilesEntryVm["kind"],
  relativePath?: string,
  sync: FilesEntrySyncVm = { status: "synced", detail: null },
  extra: Partial<FilesEntryVm> = {},
): FilesEntryVm {
  const rel = relativePath ?? name;
  return {
    name,
    relativePath: rel,
    absolutePath: `/Users/alice/Vault/${rel}`,
    kind,
    sync,
    size: null,
    folderRole: null,
    // Story 45.3's location verdict. The default is the ordinary case for the
    // fixtures in this file — a file inside a vault keeper may write — because
    // most tests are not about writing and would otherwise all have to opt in.
    // The tests that ARE about it use `readOnly` below.
    write: { writable: true, reason: null, caveat: null },
    ...extra,
  } as FilesEntryVm;
}

/** An entry whose size is what Rust would actually have sent for `bytes`.
 *
 * The label goes through {@link formatFileSize} — the mirror pinned to
 * `keeper_core::size::format_file_size` by a shared vector table — rather than
 * being typed out here. A hand-written label would let this suite keep passing
 * while the product's real answer changed, which is the whole failure mode the
 * pinning exists to close: the assertion below is against the number keeper
 * genuinely produces, not against a string this file made up. */
function sized(name: string, bytes: number): FilesEntryVm {
  return entry(
    name,
    "file",
    name,
    { status: "synced", detail: null },
    {
      size: { bytes, label: formatFileSize(bytes) },
    },
  );
}

/** A file keeper will not write, carrying the reason Rust composed. */
function readOnly(name: string, reason = OUTSIDE_VAULT): FilesEntryVm {
  return entry(
    name,
    "file",
    name,
    { status: "synced", detail: null },
    {
      write: { writable: false, reason, caveat: null },
    },
  );
}

function listed(
  profileId: string,
  subpath: string,
  entries: FilesEntryVm[],
  detail: string | null = null,
  write: FilesListingVm["write"] = { writable: true, reason: null, caveat: null },
): FilesListingVm {
  return {
    profileId,
    subpath,
    state: "listed",
    entries,
    detail,
    truncated: detail !== null,
    write,
  };
}

function notListed(
  profileId: string,
  state: FilesListingVm["state"],
  detail: string,
): FilesListingVm {
  return {
    profileId,
    subpath: "",
    state,
    entries: null,
    detail,
    truncated: false,
    // A folder keeper could not read is not a folder keeper will write into.
    write: { writable: false, reason: detail, caveat: null },
  };
}

beforeEach(() => {
  syncProfiles.mockReset();
  syncProfiles.mockResolvedValue([]);
  syncBrowse.mockReset();
  syncBrowse.mockResolvedValue(listed("01VAULT", "", []));
  syncOpenEntry.mockReset();
  syncOpenEntry.mockResolvedValue(undefined);
  syncDeletePlan.mockReset();
  syncDeletePlan.mockResolvedValue({
    files: [],
    question: "There is nothing here keeper can delete.",
    consequence: "",
    recovery: "",
    refusals: [],
  });
  syncDeleteEntries.mockReset();
  syncDeleteEntries.mockResolvedValue({ deleted: [], refusals: [] });
  syncCreateEntry.mockReset();
  syncCreateEntry.mockResolvedValue("");
  revealPath.mockReset();
  revealPath.mockResolvedValue(undefined);
  capabilitiesStore.getState().applySnapshot({
    ...DEFAULT_CAPABILITIES,
    sync: true,
    revealInFileManager: true,
  });
  // The expansion is a module-level store now (Story 46.3), so it outlives a
  // `render` exactly as it outlives a mount in the app. Every test below that
  // does not arrange it starts from nothing open.
  resetFilesTreeForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: the store persists through the document, so clearing it is part of resetting this suite
  document.cookie = `${FILES_TREE_COOKIE}=; path=/; max-age=0`;
  // Story 48.1: the tree is a surface column, and its fold is a module-level
  // store plus a cookie. Both halves, like the expansion above.
  resetColumnFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: the store persists through the document, so clearing it is part of resetting this suite
  document.cookie = `${COLUMN_FOLD_COOKIE}=; path=/; max-age=0`;
  // biome-ignore lint/suspicious/noDocumentCookie: the width outlives a mount too
  document.cookie = `${COLUMN_WIDTH_COOKIE}=; path=/; max-age=0`;
});

afterEach(() => {
  vi.clearAllMocks();
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

describe("FilesPane", () => {
  it("names itself as a region so the shell's absence assertion has something to miss", async () => {
    render(<FilesPane />);
    expect(screen.getByRole("region", { name: FILES_PANE_TITLE })).toBeInTheDocument();
    await waitFor(() => expect(syncProfiles).toHaveBeenCalled());
  });

  it("lists every enabled profile and no paused one", async () => {
    syncProfiles.mockResolvedValue([
      profile({ id: "01VAULT", name: "Vault" }),
      profile({ id: "01FIELD", name: "Field" }),
      profile({ id: "01OLD", name: "Old Archive", enabled: false }),
    ]);
    render(<FilesPane />);

    const tree = await screen.findByRole("tree", { name: FILES_TREE_LABEL });
    expect(await within(tree).findByRole("treeitem", { name: "Vault" })).toBeInTheDocument();
    expect(within(tree).getByRole("treeitem", { name: "Field" })).toBeInTheDocument();
    // A paused folder is one keeper is not watching; browsing it would say
    // otherwise.
    expect(within(tree).queryByRole("treeitem", { name: "Old Archive" })).toBeNull();
  });

  it("says which kind of nothing it has when there is nothing to browse", async () => {
    render(<FilesPane />);
    expect(await screen.findByText(FILES_NO_PROFILES_SENTENCE)).toBeInTheDocument();
  });

  it("distinguishes no folders at all from every folder paused", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01OLD", name: "Old", enabled: false })]);
    render(<FilesPane />);
    expect(await screen.findByText(FILES_ALL_PAUSED_SENTENCE)).toBeInTheDocument();
    expect(screen.queryByText(FILES_NO_PROFILES_SENTENCE)).toBeNull();
  });

  // --- Lazy ---------------------------------------------------------------

  it("asks for a folder's children only when it is expanded, and only once", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", [entry("Notes", "folder"), entry("readme.md", "file")]),
    );
    render(<FilesPane />);

    const row = await screen.findByRole("treeitem", { name: "Vault" });
    // Mounting lists profiles and nothing else. These trees hold 100 000 files.
    expect(syncBrowse).not.toHaveBeenCalled();

    await click(expander(row));
    await waitFor(() => expect(syncBrowse).toHaveBeenCalledWith("01VAULT", ""));
    expect(await screen.findByRole("treeitem", { name: "Notes" })).toBeInTheDocument();
    expect(syncBrowse).toHaveBeenCalledTimes(1);

    // Collapsing and re-opening reuses what was loaded rather than re-asking.
    await click(expander(row));
    await waitFor(() => expect(screen.queryByRole("treeitem", { name: "Notes" })).toBeNull());
    await click(expander(row));
    expect(await screen.findByRole("treeitem", { name: "Notes" })).toBeInTheDocument();
    expect(syncBrowse).toHaveBeenCalledTimes(1);
  });

  it("expands a child folder by its own relative subpath, never by a composed path", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockImplementation((_id: string, subpath: string) =>
      Promise.resolve(
        subpath === ""
          ? listed("01VAULT", "", [entry("2026", "folder")])
          : listed("01VAULT", subpath, [entry("clip.mov", "video", "2026/clip.mov")]),
      ),
    );
    render(<FilesPane />);

    const root = await screen.findByRole("treeitem", { name: "Vault" });
    await click(expander(root));
    const child = await screen.findByRole("treeitem", { name: "2026" });
    await click(within(child).getAllByRole("button")[0]);

    // The subpath is the relative path Rust handed back — the frontend joined
    // nothing (AD-65).
    await waitFor(() => expect(syncBrowse).toHaveBeenCalledWith("01VAULT", "2026"));
    expect(await screen.findByRole("treeitem", { name: "clip.mov" })).toBeInTheDocument();
  });

  // --- The absent drive ----------------------------------------------------

  it("says the drive is out rather than showing an empty folder", async () => {
    syncProfiles.mockResolvedValue([
      profile({
        id: "01FIELD",
        name: "Field",
        removable: true,
        localPath: "/Volumes/merope/Field",
      }),
    ]);
    syncBrowse.mockResolvedValue(notListed("01FIELD", "mediaAbsent", DRIVE_IS_OUT));
    render(<FilesPane />);

    const row = await screen.findByRole("treeitem", { name: "Field" });
    await click(expander(row));

    const detail = await screen.findByTestId(FILES_STATE_DETAIL_TESTID);
    expect(detail).toHaveTextContent(DRIVE_IS_OUT);
    expect(detail).toHaveAttribute("data-state", "mediaAbsent");
    // The distinction the whole surface rests on.
    expect(screen.queryByText(FILES_EMPTY_FOLDER_SENTENCE)).toBeNull();
  });

  it("says a genuinely empty folder is empty, and says nothing about drives", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(listed("01VAULT", "", []));
    render(<FilesPane />);

    const row = await screen.findByRole("treeitem", { name: "Vault" });
    await click(expander(row));

    expect(await screen.findByText(FILES_EMPTY_FOLDER_SENTENCE)).toBeInTheDocument();
    // An empty folder is not a failure state, so it produces no state detail.
    expect(screen.queryByTestId(FILES_STATE_DETAIL_TESTID)).toBeNull();
    expect(screen.queryByText(DRIVE_IS_OUT)).toBeNull();
  });

  it("keeps a foreign volume and a folder that moved apart from both", async () => {
    syncProfiles.mockResolvedValue([
      profile({ id: "01A", name: "A", removable: true }),
      profile({ id: "01B", name: "B" }),
    ]);
    syncBrowse.mockImplementation((id: string) =>
      Promise.resolve(
        id === "01A"
          ? notListed("01A", "mediaUnexpected", "A different volume (01OTHER) is mounted there.")
          : notListed("01B", "missing", "/Users/alice/B is not there."),
      ),
    );
    render(<FilesPane />);

    await click(expander(await screen.findByRole("treeitem", { name: "A" })));
    await click(expander(await screen.findByRole("treeitem", { name: "B" })));

    const details = await screen.findAllByTestId(FILES_STATE_DETAIL_TESTID);
    expect(details.map((d) => d.getAttribute("data-state")).sort()).toEqual([
      "mediaUnexpected",
      "missing",
    ]);
    expect(screen.queryByText(FILES_EMPTY_FOLDER_SENTENCE)).toBeNull();
  });

  it("says when a listing was cut short rather than pretending the folder ended", async () => {
    const truncation = "This folder holds more than 1000 items — showing the first 1000.";
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(listed("01VAULT", "", [entry("a.md", "file")], truncation));
    render(<FilesPane />);

    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    expect(await screen.findByText(truncation)).toBeInTheDocument();
    expect(screen.getByRole("treeitem", { name: "a.md" })).toBeInTheDocument();
  });

  // --- Read-only by construction (AD-75) ------------------------------------

  // ---------------------------------------------------------------------
  // The write guard.
  //
  // AD-75 — "the files surface never writes" — was RETIRED by AD-89, the
  // owner's decision, epic 45. Until Story 45.3 the assertion here was
  // "offers no control that could write, rename, move or delete", iterating a
  // list that included Delete, and it existed to catch someone adding "just a
  // rename" to a read-only pane. The rule it defended is gone; the drift it
  // caught is not, so the guard was rewritten rather than deleted. What it
  // holds now:
  //
  //   * a write control exists ONLY where the location says yes, and where it
  //     does not the pane shows the reason instead of a dead button;
  //   * every destructive control is confirmed, and the confirmation names
  //     what goes;
  //   * nothing writes outside a vault — the half of AD-75 that stands.
  //
  // The controls nobody has built yet are still asserted absent, by name, for
  // the same reason the old list was: a control that arrives before its
  // command fails on click.
  // ---------------------------------------------------------------------

  it("offers no control that could rename, move or duplicate — only the two writes 45.3 built", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", [entry("Notes", "folder"), entry("clip.mov", "video")]),
    );
    render(<FilesPane />);

    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    await screen.findByRole("treeitem", { name: "clip.mov" });

    for (const label of FILES_UNBUILT_CONTROL_LABELS) {
      expect(
        screen.queryByRole("button", { name: new RegExp(`^${label}$`, "i") }),
        `${label} has no command behind it, so it must not exist (AD-89 replaced AD-75, it did not open the gates)`,
      ).toBeNull();
    }
    // No name field until New file is pressed: a browser with a text box
    // sitting in it is a rename waiting to be wired up.
    expect(screen.queryAllByRole("textbox")).toHaveLength(0);
  });

  it("offers no delete for a file the location refuses, and shows Rust's reason instead", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", [readOnly("outside.txt")], null, {
        writable: false,
        reason: OUTSIDE_VAULT,
        caveat: null,
      }),
    );
    render(<FilesPane />);
    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    const row = await screen.findByRole("treeitem", { name: "outside.txt" });

    await click(row);

    // Selected — a file outside a vault can still be looked at and clicked —
    // and yet no Delete, because the LOCATION said no. Both questions have to
    // say yes before a write control exists.
    expect(row).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("button", { name: FILES_DELETE_LABEL })).toBeNull();
    expect(syncDeletePlan).not.toHaveBeenCalled();
    // …and no New file either, because the folder itself is not writable.
    expect(screen.queryByRole("button", { name: FILES_NEW_FILE_LABEL })).toBeNull();
  });

  it("never deletes without a confirmation that named what goes", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(listed("01VAULT", "", [entry("readme.md", "file")]));
    syncDeletePlan.mockResolvedValue({
      files: ["readme.md"],
      question: "Delete readme.md?",
      consequence: "This file syncs, so deleting it here removes it from every machine.",
      recovery: "keeper moves it into the vault's trash rather than erasing it.",
      refusals: [],
    });
    render(<FilesPane />);
    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    await click(await screen.findByRole("treeitem", { name: "readme.md" }));

    await click(screen.getByRole("button", { name: FILES_DELETE_LABEL }));

    // The plan was asked for; nothing has been deleted.
    expect(syncDeletePlan).toHaveBeenCalledWith("01VAULT", ["readme.md"]);
    expect(syncDeleteEntries).not.toHaveBeenCalled();
    const dialog = await screen.findByRole("alertdialog");
    expect(within(dialog).getByText("Delete readme.md?")).toBeInTheDocument();
  });

  it("reveals, copies and opens — and each one leaves keeper rather than changing a file", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(listed("01VAULT", "", [entry("clip.mov", "video")]));
    render(<FilesPane />);

    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    const row = await screen.findByRole("treeitem", { name: "clip.mov" });

    await click(within(row).getByRole("button", { name: FILES_REVEAL_LABEL }));
    expect(revealPath).toHaveBeenCalledWith("/Users/alice/Vault/clip.mov");

    // Open goes through the profile-rooted command, by id and relative subpath —
    // never by handing an absolute path to an opener.
    await click(within(row).getByRole("button", { name: FILES_OPEN_LABEL }));
    expect(syncOpenEntry).toHaveBeenCalledWith("01VAULT", "clip.mov");

    // Copy path last, and from the MENU: a 360px column promotes two of the
    // three verbs, so this one is only in the menu at this width. Same handler
    // either way, which is the property worth pinning — the row's cluster and
    // its menu are built from one list.
    await act(async () => {
      fireEvent.contextMenu(row);
      await Promise.resolve();
    });
    await click(
      within(await screen.findByRole("menu")).getByRole("menuitem", {
        name: FILES_COPY_PATH_LABEL,
      }),
    );
    expect(writeText).toHaveBeenCalledWith("/Users/alice/Vault/clip.mov");
  });

  it("offers no Reveal where the platform has no file manager", async () => {
    capabilitiesStore.getState().applySnapshot({
      ...DEFAULT_CAPABILITIES,
      sync: true,
      revealInFileManager: false,
    });
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(listed("01VAULT", "", [entry("clip.mov", "video")]));
    render(<FilesPane />);

    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    const row = await screen.findByRole("treeitem", { name: "clip.mov" });
    // Absent, not disabled: a control that fails on activation is worse than no
    // control.
    expect(within(row).queryByRole("button", { name: FILES_REVEAL_LABEL })).toBeNull();
    expect(within(row).getByRole("button", { name: FILES_COPY_PATH_LABEL })).toBeInTheDocument();
  });

  it("shows a folder that could not be read verbatim rather than as an empty one", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockRejectedValue({
      code: "internal",
      message: "this folder could not be read: permission denied",
      accountId: null,
      retriable: false,
    });
    render(<FilesPane />);

    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    expect(
      await screen.findByText("this folder could not be read: permission denied"),
    ).toBeInTheDocument();
    expect(screen.queryByText(FILES_EMPTY_FOLDER_SENTENCE)).toBeNull();
  });

  it("re-reads only the folders that are open when Refresh is pressed", async () => {
    syncProfiles.mockResolvedValue([
      profile({ id: "01VAULT", name: "Vault" }),
      profile({ id: "01FIELD", name: "Field" }),
    ]);
    syncBrowse.mockResolvedValue(listed("01VAULT", "", [entry("a.md", "file")]));
    render(<FilesPane />);

    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    await screen.findByRole("treeitem", { name: "a.md" });
    expect(syncBrowse).toHaveBeenCalledTimes(1);

    await click(screen.getByRole("button", { name: FILES_REFRESH_LABEL }));
    // The one open folder is re-read; the collapsed one is not woken.
    await waitFor(() => expect(syncBrowse).toHaveBeenCalledTimes(2));
    expect(syncBrowse).toHaveBeenLastCalledWith("01VAULT", "");
  });
});

/**
 * The tree's keyboard model, pressed against the rendered DOM.
 *
 * Asserted by where focus actually lands, never by reading a `tabindex`
 * attribute off a row: a roving tabindex that marks the right element and does
 * not move focus is a tree that looks correct in the markup and traps a
 * keyboard user in practice — and a synced folder on a pendrive is exactly the
 * surface someone crosses without a mouse.
 */
describe("FilesPane keyboard navigation", () => {
  /** Press a key on whatever currently has focus, and let any load it started settle. */
  async function press(key: string): Promise<void> {
    const target = document.activeElement;
    if (target === null) {
      throw new Error("nothing is focused");
    }
    await act(async () => {
      fireEvent.keyDown(target, { key });
      await Promise.resolve();
    });
  }

  /** The accessible name of the row that currently has focus. */
  function focusedName(): string | null {
    return document.activeElement?.getAttribute("aria-label") ?? null;
  }

  /** A vault with one folder (which holds one file) and one file beside it. */
  function mountVault() {
    syncProfiles.mockResolvedValue([
      profile({ id: "01VAULT", name: "Vault" }),
      profile({ id: "01FIELD", name: "Field" }),
    ]);
    syncBrowse.mockImplementation((id: string, subpath: string) =>
      Promise.resolve(
        subpath === ""
          ? listed(id, "", [entry("2026", "folder"), entry("readme.md", "file")])
          : listed(id, subpath, [entry("clip.mov", "video", "2026/clip.mov")]),
      ),
    );
    render(<FilesPane />);
  }

  it("puts exactly one row in the tab order and moves it with focus", async () => {
    mountVault();
    const tree = await screen.findByRole("tree", { name: FILES_TREE_LABEL });
    const rows = () =>
      within(tree)
        .getAllByRole("treeitem")
        .map((row) => row.getAttribute("tabindex"));

    // Tab reaches the tree once; the arrows move inside it. Two hundred files
    // must not be two hundred Tab presses.
    expect(rows()).toEqual(["0", "-1"]);

    (await screen.findByRole("treeitem", { name: "Vault" })).focus();
    await press("ArrowDown");
    expect(focusedName()).toBe("Field");
    expect(rows()).toEqual(["-1", "0"]);
  });

  it("steps down and up one visible row at a time", async () => {
    mountVault();
    (await screen.findByRole("treeitem", { name: "Vault" })).focus();

    await press("ArrowDown");
    expect(focusedName()).toBe("Field");
    await press("ArrowUp");
    expect(focusedName()).toBe("Vault");
    // The ends do not wrap: an arrow at the top stays at the top.
    await press("ArrowUp");
    expect(focusedName()).toBe("Vault");
  });

  it("jumps to the first and last visible rows with Home and End", async () => {
    mountVault();
    (await screen.findByRole("treeitem", { name: "Vault" })).focus();

    await press("End");
    expect(focusedName()).toBe("Field");
    await press("Home");
    expect(focusedName()).toBe("Vault");
  });

  it("expands a closed folder with Right, then descends into it with Right again", async () => {
    mountVault();
    (await screen.findByRole("treeitem", { name: "Vault" })).focus();

    await press("ArrowRight");
    await waitFor(() => expect(syncBrowse).toHaveBeenCalledWith("01VAULT", ""));
    // The first Right expands and stays put — it does not also move.
    expect(focusedName()).toBe("Vault");
    expect(await screen.findByRole("treeitem", { name: "2026" })).toBeInTheDocument();

    await press("ArrowRight");
    expect(focusedName()).toBe("2026");
  });

  it("collapses an open folder with Left, and climbs to the parent from a child", async () => {
    mountVault();
    (await screen.findByRole("treeitem", { name: "Vault" })).focus();
    await press("ArrowRight");
    await screen.findByRole("treeitem", { name: "2026" });

    await press("ArrowDown");
    expect(focusedName()).toBe("2026");
    // A closed folder's Left climbs rather than collapsing something already shut.
    await press("ArrowLeft");
    expect(focusedName()).toBe("Vault");

    await press("ArrowLeft");
    expect(screen.queryByRole("treeitem", { name: "2026" })).toBeNull();
    expect(focusedName()).toBe("Vault");
  });

  it("toggles a folder with Enter and with Space", async () => {
    mountVault();
    (await screen.findByRole("treeitem", { name: "Vault" })).focus();

    await press("Enter");
    expect(await screen.findByRole("treeitem", { name: "readme.md" })).toBeInTheDocument();

    await press(" ");
    await waitFor(() => expect(screen.queryByRole("treeitem", { name: "readme.md" })).toBeNull());
  });

  it("does not let Right on an empty open folder look like it descended", async () => {
    syncProfiles.mockResolvedValue([
      profile({ id: "01VAULT", name: "Vault" }),
      profile({ id: "01FIELD", name: "Field" }),
    ]);
    syncBrowse.mockResolvedValue(listed("01VAULT", "", []));
    render(<FilesPane />);

    (await screen.findByRole("treeitem", { name: "Vault" })).focus();
    await press("ArrowRight");
    expect(await screen.findByText(FILES_EMPTY_FOLDER_SENTENCE)).toBeInTheDocument();

    // The next row in the DOM is the sibling profile, not a child of this one.
    // Descending into it would be the tree telling a lie about the hierarchy.
    await press("ArrowRight");
    expect(focusedName()).toBe("Vault");
  });

  it("leaves a file row's actions reachable by Tab only while that row is focused", async () => {
    mountVault();
    (await screen.findByRole("treeitem", { name: "Vault" })).focus();
    await press("ArrowRight");
    const file = await screen.findByRole("treeitem", { name: "readme.md" });

    // Not the focused row yet: its actions are out of the tab order, so Tab
    // does not walk every action of every row in the tree. Reveal rather than
    // Copy path because the default 360px column promotes two of a file's three
    // verbs and the third is in the row's menu — see the budget suite below.
    expect(within(file).getByRole("button", { name: FILES_REVEAL_LABEL })).toHaveAttribute(
      "tabindex",
      "-1",
    );

    file.focus();
    await act(async () => {
      await Promise.resolve();
    });
    expect(within(file).getByRole("button", { name: FILES_REVEAL_LABEL })).toHaveAttribute(
      "tabindex",
      "0",
    );
  });
});

/**
 * Story 44.12 — a name the tree is too narrow to show.
 *
 * jsdom lays nothing out, so `withTextLayout` answers the two properties the
 * real hook reads (`scrollWidth`, `clientWidth`) from the element's own text.
 * The hook, the effect, the conditional render and the popover are the real
 * ones. What is NOT proved here is that the tree's CSS actually truncates in a
 * browser, or that a real font makes a real name overflow a real pane.
 */
describe("FilesPane — a name too long for the tree", () => {
  const LONG = "a-quarterly-report-with-a-name-nobody-shortened-2026-Q3-final-v4.pdf";
  let restoreLayout: (() => void) | null = null;

  afterEach(() => {
    restoreLayout?.();
    restoreLayout = null;
  });

  async function tree(available: number): Promise<HTMLElement> {
    restoreLayout = withTextLayout(available);
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", [entry(LONG, "file"), entry("ok.md", "file")]),
    );
    render(<FilesPane />);
    const root = await screen.findByRole("treeitem", { name: "Vault" });
    await click(within(root).getByRole("button", { name: "Vault" }));
    return await screen.findByRole("tree", { name: FILES_TREE_LABEL });
  }

  it("offers the whole name, and only for the name that did not fit", async () => {
    // 67 characters at 8px each, in 200px of row.
    const rows = await tree(200);

    const long = within(rows).getByRole("treeitem", { name: LONG });
    const short = within(rows).getByRole("treeitem", { name: "ok.md" });

    const trigger = within(long).getByRole("button", {
      name: `${OVERFLOW_TRIGGER_LABEL} ${FILES_NAME_LABEL}`,
    });
    // A tree with an affordance on every row is a tree with a tab stop on every
    // row, which is the tree nobody can Tab out of.
    expect(
      within(short).queryByRole("button", {
        name: `${OVERFLOW_TRIGGER_LABEL} ${FILES_NAME_LABEL}`,
      }),
    ).toBeNull();

    await click(trigger);
    expect(screen.getByLabelText(`${OVERFLOW_PANEL_LABEL}: ${FILES_NAME_LABEL}`)).toHaveTextContent(
      LONG,
    );
  });

  it("keeps the affordance out of the tab order until its row is the focused one", async () => {
    const rows = await tree(200);
    const long = within(rows).getByRole("treeitem", { name: LONG });

    const named = { name: `${OVERFLOW_TRIGGER_LABEL} ${FILES_NAME_LABEL}` };
    expect(within(long).getByRole("button", named)).toHaveAttribute("tabindex", "-1");

    long.focus();
    await act(async () => {
      await Promise.resolve();
    });
    expect(within(long).getByRole("button", named)).toHaveAttribute("tabindex", "0");
  });

  it("does not grow an unbuilt write control while offering to show a name", async () => {
    const rows = await tree(200);

    // AD-75 retired by AD-89 (owner's decision, epic 45): this pane DOES write
    // now. What it must not grow is a control with no command behind it, which
    // is the same drift the AD-75 version of this assertion caught.
    for (const label of FILES_UNBUILT_CONTROL_LABELS) {
      expect(within(rows).queryByRole("button", { name: label })).toBeNull();
    }
  });
});

/**
 * The owner's second report against 0.8.5: a file row's controls "merged and
 * messed up", and folder rows fine.
 *
 * One cause. `Open` / `Reveal in Finder` / `Copy path` were three text buttons,
 * always mounted and all `shrink-0`, next to a size cell and a sync mark that
 * are too — about 250px of a 360px column. The name is the only member of the row
 * that gives ground, so it was squeezed to roughly 30px on every file row, which
 * is also why every file row grew an Expand trigger. A folder has no size cell
 * and no Open button, so its name fitted and it looked fine.
 *
 * The policy is a pure function for `planPriorityActions`' reason: jsdom lays
 * nothing out, so a test that asserted "the third control moved into the menu at
 * 320px" against a rendered tree would be asserting a shim. Every boundary is
 * provable exactly here, and the rendered half below asserts only what a
 * stubbed `clientWidth` can honestly carry — which of the row's verbs are on it.
 */
describe("FilesPane — a row's verbs against the column's width", () => {
  /**
   * The width the tree reports for itself, before it mounts and measures.
   *
   * `role="tree"` rather than a class: it is the element the rows are laid out
   * in and the one the pane's ref reads. Every other element keeps jsdom's
   * honest zero, so nothing else in the pane changes behaviour under this.
   */
  function withTreeWidth(px: number): () => void {
    const descriptor = Object.getOwnPropertyDescriptor(Element.prototype, "clientWidth");
    Object.defineProperty(Element.prototype, "clientWidth", {
      configurable: true,
      get(this: Element) {
        return this.getAttribute("role") === "tree" ? px : 0;
      },
    });
    return () => {
      if (descriptor === undefined) {
        Reflect.deleteProperty(Element.prototype, "clientWidth");
      } else {
        Object.defineProperty(Element.prototype, "clientWidth", descriptor);
      }
    };
  }

  /** A file two levels down — a profile root and one entry inside it, which is
   *  the shallowest a file can be and therefore the widest row a file gets. */
  const FILE_LEVEL = 2;

  let restoreWidth: (() => void) | null = null;

  afterEach(() => {
    restoreWidth?.();
    restoreWidth = null;
  });

  async function rowAt(px: number): Promise<HTMLElement> {
    restoreWidth = withTreeWidth(px);
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(listed("01VAULT", "", [entry("clip.mov", "video")]));
    render(<FilesPane />);
    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    return await screen.findByRole("treeitem", { name: "clip.mov" });
  }

  /** Which verbs are ON the row, in order. */
  function verbs(row: HTMLElement): string[] {
    return within(row)
      .queryAllByRole("button")
      .map((button) => button.getAttribute("aria-label") ?? "")
      .filter((label) =>
        [FILES_OPEN_LABEL, FILES_REVEAL_LABEL, FILES_COPY_PATH_LABEL].includes(label),
      );
  }

  it("keeps the name its floor at the column's own minimum, and promotes nothing", () => {
    // 220 is `SURFACE_COLUMNS["files-tree"].minWidth`: the narrowest the seam
    // will go. Nothing is left over for a control there, which is the point —
    // the row spends what it has on the name it exists to show.
    expect(SURFACE_COLUMNS["files-tree"].minWidth).toBe(220);
    expect(
      filesRowActionsBudget({ column: SURFACE_COLUMNS["files-tree"].minWidth, level: FILE_LEVEL }),
    ).toBe(0);
  });

  it("buys one control at 320 and all three by 480", () => {
    // Each promoted control costs 32px and the 4px gap after it, so the budget
    // buys one at 66px, two at 106 and three at 226. Boundaries either side, so
    // the arithmetic is pinned rather than sampled.
    expect(filesRowActionsBudget({ column: 320, level: FILE_LEVEL })).toBe(66);
    expect(filesRowActionsBudget({ column: 360, level: FILE_LEVEL })).toBe(106);
    expect(filesRowActionsBudget({ column: 480, level: FILE_LEVEL })).toBe(226);
    // And one level deeper is 16px narrower, all the way down.
    expect(filesRowActionsBudget({ column: 480, level: FILE_LEVEL + 1 })).toBe(210);
  });

  it("never returns less than nothing, however narrow or nonsense the column", () => {
    expect(filesRowActionsBudget({ column: 48, level: FILE_LEVEL })).toBe(0);
    expect(filesRowActionsBudget({ column: 0, level: 1 })).toBe(0);
    expect(filesRowActionsBudget({ column: Number.NaN, level: FILE_LEVEL })).toBe(0);
    expect(filesRowActionsBudget({ column: 480, level: Number.NaN })).toBe(0);
  });

  it("leaves the name more than its floor once the actions have been paid for", () => {
    // The floor is what the budget reserves, so whatever the plan does not spend
    // is the name's as well: the row is a flex box in which the name is the only
    // member that grows.
    const spentAt320 = 1 * (32 + 4);
    expect(filesRowActionsBudget({ column: 320, level: FILE_LEVEL })).toBeGreaterThanOrEqual(
      spentAt320,
    );
    expect(FILES_NAME_FLOOR_PX).toBeGreaterThan(0);
  });

  it("shows a 220px row no verbs at all", async () => {
    expect(verbs(await rowAt(220))).toEqual([]);
  });

  it("shows a 320px row its first verb and no more", async () => {
    expect(verbs(await rowAt(320))).toEqual([FILES_OPEN_LABEL]);
  });

  it("shows a 480px row all three, in the order the pane declared them", async () => {
    expect(verbs(await rowAt(480))).toEqual([
      FILES_OPEN_LABEL,
      FILES_REVEAL_LABEL,
      FILES_COPY_PATH_LABEL,
    ]);
  });

  it("still reaches every verb through the menu at the narrowest column", async () => {
    const row = await rowAt(220);
    expect(verbs(row)).toEqual([]);

    // Nothing on the row and everything one right-click away. This is the whole
    // licence for a row that shows no verbs: the budget decides which of them are
    // ALSO one click away and never whether they are reachable.
    await act(async () => {
      fireEvent.contextMenu(row);
      await Promise.resolve();
    });
    expect(
      within(await screen.findByRole("menu"))
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual([
      FILES_OPEN_HERE_LABEL,
      FILES_OPEN_BESIDE_LABEL,
      FILES_OPEN_LABEL,
      FILES_REVEAL_LABEL,
      FILES_COPY_PATH_LABEL,
    ]);
  });
});

/**
 * Story 44.10 — a folder, not a screenful.
 *
 * A synced folder with a thousand photos in it is ordinary, and 43.8's roving
 * tabindex is the thing windowing this tree is most likely to destroy quietly:
 * "the next visible row" stops being a DOM sibling the moment rows unmount, and
 * a focus target that is not mounted cannot receive focus. Every assertion below
 * counts mounted rows or follows focus.
 *
 * `withListGeometry` is load-bearing. jsdom lays nothing out, so without it the
 * scroll offset can never leave zero and a tree that mounted all three thousand
 * entries in a browser would satisfy these on whatever window it first rendered.
 * What it cannot prove is whether a real row is really 32 px at the real font.
 */
describe("FilesPane — a folder, not a screenful", () => {
  const VISIBLE_ROWS = 10;
  const ROW_PX = 32;
  const OVERSCAN = 6;

  /** One profile root plus three thousand files under it. */
  const CHILDREN = Array.from({ length: 3000 }, (_, index) =>
    entry(`photo-${String(index).padStart(4, "0")}.jpg`, "image"),
  );

  let geometry: ListGeometry | null = null;

  afterEach(() => {
    geometry?.undo();
    geometry = null;
  });

  function mountedRows(): number[] {
    return Array.from(document.querySelectorAll(`[${WINDOW_ROW_ATTR}]`)).map((element) =>
      Number(element.getAttribute(WINDOW_ROW_ATTR)),
    );
  }

  function viewport(): HTMLElement {
    const element = document.querySelector(`[${WINDOW_VIEWPORT_ATTR}]`);
    if (!(element instanceof HTMLElement)) {
      throw new Error("the files tree has no scroll viewport");
    }
    return element;
  }

  /** Render the pane with the big folder already open. */
  async function openBigFolder(): Promise<void> {
    geometry = withListGeometry({ viewport: VISIBLE_ROWS * ROW_PX, row: ROW_PX });
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(listed("01VAULT", "", CHILDREN));
    render(<FilesPane />);
    const root = await screen.findByRole("treeitem", { name: "Vault" });
    await click(expander(root));
    await screen.findByRole("treeitem", { name: "photo-0000.jpg" });
  }

  it("mounts a window over three thousand files, not three thousand rows", async () => {
    await openBigFolder();

    // One root row plus a window into its children — nowhere near 3001.
    expect(mountedRows().length).toBeLessThanOrEqual(VISIBLE_ROWS + OVERSCAN * 2);
    expect(screen.queryByRole("treeitem", { name: "photo-2999.jpg" })).toBeNull();
  });

  it("reaches the last file by scrolling", async () => {
    await openBigFolder();

    act(() => geometry?.scrollTo(viewport(), 3001 * ROW_PX));

    expect(screen.getByRole("treeitem", { name: "photo-2999.jpg" })).toBeInTheDocument();
    expect(mountedRows().length).toBeLessThanOrEqual(VISIBLE_ROWS + OVERSCAN * 2 + 1);
  });

  /**
   * Story 44.11, and the AC's own shape: virtualisation ON, a fixture two
   * orders of magnitude larger than one window, and a count that is of the
   * folder rather than of the DOM.
   */
  it("says how many entries the folder holds, not how many rows are mounted", async () => {
    await openBigFolder();

    expect(mountedRows().length).toBeLessThanOrEqual(VISIBLE_ROWS + OVERSCAN * 2);
    expect(countOf("Vault")).toBe(`${(3000).toLocaleString()} items`);
  });

  it("walks End and Home across rows that were never rendered", async () => {
    await openBigFolder();

    const first = screen.getByRole("treeitem", { name: "Vault" });
    first.focus();
    fireEvent.keyDown(first, { key: "End" });

    // Three thousand rows away, and mounted only because End put it there.
    const last = screen.getByRole("treeitem", { name: "photo-2999.jpg" });
    expect(document.activeElement).toBe(last);

    // One step back is the row before it in the DATA, which is also the row
    // above it on screen — the two must not have come apart.
    fireEvent.keyDown(last, { key: "ArrowUp" });
    expect(document.activeElement).toBe(screen.getByRole("treeitem", { name: "photo-2998.jpg" }));

    fireEvent.keyDown(document.activeElement as HTMLElement, { key: "Home" });
    expect(document.activeElement).toBe(screen.getByRole("treeitem", { name: "Vault" }));
  });

  it("keeps exactly one row in the tab order, however far it is scrolled away", async () => {
    await openBigFolder();
    const stops = () => document.querySelectorAll('[role="treeitem"][tabindex="0"]');

    expect(stops()).toHaveLength(1);

    act(() => geometry?.scrollTo(viewport(), 3001 * ROW_PX));

    // The tab stop is the Vault root, three thousand rows above the viewport.
    // Unmounting it would leave the tree with no row carrying `tabIndex=0`, and
    // Tab would walk straight past the whole Files surface.
    expect(stops()).toHaveLength(1);
    expect(stops()[0]).toHaveAccessibleName("Vault");
  });

  it("keeps the remembered row focused after it is scrolled out and back", async () => {
    await openBigFolder();

    const chosen = screen.getByRole("treeitem", { name: "photo-0003.jpg" });
    fireEvent.focus(chosen);
    expect(chosen).toHaveAttribute("tabindex", "0");

    act(() => geometry?.scrollTo(viewport(), 3001 * ROW_PX));
    act(() => geometry?.scrollTo(viewport(), 0));

    // Same row, still the one tab stop, and the tree came back to where it was
    // rather than to wherever the remembered row happened to be.
    expect(screen.getByRole("treeitem", { name: "photo-0003.jpg" })).toHaveAttribute(
      "tabindex",
      "0",
    );
    expect(viewport().scrollTop).toBe(0);
  });
});

/**
 * Story 44.17 — whether a file is synced.
 *
 * The listing is mocked here on purpose: what a real repository produces is
 * proved in `keeper-sync`'s `browse::` tests against a real commit and the
 * engine's real pending list. What is only provable in a DOM is that each
 * state reaches the screen as its own thing, that the sentence Rust composed
 * arrives unaltered, and that the mark does not join the tab order 43.8 spent
 * a story getting right.
 */
describe("FilesPane — is this file synced", () => {
  /** Verbatim, because the whole point is that Rust's words reach the screen. */
  const EXCLUDED_SENTENCE =
    "A pattern in this folder's sync settings excludes it, so keeper will never copy it.";
  const WAITING_SENTENCE = "This file is new and has not been committed yet.";
  const NO_REPO_SENTENCE =
    "This folder is not a repository yet. The first sync sets one up and takes everything in it.";
  const UNKNOWN_SENTENCE =
    "keeper could not read this folder's sync state: status failed: index is sparse";

  function marked(
    name: string,
    status: FilesEntrySyncVm["status"],
    detail: string | null,
  ): FilesEntryVm {
    return entry(name, "file", name, { status, detail });
  }

  /** Open a vault holding one entry of every state. */
  async function openMixedFolder(): Promise<void> {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", [
        marked("clean.md", "synced", null),
        marked("fresh.md", "waiting", WAITING_SENTENCE),
        marked("scratch.tmp", "excluded", EXCLUDED_SENTENCE),
        marked("orphan.md", "notInRepository", NO_REPO_SENTENCE),
        marked("puzzling.md", "unknown", UNKNOWN_SENTENCE),
      ]),
    );
    render(<FilesPane />);
    const root = await screen.findByRole("treeitem", { name: "Vault" });
    await click(expander(root));
    await screen.findByRole("treeitem", { name: "clean.md" });
  }

  function markOf(rowName: string): HTMLElement {
    return within(screen.getByRole("treeitem", { name: rowName })).getByTestId(
      FILES_SYNC_MARK_TESTID,
    );
  }

  it("gives each state its own mark, so an excluded file never reads as waiting", async () => {
    await openMixedFolder();

    const states = [
      ["clean.md", "synced"],
      ["fresh.md", "waiting"],
      ["scratch.tmp", "excluded"],
      ["orphan.md", "notInRepository"],
      ["puzzling.md", "unknown"],
    ] as const;
    for (const [name, status] of states) {
      expect(markOf(name)).toHaveAttribute("data-sync-status", status);
    }

    // The distinction the story turns on, asserted as the two things a person
    // actually perceives: a different shape and a different name. A file that
    // will never sync must not be wearing the mark of one that is about to.
    const excluded = markOf("scratch.tmp");
    const waiting = markOf("fresh.md");
    expect(excluded).toHaveAccessibleName(EXCLUDED_SENTENCE);
    expect(waiting).toHaveAccessibleName(WAITING_SENTENCE);
    expect(excluded.innerHTML).not.toEqual(waiting.innerHTML);
  });

  it("renders the sentence Rust composed rather than one of its own", async () => {
    await openMixedFolder();

    // Not a paraphrase and not a shortened form: the browser and the Pending
    // card describe the same engine state, and a second copy of these words
    // here is the copy that would be edited alone.
    expect(markOf("orphan.md")).toHaveAccessibleName(NO_REPO_SENTENCE);
    // Including the engine's own failure, verbatim — a reason someone can act
    // on beats a sentence that only says something went wrong.
    expect(markOf("puzzling.md")).toHaveAccessibleName(UNKNOWN_SENTENCE);
    // A synced file has no story, so it falls back to the short name.
    expect(markOf("clean.md")).toHaveAccessibleName(FILES_SYNC_MARK_LABEL.synced);
  });

  it("stays out of the tab order, on the focused row and every other one", async () => {
    await openMixedFolder();

    // 43.8's roving tabindex: exactly one row is a tab stop and its actions
    // join the tab order only while it is focused. A mark is not an action —
    // there is nothing to activate — so it must never be a stop at all, focused
    // row or not.
    const focused = screen.getByRole("treeitem", { name: "fresh.md" });
    fireEvent.focus(focused);
    expect(focused).toHaveAttribute("tabindex", "0");

    for (const mark of screen.getAllByTestId(FILES_SYNC_MARK_TESTID)) {
      expect(mark).not.toHaveAttribute("tabindex");
    }
    expect(document.querySelectorAll('[role="treeitem"][tabindex="0"]')).toHaveLength(1);
  });

  it("gives a profile root no mark, because its children answer for themselves", async () => {
    await openMixedFolder();

    const root = screen.getByRole("treeitem", { name: "Vault" });
    expect(within(root).queryByTestId(FILES_SYNC_MARK_TESTID)).toBeNull();
  });

  it("shows the new mark once sync has moved on", async () => {
    await openMixedFolder();
    expect(markOf("fresh.md")).toHaveAttribute("data-sync-status", "waiting");

    // The mark is part of the listing, so it is only ever as old as the listing
    // — and Refresh re-reads every open folder. A cached mark that outlived the
    // sync it described would be the "waiting forever" this story removes,
    // wearing a different hat.
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", [
        marked("clean.md", "synced", null),
        marked("fresh.md", "synced", null),
        marked("scratch.tmp", "excluded", EXCLUDED_SENTENCE),
        marked("orphan.md", "notInRepository", NO_REPO_SENTENCE),
        marked("puzzling.md", "unknown", UNKNOWN_SENTENCE),
      ]),
    );
    await click(screen.getByRole("button", { name: FILES_REFRESH_LABEL }));

    await waitFor(() => expect(markOf("fresh.md")).toHaveAttribute("data-sync-status", "synced"));
    // …and the states that did not move did not move.
    expect(markOf("scratch.tmp")).toHaveAttribute("data-sync-status", "excluded");
  });
});

/**
 * One of the facts a row is described by, picked out by its slot, or `null`
 * when the row carries none of that kind.
 *
 * Read through `aria-describedby`, which is where these live: a tree row's NAME
 * is the folder, and folding a number into it would stop "Vault" being the row
 * called Vault for anyone navigating by first letter.
 *
 * `aria-describedby` is a LIST of ids (Story 45.5 added a size and a folder
 * role beside 44.11's count), so this resolves each and selects by slot rather
 * than assuming the row is described by exactly one thing.
 */
function describedBySlot(name: string, slot: string): string | null {
  const row = screen.getByRole("treeitem", { name });
  const ids = row.getAttribute("aria-describedby")?.split(" ") ?? [];
  for (const id of ids) {
    const element = document.getElementById(id);
    if (element?.dataset.slot === slot) {
      return element.textContent;
    }
  }
  return null;
}

function countOf(name: string): string | null {
  return describedBySlot(name, FILES_COUNT_SLOT);
}

describe("FilesPane — how many entries", () => {
  it("counts an open folder and says nothing about a closed one", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockImplementation((id: string, subpath: string) =>
      Promise.resolve(
        subpath === ""
          ? listed(id, "", [entry("2026", "folder"), entry("readme.md", "file")])
          : listed(id, subpath, [
              entry("clip.mov", "video", "2026/clip.mov"),
              entry("notes.md", "file", "2026/notes.md"),
              entry("still.png", "image", "2026/still.png"),
            ]),
      ),
    );
    render(<FilesPane />);

    const root = await screen.findByRole("treeitem", { name: "Vault" });
    // A closed folder has no count: keeper has not read it, and reading every
    // folder to number it is exactly what lazy expansion exists not to do.
    expect(countOf("Vault")).toBeNull();

    await click(expander(root));
    await screen.findByRole("treeitem", { name: "2026" });
    expect(countOf("Vault")).toBe("2 items");
    expect(countOf("2026")).toBeNull();

    await click(screen.getByRole("button", { name: "2026" }));
    await screen.findByRole("treeitem", { name: "clip.mov" });
    expect(countOf("2026")).toBe("3 items");

    // Closing it takes the count with it. The listing survives in memory, so a
    // count left behind would be a number taken at a moment nobody can name,
    // about rows nobody can see.
    await click(screen.getByRole("button", { name: "2026" }));
    await waitFor(() => expect(countOf("2026")).toBeNull());
  });

  it("says zero for an empty folder rather than dropping the count", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(listed("01VAULT", "", []));
    render(<FilesPane />);

    const root = await screen.findByRole("treeitem", { name: "Vault" });
    await click(expander(root));
    await screen.findByText(FILES_EMPTY_FOLDER_SENTENCE);

    expect(countOf("Vault")).toBe("0 items");
  });

  it("marks a capped listing as a floor instead of passing it off as a total", async () => {
    // `keeper_sync::browse` stops at `LISTING_CAP` and says it did — a bit that
    // has been on the wire since Story 43.8 and that nothing read until now. It
    // is exactly what tells an exact count from a floor, and `1,000+` declines
    // to be a total in the number rather than in a sentence below it.
    const CAPPED = Array.from({ length: 1000 }, (_, index) =>
      entry(`f${String(index).padStart(4, "0")}.md`, "file"),
    );
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", CAPPED, "This folder holds more than 1000 items."),
    );
    render(<FilesPane />);

    const root = await screen.findByRole("treeitem", { name: "Vault" });
    await click(expander(root));
    await screen.findByRole("treeitem", { name: "f0000.md" });

    expect(countOf("Vault")).toBe(`${(1000).toLocaleString()}+ items`);
  });

  it("gives no count to a folder keeper could not read", async () => {
    // An absent drive knows no number, and `0 items` would be a claim about a
    // folder nobody opened. The sentence Rust composed is the whole answer.
    syncProfiles.mockResolvedValue([profile({ id: "01FIELD", name: "Field", removable: true })]);
    syncBrowse.mockResolvedValue(
      notListed("01FIELD", "mediaAbsent", "/Volumes/merope/Field is not there."),
    );
    render(<FilesPane />);

    const root = await screen.findByRole("treeitem", { name: "Field" });
    await click(expander(root));
    await screen.findByText("/Volumes/merope/Field is not there.");

    expect(countOf("Field")).toBeNull();
  });
});

/**
 * The size a row shows, or `null` when it shows none (Story 45.5).
 *
 * Queried out of the row's own subtree rather than by text, so "this row has no
 * size" is distinguishable from "some other row has that size".
 */
function sizeOf(name: string): string | null {
  const row = screen.getByRole("treeitem", { name });
  return row.querySelector(`[data-slot="${FILES_SIZE_SLOT}"]`)?.textContent ?? null;
}

/**
 * The lucide glyph name a row renders, read off the icon's own class.
 *
 * `lucide-react` emits `class="lucide lucide-<kebab-name>"`, which is the only
 * handle a rendered icon has — it takes `aria-hidden` here, deliberately, so it
 * has no accessible name to query by. The first icon in the row is the row's
 * own: the chevron before it is filtered out because it is what a test asking
 * "which glyph did this file get" would otherwise get every time.
 */
function glyphOf(name: string): string | null {
  const row = screen.getByRole("treeitem", { name });
  for (const svg of row.querySelectorAll("svg")) {
    const glyph = Array.from(svg.classList).find(
      (className) => className.startsWith("lucide-") && className !== "lucide-react",
    );
    if (
      glyph === undefined ||
      glyph === "lucide-chevron-down" ||
      glyph === "lucide-chevron-right"
    ) {
      continue;
    }
    return glyph;
  }
  return null;
}

describe("FilesPane — what it is and how big", () => {
  /** Expand the one profile so its entries are on screen. */
  async function expandVault(entries: FilesEntryVm[], p: Partial<SyncProfileVm> = {}) {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault", ...p })]);
    syncBrowse.mockResolvedValue(listed("01VAULT", "", entries));
    render(<FilesPane />);
    const root = await screen.findByRole("treeitem", { name: "Vault" });
    await click(expander(root));
    await screen.findByRole("treeitem", { name: entries[0].name });
  }

  /**
   * Every size on screen is the one Rust computed, at the boundaries that tell
   * the two possible bases apart (Story 45.5, FR-178).
   *
   * The expected strings come from {@link formatFileSize} — the mirror pinned
   * to `keeper_core::size::format_file_size` by a checked-in vector table — and
   * not from literals typed here, so this cannot keep passing while the
   * product's real answer changes. What it proves about the PANE is that the
   * label passes through verbatim: nothing in this component divides, rounds,
   * abbreviates or re-units a number.
   *
   * 999 and 1000 are either side of the decimal base; 1024 is where a binary
   * implementation would step and this one has already stepped. 1 is the
   * singular. 0 is a real size for a file — it is a DIRECTORY that must show
   * nothing, which the next test pins.
   */
  it("shows the size Rust computed, decimal, at every boundary", async () => {
    await expandVault([
      sized("empty.md", 0),
      sized("one.bin", 1),
      sized("just-under.bin", 999),
      sized("exactly-a-kb.bin", 1_000),
      sized("one-kibibyte.bin", 1_024),
      sized("recording.mov", 5_000_000_000),
    ]);

    expect(sizeOf("empty.md")).toBe(formatFileSize(0));
    expect(sizeOf("empty.md")).toBe("0 bytes");
    expect(sizeOf("one.bin")).toBe("1 byte");
    expect(sizeOf("just-under.bin")).toBe("999 bytes");
    expect(sizeOf("exactly-a-kb.bin")).toBe("1.0 kB");
    // Decimal: 1024 bytes is 1.024 kB. A binary pane would say "1.0 KiB", and a
    // pane that had quietly grown its own divisor would say "1 KB".
    expect(sizeOf("one-kibibyte.bin")).toBe("1.0 kB");
    expect(sizeOf("recording.mov")).toBe("5.0 GB");
  });

  /**
   * A directory shows no size at all — not "0 B", not a dash, not an em space.
   *
   * This is the assertion worth keeping when someone later makes `size`
   * non-optional "to simplify the type": a folder rendered as zero is a false
   * claim about every folder that has anything in it, and the folder here has
   * a 4 096-byte file inside it so the wrong implementation has a real number
   * available to print.
   */
  it("gives a folder no size, and never renders it as zero", async () => {
    await expandVault([entry("Archive", "folder"), sized("inside.md", 4_096)]);

    expect(sizeOf("Archive")).toBeNull();
    const folderRow = screen.getByRole("treeitem", { name: "Archive" });
    expect(folderRow.textContent).not.toMatch(/0\s*(B|bytes)/);
    // The file beside it does carry one, so the absence above is about being a
    // folder and not about the fixture forgetting a field.
    expect(sizeOf("inside.md")).toBe("4.0 kB");
  });

  /**
   * The base the pane chose is stated where the number is, because the choice
   * is visible in the number itself.
   */
  it("says which base its sizes use, beside the exact byte count", async () => {
    await expandVault([sized("clip.mov", 1_048_576)]);

    const cell = screen
      .getByRole("treeitem", { name: "clip.mov" })
      .querySelector(`[data-slot="${FILES_SIZE_SLOT}"]`);
    expect(cell).not.toBeNull();
    expect(cell?.getAttribute("title")).toBe(`1048576 bytes. ${FILES_SIZE_BASE_NOTE}`);
    expect(FILES_SIZE_BASE_NOTE).toContain("1000");
  });

  /**
   * The glyph comes from the viewer registry, so a format the registry knows is
   * a format the pane draws (Story 45.5, AD-87).
   *
   * Before this story the pane had its own `Record<kind, LucideIcon>` over the
   * five-value attachment vocabulary, and every one of the four files below
   * except the video drew the same generic page: Rust classifies a `.csv`, a
   * `.rs` and a `.pdf` all as kind `file`, so a kind-keyed table cannot tell
   * them apart however carefully it is written. These four assertions all fail
   * against that table, which is what makes this a test of the seam and not of
   * the icon set.
   */
  it("takes a file's glyph from the viewer registry rather than from its kind", async () => {
    await expandVault([
      entry("clip.mov", "video"),
      entry("budget.csv", "file"),
      entry("main.rs", "file"),
      entry("contract.pdf", "file"),
      entry("mystery.qqq", "file"),
    ]);

    expect(glyphOf("clip.mov")).toBe("lucide-file-play");
    expect(glyphOf("budget.csv")).toBe("lucide-file-spreadsheet");
    expect(glyphOf("main.rs")).toBe("lucide-file-code");
    expect(glyphOf("contract.pdf")).toBe("lucide-file-type");
    // A format with no row is the registry's `unknown`, which is a first-class
    // answer (AD-91) and has its own glyph rather than a blank cell.
    expect(glyphOf("mystery.qqq")).toBe("lucide-file-question-mark");

    // The property behind those five, stated once so a future mapping that is
    // wrong-but-plausible still fails: five files that Rust classifies as only
    // TWO kinds must still draw five different glyphs. A kind-keyed table can
    // draw at most two, whatever it maps them to.
    const glyphs = ["clip.mov", "budget.csv", "main.rs", "contract.pdf", "mystery.qqq"].map(
      glyphOf,
    );
    expect(new Set(glyphs).size).toBe(5);
  });

  /**
   * The vault and the recordings folder are marked from CONFIGURATION, and the
   * pane never looks at a folder's name (Story 45.5, FR-178).
   *
   * The fixture is the adversarial one: the real vault is called `Second Brain`
   * and the real recordings folder is called `Clips`, while an ordinary folder
   * sitting beside them is called `10-notes` — keeper's own default vault name.
   * An implementation that matched the default names, which is the shortcut the
   * story forbids, marks the decoy and misses both real folders.
   *
   * Rust decides this and sends `folderRole`; the pane only renders it. That is
   * why the decoy carries `folderRole: null` here rather than being distinguished
   * by anything this component could compute — the pane has no way to tell the
   * three apart other than the field, which is the property being pinned.
   */
  it("marks the vault and the recordings folder from configuration, not from a name", async () => {
    await expandVault(
      [
        entry("Second Brain", "folder", "Second Brain", undefined, {
          folderRole: "notesVault",
        }),
        entry("Clips", "folder", "Clips", undefined, { folderRole: "recordings" }),
        entry("10-notes", "folder"),
      ],
      {
        notes: true,
        notesSubfolder: "Second Brain",
        recordings: true,
        recordingsSubfolder: "Clips",
      },
    );

    expect(glyphOf("Second Brain")).toBe("lucide-notebook-pen");
    expect(glyphOf("Clips")).toBe("lucide-clapperboard");
    // The decoy is an ordinary closed folder, glyph and all.
    expect(glyphOf("10-notes")).toBe("lucide-folder");

    // And the marker is speakable, not only visible: a glyph with no words is a
    // fact only a sighted reader gets, and "which of these is my vault" is a
    // question everybody asks.
    expect(describedBySlot("Second Brain", FILES_ROLE_SLOT)).toBe("Your notes vault");
    expect(describedBySlot("Clips", FILES_ROLE_SLOT)).toBe("Where recordings are saved");
    expect(describedBySlot("10-notes", FILES_ROLE_SLOT)).toBeNull();
  });

  /**
   * A marked folder keeps its marker when it is opened.
   *
   * The obvious implementation branches on open/closed first and falls through
   * to the role only for a closed folder, which loses the marker at exactly the
   * moment a person is looking inside the vault and wondering whether they are
   * in it.
   */
  it("keeps the vault's marker while the vault is open", async () => {
    syncProfiles.mockResolvedValue([
      profile({ id: "01VAULT", name: "Vault", notes: true, notesSubfolder: "Second Brain" }),
    ]);
    syncBrowse.mockImplementation((_id: string, subpath: string) =>
      Promise.resolve(
        subpath === ""
          ? listed("01VAULT", "", [
              entry("Second Brain", "folder", "Second Brain", undefined, {
                folderRole: "notesVault",
              }),
            ])
          : listed("01VAULT", subpath, [sized("daily.md", 120)]),
      ),
    );
    render(<FilesPane />);
    const root = await screen.findByRole("treeitem", { name: "Vault" });
    await click(expander(root));

    const vault = await screen.findByRole("treeitem", { name: "Second Brain" });
    expect(glyphOf("Second Brain")).toBe("lucide-notebook-pen");

    await click(expander(vault));
    await screen.findByRole("treeitem", { name: "daily.md" });
    expect(glyphOf("Second Brain")).toBe("lucide-notebook-pen");
    expect(sizeOf("daily.md")).toBe("120 bytes");
  });
});

describe("FilesPane — a row opens a panel", () => {
  /** Expand the one profile and hand back its `readme.md` row. */
  async function fileRow(): Promise<HTMLElement> {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", [entry("readme.md", "file"), entry("notes.md", "file")]),
    );
    render(<FilesPane />);
    const root = await screen.findByRole("treeitem", { name: "Vault" });
    // Named, because the row now carries more than one control (Story 45.3's
    // selection checkbox among them) and an unnamed query would pick whichever
    // one happened to be first.
    await click(within(root).getByRole("button", { name: "Vault" }));
    return await screen.findByRole("treeitem", { name: "readme.md" });
  }

  beforeEach(() => {
    resetPanelsStoreForTest();
  });

  it("sets the active panel's target on a single click, without growing the list", async () => {
    const row = await fileRow();

    await click(row);

    expect(panelsStore.getState().panels).toHaveLength(1);
    expect(activePanel(panelsStore.getState()).target).toEqual({
      kind: "file",
      profileId: "01VAULT",
      relativePath: "readme.md",
    });
  });

  it("appends a panel on a double click, keeping what was open", async () => {
    const row = await fileRow();
    const other = screen.getByRole("treeitem", { name: "notes.md" });
    await click(other);
    panelsStore.getState().openPanel({
      kind: "file",
      profileId: "01VAULT",
      relativePath: "notes.md",
    });

    // The whole gesture as the DOM delivers it: `click` fires before
    // `dblclick`, and the pane must survive that rather than assume the
    // double click arrives alone.
    await act(async () => {
      fireEvent.click(row);
      fireEvent.doubleClick(row);
      await Promise.resolve();
    });

    expect(panelsStore.getState().panels.map((panel) => panel.target)).toEqual([
      { kind: "file", profileId: "01VAULT", relativePath: "notes.md" },
      { kind: "file", profileId: "01VAULT", relativePath: "readme.md" },
    ]);
  });

  it("leaves the panel alone for a modifier click, which belongs to the selection", async () => {
    const row = await fileRow();

    await act(async () => {
      fireEvent.click(row, { metaKey: true });
      fireEvent.click(row, { shiftKey: true });
      fireEvent.click(row, { ctrlKey: true });
      await Promise.resolve();
    });

    // Somebody assembling a selection to delete does not want three panels.
    expect(activePanel(panelsStore.getState()).target).toBeNull();
  });

  it("leaves the panel alone when the click was on one of the row's own controls", async () => {
    const row = await fileRow();

    await click(within(row).getByRole("button", { name: FILES_REVEAL_LABEL }));

    // Reveal bubbles to the row. Without the guard every action button in the
    // tree would also be an open-this-file button.
    expect(activePanel(panelsStore.getState()).target).toBeNull();
  });

  it("does not make a folder a panel target", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(listed("01VAULT", "", [entry("Notes", "folder")]));
    render(<FilesPane />);
    const root = await screen.findByRole("treeitem", { name: "Vault" });
    await click(expander(root));
    const folder = await screen.findByRole("treeitem", { name: "Notes" });

    await click(folder);

    // A folder's click is expand/collapse. Opening a panel on one would replace
    // whatever the reader had beside the tree with nothing they asked for.
    expect(activePanel(panelsStore.getState()).target).toBeNull();
  });
});

/**
 * Story 46.13, FR-215 — a file row has a menu, and the three "open"s are named.
 *
 * The pane had three verbs and one label. A single click replaced what the active
 * panel showed, a double click opened a second panel, and the row's own button
 * handed the file to the operating system — and that last one, the only one with
 * a name, was called `Open`. Two of the three were undiscoverable and the third
 * did not say which it was.
 *
 * These tests are about the wording as much as the wiring: each item must do its
 * own distinct thing, and no two of them may be confusable with each other.
 */
describe("FilesPane — the row's context menu", () => {
  async function fileRow(): Promise<HTMLElement> {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", [entry("readme.md", "file"), entry("notes.md", "file")]),
    );
    render(<FilesPane />);
    const root = await screen.findByRole("treeitem", { name: "Vault" });
    await click(within(root).getByRole("button", { name: "Vault" }));
    return await screen.findByRole("treeitem", { name: "readme.md" });
  }

  /** Right-click a row and let Radix mount the menu. */
  async function openMenu(row: HTMLElement): Promise<HTMLElement> {
    await act(async () => {
      fireEvent.contextMenu(row);
      await Promise.resolve();
    });
    return await screen.findByRole("menu");
  }

  beforeEach(() => {
    resetPanelsStoreForTest();
  });

  it("holds every verb the row has, worded apart", async () => {
    const menu = await openMenu(await fileRow());

    const items = within(menu)
      .getAllByRole("menuitem")
      .map((item) => item.textContent);
    // All five, and in one order at every width: the row shows as many of its
    // verbs as it has pixels for, so a menu that dropped the promoted ones would
    // make a verb unreachable on a narrow column and would also change places as
    // the seam was dragged.
    expect(items).toEqual([
      FILES_OPEN_HERE_LABEL,
      FILES_OPEN_BESIDE_LABEL,
      FILES_OPEN_LABEL,
      FILES_REVEAL_LABEL,
      FILES_COPY_PATH_LABEL,
    ]);
    // Worded apart is the deliverable, so it is asserted rather than assumed:
    // distinct strings, none of which is a prefix of another. A reader who has to
    // guess which `Open` they pressed has not been told anything.
    expect(new Set(items).size).toBe(items.length);
    for (const one of items) {
      for (const other of items) {
        if (one !== other) {
          expect(one?.startsWith(`${other} `)).toBe(false);
        }
      }
    }
  });

  it("replaces the active panel from the first item, without growing the list", async () => {
    const row = await fileRow();
    // Something ELSE already open first, and this is load-bearing rather than
    // scene-setting: `openPanel` on an empty active panel fills it instead of
    // appending, so against a fresh keeper the two verbs are indistinguishable
    // and a test that started there would pass for either of them.
    await click(screen.getByRole("treeitem", { name: "notes.md" }));
    expect(panelsStore.getState().panels).toHaveLength(1);

    const menu = await openMenu(row);
    await click(within(menu).getByRole("menuitem", { name: FILES_OPEN_HERE_LABEL }));

    // One panel, and what it holds was REPLACED. That is the whole difference
    // between this item and the one below it.
    expect(panelsStore.getState().panels.map((panel) => panel.target)).toEqual([
      { kind: "file", profileId: "01VAULT", relativePath: "readme.md" },
    ]);
    expect(syncOpenEntry).not.toHaveBeenCalled();
  });

  it("opens a second panel from the second item, keeping what was open", async () => {
    const row = await fileRow();
    // Something already open, so "a new panel" has something to be new beside.
    await click(screen.getByRole("treeitem", { name: "notes.md" }));

    const menu = await openMenu(row);
    await click(within(menu).getByRole("menuitem", { name: FILES_OPEN_BESIDE_LABEL }));

    expect(panelsStore.getState().panels.map((panel) => panel.target)).toEqual([
      { kind: "file", profileId: "01VAULT", relativePath: "notes.md" },
      { kind: "file", profileId: "01VAULT", relativePath: "readme.md" },
    ]);
    expect(syncOpenEntry).not.toHaveBeenCalled();
  });

  it("leaves keeper entirely from the third item, and opens no panel at all", async () => {
    const menu = await openMenu(await fileRow());

    await click(within(menu).getByRole("menuitem", { name: FILES_OPEN_LABEL }));

    // The one verb that is not about panels. Rust gets the profile id and the
    // profile-relative subpath, never an absolute path (AD-65, FR-145).
    expect(syncOpenEntry).toHaveBeenCalledWith("01VAULT", "readme.md");
    expect(activePanel(panelsStore.getState()).target).toBeNull();
  });

  it("names a promoted control by the whole verb its menu item spells", async () => {
    const row = await fileRow();

    // The word is off the surface — the control is its glyph — and nowhere else.
    // A reader driving keeper by voice or by screen reader gets the whole verb,
    // and the pointer gets it as a tooltip. An icon with no name is a control
    // nobody can ask for (WCAG 2.5.3).
    const button = within(row).getByRole("button", { name: FILES_OPEN_LABEL });
    expect(button).toHaveAttribute("title", FILES_OPEN_LABEL);
    expect(button).toHaveTextContent("");
  });

  it("offers a folder its own two verbs and none of the three panel ones", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(listed("01VAULT", "", [entry("Notes", "folder")]));
    render(<FilesPane />);
    const root = await screen.findByRole("treeitem", { name: "Vault" });
    await click(expander(root));
    const folder = await screen.findByRole("treeitem", { name: "Notes" });

    // A folder is not a panel target, so none of the three ways to open a file
    // is offered on one. It still has a path to reveal and a path to copy, and
    // those are the two the row is narrowest for — a folder with no menu was a
    // folder whose only verbs vanished with the column's width.
    expect(
      within(await openMenu(folder))
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual([FILES_REVEAL_LABEL, FILES_COPY_PATH_LABEL]);
  });

  it("offers no menu on a profile root either", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(listed("01VAULT", "", [entry("readme.md", "file")]));
    render(<FilesPane />);
    const root = await screen.findByRole("treeitem", { name: "Vault" });

    await act(async () => {
      fireEvent.contextMenu(root);
      await Promise.resolve();
    });

    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("opens the same menu on a phone-tier long press, not a second one", async () => {
    // The house pattern's other half. `useLongPress` dispatches a synthetic
    // `contextmenu` at the press point after 500ms stationary, which is the event
    // the Radix trigger already listens for — so there is one menu and one visual
    // language, and this test exists to prove the bridge is actually wired to the
    // row rather than merely imported.
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
      const row = await fileRow();

      // Fake timers only around the hold: the queries above and below poll on
      // real ones, which is why the pins-strip suite switches back before it
      // asserts rather than mocking the clock for the whole test.
      vi.useFakeTimers();
      fireEvent.pointerDown(row, { pointerId: 1, clientX: 30, clientY: 30 });
      act(() => {
        vi.advanceTimersByTime(500);
      });
      vi.useRealTimers();

      const menu = await screen.findByRole("menu");
      expect(
        within(menu)
          .getAllByRole("menuitem")
          .map((item) => item.textContent),
      ).toEqual([
        FILES_OPEN_HERE_LABEL,
        FILES_OPEN_BESIDE_LABEL,
        FILES_OPEN_LABEL,
        FILES_REVEAL_LABEL,
        FILES_COPY_PATH_LABEL,
      ]);
    } finally {
      vi.useRealTimers();
      window.matchMedia = originalMatchMedia;
    }
  });
});

/**
 * Story 45.3 — the files surface can write.
 *
 * AD-75 said it never could. AD-89 retired that, deliberately and by the owner,
 * and these are the assertions that hold what replaced it: delete acts on the
 * selection, the confirmation names one file and counts many, a create makes a
 * file that then appears in a listing, and a location keeper will not write
 * says why rather than offering an action that fails.
 *
 * Every sentence the confirmation shows is Rust's and arrives through the
 * mocked client verbatim, which is the point — nothing below asserts a sentence
 * this file composed.
 */
describe("FilesPane — the write path", () => {
  /** A vault with three files in it, expanded, ready to select in. */
  async function vaultWithFiles(): Promise<void> {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", [entry("a.md", "file"), entry("b.md", "file"), entry("c.md", "file")]),
    );
    render(<FilesPane />);
    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    await screen.findByRole("treeitem", { name: "a.md" });
  }

  it("selects one row on a plain click and replaces the selection on the next", async () => {
    await vaultWithFiles();

    await click(screen.getByRole("treeitem", { name: "a.md" }));
    expect(screen.getByRole("treeitem", { name: "a.md" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("treeitem", { name: "b.md" })).toHaveAttribute(
      "aria-selected",
      "false",
    );

    await click(screen.getByRole("treeitem", { name: "b.md" }));
    // Replaced, not accumulated: a plain click is not a multiselect gesture,
    // and a browser that accumulated them would delete the file you looked at
    // five minutes ago.
    expect(screen.getByRole("treeitem", { name: "a.md" })).toHaveAttribute(
      "aria-selected",
      "false",
    );
    expect(screen.getByRole("treeitem", { name: "b.md" })).toHaveAttribute("aria-selected", "true");
  });

  it("extends the selection with Cmd-click and takes the run with Shift-click", async () => {
    await vaultWithFiles();

    await click(screen.getByRole("treeitem", { name: "a.md" }));
    await act(async () => {
      fireEvent.click(screen.getByRole("treeitem", { name: "c.md" }), { metaKey: true });
      await Promise.resolve();
    });
    expect(screen.getByTestId(FILES_SELECTED_TESTID)).toHaveTextContent("2 items selected");
    // The middle row was NOT taken: Cmd adds one, it does not fill the gap.
    expect(screen.getByRole("treeitem", { name: "b.md" })).toHaveAttribute(
      "aria-selected",
      "false",
    );

    await click(screen.getByRole("treeitem", { name: "a.md" }));
    await act(async () => {
      fireEvent.click(screen.getByRole("treeitem", { name: "c.md" }), { shiftKey: true });
      await Promise.resolve();
    });
    // Shift fills it, because a run is what a person sees between two rows.
    expect(screen.getByRole("treeitem", { name: "b.md" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId(FILES_SELECTED_TESTID)).toHaveTextContent("3 items selected");
  });

  it("treats Ctrl-click as Cmd-click, because one of them is the wrong platform", async () => {
    // Asserted on its own rather than folded into the test above: three
    // modifier clicks inside one `act` cannot tell you which modifier the
    // handler honoured, and jsdom reports a non-Mac platform.
    await vaultWithFiles();

    await click(screen.getByRole("treeitem", { name: "a.md" }));
    await act(async () => {
      fireEvent.click(screen.getByRole("treeitem", { name: "b.md" }), { ctrlKey: true });
      await Promise.resolve();
    });

    expect(screen.getByTestId(FILES_SELECTED_TESTID)).toHaveTextContent("2 items selected");
  });

  it("counts the selection into the delete request and shows Rust's counted question", async () => {
    await vaultWithFiles();
    syncDeletePlan.mockResolvedValue({
      files: ["a.md", "b.md"],
      question: "Delete 2 files?",
      consequence:
        "These 2 files sync, so deleting them here removes them from every machine that syncs Vault.",
      recovery: "keeper moves them into the vault's trash rather than erasing them.",
      refusals: [],
    });

    await click(screen.getByRole("treeitem", { name: "a.md" }));
    await act(async () => {
      fireEvent.click(screen.getByRole("treeitem", { name: "b.md" }), { metaKey: true });
      await Promise.resolve();
    });
    await click(screen.getByRole("button", { name: FILES_DELETE_LABEL }));

    // The whole selection went down, in the tree's order.
    expect(syncDeletePlan).toHaveBeenCalledWith("01VAULT", ["a.md", "b.md"]);
    const dialog = await screen.findByRole("alertdialog");
    expect(within(dialog).getByText("Delete 2 files?")).toBeInTheDocument();
    // Whether they sync is Rust's sentence, rendered verbatim.
    expect(within(dialog).getByTestId(FILES_CONFIRM_TESTID)).toHaveTextContent(
      "These 2 files sync, so deleting them here removes them from every machine that syncs Vault.",
    );
    // And every file is named, not merely counted.
    expect(within(dialog).getByText("a.md")).toBeInTheDocument();
    expect(within(dialog).getByText("b.md")).toBeInTheDocument();
  });

  it("removes every file in the multiselection and re-reads the folder they were in", async () => {
    await vaultWithFiles();
    syncDeletePlan.mockResolvedValue({
      files: ["a.md", "b.md"],
      question: "Delete 2 files?",
      consequence: "These 2 files sync.",
      recovery: "keeper moves them into the vault's trash.",
      refusals: [],
    });
    syncDeleteEntries.mockResolvedValue({ deleted: ["a.md", "b.md"], refusals: [] });

    await click(screen.getByRole("treeitem", { name: "a.md" }));
    await act(async () => {
      fireEvent.click(screen.getByRole("treeitem", { name: "b.md" }), { metaKey: true });
      await Promise.resolve();
    });
    await click(screen.getByRole("button", { name: FILES_DELETE_LABEL }));
    syncBrowse.mockClear();
    await click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: FILES_DELETE_LABEL,
      }),
    );

    // Rust's own file list, not the pane's — the plan is the authority on what
    // the confirmation was about.
    expect(syncDeleteEntries).toHaveBeenCalledWith("01VAULT", ["a.md", "b.md"]);
    // The change is visible without a manual Refresh: the folder they were in
    // is re-read, and only that folder.
    await waitFor(() => expect(syncBrowse).toHaveBeenCalledWith("01VAULT", ""));
    expect(syncBrowse).toHaveBeenCalledTimes(1);
    // Nothing stays selected: the rows are gone.
    await waitFor(() => expect(screen.queryByTestId(FILES_SELECTED_TESTID)).toBeNull());
  });

  it("names what it could not delete rather than shrinking the selection in silence", async () => {
    await vaultWithFiles();
    syncDeletePlan.mockResolvedValue({
      files: ["a.md"],
      question: "Delete a.md?",
      consequence: "This file syncs.",
      recovery: "keeper moves it into the vault's trash.",
      refusals: [],
    });
    syncDeleteEntries.mockResolvedValue({
      deleted: [],
      refusals: [
        {
          relativePath: "a.md",
          reason: "a.md is no longer in this folder, so nothing was deleted.",
        },
      ],
    });

    await click(screen.getByRole("treeitem", { name: "a.md" }));
    await click(screen.getByRole("button", { name: FILES_DELETE_LABEL }));
    await click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: FILES_DELETE_LABEL,
      }),
    );

    expect(await screen.findByTestId(FILES_WRITE_ERROR_TESTID)).toHaveTextContent(
      "a.md is no longer in this folder, so nothing was deleted.",
    );
  });

  it("creates a file in the folder it was asked for and re-reads that folder", async () => {
    await vaultWithFiles();
    syncCreateEntry.mockResolvedValue("notes.md");

    await click(screen.getByRole("button", { name: FILES_NEW_FILE_LABEL }));
    const field = screen.getByRole("textbox", { name: FILES_NEW_FILE_NAME_LABEL });
    fireEvent.change(field, { target: { value: "notes.md" } });
    syncBrowse.mockClear();
    // The listing the new file appears in.
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", [entry("a.md", "file"), entry("notes.md", "file")]),
    );
    await click(screen.getByRole("button", { name: FILES_CREATE_LABEL }));

    // The directory and the name cross separately — nothing here joins a path
    // (AD-65).
    expect(syncCreateEntry).toHaveBeenCalledWith("01VAULT", "", "notes.md");
    expect(await screen.findByRole("treeitem", { name: "notes.md" })).toBeInTheDocument();
    // The field is gone once the file exists.
    expect(screen.queryByRole("textbox", { name: FILES_NEW_FILE_NAME_LABEL })).toBeNull();
  });

  it("keeps the name on screen and shows Rust's sentence when the name collides", async () => {
    await vaultWithFiles();
    syncCreateEntry.mockRejectedValue({
      code: "internal",
      message:
        '"a.md" is already in this folder. Pick another name — keeper will not write over a file you did not name.',
      retriable: false,
    });

    await click(screen.getByRole("button", { name: FILES_NEW_FILE_LABEL }));
    fireEvent.change(screen.getByRole("textbox", { name: FILES_NEW_FILE_NAME_LABEL }), {
      target: { value: "a.md" },
    });
    syncBrowse.mockClear();
    await click(screen.getByRole("button", { name: FILES_CREATE_LABEL }));

    expect(await screen.findByTestId(FILES_WRITE_ERROR_TESTID)).toHaveTextContent(
      "is already in this folder",
    );
    // A refused name is a name to edit: the field stays, still holding it, so
    // the part that was fine does not have to be retyped.
    expect(screen.getByRole("textbox", { name: FILES_NEW_FILE_NAME_LABEL })).toHaveValue("a.md");
    // And nothing was re-read, because nothing changed on disk.
    expect(syncBrowse).not.toHaveBeenCalled();
  });

  it("offers no New file in a folder Rust says it cannot write to", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01FIELD", name: "Field" })]);
    syncBrowse.mockResolvedValue(
      listed("01FIELD", "", [readOnly("clip.mov")], null, {
        writable: false,
        reason: "Field holds no notes vault, so keeper will not change files in it.",
        caveat: null,
      }),
    );
    render(<FilesPane />);
    await click(expander(await screen.findByRole("treeitem", { name: "Field" })));
    await screen.findByRole("treeitem", { name: "clip.mov" });

    expect(screen.queryByRole("button", { name: FILES_NEW_FILE_LABEL })).toBeNull();
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(syncCreateEntry).not.toHaveBeenCalled();
  });

  it("asks to delete the focused row on the Delete key, and never deletes without asking", async () => {
    await vaultWithFiles();
    syncDeletePlan.mockResolvedValue({
      files: ["a.md"],
      question: "Delete a.md?",
      consequence: "This file syncs.",
      recovery: "keeper moves it into the vault's trash.",
      refusals: [],
    });

    const row = screen.getByRole("treeitem", { name: "a.md" });
    await act(async () => {
      fireEvent.keyDown(row, { key: "Delete" });
      await Promise.resolve();
    });

    expect(syncDeletePlan).toHaveBeenCalledWith("01VAULT", ["a.md"]);
    // A keystroke opens the confirmation. There is no key in this pane that
    // removes a file without Rust naming it first.
    expect(syncDeleteEntries).not.toHaveBeenCalled();
    expect(await screen.findByRole("alertdialog")).toBeInTheDocument();
  });

  it("clears the selection on Escape", async () => {
    await vaultWithFiles();

    await click(screen.getByRole("treeitem", { name: "a.md" }));
    expect(screen.getByTestId(FILES_SELECTED_TESTID)).toBeInTheDocument();

    await act(async () => {
      fireEvent.keyDown(screen.getByRole("treeitem", { name: "a.md" }), { key: "Escape" });
      await Promise.resolve();
    });
    expect(screen.queryByTestId(FILES_SELECTED_TESTID)).toBeNull();
  });
});

/**
 * Story 45.13's entry point into this pane.
 *
 * The control hangs off the SAME selection Delete does — Story 45.3's, the only
 * one this pane has — so these tests select with the same gestures the write
 * -path tests above use and then assert on the header. What is deliberately not
 * asserted here is what gets written into the note: that is
 * `attach-entry-points.test.tsx`'s job, where it is compared against the other
 * two entry points rather than against a literal.
 */
describe("FilesPane — attaching a selection to a note", () => {
  beforeEach(() => {
    notesAttachTargets.mockResolvedValue([
      { id: "n1", title: "Standup", path: "notes/standup.md", holds: [] },
    ]);
    notesAttachSources.mockResolvedValue([
      { name: "a.md", relPath: "a.md", copied: false, refusal: null },
    ]);
    notesBodyRead.mockResolvedValue({ rev: "r0", text: "intro\n" });
    notesBodyWrite.mockResolvedValue({
      rev: "r1",
      path: "notes/standup.md",
      frontmatter: "",
      conflictCopy: null,
    });
    notesVaultsStore.setState({ activeVaultId: "v1" });
  });

  afterEach(() => {
    notesVaultsStore.setState({ activeVaultId: null });
  });

  /** A vault with a file and a folder in it, expanded. */
  async function vaultWithBoth(): Promise<void> {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", [entry("a.md", "file"), entry("Photos", "folder")]),
    );
    render(<FilesPane />);
    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    await screen.findByRole("treeitem", { name: "a.md" });
  }

  it("offers the control once files are selected, and not before", async () => {
    await vaultWithBoth();
    expect(screen.queryByRole("button", { name: ATTACH_TO_NOTE_LABEL })).toBeNull();

    await click(screen.getByRole("treeitem", { name: "a.md" }));

    expect(screen.getByRole("button", { name: ATTACH_TO_NOTE_LABEL })).toBeInTheDocument();
  });

  /**
   * A folder is not an attachment — there is no element for a directory — so a
   * selection of only folders offers nothing rather than offering a control
   * that would be refused.
   */
  it("offers nothing for a selection of folders", async () => {
    await vaultWithBoth();

    await click(screen.getByRole("treeitem", { name: "Photos" }));

    expect(screen.getByTestId(FILES_SELECTED_TESTID)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: ATTACH_TO_NOTE_LABEL })).toBeNull();
  });

  /**
   * A note lives in the open vault. With no vault open there is nowhere for the
   * file to go, and a control that opened a chooser over nothing would be a
   * control that lies.
   */
  it("offers nothing when no vault is open", async () => {
    notesVaultsStore.setState({ activeVaultId: null });
    await vaultWithBoth();

    await click(screen.getByRole("treeitem", { name: "a.md" }));

    expect(screen.getByTestId(FILES_SELECTED_TESTID)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: ATTACH_TO_NOTE_LABEL })).toBeNull();
  });

  /**
   * A read-only file is still a perfectly good thing to put in a note. The
   * write verdict is about changing the FILE, and attaching changes the note.
   */
  it("offers the control for a file keeper may not write", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", [
        entry("report.pdf", "file", undefined, undefined, {
          write: { writable: false, reason: OUTSIDE_VAULT, caveat: null },
        }),
      ]),
    );
    render(<FilesPane />);
    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    await click(await screen.findByRole("treeitem", { name: "report.pdf" }));

    // No Delete — the location said no — but Attach, because the note is what
    // changes.
    expect(screen.queryByRole("button", { name: FILES_DELETE_LABEL })).toBeNull();
    expect(screen.getByRole("button", { name: ATTACH_TO_NOTE_LABEL })).toBeInTheDocument();
  });

  /** The chooser is handed the selection's absolute paths, which is what Rust
   *  resolves; the webview never turns one into a vault-relative path (AD-65). */
  it("opens the chooser over exactly the files selected", async () => {
    await vaultWithBoth();
    await click(screen.getByRole("treeitem", { name: "a.md" }));

    await click(screen.getByRole("button", { name: ATTACH_TO_NOTE_LABEL }));

    await waitFor(() => {
      expect(notesAttachTargets).toHaveBeenCalledWith("v1", "", ["a.md"]);
    });
    expect(await screen.findByRole("button", { name: "Attach to Standup" })).toBeInTheDocument();
  });

  /**
   * The seam between this pane and Rust, which nothing else covers.
   *
   * A mutation swapping `absolutePath` for `relativePath` in the pane's
   * derivation survived the whole sweep: the chooser's search is keyed on file
   * NAMES, and `a.md` is the basename of both spellings, so every existing
   * assertion passed. The consequence would not have been subtle —
   * `notes_attach_sources` calls `std::fs::metadata` on what it is handed, so a
   * profile-relative path resolves against the process working directory and
   * every attach from this pane refuses with "keeper could not read…" — it was
   * simply invisible until the click reached the command that consumes it.
   *
   * So this test presses Attach rather than stopping at the offer. AD-65 in the
   * direction that matters: the webview hands over the path the shell gave it
   * and composes nothing.
   */
  it("hands Rust the absolute path, not the one the tree renders", async () => {
    await vaultWithBoth();
    await click(screen.getByRole("treeitem", { name: "a.md" }));
    await click(screen.getByRole("button", { name: ATTACH_TO_NOTE_LABEL }));

    await click(await screen.findByRole("button", { name: "Attach to Standup" }));

    await waitFor(() => {
      expect(notesAttachSources).toHaveBeenCalledWith("v1", ["/Users/alice/Vault/a.md"]);
    });
  });

  /**
   * The multiselection promise, through the pane that makes it.
   *
   * Every other test here selects ONE row, and a fixture that cannot tell the
   * right answer from the mutant is a decoration: dropping all but the first
   * selected row survived the entire sweep, because nothing ever attached two.
   * "Select five, attach one, report one" is silent by construction — the
   * person sees a receipt naming a file that did go in.
   *
   * Two files AND a folder in the selection, so the same test pins the folder
   * rule at the same time: three rows selected, two paths sent.
   */
  it("sends every selected file and no folder, in the tree's own order", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", [
        entry("a.md", "file"),
        entry("Photos", "folder"),
        entry("b.md", "file"),
      ]),
    );
    notesAttachSources.mockResolvedValue([
      { name: "a.md", relPath: "a.md", copied: false, refusal: null },
      { name: "b.md", relPath: "b.md", copied: false, refusal: null },
    ]);
    render(<FilesPane />);
    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    await screen.findByRole("treeitem", { name: "a.md" });

    // Shift takes the run, which is how a person selects three adjacent rows.
    await click(screen.getByRole("treeitem", { name: "a.md" }));
    await act(async () => {
      fireEvent.click(screen.getByRole("treeitem", { name: "b.md" }), { shiftKey: true });
      await Promise.resolve();
    });

    await click(screen.getByRole("button", { name: ATTACH_TO_NOTE_LABEL }));
    await click(await screen.findByRole("button", { name: "Attach to Standup" }));

    await waitFor(() => {
      expect(notesAttachSources).toHaveBeenCalledWith("v1", [
        "/Users/alice/Vault/a.md",
        "/Users/alice/Vault/b.md",
      ]);
    });
  });
});

/**
 * Story 46.3 — the tree stays where you left it.
 *
 * The reported defect was that leaving the Files surface and coming back found
 * every folder shut. `AppShell` renders this pane conditionally on the primary
 * view, so looking at anything else unmounts it, and the expansion was
 * `useState`. Every test here is about what survives that and what deliberately
 * does not.
 */
describe("FilesPane — the tree stays where you left it", () => {
  /** What the last run left behind: a real cookie, hydrated the way `AppShell`
   *  does it, so these tests exercise the restore rather than the store. */
  function remembered(keys: readonly string[]): void {
    // biome-ignore lint/suspicious/noDocumentCookie: standing in for the write the last run made
    document.cookie = filesTreeCookie(new Set(keys));
    hydrateFilesTree(document.cookie);
  }

  /** One profile with a folder and a file in it, and a subfolder under that. */
  function vaultTree(): void {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockImplementation((_id: string, subpath: string) =>
      Promise.resolve(
        subpath === ""
          ? listed("01VAULT", "", [entry("Notes", "folder"), entry("readme.md", "file")])
          : listed("01VAULT", subpath, [entry("2026", "folder", `${subpath}/2026`)]),
      ),
    );
  }

  it("comes back open after the surface unmounts it", async () => {
    // The bug, exactly as reported: expand, leave Files, come back.
    vaultTree();
    const files = render(<FilesPane />);
    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    expect(await screen.findByRole("treeitem", { name: "Notes" })).toBeInTheDocument();

    // Looking at Notes, or Sync, or anything else. There is no "hide" here —
    // the shell genuinely unmounts the pane.
    files.unmount();
    render(<FilesPane />);

    expect(await screen.findByRole("treeitem", { name: "Notes" })).toBeInTheDocument();
  });

  it("re-reads the folder rather than restoring what it held", async () => {
    // The listings are a cache of a disk keeper has not looked at since. They
    // stay component state on purpose, so the second mount asks again.
    vaultTree();
    const files = render(<FilesPane />);
    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    await screen.findByRole("treeitem", { name: "Notes" });

    files.unmount();
    syncBrowse.mockClear();
    render(<FilesPane />);

    await waitFor(() => expect(syncBrowse).toHaveBeenCalledWith("01VAULT", ""));
  });

  it("opens the folders the last run left open", async () => {
    vaultTree();
    remembered([nodeKey("01VAULT", ""), nodeKey("01VAULT", "Notes")]);
    render(<FilesPane />);

    // Two levels deep, from a cookie, without a click.
    expect(await screen.findByRole("treeitem", { name: "2026" })).toBeInTheDocument();
    expect(syncBrowse).toHaveBeenCalledWith("01VAULT", "");
    expect(syncBrowse).toHaveBeenCalledWith("01VAULT", "Notes");
  });

  it("drops a remembered folder whose profile is gone, and says nothing about it", async () => {
    vaultTree();
    remembered([nodeKey("01VAULT", ""), nodeKey("01GONE", ""), nodeKey("01GONE", "a")]);
    render(<FilesPane />);

    expect(await screen.findByRole("treeitem", { name: "Notes" })).toBeInTheDocument();
    // Nothing was asked of a folder keeper has forgotten...
    await waitFor(() => expect(syncBrowse).toHaveBeenCalledTimes(1));
    expect(syncBrowse).toHaveBeenCalledWith("01VAULT", "");
    // ...nothing on screen mentions it — there is nothing the reader could do
    // about a profile that no longer exists...
    expect(screen.queryByText(/01GONE/)).toBeNull();
    // ...and it is out of the cookie, or it would be a key nothing can clear.
    expect(readFilesTree(document.cookie)).toEqual(new Set([nodeKey("01VAULT", "")]));
  });

  it("asks only for the remembered folders that will be on screen", async () => {
    // `Notes` is shut, so `Notes/2026` renders nowhere. Keeping it costs
    // nothing; browsing for it would be an IPC call for a row with no home.
    vaultTree();
    remembered([nodeKey("01VAULT", ""), nodeKey("01VAULT", "Notes/2026")]);
    render(<FilesPane />);

    expect(await screen.findByRole("treeitem", { name: "Notes" })).toBeInTheDocument();
    await waitFor(() => expect(syncBrowse).toHaveBeenCalledTimes(1));
    expect(syncBrowse).toHaveBeenCalledWith("01VAULT", "");
  });

  it("does not browse a paused folder, and does not forget it either", async () => {
    syncProfiles.mockResolvedValue([
      profile({ id: "01VAULT", name: "Vault" }),
      profile({ id: "01OLD", name: "Old Archive", enabled: false }),
    ]);
    syncBrowse.mockResolvedValue(listed("01VAULT", "", [entry("Notes", "folder")]));
    remembered([nodeKey("01VAULT", ""), nodeKey("01OLD", "")]);
    render(<FilesPane />);

    expect(await screen.findByRole("treeitem", { name: "Notes" })).toBeInTheDocument();
    // A paused folder is one keeper is not watching; this pane does not list it.
    await waitFor(() => expect(syncBrowse).toHaveBeenCalledTimes(1));
    expect(syncBrowse).not.toHaveBeenCalledWith("01OLD", "");
    // But pausing is not deleting, so its expansion is still remembered.
    expect(readFilesTree(document.cookie).has(nodeKey("01OLD", ""))).toBe(true);
  });

  it("forgets a folder that was shut before the surface was left", async () => {
    vaultTree();
    const files = render(<FilesPane />);
    const vault = await screen.findByRole("treeitem", { name: "Vault" });
    await click(expander(vault));
    await screen.findByRole("treeitem", { name: "Notes" });
    await click(expander(screen.getByRole("treeitem", { name: "Vault" })));

    files.unmount();
    render(<FilesPane />);

    await screen.findByRole("treeitem", { name: "Vault" });
    await waitFor(() => expect(screen.queryByRole("treeitem", { name: "Notes" })).toBeNull());
  });

  it("does not forget the tree because the profile list could not be read", async () => {
    // A folder keeper could not ask about has not been deleted. The pane
    // renders the same empty surface either way, so the expansion is the one
    // place the difference can do damage — and forgetting it over a sync engine
    // that was briefly unreachable would be worse than the bug being fixed.
    syncProfiles.mockRejectedValue(new Error("the sync engine is not running"));
    remembered([nodeKey("01VAULT", ""), nodeKey("01VAULT", "Notes")]);
    render(<FilesPane />);

    expect(await screen.findByText(FILES_NO_PROFILES_SENTENCE)).toBeInTheDocument();
    const kept = new Set([nodeKey("01VAULT", ""), nodeKey("01VAULT", "Notes")]);
    expect(filesTreeStore.getState().expanded).toEqual(kept);
    expect(readFilesTree(document.cookie)).toEqual(kept);
  });

  it("restores on the first list it really gets, even when the first call failed", async () => {
    vaultTree();
    // The first call fails; the Refresh behind it succeeds.
    syncProfiles.mockRejectedValueOnce(new Error("the sync engine is not running"));
    remembered([nodeKey("01VAULT", "")]);
    render(<FilesPane />);
    await screen.findByText(FILES_NO_PROFILES_SENTENCE);

    // Refresh is the way back from a surface that could not load, so the
    // restore has to still be waiting when it works.
    await click(screen.getByRole("button", { name: FILES_REFRESH_LABEL }));

    expect(await screen.findByRole("treeitem", { name: "Notes" })).toBeInTheDocument();
  });

  it("waits for a real list before deciding a profile is gone", async () => {
    // The other half of the same flag, and the half `refresh` hides: its own
    // loop re-reads every open folder whatever happens, so the *loads* coming
    // back proves nothing about whether the list was believed. The stale-drop
    // is the only observable that does.
    vaultTree();
    syncProfiles.mockRejectedValueOnce(new Error("the sync engine is not running"));
    remembered([nodeKey("01VAULT", ""), nodeKey("01GONE", "")]);
    render(<FilesPane />);
    await screen.findByText(FILES_NO_PROFILES_SENTENCE);

    // Nothing learned, so nothing forgotten — not even the profile that really
    // has gone. A failed call is not evidence.
    expect(readFilesTree(document.cookie).has(nodeKey("01GONE", ""))).toBe(true);

    await click(screen.getByRole("button", { name: FILES_REFRESH_LABEL }));

    await waitFor(() =>
      expect(readFilesTree(document.cookie)).toEqual(new Set([nodeKey("01VAULT", "")])),
    );
  });

  /**
   * Story 48.6 — the folder is re-read once, not twice.
   *
   * Two behaviours that are each correct met on the one path that runs both.
   * `refresh` re-reads every open folder ("Refresh means ask again"), and the
   * restore effect stays armed until it has a list Rust really answered — so
   * after a failed first call, the Refresh that is the way back from it did
   * BOTH: its own loop browsed every open folder, and then the newly-armed
   * restore browsed every remembered folder again off the same store.
   *
   * Counted per key rather than in total, because the interesting failure is
   * two calls for ONE folder and a total would also move if the fixture grew.
   * On the owner's 91,000-file tree each of those calls is a directory read.
   */
  it("re-reads a remembered folder once when Refresh rescues a failed first list", async () => {
    vaultTree();
    syncProfiles.mockRejectedValueOnce(new Error("the sync engine is not running"));
    remembered([nodeKey("01VAULT", ""), nodeKey("01VAULT", "Notes")]);
    render(<FilesPane />);
    await screen.findByText(FILES_NO_PROFILES_SENTENCE);
    // Nothing has been browsed: there was no list to browse against.
    expect(syncBrowse).not.toHaveBeenCalled();

    await click(screen.getByRole("button", { name: FILES_REFRESH_LABEL }));
    expect(await screen.findByRole("treeitem", { name: "Notes" })).toBeInTheDocument();

    const asked = syncBrowse.mock.calls.map(([id, subpath]) => `${id}:${subpath}`);
    expect(asked.filter((call) => call === "01VAULT:")).toHaveLength(1);
    expect(asked.filter((call) => call === "01VAULT:Notes")).toHaveLength(1);
  });

  it("still re-reads on every later Refresh, because Refresh means ask again", async () => {
    // The other half, and the reason the skip lives in the restore rather than
    // in `load`: a cache check inside `load` would make Refresh a no-op, which
    // is the one thing Refresh must never be.
    vaultTree();
    remembered([nodeKey("01VAULT", "")]);
    render(<FilesPane />);
    await screen.findByRole("treeitem", { name: "Notes" });
    const before = syncBrowse.mock.calls.length;

    await click(screen.getByRole("button", { name: FILES_REFRESH_LABEL }));
    await click(screen.getByRole("button", { name: FILES_REFRESH_LABEL }));

    expect(syncBrowse.mock.calls.length).toBe(before + 2);
  });
});

/**
 * Story 48.1 — the tree is a column of the shell, so it folds and it resizes.
 *
 * Against the real pane, because the defect this story fixes was a general
 * mechanism (`useResizableColumn`, shipped in Story 44.12) wired to exactly one
 * boundary in the whole app. What a folded column does is
 * `surface-column.test.tsx`; that this pane HAS one is only assertable here.
 *
 * The refusals nearby are about a different set and are not overridden: Story
 * 44.12's spec refuses seams INSIDE the tree, between a name and its size. This
 * is the boundary between the tree and the panels beside it.
 */
describe("FilesPane — the tree is a column", () => {
  const label = SURFACE_COLUMNS["files-tree"].label;

  it("folds to a strip that keeps the pane named and the way back reachable", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    render(<FilesPane />);
    expect(await screen.findByRole("treeitem", { name: "Vault" })).toBeInTheDocument();

    await click(screen.getByRole("button", { name: `${COLUMN_COLLAPSE_PREFIX} ${label}` }));

    // No tree and no header: a folded column mounts none of the body. Refresh
    // is the deliberate exception and this assertion changed with the second cut
    // of the story — it used to say Refresh was gone too, which was true and was
    // the defect. It re-reads folders into a store the folded pane is still
    // subscribed to, so it is the one header control that means the same thing at
    // 48px, and it now lives on the strip's rail.
    expect(screen.queryByRole("tree", { name: FILES_TREE_LABEL })).toBeNull();
    expect(screen.getByRole("button", { name: FILES_REFRESH_LABEL })).toHaveAttribute(
      "data-slot",
      COLUMN_RAIL_CONTROL_SLOT,
    );
    expect(screen.queryByRole("heading", { name: FILES_PANE_TITLE })).toBeNull();
    // Still a named region, and still holding the control that undoes it.
    expect(screen.getByRole("region", { name: FILES_PANE_TITLE })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `${COLUMN_EXPAND_PREFIX} ${label}` }),
    ).toBeInTheDocument();
  });

  it("comes back with the tree it had", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    render(<FilesPane />);
    await screen.findByRole("treeitem", { name: "Vault" });

    await click(screen.getByRole("button", { name: `${COLUMN_COLLAPSE_PREFIX} ${label}` }));
    await click(screen.getByRole("button", { name: `${COLUMN_EXPAND_PREFIX} ${label}` }));

    expect(await screen.findByRole("treeitem", { name: "Vault" })).toBeInTheDocument();
  });

  it("carries a seam that remembers the width across a remount", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    const first = render(<FilesPane />);
    await screen.findByRole("treeitem", { name: "Vault" });

    const seam = screen.getByRole("separator", { name: `${COLUMN_RESIZER_LABEL} ${label}` });
    fireEvent.keyDown(seam, { key: "ArrowRight" });
    const wider = SURFACE_COLUMNS["files-tree"].defaultWidth + COLUMN_KEY_STEP;
    expect(readColumnWidths(document.cookie)["files-tree"]).toBe(wider);

    first.unmount();
    render(<FilesPane />);

    expect(await screen.findByRole("region", { name: FILES_PANE_TITLE })).toHaveStyle({
      width: `${wider}px`,
    });
  });

  /**
   * The column's chrome does not move the tree's row window.
   *
   * Written because a sibling agent's gate run saw
   * `FilesPane keyboard navigation › steps down and up one visible row at a
   * time` fail once in six under three concurrent test runs, and the honest
   * question was whether the fold or the seam could make the set of rendered
   * rows disagree with the index the keyboard steps through. It cannot, and
   * this is the assertion rather than the argument: fold, unfold, then step.
   *
   * The argument, for the record, is structural. `useWindowedRows` measures
   * with `clientHeight` and never `getBoundingClientRect`, so `setup.ts`'s
   * full-viewport rect shim does not reach it; a zero `clientHeight` falls back
   * to `ASSUMED_VIEWPORT_HEIGHT` (640), which at a 32px row estimate is a
   * twenty-row window over a two-row tree. And the chrome is a sibling ABOVE
   * the scroll container while the seam is outside the `<section>` entirely,
   * so neither is inside the box whose height is measured.
   */
  it("leaves the tree's keyboard stepping intact across a fold and an unfold", async () => {
    syncProfiles.mockResolvedValue([
      profile({ id: "01VAULT", name: "Vault" }),
      profile({ id: "01FIELD", name: "Field" }),
    ]);
    render(<FilesPane />);
    const tree = await screen.findByRole("tree", { name: FILES_TREE_LABEL });
    expect(within(tree).getAllByRole("treeitem")).toHaveLength(2);

    await click(screen.getByRole("button", { name: `${COLUMN_COLLAPSE_PREFIX} ${label}` }));
    await click(screen.getByRole("button", { name: `${COLUMN_EXPAND_PREFIX} ${label}` }));

    // Both rows are back in the window, and the arrows still walk them.
    const back = await screen.findByRole("tree", { name: FILES_TREE_LABEL });
    expect(within(back).getAllByRole("treeitem")).toHaveLength(2);
    const first = await screen.findByRole("treeitem", { name: "Vault" });
    first.focus();
    await act(async () => {
      fireEvent.keyDown(document.activeElement ?? first, { key: "ArrowDown" });
      await Promise.resolve();
    });
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Field");
  });

  it("re-reads the folders from the folded rail", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    render(<FilesPane />);
    await screen.findByRole("treeitem", { name: "Vault" });
    await click(screen.getByRole("button", { name: `${COLUMN_COLLAPSE_PREFIX} ${label}` }));
    const before = syncProfiles.mock.calls.length;

    await click(screen.getByRole("button", { name: FILES_REFRESH_LABEL }));

    // The whole point of keeping it: the read happens now, and unfolding shows
    // what it found rather than what was there when the column went away.
    expect(syncProfiles.mock.calls.length).toBe(before + 1);
    expect(
      screen.getByRole("button", { name: `${COLUMN_EXPAND_PREFIX} ${label}` }),
    ).toBeInTheDocument();
  });

  /**
   * The selection is the capability a fold destroyed most completely: Delete and
   * Attach are asked about it, they live in the header, and the header goes with
   * the body. Folded, a person had a selection they could not see, could not act
   * on and could not clear.
   */
  it("says how many rows are still selected, and gives them back their header", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", [entry("a.md", "file"), entry("b.md", "file")]),
    );
    render(<FilesPane />);
    await click(expander(await screen.findByRole("treeitem", { name: "Vault" })));
    await click(await screen.findByRole("treeitem", { name: "a.md" }));
    expect(screen.getByTestId(FILES_SELECTED_TESTID)).toHaveTextContent("1 item selected");

    await click(screen.getByRole("button", { name: `${COLUMN_COLLAPSE_PREFIX} ${label}` }));

    const held = screen.getByRole("button", { name: `${FILES_SELECTION_LABEL}, 1 item selected` });
    expect(held).toBeInTheDocument();
    // And it is a way back to what can be done about it, not a Delete at 48px:
    // the count that makes deleting safe to press is in the header.
    await click(held);
    expect(screen.getByRole("button", { name: FILES_DELETE_LABEL })).toBeInTheDocument();
    expect(screen.getByTestId(FILES_SELECTED_TESTID)).toHaveTextContent("1 item selected");
  });

  it("offers no selection control when nothing is selected", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    render(<FilesPane />);
    await screen.findByRole("treeitem", { name: "Vault" });

    await click(screen.getByRole("button", { name: `${COLUMN_COLLAPSE_PREFIX} ${label}` }));

    expect(screen.queryByRole("button", { name: new RegExp(FILES_SELECTION_LABEL) })).toBeNull();
  });
});
