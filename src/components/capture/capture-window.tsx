/**
 * A capture window's own chrome (Story 45.15, FR-191, FR-192, UX-DR77, AD-260).
 *
 * The window is quick capture's; the document is not (AD-93). These three
 * controls are the whole of "the window": a way out, and a way to decide where
 * the window lives. Nothing here knows what a note is.
 *
 * # Where they are drawn, since Story 75.3
 *
 * They were a 32px strip of their own above the document, and the document's
 * own `PaneHeader` was 40px below it: 72px of chrome over a window whose
 * default height is 340. AD-260 merges them — the three controls are handed to
 * the editor's header as its **frame group** (Story 50.1, built for exactly
 * this) and the strip is gone.
 *
 * So this component no longer owns a row. It owns a *cluster*, and the two
 * things that were properties of the window rather than of the strip travel
 * with it: the conditional drag region and DW-199's Rust-measured corner
 * inset. The header it lands in becomes the window's title bar, which is one
 * fact with three consequences and is written down once, in `PaneHeader`'s
 * {@link PaneHeaderTitleBar}; {@link useCaptureWindowTitleBar} is how a capture
 * host says it.
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
 * # Why a lock, and why it is OFF at every open
 *
 * A capture window is undecorated, so it has no title bar of the platform's to
 * drag. Without an affordance it can only be where keeper puts it — a fifth of
 * the way down the monitor holding the pointer — which is the right answer for
 * a thing you type into and dismiss, and the wrong answer for a window you want
 * beside the thing you are reading. Unlocking turns the header into a drag
 * region (`data-tauri-drag-region`, which is the only way an undecorated Tauri
 * window can be moved) and tells Rust to remember where it ends up.
 *
 * **This paragraph used to say the opposite, and the rule has changed.** Until
 * Epic 75 locked was the default and persisted, on the reasoning that it was
 * what keeper had always done and a feature nobody asked for should stay
 * invisible. AD-258 rescinds that. The lock is a gesture about the capture in
 * front of you, not a preference about capture windows: it freezes the
 * geometry, and until AD-257 it also silently resized the window out from under
 * whoever had just resized it. So the open path forces it false — a window that
 * was locked when it was dismissed comes back movable — and the optimistic
 * defaults below assume UNLOCKED rather than locked, because that is now the
 * state a window arrives in.
 *
 * The pin is deliberately not symmetrical with it (AD-259): floating above
 * other applications is a preference about how this window behaves, so an
 * explicit pin survives the next open while the lock does not. That asymmetry
 * is the epic's, and it is written here so the next reader does not "fix" it.
 *
 * The lock state and the position are Rust's, not this component's: they
 * survive a restart, and a webview that stored its own window position would be
 * storing it per-document in a document that is destroyed when the window
 * closes.
 */
import { Lock, LockOpen, Pin, PinOff, X } from "lucide-react";
import { useCallback, useEffect, useMemo } from "react";
import { CaptureDocument } from "@/components/capture/capture-document";
import type { PaneHeaderTitleBar } from "@/components/layout/pane-header";
import { Button } from "@/components/ui/button";
import { IconHint } from "@/components/ui/tooltip";
import { saveNote } from "@/hooks/use-notes-body";
import { captureKey } from "@/lib/capture-target";
import {
  type CaptureTargetVm,
  type CaptureWindowVm,
  listenNotesCaptureWindows,
} from "@/lib/ipc/client";
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
 * **It names both now, and it used to name only the position.** The old wording
 * was "Lock this window where it is", chosen deliberately over "as it is"
 * because `Placement::window_size`'s locked arm returned `CAPTURE_DEFAULT_SIZE`
 * — locking really did hand the window back at keeper's own size, and a label
 * promising otherwise would have been a lie the user could see. AD-257 makes
 * the locked arm answer the remembered size, so the sentence that was accurate
 * is now the one that misleads: the size the user just dragged to is exactly
 * what the click freezes. Naming the position alone would leave the half of the
 * act the owner asked for undiscoverable — the epic exists because the size
 * snapped back.
 *
 * The pair below it says "moved and resized"; this one is its inverse, in the
 * same two nouns. Still NOT promised: the restore. `set_position` is the call a
 * Wayland compositor may refuse (UX-DR43), so neither label says "remembers".
 */
export const CAPTURE_LOCK_LABEL = "Lock this window's position and size";

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

/**
 * The header's horizontal padding, in pixels, as the capture host spells it.
 *
 * `px-3` on the `PaneHeader` below as a number, because DW-199's inset
 * arithmetic has to subtract it and a Tailwind class cannot be read from
 * JavaScript. The class in `NoteEditor` and this constant are one fact written
 * twice and MUST change together — the same bargain `PANE_HEADER_GAP_PX`
 * strikes with `gap-2`.
 */
export const CAPTURE_HEADER_GUTTER_PX = 12;

/**
 * This window's row, live, for a component that only wants to read it.
 *
 * Deliberately without the hydrate-and-listen effect that
 * {@link CaptureWindowChrome} runs: in a capture host the two are mounted in
 * the same tree — the chrome IS the header's frame group — so a second
 * subscription would be a second IPC read and a second event listener for the
 * same list. The store is a module singleton, so whichever of them is on screen
 * fills it for both.
 */
function useCaptureWindowRow(key: string): CaptureWindowVm | null {
  return useCaptureWindowsStore((state) => captureWindowFor(state, key));
}

/**
 * What a capture host passes to `PaneHeader` to say "this row is my title bar".
 *
 * One hook rather than an object spelled at each call site, because the default
 * below is a decision and not a fallback — see {@link CaptureWindowChrome} for
 * why unknown now reads as UNLOCKED — and two copies of it would disagree the
 * first time one of them was updated.
 */
export function useCaptureWindowTitleBar(key: string): PaneHeaderTitleBar {
  const window = useCaptureWindowRow(key);
  const draggable = !(window?.locked ?? false);
  return useMemo(() => ({ draggable }), [draggable]);
}

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
   * dismissal *means* differs by window and neither meaning belongs to three
   * buttons: the prewarmed window files its page and hides so the next hotkey
   * press is still instant, and a window opened on a note closes.
   */
  onClose: () => void;
}

export function CaptureWindowChrome({ captureKey, onClose }: CaptureWindowChromeProps) {
  const window = useCaptureWindowRow(captureKey);
  // Unknown reads as UNLOCKED, and that is the opposite of what it read before
  // Epic 75. The old reasoning was "until Rust has answered, the window behaves
  // the way it always has" — and the way it always was, was locked. AD-258
  // makes every open unlocked, so the optimistic answer that matches reality
  // has flipped with it: assuming locked would now draw a closed padlock over a
  // window nobody locked, and offer to unlock something that is not locked.
  const locked = window?.locked ?? false;
  // …and unknown reads as NOT on top, for the same reason with a different
  // decision behind it: AD-259 makes `false` the shipped default, so an
  // unanswered read is a window that is almost certainly an ordinary window.
  // Unlike the lock this one persists, so a pinned window shows a lit pin one
  // frame late rather than never.
  const alwaysOnTop = window?.alwaysOnTop ?? false;
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
        // A listener that could not be attached costs this cluster its live
        // lock state, never its buttons: both actions read their argument from
        // the props, not from the store.
      });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

  return (
    <div
      // The drag region is the unlocked window's entire mechanism, and it is
      // conditional rather than always-on: a locked window whose chrome was a
      // drag region would move when the user meant to click, and "locked" would
      // be a label rather than a fact.
      //
      // It is on THIS box as well as on the header and the identity group
      // (`PaneHeaderTitleBar`) because Tauri's shim matches the exact element
      // the press landed on and does not walk up its ancestors — and this box
      // covers the header's frame wrapper entirely, so the wrapper's own
      // marking would have no exposed area at all.
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
      //
      // What Story 75.3 changed is the RIGHT edge, and it is a subtraction
      // rather than an addition. The old strip's own padding was `px-1` — 4px —
      // so it added the inset on top of it. This cluster sits at the end of a
      // header that already keeps CAPTURE_HEADER_GUTTER_PX (12) of gutter to
      // the window's right edge, which is more than the inset is at 1x (5) or
      // at 2x (10): the buttons are already clear and a second gutter would be
      // 10 to 14 pixels of the row's width spent on nothing. Only a 3x display
      // (15) needs anything, and then only the 3 the gutter does not cover.
      //
      // The TOP edge still adds, because the row has no vertical padding to
      // borrow from: the 24px controls are centred in 40px, so 8px of the inset
      // is already clear, and padding P lands the glyph's top edge at 8 + P/2
      // — ahead of the inset for every inset up to 16, which covers 1x through
      // 3x. At the fixture's 10 the glyph starts at y=13 against a 10px border.
      style={
        chromeInset > 0
          ? {
              paddingTop: chromeInset,
              paddingRight: Math.max(0, chromeInset - CAPTURE_HEADER_GUTTER_PX),
            }
          : undefined
      }
      className={`flex items-center gap-2 ${locked ? "" : "cursor-grab active:cursor-grabbing"}`}
    >
      {/*
       * Left of the lock, so the close button stays flush at the top-right
       * corner where DW-199's inset protects it — the corner geometry 47.5
       * measured is unchanged by the row this cluster now lives in.
       *
       * `icon-xs` (24px) and not `icon` (36) or `icon-sm` (32), and the
       * arithmetic is the reason rather than the taste: the row is 40px, and
       * this cluster carries a top padding of the DW-199 inset, which is 10 on
       * the 2x GTK display the fixtures model and 15 at 3x. 32 + 10 = 42
       * overflows the row by two pixels and 32 + 15 = 47 by seven; 24 + 15 = 39
       * fits at every scale the shell can report. The glyph is 12px, which is
       * `icon-xs`'s own rule and legible at the type scale this header uses.
       */}
      <IconHint label={alwaysOnTop ? CAPTURE_UNPIN_LABEL : CAPTURE_PIN_LABEL}>
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label={alwaysOnTop ? CAPTURE_UNPIN_LABEL : CAPTURE_PIN_LABEL}
          aria-pressed={alwaysOnTop}
          onClick={() => {
            void setCaptureWindowAlwaysOnTop(captureKey, !alwaysOnTop);
          }}
        >
          {alwaysOnTop ? <Pin aria-hidden="true" /> : <PinOff aria-hidden="true" />}
        </Button>
      </IconHint>
      <IconHint label={locked ? CAPTURE_UNLOCK_LABEL : CAPTURE_LOCK_LABEL}>
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label={locked ? CAPTURE_UNLOCK_LABEL : CAPTURE_LOCK_LABEL}
          aria-pressed={!locked}
          onClick={() => {
            void setCaptureWindowLocked(captureKey, !locked);
          }}
        >
          {locked ? <Lock aria-hidden="true" /> : <LockOpen aria-hidden="true" />}
        </Button>
      </IconHint>
      <IconHint label={CAPTURE_CLOSE_LABEL}>
        <Button variant="ghost" size="icon-xs" aria-label={CAPTURE_CLOSE_LABEL} onClick={onClose}>
          <X aria-hidden="true" />
        </Button>
      </IconHint>
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
  const titleBar = useCaptureWindowTitleBar(key);
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

  // One child where there were two, and the flex column is kept rather than
  // collapsed: the document still has to be the thing that takes the height a
  // header does not, and `h-screen min-h-0` is what stops the editor's own
  // scroller growing the page instead of scrolling inside it.
  return (
    <div className="flex h-screen min-h-0 flex-col bg-background text-foreground">
      <div className="min-h-0 flex-1">
        <CaptureDocument
          vaultId={vaultId}
          noteId={noteId}
          titleBar={titleBar}
          frame={<CaptureWindowChrome captureKey={key} onClose={dismiss} />}
        />
      </div>
    </div>
  );
}
