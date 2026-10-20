import { MessagesSquare, Plus } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { NewSession } from "@/components/agents/new-session";
import { SessionView } from "@/components/agents/session-view";
import { useSurfaceColumn } from "@/components/layout/surface-column";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  type AgentDefaultSessionVm,
  type AgentSessionRef,
  agentSessionDefault,
} from "@/lib/ipc/client";
import { useAgentSessionsStore, useAgentSessionsSubscription } from "@/lib/stores/agent-sessions";
import { columnFoldStore } from "@/lib/stores/column-fold";
import { syncErrorMessage } from "@/lib/stores/sync";

export const DOCK_PICKER_LABEL = "Conversation";
export const NEW_CONVERSATION_LABEL = "New conversation";
export const DOCK_FOLDS_RAIL_BELOW_PX = 1136;

export function NotesAgentDock() {
  const toggle = () => columnFoldStore.getState().toggleColumn("notes-agent");
  const dock = useSurfaceColumn("notes-agent", {
    rail: [
      { id: "conversation", icon: MessagesSquare, label: DOCK_PICKER_LABEL, onSelect: toggle },
    ],
    edge: "leading",
  });
  const folded = dock.folded;
  const wasFolded = useRef(folded);
  useEffect(() => {
    const opened = wasFolded.current && !folded;
    wasFolded.current = folded;
    if (folded) return;
    const makeRoom = () => {
      const fold = columnFoldStore.getState();
      if (window.innerWidth < DOCK_FOLDS_RAIL_BELOW_PX && !fold.columns["notes-rail"])
        fold.toggleColumn("notes-rail");
    };
    if (opened) makeRoom();
    window.addEventListener("resize", makeRoom);
    return () => window.removeEventListener("resize", makeRoom);
  }, [folded]);
  return (
    <>
      {dock.seam}
      <aside
        {...dock.rootProps}
        className="flex h-full min-h-0 flex-col overflow-hidden border-border border-l bg-background"
      >
        {dock.chrome}
        {!dock.folded && <DockBody />}
      </aside>
    </>
  );
}

/** Both docks mirror the same database session list; their selection remains local. */
export function DockBody() {
  useAgentSessionsSubscription();
  const rows = useAgentSessionsStore((s) => s.rows);
  const [chosen, setChosen] = useState<AgentSessionRef | null>(null);
  const [agent, setAgent] = useState<AgentDefaultSessionVm | null>(null);
  const [creating, setCreating] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    void agentSessionDefault()
      .then((vm) => {
        if (!active) return;
        setAgent(vm);
        if (vm?.database && vm.sessionId)
          setChosen({ database: vm.database, sessionId: vm.sessionId });
        else if (vm) setCreating(true);
        setLoaded(true);
      })
      .catch((raw: unknown) => {
        if (active) {
          setError(syncErrorMessage(raw, "keeper could not read your default agent."));
          setLoaded(true);
        }
      });
    return () => {
      active = false;
    };
  }, []);
  const sessions = rows.filter((row) => row.agentActor === agent?.agentActor);
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      <div className="flex min-w-0 items-center gap-2 border-border border-b p-2">
        <Select
          value={chosen ? JSON.stringify([chosen.database, chosen.sessionId]) : ""}
          onValueChange={(value) => {
            const row = sessions.find(
              (session) => JSON.stringify([session.database, session.sessionId]) === value,
            );
            if (row) {
              setChosen({ database: row.database, sessionId: row.sessionId });
              setCreating(false);
            }
          }}
        >
          <SelectTrigger aria-label={DOCK_PICKER_LABEL} className="min-w-0 flex-1">
            <SelectValue placeholder="Choose a session" />
          </SelectTrigger>
          <SelectContent>
            {sessions.map((row) => (
              <SelectItem
                key={JSON.stringify([row.database, row.sessionId])}
                value={JSON.stringify([row.database, row.sessionId])}
              >
                {row.title} · {row.database}
                {row.archived ? " · Archived" : ""}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          variant="ghost"
          size="icon"
          aria-label={NEW_CONVERSATION_LABEL}
          onClick={() => setCreating(true)}
        >
          <Plus aria-hidden="true" />
        </Button>
      </div>
      {error && (
        // A background assistant lookup must not announce an urgent Notes-page error.
        // Explicit session actions keep their own alerts inside the session view.
        <p
          role="status"
          aria-label="Assistant availability"
          className="p-3 text-muted-foreground text-sm"
        >
          {error}
        </p>
      )}
      {!loaded && (
        <p role="status" className="p-3 text-muted-foreground text-sm">
          Loading your assistant…
        </p>
      )}
      {creating ? (
        <NewSession
          initial={agent}
          onCancel={() => setCreating(false)}
          onCreated={(target) => {
            setChosen(target);
            setCreating(false);
          }}
        />
      ) : chosen ? (
        <SessionView key={JSON.stringify([chosen.database, chosen.sessionId])} target={chosen} />
      ) : (
        loaded && (
          <p className="p-3 text-muted-foreground text-sm">
            Choose a default agent in Settings › Agents, or start a new session.
          </p>
        )
      )}
    </div>
  );
}
