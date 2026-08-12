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
 *
 * # The Save control lives here, and not on the panel's header
 *
 * Saving was `Mod-s` and nothing else (Story 46.13, FR-216). There is no
 * autosave for a file and that is deliberate — `spec-45-6` — so the only
 * feedback a reader had that their edit was still in the buffer was that nothing
 * had happened, which is indistinguishable from a save that worked.
 *
 * The button is in this frame's own chrome rather than in `PanelFrame`'s header
 * because `dirty` and `save` live in the hook that is mounted *below* that
 * header. A header button would need a registry of per-panel save functions kept
 * in step with mounts and unmounts, and a registry is a worse thing to own than
 * a button in the right place. It also means a note embed of a file gets the
 * control for free, where a panel header would have given it nothing.
 *
 * The bar exists exactly when there is a Save to offer — a writable format that
 * was not truncated on the way in. A header whose only reason to exist is a
 * control that is not there is chrome for its own sake, and a reserved status
 * slot that can never say anything is 8px of nothing.
 */
import { PaneHeader } from "@/components/layout/pane-header";
import type { CsvTableOptions } from "@/components/notes/editor/csv-table";
import { Button } from "@/components/ui/button";
import type { ViewerEntry } from "@/lib/viewers";
import type { MarkdownPreviewOptions } from "./markdown-preview";
import type { CsvCoordinates } from "./raw-rendered-view";
import { RawRenderedView } from "./raw-rendered-view";
import { TextEditorSurface } from "./text-viewer";
import type { UseTextFileResult } from "./use-text-file";

/**
 * The one word this bar says about the buffer, and why the informative state is
 * the opposite of the note editor's.
 *
 * A note autosaves, so the fact worth carrying is that the write landed and
 * when: `Saved · HH:MM`, and silence while you type. A file does not autosave,
 * so the fact worth carrying is the one the reader can still act on — there is
 * something here that is not on disk — and silence once it is. Same slot, same
 * reservation mechanism, opposite polarity, because the two surfaces make
 * opposite promises.
 *
 * Derived by a function rather than written twice so {@link FILE_SAVE_SIZERS}
 * cannot drift from what the caption actually shows.
 */
export function fileSaveWord(dirty: boolean): string {
  return dirty ? "Unsaved changes" : "";
}

/**
 * Every string the status slot can show, for `PaneHeader` to measure.
 *
 * One entry: the clean state is deliberately empty, and an empty string reserves
 * nothing. No clock and no locale here — unlike the note editor's caption — but
 * the width is still the browser's answer rather than ours, because it is a font
 * and a translation away from being wrong.
 */
export const FILE_SAVE_SIZERS: readonly string[] = [fileSaveWord(true)];

/** The Save control. Named as the verb, not as the state. */
export const FILE_SAVE_LABEL = "Save";

/**
 * Why Save is disabled when it is.
 *
 * Disabled rather than absent, which is the opposite of what this codebase does
 * for a control that cannot act — and the difference is that "nothing has
 * changed" is a state the reader leaves by typing, where "keeper will not write
 * this format" is not. A control that vanished every time the buffer matched the
 * disk would be a control nobody could find on purpose. The sentence is on the
 * button so the disabled state is never a mystery, which is the actual house
 * rule the absent-not-disabled idiom serves.
 */
export const FILE_SAVE_CLEAN_TITLE = "Nothing has changed since this file was last read or saved.";
/**
 * Test id for AD-102's standing caveat — the sentence a file keeper writes but
 * does not manage carries before it is edited.
 *
 * A slot rather than a text match, because the sentence itself is composed in
 * Rust and asserted there; what this surface owes is that it is on screen, and
 * on screen before the editor.
 */
export const TEXT_FILE_CAVEAT_TESTID = "text-file-caveat";

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
  /**
   * The standing sentence for a file keeper will write and does not manage, or
   * `null` (Story 46.14, AD-102).
   *
   * Composed in Rust (`FilesWriteVm.caveat`) and rendered verbatim. Rendered
   * ABOVE `error` and outside the `savable` gate on purpose: the caveat is a
   * standing fact about the file and the error is about the last action, and
   * the standing fact has to be on screen before the first keystroke — an edit
   * that quietly does less than the vault path does is worse than the refusal
   * it replaced.
   */
  writeCaveat?: string | null;
  /** Test seam for 44.16's backend, handed straight through. */
  csvOptions?: CsvTableOptions;
}

export function TextFileFrame({
  fileName,
  entry,
  state,
  writeCaveat = null,
  csv,
  preview,
  csvOptions,
}: TextFileFrameProps): React.ReactElement {
  const { vm, content, setContent, dirty, save, reload, error, loading } = state;

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

  // Story 46.13: the bar exists exactly when a Save could land. `refusal` is the
  // format's no; `vm.oversize` means only a prefix was read and the loader
  // declines a save that would truncate the rest — offering a button that
  // announces its own refusal is the shape 45.2 spent a paragraph rejecting.
  const savable = refusal === null && !vm.oversize;

  return (
    <div className="flex h-full min-h-0 flex-col">
      {savable ? (
        <PaneHeader
          // Horizontal padding only. The bottom edge and the 40px height are
          // `PaneHeader`'s now; this used to spell `border-b` (a second edge
          // under the component's) and `py-1.5` (a 44px row where the other
          // two callers were 40).
          className="px-3"
          // The file's own name, which nothing inside this frame renders — the
          // panel's header names it too, and the note embed that mounts this has
          // no header at all, so this is the one identity both hosts can rely on.
          identity={<span className="min-w-0 flex-1 truncate font-medium text-xs">{fileName}</span>}
          status={{ sizers: FILE_SAVE_SIZERS, caption: fileSaveWord(dirty) }}
          actions={
            <Button
              type="button"
              variant="outline"
              size="xs"
              disabled={!dirty}
              title={dirty ? undefined : FILE_SAVE_CLEAN_TITLE}
              // The same call `Mod-s` makes, and the only one: the hook holds the
              // buffer, so there is nothing for this to pass and nothing it could
              // pass that would differ from what the editor last reported.
              onClick={() => void save()}
            >
              {FILE_SAVE_LABEL}
            </Button>
          }
        />
      ) : null}
      {writeCaveat === null ? null : (
        <p
          data-testid={TEXT_FILE_CAVEAT_TESTID}
          className="shrink-0 border-b px-3 py-1.5 text-muted-foreground text-xs"
          role="status"
        >
          {writeCaveat}
        </p>
      )}
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
