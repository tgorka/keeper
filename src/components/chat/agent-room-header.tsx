/**
 * The header line of an agent room, beside its timeline (UX-DR129).
 *
 * One line: the agent's identity cell, its `agent@host` handle, the run badge,
 * its model and reasoning level where its home is on this device (100.2), and
 * what it is waiting for or doing. Under it, the scope chip (the drives in
 * scope, in the scope event's order) and the label chip (readers by name,
 * integrity as a word). Every word here that is not plain copy is Rust's — the
 * handle, the run, the model line, the detail drawn as sent, the unreadable
 * sentence, the label sentence — so this file only lays them out.
 *
 * In a gate room (UX-DR147) a line above them names the outside system: the
 * `system` the gate is the door for and its account, by the name Rust quoted
 * and cut; the label chip then says `*` as "Anyone the system shares with" —
 * whoever the outside system lets read what is said here.
 *
 * It is drawn on every tier, the phone included: the phone hides the pane's own
 * header row behind its 52px bar, and an agent's status is not something the
 * phone may lose with it.
 *
 * At the end of the status line, a menu makes the agent the default agent —
 * the one keeper opens at launch (story 100.3, R383, R397) — or stops it being
 * one. It is offered only for an agent Rust lists as a choice (one with a
 * `main` room here) or the one keeper reads as the default, and the choices
 * are read from Rust again whenever this agent or the Agents window's rooms
 * change — never a snapshot from launch. When a settings file decides the
 * default, the menu says so and names the file, and a save never announces
 * an agent that will not open. A failed read is a sentence where the menu
 * would be; a failed save is a notice; both reach `keeper.log`.
 */
import { Ellipsis } from "lucide-react";
import type { ReactNode } from "react";
import { useEffect } from "react";
import { toast } from "sonner";
import { BotIdentityCell } from "@/components/bots/bot-identity";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Lamp, type LampState } from "@/components/ui/lamp";
import type { AgentRoomHeaderVm, AgentRunVm, LabelVm } from "@/lib/ipc/client";
import { useAgentRoomsStore } from "@/lib/stores/agent-rooms";
import { defaultAgentStore, useDefaultAgentStore } from "@/lib/stores/default-agent";

export const MAKE_DEFAULT_LABEL = "Make default";
export const CLEAR_DEFAULT_LABEL = "Stop opening at launch";

/**
 * The agent's menu: make it the default agent, or stop it being one. Rendered
 * only when Rust offers the agent as a choice or it is the default.
 */
function AgentDefaultMenu({ agent }: { agent: string }) {
  const vm = useDefaultAgentStore((s) => s.vm);
  const readError = useDefaultAgentStore((s) => s.readError);
  const saving = useDefaultAgentStore((s) => s.saving);
  // Which agent rooms there are, not their previews: a new or departed room
  // may change Rust's choices; a new message in one does not.
  const agentRooms = useAgentRoomsStore((s) => s.rooms.map((room) => room.roomId).join("\n"));
  // biome-ignore lint/correctness/useExhaustiveDependencies: the read is redone when the agent or the Agents window's rooms change, which is what the dependencies say.
  useEffect(() => {
    void defaultAgentStore.getState().load();
  }, [agent, agentRooms]);
  if (readError !== null) {
    return (
      <span
        role="alert"
        className="ml-auto min-w-0 text-destructive text-xs [overflow-wrap:anywhere]"
      >
        {readError}
      </span>
    );
  }
  if (vm === null) {
    return null;
  }
  const isDefault = vm.defaultAgent === agent;
  const choice = vm.choices.find((c) => c.agent === agent);
  if (!isDefault && choice === undefined) {
    return null;
  }
  const save = async (next: string | null) => {
    const saved = await defaultAgentStore.getState().choose(next);
    if (saved === null) {
      toast.error(defaultAgentStore.getState().saveError);
    } else if (saved.fileSet !== null) {
      // Stored, but a file decides what opens: say that, not the choice.
      toast(saved.fileSet);
    } else if (next !== null && saved.defaultAgent === next) {
      toast(`${choice?.name ?? agent} opens when keeper starts.`);
    }
  };
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          aria-label="Agent menu"
          className="ml-auto size-7 shrink-0"
        >
          <Ellipsis aria-hidden="true" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="end"
        className={vm.fileSet === null ? undefined : "w-80 max-w-[calc(100vw-2rem)]"}
      >
        {vm.fileSet !== null && (
          <DropdownMenuLabel className="font-normal text-muted-foreground text-xs [overflow-wrap:anywhere]">
            {vm.fileSet}
          </DropdownMenuLabel>
        )}
        <DropdownMenuItem disabled={saving} onSelect={() => void save(isDefault ? null : agent)}>
          {isDefault ? CLEAR_DEFAULT_LABEL : MAKE_DEFAULT_LABEL}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

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

/** What a gate room's label chip says for `*` (UX-DR147). */
export const GATE_ANYONE = "Anyone the system shares with";

/** The label chip's words: readers by name (or anyone), then integrity. */
function labelChipText(label: LabelVm, gate: boolean): string {
  const readers = label.anyone ? (gate ? GATE_ANYONE : "anyone") : label.readers.join(", ");
  return [readers, label.integrity, ...(label.localOnly ? ["local only"] : [])].join(" · ");
}

export function AgentRoomHeader({
  header,
  scopeControl,
}: {
  header: AgentRoomHeaderVm;
  /**
   * Drawn after the scope chip — the notes dock's editor. The chip itself always
   * shows what the host echoed, never what was asked for.
   */
  scopeControl?: ReactNode;
}) {
  const { status, scope, scopeUnreadable, label, gate } = header;
  // The mark is the soul's icon where the agent's zone is on this device
  // (Rust checked it against the identity's bound), else the first letter of
  // the handle — its localpart — the only name the phone has for an agent
  // whose soul it cannot read. The agent's picture (story 100.4), where Rust
  // found one, is drawn over it; the mark stays the fallback.
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
      {gate !== null && (
        <p
          data-testid="agent-gate"
          className="flex min-w-0 flex-wrap items-baseline gap-x-1.5 text-sm"
        >
          <span className="text-muted-foreground">Gate for</span>{" "}
          <span className="font-medium font-mono">{gate.system}</span>{" "}
          <span className="min-w-0 truncate text-muted-foreground text-xs">
            {gate.peerName === null
              ? "— no request answered yet"
              : `— ${gate.peerName} (${gate.peer})`}
          </span>
        </p>
      )}
      {status === null ? (
        <p className="text-muted-foreground text-sm">No status from the agent yet.</p>
      ) : (
        <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-sm">
          <BotIdentityCell
            identity={{ shape: "hollow", colour: null, mark }}
            image={status.avatar ?? null}
          />
          <span className="min-w-0 truncate font-medium">{status.handle}</span>
          <Badge variant="outline" data-testid="agent-run">
            <Lamp state={RUN_LAMP[status.run]} label={null} />
            <span className="sr-only">run: </span>
            {status.run}
          </Badge>
          {status.model != null && (
            <span
              data-testid="agent-model"
              className="min-w-0 truncate text-muted-foreground text-xs"
            >
              <span className="sr-only">model: </span>
              {status.model.effort === null
                ? status.model.bot
                : `${status.model.bot} · ${status.model.effort}`}
            </span>
          )}
          {doing !== null && (
            <span className="min-w-0 truncate text-muted-foreground">{doing}</span>
          )}
          <AgentDefaultMenu agent={status.agent} />
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
        {scopeControl}
        {label !== null && (
          <Badge variant="outline" data-testid="agent-label" className="max-w-full">
            <span className="truncate">{labelChipText(label, gate !== null)}</span>
            <span className="sr-only">. {label.sentence}</span>
          </Badge>
        )}
      </div>
    </section>
  );
}
