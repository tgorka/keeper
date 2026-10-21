/**
 * Settings › Agents › Answers from (UX-DR-E106-5, AD-425): per agent, its instances — which are
 * online, and one whose definition differs from its siblings' — and the person's choice of the
 * instance that answers new sessions, with the order the others answer in while it is offline.
 *
 * Every word but the copy is Rust's (`AgentInstancesVm`): who is online (unknown while a database
 * does not sync here, never "offline"), the definition marks, what the choice does, and when the
 * rows of the person's databases differ. The choice is an `instance_prefs` row in each database the
 * person and the agent share — never a setting of this device — so every device follows it after
 * one sync. This only says which order to write; Rust checks it and writes the rows.
 *
 * On every tier, behind no capability: a phone chooses the same way. Absent, not empty, while no
 * held database lists an instance of any agent.
 */
import { ArrowDown, ArrowUp } from "lucide-react";
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { PresenceMark } from "@/components/agents/instance-presence";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { logFailure } from "@/lib/failure";
import {
  type AgentInstancesVm,
  type AgentInstanceVm,
  agentInstancesSet,
  agentInstancesSubscribe,
  agentInstancesUnsubscribe,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

export const AGENT_INSTANCES_TITLE = "Answers from";
export const AGENT_INSTANCES_NOTE =
  "An agent can run as several instances, on a server and on your laptop. Choose the one that " +
  "answers your new sessions; while it is offline the others answer in the order below. The " +
  "choice is kept in your agents' databases, so each of your devices follows it.";
const READ_FAILED = "Could not read your agents' instances.";
const SAVE_FAILED = "Your choice was not saved.";

/** The order with the instance at `at` moved one place toward `to`. */
function moved(agent: AgentInstancesVm, at: number, to: number): string[] {
  const order = agent.instances.map((i) => i.clientId);
  [order[at], order[to]] = [order[to], order[at]];
  return order;
}

const FALLBACK_PLACE = ["First", "Second", "Third", "Fourth", "Fifth"];

/** Where the person's order puts an instance, or nothing when it does not name it. */
function place(instance: AgentInstanceVm): string | null {
  if (instance.rank === undefined) return null;
  if (instance.rank === 0) return "Answers first";
  return `${FALLBACK_PLACE[instance.rank - 1] ?? `Number ${instance.rank}`} fallback`;
}

function AgentInstances({
  agent,
  busy,
  write,
}: {
  agent: AgentInstancesVm;
  busy: boolean;
  write: (order: string[]) => void;
}) {
  const id = useId();
  const box = useRef<HTMLFieldSetElement>(null);
  // The control the person used, found again once Rust's next list has moved its row: a write
  // never disables what holds focus, and a moved row must not drop it on the dialog.
  const refocus = useRef<{ client: string; move: "earlier" | "later" | null } | null>(null);
  const chosen = agent.defaultClient !== undefined;
  // Reordering needs at least two fallbacks.
  const reorder = chosen && agent.instances.length > 2;
  // biome-ignore lint/correctness/useExhaustiveDependencies: refocus once Rust's next list for this agent has rendered, which is what `agent` changing means.
  useEffect(() => {
    const wanted = refocus.current;
    const root = box.current;
    if (wanted === null || root === null) return;
    refocus.current = null;
    const row = root.querySelector<HTMLElement>(`[data-instance="${CSS.escape(wanted.client)}"]`);
    const control =
      wanted.move === null
        ? row?.querySelector<HTMLElement>('[role="radio"]')
        : (row?.querySelector<HTMLButtonElement>(`[data-move="${wanted.move}"]:not(:disabled)`) ??
          row?.querySelector<HTMLButtonElement>("[data-move]:not(:disabled)"));
    control?.focus();
  }, [agent]);
  return (
    <fieldset
      ref={box}
      aria-labelledby={`${id}-agent`}
      data-agent={agent.agentActor}
      className="flex min-w-0 flex-col gap-2 rounded-md border p-3"
    >
      <legend id={`${id}-agent`} className="px-1 font-medium [overflow-wrap:anywhere]">
        {agent.agentName}
      </legend>
      <p className="text-muted-foreground text-xs [overflow-wrap:anywhere]">
        {agent.databases.join(", ")}
      </p>
      <p className="[overflow-wrap:anywhere]">{agent.sentence}</p>
      {agent.disagreement !== undefined && (
        <p role="note" className="text-xs [overflow-wrap:anywhere]">
          {agent.disagreement}
        </p>
      )}
      <RadioGroup
        value={agent.defaultClient ?? ""}
        onValueChange={(value) => {
          if (busy) return;
          refocus.current = { client: value, move: null };
          // The chosen instance first, then every other one as shown.
          write([value, ...agent.instances.map((i) => i.clientId).filter((c) => c !== value)]);
        }}
        aria-label={`${AGENT_INSTANCES_TITLE}: ${agent.agentName}`}
        aria-busy={busy}
        className="gap-1"
      >
        {agent.instances.map((instance, at) => {
          const where = place(instance);
          const fallback = reorder && at > 0;
          return (
            <div
              key={instance.clientId}
              data-instance={instance.clientId}
              className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 py-1"
            >
              <RadioGroupItem
                id={`${id}-${instance.clientId}`}
                value={instance.clientId}
                className="shrink-0"
              />
              <Label
                htmlFor={`${id}-${instance.clientId}`}
                className="flex min-h-11 min-w-0 flex-1 flex-col items-start justify-center gap-0 font-normal [overflow-wrap:anywhere]"
              >
                <span className="font-medium">{instance.label}</span>
                {instance.description !== undefined && (
                  <span className="text-muted-foreground text-xs">{instance.description}</span>
                )}
              </Label>
              <PresenceMark presence={instance.presence} />
              {where !== null && <span className="text-muted-foreground text-xs">{where}</span>}
              {fallback && (
                <span className="flex shrink-0 gap-1">
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="size-11"
                    data-move="earlier"
                    aria-label={`Ask ${instance.label} earlier`}
                    disabled={at <= 1}
                    onClick={() => {
                      if (busy) return;
                      refocus.current = { client: instance.clientId, move: "earlier" };
                      write(moved(agent, at, at - 1));
                    }}
                  >
                    <ArrowUp aria-hidden="true" />
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="size-11"
                    data-move="later"
                    aria-label={`Ask ${instance.label} later`}
                    disabled={at >= agent.instances.length - 1}
                    onClick={() => {
                      if (busy) return;
                      refocus.current = { client: instance.clientId, move: "later" };
                      write(moved(agent, at, at + 1));
                    }}
                  >
                    <ArrowDown aria-hidden="true" />
                  </Button>
                </span>
              )}
              {instance.definitionDiffers !== undefined && (
                <p className="w-full text-xs [overflow-wrap:anywhere]">
                  <span className="font-medium">Different definition</span>
                  {instance.definition !== undefined && (
                    <span className="text-muted-foreground"> ({instance.definition})</span>
                  )}
                  : {instance.definitionDiffers}
                </p>
              )}
            </div>
          );
        })}
      </RadioGroup>
      {agent.notice !== undefined && (
        <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
          {agent.notice}
        </p>
      )}
    </fieldset>
  );
}

export function AgentInstancesSection({ open }: { open: boolean }) {
  const id = useId();
  const [agents, setAgents] = useState<AgentInstancesVm[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [alert, setAlert] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    let subscription: string | null = null;
    void agentInstancesSubscribe((next) => {
      if (!cancelled) setAgents(next);
    })
      .then((sid) => {
        if (cancelled) void agentInstancesUnsubscribe(sid);
        else subscription = sid;
      })
      .catch((raw: unknown) => {
        logFailure("agents.instancesSubscribe")(raw);
        if (!cancelled) setAlert(syncErrorMessage(raw, READ_FAILED));
      });
    return () => {
      cancelled = true;
      if (subscription !== null) void agentInstancesUnsubscribe(subscription);
    };
  }, [open]);

  const write = useCallback((agentActor: string, order: string[]) => {
    setBusy(true);
    setAlert(null);
    void agentInstancesSet(agentActor, order)
      .catch((raw: unknown) => {
        logFailure("agents.instancesSet")(raw);
        setAlert(syncErrorMessage(raw, SAVE_FAILED));
      })
      .finally(() => setBusy(false));
  }, []);

  if ((agents === null || agents.length === 0) && alert === null) return null;

  return (
    <section
      aria-labelledby={`${id}-title`}
      className="flex min-w-0 flex-col gap-3 border-t pt-4 text-sm"
    >
      <h3 id={`${id}-title`} className="font-medium">
        {AGENT_INSTANCES_TITLE}
      </h3>
      <p className="text-muted-foreground text-xs">{AGENT_INSTANCES_NOTE}</p>
      {alert !== null && (
        <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
          {alert}
        </p>
      )}
      {agents?.map((agent) => (
        <AgentInstances
          key={agent.agentActor}
          agent={agent}
          busy={busy}
          write={(order) => write(agent.agentActor, order)}
        />
      ))}
    </section>
  );
}
