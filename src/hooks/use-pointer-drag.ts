/**
 * Press an item, move it, release it somewhere else — as pointer events, because
 * in this app the HTML5 `drop` event cannot fire (Story 53.1, FR-314).
 *
 * **Why this hook exists at all.** Two surfaces reordered things with hand-rolled
 * HTML5 drag-and-drop, and on macOS neither worked. Tauri installs a wry
 * drag-drop handler whose closure always answers `true`
 * (`tauri-runtime-wry-2.11.4/src/lib.rs:4862-4896`); wry implements
 * `NSDraggingDestination` on the WKWebView subclass itself
 * (`wry-0.55.1/src/wkwebview/class/wry_web_view.rs:77-112`) and forwards
 * `performDragOperation:` to `super` only when that closure answers `false`
 * (`wry-0.55.1/src/wkwebview/drag_drop.rs:88-95`). So the drop is claimed in Rust
 * before WebKit performs it: `dragstart` and `dragover` fire, because they are
 * page-internal, and the page's `drop` never does. Turning the handler off
 * (`dragDropEnabled: false`) is per-window and config-time, and would take
 * Story 3.7's drop-an-OS-file-to-attach with it
 * (`layout/conversation-pane.tsx:814-848`, the app's only `onDragDropEvent`
 * consumer). Pointer events touch none of that machinery: no OS drag session is
 * started, so nothing in the native layer can claim the release.
 *
 * **What it does and does not own.** It owns the press/move/release state
 * machine: one tracked pointer, a slop threshold that separates a click from a
 * drag, the pointer capture that keeps the gesture alive once the pointer leaves
 * the pressed element, and the one click that has to be swallowed after a drag so
 * a released card does not also open. It owns no geometry and no writes: the
 * caller says what the pointer is over ({@link UsePointerDragOptions.resolve})
 * and what to do about it ({@link UsePointerDragOptions.onRelease}). That split
 * is what lets a four-column board and a one-row avatar strip share it.
 *
 * **Capture is taken on the slop crossing, not on the press.** Pointer capture
 * retargets the compatibility mouse events too, so capturing on `pointerdown`
 * would deliver the `click` to the captured element instead of to the button
 * inside it — a card whose title stopped opening. Crossing the slop is the moment
 * the gesture stops being a click, and it is also still inside the pressed
 * element, so the crossing `pointermove` is reliably delivered without capture.
 * This is the shape {@link "@/hooks/use-swipe-actions"} already uses (`:187`);
 * {@link "@/components/ui/resizable-columns"} (`:202`) captures on press because
 * a resize seam has no click to protect.
 */
import * as React from "react";

/**
 * Movement past this distance (px) turns a press into a drag.
 *
 * Small, because these are desktop pointers on small targets: a board card is
 * about 28 px tall, and a threshold near the phone tier's 10 px
 * ({@link "@/hooks/use-long-press"}) would swallow the first third of a
 * cross-column move and make the lift feel late. Large enough that the one or two
 * pixels a mouse slides during a deliberate click stay a click — the shake a
 * trackpad adds to a tap is under three.
 */
export const POINTER_DRAG_SLOP_PX = 6;

/** Where a press began, and which element takes the capture. */
export interface PointerPressOrigin {
  pointerId: number;
  clientX: number;
  clientY: number;
  /** The element the gesture is captured on — normally the handler's `currentTarget`. */
  target: HTMLElement;
}

export interface UsePointerDragOptions<Item, Target> {
  /**
   * What a release at this viewport point would land on, or null for nowhere.
   *
   * Called on every move past the slop and once more at the release, so it must
   * measure rather than remember: reading `getBoundingClientRect()` per call is
   * what makes a mid-gesture scroll or re-layout land in the right place, where
   * rects cached at the press would freeze the pre-scroll geometry.
   */
  resolve: (clientX: number, clientY: number) => Target | null;
  /**
   * The release. `target` is null when the press never passed the slop — that is
   * a click, and the caller must leave it to whatever the press landed on.
   */
  onRelease: (item: Item, target: Target | null, moved: boolean) => void;
  /** Override {@link POINTER_DRAG_SLOP_PX}. */
  slopPx?: number;
}

/** Spread on the element the press begins on, beside its own `onPointerDown`. */
export interface PointerDragHandlers {
  onPointerMove: (event: React.PointerEvent<HTMLElement>) => void;
  onPointerUp: (event: React.PointerEvent<HTMLElement>) => void;
  onPointerCancel: (event: React.PointerEvent<HTMLElement>) => void;
  onLostPointerCapture: (event: React.PointerEvent<HTMLElement>) => void;
  onClickCapture: (event: React.MouseEvent<HTMLElement>) => void;
}

export interface PointerDrag<Item, Target> {
  /** The pressed item while a press is in flight, else null. */
  item: Item | null;
  /** True once the press has travelled past the slop: a drag, not a click. */
  dragging: boolean;
  /** What the last move resolved to — the only thing a drop cue may be drawn from. */
  over: Target | null;
  /**
   * Start tracking a press. Called from an `onPointerDown`, or later by whatever
   * gates the gesture (the pins strip's phone long-press lifts first, and passes
   * `captureNow` because by then the gesture is already committed).
   */
  begin: (item: Item, origin: PointerPressOrigin, captureNow?: boolean) => void;
  handlers: PointerDragHandlers;
}

/** The one tracked press. Refs, so a move never renders before it has to. */
interface Press<Item> {
  pointerId: number;
  item: Item;
  startX: number;
  startY: number;
  moved: boolean;
}

export function usePointerDrag<Item, Target>(
  options: UsePointerDragOptions<Item, Target>,
): PointerDrag<Item, Target> {
  // A ref, so the handler object stays referentially stable while always reading
  // the current closures — the {@link "@/hooks/use-long-press"} shape.
  const optionsRef = React.useRef(options);
  optionsRef.current = options;

  const pressRef = React.useRef<Press<Item> | null>(null);
  // Set the moment a press becomes a drag, cleared by the click it swallows and
  // by the next press. A touch drag ends without a click, so clearing it at
  // `begin` too is what stops a stale flag eating the following tap.
  const draggedRef = React.useRef(false);

  const [item, setItem] = React.useState<Item | null>(null);
  const [dragging, setDragging] = React.useState(false);
  const [over, setOver] = React.useState<Target | null>(null);

  const forget = React.useCallback(() => {
    pressRef.current = null;
    setItem(null);
    setDragging(false);
    setOver(null);
  }, []);

  const begin = React.useCallback((next: Item, origin: PointerPressOrigin, captureNow = false) => {
    // A press whose release was never seen — a pointer that left a small
    // target without ever crossing the slop, so no capture and no `pointerup`
    // here — must not wedge the surface for good. The new press wins.
    draggedRef.current = false;
    pressRef.current = {
      pointerId: origin.pointerId,
      item: next,
      startX: origin.clientX,
      startY: origin.clientY,
      moved: false,
    };
    setItem(next);
    setDragging(false);
    setOver(null);
    if (captureNow) {
      origin.target.setPointerCapture(origin.pointerId);
    }
  }, []);

  const handlers = React.useMemo<PointerDragHandlers>(
    () => ({
      onPointerMove: (event) => {
        const press = pressRef.current;
        if (press === null || event.pointerId !== press.pointerId) {
          return;
        }
        if (!press.moved) {
          const slop = optionsRef.current.slopPx ?? POINTER_DRAG_SLOP_PX;
          if (Math.hypot(event.clientX - press.startX, event.clientY - press.startY) <= slop) {
            return;
          }
          press.moved = true;
          draggedRef.current = true;
          setDragging(true);
          // From here the pointer may leave the pressed element — and a drag that
          // stopped following the pointer exactly when the user moved fastest is
          // the defect capture exists to prevent.
          event.currentTarget.setPointerCapture(event.pointerId);
        }
        setOver(optionsRef.current.resolve(event.clientX, event.clientY));
      },
      onPointerUp: (event) => {
        const press = pressRef.current;
        if (press === null || event.pointerId !== press.pointerId) {
          return;
        }
        const { item: released, moved } = press;
        // Cleared before the release runs, so a caller that renders from `item`
        // or `over` never draws a gesture that has already ended.
        forget();
        const target = moved ? optionsRef.current.resolve(event.clientX, event.clientY) : null;
        optionsRef.current.onRelease(released, target, moved);
      },
      onPointerCancel: (event) => {
        if (pressRef.current?.pointerId === event.pointerId) {
          forget();
        }
      },
      onLostPointerCapture: (event) => {
        // The capture element left the DOM mid-gesture, or the platform took the
        // pointer back. The implicit release after `pointerup` also lands here,
        // by which time there is nothing left to forget.
        if (pressRef.current?.pointerId === event.pointerId) {
          forget();
        }
      },
      onClickCapture: (event) => {
        if (!draggedRef.current) {
          return;
        }
        draggedRef.current = false;
        event.preventDefault();
        event.stopPropagation();
      },
    }),
    [forget],
  );

  return { item, dragging, over, begin, handlers };
}
