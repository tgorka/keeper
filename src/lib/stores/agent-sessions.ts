import { useEffect } from "react";
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { logFailure } from "@/lib/failure";
import {
  type AgentDefaultSessionVm,
  type AgentSessionRef,
  type AgentSessionRowVm,
  type AgentSessionsBatch,
  agentsSessionsSubscribe,
  agentsSessionsUnsubscribe,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

interface AgentSessionsState {
  rows: AgentSessionRowVm[];
  loaded: boolean;
  error: string | null;
  selected: AgentSessionRef | null;
  newSession: AgentDefaultSessionVm | null;
  apply: (batch: AgentSessionsBatch) => void;
  select: (target: AgentSessionRef | null) => void;
}

/** Rust owns order, attention and membership; this store only applies its diffs. */
export const agentSessionsStore = createStore<AgentSessionsState>()((set) => ({
  rows: [],
  loaded: false,
  error: null,
  selected: null,
  newSession: null,
  apply: (batch) =>
    set((state) => {
      let rows = [...state.rows];
      for (const op of batch.ops) {
        switch (op.op) {
          case "reset":
            rows = [...op.rows];
            break;
          case "insert":
            rows.splice(op.index, 0, op.row);
            break;
          case "set":
            rows[op.index] = op.row;
            break;
          case "remove":
            rows.splice(op.index, 1);
            break;
        }
      }
      return { rows, loaded: true, error: null };
    }),
  select: (selected) => set({ selected, newSession: null }),
}));

export function useAgentSessionsStore<T>(selector: (state: AgentSessionsState) => T): T {
  return useStore(agentSessionsStore, selector);
}

/** One channel shared by the window and both docks; a late start is closed on teardown. */
let readers = 0;
let stop: (() => void) | null = null;
export function useAgentSessionsSubscription(): void {
  useEffect(() => {
    readers += 1;
    if (readers === 1) {
      let active = true;
      let subscription: string | null = null;
      agentSessionsStore.setState({ error: null });
      void agentsSessionsSubscribe((batch) => {
        if (active) agentSessionsStore.getState().apply(batch);
      })
        .then((id) => {
          if (active) subscription = id;
          else void agentsSessionsUnsubscribe(id).catch(logFailure("agents.sessions.close"));
        })
        .catch((error: unknown) => {
          if (active)
            agentSessionsStore.setState({
              error: syncErrorMessage(error, "keeper could not read your sessions."),
            });
        });
      stop = () => {
        active = false;
        if (subscription !== null) {
          void agentsSessionsUnsubscribe(subscription).catch(logFailure("agents.sessions.close"));
        }
      };
    }
    return () => {
      readers -= 1;
      if (readers === 0) {
        stop?.();
        stop = null;
      }
    };
  }, []);
}
