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
  type AgentNoteSessionVm,
  type AgentSessionRef,
  agentSessionDefault,
  agentSessionForNote,
} from "@/lib/ipc/client";
import { lastAgentPick } from "@/lib/stores/agent-picks";
import { useAgentSessionsStore, useAgentSessionsSubscription } from "@/lib/stores/agent-sessions";
import { columnFoldStore } from "@/lib/stores/column-fold";
import { focusInFront } from "@/lib/stores/note-caret";
import { activePanel, usePanelsStore } from "@/lib/stores/panels";
import { syncErrorMessage } from "@/lib/stores/sync";

export const DOCK_PICKER_LABEL = "Conversation";
export const NEW_CONVERSATION_LABEL = "New conversation";
export const DOCK_FOLDS_RAIL_BELOW_PX = 1136;
export const DRIVE_NOTICE_LABEL = "The note's drive";

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

/**
 * Both docks mirror the same database session list; their selection remains local. Opened on a
 * note, the dock follows the note's drive (109.28, R-HQ-86): Rust opens the default agent's chat
 * in the database named after it and ticks it, and the dock says what ticking allows at that
 * moment. A vault is one synced folder and so one drive: another note of the same vault keeps the
 * chat, and so does a note of another vault Rust names the same drive; a pick in the select holds
 * until the drive changes.
 */
export function DockBody() {
  useAgentSessionsSubscription();
  const rows = useAgentSessionsStore((s) => s.rows);
  const note = usePanelsStore((state) => {
    const target = activePanel(state)?.target;
    return target?.kind === "note" ? target : null;
  });
  const vaultId = note?.vaultId ?? null;
  // The note asked about is the one in front when the vault changes; read through a ref so a
  // note change inside the vault, which keeps the chat, asks nothing.
  const noteId = useRef<string | null>(null);
  noteId.current = note?.noteId ?? null;
  const [chosen, setChosen] = useState<AgentSessionRef | null>(null);
  const [agent, setAgent] = useState<AgentDefaultSessionVm | null>(null);
  const [creating, setCreating] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Rust's word on the note's drive: ticked now (with the warning), or why not.
  const [notice, setNotice] = useState<AgentNoteSessionVm | null>(null);
  // The drive the dock follows: `undefined` before it opened anything, `null` for none.
  const followed = useRef<string | null | undefined>(undefined);
  useEffect(() => {
    let active = true;
    const open = (target: AgentDefaultSessionVm) => {
      if (target.database && target.sessionId) {
        setChosen({ database: target.database, sessionId: target.sessionId });
        setCreating(false);
      } else setCreating(true);
    };
    const note = noteId.current;
    void (async () => {
      try {
        const main = await agentSessionDefault();
        if (!active) return;
        setAgent(main);
        if (main !== null && (vaultId === null || note === null)) {
          // No note in front: the agent's main conversation, once; the chat stays after.
          if (followed.current === undefined) {
            followed.current = null;
            open(main);
          }
        } else if (main !== null && vaultId !== null && note !== null) {
          const vm = await agentSessionForNote(vaultId, note, lastAgentPick(main.agentActor));
          if (!active) return;
          if (vm !== null) {
            setAgent(vm.target);
            setNotice(vm.warning || vm.reason ? vm : null);
            const drive = vm.drive ?? null;
            // The same drive keeps the chat, the person's own pick included.
            if (followed.current === undefined || drive !== followed.current) {
              followed.current = drive;
              open(vm.target);
            }
          }
        }
        setLoaded(true);
      } catch (raw: unknown) {
        if (active) {
          setError(syncErrorMessage(raw, "keeper could not read your default agent."));
          setLoaded(true);
        }
      }
    })();
    return () => {
      active = false;
    };
  }, [vaultId]);
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
              setNotice(null);
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
                {row.displayName} · {row.database}
                {row.archived ? " · Archived" : ""}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          variant="ghost"
          size="icon"
          aria-label={NEW_CONVERSATION_LABEL}
          onClick={() => {
            setCreating(true);
            setNotice(null);
          }}
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
      {notice && (
        // Said where the drive was ticked (R-HQ-79), or why it was not.
        <div
          role="note"
          aria-label={DRIVE_NOTICE_LABEL}
          className="border-border border-b p-3 text-sm"
        >
          {notice.warning ? (
            <>
              <p className="font-medium">
                {notice.drive} is ticked for {notice.target.agentName} to read.
              </p>
              <p className="text-muted-foreground">{notice.warning}</p>
            </>
          ) : (
            <p className="text-muted-foreground">{notice.reason}</p>
          )}
        </div>
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
        <SessionView
          key={JSON.stringify([chosen.database, chosen.sessionId])}
          target={chosen}
          // `/clean` and `/new` (and the Untrusted chip's fresh chat) move the dock to the
          // session they started and leave the Agents window's selection alone (R601-04).
          onSession={setChosen}
          focus={focusInFront}
        />
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
