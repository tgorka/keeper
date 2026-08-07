/**
 * The crash-recovery notice hook (Story 20.3, FR-73, UX-DR34).
 *
 * On mount (app start / the Recording surface) and after a session finalizes,
 * fetches the crash-recovered sessions still needing a one-time notice via
 * `recoveredSessionsList()` — but only while the `recording` capability is on
 * (never a dead fetch on a platform without recording). Exposes the list and an
 * `acknowledge(folder)` that latches the one-time notice in the Rust registry
 * seen-set AND drops the session from local state so the card disappears
 * immediately (it never re-surfaces on a later scan/restart either).
 *
 * Best-effort by design: a failed fetch keeps the previous list (transient IPC
 * noise never flashes a card away or invents one); a failed acknowledge still
 * drops the card locally (best-effort latch — it may reappear next scan, which
 * is the honest fallback the Rust side documents).
 *
 * Story 40.4: a listed session can be RENAMED from its card, which moves the
 * folder every entry is keyed on. `retitled` re-points the entry immediately so
 * a dismissal lands on the path whose manifest still loads, and a dismissed
 * folder is remembered for the mount so an in-flight re-scan cannot re-list the
 * card the user just dismissed.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import type { RecordingSummaryVm } from "@/lib/ipc/client";
import { recoveredSessionAcknowledge, recoveredSessionsList } from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";

export interface UseRecoveredSessions {
  /** The crash-recovered sessions still due a one-time notice (empty when none
   * or when recording is unavailable). */
  sessions: RecordingSummaryVm[];
  /** Re-scan disk for unacknowledged recovered sessions (call after a session
   * finalizes so a fresh salvage surfaces without a remount). */
  refresh: () => void;
  /** Acknowledge (dismiss) a recovery card: latch the one-time notice and drop
   * the session from local state immediately. Pass the folder the session is at
   * NOW — after a rename that is the post-move path, and it is the only one
   * whose manifest still loads (Story 40.4). */
  acknowledge: (folder: string) => void;
  /** Adopt the summary a listed session's rename resolved (Story 40.4): the
   * entry is replaced in place, so the list describes the session by where it
   * is now — before the disk re-scan lands, and even if that scan fails. */
  retitled: (folder: string, summary: RecordingSummaryVm) => void;
}

export function useRecoveredSessions(): UseRecoveredSessions {
  const [sessions, setSessions] = useState<RecordingSummaryVm[]>([]);
  const recording = useCapabilitiesStore((s) => s.capabilities.recording);
  const mounted = useRef(true);
  // Folders already dismissed in this mount. A dismissal is one-time, but the
  // latch is a round trip and a re-scan can be in flight when it happens (a
  // rename fires one) — without this the resolving scan would re-list the very
  // card the user just dismissed.
  const dismissed = useRef<Set<string>>(new Set());

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const refresh = useCallback(() => {
    // Never a dead fetch on a platform without recording (the list would be an
    // honest empty array, but the round-trip is pointless).
    if (!recording) {
      return;
    }
    void recoveredSessionsList()
      .then((list) => {
        if (mounted.current) {
          setSessions(list.filter((session) => !dismissed.current.has(session.sessionFolder)));
        }
      })
      .catch(() => {
        // Transient IPC noise: keep the previous list, never flash to empty.
      });
  }, [recording]);

  // Scan on mount and whenever the recording capability settles.
  useEffect(() => {
    refresh();
  }, [refresh]);

  const acknowledge = useCallback((folder: string) => {
    // Drop the card locally first (best-effort UX): the notice is one-time, so a
    // failed latch at worst re-surfaces it on a later scan — never a stuck card.
    dismissed.current.add(folder);
    setSessions((prev) => prev.filter((session) => session.sessionFolder !== folder));
    void recoveredSessionAcknowledge(folder).catch(() => {
      // Best-effort latch (the Rust write is one-way, idempotent).
    });
  }, []);

  const retitled = useCallback((folder: string, summary: RecordingSummaryVm) => {
    setSessions((prev) =>
      prev.map((session) => (session.sessionFolder === folder ? summary : session)),
    );
  }, []);

  return { sessions, refresh, acknowledge, retitled };
}
