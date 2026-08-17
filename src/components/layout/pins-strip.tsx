/**
 * Pins strip (FR-22, UX-DR4, Story 4.3; phone touch idioms Story 13.6).
 *
 * A horizontal strip of 44 px circular room avatars rendered atop the Inbox view,
 * one per pinned room in the Rust-authoritative order (the {@link pinsRoomsStore}
 * mirror). Clicking an avatar selects the room; a per-avatar context menu offers
 * "Unpin". Dragging an avatar reorders the strip, and the drop dispatches
 * {@link reorderPins} with the new full order, whose authoritative answer arrives
 * back over the stream (there is NO optimistic membership/order overlay; only an
 * ephemeral in-component preview during the drag, cleared on release).
 *
 * **The drag is pointer events, and HTML5 drag is gone (Story 53.1).** For a year
 * the desktop reorder looked live and was not: WebKit fires no `drop` for a drag
 * that named nothing, and once Story 52.7 named it, Tauri's own drag-drop handler
 * still claimed `performDragOperation:` in Rust before WebKit could perform it.
 * {@link "@/hooks/use-pointer-drag"} carries the source lines and the reason
 * `dragDropEnabled: false` is not the fix. The phone's long-press-drag was always
 * pointer-only and always worked; both entries now share that one hook.
 *
 * On the phone tier (Story 13.6) a long-press lifts the pin: dragging while
 * lifted previews a reorder and the release persists it via {@link reorderPins};
 * releasing *without* dragging opens the pin's context menu instead — which,
 * on the phone, also carries "Move up" / "Move down" items as the non-gesture
 * reorder path (disabled while an account filter makes `pins` a partial subset,
 * exactly like the drag). On the desktop the press needs no hold, and a press
 * that does not travel stays the click that selects the room.
 *
 * The strip overflows horizontally (`overflow-x-auto`, no wrap, no growth) so 9+
 * pins scroll rather than wrapping. It is hidden entirely when there are no pins.
 */
import { useRef } from "react";
import { RoomAvatar } from "@/components/chat/RoomAvatar";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { type LongPressDetail, useLongPress } from "@/hooks/use-long-press";
import { usePointerDrag } from "@/hooks/use-pointer-drag";
import { useShellLayout } from "@/hooks/use-shell-layout";
import type { InboxRoomVm } from "@/lib/ipc/client";
import { reorderPins, unpinRoom } from "@/lib/ipc/client";
import type { RoomSelection } from "@/lib/stores/rooms";
import { cn } from "@/lib/utils";

/** Movement past this distance (px) turns a press on a pin into a drag. */
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

/**
 * The press that a pin's reorder begins with.
 *
 * `viaLift` is the phone's long-press entry: there, a press that never moves
 * opens the pin's menu, where on the desktop it stays a plain click that selects
 * the room. Both entries feed one state machine
 * ({@link "@/hooks/use-pointer-drag"}), so the release arithmetic and the stale-
 * index guards are written once.
 */
interface PinPress {
  /** The pin's index in the authoritative `pins` order. */
  index: number;
  viaLift: boolean;
  /** The pressed avatar, and the point pressed: the menu a stationary lift opens
   *  has to open on that element, at that point, after the release. */
  target: HTMLElement;
  clientX: number;
  clientY: number;
}

export function PinsStrip({ pins, onSelect, selected, reorderable = true }: PinsStripProps) {
  const { phone } = useShellLayout();
  const listRef = useRef<HTMLUListElement>(null);

  /**
   * The slot a release at this x would land in: the nearest avatar's midpoint.
   *
   * Measured per call rather than cached at the press, so the strip's own
   * horizontal scroll — and the preview reorder the drag itself paints — resolve
   * where the pins are now instead of where they were.
   */
  const slotAt = (clientX: number): number | null => {
    const items = listRef.current?.querySelectorAll("li");
    if (items === undefined || items.length === 0) {
      return null;
    }
    let nearest = 0;
    let nearestDist = Number.POSITIVE_INFINITY;
    items.forEach((item, index) => {
      const rect = item.getBoundingClientRect();
      const dist = Math.abs(clientX - (rect.left + rect.width / 2));
      if (dist < nearestDist) {
        nearest = index;
        nearestDist = dist;
      }
    });
    return nearest;
  };

  /**
   * The reorder, on both tiers and by pointer only (Story 53.1).
   *
   * The HTML5 drag this strip reordered by until now could not work in the real
   * app: Tauri claims `performDragOperation:` in Rust before WebKit performs it,
   * so the page's `drop` never fires — see {@link "@/hooks/use-pointer-drag"} for
   * the source lines. `dragstart` and `dragover` did fire, which is how the
   * desktop reorder looked live and had been dead since Story 4.3. The phone's
   * long-press-drag (Story 13.6) was already pointer-only and already worked;
   * both entries now feed one state machine, so the release arithmetic and the
   * stale-index guards are written once.
   */
  const drag = usePointerDrag<PinPress, number>({
    slopPx: LIFT_DRAG_TOLERANCE_PX,
    resolve: slotAt,
    onRelease: (press, slot, moved) => {
      if (!moved) {
        if (press.viaLift) {
          // A stationary long-press opens the pin's menu — the same menu the
          // desktop right-click opens. A stationary desktop press is a plain
          // click, and selecting the room is the click's own business.
          press.target.dispatchEvent(
            new MouseEvent("contextmenu", {
              bubbles: true,
              cancelable: true,
              clientX: press.clientX,
              clientY: press.clientY,
            }),
          );
        }
        return;
      }
      // Guard stale indices against a stream Reset mid-drag: they would splice
      // the wrong (or an undefined) element and then throw reading `accountId`
      // of `undefined`. `reorderable` is re-read here and not only at the press,
      // because an account filter can arrive mid-gesture and a partial order
      // would corrupt the pins it hides.
      if (
        !reorderable ||
        slot === null ||
        slot === press.index ||
        press.index < 0 ||
        press.index >= pins.length ||
        slot >= pins.length
      ) {
        return;
      }
      persistOrder(movePin(pins, press.index, slot));
    },
  });

  // ---- Phone long-press entry (Story 13.6) ---------------------------------
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
    // Captured at the lift rather than at the slop: by the time the hold has
    // fired, the gesture is already committed and the pin is already drawn
    // lifted, so there is no click left to protect.
    drag.begin(
      {
        index,
        viaLift: true,
        target: detail.target,
        clientX: detail.clientX,
        clientY: detail.clientY,
      },
      detail,
      true,
    );
  };

  const longPress = useLongPress({ onLongPress: phone ? onPinLift : undefined });

  // Hidden entirely when empty (UX-DR4): no strip, no border, no label.
  if (pins.length === 0) {
    return null;
  }

  // Move a pin one slot without a gesture (the phone menu's Move up/down).
  const moveBy = (index: number, delta: number) => {
    const target = index + delta;
    if (!reorderable || target < 0 || target >= pins.length) {
      return;
    }
    persistOrder(movePin(pins, index, target));
  };

  // While a pin drags, preview the reordered strip; the authoritative order still
  // arrives over the stream after the release. With no HTML5 ghost to lean on,
  // this preview IS the desktop's drop cue.
  const pressed = drag.item;
  const preview =
    pressed !== null && drag.dragging && drag.over !== null && drag.over !== pressed.index
      ? { from: pressed.index, to: drag.over }
      : null;
  const displayPins = preview === null ? pins : movePin(pins, preview.from, preview.to);
  // The display slot the pressed pin occupies, for the lifted styling. A phone
  // lift is drawn lifted from the hold; a desktop press only once it is a drag,
  // so a plain click never flickers.
  let liftedDisplayIndex: number | null = null;
  if (pressed !== null && (pressed.viaLift || drag.dragging)) {
    liftedDisplayIndex = preview === null ? pressed.index : preview.to;
  }

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
          // `index` unless a drag preview is showing).
          const pinIndex =
            preview === null
              ? index
              : pins.findIndex((p) => p.accountId === room.accountId && p.roomId === room.roomId);
          return (
            <li key={`${room.accountId}:${room.roomId}`} className="shrink-0">
              <ContextMenu>
                <ContextMenuTrigger asChild>
                  <button
                    type="button"
                    // The lift handler resolves the pressed pin through this
                    // attribute; phone-gated so the desktop DOM stays identical.
                    data-pin-index={phone ? pinIndex : undefined}
                    onClick={() => onSelect?.({ accountId: room.accountId, roomId: room.roomId })}
                    onPointerDown={(e) => {
                      longPress.onPointerDown(e);
                      // The desktop entry: no hold, and the press becomes a drag
                      // only past the slop, so a click still selects the room.
                      // On the phone the long-press above owns the entry — an
                      // immediate drag there would fight the strip's own scroll.
                      if (phone || !reorderable || e.button !== 0) {
                        return;
                      }
                      drag.begin(
                        {
                          index: pinIndex,
                          viaLift: false,
                          target: e.currentTarget,
                          clientX: e.clientX,
                          clientY: e.clientY,
                        },
                        {
                          pointerId: e.pointerId,
                          clientX: e.clientX,
                          clientY: e.clientY,
                          target: e.currentTarget,
                        },
                      );
                    }}
                    onPointerMove={(e) => {
                      longPress.onPointerMove(e);
                      drag.handlers.onPointerMove(e);
                    }}
                    onPointerUp={(e) => {
                      longPress.onPointerUp(e);
                      drag.handlers.onPointerUp(e);
                    }}
                    onPointerCancel={(e) => {
                      longPress.onPointerCancel(e);
                      drag.handlers.onPointerCancel(e);
                    }}
                    onLostPointerCapture={drag.handlers.onLostPointerCapture}
                    onClickCapture={(e) => {
                      longPress.onClickCapture(e);
                      drag.handlers.onClickCapture(e);
                    }}
                    title={room.displayName}
                    aria-label={`Pinned conversation with ${room.displayName}`}
                    aria-current={isSelected ? "true" : undefined}
                    data-dragging={liftedDisplayIndex === index ? "true" : undefined}
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
