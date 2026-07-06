import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, InboxRoomVm } from "@/lib/ipc/client";

// Mock the typed IPC wrapper so the hook never touches Tauri.
const signOut = vi.fn();
const deleteAccountArchive = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  signOut: (accountId: string) => signOut(accountId),
  deleteAccountArchive: (accountId: string) => deleteAccountArchive(accountId),
}));

// Mock the toast surface so the last-account purge-failure path is observable.
const toastError = vi.fn();
vi.mock("sonner", () => ({
  toast: { error: (message: string) => toastError(message) },
}));

import { useSignOut } from "@/hooks/use-sign-out";
import { accountStatusStore } from "@/lib/stores/account-status";
import { accountsStore } from "@/lib/stores/accounts";
import { draftsStore } from "@/lib/stores/drafts";
import { roomsStore } from "@/lib/stores/rooms";
import { timelineStore } from "@/lib/stores/timeline";

function account(id: string, hue = 0): AccountVm {
  return {
    accountId: id,
    userId: `@user-${id}:example.org`,
    homeserverUrl: "https://matrix.example.org/",
    hueIndex: hue,
    provider: "password",
  };
}

function room(id: string, accountId: string): InboxRoomVm {
  return {
    accountId,
    hueIndex: 0,
    roomId: id,
    displayName: id,
    lastMessage: null,
    timestamp: null,
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isPinned: false,
    isFavourite: false,
    network: null,
    networkId: null,
    muteState: "none",
  };
}

const alice = account("alice", 0);
const bob = account("bob", 1);

beforeEach(() => {
  accountsStore.getState().clear();
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  timelineStore.getState().clear();
  accountStatusStore.getState().reset();
  draftsStore.getState().clear();
  signOut.mockReset();
  signOut.mockResolvedValue(undefined);
  deleteAccountArchive.mockReset();
  deleteAccountArchive.mockResolvedValue(undefined);
  toastError.mockReset();
});

afterEach(() => {
  accountsStore.getState().clear();
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
});

describe("useSignOut", () => {
  it("signs out the given account and removes only it, keeping the others", async () => {
    accountsStore.getState().hydrateAll([alice, bob]);

    const { result } = renderHook(() => useSignOut());
    await result.current(alice.accountId);

    expect(signOut).toHaveBeenCalledWith(alice.accountId);
    expect(accountsStore.getState().accounts.map((a) => a.accountId)).toEqual([bob.accountId]);
  });

  it("closes the open conversation only when it belonged to the signed-out account", async () => {
    accountsStore.getState().hydrateAll([alice, bob]);
    roomsStore.getState().selectRoom({ accountId: bob.accountId, roomId: "!b:example.org" });
    roomsStore.getState().applyBatch({
      ops: [{ op: "reset", rooms: [room("!b:example.org", bob.accountId)] }],
      total: 1,
    });
    timelineStore.getState().applyBatch({
      ops: [{ op: "reset", items: [{ kind: "other", key: "k1" }] }],
    });

    const { result } = renderHook(() => useSignOut());
    // Sign out alice — bob's open conversation must stay.
    await result.current(alice.accountId);
    expect(roomsStore.getState().selected).toEqual({
      accountId: bob.accountId,
      roomId: "!b:example.org",
    });

    // Now sign out bob — the open conversation closes and its timeline clears.
    await result.current(bob.accountId);
    expect(roomsStore.getState().selected).toBeNull();
    expect(timelineStore.getState().items).toEqual([]);
  });

  it("resets all mirror stores when the last account is signed out", async () => {
    accountsStore.getState().hydrateAll([alice]);
    roomsStore.getState().selectRoom({ accountId: alice.accountId, roomId: "!a:example.org" });
    roomsStore.getState().applyBatch({
      ops: [{ op: "reset", rooms: [room("!a:example.org", alice.accountId)] }],
      total: 1,
    });
    accountStatusStore.getState().setStatus(alice.accountId, "offline");

    const { result } = renderHook(() => useSignOut());
    await result.current(alice.accountId);

    expect(roomsStore.getState().selected).toBeNull();
    expect(roomsStore.getState().rooms).toEqual([]);
    expect(roomsStore.getState().total).toBeNull();
    expect(timelineStore.getState().items).toEqual([]);
    // The signed-out account's per-account status entry is removed.
    expect(accountStatusStore.getState().statuses).toEqual({});
    expect(accountsStore.getState().accounts).toEqual([]);
  });

  it("prunes the signed-out account's draft markers and mirrored bodies, keeping the others", async () => {
    accountsStore.getState().hydrateAll([alice, bob]);
    draftsStore.getState().mark(alice.accountId, "!a:example.org", true);
    draftsStore.getState().applyRemote(alice.accountId, "!a:example.org", "alice unsent text", 1);
    draftsStore.getState().mark(bob.accountId, "!b:example.org", true);
    draftsStore.getState().applyRemote(bob.accountId, "!b:example.org", "bob unsent text", 2);

    const { result } = renderHook(() => useSignOut());
    await result.current(alice.accountId);

    // Alice's marker and mirrored body no longer linger in memory.
    expect(draftsStore.getState().keys.has(`${alice.accountId} !a:example.org`)).toBe(false);
    expect(draftsStore.getState().remote.has(`${alice.accountId} !a:example.org`)).toBe(false);
    // Bob (still signed in) is untouched.
    expect(draftsStore.getState().keys.has(`${bob.accountId} !b:example.org`)).toBe(true);
    expect(draftsStore.getState().remote.has(`${bob.accountId} !b:example.org`)).toBe(true);
  });

  it("removes the signed-out account's status entry, keeping the others", async () => {
    accountsStore.getState().hydrateAll([alice, bob]);
    accountStatusStore.getState().setStatus(alice.accountId, "online");
    accountStatusStore.getState().setStatus(bob.accountId, "offline");

    const { result } = renderHook(() => useSignOut());
    await result.current(alice.accountId);

    expect(accountStatusStore.getState().statuses).toEqual({ [bob.accountId]: "offline" });
  });

  it("the default (keep-archive) path never calls deleteAccountArchive", async () => {
    accountsStore.getState().hydrateAll([alice, bob]);

    const { result } = renderHook(() => useSignOut());
    await result.current(alice.accountId);

    expect(signOut).toHaveBeenCalledWith(alice.accountId);
    expect(deleteAccountArchive).not.toHaveBeenCalled();
  });

  it("the delete path signs out then deletes the archive (in order) and removes the account", async () => {
    accountsStore.getState().hydrateAll([alice, bob]);
    const order: string[] = [];
    signOut.mockImplementation(async () => {
      order.push("signOut");
    });
    deleteAccountArchive.mockImplementation(async () => {
      order.push("deleteArchive");
    });

    const { result } = renderHook(() => useSignOut());
    await result.current(alice.accountId, { deleteArchive: true });

    expect(order).toEqual(["signOut", "deleteArchive"]);
    expect(signOut).toHaveBeenCalledWith(alice.accountId);
    expect(deleteAccountArchive).toHaveBeenCalledWith(alice.accountId);
    expect(accountsStore.getState().accounts.map((a) => a.accountId)).toEqual([bob.accountId]);
    expect(toastError).not.toHaveBeenCalled();
  });

  it("a purge failure still removes the account (sign-out not rolled back) and toasts", async () => {
    accountsStore.getState().hydrateAll([alice]);
    deleteAccountArchive.mockRejectedValue(new Error("purge failed"));

    const { result } = renderHook(() => useSignOut());
    // The last account: the delete path must not throw despite the purge failure,
    // must still remove the account, and must surface the failure via a toast.
    await result.current(alice.accountId, { deleteArchive: true });

    expect(signOut).toHaveBeenCalledWith(alice.accountId);
    expect(deleteAccountArchive).toHaveBeenCalledWith(alice.accountId);
    expect(accountsStore.getState().accounts).toEqual([]);
    expect(toastError).toHaveBeenCalledTimes(1);
  });
});
