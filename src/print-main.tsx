/**
 * The page a PDF is made from (Story 56).
 *
 * # Why this is a window and not a canvas
 *
 * `pdf_export.rs` asks the webview for its PDF. So whatever renderer produces
 * the PDF has to be a real, laid-out page — and it should be the SAME renderer
 * the reader saw in the Page tab, or the file they generate is not the file they
 * were looking at. This entry point mounts that renderer and nothing else: no
 * shell, no theme, no chrome. Everything on this page is in the PDF.
 *
 * # Why it announces itself
 *
 * `createPDF` captures what has been laid out at the moment it is called. There
 * is no way from Rust to know that a webview has finished, and a sleep long
 * enough to be safe on a slow machine is a sleep every fast machine also pays.
 * So the page says when it is done, once, and Rust waits for that word.
 */
import { emit } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { buildHtmlView } from "@/components/viewers/html-view";
import { syncReadText } from "@/lib/ipc/client";

/** The word the window says when the document is on screen and laid out. */
export const PRINT_READY_EVENT = "print:ready";
/** The word it says instead when there is nothing to print. */
export const PRINT_FAILED_EVENT = "print:failed";

export function PrintDocument({ search }: { search: string }) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    const params = new URLSearchParams(search);
    const profile = params.get("profile");
    const subpath = params.get("subpath");
    if (profile === null || subpath === null) {
      void emit(PRINT_FAILED_EVENT, "the print window was opened without a file");
      return;
    }
    const id = profile;
    const path = subpath;
    let live = true;
    void syncReadText(id, path)
      .then((file) => {
        const host = hostRef.current;
        if (!live || host === null) {
          return;
        }
        if (file.text === null) {
          // A file keeper could not decode has no text to lay out, and a blank
          // PDF beside a document is worse than no PDF: it looks like the
          // document is empty rather than like the export failed.
          const said = file.detail ?? "keeper could not read this file as text";
          setFailure(said);
          void emit(PRINT_FAILED_EVENT, said);
          return;
        }
        const view = buildHtmlView(file.text);
        // The same shadow root the Page tab uses, for the same reason: the
        // document's own stylesheet must style the document and not whatever
        // this window is made of.
        const root = host.shadowRoot ?? host.attachShadow({ mode: "open" });
        const sheets = view.styles.map((css) => {
          const element = document.createElement("style");
          element.textContent = css;
          return element;
        });
        root.replaceChildren(...sheets, view.node);
        // NOT `requestAnimationFrame`, and this is the whole reason the first
        // attempt hung. The window is positioned off-screen so that nothing
        // flashes in front of the person who pressed the button — and macOS
        // treats an off-screen window as occluded, which stops it being
        // composited, which means a frame callback never fires. The page loaded,
        // read its file, mounted, and then waited for a frame that was never
        // coming; Rust waited on it for two minutes and reported a timeout about
        // the page rather than about the mechanism.
        //
        // `document.fonts.ready` is the better signal anyway, and it is a
        // promise rather than a frame: it settles when the document's own faces
        // have loaded, which is exactly what a PDF captured too early is missing.
        // The timeout after it is one turn of the event loop for the layout that
        // the fonts arriving causes.
        void document.fonts.ready.then(() => {
          setTimeout(() => {
            if (live) {
              void emit(PRINT_READY_EVENT);
            }
          }, 50);
        });
      })
      .catch((error: unknown) => {
        const said = error instanceof Error ? error.message : String(error);
        if (live) {
          setFailure(said);
          void emit(PRINT_FAILED_EVENT, said);
        }
      });
    return () => {
      live = false;
    };
  }, [search]);

  // Shown rather than left blank: this window is off-screen in the ordinary
  // case, but a person who ever sees it should read why it is empty.
  return failure === null ? <div ref={hostRef} /> : <p>{failure}</p>;
}

// Guarded so the module can be imported by a test without mounting a root.
const container = document.getElementById("root");
if (container) {
  ReactDOM.createRoot(container).render(<PrintDocument search={window.location.search} />);
}
