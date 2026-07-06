/**
 * Coupling-caveats fetch hook (Story 8.2, FR-44).
 *
 * Fetches the data-driven per-Network coupling caveats once over IPC and exposes,
 * for a given `networkId`, the caveats that apply to it. The catalog is static,
 * account-agnostic backend data (network → coupled-behavior caveat text), so a single
 * one-shot {@link couplingCaveats} read is enough — there is no stream. The effect is
 * gated so a resolve/reject after unmount never sets state, and a rejection leaves the
 * catalog empty (the embedded data failing to parse is the only failure mode) — an
 * absent caveat simply shows no inline hint, never a UI error. No caveat copy is
 * authored here: the resolved `text` from Rust is rendered verbatim.
 */
import { useEffect, useMemo, useState } from "react";
import type { CouplingCaveatVm, IpcError } from "@/lib/ipc/client";
import { couplingCaveats } from "@/lib/ipc/client";

/**
 * Fetch the coupling caveats once and return the subset that applies to `networkId`.
 * Returns `[]` for a `null` or unknown network (a native Matrix room, or a network
 * with no coupling caveat) — the caller renders nothing in that case. Mirrors the
 * app's one-shot IPC fetch pattern with an unmount guard.
 */
export function useCouplingCaveats(networkId: string | null): CouplingCaveatVm[] {
  const [catalog, setCatalog] = useState<CouplingCaveatVm[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    couplingCaveats()
      .then((caveats) => {
        if (!cancelled) {
          setCatalog(caveats);
        }
      })
      .catch((_raw: IpcError) => {
        if (!cancelled) {
          // Best-effort: an embedded-data parse failure leaves the catalog empty, so no
          // caveat surfaces (never a UI error for a disclosure hint).
          setCatalog([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return useMemo(() => {
    if (networkId === null || catalog === null) {
      return [];
    }
    return catalog.filter((caveat) => caveat.networkId === networkId);
  }, [catalog, networkId]);
}
