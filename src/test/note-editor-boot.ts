/**
 * Let the note editor finish booting before a test file walks away from it.
 *
 * # The failure this exists for
 *
 * `NoteEditor`'s effect awaits thirteen dynamic imports before it can build a
 * CodeMirror. The header — its title, its Actions menu — renders long before
 * they land, so a test that only needs the header finishes while the module
 * graph is still resolving. Vitest then tears the environment down underneath
 * it, and the loader throws:
 *
 * ```text
 * EnvironmentTeardownError: Cannot load '…/recording-transport.ts' imported
 * from '…/media-element.ts' after the environment was torn down.
 * ```
 *
 * It arrives as an **unhandled rejection attributed to whichever file was
 * running**, not as a failing test, so a suite reports "4683 passed" and "1
 * error" beside it — and it is a race, so it appears about one run in six.
 * Observed on `export-in-the-note-editor.test.tsx`; every file that mounts the
 * real editor and does not wait for it can lose the same race.
 *
 * # Why waiting for `.cm-content` is waiting for the graph
 *
 * It is the last thing the boot creates. If it exists, every import the boot
 * awaited has resolved and there is nothing left in flight to tear down.
 *
 * # Why this can be one `afterEach` per file
 *
 * Testing Library registers its cleanup when it is imported; a hook registered
 * in the test file is registered later and runs **first**, so this sees the
 * editor still mounted and can wait for it. (Verified, not assumed — the order
 * is the whole reason a single line per file is enough.)
 *
 * A file that mounts no editor pays nothing: with no host in the document there
 * is nothing to wait for and this returns immediately.
 */
import { waitFor } from "@testing-library/react";

/** Marks the element `NoteEditor` parents its CodeMirror into. */
export const NOTE_EDITOR_HOST_SLOT = "note-editor-host";

export async function settleNoteEditorBoot(): Promise<void> {
  if (document.querySelector(`[data-slot="${NOTE_EDITOR_HOST_SLOT}"]`) === null) {
    return;
  }
  await waitFor(() => {
    if (document.querySelector(".cm-content") === null) {
      throw new Error(
        "the note editor never finished booting. If this file mounts an editor " +
          "with no note (`noteId={null}`), the boot returns early and there is " +
          "nothing to wait for — drop this hook from that file rather than " +
          "loosening it here.",
      );
    }
  });
}
