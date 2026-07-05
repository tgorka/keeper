/**
 * Per-Account bridge-discovery fetch hook (Story 6.2, FR-25, AD-16).
 *
 * Runs the zero-config, three-source discovery pass once for one account over IPC
 * ({@link bridgeDiscover}) and exposes it as render state. Discovery is a
 * point-in-time probe, so a single one-shot read is enough — there is no stream
 * (live health is Story 6.5). The effect is gated so a resolve/reject after unmount
 * (or after the `accountId` changes) never sets state, and a rejection surfaces as
 * an honest error string with its retriability so the pane can offer a retry.
 */
import { useEffect, useState } from "react";
import type { BridgeDiscoveryVm, IpcError } from "@/lib/ipc/client";
import { bridgeDiscover } from "@/lib/ipc/client";

/** The render state of a per-account bridge-discovery fetch. */
export interface BridgeDiscoveryState {
  /** The discovered networks + homeserver, or `null` until the first fetch resolves. */
  discovery: BridgeDiscoveryVm | null;
  /** `true` while a fetch is in flight. */
  loading: boolean;
  /** A non-secret error message when discovery failed, else `null`. */
  error: string | null;
  /** Whether the last error is retriable (drives whether the pane offers a retry). */
  retriable: boolean;
  /** Re-run discovery for this account (e.g. from the error state's Retry). */
  retry: () => void;
}

interface Internal {
  discovery: BridgeDiscoveryVm | null;
  loading: boolean;
  error: string | null;
  retriable: boolean;
  /** Bumped by {@link BridgeDiscoveryState.retry} to force a re-fetch. */
  attempt: number;
}

/**
 * Run bridge discovery for `accountId` and return its render state. Re-runs when
 * `accountId` changes (each account discovers independently) or on an explicit
 * retry, with an unmount / stale-account guard.
 */
export function useBridgeDiscovery(accountId: string): BridgeDiscoveryState {
  const [state, setState] = useState<Internal>({
    discovery: null,
    loading: true,
    error: null,
    retriable: false,
    attempt: 0,
  });

  const retry = () => {
    setState((prev) => ({
      discovery: null,
      loading: true,
      error: null,
      retriable: false,
      attempt: prev.attempt + 1,
    }));
  };

  // `attempt` is a genuine effect input: `retry` increments it to force a fresh
  // discovery run for the same account. Biome can't see it used in the body (it is
  // purely a re-run trigger), so the dependency is declared explicitly.
  const { attempt } = state;
  // biome-ignore lint/correctness/useExhaustiveDependencies: attempt is a deliberate re-run trigger
  useEffect(() => {
    let cancelled = false;
    bridgeDiscover(accountId)
      .then((discovery) => {
        if (!cancelled) {
          setState((prev) => ({
            ...prev,
            discovery,
            loading: false,
            error: null,
            retriable: false,
          }));
        }
      })
      .catch((raw: IpcError) => {
        if (!cancelled) {
          setState((prev) => ({
            ...prev,
            discovery: null,
            loading: false,
            error: raw.message,
            retriable: raw.retriable,
          }));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [accountId, attempt]);

  return {
    discovery: state.discovery,
    loading: state.loading,
    error: state.error,
    retriable: state.retriable,
    retry,
  };
}
