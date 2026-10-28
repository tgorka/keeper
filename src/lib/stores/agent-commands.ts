import {
  type AgentCommandResultVm,
  type AgentSessionRef,
  agentCommand,
  type ShownApprovalVm,
} from "@/lib/ipc/client";
import { lastAgentPick, rememberAgentPick } from "@/lib/stores/agent-picks";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";

/** Open `session` in the Agents window: where a command's new session goes by default. */
function selectInAgentsWindow(session: AgentSessionRef) {
  agentSessionsStore.getState().select(session);
}

/**
 * Run a slash command `line` in session `target` of `agentActor`'s: the one path the composer, the
 * untrusted chip and the session list's right-click menu share. Rust runs it with this device's
 * last pick for the agent (`/new` and `/clean` start with it); the pick Rust wrote is remembered,
 * and a session the command created goes to `open`. `declines` is what the banner showed, which
 * only `/stop` declines. `open` defaults to the Agents window's selection; a host that holds its
 * own choice, such as the Notes Assistant dock, passes its own and leaves the window where it was
 * (R601-04).
 */
export async function runAgentCommand(
  target: AgentSessionRef,
  agentActor: string | undefined,
  line: string,
  declines: ShownApprovalVm[] = [],
  open: (session: AgentSessionRef) => void = selectInAgentsWindow,
): Promise<AgentCommandResultVm> {
  const pick = agentActor === undefined ? null : lastAgentPick(agentActor);
  const result = await agentCommand(target, line, pick, declines);
  if (result.pick && agentActor !== undefined) rememberAgentPick(agentActor, result.pick);
  if (result.session) open(result.session);
  return result;
}
