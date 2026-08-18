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
 *
 * **And the capture has to be taken BACK when the DOM moves the element.** The
 * move that crosses the slop is the same move that paints the drag's preview,
 * and a preview that reorders a keyed list makes React move the pressed node:
 * `insertBefore` on a node already in that parent REMOVES it first, and the
 * removing steps are exactly what Pointer Events hooks for the *implicit release*
 * of pointer capture. So the pins strip's first drag step released its own
 * capture and every later move was discarded — the reorder never landed. The
 * release is listened for natively on the captured element rather than through
 * React's `onLostPointerCapture`, because React delegates at the root container
 * and an element removed for good never reaches it. `isConnected` then tells the
 * two causes apart, and they want opposite things: *moved* is recoverable (take
 * the capture back, the handlers are intact one step later), *unmounted* is not
 * (end the gesture, and clear the click it was going to swallow).
 *
 * **The press must prevent its own default, and the caller is what does it.**
 * Selection is a default action of the compatibility `mousedown`, and cancelling
 * `pointerdown` is what stops that `mousedown` being fired at all — Pointer
 * Events, *mapping for devices that support hover*: a cancelled `pointerdown`
 * sets the PREVENT MOUSE EVENT flag, so `mousedown`/`mousemove`/`mouseup` are
 * suppressed for the rest of the gesture, and the spec's own worked sequence
 * still ends in `click`. Without that cancel, a captured `pointermove` with the
 * button held anchors a selection at the nearest selectable position and extends
 * it across everything the pointer crosses — the board's regression (Story 54.1,
 * FR-324). It is called at the callsite rather than here because the two entries
 * do not share an event: a desktop press has the `pointerdown` in hand, which is
 * {@link "@/components/ui/resizable-columns"} (`:202-203`) exactly, and the pins
 * strip's phone lift reaches {@link PointerDrag.begin} from a long-press detail
 * with no cancellable event left to it.
 *
 * **And the document stops being selectable for the drag's duration.** The cancel
 * above covers the press; {@link DRAG_SELECTION_CLASS} covers the gesture, for
 * the entries that never had a press to cancel and for a selection anchored
 * before the press began. Armed at the slop crossing — a click must not make the
 * app unselectable — and released on every exit: the release, the cancel, a
 * mid-drag unmount, and the lost-capture-for-good branch above, all of which run
 * through one `forget`.
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

/**
 * What the body wears while a drag is live: Tailwind's own `user-select: none`,
 * applied imperatively the way {@link "@/components/layout/conversation-pane"}
 * (`:1240-1247`) applies its flash classes, and named the way
 * {@link "@/components/notes/editor/csv-table"} (`:50`) names a class its tests
 * assert on.
 *
 * A class and not `body.style.userSelect`, because an inline write has to
 * remember and restore whatever was there before, and two disarms racing (a
 * `pointerup` and an unmount in the same tick) would restore it twice from a
 * value the first one already overwrote. `classList.remove` of a class that is
 * not there is a no-op, so every exit path can run unconditionally.
 *
 * It is one class over a document that mounts this hook more than once — the
 * board and the pins strip at least — so the arming is REFERENCE COUNTED
 * ({@link suppressDocumentSelection}). Without the count, whichever of two live
 * gestures ended first stripped the suppression from under the other, and the
 * one still running spent the rest of its travel painting a selection.
 */
export const DRAG_SELECTION_CLASS = "select-none";

/**
 * How many live gestures are holding {@link DRAG_SELECTION_CLASS} on.
 *
 * Module-level, because the class is: two hook instances are two surfaces over
 * one `document.body`, and neither can see the other's state. Each instance
 * holds at most one count (`armedRef`), so the pair is always balanced.
 */
let suppressions = 0;

function suppressDocumentSelection() {
  suppressions += 1;
  document.body.classList.add(DRAG_SELECTION_CLASS);
}

function restoreDocumentSelection() {
  suppressions = Math.max(0, suppressions - 1);
  if (suppressions === 0) {
    document.body.classList.remove(DRAG_SELECTION_CLASS);
  }
}

/** How far the pointer has travelled from the press origin, in CSS pixels. */
export interface PointerDragDelta {
  x: number;
  y: number;
}

/** The delta between gestures, shared so a still card never renders on identity. */
const DRAG_DELTA_ZERO: PointerDragDelta = Object.freeze({ x: 0, y: 0 });

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
  /**
   * Publish the live pointer delta as state, for a caller that translates the
   * pressed element by it ({@link PointerDrag.delta}).
   *
   * Off by default because it costs a render per `pointermove`. A caller that
   * draws only from {@link PointerDrag.over} already renders exactly as often as
   * its own target changes — the pins strip's slot index is a number, so React
   * bails out of the moves that do not cross a slot — and a delta changes on
   * every single move, which would turn that into 60 renders a second for a
   * surface with nothing to do with them.
   */
  trackDelta?: boolean;
}

/**
 * Where these go, which is not all one place.
 *
 * `onPointerMove`, `onPointerUp` and `onPointerCancel` belong on the **surface's
 * own container box** — the board's `<section>`, the strip's `<ul>` — and not on
 * each item. Before the slop crossing there is no capture, so a move is delivered
 * only to the element under the pointer and to that element's ancestors: a press
 * three pixels from the edge of a 28 px card leaves the card before it has
 * travelled six, and the move then lands on the column box, which the card sits
 * *below* rather than above. Nothing would hear it, the drag would silently never
 * start, and the press would stay in flight until the next one. The container is
 * an ancestor of every item — and of the captured element after the crossing, which
 * is where the platform retargets the rest of the gesture — so it hears all of it.
 * A second copy on each item would only re-run the same hit-test on every move of
 * a live drag.
 *
 * `onClickCapture` belongs on the item, beside the item's own `onPointerDown`:
 * that click is the one the item was about to act on, and the capture retargets it
 * to the element the press began on.
 */
export interface PointerDragHandlers {
  onPointerMove: (event: React.PointerEvent<HTMLElement>) => void;
  onPointerUp: (event: React.PointerEvent<HTMLElement>) => void;
  onPointerCancel: (event: React.PointerEvent<HTMLElement>) => void;
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
   * How far the pointer is from where it pressed. `{x: 0, y: 0}` until the slop
   * is crossed, and again the moment the gesture ends — a released card is back
   * at zero on the render that ends the drag, which is what makes the settle a
   * transition rather than a teleport.
   *
   * Always zero unless {@link UsePointerDragOptions.trackDelta} is set.
   */
  delta: PointerDragDelta;
  /**
   * Start tracking a press. Called from an `onPointerDown`, or later by whatever
   * gates the gesture (the pins strip's phone long-press lifts first, and passes
   * `captureNow` because by then the gesture is already committed).
   */
  begin: (item: Item, origin: PointerPressOrigin, captureNow?: boolean) => void;
  /**
   * Forget that the last gesture was a drag, so the click that follows this
   * press belongs to whatever it lands on again.
   *
   * The swallowed-click flag is cleared by the click it eats and by {@link begin},
   * which is enough for a mouse and not enough for a finger: a touch drag ends
   * with no synthesised click at all, and a surface that gates its press never
   * reaches `begin` on the following tap — the pins strip returns before `begin`
   * for *every* phone press, the board for a press on a card's own menu. The flag
   * would still be set and that tap would be eaten. Call this at the top of the
   * surface's `onPointerDown`, ahead of every gate, which is where
   * {@link "@/hooks/use-long-press"} clears its own `firedRef` (`:102`).
   */
  allowNextClick: () => void;
  handlers: PointerDragHandlers;
}

/** The one tracked press. Refs, so a move never renders before it has to. */
interface Press<Item> {
  pointerId: number;
  item: Item;
  /**
   * The element the press began on: the one the capture is taken on, and the one
   * a mid-gesture DOM move loses it from. Held here rather than read off the
   * crossing move's `currentTarget`, because the move handler lives on the
   * surface's container and a crossing delivered there would capture the container
   * instead of the item.
   */
  target: HTMLElement;
  startX: number;
  startY: number;
  moved: boolean;
}

/** The captured element and the native release listener it carries. */
interface CaptureHold {
  element: HTMLElement;
  onLost: (event: PointerEvent) => void;
}

export function usePointerDrag<Item, Target>(
  options: UsePointerDragOptions<Item, Target>,
): PointerDrag<Item, Target> {
  // A ref, so the handler object stays referentially stable while always reading
  // the current closures — the {@link "@/hooks/use-long-press"} shape.
  const optionsRef = React.useRef(options);
  optionsRef.current = options;

  const pressRef = React.useRef<Press<Item> | null>(null);
  // Set the moment a press becomes a drag; cleared by the click it swallows, by
  // the next press, by {@link PointerDrag.allowNextClick}, and by the unmount
  // that ends a gesture. A touch drag ends without a click, so every one of those
  // clearing sites is load-bearing on the phone.
  const draggedRef = React.useRef(false);
  // The element holding the capture, while it holds it. Null between gestures,
  // and before the slop crossing on the entries that capture there.
  const captureRef = React.useRef<CaptureHold | null>(null);
  // Whether THIS hook is holding one of {@link DRAG_SELECTION_CLASS}'s counts.
  // Two surfaces mount this hook, and one pointer can only be dragging on one of
  // them; the flag is what stops the other one's unmount releasing a suppression
  // it never armed, and what keeps this instance's arm/release pairs balanced —
  // it bounds the instance to at most one count, which is what makes the
  // module-level total exact.
  const armedRef = React.useRef(false);

  const [item, setItem] = React.useState<Item | null>(null);
  const [dragging, setDragging] = React.useState(false);
  const [over, setOver] = React.useState<Target | null>(null);
  const [delta, setDelta] = React.useState<PointerDragDelta>(DRAG_DELTA_ZERO);

  const detach = React.useCallback(() => {
    const held = captureRef.current;
    if (held === null) {
      return;
    }
    captureRef.current = null;
    held.element.removeEventListener("lostpointercapture", held.onLost);
  }, []);

  const suppressSelection = React.useCallback(() => {
    if (armedRef.current) {
      return;
    }
    armedRef.current = true;
    suppressDocumentSelection();
  }, []);

  const restoreSelection = React.useCallback(() => {
    if (!armedRef.current) {
      return;
    }
    armedRef.current = false;
    restoreDocumentSelection();
  }, []);

  const forget = React.useCallback(() => {
    detach();
    restoreSelection();
    pressRef.current = null;
    setItem(null);
    setDragging(false);
    setOver(null);
    setDelta(DRAG_DELTA_ZERO);
  }, [detach, restoreSelection]);

  // A press interrupted by the whole surface unmounting has no element left to
  // hear the release on; the listener goes with it — and so does the document's
  // selection suppression, which would otherwise outlive by forever the gesture
  // that armed it, with nothing left alive able to release it.
  React.useEffect(
    () => () => {
      detach();
      restoreSelection();
    },
    [detach, restoreSelection],
  );

  /**
   * Take the capture and listen for losing it.
   *
   * Idempotent by design: the phone's lift captures at the hold, and the slop
   * crossing then asks again for the same pointer on the same element.
   */
  const capture = React.useCallback(
    (press: Press<Item>) => {
      if (captureRef.current !== null) {
        return;
      }
      const element = press.target;
      const onLost = (event: PointerEvent) => {
        const current = pressRef.current;
        // The implicit release that follows every `pointerup` lands here too, by
        // which time `onPointerUp` has already forgotten the press: nothing to
        // take back, and nothing to tear down twice.
        if (current === null || event.pointerId !== current.pointerId) {
          return;
        }
        if (element.isConnected) {
          // MOVED, not removed: React reconciled the drag's own preview and
          // `insertBefore` took this node out of its parent before putting it
          // back. It is still mounted and its handlers are intact, so the gesture
          // is still live — take the capture back, or every remaining move and
          // the release itself are delivered somewhere else.
          element.setPointerCapture(event.pointerId);
          return;
        }
        // Removed for good. The click this drag was going to swallow will never
        // be dispatched at a node that no longer exists, so the flag goes with
        // the press — otherwise it eats the next, unrelated click.
        draggedRef.current = false;
        forget();
      };
      captureRef.current = { element, onLost };
      element.addEventListener("lostpointercapture", onLost);
      element.setPointerCapture(press.pointerId);
    },
    [forget],
  );

  const begin = React.useCallback(
    (next: Item, origin: PointerPressOrigin, captureNow = false) => {
      // A press whose release was never seen — a pointer that left a small target
      // without ever crossing the slop, so no capture and no `pointerup` here —
      // must not wedge the surface for good. The new press wins, and it cannot
      // inherit the previous one's capture hold, or its selection suppression:
      // the replaced press's `pointerup` is dropped by the pointerId guard, so
      // this is the LAST site that could ever give the document back. A pointer
      // that crossed the slop and was then replaced left the whole app
      // unselectable until the surface unmounted.
      detach();
      restoreSelection();
      draggedRef.current = false;
      const press: Press<Item> = {
        pointerId: origin.pointerId,
        item: next,
        target: origin.target,
        startX: origin.clientX,
        startY: origin.clientY,
        moved: false,
      };
      pressRef.current = press;
      setItem(next);
      setDragging(false);
      setOver(null);
      setDelta(DRAG_DELTA_ZERO);
      if (captureNow) {
        capture(press);
      }
    },
    [capture, detach, restoreSelection],
  );

  const allowNextClick = React.useCallback(() => {
    draggedRef.current = false;
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
          // The crossing is the moment the press stops being a click, so it is
          // the moment the document stops being selectable.
          suppressSelection();
          // From here the pointer may leave the pressed element — and a drag that
          // stopped following the pointer exactly when the user moved fastest is
          // the defect capture exists to prevent.
          capture(press);
        }
        if (optionsRef.current.trackDelta === true) {
          setDelta({ x: event.clientX - press.startX, y: event.clientY - press.startY });
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
      onClickCapture: (event) => {
        if (!draggedRef.current) {
          return;
        }
        draggedRef.current = false;
        event.preventDefault();
        event.stopPropagation();
      },
    }),
    [capture, forget, suppressSelection],
  );

  return { item, dragging, over, delta, begin, allowNextClick, handlers };
}
