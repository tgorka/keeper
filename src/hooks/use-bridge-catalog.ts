/**
 * Bridge-catalog fetch hook (Story 6.1, FR-42).
 *
 * Fetches the data-driven bridge catalog once over IPC and exposes it as render
 * state. The catalog is static, account-agnostic backend data (risk tiers →
 * networks + badge + ack copy), so a single one-shot {@link bridgeCatalog} read is
 * enough — there is no stream. The effect is gated so a resolve/reject after unmount
 * never sets state, and a rejection surfaces as an honest error string for the
 * Bridges view's error state (the embedded data failing to parse is the only
 * failure mode).
 */
import { useEffect, useState } from "react";
import type { BridgeNetworkVm, IpcError } from "@/lib/ipc/client";
import { bridgeCatalog } from "@/lib/ipc/client";

/** The render state of the bridge-catalog fetch. */
export interface BridgeCatalogState {
  /** The surfaced bridge networks, or `null` until the first fetch resolves. */
  catalog: BridgeNetworkVm[] | null;
  /** `true` while the initial fetch is in flight. */
  loading: boolean;
  /** A non-secret error message when the catalog could not be loaded, else `null`. */
  error: string | null;
}

/**
 * Fetch the bridge catalog once and return its render state. Mirrors the app's
 * one-shot IPC fetch pattern with an unmount guard.
 */
export function useBridgeCatalog(): BridgeCatalogState {
  const [state, setState] = useState<BridgeCatalogState>({
    catalog: null,
    loading: true,
    error: null,
  });

  useEffect(() => {
    let cancelled = false;
    bridgeCatalog()
      .then((catalog) => {
        if (!cancelled) {
          setState({ catalog, loading: false, error: null });
        }
      })
      .catch((raw: IpcError) => {
        if (!cancelled) {
          setState({ catalog: null, loading: false, error: raw.message });
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return state;
}
