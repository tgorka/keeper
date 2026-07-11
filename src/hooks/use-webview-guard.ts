/**
 * Capability-gated blank-webview reload guard (Story 14.4, tauri#14371) — the JS
 * stopgap trigger for the iOS resume-recovery path.
 *
 * While backgrounded, iOS can jettison the WKWebView web-content process; on
 * foreground the view can come back blank/frozen with no recovery. On the
 * reduced-capability (iOS/phone) tier — read from the capabilities mirror via
 * {@link useIsReducedCapabilityPlatform}, never user-agent/build flags — this hook
 * runs an animation-frame liveness probe on every real resume (`visibilitychange` →
 * visible after having been hidden) and on attach: a healthy webview services
 * `requestAnimationFrame`, so a frame observed inside the probe window confirms a
 * healthy render (and re-arms the guard); only MULTIPLE consecutive missed frame
 * windows declare the view blank/frozen — a single missed frame is indistinguishable
 * from a slow-but-healthy cold resume doing snapshot-then-diff re-subscribe work on a
 * constrained device, and a healthy webview must never be reloaded (Review R1).
 *
 * The recovery is one loop-guarded `location.reload()`: the attempt is recorded in
 * `sessionStorage` BEFORE reloading and verified read-back — if the flag cannot be
 * durably recorded the guard does NOT reload (a recoverable blank beats a reload
 * loop; the native `webViewWebContentProcessDidTerminate` upgrade path covers a
 * fully-dead context later — never built here). The stored flag is transferred into
 * per-document-load memory at attach (read + remove), so it suppresses at most the
 * one document generation that follows the reload — a stale flag from a prior
 * session can never suppress a legitimate future recovery. A confirmed healthy
 * render clears both, re-arming the guard. After the reload, the Rust-held nav
 * state ({@link useNavStatePersistence}) lands the user back on their last stack
 * level. Desktop attaches nothing.
 */
import { useEffect, useRef } from "react";
import { useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
import { reloadWebview } from "@/lib/webview-reload";

/** The `sessionStorage` key recording an in-flight reload attempt (loop guard). */
export const WEBVIEW_GUARD_FLAG_KEY = "keeper.webviewGuard.reloadAttempted";

/** How long each probe attempt waits for a frame before counting it missed. */
export const PROBE_FRAME_WINDOW_MS = 250;

/**
 * How many CONSECUTIVE missed frame windows declare the webview blank/frozen
 * (Review R1): one missed frame is a busy resume, not a dead view.
 */
export const PROBE_REQUIRED_MISSES = 3;

export function useWebviewGuard(): void {
  const isReducedCapability = useIsReducedCapabilityPlatform();
  // The per-document-load loop-guard memory: `null` = the stored flag has not been
  // transferred yet; `true` = a reload was already attempted for this recovery
  // chain (never reload again until a healthy render re-arms); `false` = armed.
  const blockedRef = useRef<boolean | null>(null);

  useEffect(() => {
    // Desktop tier (and pre-hydration): no probe, no listener — desktop resume
    // behavior is byte-for-byte unchanged.
    if (!isReducedCapability) {
      return;
    }

    // Transfer the stored attempt flag into this document load's memory (stamp per
    // document-load, Review R1): read it once, then remove it so a stale flag from
    // a prior session/generation can never suppress a later legitimate reload. The
    // in-memory block still guards THIS document — a post-reload page that is
    // still blank must not reload again (loop guard).
    if (blockedRef.current === null) {
      let attempted = false;
      try {
        attempted = sessionStorage.getItem(WEBVIEW_GUARD_FLAG_KEY) !== null;
        sessionStorage.removeItem(WEBVIEW_GUARD_FLAG_KEY);
      } catch {
        // Unreadable storage: treat as armed — the write path below is what must
        // be durable before any reload happens.
        attempted = false;
      }
      blockedRef.current = attempted;
    }

    let rafId: number | null = null;
    let timeoutId: number | null = null;
    let misses = 0;
    // Only a probe opened by a REAL resume may reload; the attach-time probe (a
    // cold boot / just-reloaded page) confirms health and re-arms but never
    // reloads, so a slow-but-healthy launch — the main thread saturated by
    // hydration + first render past the miss window — can't be mistaken for a
    // jettisoned view and reloaded (Review R2).
    let probeAllowReload = false;

    const cancelProbe = (): void => {
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
      if (timeoutId !== null) {
        window.clearTimeout(timeoutId);
        timeoutId = null;
      }
    };

    const onHealthy = (): void => {
      cancelProbe();
      // A confirmed healthy render re-arms the guard and clears the durable flag.
      blockedRef.current = false;
      try {
        sessionStorage.removeItem(WEBVIEW_GUARD_FLAG_KEY);
      } catch {
        // Best-effort — the in-memory re-arm above is what this document uses.
      }
    };

    const onBlank = (): void => {
      cancelProbe();
      if (!probeAllowReload) {
        // Attach-time probe (cold boot / recovered page): confirm-only, never
        // reload a view that was never observed to resume from a jettison.
        return;
      }
      if (blockedRef.current === true) {
        // Already reloaded once this recovery chain — never loop.
        return;
      }
      // Fail safe (Review R1): only reload when the one-shot attempt is durably
      // recorded. If the flag can't be written (and read back), a reload loop
      // would be unguarded — keep the recoverable blank instead.
      try {
        sessionStorage.setItem(WEBVIEW_GUARD_FLAG_KEY, String(Date.now()));
        if (sessionStorage.getItem(WEBVIEW_GUARD_FLAG_KEY) === null) {
          return;
        }
      } catch {
        return;
      }
      blockedRef.current = true;
      reloadWebview();
    };

    const attempt = (): void => {
      rafId = requestAnimationFrame(() => {
        rafId = null;
        onHealthy();
      });
      timeoutId = window.setTimeout(() => {
        timeoutId = null;
        if (rafId !== null) {
          cancelAnimationFrame(rafId);
          rafId = null;
        }
        misses += 1;
        if (misses >= PROBE_REQUIRED_MISSES) {
          onBlank();
        } else {
          attempt();
        }
      }, PROBE_FRAME_WINDOW_MS);
    };

    const startProbe = (allowReload: boolean): void => {
      cancelProbe();
      misses = 0;
      probeAllowReload = allowReload;
      attempt();
    };

    // Probe on a REAL resume only: visible after having been hidden. Going hidden
    // cancels any in-flight probe — rAF is suspended while hidden, so letting the
    // timers keep counting would fabricate misses on a perfectly healthy view.
    let wasHidden = document.visibilityState === "hidden";
    const onVisibilityChange = (): void => {
      if (document.visibilityState === "hidden") {
        wasHidden = true;
        cancelProbe();
        return;
      }
      if (document.visibilityState === "visible" && wasHidden) {
        wasHidden = false;
        startProbe(true);
      }
    };
    document.addEventListener("visibilitychange", onVisibilityChange);

    // Attach-time confirmation: a fresh (possibly just-reloaded) visible document
    // proves its health once, clearing the attempt flag and re-arming the guard —
    // without it, a recovered page would carry the block forever. Confirm-only
    // (no reload): a cold boot never resumed from a jettison (Review R2).
    if (document.visibilityState === "visible") {
      startProbe(false);
    }

    return () => {
      document.removeEventListener("visibilitychange", onVisibilityChange);
      cancelProbe();
    };
  }, [isReducedCapability]);
}
