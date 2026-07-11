/**
 * Capability-gated active-chat reporter (Story 14.3, AD-18) — mirrors the
 * currently-open conversation to the shared Rust notify engine so a foreground
 * notification for the Chat already on screen is suppressed (visible-Chat suppression).
 *
 * Only on the reduced-capability (iOS/phone) tier — read from the capabilities mirror via
 * {@link useIsReducedCapabilityPlatform}, never from user-agent or build flags — does this
 * subscribe to {@link roomsStore}'s `selected` slice and push each change to
 * `activeChatSet`:
 *
 * - `selected = { accountId, roomId }` → `activeChatSet(selection)` → the core suppresses a
 *   banner for exactly that Chat.
 * - `selected = null` (no Chat open) → `activeChatSet(null)` → nothing is suppressed on
 *   that ground.
 *
 * On desktop the predicate is false and NO subscription is attached, so desktop
 * notification behavior is byte-for-byte unchanged (the Story 10.1 focus-suppression
 * deferral stays open for desktop). Before capabilities hydrate the predicate is also
 * false, so nothing reports until the iOS tier resolves.
 *
 * The current selection is reported once at attach (so a Chat already open when the hook
 * mounts, or when capabilities hydrate, is suppressed immediately), and the active Chat is
 * cleared on unmount / when the tier flips off. IPC errors are swallowed (best-effort, no
 * toast) — a failed report must never surface UI.
 */
import { useEffect } from "react";
import { activeChatSet } from "@/lib/ipc/client";
import { useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
import { roomsStore } from "@/lib/stores/rooms";

export function useActiveChatReporter(): void {
  const isReducedCapability = useIsReducedCapabilityPlatform();

  useEffect(() => {
    // Desktop tier (and pre-hydration): report nothing so desktop notification behavior
    // is untouched (the 10.1 desktop focus-suppression deferral stays open).
    if (!isReducedCapability) {
      return;
    }

    // Best-effort: swallow IPC errors (no toast). Dedupe by VALUE, not reference:
    // `selectRoom`/`requestFocus` (`rooms.ts`) store a fresh `{ accountId, roomId }`
    // literal on every call, so re-selecting the Chat already on screen changes
    // `selected`'s identity while its value is identical. Reporting per reference-change
    // would fire a redundant `activeChatSet` round-trip; keying on the value keeps the
    // reporter's contract ("push each change") honest. The first report always fires
    // (even the attach-time clear) so Rust's active-room state is seeded exactly as before.
    let reported = false;
    let lastAccountId: string | null = null;
    let lastRoomId: string | null = null;
    const report = (selection: { accountId: string; roomId: string } | null): void => {
      const nextAccountId = selection?.accountId ?? null;
      const nextRoomId = selection?.roomId ?? null;
      if (reported && nextAccountId === lastAccountId && nextRoomId === lastRoomId) {
        return;
      }
      reported = true;
      lastAccountId = nextAccountId;
      lastRoomId = nextRoomId;
      void activeChatSet(selection).catch(() => {});
    };

    // Report the CURRENT selection once at attach so a Chat already open (or one open when
    // capabilities hydrate) is suppressed immediately — a change-only subscription would
    // miss that ordering.
    report(roomsStore.getState().selected);

    const unsubscribe = roomsStore.subscribe((state, prevState) => {
      if (state.selected !== prevState.selected) {
        report(state.selected);
      }
    });

    return () => {
      unsubscribe();
      // Leaving the reduced tier (or unmounting) clears the active Chat so no stale
      // selection keeps suppressing notifications.
      report(null);
    };
  }, [isReducedCapability]);
}
