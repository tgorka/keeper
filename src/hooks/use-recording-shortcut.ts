/**
 * Recording keyboard shortcut (Story 16.3).
 *
 * Wires `⌘5` (Ctrl+5 for non-mac parity) to switch the shell to the Recording
 * primary view, following the app's ad-hoc `window.addEventListener("keydown", …)`
 * shortcut pattern (mirrors {@link useBridgesShortcut}; there is no central
 * registry). `preventDefault` keeps the webview from acting on the chord.
 *
 * Capability-gated (AD-27): the chord is a no-op unless the Rust-served
 * `recording` capability is on (desktop macOS ≥ 13.0), so there is no dead
 * ⌘5 affordance on platforms that cannot record. The capability is read from the
 * `capabilitiesStore` mirror — never from the browser environment or build flags.
 */
import { useEffect } from "react";
import { capabilitiesStore } from "@/lib/stores/capabilities";
import { primaryViewStore } from "@/lib/stores/primary-view";

export function useRecordingShortcut(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (!mod || event.key !== "5") {
        return;
      }
      // No dead ⌘5 where recording is unavailable — read the capability mirror.
      if (!capabilitiesStore.getState().capabilities.recording) {
        return;
      }
      // Don't hijack the chord while the user is typing in a field — swallowing it
      // would yank them to Recording mid-edit.
      const target = event.target as HTMLElement | null;
      if (
        target !== null &&
        (target.isContentEditable ||
          target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "SELECT")
      ) {
        return;
      }
      event.preventDefault();
      primaryViewStore.getState().setView("recording");
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
