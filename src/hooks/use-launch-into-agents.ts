import { useEffect, useRef } from "react";
import { toast } from "sonner";
import { agentSessionDefault } from "@/lib/ipc/client";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";
import { useCapabilitiesStore, useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
import { useDefaultAgentStore } from "@/lib/stores/default-agent";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";
import { syncErrorMessage } from "@/lib/stores/sync";

/** Rust chooses a local default session, without waiting for Matrix or a network read. */
export function useLaunchIntoAgents(): void {
  const ready = useCapabilitiesStore((s) => s.hydrated);
  const phone = useIsReducedCapabilityPlatform();
  const navRestore = useDefaultAgentStore((s) => s.navRestore);
  const asked = useRef(false);
  const moved = useRef(false);
  const applying = useRef(false);
  useEffect(() => {
    const offView = primaryViewStore.subscribe((state, previous) => {
      if (state.view !== previous.view && !applying.current) moved.current = true;
    });
    const offRoom = roomsStore.subscribe((state, previous) => {
      if (state.selected !== previous.selected && !applying.current) moved.current = true;
    });
    const offSession = agentSessionsStore.subscribe((state, previous) => {
      if (state.selected !== previous.selected && !applying.current) moved.current = true;
    });
    return () => {
      offView();
      offRoom();
      offSession();
    };
  }, []);
  useEffect(() => {
    if (asked.current || !ready || (phone && navRestore === "pending")) return;
    asked.current = true;
    if (moved.current || (phone && navRestore === "restored")) return;
    void agentSessionDefault()
      .then((vm) => {
        if (moved.current || vm === null) return;
        applying.current = true;
        roomsStore.getState().selectRoom(null);
        primaryViewStore.getState().setView("agents");
        if (vm.database !== null && vm.sessionId !== null) {
          agentSessionsStore.getState().select({ database: vm.database, sessionId: vm.sessionId });
        } else agentSessionsStore.setState({ selected: null, newSession: vm });
        applying.current = false;
      })
      .catch((raw: unknown) =>
        toast.error(syncErrorMessage(raw, "keeper could not open your default agent.")),
      );
  }, [ready, phone, navRestore]);
}
