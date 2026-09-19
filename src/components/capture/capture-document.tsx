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
import type { PaneHeaderTitleBar } from "@/components/layout/pane-header";
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
  /**
   * The window's own controls — close, lock, pin — for the editor's header to
   * carry as its frame group (Story 75.3, AD-260).
   *
   * Passed straight through and never rendered here. This file's whole job is
   * the seam between a window and a document (see the module doc); a second
   * header drawn at this level is precisely the 72px of chrome the story
   * removed, so the controls travel to the one header there is.
   */
  frame?: ReactNode;
  /**
   * Present when nothing is drawn above that header — when it IS the window's
   * title bar. Forwarded verbatim to `PaneHeader`, which is where the one
   * concept and its three consequences are documented.
   */
  titleBar?: PaneHeaderTitleBar | null;
}

/**
 * One capture window's note, in the real editor.
 *
 * Fills its parent (`h-full`), so the host decides how tall a capture window's
 * document is — and since Story 75.3 both hosts give it the whole viewport,
 * because there is no longer a strip above it taking 32 of them.
 */
export function CaptureDocument({
  vaultId,
  noteId,
  notices = NO_NOTICES,
  frame,
  titleBar = null,
}: CaptureDocumentProps) {
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
        <NoteEditor vaultId={vaultId} noteId={noteId} frame={frame} titleBar={titleBar} />
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
   * Story 45.15's window chrome — close button, lock, pin — **handed the
   * dismissal act**.
   *
   * A slot rather than a fixed element, and it receives `dismiss` rather than
   * arranging its own, because a close button that hid the window itself would
   * be a second spelling of one sentence: it would skip the force-flush that
   * makes Rust see the page as written on, and it would skip the immediate
   * re-arm, so the next summon would pay for a resolve the hotkey path never
   * pays for. One act, one implementation, two affordances.
   *
   * **Where it is rendered changed in Story 75.3 and the contract did not.** It
   * used to be a row of its own above the editor; it is now the editor's header
   * frame group (AD-260), so the window's controls and the document's sit in
   * one 40px row. The one exception is the state where there is no editor to
   * put a header on — see the fallback below, which exists so the way out does
   * not vanish in the one state where nothing else is on screen.
   */
  chrome?: (dismiss: () => void) => ReactNode;
  /**
   * Present when nothing is drawn above the editor's header — when it IS this
   * window's title bar. Forwarded to {@link CaptureDocument} and on to
   * `PaneHeader`, where the concept is documented.
   *
   * A prop rather than a hook called here, deliberately: the hook lives beside
   * the chrome in `capture-window.tsx`, which already imports this file, and a
   * capture host that reached back for it would close the cycle. The host owns
   * both halves of the pair and passes both.
   */
  titleBar?: PaneHeaderTitleBar | null;
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
export function CaptureDraftDocument({
  captureKey,
  chrome,
  titleBar = null,
}: CaptureDraftDocumentProps) {
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
      {note === null ? (
        <>
          {/* No page means no editor, and no editor means no header for the
              window's controls to live in. This is the ONE state where they are
              drawn on a row of their own, and it is not a leftover of the old
              strip: it is the state where the window is showing a sentence and
              nothing else, so the close button disappearing here would leave a
              stuck window whose only exit is a keystroke nobody can see.

              A `div` and not a `header`: there is no identity, no status and no
              document in it, and a second `header` element is exactly the thing
              Story 75.3 exists to refuse. `h-10 justify-end px-3` so the
              controls sit where the merged header puts them, and the row
              vanishes the instant the editor arrives to carry them. */}
          <div className="flex h-10 shrink-0 items-center justify-end gap-2 border-border border-b px-3">
            {chrome?.(dismiss)}
          </div>
          {error === null ? (
            <p className="p-4 text-muted-foreground text-sm">{CAPTURE_OPENING_LABEL}</p>
          ) : null}
        </>
      ) : (
        <div className="min-h-0 flex-1">
          <CaptureDocument
            vaultId={note.vaultId}
            noteId={note.id}
            notices={notices}
            titleBar={titleBar}
            frame={chrome?.(dismiss)}
          />
        </div>
      )}
    </div>
  );
}
