/**
 * The session header's instance line (UX-DR-E106-5, FR-854): which instance of the agent is
 * answering — the claim holder — and each instance's online state, from the session's own database.
 *
 * Every word but the copy is Rust's (`AgentSessionAnsweringVm`): who holds the session, what it
 * means that the holder is offline, which instance takes a session nobody holds, and whether this
 * device can see presence at all ("Not known here" while the database does not sync, never
 * "Offline"). The frontend draws it and decides nothing.
 */
import { PresenceMark } from "@/components/agents/instance-presence";
import type { AgentSessionAnsweringVm } from "@/lib/ipc/client";

export function SessionInstances({
  answering,
  agentName,
}: {
  answering: AgentSessionAnsweringVm;
  agentName: string;
}) {
  return (
    <div
      data-slot="agent-session-instances"
      className="flex min-w-0 shrink-0 flex-col gap-1 border-border border-b px-3 py-1.5 text-xs"
    >
      <p aria-live="polite" className="[overflow-wrap:anywhere]">
        {answering.sentence}
      </p>
      <ul
        aria-label={`Instances of ${agentName}`}
        className="flex min-w-0 flex-wrap gap-x-4 gap-y-1"
      >
        {answering.instances.map((instance) => (
          <li
            key={instance.clientId}
            data-instance={instance.clientId}
            className="flex min-w-0 flex-wrap items-center gap-x-1.5"
          >
            <span className={instance.holds ? "font-medium" : ""}>{instance.label}</span>
            <PresenceMark presence={instance.presence} />
            {instance.holds && <span className="text-muted-foreground">· holds it</span>}
          </li>
        ))}
      </ul>
    </div>
  );
}
