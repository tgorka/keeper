import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AccountVm,
  FilesEntrySyncVm,
  FilesEntryVm,
  FilesListingVm,
  SyncOutcomeVm,
  SyncProfileVm,
  TextFileVm,
} from "@/lib/ipc/client";
import { accountStatusStore } from "@/lib/stores/account-status";
import { accountsStore } from "@/lib/stores/accounts";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { leadingDrawerStore } from "@/lib/stores/leading-drawer";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

// The reads this surface makes, each a spy so a test seeds what the column
// draws and asserts the ORDER the phone calls them in: list, materialize, read.
const syncProfiles = vi.fn(async (): Promise<SyncProfileVm[]> => []);
const syncBrowse = vi.fn(async (_id: string, _subpath: string): Promise<FilesListingVm> => {
  throw new Error("unseeded");
});
const syncMaterializeEntry = vi.fn(async (_id: string, _subpath: string): Promise<void> => {});
const OUTCOME: SyncOutcomeVm = {
  committed: false,
  pushed: false,
  pulled: true,
  filesChanged: 0,
  conflicts: [],
  stale: [],
  bytes: 0,
  line: "Up to date.",
};
const syncFolderNow = vi.fn(async (_id: string): Promise<SyncOutcomeVm> => OUTCOME);
const syncReadText = vi.fn(async (_id: string, _subpath: string): Promise<TextFileVm> => {
  throw new Error("unseeded");
});
const shareOut = vi.fn(async (_id: string, _subpath: string): Promise<void> => {});
const revealPath = vi.fn(async (_path: string): Promise<void> => {});
/** A read that never answers, for the voice surfaces the shell mounts (AD-179). */
const pending = new Promise<never>(() => {});
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    syncProfiles: () => syncProfiles(),
    syncBrowse: (id: string, subpath: string) => syncBrowse(id, subpath),
    syncMaterializeEntry: (id: string, subpath: string) => syncMaterializeEntry(id, subpath),
    syncFolderNow: (id: string) => syncFolderNow(id),
    syncReadText: (id: string, subpath: string) => syncReadText(id, subpath),
    shareOut: (id: string, subpath: string) => shareOut(id, subpath),
    revealPath: (path: string) => revealPath(path),
    // What the rest of the stack reads on mount, stubbed so the shell never
    // touches Tauri: the Inbox subscription, the drawer's settings dialog, the
    // search surface, the voice band.
    subscribeInbox: vi.fn(async (): Promise<number> => 1),
    unsubscribeInbox: vi.fn(async (): Promise<void> => {}),
    listDrafts: vi.fn(async (): Promise<Array<[string, string]>> => []),
    getFavoritesCollapsed: vi.fn(async (): Promise<boolean> => false),
    setFavoritesCollapsed: vi.fn(async (): Promise<void> => {}),
    encryptionPosture: vi.fn(() => Promise.resolve(false)),
    paletteQuery: vi.fn(async () => ({ contacts: [], chats: [], actions: [] })),
    searchArchive: vi.fn(async () => []),
    syncNow: vi.fn(async (): Promise<void> => {}),
    notesVaults: vi.fn(async () => []),
    voiceAvailability: vi.fn(() => pending),
    voiceWakeGet: vi.fn(() => pending),
  };
});

vi.mock("@/hooks/use-sign-out", () => ({
  useSignOut: () => vi.fn(),
}));

vi.mock("@/hooks/use-stale-resume-pill", () => ({
  useStaleResumePill: () => false,
}));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn((_handler?: (e: unknown) => void) => Promise.resolve(() => {})),
  }),
}));

import {
  FILES_PHONE_DOCUMENT_SLOT,
  FILES_PHONE_LISTING_SLOT,
  FILES_PHONE_PROFILES_SLOT,
  FILES_PHONE_PULL_TESTID,
  FILES_PHONE_PULL_THRESHOLD_PX,
  FILES_PHONE_ROW_TESTID,
  FILES_PHONE_SHARE_LABEL,
  FILES_PHONE_STATUS_TESTID,
  FilesPhonePane,
  filesPhoneArrivingSentence,
} from "@/components/files/files-phone-pane";
import {
  FILES_NO_PROFILES_SENTENCE,
  FILES_PANE_TITLE,
  FILES_REFRESH_LABEL,
  FILES_REVEAL_LABEL,
} from "@/components/layout/files-pane";
import { PhoneShell } from "@/components/layout/phone-shell";
import { OFFLINE_PILL_TEXT } from "@/components/layout/sidebar-pane";

/** The tier a phone hydrates to once its folder links (Epic 66): only what the OS refuses is off. */
const PHONE = { ...DEFAULT_CAPABILITIES, bots: true, sync: true, shareOut: true };
/** A Mac whose window is narrower than the phone breakpoint: the stack, with Finder. */
const NARROW_DESKTOP = {
  ...DEFAULT_CAPABILITIES,
  trayIcon: true,
  globalHotkey: true,
  launchAtLogin: true,
  inAppUpdater: true,
  nativeMenuBar: true,
  bridgeSidecar: true,
  revealInFileManager: true,
  sync: true,
  notes: true,
  sessions: true,
  bots: true,
  botTools: true,
};

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
  provider: "password",
};

function profile(p: Partial<SyncProfileVm> & Pick<SyncProfileVm, "id" | "name">): SyncProfileVm {
  return {
    localPath: "",
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
    sessions: false,
    sessionsSubfolder: "60-sessions",
    ...p,
  } as SyncProfileVm;
}

function entry(
  name: string,
  kind: FilesEntryVm["kind"],
  relativePath = name,
  sync: FilesEntrySyncVm = { status: "synced", detail: null },
): FilesEntryVm {
  return {
    name,
    relativePath,
    absolutePath: `/private/var/mobile/Containers/keeper/sync/p1/${relativePath}`,
    kind,
    sync,
    size: kind === "folder" ? null : { bytes: 1234, label: "1.2 kB" },
    lfsOid: null,
    mtimeMs: 1_700_000_000_000,
    folderRole: null,
    write: { writable: true, reason: null, caveat: null, caveatShort: null },
    release: null,
  };
}

function listing(subpath: string, entries: FilesEntryVm[]): FilesListingVm {
  return {
    profileId: "p1",
    subpath,
    state: "listed",
    entries,
    detail: null,
    truncated: false,
    write: { writable: true, reason: null, caveat: null, caveatShort: null },
  };
}

const POINTER: FilesEntrySyncVm = {
  status: "virtual",
  detail: "Content not on this phone; opening it fetches 1.2 kB.",
};

/** Seed one folder with a text file, a pointer and a subfolder. */
function seedFolder() {
  syncProfiles.mockResolvedValue([profile({ id: "p1", name: "tgdrive" })]);
  const rows: Record<string, FilesEntryVm[]> = {
    "": [
      entry("notes", "folder"),
      entry("README.md", "file"),
      entry("deck.pdf", "file", "deck.pdf", POINTER),
    ],
    notes: [entry("plan.md", "file", "notes/plan.md")],
  };
  syncBrowse.mockImplementation(async (_id, subpath) => listing(subpath, rows[subpath] ?? []));
  syncReadText.mockResolvedValue({
    text: "# Hello\n\nfrom the phone\n",
    sizeBytes: 24,
    sizeLabel: "24 B",
    oversize: false,
    binary: false,
    detail: null,
  });
  return rows;
}

const originalMatchMedia = window.matchMedia;
function mockViewportWidth(width: number) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => {
    const match = query.match(/max-width:\s*(\d+)px/);
    const maxWidth = match ? Number(match[1]) : Number.POSITIVE_INFINITY;
    const matches = query.includes("prefers-reduced-motion") ? true : width <= maxWidth;
    return {
      matches,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    };
  });
}

function stackLevel(level: 0 | 1 | 2): HTMLElement {
  const node = document.querySelector<HTMLElement>(`[data-level="${level}"]`);
  if (node === null) {
    throw new Error(`stack level ${level} is not mounted`);
  }
  return node;
}

beforeEach(() => {
  mockViewportWidth(390);
  accountsStore.getState().clear();
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  primaryViewStore.getState().setView("inbox");
  leadingDrawerStore.getState().close();
  accountStatusStore.getState().reset();
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  syncProfiles.mockReset();
  syncProfiles.mockResolvedValue([]);
  syncBrowse.mockReset();
  syncMaterializeEntry.mockReset();
  syncMaterializeEntry.mockResolvedValue(undefined);
  syncFolderNow.mockReset();
  syncFolderNow.mockResolvedValue(OUTCOME);
  syncReadText.mockReset();
  shareOut.mockReset();
  shareOut.mockResolvedValue(undefined);
  revealPath.mockReset();
  revealPath.mockResolvedValue(undefined);
});

afterEach(() => {
  window.matchMedia = originalMatchMedia;
  vi.restoreAllMocks();
});

describe("Files on the phone — the stack (Story 66.3, AD-200)", () => {
  it("opens from the drawer as a level 1: profile → listing → document, one column, and back", async () => {
    capabilitiesStore.getState().applySnapshot(PHONE);
    syncProfiles.mockResolvedValue([
      profile({ id: "p1", name: "tgdrive" }),
      profile({ id: "p2", name: "archive" }),
    ]);
    seedFolder();
    syncProfiles.mockResolvedValue([
      profile({ id: "p1", name: "tgdrive" }),
      profile({ id: "p2", name: "archive" }),
    ]);
    accountsStore.getState().addAccount(account);
    render(<PhoneShell />);

    fireEvent.click(screen.getByRole("button", { name: "Open navigation" }));
    const nav = await screen.findByRole("navigation", { name: "Views" });
    fireEvent.click(within(nav).getByRole("button", { name: /^Files/ }));
    await waitFor(() => expect(leadingDrawerStore.getState().isOpen).toBe(false));
    expect(primaryViewStore.getState().view).toBe("files");

    const level = stackLevel(1);
    expect(within(level).getByRole("button", { name: "Back to Inbox" })).toBeInTheDocument();
    const pane = within(level).getByRole("region", { name: FILES_PANE_TITLE });

    // Two folders: the list is the surface, one row each, nothing beside it.
    const profiles = await within(pane).findByTestId(`${FILES_PHONE_ROW_TESTID}-p1`);
    expect(pane.querySelector(`[data-slot="${FILES_PHONE_PROFILES_SLOT}"]`)).not.toBeNull();
    expect(pane.querySelector(`[data-slot="${FILES_PHONE_LISTING_SLOT}"]`)).toBeNull();
    fireEvent.click(profiles);

    // The listing replaces the list — the same column, not a second one — and
    // is what `sync_browse` answered for the root.
    await within(pane).findByTestId(`${FILES_PHONE_ROW_TESTID}-README.md`);
    expect(syncBrowse).toHaveBeenCalledWith("p1", "");
    expect(pane.querySelector(`[data-slot="${FILES_PHONE_PROFILES_SLOT}"]`)).toBeNull();
    expect(within(pane).getByRole("heading", { level: 1 })).toHaveTextContent("tgdrive");
    // Every row is a thumb's height.
    for (const row of within(pane).getAllByRole("button", { name: /README|deck|notes/ })) {
      expect(row).toHaveClass("min-h-11");
    }

    // A subfolder descends; its back lands on the profile root.
    fireEvent.click(within(pane).getByTestId(`${FILES_PHONE_ROW_TESTID}-notes`));
    await within(pane).findByTestId(`${FILES_PHONE_ROW_TESTID}-notes/plan.md`);
    expect(syncBrowse).toHaveBeenLastCalledWith("p1", "notes");
    fireEvent.click(within(pane).getByRole("button", { name: "Back to tgdrive" }));
    await within(pane).findByTestId(`${FILES_PHONE_ROW_TESTID}-README.md`);

    // A text file opens full-screen through the registry's text viewer, read
    // by the same command the desktop panel uses, with no listing beside it.
    fireEvent.click(within(pane).getByTestId(`${FILES_PHONE_ROW_TESTID}-README.md`));
    await waitFor(() =>
      expect(pane.querySelector(`[data-slot="${FILES_PHONE_DOCUMENT_SLOT}"]`)).not.toBeNull(),
    );
    expect(pane.querySelector(`[data-slot="${FILES_PHONE_LISTING_SLOT}"]`)).toBeNull();
    await waitFor(() => expect(syncReadText).toHaveBeenCalledWith("p1", "README.md"));
    expect(syncMaterializeEntry).not.toHaveBeenCalled();
    // Back from the document is the folder it came from, without a re-read.
    const reads = syncBrowse.mock.calls.length;
    fireEvent.click(within(pane).getByRole("button", { name: "Back to tgdrive" }));
    await within(pane).findByTestId(`${FILES_PHONE_ROW_TESTID}-README.md`);
    expect(syncBrowse.mock.calls.length).toBe(reads);

    // And the shell's own back pops the level.
    fireEvent.click(within(level).getByRole("button", { name: "Back to Inbox" }));
    expect(primaryViewStore.getState().view).toBe("inbox");
    await waitFor(() => expect(document.querySelector('[data-level="1"]')).toBeNull());
  });

  it("has no Files row where the folder cannot sync", async () => {
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, bots: true });
    render(<PhoneShell />);
    fireEvent.click(screen.getByRole("button", { name: "Open navigation" }));
    const nav = await screen.findByRole("navigation", { name: "Views" });
    expect(within(nav).queryByRole("button", { name: /^Files/ })).toBeNull();
    act(() => {
      leadingDrawerStore.getState().close();
    });
    act(() => {
      primaryViewStore.getState().setView("files");
    });
    expect(document.querySelector('[data-level="1"]')).toBeNull();
  });
});

describe("Files on the phone — the pane", () => {
  it("says so when no folder is set up, and offers nothing", async () => {
    capabilitiesStore.getState().applySnapshot(PHONE);
    render(<FilesPhonePane />);
    expect(await screen.findByText(FILES_NO_PROFILES_SENTENCE)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: FILES_REFRESH_LABEL })).toBeNull();
    expect(syncBrowse).not.toHaveBeenCalled();
  });

  it("with one folder, opens it straight away", async () => {
    capabilitiesStore.getState().applySnapshot(PHONE);
    seedFolder();
    render(<FilesPhonePane />);
    await screen.findByTestId(`${FILES_PHONE_ROW_TESTID}-README.md`);
    expect(syncBrowse).toHaveBeenCalledWith("p1", "");
    // Back from the root is the list of folders, even a list of one.
    fireEvent.click(screen.getByRole("button", { name: `Back to ${FILES_PANE_TITLE}` }));
    expect(await screen.findByTestId(`${FILES_PHONE_ROW_TESTID}-p1`)).toBeInTheDocument();
  });

  it("materializes a pointer before opening it, in that order, and the second open reads without a fetch", async () => {
    capabilitiesStore.getState().applySnapshot(PHONE);
    const rows = seedFolder();
    // The materialize lands the bytes: the re-read after it says so.
    syncMaterializeEntry.mockImplementation(async () => {
      rows[""] = rows[""].map((e) =>
        e.name === "deck.pdf"
          ? { ...e, sync: { status: "materialized", detail: "Content on this phone." } }
          : e,
      );
    });
    // Hold the materialize so the progress sentence is observable.
    let release: () => void = () => {};
    syncMaterializeEntry.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          release = () => {
            rows[""] = rows[""].map((e) =>
              e.name === "deck.pdf"
                ? { ...e, sync: { status: "materialized", detail: "Content on this phone." } }
                : e,
            );
            resolve();
          };
        }),
    );
    render(<FilesPhonePane />);
    const pointer = await screen.findByTestId(`${FILES_PHONE_ROW_TESTID}-deck.pdf`);
    fireEvent.click(pointer);

    // The sync mark's own word, while the content is on its way; no document yet.
    expect(await screen.findByTestId(FILES_PHONE_STATUS_TESTID)).toHaveTextContent(
      filesPhoneArrivingSentence("deck.pdf"),
    );
    expect(syncMaterializeEntry).toHaveBeenCalledWith("p1", "deck.pdf");
    expect(document.querySelector(`[data-slot="${FILES_PHONE_DOCUMENT_SLOT}"]`)).toBeNull();

    await act(async () => {
      release();
    });
    // Materialize, then the folder re-read, then the open — the order the
    // epic names (materialise on open, through the batch client).
    await waitFor(() =>
      expect(document.querySelector(`[data-slot="${FILES_PHONE_DOCUMENT_SLOT}"]`)).not.toBeNull(),
    );
    expect(syncBrowse).toHaveBeenCalledTimes(2);
    expect(syncMaterializeEntry.mock.invocationCallOrder[0]).toBeLessThan(
      syncBrowse.mock.invocationCallOrder[1],
    );
    expect(screen.queryByTestId(FILES_PHONE_STATUS_TESTID)).toBeNull();

    // Back, then the same row: its mark says the bytes are here, so no fetch.
    fireEvent.click(screen.getByRole("button", { name: "Back to tgdrive" }));
    fireEvent.click(await screen.findByTestId(`${FILES_PHONE_ROW_TESTID}-deck.pdf`));
    await waitFor(() =>
      expect(document.querySelector(`[data-slot="${FILES_PHONE_DOCUMENT_SLOT}"]`)).not.toBeNull(),
    );
    expect(syncMaterializeEntry).toHaveBeenCalledTimes(1);
  });

  it("shows a refused materialize in Rust's words and opens nothing", async () => {
    capabilitiesStore.getState().applySnapshot(PHONE);
    seedFolder();
    syncMaterializeEntry.mockRejectedValue({
      code: "internal",
      message: "deck.pdf has local changes keeper will not overwrite.",
      accountId: null,
      retriable: false,
    });
    render(<FilesPhonePane />);
    fireEvent.click(await screen.findByTestId(`${FILES_PHONE_ROW_TESTID}-deck.pdf`));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "deck.pdf has local changes keeper will not overwrite.",
    );
    expect(document.querySelector(`[data-slot="${FILES_PHONE_DOCUMENT_SLOT}"]`)).toBeNull();
    expect(screen.getByTestId(`${FILES_PHONE_ROW_TESTID}-deck.pdf`)).toBeInTheDocument();
  });

  it("offers Share on the phone — addressed by profile and subpath — and no Reveal, export or copy", async () => {
    capabilitiesStore.getState().applySnapshot(PHONE);
    seedFolder();
    render(<FilesPhonePane />);
    fireEvent.click(await screen.findByTestId(`${FILES_PHONE_ROW_TESTID}-README.md`));
    const share = await screen.findByRole("button", { name: FILES_PHONE_SHARE_LABEL });
    expect(share).toHaveClass("min-h-11");
    expect(screen.queryByRole("button", { name: FILES_REVEAL_LABEL })).toBeNull();
    expect(screen.queryByRole("button", { name: /Export|Copy path|Open in/ })).toBeNull();
    fireEvent.click(share);
    await waitFor(() => expect(shareOut).toHaveBeenCalledWith("p1", "README.md"));
    expect(syncMaterializeEntry).not.toHaveBeenCalled();
  });

  it("materializes a pointer before sharing it", async () => {
    capabilitiesStore.getState().applySnapshot(PHONE);
    const rows = seedFolder();
    // Open lands the bytes once; Share on the opened document must not fetch again —
    // but a document whose row is STILL a pointer (a stale listing) is fetched first.
    syncMaterializeEntry.mockResolvedValue(undefined);
    render(<FilesPhonePane />);
    fireEvent.click(await screen.findByTestId(`${FILES_PHONE_ROW_TESTID}-deck.pdf`));
    await waitFor(() =>
      expect(document.querySelector(`[data-slot="${FILES_PHONE_DOCUMENT_SLOT}"]`)).not.toBeNull(),
    );
    expect(syncMaterializeEntry).toHaveBeenCalledTimes(1);
    // The re-read still says `virtual` (nothing changed the rows): Share fetches again
    // before handing the file to the sheet, and never shares a pointer.
    expect(rows[""].find((e) => e.name === "deck.pdf")?.sync.status).toBe("virtual");
    fireEvent.click(screen.getByRole("button", { name: FILES_PHONE_SHARE_LABEL }));
    await waitFor(() => expect(shareOut).toHaveBeenCalledWith("p1", "deck.pdf"));
    expect(syncMaterializeEntry).toHaveBeenCalledTimes(2);
    expect(syncMaterializeEntry.mock.invocationCallOrder[1]).toBeLessThan(
      shareOut.mock.invocationCallOrder[0],
    );
  });

  it("offers Reveal and no Share on a narrow desktop window", async () => {
    capabilitiesStore.getState().applySnapshot(NARROW_DESKTOP);
    seedFolder();
    render(<FilesPhonePane />);
    fireEvent.click(await screen.findByTestId(`${FILES_PHONE_ROW_TESTID}-README.md`));
    const reveal = await screen.findByRole("button", { name: FILES_REVEAL_LABEL });
    expect(screen.queryByRole("button", { name: FILES_PHONE_SHARE_LABEL })).toBeNull();
    fireEvent.click(reveal);
    await waitFor(() =>
      expect(revealPath).toHaveBeenCalledWith(
        "/private/var/mobile/Containers/keeper/sync/p1/README.md",
      ),
    );
  });

  it("pull-to-refresh past the threshold calls sync_folder_now and re-browses; below it, nothing", async () => {
    capabilitiesStore.getState().applySnapshot(PHONE);
    seedFolder();
    render(<FilesPhonePane />);
    await screen.findByTestId(`${FILES_PHONE_ROW_TESTID}-README.md`);
    expect(syncBrowse).toHaveBeenCalledTimes(1);
    const zone = screen.getByTestId(FILES_PHONE_PULL_TESTID);

    // Short pull: snaps back, no fetch.
    fireEvent.pointerDown(zone, { pointerId: 1, clientY: 5 });
    fireEvent.pointerMove(zone, { pointerId: 1, clientY: 5 + FILES_PHONE_PULL_THRESHOLD_PX / 2 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientY: 5 + FILES_PHONE_PULL_THRESHOLD_PX / 2 });
    expect(syncFolderNow).not.toHaveBeenCalled();

    // Past it: the engine fetches the folder, then the listing is re-read.
    fireEvent.pointerDown(zone, { pointerId: 2, clientY: 5 });
    fireEvent.pointerMove(zone, { pointerId: 2, clientY: 5 + FILES_PHONE_PULL_THRESHOLD_PX + 20 });
    fireEvent.pointerUp(zone, { pointerId: 2, clientY: 5 + FILES_PHONE_PULL_THRESHOLD_PX + 20 });
    await waitFor(() => expect(syncFolderNow).toHaveBeenCalledWith("p1"));
    await waitFor(() => expect(syncBrowse).toHaveBeenCalledTimes(2));
    expect(syncFolderNow.mock.invocationCallOrder[0]).toBeLessThan(
      syncBrowse.mock.invocationCallOrder[1],
    );

    // The visible control does the same without a gesture.
    fireEvent.click(screen.getByRole("button", { name: FILES_REFRESH_LABEL }));
    await waitFor(() => expect(syncFolderNow).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(syncBrowse).toHaveBeenCalledTimes(3));
  });

  it("shows a failed fetch's sentence and keeps the listing", async () => {
    capabilitiesStore.getState().applySnapshot(PHONE);
    seedFolder();
    syncFolderNow.mockRejectedValue({
      code: "internal",
      message: "the phone has 1 commit the remote does not; push it or reset the phone's copy",
      accountId: null,
      retriable: false,
    });
    render(<FilesPhonePane />);
    await screen.findByTestId(`${FILES_PHONE_ROW_TESTID}-README.md`);
    fireEvent.click(screen.getByRole("button", { name: FILES_REFRESH_LABEL }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/the phone has 1 commit/);
    expect(screen.getByTestId(`${FILES_PHONE_ROW_TESTID}-README.md`)).toBeInTheDocument();
  });

  it("wears the offline pill on the phone tier while every account is offline", async () => {
    capabilitiesStore.getState().applySnapshot(PHONE);
    seedFolder();
    accountsStore.getState().addAccount(account);
    act(() => {
      accountStatusStore.getState().setStatus(account.accountId, "offline");
    });
    render(<FilesPhonePane />);
    expect(await screen.findByTestId("offline-pill")).toHaveTextContent(OFFLINE_PILL_TEXT);
    act(() => {
      accountStatusStore.getState().setStatus(account.accountId, "online");
    });
    expect(screen.queryByTestId("offline-pill")).toBeNull();
  });

  it("says why a folder could not be listed, never that it is empty", async () => {
    capabilitiesStore.getState().applySnapshot(PHONE);
    syncProfiles.mockResolvedValue([profile({ id: "p1", name: "tgdrive" })]);
    syncBrowse.mockResolvedValue({
      profileId: "p1",
      subpath: "",
      state: "missing",
      entries: null,
      detail: "tgdrive is not on this phone yet; it clones on the next sync.",
      truncated: false,
      write: { writable: false, reason: "not listed", caveat: null, caveatShort: null },
    });
    render(<FilesPhonePane />);
    expect(
      await screen.findByText("tgdrive is not on this phone yet; it clones on the next sync."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/empty/)).toBeNull();
  });
});
