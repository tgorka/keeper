/**
 * Streaming content search over one sessions zone (FR-267).
 *
 * The twin of {@link import("./use-notes-search").useNotesSearch}, and a twin
 * rather than a widening of it because a zone can never be a vault: a subfolder
 * flagged as both is refused at profile validation, so `notes_search` cannot
 * reach a session file whatever id it is handed. Two commands, one matcher —
 * which is why the two hooks look alike and are still two.
 *
 * Supersession is handled on both sides. Rust aborts the previous scan of the
 * same root when a new one starts, so a fast typist funds one walk rather than
 * one per keystroke; a batch already in flight can still land, so every scan
 * carries a generation and older batches are dropped on arrival. Unmounting
 * cancels: a search surface that closes should not leave a zone walk running
 * for an answer nobody will read.
 */
import { useEffect, useState } from "react";
import {
  type IpcError,
  type SessionSearchHitVm,
  sessionsSearch,
  sessionsSearchCancel,
} from "@/lib/ipc/client";

/** How long the query rests before a scan is started. */
export const SESSION_SEARCH_DEBOUNCE_MS = 150;

/** The default cap on hits requested from one scan. */
export const SESSION_SEARCH_LIMIT = 200;

/** Structural guard for the {@link IpcError} envelope thrown by the IPC client. */
function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.code === "string" && typeof candidate.message === "string";
}

export interface UseSessionsSearch {
  /** Every hit delivered by the current scan, in arrival order. */
  hits: SessionSearchHitVm[];
  /** Whether a scan is still running. */
  running: boolean;
  /** The scan's failure, or null. */
  error: string | null;
}

export function useSessionsSearch(
  rootId: string | null,
  query: string,
  limit: number = SESSION_SEARCH_LIMIT,
): UseSessionsSearch {
  const [hits, setHits] = useState<SessionSearchHitVm[]>([]);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const needle = query.trim();
    if (rootId === null || needle === "") {
      setHits([]);
      setRunning(false);
      setError(null);
      return;
    }
    let live = true;
    // The scan's own id, once Rust has one. Held so the cleanup can cancel a
    // walk that is still going — the abort-on-supersede in Rust covers the
    // next query, but nothing covers a surface that simply closed.
    let scanId: string | null = null;
    const timer = setTimeout(() => {
      setHits([]);
      setRunning(true);
      setError(null);
      void sessionsSearch(rootId, { text: needle, limit }, (batch) => {
        if (!live) {
          return;
        }
        setHits((previous) => [...previous, ...batch.hits]);
        if (batch.done) {
          setRunning(false);
        }
      })
        .then((id) => {
          if (live) {
            scanId = id;
          } else {
            // Resolved after the cleanup ran: cancel what we were just handed,
            // since the cleanup had no id to cancel with.
            void sessionsSearchCancel(id).catch(() => {});
          }
        })
        .catch((failure: unknown) => {
          if (live) {
            setRunning(false);
            setError(isIpcError(failure) ? failure.message : String(failure));
          }
        });
    }, SESSION_SEARCH_DEBOUNCE_MS);

    return () => {
      live = false;
      clearTimeout(timer);
      if (scanId !== null) {
        // Cancelling an id that already finished is a no-op in Rust, so a race
        // with the last batch is not an error to swallow loudly.
        void sessionsSearchCancel(scanId).catch(() => {});
      }
    };
  }, [rootId, query, limit]);

  return { hits, running, error };
}
