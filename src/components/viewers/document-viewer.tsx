/**
 * The registry's `document` viewer: a PDF, a Word document, a presentation or a
 * spreadsheet, shown (Story 45.8, FR-181, FR-182, UX-DR71).
 *
 * # One entry, two surfaces
 *
 * This is bound to the `document` viewer id, so the Files pane and a note embed
 * mount the same component over the same bytes through the same registry row
 * (AD-87). "It opens in Files but not in a note" is the bug this epic is about,
 * and the way to make it impossible is for neither surface to hold an opinion.
 *
 * # Where the pixels come from, per format
 *
 * **PDF is the webview's own renderer.** An `<embed>` pointed at Story 45.7's
 * `keeper-file://` URL, which is Range-served from the sync profile's root. The
 * page images are drawn by the platform, which is both better than anything
 * keeper would ship and the reason a 400-page document mounts ONE element: the
 * bound is structural, not a cap that has to be chosen and tuned. What keeper
 * adds around it is the header — version, page count, encryption — from
 * `keeper_core::document`'s probe.
 *
 * **The other three are parsed in Rust and arrive as a bounded view model.**
 * DOCX, PPTX and XLSX are a ZIP of XML; parsing them here would have meant
 * roughly 1.2 MB of JavaScript in every user's bundle, including the users who
 * never open a spreadsheet. What crosses IPC is already capped, so this file
 * renders a projection rather than a document.
 *
 * # Bounded twice, and the second bound is this file's
 *
 * Rust caps what is SENT. {@link useWindowedRows} caps what is MOUNTED. Both
 * are needed and neither substitutes for the other: 3 000 paragraphs is a small
 * message and a large DOM. 44.10's window is the one this app has, shared with
 * the notes list, the recordings list and the Files tree (AD-84), and it fits
 * here without modification because a document body is exactly what it is for —
 * a long, flat, keyed list.
 *
 * It does NOT fit the PDF, and that is worth saying rather than leaving as an
 * omission: there is no list of pages to window, because the pages are inside
 * one element that the platform pages itself.
 *
 * # Read-only, deliberately
 *
 * Nothing here writes and there is no save. `entry.writable` is `false` for all
 * four rows because a lossy round trip through a document container is how
 * people lose work (the epic's "what is NOT in this epic"). The absence of a
 * write path is the feature.
 *
 * # Nothing renders as a blank pane
 *
 * Every failure — not a document, over a cap, corrupt, a decompression bomb, no
 * profile to read from — falls to 45.2's {@link UnknownViewer} carrying the
 * reason Rust worded. That placeholder already names the extension, states the
 * size and offers Reveal and Open With (AD-91), so a format keeper cannot draw
 * degrades to a useful screen rather than an empty one. An empty box tells the
 * reader their file is gone when it is intact, which is the most alarming lie a
 * viewer can tell (`mermaid-widget.ts`'s rule, applied to a second surface).
 */
import { useCallback, useMemo, useState } from "react";
import { useWindowedRows } from "@/components/ui/window-list";
import type { DocumentVm, SheetVm, SlideVm, WordBlockVm, WordsVm } from "@/lib/ipc/client";
import { fileAssetUrl, UnknownViewer, type ViewerProps } from "@/lib/viewers";
import { useDocumentFile } from "./use-document-file";

/** Test id for the whole document surface, whichever format it drew. */
export const DOCUMENT_VIEWER_TESTID = "document-viewer";

/** Test id for the one-line header above every body. */
export const DOCUMENT_HEADER_TESTID = "document-header";

/** Test id for the sentence Rust worded — truncation, encryption, a warning
 *  that did not stop the render. Absent when there is nothing to say. */
export const DOCUMENT_DETAIL_TESTID = "document-detail";

/** Test id for the PDF's `<embed>`. Counting these is the AD-84-shaped
 *  assertion for a PDF: one, however many pages the document has. */
export const DOCUMENT_PDF_TESTID = "document-pdf";

/** Test id for the value cell naming the page count. Absent when the probe
 *  could not determine one — an omitted count, never a guessed one. */
export const DOCUMENT_PAGES_TESTID = "document-pages";

/** Test id for one sheet tab. */
export const DOCUMENT_SHEET_TAB_TESTID = "document-sheet-tab";

/** Test id for the active sheet's grid. */
export const DOCUMENT_SHEET_TESTID = "document-sheet";

/** Test id for one slide in the outline. */
export const DOCUMENT_SLIDE_TESTID = "document-slide";

/** Test id for the whole Word body. */
export const DOCUMENT_WORDS_TESTID = "document-words";

/**
 * What the header says a presentation body is.
 *
 * The word is load-bearing. keeper extracts a deck's text in reading order; it
 * does not lay out DrawingML, so what is on screen is not what is on the slide.
 * Calling it an outline is the difference between a reader who knows they are
 * looking at a summary and a reader who thinks their deck lost its pictures.
 */
export const DOCUMENT_SLIDES_LABEL = "Slide outline";

/** The heights the window paces by until a row has been measured once. */
const WORD_BLOCK_ESTIMATE = 28;
const SLIDE_ESTIMATE = 96;
const SHEET_ROW_HEIGHT = 30;

/** Tailwind classes per paragraph style. Three heading levels and no more —
 *  see `keeper_core::document::style_for` for why six would be a wall of
 *  near-identical text. */
const BLOCK_CLASS: Record<WordBlockVm["style"], string> = {
  title: "font-heading text-display",
  heading1: "font-heading text-title",
  // Below `title`, the levels separate by weight rather than by inventing more
  // sizes: the scale is six steps, and a document's own hierarchy does not get
  // to grow a seventh.
  heading2: "font-heading text-sm font-semibold",
  heading3: "font-heading text-xs font-semibold",
  listItem: "text-sm before:mr-2 before:content-['\\2022']",
  quote: "border-muted border-l-2 pl-3 text-muted-foreground text-sm italic",
  paragraph: "text-sm",
};

/**
 * The sentence for a document that could not be shown.
 *
 * Rust's own `detail` when there is one — those are written to be read by a
 * person and name the specific refusal — and a plain fallback when there is
 * not. The fallback should be unreachable: `open_document` sets a detail on
 * every path that produces no body. It is here because "unreachable" and
 * "renders an empty string" are one refactor apart.
 */
function refusalOf(vm: DocumentVm): string {
  return vm.detail ?? "keeper could not read this file as a document";
}

/** One fact in the header: a label and the value beside it. */
function Fact({ label, slot, value }: { label: string; slot?: string; value: string }) {
  return (
    <div className="min-w-0">
      <dt className="text-muted-foreground text-xs">{label}</dt>
      <dd className="truncate text-sm" data-testid={slot}>
        {value}
      </dd>
    </div>
  );
}

/**
 * The header every body shares: the name, the facts, and the sentence.
 *
 * Shared rather than repeated per format so the four bodies cannot drift into
 * saying the size four slightly different ways.
 */
function Header({
  name,
  relativePath,
  kind,
  sizeLabel,
  detail,
  extra,
}: {
  name: string;
  relativePath: string;
  kind: string;
  sizeLabel: string;
  detail: string | null;
  extra?: React.ReactNode;
}) {
  return (
    <header data-testid={DOCUMENT_HEADER_TESTID} className="flex flex-col gap-3 border-b p-4">
      <div className="min-w-0">
        <h2 className="truncate font-heading text-title">{name}</h2>
        {/* The relative path, which is the only path that may be rendered
            (FR-145). Empty for a file at the root of what is being browsed. */}
        {relativePath !== "" && (
          <p className="truncate text-muted-foreground text-sm">{relativePath}</p>
        )}
      </div>
      <dl className="flex flex-wrap gap-6">
        <Fact label="Kind" value={kind} />
        <Fact label="Size" value={sizeLabel} />
        {extra}
      </dl>
      {detail !== null && (
        <p data-testid={DOCUMENT_DETAIL_TESTID} className="text-muted-foreground text-sm">
          {detail}
        </p>
      )}
    </header>
  );
}

/**
 * A PDF: the platform's renderer, with keeper's facts above it.
 *
 * `<embed>` rather than `<iframe>`: an iframe of a `keeper-file://` URL is a
 * navigable document with its own origin rules, and the plugin element is what
 * WKWebView routes to PDFKit. `type` is stated rather than sniffed so the
 * webview does not have to guess from a scheme it does not know.
 *
 * The page count comes from the probe and is simply omitted when the probe
 * returned `null`. An omitted number is a reader noticing nothing; a guessed
 * one is a reader trusting a wrong number.
 */
function PdfBody({ src, pageCount }: { src: string; pageCount: number | null }) {
  return (
    <embed
      data-testid={DOCUMENT_PDF_TESTID}
      data-page-count={pageCount ?? undefined}
      src={src}
      type="application/pdf"
      className="h-full min-h-0 w-full"
    />
  );
}

/** A Word document: its paragraphs, windowed. */
function WordsBody({ words }: { words: WordsVm }) {
  const getKey = useCallback((index: number) => index, []);
  const list = useWindowedRows({
    count: words.blocks.length,
    getKey,
    rowHeight: WORD_BLOCK_ESTIMATE,
  });

  return (
    <div
      data-testid={DOCUMENT_WORDS_TESTID}
      className="h-full min-h-0 overflow-auto p-4"
      {...list.viewportProps}
    >
      <div className="relative" style={{ height: `${list.totalSize}px` }}>
        {list.rows.map((windowed) => {
          const block = words.blocks[windowed.index];
          return (
            <p
              key={windowed.key}
              {...list.rowProps(windowed)}
              className={BLOCK_CLASS[block.style]}
              data-block-style={block.style}
            >
              {block.runs.map((run, at) => (
                <span
                  // Runs have no identity of their own — they are a position in
                  // a paragraph — and a paragraph is never reordered, so the
                  // index IS the identity here rather than standing in for a
                  // missing one.
                  // biome-ignore lint/suspicious/noArrayIndexKey: reason above — the position within one windowed row IS the identity
                  key={`${windowed.key}-${at}`}
                  className={`${run.bold ? "font-semibold" : ""} ${run.italic ? "italic" : ""}`}
                >
                  {run.text}
                </span>
              ))}
            </p>
          );
        })}
      </div>
    </div>
  );
}

/** A presentation: its slides' text, windowed. */
function SlidesBody({ slides }: { slides: readonly SlideVm[] }) {
  const getKey = useCallback((index: number) => index, []);
  const list = useWindowedRows({ count: slides.length, getKey, rowHeight: SLIDE_ESTIMATE });

  return (
    <div className="h-full min-h-0 overflow-auto p-4" {...list.viewportProps}>
      <ol className="relative m-0 list-none p-0" style={{ height: `${list.totalSize}px` }}>
        {list.rows.map((windowed) => {
          const slide = slides[windowed.index];
          return (
            <li
              key={windowed.key}
              {...list.rowProps(windowed)}
              data-testid={DOCUMENT_SLIDE_TESTID}
              data-slide-number={slide.number}
              className="flex flex-col gap-1 border-b py-3"
            >
              <div className="flex items-baseline gap-2">
                <span className="figures shrink-0 text-muted-foreground text-xs">
                  {slide.number}
                </span>
                <span className="truncate font-heading text-sm font-semibold">
                  {slide.title ?? ""}
                </span>
              </div>
              {/* A slide's lines are positions in a slide, never reordered, so
                  the index scoped to the slide's own key is their identity. */}
              {slide.lines.map((line, at) => (
                // A slide line has no identity but its position within its own slide.
                // biome-ignore lint/suspicious/noArrayIndexKey: reason above
                <p key={`${windowed.key}-${at}`} className="pl-6 text-muted-foreground text-sm">
                  {line}
                </p>
              ))}
            </li>
          );
        })}
      </ol>
    </div>
  );
}

/** One worksheet's cells, windowed by row. */
function SheetGrid({ sheet }: { sheet: SheetVm }) {
  const getKey = useCallback((index: number) => index, []);
  const list = useWindowedRows({
    count: sheet.rows.length,
    getKey,
    rowHeight: SHEET_ROW_HEIGHT,
  });
  // Ragged rows stay ragged in the model; the grid pads them so a column lines
  // up down the sheet. `column_count` is the widest row Rust saw anywhere,
  // already capped.
  const columns = Math.max(sheet.columnCount, 1);

  return (
    <div
      data-testid={DOCUMENT_SHEET_TESTID}
      data-sheet-name={sheet.name}
      data-row-count={sheet.rowCount}
      className="h-full min-h-0 overflow-auto"
      {...list.viewportProps}
    >
      <div className="relative" style={{ height: `${list.totalSize}px` }}>
        {list.rows.map((windowed) => {
          const row = sheet.rows[windowed.index];
          return (
            <div
              key={windowed.key}
              {...list.rowProps(windowed)}
              className="flex border-b"
              data-row-index={windowed.index}
            >
              {/* A cell's identity IS its column: the grid is fixed-width and
                  a column never moves. */}
              {Array.from({ length: columns }, (_, column) => (
                <span
                  // biome-ignore lint/suspicious/noArrayIndexKey: reason above — the position within one windowed row IS the identity
                  key={`${windowed.key}-${column}`}
                  className="w-32 shrink-0 truncate border-r px-2 py-1 text-sm"
                >
                  {row[column] ?? ""}
                </span>
              ))}
            </div>
          );
        })}
      </div>
    </div>
  );
}

/**
 * A workbook: a tab per sheet, and the active one's grid.
 *
 * The active sheet is state here rather than a prop because which sheet you are
 * looking at is a property of the view, not of the file — a note that embeds
 * the same workbook twice should be able to show two different sheets.
 */
function SheetsBody({ sheets }: { sheets: readonly SheetVm[] }) {
  const [active, setActive] = useState(0);
  // Clamped rather than trusted: the workbook can change under a panel that
  // kept its index, and `sheets[7]` of a five-sheet workbook is a crash.
  const sheet = sheets[Math.min(active, sheets.length - 1)];

  return (
    <div className="flex h-full min-h-0 flex-col">
      {sheets.length > 1 && (
        <div className="flex shrink-0 gap-1 overflow-x-auto border-b px-2" role="tablist">
          {sheets.map((candidate, index) => (
            <button
              key={candidate.name}
              type="button"
              role="tab"
              aria-selected={candidate === sheet}
              data-testid={DOCUMENT_SHEET_TAB_TESTID}
              onClick={() => setActive(index)}
              className={`shrink-0 px-3 py-1.5 text-sm ${
                candidate === sheet
                  ? "border-primary border-b-2 font-medium"
                  : "text-muted-foreground"
              }`}
            >
              {candidate.name}
            </button>
          ))}
        </div>
      )}
      <SheetGrid sheet={sheet} />
    </div>
  );
}

/**
 * What {@link DocumentView} renders — the loaded document plus everything the
 * placeholder needs if it turns out there is nothing to draw.
 */
export interface DocumentViewProps extends ViewerProps {
  /** What Rust made of the file. */
  readonly vm: DocumentVm;
  /**
   * Where the PDF's bytes are, or `null` when this surface cannot serve them.
   *
   * A parameter rather than something this component composes, for the reason
   * `ViewerFile.openWith` is a thunk: WHICH transport is legal depends on where
   * the file came from. A Files-pane file is served by `keeper-file://` from a
   * sync profile's root; a surface holding different coordinates will have a
   * different URL, and a component that built its own would eventually build
   * the wrong one. `null` renders the placeholder with a reason rather than an
   * `<embed>` pointed at nothing, which is the blank pane this file exists to
   * avoid.
   */
  readonly pdfSrc: string | null;
}

/**
 * The presentational half: a `DocumentVm` in, a document out.
 *
 * Separate from {@link DocumentViewer} so a host that loaded the same VM by
 * other coordinates — a note embed holding a vault id rather than a profile id
 * — mounts this directly instead of forking the rendering. That is the seam
 * 45.4 and 45.6 established with `RawRenderedView` and `useTextFile`, applied
 * to the format that most needs it.
 */
export function DocumentView({ file, entry, vm, pdfSrc }: DocumentViewProps): React.ReactElement {
  // What Rust FOUND, against what the name implied. A `.docx` holding a PDF is
  // a real thing that happens when somebody renames a download, and rendering
  // it correctly while saying nothing would leave a reader wondering why their
  // Word document has pages.
  const mismatch =
    vm.format !== null && vm.format !== entry.format
      ? `this file is named as ${entry.label.toLowerCase()} but its contents are a ${
          { pdf: "PDF", docx: "Word document", pptx: "presentation", xlsx: "spreadsheet" }[
            vm.format
          ]
        }`
      : null;

  const detail = mismatch === null ? vm.detail : [mismatch, vm.detail].filter(Boolean).join("; ");

  if (vm.pdf !== null) {
    // Two ways there is no URL worth mounting, and BOTH must be checked before
    // an `<embed>` exists. `pdfSrc === null` is this surface having no
    // profile-scoped transport at all. `servable` is Rust's answer to whether
    // the protocol will actually serve THIS name — which can be false for a
    // real, readable PDF, because the format was sniffed from the content and
    // the protocol's allow-list reads the name.
    //
    // Mounting anyway would 404 into an empty element, and a failed plugin
    // render is not observable from JavaScript: the reader would get a blank
    // pane and no sentence. That is the one lie this whole story is written
    // against, so it is refused here where it can still be worded.
    if (pdfSrc === null || !vm.pdf.servable) {
      return (
        <UnknownViewer
          file={file}
          entry={entry}
          reason={
            pdfSrc === null
              ? "keeper cannot reach this PDF's bytes from here, so its pages cannot be drawn — use Open With"
              : `this is a PDF, but it is named ${file.name} — keeper only draws pages for a file named .pdf, so rename it or use Open With`
          }
        />
      );
    }
    return (
      <section
        data-testid={DOCUMENT_VIEWER_TESTID}
        data-format={vm.format ?? "unknown"}
        aria-label={file.name}
        className="flex h-full min-h-0 flex-col"
      >
        <Header
          name={file.name}
          relativePath={file.relativePath}
          kind="PDF"
          sizeLabel={vm.sizeLabel}
          detail={detail}
          extra={
            <>
              {vm.pdf.pageCount !== null && (
                <Fact label="Pages" slot={DOCUMENT_PAGES_TESTID} value={String(vm.pdf.pageCount)} />
              )}
              {vm.pdf.version !== null && <Fact label="Version" value={vm.pdf.version} />}
            </>
          }
        />
        <PdfBody src={pdfSrc} pageCount={vm.pdf.pageCount} />
      </section>
    );
  }

  const body =
    vm.words !== null ? (
      <WordsBody words={vm.words} />
    ) : vm.slides !== null ? (
      <SlidesBody slides={vm.slides.slides} />
    ) : vm.sheets !== null ? (
      <SheetsBody sheets={vm.sheets.sheets} />
    ) : null;

  if (body === null) {
    // Nothing to draw. The reason is Rust's — "it is a decompression bomb",
    // "word/document.xml is missing", "larger than the 50.0 MB keeper will
    // open" — and the placeholder around it already offers Open With, which is
    // the useful thing to do with a file keeper declined to read.
    return <UnknownViewer file={file} entry={entry} reason={refusalOf(vm)} />;
  }

  const kind =
    vm.words !== null
      ? "Word document"
      : vm.slides !== null
        ? DOCUMENT_SLIDES_LABEL
        : "Spreadsheet";

  return (
    <section
      data-testid={DOCUMENT_VIEWER_TESTID}
      data-format={vm.format ?? "unknown"}
      aria-label={file.name}
      className="flex h-full min-h-0 flex-col"
    >
      <Header
        name={file.name}
        relativePath={file.relativePath}
        kind={kind}
        sizeLabel={vm.sizeLabel}
        detail={detail}
        extra={
          <>
            {vm.words !== null && <Fact label="Paragraphs" value={String(vm.words.blockCount)} />}
            {vm.slides !== null && <Fact label="Slides" value={String(vm.slides.slideCount)} />}
            {vm.sheets !== null && <Fact label="Sheets" value={String(vm.sheets.sheetCount)} />}
          </>
        }
      />
      {body}
    </section>
  );
}

/**
 * The registry's `document` viewer.
 *
 * Loads its own bytes from the coordinates the surface supplied, then hands the
 * result to {@link DocumentView}. This is the only stateful layer and it owns
 * none of the rendering.
 */
export function DocumentViewer({ file, entry }: ViewerProps): React.ReactElement {
  const { vm, error, loading } = useDocumentFile({
    profileId: file.profileId,
    subpath: file.relativePath,
  });

  // Composed here rather than inside the view so the view stays a pure function
  // of a VM. `profileId` is the surface's, not derived from anything (AD-65).
  const pdfSrc = useMemo(
    () => (file.profileId === null ? null : fileAssetUrl(file.profileId, file.relativePath)),
    [file.profileId, file.relativePath],
  );

  if (loading) {
    return (
      <section
        data-testid={DOCUMENT_VIEWER_TESTID}
        data-format="loading"
        aria-label={file.name}
        aria-busy="true"
        className="p-6 text-muted-foreground text-sm"
      >
        Opening {file.name}…
      </section>
    );
  }

  // No VM at all: the command failed, or this file is not inside a profile and
  // there is nothing to read. The hook has already worded both.
  if (vm === null) {
    return <UnknownViewer file={file} entry={entry} reason={error ?? undefined} />;
  }

  return <DocumentView file={file} entry={entry} vm={vm} pdfSrc={pdfSrc} />;
}
