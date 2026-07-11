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
 * is byte-for-byte unchanged from the pre-extraction overlay.
 */
import { useCallback, useMemo } from "react";
import { SearchPanel } from "@/components/search/search-panel";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { useRoomsStore } from "@/lib/stores/rooms";
import { searchStore, useSearchStore } from "@/lib/stores/search";

export function SearchOverlay() {
  const isOpen = useSearchStore((s) => s.isOpen);
  const scope = useSearchStore((s) => s.scope);
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

  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogContent
        className="top-24 max-w-2xl translate-y-0 gap-3 p-4 sm:max-w-2xl"
        aria-label="Search your local archive"
      >
        <SearchPanel active={isOpen} scope={scope} chatLock={chatLock} onClose={close} />
      </DialogContent>
    </Dialog>
  );
}
