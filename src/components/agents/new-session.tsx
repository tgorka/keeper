import { useCallback, useEffect, useId, useRef, useState } from "react";
import { Composer } from "@/components/chat/composer";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  type AgentChoiceVm,
  type AgentDefaultSessionVm,
  type AgentNoteCaret,
  type AgentSessionRef,
  agentSessionNew,
  agentsChoices,
} from "@/lib/ipc/client";
import { lastAgentPick } from "@/lib/stores/agent-picks";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";
import {
  ConversationStoresProvider,
  createConversationStores,
} from "@/lib/stores/conversation-stores";
import { syncErrorMessage } from "@/lib/stores/sync";

/**
 * The new-session composer. `focus` is the note in front of the person (the Notes Assistant),
 * read when the first message is sent and carried by it (109.28); without it, nothing is.
 */
export function NewSession({
  initial,
  onCreated,
  onCancel,
  focus,
}: {
  initial?: AgentDefaultSessionVm | null;
  onCreated: (target: AgentSessionRef) => void;
  onCancel: () => void;
  focus?: () => AgentNoteCaret | null;
}) {
  // Words a screen that took over left here (a tapped notification) come back.
  const [kept] = useState(() => agentSessionsStore.getState().newDraft);
  const [opening] = useState(() => initial?.database ?? kept?.database ?? "");
  const [choices, setChoices] = useState<AgentChoiceVm[] | null>(null);
  const [agent, setAgent] = useState(initial?.agentActor ?? kept?.agentActor ?? "");
  const [database, setDatabase] = useState(opening);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [stores] = useState(createConversationStores);
  const id = useId();
  const written = useRef(kept?.body ?? "");
  // What is written here is kept outside this screen, so leaving it any way but
  // Cancel or Send keeps it.
  const keep = useCallback(() => {
    const body = written.current;
    agentSessionsStore
      .getState()
      .keepNewDraft(body.trim() === "" ? null : { agentActor: agent, database, body });
  }, [agent, database]);
  useEffect(keep, [keep]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: once, into the composer just mounted
  useEffect(() => {
    if (kept?.body) stores.composer.getState().restore(opening, "new", kept.body);
  }, []);
  // biome-ignore lint/correctness/useExhaustiveDependencies: explicit retry rereads the failed directory
  useEffect(() => {
    let active = true;
    setError(null);
    void agentsChoices()
      .then((rows) => {
        if (!active) return;
        setChoices(rows);
        const choice = rows.find((row) => row.agentActor === initial?.agentActor);
        if (choice) setDatabase(initial?.database ?? choice.database ?? "");
      })
      .catch((raw: unknown) => {
        if (active)
          setError(syncErrorMessage(raw, "keeper could not read the agents available here."));
      });
    return () => {
      active = false;
    };
  }, [initial, attempt]);
  const choice = choices?.find((row) => row.agentActor === agent);
  return (
    <section aria-label="New session" className="flex min-h-0 min-w-0 flex-1 flex-col">
      <header className="flex items-center justify-between gap-2 border-border border-b px-3 py-2">
        <h2 className="font-semibold text-title">New session</h2>
        <Button data-slot="agent-screen-back" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
      </header>
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
        {error && (
          <div role="alert" className="text-destructive text-sm">
            {error}{" "}
            <Button variant="outline" size="sm" onClick={() => setAttempt((n) => n + 1)}>
              Try again
            </Button>
          </div>
        )}
        {!choices && !error && (
          <p role="status" className="text-muted-foreground text-sm">
            Loading agents…
          </p>
        )}
        {choices?.length === 0 && (
          <p className="text-muted-foreground text-sm">
            No agents are available on this device. Check Settings › Agents.
          </p>
        )}
        {!!choices?.length && (
          <>
            <div className="flex flex-col gap-2">
              <Label htmlFor={`${id}-agent`}>Agent</Label>
              <Select
                value={agent}
                onValueChange={(actor) => {
                  setAgent(actor);
                  setDatabase(choices.find((row) => row.agentActor === actor)?.database ?? "");
                }}
              >
                <SelectTrigger id={`${id}-agent`} className="w-full min-w-0">
                  <SelectValue placeholder="Choose an agent" />
                </SelectTrigger>
                <SelectContent>
                  {choices.map((row) => (
                    <SelectItem
                      className="min-h-11 min-w-11"
                      key={row.agentActor}
                      value={row.agentActor}
                    >
                      {row.agentName}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            {choice?.ask ? (
              <div className="flex flex-col gap-2">
                <Label htmlFor={`${id}-database`}>Database</Label>
                <Select value={database} onValueChange={setDatabase}>
                  <SelectTrigger id={`${id}-database`} className="w-full min-w-0">
                    <SelectValue placeholder="Choose a database" />
                  </SelectTrigger>
                  <SelectContent>
                    {choice.databases.map((name) => (
                      <SelectItem className="min-h-11 min-w-11" key={name} value={name}>
                        {name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <p className="text-muted-foreground text-xs">
                  This session and its messages stay in the database you choose.
                </p>
              </div>
            ) : (
              choice && (
                <p className="break-all text-muted-foreground text-sm">
                  Database: {choice.database}
                </p>
              )
            )}
          </>
        )}
      </div>
      <div className="shrink-0 border-border border-t p-3">
        <ConversationStoresProvider value={stores}>
          <Composer
            transport="session"
            accountId={database}
            roomId="new"
            disabled={!choice || !database}
            onDraftChange={(text) => {
              written.current = text;
              keep();
            }}
            onSend={async (body) => {
              if (!choice || !database) return;
              const target = await agentSessionNew(
                database,
                choice.agentActor,
                body,
                lastAgentPick(choice.agentActor),
                ...(focus ? [focus()] : []),
              );
              agentSessionsStore.getState().keepNewDraft(null);
              onCreated(target);
            }}
          />
        </ConversationStoresProvider>
      </div>
    </section>
  );
}
