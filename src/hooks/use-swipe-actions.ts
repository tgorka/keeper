/**
 * Pointer-based horizontal row-swipe engine (Story 13.6, touch idioms).
 *
 * Mirrors the Story 13.2 edge-swipe pointer idiom: pointer capture once
 * horizontal intent is established, a clamped live `dx`, a half-width commit
 * on release, a flick commit (same `FLICK_MIN_DX_PX` / `FLICK_VELOCITY_PX_PER_MS`
 * constants), and a vertical-intent bailout that leaves scrolling to the list.
 *
 * `leading` is a rightward swipe (`dx > 0`), `trailing` a leftward one
 * (`dx < 0`); a side without a config is clamped to zero, so the row never
 * drags toward an action that does not exist. A side may opt into a sticky
 * reveal (`revealPx`): released past half that width but short of the commit
 * threshold, the row settles open at exactly that offset so revealed action
 * buttons are tappable; a later commit-past-half, an inward drag, or `close()`
 * resolves it. Presentation (transforms, transitions, reduced-motion cuts) is
 * the caller's job — this hook only owns the gesture state machine.
 */
import * as React from "react";

/** Commit the swipe as a flick when the drag averaged over this speed… (13.2). */
export const FLICK_VELOCITY_PX_PER_MS = 0.5;
/** …and travelled at least this far, so a tap never commits (13.2). */
export const FLICK_MIN_DX_PX = 40;
/**
 * Horizontal movement (px) before the swipe claims the pointer. Kept above the
 * long-press move tolerance (10px) so a drag always cancels a pending
 * long-press before it starts swiping.
 */
export const SWIPE_INTENT_SLOP_PX = 12;
/** Commit fraction of the swiped row's width (the "half-width commit"). */
const COMMIT_FRACTION = 0.5;

export type SwipeSide = "leading" | "trailing";

export interface SwipeSideConfig {
  /** Fired when this side's swipe commits (release past half width, or flick). */
  onCommit: () => void;
  /**
   * Sticky reveal width (px). Released with `|dx| >= revealPx / 2` but below
   * the commit threshold, the row settles open at this offset (`revealed`)
   * instead of snapping back. Omit for commit-or-snap-back sides.
   */
  revealPx?: number;
}

export interface UseSwipeActionsOptions {
  /** Master gate — callers pass the phone tier. Disabled = inert handlers. */
  enabled: boolean;
  /** Rightward swipe (`dx > 0`). */
  leading?: SwipeSideConfig;
  /** Leftward swipe (`dx < 0`). */
  trailing?: SwipeSideConfig;
}

export interface SwipeHandlers {
  onPointerDown: (e: React.PointerEvent<HTMLElement>) => void;
  onPointerMove: (e: React.PointerEvent<HTMLElement>) => void;
  onPointerUp: (e: React.PointerEvent<HTMLElement>) => void;
  onPointerCancel: (e: React.PointerEvent<HTMLElement>) => void;
  onLostPointerCapture: (e: React.PointerEvent<HTMLElement>) => void;
  /**
   * Swallows (capture phase) the synthetic `click` a browser fires after a
   * horizontal drag — so a swipe that snaps back never also taps the row
   * through to `onClick` (opening the conversation / inline editor). Mirrors
   * the long-press bridge's click suppression.
   */
  onClickCapture: (e: React.MouseEvent<HTMLElement>) => void;
}

export interface SwipeActions {
  /** Live horizontal offset: the drag position, the settled reveal, or 0. */
  dx: number;
  /** Whether a horizontal drag is in flight (callers drop transitions). */
  dragging: boolean;
  /** The side whose commit threshold the in-flight drag has crossed, if any. */
  committing: SwipeSide | null;
  /** The side settled open at its reveal width, if any. */
  revealed: SwipeSide | null;
  /** Close a settled reveal (tap-on-row / after an action button fires). */
  close: () => void;
  /** Spread on the swiped element. */
  handlers: SwipeHandlers;
}

/**
 * One tracked pointer. `pending` until intent is decided: vertical intent
 * bails the gesture out entirely (native scroll wins); horizontal intent
 * captures the pointer and starts dragging from `baseDx` (non-zero when the
 * drag continues from a settled reveal).
 */
interface PointerState {
  pointerId: number;
  startX: number;
  startY: number;
  startT: number;
  width: number;
  baseDx: number;
  phase: "pending" | "dragging" | "bailed";
}

export function useSwipeActions(options: UseSwipeActionsOptions): SwipeActions {
  const optionsRef = React.useRef(options);
  optionsRef.current = options;

  const [drag, setDrag] = React.useState<{ dx: number; committing: SwipeSide | null } | null>(null);
  const [revealed, setRevealed] = React.useState<SwipeSide | null>(null);
  const revealedRef = React.useRef(revealed);
  revealedRef.current = revealed;
  const pointerRef = React.useRef<PointerState | null>(null);
  // Set once a drag actually starts; consumed by the next `click` so a
  // snap-back swipe never taps the row through. Reset on every fresh press so a
  // stale flag (e.g. a drag whose click never arrived) can't eat an unrelated tap.
  const didDragRef = React.useRef(false);

  const settledDxFor = (side: SwipeSide | null): number => {
    const opts = optionsRef.current;
    if (side === "trailing") {
      return -(opts.trailing?.revealPx ?? 0);
    }
    if (side === "leading") {
      return opts.leading?.revealPx ?? 0;
    }
    return 0;
  };

  const handlers = React.useMemo<SwipeHandlers>(() => {
    /** Clamp a drag offset to the sides that actually have actions. */
    const clampDx = (dx: number, width: number): number => {
      const opts = optionsRef.current;
      const min = opts.trailing !== undefined ? -width : 0;
      const max = opts.leading !== undefined ? width : 0;
      return Math.min(Math.max(dx, min), max);
    };

    const settledDx = (side: SwipeSide | null): number => {
      const opts = optionsRef.current;
      if (side === "trailing") {
        return -(opts.trailing?.revealPx ?? 0);
      }
      if (side === "leading") {
        return opts.leading?.revealPx ?? 0;
      }
      return 0;
    };

    const reset = (e: React.PointerEvent<HTMLElement>) => {
      const pointer = pointerRef.current;
      if (pointer === null || e.pointerId !== pointer.pointerId) {
        return;
      }
      pointerRef.current = null;
      setDrag(null);
    };

    return {
      onPointerDown: (e: React.PointerEvent<HTMLElement>) => {
        if (!optionsRef.current.enabled || pointerRef.current !== null) {
          return;
        }
        didDragRef.current = false;
        const rectWidth = e.currentTarget.getBoundingClientRect().width;
        pointerRef.current = {
          pointerId: e.pointerId,
          startX: e.clientX,
          startY: e.clientY,
          startT: e.timeStamp,
          width: rectWidth > 0 ? rectWidth : window.innerWidth,
          baseDx: settledDx(revealedRef.current),
          phase: "pending",
        };
      },
      onPointerMove: (e: React.PointerEvent<HTMLElement>) => {
        const pointer = pointerRef.current;
        if (pointer === null || e.pointerId !== pointer.pointerId || pointer.phase === "bailed") {
          return;
        }
        const rawDx = e.clientX - pointer.startX;
        const rawDy = e.clientY - pointer.startY;
        if (pointer.phase === "pending") {
          // Vertical intent wins: leave the gesture to native scrolling.
          if (Math.abs(rawDy) > Math.abs(rawDx) && Math.abs(rawDy) > SWIPE_INTENT_SLOP_PX) {
            pointer.phase = "bailed";
            return;
          }
          if (Math.abs(rawDx) >= SWIPE_INTENT_SLOP_PX && Math.abs(rawDx) >= Math.abs(rawDy)) {
            pointer.phase = "dragging";
            e.currentTarget.setPointerCapture(e.pointerId);
          } else {
            return;
          }
        }
        const dx = clampDx(pointer.baseDx + rawDx, pointer.width);
        const committing: SwipeSide | null =
          Math.abs(dx) >= pointer.width * COMMIT_FRACTION
            ? dx < 0
              ? "trailing"
              : "leading"
            : null;
        setDrag({ dx, committing });
      },
      onPointerUp: (e: React.PointerEvent<HTMLElement>) => {
        const pointer = pointerRef.current;
        if (pointer === null || e.pointerId !== pointer.pointerId) {
          return;
        }
        pointerRef.current = null;
        if (pointer.phase !== "dragging") {
          // A tap or a bailed-out vertical scroll: no swipe state to resolve.
          return;
        }
        setDrag(null);
        const dx = clampDx(pointer.baseDx + (e.clientX - pointer.startX), pointer.width);
        const side: SwipeSide | null = dx < 0 ? "trailing" : dx > 0 ? "leading" : null;
        if (side === null) {
          // A drag that returned to origin: suppress the synthetic click so it
          // does not tap the row through (open the conversation / editor).
          didDragRef.current = true;
          setRevealed(null);
          return;
        }
        const opts = optionsRef.current;
        const config = side === "trailing" ? opts.trailing : opts.leading;
        // Flick: the *travel of this drag* (not the settled base) is fast and
        // far enough, moving outward (the same direction the row now sits).
        const traveled = dx - pointer.baseDx;
        const dt = Math.max(e.timeStamp - pointer.startT, 1);
        const flick =
          Math.abs(traveled) > FLICK_MIN_DX_PX &&
          Math.abs(traveled) / dt > FLICK_VELOCITY_PX_PER_MS &&
          Math.sign(traveled) === Math.sign(dx);
        if (Math.abs(dx) >= pointer.width * COMMIT_FRACTION || flick) {
          // Committed: the action fires; suppress the trailing click so the row
          // does not also open the conversation as the row snaps back to origin.
          didDragRef.current = true;
          setRevealed(null);
          config?.onCommit();
          return;
        }
        if (config?.revealPx !== undefined && Math.abs(dx) >= config.revealPx / 2) {
          // Settled open: leave the click alone so the next tap can close the
          // reveal or press a revealed action button.
          setRevealed(side);
          return;
        }
        // Snapped back below the reveal threshold: same click-through guard.
        didDragRef.current = true;
        setRevealed(null);
      },
      onPointerCancel: (e: React.PointerEvent<HTMLElement>) => {
        reset(e);
      },
      onLostPointerCapture: (e: React.PointerEvent<HTMLElement>) => {
        reset(e);
      },
      onClickCapture: (e: React.MouseEvent<HTMLElement>) => {
        if (!didDragRef.current) {
          return;
        }
        didDragRef.current = false;
        e.preventDefault();
        e.stopPropagation();
      },
    };
  }, []);

  const close = React.useCallback(() => {
    setRevealed(null);
    setDrag(null);
  }, []);

  return {
    dx: drag !== null ? drag.dx : settledDxFor(revealed),
    dragging: drag !== null,
    committing: drag?.committing ?? null,
    revealed,
    close,
    handlers,
  };
}
