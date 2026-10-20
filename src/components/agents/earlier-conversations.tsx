import { useEffect, useState } from "react";
import { MediaAttachment } from "@/components/chat/media-attachment";
import { Button } from "@/components/ui/button";
import { logFailure } from "@/lib/failure";
import {
  type InboxRoomVm,
  loadEarlierMessages,
  type PaginationStatusBatch,
  subscribeInbox,
  subscribeTimeline,
  type TimelineItemVm,
  unsubscribeInbox,
  unsubscribeTimeline,
} from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { syncErrorMessage } from "@/lib/stores/sync";
import { applyDiffOp } from "@/lib/stores/vector-diff";

/** Transitional Matrix history is explicitly read-only, never a second live agent path. */
export function EarlierConversations() {
  const [expanded, setExpanded] = useState(false);
  const [rows, setRows] = useState<InboxRoomVm[]>([]);
  const [chosen, setChosen] = useState<InboxRoomVm | null>(null);
  const [error, setError] = useState<string | null>(null);
  const accounts = useAccountsStore((s) => s.accounts.map((a) => a.accountId).join("\n"));
  useEffect(() => {
    if (!expanded || !accounts) return;
    let active = true;
    let id: number | null = null;
    const ignore = () => {};
    void subscribeInbox(ignore, ignore, ignore, ignore, ignore, ignore, (batch) => {
      if (active) setRows((old) => batch.ops.reduce<InboxRoomVm[]>(applyDiffOp, old));
    })
      .then((value) => {
        if (active) id = value;
        else void unsubscribeInbox(value).catch(logFailure("agents.earlier.close"));
      })
      .catch((raw: unknown) => {
        if (active) setError(syncErrorMessage(raw, "keeper could not read earlier conversations."));
      });
    return () => {
      active = false;
      if (id !== null) void unsubscribeInbox(id).catch(logFailure("agents.earlier.close"));
    };
  }, [expanded, accounts]);
  return (
    <div className="border-border border-t p-3">
      <Button
        variant="ghost"
        className="w-full justify-start"
        aria-expanded={expanded}
        onClick={() => setExpanded(!expanded)}
      >
        Earlier conversations
      </Button>
      {expanded && (
        <>
          <p className="my-2 text-muted-foreground text-xs">
            Earlier Matrix conversations are read-only.
          </p>
          {error && (
            <p role="alert" className="text-destructive text-sm">
              {error}
            </p>
          )}
          {rows.map((row) => (
            <Button
              key={JSON.stringify([row.accountId, row.roomId])}
              variant="ghost"
              className="w-full justify-start truncate"
              onClick={() => setChosen(row)}
            >
              {row.displayName}
            </Button>
          ))}
          {chosen && (
            <EarlierMessages
              key={JSON.stringify([chosen.accountId, chosen.roomId])}
              room={chosen}
            />
          )}
        </>
      )}
    </div>
  );
}

function EarlierMessages({ room }: { room: InboxRoomVm }) {
  const [items, setItems] = useState<TimelineItemVm[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [pagination, setPagination] = useState<PaginationStatusBatch | null>(null);
  const [paging, setPaging] = useState(false);
  useEffect(() => {
    let active = true;
    let id: number | null = null;
    void subscribeTimeline(room.accountId, room.roomId, (batch) => {
      if (active) {
        setItems((old) => batch.ops.reduce<TimelineItemVm[]>(applyDiffOp, old));
        if (batch.pagination) setPagination(batch.pagination);
      }
    })
      .then((value) => {
        if (active) id = value;
        else
          void unsubscribeTimeline(room.accountId, value).catch(logFailure("agents.earlier.close"));
      })
      .catch((raw: unknown) => {
        if (active) setError(syncErrorMessage(raw, "keeper could not read this conversation."));
      });
    return () => {
      active = false;
      if (id !== null)
        void unsubscribeTimeline(room.accountId, id).catch(logFailure("agents.earlier.close"));
    };
  }, [room.accountId, room.roomId]);
  return (
    <section
      aria-label={`Earlier conversation: ${room.displayName}`}
      className="mt-3 flex flex-col gap-3 text-sm"
    >
      <h3 className="font-medium">{room.displayName}</h3>
      {error && (
        <p role="alert" className="text-destructive">
          {error}
        </p>
      )}
      {pagination && !pagination.hitStart && (
        <Button
          variant="outline"
          disabled={paging || pagination.state === "paginating"}
          onClick={async () => {
            setPaging(true);
            setError(null);
            try {
              await loadEarlierMessages(room.accountId, room.roomId);
            } catch (raw) {
              setError(syncErrorMessage(raw, "keeper could not load earlier history."));
            } finally {
              setPaging(false);
            }
          }}
        >
          {pagination.state === "paginating" ? "Loading earlier history…" : "Load earlier history"}
        </Button>
      )}
      {items.map((item) =>
        item.kind === "message" ? (
          <div key={item.key}>
            <p className="font-medium text-xs">{item.senderDisplayName ?? item.sender}</p>
            <p className="whitespace-pre-wrap break-words [overflow-wrap:anywhere]">{item.body}</p>
            {item.media && <MediaAttachment media={item.media} messageKey={item.key} />}
          </div>
        ) : item.kind === "utd" ? (
          <p key={item.key} className="text-muted-foreground">
            This message could not be decrypted.
          </p>
        ) : item.kind === "redacted" ? (
          <p key={item.key} className="text-muted-foreground">
            Message removed.
          </p>
        ) : null,
      )}
    </section>
  );
}
