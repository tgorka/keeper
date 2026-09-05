/**
 * Quick capture on a phone: a sheet in the stack, not a window (Epic 66,
 * Story 66.4, FR-469, AD-200).
 *
 * The desktop's quick capture is a prewarmed window `notes_window` shows on
 * the hotkey (`capture-main.tsx`). A phone has no second window, so the same
 * page — Rust's `notes_capture_draft`, the one creation path with 44.6's
 * `notices` channel attached — opens as a bottom sheet over whatever level the
 * stack is on, with the real `NoteEditor` in it. Nothing about the document is
 * forked: {@link CaptureDocument} is the desktop's, `useCaptureDraft` is the
 * desktop's hook with its hide seam pointed at the sheet's close instead of
 * `notes_capture_hide`, so the order the desktop proves — save, then hide, then
 * re-arm — holds here too (AD-62). Done, Escape and the overlay are one act.
 *
 * # Text, and why only text
 *
 * The desktop capture takes text, tags, attachments from a file picker and a
 * drag — and refuses a pasted image, honestly (`notes_attachment_paste` names
 * the clipboard backend the build does not link). So the phone's sheet takes
 * exactly what the desktop's window does: an iOS clipboard image read would be
 * a capability the Mac's capture never had, and this story mirrors, it does
 * not lead. The Attach file control is the dialog plugin's document picker,
 * which iOS answers.
 *
 * # Always mounted, like the drawer
 *
 * Mounted once in `PhoneShell` beside `LeadingDrawer`, open state in
 * `captureSheetStore`, so the Inbox header, the Notes level, the palette's
 * `notes-capture` and the ⌘⌥K chord open one sheet. The page is resolved when
 * the sheet's content mounts — not at app start as the desktop window does —
 * because a sheet has no hidden state to prewarm in, and one IPC round trip
 * behind a tap is not NFR-27's hotkey budget.
 */
import { type MutableRefObject, useEffect, useRef } from "react";
import { CaptureDocument } from "@/components/capture/capture-document";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetTitle } from "@/components/ui/sheet";
import { useCaptureDraft } from "@/hooks/use-capture-draft";
import { DRAFT_CAPTURE_KEY } from "@/lib/capture-target";
import { captureSheetStore, useCaptureSheetStore } from "@/lib/stores/capture-sheet";

/** The sheet's title. */
export const CAPTURE_SHEET_TITLE = "Quick capture";

/**
 * The sheet's close: files the thought (saves), then puts the sheet away. The
 * desktop window's close button and Escape are the same act (UX-DR35), and
 * nothing anywhere on this sheet discards text.
 */
export const CAPTURE_SHEET_DONE_LABEL = "Done";

/** What the sheet says while the first resolve is in flight. */
export const CAPTURE_SHEET_OPENING = "Opening a page…";

/** Names the sheet's content, so a test can find its bands. */
export const CAPTURE_SHEET_SLOT = "capture-phone-sheet";

/** Close the sheet once the page is saved: the hook's hide seam on this tier. */
async function closeSheet(): Promise<void> {
  captureSheetStore.getState().close();
}

/**
 * The sheet's content: mounted only while open, so every open resolves the
 * page afresh and Rust decides whether it is the same one.
 */
function CaptureSheetBody({
  dismissRef,
}: {
  /** Handed the body's dismissal so the sheet's overlay can file the thought too. */
  dismissRef: MutableRefObject<(() => void) | null>;
}) {
  const { note, notices, error, windowError, dismiss } = useCaptureDraft(
    DRAFT_CAPTURE_KEY,
    closeSheet,
  );
  dismissRef.current = dismiss;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // `defaultPrevented` is the whole guard: Escape closes the `/` menu and
      // the choosers first, and CodeMirror marks the event handled when it
      // does (the desktop window's rule, `capture-document.tsx`).
      if (event.defaultPrevented || event.key !== "Escape") {
        return;
      }
      event.preventDefault();
      dismiss();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dismiss]);

  return (
    <div
      data-slot={CAPTURE_SHEET_SLOT}
      className="flex min-h-0 flex-1 flex-col bg-background text-foreground"
    >
      <header className="flex h-13 shrink-0 items-center gap-2 border-b px-3">
        <SheetTitle className="min-w-0 flex-1 truncate font-heading text-title">
          {CAPTURE_SHEET_TITLE}
        </SheetTitle>
        <Button
          type="button"
          variant="ghost"
          onClick={dismiss}
          // 44pt: the phone's minimum target.
          className="h-11 min-w-11 shrink-0"
        >
          {CAPTURE_SHEET_DONE_LABEL}
        </Button>
      </header>
      {error === null ? null : (
        // There is no page, so this is the whole content: a sheet that took
        // keystrokes it could not keep would be the failure capture exists to
        // prevent.
        <p role="alert" className="border-b px-3 py-1 text-destructive text-xs">
          {error}
        </p>
      )}
      {windowError === null ? null : (
        <p role="alert" className="border-b px-3 py-1 text-destructive text-xs">
          {windowError}
        </p>
      )}
      {note === null ? (
        error === null ? (
          <p className="p-4 text-muted-foreground text-sm">{CAPTURE_SHEET_OPENING}</p>
        ) : null
      ) : (
        <div className="min-h-0 flex-1">
          <CaptureDocument vaultId={note.vaultId} noteId={note.id} notices={notices} />
        </div>
      )}
    </div>
  );
}

export function CapturePhoneSheet() {
  const isOpen = useCaptureSheetStore((s) => s.isOpen);
  const dismissRef = useRef<(() => void) | null>(null);
  return (
    <Sheet
      open={isOpen}
      // Every way Radix would close this — the overlay above all — files the
      // thought first: the body's `dismiss` saves and then closes the store,
      // so the sheet never goes away with words unsaved (UX-DR35).
      onOpenChange={(open) => {
        if (!open) {
          dismissRef.current?.();
        }
      }}
    >
      <SheetContent
        side="bottom"
        showCloseButton={false}
        // Most of the screen, above the keyboard's inset and the home
        // indicator; the editor's scroll host is the one region that flexes.
        className="h-[85dvh] gap-0 p-0 pb-[calc(var(--kb-inset,0px)_+_var(--safe-bottom))] motion-reduce:animate-none motion-reduce:transition-none"
        // Escape is the body's: CodeMirror closes its own popups on it first
        // and marks the event handled, which Radix would not honour.
        onEscapeKeyDown={(event) => event.preventDefault()}
        data-testid="capture-phone-sheet"
      >
        {isOpen && <CaptureSheetBody dismissRef={dismissRef} />}
      </SheetContent>
    </Sheet>
  );
}
