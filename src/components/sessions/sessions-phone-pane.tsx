/**
 * The sessions board on a phone (Epic 98, story 98.2, UX-DR145, AD-413).
 *
 * # One thing at a time
 *
 * The desktop's Sessions view is a list beside a panel strip, and its detail is
 * a log, a file tree, spaces, references and a four-column board stacked in one
 * scrolling pane. A 390 pt column has room for one of those, and what a person
 * opens the board on a phone for is the cards: who works which, where it runs,
 * which one waits for them. So the surface is two stack levels in `PhoneShell`,
 * the Notes shape (Story 66.4): {@link SessionsPhoneList} is level 1 over the
 * Inbox — the zone's sessions — and {@link SessionsPhoneBoard} is level 2 over
 * it — one session's cards. The shell's own back button, edge-swipe and Escape
 * pop them.
 *
 * # The board, paged
 *
 * Four columns side by side are four ~90 px strips on a phone. UX-DR145 pages
 * them instead: one column per page, swiped sideways (native scroll snapping, so
 * the swipe is the platform's and a reduced-motion setting is honoured by it),
 * with the four names and their counts above as a row a finger can tap. A card
 * whose `status:` is none of the four gets a fifth page rather than vanishing —
 * the desktop's stray row, for the desktop's reason. The page a person is on is
 * decided when a scroll comes to rest (`scrollend`, or a short quiet after the
 * last `scroll` where a WebView has no such event), so a tap on a far tab does
 * not walk the current-tab mark through every page the slide passes; and the
 * shell keeps it beside the session, so the board → card → room → back loop
 * lands on the page it left from.
 *
 * # What a card does here
 *
 * It shows what the desktop card shows about the work — the title, the run
 * badge (`blocked` is how a card says an approval is waiting, Q3 (a)), the
 * assignee, *running on `<host>`* or why it waits — through the desktop's own
 * {@link BoardCardAgent}. It moves by *Move to…*, a native picker, never by a
 * drag: the gesture that pages the board is a horizontal swipe, and a card that
 * also answered one would fight it. The picker's first entry, *Move to…*, is a
 * real, choosable option that moves nothing: iOS's wheel cannot rest on a
 * disabled row, so a disabled placeholder would put it on the first column and
 * a *Done* tapped only to look would move the card. *Allow* is not offered — the
 * command is a desktop twin on the phone, and the card says where to allow it.
 * Nothing else writes: no create, archive or edit (UX-DR145; the verbs are
 * `unsupported` twins in the phone's build anyway).
 * A move Rust refuses — the folder cannot take it, "The card was not moved —
 * …" — is said in its own words and nothing is re-read; a move that landed
 * here but whose push did not (`SessionTaskMoveVm.unpushed`, R346) is said too,
 * and the board re-reads, because the card has moved on this phone.
 *
 * An agent's session carries its room (`SessionDetailVm.room`). Tapping a card
 * of that session opens the room, where 93.3's approval card is decided by the
 * same Matrix event a Mac sends (AD-395) — the phone never reads or writes a
 * session's `approvals/`. The room is opened on the account the Agents window
 * streams it for: that window is Rust's list of every agent room this phone's
 * accounts are in, so a room it does not hold is one no account here can open,
 * and the card says so instead of opening nothing — once the window has
 * arrived: before its first batch, nothing is known yet, and the card says
 * keeper is still reading the rooms rather than that no account is in this one.
 * The view stays Sessions, so popping the room lands back on the board.
 *
 * # The reads
 *
 * The list is the desktop's mirror (`useSessionsChanges`); the board reads
 * `sessions_detail` on open, after a move and on `keeper://sessions-changed` —
 * which on a phone fires after each sync pass rescans the zone, since nothing
 * watches a folder there (`sessions_root::phone_synced`).
 */
import { MessageSquare } from "lucide-react";
import { type Ref, useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  PHONE_BACK_TO_INBOX,
  PHONE_INBOX_TITLE,
  PhoneBackBar,
} from "@/components/layout/phone-header";
import { BoardCardAgent } from "@/components/notes/board-card-agent";
import {
  BOARD_COLUMNS,
  BOARD_EMPTY_COLUMN,
  BOARD_STRAY_HEADING,
  BOARD_STRAY_HINT,
  type BoardCard,
  columnOf,
} from "@/components/notes/task-board";
import { cardOf, SESSION_BOARD_MOVE_FAILED } from "@/components/sessions/session-board";
import { SessionRow } from "@/components/sessions/session-row";
import {
  SESSIONS_LIST_LABEL,
  SESSIONS_NO_ROOT_BODY,
  SESSIONS_NO_ROOT_TITLE,
  SESSIONS_PANE_TITLE,
} from "@/components/sessions/sessions-pane";
import { Button } from "@/components/ui/button";
import { useReducedMotion } from "@/hooks/use-reduced-motion";
import { useSessionsChanges } from "@/hooks/use-sessions-changes";
import { logFailure } from "@/lib/failure";
import type { InboxRoomVm, SessionDetailVm, SessionRowVm } from "@/lib/ipc/client";
import { listenSessionsChanged, sessionsDetail, sessionsTaskMove } from "@/lib/ipc/client";
import { agentRoomsStore } from "@/lib/stores/agent-rooms";
import { type RoomSelection, roomsStore } from "@/lib/stores/rooms";
import { useSessionsListStore } from "@/lib/stores/sessions-list";
import {
  refreshSessionsRoots,
  useActiveSessionsRoot,
  useSessionsRootsStore,
} from "@/lib/stores/sessions-roots";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/** The board level's back control: the level beneath it is the sessions list. */
export const SESSIONS_PHONE_BACK_TO_LIST = `Back to ${SESSIONS_PANE_TITLE}`;

/** The session's own way into its room, beside the board. */
export const SESSIONS_PHONE_OPEN_ROOM = "Open the session's room";

/** A card's way into its session's room, as its accessible name. */
export function sessionsPhoneOpenCardRoom(title: string): string {
  return `${title} — open the session's room`;
}

/** The per-card picker: what it says before a column is chosen. */
export const SESSIONS_PHONE_MOVE_TO = "Move to…";

/** The picker's accessible name. */
export function sessionsPhoneMoveName(title: string): string {
  return `${SESSIONS_PHONE_MOVE_TO} — ${title}`;
}

/** The pager's accessible name. */
export const SESSIONS_PHONE_COLUMNS_LABEL = "Columns";

/** A session with no task cards. Not the desktop's sentence: there is no Files above here. */
export const SESSIONS_PHONE_NO_TASKS =
  "No tasks in this session. A task is a markdown file tagged `task`; make one on your Mac or in Obsidian.";

/** The zone has no sessions at all. */
export const SESSIONS_PHONE_NO_SESSIONS = "No sessions in this folder yet.";

/** The session's room is not one any account on this phone streams. */
export const SESSIONS_PHONE_ROOM_ABSENT =
  "None of the accounts on this phone is in this session's room, so keeper cannot open it here.";

/** A card tapped before the Agents window's first batch: nothing is known yet. */
export const SESSIONS_PHONE_ROOMS_READING =
  "keeper is still reading the rooms on this phone. Try again in a moment.";

/**
 * How long the pager waits after its last `scroll` before deciding which page
 * it rests on, where the WebView sends no `scrollend`. Longer than a frame of a
 * smooth slide, shorter than a person's next look.
 */
const PAGE_SETTLE_MS = 150;

/** The board while its first read is out. */
export const SESSIONS_PHONE_READING = "Reading the board…";

/**
 * Where `roomId` opens on this phone: the Agents window's row for it, which
 * carries the account it was streamed for — or `null` when no account here
 * holds that room. First match wins, in Rust's recency order, as everywhere a
 * window row is looked up (`use-selected-room-vm.ts`).
 */
export function sessionRoomOnThisPhone(
  roomId: string,
  agentRooms: readonly InboxRoomVm[],
): RoomSelection | null {
  const row = agentRooms.find((room) => room.roomId === roomId);
  return row === undefined ? null : { accountId: row.accountId, roomId: row.roomId };
}

/**
 * Open `roomId` over the board, or say why not. The primary view stays
 * Sessions: a selected room outranks every view level, so the room covers the
 * board, and popping it lands on the board again rather than on a chat list.
 */
function openSessionRoom(roomId: string): string | null {
  const { rooms, total } = agentRoomsStore.getState();
  const target = sessionRoomOnThisPhone(roomId, rooms);
  if (target === null) {
    // `total === null` is "not streamed yet", never "absent".
    return total === null ? SESSIONS_PHONE_ROOMS_READING : SESSIONS_PHONE_ROOM_ABSENT;
  }
  roomsStore.getState().selectRoom(target);
  return null;
}

/**
 * Level 1: the zone's sessions. A row pushes its board.
 */
export function SessionsPhoneList({
  onBack,
  backRef,
  onOpen,
}: {
  /** Pop to the Inbox (the shell's `onBack`). */
  onBack: () => void;
  /** Forwarded to the back button so the shell can focus it on push (UX-DR28). */
  backRef?: Ref<HTMLButtonElement>;
  /** Push the board of this session of this root. */
  onOpen: (target: { rootId: string; sessionId: string }) => void;
}) {
  const roots = useSessionsRootsStore((s) => s.roots);
  const activeRoot = useActiveSessionsRoot();
  const setActiveRootId = useSessionsRootsStore((s) => s.setActiveRootId);
  const rows = useSessionsListStore((s) => s.rows);
  const error = useSessionsListStore((s) => s.error);
  // A failed roots read leaves `roots === null`: without its sentence the
  // list would be an empty page that says nothing.
  const rootsError = useSessionsRootsStore((s) => s.error);

  useEffect(() => {
    void refreshSessionsRoots();
  }, []);
  useSessionsChanges(activeRoot?.id ?? null);

  const open = (row: SessionRowVm) => {
    if (activeRoot !== null) {
      onOpen({ rootId: activeRoot.id, sessionId: row.id });
    }
  };

  return (
    <section
      aria-label={SESSIONS_PANE_TITLE}
      className="flex min-h-0 min-w-0 flex-1 flex-col bg-background"
    >
      <PhoneBackBar
        backLabel={PHONE_BACK_TO_INBOX}
        backTitle={PHONE_INBOX_TITLE}
        backRef={backRef}
        onBack={onBack}
      >
        <h1 className="min-w-0 flex-1 truncate font-heading text-title">{SESSIONS_PANE_TITLE}</h1>
      </PhoneBackBar>
      {/* Two zones or more: which one the list is. One is just the list. */}
      {roots !== null && roots.length > 1 && (
        <div className="flex shrink-0 gap-1 overflow-x-auto border-border border-b px-2 py-2">
          {roots.map((root) => (
            <Button
              key={root.id}
              type="button"
              variant={root.id === activeRoot?.id ? "secondary" : "ghost"}
              aria-pressed={root.id === activeRoot?.id}
              onClick={() => setActiveRootId(root.id)}
              className="min-h-11 shrink-0"
            >
              {root.name}
            </Button>
          ))}
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-y-auto px-2 py-2 pb-[calc(0.5rem+var(--safe-bottom))]">
        {[rootsError, error].map(
          (sentence, index) =>
            sentence !== null && (
              <p
                // biome-ignore lint/suspicious/noArrayIndexKey: two fixed slots, roots then list.
                key={index}
                role="alert"
                className="mb-2 rounded-md bg-destructive/10 p-3 text-destructive text-sm"
              >
                {sentence}
              </p>
            ),
        )}
        {roots !== null && roots.length === 0 ? (
          <div className="flex flex-col items-center gap-2 p-4 text-center">
            <p className="font-medium text-sm">{SESSIONS_NO_ROOT_TITLE}</p>
            <p className="text-muted-foreground text-sm">{SESSIONS_NO_ROOT_BODY}</p>
          </div>
        ) : rows !== null && rows.length === 0 ? (
          <p className="p-4 text-center text-muted-foreground text-sm">
            {SESSIONS_PHONE_NO_SESSIONS}
          </p>
        ) : (
          <ul aria-label={SESSIONS_LIST_LABEL} className="flex flex-col gap-2">
            {(rows ?? []).map((row) => (
              <li key={row.id}>
                <SessionRow row={row} onOpen={open} />
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}

/** One page of the board: a column, or the strays. */
interface BoardPage {
  key: string;
  label: string;
  cards: BoardCard[];
}

/**
 * Level 2: one session's cards, a column per page.
 */
export function SessionsPhoneBoard({
  rootId,
  sessionId,
  onBack,
  backRef,
  initialPage = 0,
  onPageChange,
}: {
  rootId: string;
  sessionId: string;
  /** Pop to the sessions list (the shell's `onBack`). */
  onBack: () => void;
  /** Forwarded to the back button so the shell can focus it on push (UX-DR28). */
  backRef?: Ref<HTMLButtonElement>;
  /** The page to open on: the one the person left when a card's room covered the board. */
  initialPage?: number;
  /** Told each page the person comes to rest on, so the shell can keep it. */
  onPageChange?: (page: number) => void;
}) {
  const [detail, setDetail] = useState<SessionDetailVm | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const [page, setPage] = useState(initialPage);
  const pager = useRef<HTMLDivElement>(null);
  const settleTimer = useRef<number | null>(null);
  const restored = useRef(false);
  // Read once, when the pager first mounts: later pages come from the person.
  const initialRef = useRef(initialPage);
  const reducedMotion = useReducedMotion();

  // Read on open, after a move, and on the zone's changed event — on a phone
  // that is the rescan after each sync pass.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `reload` is a deliberate re-run trigger, not a read — a move bumps it so the board re-reads without waiting for the zone's event.
  useEffect(() => {
    let live = true;
    const read = () => {
      sessionsDetail(rootId, sessionId).then(
        (vm) => {
          if (live) {
            setDetail(vm);
            setError(null);
          }
        },
        (e: unknown) => {
          if (live) {
            setError(syncErrorMessage(e, "keeper could not read this session."));
          }
        },
      );
    };
    read();
    let unlisten: (() => void) | null = null;
    void listenSessionsChanged((changedRootId) => {
      if (live && changedRootId === rootId) {
        read();
      }
    })
      .then((stop) => {
        if (live) {
          unlisten = stop;
        } else {
          stop();
        }
      })
      // No Tauri host — nothing will announce a change.
      .catch(logFailure("sessions.listenChanged"));
    return () => {
      live = false;
      unlisten?.();
    };
  }, [rootId, sessionId, reload]);

  useEffect(
    () => () => {
      if (settleTimer.current !== null) {
        window.clearTimeout(settleTimer.current);
      }
    },
    [],
  );

  const cards = (detail?.tasks ?? []).map(cardOf);
  const known = new Set(BOARD_COLUMNS.map((column) => column.status));
  const strays = cards.filter((card) => card.status === null || !known.has(card.status));
  const pages: BoardPage[] = [
    ...BOARD_COLUMNS.map((column) => ({
      key: column.status,
      label: column.label,
      cards: columnOf(cards, column.status),
    })),
    ...(strays.length > 0 ? [{ key: "stray", label: BOARD_STRAY_HEADING, cards: strays }] : []),
  ];
  const shown = Math.min(page, pages.length - 1);
  const hasPager = detail !== null && cards.length > 0;

  // The pager mounts once the first read is in: put it on the page the board
  // opened on, at once — the slide that matters is the level's, not this.
  useLayoutEffect(() => {
    const box = pager.current;
    if (!hasPager || restored.current || box === null) {
      return;
    }
    restored.current = true;
    if (box.clientWidth > 0) {
      box.scrollLeft = initialRef.current * box.clientWidth;
    }
  }, [hasPager]);
  const room = detail?.room ?? null;

  const move = async (key: string, status: string) => {
    setNotice(null);
    try {
      // To the end of the column it joins, as the desktop's column menu does:
      // the index Rust resolves against is a slot among that column's cards.
      const moved = await sessionsTaskMove(
        rootId,
        sessionId,
        key,
        status,
        columnOf(cards, status).length,
      );
      // The card moved here; a push that did not land says so rather than
      // pretending the Mac has it (R346): a paused folder, a deferred push.
      setNotice(moved.unpushed);
      setReload((n) => n + 1);
    } catch (refusal) {
      logFailure("sessions.taskMove")(refusal);
      // keeper's own sentence when it sent one (UX-DR43).
      setNotice(syncErrorMessage(refusal, SESSION_BOARD_MOVE_FAILED));
    }
  };

  const openRoom = () => {
    if (room !== null) {
      setNotice(openSessionRoom(room));
    }
  };

  const choose = (index: number) => {
    setPage(index);
    onPageChange?.(index);
  };

  const goTo = (index: number) => {
    choose(index);
    const box = pager.current;
    if (box !== null && typeof box.scrollTo === "function") {
      box.scrollTo({ left: index * box.clientWidth, behavior: reducedMotion ? "auto" : "smooth" });
    }
  };

  // Which page the pager rests on, read only once it rests: a smooth slide
  // from a tab passes every page between, and each would otherwise become
  // `aria-current` for a few frames — and re-render the board per frame.
  const settle = () => {
    if (settleTimer.current !== null) {
      window.clearTimeout(settleTimer.current);
      settleTimer.current = null;
    }
    const box = pager.current;
    if (box !== null && box.clientWidth > 0) {
      choose(Math.round(box.scrollLeft / box.clientWidth));
    }
  };

  return (
    <section
      aria-label={detail?.title ?? SESSIONS_PANE_TITLE}
      className="flex min-h-0 min-w-0 flex-1 flex-col bg-background"
    >
      <PhoneBackBar
        backLabel={SESSIONS_PHONE_BACK_TO_LIST}
        backTitle={SESSIONS_PANE_TITLE}
        backRef={backRef}
        onBack={onBack}
      >
        <h1 className="min-w-0 flex-1 truncate font-heading text-title">{detail?.title ?? ""}</h1>
      </PhoneBackBar>
      {room !== null && (
        <div className="shrink-0 border-border border-b px-2 py-2">
          <Button type="button" variant="outline" onClick={openRoom} className="min-h-11 w-full">
            <MessageSquare aria-hidden="true" />
            {SESSIONS_PHONE_OPEN_ROOM}
          </Button>
        </div>
      )}
      {(error !== null || notice !== null) && (
        <p
          role={error !== null ? "alert" : "status"}
          className="shrink-0 px-3 py-2 text-destructive text-xs"
        >
          {error ?? notice}
        </p>
      )}
      {detail === null ? (
        error === null && (
          <p className="p-4 text-center text-muted-foreground text-sm">{SESSIONS_PHONE_READING}</p>
        )
      ) : cards.length === 0 ? (
        <p className="p-4 text-center text-muted-foreground text-sm">{SESSIONS_PHONE_NO_TASKS}</p>
      ) : (
        <>
          <nav
            aria-label={SESSIONS_PHONE_COLUMNS_LABEL}
            className="flex shrink-0 gap-1 overflow-x-auto border-border border-b px-2 py-1.5"
          >
            {pages.map((entry, index) => (
              <button
                key={entry.key}
                type="button"
                aria-current={index === shown ? "true" : undefined}
                onClick={() => goTo(index)}
                className={cn(
                  "flex min-h-11 shrink-0 items-center gap-1.5 rounded-md px-2.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring",
                  index === shown
                    ? "bg-secondary font-medium text-secondary-foreground"
                    : "text-muted-foreground",
                )}
              >
                <span>{entry.label}</span>{" "}
                <span className="figures text-xs">{entry.cards.length}</span>
              </button>
            ))}
          </nav>
          <div
            ref={pager}
            onScroll={() => {
              if (settleTimer.current !== null) {
                window.clearTimeout(settleTimer.current);
              }
              settleTimer.current = window.setTimeout(settle, PAGE_SETTLE_MS);
            }}
            onScrollEnd={settle}
            className="flex min-h-0 min-w-0 flex-1 snap-x snap-mandatory overflow-x-auto overflow-y-hidden overscroll-x-contain"
          >
            {pages.map((entry) => (
              <section
                key={entry.key}
                aria-label={entry.label}
                data-board-page={entry.key}
                className="flex h-full w-full shrink-0 snap-start flex-col gap-2 overflow-y-auto px-2 py-2 pb-[calc(0.5rem+var(--safe-bottom))]"
              >
                {entry.key === "stray" && (
                  <p className="text-muted-foreground text-xs">{BOARD_STRAY_HINT}</p>
                )}
                <ul aria-label={entry.label} className="flex flex-col gap-2">
                  {entry.cards.length === 0 ? (
                    <li className="p-2 text-muted-foreground text-sm">{BOARD_EMPTY_COLUMN}</li>
                  ) : (
                    entry.cards.map((card) => (
                      <PhoneCard
                        key={card.key}
                        card={card}
                        onOpen={room !== null ? openRoom : undefined}
                        onMove={(status) => void move(card.key, status)}
                      />
                    ))
                  )}
                </ul>
              </section>
            ))}
          </div>
        </>
      )}
    </section>
  );
}

/**
 * One card: what is worked, by whom, where — and *Move to…*. A card of an
 * agent's session is a button into that session's room.
 */
function PhoneCard({
  card,
  onOpen,
  onMove,
}: {
  card: BoardCard;
  /** Open the session's room; absent for a session that has none. */
  onOpen?: () => void;
  onMove: (status: string) => void;
}) {
  return (
    <li
      data-card-key={card.key}
      className="flex flex-col gap-1.5 rounded-md border border-border bg-card px-3 py-2"
    >
      {onOpen !== undefined ? (
        <button
          type="button"
          onClick={onOpen}
          aria-label={sessionsPhoneOpenCardRoom(card.title)}
          className="min-h-11 text-left font-medium text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {card.title}
        </button>
      ) : (
        <p className="font-medium text-sm">{card.title}</p>
      )}
      {/* No `onAllow`: the command is the desktop's, so the block names where to allow. */}
      {card.agent != null && <BoardCardAgent agent={card.agent} title={card.title} />}
      {/* A native picker, as every sessions surface uses: on a phone it is the
          system's own sheet, and its first entry is the verb, so the closed
          control reads as a menu rather than as the card's current column. */}
      <select
        aria-label={sessionsPhoneMoveName(card.title)}
        value=""
        onChange={(event) => {
          const next = event.target.value;
          if (next !== "") {
            onMove(next);
          }
        }}
        className="h-11 w-full rounded-md border border-input bg-transparent px-2 text-muted-foreground text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <option value="">{SESSIONS_PHONE_MOVE_TO}</option>
        {BOARD_COLUMNS.filter((column) => column.status !== card.status).map((column) => (
          <option key={column.status} value={column.status}>
            {column.label}
          </option>
        ))}
      </select>
    </li>
  );
}
