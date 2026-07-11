/**
 * Pins strip (FR-22, UX-DR4, Story 4.3; phone touch idioms Story 13.6).
 *
 * A horizontal strip of 44 px circular room avatars rendered atop the Inbox view,
 * one per pinned room in the Rust-authoritative order (the {@link pinsRoomsStore}
 * mirror). Clicking an avatar selects the room; a per-avatar context menu offers
 * "Unpin". Native HTML5 drag reorders the avatars — the drop dispatches
 * {@link reorderPins} with the new full order and the authoritative order arrives
 * back over the stream (there is NO optimistic membership/order overlay; only an
 * ephemeral in-component preview during the drag, cleared on drop).
 *
 * On the phone tier (Story 13.6) a long-press lifts the pin: dragging while
 * lifted previews a reorder and the drop persists it via {@link reorderPins};
 * releasing *without* dragging opens the pin's context menu instead — which,
 * on the phone, also carries "Move up" / "Move down" items as the non-gesture
 * reorder path (disabled while an account filter makes `pins` a partial subset,
 * exactly like the drag). Desktop/tablet renders byte-for-byte as before.
 *
 * The strip overflows horizontally (`overflow-x-auto`, no wrap, no growth) so 9+
 * pins scroll rather than wrapping. It is hidden entirely when there are no pins.
 */
import { type PointerEvent as ReactPointerEvent, useRef, useState } from "react";
import { RoomAvatar } from "@/components/chat/RoomAvatar";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { type LongPressDetail, useLongPress } from "@/hooks/use-long-press";
import { useShellLayout } from "@/hooks/use-shell-layout";
import type { InboxRoomVm } from "@/lib/ipc/client";
import { reorderPins, unpinRoom } from "@/lib/ipc/client";
import type { RoomSelection } from "@/lib/stores/rooms";
import { cn } from "@/lib/utils";

/** Movement past this distance (px) turns a lifted pin into a drag (Story 13.6). */
const LIFT_DRAG_TOLERANCE_PX = 10;

interface PinsStripProps {
  /** The Pins window, in Rust-authoritative order. */
  pins: InboxRoomVm[];
  /** Select callback; receives the pinned room's account + room ids. */
  onSelect?: (selection: RoomSelection) => void;
  /** The currently open conversation, to mark the active pin. */
  selected?: RoomSelection | null;
  /**
   * Whether drag-to-reorder is enabled. Reorder rewrites the *full* pin set to a
   * contiguous order, so it is only sound when `pins` is the complete set. While an
   * account switcher filter is active `pins` is a filtered subset, so reordering it
   * would submit a partial order and collide with the hidden pins' orders — drag is
   * therefore disabled while filtered (default `true`).
   */
  reorderable?: boolean;
}

/** Move one element of `pins` from `from` to `to`, returning the new array. */
function movePin(pins: InboxRoomVm[], from: number, to: number): InboxRoomVm[] {
  const next = [...pins];
  const [moved] = next.splice(from, 1);
  if (moved === undefined) {
    return pins;
  }
  next.splice(to, 0, moved);
  return next;
}

/** Dispatch the authoritative full-order reorder; best-effort (stream is truth). */
function persistOrder(next: InboxRoomVm[]): void {
  void reorderPins(next.map((room) => ({ accountId: room.accountId, roomId: room.roomId }))).catch(
    () => {},
  );
}

export function PinsStrip({ pins, onSelect, selected, reorderable = true }: PinsStripProps) {
  // Ephemeral drag state: the index being dragged. Cleared on drop/end. The
  // authoritative order always arrives via the stream — this only styles the
  // in-flight avatar during the gesture.
  const [dragIndex, setDragIndex] = useState<number | null>(null);

  // ---- Phone long-press-drag reorder (Story 13.6) --------------------------
  const { phone } = useShellLayout();
  const listRef = useRef<HTMLUListElement>(null);
  // The lifted pin (post long-press): its index in `pins`, the tracked pointer,
  // the press origin, and whether the pointer has actually dragged.
  const liftRef = useRef<{
    pointerId: number;
    index: number;
    startX: number;
    startY: number;
    moved: boolean;
  } | null>(null);
  const [liftedIndex, setLiftedIndex] = useState<number | null>(null);
  // The lifted pin's current preview slot while dragging, or null.
  const [liftTarget, setLiftTarget] = useState<number | null>(null);

  const onPinLift = (detail: LongPressDetail) => {
    const indexAttr = detail.target.closest("[data-pin-index]")?.getAttribute("data-pin-index");
    const index = indexAttr === undefined || indexAttr === null ? Number.NaN : Number(indexAttr);
    if (Number.isNaN(index)) {
      return;
    }
    if (!reorderable) {
      // Filtered subset: reorder is unsound, so the long-press goes straight to
      // the menu (where Move up/down are disabled too).
      detail.target.dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          cancelable: true,
          clientX: detail.clientX,
          clientY: detail.clientY,
        }),
      );
      return;
    }
    liftRef.current = {
      pointerId: detail.pointerId,
      index,
      startX: detail.clientX,
      startY: detail.clientY,
      moved: false,
    };
    setLiftedIndex(index);
    detail.target.setPointerCapture(detail.pointerId);
  };

  const longPress = useLongPress({ onLongPress: phone ? onPinLift : undefined });

  /** Resolve the preview slot for a lifted drag from the pointer's x position. */
  const liftTargetFor = (clientX: number): number | null => {
    const items = listRef.current?.querySelectorAll("li");
    if (items === undefined || items.length === 0) {
      return null;
    }
    let nearest = 0;
    let nearestDist = Number.POSITIVE_INFINITY;
    items.forEach((item, index) => {
      const rect = item.getBoundingClientRect();
      const mid = rect.left + rect.width / 2;
      const dist = Math.abs(clientX - mid);
      if (dist < nearestDist) {
        nearest = index;
        nearestDist = dist;
      }
    });
    return nearest;
  };

  const onLiftPointerMove = (e: ReactPointerEvent<HTMLElement>) => {
    const lift = liftRef.current;
    if (lift === null || e.pointerId !== lift.pointerId) {
      return;
    }
    if (
      !lift.moved &&
      Math.hypot(e.clientX - lift.startX, e.clientY - lift.startY) <= LIFT_DRAG_TOLERANCE_PX
    ) {
      return;
    }
    lift.moved = true;
    setLiftTarget(liftTargetFor(e.clientX));
  };

  const onLiftPointerUp = (e: ReactPointerEvent<HTMLElement>) => {
    const lift = liftRef.current;
    if (lift === null || e.pointerId !== lift.pointerId) {
      return;
    }
    liftRef.current = null;
    setLiftedIndex(null);
    setLiftTarget(null);
    if (!lift.moved) {
      // A stationary long-press: open the pin's menu at the press point — the
      // same menu the desktop right-click opens.
      e.currentTarget.dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          cancelable: true,
          clientX: e.clientX,
          clientY: e.clientY,
        }),
      );
      return;
    }
    const target = liftTargetFor(e.clientX);
    // Guard stale indices against a stream Reset mid-drag, mirroring onDrop.
    if (
      target === null ||
      target === lift.index ||
      lift.index < 0 ||
      lift.index >= pins.length ||
      target >= pins.length
    ) {
      return;
    }
    persistOrder(movePin(pins, lift.index, target));
  };

  const onLiftPointerCancel = (e: ReactPointerEvent<HTMLElement>) => {
    const lift = liftRef.current;
    if (lift === null || e.pointerId !== lift.pointerId) {
      return;
    }
    liftRef.current = null;
    setLiftedIndex(null);
    setLiftTarget(null);
  };

  // Hidden entirely when empty (UX-DR4): no strip, no border, no label.
  if (pins.length === 0) {
    return null;
  }

  const onDrop = (targetIndex: number) => {
    // Ignore no-op drops, and any drop while reorder is disabled (filtered view).
    if (!reorderable || dragIndex === null || dragIndex === targetIndex) {
      setDragIndex(null);
      return;
    }
    // Guard against a stream Reset that shrank or replaced `pins` between drag-start
    // and drop: stale indices would splice the wrong (or an undefined) element and
    // then throw while reading `accountId` of `undefined`.
    if (
      dragIndex < 0 ||
      dragIndex >= pins.length ||
      targetIndex < 0 ||
      targetIndex >= pins.length
    ) {
      setDragIndex(null);
      return;
    }
    const next = movePin(pins, dragIndex, targetIndex);
    setDragIndex(null);
    if (next === pins) {
      return;
    }
    // Dispatch the authoritative reorder; the stream reflects it back. Best-effort.
    persistOrder(next);
  };

  // Move a pin one slot without a gesture (the phone menu's Move up/down).
  const moveBy = (index: number, delta: number) => {
    const target = index + delta;
    if (!reorderable || target < 0 || target >= pins.length) {
      return;
    }
    persistOrder(movePin(pins, index, target));
  };

  // While a lifted pin drags, preview the reordered strip; the authoritative
  // order still arrives over the stream after the drop.
  const liftPreviewActive =
    liftedIndex !== null && liftTarget !== null && liftTarget !== liftedIndex;
  const displayPins = liftPreviewActive ? movePin(pins, liftedIndex, liftTarget) : pins;
  const liftedDisplayIndex = liftPreviewActive ? liftTarget : liftedIndex;

  return (
    <div className="shrink-0 border-border border-b">
      <ul
        ref={listRef}
        aria-label="Pinned conversations"
        className="flex flex-nowrap items-center gap-2 overflow-x-auto p-2"
      >
        {displayPins.map((room, index) => {
          const isSelected =
            selected?.roomId === room.roomId && selected?.accountId === room.accountId;
          // The pin's index in the authoritative `pins` order (identical to
          // `index` unless a lift preview is showing).
          const pinIndex = liftPreviewActive
            ? pins.findIndex((p) => p.accountId === room.accountId && p.roomId === room.roomId)
            : index;
          return (
            <li key={`${room.accountId}:${room.roomId}`} className="shrink-0">
              <ContextMenu>
                <ContextMenuTrigger asChild>
                  <button
                    type="button"
                    // The lift handler resolves the pressed pin through this
                    // attribute; phone-gated so the desktop DOM stays identical.
                    data-pin-index={phone ? pinIndex : undefined}
                    draggable={reorderable}
                    onDragStart={() => reorderable && setDragIndex(pinIndex)}
                    onDragOver={(e) => e.preventDefault()}
                    onDrop={() => onDrop(pinIndex)}
                    onDragEnd={() => setDragIndex(null)}
                    onClick={() => onSelect?.({ accountId: room.accountId, roomId: room.roomId })}
                    onPointerDown={longPress.onPointerDown}
                    onPointerMove={(e) => {
                      longPress.onPointerMove(e);
                      onLiftPointerMove(e);
                    }}
                    onPointerUp={(e) => {
                      longPress.onPointerUp(e);
                      onLiftPointerUp(e);
                    }}
                    onPointerCancel={(e) => {
                      longPress.onPointerCancel(e);
                      onLiftPointerCancel(e);
                    }}
                    onClickCapture={longPress.onClickCapture}
                    title={room.displayName}
                    aria-label={`Pinned conversation with ${room.displayName}`}
                    aria-current={isSelected ? "true" : undefined}
                    data-dragging={
                      dragIndex === pinIndex || liftedDisplayIndex === index ? "true" : undefined
                    }
                    className={cn(
                      "rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring data-[dragging=true]:opacity-50",
                      // Phone (Story 13.6): the pin is a long-press/drag target —
                      // suppress the native callout/selection and let the pointer
                      // (not native panning) own the gesture.
                      phone && "touch-callout-none touch-none select-none",
                    )}
                  >
                    <RoomAvatar room={room} size="xl" />
                  </button>
                </ContextMenuTrigger>
                <ContextMenuContent>
                  <ContextMenuItem
                    className={phone ? "min-h-11" : undefined}
                    onSelect={() => {
                      void unpinRoom(room.accountId, room.roomId).catch(() => {});
                    }}
                  >
                    Unpin
                  </ContextMenuItem>
                  {/* Non-gesture reorder (Story 13.6, phone): Move up/down mirror
                      the long-press-drag; disabled while `pins` is a filtered
                      subset (a partial order would corrupt hidden pins). */}
                  {phone && (
                    <>
                      <ContextMenuSeparator />
                      <ContextMenuItem
                        className="min-h-11"
                        disabled={!reorderable || pinIndex === 0}
                        onSelect={() => moveBy(pinIndex, -1)}
                      >
                        Move up
                      </ContextMenuItem>
                      <ContextMenuItem
                        className="min-h-11"
                        disabled={!reorderable || pinIndex === pins.length - 1}
                        onSelect={() => moveBy(pinIndex, 1)}
                      >
                        Move down
                      </ContextMenuItem>
                    </>
                  )}
                </ContextMenuContent>
              </ContextMenu>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
