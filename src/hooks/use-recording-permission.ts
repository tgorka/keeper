/**
 * Live Screen Recording permission pre-flight hook (Story 16.5, FR-67, AD-36).
 *
 * Fetches the honest tri-state {@link RecordingPermissionVm} through the Rust
 * `recording_permission` command on mount, and re-detects on every
 * `visibilitychange` → visible and window `focus` — the user may grant (or
 * revoke) the permission in System Settings and return, and the row must flip
 * without a relaunch where the OS allows. Detection is always live (the Rust
 * side spawns a fresh sidecar probe per call, bounded by a timeout); nothing is
 * cached optimistically here — the state held is only the latest probe result.
 *
 * Error-safe by design: every IPC failure (sidecar unavailable / hung / iOS) is
 * swallowed to the safe default — Start stays disabled and the row shows the
 * request affordance — never a crash, never an infinite spinner.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import type { RecordingPermissionVm } from "@/lib/ipc/client";
import {
  openScreenRecordingSettings,
  recordingPermission,
  requestScreenRecordingPermission,
} from "@/lib/ipc/client";

/**
 * The safe default while no probe has resolved (and after any failed probe):
 * not yet requested, Start disabled. Frozen so no code path can mutate the
 * shared fallback in place.
 */
export const DEFAULT_RECORDING_PERMISSION: RecordingPermissionVm = Object.freeze({
  screenRecording: "notYetRequested",
  canStart: false,
});

export interface UseRecordingPermission {
  /** The latest resolved pre-flight (the safe default until a probe lands). */
  permission: RecordingPermissionVm;
  /** Trigger the OS request (one real prompt per app lifetime where allowed). */
  request: () => Promise<void>;
  /** Deep-link to the Screen Recording pane in System Settings (best-effort). */
  openSettings: () => void;
  /** Re-run the live pre-flight now (the focus/visibility paths call this). */
  refresh: () => Promise<void>;
}

export function useRecordingPermission(): UseRecordingPermission {
  const [permission, setPermission] = useState<RecordingPermissionVm>(DEFAULT_RECORDING_PERMISSION);
  // Guard state writes after unmount without tearing down in-flight probes.
  const mounted = useRef(true);
  // Monotonic probe token, shared across refresh() and request(). macOS commonly
  // fires `focus` and `visibilitychange` back-to-back on a window return, so
  // several probes (each a fresh sidecar spawn) can be in flight at once. Only the
  // most-recently-initiated probe may write state — a slower earlier probe must not
  // clobber a newer result with a stale grant read (last-initiated wins).
  const seq = useRef(0);

  const refresh = useCallback(async () => {
    const token = ++seq.current;
    try {
      const vm = await recordingPermission();
      if (mounted.current && token === seq.current) {
        setPermission(vm);
      }
    } catch {
      // Safe default: a failed probe must never crash or spin — Start stays
      // disabled and the row keeps its request affordance.
      if (mounted.current && token === seq.current) {
        setPermission(DEFAULT_RECORDING_PERMISSION);
      }
    }
  }, []);

  const request = useCallback(async () => {
    const token = ++seq.current;
    try {
      const vm = await requestScreenRecordingPermission();
      if (mounted.current && token === seq.current) {
        setPermission(vm);
      }
    } catch {
      // A failed request round-trip degrades to a fresh live probe (which itself
      // degrades to the safe default) — never a crash. refresh() takes a newer
      // token, so it supersedes this request's dropped write.
      await refresh();
    }
  }, [refresh]);

  const openSettings = useCallback(() => {
    // Best-effort deep link; a rejection is swallowed (the user can still open
    // System Settings manually).
    void openScreenRecordingSettings().catch(() => {});
  }, []);

  useEffect(() => {
    mounted.current = true;
    // Live-detect at render (never cached), then re-detect on every return to
    // the window: `visibilitychange` → visible covers un-hiding, `focus` covers
    // the System Settings round-trip where the document never went hidden.
    void refresh();
    const onVisibility = (): void => {
      if (document.visibilityState === "visible") {
        void refresh();
      }
    };
    const onFocus = (): void => {
      void refresh();
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      mounted.current = false;
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [refresh]);

  return { permission, request, openSettings, refresh };
}
