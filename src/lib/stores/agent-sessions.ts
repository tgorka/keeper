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

/** What the person had written in New session, kept while another screen is shown. */
export interface NewSessionDraft {
  agentActor: string;
  database: string;
  body: string;
}

interface AgentSessionsState {
  rows: AgentSessionRowVm[];
  loaded: boolean;
  error: string | null;
  selected: AgentSessionRef | null;
  /** The open session Rust's list dropped (left, or removed by its owner), until another opens. */
  gone: { database: string; sessionId: string; displayName: string } | null;
  newSession: AgentDefaultSessionVm | null;
  /** New session is open because the person asked for it. */
  creating: boolean;
  /**
   * The words New session holds, so a screen that takes over — a tapped notification —
   * does not lose them: New session shows them again when it next opens. Cancel and a
   * created session drop them.
   */
  newDraft: NewSessionDraft | null;
  apply: (batch: AgentSessionsBatch) => void;
  /**
   * The one way anything opens a session or goes back to the list: it ends New session,
   * whatever opened it, and keeps {@link newDraft}.
   */
  select: (target: AgentSessionRef | null) => void;
  /**
   * A session whose Drives menu was asked for from the list's right-click menu (109.23): the
   * session opens and its view opens the menu once Rust's view carries the drives.
   */
  drivesFor: AgentSessionRef | null;
  /** Open `target` and ask its view for its Drives menu. */
  openDrives: (target: AgentSessionRef) => void;
  /** The view opened the menu it was asked for. */
  drivesShown: () => void;
  startNew: () => void;
  keepNewDraft: (draft: NewSessionDraft | null) => void;
}

/** Rust owns order, attention and membership; this store only applies its diffs. */
export const agentSessionsStore = createStore<AgentSessionsState>()((set) => ({
  rows: [],
  loaded: false,
  error: null,
  selected: null,
  gone: null,
  newSession: null,
  creating: false,
  newDraft: null,
  drivesFor: null,
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
      const { selected } = state;
      const dropped = state.rows.find(
        (row) => row.database === selected?.database && row.sessionId === selected.sessionId,
      );
      if (
        dropped &&
        !rows.some(
          (row) => row.database === dropped.database && row.sessionId === dropped.sessionId,
        )
      ) {
        return {
          rows,
          loaded: true,
          error: null,
          selected: null,
          gone: {
            database: dropped.database,
            sessionId: dropped.sessionId,
            displayName: dropped.displayName,
          },
        };
      }
      // A refused leave (R-HQ-40) brings the session back: the note would no longer be true.
      const { gone } = state;
      if (
        gone &&
        rows.some((row) => row.database === gone.database && row.sessionId === gone.sessionId)
      ) {
        return { rows, loaded: true, error: null, gone: null };
      }
      return { rows, loaded: true, error: null };
    }),
  select: (selected) =>
    set({ selected, gone: null, newSession: null, creating: false, drivesFor: null }),
  openDrives: (target) => {
    const selected = { database: target.database, sessionId: target.sessionId };
    set({ selected, gone: null, newSession: null, creating: false, drivesFor: selected });
  },
  drivesShown: () => set({ drivesFor: null }),
  startNew: () => set({ selected: null, gone: null, newSession: null, creating: true }),
  keepNewDraft: (newDraft) => set({ newDraft }),
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
