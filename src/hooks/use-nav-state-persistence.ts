/**
 * Capability-gated nav-state persistence (Story 14.4) — keeps the last phone-stack
 * level `(accountId, roomId, detailOpen)` in Rust so a webview reload after a
 * WKWebView content-process jettison (tauri#14371) lands the user exactly where they
 * were. Nav *selection* only, never message/room data — after any reload the existing
 * `subscribeInbox`/`subscribeTimeline` mount effects re-deliver a full snapshot (AD-8).
 *
 * Only on the reduced-capability (iOS/phone) tier — read from the capabilities mirror
 * via {@link useIsReducedCapabilityPlatform}, never user-agent/build flags — does this
 * hook do anything; on desktop (and pre-hydration) it attaches nothing.
 *
 * Restore (once per app session): `navStateGet()` → re-select the stored room, then —
 * one macrotask later, so the phone shell's DW-109 close-on-selection guard has
 * settled — reopen the Detail level when `detailOpen` was stored. User navigation
 * always wins: a selection made before the read resolves (or during the detail-reopen
 * window) aborts the restore. The read is held in a ref so a StrictMode effect re-run
 * reuses the same in-flight promise instead of re-issuing (or racing) it.
 *
 * Report (after the restore settles): mirrors `use-active-chat-reporter`'s shape —
 * value-deduped pushes of each nav change; a Chat open ⇒ `navStateSet`, back to the
 * Inbox ⇒ `navStateClear`. The dedup baseline is seeded from the *intended* restored
 * state (including `detailOpen: true`) before any report, so the transient
 * selected-but-detail-not-yet-reopened window never writes `detailOpen: false` back to
 * Rust — a re-jettison in that window would otherwise lose the Detail level. IPC
 * errors are swallowed (best-effort, no toast). Unmount does NOT clear the Rust state
 * — surviving the death of this JS context is the whole point.
 */
import { useEffect, useRef } from "react";
import { type NavState, navStateClear, navStateGet, navStateSet } from "@/lib/ipc/client";
import { useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
import { detailStore } from "@/lib/stores/detail-ui";
import { roomsStore } from "@/lib/stores/rooms";

export function useNavStatePersistence(): void {
  const isReducedCapability = useIsReducedCapabilityPlatform();
  // The one-shot restore read, shared across effect re-runs (StrictMode-safe: a
  // re-run reuses the same promise, so it can never race a second read or clear
  // state out from under the first). Rejections fold to `null` (start at the Inbox).
  const restoreReadRef = useRef<Promise<NavState | null> | null>(null);
  // Whether the restore has already been applied (or deliberately skipped) this app
  // session, so a later effect re-run (tier flip off/on) never re-applies a stale
  // restore over live user navigation.
  const restoreSettledRef = useRef(false);

  useEffect(() => {
    // Desktop tier (and pre-hydration): no persistence, no restore — desktop
    // navigation behavior is byte-for-byte unchanged.
    if (!isReducedCapability) {
      return;
    }

    let cancelled = false;
    // Set once the restore read has settled for THIS effect run — nothing is
    // written to Rust before then, so a slow read can never be clobbered by an
    // attach-time report (and a StrictMode re-run can't clear-before-get).
    let restoreDone = false;
    // Any user navigation observed before the restore applies wins over it.
    let userNavigated = false;
    // Suppresses the `userNavigated` marking while the restore itself drives the
    // stores (our own `selectRoom`/`openDetail` are not user navigation).
    let applyingRestore = false;

    // ---- Reporter (value-deduped, mirrors use-active-chat-reporter) ---------
    let hasBaseline = false;
    let lastAccountId: string | null = null;
    let lastRoomId: string | null = null;
    let lastDetailOpen = false;

    const report = (): void => {
      if (!restoreDone) {
        return;
      }
      const selected = roomsStore.getState().selected;
      const accountId = selected?.accountId ?? null;
      const roomId = selected?.roomId ?? null;
      // Detail is a level ON TOP of a Room: with no selection there is no detail
      // level to restore, so the persisted flag is only meaningful alongside one.
      const detailOpen = selected !== null && detailStore.getState().open;
      if (
        hasBaseline &&
        accountId === lastAccountId &&
        roomId === lastRoomId &&
        detailOpen === lastDetailOpen
      ) {
        return;
      }
      hasBaseline = true;
      lastAccountId = accountId;
      lastRoomId = roomId;
      lastDetailOpen = detailOpen;
      // Best-effort: swallow IPC errors (no toast).
      if (selected === null) {
        void navStateClear().catch(() => {});
      } else {
        void navStateSet(selected, detailOpen).catch(() => {});
      }
    };

    const unsubscribeRooms = roomsStore.subscribe((state, prevState) => {
      if (state.selected !== prevState.selected) {
        if (!applyingRestore) {
          userNavigated = true;
        }
        report();
      }
    });
    const unsubscribeDetail = detailStore.subscribe((state, prevState) => {
      if (state.open !== prevState.open) {
        if (!applyingRestore) {
          userNavigated = true;
        }
        report();
      }
    });

    // ---- Restore ------------------------------------------------------------
    const finishRestore = (): void => {
      restoreDone = true;
      // Push the current state once the gate opens — deduped against the seeded
      // baseline, so a clean restore writes nothing and any divergence (user nav,
      // cold launch) is reported exactly once.
      report();
    };

    if (restoreReadRef.current === null) {
      restoreReadRef.current = navStateGet().catch(() => null);
    }
    const read = restoreReadRef.current;
    void read.then((nav) => {
      if (cancelled) {
        return;
      }
      if (restoreSettledRef.current) {
        // A previous effect run already applied/skipped the restore — this run
        // only reports.
        finishRestore();
        return;
      }
      restoreSettledRef.current = true;
      // User navigation made before the read resolves always wins — including a
      // selection that already existed when the hook attached.
      if (nav === null || userNavigated || roomsStore.getState().selected !== null) {
        finishRestore();
        return;
      }
      // Seed the reporter baseline from the INTENDED restored state (Review R1):
      // during the detail-reopen macrotask below the stores transiently read
      // `detailOpen: false`, and reporting that would overwrite the stored `true`.
      hasBaseline = true;
      lastAccountId = nav.accountId;
      lastRoomId = nav.roomId;
      lastDetailOpen = nav.detailOpen;

      applyingRestore = true;
      roomsStore.getState().selectRoom({ accountId: nav.accountId, roomId: nav.roomId });
      applyingRestore = false;
      if (!nav.detailOpen) {
        finishRestore();
        return;
      }
      // DW-109 ordering: the phone shell closes Detail on any selection change in a
      // post-commit effect, so reopening Detail is deferred one macrotask — after
      // that guard has settled — and only if the user hasn't navigated meanwhile.
      window.setTimeout(() => {
        if (cancelled) {
          return;
        }
        const selected = roomsStore.getState().selected;
        const stillRestored =
          !userNavigated &&
          selected?.accountId === nav.accountId &&
          selected?.roomId === nav.roomId;
        if (stillRestored) {
          applyingRestore = true;
          detailStore.getState().openDetail();
          applyingRestore = false;
        }
        finishRestore();
      }, 0);
    });

    return () => {
      cancelled = true;
      unsubscribeRooms();
      unsubscribeDetail();
      // Deliberately NO navStateClear here: the Rust-held state must outlive this
      // JS context (that is the entire recovery guarantee).
    };
  }, [isReducedCapability]);
}
