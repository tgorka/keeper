/**
 * The terminal-session summary hook (Story 20.3, FR-71).
 *
 * When the live session reaches a card-worthy terminal (`finalized` or the
 * in-app `recovered`), fetches the authoritative on-disk summary for its folder
 * via `recordingSessionSummary(folder)` — the "Saved N segments · {size}"
 * figures the completion / in-app-recovery card renders, never the live
 * `segmentsClosed` rotation counter.
 *
 * Best-effort: a manifest load failure resolves `null` (the card falls back to
 * folder + Reveal, omitting count/size — the honest degraded shape). The summary
 * clears whenever the folder/terminal condition no longer holds, so a stale
 * card's figures never bleed into the next session.
 */
import { useEffect, useRef, useState } from "react";
import type { RecordingSummaryVm } from "@/lib/ipc/client";
import { recordingSessionSummary } from "@/lib/ipc/client";

/**
 * Fetch the summary for `folder` when `enabled` (a card-worthy terminal with a
 * folder). Returns the summary, or `null` while loading / when disabled / on a
 * load failure.
 */
export function useRecordedSessionSummary(
  folder: string | null,
  enabled: boolean,
): RecordingSummaryVm | null {
  const [summary, setSummary] = useState<RecordingSummaryVm | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    if (!enabled || folder === null) {
      setSummary(null);
      return;
    }
    // Clear first so a previous session's figures never bleed onto the new
    // card while this fetch is in flight; `stale` drops a late-resolving fetch
    // for a superseded folder (an out-of-order resolution would otherwise show
    // the wrong session's count/size).
    setSummary(null);
    let stale = false;
    void recordingSessionSummary(folder)
      .then((vm) => {
        if (!stale && mounted.current) {
          setSummary(vm);
        }
      })
      .catch(() => {
        // Best-effort: no summary → the card falls back to folder + Reveal.
        if (!stale && mounted.current) {
          setSummary(null);
        }
      });
    return () => {
      stale = true;
    };
  }, [folder, enabled]);

  return summary;
}
