import { type ReactElement, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  type AgentSessionRowVm,
  agentSessionArchive,
  agentSessionRename,
  agentSessionUnarchive,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

export function SessionRowMenu({
  row,
  children,
}: {
  row: AgentSessionRowVm;
  children: ReactElement;
}) {
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(row.title);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [details, setDetails] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const finishEditing = () => {
    const item = input.current?.closest("li");
    setEditing(false);
    requestAnimationFrame(() => item?.querySelector("button")?.focus());
  };
  const run = async (action: () => Promise<void>, success?: string) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await action();
      if (success) setNotice(success);
    } catch (cause) {
      setError(syncErrorMessage(cause, "keeper could not complete this action."));
    } finally {
      setBusy(false);
    }
  };
  const copy = (value: string) =>
    run(async () => {
      if (!navigator.clipboard) throw new Error("Clipboard is unavailable.");
      await navigator.clipboard.writeText(value);
    }, "Copied.");
  return (
    <>
      <ContextMenu>
        <ContextMenuTrigger asChild>
          {editing ? (
            <form
              className="flex min-w-0 flex-wrap gap-2 border-b p-3"
              onSubmit={(event) => {
                event.preventDefault();
                void run(async () => {
                  await agentSessionRename(row, title);
                  finishEditing();
                });
              }}
            >
              <Input
                ref={input}
                aria-label="Session title"
                value={title}
                disabled={busy}
                className="min-w-0 w-full"
                onChange={(event) => setTitle(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Escape") {
                    event.preventDefault();
                    event.stopPropagation();
                    if (!busy) finishEditing();
                  }
                }}
              />
              <Button type="submit" size="sm" disabled={busy}>
                Save
              </Button>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                disabled={busy}
                onClick={finishEditing}
              >
                Cancel
              </Button>
            </form>
          ) : (
            children
          )}
        </ContextMenuTrigger>
        <ContextMenuContent
          className="w-52 [&_[role=menuitem]]:min-h-11"
          onCloseAutoFocus={(event) => {
            if (editing) {
              event.preventDefault();
              input.current?.focus();
              input.current?.select();
            }
          }}
        >
          <ContextMenuItem
            onSelect={() => {
              setTitle(row.title);
              setEditing(true);
            }}
          >
            Rename…
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => void copy(row.sessionId)}>Copy id</ContextMenuItem>
          <ContextMenuItem onSelect={() => void copy(row.displayName)}>Copy name</ContextMenuItem>
          <ContextMenuItem onSelect={() => setDetails(true)}>Details</ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem
            onSelect={() =>
              void run(() => (row.archived ? agentSessionUnarchive(row) : agentSessionArchive(row)))
            }
          >
            {row.archived ? "Unarchive" : "Archive"}
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
      {error && (
        <p role="alert" className="break-words p-3 text-destructive text-sm">
          {error}
        </p>
      )}
      {notice && (
        <p role="status" className="px-3 text-muted-foreground text-xs">
          {notice}
        </p>
      )}
      <Dialog open={details} onOpenChange={setDetails}>
        <DialogContent className="max-h-[85dvh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Session details</DialogTitle>
          </DialogHeader>
          <dl className="grid min-w-0 gap-2 break-words text-sm [overflow-wrap:anywhere]">
            <dt>Name</dt>
            <dd>{row.displayName}</dd>
            <dt>Handle</dt>
            <dd>{row.handle}</dd>
            <dt>Session id</dt>
            <dd>{row.sessionId}</dd>
            <dt>Agent</dt>
            <dd>{row.agentName}</dd>
            <dt>Database</dt>
            <dd>{row.database}</dd>
            <dt>Instances</dt>
            <dd>{row.instances.join(", ") || "None reported"}</dd>
            <dt>Trust</dt>
            <dd>{row.trust}</dd>
            <dt>Created</dt>
            <dd>{new Date(row.createdAtMs).toLocaleString()}</dd>
          </dl>
        </DialogContent>
      </Dialog>
    </>
  );
}
