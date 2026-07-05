/**
 * Native bridge-login session lifecycle (Story 6.3, FR-26, AD-16).
 *
 * Owns one login session for a `(accountId, networkId)`: {@link start} opens the
 * streaming subscription (Rust connects the provisioning transport and drives the
 * bridgev2 state machine), mirroring each {@link BridgeLoginVm} snapshot into render
 * state; {@link submit} pushes a flow choice or entered field values into the
 * running driver; {@link cancel} tears the session down. A late snapshot after
 * cleanup never mutates state (a `cancelled` guard, mirroring
 * `use-encryption-statuses`), and unmount / close always cancels the backend session
 * so no driver task leaks. A `start` failure surfaces as a synthetic `failure` VM so
 * the Sheet renders the honest error + Retry rather than a stuck spinner.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import type { BridgeLoginInput, BridgeLoginVm, IpcError } from "@/lib/ipc/client";
import { cancelBridgeLogin, startBridgeLogin, submitBridgeLogin } from "@/lib/ipc/client";

/** The render state + actions of a native bridge-login session. */
export interface BridgeLoginState {
  /** The latest streamed login snapshot, or `null` before the first arrives. */
  vm: BridgeLoginVm | null;
  /** Start (or restart) the login for `networkId`. Idempotent-safe to re-call. */
  start: () => void;
  /** Submit a flow choice or entered field values into the running login. */
  submit: (input: BridgeLoginInput) => void;
  /** Cancel and tear down the running login session. */
  cancel: () => void;
}

/**
 * Drive a native bridge login for `(accountId, networkId)`. `active` gates the
 * session so the Sheet only opens a login when it is shown; closing it (`active`
 * false) or unmounting cancels the backend session and clears the snapshot.
 */
export function useBridgeLogin(
  accountId: string,
  networkId: string,
  active: boolean,
): BridgeLoginState {
  const [vm, setVm] = useState<BridgeLoginVm | null>(null);
  // The resolved backend session id (for submit / cancel), null until start
  // resolves. A ref so submit/cancel read the live value without re-rendering.
  const sessionIdRef = useRef<number | null>(null);
  // Bumped by `start` to force a fresh session for the same target.
  const [attempt, setAttempt] = useState(0);

  const start = useCallback(() => {
    setVm(null);
    setAttempt((n) => n + 1);
  }, []);

  const submit = useCallback(
    (input: BridgeLoginInput) => {
      const id = sessionIdRef.current;
      if (id === null) {
        return;
      }
      void submitBridgeLogin(accountId, id, input).catch(() => {
        // A submit into an already-ended session is non-fatal — the stream will
        // have surfaced the terminal state already.
      });
    },
    [accountId],
  );

  const cancel = useCallback(() => {
    const id = sessionIdRef.current;
    sessionIdRef.current = null;
    if (id !== null) {
      void cancelBridgeLogin(accountId, id).catch(() => {
        // Cancel is best-effort.
      });
    }
    setVm(null);
  }, [accountId]);

  useEffect(() => {
    if (!active || attempt === 0) {
      return;
    }
    // A PER-RUN flag captured by this run's stream callback, `.then`, `.catch`,
    // and cleanup (mirroring `use-encryption-statuses`). A shared ref would let a
    // rapid Retry's cleanup un-guard a prior in-flight run, whose late-resolving
    // start could then write `sessionIdRef` to an already-cleaned-up id and orphan
    // that backend session.
    let cancelled = false;
    let resolvedId: number | null = null;

    startBridgeLogin(accountId, networkId, (snapshot) => {
      if (!cancelled) {
        setVm(snapshot);
      }
    })
      .then((id) => {
        if (cancelled) {
          // Torn down before the id resolved — cancel immediately.
          void cancelBridgeLogin(accountId, id).catch(() => {});
          return;
        }
        resolvedId = id;
        sessionIdRef.current = id;
      })
      .catch((raw: IpcError) => {
        if (!cancelled) {
          // Render an honest failure VM so the Sheet offers Retry.
          setVm({
            networkId,
            phase: "failure",
            instruction: null,
            qrSvg: null,
            qrRefreshed: false,
            fields: [],
            flows: [],
            error: raw.message,
          });
        }
      });

    return () => {
      cancelled = true;
      const id = resolvedId ?? sessionIdRef.current;
      sessionIdRef.current = null;
      if (id !== null) {
        void cancelBridgeLogin(accountId, id).catch(() => {});
      }
    };
  }, [accountId, networkId, active, attempt]);

  return { vm, start, submit, cancel };
}
