/**
 * Sign-out hook (AD-10, Story 1.8).
 *
 * Returns an async handler bound to the current account id. It awaits the
 * Rust-authoritative `sign_out` command (local-only: tears down supervision and
 * deletes the account's SDK store dir + Keychain session + registry row), then
 * resets the frontend stores in a deliberate order:
 *
 *   1. `roomsStore.selectRoom(null)` — close the open conversation (room-list
 *      `clear()` deliberately preserves `selectedRoomId`, so it must be reset
 *      explicitly here).
 *   2. `roomsStore.clear()` — drop the streamed room window.
 *   3. `timelineStore.clear()` — drop the open room's mirrored timeline.
 *   4. `connectionStore.reset()` — return the connectivity pill to its default.
 *   5. `accountsStore.clear()` **last** — this unmounts the shell, and the pane
 *      cleanups (timeline/connection unsubscribe) run as the components leave.
 *
 * The handler returns `null` (no-ops) when there is no current account.
 */
import { useCallback } from "react";
import { signOut } from "@/lib/ipc/client";
import { accountsStore, useAccountsStore } from "@/lib/stores/accounts";
import { connectionStore } from "@/lib/stores/connection";
import { roomsStore } from "@/lib/stores/rooms";
import { timelineStore } from "@/lib/stores/timeline";

export function useSignOut(): () => Promise<void> {
  const accountId = useAccountsStore((s) => s.currentAccount?.accountId ?? null);

  return useCallback(async () => {
    if (accountId === null) {
      return;
    }
    await signOut(accountId);
    // Reset stores in order; `accountsStore.clear()` last so the shell unmounts
    // only after the mirror stores are cleared.
    roomsStore.getState().selectRoom(null);
    roomsStore.getState().clear();
    timelineStore.getState().clear();
    connectionStore.getState().reset();
    accountsStore.getState().clear();
  }, [accountId]);
}
