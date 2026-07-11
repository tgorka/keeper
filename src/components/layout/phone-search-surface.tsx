/**
 * Merged full-screen phone Search surface (Story 13.4, FR-34/FR-48/FR-58, UX-DR24).
 *
 * The phone's single reachable Search: a full-screen, focus-trapping overlay
 * (a portalled radix `Dialog`, always mounted in `PhoneShell` and driven by
 * `searchSurfaceStore`) that hosts the *reused desktop engines* under one
 * segmented Chats / Messages / Actions input — a new *arrangement container*,
 * never a forked search. It forks no scoring, filtering, debounce, or deep-link:
 *
 * - **Chats** — `paletteQuery(needle, "default", openChat)` → the shared
 *   `PaletteChatRow`s (contacts + chats). Select → open the Chat + close.
 * - **Actions** — `paletteQuery(needle, "action", openChat)` → the shared
 *   `PaletteActionRow`s, dispatched by id via `dispatchPaletteAction`. Every
 *   registered action is reachable; the phone registers none of its own.
 * - **Messages** — the shared `SearchPanel` (message FTS via `searchArchive`),
 *   opened `"chat"`-locked when a `chatLock` is set (the Room ⋯ "Search in chat"
 *   entry), else `"global"`.
 *
 * A leading `>` jumps to Actions and strips itself from the needle (the palette's
 * `parseInput` rule). Chats/Actions feed a bare `Command`/`CommandList` (cmdk
 * without the Dialog wrapper) so the shared rows and ↑/↓/Enter nav come for free.
 * Reduced motion renders the open/close as an instant cut (`motion-reduce:*`);
 * radix restores focus to the opener (magnifier / overflow item) on close. Every
 * tappable target is ≥44pt with an accessible name; there is no bottom tab bar.
 */
import { Dialog as DialogPrimitive } from "radix-ui";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { dispatchPaletteAction } from "@/components/command-palette/actions";
import { PaletteActionRow, PaletteChatRow } from "@/components/command-palette/palette-rows";
import { SearchPanel } from "@/components/search/search-panel";
import { Command, CommandEmpty, CommandGroup, CommandList } from "@/components/ui/command";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import type { PaletteChatVm, PaletteMode, PaletteResultsVm } from "@/lib/ipc/client";
import { paletteQuery } from "@/lib/ipc/client";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore, useRoomsStore } from "@/lib/stores/rooms";
import {
  type SearchSurfaceScope,
  searchSurfaceStore,
  useSearchSurfaceStore,
} from "@/lib/stores/search-surface";
import { cn } from "@/lib/utils";

/** Debounce (ms) before a keystroke fires `palette_query` (matches the desktop palette). */
const DEBOUNCE_MS = 120;

/** The empty palette results shape rendered before the first query resolves. */
const EMPTY_RESULTS: PaletteResultsVm = { contacts: [], chats: [], actions: [] };

/** The segmented scope buttons, in order. */
const SCOPES: Array<{ scope: SearchSurfaceScope; label: string }> = [
  { scope: "chats", label: "Chats" },
  { scope: "messages", label: "Messages" },
  { scope: "actions", label: "Actions" },
];

/**
 * Derive the effective needle + whether a leading `>` forced Actions scope (the
 * palette's `parseInput` `>`-rule): `>` jumps to Actions and strips itself from
 * the needle.
 */
function parseNeedle(raw: string): { forcedActions: boolean; needle: string } {
  if (raw.startsWith(">")) {
    return { forcedActions: true, needle: raw.slice(1).trimStart() };
  }
  return { forcedActions: false, needle: raw };
}

export function PhoneSearchSurface() {
  const isOpen = useSearchSurfaceStore((s) => s.isOpen);
  const storeScope = useSearchSurfaceStore((s) => s.scope);
  const chatLock = useSearchSurfaceStore((s) => s.chatLock);
  const selected = useRoomsStore((s) => s.selected);

  const close = useCallback(() => searchSurfaceStore.getState().close(), []);

  const [value, setValue] = useState("");
  // The scope the user last switched to via the segmented control; seeded from the
  // store's open scope. A leading `>` overrides it to Actions without mutating it.
  const [scope, setScope] = useState<SearchSurfaceScope>(storeScope);
  const [results, setResults] = useState<PaletteResultsVm>(EMPTY_RESULTS);
  // Whether the first palette response has landed (gate the empty state so an
  // empty query doesn't flash "No results.").
  const [hasResponded, setHasResponded] = useState(false);
  // Monotonic request id so an out-of-order (stale) palette response never clobbers
  // a newer one — the query is per-keystroke and races are expected.
  const requestSeq = useRef(0);

  const { forcedActions, needle } = useMemo(() => parseNeedle(value), [value]);
  // A leading `>` forces Actions regardless of the segmented control.
  const effectiveScope: SearchSurfaceScope = forcedActions ? "actions" : scope;

  // Reset the surface each time it opens: clear the input/results and seed the
  // scope from the store (Chats by default; Messages when opened chat-locked).
  useEffect(() => {
    if (!isOpen) {
      return;
    }
    setValue("");
    setResults(EMPTY_RESULTS);
    setHasResponded(false);
    setScope(searchSurfaceStore.getState().scope);
  }, [isOpen]);

  // Debounced palette query for the Chats/Actions scopes (Messages uses SearchPanel
  // and never hits paletteQuery). The open-chat context drives action gating.
  useEffect(() => {
    if (!isOpen || effectiveScope === "messages") {
      return;
    }
    const mode: PaletteMode = effectiveScope === "actions" ? "action" : "default";
    const openChat = selected !== null;
    const seq = ++requestSeq.current;
    const handle = window.setTimeout(() => {
      paletteQuery(needle, mode, openChat)
        .then((res) => {
          if (seq === requestSeq.current && searchSurfaceStore.getState().isOpen) {
            setResults(res);
            setHasResponded(true);
          }
        })
        .catch(() => {
          if (seq === requestSeq.current && searchSurfaceStore.getState().isOpen) {
            setResults(EMPTY_RESULTS);
            setHasResponded(true);
          }
        });
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(handle);
  }, [isOpen, effectiveScope, needle, selected]);

  // Clear stale palette results the instant the scope changes, so the previous
  // scope's rows (e.g. Chats) never linger under the new scope (e.g. Actions)
  // during the debounce window before the fresh query lands. Messages holds no
  // palette results (it renders SearchPanel), so only the palette scopes reset.
  useEffect(() => {
    if (effectiveScope !== "messages") {
      setResults(EMPTY_RESULTS);
      setHasResponded(false);
    }
  }, [effectiveScope]);

  // Select a Chat/Contact: push to the inbox so the conversation is visible, open
  // it, and close the surface (the phone stack pushes to the Room level).
  const openChatResult = useCallback(
    (chat: PaletteChatVm) => {
      primaryViewStore.getState().setView("inbox");
      roomsStore.getState().selectRoom({ accountId: chat.accountId, roomId: chat.roomId });
      close();
    },
    [close],
  );

  // Dispatch an action by id with the open-chat context, then close.
  const runAction = useCallback(
    async (id: string) => {
      const ctx = roomsStore.getState().selected;
      close();
      await dispatchPaletteAction(id, ctx);
    },
    [close],
  );

  const onOpenChange = useCallback(
    (open: boolean) => {
      if (!open) {
        close();
      }
    },
    [close],
  );

  const hasAnyPaletteResult =
    results.contacts.length > 0 || results.chats.length > 0 || results.actions.length > 0;

  // The Messages panel's chat lock is meaningful only in Messages scope; the shared
  // SearchPanel maps `scope` ("chat" | "global") + the explicit lock.
  const messagesActive = effectiveScope === "messages";

  return (
    <DialogPrimitive.Root open={isOpen} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay
          data-slot="dialog-overlay"
          className="fixed inset-0 z-50 bg-black/10 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0 motion-reduce:animate-none"
        />
        <DialogPrimitive.Content
          data-testid="phone-search-surface"
          aria-label="Search"
          className={cn(
            "fixed inset-0 z-50 flex flex-col bg-background text-sm text-foreground outline-none",
            // Safe-area padding (Story 13.5): the full-screen overlay's content
            // clears the notch and home indicator; the vars resolve to 0 off-phone.
            "pt-[var(--safe-top)] pb-[var(--safe-bottom)]",
            "data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0",
            "motion-reduce:animate-none motion-reduce:transition-none",
          )}
        >
          <DialogPrimitive.Title className="sr-only">Search</DialogPrimitive.Title>
          <DialogPrimitive.Description className="sr-only">
            Search chats, messages, and actions.
          </DialogPrimitive.Description>

          {/* Header: back/close affordance + segmented scope control. */}
          <div className="flex h-[var(--phone-header)] shrink-0 items-center gap-1 border-border border-b px-1">
            <DialogPrimitive.Close asChild>
              <button
                type="button"
                aria-label="Close search"
                className="flex size-11 shrink-0 items-center justify-center rounded-full text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
              >
                <span aria-hidden className="text-lg">
                  ←
                </span>
              </button>
            </DialogPrimitive.Close>
            <div
              role="tablist"
              aria-label="Search scope"
              className="ml-1 flex min-w-0 flex-1 items-center gap-1"
            >
              {SCOPES.map((s) => {
                const activeTab = effectiveScope === s.scope;
                return (
                  <button
                    key={s.scope}
                    type="button"
                    role="tab"
                    aria-selected={activeTab}
                    onClick={() => {
                      // A leading `>` forces Actions via `parseNeedle`, which would
                      // otherwise make an explicit scope tap appear inert; strip it
                      // so the tapped scope actually takes effect.
                      setValue((v) => (v.startsWith(">") ? v.slice(1).trimStart() : v));
                      setScope(s.scope);
                    }}
                    className={cn(
                      "inline-flex h-11 min-w-11 items-center justify-center rounded-md px-3 font-medium text-sm",
                      "focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
                      activeTab
                        ? "bg-accent text-accent-foreground"
                        : "text-muted-foreground hover:text-foreground",
                    )}
                  >
                    {s.label}
                  </button>
                );
              })}
            </div>
          </div>

          {/* Body: Messages hosts the shared SearchPanel; Chats/Actions host a bare
              Command with the shared rows. */}
          <div className="flex min-h-0 flex-1 flex-col overflow-y-auto p-3">
            {messagesActive ? (
              <SearchPanel
                active={isOpen && messagesActive}
                scope={chatLock !== null ? "chat" : "global"}
                chatLock={chatLock}
                onClose={close}
                className="min-h-0 flex-1"
                resultsClassName="max-h-none flex-1 min-h-0"
              />
            ) : (
              <Command shouldFilter={false} className="flex min-h-0 flex-1 flex-col bg-transparent">
                <InputGroup>
                  <InputGroupAddon>
                    <span aria-hidden className="text-muted-foreground">
                      ⌕
                    </span>
                  </InputGroupAddon>
                  <InputGroupInput
                    autoFocus
                    value={value}
                    onChange={(e) => setValue(e.target.value)}
                    placeholder={
                      effectiveScope === "actions"
                        ? "Search actions…"
                        : "Search chats and contacts… (type > for actions)"
                    }
                    aria-label="Search query"
                  />
                </InputGroup>
                <CommandList className="mt-2 max-h-none">
                  {hasResponded && !hasAnyPaletteResult && <CommandEmpty>No results.</CommandEmpty>}

                  {results.contacts.length > 0 && (
                    <CommandGroup heading="Contacts">
                      {results.contacts.map((chat) => (
                        <PaletteChatRow
                          key={chat.id}
                          chat={chat}
                          onSelect={() => openChatResult(chat)}
                        />
                      ))}
                    </CommandGroup>
                  )}

                  {results.chats.length > 0 && (
                    <CommandGroup heading="Chats">
                      {results.chats.map((chat) => (
                        <PaletteChatRow
                          key={chat.id}
                          chat={chat}
                          onSelect={() => openChatResult(chat)}
                        />
                      ))}
                    </CommandGroup>
                  )}

                  {results.actions.length > 0 && (
                    <CommandGroup heading="Actions">
                      {results.actions.map((action) => (
                        <PaletteActionRow
                          key={action.id}
                          action={action}
                          onSelect={() => {
                            void runAction(action.id);
                          }}
                        />
                      ))}
                    </CommandGroup>
                  )}
                </CommandList>
              </Command>
            )}
          </div>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
