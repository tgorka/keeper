/**
 * Settings › Agents & bots › Old agent rooms (108.2, UX-DR-E108-2).
 *
 * Before 108.2 an agent's conversation was a Matrix room; the agents now live
 * in their databases, and keeper hides those rooms from every list by their
 * create type. The person's accounts are still in them, so this section says
 * how many and offers the one action that leaves them all — only the person's
 * own membership changes. Which rooms are the agents' and every sentence of a
 * failure are Rust's (`keeper_core::old_agent_rooms`).
 *
 * On every tier and behind no capability: a phone's accounts are in the same
 * rooms. Absent, not empty (AD-27): with no old room left and nothing just
 * left, the section renders nothing.
 */
import { useEffect, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { logFailure } from "@/lib/failure";
import { type OldAgentRoomsVm, oldAgentRoomsGet, oldAgentRoomsLeave } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

export const OLD_AGENT_ROOMS_TITLE = "Old agent rooms";
export const OLD_AGENT_ROOMS_LEAVE = "Leave old agent rooms";
export const OLD_AGENT_ROOMS_LEAVING = "Leaving…";
const OLD_AGENT_ROOMS_FAILED = "keeper could not leave the old agent rooms.";

/** What the section says of the rooms still there. */
export function oldAgentRoomsSentence(remaining: number): string {
  const rooms = remaining === 1 ? "one room" : `${remaining} rooms`;
  return `Your accounts are still in ${rooms} where your agents worked before they moved to the agents server. keeper no longer shows them; leaving takes you out of them on the server too.`;
}

/** What the section says once a leave is done. */
export function oldAgentRoomsLeftSentence(left: number): string {
  return left === 1 ? "Left one old agent room." : `Left ${left} old agent rooms.`;
}

export function OldAgentRoomsSection({ open }: { open: boolean }) {
  const id = useId();
  const [vm, setVm] = useState<OldAgentRoomsVm | null>(null);
  const [leaving, setLeaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      return;
    }
    let live = true;
    void oldAgentRoomsGet()
      .then((read) => {
        if (live) {
          setVm(read);
        }
      })
      .catch(logFailure("settings.oldAgentRooms"));
    return () => {
      live = false;
    };
  }, [open]);

  if (vm === null || (vm.remaining === 0 && vm.left === 0 && vm.failure === null)) {
    return null;
  }

  const leave = () => {
    setLeaving(true);
    setError(null);
    void oldAgentRoomsLeave()
      .then(setVm)
      .catch((raw: unknown) => {
        logFailure("settings.oldAgentRoomsLeave")(raw);
        setError(syncErrorMessage(raw, OLD_AGENT_ROOMS_FAILED));
      })
      .finally(() => setLeaving(false));
  };

  return (
    <section
      aria-labelledby={`${id}-title`}
      className="flex min-w-0 flex-col gap-2 border-t pt-4 text-sm"
    >
      <h3 id={`${id}-title`} className="font-medium">
        {OLD_AGENT_ROOMS_TITLE}
      </h3>
      {vm.remaining > 0 && (
        <p className="text-muted-foreground text-xs [overflow-wrap:anywhere]">
          {oldAgentRoomsSentence(vm.remaining)}
        </p>
      )}
      {vm.left > 0 && (
        <p role="status" className="text-xs">
          {oldAgentRoomsLeftSentence(vm.left)}
        </p>
      )}
      {(vm.failure ?? error) !== null && (
        <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
          {vm.failure ?? error}
        </p>
      )}
      {vm.remaining > 0 && (
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={leaving}
          onClick={leave}
          className="self-start"
        >
          {leaving ? OLD_AGENT_ROOMS_LEAVING : OLD_AGENT_ROOMS_LEAVE}
        </Button>
      )}
    </section>
  );
}
