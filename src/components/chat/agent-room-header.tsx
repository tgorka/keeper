/**
 * The header line of an agent room, beside its timeline (UX-DR129).
 *
 * One line: the agent's identity cell, its `agent@host` handle, the run badge,
 * and what it is waiting for or doing. Under it, the scope chip (the drives in
 * scope, in the scope event's order) and the label chip (readers by name,
 * integrity as a word). Every word here that is not plain copy is Rust's — the
 * handle, the run, the detail drawn as sent, the unreadable sentence, the label
 * sentence — so this file only lays them out.
 *
 * It is drawn on every tier, the phone included: the phone hides the pane's own
 * header row behind its 52px bar, and an agent's status is not something the
 * phone may lose with it.
 */
import { BotIdentityCell } from "@/components/bots/bot-identity";
import { Badge } from "@/components/ui/badge";
import { Lamp, type LampState } from "@/components/ui/lamp";
import type { AgentRoomHeaderVm, AgentRunVm, LabelVm } from "@/lib/ipc/client";

/**
 * The lamp each run wears beside its word. The word carries the meaning; the
 * lamp is the second channel, so two runs may share a shape.
 */
const RUN_LAMP: Record<AgentRunVm, LampState> = {
  idle: "idle",
  running: "working",
  waiting: "idle",
  blocked: "fault",
  done: "live",
  unreadable: "fault",
};

/** The label chip's words: readers by name (or anyone), then integrity. */
function labelChipText(label: LabelVm): string {
  const readers = label.anyone ? "anyone" : label.readers.join(", ");
  return [readers, label.integrity, ...(label.localOnly ? ["local only"] : [])].join(" · ");
}

export function AgentRoomHeader({ header }: { header: AgentRoomHeaderVm }) {
  const { status, scope, scopeUnreadable, label } = header;
  // The mark is the soul's icon where the agent's zone is on this device
  // (Rust checked it against the identity's bound), else the first letter of
  // the handle — its localpart — the only name the phone has for an agent
  // whose soul it cannot read.
  const mark =
    status === null ? null : (status.icon ?? [...status.handle][0]?.toUpperCase() ?? null);
  const doing =
    status === null
      ? null
      : status.waiting !== null
        ? `waiting: ${status.waiting}${status.detail === null ? "" : ` — ${status.detail}`}`
        : status.detail;

  return (
    <section
      aria-label="Agent status"
      data-testid="agent-room-header"
      className="flex shrink-0 flex-col gap-1 border-border border-b px-3 py-2"
    >
      {status === null ? (
        <p className="text-muted-foreground text-sm">No status from the agent yet.</p>
      ) : (
        <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-sm">
          <BotIdentityCell identity={{ shape: "hollow", colour: null, mark }} />
          <span className="min-w-0 truncate font-medium">{status.handle}</span>
          <Badge variant="outline" data-testid="agent-run">
            <Lamp state={RUN_LAMP[status.run]} label={null} />
            <span className="sr-only">run: </span>
            {status.run}
          </Badge>
          {doing !== null && (
            <span className="min-w-0 truncate text-muted-foreground">{doing}</span>
          )}
        </div>
      )}
      {status?.unreadable != null && (
        <p className="text-muted-foreground text-xs">{status.unreadable}</p>
      )}
      <div className="flex min-w-0 flex-wrap items-center gap-1 text-xs">
        {scopeUnreadable !== null ? (
          <span className="text-muted-foreground">{scopeUnreadable}</span>
        ) : scope === null ? (
          <span className="text-muted-foreground">no scope yet</span>
        ) : scope.length === 0 ? (
          <span className="text-muted-foreground">no drives in scope</span>
        ) : (
          <ul aria-label="Drives in scope" className="flex flex-wrap gap-1">
            {scope.map((drive) => (
              <li key={drive.id}>
                <Badge variant="secondary">{drive.title}</Badge>
              </li>
            ))}
          </ul>
        )}
        {label !== null && (
          <Badge variant="outline" data-testid="agent-label" className="max-w-full">
            <span className="truncate">{labelChipText(label)}</span>
            <span className="sr-only">. {label.sentence}</span>
          </Badge>
        )}
      </div>
    </section>
  );
}
