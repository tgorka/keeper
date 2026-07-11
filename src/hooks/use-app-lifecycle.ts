/**
 * Capability-gated app-lifecycle driver (Epic 14-1) — the zero-native stopgap
 * that turns the webview `visibilitychange` event into the single Rust
 * `app_lifecycle_changed` command.
 *
 * Only on the reduced-capability (iOS/phone) tier — read from the capabilities
 * mirror via {@link useIsReducedCapabilityPlatform}, never from user-agent or
 * build flags — does this attach a `document` `visibilitychange` listener:
 *
 * - `document.visibilityState === "hidden"` → `appLifecycleChanged("background")`
 *   → the core gracefully pauses every live account's `SyncService` (the
 *   long-poll ends cleanly; account state retained).
 * - otherwise (`"visible"`) → `appLifecycleChanged("foreground")` → the core
 *   routes through `AccountManager::sync_now()` (the same kick pull-to-refresh
 *   uses), resuming each live sync loop while cached mirrors are already
 *   rendered.
 *
 * On desktop the predicate is false and NO listener is attached, so Story 10.3
 * background operation is untouched — sync stays alive while hidden. Before
 * capabilities hydrate the predicate is also false, so nothing attaches until
 * the iOS tier resolves; the effect re-runs (attaching or removing the
 * listener) whenever the predicate flips.
 *
 * IPC errors are swallowed (best-effort, no toast) — a failed pause/resume must
 * never surface UI. A later Swift `UIApplication` plugin will call the same
 * command; this hook is the interim driver only.
 *
 * This same dispatch also feeds the frontend {@link lifecycleStore} (Story 14.5)
 * so the media shed derives from this one listener — one visibility signal, one
 * lifecycle truth. The store write is only reached on the reduced tier (the hook
 * attaches nothing on desktop), so desktop keeps the store at its `"foreground"`
 * default and never sheds.
 */
import { useEffect } from "react";
import { appLifecycleChanged } from "@/lib/ipc/client";
import { useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
import { lifecycleStore } from "@/lib/stores/lifecycle";

export function useAppLifecycle(): void {
  const isReducedCapability = useIsReducedCapabilityPlatform();

  useEffect(() => {
    // Desktop tier (and pre-hydration): attach nothing so sync stays alive while
    // hidden (Story 10.3 unregressed).
    if (!isReducedCapability) {
      return;
    }

    // Map the webview visibility to a lifecycle phase. Only the two states the
    // Page Visibility API reports on the phone tier drive a transition; any other
    // value is ignored rather than collapsed to "foreground", so an off-screen
    // state never spuriously kicks the network.
    const phaseFor = (visibility: DocumentVisibilityState): "background" | "foreground" | null => {
      if (visibility === "hidden") {
        return "background";
      }
      if (visibility === "visible") {
        return "foreground";
      }
      return null;
    };

    const dispatch = (): void => {
      const phase = phaseFor(document.visibilityState);
      if (phase === null) {
        return;
      }
      // Feed the frontend lifecycle store alongside the IPC call so the media
      // shed derives from this one listener (Story 14.5). Only reached on the
      // reduced tier, so desktop keeps the store at its "foreground" default.
      lifecycleStore.getState().setPhase(phase);
      // Best-effort: swallow IPC errors (no toast).
      void appLifecycleChanged(phase).catch(() => {});
    };

    // Emit the CURRENT visibility once at attach so the "hidden ⇒ paused"
    // guarantee holds even when the hook mounts (or capabilities hydrate) while
    // the document is already hidden — a transition-only listener would miss that
    // ordering and leave sync running in the background.
    dispatch();
    document.addEventListener("visibilitychange", dispatch);
    return () => {
      document.removeEventListener("visibilitychange", dispatch);
    };
  }, [isReducedCapability]);
}
