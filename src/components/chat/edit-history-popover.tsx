/**
 * Edit-history popover fed by the Local Archive (FR-11, Story 5.2).
 *
 * Wraps the "Edited" caption on an edited message as a clickable {@link Popover}
 * trigger. On open it fetches the message's version chain via `getEditHistory`
 * (which reads `archive.db` — never a fresh homeserver fetch) and lists the PRIOR
 * versions newest→oldest with timestamps. The current (newest) version is not
 * repeated (it is already the bubble body). Keyboard-operable: the underlying
 * radix Popover handles Esc-to-close and focus return.
 */

import type { ReactNode } from "react";
import { useCallback, useRef, useState } from "react";
import {
  Popover,
  PopoverContent,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover";
import { formatMessageTime } from "@/lib/format-time";
import type { EditVersionVm } from "@/lib/ipc/client";
import { getEditHistory } from "@/lib/ipc/client";

interface EditHistoryPopoverProps {
  /** The account whose archive holds this message's history. */
  accountId: string;
  /** The room the message belongs to. */
  roomId: string;
  /** The message's opaque render `key` (its `unique_id`). */
  messageKey: string;
  /** The trigger content — the "Edited" caption rendered as a button. */
  children: ReactNode;
}

/** A fetch is one of these states while the popover is open. */
type FetchState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "loaded"; versions: EditVersionVm[] }
  | { status: "error" };

export function EditHistoryPopover({
  accountId,
  roomId,
  messageKey,
  children,
}: EditHistoryPopoverProps) {
  const [state, setState] = useState<FetchState>({ status: "idle" });
  // Monotonic token so a fetch from a previous open cannot overwrite the state
  // after the popover was closed or reopened (stale-resolution guard).
  const requestId = useRef(0);

  const onOpenChange = useCallback(
    (open: boolean) => {
      if (!open) {
        // Invalidate any in-flight fetch and reset so the next open re-fetches
        // (history may have grown).
        requestId.current += 1;
        setState({ status: "idle" });
        return;
      }
      requestId.current += 1;
      const id = requestId.current;
      setState({ status: "loading" });
      getEditHistory(accountId, roomId, messageKey)
        .then((versions) => {
          if (id === requestId.current) setState({ status: "loaded", versions });
        })
        .catch(() => {
          if (id === requestId.current) setState({ status: "error" });
        });
    },
    [accountId, roomId, messageKey],
  );

  return (
    <Popover onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent className="w-72">
        <PopoverHeader>
          <PopoverTitle>Edit history</PopoverTitle>
        </PopoverHeader>
        <EditHistoryBody state={state} />
      </PopoverContent>
    </Popover>
  );
}

/** The popover body: loading, error, empty, or the list of prior versions. */
function EditHistoryBody({ state }: { state: FetchState }) {
  if (state.status === "loading" || state.status === "idle") {
    return <p className="text-muted-foreground text-sm">Loading…</p>;
  }
  if (state.status === "error") {
    // Distinct from the empty state: a real read failure is not the same as
    // "this message has no prior versions".
    return <p className="text-muted-foreground text-sm">Couldn't load history.</p>;
  }
  // Prior versions only (drop the current/newest one — it is the bubble body),
  // listed newest→oldest.
  const prior = state.versions.filter((v) => !v.isCurrent).reverse();
  if (prior.length === 0) {
    return <p className="text-muted-foreground text-sm">No local history.</p>;
  }
  return (
    <ol className="mt-1 flex max-h-64 flex-col gap-2 overflow-y-auto">
      {prior.map((version, index) => {
        const iso = safeIso(version.timestamp);
        return (
          <li
            // biome-ignore lint/suspicious/noArrayIndexKey: this list is rendered wholesale per open and never reordered; the index keeps keys unique when two versions share a timestamp and body.
            key={`${index}-${version.timestamp}`}
            className="flex flex-col gap-0.5 border-border border-l-2 pl-2"
          >
            <time dateTime={iso} className="text-muted-foreground text-xs">
              {formatMessageTime(version.timestamp)}
            </time>
            <span className="whitespace-pre-wrap text-sm [overflow-wrap:anywhere]">
              {version.body}
            </span>
          </li>
        );
      })}
    </ol>
  );
}

/**
 * Format a ms-epoch timestamp as an ISO string for the `<time dateTime>`
 * attribute, or `undefined` when it is out of the representable Date range —
 * `new Date(ms).toISOString()` throws a RangeError on such values, which would
 * crash the popover render.
 */
function safeIso(ms: number): string | undefined {
  if (!Number.isFinite(ms)) return undefined;
  const date = new Date(ms);
  return Number.isNaN(date.getTime()) ? undefined : date.toISOString();
}
