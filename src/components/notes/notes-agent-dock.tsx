/**
 * The assistant beside your notes (UX-DR130, AD-382).
 *
 * A folding column RIGHT of the panel strip holding the person's proxy room —
 * the DM with it by default, or one of its conversations from the picker — as
 * the same timeline and composer the chat draws ({@link ConversationBody}),
 * under stores of its own: the dock never changes which room the chat has
 * selected, its reply context or its focus.
 *
 * What it says to the proxy, and when:
 * - **The note in front of you.** While the dock is open, every change of the
 *   active panel's note, of the caret's line in it or of the editor's buffer is
 *   handed to Rust as `(vault, note, line, the buffer through that line)`; Rust
 *   names the note's drive, path and heading once it has been still a second,
 *   and sends it only when it changed. Every call carries a growing number, so
 *   Rust drops one that arrives after the close it preceded. Open, the dock says
 *   the note again every {@link FOCUS_HEARTBEAT_MS}: the host forgets a note it
 *   has not heard for three of those, so a quit or a crash that never sent the
 *   clear stops being stated. Closing the dock (folding it, or leaving Notes)
 *   says there is none. Folded, nothing is sent: the body is unmounted, and so
 *   is this.
 * - **The drives in scope.** The scope chip is the room's header, which shows
 *   what the proxy's host ECHOED. The editor beside it asks for drives from the
 *   proxy's `[tools].drives`; it never draws its own answer, and says it asked
 *   until the host answers. Where the proxy's agents zone is not on this device
 *   (`allowed` is null) there is no editor.
 * - **A new conversation.** Asked for in the DM; the proxy's host makes the
 *   room, and the dock looks for it and opens it once its status has arrived.
 */
import { MessagesSquare, Plus, SlidersHorizontal } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ConversationBody } from "@/components/chat/conversation-body";
import { useSurfaceColumn } from "@/components/layout/surface-column";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { IconHint } from "@/components/ui/tooltip";
import type { AgentFocusReq, ProxyRoomVm, ScopeDriveVm } from "@/lib/ipc/client";
import { agentConversationNew, agentFocus, agentRoomsList, agentScopeSet } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { columnFoldStore } from "@/lib/stores/column-fold";
import {
  ConversationStoresProvider,
  createConversationStores,
} from "@/lib/stores/conversation-stores";
import { textThroughCaret, useNoteCaret } from "@/lib/stores/note-caret";
import { activePanel, usePanelsStore } from "@/lib/stores/panels";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The session picker's accessible name. */
export const DOCK_PICKER_LABEL = "Conversation";

/** The control that asks the proxy for a new conversation. */
export const NEW_CONVERSATION_LABEL = "New conversation";

/** The scope editor's trigger, beside the scope chip. */
export const SCOPE_EDIT_LABEL = "Choose drives in scope";

/** What the dock says while the proxy rooms are being listed. */
export const DOCK_LOADING_LABEL = "Finding your assistant…";

/** What the dock says to a person with no proxy. */
export const DOCK_NO_PROXY_SENTENCE =
  "You have no assistant yet. Once an agent is set up as yours (Settings › Agents › Set up agents), its conversation with you opens here.";

/** The control that lists the proxy's rooms again. */
export const DOCK_LOOK_AGAIN_LABEL = "Look again";

/**
 * How often the open dock says the note in front of the person again
 * (`keeper_core::agents::focus::FOCUS_HEARTBEAT`).
 */
export const FOCUS_HEARTBEAT_MS = 5 * 60_000;

/**
 * While the dock lists no assistant, it looks again after this, doubling up to a
 * minute: right after a cold start the rooms' statuses may not be decrypted
 * yet, and right after *Set up agents* the DM's status may not have arrived.
 */
export const RELIST_FIRST_MS = 5_000;
const RELIST_MOST_MS = 60_000;

/** After asking for a new conversation, the dock looks for it this often, this long. */
export const NEW_CONVERSATION_POLL_MS = 3_000;
export const NEW_CONVERSATION_WAIT_MS = 120_000;

/** How long the scope editor waits for the host's answer before saying none came. */
export const SCOPE_ANSWER_WAIT_MS = 30_000;

/**
 * Below this window width the open dock does not fit beside the notes rail, the
 * list and one panel at their floors with the 156 px drawer (156 + 180 + 240 +
 * 280 + 280), so opening it folds the rail to its strip. `window-minimum.test.ts`
 * holds this number to those floors.
 */
export const DOCK_FOLDS_RAIL_BELOW_PX = 1136;

/** A proxy room and the account that is in it. */
type DockRoom = ProxyRoomVm & { accountId: string };

interface RoomRef {
  accountId: string;
  roomId: string;
}

let lastFocusSeq = 0;

/**
 * The next `agentFocus` call's number: microseconds since the epoch, and past
 * the last one, so it grows across reloads too.
 */
function nextFocusSeq(): number {
  lastFocusSeq = Math.max(
    lastFocusSeq + 1,
    Math.floor((performance.timeOrigin + performance.now()) * 1000),
  );
  return lastFocusSeq;
}

export function NotesAgentDock() {
  const toggle = () => columnFoldStore.getState().toggleColumn("notes-agent");
  const dock = useSurfaceColumn("notes-agent", {
    rail: [
      { id: "conversation", icon: MessagesSquare, label: DOCK_PICKER_LABEL, onSelect: toggle },
    ],
    edge: "leading",
  });
  const folded = dock.folded;
  // Opened in a window too narrow for every column at its floor (or the window
  // narrowed while it is open), the dock takes the notes rail's width rather
  // than clipping the note or itself. A dock that comes up open was opened in
  // an earlier window, where this already ran; the fold it made is remembered.
  const wasFolded = useRef(folded);
  useEffect(() => {
    const opened = wasFolded.current && !folded;
    wasFolded.current = folded;
    if (folded) return;
    const makeRoom = () => {
      const fold = columnFoldStore.getState();
      if (window.innerWidth < DOCK_FOLDS_RAIL_BELOW_PX && !fold.columns["notes-rail"]) {
        fold.toggleColumn("notes-rail");
      }
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
        // The dock owns the hairline against the strip: the strip's last
        // panel cancels its own `border-r`, so this edge has one owner.
        className="flex h-full min-h-0 flex-col overflow-hidden border-border border-l bg-background"
      >
        {dock.chrome}
        {!dock.folded && <DockBody />}
      </aside>
    </>
  );
}

function DockBody() {
  const accounts = useAccountsStore((s) => s.accounts);
  // `null` while listing. Every account's proxy rooms, account by account, each
  // account's DM first as Rust ordered them — so the first row is the DM.
  const [listed, setListed] = useState<DockRoom[] | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [chosen, setChosen] = useState<RoomRef | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // A conversation asked for and not listed yet: the rooms the account had then.
  const [awaiting, setAwaiting] = useState<{
    accountId: string;
    proxyName: string;
    known: ReadonlySet<string>;
    until: number;
  } | null>(null);
  // The dock's own timeline, composer and tray, for as long as it is open.
  const [stores] = useState(createConversationStores);
  const alive = useRef(true);
  const generation = useRef(0);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  // The newest listing wins: one the accounts outdated, or a slow one that a
  // later look overtook, is dropped.
  const relist = useCallback(async (): Promise<DockRoom[] | null> => {
    const mine = ++generation.current;
    try {
      const per = await Promise.all(
        accounts.map(async ({ accountId }) =>
          (await agentRoomsList(accountId)).map((room) => ({ ...room, accountId })),
        ),
      );
      const next = per.flat();
      if (alive.current && mine === generation.current) {
        setListed(next);
        setListError(null);
      }
      return next;
    } catch (error: unknown) {
      if (alive.current && mine === generation.current) {
        setListed((was) => was ?? []);
        setListError(syncErrorMessage(error, "keeper could not list your assistant's rooms."));
      }
      return null;
    }
  }, [accounts]);

  useEffect(() => {
    void relist();
  }, [relist]);

  // A signed-out account's rooms go now, not when the relist answers.
  const rooms = useMemo(() => {
    if (listed === null) return null;
    const live = new Set(accounts.map((account) => account.accountId));
    return listed.filter((room) => live.has(room.accountId));
  }, [listed, accounts]);
  const empty = rooms !== null && rooms.length === 0;

  useEffect(() => {
    if (!empty) return;
    let delay = RELIST_FIRST_MS;
    let timer = 0;
    const look = () => {
      timer = window.setTimeout(() => {
        void relist();
        delay = Math.min(delay * 2, RELIST_MOST_MS);
        look();
      }, delay);
    };
    look();
    return () => clearTimeout(timer);
  }, [empty, relist]);

  useEffect(() => {
    if (awaiting === null) return;
    const timer = setInterval(() => {
      if (Date.now() > awaiting.until) {
        setAwaiting(null);
        setNotice(
          `${awaiting.proxyName} has not opened the conversation yet. It is in the list once it has.`,
        );
        return;
      }
      void relist().then((next) => {
        const made = next?.find(
          (room) =>
            room.accountId === awaiting.accountId &&
            room.kind === "conversation" &&
            !awaiting.known.has(room.roomId),
        );
        if (made !== undefined && alive.current) {
          setChosen({ accountId: made.accountId, roomId: made.roomId });
          setAwaiting(null);
          setNotice(null);
        }
      });
    }, NEW_CONVERSATION_POLL_MS);
    return () => clearInterval(timer);
  }, [awaiting, relist]);

  const current =
    rooms === null
      ? null
      : (rooms.find((r) => r.accountId === chosen?.accountId && r.roomId === chosen.roomId) ??
        rooms[0] ??
        null);

  if (rooms === null) {
    return (
      <p role="status" className="p-3 text-muted-foreground text-sm">
        {DOCK_LOADING_LABEL}
      </p>
    );
  }
  if (current === null) {
    return (
      <div className="flex flex-col items-start gap-2 p-3 text-sm">
        {listError === null ? (
          <p className="text-muted-foreground">{DOCK_NO_PROXY_SENTENCE}</p>
        ) : (
          <p role="alert" className="text-destructive text-xs">
            {listError}
          </p>
        )}
        <Button type="button" variant="outline" size="sm" onClick={() => void relist()}>
          {DOCK_LOOK_AGAIN_LABEL}
        </Button>
      </div>
    );
  }

  const dm = rooms.find((r) => r.accountId === current.accountId && r.kind === "main") ?? null;
  const allowed = current.allowed;
  // Two accounts' proxies may share a name; the account tells them apart.
  const manyAccounts = new Set(rooms.map((room) => room.accountId)).size > 1;
  const userOf = (accountId: string) =>
    accounts.find((account) => account.accountId === accountId)?.userId ?? accountId;

  return (
    <ConversationStoresProvider value={stores}>
      <NoteFocus accountId={current.accountId} roomId={current.roomId} />
      <div className="flex shrink-0 items-center gap-1 border-border border-b p-2">
        <Select
          value={`${current.accountId} ${current.roomId}`}
          onValueChange={(value) => {
            const [account, ...room] = value.split(" ");
            setChosen({ accountId: account, roomId: room.join(" ") });
            setNotice(null);
          }}
          onOpenChange={(open) => {
            // A conversation made since the list was read is offered here.
            if (open) void relist();
          }}
        >
          <SelectTrigger className="min-w-0 flex-1" aria-label={DOCK_PICKER_LABEL}>
            <SelectValue placeholder={DOCK_PICKER_LABEL} />
          </SelectTrigger>
          <SelectContent>
            {rooms.map((room) => (
              <SelectItem
                key={`${room.accountId} ${room.roomId}`}
                value={`${room.accountId} ${room.roomId}`}
              >
                {manyAccounts ? `${room.name} · ${userOf(room.accountId)}` : room.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {dm !== null && (
          <NewConversation
            // Another account's DM starts the form over: its title and error
            // were the other proxy's.
            key={dm.accountId}
            accountId={dm.accountId}
            dmRoomId={dm.roomId}
            proxyName={dm.name}
            onAsked={() => {
              setNotice(
                `Asked ${dm.name} for a new conversation. It opens here once ${dm.name} has made it.`,
              );
              setAwaiting({
                accountId: dm.accountId,
                proxyName: dm.name,
                known: new Set(
                  rooms.filter((r) => r.accountId === dm.accountId).map((r) => r.roomId),
                ),
                until: Date.now() + NEW_CONVERSATION_WAIT_MS,
              });
            }}
          />
        )}
      </div>
      {notice !== null && (
        <p
          role="status"
          className="shrink-0 border-border border-b px-3 py-2 text-muted-foreground text-xs"
        >
          {notice}
        </p>
      )}
      <ConversationBody
        accountId={current.accountId}
        roomId={current.roomId}
        followsSearch={false}
        acceptsWindowDrops={false}
        scopeControl={
          allowed === null || allowed.length === 0
            ? undefined
            : (header) => (
                <ScopeEditor
                  // A room switch starts the editor over; its draft was the other room's.
                  key={`${current.accountId} ${current.roomId}`}
                  accountId={current.accountId}
                  roomId={current.roomId}
                  allowed={allowed}
                  scope={header.scope}
                  agentName={header.status?.agentName ?? current.name}
                  detail={header.status?.detail ?? null}
                />
              )
        }
      />
    </ConversationStoresProvider>
  );
}

/**
 * Tell the docked room which note is in front of the person while the dock is
 * open, again every {@link FOCUS_HEARTBEAT_MS}, and that there is none when it
 * closes or moves to another room. A component of its own so that a keystroke,
 * which changes the buffer, re-renders this and not the docked conversation.
 *
 * Every change is handed over; Rust debounces (one event per second of
 * stillness, only on change), so this never times anything but the heartbeat.
 */
function NoteFocus({ accountId, roomId }: RoomRef) {
  const note = usePanelsStore((s) => {
    const target = activePanel(s).target;
    return target?.kind === "note" ? target : null;
  });
  const vaultId = note?.vaultId ?? null;
  const noteId = note?.noteId ?? null;
  // An editor publishes its caret and buffer as soon as it opens; until one has
  // (a note shown as Preview has neither) the note is read from its top, from disk.
  const caret = useNoteCaret(vaultId, noteId);
  const focus = useMemo((): AgentFocusReq | null => {
    if (vaultId === null || noteId === null) return null;
    return caret === null
      ? { vaultId, noteId, line: 1 }
      : { vaultId, noteId, line: caret.line, text: textThroughCaret(caret) };
  }, [vaultId, noteId, caret]);
  const focusRef = useRef(focus);
  focusRef.current = focus;

  useEffect(() => {
    void agentFocus(accountId, roomId, nextFocusSeq(), focus).catch(() => {});
  }, [accountId, roomId, focus]);

  useEffect(() => {
    const timer = setInterval(() => {
      const now = focusRef.current;
      if (now !== null) void agentFocus(accountId, roomId, nextFocusSeq(), now).catch(() => {});
    }, FOCUS_HEARTBEAT_MS);
    return () => {
      clearInterval(timer);
      void agentFocus(accountId, roomId, nextFocusSeq(), null).catch(() => {});
    };
  }, [accountId, roomId]);

  return null;
}

function NewConversation({
  accountId,
  dmRoomId,
  proxyName,
  onAsked,
}: {
  accountId: string;
  dmRoomId: string;
  proxyName: string;
  onAsked: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const ask = () => {
    setAsking(true);
    setError(null);
    const trimmed = title.trim();
    agentConversationNew(accountId, dmRoomId, trimmed === "" ? null : trimmed)
      .then(() => {
        setOpen(false);
        setTitle("");
        onAsked();
      })
      .catch((raw: unknown) => {
        setError(syncErrorMessage(raw, `keeper could not ask ${proxyName}.`));
      })
      .finally(() => setAsking(false));
  };

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        // Reopened, the form starts without the last attempt's error.
        if (next) setError(null);
        setOpen(next);
      }}
    >
      <IconHint label={NEW_CONVERSATION_LABEL}>
        <PopoverTrigger asChild>
          <Button type="button" variant="ghost" size="icon" aria-label={NEW_CONVERSATION_LABEL}>
            <Plus aria-hidden="true" />
          </Button>
        </PopoverTrigger>
      </IconHint>
      <PopoverContent align="end" className="w-72 gap-3">
        <form
          className="flex flex-col gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            ask();
          }}
        >
          <Label htmlFor="dock-new-conversation-title">Title (optional)</Label>
          <Input
            id="dock-new-conversation-title"
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            placeholder="conversation"
          />
          <p className="text-muted-foreground text-xs">
            The title travels encrypted; the room itself is named after {proxyName}.
          </p>
          <Button type="submit" size="sm" disabled={asking}>
            {`Ask ${proxyName}`}
          </Button>
          {error !== null && (
            <p role="alert" className="text-destructive text-xs">
              {error}
            </p>
          )}
        </form>
      </PopoverContent>
    </Popover>
  );
}

/**
 * Ask for drives in scope. Starts from the drives the host last echoed — and
 * starts over whenever it echoes again; the home drive (the first `allowed`) is
 * always in scope, so it is shown ticked and cannot be unticked. A drive the
 * echo holds that this device's `allowed` does not list is shown too, so asking
 * never drops it unseen. What the chip shows afterwards is the host's answer;
 * until it comes, this says it asked.
 */
function ScopeEditor({
  accountId,
  roomId,
  allowed,
  scope,
  agentName,
  detail,
}: {
  accountId: string;
  roomId: string;
  allowed: ScopeDriveVm[];
  scope: ScopeDriveVm[] | null;
  agentName: string;
  detail: string | null;
}) {
  const home = allowed[0].id;
  const [open, setOpen] = useState(false);
  const [picked, setPicked] = useState<ReadonlySet<string>>(new Set());
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [waiting, setWaiting] = useState<"asked" | "silent" | null>(null);

  const offered = useMemo(
    () => [
      ...allowed,
      ...(scope ?? []).filter((drive) => !allowed.some((known) => known.id === drive.id)),
    ],
    [allowed, scope],
  );
  // The host's answer as one value, so a header redrawn with the same scope
  // is no news.
  const echoedKey = [home, ...(scope ?? []).map((drive) => drive.id)].join("\n");
  const echoed = useMemo(() => new Set(echoedKey.split("\n")), [echoedKey]);
  // The answer may arrive before the ask resolves: then there is nothing to wait for.
  const echoedNow = useRef(echoedKey);
  echoedNow.current = echoedKey;

  // An answer — a new scope, or a refusal in the status — ends the wait, and
  // the draft starts again from what the host now says.
  useEffect(() => {
    setWaiting(null);
    setPicked(echoed);
  }, [echoed]);
  useEffect(() => {
    if (detail !== null) setWaiting(null);
  }, [detail]);
  useEffect(() => {
    if (waiting !== "asked") return;
    const timer = setTimeout(() => setWaiting("silent"), SCOPE_ANSWER_WAIT_MS);
    return () => clearTimeout(timer);
  }, [waiting]);

  const asked = offered
    .filter((drive) => drive.id === home || picked.has(drive.id))
    .map((drive) => drive.id);
  const unchanged = asked.length === echoed.size && asked.every((id) => echoed.has(id));

  const onOpenChange = (next: boolean) => {
    if (next) {
      setPicked(echoed);
      setError(null);
    }
    setOpen(next);
  };

  const apply = () => {
    setSending(true);
    setError(null);
    const before = echoedKey;
    agentScopeSet(accountId, roomId, asked)
      .then(() => {
        setOpen(false);
        if (echoedNow.current === before) setWaiting("asked");
      })
      .catch((raw: unknown) => setError(syncErrorMessage(raw, "keeper could not ask for that.")))
      .finally(() => setSending(false));
  };

  return (
    <>
      <Popover open={open} onOpenChange={onOpenChange}>
        <IconHint label={SCOPE_EDIT_LABEL}>
          <PopoverTrigger asChild>
            <Button type="button" variant="ghost" size="icon-xs" aria-label={SCOPE_EDIT_LABEL}>
              <SlidersHorizontal aria-hidden="true" />
            </Button>
          </PopoverTrigger>
        </IconHint>
        <PopoverContent align="start" className="w-64 gap-3">
          <fieldset className="flex flex-col gap-2">
            <legend className="mb-1 font-medium text-sm">Drives in scope</legend>
            {offered.map((drive) => {
              const id = `dock-scope-${drive.id}`;
              const isHome = drive.id === home;
              return (
                <div key={drive.id} className="flex items-center gap-2">
                  <Checkbox
                    id={id}
                    checked={isHome || picked.has(drive.id)}
                    disabled={isHome}
                    onCheckedChange={(checked) =>
                      setPicked((was) => {
                        const next = new Set(was);
                        if (checked === true) next.add(drive.id);
                        else next.delete(drive.id);
                        return next;
                      })
                    }
                  />
                  <Label htmlFor={id} className="font-normal">
                    {drive.title}
                    {isHome && (
                      <span className="text-muted-foreground"> — home, always in scope</span>
                    )}
                  </Label>
                </div>
              );
            })}
          </fieldset>
          <Button type="button" size="sm" disabled={sending || unchanged} onClick={apply}>
            Ask for these drives
          </Button>
          {error !== null && (
            <p role="alert" className="text-destructive text-xs">
              {error}
            </p>
          )}
        </PopoverContent>
      </Popover>
      {waiting !== null && (
        <span role="status" className="text-muted-foreground text-xs">
          {waiting === "asked"
            ? `Asked ${agentName}; waiting for the answer.`
            : `No answer from ${agentName} yet.`}
        </span>
      )}
    </>
  );
}
