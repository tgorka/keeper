/**
 * Live recording permission pre-flight hook (Story 16.5, FR-67, AD-36;
 * mic/camera legs Story 20.2).
 *
 * Fetches the honest {@link RecordingPermissionVm} through the Rust
 * `recording_permission` command on mount, re-detects on every
 * `visibilitychange` → visible and window `focus` — the user may grant (or
 * revoke) a permission in System Settings and return, and the rows must flip
 * without a relaunch where the OS allows — and re-fetches whenever the mic or
 * webcam enabled state changes, so an enabled source's leg appears (and gates
 * Start) immediately. The enabled flags come from the same stores the setup
 * cards write ({@link useMicEnabled}/{@link useWebcamEnabled}); all three legs
 * resolve from one `getCapabilities` probe on the Rust side. Detection is
 * always live (a fresh sidecar probe per call, bounded by a timeout); nothing
 * is cached optimistically here — the state held is only the latest probe
 * result.
 *
 * Error-safe by design: every IPC failure (sidecar unavailable / hung / iOS) is
 * swallowed to the safe default — Start stays disabled and no row claims a
 * grant — never a crash, never an infinite spinner.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import type { RecordingPermissionVm } from "@/lib/ipc/client";
import {
  openCameraSettings as ipcOpenCameraSettings,
  openMicrophoneSettings as ipcOpenMicrophoneSettings,
  openScreenRecordingSettings,
  recordingPermission,
  requestCameraPermission,
  requestMicrophonePermission,
  requestScreenRecordingPermission,
} from "@/lib/ipc/client";
import { micEnabled as isMicEnabledNow, useMicEnabled } from "@/lib/stores/recording-mic";
import {
  webcamEnabled as isWebcamEnabledNow,
  useWebcamEnabled,
} from "@/lib/stores/recording-webcam";

/**
 * The safe default while no probe has resolved (and after any failed probe):
 * not yet requested, no source leg claimed, Start disabled. Frozen so no code
 * path can mutate the shared fallback in place.
 */
export const DEFAULT_RECORDING_PERMISSION: RecordingPermissionVm = Object.freeze({
  screenRecording: "notYetRequested",
  microphone: null,
  camera: null,
  canStart: false,
});

export interface UseRecordingPermission {
  /** The latest resolved pre-flight (the safe default until a probe lands). */
  permission: RecordingPermissionVm;
  /** Trigger the OS request (one real prompt per app lifetime where allowed). */
  request: () => Promise<void>;
  /** Deep-link to the Screen Recording pane in System Settings (best-effort). */
  openSettings: () => void;
  /** Trigger the OS microphone request, then re-probe the pre-flight (20.2). */
  requestMicrophone: () => Promise<void>;
  /** Deep-link to the Microphone pane in System Settings (best-effort). */
  openMicrophoneSettings: () => void;
  /** Trigger the OS camera request, then re-probe the pre-flight (20.2). */
  requestCamera: () => Promise<void>;
  /** Deep-link to the Camera pane in System Settings (best-effort). */
  openCameraSettings: () => void;
  /** Re-run the live pre-flight now (the focus/visibility paths call this). */
  refresh: () => Promise<void>;
}

export function useRecordingPermission(): UseRecordingPermission {
  const [permission, setPermission] = useState<RecordingPermissionVm>(DEFAULT_RECORDING_PERMISSION);
  // The enabled-source flags (Story 20.2), subscribed reactively only to drive
  // the enabled-change re-probe effect below (so a toggle makes the row
  // appear/disappear live). The probe/request paths themselves read the *live*
  // store value imperatively at call time (`isMicEnabledNow`/`isWebcamEnabledNow`)
  // rather than a rendered flag — this keeps `refresh`/`request` stable (bound
  // once) so a post-prompt re-sync callback can never capture a stale-flag
  // closure and probe with the wrong enabled state.
  const micOn = useMicEnabled();
  const webcamOn = useWebcamEnabled();
  // Guard state writes after unmount without tearing down in-flight probes.
  const mounted = useRef(true);
  // Monotonic probe token, shared across refresh() and the request paths. macOS
  // commonly fires `focus` and `visibilitychange` back-to-back on a window
  // return, so several probes (each a fresh sidecar spawn) can be in flight at
  // once. Only the most-recently-initiated probe may write state — a slower
  // earlier probe must not clobber a newer result with a stale grant read
  // (last-initiated wins).
  const seq = useRef(0);

  // Probe the live pre-flight. Callers on the focus/visibility and post-prompt
  // re-sync paths pass no flags and read the *live* store value imperatively —
  // robust to React render timing and free of stale-closure capture. The
  // enabled-change effect passes the flags that triggered it explicitly (they
  // are exact for that run). `refresh` itself is stable so those callback paths
  // never capture an out-of-date flag.
  const refresh = useCallback(async (micOverride?: boolean, camOverride?: boolean) => {
    const token = ++seq.current;
    const mic = micOverride ?? isMicEnabledNow();
    const cam = camOverride ?? isWebcamEnabledNow();
    try {
      const vm = await recordingPermission(mic, cam);
      if (mounted.current && token === seq.current) {
        setPermission(vm);
      }
    } catch {
      // Safe default: a failed probe must never crash or spin — Start stays
      // disabled and the rows keep their request affordances.
      if (mounted.current && token === seq.current) {
        setPermission(DEFAULT_RECORDING_PERMISSION);
      }
    }
  }, []);

  const request = useCallback(async () => {
    const token = ++seq.current;
    try {
      const vm = await requestScreenRecordingPermission(isMicEnabledNow(), isWebcamEnabledNow());
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

  const requestMicrophone = useCallback(async () => {
    try {
      // The row's explicit request action (Story 20.2) — the same command the
      // Audio card's enable fires. The outcome itself is not adopted here; the
      // refresh below re-probes the full three-leg pre-flight live.
      await requestMicrophonePermission();
    } catch {
      // A failed round-trip makes no claim either way — the live re-probe
      // below resolves whatever is honest (or the safe default).
    }
    await refresh();
  }, [refresh]);

  const requestCamera = useCallback(async () => {
    try {
      await requestCameraPermission();
    } catch {
      // Same no-claim degradation as the microphone request.
    }
    await refresh();
  }, [refresh]);

  const openSettings = useCallback(() => {
    // Best-effort deep link; a rejection is swallowed (the user can still open
    // System Settings manually).
    void openScreenRecordingSettings().catch(() => {});
  }, []);

  const openMicrophoneSettings = useCallback(() => {
    void ipcOpenMicrophoneSettings().catch(() => {});
  }, []);

  const openCameraSettings = useCallback(() => {
    void ipcOpenCameraSettings().catch(() => {});
  }, []);

  useEffect(() => {
    mounted.current = true;
    // Live-detect at render (never cached), then re-detect on every return to
    // the window: `visibilitychange` → visible covers un-hiding, `focus` covers
    // the System Settings round-trip where the document never went hidden.
    // `refresh` is stable, so the listeners bind exactly once for the hook's
    // lifetime (no rebind churn on source toggles).
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

  // Re-probe when a source is toggled (Story 20.2): the enabled flag decides
  // which legs the resolver returns, so a toggle must make the row appear (or
  // vanish) and re-gate Start immediately — without waiting on a focus/return.
  // Skips the mount run (the effect above already did the initial probe) so a
  // toggle is the only thing that triggers an extra spawn here.
  const didProbeOnMount = useRef(false);
  useEffect(() => {
    if (!didProbeOnMount.current) {
      didProbeOnMount.current = true;
      return;
    }
    void refresh(micOn, webcamOn);
  }, [micOn, webcamOn, refresh]);

  return {
    permission,
    request,
    openSettings,
    requestMicrophone,
    openMicrophoneSettings,
    requestCamera,
    openCameraSettings,
    refresh,
  };
}
