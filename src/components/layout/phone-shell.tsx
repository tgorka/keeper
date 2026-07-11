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
import { RefreshCw, WifiOff } from "lucide-react";
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
import { LeadingDrawer } from "@/components/layout/leading-drawer";
import { PhoneHeader } from "@/components/layout/phone-header";
import { PhoneInboxHeader } from "@/components/layout/phone-inbox-header";
import { PhoneSearchSurface } from "@/components/layout/phone-search-surface";
import { OFFLINE_PILL_TEXT } from "@/components/layout/sidebar-pane";
import { useKeyboardInset } from "@/hooks/use-keyboard-inset";
import { useReducedMotion } from "@/hooks/use-reduced-motion";
import { useShellLayout } from "@/hooks/use-shell-layout";
import { useStaleResumePill } from "@/hooks/use-stale-resume-pill";
import { syncNow } from "@/lib/ipc/client";
import { accountStatusStore, useShellOffline } from "@/lib/stores/account-status";
import { detailStore, useDetailStore } from "@/lib/stores/detail-ui";
import { leadingDrawerStore, useLeadingDrawerStore } from "@/lib/stores/leading-drawer";
import { roomsStore, useRoomsStore } from "@/lib/stores/rooms";
import { searchSurfaceStore, useSearchSurfaceStore } from "@/lib/stores/search-surface";
import { cn } from "@/lib/utils";

/** Commit the edge-swipe as a flick when the drag averaged over this speed… */
const FLICK_VELOCITY_PX_PER_MS = 0.5;
/** …and travelled at least this far (so a tap on the edge zone never pops). */
const FLICK_MIN_DX_PX = 40;

/**
 * The level-0 pull-down reveal threshold (Story 13.4): a downward pull released
 * in `[reveal, refresh)` opens Search; below it snaps back with no open.
 */
const PULL_REVEAL_THRESHOLD_PX = 64;

/**
 * The second, larger threshold on the same pull axis (Story 13.6): released at
 * or past this distance the pull triggers a refresh — `syncNow()` kicks each
 * active account's SyncService — instead of opening Search. The indicator's
 * affordance switches as the finger crosses it.
 */
const PULL_REFRESH_THRESHOLD_PX = 128;

/**
 * Fallback ceiling on the refresh spinner: it normally clears on the next
 * connection-status tick, but a fully offline (tickless) session must still
 * resolve — never a stuck spinner, never an error toast.
 */
const REFRESH_SPINNER_TIMEOUT_MS = 8000;

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

  // Keyboard avoidance (Story 13.5, UX-DR25): only the phone tier drives the
  // `--kb-inset` var — the visualViewport listener never runs on desktop/tablet,
  // so the desktop layout stays byte-for-byte.
  const { phone } = useShellLayout();
  useKeyboardInset({ enabled: phone });

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

  // ---- Leading drawer (Story 13.3) ----------------------------------------
  const drawerOpen = useLeadingDrawerStore((s) => s.isOpen);
  const drawerButtonRef = useRef<HTMLButtonElement>(null);
  // UX-DR28: return focus to the avatar drawer button whenever the drawer
  // transitions open → closed (the edge-swipe-open path radix cannot auto-restore
  // for; a button-opened close is already covered by radix, and re-focusing the
  // same element is a no-op).
  const prevDrawerOpenRef = useRef(drawerOpen);
  useEffect(() => {
    const wasOpen = prevDrawerOpenRef.current;
    prevDrawerOpenRef.current = drawerOpen;
    if (wasOpen && !drawerOpen) {
      drawerButtonRef.current?.focus();
    }
  }, [drawerOpen]);

  // The level-0 leading-edge swipe-to-open zone (mirrors the 13.2 back-swipe
  // pointer math): a leading→trailing drag past half the width, or a rightward
  // flick, opens the drawer. Reserved for level 0 only — level ≥ 1's leading edge
  // is the back-swipe. The zone starts *below* the header so it never overlaps the
  // avatar drawer button's 44pt hit area (the top-left corner is the tap
  // affordance; the list's leading edge is the swipe affordance).
  const openPointerRef = useRef<{
    pointerId: number;
    startX: number;
    startT: number;
    width: number;
  } | null>(null);

  const onOpenEdgePointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (openPointerRef.current !== null) {
      return;
    }
    const rectWidth = containerRef.current?.getBoundingClientRect().width ?? 0;
    const width = rectWidth > 0 ? rectWidth : window.innerWidth;
    openPointerRef.current = {
      pointerId: e.pointerId,
      startX: e.clientX,
      startT: e.timeStamp,
      width,
    };
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const onOpenEdgePointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    const pointer = openPointerRef.current;
    if (pointer === null || e.pointerId !== pointer.pointerId) {
      return;
    }
    openPointerRef.current = null;
    const dx = clampDx(e.clientX - pointer.startX, pointer.width);
    const dt = Math.max(e.timeStamp - pointer.startT, 1);
    const flick = dx > FLICK_MIN_DX_PX && dx / dt > FLICK_VELOCITY_PX_PER_MS;
    if (dx > pointer.width * 0.5 || flick) {
      leadingDrawerStore.getState().open();
    }
  };

  const onOpenEdgePointerCancel = (e: ReactPointerEvent<HTMLDivElement>) => {
    const pointer = openPointerRef.current;
    if (pointer === null || e.pointerId !== pointer.pointerId) {
      return;
    }
    openPointerRef.current = null;
  };

  // ---- Level-0 pull-down: open Search / refresh (Stories 13.4 + 13.6) -----
  // One continuous gesture axis, mirroring the drawer-open pointer-threshold
  // math vertically: a downward pull that starts while the Inbox list is
  // scrolled to top and is released in `[reveal, refresh)` (or flicks) opens the
  // Search surface; released at ≥ the refresh threshold it kicks the sync loop
  // instead (Story 13.6). Below the reveal threshold it snaps back with no
  // action, and a pull that starts scrolled away from the top is left to native
  // scrolling (armed === false).
  const searchSurfaceOpen = useSearchSurfaceStore((s) => s.isOpen);
  const magnifierRef = useRef<HTMLButtonElement>(null);
  const offline = useShellOffline();
  // The live pull distance while an armed pull drags (drives the indicator's
  // affordance switch), and the post-release refresh spinner.
  const [pullDy, setPullDy] = useState<number | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const pullPointerRef = useRef<{
    pointerId: number;
    startY: number;
    startT: number;
    armed: boolean;
  } | null>(null);

  const onPullPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (pullPointerRef.current !== null) {
      return;
    }
    // Arm only when the Inbox list is at its top (native scroll otherwise). The
    // scroll viewport is the shadcn ScrollArea's; absent (empty/loading) counts as
    // "at top" so the gesture still opens Search from an empty inbox.
    const viewport = containerRef.current?.querySelector<HTMLElement>(
      '[data-slot="scroll-area-viewport"]',
    );
    const atTop = viewport === null || viewport === undefined || viewport.scrollTop <= 0;
    if (!atTop) {
      // Not at the top: native scroll owns this gesture. Tracking an uncaptured
      // pointer here would strand `pullPointerRef` if the finger then lifts off
      // the thin pull band (no `pointerup` reaches the zone), and the guard above
      // would kill every later pull. Leave the ref null so scrolling and future
      // pulls both keep working.
      return;
    }
    pullPointerRef.current = {
      pointerId: e.pointerId,
      startY: e.clientY,
      startT: e.timeStamp,
      armed: true,
    };
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const onPullPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const pointer = pullPointerRef.current;
    if (pointer === null || e.pointerId !== pointer.pointerId || !pointer.armed) {
      return;
    }
    setPullDy(Math.max(e.clientY - pointer.startY, 0));
  };

  const onPullPointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    const pointer = pullPointerRef.current;
    if (pointer === null || e.pointerId !== pointer.pointerId) {
      return;
    }
    pullPointerRef.current = null;
    setPullDy(null);
    if (!pointer.armed) {
      return;
    }
    const dy = e.clientY - pointer.startY;
    // Past the refresh threshold the release refreshes instead of opening
    // Search (Story 13.6): best-effort `syncNow()` resumes each active
    // account's SyncService. An IpcError is swallowed and only clears the
    // spinner — never an error toast; a fully offline session resolves the
    // spinner into the persistent offline pill below.
    if (dy >= PULL_REFRESH_THRESHOLD_PX) {
      // Guard against a second qualifying pull re-firing `syncNow()` and
      // re-arming the spinner timeout while a refresh is already in flight.
      if (!refreshing) {
        setRefreshing(true);
        void syncNow().catch(() => setRefreshing(false));
      }
      return;
    }
    const dt = Math.max(e.timeStamp - pointer.startT, 1);
    const flick = dy > FLICK_MIN_DX_PX && dy / dt > FLICK_VELOCITY_PX_PER_MS;
    if (dy > PULL_REVEAL_THRESHOLD_PX || flick) {
      searchSurfaceStore.getState().open();
    }
  };

  const onPullPointerCancel = (e: ReactPointerEvent<HTMLDivElement>) => {
    const pointer = pullPointerRef.current;
    if (pointer === null || e.pointerId !== pointer.pointerId) {
      return;
    }
    pullPointerRef.current = null;
    setPullDy(null);
  };

  // The refresh spinner clears on the next connection-status tick (the streamed
  // Rust status is the honest "sync answered" signal), with a timeout ceiling so
  // a tickless offline session never strands a spinner.
  useEffect(() => {
    if (!refreshing) {
      return;
    }
    const unsubscribe = accountStatusStore.subscribe(() => setRefreshing(false));
    const timeout = window.setTimeout(() => setRefreshing(false), REFRESH_SPINNER_TIMEOUT_MS);
    return () => {
      unsubscribe();
      window.clearTimeout(timeout);
    };
  }, [refreshing]);

  // UX-DR28: return focus to the Inbox magnifier when the Search surface
  // transitions open → closed. Radix restores focus to the opener it captured, but
  // the pull-down opens the surface with no focused trigger — so re-focus the
  // magnifier here for that path (a magnifier-opened close is already covered by
  // radix; re-focusing the same element is a harmless no-op).
  const prevSearchOpenRef = useRef(searchSurfaceOpen);
  useEffect(() => {
    const wasOpen = prevSearchOpenRef.current;
    prevSearchOpenRef.current = searchSurfaceOpen;
    if (wasOpen && !searchSurfaceOpen) {
      magnifierRef.current?.focus();
    }
  }, [searchSurfaceOpen]);

  // The level-0 pull-down zone: a thin band across the top of the Inbox list,
  // below the header, that arms the pull-to-open-Search / pull-to-refresh
  // gesture axis. Below-threshold releases and pulls that start scrolled-away
  // no-op (native scroll).
  const pullDownZone = (
    <div
      aria-hidden="true"
      data-testid="pull-down-search"
      className="absolute top-[calc(var(--safe-top)+var(--phone-header))] right-0 left-5 z-10 h-6 touch-none"
      onPointerDown={onPullPointerDown}
      onPointerMove={onPullPointerMove}
      onPointerUp={onPullPointerUp}
      onPointerCancel={onPullPointerCancel}
      onLostPointerCapture={onPullPointerCancel}
    />
  );

  // The pull indicator (Story 13.6): appears once the drag crosses the Search
  // reveal band and switches affordance at the refresh threshold; after a
  // refreshing release it stays as the spinner until the next status tick — or,
  // when every signed-in account is offline, resolves into the persistent
  // offline pill (same copy as the sidebar pill; never an error toast).
  const showSpinner = refreshing || (pullDy !== null && pullDy >= PULL_REFRESH_THRESHOLD_PX);
  const pullIndicator =
    refreshing || (pullDy !== null && pullDy >= PULL_REVEAL_THRESHOLD_PX) ? (
      <div
        data-testid="pull-indicator"
        className="pointer-events-none absolute top-[calc(var(--safe-top)+var(--phone-header))] right-0 left-0 z-10 flex justify-center pt-2"
      >
        {refreshing && offline ? (
          <div
            role="status"
            data-testid="pull-offline-pill"
            className="flex items-center gap-2 rounded-full bg-held/10 px-3 py-1.5 text-held text-xs shadow-xs"
          >
            <WifiOff aria-hidden="true" className="size-4 shrink-0" />
            <span>{OFFLINE_PILL_TEXT}</span>
          </div>
        ) : showSpinner ? (
          <div
            role="status"
            aria-label="Refreshing"
            data-testid="pull-refresh-spinner"
            className="rounded-full border border-border bg-background p-1.5 shadow-xs"
          >
            <RefreshCw
              aria-hidden="true"
              className={cn("size-4 text-muted-foreground", !reducedMotion && "animate-spin")}
            />
          </div>
        ) : (
          <div
            data-testid="pull-release-search"
            className="rounded-full border border-border bg-background px-3 py-1.5 text-muted-foreground text-xs shadow-xs"
          >
            Release to search
          </div>
        )}
      </div>
    ) : null;

  // The stale-resume "Connecting…" pill (Story 14.4): a quiet transient indicator
  // under the Inbox header while a resumed sync is still answering. Hidden while
  // an ACTUAL refresh spinner is in flight (`refreshing`, which says the same
  // thing) and while genuinely offline (`offline` — "Connecting…" would be
  // dishonest when there is no connection; the offline surface owns that state,
  // Review R2). Also suppressed during an active pull gesture (`pullDy`), whose
  // band shares this exact slot — so the reconnect pill and the pull affordance
  // never overlap; a passive resume with no gesture still shows it (Review R2).
  // Clears on the sync answering or its own timeout backstop — never stuck.
  const connecting = useStaleResumePill();
  const connectingPill =
    connecting && !refreshing && !offline && pullDy === null ? (
      <div
        data-testid="stale-resume-band"
        className="pointer-events-none absolute top-[calc(var(--safe-top)+var(--phone-header))] right-0 left-0 z-10 flex justify-center pt-2"
      >
        <div
          role="status"
          data-testid="stale-resume-pill"
          className="rounded-full bg-held/10 px-3 py-1.5 text-held text-xs shadow-xs"
        >
          Connecting…
        </div>
      </div>
    ) : null;

  const drawerOpenZone = (
    <div
      aria-hidden="true"
      data-testid="edge-swipe-open"
      className="absolute top-[calc(var(--safe-top)+var(--phone-header))] bottom-0 left-0 z-10 w-5 touch-none"
      onPointerDown={onOpenEdgePointerDown}
      onPointerUp={onOpenEdgePointerUp}
      onPointerCancel={onOpenEdgePointerCancel}
      onLostPointerCapture={onOpenEdgePointerCancel}
    />
  );

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
        {/* Level 0's leading edge opens the drawer (the edge 13.2 reserved); the
            top band pulls down to open Search (Story 13.4). */}
        {level === 0 && drawerOpenZone}
        {level === 0 && pullDownZone}
        {level === 0 && pullIndicator}
        {level === 0 && connectingPill}
        <PhoneInboxHeader drawerButtonRef={drawerButtonRef} magnifierRef={magnifierRef} />
        <div className="flex min-h-0 min-w-0 flex-1">
          <ChatListPane />
        </div>
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
      {/* The always-mounted leading nav drawer (Story 13.3); a portalled Sheet,
          so it renders outside the stack's transform layers. */}
      <LeadingDrawer />
      {/* The always-mounted merged full-screen Search surface (Story 13.4); a
          portalled Dialog, store-driven, so it renders outside the transform
          layers and never mounts on the desktop tier (this shell is phone-only). */}
      <PhoneSearchSurface />
    </div>
  );
}
