/**
 * The document half of a quick-capture window (Story 45.14, FR-190, AD-93).
 *
 * **Quick capture mounts `NoteEditor`.** Not "quick capture gains markdown":
 * the same component, so the format toolbar, the `/` menu, emoji completion,
 * tags, properties and attachments arrive together and stay in step forever.
 * Six items of the owner's report are one decision made once, and the way to
 * keep it made is that there is nothing here for them to arrive *into* — this
 * file mounts the editor and renders what the create had to say, and that is
 * all it is allowed to do.
 *
 * # The seam
 *
 * The **window** is quick capture's own — its size, its position, its lock, its
 * close button (Story 45.15). The **document** is not. That split is what this
 * file draws:
 *
 * - {@link CaptureDocument} takes a note and shows it. A window opened on an
 *   existing note renders this directly, which is what makes "any note openable
 *   as a capture window" a prop rather than a feature.
 * - {@link CaptureDraftDocument} is the hotkey window: it resolves the page
 *   this window holds — creating one through the single Rust creation path when
 *   there is none — and then renders the same {@link CaptureDocument}.
 *
 * # What this deliberately does not add
 *
 * No title field, no folder picker, no save button, no discard affordance. The
 * old textarea panel refused those permanently (UX-DR35) and the reasons did
 * not change when the editor arrived; the editor has no save button either,
 * because nothing in this product does. Escape files the thought and puts a
 * fresh page in front of you. Nothing anywhere on this window discards text.
 */
import { type ReactNode, useEffect } from "react";
import { NoteEditor } from "@/components/notes/note-editor";
import { useCaptureDraft } from "@/hooks/use-capture-draft";

/** Shared empty list, so an absent `notices` prop is not a new array a frame. */
const NO_NOTICES: readonly string[] = [];

/** What the window says while the first resolve is in flight. */
export const CAPTURE_OPENING_LABEL = "Opening a page…";

export interface CaptureDocumentProps {
  vaultId: string;
  noteId: string;
  /**
   * Sentences from the create that produced this note — 44.6's channel.
   *
   * Absent for a window opened on a note that already existed, which is an
   * honest "there was no create to have anything to say" rather than a default
   * standing in for one.
   */
  notices?: readonly string[];
}

/**
 * One capture window's note, in the real editor.
 *
 * Fills its parent (`h-full`), so the host decides how tall a capture window's
 * document is — the draft window gives it the whole viewport, and Story 45.15's
 * chrome gives it what is left under the title bar.
 */
export function CaptureDocument({ vaultId, noteId, notices = NO_NOTICES }: CaptureDocumentProps) {
  return (
    <div className="flex h-full min-h-0 flex-col bg-background text-foreground">
      {notices.map((notice) => (
        // Keyed by the sentence: two notices come from two code paths and
        // cannot be identical, which is the property 44.6 keyed on too.
        <p key={notice} role="status" className="border-b px-3 py-1 text-meta">
          {notice}
        </p>
      ))}
      <div className="min-h-0 flex-1">
        <NoteEditor vaultId={vaultId} noteId={noteId} />
      </div>
    </div>
  );
}

export interface CaptureDraftDocumentProps {
  /**
   * Which window is asking, so two capture windows never share one page.
   *
   * Produced by `captureKey` in `@/lib/capture-target` (Story 45.15) and never
   * built here: a key this file spelled itself would be a second definition of
   * the identity Rust stores drafts under.
   */
  captureKey: string;
  /**
   * Story 45.15's window chrome — close button, lock, drag handle — rendered
   * above the editor and **handed the dismissal act**.
   *
   * A slot rather than a fixed element, and it receives `dismiss` rather than
   * arranging its own, because a close button that hid the window itself would
   * be a second spelling of one sentence: it would skip the force-flush that
   * makes Rust see the page as written on, and it would skip the immediate
   * re-arm, so the next summon would pay for a resolve the hotkey path never
   * pays for. One act, one implementation, two affordances.
   */
  chrome?: (dismiss: () => void) => ReactNode;
}

/**
 * The hotkey window: the page you get when you press the chord.
 *
 * Escape and ⌘W/Ctrl+W are the same act, and it is the act the old panel had —
 * file this thought and go away. What changed is that filing it is no longer a
 * write assembled out of a text buffer: the note has existed since before the
 * first keystroke and has been autosaving, so dismissal flushes, hides, and
 * arms a fresh page for next time.
 */
export function CaptureDraftDocument({ captureKey, chrome }: CaptureDraftDocumentProps) {
  const { note, notices, error, windowError, dismiss } = useCaptureDraft(captureKey);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // `defaultPrevented` is the whole of this guard. Escape closes the `/`
      // menu, the tag chooser and the emoji chooser, and CodeMirror marks the
      // event handled when it does — without this, dismissing a completion
      // popup would also throw the window away, which is a keystroke that
      // destroys the surface the user was in the middle of using.
      if (event.defaultPrevented) {
        return;
      }
      // `metaKey || ctrlKey` rather than a platform test: this app never reads
      // the platform (`src/test/no-user-agent-gating.test.ts`), and it is the
      // same pair CodeMirror's own `Mod-` bindings resolve to.
      const closing = event.key === "w" && (event.metaKey || event.ctrlKey);
      if (event.key !== "Escape" && !closing) {
        return;
      }
      event.preventDefault();
      dismiss();
    };
    // On the window rather than on a wrapper element: the chord has to work
    // with the caret in the editor, with focus on a toolbar button, and with
    // focus nowhere at all after the compositor handed the window back.
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dismiss]);

  return (
    <div className="flex h-screen min-h-0 flex-col bg-background text-foreground">
      {error === null ? null : (
        // There is no page, so this is the whole content: a window that took
        // keystrokes it could not keep would be the failure capture exists to
        // prevent.
        <p role="alert" className="border-b px-3 py-1 text-destructive text-xs">
          {error}
        </p>
      )}
      {windowError === null ? null : (
        // Above the editor and never instead of it. A hide that failed is the
        // window's problem; the words stay exactly where they are, because the
        // one thing capture may never do is lose them.
        <p role="alert" className="border-b px-3 py-1 text-destructive text-xs">
          {windowError}
        </p>
      )}
      {chrome?.(dismiss)}
      {note === null ? (
        error === null ? (
          <p className="p-4 text-muted-foreground text-sm">{CAPTURE_OPENING_LABEL}</p>
        ) : null
      ) : (
        <div className="min-h-0 flex-1">
          <CaptureDocument vaultId={note.vaultId} noteId={note.id} notices={notices} />
        </div>
      )}
    </div>
  );
}
