import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
import { chatListFocusStore } from "@/lib/stores/chat-list-focus";
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
// Draft-mirror subscription (Story 7.2): capture the batch handler so a test can drive
// a live remote edit, and record the subscribe/unsubscribe lifecycle.
const subscribeDraftMirror = vi.fn();
const unsubscribeDraftMirror = vi.fn();
// Verb command wrappers the keyboard navigation (Story 9.2) invokes on the focused
// row. Hoisted so the nav tests can assert the correct command + direction fired.
const archiveRoomMock = vi.fn(async (_accountId: string, _roomId: string): Promise<void> => {});
const unarchiveRoomMock = vi.fn(async (_accountId: string, _roomId: string): Promise<void> => {});
const pinRoomMock = vi.fn(async (_accountId: string, _roomId: string): Promise<void> => {});
const unpinRoomMock = vi.fn(async (_accountId: string, _roomId: string): Promise<void> => {});
const favoriteRoomMock = vi.fn(async (_accountId: string, _roomId: string): Promise<void> => {});
const unfavoriteRoomMock = vi.fn(async (_accountId: string, _roomId: string): Promise<void> => {});
const markRoomReadMock = vi.fn(async (_accountId: string, _roomId: string): Promise<void> => {});
const markRoomUnreadMock = vi.fn(async (_accountId: string, _roomId: string): Promise<void> => {});
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
  // Draft-mirror subscription (Story 7.2): app-lifetime remote-edit stream.
  subscribeDraftMirror: (onBatch: (b: unknown) => void) => subscribeDraftMirror(onBatch),
  unsubscribeDraftMirror: (id: number) => unsubscribeDraftMirror(id),
  // Best-effort mutation wrappers the strip/rows/keyboard verbs may call.
  reorderPins: vi.fn(async () => {}),
  unpinRoom: (accountId: string, roomId: string) => unpinRoomMock(accountId, roomId),
  pinRoom: (accountId: string, roomId: string) => pinRoomMock(accountId, roomId),
  markRoomRead: (accountId: string, roomId: string) => markRoomReadMock(accountId, roomId),
  markRoomUnread: (accountId: string, roomId: string) => markRoomUnreadMock(accountId, roomId),
  archiveRoom: (accountId: string, roomId: string) => archiveRoomMock(accountId, roomId),
  unarchiveRoom: (accountId: string, roomId: string) => unarchiveRoomMock(accountId, roomId),
  favoriteRoom: (accountId: string, roomId: string) => favoriteRoomMock(accountId, roomId),
  unfavoriteRoom: (accountId: string, roomId: string) => unfavoriteRoomMock(accountId, roomId),
}));

import { ChatListPane } from "@/components/layout/chat-list-pane";
import { composerStore } from "@/lib/stores/composer";
import { draftsStore } from "@/lib/stores/drafts";
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
  subscribeDraftMirror.mockReset();
  subscribeDraftMirror.mockResolvedValue(1);
  unsubscribeDraftMirror.mockReset();
  unsubscribeDraftMirror.mockResolvedValue(undefined);
  setSpaceFilter.mockReset();
  setNetworkFilter.mockReset();
  getFavoritesCollapsed.mockReset();
  getFavoritesCollapsed.mockResolvedValue(false);
  setFavoritesCollapsed.mockReset();
  archiveRoomMock.mockClear();
  unarchiveRoomMock.mockClear();
  pinRoomMock.mockClear();
  unpinRoomMock.mockClear();
  favoriteRoomMock.mockClear();
  unfavoriteRoomMock.mockClear();
  markRoomReadMock.mockClear();
  markRoomUnreadMock.mockClear();
  composerStore.setState({ focusNonce: 0 });
  chatListFocusStore.setState({ focusNonce: 0 });
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
  draftsStore.getState().clear();
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

describe("ChatListPane draft-mirror subscription (Story 7.2)", () => {
  it("starts the app-lifetime mirror subscription and pumps edits into the drafts store", async () => {
    subscribeInbox.mockResolvedValue(1);
    const captured: { onBatch: ((b: unknown) => void) | null } = { onBatch: null };
    subscribeDraftMirror.mockImplementation((onBatch: (b: unknown) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(9);
    });
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    await waitFor(() => {
      expect(captured.onBatch).not.toBeNull();
    });
    // A live remote edit is fed into the drafts store's remote map.
    captured.onBatch?.({
      accountId: account.accountId,
      roomId: "!r1:example.org",
      body: "remote draft",
      updatedTs: 100,
    });
    await waitFor(() => {
      expect(draftsStore.getState().remote.get(`${account.accountId} !r1:example.org`)).toEqual({
        body: "remote draft",
        updatedTs: 100,
      });
    });
    // A tombstone (null body) removes the remote entry.
    captured.onBatch?.({
      accountId: account.accountId,
      roomId: "!r1:example.org",
      body: null,
      updatedTs: 101,
    });
    await waitFor(() => {
      expect(draftsStore.getState().remote.has(`${account.accountId} !r1:example.org`)).toBe(false);
    });
  });

  it("unsubscribes the mirror on unmount", async () => {
    subscribeInbox.mockResolvedValue(1);
    subscribeDraftMirror.mockResolvedValue(11);
    accountsStore.getState().addAccount(account);
    const { unmount } = render(<ChatListPane />);

    await waitFor(() => {
      expect(subscribeDraftMirror).toHaveBeenCalled();
    });
    unmount();
    await waitFor(() => {
      expect(unsubscribeDraftMirror).toHaveBeenCalledWith(11);
    });
  });
});

describe("ChatListPane keyboard navigation (Story 9.2)", () => {
  // Render the pane and stream a set of inbox rows, returning the row buttons in
  // Rust order. Each row overrides come from the passed VM patches.
  async function renderWithRooms(
    rooms: Array<Partial<ReturnType<typeof inboxRoom>> & { roomId: string; displayName: string }>,
  ) {
    const captured: { onInbox: ((b: InboxBatch) => void) | null } = { onInbox: null };
    subscribeInbox.mockImplementation((onInbox: (b: InboxBatch) => void) => {
      captured.onInbox = onInbox;
      return Promise.resolve(1);
    });
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);
    captured.onInbox?.({
      ops: [
        {
          op: "reset",
          rooms: rooms.map((r) => ({
            ...inboxRoom(r.roomId, account.accountId, r.displayName, ""),
            ...r,
          })),
        },
      ],
      total: rooms.length,
    });
    await waitFor(() => {
      expect(screen.getByText(rooms[0].displayName)).toBeInTheDocument();
    });
    // Return the captured inbox emitter so a test can stream a later batch (e.g. a
    // recency re-order) and assert the roving cursor tracks identity, not position.
    return (
      rooms: Array<Partial<ReturnType<typeof inboxRoom>> & { roomId: string; displayName: string }>,
    ) =>
      captured.onInbox?.({
        ops: [
          {
            op: "reset",
            rooms: rooms.map((r) => ({
              ...inboxRoom(r.roomId, account.accountId, r.displayName, ""),
              ...r,
            })),
          },
        ],
        total: rooms.length,
      });
  }

  function rowButton(displayName: string): HTMLElement {
    return screen.getByRole("button", { name: `Conversation with ${displayName}` });
  }

  it("moves the roving focus ring through rows in Rust order on ArrowDown / j", async () => {
    await renderWithRooms([
      { roomId: "!a", displayName: "Alpha" },
      { roomId: "!b", displayName: "Beta" },
      { roomId: "!c", displayName: "Gamma" },
    ]);
    const container = screen.getByLabelText("Conversations");

    fireEvent.keyDown(container, { key: "ArrowDown" });
    expect(rowButton("Alpha")).toHaveFocus();
    expect(rowButton("Alpha")).toHaveAttribute("tabindex", "0");

    fireEvent.keyDown(container, { key: "j" });
    expect(rowButton("Beta")).toHaveFocus();
    expect(rowButton("Alpha")).toHaveAttribute("tabindex", "-1");
  });

  it("clamps at the ends deterministically and moves back up with ArrowUp / k", async () => {
    await renderWithRooms([
      { roomId: "!a", displayName: "Alpha" },
      { roomId: "!b", displayName: "Beta" },
    ]);
    const container = screen.getByLabelText("Conversations");

    fireEvent.keyDown(container, { key: "ArrowDown" });
    fireEvent.keyDown(container, { key: "ArrowDown" });
    fireEvent.keyDown(container, { key: "ArrowDown" }); // clamps at the last row
    expect(rowButton("Beta")).toHaveFocus();

    fireEvent.keyDown(container, { key: "k" });
    expect(rowButton("Alpha")).toHaveFocus();
    fireEvent.keyDown(container, { key: "ArrowUp" }); // clamps at the first row
    expect(rowButton("Alpha")).toHaveFocus();
  });

  it("Enter selects the focused row and requests composer focus", async () => {
    await renderWithRooms([
      { roomId: "!a", displayName: "Alpha" },
      { roomId: "!b", displayName: "Beta" },
    ]);
    const container = screen.getByLabelText("Conversations");
    fireEvent.keyDown(container, { key: "ArrowDown" });
    fireEvent.keyDown(container, { key: "ArrowDown" });

    fireEvent.keyDown(container, { key: "Enter" });
    expect(roomsStore.getState().selected).toEqual({
      accountId: account.accountId,
      roomId: "!b",
    });
    expect(composerStore.getState().focusNonce).toBe(1);
  });

  it("`e` archives an inbox row and unarchives an archived one", async () => {
    await renderWithRooms([
      { roomId: "!a", displayName: "Alpha", isArchived: false },
      { roomId: "!b", displayName: "Beta", isArchived: true },
    ]);
    const container = screen.getByLabelText("Conversations");

    fireEvent.keyDown(container, { key: "ArrowDown" }); // focus Alpha
    fireEvent.keyDown(container, { key: "e" });
    expect(archiveRoomMock).toHaveBeenCalledWith(account.accountId, "!a");
    expect(unarchiveRoomMock).not.toHaveBeenCalled();

    fireEvent.keyDown(container, { key: "ArrowDown" }); // focus Beta (archived)
    fireEvent.keyDown(container, { key: "e" });
    expect(unarchiveRoomMock).toHaveBeenCalledWith(account.accountId, "!b");
  });

  it("`p` pins / unpins and `f` favorites / unfavorites per current flag", async () => {
    await renderWithRooms([
      { roomId: "!a", displayName: "Alpha", isPinned: false, isFavourite: true },
    ]);
    const container = screen.getByLabelText("Conversations");
    fireEvent.keyDown(container, { key: "ArrowDown" });

    fireEvent.keyDown(container, { key: "p" });
    expect(pinRoomMock).toHaveBeenCalledWith(account.accountId, "!a");

    fireEvent.keyDown(container, { key: "f" });
    expect(unfavoriteRoomMock).toHaveBeenCalledWith(account.accountId, "!a");
  });

  it("`u` sets the optimistic overlay and marks read for an unread row", async () => {
    await renderWithRooms([{ roomId: "!a", displayName: "Alpha", isUnread: true }]);
    const container = screen.getByLabelText("Conversations");
    fireEvent.keyDown(container, { key: "ArrowDown" });

    fireEvent.keyDown(container, { key: "u" });
    // Optimistic overlay flips the row to read within the frame; the command fires.
    expect(roomsStore.getState().optimisticUnread.get(`${account.accountId}|!a`)).toBe(false);
    expect(markRoomReadMock).toHaveBeenCalledWith(account.accountId, "!a");
  });

  it("`u` reverts the optimistic overlay when the mark command hard-rejects", async () => {
    markRoomUnreadMock.mockRejectedValueOnce(new Error("nope"));
    await renderWithRooms([{ roomId: "!a", displayName: "Alpha", isUnread: false }]);
    const container = screen.getByLabelText("Conversations");
    fireEvent.keyDown(container, { key: "ArrowDown" });

    fireEvent.keyDown(container, { key: "u" });
    expect(markRoomUnreadMock).toHaveBeenCalledWith(account.accountId, "!a");
    // The overlay was set optimistically, then reverted on the rejection.
    await waitFor(() => {
      expect(roomsStore.getState().optimisticUnread.has(`${account.accountId}|!a`)).toBe(false);
    });
  });

  it("passes modifier chords through (no bare-verb hijack while a modifier is held)", async () => {
    await renderWithRooms([{ roomId: "!a", displayName: "Alpha", isArchived: false }]);
    const container = screen.getByLabelText("Conversations");
    fireEvent.keyDown(container, { key: "ArrowDown" });

    // ⌘F (search) must not archive; the list handler ignores modifier chords.
    fireEvent.keyDown(container, { key: "e", metaKey: true });
    expect(archiveRoomMock).not.toHaveBeenCalled();
  });

  it("Esc clears the focused-row ring when no filter is active", async () => {
    await renderWithRooms([{ roomId: "!a", displayName: "Alpha" }]);
    const container = screen.getByLabelText("Conversations");
    fireEvent.keyDown(container, { key: "ArrowDown" });
    expect(rowButton("Alpha")).toHaveAttribute("tabindex", "0");

    fireEvent.keyDown(container, { key: "Escape" });
    // Ring cleared: the first row falls back to the default tab stop (0) but no row
    // is keyboard-focused; a subsequent verb no-ops. Assert the verb no-ops.
    fireEvent.keyDown(container, { key: "e" });
    expect(archiveRoomMock).not.toHaveBeenCalled();
  });

  it("keeps the roving cursor on the same room after a recency re-order (verb targets by identity)", async () => {
    const restream = await renderWithRooms([
      { roomId: "!a", displayName: "Alpha", isArchived: false },
      { roomId: "!b", displayName: "Beta", isArchived: false },
    ]);
    const container = screen.getByLabelText("Conversations");
    fireEvent.keyDown(container, { key: "ArrowDown" }); // focus Alpha (index 0)

    // A recency bump re-orders the window so Beta is now index 0 and Alpha index 1.
    restream([
      { roomId: "!b", displayName: "Beta", isArchived: false },
      { roomId: "!a", displayName: "Alpha", isArchived: false },
    ]);
    await waitFor(() => {
      // The tab stop follows Alpha to its new position — it is not stranded on index 0.
      expect(rowButton("Alpha")).toHaveAttribute("tabindex", "0");
      expect(rowButton("Beta")).toHaveAttribute("tabindex", "-1");
    });

    // `e` archives Alpha (the keyed row), NOT whatever row now sits at the old index.
    fireEvent.keyDown(container, { key: "e" });
    expect(archiveRoomMock).toHaveBeenCalledWith(account.accountId, "!a");
  });

  it("no-ops a verb when the focused row has left the window", async () => {
    const restream = await renderWithRooms([
      { roomId: "!a", displayName: "Alpha", isArchived: false },
      { roomId: "!b", displayName: "Beta", isArchived: false },
    ]);
    const container = screen.getByLabelText("Conversations");
    fireEvent.keyDown(container, { key: "ArrowDown" }); // focus Alpha

    // Alpha leaves the inbox window (e.g. archived elsewhere); only Beta remains.
    restream([{ roomId: "!b", displayName: "Beta", isArchived: false }]);
    await waitFor(() => {
      // The list keeps exactly one tab stop — the surviving first row.
      expect(rowButton("Beta")).toHaveAttribute("tabindex", "0");
    });

    // The gone row is not acted on, and the cursor did not silently jump to Beta.
    fireEvent.keyDown(container, { key: "e" });
    expect(archiveRoomMock).not.toHaveBeenCalled();
  });

  it("ignores list keys bubbling from outside the conversations list (pins/favorites/chips)", async () => {
    await renderWithRooms([{ roomId: "!a", displayName: "Alpha", isArchived: false }]);
    const list = screen.getByLabelText("Conversations");
    fireEvent.keyDown(list, { key: "ArrowDown" }); // focus Alpha in the main list

    // A bare `e` whose target is a sibling of the <ul> (a Pins/Favorites/chip button
    // lives in the same keydown container but outside the conversations list) must
    // bubble to the container handler and be ignored — no main-list archive verb.
    const sibling = document.createElement("button");
    list.parentElement?.appendChild(sibling);
    fireEvent.keyDown(sibling, { key: "e" });
    expect(archiveRoomMock).not.toHaveBeenCalled();
    sibling.remove();
  });

  // ── Global summon-hotkey focus request (Story 9.4) ─────────────────────────
  it("moves keyboard focus to the first Inbox row when a focus request is made", async () => {
    await renderWithRooms([
      { roomId: "!a", displayName: "Alpha", isArchived: false },
      { roomId: "!b", displayName: "Beta", isArchived: false },
    ]);
    // No row focused initially.
    expect(rowButton("Alpha")).not.toHaveFocus();

    // A focus request (the global hotkey raise) lands focus on the first row.
    act(() => {
      chatListFocusStore.getState().requestFocus();
    });
    await waitFor(() => {
      expect(rowButton("Alpha")).toHaveFocus();
    });
  });

  it("falls back to the list container when the Inbox is empty on a focus request", async () => {
    const captured: { onInbox: ((b: InboxBatch) => void) | null } = { onInbox: null };
    subscribeInbox.mockImplementation((onInbox: (b: InboxBatch) => void) => {
      captured.onInbox = onInbox;
      return Promise.resolve(1);
    });
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);
    captured.onInbox?.({ ops: [{ op: "reset", rooms: [] }], total: 0 });
    await waitFor(() => {
      expect(screen.getByText("No conversations yet.")).toBeInTheDocument();
    });

    act(() => {
      chatListFocusStore.getState().requestFocus();
    });
    // With no row to focus, the focusable list container receives focus so keyboard
    // focus still lands in the pane (matrix: empty inbox).
    await waitFor(() => {
      expect(document.activeElement).not.toBe(document.body);
      expect((document.activeElement as HTMLElement)?.tabIndex).toBe(-1);
    });
  });

  it("completes a pending focus request on the first row once cold-start rooms arrive", async () => {
    // Cold-start raise: the hotkey fires before the first inbox batch has streamed in.
    const captured: { onInbox: ((b: InboxBatch) => void) | null } = { onInbox: null };
    subscribeInbox.mockImplementation((onInbox: (b: InboxBatch) => void) => {
      captured.onInbox = onInbox;
      return Promise.resolve(1);
    });
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);
    captured.onInbox?.({ ops: [{ op: "reset", rooms: [] }], total: 0 });
    await waitFor(() => {
      expect(screen.getByText("No conversations yet.")).toBeInTheDocument();
    });

    // Request focus while the list is still empty → container fallback, request pending.
    act(() => {
      chatListFocusStore.getState().requestFocus();
    });
    await waitFor(() => {
      expect((document.activeElement as HTMLElement)?.tabIndex).toBe(-1);
    });

    // Rooms stream in a moment later; the pending request completes onto the first row.
    act(() => {
      captured.onInbox?.({
        ops: [
          {
            op: "reset",
            rooms: [inboxRoom("!a:example.org", account.accountId, "Alpha", "")],
          },
        ],
        total: 1,
      });
    });
    await waitFor(() => {
      expect(rowButton("Alpha")).toHaveFocus();
    });
  });
});
