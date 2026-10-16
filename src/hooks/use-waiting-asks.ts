/**
 * The questions waiting for the person that are answered in an agent room
 * (Story 101.3, R430, R431), as Rust follows them (`subscribeAgentAsks`): Rust
 * sends a fresh snapshot whenever an agent's session room this account reads —
 * where asks are asked, answered, edited and redacted — or this room's held
 * sends or send queue change. A subscription Rust refuses is logged to
 * `keeper.log` and said beside the panel (R381); the person still reads the
 * question in the room it was asked in and can answer it by typing.
 */
import { useEffect, useState } from "react";
import { logFailure } from "@/lib/failure";
import { type AgentAsksVm, subscribeAgentAsks, unsubscribeAgentAsks } from "@/lib/ipc/client";

/** What the panel says when keeper could not follow the questions here at all. */
export const ASKS_UNFOLLOWED =
  "keeper could not read the questions waiting for you here. They still show in the rooms they were asked in.";

const NONE: AgentAsksVm = { asks: [], note: null };

export function useWaitingAsks(
  accountId: string | null,
  roomId: string | null,
  enabled: boolean,
): AgentAsksVm {
  const [asks, setAsks] = useState<AgentAsksVm>(NONE);

  useEffect(() => {
    setAsks(NONE);
    if (!enabled || accountId === null || roomId === null) {
      return;
    }
    let live = true;
    let id: number | null = null;
    subscribeAgentAsks(accountId, roomId, (batch) => {
      if (live) {
        setAsks(batch);
      }
    })
      .then((subscription) => {
        id = subscription;
        if (!live) {
          void unsubscribeAgentAsks(accountId, subscription);
        }
      })
      .catch((raw: unknown) => {
        logFailure("agents.asks")(raw);
        if (live) {
          setAsks({ asks: [], note: ASKS_UNFOLLOWED });
        }
      });
    return () => {
      live = false;
      if (id !== null) {
        void unsubscribeAgentAsks(accountId, id);
      }
    };
  }, [enabled, accountId, roomId]);

  return asks;
}
