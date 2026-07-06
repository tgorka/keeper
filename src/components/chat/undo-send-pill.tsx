/**
 * Undo-Send pill(s) (Story 8.3, FR-46, UX-DR6).
 *
 * A floating pill above the composer for each held send in the open Chat, stacked
 * oldest-first. Each pill shows a radial SVG countdown ring computed from
 * `dispatchAtMs - Date.now()` via a shared 1 s interval, labelled "Sending in Ns —
 * Undo". Under `motion-reduce` the ring animation is suppressed and only the numeric
 * seconds render (no `motion-safe:` ring). The remaining count is announced to
 * VoiceOver **once** on mount (`aria-live="polite"`), not per second.
 *
 * Clicking a pill's Undo — or pressing `⌘⇧Z` while the focused Chat has a pending hold
 * (undoes the OLDEST) — calls `cancelHeldSend` and restores the returned body into the
 * composer as a draft (via `composerStore.restore`). `⌘⇧Z` is a LOCAL keydown scoped to
 * this pane (not a global command registry — that is Epic 9); `⌘Z` is left to the
 * composer's own text-undo.
 *
 * Held state is a pure mirror of the Rust `outbox` stream (`useHeldSends`); the row
 * disappears from this list when the scheduler dispatches it or the user undoes it.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { HeldSendVm } from "@/lib/ipc/client";
import { undoHeldSend, useHeldSends } from "@/lib/stores/outbox";
import { cn } from "@/lib/utils";

/** Whole seconds remaining until `dispatchAtMs`, clamped at 0 (never negative). */
function secondsLeft(dispatchAtMs: number, now: number): number {
  return Math.max(0, Math.ceil((dispatchAtMs - now) / 1000));
}

interface UndoSendPillProps {
  accountId: string;
  roomId: string;
}

/**
 * The stack of undo-send pills for the open Chat. Renders nothing when the Chat has no
 * held sends. Owns the shared 1 s tick and the `⌘⇧Z` keydown for the oldest hold.
 */
export function UndoSendPill({ accountId, roomId }: UndoSendPillProps) {
  const held = useHeldSends(accountId, roomId);
  // One shared tick drives every pill's countdown so N pills don't run N intervals.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (held.length === 0) {
      return;
    }
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [held.length]);

  const undo = useCallback(
    // The shared undo effect (Story 8.4): cancel is idempotent in Rust (an
    // already-dispatched row returns ""); it only restores on a non-empty body. This is
    // the same helper the timeline Delete affordance on a held bubble calls, so the two
    // cannot drift.
    (id: string) => void undoHeldSend(accountId, roomId, id),
    [accountId, roomId],
  );

  // `⌘⇧Z` undoes the OLDEST pending hold for this Chat. Held is oldest-first, so index 0
  // is the oldest. `⌘Z` is left alone (composer text-undo). The listener is on `window`
  // but scoped to the open Chat: it ignores the keystroke while a modal (e.g. the
  // Settings dialog) holds focus, so `⌘⇧Z` there never silently cancels a held send.
  const heldRef = useRef(held);
  heldRef.current = held;
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.shiftKey && (e.key === "z" || e.key === "Z")) {
        // Don't fire when a modal dialog owns focus — the pill stays mounted under it.
        if (document.activeElement?.closest('[role="dialog"]') != null) {
          return;
        }
        const oldest = heldRef.current[0];
        if (oldest !== undefined) {
          e.preventDefault();
          void undo(oldest.id);
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [undo]);

  if (held.length === 0) {
    return null;
  }

  return (
    <div className="mb-2 flex flex-col gap-1.5" data-testid="undo-send-pill-stack">
      {held.map((row) => (
        <PillRow key={row.id} row={row} now={now} onUndo={undo} />
      ))}
    </div>
  );
}

interface PillRowProps {
  row: HeldSendVm;
  now: number;
  onUndo: (id: string) => void;
}

/** Ring geometry: a small circle whose stroke dash encodes the countdown fraction. */
const RING_RADIUS = 8;
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;

function PillRow({ row, now, onUndo }: PillRowProps) {
  const remaining = secondsLeft(row.dispatchAtMs, now);
  // Total window in seconds (held → dispatch), used for the ring's swept fraction.
  const total = useMemo(
    () => Math.max(1, Math.round((row.dispatchAtMs - row.heldAtMs) / 1000)),
    [row.dispatchAtMs, row.heldAtMs],
  );
  const fraction = Math.min(1, Math.max(0, remaining / total));
  // Announce the countdown to VoiceOver ONCE on mount (not per second): a stable
  // per-row live region seeded with the initial remaining seconds. Later ticks update
  // the visible number but not this announced value.
  const announced = useRef(`Sending in ${remaining} seconds`);

  return (
    <div
      className="flex items-center gap-2 self-start rounded-full border border-held/40 bg-held/10 px-3 py-1"
      data-testid="undo-send-pill"
    >
      {/* Announce-once region: seeded on mount, never re-computed per tick. */}
      <span className="sr-only" aria-live="polite">
        {announced.current}
      </span>
      {/* Radial ring under motion-safe; numeric-only under motion-reduce. */}
      <span className="relative inline-flex h-5 w-5 items-center justify-center motion-reduce:hidden">
        <svg className="-rotate-90 h-5 w-5" viewBox="0 0 20 20" aria-hidden="true">
          <circle
            cx="10"
            cy="10"
            r={RING_RADIUS}
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            className="text-held/25"
          />
          <circle
            cx="10"
            cy="10"
            r={RING_RADIUS}
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            className="text-held transition-[stroke-dashoffset] duration-1000 ease-linear"
            strokeDasharray={RING_CIRCUMFERENCE}
            strokeDashoffset={RING_CIRCUMFERENCE * (1 - fraction)}
          />
        </svg>
        <span
          className="absolute inset-0 flex items-center justify-center text-[10px] text-held tabular-nums"
          aria-hidden="true"
        >
          {remaining}
        </span>
      </span>
      <span className={cn("text-held text-xs tabular-nums")}>Sending in {remaining}s</span>
      <button
        type="button"
        className="rounded-full font-medium text-held text-xs underline underline-offset-2 hover:no-underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-held"
        onClick={() => onUndo(row.id)}
        data-testid="undo-send-button"
      >
        Undo
      </button>
    </div>
  );
}
