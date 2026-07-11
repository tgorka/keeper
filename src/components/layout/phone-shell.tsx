/**
 * Phone stack shell (Stories 13.1/13.2, AD-31).
 *
 * The single-pane projection of the existing selection state for viewports
 * below 768px: exactly one visible level at a time — level 0 Inbox, level 1
 * Room, level 2 Detail — derived purely from `roomsStore.selected` (0 ↔ 1) and
 * the lifted `detailStore` (1 ↔ 2). No routing library, no forked components:
 * the stack mounts the unchanged `ChatListPane` / `ConversationPane` /
 * `DetailPanel` trees. Level 0 stays mounted under every push so the Inbox
 * scroll offset survives; higher levels are opaque `bg-background` overlays.
 *
 * Story 13.2 adds the native-feeling, reversible, accessible stack:
 * - A single 52px `PhoneHeader` owns the bar at both overlay levels (UX-DR21);
 *   `ConversationPane` renders with `showHeader={false}` so there is never a
 *   second bar.
 * - Transform-driven push/pop transitions (~250ms ease-out; the level beneath
 *   shifts back 25% and dims), rendered as instant cuts under
 *   `prefers-reduced-motion` (`useReducedMotion`). An exiting level stays
 *   mounted until its transform transition ends (presence), so pop animates.
 * - An edge-swipe-back gesture on the active overlay's leading ~20px, active
 *   only at level ≥ 1 (FR-60): the drag tracks the finger, commits `onBack`
 *   past half the width or on a fast flick, and snaps back otherwise. Level
 *   0's leading edge is reserved for the Story 13.3 drawer.
 * - Focus management (UX-DR28 / DW-110): push focuses the new level's back
 *   button, pop restores focus to the element that pushed, Escape pops one
 *   level, and covered levels are `inert`.
 * - DW-109: a phone-scoped effect closes the Detail panel whenever the
 *   selection changes, so a room (re)selected with Detail open lands on the
 *   Room level — never on Detail. Desktop detail persistence is untouched
 *   (this component mounts only on the phone tier).
 */
import {
  type FocusEventHandler,
  type KeyboardEvent,
  type ReactNode,
  type PointerEvent as ReactPointerEvent,
  type TransitionEvent,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { ChatListPane } from "@/components/layout/chat-list-pane";
import { ConversationPane } from "@/components/layout/conversation-pane";
import { DetailPanel } from "@/components/layout/detail-panel";
import { PhoneHeader } from "@/components/layout/phone-header";
import { useReducedMotion } from "@/hooks/use-reduced-motion";
import { detailStore, useDetailStore } from "@/lib/stores/detail-ui";
import { roomsStore, useRoomsStore } from "@/lib/stores/rooms";
import { cn } from "@/lib/utils";

/** Commit the edge-swipe as a flick when the drag averaged over this speed… */
const FLICK_VELOCITY_PX_PER_MS = 0.5;
/** …and travelled at least this far (so a tap on the edge zone never pops). */
const FLICK_MIN_DX_PX = 40;

/** Clamp a drag delta to the swipeable range `0..width`. */
function clampDx(dx: number, width: number): number {
  return Math.min(Math.max(dx, 0), width);
}

interface StackLevelProps {
  /** Which stack level this wrapper renders (exposed as `data-level` for tests). */
  levelIndex: 0 | 1 | 2;
  /** Whether the level should be shown (current level ≥ this one). */
  open: boolean;
  /** Whether a higher level covers this one (shift back 25% + dim). */
  covered: boolean;
  /** Covered/exited levels are inert so keyboard/AT never reach behind the top. */
  inert: boolean;
  /** Reduced motion renders every role change as an instant cut. */
  reducedMotion: boolean;
  /** Live px offset while this level is being edge-dragged, else `null`. */
  dragX: number | null;
  /** 0..1 progress of the level above being dragged away (returns -25% → 0), else `null`. */
  coveredProgress: number | null;
  className?: string;
  onFocusCapture?: FocusEventHandler<HTMLDivElement>;
  children: ReactNode;
}

/**
 * One stack level with presence: it mounts as soon as `open` flips true
 * (sliding in from the trailing edge unless reduced motion) and, on `open`
 * flipping false, stays mounted at `translateX(100%)` until its transform
 * transition ends — so pop animates — then unmounts. The transform is derived
 * from the level's role: active → 0, covered → -25% + dim, entering/exiting →
 * 100%; an in-flight edge-drag overrides it with the finger's offset.
 */
function StackLevel({
  levelIndex,
  open,
  covered,
  inert,
  reducedMotion,
  dragX,
  coveredProgress,
  className,
  onFocusCapture,
  children,
}: StackLevelProps) {
  const nodeRef = useRef<HTMLDivElement>(null);
  const [present, setPresent] = useState(open);
  // `entering` holds the just-mounted level at translateX(100%) for one styled
  // frame so the transition to translateX(0) has a start point to animate from.
  const [entering, setEntering] = useState(false);

  // Presence bookkeeping is derived during render (the sanctioned setState-in-
  // render adjustment) so a push mounts the level in the same pass it opens.
  if (open && !present) {
    setPresent(true);
    setEntering(!reducedMotion);
  }
  if (!open && present && (reducedMotion || entering)) {
    // Reduced motion pops as an instant cut; a pop that interrupts a not-yet-
    // started enter has no transition to wait for — unmount immediately.
    setPresent(false);
    setEntering(false);
  }

  useLayoutEffect(() => {
    if (!entering) {
      return;
    }
    // Force a style/layout flush at the off-screen transform, then move to the
    // active transform — the computed-style change is what starts the slide.
    nodeRef.current?.getBoundingClientRect();
    setEntering(false);
  }, [entering]);

  // Presence normally ends on the transform `transitionend`, but that event can be
  // missed (transition interrupted, tab hidden mid-animation). Guard the animated-
  // exit path with a timeout so a popped level can never leak as a permanently-
  // mounted inert pane. Reduced-motion / not-yet-started enters already unmount
  // synchronously in the render-time presence check above.
  useEffect(() => {
    if (open || reducedMotion || !present) {
      return;
    }
    const id = window.setTimeout(() => setPresent(false), 400);
    return () => window.clearTimeout(id);
  }, [open, reducedMotion, present]);

  if (!present) {
    return null;
  }

  const exiting = !open;
  const dragging = dragX !== null || coveredProgress !== null;

  let transform: string;
  if (dragX !== null) {
    transform = `translateX(${dragX}px)`;
  } else if (entering || exiting) {
    transform = "translateX(100%)";
  } else if (coveredProgress !== null) {
    transform = `translateX(${-25 * (1 - coveredProgress)}%)`;
  } else if (covered) {
    transform = "translateX(-25%)";
  } else {
    transform = "translateX(0)";
  }

  const onTransitionEnd = (e: TransitionEvent<HTMLDivElement>) => {
    // Only the wrapper's own transform transition ends presence — a child's
    // transition (or the covered shift) must never unmount the level.
    if (e.target !== e.currentTarget || e.propertyName !== "transform") {
      return;
    }
    if (!open) {
      setPresent(false);
    }
  };

  return (
    <div
      ref={nodeRef}
      data-level={levelIndex}
      inert={inert}
      onFocusCapture={onFocusCapture}
      onTransitionEnd={onTransitionEnd}
      className={cn(
        "flex flex-col bg-background ease-out",
        // An in-flight drag tracks the finger 1:1; transitions resume on release.
        dragging ? "transition-none" : "transition-transform",
        reducedMotion ? "duration-0" : "duration-[250ms]",
        covered && "brightness-95",
        exiting && "pointer-events-none",
        className,
      )}
      style={{ transform }}
    >
      {children}
    </div>
  );
}

export function PhoneShell() {
  const selected = useRoomsStore((s) => s.selected);
  const detailOpen = useDetailStore((s) => s.open);
  const toggleDetail = useDetailStore((s) => s.toggleDetail);
  const reducedMotion = useReducedMotion();

  // One visible level, derived purely from existing selection state:
  //   detailOpen && selected -> 2 (Detail); selected -> 1 (Room); else 0 (Inbox).
  const level = detailOpen && selected !== null ? 2 : selected !== null ? 1 : 0;

  // DW-109 (phone-scoped): a selection change never lands on Detail — close it
  // whenever `selected` changes so a room (re)selected with Detail open resolves
  // to the Room level. `openDetail()` leaves `selected` unchanged, so the
  // tap-identity → Detail push still works, and the previous-value guard keeps
  // a mount (e.g. crossing the breakpoint) from discarding an open Detail.
  // Desktop persistence is untouched: this component mounts only on the phone tier.
  // Compare by value (accountId + roomId), not object identity, so a store re-emit
  // of the *same* room with a fresh object reference (a future deep-link/search
  // re-selection) never spuriously closes an open Detail — only a genuine room
  // change closes it, landing the stack on the Room level.
  const prevSelectedRef = useRef(selected);
  useEffect(() => {
    const prev = prevSelectedRef.current;
    const changed = prev?.accountId !== selected?.accountId || prev?.roomId !== selected?.roomId;
    if (changed) {
      prevSelectedRef.current = selected;
      detailStore.getState().closeDetail();
    }
  }, [selected]);

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

  // ---- Focus management (UX-DR28 / DW-110) -------------------------------
  const back1Ref = useRef<HTMLButtonElement>(null);
  const back2Ref = useRef<HTMLButtonElement>(null);
  // The most recently focused element per level, tracked via focus-capture on
  // each level's wrapper — captured *before* a push makes the level inert (an
  // inert subtree blurs its focus, so reading `document.activeElement` in the
  // push effect would already be too late).
  const lastFocusedRef = useRef(new Map<number, HTMLElement>());
  // The element that pushed each level, restored on the matching pop.
  const pushersRef = useRef(new Map<number, HTMLElement | null>());
  const prevLevelRef = useRef(level);

  useEffect(() => {
    const prev = prevLevelRef.current;
    prevLevelRef.current = level;
    if (level === prev) {
      return;
    }
    if (level > prev) {
      // Push: remember the pusher (the element focused on the level we left)
      // and move focus to the new level's back button.
      pushersRef.current.set(level, lastFocusedRef.current.get(prev) ?? null);
      const backRef = level === 2 ? back2Ref : back1Ref;
      backRef.current?.focus();
      return;
    }
    // Pop: restore focus to the element that pushed the popped level; when it
    // is gone (deep link, unmounted row), fall back to the active back button.
    const pusher = pushersRef.current.get(prev) ?? null;
    for (let l = prev; l > level; l--) {
      pushersRef.current.delete(l);
    }
    if (pusher?.isConnected) {
      pusher.focus();
    } else if (level === 1) {
      back1Ref.current?.focus();
    }
  }, [level]);

  const captureFocusFor =
    (l: number): FocusEventHandler<HTMLDivElement> =>
    (e) => {
      lastFocusedRef.current.set(l, e.target as HTMLElement);
    };

  // Escape pops one level at any level ≥ 1 (the keyboard twin of the chevron).
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "Escape" || e.defaultPrevented || level === 0) {
      return;
    }
    // Ignore keys from portalled overlays (menus/popovers live outside the
    // stack's DOM but still bubble through the React tree).
    if (!e.currentTarget.contains(e.target as Node)) {
      return;
    }
    e.preventDefault();
    onBack();
  };

  // ---- Edge-swipe back (FR-60), active only at level ≥ 1 ------------------
  const containerRef = useRef<HTMLDivElement>(null);
  const pointerRef = useRef<{
    pointerId: number;
    startX: number;
    startT: number;
    width: number;
  } | null>(null);
  const [drag, setDrag] = useState<{ dx: number; width: number } | null>(null);

  const onEdgePointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (pointerRef.current !== null) {
      return;
    }
    const rectWidth = containerRef.current?.getBoundingClientRect().width ?? 0;
    const width = rectWidth > 0 ? rectWidth : window.innerWidth;
    pointerRef.current = { pointerId: e.pointerId, startX: e.clientX, startT: e.timeStamp, width };
    e.currentTarget.setPointerCapture(e.pointerId);
    setDrag({ dx: 0, width });
  };

  const onEdgePointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const pointer = pointerRef.current;
    if (pointer === null || e.pointerId !== pointer.pointerId) {
      return;
    }
    setDrag({ dx: clampDx(e.clientX - pointer.startX, pointer.width), width: pointer.width });
  };

  const onEdgePointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    const pointer = pointerRef.current;
    if (pointer === null || e.pointerId !== pointer.pointerId) {
      return;
    }
    pointerRef.current = null;
    setDrag(null);
    const dx = clampDx(e.clientX - pointer.startX, pointer.width);
    const dt = Math.max(e.timeStamp - pointer.startT, 1);
    const flick = dx > FLICK_MIN_DX_PX && dx / dt > FLICK_VELOCITY_PX_PER_MS;
    if (dx > pointer.width * 0.5 || flick) {
      onBack();
      return;
    }
    // Released below the threshold: `drag` cleared above, so the active level's
    // transform returns to 0 through the normal transition (snap back).
  };

  const onEdgePointerCancel = (e: ReactPointerEvent<HTMLDivElement>) => {
    const pointer = pointerRef.current;
    if (pointer === null || e.pointerId !== pointer.pointerId) {
      return;
    }
    pointerRef.current = null;
    setDrag(null);
  };

  // The ~20px leading-edge hit zone, rendered only inside the *active* overlay
  // (level ≥ 1). Level 0's leading edge is deliberately bare — reserved for the
  // Story 13.3 drawer.
  const edgeSwipeZone = (
    <div
      aria-hidden="true"
      data-testid="edge-swipe-back"
      className="absolute inset-y-0 left-0 z-10 w-5 touch-none"
      onPointerDown={onEdgePointerDown}
      onPointerMove={onEdgePointerMove}
      onPointerUp={onEdgePointerUp}
      onPointerCancel={onEdgePointerCancel}
      onLostPointerCapture={onEdgePointerCancel}
    />
  );

  // While a drag is live, the active level tracks the finger and the level
  // directly beneath returns from -25% to 0 proportionally.
  const dragProgress = drag !== null && drag.width > 0 ? drag.dx / drag.width : null;
  const dragXFor = (l: number): number | null => (drag !== null && level === l ? drag.dx : null);
  const coveredProgressFor = (l: number): number | null =>
    dragProgress !== null && level === l + 1 ? dragProgress : null;

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: Escape-pops-one-level is a stack-wide keyboard affordance (UX-DR28); the chevron button is the labeled interactive twin.
    <div
      ref={containerRef}
      onKeyDown={onKeyDown}
      className="relative flex min-h-0 min-w-0 flex-1 overflow-hidden"
    >
      {/* Level 0 — always mounted so the Inbox scroll position survives pushes. */}
      <StackLevel
        levelIndex={0}
        open
        covered={level > 0}
        inert={level !== 0}
        reducedMotion={reducedMotion}
        dragX={null}
        coveredProgress={coveredProgressFor(0)}
        onFocusCapture={captureFocusFor(0)}
        className="min-h-0 min-w-0 flex-1"
      >
        <ChatListPane />
      </StackLevel>
      <StackLevel
        levelIndex={1}
        open={selected !== null}
        covered={level > 1}
        inert={level !== 1}
        reducedMotion={reducedMotion}
        dragX={dragXFor(1)}
        coveredProgress={coveredProgressFor(1)}
        onFocusCapture={captureFocusFor(1)}
        className="absolute inset-0 z-10"
      >
        {level === 1 && edgeSwipeZone}
        <PhoneHeader level={1} onBack={onBack} backRef={back1Ref} />
        <div className="flex min-h-0 flex-1">
          <ConversationPane
            detailOpen={detailOpen}
            onToggleDetail={toggleDetail}
            showHeader={false}
          />
        </div>
      </StackLevel>
      <StackLevel
        levelIndex={2}
        open={selected !== null && detailOpen}
        covered={false}
        inert={level !== 2}
        reducedMotion={reducedMotion}
        dragX={dragXFor(2)}
        coveredProgress={null}
        onFocusCapture={captureFocusFor(2)}
        className="absolute inset-0 z-20"
      >
        {level === 2 && edgeSwipeZone}
        <PhoneHeader level={2} onBack={onBack} backRef={back2Ref} />
        <div className="flex min-h-0 flex-1">
          <DetailPanel />
        </div>
      </StackLevel>
    </div>
  );
}
