/**
 * The command palette (⌘K) (Story 9.1, epic 9 spine).
 *
 * A 640 px shadcn `CommandDialog` that renders the grouped, ranked results of the
 * Rust `palette_query` command — Contacts (DM rooms), Chats (non-DM rooms), and
 * Actions — and dispatches a selected result. All filtering, scoring, and ordering
 * are authoritative in Rust (AD-20); this component only renders and dispatches, and
 * sets `shouldFilter={false}` so cmdk never re-filters the Rust results.
 *
 * Keys: type-to-filter (debounced), ↑/↓ move the selection (cmdk), Enter executes
 * the highlighted result, ⌘Enter on a chat/contact *peeks* (opens/focuses the chat
 * via `roomsStore.selectRoom` while keeping the palette open), and a leading `>`
 * switches to action mode. An empty/short/no-match query shows the top registered
 * actions plus a `>` hint. The palette is a single modal overlay (depth ≤ 1) — the
 * ⌘K hook toggles the store, and opening it closes anything below by being the only
 * dialog mounted here.
 */

import { useCommandState } from "cmdk";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Badge } from "@/components/ui/badge";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Kbd } from "@/components/ui/kbd";
import { accountHueVar } from "@/lib/account-hue";
import type { PaletteChatVm, PaletteMode, PaletteResultsVm } from "@/lib/ipc/client";
import { paletteQuery } from "@/lib/ipc/client";
import { commandPaletteStore, useCommandPaletteStore } from "@/lib/stores/command-palette";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore, useRoomsStore } from "@/lib/stores/rooms";
import { dispatchPaletteAction } from "./actions";

/** Debounce (ms) before a keystroke fires `palette_query`. */
const DEBOUNCE_MS = 120;

/** The empty results shape rendered before the first query resolves. */
const EMPTY_RESULTS: PaletteResultsVm = { contacts: [], chats: [], actions: [] };

/** Derive the query mode and the effective needle from the raw input value. */
function parseInput(raw: string): { mode: PaletteMode; needle: string } {
  if (raw.startsWith(">")) {
    return { mode: "action", needle: raw.slice(1).trimStart() };
  }
  return { mode: "default", needle: raw };
}

export function CommandPalette() {
  const isOpen = useCommandPaletteStore((s) => s.isOpen);
  const selected = useRoomsStore((s) => s.selected);

  const [value, setValue] = useState("");
  const [results, setResults] = useState<PaletteResultsVm>(EMPTY_RESULTS);
  // Whether the first `palette_query` response has landed since open. Until then the
  // results are the reset-empty sentinel, and showing "No results." would flash even
  // though an empty query is meant to show top actions + a `>` hint — so gate the
  // empty state on this.
  const [hasResponded, setHasResponded] = useState(false);
  // Monotonic request id so an out-of-order (stale) response never clobbers a newer
  // one — the palette query is per-keystroke and races are expected.
  const requestSeq = useRef(0);
  // The cmdk-highlighted item's value (a chat id or action id), tracked in a ref so
  // the keydown handler can read the current selection synchronously for ⌘Enter.
  const selectedValue = useRef("");

  const { mode, needle } = useMemo(() => parseInput(value), [value]);

  // Reset the input whenever the palette opens so it never reopens pre-filled.
  useEffect(() => {
    if (isOpen) {
      setValue("");
      setResults(EMPTY_RESULTS);
      setHasResponded(false);
    }
  }, [isOpen]);

  // Debounced query: any input change (or open) re-queries Rust. The open-chat
  // context (whether a chat is selected) drives action gating + context ranking.
  useEffect(() => {
    if (!isOpen) {
      return;
    }
    const openChat = selected !== null;
    const seq = ++requestSeq.current;
    const handle = setTimeout(() => {
      paletteQuery(needle, mode, openChat)
        .then((res) => {
          // Apply only if this is still the latest request AND the palette is still
          // open — a response that resolves after close must never set state.
          if (seq === requestSeq.current && commandPaletteStore.getState().isOpen) {
            setResults(res);
            setHasResponded(true);
          }
        })
        .catch(() => {
          if (seq === requestSeq.current && commandPaletteStore.getState().isOpen) {
            setResults(EMPTY_RESULTS);
            setHasResponded(true);
          }
        });
    }, DEBOUNCE_MS);
    return () => clearTimeout(handle);
  }, [isOpen, needle, mode, selected]);

  const close = useCallback(() => commandPaletteStore.getState().close(), []);

  const onOpenChange = useCallback((open: boolean) => {
    commandPaletteStore.getState()[open ? "open" : "close"]();
  }, []);

  // Peek a chat/contact: open/focus it without closing the palette (⌘Enter). Switch
  // to the inbox first so the conversation pane is actually visible — the palette is
  // global, so `primaryView` may be bridges/approval/archive where the pane is
  // replaced and `selectRoom` alone would leave the picked chat invisible.
  const peekChat = useCallback((chat: PaletteChatVm) => {
    primaryViewStore.getState().setView("inbox");
    roomsStore.getState().selectRoom({ accountId: chat.accountId, roomId: chat.roomId });
  }, []);

  // Select a chat/contact: open it and close the palette (Enter/click). Switch to the
  // inbox first (same reason as peek) so the conversation is visible from any view.
  const openChat = useCallback(
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

  const hasRoomResults = results.contacts.length > 0 || results.chats.length > 0;
  const hasAnyResult = hasRoomResults || results.actions.length > 0;
  // The `>` hint shows when a default-mode query yielded no chat/contact matches —
  // the frontend then presents the top actions plus the hint to try action mode.
  const showActionHint = mode === "default" && !hasRoomResults;

  return (
    <CommandDialog open={isOpen} onOpenChange={onOpenChange} className="w-[640px] max-w-[640px]">
      <Command
        shouldFilter={false}
        onKeyDown={(event) => {
          // ⌘Enter peeks the highlighted chat/contact without closing. Always swallow
          // the chord (preventDefault) so cmdk's default Enter can't fire and run+close
          // an action when a non-chat row is highlighted — a surprising "peek". Only
          // chat/contact rows peek (scoped by kind: actions are not in this set); an
          // action or anything else is a deliberate no-op.
          if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
            event.preventDefault();
            event.stopPropagation();
            const id = selectedValue.current;
            const chat = [...results.contacts, ...results.chats].find((c) => c.id === id);
            if (chat) {
              peekChat(chat);
            }
          }
        }}
      >
        <SelectionTracker
          onChange={(v) => {
            selectedValue.current = v;
          }}
        />
        <CommandInput
          value={value}
          onValueChange={setValue}
          placeholder="Search chats, contacts, and actions… (type > for actions)"
        />
        <CommandList>
          {hasResponded && !hasAnyResult && <CommandEmpty>No results.</CommandEmpty>}

          {results.contacts.length > 0 && (
            <CommandGroup heading="Contacts">
              {results.contacts.map((chat) => (
                <ChatRow key={chat.id} chat={chat} onSelect={() => openChat(chat)} />
              ))}
            </CommandGroup>
          )}

          {results.chats.length > 0 && (
            <CommandGroup heading="Chats">
              {results.chats.map((chat) => (
                <ChatRow key={chat.id} chat={chat} onSelect={() => openChat(chat)} />
              ))}
            </CommandGroup>
          )}

          {results.actions.length > 0 && (
            <CommandGroup heading={showActionHint ? "Actions — type > to filter" : "Actions"}>
              {results.actions.map((action) => (
                <CommandItem
                  key={action.id}
                  value={action.id}
                  onSelect={() => {
                    void runAction(action.id);
                  }}
                >
                  <span aria-hidden className="text-muted-foreground">
                    ⚡
                  </span>
                  <span className="truncate">{action.title}</span>
                  {action.shortcut !== null && <Kbd className="ml-auto">{action.shortcut}</Kbd>}
                </CommandItem>
              ))}
            </CommandGroup>
          )}
        </CommandList>
      </Command>
    </CommandDialog>
  );
}

/**
 * A zero-DOM tracker that surfaces cmdk's highlighted item value into a callback,
 * so the parent's keydown handler can read the current selection for ⌘Enter without
 * poking the DOM. Renders nothing.
 */
function SelectionTracker({ onChange }: { onChange: (value: string) => void }) {
  const value = useCommandState((state) => state.value) as string;
  useEffect(() => {
    onChange(value ?? "");
  }, [value, onChange]);
  return null;
}

/** One chat/contact result row: type glyph, hue dot, name, network badge. */
function ChatRow({ chat, onSelect }: { chat: PaletteChatVm; onSelect: () => void }) {
  return (
    <CommandItem value={chat.id} onSelect={onSelect}>
      <span aria-hidden className="text-muted-foreground">
        {chat.isDirect ? "◍" : "◆"}
      </span>
      <span
        aria-hidden
        data-testid="account-hue-dot"
        className="size-2 shrink-0 rounded-full"
        style={{ backgroundColor: accountHueVar(chat.hueIndex) }}
      />
      <span className="truncate">{chat.displayName}</span>
      {chat.network !== null && (
        <Badge variant="secondary" className="ml-auto shrink-0">
          {chat.network}
        </Badge>
      )}
    </CommandItem>
  );
}
