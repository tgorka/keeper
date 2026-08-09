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
const revealPath = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  syncProfiles: () => syncProfiles(),
  syncBrowse: (id: unknown, subpath: unknown) => syncBrowse(id, subpath),
  syncOpenEntry: (id: unknown, subpath: unknown) => syncOpenEntry(id, subpath),
  revealPath: (path: unknown) => revealPath(path),
}));

import {
  FILES_ALL_PAUSED_SENTENCE,
  FILES_COPY_PATH_LABEL,
  FILES_EMPTY_FOLDER_SENTENCE,
  FILES_NAME_LABEL,
  FILES_NO_PROFILES_SENTENCE,
  FILES_OPEN_LABEL,
  FILES_PANE_TITLE,
  FILES_REFRESH_LABEL,
  FILES_REVEAL_LABEL,
  FILES_STATE_DETAIL_TESTID,
  FILES_TREE_LABEL,
  FILES_WRITE_CONTROL_LABELS,
  FilesPane,
} from "@/components/layout/files-pane";
import {
  FILES_SYNC_MARK_LABEL,
  FILES_SYNC_MARK_TESTID,
} from "@/components/layout/sync-status-mark";
import { OVERFLOW_PANEL_LABEL, OVERFLOW_TRIGGER_LABEL } from "@/components/ui/overflow-value";
import { WINDOW_ROW_ATTR, WINDOW_VIEWPORT_ATTR } from "@/components/ui/window-list";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { type ListGeometry, withListGeometry, withTextLayout } from "@/test/layout";

/** The exact sentence Rust composes for an unplugged profile. Verbatim, because
 * the whole point of the state is that this reaches the screen unaltered. */
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
 * every test that is not about the mark keeps reading as before. */
function entry(
  name: string,
  kind: FilesEntryVm["kind"],
  relativePath?: string,
  sync: FilesEntrySyncVm = { status: "synced", detail: null },
): FilesEntryVm {
  const rel = relativePath ?? name;
  return { name, relativePath: rel, absolutePath: `/Users/alice/Vault/${rel}`, kind, sync };
}

function listed(
  profileId: string,
  subpath: string,
  entries: FilesEntryVm[],
  detail: string | null = null,
): FilesListingVm {
  return { profileId, subpath, state: "listed", entries, detail, truncated: detail !== null };
}

function notListed(
  profileId: string,
  state: FilesListingVm["state"],
  detail: string,
): FilesListingVm {
  return { profileId, subpath: "", state, entries: null, detail, truncated: false };
}

beforeEach(() => {
  syncProfiles.mockReset();
  syncProfiles.mockResolvedValue([]);
  syncBrowse.mockReset();
  syncBrowse.mockResolvedValue(listed("01VAULT", "", []));
  syncOpenEntry.mockReset();
  syncOpenEntry.mockResolvedValue(undefined);
  revealPath.mockReset();
  revealPath.mockResolvedValue(undefined);
  capabilitiesStore.getState().applySnapshot({
    ...DEFAULT_CAPABILITIES,
    sync: true,
    revealInFileManager: true,
  });
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

    await click(within(row).getByRole("button"));
    await waitFor(() => expect(syncBrowse).toHaveBeenCalledWith("01VAULT", ""));
    expect(await screen.findByRole("treeitem", { name: "Notes" })).toBeInTheDocument();
    expect(syncBrowse).toHaveBeenCalledTimes(1);

    // Collapsing and re-opening reuses what was loaded rather than re-asking.
    await click(within(row).getByRole("button"));
    await waitFor(() => expect(screen.queryByRole("treeitem", { name: "Notes" })).toBeNull());
    await click(within(row).getByRole("button"));
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
    await click(within(root).getByRole("button"));
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
    await click(within(row).getByRole("button"));

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
    await click(within(row).getByRole("button"));

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

    await click(within(await screen.findByRole("treeitem", { name: "A" })).getByRole("button"));
    await click(within(await screen.findByRole("treeitem", { name: "B" })).getByRole("button"));

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

    await click(within(await screen.findByRole("treeitem", { name: "Vault" })).getByRole("button"));
    expect(await screen.findByText(truncation)).toBeInTheDocument();
    expect(screen.getByRole("treeitem", { name: "a.md" })).toBeInTheDocument();
  });

  // --- Read-only by construction (AD-75) ------------------------------------

  it("offers no control that could write, rename, move or delete", async () => {
    syncProfiles.mockResolvedValue([profile({ id: "01VAULT", name: "Vault" })]);
    syncBrowse.mockResolvedValue(
      listed("01VAULT", "", [entry("Notes", "folder"), entry("clip.mov", "video")]),
    );
    render(<FilesPane />);

    await click(within(await screen.findByRole("treeitem", { name: "Vault" })).getByRole("button"));
    await screen.findByRole("treeitem", { name: "clip.mov" });

    for (const label of FILES_WRITE_CONTROL_LABELS) {
      expect(
        screen.queryByRole("button", { name: new RegExp(`^${label}$`, "i") }),
        `${label} must not exist in a read-only browser (AD-75)`,
      ).toBeNull();
    }
    // No text input either: a browser with a name field in it is a rename
    // waiting to be wired up.
    expect(screen.queryAllByRole("textbox")).toHaveLength(0);
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

    await click(within(await screen.findByRole("treeitem", { name: "Vault" })).getByRole("button"));
    const row = await screen.findByRole("treeitem", { name: "clip.mov" });

    await click(within(row).getByRole("button", { name: FILES_REVEAL_LABEL }));
    expect(revealPath).toHaveBeenCalledWith("/Users/alice/Vault/clip.mov");

    await click(within(row).getByRole("button", { name: FILES_COPY_PATH_LABEL }));
    expect(writeText).toHaveBeenCalledWith("/Users/alice/Vault/clip.mov");

    // Open goes through the profile-rooted command, by id and relative subpath —
    // never by handing an absolute path to an opener.
    await click(within(row).getByRole("button", { name: FILES_OPEN_LABEL }));
    expect(syncOpenEntry).toHaveBeenCalledWith("01VAULT", "clip.mov");
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

    await click(within(await screen.findByRole("treeitem", { name: "Vault" })).getByRole("button"));
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

    await click(within(await screen.findByRole("treeitem", { name: "Vault" })).getByRole("button"));
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

    await click(within(await screen.findByRole("treeitem", { name: "Vault" })).getByRole("button"));
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
    // does not walk every action of every row in the tree.
    expect(within(file).getByRole("button", { name: FILES_COPY_PATH_LABEL })).toHaveAttribute(
      "tabindex",
      "-1",
    );

    file.focus();
    await act(async () => {
      await Promise.resolve();
    });
    expect(within(file).getByRole("button", { name: FILES_COPY_PATH_LABEL })).toHaveAttribute(
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

  it("does not grow a write control while offering to show a name", async () => {
    const rows = await tree(200);

    // The pane's standing promise (Story 43.8): nothing here moves, renames or
    // deletes. A new control is a new chance to break it.
    for (const label of FILES_WRITE_CONTROL_LABELS) {
      expect(within(rows).queryByRole("button", { name: label })).toBeNull();
    }
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
    await click(within(root).getByRole("button"));
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
    await click(within(root).getByRole("button"));
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
