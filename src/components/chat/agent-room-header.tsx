/**
 * The header line of an agent room, beside its timeline (UX-DR129).
 *
 * One line: the agent's identity cell, its `agent@host` handle, the run badge,
 * its model and reasoning level where its home is on this device (100.2), a
 * marker while it searches the web without asking (R457), and what it is
 * waiting for or doing. Under it, the scope chip (the drives in scope, in the
 * scope event's order) and the label chip (readers by name, integrity as a
 * word). Every word here that is not plain copy is Rust's — the handle, the
 * run, the model line, the unasked-search sentence, the detail drawn as sent,
 * the unreadable sentence, the label sentence — so this file only lays them
 * out.
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
 * At the end of the status line, the agent's menu. Where this device writes
 * the agent's `agent.toml` (the desktop), it switches the agent's web search
 * (Off / Ask / Without asking) and its console's mode (story 102.3, R469)
 * through the commands Settings › Agents saves with. It also makes the agent
 * the default agent — the one keeper opens at launch (story 100.3, R383,
 * R397) — or stops it being one, for an agent Rust lists as a choice (one with
 * a `main` room here) or the one keeper reads as the default; the choices are
 * read from Rust again whenever this agent or the Agents window's rooms change
 * — never a snapshot from launch. When a settings file decides the default,
 * the menu says so and names the file, and a save never announces an agent
 * that will not open. A failed read is a sentence where the menu would be; a
 * failed save is a notice; both reach `keeper.log`.
 *
 * Under the status line: the warning the host serving the agent gives about
 * the web search its home asks for (story 102.3), in place of the globe.
 */
import { Ellipsis, Globe, LockOpen } from "lucide-react";
import type { ReactNode } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { BotIdentityCell } from "@/components/agents/identity";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Lamp, type LampState } from "@/components/ui/lamp";
import { saveDefaultAgent } from "@/lib/default-agent-actions";
import { logFailure, sayFailure } from "@/lib/failure";
import type {
  AgentCopyVm,
  AgentRoomHeaderVm,
  AgentRunVm,
  LabelVm,
  RunMode,
  WebSearchLevel,
} from "@/lib/ipc/client";
import { agentsConsoleSet, agentsCopies, agentsWebSearchSet } from "@/lib/ipc/client";
import { useAgentRoomsStore } from "@/lib/stores/agent-rooms";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { defaultAgentStore, useDefaultAgentStore } from "@/lib/stores/default-agent";
import { syncErrorMessage } from "@/lib/stores/sync";
import { WEB_SEARCH_LEVELS } from "@/lib/web-search-levels";

export const MAKE_DEFAULT_LABEL = "Make default";
export const CLEAR_DEFAULT_LABEL = "Stop opening at launch";

/**
 * The agent's row of Settings › Agents, where this device writes its
 * `agent.toml` (the desktop, `botTools`): what the menu's *Web search* and
 * *Console* choices show and save against (story 102.3, R469). Read when the
 * agent changes and each time the menu opens; a save hands back the rows as
 * Rust reads them after it. A failed read is the sentence the menu shows where
 * the choices would be, and a `keeper.log` line.
 *
 * Ordered as the default agent's store is (R397, R409; review R102T1-04):
 * every read and save takes a sequence number when it is issued, and an
 * answer — success or failure — older than the newest one applied is dropped,
 * so a read issued before a save never replaces what the save handed back. A
 * read asked for while a save is out waits for it and is issued after it.
 */
function useAgentRow(agent: string, enabled: boolean) {
  const [row, setRow] = useState<AgentCopyVm | null>(null);
  const [readError, setReadError] = useState<string | null>(null);
  const current = useRef(agent);
  current.current = agent;
  const order = useRef({ issued: 0, applied: 0, saving: 0, saved: Promise.resolve() });
  /** Run `patch` unless the agent changed or a newer answer was applied. */
  const apply = useCallback((seq: number, asked: string, patch: () => void) => {
    const now = order.current;
    if (current.current !== asked || seq < now.applied) return;
    now.applied = seq;
    patch();
  }, []);
  const shown = useCallback(
    (rows: AgentCopyVm[], asked: string) => () => {
      setRow(
        rows.find(
          (one) => one.matrixUser === asked && (one.webSearch !== null || one.console !== null),
        ) ?? null,
      );
      setReadError(null);
    },
    [],
  );
  const load = useCallback(async () => {
    if (!enabled) return;
    const asked = current.current;
    const now = order.current;
    while (now.saving > 0) {
      await now.saved;
    }
    const seq = ++now.issued;
    try {
      apply(seq, asked, shown(await agentsCopies(), asked));
    } catch (failure) {
      logFailure("agents.copies")(failure);
      apply(seq, asked, () => setReadError(syncErrorMessage(failure, AGENT_ROW_UNREAD)));
    }
  }, [enabled, apply, shown]);
  /** Save with `saving`, showing the rows it hands back; a refusal is thrown. */
  const save = useCallback(
    async (saving: () => Promise<AgentCopyVm[]>) => {
      const asked = current.current;
      const now = order.current;
      const seq = ++now.issued;
      const request = saving();
      now.saving += 1;
      // A barrier, not a handler: the rejection reaches the caller.
      now.saved = Promise.allSettled([now.saved, request]).then(() => {});
      try {
        apply(seq, asked, shown(await request, asked));
      } catch (failure) {
        // Older answers stay dropped; the caller reads the file again.
        apply(seq, asked, () => {});
        throw failure;
      } finally {
        now.saving -= 1;
      }
    },
    [apply, shown],
  );
  // biome-ignore lint/correctness/useExhaustiveDependencies: the row is read again when the agent changes, which is what `agent` says.
  useEffect(() => {
    setRow(null);
    setReadError(null);
    void load();
  }, [agent, load]);
  return { row, readError, save, load };
}

/** What the menu says when the agent's row could not be read. */
export const AGENT_ROW_UNREAD = "keeper could not read this agent's settings. Try again.";
/** The menu's two groups of tool choices (story 102.3). */
export const MENU_WEB_SEARCH_LABEL = "Web search";
export const MENU_CONSOLE_LABEL = "Console";
/** Where the console's folders are chosen: the menu switches the mode only. */
export const MENU_CONSOLE_FOLDERS = "Folders are chosen in Settings › Agents.";
/** The confirming choice after *Full access* is picked and its warning said. */
export const MENU_FULL_CONFIRM = "Give full access";
/** The choice that keeps the console as it is instead. */
export const MENU_FULL_CANCEL = "Keep the console as it is";

/**
 * What a save from the menu says (R486): it is saved, and an agent served
 * elsewhere has it once its serving host has the file — not before.
 */
export function savedSentence(name: string, what: string, label: string): string {
  return `${name}'s ${what}: ${label}. Saved; it takes effect once the host serving ${name} has it.`;
}

/**
 * The agent's menu: its *Web search* level and its *Console* mode where this
 * device writes its `agent.toml` (story 102.3, R469) — through the very
 * commands Settings › Agents saves with, against what the menu showed, so the
 * older-host check and the compare-and-replace hold here too — and making it
 * the default agent or stopping it being one, where Rust offers it as a choice
 * or it is the default. Picking *Full access* saves nothing yet: the menu says
 * Rust's full-access warning and that no folder holds it, and only *Give full
 * access* saves it (R486; review R102T1-05). A save says it is saved and takes
 * effect once the serving host has it; a refusal is Rust's sentence as a
 * notice and a `keeper.log` line.
 */
function AgentMenu({ agent }: { agent: string }) {
  const vm = useDefaultAgentStore((s) => s.vm);
  const readError = useDefaultAgentStore((s) => s.readError);
  const saving = useDefaultAgentStore((s) => s.saving);
  const writes = useCapabilitiesStore((s) => s.capabilities.botTools);
  const tools = useAgentRow(agent, writes);
  const [busy, setBusy] = useState(false);
  // *Full access* picked from a narrower mode, its warning not yet accepted.
  const [askedFull, setAskedFull] = useState(false);
  // Which agent rooms there are, not their previews: a new or departed room
  // may change Rust's choices; a new message in one does not.
  const agentRooms = useAgentRoomsStore((s) => s.rooms.map((room) => room.roomId).join("\n"));
  // biome-ignore lint/correctness/useExhaustiveDependencies: the read is redone when the agent or the Agents window's rooms change, which is what the dependencies say.
  useEffect(() => {
    void defaultAgentStore.getState().load();
  }, [agent, agentRooms]);
  if (readError !== null && tools.row === null) {
    return (
      <span
        role="alert"
        className="ml-auto min-w-0 text-destructive text-xs [overflow-wrap:anywhere]"
      >
        {readError}
      </span>
    );
  }
  const isDefault = vm !== null && vm.defaultAgent === agent;
  const choice = vm?.choices.find((c) => c.agent === agent);
  const offersDefault = vm !== null && (isDefault || choice !== undefined);
  const { row } = tools;
  if (!offersDefault && row === null && tools.readError === null) {
    return null;
  }
  const write = async (source: string, saved: () => Promise<AgentCopyVm[]>, said: string) => {
    setBusy(true);
    try {
      await tools.save(saved);
      toast(said);
    } catch (failure) {
      sayFailure(source, AGENT_ROW_UNREAD)(failure);
      // What the file says now, whatever the refusal was.
      void tools.load();
    } finally {
      setBusy(false);
    }
  };
  const webSearch = row?.webSearch ?? null;
  const runConsole = row?.console ?? null;
  const chooseLevel = (level: WebSearchLevel) => {
    if (row === null || webSearch === null || level === webSearch.level) return;
    const label = WEB_SEARCH_LEVELS.find(([one]) => one === level)?.[1] ?? level;
    void write(
      "agents.webSearchSet",
      () =>
        agentsWebSearchSet({
          profileId: row.profileId,
          agent: row.agent,
          level,
          shownLevel: webSearch.level,
        }),
      savedSentence(row.name, "web search", label),
    );
  };
  const saveMode = (mode: RunMode) => {
    if (row === null || runConsole === null || mode === runConsole.mode) return;
    const label = runConsole.modes.find((one) => one.mode === mode)?.label ?? mode;
    void write(
      "agents.consoleSet",
      () =>
        agentsConsoleSet({
          profileId: row.profileId,
          agent: row.agent,
          mode,
          paths: runConsole.paths,
          shownMode: runConsole.mode,
          shownPaths: runConsole.paths,
        }),
      savedSentence(row.name, "console", label),
    );
  };
  const chooseMode = (mode: RunMode) => {
    if (runConsole === null || mode === runConsole.mode) {
      setAskedFull(false);
      return;
    }
    if (mode === "full") {
      // Said before it is saved: what full access gives, and that no
      // folder holds it.
      setAskedFull(true);
      return;
    }
    setAskedFull(false);
    saveMode(mode);
  };
  const full = runConsole?.modes.find((one) => one.mode === "full") ?? null;
  const note = "font-normal text-muted-foreground text-xs [overflow-wrap:anywhere]";
  return (
    <DropdownMenu
      onOpenChange={(open) => {
        if (open) void tools.load();
        else setAskedFull(false);
      }}
    >
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
        className="max-h-[min(36rem,var(--radix-dropdown-menu-content-available-height))] w-80 max-w-[calc(100vw-2rem)] overflow-y-auto"
      >
        {readError !== null && (
          <DropdownMenuLabel role="alert" className="font-normal text-destructive text-xs">
            {readError}
          </DropdownMenuLabel>
        )}
        {tools.readError !== null && (
          <DropdownMenuLabel role="alert" className="font-normal text-destructive text-xs">
            {tools.readError}
          </DropdownMenuLabel>
        )}
        {webSearch !== null && (
          <DropdownMenuGroup data-testid="agent-menu-web-search">
            <DropdownMenuLabel>{MENU_WEB_SEARCH_LABEL}</DropdownMenuLabel>
            <DropdownMenuRadioGroup
              value={webSearch.level}
              onValueChange={(level) => chooseLevel(level as WebSearchLevel)}
            >
              {WEB_SEARCH_LEVELS.map(([level, label]) => (
                <DropdownMenuRadioItem
                  key={level}
                  value={level}
                  disabled={
                    busy ||
                    webSearch.locked !== null ||
                    (level !== "off" && webSearch.unavailable !== null)
                  }
                >
                  {label}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
            {(webSearch.locked ?? webSearch.unavailable) !== null && (
              <DropdownMenuLabel className={note}>
                {webSearch.locked ?? webSearch.unavailable}
              </DropdownMenuLabel>
            )}
            {webSearch.host !== null && (
              <DropdownMenuLabel
                data-testid="agent-menu-web-search-host"
                className="font-normal text-held text-xs [overflow-wrap:anywhere]"
              >
                {webSearch.host}
              </DropdownMenuLabel>
            )}
            {webSearch.warning !== null && (
              <DropdownMenuLabel className={note}>{webSearch.warning}</DropdownMenuLabel>
            )}
          </DropdownMenuGroup>
        )}
        {runConsole !== null && (
          <>
            {webSearch !== null && <DropdownMenuSeparator />}
            <DropdownMenuGroup data-testid="agent-menu-console">
              <DropdownMenuLabel>{MENU_CONSOLE_LABEL}</DropdownMenuLabel>
              <DropdownMenuRadioGroup
                value={runConsole.mode}
                onValueChange={(mode) => chooseMode(mode as RunMode)}
              >
                {runConsole.modes.map((candidate) => (
                  <DropdownMenuRadioItem
                    key={candidate.mode}
                    value={candidate.mode}
                    disabled={
                      busy ||
                      runConsole.locked !== null ||
                      (candidate.refused !== null && candidate.mode !== runConsole.mode)
                    }
                    onSelect={(event) => {
                      // Full access is said before it is saved: the menu
                      // stays open on its warning.
                      if (candidate.mode === "full" && runConsole.mode !== "full") {
                        event.preventDefault();
                      }
                    }}
                  >
                    {candidate.label}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
              {runConsole.locked !== null && (
                <DropdownMenuLabel className={note}>{runConsole.locked}</DropdownMenuLabel>
              )}
              {askedFull && full !== null && runConsole.mode !== "full" && (
                <div data-testid="agent-menu-full-access" role="alert">
                  <DropdownMenuLabel className={note}>{full.description}</DropdownMenuLabel>
                  <DropdownMenuLabel className="font-normal text-destructive text-xs [overflow-wrap:anywhere]">
                    {runConsole.warning}
                  </DropdownMenuLabel>
                  <DropdownMenuLabel className={note}>{runConsole.unrestricted}</DropdownMenuLabel>
                  <DropdownMenuItem
                    disabled={busy}
                    className="text-destructive"
                    onSelect={() => {
                      setAskedFull(false);
                      saveMode("full");
                    }}
                  >
                    {MENU_FULL_CONFIRM}
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    onSelect={(event) => {
                      event.preventDefault();
                      setAskedFull(false);
                    }}
                  >
                    {MENU_FULL_CANCEL}
                  </DropdownMenuItem>
                </div>
              )}
              {runConsole.mode === "full" && (
                <>
                  <DropdownMenuLabel className="font-normal text-destructive text-xs [overflow-wrap:anywhere]">
                    {runConsole.warning}
                  </DropdownMenuLabel>
                  <DropdownMenuLabel className={note}>{runConsole.unrestricted}</DropdownMenuLabel>
                </>
              )}
              {runConsole.locked === null && runConsole.mode !== "full" && !askedFull && (
                <DropdownMenuLabel className={note}>{MENU_CONSOLE_FOLDERS}</DropdownMenuLabel>
              )}
            </DropdownMenuGroup>
          </>
        )}
        {offersDefault && (
          <>
            {(webSearch !== null || runConsole !== null) && <DropdownMenuSeparator />}
            {vm?.fileSet != null && (
              <DropdownMenuLabel className={note}>{vm.fileSet}</DropdownMenuLabel>
            )}
            <DropdownMenuItem
              disabled={saving}
              onSelect={() => void saveDefaultAgent(isDefault ? null : agent)}
            >
              {isDefault ? CLEAR_DEFAULT_LABEL : MAKE_DEFAULT_LABEL}
            </DropdownMenuItem>
          </>
        )}
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
          {status.searchesUnasked != null && (
            <Badge
              variant="outline"
              data-testid="agent-searches-unasked"
              title={status.searchesUnasked}
              className="text-held"
            >
              <Globe aria-hidden="true" />
              <span className="sr-only">{status.searchesUnasked}</span>
            </Badge>
          )}
          {status.fullAccess != null && (
            // The console's standing full-access marker (story 101.5, R420),
            // from the serving host's status on every device (R440 09):
            // Rust's sentence says what it means, for the eye and the reader.
            <Badge variant="destructive" data-testid="agent-full-access" title={status.fullAccess}>
              <LockOpen aria-hidden="true" />
              Full access
              <span className="sr-only">. {status.fullAccess}</span>
            </Badge>
          )}
          {doing !== null && (
            <span className="min-w-0 truncate text-muted-foreground">{doing}</span>
          )}
          <AgentMenu agent={status.agent} />
        </div>
      )}
      {status?.unreadable != null && (
        <p className="text-muted-foreground text-xs">{status.unreadable}</p>
      )}
      {status?.webSearch != null && (
        // What the host serving the agent makes of the web search its home
        // asks for, when it cannot run it or does not say (story 102.3):
        // Rust's warning, in place of the globe.
        <p
          data-testid="agent-web-search-note"
          className="text-held text-xs [overflow-wrap:anywhere]"
        >
          {status.webSearch}
        </p>
      )}
      {status?.console != null && (
        // What else the serving host says of the console (R440 09, R381):
        // "console: unknown" from a host that does not say — never a safe
        // console — or the folders it does not give.
        <p
          data-testid="agent-console-note"
          className="text-muted-foreground text-xs [overflow-wrap:anywhere]"
        >
          {status.console}
        </p>
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
