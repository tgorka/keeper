/**
 * Pure search-filter construction (Story 5.4, FR-34).
 *
 * The search overlay's chips describe *intent*; the archive engine takes a flat
 * {@link SearchFilterVm} of `roomIds`/`accountIds`/`sender`/date bounds. This
 * module is the single, unit-testable place that translates one to the other —
 * critically it resolves a **Network** selection (a live per-room bridge label,
 * not an archive column) and a **Chat** selection to their `roomId` sets from the
 * already-merged room store *before* the call, so the tauri-free engine stays pure
 * and offline-capable. No Matrix or search logic lives here beyond that mapping.
 */
import type { InboxRoomVm, SearchFilterVm } from "@/lib/ipc/client";

/**
 * The overlay's filter selections, all optional. A `chatLock` (set when opened
 * in-chat via the in-chat shortcut) forces the room/account scope regardless of
 * the other chips.
 */
export interface SearchUiFilter {
  /** The verbatim query text (passed through untouched). */
  query: string;
  /**
   * A selected Chat, keyed by its owning account + room. Resolves to that single
   * room id in `roomIds` (and, so cross-account rooms stay disambiguated, its
   * account id in `accountIds`).
   */
  chat: { accountId: string; roomId: string } | null;
  /**
   * A selected Network by its display name (cross-account). Resolves to the set of
   * `roomId` from the merged room store whose `network` equals it (deduped, all
   * accounts).
   */
  network: string | null;
  /** A selected account id, or `null` for all accounts. */
  accountId: string | null;
  /**
   * A sender filter — an exact full Matrix user id (the engine matches
   * `events.sender = ?`). Trimmed; blank means no sender filter.
   */
  sender: string | null;
  /**
   * Inclusive date range in local calendar days (`YYYY-MM-DD` strings), or `null`.
   * `startDate` maps to that day's start-of-day ms; `endDate` to its
   * end-of-day-inclusive ms.
   */
  startDate: string | null;
  endDate: string | null;
  /**
   * A hard scope lock applied when the surface was opened in-chat: its room is
   * forced into `roomIds` and its account into `accountIds`, overriding the
   * Chat/Network/Account chips.
   */
  chatLock: { accountId: string; roomId: string } | null;
}

/** Milliseconds in one day (used for the end-of-day-inclusive bound). */
const DAY_MS = 24 * 60 * 60 * 1000;

/**
 * The sentinel room id for "a Network filter was chosen but it has no rooms in the
 * merged window". It guarantees the engine's `room_ids` IN-clause matches nothing,
 * producing an honest empty result rather than the "empty means unrestricted"
 * all-rooms behavior. The value is not a valid Matrix room id, so it can never
 * accidentally match a real row.
 */
export const EMPTY_MATCH_ROOM_ID = " keeper-network-empty";

/**
 * Start-of-day epoch ms for a `YYYY-MM-DD` local calendar date, or `null` when the
 * string is absent or unparsable. Uses the local timezone so "today" means the
 * user's day.
 */
function startOfDayMs(date: string | null): number | null {
  if (date === null || date.trim() === "") {
    return null;
  }
  const parts = date.split("-").map(Number);
  if (parts.length !== 3 || parts.some((n) => !Number.isFinite(n))) {
    return null;
  }
  const [year, month, day] = parts;
  const d = new Date(year, month - 1, day, 0, 0, 0, 0);
  // Reject rollover inputs (e.g. month 13, day 45, Feb 30): `new Date` silently
  // normalizes them into a valid — but wrong — day, so a NaN check never trips.
  // Verify the constructed date's components round-trip the input instead.
  if (d.getFullYear() !== year || d.getMonth() !== month - 1 || d.getDate() !== day) {
    return null;
  }
  return d.getTime();
}

/**
 * End-of-day-inclusive epoch ms for a `YYYY-MM-DD` local calendar date, or `null`.
 * Computed as the next day's start minus one ms so the whole selected day is
 * included (the engine's `endTs` bound is inclusive).
 */
function endOfDayInclusiveMs(date: string | null): number | null {
  const start = startOfDayMs(date);
  return start === null ? null : start + DAY_MS - 1;
}

/**
 * Resolve a Network display name to the deduped set of room ids that carry it,
 * across every account in the merged room store. A Network with zero rooms in the
 * merged window resolves to an empty set.
 */
function roomIdsForNetwork(network: string, rooms: InboxRoomVm[]): string[] {
  const set = new Set<string>();
  for (const room of rooms) {
    if (room.network === network) {
      set.add(room.roomId);
    }
  }
  return [...set];
}

/**
 * Build the {@link SearchFilterVm} from the overlay's UI selections and the merged
 * room store. Pure and total: no IPC, no clock reads beyond the local-date math.
 *
 * Precedence: a `chatLock` (in-chat scope) wins outright — its room/account scope
 * the filter. Otherwise a Chat selection contributes its room + account; a Network
 * selection contributes its resolved room-id set; an Account selection contributes
 * its account id. Empty `roomIds`/`accountIds` mean unrestricted. A Network with 0
 * matching rooms still emits an unmatchable sentinel so the result is an honest
 * empty, not "everything".
 */
export function buildSearchFilter(ui: SearchUiFilter, rooms: InboxRoomVm[]): SearchFilterVm {
  const roomIds: string[] = [];
  const accountIds: string[] = [];

  if (ui.chatLock !== null) {
    // In-chat scope: lock to exactly this Chat, ignoring the other chips.
    roomIds.push(ui.chatLock.roomId);
    accountIds.push(ui.chatLock.accountId);
  } else {
    if (ui.chat !== null) {
      roomIds.push(ui.chat.roomId);
      accountIds.push(ui.chat.accountId);
    }
    if (ui.network !== null) {
      for (const id of roomIdsForNetwork(ui.network, rooms)) {
        if (!roomIds.includes(id)) {
          roomIds.push(id);
        }
      }
    }
    if (ui.accountId !== null && !accountIds.includes(ui.accountId)) {
      accountIds.push(ui.accountId);
    }
  }

  const sender = ui.sender === null || ui.sender.trim() === "" ? null : ui.sender.trim();
  // A Network with 0 rooms in the window MUST still restrict (to empty) rather than
  // fall back to "empty means unrestricted": emit the unmatchable sentinel so the
  // result set empties honestly.
  const networkPickedButEmpty = ui.chatLock === null && ui.network !== null && roomIds.length === 0;

  return {
    query: ui.query,
    accountIds,
    roomIds: networkPickedButEmpty ? [EMPTY_MATCH_ROOM_ID] : roomIds,
    sender,
    startTs: startOfDayMs(ui.startDate),
    endTs: endOfDayInclusiveMs(ui.endDate),
    limit: null,
  };
}
