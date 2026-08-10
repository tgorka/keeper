/**
 * The quick-capture window's entry point (AD-60, AD-93, NFR-27, UX-DR35).
 *
 * This file used to say that the panel was "deliberately the smallest React
 * application in the repo… no editor", because a textarea was the whole surface
 * and every kilobyte of editor was a kilobyte parsed before the first keystroke
 * could land. Story 45.14 overturns that, deliberately and by the owner
 * (AD-93): quick capture mounts the **real** `NoteEditor`, because "quick
 * capture takes text and refuses a tag" and "quick capture cannot do markdown"
 * and four sibling complaints are one complaint — there were two editors.
 *
 * # NFR-27 is not paid for at the hotkey, and never was
 *
 * The 300 ms budget belongs to the *show*, not to the mount. `notes_window`
 * creates this window hidden at startup and never destroys it, so the hotkey
 * path is `set_position` → `show` → `set_focus` and nothing else. Everything
 * expensive happens once, at app start, with nobody waiting:
 *
 *   - the editor's chunk, which `NoteEditor` fetches behind one `import()` in
 *     its boot effect;
 *   - the page itself, which `useCaptureDraft` resolves at mount and re-arms
 *     after each dismissal, while the window is hidden.
 *
 * By the time the chord is pressed the window holds a live, focused editor on a
 * live note. That is the same trick the window itself has always used, applied
 * to the document — and it is the reason the resolve is on mount and on hide
 * rather than on show.
 *
 * # One entry point, several windows (Story 45.15)
 *
 * There is no longer one capture window. The prewarmed one is still declared in
 * `tauri.conf.json`, created hidden at startup and never destroyed; every other
 * one is created on demand, holds a note that already exists, and is destroyed
 * when it is closed. They are all this document — a window learns which it is
 * from its own URL, because reading a query string costs nothing and an IPC
 * round trip in front of the first keystroke is the one thing NFR-27 forbids.
 *
 * Both branches get the same chrome: a close button, because Escape being the
 * only way out is a way out nobody discovers, and a lock, because an
 * undecorated window that keeper always places is a window you cannot put where
 * you need it.
 *
 * What is still deliberately absent, and must stay absent: a title field, a
 * folder picker, a save button, a discard affordance. Escape files the thought;
 * nothing anywhere on this window discards text.
 */
import ReactDOM from "react-dom/client";
import { CaptureDraftDocument } from "@/components/capture/capture-document";
import { CaptureNoteWindow, CaptureWindowChrome } from "@/components/capture/capture-window";
import { captureTargetFromSearch, DRAFT_CAPTURE_KEY } from "@/lib/capture-target";
import "./index.css";

export function CapturePanel({ search }: { search: string }) {
  const target = captureTargetFromSearch(search);
  if (target.kind === "note") {
    return <CaptureNoteWindow vaultId={target.vaultId} noteId={target.noteId} />;
  }
  // The prewarmed window. Its chrome is handed the document's OWN dismissal, so
  // the close button and Escape are one act rather than two spellings of one —
  // filing the thought, hiding, and arming a fresh page for next time.
  return (
    <CaptureDraftDocument
      captureKey={DRAFT_CAPTURE_KEY}
      chrome={(dismiss) => <CaptureWindowChrome captureKey={DRAFT_CAPTURE_KEY} onClose={dismiss} />}
    />
  );
}

// Guarded so the module can be imported by a test without mounting a root.
const container = document.getElementById("root");
if (container) {
  ReactDOM.createRoot(container).render(<CapturePanel search={window.location.search} />);
}
