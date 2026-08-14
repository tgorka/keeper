/**
 * The desktop search surface (Story 5.4 / Story 13.4, FR-34, UX-DR13).
 *
 * A thin `Dialog` wrapper over the shared {@link SearchPanel} — the single source
 * of message-search behavior, reused verbatim by the phone `PhoneSearchSurface`.
 * Opened two ways from the same `searchStore`: global (`⌘⇧F`, no room lock) and
 * in-chat (`⌘F`, scoped + locked to the open Chat). This wrapper only owns the
 * centered `Dialog` chrome (open state, Escape/scrim close); everything below the
 * chrome — the query field + filter chips, the debounced `searchArchive`, the
 * out-of-order guard, the honest offline header, the grouped `SearchResultList`,
 * the deep-link, and the export/approval shortcuts — lives in `SearchPanel`.
 *
 * The in-chat lock is derived here (as before): `scope === "chat"` with a
 * selected room supplies `chatLock`; otherwise `null` (global). Desktop behavior
 * for messages is byte-for-byte unchanged from the pre-extraction overlay.
 *
 * FR-267 adds two more sources beside messages — notes and sessions — as a
 * source switcher above the body rather than a second overlay. One surface for
 * "find the words I typed" is the whole point: the operator does not know, at
 * the moment of pressing the chord, whether the sentence they remember was in a
 * message, a note or a session log, and three separate shortcuts would make them
 * decide before they can look. The switcher keeps the query across a switch, so
 * looking in the wrong place first costs one click and no retyping.
 *
 * An in-chat surface (`⌘F` over a Chat) shows no switcher: it is locked to that
 * Chat by construction, and a "Notes" tab inside a chat-scoped search would be
 * offering to leave the scope the user just asked for.
 */
import { useCallback, useMemo } from "react";
import { DocumentSearchPanel } from "@/components/search/document-search-panel";
import { SearchPanel } from "@/components/search/search-panel";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { useRoomsStore } from "@/lib/stores/rooms";
import { type SearchSource, searchStore, useSearchStore } from "@/lib/stores/search";
import { cn } from "@/lib/utils";

/** The three sources, in the order the switcher shows them. */
const SOURCES: Array<{ source: SearchSource; label: string }> = [
  { source: "messages", label: "Messages" },
  { source: "notes", label: "Notes" },
  { source: "sessions", label: "Sessions" },
];

export function SearchOverlay() {
  const isOpen = useSearchStore((s) => s.isOpen);
  const scope = useSearchStore((s) => s.scope);
  const source = useSearchStore((s) => s.source);
  const selected = useRoomsStore((s) => s.selected);

  // The in-chat scope lock: forces the room/account scope and shows a locked Chat
  // chip. `null` for global scope.
  const chatLock = useMemo(
    () => (scope === "chat" && selected !== null ? selected : null),
    [scope, selected],
  );

  const close = useCallback(() => searchStore.getState().close(), []);

  const onOpenChange = useCallback(
    (open: boolean) => {
      if (!open) {
        close();
      }
    },
    [close],
  );

  // A chat-locked surface is a message search by construction (the store forces
  // the source too) — the switcher would be a row of tabs that leave the lock.
  const showSwitcher = scope === "global";
  const messages = !showSwitcher || source === "messages";

  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogContent
        className="top-24 max-w-2xl translate-y-0 gap-3 p-4 sm:max-w-2xl"
        aria-label="Search your local archive"
      >
        {showSwitcher && (
          <div role="tablist" aria-label="Search source" className="flex items-center gap-1">
            {SOURCES.map((entry) => {
              const activeTab = source === entry.source;
              return (
                <button
                  key={entry.source}
                  type="button"
                  role="tab"
                  aria-selected={activeTab}
                  onClick={() => searchStore.getState().setSource(entry.source)}
                  className={cn(
                    "inline-flex h-7 items-center justify-center rounded-md px-3 font-medium text-xs",
                    "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                    activeTab
                      ? "bg-accent text-accent-foreground"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {entry.label}
                </button>
              );
            })}
          </div>
        )}
        {messages ? (
          <SearchPanel active={isOpen} scope={scope} chatLock={chatLock} onClose={close} />
        ) : (
          <DocumentSearchPanel
            source={source === "sessions" ? "sessions" : "notes"}
            active={isOpen}
            onClose={close}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}
