/**
 * The registry's `text` viewer: a file in a sync profile, loaded, in whichever
 * of its two views the reader last chose (Story 45.4, FR-177, AD-88).
 *
 * **This is a binding, and almost nothing else.** Loading, dirty tracking and
 * saving are 45.6's `useTextFile`; the four states above a loaded file and the
 * mapping onto the toggle are {@link TextFileFrame}, shared with the note
 * embed Story 45.12 mounts. What is left here is the two things only this
 * surface knows: that the file is addressed by a sync profile, and that a
 * profile is not a notes vault.
 *
 * **It reads no path and joins nothing.** `file.profileId` and
 * `file.relativePath` go to Rust as they arrived; Rust re-resolves them through
 * `keeper_sync::browse`'s containment on every call (AD-65). `absolutePath` is
 * never touched here, and never rendered anywhere (FR-145).
 */
import type { ViewerProps } from "@/lib/viewers";
import { TextFileFrame } from "./text-file-frame";
import { useTextFile } from "./use-text-file";

/**
 * Why the CSV table cannot be offered from a Files panel yet.
 *
 * 44.16's `notes_csv_read` / `notes_csv_set_cell` are addressed by a **notes
 * vault id** plus a vault-relative target. A panel holds a **sync profile id**
 * plus a profile-relative path. Those are different identifiers over
 * overlapping bytes, and deriving one from the other in the webview would be
 * the frontend deciding which folders are vaults — the beginning of exactly the
 * path arithmetic AD-65 forbids.
 *
 * The resolution belongs in Rust and is Story 45.18's ("a note knows its file,
 * a file knows its note" is the same question one level up). Until it exists
 * this viewer says so and opens the source, which is editable; a host that
 * already holds vault coordinates — a note embed, Story 45.12 — passes them to
 * {@link TextFileFrame} and gets the table.
 */
const CSV_NEEDS_A_VAULT = null;

/**
 * A file in a profile is not in a notes vault as far as this surface knows, so
 * a markdown preview here resolves no embeds. Same reason as above, same story
 * to fix it.
 */
const PREVIEW_WITHOUT_A_VAULT = { vaultId: null };

export function TextFileViewer({ file, entry }: ViewerProps): React.ReactElement {
  const state = useTextFile({ profileId: file.profileId, subpath: file.relativePath });

  return (
    <TextFileFrame
      fileName={file.name}
      entry={entry}
      state={state}
      csv={CSV_NEEDS_A_VAULT}
      preview={PREVIEW_WITHOUT_A_VAULT}
    />
  );
}
