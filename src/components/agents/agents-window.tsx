import { Plus } from "lucide-react";
import { useState } from "react";
import { AgentAttentionMark, agentAttentionLabel } from "@/components/agents/agent-attention";
import { DatabaseDecisionsPanel } from "@/components/agents/decisions-panel";
import { EarlierConversations } from "@/components/agents/earlier-conversations";
import { NewSession } from "@/components/agents/new-session";
import { SessionView } from "@/components/agents/session-view";
import { Button } from "@/components/ui/button";
import { useShellLayout } from "@/hooks/use-shell-layout";
import { formatRoomTimestamp } from "@/lib/format-time";
import {
  agentSessionsStore,
  useAgentSessionsStore,
  useAgentSessionsSubscription,
} from "@/lib/stores/agent-sessions";
import { cn } from "@/lib/utils";

export function AgentsWindow() {
  useAgentSessionsSubscription();
  const rows = useAgentSessionsStore((s) => s.rows);
  const loaded = useAgentSessionsStore((s) => s.loaded);
  const error = useAgentSessionsStore((s) => s.error);
  const selected = useAgentSessionsStore((s) => s.selected);
  const initial = useAgentSessionsStore((s) => s.newSession);
  const [creating, setCreating] = useState(false);
  const [filter, setFilter] = useState<"active" | "archived" | "all">("active");
  const { phone } = useShellLayout();
  const newOpen = creating || initial !== null;
  const showingDetail = newOpen || selected !== null;
  const visible = rows.filter(
    (row) => filter === "all" || row.archived === (filter === "archived"),
  );
  const back = () => {
    setCreating(false);
    agentSessionsStore.getState().select(null);
  };
  return (
    <section className="flex min-h-0 min-w-0 flex-1" aria-label="Agents">
      {(!phone || !showingDetail) && (
        <section
          aria-label="Agent sessions"
          className={cn(
            "flex min-h-0 min-w-0 flex-col border-border border-r last:border-r-0",
            phone ? "flex-1" : "w-80 shrink-0",
          )}
        >
          <header className="flex items-center justify-between gap-2 border-border border-b px-3 py-2">
            <h1 className="font-semibold text-title">Agents</h1>
            <Button
              variant="ghost"
              size="icon"
              className="size-11"
              aria-label="New session"
              onClick={() => {
                agentSessionsStore.getState().select(null);
                setCreating(true);
              }}
            >
              <Plus aria-hidden="true" />
            </Button>
          </header>
          <fieldset
            aria-label="Session filter"
            className="flex shrink-0 flex-wrap gap-1 border-border border-b px-3 py-2"
          >
            {(["active", "archived", "all"] as const).map((value) => (
              <Button
                key={value}
                size="sm"
                className={phone ? "min-h-11" : undefined}
                variant={filter === value ? "secondary" : "ghost"}
                aria-pressed={filter === value}
                onClick={() => setFilter(value)}
              >
                {value === "active" ? "Active" : value === "archived" ? "Archived" : "All"}
              </Button>
            ))}
          </fieldset>
          <div className="min-h-0 flex-1 overflow-y-auto">
            <DatabaseDecisionsPanel />
            {error && (
              <p role="alert" className="p-3 text-destructive text-sm">
                {error}
              </p>
            )}
            {!loaded && !error && (
              <p role="status" className="p-3 text-muted-foreground text-sm">
                Loading sessions…
              </p>
            )}
            {loaded && visible.length === 0 && (
              <p className="p-4 text-muted-foreground text-sm">
                {filter === "archived"
                  ? "No archived sessions."
                  : "No sessions here yet. Start a new session with an agent."}
              </p>
            )}
            <ul aria-label="Sessions" className="flex flex-col">
              {visible.map((row) => (
                <li key={JSON.stringify([row.database, row.sessionId])}>
                  <button
                    type="button"
                    aria-label={`${row.title}, ${row.agentName}, ${row.database}, ${formatRoomTimestamp(row.lastActivityMs)}${row.archived ? ", archived" : ""}${agentAttentionLabel(row.attention)}`}
                    aria-current={
                      selected?.database === row.database && selected.sessionId === row.sessionId
                        ? "true"
                        : undefined
                    }
                    className={cn(
                      "flex w-full min-w-0 flex-col gap-1 border-border border-b px-3 py-3 text-left outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
                      selected?.database === row.database &&
                        selected.sessionId === row.sessionId &&
                        "bg-accent",
                    )}
                    onClick={() => {
                      setCreating(false);
                      agentSessionsStore
                        .getState()
                        .select({ database: row.database, sessionId: row.sessionId });
                    }}
                  >
                    <span className="flex w-full min-w-0 items-baseline gap-2">
                      <span
                        className="min-w-0 flex-1 truncate font-medium text-sm"
                        title={row.title}
                      >
                        {row.title}
                      </span>
                      <span className="shrink-0 text-muted-foreground text-xs">
                        {formatRoomTimestamp(row.lastActivityMs)}
                      </span>
                    </span>
                    <span className="flex w-full min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-muted-foreground text-xs">
                      <span className="truncate">{row.agentName}</span>
                      <span className="break-all rounded-sm bg-muted px-1.5 py-0.5">
                        {row.database}
                      </span>
                      {row.archived && <span>Archived</span>}
                    </span>
                    {row.attention && <AgentAttentionMark state={row.attention} />}
                  </button>
                </li>
              ))}
            </ul>
            <EarlierConversations />
          </div>
        </section>
      )}
      {newOpen ? (
        <NewSession
          initial={initial}
          onCancel={back}
          onCreated={(target) => {
            setCreating(false);
            agentSessionsStore.getState().select(target);
          }}
        />
      ) : selected ? (
        <SessionView
          key={JSON.stringify([selected.database, selected.sessionId])}
          target={selected}
          onBack={phone ? back : undefined}
        />
      ) : (
        !phone && (
          <div className="flex min-w-0 flex-1 items-center justify-center p-6 text-muted-foreground text-sm">
            Choose a session to read its messages.
          </div>
        )
      )}
    </section>
  );
}
