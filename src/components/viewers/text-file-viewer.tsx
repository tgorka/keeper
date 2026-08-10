/**
 * The registry's `text` viewer: a file, loaded, in whichever of its two views
 * the reader last chose (Story 45.4, FR-177, AD-88).
 *
 * **This is the only stateful layer, and it owns none of the three things it
 * composes.** Loading, dirty tracking and saving are 45.6's `useTextFile`; the
 * editor is 45.6's `TextEditorSurface`; the toggle, the rendered views and the
 * parse banner are {@link RawRenderedView}. What this file adds is the mapping
 * from 45.2's `ViewerProps` onto those three, and the sentences for the states
 * where there is nothing to show yet — which is exactly the seam AD-88 asks
 * for: one place that decides, three pieces that do not each grow their own
 * copy of the decision.
 *
 * **It reads no path and joins nothing.** `file.profileId` and
 * `file.relativePath` go to Rust as they arrived; Rust re-resolves them through
 * `keeper_sync::browse`'s containment on every call (AD-65). `absolutePath` is
 * never touched here, and never rendered anywhere (FR-145).
 */
import type { ViewerProps } from "@/lib/viewers";
import { RawRenderedView } from "./raw-rendered-view";
import { TextEditorSurface } from "./text-viewer";
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
 * {@link RawRenderedView} directly and gets the table.
 */
const CSV_NEEDS_A_VAULT = null;

export function TextFileViewer({ file, entry }: ViewerProps): React.ReactElement {
  const { vm, content, setContent, save, reload, error, loading } = useTextFile({
    profileId: file.profileId,
    subpath: file.relativePath,
  });

  if (loading) {
    return (
      <p className="px-3 py-2 text-muted-foreground text-xs" role="status">
        opening {file.name}
      </p>
    );
  }

  // No VM at all: unreadable, or outside every profile. The hook has already
  // worded it — including the case where there is nothing to read because the
  // file is not inside a synced folder.
  if (vm === null) {
    return (
      <p className="px-3 py-2 text-destructive text-xs" role="alert">
        {error ?? `keeper could not open ${file.name}`}
      </p>
    );
  }

  // Bytes that are not text. Rendering `text ?? ""` here would put an empty
  // editable pane over a binary file and offer to save it, which is how an
  // editor overwrites a `.png` with nothing.
  if (vm.binary) {
    return (
      <p className="px-3 py-2 text-destructive text-xs" role="alert">
        {vm.detail ?? `${file.name} is not text, so keeper will not open it in an editor`}
      </p>
    );
  }

  // 45.2's rule is that writability is two questions and both must say yes.
  //
  // The FORMAT's answer is `entry.writable`. No `viewer: "text"` row is
  // non-writable today, so the registry cannot currently produce this input —
  // which is exactly why the guard is here and why its test builds the row by
  // hand. The day somebody adds a text-shaped format keeper must not rewrite,
  // the reader gets a sentence instead of an editor that silently refuses.
  //
  // The LOCATION's answer arrives two ways. A file inside no profile never gets
  // this far — the hook has already returned no VM and its own sentence. A file
  // in a profile keeper cannot write to is Rust's answer, and it arrives as a
  // refused save carrying Rust's own words, in the banner below. Guessing it
  // here would mean the frontend deciding which volumes are writable.
  const refusal = entry.writable
    ? null
    : `keeper does not write ${entry.label} files: a lossy round trip through this format is how people lose work`;

  return (
    <div className="flex h-full min-h-0 flex-col">
      {error === null ? null : (
        <p className="shrink-0 border-b px-3 py-1.5 text-destructive text-xs" role="alert">
          {error}
        </p>
      )}
      <div className="min-h-0 flex-1">
        <RawRenderedView
          fileName={file.name}
          format={entry.format}
          rendered={entry.rendered}
          language={entry.language}
          content={content}
          sizeLabel={vm.sizeLabel}
          readOnly={refusal !== null}
          readOnlyReason={refusal}
          onChange={setContent}
          // The editor hands over its exact current text and the hook already
          // holds it: `onChange` has fired for every keystroke that produced
          // it, so `save()` writes the same characters the editor just named.
          onSave={() => save()}
          csv={CSV_NEEDS_A_VAULT}
          preview={{ vaultId: null }}
          onExternalWrite={() => void reload()}
          editor={TextEditorSurface}
        />
      </div>
    </div>
  );
}
