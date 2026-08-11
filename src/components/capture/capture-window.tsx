/**
 * A capture window's own chrome (Story 45.15, FR-191, FR-192, UX-DR77).
 *
 * The window is quick capture's; the document is not (AD-93). This strip is the
 * whole of "the window": a way out, and a way to decide where the window lives.
 * Nothing here knows what a note is.
 *
 * # Why a close button when Escape already works
 *
 * Escape has dismissed this panel since Epic 36 and still does. It is also
 * invisible. A surface whose only exit is a keystroke is a surface some people
 * never leave — they drag it aside, or they quit keeper. The button is the
 * discoverable spelling of an act that already existed, and it is deliberately
 * the *same* act: for the prewarmed window it is handed the document's own
 * dismissal, so there is one code path and not two.
 *
 * # Why a lock, and why locked is the default
 *
 * A capture window is undecorated, so it has no title bar to drag. Without an
 * affordance it can only be where keeper puts it — a fifth of the way down the
 * monitor holding the pointer — which is the right answer for a thing you type
 * into and dismiss, and the wrong answer for a window you want beside the thing
 * you are reading. Unlocking turns the strip into a drag region
 * (`data-tauri-drag-region`, which is the only way an undecorated Tauri window
 * can be moved) and tells Rust to remember where it ends up. Locked is the
 * default because it is what keeper has always done, and a feature nobody asked
 * for should be invisible until it is asked for.
 *
 * The lock state and the position are Rust's, not this component's: they
 * survive a restart, and a webview that stored its own window position would be
 * storing it per-document in a document that is destroyed when the window
 * closes.
 */
import { Lock, LockOpen, Pin, PinOff, X } from "lucide-react";
import { useCallback, useEffect } from "react";
import { CaptureDocument } from "@/components/capture/capture-document";
import { Button } from "@/components/ui/button";
import { saveNote } from "@/hooks/use-notes-body";
import { captureKey } from "@/lib/capture-target";
import { type CaptureTargetVm, listenNotesCaptureWindows } from "@/lib/ipc/client";
import {
  captureWindowFor,
  closeCaptureWindow,
  hydrateCaptureWindows,
  setCaptureWindowAlwaysOnTop,
  setCaptureWindowLocked,
  useCaptureWindowsStore,
} from "@/lib/stores/capture-windows";

/** The accessible name of the way out. */
export const CAPTURE_CLOSE_LABEL = "Close this capture window";

/**
 * The accessible name of the lock, when the window is keeper-placed.
 *
 * Both verbs, since Story 46.15. The wording is still the module's rule —
 * promise only what the platform can deliver — and this control now delivers
 * two things: the drag region below, and `set_resizable` on the Rust side.
 * Naming only the move would leave the resize undiscoverable, which is how it
 * came to be asked for in the first place. What is still NOT promised is the
 * restore: `set_position` is the call a Wayland compositor may refuse
 * (UX-DR43), so the label says nothing about remembering.
 */
export const CAPTURE_UNLOCK_LABEL = "Unlock this window so it can be moved and resized";

/**
 * The accessible name of the lock, when the window is where the user put it.
 *
 * "Where it is" and not "as it is": locking keeps the position and returns the
 * window to keeper's own size, which is what `Placement::window_size` does with
 * a locked placement. The size is remembered, not discarded — unlocking brings
 * it back — but the label must not imply the current size survives the click.
 */
export const CAPTURE_LOCK_LABEL = "Lock this window where it is";

/**
 * The accessible name of the pin, when the window floats above other apps.
 *
 * Names the STATE it moves to, exactly as the lock's pair does, because that is
 * what a person reads before pressing. "Stop floating" and not "unpin": the
 * window is not pinned to anything, it is held above everything, and the only
 * word for what the user gets back is the ordinary behaviour of every other
 * window.
 *
 * What this deliberately does NOT promise is that it will work. A window
 * manager may decline the request — most tiling ones do — so the label says
 * what is being asked for, and the button's pressed state reports what the
 * compositor actually did. See `notes_window::set_always_on_top`.
 */
export const CAPTURE_UNPIN_LABEL = "Stop this window floating above other apps";

/**
 * The accessible name of the pin, when the window is an ordinary window.
 */
export const CAPTURE_PIN_LABEL = "Keep this window floating above other apps";

export interface CaptureWindowChromeProps {
  /**
   * Which window this is, in Rust's vocabulary.
   *
   * Built by `captureKey` in `@/lib/capture-target` and never spelled here: a
   * key this component invented would be a second definition of the identity
   * Rust stores placements under, and the two would agree for every ASCII vault
   * name and disagree for the first one with a space in it.
   */
  captureKey: string;
  /**
   * Dismiss this window.
   *
   * Required, and supplied by the host rather than performed here, because what
   * dismissal *means* differs by window and neither meaning belongs to a strip
   * of buttons: the prewarmed window files its page and hides so the next
   * hotkey press is still instant, and a window opened on a note closes.
   */
  onClose: () => void;
}

export function CaptureWindowChrome({ captureKey, onClose }: CaptureWindowChromeProps) {
  const window = useCaptureWindowsStore((state) => captureWindowFor(state, captureKey));
  // Unknown reads as locked: until Rust has answered, the window behaves the
  // way it always has. The alternative — assuming unlocked — would put a live
  // drag region over the strip for one frame and let a click that was aiming
  // at the close button move the window instead.
  const locked = window?.locked ?? true;
  // …and unknown reads as on-top, which is what every capture window has been
  // since the panel existed. Same direction of error as the lock: assume the
  // behaviour the window already has, so the frame before Rust answers is not
  // a frame in which the button offers to undo something that never happened.
  const alwaysOnTop = window?.alwaysOnTop ?? true;
  // …and unknown reads as no inset, for the same reason and with the same
  // direction of error: a gap that appears a frame late is invisible, where a
  // gap that appears on a window with no resize border is a permanent gutter.
  const chromeInset = window?.chromeInset ?? 0;

  useEffect(() => {
    void hydrateCaptureWindows();
    let cancelled = false;
    let stop: (() => void) | null = null;
    void listenNotesCaptureWindows(() => {
      void hydrateCaptureWindows();
    })
      .then((unlisten) => {
        if (cancelled) {
          unlisten();
          return;
        }
        stop = unlisten;
      })
      .catch(() => {
        // A listener that could not be attached costs this strip its live lock
        // state, never its buttons: both actions read their argument from the
        // props, not from the store.
      });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

  return (
    <div
      // The drag region is the unlocked window's entire mechanism, and it is
      // conditional rather than always-on: a locked window whose strip was a
      // drag region would move when the user meant to click, and "locked" would
      // be a label rather than a fact.
      {...(locked ? {} : { "data-tauri-drag-region": true })}
      data-testid="capture-window-chrome"
      // The window's OWN resize border, kept off the buttons (DW-199). On GTK,
      // tao hit-tests an undecorated resizable window's resize edges INSIDE the
      // surface and the webview never sees a click that lands there — and the
      // close button is flush into the top-right corner, where the top and
      // right strips overlap. So aiming at close starts a resize, with the
      // arrow cursor still showing (tao's own FIXME).
      //
      // The NUMBER comes from Rust and is never worked out here. It is
      // `scale_factor() * 5`, so a hard-coded 5 would be half the border on a
      // 2x display, and it is zero while locked, while maximized and on every
      // non-GTK backend. This app reads the platform nowhere
      // (`src/test/no-user-agent-gating.test.ts`), so the only honest shape is
      // to render a number the shell measured: see `notes_window::edge_inset`
      // and `keeper_core::capture::chrome_edge_inset`.
      style={
        chromeInset > 0
          ? { paddingTop: chromeInset, paddingRight: `calc(0.25rem + ${chromeInset}px)` }
          : undefined
      }
      className={`flex h-8 shrink-0 items-center justify-end gap-1 border-b px-1 ${
        locked ? "" : "cursor-grab active:cursor-grabbing"
      }`}
    >
      {/*
       * Left of the lock, so the close button stays flush in the top-right
       * corner where DW-199's inset protects it — the corner geometry 47.5
       * measured is unchanged by adding a third button on the far side.
       * `justify-end gap-1` needs no layout work for it.
       */}
      <Button
        variant="ghost"
        size="icon"
        aria-label={alwaysOnTop ? CAPTURE_UNPIN_LABEL : CAPTURE_PIN_LABEL}
        aria-pressed={alwaysOnTop}
        onClick={() => {
          void setCaptureWindowAlwaysOnTop(captureKey, !alwaysOnTop);
        }}
      >
        {alwaysOnTop ? <Pin aria-hidden="true" /> : <PinOff aria-hidden="true" />}
      </Button>
      <Button
        variant="ghost"
        size="icon"
        aria-label={locked ? CAPTURE_UNLOCK_LABEL : CAPTURE_LOCK_LABEL}
        aria-pressed={!locked}
        onClick={() => {
          void setCaptureWindowLocked(captureKey, !locked);
        }}
      >
        {locked ? <Lock aria-hidden="true" /> : <LockOpen aria-hidden="true" />}
      </Button>
      <Button variant="ghost" size="icon" aria-label={CAPTURE_CLOSE_LABEL} onClick={onClose}>
        <X aria-hidden="true" />
      </Button>
    </div>
  );
}

/**
 * Escape and ⌘W/Ctrl+W dismiss a capture window.
 *
 * Extracted rather than written twice: the prewarmed window and a window opened
 * on a note dismiss to *different acts* — one files its page and hides, the
 * other closes — but they must dismiss to the same **keys**, with the same
 * guard, or the chord that works in one capture window is dead in the next.
 *
 * Two details that are the whole of the guard, and both are load-bearing:
 *
 * - **`defaultPrevented` first.** Escape closes the `/` menu, the tag chooser
 *   and the emoji chooser, and CodeMirror marks the event handled when it does.
 *   Without this, dismissing a completion popup would also throw the window
 *   away — a keystroke that destroys the surface the user is in the middle of
 *   using.
 * - **`metaKey || ctrlKey`, never a platform test.** This app reads the
 *   platform nowhere (`src/test/no-user-agent-gating.test.ts` enforces it), and
 *   it is the pair CodeMirror's own `Mod-` bindings resolve to anyway.
 *
 * Listens on `window` rather than on an element: the chord has to work with the
 * caret in the editor, with focus on a chrome button, and with focus nowhere at
 * all after the compositor has handed the window back.
 */
export function useCaptureDismissKeys(onDismiss: () => void): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) {
        return;
      }
      const closing = event.key === "w" && (event.metaKey || event.ctrlKey);
      if (event.key !== "Escape" && !closing) {
        return;
      }
      event.preventDefault();
      onDismiss();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onDismiss]);
}

/**
 * A capture window opened on a note that already exists (Story 45.15, FR-191).
 *
 * The other half of "any note openable as a capture window": the same editor
 * over the same file the Notes pane shows, in a window of its own. Nothing is
 * copied and nothing is converted, so closing this window changes nothing about
 * the note — which is exactly why the small window stops being a special kind
 * of note.
 *
 * Dismissal **saves first, is awaited, and is CHECKED**, and the check is the
 * part that took a second pair of eyes (W3Capture, 45.14, from W3NoteFile's
 * shape): `await` is not a success test when the callee catches its own
 * failure. `saveNote` catches — the editor's caption is fed from the same
 * store — so awaiting it proves only that it finished, not that the bytes
 * landed.
 *
 * Here that matters more than anywhere else in the product. The prewarmed
 * window merely *hides* on a refused write, so the words survive in a buffer on
 * a page that is handed back. **This window is DESTROYED.** Closing it over a
 * write Rust refused — a vault folder renamed out from under it, a read-only
 * volume — takes the webview, the buffer and the unsaved text with it, and says
 * nothing, because the only surface that could have said anything is the one
 * that just vanished.
 *
 * So a refused save **cancels the close**. The window stays, the words stay in
 * front of the person, and the reason is already on screen: `markSaveFailed`
 * put it in the store the editor renders from. One write, one error channel
 * (UX-DR35).
 */
export function CaptureNoteWindow({ vaultId, noteId }: { vaultId: string; noteId: string }) {
  const target: CaptureTargetVm = { kind: "note", vaultId, noteId };
  const key = captureKey(target);
  const dismiss = useCallback(() => {
    void (async () => {
      // Story 46.12: named, not "the open note". This window's editor is over
      // exactly this note, and the save that gates the close has to be that
      // note's save rather than whichever one a module singleton was holding.
      if (!(await saveNote(vaultId, noteId))) {
        return;
      }
      await closeCaptureWindow(key);
    })();
  }, [key, vaultId, noteId]);
  useCaptureDismissKeys(dismiss);

  return (
    <div className="flex h-screen min-h-0 flex-col bg-background text-foreground">
      <CaptureWindowChrome captureKey={key} onClose={dismiss} />
      <div className="min-h-0 flex-1">
        <CaptureDocument vaultId={vaultId} noteId={noteId} />
      </div>
    </div>
  );
}
