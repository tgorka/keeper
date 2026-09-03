/**
 * The conversation list, which is also the archive (Epic 61, Story 61.6,
 * FR-381, FR-382).
 *
 * The shape is the recordings archive's, deliberately and not by coincidence:
 * search first and above the fold, debounced with a stale-response guard, a
 * count line that reads the backend's own total rather than the number of rows
 * it just drew, three scope chips that are simultaneously the control and the
 * state, and two empty states in two different sentences. `recordings-pane.tsx`
 * argues each of those in its own header and UX-DR92 makes it binding: "a user
 * who knows notes already knows sessions … divergence in look or behaviour is a
 * defect, not a choice".
 *
 * **Ordering is Rust's and is never re-derived here.** The rows arrive in
 * `latest_activity DESC, id DESC`, where the activity is the newest message or
 * the conversation's own last change — whichever is later — so a conversation
 * with nothing in it still has an activity and does not sink to the bottom as
 * if nothing had ever happened to it. Sorting here would be a second order for
 * one list, which is how a row moves between two reads for no reason a reader
 * can see.
 *
 * **Continue is not a verb of its own.** Opening a conversation replays it from
 * keeper's store, which is the whole of FR-382: the same click on a live row and
 * on an archived one, with the endpoint up or down, because nothing is fetched
 * from the far side to read what was already said. Where a conversation holds a
 * remote session id, the open row says so in one sentence — the remote may have
 * compressed its own copy into a renamed successor, and keeper's record is
 * unaffected by that because keeper's record is the record (AD-154).
 *
 * **Archive is a column and delete is a transaction.** Archiving needs no
 * confirmation: it is reversible with the same control, and a dialog in front of
 * a reversible act trains people to dismiss dialogs. Delete gets one, and it
 * names the conversation and what goes with it — the chain-of-custody rule the
 * sessions board's own copy follows (`session-actions.tsx:52-66`).
 *
 * **It is a column, not a band (Story 61.14).** The first cut stacked this
 * above the transcript, where the label, New, the search field, its sentence,
 * the three scope chips and the rows were a permanent 241px, and the transcript
 * took what was left. It is now the body of a surface column beside the
 * transcript — the Tasks pane's Level 1 — so it folds to a rail that says how
 * many conversations it holds, and inside it the head stays put while the rows
 * scroll. The column's own title band names it; the `<ul>` keeps the noun.
 */
import { Archive, ArchiveRestore, MoreHorizontal, Pencil, Trash2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { InputGroup, InputGroupInput } from "@/components/ui/input-group";
import { ScrollArea } from "@/components/ui/scroll-area";
import { type CountNoun, countLabel } from "@/lib/count-label";
import { formatDraftAge } from "@/lib/format-time";
import type { BotSessionRowVm, BotSessionScope, BotSessionVm } from "@/lib/ipc/client";
import {
  botsSessionArchive,
  botsSessionDelete,
  botsSessionRename,
  botsSessionsSearch,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The list's accessible name. */
export const BOT_SESSION_LIST_LABEL = "Conversations";

/** The new-conversation verb. */
export const BOT_SESSION_NEW_LABEL = "New conversation";

/** The find field, whose placeholder and accessible name are the same words. */
export const BOT_SESSION_SEARCH_LABEL = "Search conversations";

/**
 * What the field says it searches. It says both halves because the second one
 * is the surprising one: the words you remember are usually in something you
 * asked, not in a name keeper minted from your first line.
 */
export const BOT_SESSION_SEARCH_POSTURE = "Searching titles and everything said";

/** The three scope positions, in the sessions board's own order and words. */
export const BOT_SESSION_SCOPE_CHOICES: readonly { value: BotSessionScope; label: string }[] = [
  { value: "live", label: "Active" },
  { value: "all", label: "All" },
  { value: "archived", label: "Archived" },
];

/**
 * What the list says with nothing in it — distinct from the pane's own empty
 * state, which is about having nothing to ask rather than nothing asked.
 */
export const BOT_SESSION_LIST_EMPTY =
  "No conversations yet. Ask a model something and it lands here.";

/** The other empty list: the same absence, the opposite fact. */
export const BOT_SESSION_NO_MATCH = "No conversations match this filter.";

/** What a failed read says when Rust gave no sentence of its own. */
export const BOT_SESSION_READ_FAILED = "keeper couldn't read your conversations.";

/** What a failed write says when Rust gave no sentence of its own. */
export const BOT_SESSION_WRITE_FAILED = "keeper couldn't change that conversation.";

/** The row's overflow menu, and its verbs. */
export const BOT_SESSION_ACTIONS_LABEL = "Conversation actions";
export const BOT_SESSION_RENAME_LABEL = "Rename";
export const BOT_SESSION_RENAME_FIELD_LABEL = "New conversation name";
export const BOT_SESSION_RENAME_CONFIRM = "Save name";
export const BOT_SESSION_RENAME_CANCEL = "Cancel";
export const BOT_SESSION_ARCHIVE_LABEL = "Archive";
export const BOT_SESSION_UNARCHIVE_LABEL = "Unarchive";
export const BOT_SESSION_DELETE_LABEL = "Delete…";

/** The marker an archived row carries, since a filter may be showing both. */
export const BOT_SESSION_ARCHIVED_MARK = "Archived";

/**
 * The delete dialog's words: which conversation, what goes with it, what does
 * not, and that it does not come back.
 *
 * Named rather than counted-and-anonymous, because the reader is about to lose
 * one specific thing and "this conversation" is what a dialog says when it has
 * not checked which one it is aimed at.
 */
export const BOT_SESSION_DELETE_TITLE = (title: string) => `Delete "${title}"?`;
export const BOT_SESSION_DELETE_BODY = (messageCount: number) =>
  `The conversation and its ${countLabel(messageCount, MESSAGES)} are removed from keeper's own store on this Mac, in one step, and cannot be brought back. Nothing on your drive changes, and the model is not told.`;
export const BOT_SESSION_DELETE_CONFIRM = "Delete conversation";
export const BOT_SESSION_DELETE_CANCEL = "Keep it";

/**
 * The remote-session sentence, shown on an open conversation that holds one
 * (Story 61.6, §2.10).
 *
 * The id is worth showing because it is the handle a person needs to ask the
 * far side about their own data. The second half is worth saying because the
 * far side may have replaced it: a gateway that compresses a session mints a
 * renamed successor, so the id keeper holds can name something that no longer
 * exists — and keeper's replay is unaffected either way, because keeper's store
 * is the record.
 */
export const BOT_SESSION_REMOTE_NOTE = (remoteSessionId: string) =>
  `The other side calls this session ${remoteSessionId}. keeper replays from its own store, and the other side may have compressed its copy into a renamed successor.`;

/** The list's counting noun. Local, because no other surface counts these. */
const CONVERSATIONS: CountNoun = { one: "conversation", many: "conversations" };

/** The delete dialog's noun, for the same reason. */
const MESSAGES: CountNoun = { one: "message", many: "messages" };

/**
 * How long after the last keystroke the query goes out.
 *
 * `recordings-pane.tsx:67`'s 200 ms, and the search panel's, and the notes
 * filter's. One number for the same gesture everywhere.
 */
const DEBOUNCE_MS = 200;

export function BotSessionList({
  sessions,
  openId,
  onOpen,
  onNew,
  onChanged,
  onClosed,
}: {
  /**
   * The pane's own conversation mirror.
   *
   * Read as a **revision signal** and not as the rows: this component queries
   * its own bounded, searched page, and the pane re-reads the store's list
   * after every send. A new array identity therefore means "something happened
   * to the conversations" and is exactly the moment this page is stale.
   */
  sessions: BotSessionVm[];
  /** The conversation on screen, or `null` for a fresh one. */
  openId: string | null;
  onOpen: (sessionId: string) => void;
  onNew: () => void;
  /** A write landed here; the pane should re-read what it holds. */
  onChanged: () => void;
  /** The conversation on screen no longer exists; close it. */
  onClosed: () => void;
}) {
  const [text, setText] = useState("");
  const [scope, setScope] = useState<BotSessionScope>("live");
  const [rows, setRows] = useState<BotSessionRowVm[] | null>(null);
  const [total, setTotal] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<{ id: string; draft: string } | null>(null);
  const [deleting, setDeleting] = useState<BotSessionRowVm | null>(null);
  /** Bumped by every write here, so the query re-runs on our own changes too. */
  const [revision, setRevision] = useState(0);
  /**
   * A monotonic read token, the recordings archive's stale guard (`:148`):
   * a slow first query landing after a fast second one must not restore the
   * older answer, and both the fulfilled and the rejected path check it.
   */
  const readToken = useRef(0);

  // biome-ignore lint/correctness/useExhaustiveDependencies: `revision` and `sessions` are re-run triggers, not reads — a write here bumps the first, and the pane handing down a fresh mirror after a send is the second; both mean this page is stale.
  useEffect(() => {
    const timer = window.setTimeout(() => {
      readToken.current += 1;
      const mine = readToken.current;
      // `limit: 0` takes the store's own page size rather than naming a second
      // one here — the ceiling is Rust's, and `total` says what it declined.
      void botsSessionsSearch({ text, scope, limit: 0 }).then(
        (page) => {
          if (mine !== readToken.current) {
            return;
          }
          setRows(page.rows);
          setTotal(page.total);
          setError(null);
        },
        (raw: unknown) => {
          if (mine !== readToken.current) {
            return;
          }
          setError(syncErrorMessage(raw, BOT_SESSION_READ_FAILED));
        },
      );
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [text, scope, revision, sessions]);

  /** Re-read this page, and tell the pane to re-read what it holds. */
  const refresh = useCallback(() => {
    setRevision((n) => n + 1);
    onChanged();
  }, [onChanged]);

  const rename = useCallback(
    (sessionId: string, draft: string) => {
      setError(null);
      void botsSessionRename(sessionId, draft).then(
        () => {
          setRenaming(null);
          refresh();
        },
        (raw: unknown) => setError(syncErrorMessage(raw, BOT_SESSION_WRITE_FAILED)),
      );
    },
    [refresh],
  );

  const archive = useCallback(
    (sessionId: string, archived: boolean) => {
      setError(null);
      void botsSessionArchive(sessionId, archived).then(refresh, (raw: unknown) =>
        setError(syncErrorMessage(raw, BOT_SESSION_WRITE_FAILED)),
      );
    },
    [refresh],
  );

  const remove = useCallback(
    (sessionId: string) => {
      setError(null);
      void botsSessionDelete(sessionId).then(
        () => {
          setDeleting(null);
          // The conversation on screen may be the one that just went. Closing
          // it before the re-read is what stops the pane rendering rows whose
          // conversation no longer exists.
          if (sessionId === openId) {
            onClosed();
          }
          refresh();
        },
        (raw: unknown) => setError(syncErrorMessage(raw, BOT_SESSION_WRITE_FAILED)),
      );
    },
    [openId, onClosed, refresh],
  );

  // A filter is on when text was typed or the scope was moved off the live
  // list; that is what decides which of the two empty sentences is honest.
  const filtered = text !== "" || scope !== "live";
  const shown = rows ?? [];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 flex-col gap-2 px-3 py-3">
        <Button type="button" variant="outline" size="sm" onClick={onNew}>
          {BOT_SESSION_NEW_LABEL}
        </Button>

        {/* Search above the fold, the browse-surface rule. */}
        <InputGroup>
          <InputGroupInput
            placeholder={BOT_SESSION_SEARCH_LABEL}
            aria-label={BOT_SESSION_SEARCH_LABEL}
            value={text}
            onChange={(e) => setText(e.target.value)}
          />
        </InputGroup>
        <p className="text-muted-foreground text-xs">{BOT_SESSION_SEARCH_POSTURE}</p>

        {/* Wraps rather than clips: the column's width is the user's business. */}
        <div className="flex flex-wrap items-center gap-1">
          {BOT_SESSION_SCOPE_CHOICES.map((choice) => (
            <Button
              key={choice.value}
              type="button"
              size="sm"
              variant={scope === choice.value ? "secondary" : "ghost"}
              aria-pressed={scope === choice.value}
              onClick={() => setScope(choice.value)}
            >
              {choice.label}
            </Button>
          ))}
          <span className="min-w-0 flex-1" />
          {/* The backend's total, never `shown.length` — the page is bounded and
              the two numbers differ by the cap. */}
          <span className="figures text-muted-foreground text-xs">
            {countLabel(shown.length, CONVERSATIONS, { of: total })}
          </span>
        </div>

        {error !== null && (
          <p role="alert" className="text-destructive text-xs">
            {error}
          </p>
        )}
      </div>

      {/* The rows scroll under a head that does not: the search field and the
          scope chips are the way to any row, so they may not scroll away with
          the rows they filter. */}
      <ScrollArea fitWidth className="min-h-0 flex-1">
        {rows !== null && shown.length === 0 ? (
          <p className="px-3 pb-3 text-muted-foreground text-xs">
            {filtered ? BOT_SESSION_NO_MATCH : BOT_SESSION_LIST_EMPTY}
          </p>
        ) : (
          <ul aria-label={BOT_SESSION_LIST_LABEL} className="flex flex-col gap-1 px-3 pb-3">
            {shown.map((row) => (
              <li key={row.session.id} className="flex flex-col gap-1">
                {renaming !== null && renaming.id === row.session.id ? (
                  <div className="flex items-center gap-1">
                    <Input
                      className="h-8"
                      aria-label={BOT_SESSION_RENAME_FIELD_LABEL}
                      value={renaming.draft}
                      // Autofocus because the row became a field: the gesture
                      // that asked for the rename is the one that must land the
                      // next keystroke. `space-row-menu.tsx:469` and the
                      // templates room's rename field both do this.
                      autoFocus
                      onChange={(e) => setRenaming({ id: row.session.id, draft: e.target.value })}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") {
                          e.preventDefault();
                          rename(row.session.id, renaming.draft);
                          return;
                        }
                        if (e.key === "Escape") {
                          e.preventDefault();
                          setRenaming(null);
                        }
                      }}
                    />
                    <Button
                      type="button"
                      size="sm"
                      variant="secondary"
                      onClick={() => rename(row.session.id, renaming.draft)}
                    >
                      {BOT_SESSION_RENAME_CONFIRM}
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      onClick={() => setRenaming(null)}
                    >
                      {BOT_SESSION_RENAME_CANCEL}
                    </Button>
                  </div>
                ) : (
                  <div className="flex items-center gap-1">
                    {/* The open button and the menu are siblings: a menu button
                      nested in a button is not HTML (`session-row.tsx:46-48`). */}
                    <Button
                      type="button"
                      size="sm"
                      className="min-w-0 flex-1 justify-start"
                      variant={row.session.id === openId ? "secondary" : "ghost"}
                      aria-current={row.session.id === openId ? "true" : undefined}
                      onClick={() => onOpen(row.session.id)}
                    >
                      <span className="min-w-0 flex-1 truncate text-left">{row.session.title}</span>
                      {row.session.archived && (
                        <span className="shrink-0 text-muted-foreground text-xs">
                          {BOT_SESSION_ARCHIVED_MARK}
                        </span>
                      )}
                      <span className="figures shrink-0 text-muted-foreground text-xs">
                        {formatDraftAge(row.latestActivityMs)}
                      </span>
                      <span className="figures shrink-0 text-muted-foreground text-xs">
                        {countLabel(row.messageCount, MESSAGES)}
                      </span>
                    </Button>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button
                          type="button"
                          size="sm"
                          variant="ghost"
                          aria-label={`${BOT_SESSION_ACTIONS_LABEL} ${row.session.title}`}
                        >
                          <MoreHorizontal aria-hidden="true" className="size-4" />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuItem
                          onSelect={() =>
                            setRenaming({ id: row.session.id, draft: row.session.title })
                          }
                        >
                          <Pencil aria-hidden="true" className="size-4" />
                          {BOT_SESSION_RENAME_LABEL}
                        </DropdownMenuItem>
                        {row.session.archived ? (
                          <DropdownMenuItem onSelect={() => archive(row.session.id, false)}>
                            <ArchiveRestore aria-hidden="true" className="size-4" />
                            {BOT_SESSION_UNARCHIVE_LABEL}
                          </DropdownMenuItem>
                        ) : (
                          <DropdownMenuItem onSelect={() => archive(row.session.id, true)}>
                            <Archive aria-hidden="true" className="size-4" />
                            {BOT_SESSION_ARCHIVE_LABEL}
                          </DropdownMenuItem>
                        )}
                        <DropdownMenuSeparator />
                        <DropdownMenuItem variant="destructive" onSelect={() => setDeleting(row)}>
                          <Trash2 aria-hidden="true" className="size-4" />
                          {BOT_SESSION_DELETE_LABEL}
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                )}
                {/* Only on the conversation being read, and only when one is
                  held: a list of ids nobody asked about is noise, and a row
                  with no id has nothing honest to say here. */}
                {row.session.id === openId && row.session.remoteSessionId !== null && (
                  <p className="text-muted-foreground text-xs">
                    {BOT_SESSION_REMOTE_NOTE(row.session.remoteSessionId)}
                  </p>
                )}
              </li>
            ))}
          </ul>
        )}
      </ScrollArea>

      <AlertDialog open={deleting !== null} onOpenChange={(open) => !open && setDeleting(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {BOT_SESSION_DELETE_TITLE(deleting?.session.title ?? "")}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {BOT_SESSION_DELETE_BODY(deleting?.messageCount ?? 0)}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{BOT_SESSION_DELETE_CANCEL}</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                if (deleting !== null) {
                  remove(deleting.session.id);
                }
              }}
            >
              {BOT_SESSION_DELETE_CONFIRM}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
