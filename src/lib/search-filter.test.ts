import { describe, expect, it } from "vitest";
import type { InboxRoomVm } from "@/lib/ipc/client";
import { buildSearchFilter, EMPTY_MATCH_ROOM_ID, type SearchUiFilter } from "@/lib/search-filter";

function room(
  partial: Partial<InboxRoomVm> & Pick<InboxRoomVm, "accountId" | "roomId">,
): InboxRoomVm {
  return {
    accountId: partial.accountId,
    hueIndex: partial.hueIndex ?? 0,
    roomId: partial.roomId,
    displayName: partial.displayName ?? partial.roomId,
    lastMessage: null,
    timestamp: null,
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isFavourite: false,
    isPinned: false,
    network: partial.network ?? null,
  };
}

const EMPTY_UI: SearchUiFilter = {
  query: "hello",
  chat: null,
  network: null,
  accountId: null,
  sender: null,
  startDate: null,
  endDate: null,
  chatLock: null,
};

describe("buildSearchFilter", () => {
  it("passes the query verbatim and leaves empty account/room lists unrestricted", () => {
    const f = buildSearchFilter(EMPTY_UI, []);
    expect(f.query).toBe("hello");
    expect(f.accountIds).toEqual([]);
    expect(f.roomIds).toEqual([]);
    expect(f.sender).toBeNull();
    expect(f.startTs).toBeNull();
    expect(f.endTs).toBeNull();
    expect(f.limit).toBeNull();
  });

  it("resolves a Chat selection to its room id and account id", () => {
    const f = buildSearchFilter(
      { ...EMPTY_UI, chat: { accountId: "acct-1", roomId: "!r1:x" } },
      [],
    );
    expect(f.roomIds).toEqual(["!r1:x"]);
    expect(f.accountIds).toEqual(["acct-1"]);
  });

  it("resolves a Network selection to its deduped room-id set across accounts", () => {
    const rooms = [
      room({ accountId: "a1", roomId: "!r1:x", network: "Telegram" }),
      room({ accountId: "a2", roomId: "!r2:x", network: "Telegram" }),
      room({ accountId: "a1", roomId: "!r3:x", network: "WhatsApp" }),
      room({ accountId: "a1", roomId: "!r1:x", network: "Telegram" }), // dup room id
    ];
    const f = buildSearchFilter({ ...EMPTY_UI, network: "Telegram" }, rooms);
    expect(new Set(f.roomIds)).toEqual(new Set(["!r1:x", "!r2:x"]));
  });

  it("empties the result honestly when a Network has no rooms in the window", () => {
    const f = buildSearchFilter({ ...EMPTY_UI, network: "Telegram" }, []);
    // Not [] (which would mean unrestricted): an unmatchable sentinel.
    expect(f.roomIds).toEqual([EMPTY_MATCH_ROOM_ID]);
  });

  it("resolves an Account selection to its account id", () => {
    const f = buildSearchFilter({ ...EMPTY_UI, accountId: "acct-9" }, []);
    expect(f.accountIds).toEqual(["acct-9"]);
    expect(f.roomIds).toEqual([]);
  });

  it("passes a trimmed sender exactly and drops a blank one", () => {
    expect(buildSearchFilter({ ...EMPTY_UI, sender: "  @bob:x " }, []).sender).toBe("@bob:x");
    expect(buildSearchFilter({ ...EMPTY_UI, sender: "   " }, []).sender).toBeNull();
  });

  it("maps a date range to start-of-day and end-of-day-inclusive ms bounds", () => {
    const f = buildSearchFilter(
      { ...EMPTY_UI, startDate: "2026-07-05", endDate: "2026-07-05" },
      [],
    );
    const start = new Date(2026, 6, 5, 0, 0, 0, 0).getTime();
    const end = start + 24 * 60 * 60 * 1000 - 1;
    expect(f.startTs).toBe(start);
    expect(f.endTs).toBe(end);
    // The end bound is strictly after the start bound (inclusive whole day).
    expect(f.endTs).toBeGreaterThan(f.startTs ?? 0);
  });

  it("maps a rollover or malformed date to a null bound instead of a wrong one", () => {
    // `new Date` silently normalizes month 13 / day 45 into a valid-but-wrong day;
    // the helper must reject such inputs (round-trip check) rather than emit a bound.
    const f = buildSearchFilter(
      { ...EMPTY_UI, startDate: "2026-13-45", endDate: "2026-02-30" },
      [],
    );
    expect(f.startTs).toBeNull();
    expect(f.endTs).toBeNull();
  });

  it("chat-scope lock overrides the Chat/Network/Account chips", () => {
    const rooms = [room({ accountId: "a2", roomId: "!other:x", network: "Telegram" })];
    const f = buildSearchFilter(
      {
        ...EMPTY_UI,
        chatLock: { accountId: "acct-lock", roomId: "!locked:x" },
        // These must be ignored under a lock.
        chat: { accountId: "a2", roomId: "!other:x" },
        network: "Telegram",
        accountId: "a2",
      },
      rooms,
    );
    expect(f.roomIds).toEqual(["!locked:x"]);
    expect(f.accountIds).toEqual(["acct-lock"]);
  });
});
