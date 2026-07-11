/**
 * Boot capabilities-hydration hook (Story 12.2).
 *
 * On mount — once — it calls the Rust-authoritative `capabilities` command and
 * mirrors the served {@link CapabilitiesVm} into the capabilities store, the
 * per-platform handshake the UI consults instead of user agents or build flags.
 * Fire-and-forget: a rejected call is logged and leaves the store at its
 * declared safe default (every optional surface absent), and nothing here gates
 * rendering — hydration must never block boot.
 */
import { useEffect } from "react";
import { capabilities } from "@/lib/ipc/client";
import { capabilitiesStore } from "@/lib/stores/capabilities";

export function useCapabilitiesHydrate(): void {
  useEffect(() => {
    let cancelled = false;
    capabilities()
      .then((vm) => {
        if (!cancelled) {
          capabilitiesStore.getState().applySnapshot(vm);
        }
      })
      .catch((error: unknown) => {
        // Fail-safe: keep the declared safe default (all surfaces absent) and
        // surface the anomaly in the console — the app boots regardless.
        console.error("capabilities: hydration failed; keeping safe defaults", error);
      });
    return () => {
      cancelled = true;
    };
  }, []);
}
