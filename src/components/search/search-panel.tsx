/**
 * The message-search body (Story 5.4 / Story 13.4, FR-34, UX-DR13).
 *
 * Extracted verbatim from the desktop `SearchOverlay`'s `DialogContent` body so
 * desktop (`SearchOverlay`, a thin `Dialog` wrapper) and the phone
 * `PhoneSearchSurface` render byte-identical message search over the *same*
 * behavior — the single source of the query field + filter chips, the 200ms
 * debounce into `searchArchive`, the out-of-order (stale) response guard, the
 * honest "Searching your local archive" + offline header, the grouped
 * `SearchResultList` with tinted matches, the `roomsStore.requestFocus`
 * deep-link, and the export/approval shortcuts. Results live only here — the
 * surface discards them on close (the Rust archive is the source of truth).
 *
 * The caller drives it with `active` (the surface's open flag — a rising edge
 * resets all state so results never leak across opens), the `scope`
 * (`"global"` | `"chat"`), an explicit `chatLock` (the locked Chat in `"chat"`
 * scope), and `onClose` (close the owning surface). No IPC or Matrix logic lives
 * here beyond the reused engine calls.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { SearchResultList } from "@/components/search/search-result-list";
import { Badge } from "@/components/ui/badge";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import type { IpcError, SearchHitVm } from "@/lib/ipc/client";
import { searchArchive } from "@/lib/ipc/client";
import { buildSearchFilter, type SearchUiFilter } from "@/lib/search-filter";
import { useAccountsStore } from "@/lib/stores/accounts";
import { exportStore } from "@/lib/stores/export";
import { useNetworksStore } from "@/lib/stores/networks";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore, useRoomsStore } from "@/lib/stores/rooms";
import type { SearchScope } from "@/lib/stores/search";
import { cn } from "@/lib/utils";

/** Debounce (ms) before a keystroke fires `searchArchive`. */
const DEBOUNCE_MS = 200;

/** Structural guard for the IpcError envelope surfaced on a search rejection. */
function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const v = value as Record<string, unknown>;
  return typeof v.code === "string" && typeof v.message === "string";
}

export interface SearchPanelProps {
  /**
   * Whether the owning surface is open. A rising edge (`false` → `true`) resets
   * all panel state so results never leak across opens and an in-chat scope
   * starts clean; while `false` the debounced search makes no call.
   */
  active: boolean;
  /** `"global"` searches everything; `"chat"` locks to `chatLock`. */
  scope: SearchScope;
  /**
   * The locked Chat in `"chat"` scope (shows a non-removable Chat chip and forces
   * the room/account filter). `null` in `"global"` scope.
   */
  chatLock: { accountId: string; roomId: string } | null;
  /** Close the owning surface (Escape/click deep-link/export both call this). */
  onClose: () => void;
  /**
   * Extra classes for the `<search>` root. The desktop `Dialog` leaves this unset
   * (content-height, capped by the results region); the full-screen phone surface
   * passes `flex-1 min-h-0` so the panel fills the viewport.
   */
  className?: string;
  /**
   * Extra classes for the results scroll region. Defaults to the desktop
   * `max-h-[50vh]` cap; the full-screen phone surface passes `max-h-none flex-1`
   * so results fill the screen instead of a half-viewport box.
   */
  resultsClassName?: string;
}

export function SearchPanel({
  active,
  scope,
  chatLock,
  onClose,
  className,
  resultsClassName,
}: SearchPanelProps) {
  const rooms = useRoomsStore((s) => s.rooms);
  const accounts = useAccountsStore((s) => s.accounts);
  const networks = useNetworksStore((s) => s.networks);

  const [query, setQuery] = useState("");
  const [chat, setChat] = useState<{ accountId: string; roomId: string } | null>(null);
  const [network, setNetwork] = useState<string | null>(null);
  const [accountId, setAccountId] = useState<string | null>(null);
  const [sender, setSender] = useState<string | null>(null);
  const [startDate, setStartDate] = useState<string | null>(null);
  const [endDate, setEndDate] = useState<string | null>(null);
  const [hits, setHits] = useState<SearchHitVm[]>([]);
  const [activeIndex, setActiveIndex] = useState<number | null>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const [hasSearched, setHasSearched] = useState(false);

  // Monotonic request sequence: a response is applied only if it is the newest
  // dispatched — an older (superseded) response is discarded (out-of-order guard).
  const seqRef = useRef(0);

  // The in-chat scope lock: forces the room/account scope and shows a locked Chat
  // chip. `null` for global scope. Provided by the owning surface (desktop derives
  // it from `searchStore.scope` + `roomsStore.selected`; the phone from its store).
  const effectiveChatLock = useMemo(() => (scope === "chat" ? chatLock : null), [scope, chatLock]);

  // Reset all surface state each time it opens, so results never leak across opens
  // (results are never held in a store) and the in-chat scope starts clean.
  useEffect(() => {
    if (!active) {
      return;
    }
    setQuery("");
    setChat(null);
    setNetwork(null);
    setAccountId(null);
    setSender(null);
    setStartDate(null);
    setEndDate(null);
    setHits([]);
    setActiveIndex(null);
    setError(null);
    setHasSearched(false);
    seqRef.current += 1;
  }, [active]);

  const uiFilter = useMemo<SearchUiFilter>(
    () => ({
      query,
      chat,
      network,
      accountId,
      sender,
      startDate,
      endDate,
      chatLock: effectiveChatLock,
    }),
    [query, chat, network, accountId, sender, startDate, endDate, effectiveChatLock],
  );

  // Debounced search. An empty query makes no call (and clears any prior results);
  // otherwise the newest keystroke wins via the sequence guard.
  useEffect(() => {
    if (!active) {
      return;
    }
    if (query.trim() === "") {
      setHits([]);
      setActiveIndex(null);
      setError(null);
      setHasSearched(false);
      return;
    }
    const handle = window.setTimeout(() => {
      seqRef.current += 1;
      const seq = seqRef.current;
      // Read the merged room list at call time (not via effect deps): the search
      // must re-run only when the query/filter selections change, never on every
      // streamed inbox batch (which replaces `rooms` with a fresh array).
      const filter = buildSearchFilter(uiFilter, roomsStore.getState().rooms);
      searchArchive(filter)
        .then((result) => {
          // Discard a superseded (out-of-order) response.
          if (seq !== seqRef.current) {
            return;
          }
          setHits(result);
          setActiveIndex(result.length > 0 ? 0 : null);
          setError(null);
          setHasSearched(true);
        })
        .catch((e: unknown) => {
          if (seq !== seqRef.current) {
            return;
          }
          setHits([]);
          setActiveIndex(null);
          setError(
            isIpcError(e)
              ? e
              : { code: "internal", message: String(e), accountId: null, retriable: false },
          );
          setHasSearched(true);
        });
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(handle);
  }, [active, query, uiFilter]);

  // Activate a hit: open its Chat and record a pending deep-link focus, then close.
  const activate = useCallback(
    (hit: SearchHitVm) => {
      roomsStore.getState().requestFocus({
        accountId: hit.accountId,
        roomId: hit.roomId,
        eventId: hit.eventId,
      });
      onClose();
    },
    [onClose],
  );

  // Keyboard nav within the surface: ↑/↓ move the active row, Enter activates it,
  // Esc closes (Dialog also closes on Esc; this keeps arrow/Enter pointer-free).
  const onKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (hits.length === 0) {
        return;
      }
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setActiveIndex((i) => (i === null ? 0 : Math.min(hits.length - 1, i + 1)));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setActiveIndex((i) => (i === null ? hits.length - 1 : Math.max(0, i - 1)));
      } else if (e.key === "Enter") {
        e.preventDefault();
        const idx = activeIndex ?? 0;
        const hit = hits[idx];
        if (hit !== undefined) {
          activate(hit);
        }
      }
    },
    [hits, activeIndex, activate],
  );

  // Sender suggestions seeded from the current result set's distinct senders
  // (no member-list fetch — sender is exact-match, honestly). Also settable as
  // free-text of a Matrix id.
  const senderSuggestions = useMemo(() => [...new Set(hits.map((h) => h.sender))], [hits]);

  const chatLabel = useCallback(
    (sel: { accountId: string; roomId: string }) => {
      const room = rooms.find((r) => r.accountId === sel.accountId && r.roomId === sel.roomId);
      return room?.displayName ?? sel.roomId;
    },
    [rooms],
  );

  // Open the Export dialog preset to the current search scope (Story 5.5): an
  // in-chat lock → that Chat; a single Account chip → that account; else everything.
  // Closes the search surface first (the two overlays never stack).
  const openExport = useCallback(() => {
    if (effectiveChatLock !== null) {
      exportStore.getState().open({
        scope: "chat",
        accountId: effectiveChatLock.accountId,
        roomId: effectiveChatLock.roomId,
      });
    } else if (accountId !== null) {
      exportStore.getState().open({ scope: "account", accountId, roomId: null });
    } else {
      exportStore.getState().open({ scope: "everything", accountId: null, roomId: null });
    }
    onClose();
  }, [effectiveChatLock, accountId, onClose]);

  // Navigate to the Approval Pane from the ⌘K surface (Story 7.3), closing the
  // overlay first (the two surfaces never stack).
  const goToApprovals = useCallback(() => {
    primaryViewStore.getState().setView("approval");
    onClose();
  }, [onClose]);

  const showNoResults = hasSearched && error === null && hits.length === 0 && query.trim() !== "";

  return (
    <search
      className={cn("flex flex-col gap-3", className)}
      onKeyDown={onKeyDown}
      aria-label="Search your local archive"
    >
      <div className="flex items-start justify-between gap-2">
        <div className="flex flex-col gap-1">
          <h2 className="text-sm font-semibold text-foreground">Searching your local archive</h2>
          <p className="text-xs text-muted-foreground">
            Search works fully offline against your local archive on this Mac.
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <button
            type="button"
            onClick={goToApprovals}
            aria-label="Go to Approval Pane"
            className="h-7 shrink-0 rounded-md border border-input px-2 text-xs text-muted-foreground hover:text-foreground"
          >
            Go to Approval Pane
          </button>
          <button
            type="button"
            onClick={openExport}
            aria-label="Export this scope"
            className="h-7 shrink-0 rounded-md border border-input px-2 text-xs text-muted-foreground hover:text-foreground"
          >
            Export…
          </button>
        </div>
      </div>

      <InputGroup>
        <InputGroupInput
          autoFocus
          placeholder="Search messages"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label="Search query"
        />
        <InputGroupAddon align="inline-end">
          <input
            type="date"
            aria-label="Start date"
            className="bg-transparent text-xs text-muted-foreground outline-none"
            value={startDate ?? ""}
            onChange={(e) => setStartDate(e.target.value === "" ? null : e.target.value)}
          />
          <input
            type="date"
            aria-label="End date"
            className="bg-transparent text-xs text-muted-foreground outline-none"
            value={endDate ?? ""}
            onChange={(e) => setEndDate(e.target.value === "" ? null : e.target.value)}
          />
        </InputGroupAddon>
      </InputGroup>

      {/* Filter controls: sender field + Chat/Network/Account pickers. */}
      <div className="flex flex-wrap items-center gap-2">
        <input
          type="text"
          list="search-sender-suggestions"
          placeholder="Sender (Matrix id)"
          aria-label="Sender"
          className="h-7 rounded-md border border-input bg-transparent px-2 text-xs outline-none"
          value={sender ?? ""}
          onChange={(e) => setSender(e.target.value === "" ? null : e.target.value)}
        />
        <datalist id="search-sender-suggestions">
          {senderSuggestions.map((s) => (
            <option key={s} value={s} />
          ))}
        </datalist>

        {effectiveChatLock === null && (
          <>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  className="h-7 rounded-md border border-input px-2 text-xs text-muted-foreground"
                >
                  Chat
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent className="max-h-64 overflow-y-auto">
                {rooms.map((r) => (
                  <DropdownMenuItem
                    key={`${r.accountId}|${r.roomId}`}
                    onSelect={() => setChat({ accountId: r.accountId, roomId: r.roomId })}
                  >
                    {r.displayName}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>

            {networks.length > 0 && (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button
                    type="button"
                    className="h-7 rounded-md border border-input px-2 text-xs text-muted-foreground"
                  >
                    Network
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent className="max-h-64 overflow-y-auto">
                  {networks.map((n) => (
                    <DropdownMenuItem key={n.name} onSelect={() => setNetwork(n.name)}>
                      {n.name}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            )}

            {accounts.length > 1 && (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button
                    type="button"
                    className="h-7 rounded-md border border-input px-2 text-xs text-muted-foreground"
                  >
                    Account
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent className="max-h-64 overflow-y-auto">
                  {accounts.map((a) => (
                    <DropdownMenuItem key={a.accountId} onSelect={() => setAccountId(a.accountId)}>
                      {a.userId}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            )}
          </>
        )}
      </div>

      {/* Active filter chips, each one-tap removable (the locked in-chat Chat is
          shown but not removable). */}
      <div className="flex flex-wrap items-center gap-1.5">
        {effectiveChatLock !== null && (
          <Badge variant="secondary" aria-label="Chat filter (locked)">
            Chat: {chatLabel(effectiveChatLock)}
          </Badge>
        )}
        {chat !== null && (
          <RemovableChip label={`Chat: ${chatLabel(chat)}`} onRemove={() => setChat(null)} />
        )}
        {network !== null && (
          <RemovableChip label={`Network: ${network}`} onRemove={() => setNetwork(null)} />
        )}
        {accountId !== null && (
          <RemovableChip
            label={`Account: ${accounts.find((a) => a.accountId === accountId)?.userId ?? accountId}`}
            onRemove={() => setAccountId(null)}
          />
        )}
        {sender !== null && sender.trim() !== "" && (
          <RemovableChip label={`Sender: ${sender}`} onRemove={() => setSender(null)} />
        )}
        {startDate !== null && (
          <RemovableChip label={`From: ${startDate}`} onRemove={() => setStartDate(null)} />
        )}
        {endDate !== null && (
          <RemovableChip label={`To: ${endDate}`} onRemove={() => setEndDate(null)} />
        )}
      </div>

      <div className={cn("max-h-[50vh] overflow-y-auto", resultsClassName)}>
        {error !== null ? (
          <div role="alert" className="rounded-md bg-destructive/10 p-3 text-sm text-destructive">
            <p>Search failed: {error.message}</p>
            {error.retriable && (
              <p className="text-xs text-muted-foreground">
                This is usually temporary — try again.
              </p>
            )}
          </div>
        ) : showNoResults ? (
          <p className="p-3 text-sm text-muted-foreground">No matches in your archive.</p>
        ) : hits.length > 0 ? (
          <SearchResultList
            hits={hits}
            rooms={rooms}
            accounts={accounts}
            query={query}
            activeIndex={activeIndex}
            onActivate={activate}
          />
        ) : null}
      </div>
    </search>
  );
}

/** A one-tap-removable filter chip. */
function RemovableChip({ label, onRemove }: { label: string; onRemove: () => void }) {
  return (
    <Badge variant="secondary" className="gap-1">
      {label}
      <button
        type="button"
        onClick={onRemove}
        aria-label={`Remove ${label}`}
        className="ml-0.5 rounded-full text-muted-foreground hover:text-foreground"
      >
        ×
      </button>
    </Badge>
  );
}
