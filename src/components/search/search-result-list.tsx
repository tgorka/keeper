/**
 * Grouped search-result list (Story 5.4, FR-34, FR-24).
 *
 * Renders {@link SearchHitVm}[] grouped by Chat, keyed `(accountId, roomId)`. Each
 * group header resolves its Chat display name + account hue from the merged room
 * store (falling back to the raw room id when the hit's room is outside the merged
 * window — never a crash), and disambiguates cross-account identity with the
 * account hue dot + account `userId` (FR-24). Inside each hit body, occurrences of
 * the query terms are wrapped in the `search-highlight` background tint (background
 * only — never borders or text color). Pure render: it holds no state and calls no
 * IPC; activation is delegated to `onActivate`.
 */
import { Fragment, useMemo } from "react";
import { accountHueVar } from "@/lib/account-hue";
import type { AccountVm, InboxRoomVm, SearchHitVm } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

/** A single Chat group: its identity, resolved label/hue, and its ordered hits. */
interface HitGroup {
  accountId: string;
  roomId: string;
  /** The resolved Chat display name, or the raw room id when outside the window. */
  displayName: string;
  /** The account's hue index (0–7), or `null` when the account is unknown. */
  hueIndex: number | null;
  /** The account's Matrix user id for the meta line, or `null` when unknown. */
  userId: string | null;
  hits: SearchHitVm[];
}

/**
 * Group hits by `(accountId, roomId)` preserving first-seen order, resolving each
 * group's label/hue/userId from the merged stores. A group whose room is outside
 * the merged window falls back to the raw room id and a `null` hue/userId.
 */
export function groupHits(
  hits: SearchHitVm[],
  rooms: InboxRoomVm[],
  accounts: AccountVm[],
): HitGroup[] {
  const order: string[] = [];
  const byKey = new Map<string, HitGroup>();
  for (const hit of hits) {
    const key = `${hit.accountId}|${hit.roomId}`;
    let group = byKey.get(key);
    if (group === undefined) {
      const room = rooms.find((r) => r.accountId === hit.accountId && r.roomId === hit.roomId);
      const account = accounts.find((a) => a.accountId === hit.accountId);
      group = {
        accountId: hit.accountId,
        roomId: hit.roomId,
        displayName: room?.displayName ?? hit.roomId,
        hueIndex: room?.hueIndex ?? account?.hueIndex ?? null,
        userId: account?.userId ?? null,
        hits: [],
      };
      byKey.set(key, group);
      order.push(key);
    }
    group.hits.push(hit);
  }
  return order.map((key) => {
    const group = byKey.get(key);
    if (group === undefined) {
      // Unreachable: every key in `order` was inserted into `byKey`.
      throw new Error("search group vanished");
    }
    return group;
  });
}

/**
 * Split `body` into alternating plain / highlighted segments for the query terms.
 * Case-insensitive; terms are the whitespace-split query tokens (deduped). Returns
 * segments so the caller wraps only the matched runs in the tint — the frontend
 * never re-implements the engine's match, this is a display affordance only.
 */
export function highlightSegments(
  body: string,
  query: string,
): Array<{ key: string; text: string; match: boolean }> {
  const terms = [...new Set(query.trim().split(/\s+/).filter(Boolean))];
  if (terms.length === 0) {
    return [{ key: "0", text: body, match: false }];
  }
  // Build a single case-insensitive alternation, escaping regex metachars.
  const escaped = terms.map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  const re = new RegExp(`(${escaped.join("|")})`, "gi");
  const segments: Array<{ key: string; text: string; match: boolean }> = [];
  let last = 0;
  for (const m of body.matchAll(re)) {
    const idx = m.index ?? 0;
    if (idx > last) {
      segments.push({ key: `p${last}`, text: body.slice(last, idx), match: false });
    }
    segments.push({ key: `m${idx}`, text: m[0], match: true });
    last = idx + m[0].length;
  }
  if (last < body.length) {
    segments.push({ key: `p${last}`, text: body.slice(last), match: false });
  }
  return segments.length === 0 ? [{ key: "0", text: body, match: false }] : segments;
}

/** A single hit body with the query terms tinted. */
function HighlightedBody({ body, query }: { body: string; query: string }) {
  const segments = useMemo(() => highlightSegments(body, query), [body, query]);
  return (
    <>
      {segments.map((seg) =>
        seg.match ? (
          <mark
            key={seg.key}
            className="rounded-[2px] bg-search-highlight text-search-highlight-foreground"
          >
            {seg.text}
          </mark>
        ) : (
          <Fragment key={seg.key}>{seg.text}</Fragment>
        ),
      )}
    </>
  );
}

export interface SearchResultListProps {
  hits: SearchHitVm[];
  rooms: InboxRoomVm[];
  accounts: AccountVm[];
  query: string;
  /** The flat index of the row that is keyboard-focused, or `null`. */
  activeIndex: number | null;
  /** Activate (deep-link to) the hit at the given flat index. */
  onActivate: (hit: SearchHitVm) => void;
}

/**
 * The grouped result list. Rows are also indexed flat (across groups, in render
 * order) so arrow-key navigation in the overlay can address them by a single
 * index; `activeIndex` marks the focused row and `onActivate` fires the deep-link.
 */
export function SearchResultList({
  hits,
  rooms,
  accounts,
  query,
  activeIndex,
  onActivate,
}: SearchResultListProps) {
  const groups = useMemo(() => groupHits(hits, rooms, accounts), [hits, rooms, accounts]);
  // Flat index counter shared across groups so arrow-nav addresses a single list.
  let flat = -1;
  return (
    <div className="flex flex-col gap-4" role="listbox" aria-label="Search results">
      {groups.map((group) => (
        <div key={`${group.accountId}|${group.roomId}`} className="flex flex-col gap-1">
          <div className="flex items-center gap-2 px-1 text-xs text-muted-foreground">
            {group.hueIndex !== null && (
              <span
                aria-hidden
                className="size-2 shrink-0 rounded-full"
                style={{ backgroundColor: accountHueVar(group.hueIndex) }}
              />
            )}
            <span className="truncate font-medium text-foreground">{group.displayName}</span>
            {group.userId !== null && (
              <span className="truncate" title={group.userId}>
                {group.userId}
              </span>
            )}
          </div>
          {group.hits.map((hit) => {
            flat += 1;
            const index = flat;
            const active = index === activeIndex;
            return (
              <button
                type="button"
                key={hit.eventId}
                data-result-index={index}
                aria-selected={active}
                role="option"
                onClick={() => onActivate(hit)}
                className={cn(
                  "flex flex-col items-start gap-0.5 rounded-md px-2 py-1.5 text-left text-sm",
                  "hover:bg-accent focus:outline-none",
                  active && "bg-accent",
                )}
              >
                <span className="w-full break-words text-foreground">
                  {hit.redacted ? (
                    <span className="italic text-muted-foreground">Message deleted</span>
                  ) : (
                    <HighlightedBody body={hit.body} query={query} />
                  )}
                </span>
                <span className="text-xs text-muted-foreground">
                  {hit.sender} · {new Date(hit.timestamp).toLocaleString()}
                </span>
              </button>
            );
          })}
        </div>
      ))}
    </div>
  );
}
