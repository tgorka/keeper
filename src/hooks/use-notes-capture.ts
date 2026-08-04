/**
 * The quick-capture panel's only hook (Epic 36, Stories 36.3/36.4, NFR-27).
 *
 * Binds the durable buffer in `@/lib/stores/notes-capture` to the one textarea
 * the panel renders. It does three things and refuses to grow a fourth:
 *
 * - **Focus.** The window is created hidden with the textarea already focused,
 *   so `show()` reveals a live focused node rather than racing an effect. The
 *   mount focus here is the belt to that braces, and the
 *   `keeper://notes-capture-shown` re-focus is the answer to the one case where
 *   the compositor takes focus away from us on Linux (UX-DR43).
 * - **Restore.** Rust's stored buffer is read once per mount and the caret is
 *   parked at its end, so re-summoning lands you where you stopped typing.
 * - **Commit.** Escape flushes the buffer and asks Rust to write and hide.
 *
 * There is no save, no discard and no confirm — by decision, permanently
 * (UX-DR35).
 */
import { listen } from "@tauri-apps/api/event";
import { type RefObject, useCallback, useEffect, useRef } from "react";
import {
  commitCapture,
  hydrateCaptureBuffer,
  setCaptureText,
  useNotesCaptureStore,
} from "@/lib/stores/notes-capture";

/** Emitted by Rust after the panel is shown, so focus can be re-asserted. */
export const CAPTURE_SHOWN_EVENT = "keeper://notes-capture-shown";

export interface UseNotesCapture {
  /** The panel's text. */
  text: string;
  /** The last commit failure, or null. Non-null keeps the panel open. */
  error: string | null;
  /** The textarea to focus and to park the restored caret in. */
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  /** Adopt a keystroke. */
  setText: (text: string) => void;
  /** Escape: write the note and hide the panel. */
  commit: () => void;
}

export function useNotesCapture(): UseNotesCapture {
  const text = useNotesCaptureStore((state) => state.text);
  const error = useNotesCaptureStore((state) => state.error);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const field = textareaRef.current;
    field?.focus();
    // Park the caret at the end of whatever Rust restores. Awaited rather than
    // fired and forgotten because the caret move has to happen after React has
    // rendered the restored value into the node.
    void hydrateCaptureBuffer().then(() => {
      const restored = textareaRef.current;
      if (restored) {
        const end = restored.value.length;
        restored.setSelectionRange(end, end);
      }
    });

    const unlisten = listen(CAPTURE_SHOWN_EVENT, () => {
      textareaRef.current?.focus();
    });
    return () => {
      void unlisten.then((off) => {
        off();
      });
    };
  }, []);

  const commit = useCallback(() => {
    void commitCapture();
  }, []);

  return { text, error, textareaRef, setText: setCaptureText, commit };
}
