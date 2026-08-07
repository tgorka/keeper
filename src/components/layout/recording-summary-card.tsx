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
 *
 * Story 40.4 makes the title line an affordance: the session can be named (or
 * renamed, or un-named) right here, and because the path template names the
 * folder from the title, saving MOVES the session on disk. The card therefore
 * re-renders BOTH the title and the mono folder line from what
 * `recordingRetitle` resolves — the path it was called with no longer exists,
 * and Reveal in Finder points at that line. A refused rename (a session that is
 * still recording, an exhausted ordinal run) prints the Rust-authored sentence
 * beside the field it is about, with the typed text left standing to be
 * corrected — the same treatment the Destination card gives a refused template.
 */
import { useEffect, useId, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import type { RecordingSummaryVm } from "@/lib/ipc/client";
import { recordingRetitle, revealPath } from "@/lib/ipc/client";
import { formatSize } from "@/lib/recording-format";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/** The Reveal-in-Finder control's label (recording voice, matches export). */
export const REVEAL_IN_FINDER_LABEL = "Reveal in Finder";

/** The recovery card's Dismiss control label (latches the one-time notice). */
export const RECOVERY_DISMISS_LABEL = "Dismiss";

/** The rename affordance's label on a session that already has a title. */
export const SUMMARY_RETITLE_LABEL = "Rename";

/** The same affordance on an untitled session — a prompt, not a verb. Doubles
 * as the field's placeholder, so the invitation reads the same either way. */
export const SUMMARY_RETITLE_UNTITLED_LABEL = "Name this recording";

/** The rename editor's commit affordance ("Save" alone would read as the
 * recording being saved, which happened already). */
export const SUMMARY_RETITLE_SAVE_LABEL = "Save name";

/** The rename editor's discard affordance. */
export const SUMMARY_RETITLE_CANCEL_LABEL = "Cancel";

/** Accessible name for the rename field ("Title" is the manifest's word). */
export const SUMMARY_RETITLE_FIELD_LABEL = "Session title";

/** Last-resort message when a rename rejection carries no readable sentence. */
export const SUMMARY_RETITLE_UNKNOWN_ERROR = "keeper could not rename this recording.";

/** Test id for the mono session-folder line (the Reveal-in-Finder target). */
export const SUMMARY_FOLDER_TESTID = "recording-summary-folder";

/** Test id for the rename affordance that opens the editor. */
export const SUMMARY_RETITLE_EDIT_TESTID = "recording-retitle-edit";

/** Test id for the rename field. */
export const SUMMARY_RETITLE_FIELD_TESTID = "recording-retitle-field";

/** Test id for the rename editor's commit affordance. */
export const SUMMARY_RETITLE_SAVE_TESTID = "recording-retitle-save";

/** Test id for the rename editor's discard affordance. */
export const SUMMARY_RETITLE_CANCEL_TESTID = "recording-retitle-cancel";

/** Test id for the inline fault line (a Rust-composed refusal, verbatim). */
export const SUMMARY_RETITLE_FAULT_TESTID = "recording-retitle-fault";

/** The completion / recovery card variants (same shape, distinct edge + copy). */
export type RecordingSummaryVariant = "completion" | "recovered";

export interface RecordingSummaryCardProps {
  /** Which variant to render — completion (plain) or recovered (warning edge). */
  variant: RecordingSummaryVariant;
  /** The session folder path (mono line + Reveal-in-Finder / bytes fallback).
   * A rename supersedes this with the folder the session moved to. */
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
  /** The recovery card's Dismiss handler — latches the one-time notice. Called
   * with the folder the session is at NOW: a rename moves it, and only the
   * post-move path still has a manifest to key the latch off (Story 40.4). Omit
   * on the completion variant (a finalized session is never dismissed). */
  onDismiss?: (folder: string) => void;
  /** Called with the summary a rename resolved, whose `sessionFolder` is the
   * session's NEW path (Story 40.4). The card's own override dies with the next
   * unmount, so the surface that OWNS the path — the recording-session snapshot
   * behind the completion card, the recovery list behind a recovered one — must
   * adopt the move here or a remount will paint a folder that no longer
   * exists. */
  onRetitled?: (summary: RecordingSummaryVm) => void;
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
  onRetitled,
}: RecordingSummaryCardProps) {
  const canReveal = useCapabilitiesStore((s) => s.capabilities.revealInFileManager);
  const recovered = variant === "recovered";
  // A stable per-card id: two recovery cards can be open at once, so the fault
  // the field points at with `aria-describedby` must be this card's.
  const faultId = useId();

  // What a successful rename resolved, kept beside the folder it moved FROM.
  // It outranks the props for everything the move re-derived, because the
  // surface above may still hold the pre-rename path for a round trip (the
  // completion card is mounted from the live status's `outputPath`) and the
  // card must never point Reveal at a folder that no longer exists.
  const [moved, setMoved] = useState<{ from: string; summary: RecordingSummaryVm } | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  // A Rust-authored refusal, rendered verbatim beside the field it is about.
  const [refusal, setRefusal] = useState<string | null>(null);

  // The override describes exactly one session, known by both the folder it was
  // at and the folder it moved to — the owner adopts the latter from the summary
  // handed up by `onRetitled`, so the prop legitimately becomes the destination.
  // ANY other folder means the pane reused this card slot for a different
  // session, and the override (plus any half-finished edit) belongs to a session
  // that has left the card.
  const override =
    moved !== null &&
    (sessionFolder === moved.from || sessionFolder === moved.summary.sessionFolder)
      ? moved.summary
      : null;
  useEffect(() => {
    if (moved !== null && override === null) {
      setMoved(null);
      setEditing(false);
      setRefusal(null);
    }
  }, [moved, override]);

  const effectiveFolder = override?.sessionFolder ?? sessionFolder;
  const effectiveTitle = override === null ? (title ?? null) : override.title;
  // The rename resolved the manifest's own figures for the moved session, so a
  // successful move never leaves the authoritative count/size behind in favour
  // of a prop that is `null` while the owner re-fetches the new path.
  const effectiveCount = override === null ? screenSegmentCount : override.screenSegmentCount;
  const effectiveBytes = override === null ? totalBytes : override.totalBytes;
  const named = effectiveTitle !== null && effectiveTitle !== "";

  // The title arrives one IPC round trip after the completion card mounts, so an
  // editor opened inside that window seeds an EMPTY draft over a title the user
  // never saw — and an empty draft is a clear, which moves the folder back to
  // the untitled path. Re-seed the untouched draft when the real title lands
  // (a draft the user has since typed into is their edit and stands).
  const seededWith = useRef("");
  useEffect(() => {
    if (!editing) {
      return;
    }
    const loaded = effectiveTitle ?? "";
    if (seededWith.current === loaded) {
      return;
    }
    setDraft((current) => (current === seededWith.current ? loaded : current));
    seededWith.current = loaded;
  }, [editing, effectiveTitle]);

  // Focus follows the editor across the affordance↔field swap, the way the
  // approval row's inline body editor does (approval-pane.tsx: focus the field
  // on the not-editing→editing transition) and the way the shell returns focus
  // to the control that opened a surface on close (app-shell.tsx `closeDetail`).
  // Without it both transitions drop focus to `document.body`.
  const fieldRef = useRef<HTMLInputElement>(null);
  const openRef = useRef<HTMLButtonElement>(null);
  const wasEditing = useRef(false);
  useEffect(() => {
    if (editing && !wasEditing.current) {
      fieldRef.current?.focus();
    } else if (!editing && wasEditing.current) {
      openRef.current?.focus();
    }
    wasEditing.current = editing;
  }, [editing]);

  const typed = draft.trim();
  // Nothing to send when the trimmed text is already the session's title (and
  // an empty field on an untitled session is exactly that case), and nothing to
  // send twice while a rename is in flight.
  const saveDisabled = saving || typed === (effectiveTitle ?? "");

  /** Commit the typed title; print a refusal in the fault slot and keep it. */
  const save = async () => {
    setSaving(true);
    setRefusal(null);
    try {
      // A cleared field is `null`, not `""`: clearing the title is a real edit,
      // and it moves the session back to its untitled path.
      const summary = await recordingRetitle(effectiveFolder, typed === "" ? null : typed);
      // Keyed on the folder this card is currently mounted for, so the override
      // still recognises the session once the owner adopts the new path.
      setMoved({ from: sessionFolder, summary });
      setEditing(false);
      onRetitled?.(summary);
    } catch (raw) {
      // `syncErrorMessage`, never `String(raw)`: an IPC rejection is a
      // `{ code, message }` object, and stringifying one prints
      // "[object Object]" exactly where the Rust-authored reason belongs. The
      // editor stays open on the typed text — a refusal the user can act on
      // ("stop the recording first") is worthless once their words are gone.
      setRefusal(syncErrorMessage(raw, SUMMARY_RETITLE_UNKNOWN_ERROR));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card
      size="sm"
      className={cn(recovered && "border-bridge-degraded/50 text-bridge-degraded ring-0 border")}
    >
      <CardContent className="flex flex-col gap-3">
        {/* The rename editor sits OUTSIDE the live region below: a text field
            and its buttons inside an aria-atomic `role="status"` re-announce the
            whole card on every keystroke — the same reason the active-recording
            banner keeps its ticking elapsed out of one. */}
        <div className="flex flex-col gap-1">
          {editing ? (
            <div className="flex items-center gap-1">
              <Input
                ref={fieldRef}
                className="h-8 text-sm"
                data-testid={SUMMARY_RETITLE_FIELD_TESTID}
                aria-label={SUMMARY_RETITLE_FIELD_LABEL}
                aria-invalid={refusal !== null}
                aria-describedby={refusal === null ? undefined : faultId}
                placeholder={SUMMARY_RETITLE_UNTITLED_LABEL}
                value={draft}
                disabled={saving}
                onChange={(event) => {
                  setDraft(event.target.value);
                  // A refusal is about the text that was sent; retract it (and
                  // its `aria-invalid`) as the user edits toward a correction.
                  setRefusal(null);
                }}
              />
              <Button
                type="button"
                size="xs"
                variant="outline"
                className="shrink-0"
                data-testid={SUMMARY_RETITLE_SAVE_TESTID}
                disabled={saveDisabled}
                onClick={() => {
                  void save();
                }}
              >
                {SUMMARY_RETITLE_SAVE_LABEL}
              </Button>
              <Button
                type="button"
                size="xs"
                variant="ghost"
                className="shrink-0"
                data-testid={SUMMARY_RETITLE_CANCEL_TESTID}
                disabled={saving}
                onClick={() => {
                  setEditing(false);
                  setRefusal(null);
                }}
              >
                {SUMMARY_RETITLE_CANCEL_LABEL}
              </Button>
            </div>
          ) : (
            <div className="flex items-center gap-2">
              {named && <p className="font-medium text-sm">{effectiveTitle}</p>}
              <Button
                ref={openRef}
                type="button"
                size="xs"
                variant="ghost"
                className="shrink-0"
                data-testid={SUMMARY_RETITLE_EDIT_TESTID}
                onClick={() => {
                  // Seed from what is on screen, so a rename starts from the
                  // current name rather than blanking it. `seededWith` records
                  // what that was, so a title still in flight can re-seed the
                  // untouched draft when it lands.
                  seededWith.current = effectiveTitle ?? "";
                  setDraft(seededWith.current);
                  setRefusal(null);
                  setEditing(true);
                }}
              >
                {named ? SUMMARY_RETITLE_LABEL : SUMMARY_RETITLE_UNTITLED_LABEL}
              </Button>
            </div>
          )}
          {refusal !== null && (
            <p
              id={faultId}
              role="alert"
              className="text-destructive text-xs"
              data-testid={SUMMARY_RETITLE_FAULT_TESTID}
            >
              {refusal}
            </p>
          )}
        </div>
        {/* The announced outcome: what was saved and where it is. Non-interactive
            by contract — see the editor note above. */}
        <div role="status" className="flex flex-col gap-3">
          <p className="text-sm">{summaryLine(variant, effectiveCount, effectiveBytes)}</p>
          <p
            className="break-all font-mono text-muted-foreground text-xs"
            data-testid={SUMMARY_FOLDER_TESTID}
          >
            {effectiveFolder}
          </p>
        </div>
        <div className="flex items-center gap-2">
          {canReveal && (
            <Button
              type="button"
              size="sm"
              onClick={() => {
                void revealPath(effectiveFolder);
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
              onClick={() => {
                // The folder the session is at NOW: a dismissal keyed on the
                // path a rename invalidated loads no manifest and latches
                // nothing, so the card returns on the next scan.
                onDismiss(effectiveFolder);
              }}
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
