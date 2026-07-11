/**
 * Phone stack shell (Story 13.1, AD-31).
 *
 * The single-pane projection of the existing selection state for viewports
 * below 768px: exactly one visible level at a time — level 0 Inbox, level 1
 * Room, level 2 Detail — derived purely from `roomsStore.selected` (0 ↔ 1) and
 * the lifted `detailStore` (1 ↔ 2). No routing library, no forked components:
 * the stack mounts the unchanged `ChatListPane` / `ConversationPane` /
 * `DetailPanel` trees. Level 0 stays mounted under every push so the Inbox
 * scroll offset survives (and Story 13.2's pop transition has something to
 * animate); higher levels are opaque `bg-background` overlays covering it.
 *
 * The back control here is deliberately minimal-but-accessible (≥44pt hit
 * area, `aria-label="Back"`, pops exactly one level); Story 13.2 replaces it
 * with the styled 52px phone-header, transitions, and edge-swipe back.
 */
import { ChevronLeft } from "lucide-react";
import { ChatListPane } from "@/components/layout/chat-list-pane";
import { ConversationPane } from "@/components/layout/conversation-pane";
import { DetailPanel } from "@/components/layout/detail-panel";
import { detailStore, useDetailStore } from "@/lib/stores/detail-ui";
import { roomsStore, useRoomsStore } from "@/lib/stores/rooms";

/** Minimal accessible back control: ≥44pt (h-11/w-11) hit area, pops one level. */
function BackControl({ onBack }: { onBack: () => void }) {
  return (
    <div className="flex shrink-0 items-center border-border border-b">
      <button
        type="button"
        aria-label="Back"
        onClick={onBack}
        className="flex h-11 w-11 items-center justify-center text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <ChevronLeft className="size-5" aria-hidden="true" />
      </button>
    </div>
  );
}

export function PhoneShell() {
  const selected = useRoomsStore((s) => s.selected);
  const detailOpen = useDetailStore((s) => s.open);
  const toggleDetail = useDetailStore((s) => s.toggleDetail);

  // One visible level, derived purely from existing selection state:
  //   detailOpen && selected -> 2 (Detail); selected -> 1 (Room); else 0 (Inbox).
  const level = detailOpen && selected !== null ? 2 : selected !== null ? 1 : 0;

  // Pop exactly one level: Detail closes back to the Room; the Room clears the
  // selection back to the Inbox. Read the stores imperatively so the handler
  // never closes over stale render state.
  const onBack = () => {
    if (detailStore.getState().open) {
      detailStore.getState().closeDetail();
      return;
    }
    roomsStore.getState().selectRoom(null);
  };

  return (
    <div className="relative flex min-h-0 min-w-0 flex-1">
      {/* Level 0 — always mounted so the Inbox scroll position survives pushes. */}
      <ChatListPane />
      {selected !== null && (
        <div className="absolute inset-0 z-10 flex flex-col bg-background">
          {level === 1 && <BackControl onBack={onBack} />}
          <div className="flex min-h-0 flex-1">
            <ConversationPane detailOpen={detailOpen} onToggleDetail={toggleDetail} />
          </div>
        </div>
      )}
      {selected !== null && detailOpen && (
        <div className="absolute inset-0 z-20 flex flex-col bg-background">
          <BackControl onBack={onBack} />
          <div className="flex min-h-0 flex-1">
            <DetailPanel />
          </div>
        </div>
      )}
    </div>
  );
}
