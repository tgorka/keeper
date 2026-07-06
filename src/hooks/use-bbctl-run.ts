/**
 * `bbctl` self-hosted-bridge run session lifecycle (Story 6.7, FR-29, AD-16).
 *
 * Owns one run session for a `(accountId, networkId)`: {@link start} opens the
 * streaming subscription (Rust gates Beeper-only, then drives `bbctl register`/`run`
 * as a launch-on-demand sidecar), mirroring each {@link BbctlProgressVm} snapshot into
 * render state; {@link cancel} tears the session down. A late snapshot after cleanup
 * never mutates state (a per-run `cancelled` ref, mirroring {@link import("./use-bridge-login").useBridgeLogin}),
 * and unmount / close always cancels the backend session so no streaming task leaks. A
 * `start` failure surfaces as a synthetic `failure` VM so the Sheet renders the honest
 * error + Retry rather than a stuck spinner.
 *
 * The launched `bbctl run` daemon is launch-and-leave (v1.x — no supervision), so
 * `cancel` only tears down keeper's streaming task, not the detached bridge process.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import type { BbctlProgressVm, IpcError } from "@/lib/ipc/client";
import { bbctlRunCancel, bbctlRunStart } from "@/lib/ipc/client";

/** The render state + actions of a `bbctl` run session. */
export interface BbctlRunState {
  /** The latest streamed run snapshot, or `null` before the first arrives. */
  vm: BbctlProgressVm | null;
  /** Start (or restart) the run for `networkId`. Idempotent-safe to re-call. */
  start: () => void;
  /** Cancel and tear down the running session. */
  cancel: () => void;
}

/**
 * Drive a `bbctl` self-hosted-bridge run for `(accountId, networkId)`. `active` gates
 * the session so the Sheet only opens a run when it is shown; closing it (`active`
 * false) or unmounting cancels the backend session and clears the snapshot.
 */
export function useBbctlRun(accountId: string, networkId: string, active: boolean): BbctlRunState {
  const [vm, setVm] = useState<BbctlProgressVm | null>(null);
  // The resolved backend session id (for cancel), null until start resolves. A ref
  // so cancel reads the live value without re-rendering.
  const sessionIdRef = useRef<number | null>(null);
  // Set by `cancel()` so a late-resolving `start().then` cannot re-register a session
  // the user already cancelled. Reset by each fresh `start`.
  const cancelledRef = useRef(false);
  // Bumped by `start` to force a fresh session for the same target.
  const [attempt, setAttempt] = useState(0);

  const start = useCallback(() => {
    cancelledRef.current = false;
    setVm(null);
    setAttempt((n) => n + 1);
  }, []);

  const cancel = useCallback(() => {
    cancelledRef.current = true;
    const id = sessionIdRef.current;
    sessionIdRef.current = null;
    if (id !== null) {
      void bbctlRunCancel(id).catch(() => {
        // Cancel is best-effort.
      });
    }
    setVm(null);
  }, []);

  useEffect(() => {
    if (!active || attempt === 0) {
      return;
    }
    // A PER-RUN flag captured by this run's stream callback, `.then`, `.catch`, and
    // cleanup (mirroring `use-bridge-login`). A shared ref would let a rapid Retry's
    // cleanup un-guard a prior in-flight run, whose late-resolving start could then
    // write `sessionIdRef` to an already-cleaned-up id and orphan that session.
    let cancelled = false;
    let resolvedId: number | null = null;

    bbctlRunStart(accountId, networkId, (snapshot) => {
      if (!cancelled && !cancelledRef.current) {
        setVm(snapshot);
      }
    })
      .then((id) => {
        if (cancelled || cancelledRef.current) {
          // Torn down (or user-cancelled) before the id resolved — cancel it now.
          void bbctlRunCancel(id).catch(() => {});
          return;
        }
        resolvedId = id;
        sessionIdRef.current = id;
      })
      .catch((raw: IpcError) => {
        if (!cancelled && !cancelledRef.current) {
          // Render an honest failure VM so the Sheet offers Retry.
          setVm({
            networkId,
            phase: "failure",
            message: null,
            error: raw.message,
          });
        }
      });

    return () => {
      cancelled = true;
      const id = resolvedId ?? sessionIdRef.current;
      sessionIdRef.current = null;
      if (id !== null) {
        void bbctlRunCancel(id).catch(() => {});
      }
    };
  }, [accountId, networkId, active, attempt]);

  return { vm, start, cancel };
}
