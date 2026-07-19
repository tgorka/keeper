/**
 * The live recording-session hook (Story 16.6, FR-68/FR-69/FR-71, UX-DR30).
 *
 * Drives the walking-skeleton capture cycle from the Recording view: `start()`
 * fires the Rust `recording_start` command (which spawns the capture sidecar and
 * resolves the initial snapshot), then a 1 s poll of `recording_status` keeps
 * the {@link RecordingStatusVm} snapshot current while the session is live
 * (preflight / recording / rotating / stopping). The poll stops on a terminal
 * state (finalized / recovered / failed) — the terminal snapshot stays rendered
 * (the honest outcome, including a failure message) until the next start.
 *
 * The ticking elapsed line is client-computed from the host-reported
 * `startedAtEpochMs` on a 1 s interval — a slow poll never freezes the clock.
 *
 * Error-safe by design: a failed `start()` surfaces as a failed snapshot (never
 * a crash); a failed poll keeps the previous snapshot (transient IPC noise must
 * not flicker the UI back to idle mid-recording).
 */
import { useCallback, useEffect, useRef, useState } from "react";
import type { RecordingStatusVm, RecordingTargetVm } from "@/lib/ipc/client";
import { recordingStart, recordingStatus, recordingStop } from "@/lib/ipc/client";

/** The states with a session worth polling (anything non-terminal, non-idle). */
const LIVE_STATES: ReadonlyArray<RecordingStatusVm["state"]> = [
  "preflight",
  "recording",
  "rotating",
  "stopping",
];

/** The honest boot snapshot (no session yet). */
export const IDLE_RECORDING_STATUS: RecordingStatusVm = Object.freeze({
  state: "idle",
  segmentsClosed: 0,
  startedAtEpochMs: null,
  outputPath: null,
  error: null,
  // The sticky, non-fatal session warning (Story 19.4): none before a session.
  warning: null,
  // Read-time byte figures + session-captured cap (Story 18.3): zero with no
  // session; the enriched Rust snapshot fills them while one is live.
  onDiskBytes: 0,
  currentSegmentBytes: 0,
  segmentCapMb: 0,
});

/** Whether a snapshot represents a live (pollable, stoppable) session. */
export function isLiveRecording(status: RecordingStatusVm): boolean {
  return LIVE_STATES.includes(status.state);
}

/** Format elapsed milliseconds as `H:MM:SS` / `M:SS` (mono elapsed line, UX-DR30). */
export function formatElapsed(elapsedMs: number): string {
  const totalSeconds = Math.max(0, Math.floor(elapsedMs / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const two = (n: number) => String(n).padStart(2, "0");
  return hours > 0 ? `${hours}:${two(minutes)}:${two(seconds)}` : `${minutes}:${two(seconds)}`;
}

export interface UseRecordingSession {
  /** The latest session snapshot (the idle default until a session exists). */
  status: RecordingStatusVm;
  /** The ticking `H:MM:SS` elapsed line, or `null` before capture starts. */
  elapsed: string | null;
  /** Start the session for the selected capture target (Story 19.1) — a display
   * or an application; omit for the main-display default (no-op while live) —
   * the Audio card's system-audio toggle (Story 19.2); omit for the
   * default-on path — and the Audio card's mic selection (Story 19.3); omit
   * for the mic-off default (`micDeviceId` null = system default input). */
  start: (
    target?: RecordingTargetVm,
    systemAudio?: boolean,
    micEnabled?: boolean,
    micDeviceId?: string | null,
  ) => Promise<void>;
  /** Request the graceful stop-and-finalize (idempotent). */
  stop: () => Promise<void>;
}

export function useRecordingSession(): UseRecordingSession {
  const [status, setStatus] = useState<RecordingStatusVm>(IDLE_RECORDING_STATUS);
  const [elapsed, setElapsed] = useState<string | null>(null);
  const mounted = useRef(true);

  // On mount, adopt whatever session already exists (the view may have been
  // closed and reopened mid-recording — the session lives in Rust, not here).
  useEffect(() => {
    mounted.current = true;
    void recordingStatus()
      .then((vm) => {
        if (mounted.current) {
          setStatus(vm);
        }
      })
      .catch(() => {
        // No runtime / early boot: keep the idle default.
      });
    return () => {
      mounted.current = false;
    };
  }, []);

  // Poll while live: 1 s cadence, stopped on any terminal state. A failed poll
  // keeps the previous snapshot (never flickers to idle mid-recording).
  const live = isLiveRecording(status);
  useEffect(() => {
    if (!live) {
      return;
    }
    const interval = setInterval(() => {
      void recordingStatus()
        .then((vm) => {
          if (mounted.current) {
            setStatus(vm);
          }
        })
        .catch(() => {});
    }, 1000);
    return () => {
      clearInterval(interval);
    };
  }, [live]);

  // The ticking elapsed line, client-computed from the host start instant.
  const startedAt = status.startedAtEpochMs;
  useEffect(() => {
    if (startedAt === null || !live) {
      setElapsed(startedAt === null ? null : formatElapsed(Date.now() - Number(startedAt)));
      return;
    }
    const tick = () => {
      setElapsed(formatElapsed(Date.now() - Number(startedAt)));
    };
    tick();
    const interval = setInterval(tick, 1000);
    return () => {
      clearInterval(interval);
    };
  }, [startedAt, live]);

  const start = useCallback(
    async (
      target?: RecordingTargetVm,
      systemAudio?: boolean,
      micEnabled?: boolean,
      micDeviceId?: string | null,
    ) => {
      try {
        const vm = await recordingStart(target, systemAudio, micEnabled, micDeviceId);
        if (mounted.current) {
          setStatus(vm);
        }
      } catch (raw) {
        // An honest failed snapshot — never a crash, never a silent no-op.
        if (mounted.current) {
          const message =
            typeof raw === "object" && raw !== null && "message" in raw
              ? String((raw as { message: unknown }).message)
              : "could not start the recording";
          setStatus({
            ...IDLE_RECORDING_STATUS,
            state: "failed",
            error: message,
          });
        }
      }
    },
    [],
  );

  const stop = useCallback(async () => {
    // Best-effort: the outcome arrives through the poll (stopping → finalized).
    await recordingStop().catch(() => {});
  }, []);

  return { status, elapsed, start, stop };
}
