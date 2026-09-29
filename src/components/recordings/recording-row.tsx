/**
 * One row of the recordings browser (Story 42.3, Epic 42).
 *
 * A pure presentational component in its own file, the way
 * {@link SearchResultList} is: the pane owns the query, the debounce and the
 * results, and this file owns what one session looks like and what you can do
 * to it. Nothing here searches, and nothing here writes.
 *
 * The row says what identifies a session at a glance — a headline, the date,
 * how long it ran, how big it is, its tags, and one glyph for how far its bytes
 * have travelled — and then offers the three actions that get you out of keeper
 * and into the file.
 *
 * Three rules the row exists to keep:
 *
 *   - **Never a blank line.** An untitled session has no title to print, so its
 *     headline becomes its date and its folder (the two things that identify it
 *     when nothing else does) and the meta line drops the date it would
 *     otherwise repeat. A titled session keeps the date on the meta line. One
 *     ternary, and every row names a moment in time.
 *   - **Reveal is absence, not a disabled button.** Where the platform has no
 *     user-visible file manager (`revealInFileManager` off) the affordance is
 *     gone and the session's path renders as inert text instead, exactly as
 *     `SyncFolderPath` and the recording completion card do. A control that
 *     fails on activation is worse than no control.
 *   - **Play only where there is something to play.** A session with no segment
 *     row has no media file, so Rust hands us `playablePath: null` and the
 *     action is absent — the same idiom, one layer down. Transcribe follows it:
 *     present where Rust says the folder has media and is not recording
 *     (`transcribable`) and this Mac can transcribe, and Show transcript only
 *     where a transcript exists (`transcript`, or the job that just wrote one).
 *
 * A running or failed transcription shows under the row as the job strip the
 * Files pane uses — the bar, the phase, the elapsed time, Cancel and Try again —
 * so a session transcribed from here reads the same as one transcribed there.
 *
 * The durability glyph reuses epic 41's exact vocabulary (the constants
 * exported beside `durabilityLabel`) rather than inventing a second set of
 * words for the same four states: the live banner and this row must never word
 * the same promise differently. A `durability` word this file does not know
 * prints nothing at all — a guessed promise about where someone's bytes are is
 * the one thing worse than silence.
 */
import type { LucideIcon } from "lucide-react";
import {
  AudioLines,
  CloudCheck,
  Copy,
  FileText,
  FolderOpen,
  GitCommitHorizontal,
  HardDrive,
  Play,
} from "lucide-react";
import { useState } from "react";
import {
  DURABILITY_COMMITTED_LABEL,
  DURABILITY_LOCAL_LABEL,
  DURABILITY_PUSHED_LABEL,
} from "@/components/recording/active-recording-banner";
import {
  TRANSCRIBE_AGAIN_LABEL,
  TranscribeAgainDialog,
} from "@/components/transcription/transcribe-again";
import { TranscriptionJob } from "@/components/transcription/transcription-job";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { formatElapsed } from "@/hooks/use-recording-session";
import type { RecordingHitVm, TranscriptionProgressVm } from "@/lib/ipc/client";
import { formatSize } from "@/lib/recording-format";

/**
 * The Reveal control's label. Worded identically to `REVEAL_IN_FINDER_LABEL`
 * (the recording completion card) and `SYNC_OPEN_PATH_LABEL` (the sync folder
 * line): one affordance, one wording, wherever it appears.
 */
export const RECORDINGS_REVEAL_LABEL = "Reveal in Finder";

/** The Play control's label — it hands the file to the system handler and stops caring. */
export const RECORDINGS_PLAY_LABEL = "Play";

/** The copy control's label. */
export const RECORDINGS_COPY_ID_LABEL = "Copy session id";

/** What the copy control becomes for as long as the id is on the clipboard. */
export const RECORDINGS_COPIED_LABEL = "Copied";

/** The transcribe control's label. */
export const RECORDINGS_TRANSCRIBE_LABEL = "Transcribe";

/** The control that opens the session's transcript. */
export const RECORDINGS_SHOW_TRANSCRIPT_LABEL = "Show transcript";

/** What a session with no start stamp (a pre-21.5 manifest) says instead of a date. */
export const RECORDINGS_NO_DATE_LABEL = "Date unknown";

/** Test id for the row's one durability glyph — the banner's convention, since
 * the visible part is an icon and the word beside it is `sr-only`. */
export const RECORDINGS_ROW_DURABILITY_TESTID = "recording-row-durability";

/** What one durability word looks like: epic 41's word, and a glyph for it. */
const DURABILITY_GLYPH: Record<string, { label: string; Icon: LucideIcon }> = {
  local: { label: DURABILITY_LOCAL_LABEL, Icon: HardDrive },
  committed: { label: DURABILITY_COMMITTED_LABEL, Icon: GitCommitHorizontal },
  // `verified` reads the same as `pushed` for the reason the banner gives: the
  // extra certainty is not a different promise to the person recording.
  pushed: { label: DURABILITY_PUSHED_LABEL, Icon: CloudCheck },
  verified: { label: DURABILITY_PUSHED_LABEL, Icon: CloudCheck },
};

export interface RecordingRowProps {
  hit: RecordingHitVm;
  /** Whether this platform has a user-visible file manager to reveal into. */
  canReveal: boolean;
  /** Reveal the session's folder (absent when `canReveal` is false). */
  onReveal: (hit: RecordingHitVm) => void;
  /** Hand the session's media file to the system handler. */
  onPlay: (hit: RecordingHitVm) => void;
  /** Whether this Mac can transcribe (the `transcription` capability). */
  canTranscribe: boolean;
  /** This session's transcription job, when one has run since launch. */
  job: TranscriptionProgressVm | undefined;
  /** Start transcribing the session folder; `replace` writes over its transcript. */
  onTranscribe: (hit: RecordingHitVm, replace: boolean) => void;
  /** Open a transcript in the transcript dialog. */
  onShowTranscript: (path: string) => void;
}

export function RecordingRow({
  hit,
  canReveal,
  onReveal,
  onPlay,
  canTranscribe,
  job,
  onTranscribe,
  onShowTranscript,
}: RecordingRowProps) {
  // The transient copy confirmation, held here rather than in the pane so one
  // row's "Copied" cannot survive into the row that replaces it on a re-query.
  const [copied, setCopied] = useState(false);
  const [asking, setAsking] = useState(false);

  const title = hit.title === null || hit.title.trim() === "" ? null : hit.title;
  const dateLabel =
    hit.startedTs === null ? RECORDINGS_NO_DATE_LABEL : new Date(hit.startedTs).toLocaleString();
  // The session folder: the last segment of the path it is stored under.
  const folder =
    hit.relativePath
      .split("/")
      .filter((segment) => segment !== "")
      .pop() ?? hit.relativePath;
  const headline = title ?? `${dateLabel} · ${folder}`;
  const meta = [
    // Already in the headline when the session is untitled — printing it twice
    // would be the row repeating itself.
    title === null ? null : dateLabel,
    hit.durationMs === null ? null : formatElapsed(hit.durationMs),
    formatSize(hit.totalBytes),
  ]
    .filter((part): part is string => part !== null)
    .join(" · ");

  const glyph = DURABILITY_GLYPH[hit.durability];
  // A job that has just finished knows its transcript before the list is read
  // again; the row does not wait for the re-query to offer it.
  const transcript = hit.transcript ?? (job?.phase === "done" ? job.transcriptPath : null);
  // While a job runs or has failed, its strip carries Cancel and Try again, so
  // the row's own Transcribe would be a second button for the same thing.
  const jobShown = job !== undefined && job.phase !== "done" && job.phase !== "cancelled";
  const offerTranscribe = canTranscribe && hit.transcribable && !jobShown;

  return (
    // A `div`, not an `li`: the windowed list (Story 44.10) owns the positioned
    // `li` this sits inside, because the row's box has to be measured
    // independently of the row's own padding.
    <div className="flex flex-col gap-2 rounded-md border border-border px-3 py-2">
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-40 flex-1 flex-col gap-1">
          <span className="truncate font-medium text-foreground text-sm">{headline}</span>
          <span className="figures text-muted-foreground text-xs">{meta}</span>
          {hit.tags.length > 0 && (
            <div className="flex flex-wrap items-center gap-1">
              {hit.tags.map((tag) => (
                <Badge key={tag} variant="secondary">
                  {tag}
                </Badge>
              ))}
            </div>
          )}
          {/* Where Reveal cannot exist the path is still worth knowing — as text
            you can read and copy, not as a control that would refuse. */}
          {!canReveal && (
            <span
              className="truncate font-mono text-muted-foreground text-xs"
              title={hit.relativePath}
            >
              {hit.relativePath}
            </span>
          )}
        </div>

        {/* Wraps rather than squeezing the headline to nothing: five labelled
            controls are wider than a narrow window gives the row. */}
        <div className="flex min-w-0 flex-wrap items-center justify-end gap-1">
          {glyph !== undefined && (
            <span
              data-testid={RECORDINGS_ROW_DURABILITY_TESTID}
              className="mr-1 inline-flex items-center gap-1 text-muted-foreground text-xs"
              title={glyph.label}
            >
              <glyph.Icon aria-hidden="true" className="size-3.5" />
              {/* A glyph alone is a promise nobody can read out; the word rides
                along for assistive tech and for hover. */}
              <span className="sr-only">{glyph.label}</span>
            </span>
          )}
          {hit.playablePath !== null && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              // The visible labels repeat down the list, so each accessible name
              // carries the session it acts on — otherwise every row's Play reads
              // identically to a screen reader walking the list.
              aria-label={`${RECORDINGS_PLAY_LABEL}: ${headline}`}
              onClick={() => onPlay(hit)}
            >
              <Play aria-hidden="true" />
              {RECORDINGS_PLAY_LABEL}
            </Button>
          )}
          {transcript !== null && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              aria-label={`${RECORDINGS_SHOW_TRANSCRIPT_LABEL}: ${headline}`}
              onClick={() => onShowTranscript(transcript)}
            >
              <FileText aria-hidden="true" />
              {RECORDINGS_SHOW_TRANSCRIPT_LABEL}
            </Button>
          )}
          {/* Where a transcript exists the verb is to replace it, and that
              asks first: the corrections in it are about to go. */}
          {offerTranscribe && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              aria-label={`${transcript === null ? RECORDINGS_TRANSCRIBE_LABEL : `${TRANSCRIBE_AGAIN_LABEL}…`}: ${headline}`}
              onClick={() => (transcript === null ? onTranscribe(hit, false) : setAsking(true))}
            >
              <AudioLines aria-hidden="true" />
              {transcript === null ? RECORDINGS_TRANSCRIBE_LABEL : `${TRANSCRIBE_AGAIN_LABEL}…`}
            </Button>
          )}
          {canReveal && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              aria-label={`${RECORDINGS_REVEAL_LABEL}: ${headline}`}
              onClick={() => onReveal(hit)}
            >
              <FolderOpen aria-hidden="true" />
              {RECORDINGS_REVEAL_LABEL}
            </Button>
          )}
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label={`${RECORDINGS_COPY_ID_LABEL}: ${headline}`}
            onClick={() => {
              // Best effort, and deliberately so: a clipboard the browser refuses
              // is not something the person browsing their recordings needs a
              // dialog about. Mirrors the recovery-key card exactly.
              void navigator.clipboard
                ?.writeText(hit.sessionId)
                .then(() => setCopied(true))
                .catch(() => {});
            }}
          >
            <Copy aria-hidden="true" />
            {copied ? RECORDINGS_COPIED_LABEL : RECORDINGS_COPY_ID_LABEL}
          </Button>
        </div>
      </div>
      {jobShown && <TranscriptionJob path={hit.absolutePath} onOpen={onShowTranscript} />}
      <TranscribeAgainDialog
        open={asking}
        onClose={() => setAsking(false)}
        onConfirm={() => onTranscribe(hit, true)}
      />
    </div>
  );
}
