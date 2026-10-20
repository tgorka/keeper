/**
 * The default agent follows a settings sync (story 102.7, DW-1572).
 *
 * When another device changes the default agent and the setting syncs here,
 * Rust says so (`AGENTS_DEFAULT_CHANGED_EVENT`) and the shared answer is read
 * again through the store's ordered read: every "Default" marker and checked
 * "Default" menu item then names the agent keeper now opens, with or without
 * a conversation or Settings open. Mounted once, app-wide. No polling.
 */
import { useEffect } from "react";
import { logFailure } from "@/lib/failure";
import { listenAgentsDefaultChanged } from "@/lib/ipc/client";
import { defaultAgentStore } from "@/lib/stores/default-agent";

export function useDefaultAgentFollowsSync(): void {
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listenAgentsDefaultChanged(() => {
      void defaultAgentStore.getState().load();
    })
      .then((stop) => {
        if (cancelled) {
          stop();
        } else {
          unlisten = stop;
        }
      })
      .catch(logFailure("agents.default.listen"));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
