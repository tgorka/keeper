/**
 * Rendered-window projection for the keyboard chords (Story 9.2).
 *
 * The quick-switcher and unread-jump chords operate on exactly the window the
 * chat-list pane is currently rendering: the inbox window when
 * `primaryView === "inbox"`, the archive window when `"archive"`, and nothing when
 * "bridges"/"approval" replaces the cluster. The account-switcher's pure display
 * filter is applied the same way the pane applies it — hiding non-matching rows
 * without touching the Rust-authoritative order. This is a read-only projection: it
 * only slices/filters the arrays Rust already ordered, never re-sorting or
 * re-deriving order in TS (AD-20).
 */
import type { InboxRoomVm } from "@/lib/ipc/client";
import type { PrimaryView } from "@/lib/stores/primary-view";

/**
 * Resolve the currently-rendered chat-list window for the keyboard chords.
 *
 * Returns the account-filtered inbox or archive rooms (in Rust recency order), or
 * `null` when the active view is "bridges"/"approval" — those replace the chat-list
 * cluster entirely, so the chords must no-op there.
 */
export function renderedWindowRooms(
  view: PrimaryView,
  inboxRooms: InboxRoomVm[],
  archiveRooms: InboxRoomVm[],
  filterAccountId: string | null,
): InboxRoomVm[] | null {
  if (view !== "inbox" && view !== "archive") {
    return null;
  }
  const active = view === "archive" ? archiveRooms : inboxRooms;
  return filterAccountId === null
    ? active
    : active.filter((room) => room.accountId === filterAccountId);
}
