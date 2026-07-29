/**
 * The window drag the overlay title bar's band starts (Story 34.3).
 *
 * Tauri ships a shim for `data-tauri-drag-region`: a document-level `mousedown`
 * listener that invokes `plugin:window|start_dragging` and drops the promise it
 * gets back. That is fine while it works and useless when it does not — a refused
 * drag leaves the window sitting still and leaves no trace anywhere. Two distinct
 * things refuse it, and both were live on this app:
 *
 * 1. **The Tauri ACL.** `core:window:default` does not include
 *    `allow-start-dragging` — Tauri's own custom-titlebar recipe adds it as a
 *    separate line — so until the `desktop` capability granted it, every drag the
 *    webview asked for was denied before it reached AppKit. While the window is
 *    inactive AppKit drags it natively without consulting the webview at all,
 *    which is exactly why the band moved the window until you focused it.
 * 2. **AppKit.** `start_dragging` resolves to `performWindowDragWithEvent:` on
 *    `NSApp.currentEvent`, which is honoured only while that event is the
 *    mouse-down being processed. The IPC hop is asynchronous, so the frontend can
 *    make the window as small as possible but cannot close it — only observe it.
 *
 * Hence this module: issue the drag from the app, and report each stage so the app
 * log states which of the two happened rather than leaving the next person to guess.
 */
import { startWindowDragging, type TitlebarDragStage, titlebarDragReport } from "@/lib/ipc/client";

/** Turn whatever the window plugin rejected with into one loggable line. */
function describeRefusal(error: unknown): string {
  // An ACL denial rejects with a bare string ("window.start_dragging not
  // allowed. Permissions associated with this command: …"), which is the single
  // most informative shape this can arrive in — keep it verbatim.
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "object" && error !== null) {
    const { message } = error as { message?: unknown };
    if (typeof message === "string") {
      return message;
    }
  }
  return `unrecognized rejection: ${String(error)}`;
}

/**
 * Report one stage, never letting the report itself become a failure path: a
 * diagnostic that cannot be recorded is not worth an unhandled rejection, still
 * less a broken drag.
 */
function report(stage: TitlebarDragStage, detail?: string): void {
  titlebarDragReport(stage, detail).catch(() => {});
}

/**
 * Drag the window from the mouse-down currently being delivered to the drag band.
 *
 * Call this synchronously from the `mousedown` handler and do not await anything
 * first: the whole reason a drag gets refused is that the originating event is no
 * longer current by the time Rust asks AppKit, so the drag is issued before even
 * the "issued" report is sent. Never rejects — the outcome is a log line, not an
 * error the caller has to handle.
 */
export async function beginTitleBarDrag(): Promise<void> {
  const dragging = startWindowDragging();
  report("issued");
  try {
    await dragging;
    report("accepted");
  } catch (error) {
    report("refused", describeRefusal(error));
    // Visible with devtools open; the app log is the copy that survives to a
    // bug report.
    console.warn("titlebar-drag: the window layer refused the drag", error);
  }
}
