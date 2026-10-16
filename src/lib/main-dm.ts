/**
 * Opening the person's conversation with one of their own agents when this
 * device has none (story 101.1, R416): Rust asks the agent's host for it
 * through the control room and answers once the room has landed, within its
 * bound. This says Rust's "Opening a conversation with Nixi…" while it waits
 * and, when it does not open, Rust's sentence for why — where the person is
 * looking and in `keeper.log`. Which room, and why not, are Rust's.
 */
import { toast } from "sonner";
import { sayFailure } from "@/lib/failure";
import type { LaunchRoomVm, MainDmAskVm } from "@/lib/ipc/client";
import { agentMainDmOpen } from "@/lib/ipc/client";

/**
 * Ask for `ask`'s conversation; the room it opened, or `null` when it did
 * not (already said and logged).
 */
export async function openMainDm(ask: MainDmAskVm): Promise<LaunchRoomVm | null> {
  const waiting = toast.loading(ask.opening);
  try {
    return await agentMainDmOpen(ask.accountId, ask.agent);
  } catch (error: unknown) {
    sayFailure("agents.mainDm", `keeper could not open a conversation with ${ask.name}.`)(error);
    return null;
  } finally {
    toast.dismiss(waiting);
  }
}
