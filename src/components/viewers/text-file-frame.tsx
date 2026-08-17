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
 *
 * # The writing tools are decided here and mounted two levels down
 *
 * Story 50.3 gives a markdown file the format toolbar, the slash menu and emoji
 * completion (FR-233). The DECISION is here, because this is the one place that
 * holds both halves of it — the registry row that says what the format is, and
 * the same `savable` flag the Save button stands on. The MOUNT is in the raw
 * editor, because a toolbar acts on a live view and this frame holds none: what
 * travels down is a boolean, never an editor handle.
 *
 * Story 51.5's Note tab is decided in the same breath and by the same
 * predicate. It is a way of writing text too — the live-preview editor over the
 * same buffer and the same Save (FR-294) — so it appears exactly where the
 * toolbar and the Save button do, and a `workspace/` file (AD-113) or an
 * oversize one gets Preview and Source as before. Absent rather than
 * present-and-refusing: a third tab that opened an editor nothing would accept
 * is the shape 45.2 spent a paragraph rejecting.
 *
 * # And so are the file's own properties
 *
 * Story 50.4 mounts the notes properties panel over a file (FR-283). Same
 * decision, same predicate, one level up: `entry.format === "markdown" &&
 * savable`, because on a sessions zone a tag is not decoration — it is what
 * decides which space lists the file (AD-120) — and a panel over a buffer no
 * save can follow would be controls that announce their own refusal.
 *
 * What differs from the toolbar is that a property write does not go through
 * this buffer. It is its own command over the file's bytes, so the buffer must
 * re-read afterwards; `reload` is handed over for exactly that, and without it
 * a later Save would put the old block back.
 */
import { PaneHeader } from "@/components/layout/pane-header";
import type { CsvTableOptions } from "@/components/notes/editor/csv-table";
import { FileProperties } from "@/components/notes/properties-panel";
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

/**
 * Where a file's own properties are addressed (Story 50.4, FR-283).
 *
 * The pair `(profile id, profile-relative subpath)` and nothing else — the same
 * two identifiers 45.6's loader was built from, and both arrive from the host
 * exactly as Rust produced them (AD-65).
 */
export interface FilePropertiesCoordinates {
  profileId: string;
  relativePath: string;
}

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
   * Where this file's own properties live, or `null` when this surface holds no
   * sync-profile coordinates for it (Story 50.4, FR-283).
   *
   * Shaped after {@link csv} and for the identical reason: the pair of commands
   * behind it is addressed by a sync profile id and a profile-relative subpath,
   * which a Files panel holds and a note embed does not. Deriving one address
   * from the other in the webview would be the frontend deciding which folders
   * are which (AD-65), so the host that has the address passes it and the host
   * that has none passes `null` and gets no panel.
   *
   * Coordinates rather than a rendered panel: the frame decides WHETHER — a
   * writable markdown file and nothing else — and the panel decides everything
   * about what a property is. The same split 50.3 used for the writing tools.
   */
  properties: FilePropertiesCoordinates | null;
  /**
   * Re-address this surface at the file the properties panel just RENAMED, or
   * omitted when the host holds no panel target to move (Story 52.2, FR-302).
   *
   * `next` is the file's new profile-relative subpath exactly as
   * `sessions_file_rename` answered it (AD-65) — the frame joins nothing. A
   * host that omits this keeps today's behaviour precisely: the panel calls
   * `onWritten` and this frame re-reads the address it already holds.
   */
  onPropertiesRenamed?: (next: string) => void;
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
  /**
   * Why keeper will not write this file's LOCATION, or `null` (Story 45.3's
   * `FilesWriteVm`, threaded by Story 50.3's fix).
   *
   * The half of 45.2's two questions this frame used to be missing. It had the
   * FORMAT's verdict (`entry.writable`) and it discovered the LOCATION's only
   * by attempting a save and rendering Rust's refusal afterwards — which is
   * honest for a volume that goes read-only under the reader's hands, and
   * wrong for a fence that was already known when the row was listed. A
   * session's `workspace/` file (AD-113) is the case: markdown, writable
   * format, and every write refused. Without this it got the Save button, the
   * format toolbar, the slash menu and emoji completion over a buffer nothing
   * would ever accept.
   *
   * Rust's own sentence, rendered verbatim in the read-only notice. A default
   * of `null` for the note embed, which addresses a vault rather than a
   * profile and holds no such verdict.
   */
  writeRefusal?: string | null;
  /** Test seam for 44.16's backend, handed straight through. */
  csvOptions?: CsvTableOptions;
}

export function TextFileFrame({
  fileName,
  entry,
  state,
  writeCaveat = null,
  writeRefusal = null,
  csv,
  properties,
  onPropertiesRenamed,
  preview,
  csvOptions,
}: TextFileFrameProps): React.ReactElement {
  const { vm, content, setContent, dirty, save, reload, error, loading, loadedFrom } = state;

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
  // The LOCATION's answer is `writeRefusal`, and it is Rust's — composed by
  // `keeper_sync::files_write::WriteRefusal` and carried on the listing row
  // the panel opened, so consulting it is not the frontend deciding which
  // volumes are writable; re-deriving it here would be. Story 50.3 shipped
  // without it and the hole was exact: a session's `workspace/` file (AD-113)
  // is markdown, of a writable format, that every write refuses, so it got the
  // toolbar, the menu, the completion and a Save button, and was not even
  // marked read-only. A refusal that only exists AFTER a save is attempted can
  // be the whole answer for a volume that goes read-only under the reader's
  // hands; it cannot be the answer for a fence that was known when the row was
  // listed.
  //
  // Format first when both speak: the format's no is about the file itself and
  // survives moving it, and a reader who is told the deeper fact acts on it.
  const refusal = entry.writable
    ? writeRefusal
    : `keeper does not write ${entry.label} files: a lossy round trip through this format is how people lose work`;

  // Story 46.13: the bar exists exactly when a Save could land. `refusal` is
  // every standing no — the format's and the location's; `vm.oversize` means
  // only a prefix was read and the loader declines a save that would truncate
  // the rest — offering a button that announces its own refusal is the shape
  // 45.2 spent a paragraph rejecting.
  const savable = refusal === null && !vm.oversize;

  // Story 50.3's verdict, and both halves of it are here.
  //
  // MARKDOWN is `entry.format`, the registry's own answer (AD-87). Never the
  // file name: `src/lib/viewers` owns extension-to-format and a second sniff
  // here is how one surface comes to think a `.mkd` is prose and another does
  // not.
  //
  // WRITABLE is `savable`, the same flag that decides whether Save exists. That
  // equality is the point rather than a convenience: FR-233's tools are ways of
  // writing text, and offering them where no save can follow would be the
  // shape 45.2 spent a paragraph rejecting — a control that announces its own
  // refusal. Since `savable` now reads the location too, `workspace/` markdown
  // is inside that rule rather than beside it: the tools are off it because
  // keeper will not write there, which is the reason a reader would give.
  const writingTools = entry.format === "markdown" && savable;

  // Story 50.4's verdict, and it is deliberately the SAME predicate.
  //
  // A property is a way of writing text too — on a sessions zone it is the way
  // a file gets filed at all (AD-120) — so the panel appears exactly where the
  // writing tools do and exactly where a Save can land. A markdown file with no
  // save behind it would get a panel whose every control announced its own
  // refusal, which is the shape 45.2 rejected; a `.csv` has no frontmatter to
  // show in the first place (matrix row 9).
  //
  // `workspace/` (row 8) is still Rust's answer and not a second opinion about
  // which folders are scratch — what changed with 50.3's fix is only WHEN that
  // answer arrives. It now rides in on the listing row as `writeRefusal`, so
  // the panel is absent rather than mounted and then emptied by a rejected
  // read inside `FileProperties`. Both layers still refuse; the inner one is
  // what makes the claim true for a host that passes no refusal at all.
  const propertiesPanel = writingTools ? properties : null;

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
      {/* Above the editor and below the banners, which is where the note editor
          keeps it: the properties of a file are a standing fact about it, and
          the banners are about the last action. `reload` is passed as the
          after-write hook because a properties write changes the bytes this
          buffer is holding — without it a later Save would put the old block
          back, which is the one way this panel could lose somebody's edit. */}
      {propertiesPanel === null ? null : (
        <div className="shrink-0">
          <FileProperties
            profileId={propertiesPanel.profileId}
            relativePath={propertiesPanel.relativePath}
            onWritten={() => void reload()}
            onRenamed={onPropertiesRenamed}
          />
        </div>
      )}
      <div className="min-h-0 flex-1">
        <RawRenderedView
          fileName={fileName}
          // Which file this is, straight off the loader that read it rather than
          // down a second prop chain: the two hosts address a file differently —
          // a sync profile and a subpath here, a notes vault and a resolved
          // target in an embed (AD-65 forbids deriving either from the other) —
          // and `useTextBuffer` is already where that difference is absorbed. So
          // the identity cannot come to disagree with the bytes it describes,
          // which a prop each host assembled for itself could.
          loadedFrom={loadedFrom}
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
          writingTools={writingTools}
          // The same flag, deliberately, and passed twice because the view asks
          // two different questions of it: which tools its editor mounts, and
          // whether a third tab exists. Story 51.5's mode is a way of writing
          // text, so it lives exactly where a Save can land — see the header.
          noteMode={writingTools}
          // The same value the panel above is drawn from, and never a second
          // opinion about it: the block is hidden from the live-preview panes
          // exactly when this frame drew it as a form, so the two cannot come to
          // disagree about whether it is on screen twice (Story 52.3, FR-304).
          // The Source tab still shows every byte, and a save still writes them.
          frontmatterInForm={propertiesPanel !== null}
          onExternalWrite={() => void reload()}
          editor={TextEditorSurface}
          csvOptions={csvOptions}
        />
      </div>
    </div>
  );
}
