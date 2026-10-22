import { useEffect } from "react";
import { logFailure } from "@/lib/failure";
import { listenHqPairOffer } from "@/lib/ipc/client";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { settingsUiStore } from "@/lib/stores/settings-ui";

/**
 * A scanned pairing QR opened keeper (`keeper://agents/pair`): Rust checked and kept the
 * link, and Settings › Agents server asks the person to confirm it. This only opens that
 * section; the offer itself arrives in the section's status stream.
 */
export function useHqPairOffer(): void {
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listenHqPairOffer(() => {
      settingsUiStore.getState().revealSection("agents-server");
      primaryViewStore.getState().setView("settings");
    })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(logFailure("hq.listenPairOffer"));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
