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
 *
 * Story 42.4 makes this the surface where a recording gets written about. The
 * minute a session finalizes is the entire window in which anyone will note
 * what it was FOR, and this card is what is on screen for that minute — so the
 * composed note stub is presented inside it, cursor already in the body
 * (UX-DR51), and Escape closes it. Only on the `completion` variant: a
 * crash-salvaged session surfaces hours later, which is not that minute, and
 * the recovery scan deliberately never resolves a stub for one. The stub is
 * strictly additive — a session whose stub could not be written shows its
 * summary exactly as before, because finalize already succeeded and this card
 * must keep saying so.
 */
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { RecordingMetaFieldSet } from "@/components/recording/recording-meta-fields";
import { RecordingNoteStub } from "@/components/recording/recording-note-stub";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import type { RecordingSessionMetaVm, RecordingSummaryVm } from "@/lib/ipc/client";
import {
  recordingMetaUpdate,
  recordingRetitle,
  recordingSessionMeta,
  revealPath,
} from "@/lib/ipc/client";
import { formatSize } from "@/lib/recording-format";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import type { RecordingMetaFields } from "@/lib/stores/recording-meta";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/** The Reveal-in-Finder control's label (recording voice, matches export). */
export const REVEAL_IN_FINDER_LABEL = "Reveal in Finder";

/** The recovery card's Dismiss control label (latches the one-time notice). */
export const RECOVERY_DISMISS_LABEL = "Dismiss";

/**
 * The details affordance's label on a session that already has a title.
 *
 * It said "Rename" until Story 45.19, and that stopped being true when the
 * editor grew the other four fields the manifest carries. An affordance whose
 * label names one of the five things behind it is how people never find the
 * other four.
 */
export const SUMMARY_RETITLE_LABEL = "Edit details";

/** The same affordance on an untitled session — a prompt, not a verb, and
 * still the name first, because that is what an untitled recording is missing.
 * Doubles as the title field's placeholder, so the invitation reads the same
 * either way. */
export const SUMMARY_RETITLE_UNTITLED_LABEL = "Name this recording";

/** The editor's commit affordance ("Save" alone would read as the recording
 * being saved, which happened already). */
export const SUMMARY_RETITLE_SAVE_LABEL = "Save details";

/** The editor's discard affordance. */
export const SUMMARY_RETITLE_CANCEL_LABEL = "Cancel";

/** Accessible name for the title field ("Title" is the manifest's word). */
export const SUMMARY_RETITLE_FIELD_LABEL = "Session title";

/** Last-resort message when a save rejection carries no readable sentence. */
export const SUMMARY_RETITLE_UNKNOWN_ERROR = "keeper could not save these details.";

/**
 * What the editor says when the session's own `manifest.json` will not load
 * (Story 45.19).
 *
 * The four detail fields are frozen under it rather than hidden, because the
 * alternative is an editor that silently offers four empty boxes over values
 * keeper does not know — and a save from those would be a save of fabricated
 * blanks. The title stays live: it goes through the rename path, which has its
 * own refusal for exactly this and will say so in the same fault slot.
 */
export const SUMMARY_DETAILS_UNAVAILABLE =
  "keeper can't read this recording's details, so only its name can be changed.";

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

/** Test id for the detail field set, so a test can scope inside one card. */
export const SUMMARY_DETAILS_TESTID = "recording-details-fields";

/** Test id for the "keeper cannot read this session" line. */
export const SUMMARY_DETAILS_UNAVAILABLE_TESTID = "recording-details-unavailable";

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

/**
 * The editor's empty seed: what the four detail fields hold before the
 * session's manifest has been read.
 *
 * `title` rides along because {@link RecordingMetaFieldSet} renders all five as
 * one set, but on this surface it is not the field set's to own — the card's
 * own `draft` is the title, because a title change is a rename and takes a
 * different path out.
 */
const EMPTY_DETAILS: RecordingMetaFields = {
  title: "",
  participants: "",
  note: "",
  tags: "",
  custom: [],
};

/** The stored metadata as the field set holds it. */
function detailsOf(meta: RecordingSessionMetaVm): RecordingMetaFields {
  return {
    title: meta.title,
    participants: meta.participants,
    note: meta.note,
    tags: meta.tags,
    custom: meta.custom.map((row) => ({ name: row.name, value: row.value })),
  };
}

/** Whether two custom-row lists say the same thing, in the same order. */
function sameCustom(
  a: readonly { name: string; value: string }[],
  b: readonly { name: string; value: string }[],
): boolean {
  return (
    a.length === b.length &&
    a.every((row, index) => row.name === b[index]?.name && row.value === b[index]?.value)
  );
}

/**
 * Whether two drafts hold the same four details.
 *
 * The TITLE is deliberately not compared: it is the card's own `draft`, it goes
 * out through the rename path, and folding it in here would make a rename look
 * like a details change and send a second write that has nothing to say.
 */
function sameDetails(a: RecordingMetaFields, b: RecordingMetaFields): boolean {
  return (
    a.participants === b.participants &&
    a.note === b.note &&
    a.tags === b.tags &&
    sameCustom(a.custom, b.custom)
  );
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
  // The same per-card uniqueness, for the five element ids the detail field set
  // mints — a label pointing at another card's input is a label pointing at the
  // wrong recording.
  const detailsPrefix = useId();

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
  // The four fields the manifest holds beside the title (Story 45.19), drafted
  // locally: this editor is not the pre-Start store's, and typing here must not
  // put words into the form describing the NEXT session.
  const [details, setDetails] = useState<RecordingMetaFields>(EMPTY_DETAILS);
  // What the session's manifest last said those four were — `null` until the
  // read lands, and `null` FOREVER for a session whose manifest will not load.
  // Save compares against it, so an editor that only touched the title sends no
  // details write at all.
  const [stored, setStored] = useState<RecordingSessionMetaVm | null>(null);
  const [detailsUnavailable, setDetailsUnavailable] = useState(false);

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

  // The title arrives one IPC round trip after the completion card mounts, and
  // the other four arrive one round trip after the editor opens. Either way an
  // editor opened inside that window seeds EMPTY fields over values the user
  // never saw — and an empty field is a clear, which for the title moves the
  // folder back to its untitled path and for the rest wipes the manifest.
  // Re-seed the untouched fields when the real values land, per field, so a
  // field the user has since typed into is their edit and stands while its
  // neighbours still catch up.
  const seededWith = useRef<RecordingMetaFields>(EMPTY_DETAILS);
  useEffect(() => {
    if (!editing) {
      return;
    }
    const loaded = effectiveTitle ?? "";
    if (seededWith.current.title === loaded) {
      return;
    }
    const seeded = seededWith.current.title;
    setDraft((current) => (current === seeded ? loaded : current));
    seededWith.current = { ...seededWith.current, title: loaded };
  }, [editing, effectiveTitle]);

  /**
   * Read the session's stored details, once per opening of the editor.
   *
   * **The fields and the baseline are set in the SAME update, on purpose.** An
   * earlier shape landed the baseline in state and re-seeded the fields from an
   * effect, which left one render in which the baseline said "the manifest has
   * participants" while the fields still said "" — and in that render Save was
   * enabled over an empty form. Pressing it would have written the empty seed
   * over the stored details, which is the exact edit nobody asked for. Doing
   * both here means the two can never disagree.
   *
   * Per field rather than wholesale: a field the user typed into while the read
   * was in flight is their edit and stands, while its untouched neighbours
   * still catch up.
   */
  const loadDetails = useCallback((folder: string) => {
    setStored(null);
    setDetailsUnavailable(false);
    void recordingSessionMeta(folder)
      .then((meta) => {
        if (meta === null) {
          // No loadable manifest. Say so and freeze the four fields rather than
          // leave four empty boxes standing in for values keeper cannot read.
          setDetailsUnavailable(true);
          return;
        }
        const loaded = detailsOf(meta);
        const seeded = seededWith.current;
        setDetails((current) => ({
          title: current.title,
          participants:
            current.participants === seeded.participants
              ? loaded.participants
              : current.participants,
          note: current.note === seeded.note ? loaded.note : current.note,
          tags: current.tags === seeded.tags ? loaded.tags : current.tags,
          custom: sameCustom(current.custom, seeded.custom) ? loaded.custom : current.custom,
        }));
        seededWith.current = { ...loaded, title: seededWith.current.title };
        setStored(meta);
      })
      .catch(() => {
        setDetailsUnavailable(true);
      });
  }, []);

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
  const titleChanged = typed !== (effectiveTitle ?? "");
  // Compared against what the manifest LAST SAID, not against what the editor
  // opened with: a session whose details have not been read yet has nothing to
  // compare to, and sending a write from an unread editor would save the empty
  // seed over whatever is really in the file.
  const detailsChanged = stored !== null && !sameDetails(details, detailsOf(stored));
  // Nothing to send when neither half changed (an empty title field on an
  // untitled session is exactly that case), and nothing to send twice while a
  // save is in flight.
  const saveDisabled = saving || (!titleChanged && !detailsChanged);

  /**
   * Commit the edit; print a refusal in the fault slot and keep the editor open
   * on the typed text.
   *
   * **The details go first and the title goes last, and the order is the whole
   * error story.** The details are a rewrite of four keys in a file that always
   * succeeds if the file is there; the title MOVES the session, and Rust refuses
   * that for a live session, for an exhausted ordinal run and for a folder
   * outside the destination. Doing the refusable half second means a refusal
   * costs only the rename — the details are already saved, the reason is on
   * screen, and the user can act on it. The other order would throw away an edit
   * that had nothing wrong with it.
   */
  const save = async () => {
    setSaving(true);
    setRefusal(null);
    try {
      if (detailsChanged) {
        // Answered from the manifest that was written, not echoed from the
        // request: Rust trims, drops nameless custom rows and re-joins the tag
        // line, and the editor must show what was stored rather than what was
        // typed.
        const saved = await recordingMetaUpdate(
          effectiveFolder,
          details.participants,
          details.note,
          details.tags,
          details.custom,
        );
        setStored(saved);
        setDetails(detailsOf(saved));
        seededWith.current = { ...detailsOf(saved), title: seededWith.current.title };
      }
      if (titleChanged) {
        // A cleared field is `null`, not `""`: clearing the title is a real
        // edit, and it moves the session back to its untitled path.
        const summary = await recordingRetitle(effectiveFolder, typed === "" ? null : typed);
        // Keyed on the folder this card is currently mounted for, so the
        // override still recognises the session once the owner adopts the new
        // path.
        setMoved({ from: sessionFolder, summary });
        onRetitled?.(summary);
      }
      setEditing(false);
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
        {/* The details editor sits OUTSIDE the live region below: text fields
            and their buttons inside an aria-atomic `role="status"` re-announce
            the whole card on every keystroke — the same reason the
            active-recording banner keeps its ticking elapsed out of one. */}
        <div className="flex flex-col gap-1">
          {editing ? (
            <div className="flex flex-col gap-3">
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
              {detailsUnavailable && (
                <p
                  className="text-muted-foreground text-xs"
                  data-testid={SUMMARY_DETAILS_UNAVAILABLE_TESTID}
                >
                  {SUMMARY_DETAILS_UNAVAILABLE}
                </p>
              )}
              {/* The SAME five fields the "Next session" card collects (Story
                  45.19). The title one is rendered above instead, because on
                  this surface it is a rename and leaves by a different door —
                  so the set is handed a title it never shows a change for.

                  `detailsPrefix` is per-card: two recovery cards can be open at
                  once, and a duplicated element id points a `<label for>` at
                  whichever input the browser found first. */}
              <div className="flex flex-col gap-3" data-testid={SUMMARY_DETAILS_TESTID}>
                <RecordingMetaFieldSet
                  fields={details}
                  idPrefix={detailsPrefix}
                  disabled={saving || detailsUnavailable}
                  onChange={(patch) => {
                    setDetails((current) => ({ ...current, ...patch }));
                    setRefusal(null);
                  }}
                />
              </div>
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
                  seededWith.current = { ...EMPTY_DETAILS, title: effectiveTitle ?? "" };
                  setDraft(seededWith.current.title);
                  setDetails(EMPTY_DETAILS);
                  // The other four are read now rather than on mount: a
                  // recovery scan can put a dozen of these cards on screen, and
                  // a manifest read per card for a form nobody opened is a
                  // dozen reads off a possibly-removable volume for nothing.
                  loadDetails(effectiveFolder);
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
        {/* The note stub (Story 42.4, UX-DR51) — after the outcome, before the
            actions: what was saved is the first thing the user reads, and the
            invitation to write about it is the next. Outside the live region
            above for the same reason the rename editor is: a textarea inside an
            aria-atomic `role="status"` re-announces the whole card on every
            keystroke. Deliberately NOT keyed on the folder: a rename MOVES the
            session (Story 40.4) and would remount the editor mid-sentence. The
            stub itself carries the immutable session id, and the editor uses it
            to tell "this session, re-resolved at its new path" from "a
            different session in this slot". */}
        {!recovered && <RecordingNoteStub folder={effectiveFolder} />}
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
