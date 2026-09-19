/**
 * The quick-capture window, in a real browser, over the mock shell.
 *
 * `dev/probe/main.tsx` measures the main window; this measures the other one.
 * They are separate pages because the app itself is: `capture.html` is its own
 * Vite entry (`vite.config.ts`), loaded by the statically declared window, and
 * it deliberately imports neither the notes surface nor mermaid. A probe that
 * mounted the capture document inside the main window's bundle would be
 * measuring a tree the product never renders.
 *
 * # Why this exists at all
 *
 * Story 75.3 merged two header rows into one and moved the window controls into
 * the editor's own header. Every number that decides whether that fits —
 * `PANE_HEADER_IDENTITY_MIN_PX`, the actions budget, the 24px window controls,
 * `CAPTURE_MIN_SIZE` — is arithmetic until a layout engine runs it. jsdom lays
 * nothing out, so the component tests can only pin structure; this page is
 * where the widths become facts. The same gap is what let a 24px overflow ship
 * in the Tasks add form two epics ago.
 *
 * It renders the real `CaptureDraftWindow` shape from `src/capture-main.tsx` —
 * the same document, chrome and title-bar hook, composed the same way. The ten
 * lines are duplicated rather than imported because that module mounts itself
 * into `#root` on import against `window.location.search`; importing it would
 * run the product's own mount before the mock shell was installed.
 */
import ReactDOM from "react-dom/client";
import { CaptureNoteWindow } from "@/components/capture/capture-window";
import { markSaveFailed } from "@/lib/stores/notes-editor";
import { installMockShell } from "../mock-shell";
import "../../src/index.css";

installMockShell();

// The NOTE branch, not the draft one, and the choice is the harness's only
// real decision. Both branches render the same document, the same chrome and
// the same merged header — but the draft asks the shell for a prewarmed page
// (`notesCaptureDraft`), which the mock does not mint, while a note window
// asks for a note the mock has had since its first fixture. Measuring the
// branch the harness can actually serve beats seeding a second fixture to
// measure the one it cannot.
const params = new URLSearchParams(window.location.search);
const vaultId = params.get("vault") ?? "v1";
const noteId = params.get("note") ?? "n1";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <CaptureNoteWindow vaultId={vaultId} noteId={noteId} />,
);

// `?error=<sentence>` renders the one caption that is not reserved for: a
// write the vault refused. It is here because that state decides a layout
// question no arithmetic and no jsdom test can answer — the caption keeps its
// box and the identity floor is released (`PaneHeaderStatus.unsqueezable`), so
// whether the sentence is actually on screen at `CAPTURE_MIN_SIZE` is a fact
// about a real layout engine. Applied after the first paint, because the
// editor's opening `Reset` would otherwise clear it.
const refused = params.get("error");
if (refused !== null && refused !== "") {
  setTimeout(() => markSaveFailed(vaultId, noteId, refused), 300);
}
