/**
 * The shared recording completion / recovery card (Story 20.3, FR-71/FR-73,
 * UX-DR34).
 *
 * One card shape, two honest variants:
 * - `"completion"` — shown when a session finalizes: "Saved N segment(s) ·
 *   {size}", the session folder in mono, and a primary Reveal in Finder.
 * - `"recovered"` — the same shape with a `bridge-degraded`-tinted warning edge
 *   for a crash-salvaged session: "A recording was interrupted; N segment(s)
 *   were saved". A `bridge-degraded`-tinted Dismiss latches the one-time notice.
 *
 * N and {size} come from the authoritative on-disk manifest (via the summary
 * command), never the live `segmentsClosed` rotation counter. There is NO
 * preview, trim, share, upload, or cloud affordance — recorded files stay
 * exactly as captured (no remux); Reveal opens the folder as-is.
 *
 * The Reveal in Finder button is capability-gated on
 * `capabilities.revealInFileManager` — absent (never a dead affordance) on a
 * platform without a user-visible file manager.
 */
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { revealPath } from "@/lib/ipc/client";
import { formatSize } from "@/lib/recording-format";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { cn } from "@/lib/utils";

/** The Reveal-in-Finder control's label (recording voice, matches export). */
export const REVEAL_IN_FINDER_LABEL = "Reveal in Finder";

/** The recovery card's Dismiss control label (latches the one-time notice). */
export const RECOVERY_DISMISS_LABEL = "Dismiss";

/** The completion / recovery card variants (same shape, distinct edge + copy). */
export type RecordingSummaryVariant = "completion" | "recovered";

export interface RecordingSummaryCardProps {
  /** Which variant to render — completion (plain) or recovered (warning edge). */
  variant: RecordingSummaryVariant;
  /** The session folder path (mono line + Reveal-in-Finder / bytes fallback). */
  sessionFolder: string;
  /** The user session title when one was set (Story 21.5) — rendered as the
   * card's first line; omitted otherwise. */
  title?: string | null;
  /** The manifest-authoritative screen-segment count ("Saved N segments"), or
   * `null` when the summary is unavailable (still loading / manifest load
   * failed) — the card then omits the figures rather than fabricating a zero. */
  screenSegmentCount: number | null;
  /** The manifest-authoritative total on-disk bytes across all tracks, or `null`
   * when the summary is unavailable (see `screenSegmentCount`). */
  totalBytes: number | null;
  /** The recovery card's Dismiss handler — latches the one-time notice. Omit on
   * the completion variant (a finalized session is never dismissed). */
  onDismiss?: () => void;
}

/** "1 segment" / "N segments" — honest singular/plural (recording voice). */
function segmentsLabel(count: number): string {
  return count === 1 ? "1 segment" : `${count} segments`;
}

/**
 * The saved / interrupted headline. Completion states the outcome; recovery
 * names the interruption and what was salvaged. Size only when at least one
 * whole MB reached disk (a sub-MB salvage reads "0 MB" honestly — never a
 * fabricated figure).
 *
 * When the summary is unavailable (`count`/`totalBytes` null — loading or a
 * manifest load failure), the figures are omitted entirely rather than
 * fabricated as "0 segments · 0 MB": the card degrades to a figureless headline
 * plus the folder + Reveal, so it never dishonestly claims nothing was saved.
 */
function summaryLine(
  variant: RecordingSummaryVariant,
  count: number | null,
  totalBytes: number | null,
): string {
  if (count === null || totalBytes === null) {
    return variant === "recovered" ? "A recording was interrupted" : "Recording saved";
  }
  const segments = segmentsLabel(count);
  if (variant === "recovered") {
    return `A recording was interrupted; ${segments} were saved · ${formatSize(totalBytes)}`;
  }
  return `Saved ${segments} · ${formatSize(totalBytes)}`;
}

export function RecordingSummaryCard({
  variant,
  sessionFolder,
  title = null,
  screenSegmentCount,
  totalBytes,
  onDismiss,
}: RecordingSummaryCardProps) {
  const canReveal = useCapabilitiesStore((s) => s.capabilities.revealInFileManager);
  const recovered = variant === "recovered";

  return (
    <Card
      size="sm"
      role="status"
      className={cn(recovered && "border-bridge-degraded/50 text-bridge-degraded ring-0 border")}
    >
      <CardContent className="flex flex-col gap-3">
        {title !== null && title !== "" && <p className="font-medium text-sm">{title}</p>}
        <p className="text-sm">{summaryLine(variant, screenSegmentCount, totalBytes)}</p>
        <p className="break-all font-mono text-muted-foreground text-xs">{sessionFolder}</p>
        <div className="flex items-center gap-2">
          {canReveal && (
            <Button
              type="button"
              size="sm"
              onClick={() => {
                void revealPath(sessionFolder);
              }}
            >
              {REVEAL_IN_FINDER_LABEL}
            </Button>
          )}
          {recovered && onDismiss && (
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={onDismiss}
              className="text-bridge-degraded hover:text-bridge-degraded"
            >
              {RECOVERY_DISMISS_LABEL}
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
