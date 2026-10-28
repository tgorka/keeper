import { MessagesSquare, Plus } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
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
  agentNoteDrive,
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
 * in the database named after it and ticks it. A vault is one synced folder and so one drive:
 * another note of the same vault keeps the chat, and so does a note of another vault Rust names the
 * same drive; a pick in the select holds until the drive changes.
 *
 * What ticking allows is never the dock's to say: Rust marks every dock tick unseen, and the
 * session itself draws the warning beside its chip, with Untick and OK, wherever it is shown
 * (`SessionDrives`, round 5 ruling). The dock only says why it did not tick.
 *
 * Opening and ticking are writes, so the dock decides which drive it follows before it asks for
 * any (R605-02): it first reads the note's drive, and a note on the drive it already follows asks
 * for nothing. The writing call runs one at a time; one answered after the person moved to another
 * drive, left the note, or closed the dock, does nothing here. Nothing is ever taken back: a tick
 * it made stays, unseen, and its chat says so when it is opened.
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
  // Why the note's drive is not ticked, and the chat it is about: shown only while that chat is
  // the one in the dock.
  const [notice, setNotice] = useState<AgentNoteSessionVm | null>(null);
  // The drive the dock follows: `undefined` before it followed any, `null` for a note on none.
  const followed = useRef<string | null | undefined>(undefined);
  // The note last in front and its drive: what the dock wants to follow.
  const wanted = useRef<{ vaultId: string; noteId: string; drive: string | null } | null>(null);
  // Whether a chat is open in the dock yet, and whether a writing call is under way.
  const opened = useRef(false);
  const working = useRef(false);
  const gone = useRef(false);
  useEffect(() => {
    gone.current = false;
    return () => {
      gone.current = true;
    };
  }, []);
  const show = useCallback((target: AgentDefaultSessionVm) => {
    opened.current = true;
    if (target.database && target.sessionId) {
      setChosen({ database: target.database, sessionId: target.sessionId });
      setCreating(false);
    } else setCreating(true);
  }, []);
  // Ask Rust for the wanted drive's chat until the dock follows the drive in front, one call at a
  // time, so no answer races another.
  const follow = useCallback(
    async (main: AgentDefaultSessionVm) => {
      if (working.current) return;
      working.current = true;
      try {
        for (;;) {
          const want = wanted.current;
          if (gone.current || want === null) return;
          if (followed.current !== undefined && want.drive === followed.current) return;
          const vm = await agentSessionForNote(
            want.vaultId,
            want.noteId,
            lastAgentPick(main.agentActor),
          );
          if (vm === null) return;
          // Nobody wants this answer any more (the dock closed, the person left the note or its
          // drive): it changes nothing here, and the loop goes on to what is in front now. A tick
          // it made stays, unseen; that chat draws its warning when it is opened (round 5 ruling).
          if (gone.current || wanted.current?.drive !== want.drive) continue;
          followed.current = want.drive;
          setAgent(vm.target);
          show(vm.target);
          setNotice(vm.reason ? vm : null);
        }
      } finally {
        working.current = false;
      }
    },
    [show],
  );
  useEffect(() => {
    let active = true;
    const note = noteId.current;
    // The note in front changed: until its drive is read, no earlier answer is wanted (R605-06).
    wanted.current = null;
    void (async () => {
      try {
        const main = await agentSessionDefault();
        if (!active) return;
        setAgent(main);
        if (main !== null && vaultId !== null && note !== null) {
          const drive = await agentNoteDrive(vaultId, note);
          if (!active) return;
          wanted.current = { vaultId, noteId: note, drive };
          await follow(main);
        } else if (main !== null && !opened.current) {
          // No note in front: the agent's main conversation; the chat stays after.
          show(main);
        }
        if (active) setLoaded(true);
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
  }, [vaultId, follow, show]);
  const noticeHere =
    notice !== null &&
    !creating &&
    chosen !== null &&
    notice.target.database === chosen.database &&
    notice.target.sessionId === chosen.sessionId
      ? notice
      : null;
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
      {noticeHere && (
        // Why the note's drive is not ticked in this chat.
        <div
          role="note"
          aria-label={DRIVE_NOTICE_LABEL}
          className="border-border border-b p-3 text-sm"
        >
          <p className="text-muted-foreground">{noticeHere.reason}</p>
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
          focus={focusInFront}
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
