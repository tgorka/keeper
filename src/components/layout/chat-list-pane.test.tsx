import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AccountVm,
  InboxBatch,
  IpcError,
  NetworksSnapshot,
  SpacesSnapshot,
} from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { archiveRoomsStore } from "@/lib/stores/archive-rooms";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

// Mock the typed IPC wrapper so the pane never touches Tauri. `subscribeInbox`
// captures the `onInbox`/`onArchive`/`onPins`/`onFavourites`/`onSpaces` handlers so
// the test can drive every window of the merged stream (Story 4.2 + 4.3 + 4.4 + 4.5).
const subscribeInbox = vi.fn();
const unsubscribeInbox = vi.fn();
const setSpaceFilter = vi.fn(
  async (_accountId: string | null, _spaceId: string | null): Promise<void> => {},
);
const setNetworkFilter = vi.fn(async (_network: string | null): Promise<void> => {});
const getFavoritesCollapsed = vi.fn(async (): Promise<boolean> => false);
const setFavoritesCollapsed = vi.fn(async (_collapsed: boolean): Promise<void> => {});
vi.mock("@/lib/ipc/client", () => ({
  subscribeInbox: (
    onInbox: (b: InboxBatch) => void,
    onArchive: (b: InboxBatch) => void,
    onPins: (b: InboxBatch) => void,
    onFavourites: (b: InboxBatch) => void,
    onSpaces: (s: SpacesSnapshot) => void,
    onNetworks: (n: NetworksSnapshot) => void,
  ) => subscribeInbox(onInbox, onArchive, onPins, onFavourites, onSpaces, onNetworks),
  unsubscribeInbox: (id: number) => unsubscribeInbox(id),
  setSpaceFilter: (accountId: string | null, spaceId: string | null) =>
    setSpaceFilter(accountId, spaceId),
  setNetworkFilter: (network: string | null) => setNetworkFilter(network),
  getFavoritesCollapsed: () => getFavoritesCollapsed(),
  setFavoritesCollapsed: (v: boolean) => setFavoritesCollapsed(v),
  // Draft-marker seed on mount (Story 7.1): no drafts by default.
  listDrafts: vi.fn(async (): Promise<Array<[string, string]>> => []),
  // Best-effort mutation wrappers the strip/rows may call; no-ops here.
  reorderPins: vi.fn(async () => {}),
  unpinRoom: vi.fn(async () => {}),
  pinRoom: vi.fn(async () => {}),
  markRoomRead: vi.fn(async () => {}),
  markRoomUnread: vi.fn(async () => {}),
  archiveRoom: vi.fn(async () => {}),
  unarchiveRoom: vi.fn(async () => {}),
  favoriteRoom: vi.fn(async () => {}),
  unfavoriteRoom: vi.fn(async () => {}),
}));

import { ChatListPane } from "@/components/layout/chat-list-pane";
import { favoritesRoomsStore } from "@/lib/stores/favorites-rooms";
import { favoritesUiStore } from "@/lib/stores/favorites-ui";
import { networksStore } from "@/lib/stores/networks";
import { pinsRoomsStore } from "@/lib/stores/pins-rooms";
import { spacesStore } from "@/lib/stores/spaces";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
  provider: "password",
};

const bob: AccountVm = {
  accountId: "01BX5ZZKBKACTAV9WEVGEMMVRZ",
  userId: "@bob:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 1,
  provider: "password",
};

function ipcError(code: IpcError["code"]): IpcError {
  return { code, message: "ignored", accountId: null, retriable: true };
}

function inboxRoom(roomId: string, accountId: string, displayName: string, lastMessage: string) {
  return {
    accountId,
    hueIndex: 0,
    roomId,
    displayName,
    lastMessage,
    timestamp: null,
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isPinned: false,
    isFavourite: false,
    network: null,
    networkId: null,
  };
}

beforeEach(() => {
  accountsStore.getState().clear();
  accountsStore.setState({ filterAccountId: null });
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  archiveRoomsStore.getState().clear();
  pinsRoomsStore.getState().clear();
  favoritesRoomsStore.getState().clear();
  favoritesUiStore.getState().setCollapsed(false);
  spacesStore.getState().clear();
  networksStore.getState().clear();
  primaryViewStore.getState().setView("inbox");
  subscribeInbox.mockReset();
  unsubscribeInbox.mockReset();
  setSpaceFilter.mockReset();
  setNetworkFilter.mockReset();
  getFavoritesCollapsed.mockReset();
  getFavoritesCollapsed.mockResolvedValue(false);
  setFavoritesCollapsed.mockReset();
});

afterEach(() => {
  accountsStore.getState().clear();
  accountsStore.setState({ filterAccountId: null });
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  archiveRoomsStore.getState().clear();
  pinsRoomsStore.getState().clear();
  favoritesRoomsStore.getState().clear();
  favoritesUiStore.getState().setCollapsed(false);
  spacesStore.getState().clear();
  primaryViewStore.getState().setView("inbox");
});

describe("ChatListPane", () => {
  it("shows the loading skeleton before the first batch arrives", () => {
    subscribeInbox.mockResolvedValue(1);
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
    expect(screen.queryByText("No conversations yet.")).not.toBeInTheDocument();
  });

  it("shows the empty state after a batch delivers no rooms", async () => {
    const captured: { onBatch: ((b: InboxBatch) => void) | null } = { onBatch: null };
    subscribeInbox.mockImplementation((onBatch: (b: InboxBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    captured.onBatch?.({ ops: [{ op: "reset", rooms: [] }], total: 0 });

    await waitFor(() => {
      expect(screen.getByText("No conversations yet.")).toBeInTheDocument();
    });
    expect(screen.queryByLabelText("Loading conversations")).not.toBeInTheDocument();
  });

  it("subscribes to the merged inbox and renders streamed rows from multiple accounts", async () => {
    const captured: { onBatch: ((b: InboxBatch) => void) | null } = { onBatch: null };
    subscribeInbox.mockImplementation((onBatch: (b: InboxBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().hydrateAll([account, bob]);
    render(<ChatListPane />);

    expect(subscribeInbox).toHaveBeenCalledWith(
      expect.any(Function),
      expect.any(Function),
      expect.any(Function),
      expect.any(Function),
      expect.any(Function),
      expect.any(Function),
    );

    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          rooms: [
            inboxRoom("!a:example.org", account.accountId, "Alpha Room", "first"),
            inboxRoom("!b:example.org", "01BX5ZZKBKACTAV9WEVGEMMVRZ", "Beta Room", "second"),
          ],
        },
      ],
      total: 2,
    });

    await waitFor(() => {
      expect(screen.getByText("Alpha Room")).toBeInTheDocument();
    });
    expect(screen.getByText("Beta Room")).toBeInTheDocument();
  });

  it("selects a row by its account + room ids and highlights it", async () => {
    const captured: { onBatch: ((b: InboxBatch) => void) | null } = { onBatch: null };
    subscribeInbox.mockImplementation((onBatch: (b: InboxBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    captured.onBatch?.({
      ops: [
        { op: "reset", rooms: [inboxRoom("!a:example.org", account.accountId, "Alpha Room", "")] },
      ],
      total: 1,
    });

    await waitFor(() => {
      expect(screen.getByText("Alpha Room")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Conversation with Alpha Room" }));

    expect(roomsStore.getState().selected).toEqual({
      accountId: account.accountId,
      roomId: "!a:example.org",
    });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Conversation with Alpha Room" })).toHaveAttribute(
        "aria-current",
        "true",
      );
    });
  });

  it("unsubscribes and clears the store on unmount", async () => {
    subscribeInbox.mockResolvedValue(7);
    accountsStore.getState().addAccount(account);
    const { unmount } = render(<ChatListPane />);

    await waitFor(() => {
      expect(subscribeInbox).toHaveBeenCalled();
    });

    unmount();

    await waitFor(() => {
      expect(unsubscribeInbox).toHaveBeenCalledWith(7);
    });
    expect(roomsStore.getState().rooms).toEqual([]);
  });

  it("shows an inline error when the merged subscribe fails with syncUnavailable", async () => {
    subscribeInbox.mockRejectedValue(ipcError("syncUnavailable"));
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    await waitFor(() => {
      expect(screen.getByText(/Couldn't start syncing/)).toBeInTheDocument();
    });
  });

  it("does not subscribe when there are no accounts", () => {
    render(<ChatListPane />);
    expect(subscribeInbox).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
  });

  it("ignores a batch delivered after effect cleanup", async () => {
    const captured: { onBatch: ((b: InboxBatch) => void) | null } = { onBatch: null };
    subscribeInbox.mockImplementation((onBatch: (b: InboxBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().addAccount(account);
    const { unmount } = render(<ChatListPane />);

    await waitFor(() => {
      expect(subscribeInbox).toHaveBeenCalled();
    });

    unmount();

    captured.onBatch?.({
      ops: [
        { op: "reset", rooms: [inboxRoom("!late:example.org", account.accountId, "Late", "")] },
      ],
      total: 1,
    });

    expect(roomsStore.getState().rooms).toEqual([]);
  });

  it("applies the account filter as a pure display filter, hiding other accounts' rooms", async () => {
    const captured: { onBatch: ((b: InboxBatch) => void) | null } = { onBatch: null };
    subscribeInbox.mockImplementation((onBatch: (b: InboxBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().hydrateAll([account, bob]);
    render(<ChatListPane />);

    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          rooms: [
            inboxRoom("!a:example.org", account.accountId, "Alpha Room", "first"),
            inboxRoom("!b:example.org", bob.accountId, "Beta Room", "second"),
          ],
        },
      ],
      total: 2,
    });

    await waitFor(() => {
      expect(screen.getByText("Alpha Room")).toBeInTheDocument();
    });
    expect(screen.getByText("Beta Room")).toBeInTheDocument();

    // Filter to account (alice): only its room stays; the merged subscription is
    // untouched (no re-subscribe).
    accountsStore.getState().toggleFilter(account.accountId);
    await waitFor(() => {
      expect(screen.queryByText("Beta Room")).not.toBeInTheDocument();
    });
    expect(screen.getByText("Alpha Room")).toBeInTheDocument();
    expect(subscribeInbox).toHaveBeenCalledTimes(1);

    // Clearing the filter shows every account's rooms again.
    accountsStore.getState().toggleFilter(account.accountId);
    await waitFor(() => {
      expect(screen.getByText("Beta Room")).toBeInTheDocument();
    });
  });

  it("renders the archive window and empty-state text when the primary view is archive", async () => {
    const captured: {
      onInbox: ((b: InboxBatch) => void) | null;
      onArchive: ((b: InboxBatch) => void) | null;
    } = { onInbox: null, onArchive: null };
    subscribeInbox.mockImplementation(
      (onInbox: (b: InboxBatch) => void, onArchive: (b: InboxBatch) => void) => {
        captured.onInbox = onInbox;
        captured.onArchive = onArchive;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    // Feed the inbox window one row and the archive window another.
    captured.onInbox?.({
      ops: [
        { op: "reset", rooms: [inboxRoom("!a:example.org", account.accountId, "Inbox Room", "")] },
      ],
      total: 1,
    });
    captured.onArchive?.({
      ops: [
        {
          op: "reset",
          rooms: [inboxRoom("!z:example.org", account.accountId, "Archived Room", "")],
        },
      ],
      total: 1,
    });

    // Inbox view (default) shows the inbox row, not the archive row.
    await waitFor(() => {
      expect(screen.getByText("Inbox Room")).toBeInTheDocument();
    });
    expect(screen.queryByText("Archived Room")).not.toBeInTheDocument();

    // Switching to the archive view renders the archive row instead.
    primaryViewStore.getState().setView("archive");
    await waitFor(() => {
      expect(screen.getByText("Archived Room")).toBeInTheDocument();
    });
    expect(screen.queryByText("Inbox Room")).not.toBeInTheDocument();
  });

  it("shows the archive empty-state text when the archive window is empty", async () => {
    const captured: {
      onInbox: ((b: InboxBatch) => void) | null;
      onArchive: ((b: InboxBatch) => void) | null;
    } = { onInbox: null, onArchive: null };
    subscribeInbox.mockImplementation(
      (onInbox: (b: InboxBatch) => void, onArchive: (b: InboxBatch) => void) => {
        captured.onInbox = onInbox;
        captured.onArchive = onArchive;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    primaryViewStore.getState().setView("archive");
    render(<ChatListPane />);

    captured.onArchive?.({ ops: [{ op: "reset", rooms: [] }], total: 0 });

    await waitFor(() => {
      expect(screen.getByText(/Nothing archived\./)).toBeInTheDocument();
    });
    // The code-font `E` verb (UX-DR13).
    expect(screen.getByText("E")).toBeInTheDocument();
    expect(screen.queryByText("No conversations yet.")).not.toBeInTheDocument();
  });

  it("keeps the archive skeleton (not a premature empty-state) until the archive window itself loads", async () => {
    const captured: {
      onInbox: ((b: InboxBatch) => void) | null;
      onArchive: ((b: InboxBatch) => void) | null;
    } = { onInbox: null, onArchive: null };
    subscribeInbox.mockImplementation(
      (onInbox: (b: InboxBatch) => void, onArchive: (b: InboxBatch) => void) => {
        captured.onInbox = onInbox;
        captured.onArchive = onArchive;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    primaryViewStore.getState().setView("archive");
    render(<ChatListPane />);

    // Only the inbox window has delivered; the archive channel has not emitted
    // yet. The archive view must still show its loading skeleton, never the
    // "Nothing archived." empty-state (per-window loaded gating).
    captured.onInbox?.({
      ops: [
        { op: "reset", rooms: [inboxRoom("!a:example.org", account.accountId, "Inbox Room", "")] },
      ],
      total: 1,
    });

    await waitFor(() => {
      expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
    });
    expect(screen.queryByText(/Nothing archived\./)).not.toBeInTheDocument();

    // Once the archive window delivers (empty), the empty-state replaces the skeleton.
    captured.onArchive?.({ ops: [{ op: "reset", rooms: [] }], total: 0 });
    await waitFor(() => {
      expect(screen.getByText(/Nothing archived\./)).toBeInTheDocument();
    });
  });

  it("feeds the third channel into the pins store and renders the strip in the inbox view", async () => {
    const captured: { onPins: ((b: InboxBatch) => void) | null } = { onPins: null };
    subscribeInbox.mockImplementation(
      (
        _onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        onPins: (b: InboxBatch) => void,
      ) => {
        captured.onPins = onPins;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    // No pins yet → the strip is hidden entirely.
    expect(screen.queryByLabelText("Pinned conversations")).not.toBeInTheDocument();

    // The pins channel delivers one pinned room.
    captured.onPins?.({
      ops: [
        {
          op: "reset",
          rooms: [
            {
              ...inboxRoom("!p:example.org", account.accountId, "Pinned Room", ""),
              isPinned: true,
            },
          ],
        },
      ],
      total: 1,
    });

    await waitFor(() => {
      expect(screen.getByLabelText("Pinned conversations")).toBeInTheDocument();
    });
    expect(
      screen.getByRole("button", { name: "Pinned conversation with Pinned Room" }),
    ).toBeInTheDocument();
    // The store mirrors the window.
    expect(pinsRoomsStore.getState().rooms).toHaveLength(1);
  });

  it("hides the pins strip in the archive view", async () => {
    const captured: { onPins: ((b: InboxBatch) => void) | null } = { onPins: null };
    subscribeInbox.mockImplementation(
      (
        _onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        onPins: (b: InboxBatch) => void,
      ) => {
        captured.onPins = onPins;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    captured.onPins?.({
      ops: [
        {
          op: "reset",
          rooms: [
            {
              ...inboxRoom("!p:example.org", account.accountId, "Pinned Room", ""),
              isPinned: true,
            },
          ],
        },
      ],
      total: 1,
    });

    await waitFor(() => {
      expect(screen.getByLabelText("Pinned conversations")).toBeInTheDocument();
    });

    // Switch to the archive view: the strip is hidden (it lives atop the inbox only).
    primaryViewStore.getState().setView("archive");
    await waitFor(() => {
      expect(screen.queryByLabelText("Pinned conversations")).not.toBeInTheDocument();
    });
  });

  it("feeds the fourth channel into the favorites store and renders the section (inbox view)", async () => {
    const captured: { onFavourites: ((b: InboxBatch) => void) | null } = { onFavourites: null };
    subscribeInbox.mockImplementation(
      (
        _onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        _onPins: (b: InboxBatch) => void,
        onFavourites: (b: InboxBatch) => void,
      ) => {
        captured.onFavourites = onFavourites;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    // No favourites yet → the section is hidden entirely.
    expect(screen.queryByRole("region", { name: "Favorites" })).not.toBeInTheDocument();

    // The favourites channel delivers one favourited room.
    captured.onFavourites?.({
      ops: [
        {
          op: "reset",
          rooms: [
            {
              ...inboxRoom("!f:example.org", account.accountId, "Favorite Room", ""),
              isFavourite: true,
            },
          ],
        },
      ],
      total: 1,
    });

    await waitFor(() => {
      expect(screen.getByRole("region", { name: "Favorites" })).toBeInTheDocument();
    });
    expect(
      screen.getByRole("button", { name: "Favorite conversation with Favorite Room" }),
    ).toBeInTheDocument();
    // The store mirrors the window.
    expect(favoritesRoomsStore.getState().rooms).toHaveLength(1);
  });

  it("hides the favorites section in the archive view", async () => {
    const captured: { onFavourites: ((b: InboxBatch) => void) | null } = { onFavourites: null };
    subscribeInbox.mockImplementation(
      (
        _onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        _onPins: (b: InboxBatch) => void,
        onFavourites: (b: InboxBatch) => void,
      ) => {
        captured.onFavourites = onFavourites;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    captured.onFavourites?.({
      ops: [
        {
          op: "reset",
          rooms: [
            {
              ...inboxRoom("!f:example.org", account.accountId, "Favorite Room", ""),
              isFavourite: true,
            },
          ],
        },
      ],
      total: 1,
    });

    await waitFor(() => {
      expect(screen.getByRole("region", { name: "Favorites" })).toBeInTheDocument();
    });

    // Switch to the archive view: the section is hidden (it lives atop the inbox only).
    primaryViewStore.getState().setView("archive");
    await waitFor(() => {
      expect(screen.queryByRole("region", { name: "Favorites" })).not.toBeInTheDocument();
    });
  });

  it("hydrates the favorites collapse state on mount", async () => {
    getFavoritesCollapsed.mockResolvedValue(true);
    subscribeInbox.mockResolvedValue(1);
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    await waitFor(() => {
      expect(getFavoritesCollapsed).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(favoritesUiStore.getState().isCollapsed).toBe(true);
    });
  });

  it("feeds the fifth channel into the spaces store", async () => {
    const captured: { onSpaces: ((s: SpacesSnapshot) => void) | null } = { onSpaces: null };
    subscribeInbox.mockImplementation(
      (
        _onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        _onPins: (b: InboxBatch) => void,
        _onFavourites: (b: InboxBatch) => void,
        onSpaces: (s: SpacesSnapshot) => void,
      ) => {
        captured.onSpaces = onSpaces;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    captured.onSpaces?.({
      spaces: [
        {
          accountId: account.accountId,
          spaceId: "!s:example.org",
          name: "Design",
          avatarUrl: null,
        },
      ],
    });

    await waitFor(() => {
      expect(spacesStore.getState().spaces).toHaveLength(1);
    });
    expect(spacesStore.getState().spaces[0].name).toBe("Design");
  });

  it("reconciles a stale Space selection absent from a streamed snapshot", async () => {
    const captured: { onSpaces: ((s: SpacesSnapshot) => void) | null } = { onSpaces: null };
    subscribeInbox.mockImplementation(
      (
        _onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        _onPins: (b: InboxBatch) => void,
        _onFavourites: (b: InboxBatch) => void,
        onSpaces: (s: SpacesSnapshot) => void,
      ) => {
        captured.onSpaces = onSpaces;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    // The active selection points at a Space owned by an account that is about to
    // vanish from the streamed list (e.g. its owner signed out with others left).
    spacesStore
      .getState()
      .setActiveSpace({ accountId: "gone-account", spaceId: "!gone:example.org" });
    render(<ChatListPane />);
    await waitFor(() => {
      expect(captured.onSpaces).not.toBeNull();
    });
    // A snapshot arrives WITHOUT the selected Space.
    captured.onSpaces?.({
      spaces: [
        {
          accountId: account.accountId,
          spaceId: "!other:example.org",
          name: "Design",
          avatarUrl: null,
        },
      ],
    });

    // The stale selection is dropped and the Rust filter cleared, so the inbox
    // does not stay filtered on a Space with no members (empty-inbox-forever).
    await waitFor(() => {
      expect(setSpaceFilter).toHaveBeenCalledWith(null, null);
    });
    expect(spacesStore.getState().activeSpace).toBeNull();
  });

  it("renders a dismissible Space filter chip that clears the filter", async () => {
    const captured: { onSpaces: ((s: SpacesSnapshot) => void) | null } = { onSpaces: null };
    subscribeInbox.mockImplementation(
      (
        _onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        _onPins: (b: InboxBatch) => void,
        _onFavourites: (b: InboxBatch) => void,
        onSpaces: (s: SpacesSnapshot) => void,
      ) => {
        captured.onSpaces = onSpaces;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    // The selection survives the mount effect; the list arrives via the channel.
    spacesStore
      .getState()
      .setActiveSpace({ accountId: account.accountId, spaceId: "!s:example.org" });
    render(<ChatListPane />);
    await waitFor(() => {
      expect(captured.onSpaces).not.toBeNull();
    });
    captured.onSpaces?.({
      spaces: [
        {
          accountId: account.accountId,
          spaceId: "!s:example.org",
          name: "Design",
          avatarUrl: null,
        },
      ],
    });

    // The chip shows the Space name and a clear ✕.
    const clearBtn = await screen.findByRole("button", { name: "Clear Design filter" });
    expect(clearBtn).toBeInTheDocument();

    fireEvent.click(clearBtn);
    // Clearing pokes the Rust filter with null/null and drops the selection.
    await waitFor(() => {
      expect(setSpaceFilter).toHaveBeenCalledWith(null, null);
    });
    expect(spacesStore.getState().activeSpace).toBeNull();
  });

  it("clears the Space filter on Esc from the list", async () => {
    const captured: { onSpaces: ((s: SpacesSnapshot) => void) | null } = { onSpaces: null };
    subscribeInbox.mockImplementation(
      (
        _onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        _onPins: (b: InboxBatch) => void,
        _onFavourites: (b: InboxBatch) => void,
        onSpaces: (s: SpacesSnapshot) => void,
      ) => {
        captured.onSpaces = onSpaces;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    spacesStore
      .getState()
      .setActiveSpace({ accountId: account.accountId, spaceId: "!s:example.org" });
    render(<ChatListPane />);
    await waitFor(() => {
      expect(captured.onSpaces).not.toBeNull();
    });
    captured.onSpaces?.({
      spaces: [
        {
          accountId: account.accountId,
          spaceId: "!s:example.org",
          name: "Design",
          avatarUrl: null,
        },
      ],
    });

    await screen.findByRole("button", { name: "Clear Design filter" });
    // Esc anywhere in the list container clears the active filter.
    fireEvent.keyDown(screen.getByLabelText("Clear Design filter"), { key: "Escape" });
    await waitFor(() => {
      expect(setSpaceFilter).toHaveBeenCalledWith(null, null);
    });
    expect(spacesStore.getState().activeSpace).toBeNull();
  });

  it("shows the 'No chats in {Space}' empty state when the filtered inbox is empty", async () => {
    const captured: {
      onInbox: ((b: InboxBatch) => void) | null;
      onSpaces: ((s: SpacesSnapshot) => void) | null;
    } = { onInbox: null, onSpaces: null };
    subscribeInbox.mockImplementation(
      (
        onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        _onPins: (b: InboxBatch) => void,
        _onFavourites: (b: InboxBatch) => void,
        onSpaces: (s: SpacesSnapshot) => void,
      ) => {
        captured.onInbox = onInbox;
        captured.onSpaces = onSpaces;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    spacesStore
      .getState()
      .setActiveSpace({ accountId: account.accountId, spaceId: "!s:example.org" });
    render(<ChatListPane />);
    await waitFor(() => {
      expect(captured.onSpaces).not.toBeNull();
    });
    captured.onSpaces?.({
      spaces: [
        {
          accountId: account.accountId,
          spaceId: "!s:example.org",
          name: "Design",
          avatarUrl: null,
        },
      ],
    });

    // The (filtered) inbox window delivers no rows.
    captured.onInbox?.({ ops: [{ op: "reset", rooms: [] }], total: 0 });

    await waitFor(() => {
      // The label is split across text nodes (interpolated Space name); match the
      // leading text node and the interpolated Space name separately.
      expect(screen.getByText(/No chats in/)).toBeInTheDocument();
    });
    expect(screen.getByText("Design")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear filter" })).toBeInTheDocument();
    expect(screen.queryByText("No conversations yet.")).not.toBeInTheDocument();
  });

  it("re-applies the Space filter after an account-set re-subscribe", async () => {
    subscribeInbox.mockResolvedValue(1);
    accountsStore.getState().addAccount(account);
    spacesStore
      .getState()
      .setActiveSpace({ accountId: account.accountId, spaceId: "!s:example.org" });
    render(<ChatListPane />);

    await waitFor(() => {
      expect(subscribeInbox).toHaveBeenCalledTimes(1);
    });
    // The initial subscribe re-applies the carried-over selection.
    await waitFor(() => {
      expect(setSpaceFilter).toHaveBeenCalledWith(account.accountId, "!s:example.org");
    });

    // Adding a second account re-subscribes; the filter is re-applied again.
    setSpaceFilter.mockClear();
    accountsStore.getState().addAccount(bob);
    await waitFor(() => {
      expect(subscribeInbox).toHaveBeenCalledTimes(2);
    });
    await waitFor(() => {
      expect(setSpaceFilter).toHaveBeenCalledWith(account.accountId, "!s:example.org");
    });
  });
});

describe("ChatListPane — Network filter (Story 4.6)", () => {
  it("feeds the 6th channel into the networks store", async () => {
    const captured: { onNetworks: ((n: NetworksSnapshot) => void) | null } = { onNetworks: null };
    subscribeInbox.mockImplementation(
      (
        _onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        _onPins: (b: InboxBatch) => void,
        _onFavourites: (b: InboxBatch) => void,
        _onSpaces: (s: SpacesSnapshot) => void,
        onNetworks: (n: NetworksSnapshot) => void,
      ) => {
        captured.onNetworks = onNetworks;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);
    await waitFor(() => {
      expect(captured.onNetworks).not.toBeNull();
    });
    captured.onNetworks?.({ networks: [{ name: "Telegram" }, { name: "Signal" }] });
    expect(networksStore.getState().networks.map((n) => n.name)).toEqual(["Telegram", "Signal"]);
  });

  it("renders a dismissible Network chip that clears the filter", async () => {
    subscribeInbox.mockResolvedValue(1);
    accountsStore.getState().addAccount(account);
    networksStore.getState().setActiveNetwork("Telegram");
    render(<ChatListPane />);

    const clearBtn = await screen.findByRole("button", { name: "Clear Telegram filter" });
    expect(clearBtn).toBeInTheDocument();

    fireEvent.click(clearBtn);
    await waitFor(() => {
      expect(setNetworkFilter).toHaveBeenCalledWith(null);
    });
    expect(networksStore.getState().activeNetwork).toBeNull();
  });

  it("shows BOTH chips when a Space and a Network filter compose (AND)", async () => {
    const captured: { onSpaces: ((s: SpacesSnapshot) => void) | null } = { onSpaces: null };
    subscribeInbox.mockImplementation(
      (
        _onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        _onPins: (b: InboxBatch) => void,
        _onFavourites: (b: InboxBatch) => void,
        onSpaces: (s: SpacesSnapshot) => void,
      ) => {
        captured.onSpaces = onSpaces;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    spacesStore
      .getState()
      .setActiveSpace({ accountId: account.accountId, spaceId: "!s:example.org" });
    networksStore.getState().setActiveNetwork("Telegram");
    render(<ChatListPane />);
    await waitFor(() => {
      expect(captured.onSpaces).not.toBeNull();
    });
    captured.onSpaces?.({
      spaces: [
        {
          accountId: account.accountId,
          spaceId: "!s:example.org",
          name: "Design",
          avatarUrl: null,
        },
      ],
    });

    // Both chips render side by side (AND composition).
    expect(await screen.findByRole("button", { name: "Clear Design filter" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear Telegram filter" })).toBeInTheDocument();
  });

  it("the Network chip's ✕ clears ONLY the Network filter (Space stays active)", async () => {
    const captured: { onSpaces: ((s: SpacesSnapshot) => void) | null } = { onSpaces: null };
    subscribeInbox.mockImplementation(
      (
        _onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        _onPins: (b: InboxBatch) => void,
        _onFavourites: (b: InboxBatch) => void,
        onSpaces: (s: SpacesSnapshot) => void,
      ) => {
        captured.onSpaces = onSpaces;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    spacesStore
      .getState()
      .setActiveSpace({ accountId: account.accountId, spaceId: "!s:example.org" });
    networksStore.getState().setActiveNetwork("Telegram");
    render(<ChatListPane />);
    await waitFor(() => {
      expect(captured.onSpaces).not.toBeNull();
    });
    captured.onSpaces?.({
      spaces: [
        {
          accountId: account.accountId,
          spaceId: "!s:example.org",
          name: "Design",
          avatarUrl: null,
        },
      ],
    });

    // Click ONLY the Network chip's ✕.
    fireEvent.click(await screen.findByRole("button", { name: "Clear Telegram filter" }));
    await waitFor(() => {
      expect(setNetworkFilter).toHaveBeenCalledWith(null);
    });
    // Network cleared; Space untouched (no null/null poke, selection + chip remain).
    expect(networksStore.getState().activeNetwork).toBeNull();
    expect(setSpaceFilter).not.toHaveBeenCalledWith(null, null);
    expect(spacesStore.getState().activeSpace).not.toBeNull();
    expect(screen.getByRole("button", { name: "Clear Design filter" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Clear Telegram filter" })).not.toBeInTheDocument();
  });

  it("the Space chip's ✕ clears ONLY the Space filter (Network stays active)", async () => {
    const captured: { onSpaces: ((s: SpacesSnapshot) => void) | null } = { onSpaces: null };
    subscribeInbox.mockImplementation(
      (
        _onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        _onPins: (b: InboxBatch) => void,
        _onFavourites: (b: InboxBatch) => void,
        onSpaces: (s: SpacesSnapshot) => void,
      ) => {
        captured.onSpaces = onSpaces;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    spacesStore
      .getState()
      .setActiveSpace({ accountId: account.accountId, spaceId: "!s:example.org" });
    networksStore.getState().setActiveNetwork("Telegram");
    render(<ChatListPane />);
    await waitFor(() => {
      expect(captured.onSpaces).not.toBeNull();
    });
    captured.onSpaces?.({
      spaces: [
        {
          accountId: account.accountId,
          spaceId: "!s:example.org",
          name: "Design",
          avatarUrl: null,
        },
      ],
    });

    // Click ONLY the Space chip's ✕.
    fireEvent.click(await screen.findByRole("button", { name: "Clear Design filter" }));
    await waitFor(() => {
      expect(setSpaceFilter).toHaveBeenCalledWith(null, null);
    });
    // Space cleared; Network untouched (no null poke, selection + chip remain).
    expect(spacesStore.getState().activeSpace).toBeNull();
    expect(setNetworkFilter).not.toHaveBeenCalledWith(null);
    expect(networksStore.getState().activeNetwork).toBe("Telegram");
    expect(screen.getByRole("button", { name: "Clear Telegram filter" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Clear Design filter" })).not.toBeInTheDocument();
  });

  it("Esc clears BOTH the Space and Network filters", async () => {
    subscribeInbox.mockResolvedValue(1);
    accountsStore.getState().addAccount(account);
    spacesStore
      .getState()
      .setActiveSpace({ accountId: account.accountId, spaceId: "!s:example.org" });
    networksStore.getState().setActiveNetwork("Telegram");
    render(<ChatListPane />);

    const chip = await screen.findByRole("button", { name: "Clear Telegram filter" });
    fireEvent.keyDown(chip, { key: "Escape" });
    await waitFor(() => {
      expect(setSpaceFilter).toHaveBeenCalledWith(null, null);
      expect(setNetworkFilter).toHaveBeenCalledWith(null);
    });
    expect(spacesStore.getState().activeSpace).toBeNull();
    expect(networksStore.getState().activeNetwork).toBeNull();
  });

  it("shows a ' · '-joined empty-state label under composed filters", async () => {
    const captured: {
      onInbox: ((b: InboxBatch) => void) | null;
      onSpaces: ((s: SpacesSnapshot) => void) | null;
    } = { onInbox: null, onSpaces: null };
    subscribeInbox.mockImplementation(
      (
        onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        _onPins: (b: InboxBatch) => void,
        _onFavourites: (b: InboxBatch) => void,
        onSpaces: (s: SpacesSnapshot) => void,
      ) => {
        captured.onInbox = onInbox;
        captured.onSpaces = onSpaces;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    spacesStore
      .getState()
      .setActiveSpace({ accountId: account.accountId, spaceId: "!s:example.org" });
    networksStore.getState().setActiveNetwork("Telegram");
    render(<ChatListPane />);
    await waitFor(() => {
      expect(captured.onSpaces).not.toBeNull();
    });
    captured.onSpaces?.({
      spaces: [
        {
          accountId: account.accountId,
          spaceId: "!s:example.org",
          name: "Design",
          avatarUrl: null,
        },
      ],
    });
    // Empty filtered inbox.
    captured.onInbox?.({ ops: [{ op: "reset", rooms: [] }], total: 0 });

    await waitFor(() => {
      // The label joins the active filter names with " · " (Design · Telegram).
      expect(screen.getByText(/Design · Telegram/)).toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: "Clear filter" })).toBeInTheDocument();
  });

  it("re-applies the Network filter after an account-set re-subscribe", async () => {
    subscribeInbox.mockResolvedValue(1);
    accountsStore.getState().addAccount(account);
    networksStore.getState().setActiveNetwork("Telegram");
    render(<ChatListPane />);

    await waitFor(() => {
      expect(setNetworkFilter).toHaveBeenCalledWith("Telegram");
    });

    setNetworkFilter.mockClear();
    accountsStore.getState().addAccount(bob);
    await waitFor(() => {
      expect(subscribeInbox).toHaveBeenCalledTimes(2);
    });
    await waitFor(() => {
      expect(setNetworkFilter).toHaveBeenCalledWith("Telegram");
    });
  });

  it("reconciles a stale Network selection absent from a streamed snapshot", async () => {
    const captured: { onNetworks: ((n: NetworksSnapshot) => void) | null } = { onNetworks: null };
    subscribeInbox.mockImplementation(
      (
        _onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        _onPins: (b: InboxBatch) => void,
        _onFavourites: (b: InboxBatch) => void,
        _onSpaces: (s: SpacesSnapshot) => void,
        onNetworks: (n: NetworksSnapshot) => void,
      ) => {
        captured.onNetworks = onNetworks;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    networksStore.getState().setActiveNetwork("Telegram");
    render(<ChatListPane />);
    await waitFor(() => {
      expect(captured.onNetworks).not.toBeNull();
    });
    // A snapshot that no longer lists the active Network reconciles the selection.
    captured.onNetworks?.({ networks: [{ name: "Signal" }] });
    await waitFor(() => {
      expect(networksStore.getState().activeNetwork).toBeNull();
      expect(setNetworkFilter).toHaveBeenCalledWith(null);
    });
  });
});
