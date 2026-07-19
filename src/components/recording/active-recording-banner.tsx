/**
 * The in-app active-recording banner + segment meter (Story 18.3, Epic 18) — the
 * pinned, persistent twin of the menu-bar tray for when the user is looking at
 * the Recording view instead of the menu bar.
 *
 * A pure renderer of the Rust-owned {@link RecordingStatusVm} snapshot plus the
 * client-computed `elapsed` string: it never invents, estimates, or duplicates
 * recording state (size and cap come from the VM; elapsed is the hook's reused
 * `formatElapsed`). It renders **only** while the session is live
 * (preflight/recording/rotating/stopping) — any terminal/idle state renders
 * `null` (the pane's header carries those notes).
 *
 * The warning variant (Story 19.4): when `status.warning` is set (a sticky,
 * non-fatal session warning — e.g. a microphone hot-unplug), the left edge
 * turns amber and a persistent, non-dismissible warning line renders under the
 * live row. It never auto-clears — the Rust snapshot owns the slot (reset only
 * when a new session starts) and this stays a pure renderer of it. The full
 * loud-failure triad (native notification leg) remains Story 18.4.
 *
 * Chrome: a recording-red 3px left edge, a reduced-motion-aware record dot,
 * "Recording", a monospace `elapsed · segment · size` line, and a
 * destructive-styled Stop button (never recording-red — the two reds stay
 * distinct). Below sits the segment meter: a bar filling toward the
 * session-captured cap, captioned `segment N · used / cap MB`, hidden when the
 * cap is 0 (defensive — no NaN/∞ fraction).
 *
 * Accessibility: recording state is announced **assertively** via a dedicated
 * `sr-only` live region keyed on state + segment (so it fires on
 * start/stop/rotation, never once per second); the ticking mono line is kept out
 * of any live region. Stop is an explicit focusable button — `Esc` never stops.
 */
import { Button } from "@/components/ui/button";
import { isLiveRecording } from "@/hooks/use-recording-session";
import { useReducedMotion } from "@/hooks/use-reduced-motion";
import type { RecordingStatusVm } from "@/lib/ipc/client";
import { bytesToWholeMb, formatSize } from "@/lib/recording-format";
import { cn } from "@/lib/utils";

/** The banner's Stop affordance label (recording voice; shared wording with the pane). */
export const BANNER_STOP_LABEL = "Stop";

/** The label shown on the Stop button while a graceful stop is in flight. */
export const BANNER_STOPPING_LABEL = "Stopping…";

export interface ActiveRecordingBannerProps {
  /** The live session snapshot (the enriched Rust-owned view model). */
  status: RecordingStatusVm;
  /** The ticking `H:MM:SS` / `M:SS` elapsed line (client-computed), or `null`. */
  elapsed: string | null;
  /** Fire the idempotent graceful stop-and-finalize (identical to the tray's Stop). */
  onStop: () => void;
}

/** One decimal megabyte, in bytes — the meter denominator's unit (`10^6`). */
const BYTES_PER_MB = 1_000_000;

export function ActiveRecordingBanner({ status, elapsed, onStop }: ActiveRecordingBannerProps) {
  const reducedMotion = useReducedMotion();

  // Render nothing on any terminal/idle state — the banner is a live-only
  // surface (the mic-loss warning, Story 19.4, is a live-session state too;
  // terminal error surfaces are Story 18.4).
  if (!isLiveRecording(status)) {
    return null;
  }

  // The segment currently being written is one past the closed count.
  const segment = status.segmentsClosed + 1;
  const stopping = status.state === "stopping";

  // The meter is hidden when the session-captured cap is 0 (defensive — never a
  // NaN/∞ fraction). The denominator is the VM's cap, never the settings store.
  const showMeter = status.segmentCapMb > 0;
  const capBytes = status.segmentCapMb * BYTES_PER_MB;
  const fraction = showMeter ? Math.min(1, Math.max(0, status.currentSegmentBytes / capBytes)) : 0;
  const usedMb = bytesToWholeMb(status.currentSegmentBytes);

  // The sticky, non-dismissible session warning (Story 19.4): amber left
  // edge + a persistent warning line. `held` is the app's amber token (the
  // same one the denied-permission captions use).
  const warning = status.warning;

  return (
    <div
      data-slot="active-recording-banner"
      className={cn(
        "shrink-0 border-l-[3px] bg-card px-6 py-3",
        warning === null ? "border-recording-red" : "border-held",
      )}
    >
      {/* Assertive announcement of state + segment only (never the per-second
          elapsed): keyed content changes on start/stop/rotation. */}
      <span aria-live="assertive" className="sr-only">
        {`Recording, segment ${segment}`}
      </span>

      <div className="flex items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-3">
          {/* The live record dot: steady (never pulsing) under reduced motion. */}
          <span
            aria-hidden="true"
            data-testid="recording-dot"
            className={cn(
              "size-2.5 shrink-0 rounded-full bg-recording-red",
              !reducedMotion && "animate-pulse",
            )}
          />
          <span className="font-medium text-sm">Recording</span>
          <span className="truncate font-mono text-muted-foreground text-sm tabular-nums">
            {`${elapsed ?? "0:00"} · segment ${segment} · ${formatSize(status.onDiskBytes)}`}
          </span>
        </div>

        <Button type="button" variant="destructive" disabled={stopping} onClick={onStop}>
          {stopping ? BANNER_STOPPING_LABEL : BANNER_STOP_LABEL}
        </Button>
      </div>

      {/* The persistent warning line (Story 19.4): non-dismissible (no close
          affordance), announced as an alert once when it appears, and rendered
          for as long as the snapshot carries it (i.e. the rest of the session). */}
      {warning !== null && (
        <p
          role="alert"
          data-testid="recording-warning"
          className="mt-1.5 flex items-center gap-1.5 text-held text-xs"
        >
          <span aria-hidden="true">⚠</span>
          {warning}
        </p>
      )}

      {showMeter && (
        <div className="mt-2 flex flex-col gap-1">
          <div
            role="progressbar"
            aria-label="Segment size"
            aria-valuemin={0}
            aria-valuemax={status.segmentCapMb}
            // Clamp to the cap so assistive tech never announces ">100%" when the
            // open segment momentarily overshoots the cap before rotation; the
            // honest over-cap figure still shows in the visible caption below.
            aria-valuenow={Math.min(usedMb, status.segmentCapMb)}
            aria-valuetext={`${usedMb} / ${status.segmentCapMb} MB`}
            className="h-1.5 w-full overflow-hidden rounded-full bg-muted"
          >
            <div
              className="h-full rounded-full bg-recording-red transition-all"
              style={{ width: `${fraction * 100}%` }}
            />
          </div>
          <span className="font-mono text-muted-foreground text-xs tabular-nums">
            {`segment ${segment} · ${usedMb} / ${status.segmentCapMb} MB`}
          </span>
        </div>
      )}
    </div>
  );
}
