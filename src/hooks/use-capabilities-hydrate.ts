/**
 * Boot capabilities-hydration hook (Story 12.2).
 *
 * On mount it calls the Rust-authoritative `capabilities` command and mirrors
 * the served {@link CapabilitiesVm} into the capabilities store, the
 * per-platform handshake the UI consults instead of user agents or build flags.
 * Fire-and-forget: a rejected call is logged and leaves the store at its
 * declared safe default (every optional surface absent), and nothing here gates
 * rendering — hydration must never block boot.
 *
 * It asks again whenever Rust says a capability changed
 * (`keeper://capabilities-changed`): on the phone `sessions` follows whether a
 * sessions zone is there (98.2), which a sync pass or a saved folder changes
 * while keeper runs. It asks once more as soon as that subscription is in
 * place: Rust says so only on the change itself, so a flip that landed between
 * the first answer and the subscription would otherwise be missed for good.
 */
import { useEffect } from "react";
import { logFailure } from "@/lib/failure";
import { capabilities, listenCapabilitiesChanged } from "@/lib/ipc/client";
import { capabilitiesStore } from "@/lib/stores/capabilities";

export function useCapabilitiesHydrate(): void {
  useEffect(() => {
    let cancelled = false;
    const hydrate = () => {
      capabilities()
        .then((vm) => {
          if (!cancelled) {
            capabilitiesStore.getState().applySnapshot(vm);
          }
        })
        .catch((error: unknown) => {
          logFailure("capabilities.hydrate")(error);
          // Fail-safe: keep the declared safe default (all surfaces absent) and
          // surface the anomaly in the console — the app boots regardless.
          console.error("capabilities: hydration failed; keeping safe defaults", error);
        });
    };
    hydrate();
    let unlisten: (() => void) | undefined;
    listenCapabilitiesChanged(hydrate)
      .then((stop) => {
        if (cancelled) {
          stop();
        } else {
          unlisten = stop;
          hydrate();
        }
      })
      .catch((error: unknown) => {
        logFailure("capabilities.listen")(error);
        console.error("capabilities: could not listen for changes", error);
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
