/**
 * The note stub, at the only moment it will ever be written (Story 42.4,
 * FR-142, UX-DR51).
 *
 * Nobody documents a meeting an hour later. The minute a recording stops is the
 * entire window in which anything will be written about it, so finalize composes
 * a markdown stub — prefilled with what keeper already knows — and this is the
 * surface that puts a cursor in it. It renders inside the recording summary
 * card, never as a second card beside it: the card is already what the user is
 * looking at, and a note that arrives in its own container is a note that
 * arrives somewhere else.
 *
 * **The user edits the body and only the body.** The stub's frontmatter is the
 * notes subsystem's frontmatter, and AC1 requires it to round-trip through
 * `Frontmatter::parse` with every key read back unchanged — a promise no
 * textarea can keep. `RecordingNoteStubVm` therefore carries the whole file
 * plus `bodyOffset`, the exact position where the body begins, in UTF-16 code
 * units so it indexes a JavaScript string directly. This surface splits there
 * once, edits only the tail, and sends back `head + draft`, so keeper's own
 * block returns byte-identical and cannot be damaged from here. The offset is
 * used, never re-derived: parsing frontmatter is Rust's job in this story.
 *
 * **There is no Save button**, matching the notes editor ("there is no save
 * button anywhere in the product"). Words are committed two ways:
 * - Blur commits a dirty draft through `recordingNoteStubSave`, the idiom the
 *   approval row's inline editor already uses. Without it, clicking Reveal in
 *   Finder or switching views would silently eat the one paragraph this entire
 *   story exists to capture.
 * - Escape saves a dirty draft FIRST, then always dismisses. `recordingNoteStubDismiss`
 *   deletes only a file still byte-identical to what keeper composed, so the
 *   save is what makes the deletion decision honest: an untouched stub is gone
 *   (an archive full of empty notes is worse than one with none), and a written
 *   one is on disk before anything is allowed to look at it. A save that fails
 *   cancels the dismissal outright — deleting a stub whose replacement never
 *   landed is the one unrecoverable move on this surface.
 *
 * The deletion decision itself is deliberately NOT taken here. This surface's
 * whole job is to make the disk tell the truth about what the user typed, and
 * then report which way Rust went.
 *
 * A stub that could not be written resolves to `null`: the area is simply
 * absent, and the recording summary around it is untouched. Finalize already
 * succeeded, the card still says so, and nothing here is ever allowed to make a
 * finalized recording look like a failure — which is also why a failed
 * *resolution* is swallowed rather than surfaced.
 */
import { useEffect, useRef, useState } from "react";
import { Textarea } from "@/components/ui/textarea";
import type { RecordingNoteStubVm } from "@/lib/ipc/client";
import {
  recordingNoteStub,
  recordingNoteStubDismiss,
  recordingNoteStubSave,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The stub's heading — names what the box is for, in the recording voice. */
export const NOTE_STUB_HEADING = "Note about this recording";

/** Accessible name for the body field (the heading is decorative beside it). */
export const NOTE_STUB_BODY_LABEL = "Note about this recording";

/** Where the note lives when the destination resolved to a notes vault. */
export const NOTE_STUB_IN_VAULT_LABEL = "in your notes";

/** Where it lives when no vault destination resolved: beside the recording, in
 *  the session folder's parent. A real file at a real path — not a degraded
 *  case, just a different one. */
export const NOTE_STUB_BESIDE_LABEL = "beside the recording";

/** The one-line hint under the field: what the key does, and the honest
 *  consequence of not writing anything. Stated up front, because a note that
 *  vanishes without warning is a note the user will not trust next time. */
export const NOTE_STUB_HINT = "Esc closes this. A note you don't write isn't kept.";

/** The caption while a write is in flight. */
export const NOTE_STUB_SAVING = "Saving…";

/** The caption once a write landed — the notes editor's one-word feedback. */
export const NOTE_STUB_SAVED = "Saved";

/** Last-resort message when a rejection carries no readable sentence. */
export const NOTE_STUB_UNKNOWN_ERROR = "keeper could not save this note.";

/** Test id for the editable body (the autofocus target — UX-DR51). */
export const NOTE_STUB_BODY_TESTID = "recording-note-stub-body";

/** Test id for the hint / status caption. */
export const NOTE_STUB_HINT_TESTID = "recording-note-stub-hint";

/** Test id for the line left behind when a written note was kept. */
export const NOTE_STUB_KEPT_TESTID = "recording-note-stub-kept";

/** Test id for a Rust-composed refusal, rendered verbatim. */
export const NOTE_STUB_FAULT_TESTID = "recording-note-stub-fault";

export interface RecordingNoteStubProps {
  /** The session folder the stub is resolved from, and the key all three
   *  commands take. The card's CURRENT folder: a rename moves the session
   *  (Story 40.4) while the stub stays put in the parent, and resolution walks
   *  from the manifest at this folder to the note carrying its session id. */
  folder: string;
}

export function RecordingNoteStub({ folder }: RecordingNoteStubProps) {
  const [stub, setStub] = useState<RecordingNoteStubVm | null>(null);
  const [draft, setDraft] = useState("");
  // How the dismissal went, or `null` while the note is still open.
  const [outcome, setOutcome] = useState<"deleted" | "kept" | null>(null);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  // A Rust-authored refusal, rendered verbatim where the words it is about are.
  const [fault, setFault] = useState<string | null>(null);

  const bodyRef = useRef<HTMLTextAreaElement>(null);
  // The resolved stub, readable from the fetch callback so a late resolve can
  // tell "the same session again" from "a different session in this slot"
  // without a state updater that would have to run side effects to do it.
  const stubRef = useRef<RecordingNoteStubVm | null>(null);
  // True from the first keystroke onward. A re-resolve may never overwrite a
  // draft the user has typed into — not even with what Rust says is on disk,
  // because the words in front of them are the newer truth. The rename editor
  // in the summary card draws the same line.
  const touched = useRef(false);
  // The body Rust last accepted (or composed). Equality with the draft is the
  // definition of "not dirty", so a second blur on committed text writes
  // nothing and an untouched stub is never saved into existence.
  const committedBody = useRef("");
  // A write or a dismissal is in flight. A ref, not the `busy` state, because
  // two events inside one React batch both read the pre-render state.
  const inFlight = useRef(false);
  // The folder whose stub has already been dismissed, so the resolve effect
  // does not re-fetch a note the user just closed — while a DIFFERENT folder
  // still resolves normally.
  const dismissedFolder = useRef<string | null>(null);
  // Focus is offered once. If something else already holds it, this surface
  // does not chase it on the next render.
  const offeredFocus = useRef(false);

  // Resolve the stub for the folder this card is currently showing. It arrives
  // one round trip after the card mounts (and again after a rename re-points
  // the card), so this must never overwrite words typed inside that window.
  useEffect(() => {
    if (dismissedFolder.current === folder) {
      return;
    }
    let cancelled = false;
    void (async () => {
      let resolved: RecordingNoteStubVm | null;
      try {
        resolved = await recordingNoteStub(folder);
      } catch {
        // A stub that cannot be resolved is absent, not an error: the recording
        // finalized, and this surface is never allowed to say otherwise.
        return;
      }
      if (cancelled || resolved === null) {
        return;
      }
      const body = resolved.contents.slice(resolved.bodyOffset);
      // A different session in the same card slot brings its own note; the
      // previous session's draft belongs to the previous session. The same
      // session re-resolved (a rename, a re-fetch after a save) keeps whatever
      // the user has typed — Rust returns the on-disk contents, so re-seeding
      // an UNTOUCHED draft from it is always the truth.
      const foreign = stubRef.current !== null && stubRef.current.sessionId !== resolved.sessionId;
      if (foreign || !touched.current) {
        committedBody.current = body;
        setDraft(body);
      }
      if (foreign) {
        touched.current = false;
        setOutcome(null);
        setSaved(false);
        setFault(null);
        offeredFocus.current = false;
      }
      stubRef.current = resolved;
      setStub(resolved);
    })();
    return () => {
      cancelled = true;
    };
  }, [folder]);

  // UX-DR51: the cursor lands in the BODY, not on the card. At the end of what
  // keeper prefilled, because the prefill is context keeper wrote and the
  // user's sentence goes after it — a caret at 0 would shove keeper's own first
  // line down the page on the first keystroke.
  //
  // Guarded on what already has focus: the note arrives a round trip late, and
  // the rename field one click away must not lose the caret mid-word to it.
  useEffect(() => {
    const field = bodyRef.current;
    if (stub === null || field === null || offeredFocus.current) {
      return;
    }
    offeredFocus.current = true;
    const active = document.activeElement;
    if (active !== null && active !== document.body) {
      return;
    }
    field.focus();
    field.setSelectionRange(field.value.length, field.value.length);
  }, [stub]);

  /**
   * Write the draft, recomposed onto keeper's own frontmatter.
   *
   * `head` is everything before `bodyOffset` — the block this surface renders
   * nowhere and edits never — so what Rust receives round-trips exactly as it
   * composed it, and its byte-identity check keeps meaning what it says.
   */
  const write = async (stubNow: RecordingNoteStubVm, body: string) => {
    await recordingNoteStubSave(folder, stubNow.contents.slice(0, stubNow.bodyOffset) + body);
    committedBody.current = body;
  };

  /** Blur commits a dirty draft — the approval editor's idiom. */
  const commit = async () => {
    if (stub === null || inFlight.current || draft === committedBody.current) {
      return;
    }
    inFlight.current = true;
    setBusy(true);
    setFault(null);
    try {
      await write(stub, draft);
      setSaved(true);
    } catch (raw) {
      // `syncErrorMessage`, never `String(raw)`: an IPC rejection is a
      // `{ code, message }` object and stringifying one prints
      // "[object Object]" exactly where the reason belongs. The draft stays on
      // screen — a failed write whose text is gone loses the words twice.
      setFault(syncErrorMessage(raw, NOTE_STUB_UNKNOWN_ERROR));
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  };

  /**
   * Escape: make the disk true, then hand the decision to Rust.
   *
   * The order is the whole point. `recordingNoteStubDismiss` deletes only a
   * file still byte-identical to what keeper composed, so words that never
   * reached disk would be deleted along with the stub they were written into.
   * Saving first is what makes the deletion honest — and a save that throws
   * skips the dismissal by control flow, because deleting the pristine file
   * those words were meant to replace is the one unrecoverable move here.
   *
   * Re-entry is barred on a ref rather than on `busy`: two keypresses inside
   * one React batch would both read the pre-render `busy`, and a stub deleted
   * twice is a second dismissal against a file that is already gone.
   */
  const dismiss = async () => {
    if (stub === null || inFlight.current) {
      return;
    }
    inFlight.current = true;
    setBusy(true);
    setFault(null);
    try {
      if (draft !== committedBody.current) {
        await write(stub, draft);
      }
      const deleted = await recordingNoteStubDismiss(folder);
      dismissedFolder.current = folder;
      setOutcome(deleted ? "deleted" : "kept");
    } catch (raw) {
      // Either the words never landed or the dismissal did not: the note is
      // still on disk and the draft is still on screen. Stay open rather than
      // trapping the text behind a closed surface.
      setFault(syncErrorMessage(raw, NOTE_STUB_UNKNOWN_ERROR));
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  };

  // Absent: no stub for this session (never written, or already dismissed).
  // The summary card around this renders in full — the recording succeeded.
  if (stub === null || outcome === "deleted") {
    return null;
  }

  if (outcome === "kept") {
    return (
      <p className="text-muted-foreground text-xs" data-testid={NOTE_STUB_KEPT_TESTID}>
        {`Kept ${stub.inVault ? NOTE_STUB_IN_VAULT_LABEL : NOTE_STUB_BESIDE_LABEL} as ${stub.relativePath}`}
      </p>
    );
  }

  // The one caption under the field. It leads with the honest consequence of
  // writing nothing, and only reports on writing while there is writing to
  // report — the notes editor's rule, for the same reason: a word that
  // flickers on every keystroke is noise rather than information.
  let caption = NOTE_STUB_HINT;
  if (busy) {
    caption = NOTE_STUB_SAVING;
  } else if (saved) {
    caption = NOTE_STUB_SAVED;
  }

  return (
    // Deliberately NOT inside the card's `role="status"` region: a textarea in
    // an aria-atomic live region re-announces the whole card — headline, folder
    // path and all — on every keystroke. The rename editor sits out for the
    // same reason.
    <div className="flex flex-col gap-1">
      <div className="flex items-baseline justify-between gap-2">
        <p className="font-medium text-sm">{NOTE_STUB_HEADING}</p>
        <p className="truncate font-mono text-muted-foreground text-xs">
          {stub.inVault ? NOTE_STUB_IN_VAULT_LABEL : NOTE_STUB_BESIDE_LABEL} · {stub.filename}
        </p>
      </div>
      <Textarea
        ref={bodyRef}
        className="min-h-24 text-sm"
        data-testid={NOTE_STUB_BODY_TESTID}
        aria-label={NOTE_STUB_BODY_LABEL}
        aria-invalid={fault !== null}
        value={draft}
        onChange={(event) => {
          touched.current = true;
          setDraft(event.target.value);
          // "Saved" describes text that stopped existing the moment it changed,
          // and a refusal is about the words that were sent — retract both as
          // the user types toward something else.
          setSaved(false);
          setFault(null);
        }}
        onKeyDown={(event) => {
          if (event.key !== "Escape") {
            return;
          }
          // Consumed here, the shape the approval editor and the conflict
          // resolver already use, so the keypress that closed the note does not
          // also pop a level of the phone stack behind it (UX-DR28).
          event.preventDefault();
          void dismiss();
        }}
        onBlur={() => {
          void commit();
        }}
      />
      {fault === null ? (
        <p className="text-muted-foreground text-xs" data-testid={NOTE_STUB_HINT_TESTID}>
          {caption}
        </p>
      ) : (
        <p role="alert" className="text-destructive text-xs" data-testid={NOTE_STUB_FAULT_TESTID}>
          {fault}
        </p>
      )}
    </div>
  );
}
