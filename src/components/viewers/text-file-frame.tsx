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
 * was not truncated on the way in — OR when the host handed its own controls
 * down, which is Story 53.3's merge and the next section.
 *
 * # One title bar, when the host gives up its own (Story 53.3, FR-317)
 *
 * The owner's report is that the name is on screen twice: this bar says it, and
 * the panel's header above it said it again with the Export control and the
 * fold and the close. The notes surface already fixed this shape — a note panel
 * draws no row and hands its two controls into the editor's header
 * (`panel-strip.tsx`, Story 50.1) — and {@link TextFileFrameProps.frame} is the
 * same seam for a file.
 *
 * **What makes it harder here, and the trap it is guarded against.** A note
 * panel can decide up front, because `noteVaultReason` is a pure store read.
 * `savable` is decided in this function from the registry's row, Rust's refusal
 * and `vm.oversize` — and the last of those only exists after the read lands.
 * This frame also renders a bare sentence and NO header while it is loading, for
 * a file it cannot read and for bytes that are not text. So the rule is a
 * promise this component keeps rather than a condition the host evaluates: **a
 * frame handed `frame` draws a header in every one of those states**, with
 * whatever of the Save half is true at the time. A host that kept its row on a
 * guess about `savable` would leave five states with no title at all, which is
 * exactly the defect a naive port of 50.1 produces.
 *
 * # The two bands above the file fold (Story 53.3, FR-316, FR-318)
 *
 * The properties form (Story 50.4) and AD-102's caveat both sat above the
 * Preview|Source|Note tabs unconditionally, and both pushed the file down in all
 * three views. Each now folds from a control on this bar, defaulting folded, and
 * the answer is remembered per SURFACE rather than per file
 * (`@/lib/stores/file-frame-fold`) — this component outlives the file it shows
 * and a folded panel unmounts it entirely, so `useState` here would lose the
 * reader's answer twice over.
 *
 * The caveat's fold is a narrowing of AD-102 and never a deletion: folded, it
 * shows Rust's ONE-LINE composition of the same fact
 * ({@link TextFileFrameProps.writeCaveatShort}), which still names what a file
 * outside the vault does not get. Never a truncation of the long one here — the
 * short form is Rust's sentence too, for the reason `viewers/types.ts` gives.
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
 * this buffer. It is its own command over the file's bytes — the block, and
 * nothing else, because Rust guards on the block it was handed and preserves
 * every other byte (`file_properties::replace_block`). So the buffer must learn
 * the new block, and this frame is what learns it: `FileProperties` reports the
 * block it is holding, and that report is both what the panes hide and what gets
 * spliced in front of text the reader has not saved yet.
 *
 * This used to be an unconditional `reload()` after every properties write, and
 * `reload` is `read`, which replaces the buffer with no dirty check. That was
 * survivable while a savable markdown file landed in read-only Preview and
 * reaching an editable pane took a deliberate tab press. Story 52.3 put a caret
 * in Note mode by default, which makes "type a paragraph, then set `tags:` in the
 * form above it" the ordinary way to use this pane — and it destroyed the
 * paragraph, silently, with no prompt.
 */
import { ChevronDown, ChevronRight, SlidersHorizontal } from "lucide-react";
import { type ReactNode, useEffect, useId, useRef, useState } from "react";
import { FOLD_STRIP } from "@/components/layout/fold-strip";
import { PaneHeader } from "@/components/layout/pane-header";
import type { CsvTableOptions } from "@/components/notes/editor/csv-table";
import { FileProperties, PROPERTIES_LABEL } from "@/components/notes/properties-panel";
import { Button } from "@/components/ui/button";
import {
  fileFrameFoldStore,
  hydrateFileFrameFold,
  useFileFrameFold,
} from "@/lib/stores/file-frame-fold";
import type { ViewerEntry } from "@/lib/viewers";
import type { MarkdownPreviewOptions } from "./markdown-preview";
import type { CsvCoordinates } from "./raw-rendered-view";
import { leadingFormBlock, RawRenderedView } from "./raw-rendered-view";
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
 * The control that shows the whole of AD-102's caveat, and folds it back to one
 * line (Story 53.3, FR-318).
 *
 * ONE label across both states, with `aria-expanded` saying which state it is
 * in — the disclosure form the note editor's Properties control uses
 * (`priority-actions.tsx`: "pressed says this control is on, expanded says the
 * thing this control names is open"). The panel fold's two-label form is for a
 * control that is the only thing left on screen; this one sits beside the
 * sentence it belongs to.
 *
 * It names the FACT rather than the act, because the act is a chevron anybody
 * recognises and the fact is what a reader is deciding whether to read.
 */
export const TEXT_FILE_CAVEAT_LABEL = "What keeper does not do for this file";

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
   * host that omits this gets the panel's `onWritten` instead, and this frame
   * re-reads the address it already holds — unless the buffer has unsaved text
   * in it, which a re-read would replace with the disk's (Story 52.3's fix).
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
   *
   * Since Story 53.3 this is the form shown on REQUEST, and
   * {@link TextFileFrameProps.writeCaveatShort} is what stands there by default.
   * Both are on screen before the first keystroke in the only sense AD-102 asks
   * for: the fact is, in whichever form the reader has chosen.
   */
  writeCaveat?: string | null;
  /**
   * The same fact in one line, or `null` (Story 53.3, FR-318).
   *
   * Composed in Rust too (`FilesWriteVm.caveatShort`) and rendered verbatim —
   * this frame never derives it by clipping {@link TextFileFrameProps.writeCaveat},
   * which would be a paraphrase of the clause that names what is missing.
   *
   * A host that carries a caveat carries both forms, because Rust sets them
   * together. When only the long one arrives — a host written before this story,
   * or a fixture — the fold has nothing to show, so the band stays whole rather
   * than empty: the fact on screen is the invariant, and the fold is the
   * preference.
   */
  writeCaveatShort?: string | null;
  /**
   * The controls of the frame holding this surface — a panel's Export, its fold
   * and its close — or `null` when that frame draws its own row (Story 53.3,
   * FR-317).
   *
   * **Handing these over is what makes this bar the panel's title bar**, and it
   * comes with an obligation this component keeps in every branch: a frame with a
   * `frame` draws a header while the file is opening, for a file it cannot read,
   * for bytes that are not text, and for a file no save can follow — states in
   * which it otherwise renders one bare sentence and nothing else. The host has
   * given up its own row; a state with no row would be a panel a reader cannot
   * name, fold or close.
   *
   * `null` for the note embed (`file-embed-host.tsx`), which is inside a
   * document rather than inside a frame and has no controls to lend.
   */
  frame?: ReactNode;
  /**
   * The host's own bands — what it offers about this file and what it has to
   * report about the last thing it tried — or omitted when it has none
   * (Story 53.3's fix).
   *
   * **They belong here because this frame draws the title row.** They used to be
   * siblings ABOVE the mounted frame in `text-file-viewer.tsx`, which was right
   * while the panel drew its own header on top of them. Since this story the
   * header below IS the panel's title row, so a band left above it made the
   * panel's first row a lone right-aligned button — pushing the name, the fold
   * and the close ~27px down for the ordinary Files→vault markdown file, and
   * for the whole life of a notice a wikilink left behind. Across a strip the
   * fold and close controls then stopped lining up with the panels beside them.
   *
   * Rendered directly under the header and above this frame's own bands, which
   * is the order the host had: its band first, then the caveat, then the error.
   * The only thing that moved is the row, and it moved to the top where a title
   * row belongs.
   *
   * A `ReactNode` rather than coordinates, unlike {@link
   * TextFileFrameProps.properties}: what the band SAYS is the host's own
   * knowledge — which vault holds the file, which link went nowhere — and this
   * frame has no opinion to add. It owns only where the nodes sit.
   */
  notices?: ReactNode;
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
  writeCaveatShort = null,
  writeRefusal = null,
  frame = null,
  notices = null,
  csv,
  properties,
  onPropertiesRenamed,
  preview,
  csvOptions,
}: TextFileFrameProps): React.ReactElement {
  const { vm, content, setContent, dirty, save, reload, error, loading, loadedFrom } = state;

  // Story 53.3's two folds, and the reader's standing answer to both.
  //
  // The restore is mounted HERE rather than at the shell: these two keys belong
  // to no other surface, every surface that draws them mounts this component,
  // and this is the one place the call can be forgotten (DW-172) — which is why
  // the frame's own suite asserts it over a real cookie and the store's cannot.
  const folds = useFileFrameFold((each) => each.bands);
  useEffect(() => {
    hydrateFileFrameFold(typeof document === "undefined" ? "" : document.cookie);
  }, []);

  // Both regions are named by the control that opens them, so the ids have to be
  // unique per mounted frame: a panel strip can hold four of these at once, and
  // two `aria-controls` pointing at one id is a promise to a screen reader that
  // resolves to the wrong pane.
  const frameId = useId();
  const propertiesRegionId = `${frameId}-properties`;
  const caveatRegionId = `${frameId}-caveat`;

  // The `---` block the properties form below is holding: `null` while its read
  // is in flight, `null` again if that read refused, and `null` when this file
  // gets no form at all. What the panes hide is derived from THIS rather than
  // from "a form was mounted" — see `RawRenderedView.frontmatterInForm`.
  const [formBlock, setFormBlock] = useState<string | null>(null);

  // Everything the repair below reads and must not re-run for. A keystroke
  // changes `content` and `dirty` and says nothing about the block on disk, so
  // the effect is keyed on the block alone and reaches the buffer through a ref —
  // the same indirection, for the same reason, as `MarkdownPane`'s.
  const latest = useRef(state);
  latest.current = state;

  // The block this buffer's first bytes are known to be, which is what makes the
  // repair a splice rather than a guess about where a block ends.
  //
  // Deliberately NOT cleared when the form reports `null`: `FileProperties` sets
  // that at the start of every read, including the re-read a retitle does, and
  // forgetting the old block there would leave the new one unspliced.
  const carried = useRef<string | null>(null);

  // Whether the block below changed because THIS surface wrote it, set by
  // `onWritten` and consumed by the repair.
  //
  // The ordering it stands on is guaranteed rather than lucky: `FileProperties`
  // calls `onWritten` inside the write's own promise callback and reports the
  // block from an effect, so the flag is always set before the repair reads it.
  // What it buys is that the file is read at most once per property write — the
  // reader is on a pendrive as often as not — while a block that changed because
  // the FILE changed, with nothing else on its way to re-read it, still gets one.
  const wroteHere = useRef(false);

  // Which file the carried block belongs to, because a panel replaces its target
  // IN PLACE: this component outlives the file it is showing, and the next file's
  // first block is a FIRST block rather than a change to this one's. Without the
  // reset, opening a second markdown file would look like the block changing under
  // a clean buffer and cost an extra read of a file the host had just read.
  //
  // Field by field and adopted during render, not in an effect: both hosts build
  // the pair inline, so its identity changes every render, and an effect would
  // compare the new file's block against the old file's for one commit.
  const shownFile = useRef<FilePropertiesCoordinates | null>(null);
  if (
    properties?.profileId !== shownFile.current?.profileId ||
    properties?.relativePath !== shownFile.current?.relativePath
  ) {
    shownFile.current = properties;
    carried.current = null;
  }

  // A properties write changed the file's block and nothing else. Repair the
  // buffer, and never by replacing it.
  useEffect(() => {
    if (formBlock === null) {
      return;
    }
    const was = carried.current;
    if (was === formBlock) {
      return;
    }
    // Consumed whatever branch is taken below: it describes THIS change of block.
    const afterWrite = wroteHere.current;
    wroteHere.current = false;
    if (was === null) {
      // The first block this file's form has held. The buffer was read from the
      // same bytes, so there is nothing to repair — and a re-read here would be
      // an extra read of every markdown file a panel opens.
      carried.current = formBlock;
      return;
    }
    const buffer = latest.current;
    if (buffer.loading) {
      // A read is already in flight and its answer is the whole file, this block
      // included. Splicing into a buffer that is about to be replaced writes bytes
      // nobody will ever see.
      carried.current = formBlock;
      return;
    }
    if (!buffer.dirty) {
      carried.current = formBlock;
      if (!afterWrite) {
        // The block changed under this surface rather than because of it: the
        // panel's own re-read after a refused write, or a retitle that answered
        // with the subpath the panel already had. Nothing else is going to notice
        // the file moved on, and with nothing typed the truthful repair is the
        // file itself — a read also advances what the loader believes is on disk,
        // which a splice cannot, and that is what keeps the Save button honest.
        void buffer.reload();
      }
      return;
    }
    const prefix = leadingFormBlock(buffer.content, was);
    if (prefix === null) {
      // The buffer no longer begins with the block the form was holding, so there
      // is nothing here to splice over without guessing where its block ends —
      // and guessing is the defect this whole seam was rewritten to remove.
      // Leaving the typed text alone is the one answer that cannot lose it.
      return;
    }
    carried.current = formBlock;
    // The block that landed, in front of the body the reader is still typing.
    // Rust wrote this file's block and preserved every other byte, so the body in
    // the buffer is the body on disk plus the reader's own unsaved edits — which
    // is exactly what the re-read this replaced used to throw away.
    buffer.setContent(
      prefix.slice(0, prefix.length - was.length) + formBlock + buffer.content.slice(prefix.length),
    );
  }, [formBlock]);

  // The file's own name, which nothing else inside this frame renders — the note
  // embed that mounts it has no header at all, and since Story 53.3 a panel that
  // handed its controls down has no row of its own either, so this is the one
  // identity every host can rely on.
  //
  // TWO treatments, because this row is two different things depending on who
  // mounted it. With a `frame` it IS the panel's title row, and every other
  // panel title in the strip is drawn in `FOLD_STRIP.titleClass` —
  // `DESIGN.md`'s `pane-header` typography, which `panel-strip.tsx` gives up its
  // own row to keep (`panel-strip.tsx:684-688`) and which the notes surface's
  // merged row already wears (`note-editor.tsx`'s `deriveTitle` heading). Kept
  // small, a strip holding `notes.md`, `report.pdf` and a note showed three
  // typographies for one thing, and folding the `.md` changed the size of its
  // own name.
  //
  // Without one — the note embed (`file-embed-host.tsx`) — this is not a panel
  // title at all but a label inside somebody's document, where a 15px heading
  // would outshout the prose around it. The heading semantics stay off both:
  // `panel-strip.tsx` says why a second `h2` naming the same file is wrong.
  const identity = (
    <span
      className={
        frame === null ? "min-w-0 flex-1 truncate font-medium text-xs" : FOLD_STRIP.titleClass
      }
    >
      {fileName}
    </span>
  );

  // A state with no file in it: one sentence, and — when a host gave up its row
  // for this frame — the row it is owed above it (Story 53.3, FR-317).
  //
  // Three call sites and they must stay in lockstep: these are exactly the states
  // that used to render a bare `<p>` and no header, and the panel above is now
  // relying on this component for its title, its fold and its close. One of them
  // drawing nothing is a panel a reader cannot close.
  //
  // No status and no actions: there is no buffer to be dirty and no Save to
  // offer. `PaneHeader` reserves nothing for a status it is not given, so this is
  // the same 40px row with the name in it and the frame's controls at the end.
  //
  // The host's own bands come with it. They are about the FILE, not about the
  // buffer — a vault markdown file has its note whether the bytes arrived or
  // not — so dropping them here would make Open in Notes flicker out for the
  // whole of a pendrive's read, which is what it did while these nodes were the
  // host's own siblings.
  const framed = (sentence: ReactNode): React.ReactElement =>
    frame === null ? (
      <>
        {notices}
        {sentence}
      </>
    ) : (
      <div className="flex h-full min-h-0 flex-col">
        <PaneHeader className="px-3" identity={identity} actions={null} frame={frame} />
        {notices}
        {sentence}
      </div>
    );

  if (loading) {
    return framed(
      <p className="px-3 py-2 text-muted-foreground text-xs" role="status">
        opening {fileName}
      </p>,
    );
  }

  // No VM at all: unreadable, or outside every profile. The loader has already
  // worded it — including the case where there is nothing to read because the
  // file is not inside a synced folder, and the case where the vault does not
  // have the file an embed names, where Rust's sentence lists the paths it
  // looked for.
  if (vm === null) {
    return framed(
      <p className="px-3 py-2 text-destructive text-xs" role="alert">
        {error ?? `keeper could not open ${fileName}`}
      </p>,
    );
  }

  // Bytes that are not text. Rendering `text ?? ""` here would put an empty
  // editable pane over a binary file and offer to save it, which is how an
  // editor overwrites a `.png` with nothing.
  if (vm.binary) {
    return framed(
      <p className="px-3 py-2 text-destructive text-xs" role="alert">
        {vm.detail ?? `${fileName} is not text, so keeper will not open it in an editor`}
      </p>,
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

  // Story 53.3: the form is behind a fold now, and the fold defaults CLOSED —
  // the same default the notes surface has had since Story 49
  // (`note-editor.tsx`'s `showProperties`). The CONTROL exists wherever the form
  // would: a disclosure for a panel this file is never going to get would be a
  // control that announces its own refusal.
  const propertiesOpen = propertiesPanel !== null && !folds.properties;

  // Story 53.3's second fold. `writeCaveatShort` is Rust's one-line form, and
  // the fold is only offered when there is one: a host that carries the long
  // sentence alone keeps showing it whole rather than folding the fact off the
  // screen.
  const caveatFoldable = writeCaveat !== null && writeCaveatShort !== null;
  const caveatOpen = !caveatFoldable || !folds.caveat;

  return (
    <div className="flex h-full min-h-0 flex-col">
      {savable || frame !== null ? (
        <PaneHeader
          // Horizontal padding only. The bottom edge and the 40px height are
          // `PaneHeader`'s now; this used to spell `border-b` (a second edge
          // under the component's) and `py-1.5` (a 44px row where the other
          // two callers were 40).
          className="px-3"
          identity={identity}
          // Reserved only where there is a buffer to be dirty. A row that exists
          // because the HOST handed its controls over, for a file no save can
          // follow, has nothing to say here — and an empty reserved slot is 8px
          // of nothing (`pane-header.tsx`).
          status={savable ? { sizers: FILE_SAVE_SIZERS, caption: fileSaveWord(dirty) } : null}
          actions={
            <>
              {/* The properties fold, in the note editor's own disclosure shape:
                  the same word, the same glyph, `aria-expanded` for the state and
                  `aria-controls` naming the region while it is open (and omitted
                  while it is closed, rather than pointing at an id nothing owns).
                  Not `FoldSection`, which is the 48px rail-row shape. */}
              {propertiesPanel === null ? null : (
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-sm"
                  aria-label={PROPERTIES_LABEL}
                  title={PROPERTIES_LABEL}
                  aria-expanded={propertiesOpen}
                  aria-controls={propertiesOpen ? propertiesRegionId : undefined}
                  className="shrink-0"
                  onClick={() => fileFrameFoldStore.getState().toggleBand("properties")}
                >
                  <SlidersHorizontal aria-hidden="true" />
                </Button>
              )}
              {savable ? (
                <Button
                  type="button"
                  variant="outline"
                  size="xs"
                  disabled={!dirty}
                  title={dirty ? undefined : FILE_SAVE_CLEAN_TITLE}
                  // The same call `Mod-s` makes, and the only one: the hook holds
                  // the buffer, so there is nothing for this to pass and nothing
                  // it could pass that would differ from what the editor last
                  // reported.
                  onClick={() => void save()}
                >
                  {FILE_SAVE_LABEL}
                </Button>
              ) : null}
            </>
          }
          // The host's own controls, last and never demoted into this surface's
          // overflow — `PaneHeader`'s fourth group, where Story 50.1 puts a note
          // panel's fold and close.
          frame={frame}
        />
      ) : null}
      {/* The host's bands, directly under the row and above this frame's own —
          the order the host used to draw them in, minus the panel header that
          used to sit between them and the file (see `notices`). Whatever they
          are, the title row is the panel's FIRST row. */}
      {notices}
      {writeCaveat === null ? null : (
        <div
          data-testid={TEXT_FILE_CAVEAT_TESTID}
          className="flex shrink-0 items-start gap-2 border-b px-3 py-1.5"
        >
          {/* Rust's sentence, in whichever form the reader has asked for, and
              never clipped here: the short one is composed in Rust too
              (`WriteScope::unmanaged_caveat_short`), because a truncation is a
              paraphrase of the clause that names what is missing.

              One element for both forms, and therefore one `aria-controls`
              target that always owns something: the region is the caveat itself,
              expanded or not, so the promise `aria-expanded` makes is one this
              surface can keep in both states. */}
          <p
            id={caveatRegionId}
            className="min-w-0 flex-1 text-muted-foreground text-xs"
            role="status"
          >
            {caveatOpen ? writeCaveat : writeCaveatShort}
          </p>
          {caveatFoldable ? (
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label={TEXT_FILE_CAVEAT_LABEL}
              title={TEXT_FILE_CAVEAT_LABEL}
              aria-expanded={caveatOpen}
              aria-controls={caveatRegionId}
              className="-my-0.5 shrink-0"
              onClick={() => fileFrameFoldStore.getState().toggleBand("caveat")}
            >
              {caveatOpen ? (
                <ChevronDown aria-hidden="true" />
              ) : (
                <ChevronRight aria-hidden="true" />
              )}
            </Button>
          ) : null}
        </div>
      )}
      {error === null ? null : (
        <p className="shrink-0 border-b px-3 py-1.5 text-destructive text-xs" role="alert">
          {error}
        </p>
      )}
      {/* Above the editor and below the banners, which is where the note editor
          keeps it: the properties of a file are a standing fact about it, and
          the banners are about the last action.

          `onBlock` is how the buffer below learns what a property write did, and
          it carries the block ITSELF: the panes hide those exact bytes, and the
          repair above splices them in front of text the reader has not saved.

          `onWritten` used to re-read unconditionally, and `reload` is `read`,
          which replaces the buffer with no dirty check — so the sequence 52.3
          made ordinary, "type a paragraph and then set `tags:` in the form above
          it", destroyed the paragraph silently and with no prompt. It is now
          dirty-guarded: with nothing typed the file itself is the repair, and
          with something typed the repair is the splice above, which needs the
          block that LANDED and so has to wait one commit for `onBlock`. The flag
          is how the two stay one read: the repair skips its own re-read for a
          block that changed because this surface wrote it.

          It is called for a rename this frame had no host to re-address, too
          (`FileProperties` calls it instead of `onRenamed` then), where the
          address here is the one Rust just emptied and no block changes — so the
          re-read is what renders Rust's "is no longer in tgdrive", exactly as
          before.

          UNMOUNTED rather than hidden while the fold is closed (Story 53.3), for
          the reason `panel-strip.tsx` gives about a folded panel: a form kept
          alive behind `hidden` keeps its read and its subscription over a file
          nobody can see. The id rides on the box this form already had rather than
          on a wrapper of its own — that box is what keeps the form out of the
          editor's `flex-1`, and it is exactly the region the control names. */}
      {propertiesOpen && propertiesPanel !== null ? (
        <div id={propertiesRegionId} className="shrink-0">
          <FileProperties
            profileId={propertiesPanel.profileId}
            relativePath={propertiesPanel.relativePath}
            onBlock={setFormBlock}
            onWritten={() => {
              wroteHere.current = true;
              if (!dirty) {
                void reload();
              }
            }}
            onRenamed={onPropertiesRenamed}
          />
        </div>
      ) : null}
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
          // The block the form above is HOLDING, and never a second opinion about
          // which bytes that is: the panes hide exactly what the form draws, so
          // the two cannot come to disagree about whether it is on screen twice
          // (Story 52.3, FR-304). `null` until the form's read lands, if it
          // refuses, and for a file that gets no form — nothing is hidden then.
          // The Source tab still shows every byte, and a save still writes them.
          //
          // And `null` while the fold is closed (Story 53.3): with no form on
          // screen there is nothing drawing the block, so the document has to
          // draw it — hiding bytes that are in neither place would put a file's
          // `tags:` nowhere at all. The carried block is deliberately NOT
          // forgotten on the way, so unfolding costs no re-read.
          frontmatterInForm={propertiesOpen ? formBlock : null}
          onExternalWrite={() => void reload()}
          editor={TextEditorSurface}
          csvOptions={csvOptions}
        />
      </div>
    </div>
  );
}
