import {
  type AgentCommandResultVm,
  type AgentSessionRef,
  agentCommand,
  type ShownApprovalVm,
} from "@/lib/ipc/client";
import { lastAgentPick, rememberAgentPick } from "@/lib/stores/agent-picks";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";

/**
 * Run a slash command `line` in session `target` of `agentActor`'s: the one path the composer and
 * the session list's right-click menu share. Rust runs it with this device's last pick for the
 * agent (`/new` and `/clean` start with it); the pick Rust wrote is remembered, and a session the
 * command created opens. `declines` is what the banner showed, which only `/stop` declines.
 */
export async function runAgentCommand(
  target: AgentSessionRef,
  agentActor: string | undefined,
  line: string,
  declines: ShownApprovalVm[] = [],
): Promise<AgentCommandResultVm> {
  const pick = agentActor === undefined ? null : lastAgentPick(agentActor);
  const result = await agentCommand(target, line, pick, declines);
  if (result.pick && agentActor !== undefined) rememberAgentPick(agentActor, result.pick);
  if (result.session) agentSessionsStore.getState().select(result.session);
  return result;
}
