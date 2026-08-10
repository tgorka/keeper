/**
 * A loaded file, in whichever of its two views the reader last chose — the part
 * that is the same wherever the file came from (Story 45.4, Story 45.12,
 * AD-87, AD-88).
 *
 * **Lifted out of `TextFileViewer` rather than copied into a note embed.** Two
 * surfaces now open a file this way: the registry's `text` viewer in a Files
 * panel, and a `![[…]]` embed inside a note. They differ in exactly one thing —
 * which coordinates address the file, a sync profile id or a notes vault id —
 * and 45.6's {@link useTextBuffer} already absorbs that difference. Everything
 * above it is the decision layer AD-88 asks to exist once: what to show while
 * the file is opening, what to show when there is no file, what to show when
 * the bytes are not text, and how a format keeper will not write says so.
 *
 * A second copy of those four branches is not a tidiness problem. It is the
 * shape of the defect AD-88 names: the day one surface learns that `binary`
 * means "offer no editor" and the other keeps rendering `text ?? ""`, keeper
 * has a pane that offers to save nothing over a `.png`.
 *
 * **It reads no path and joins nothing.** Every identifier arrives from the
 * caller as Rust produced it (AD-65), and `absolutePath` is never rendered
 * anywhere (FR-145).
 */
import type { CsvTableOptions } from "@/components/notes/editor/csv-table";
import type { ViewerEntry } from "@/lib/viewers";
import type { MarkdownPreviewOptions } from "./markdown-preview";
import type { CsvCoordinates } from "./raw-rendered-view";
import { RawRenderedView } from "./raw-rendered-view";
import { TextEditorSurface } from "./text-viewer";
import type { UseTextFileResult } from "./use-text-file";

export interface TextFileFrameProps {
  /** The file's own name. Display and aria only; never a path. */
  fileName: string;
  /** The registry row that chose this viewer. Passed in rather than resolved
   *  here, so the frame and the surface that mounted it cannot disagree about
   *  the format they are looking at. */
  entry: ViewerEntry;
  /** 45.6's loader, already pointed at whichever commands address this file. */
  state: UseTextFileResult;
  /**
   * Where 44.16's CSV table should read and write, or `null` when this surface
   * holds no notes-vault coordinates for the file.
   *
   * This is the only prop whose value differs by surface in a way the reader
   * can see, and the reason is worth stating where both callers can read it.
   * 44.16's `notes_csv_read` / `notes_csv_set_cell` are addressed by a notes
   * vault id plus a vault-relative target. A Files panel holds a sync profile
   * id, which is a different identifier over overlapping bytes; deriving one
   * from the other in the webview would be the frontend deciding which folders
   * are vaults (AD-65), and the resolution is Story 45.18's. A note embed
   * already holds the vault id, so it passes one and gets the table — which is
   * why the same `.csv` shows a table in a note and its source in a panel, on
   * purpose and with a sentence saying so.
   */
  csv: CsvCoordinates | null;
  /** What a markdown preview resolves embeds against. */
  preview: MarkdownPreviewOptions;
  /** Test seam for 44.16's backend, handed straight through. */
  csvOptions?: CsvTableOptions;
}

export function TextFileFrame({
  fileName,
  entry,
  state,
  csv,
  preview,
  csvOptions,
}: TextFileFrameProps): React.ReactElement {
  const { vm, content, setContent, save, reload, error, loading } = state;

  if (loading) {
    return (
      <p className="px-3 py-2 text-muted-foreground text-xs" role="status">
        opening {fileName}
      </p>
    );
  }

  // No VM at all: unreadable, or outside every profile. The loader has already
  // worded it — including the case where there is nothing to read because the
  // file is not inside a synced folder, and the case where the vault does not
  // have the file an embed names, where Rust's sentence lists the paths it
  // looked for.
  if (vm === null) {
    return (
      <p className="px-3 py-2 text-destructive text-xs" role="alert">
        {error ?? `keeper could not open ${fileName}`}
      </p>
    );
  }

  // Bytes that are not text. Rendering `text ?? ""` here would put an empty
  // editable pane over a binary file and offer to save it, which is how an
  // editor overwrites a `.png` with nothing.
  if (vm.binary) {
    return (
      <p className="px-3 py-2 text-destructive text-xs" role="alert">
        {vm.detail ?? `${fileName} is not text, so keeper will not open it in an editor`}
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
  // The LOCATION's answer arrives two ways. A file this surface cannot address
  // never gets this far — the loader has already returned no VM and its own
  // sentence. A file keeper cannot write to is Rust's answer, and it arrives as
  // a refused save carrying Rust's own words, in the banner below. Guessing it
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
          fileName={fileName}
          format={entry.format}
          rendered={entry.rendered}
          language={entry.language}
          content={content}
          sizeLabel={vm.sizeLabel}
          readOnly={refusal !== null}
          readOnlyReason={refusal}
          onChange={setContent}
          // The editor hands over its exact current text and the loader already
          // holds it: `onChange` has fired for every keystroke that produced
          // it, so `save()` writes the same characters the editor just named.
          onSave={() => save()}
          csv={csv}
          preview={preview}
          onExternalWrite={() => void reload()}
          editor={TextEditorSurface}
          csvOptions={csvOptions}
        />
      </div>
    </div>
  );
}
