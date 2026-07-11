/**
 * Capability-gated stale-resume "Connecting…" pill state (Story 14.4, FR-53/FR-62).
 *
 * After a long iOS suspension the resumed sliding-sync takes a moment (and, when the
 * Rust lifecycle entry judged the session stale, an extra full restart —
 * matrix-rust-sdk#3935) to answer. This hook gives the phone Inbox an honest,
 * transient indicator for exactly that window: on a real resume (`visibilitychange`
 * → visible after having been hidden) a short show-delay timer starts; a fresh
 * resume settles before it fires and the pill never appears, while a stale
 * reconnect crosses it and `connecting` flips true.
 *
 * Settling keys on a status transition that represents the resumed sync actually
 * answering — some account's status CHANGING to `"online"` (diffing `state` against
 * `prevState`) — never on a bare store tick: on a multi-account phone an unrelated
 * account's no-op/same-value write must NOT hide the pill while the watched sync is
 * still reconnecting (Review R1). A timeout backstop guarantees the pill always
 * clears (a resume that stays offline simply drops the pill — the persistent
 * offline surface is Story 14.6's concern, never this one's), so it can never
 * become a stuck spinner.
 *
 * Reduced-capability (iOS/phone) tier only, read from the capabilities mirror via
 * {@link useIsReducedCapabilityPlatform}: on desktop (and pre-hydration) this
 * returns `false` and attaches nothing.
 */
import { useEffect, useState } from "react";
import { accountStatusStore } from "@/lib/stores/account-status";
import { useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";

/**
 * How long a resume must go unanswered before the pill shows. A fresh resume's
 * first status tick beats this comfortably, so the pill never flashes on the
 * common path; a stale reconnect (carrying the restart latency) crosses it.
 */
export const STALE_RESUME_PILL_SHOW_DELAY_MS = 1000;

/**
 * Fallback ceiling on the pill: it normally clears on the sync answering, but a
 * fully offline (tickless) resume must still resolve — never a stuck indicator.
 */
export const STALE_RESUME_PILL_TIMEOUT_MS = 15000;

export function useStaleResumePill(): boolean {
  const isReducedCapability = useIsReducedCapabilityPlatform();
  const [connecting, setConnecting] = useState(false);

  useEffect(() => {
    // Desktop tier (and pre-hydration): no listener, always false.
    if (!isReducedCapability) {
      return;
    }

    let showTimer: number | null = null;
    let backstopTimer: number | null = null;
    let unsubscribe: (() => void) | null = null;

    // End the current resume window: cancel timers, drop the status watch, hide
    // the pill. Idempotent — safe to call from every path.
    const settle = (): void => {
      if (showTimer !== null) {
        window.clearTimeout(showTimer);
        showTimer = null;
      }
      if (backstopTimer !== null) {
        window.clearTimeout(backstopTimer);
        backstopTimer = null;
      }
      unsubscribe?.();
      unsubscribe = null;
      setConnecting(false);
    };

    const onResume = (): void => {
      // A new resume supersedes any window still in flight.
      settle();
      // Settle only on real progress: an account's status CHANGING to online —
      // an unrelated same-value tick (multi-account) or an offline churn must
      // not hide the pill while the resumed sync is still reconnecting.
      unsubscribe = accountStatusStore.subscribe((state, prevState) => {
        const progressed = Object.entries(state.statuses).some(
          ([accountId, status]) =>
            status === "online" && prevState.statuses[accountId] !== "online",
        );
        if (progressed) {
          settle();
        }
      });
      showTimer = window.setTimeout(() => {
        showTimer = null;
        setConnecting(true);
      }, STALE_RESUME_PILL_SHOW_DELAY_MS);
      backstopTimer = window.setTimeout(settle, STALE_RESUME_PILL_TIMEOUT_MS);
    };

    // Only a REAL resume (visible after having been hidden) opens a window — the
    // attach-time visible state is a normal boot, not a reconnect.
    let wasHidden = document.visibilityState === "hidden";
    const onVisibilityChange = (): void => {
      if (document.visibilityState === "hidden") {
        wasHidden = true;
        // Going hidden ends the window — nothing honest to indicate off-screen.
        settle();
        return;
      }
      if (document.visibilityState === "visible" && wasHidden) {
        wasHidden = false;
        onResume();
      }
    };
    document.addEventListener("visibilitychange", onVisibilityChange);

    return () => {
      document.removeEventListener("visibilitychange", onVisibilityChange);
      settle();
    };
  }, [isReducedCapability]);

  return isReducedCapability ? connecting : false;
}
