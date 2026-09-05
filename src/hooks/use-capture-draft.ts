/**
 * The note one quick-capture window is holding (Story 45.14, FR-190, AD-93).
 *
 * Quick capture mounts the real note editor, so the thing it edits has to be a
 * real note. That is not a convenience: **a tag is frontmatter on a note and an
 * attachment is a file copied relative to a note's path**, and neither can be
 * applied to a string in a settings table. The old panel's durable value was
 * `notes.capture_buffer`, a debounced mirror of a textarea; the durable value
 * now is the note file, which is what makes markdown, tags and attachments
 * arrive together rather than one story at a time.
 *
 * # Why the note is resolved before the hotkey, never after it
 *
 * NFR-27 gives the panel 300 ms and `notes_window` spends all of it: the window
 * is created hidden at startup and never destroyed, so the hotkey path is
 * position → show → focus with no webview construction in it. This hook is the
 * document's half of the same trick. It resolves at **mount** — app startup,
 * with nobody waiting — and re-arms the next page immediately after a
 * dismissal, while the window is hidden. A resolve on `show` would put an IPC
 * round trip in front of the first keystroke, which is the one thing this
 * surface may not do.
 *
 * # Why dismissal saves before it re-resolves
 *
 * Rust decides whether the draft was written in by comparing the bytes on disk
 * with what creation put there. The editor autosaves after 1.5 s of typing
 * idle, so a thought typed and immediately dismissed is not on disk yet, and
 * Rust would read the page as untouched and hand it back — the next thought
 * would land underneath the last one. So dismissal is a force-flush point
 * (AD-62): {@link saveOpenNote} first, awaited, then hide, then re-arm.
 *
 * # What is deliberately still Rust's
 *
 * Which note, whether it is reusable, and what the create had to say about it.
 * This hook never decides that a page is blank and never creates a note itself:
 * one creation path, in Rust, with 44.6's `notices` channel attached to it.
 */
import { useCallback, useEffect, useState } from "react";
import { saveNote } from "@/hooks/use-notes-body";
import {
  type IpcError,
  listenNotesCaptureShown,
  type NoteRefVm,
  notesCaptureDraft,
  notesCaptureHide,
} from "@/lib/ipc/client";

/** Structural guard for the {@link IpcError} envelope thrown by the IPC client. */
function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.code === "string" && typeof candidate.message === "string";
}

/** The message an IPC rejection carries, or the value itself. */
function reasonOf(error: unknown): string {
  return isIpcError(error) ? error.message : String(error);
}

/**
 * A resolved page: the note, and the sentences the create that produced it had
 * to say. They travel together because a notice is about **that** note — a
 * reused page must not re-show a notice from a create that already happened,
 * and a fresh page must not inherit the previous page's.
 */
interface ResolvedDraft {
  note: NoteRefVm;
  notices: readonly string[];
}

export interface UseCaptureDraft {
  /** The note this window holds, or null until the first resolve lands. */
  note: NoteRefVm | null;
  /** 44.6's sentences about the create that produced {@link UseCaptureDraft.note}. */
  notices: readonly string[];
  /**
   * Why there is no page to write on, or null.
   *
   * "There is nowhere to put a thought" — no vault is flagged — and the window
   * says so rather than accepting keystrokes it cannot keep.
   */
  error: string | null;
  /**
   * Why the window would not go away, or null.
   *
   * **A separate field from {@link UseCaptureDraft.error} because the two are
   * produced by steps that run one after the other.** Dismissal is hide and
   * then re-arm; the re-arm succeeds, and if both wrote one slot its success
   * would clear the hide's failure in the same tick — the sentence would be
   * set and erased before a frame was painted, which is indistinguishable from
   * never saying anything. They also mean different things to the reader: no
   * page blocks the editor, a stuck window does not.
   */
  windowError: string | null;
  /** File this thought, hide the window, and put a fresh page in front. */
  dismiss: () => void;
}

/**
 * `hide` is what puts the page away once it is saved: the desktop's
 * `notes_capture_hide` (a window verb) by default, and the phone's sheet close
 * where quick capture is a sheet in the stack (Story 66.4, AD-200). One hook,
 * one order — save, then hide, then re-arm — on both tiers.
 */
export function useCaptureDraft(
  captureKey: string,
  hide: () => Promise<void> = notesCaptureHide,
): UseCaptureDraft {
  const [draft, setDraft] = useState<ResolvedDraft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [windowError, setWindowError] = useState<string | null>(null);

  const resolve = useCallback(async () => {
    try {
      const resolved = await notesCaptureDraft(captureKey);
      // Identity is the note's id, not the object: Rust hands back a fresh
      // `NoteRefVm` every call, and replacing an unchanged page would discard
      // the notices it arrived with and remount an editor holding a caret.
      setDraft((held) =>
        held !== null && held.note.id === resolved.note.id
          ? held
          : { note: resolved.note, notices: resolved.notices },
      );
      setError(null);
    } catch (failure: unknown) {
      setError(reasonOf(failure));
    }
  }, [captureKey]);

  useEffect(() => {
    void resolve();

    // `listenNotesCaptureShown` has existed since Epic 36 and, until this
    // story, was called from nowhere — the third of DW-172's
    // declared-and-never-mounted listeners. It is the belt to the re-arm in
    // `dismiss`: a window shown after a dismissal keeper did not drive (the
    // tray, a second press of the hotkey) still gets its page checked.
    let cancelled = false;
    let stop: (() => void) | null = null;
    void listenNotesCaptureShown(() => {
      void resolve();
    })
      .then((unlisten) => {
        if (cancelled) {
          unlisten();
          return;
        }
        stop = unlisten;
      })
      .catch(() => {
        // A listener that could not be attached costs the belt, never the page:
        // `dismiss` re-arms on the path that matters.
      });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [resolve]);
  const held = draft?.note ?? null;
  const dismiss = useCallback(() => {
    void (async () => {
      // Awaited, and first: Rust reads the file to decide whether this page was
      // written on, so the last 1.5 s of typing has to be in it (AD-62).
      //
      // **And the answer is read, not assumed.** `saveNote` catches its own
      // failure — it has to, because the editor's caption is fed from the same
      // store — so awaiting it is not a success check. Hiding on a refused
      // write would take the panel away with the reason legible only inside the
      // window that just disappeared, and would then ask Rust "was this page
      // written on?" about bytes that never reached the disk, which answers
      // "no" and hands the same page back. The words would survive; the person
      // would be told nothing. UX-DR35's error branch is that a failed write
      // leaves the text where it is and the panel open, and it survived the
      // move from a buffer to a note.
      //
      // Story 46.12: it names the page this window is holding. This webview is
      // its own JS realm and therefore its own store, so there is only ever one
      // note in it — but "the only one" and "the one I mean" are different
      // claims, and after 46.12 only the second is expressible.
      if (held !== null && !(await saveNote(held.vaultId, held.id))) {
        return;
      }
      setWindowError(null);
      try {
        await hide();
      } catch (failure: unknown) {
        // Said rather than swallowed, and the page is kept: a window that would
        // not hide is a window problem, and throwing the note away over it
        // would be capture losing words, which it may never do.
        setWindowError(reasonOf(failure));
      }
      // Re-armed while hidden, so the next summon reveals a live editor on a
      // live note with no round trip in front of the first keystroke.
      await resolve();
    })();
  }, [held, resolve, hide]);

  return {
    note: draft?.note ?? null,
    notices: draft?.notices ?? [],
    error,
    windowError,
    dismiss,
  };
}
