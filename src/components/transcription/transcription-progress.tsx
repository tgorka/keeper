/**
 * How far a transcription job has got, drawn the same way wherever a job shows:
 * the job strip, the menu verb's toast, a Recordings row (the bar) and a Files
 * row's Transcribe control (the ring).
 *
 * The shell sends a heartbeat every second while a job runs, carrying its
 * weighted estimate (`fraction`) and the time the job has taken so far. The
 * estimate is the shell's, and it never goes backwards there, so nothing here
 * smooths or remembers it: a batch without one (the store's own `queued`
 * placeholder, before the shell has answered) draws a pulsing, indeterminate
 * bar rather than a guessed zero.
 */
import { Progress } from "@/components/ui/progress";
import { formatElapsed } from "@/hooks/use-recording-session";
import type { TranscriptionProgressVm } from "@/lib/ipc/client";
import { transcriptionJobLine } from "@/lib/stores/transcription";
import { cn } from "@/lib/utils";

/** The bar's accessible name. */
export const TRANSCRIPTION_PROGRESS_LABEL = "Transcription progress";

/** The shell's estimate as a whole percentage, or null when it has none yet. */
export function transcriptionPercent(job: TranscriptionProgressVm): number | null {
  if (job.fraction === null) return null;
  return Math.round(Math.min(1, Math.max(0, job.fraction)) * 100);
}

/** The phase and the percentage, short enough for a menu item or a tooltip. */
export function transcriptionShortLine(job: TranscriptionProgressVm): string {
  const percent = transcriptionPercent(job);
  return `Transcription: ${job.phase}${percent === null ? "" : ` · ${percent}%`}`;
}

/**
 * The bar, the job's line and the elapsed time, for a running job.
 *
 * `announce` makes the line a live region, where the surface has none of its
 * own (a toast is one already). Only the line: the elapsed time changes every
 * second, and a region that spoke it would never be quiet.
 */
export function TranscriptionProgress({
  job,
  announce = false,
  className,
}: {
  job: TranscriptionProgressVm;
  announce?: boolean;
  className?: string;
}) {
  const percent = transcriptionPercent(job);
  const line = transcriptionJobLine(job);
  return (
    <div className={cn("flex min-w-0 flex-col gap-1", className)}>
      <Progress
        // The shadcn wrapper spends `value` on the indicator's width and never
        // hands it to Radix, so the value semantics are stated here.
        value={percent ?? 100}
        aria-label={TRANSCRIPTION_PROGRESS_LABEL}
        aria-valuenow={percent ?? undefined}
        aria-valuetext={percent === null ? line : `${percent}% · ${line}`}
        className={cn(percent === null && "motion-safe:animate-pulse")}
      />
      <p className="flex min-w-0 flex-wrap gap-x-1 text-muted-foreground text-xs">
        <span role={announce ? "status" : undefined} className="break-words">
          {line}
        </span>
        <span aria-hidden="true">·</span>
        <span className="figures">{formatElapsed(job.elapsedMs)}</span>
      </p>
    </div>
  );
}

/** The ring's geometry, in the SVG's own 20×20 box. */
const RING_RADIUS = 8.5;
const RING_LENGTH = 2 * Math.PI * RING_RADIUS;

/**
 * A ring drawn round an icon control while its job runs: the Files row's
 * Transcribe, whose control is a 28px icon with no room for a bar.
 * `fraction` null spins a quarter arc, the ring's indeterminate state.
 */
export function TranscriptionProgressRing({ fraction }: { fraction: number | null }) {
  const shown = fraction === null ? 0.25 : Math.min(1, Math.max(0, fraction));
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 20 20"
      className={cn(
        "-rotate-90 pointer-events-none absolute inset-0 size-full",
        fraction === null && "motion-safe:animate-spin",
      )}
    >
      <circle
        cx="10"
        cy="10"
        r={RING_RADIUS}
        fill="none"
        strokeWidth="1.5"
        className="stroke-muted"
      />
      <circle
        cx="10"
        cy="10"
        r={RING_RADIUS}
        fill="none"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeDasharray={RING_LENGTH}
        strokeDashoffset={RING_LENGTH * (1 - shown)}
        className="stroke-primary transition-[stroke-dashoffset]"
      />
    </svg>
  );
}
