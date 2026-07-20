/**
 * Global Start/Stop Recording hotkey listener (Story 20.4, FR-50).
 *
 * The optional recording hotkey is registered in Rust (`hotkey.recording`,
 * unset by default); a press emits `keeper://recording-hotkey-toggled` — it
 * never raises the window. This hook listens for that event and routes through
 * the shared {@link toggleRecording}: Rust is asked for the authoritative live
 * state, then the session stops, or starts with the CURRENT capture selections
 * (the same module-level stores the Start button reads). The webview runs even
 * while the window is hidden, so a backgrounded press still reaches this hook.
 *
 * Capability-gated: on a platform without the `recording` capability the hook
 * subscribes to nothing (the Rust registry also never offers the binding
 * there). Registering the listener is best-effort and graceful outside a Tauri
 * webview (jsdom in tests), mirroring {@link useGlobalHotkey}: a failure just
 * means the hotkey bridge is inert — it never crashes the shell.
 */
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { toggleRecording } from "@/lib/recording-control";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";

/** The event the Rust recording-hotkey handler emits on a press. */
export const RECORDING_HOTKEY_EVENT = "keeper://recording-hotkey-toggled";

export function useRecordingHotkey(): void {
  const recording = useCapabilitiesStore((s) => s.capabilities.recording);
  useEffect(() => {
    if (!recording) {
      // No recording capability (iOS / macOS < 13): never subscribe — the
      // surface is absent, not disabled (FR-66).
      return;
    }
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    try {
      void listen(RECORDING_HOTKEY_EVENT, () => {
        void toggleRecording();
      })
        .then((fn) => {
          if (cancelled) {
            fn();
          } else {
            unlisten = fn;
          }
        })
        .catch(() => {
          // No Tauri host — the hotkey bridge is inert in this environment.
        });
    } catch {
      // `listen` can throw synchronously when the Tauri IPC internals are absent.
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [recording]);
}
