/**
 * Streaming content search over the active vault (Story 37.5, FR-118).
 *
 * `notes_search` is a bounded parallel scan, not an index query, so results
 * arrive as they are found rather than all at once — the hook renders each
 * batch as it lands and never waits for `done`. There is no index to be stale
 * against: a file written a millisecond ago is matched by the next query.
 *
 * Supersession is the subtle part. Rust cancels the previous scan when a new
 * one starts for the same vault, but a batch already in flight can still be
 * delivered after that. Every scan therefore carries a generation number and
 * batches from an older generation are dropped on arrival, so a six-character
 * query can never paint the results of its own three-character prefix.
 */
import { useEffect, useState } from "react";
import { type IpcError, type NoteSearchHitVm, notesSearch } from "@/lib/ipc/client";

/** How long the query rests before a scan is started. */
export const NOTE_SEARCH_DEBOUNCE_MS = 150;

/** The default cap on hits requested from one scan. */
export const NOTE_SEARCH_LIMIT = 200;

/** Structural guard for the {@link IpcError} envelope thrown by the IPC client. */
function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.code === "string" && typeof candidate.message === "string";
}

export interface UseNotesSearch {
  /** Every hit delivered by the current scan, in arrival order. */
  hits: NoteSearchHitVm[];
  /** Whether a scan is still running. */
  running: boolean;
  /** The scan's failure, or null. */
  error: string | null;
}

export function useNotesSearch(
  vaultId: string | null,
  query: string,
  limit: number = NOTE_SEARCH_LIMIT,
): UseNotesSearch {
  const [hits, setHits] = useState<NoteSearchHitVm[]>([]);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const needle = query.trim();
    if (vaultId === null || needle === "") {
      setHits([]);
      setRunning(false);
      setError(null);
      return;
    }
    let live = true;
    const timer = setTimeout(() => {
      setHits([]);
      setRunning(true);
      setError(null);
      void notesSearch(vaultId, { text: needle, limit }, (batch) => {
        if (!live) {
          return;
        }
        setHits((previous) => [...previous, ...batch.hits]);
        if (batch.done) {
          setRunning(false);
        }
      }).catch((failure: unknown) => {
        if (live) {
          setRunning(false);
          setError(isIpcError(failure) ? failure.message : String(failure));
        }
      });
    }, NOTE_SEARCH_DEBOUNCE_MS);

    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [vaultId, query, limit]);

  return { hits, running, error };
}
