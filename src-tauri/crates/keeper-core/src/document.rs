//! Reading a PDF, a DOCX, a PPTX or an XLSX well enough to show it (Story 45.8,
//! FR-181, FR-182, UX-DR71).
//!
//! # Why this is Rust and not a JavaScript bundle
//!
//! DOCX, PPTX and XLSX are a ZIP of XML. Rendering them in the webview would
//! have meant roughly 1.2 MB of SheetJS plus mammoth shipped to every user,
//! including the ones who never open a spreadsheet. Both crates this module
//! needs — `zip` and `quick-xml` — are already compiled in every keeper build
//! for unrelated reasons, so parsing here costs no new package and no bundle
//! bytes. The webview receives a bounded view model, never a document.
//!
//! It is also the only place the work is testable. The Tauri shell does not
//! build on a Linux developer machine (AD-55, AD-56, and the same reasoning
//! [`crate::text_file`] spells out), so a parser living in `sync_ipc.rs` would
//! be a parser nobody could run until macOS. Everything below is proved over a
//! temp directory on any machine.
//!
//! # The bytes are read-only, and that is a decision, not an omission
//!
//! Nothing here writes. The epic's "what is NOT in this epic" says why: a lossy
//! round trip through a document container is how people lose work. keeper can
//! read a `.docx` faithfully enough to show it and nowhere near faithfully
//! enough to rewrite it, and a viewer that can only destroy information should
//! not offer a save button. `ViewerEntry.writable` is `false` for all four rows
//! and this module gives the frontend nothing it could write back with.
//!
//! # The format comes from the CONTENT, never the extension
//!
//! [`sniff`] reads the magic bytes and, for a ZIP, which main part the
//! container holds. A `.docx` that is really a PDF renders as a PDF and says
//! so. This is deliberately not a second copy of 43.5's `kind_for_file_name`
//! and does not compete with it: `kind_for_file_name` answers *which viewer
//! should keeper mount*, from a name, before anything is read; this answers
//! *what did keeper actually find in the file*, after opening it. The first is
//! a routing decision that must be cheap and name-based; the second is a fact.
//! When they disagree the frontend says so rather than picking a winner.
//!
//! # Everything is bounded, in four separate places
//!
//! A document is a file a stranger put in a folder that syncs. Four different
//! caps exist because there are four different ways one can be hostile:
//!
//! 1. **The file.** [`OOXML_MAX_BYTES`] bounds what will be opened at all, so
//!    "how many sheets does this have" cannot cost a 2 GB read.
//! 2. **The decompression.** A 40 kB ZIP can inflate to gigabytes, and `zip`
//!    hands out a `Read` that will happily do it. [`OOXML_MAX_PART_BYTES`] and
//!    [`OOXML_MAX_INFLATED_BYTES`] bound one part and the whole document, and
//!    exceeding either is a *named refusal*, not an error.
//! 3. **The entities.** A billion-laughs DTD is a real path into a note-taking
//!    app that opens a colleague's file. `quick-xml` does not expand
//!    unrecognised entities — it errors — and [`tests::entity_expansion_is_refused`]
//!    fails if that ever changes, because this module's safety currently rests
//!    on it.
//! 4. **The projection.** Parsing within budget can still produce a view model
//!    too large to send. [`MAX_SHEETS`], [`MAX_ROWS_PER_SHEET`], [`MAX_CELLS`],
//!    [`MAX_BLOCKS`] and [`MAX_SLIDES`] bound what crosses IPC.
//!
//! # A truncated thing says so, and its count is still the real count
//!
//! Every truncatable collection carries both a `_count` of what the document
//! *has* and a `truncated` flag. This is 44.11's rule: a count that quietly
//! means "loaded so far" makes a 50 000-row spreadsheet look like a 500-row
//! spreadsheet, and the reader has no way to notice. Counting is cheap — the
//! parser streams the whole part and keeps a bounded prefix — so there is no
//! excuse for the dishonest version.

use std::collections::HashMap;
use std::io::{self, Read as _};
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::size::format_file_size;

/// The largest OOXML container keeper will open.
///
/// 50 MB decimal, so it renders through [`format_file_size`] as `50.0 MB` and a
/// person can compare it to the size in the same banner without arithmetic —
/// the alignment [`crate::text_file::TEXT_EDIT_MAX_BYTES`] chose for the same
/// reason.
///
/// This bounds the *container*, which is the number that decides whether keeper
/// opens the file at all. A real `.docx` is a few hundred kilobytes; a real
/// `.pptx` full of photographs can reach tens of megabytes. Above this the
/// viewer shows name, size and Open With — the unknown viewer's shape, and a
/// perfectly good answer for a file the system's own application will open
/// better anyway.
pub const OOXML_MAX_BYTES: u64 = 50_000_000;

/// The largest a single inflated part may be.
///
/// The one cap that actually stops a ZIP bomb, because it is checked against
/// bytes *produced* rather than bytes stored. A part's declared uncompressed
/// size is attacker-controlled and is only used as a cheap early refusal; this
/// is the guard that holds when the header lies.
pub const OOXML_MAX_PART_BYTES: u64 = 16_000_000;

/// The largest total inflation one document may cause across every part read.
///
/// Separate from [`OOXML_MAX_PART_BYTES`] because a bomb does not have to be
/// one big part: a hundred parts just under the per-part cap would pass that
/// check individually and still exhaust memory.
pub const OOXML_MAX_INFLATED_BYTES: u64 = 48_000_000;

/// The largest PDF whose page count keeper will go looking for.
///
/// Only the *probe* is capped, never the rendering: PDF pages are drawn by the
/// webview from a Range-served URL, so an 800 MB scan still opens and still
/// scrolls. What a cap above this buys is a page number in a header, and
/// reading 800 MB to print "412 pages" is not a trade worth making. Above it
/// [`PdfProbeVm::page_count`] is `None`, which the viewer renders by omitting
/// the count rather than by guessing one.
pub const PDF_PROBE_MAX_BYTES: u64 = 25_000_000;

/// The largest total inflation the PDF object-stream probe may cause.
pub const PDF_MAX_INFLATED_BYTES: u64 = 32_000_000;

/// Worksheets projected from a workbook.
pub const MAX_SHEETS: usize = 16;

/// Rows projected from one worksheet.
pub const MAX_ROWS_PER_SHEET: usize = 500;

/// Columns projected from one row.
pub const MAX_COLUMNS: usize = 40;

/// Cells projected from the whole workbook, shared across its sheets.
///
/// Separate from the per-sheet caps because sixteen sheets each just under
/// [`MAX_ROWS_PER_SHEET`] multiply into a view model far larger than any one
/// sheet's cap suggests.
pub const MAX_CELLS: usize = 20_000;

/// Characters kept from one cell. A spreadsheet cell holding a novel is a
/// spreadsheet cell, not a document.
pub const MAX_CELL_CHARS: usize = 256;

/// Paragraphs projected from a Word document.
pub const MAX_BLOCKS: usize = 3_000;

/// Characters kept from one paragraph.
pub const MAX_BLOCK_CHARS: usize = 20_000;

/// Slides projected from a presentation.
pub const MAX_SLIDES: usize = 150;

/// Text lines projected from one slide.
pub const MAX_SLIDE_LINES: usize = 40;

/// What keeper found when it opened the file.
///
/// Determined by [`sniff`] from the bytes, never from the name. There is no
/// `Unknown` member: a file that is none of these produces no `DocumentVm` at
/// all, because "keeper opened it and it is not a document" is a sentence for
/// [`DocumentVm::detail`] rather than a format to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DocumentFormat {
    Pdf,
    Docx,
    Pptx,
    Xlsx,
}

impl DocumentFormat {
    /// What a person calls this format. Matches the `label` on the matching
    /// `ViewerEntry` row so the two surfaces cannot word the same file
    /// differently.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pdf => "PDF",
            Self::Docx => "Word document",
            Self::Pptx => "Presentation",
            Self::Xlsx => "Spreadsheet",
        }
    }
}

/// A document as a viewer can show it (Story 45.8).
///
/// **Exactly one of the four bodies is `Some`, and only when parsing produced
/// something worth drawing.** All four `None` with a `detail` set is the honest
/// failure: the file was found, keeper could not read it as a document, and the
/// sentence says why. The viewer renders name, size, Open With and that
/// sentence — never a blank pane, which is the "empty box" lie
/// `mermaid-widget.ts` was written to avoid.
///
/// **`format` is what was FOUND, not what was expected.** A `.xlsx` holding a
/// PDF reports `Pdf`. The frontend compares it to the registry's row and says
/// so when they differ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DocumentVm {
    /// What the bytes turned out to be, or `None` when they are not a document
    /// keeper knows.
    pub format: Option<DocumentFormat>,
    /// The file's real size in bytes.
    ///
    /// `number` rather than ts-rs's `bigint` for `u64`, the reading
    /// [`crate::text_file::TextFileVm::size_bytes`] takes: Tauri delivers JSON,
    /// a JSON number loses precision above 2^53, and 2^53 bytes is nine
    /// petabytes.
    #[ts(type = "number")]
    pub size_bytes: u64,
    /// `size_bytes` in the units a person reads, formatted once in Rust by
    /// [`format_file_size`] so this and the Files pane's size column can never
    /// disagree about the same file.
    pub size_label: String,
    /// The one sentence explaining a non-ordinary outcome — unreadable, over a
    /// cap, encrypted, truncated. `None` when the document opened cleanly and
    /// there is nothing to explain.
    pub detail: Option<String>,
    /// Something the document contains was left out to stay within a cap. The
    /// per-collection flags say what; this is the summary a header can read
    /// without inspecting the body.
    pub truncated: bool,
    pub pdf: Option<PdfProbeVm>,
    pub words: Option<WordsVm>,
    pub slides: Option<SlidesVm>,
    pub sheets: Option<SheetsVm>,
}

/// What keeper can say about a PDF without rendering one.
///
/// **A probe, not a parser.** It reads the magic, the version, whether the file
/// declares encryption, and — by scanning for the page tree, inflating object
/// streams when it must — how many pages there are. It resolves no cross
/// reference table and follows no indirect object, because the thing that draws
/// the pages is the webview's own PDF renderer and it does not need keeper's
/// help.
///
/// Every field is allowed to be `None`, and `None` means *keeper does not
/// know*. Nothing here guesses. A header that says "12 pages" when the document
/// has 400 is worse than a header that says nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PdfProbeVm {
    /// The version from the `%PDF-` header, e.g. `1.7`.
    pub version: Option<String>,
    /// How many pages, or `None` when the probe could not determine it — the
    /// file is above [`PDF_PROBE_MAX_BYTES`], or its page tree is somewhere
    /// this probe deliberately does not follow.
    pub page_count: Option<u32>,
    /// The document declares an `/Encrypt` dictionary. The webview may still
    /// render it (an owner-password PDF opens read-only for everyone), but a
    /// blank pane is expected rather than surprising, so the viewer says so up
    /// front.
    pub encrypted: bool,
    /// Whether Story 45.7's `keeper-file://` scheme will serve these bytes to
    /// the webview — which is what decides whether the pages can be drawn at
    /// all.
    ///
    /// **This exists because two correct rules disagree, and the disagreement
    /// is silent.** [`sniff`] decides what a document IS from its CONTENT, so a
    /// PDF somebody renamed `quarterly.xlsx` is correctly reported as a PDF.
    /// `file_asset::is_servable_path` decides what the protocol will serve from
    /// its NAME, deliberately and for reasons of its own — it must answer
    /// before any path work happens. So that renamed file is a PDF the protocol
    /// refuses, and an `<embed>` pointed at it would 404 into an empty element.
    /// A failed plugin render is NOT observable from JavaScript, so the viewer
    /// would show a blank pane and say nothing: the exact "empty box" lie this
    /// story is written against.
    ///
    /// Answered by calling `is_servable_path` itself, never by the frontend
    /// re-deriving it. A second copy of that allow-list in TypeScript would be
    /// the fourth classifier AD-73 exists to prevent, and it would drift the
    /// first time somebody widened one of them.
    pub servable: bool,
}

/// A Word document as flowed text.
///
/// **Text fidelity, not layout fidelity, and the distinction is deliberate.**
/// Paragraphs, their outline level, and bold/italic within them survive.
/// Columns, floats, page geometry, images, footnotes and table *structure* do
/// not. Reproducing Word's layout engine is not a story, it is a product, and a
/// half-built one renders documents subtly wrong in a way a reader cannot
/// detect. What this does guarantee is that no text is silently dropped:
/// paragraphs inside a table are still paragraphs and still appear, in document
/// order, so a reader never sees a document that looks complete and is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WordsVm {
    /// The first [`MAX_BLOCKS`] paragraphs, in document order.
    pub blocks: Vec<WordBlockVm>,
    /// How many paragraphs the document has — all of them, counted while
    /// streaming, not `blocks.len()`.
    pub block_count: u32,
    /// `blocks` is a prefix.
    pub truncated: bool,
}

/// What a paragraph is, coarsely enough to render and no finer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum WordBlockStyle {
    Title,
    Heading1,
    Heading2,
    Heading3,
    ListItem,
    Quote,
    Paragraph,
}

/// One paragraph: what kind it is, and the runs that make it up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WordBlockVm {
    pub style: WordBlockStyle,
    /// The runs, in order. Empty for an empty paragraph, which is kept because
    /// a blank line between paragraphs is content.
    pub runs: Vec<WordRunVm>,
}

/// A stretch of text with one set of emphasis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WordRunVm {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
}

/// A presentation as a slide outline.
///
/// **An outline, and the viewer says the word.** A slide is a canvas of
/// positioned shapes with themes, masters, transforms and inherited layout;
/// drawing one faithfully means implementing DrawingML. keeper extracts the
/// text in reading order and labels what it is showing, which is genuinely
/// useful for finding the deck you meant and honest about not being the deck.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SlidesVm {
    /// The first [`MAX_SLIDES`] slides, in presentation order.
    pub slides: Vec<SlideVm>,
    /// How many slides the deck has. Counted from the container's part list, so
    /// it is exact even when `slides` is a prefix and even when a slide failed
    /// to parse.
    pub slide_count: u32,
    /// `slides` is a prefix.
    pub truncated: bool,
}

/// One slide's text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SlideVm {
    /// 1-based, matching what the presentation itself calls this slide.
    pub number: u32,
    /// The title placeholder's text, when the slide has one.
    pub title: Option<String>,
    /// Every other text line on the slide, in reading order.
    pub lines: Vec<String>,
}

/// A workbook as sheets of strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SheetsVm {
    /// The first [`MAX_SHEETS`] sheets, in workbook order.
    pub sheets: Vec<SheetVm>,
    /// How many sheets the workbook has.
    pub sheet_count: u32,
    /// `sheets` is a prefix.
    pub truncated: bool,
}

/// One worksheet.
///
/// Cells are strings because this is a viewer: a number's *displayed* form
/// depends on its number format, and keeper does not implement Excel's format
/// language. The raw value is shown, which is what the file actually stores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SheetVm {
    pub name: String,
    /// The first [`MAX_ROWS_PER_SHEET`] rows, each padded to the row's own
    /// width. Ragged rows stay ragged; the frontend pads to `column_count`.
    pub rows: Vec<Vec<String>>,
    /// How many rows the sheet has — all of them, counted while streaming.
    pub row_count: u32,
    /// The widest row seen anywhere in the sheet, capped at [`MAX_COLUMNS`].
    pub column_count: u32,
    /// `rows` is a prefix, or a row was cut at [`MAX_COLUMNS`], or the
    /// workbook-wide [`MAX_CELLS`] budget ran out on this sheet.
    pub truncated: bool,
}

/// Why a document could not be read. Every member is a sentence a viewer shows
/// verbatim, so the wording lives here rather than being reinvented per surface.
#[derive(Debug)]
enum Refusal {
    /// The bytes are not a format this module knows.
    NotADocument,
    /// The container is larger than [`OOXML_MAX_BYTES`].
    TooLarge(u64),
    /// The ZIP could not be opened, or a part is missing.
    Malformed(String),
    /// A cap fired.
    Bomb(String),
}

impl Refusal {
    fn sentence(&self, format: Option<DocumentFormat>) -> String {
        let what = format.map_or("this file", DocumentFormat::label);
        match self {
            Self::NotADocument => {
                "keeper could not recognise this file as a PDF, Word, PowerPoint or Excel document"
                    .to_owned()
            }
            Self::TooLarge(size) => format!(
                "this {} is {}, larger than the {} keeper will open — use Open With",
                what.to_lowercase(),
                format_file_size(*size),
                format_file_size(OOXML_MAX_BYTES)
            ),
            Self::Malformed(why) => {
                format!("keeper could not read this {}: {why}", what.to_lowercase())
            }
            Self::Bomb(why) => {
                format!("keeper stopped reading this {}: {why}", what.to_lowercase())
            }
        }
    }
}

/// A running inflation budget, shared across every part of one document.
///
/// Passed by `&mut` rather than being a field on a parser struct so that every
/// call site that inflates has to name it, and adding a part that forgets to
/// charge the budget does not compile.
struct Budget {
    remaining: u64,
}

impl Budget {
    const fn new(total: u64) -> Self {
        Self { remaining: total }
    }

    /// Charge `used` bytes, or refuse when the document has spent its budget.
    fn charge(&mut self, used: u64) -> Result<(), Refusal> {
        self.remaining = self.remaining.saturating_sub(used);
        if self.remaining == 0 {
            return Err(Refusal::Bomb(format!(
                "it expands to more than {} once decompressed, which is the shape of a decompression bomb rather than a document",
                format_file_size(OOXML_MAX_INFLATED_BYTES)
            )));
        }
        Ok(())
    }
}

/// Open one file and produce whatever a viewer can show of it.
///
/// Never `Err` for a document-shaped problem: an unreadable, oversize, corrupt
/// or encrypted file is a `DocumentVm` carrying a sentence, because all four are
/// things the viewer draws rather than things the caller handles. `Err` is
/// reserved for the file not being readable at all — no such path, no
/// permission — which is the OS's own error and deserves its own words.
pub fn open_document(path: &Path) -> io::Result<DocumentVm> {
    let size_bytes = std::fs::metadata(path)?.len();
    let head = read_prefix(path, SNIFF_BYTES)?;
    let format = sniff(path, &head);

    let mut vm = DocumentVm {
        format,
        size_bytes,
        size_label: format_file_size(size_bytes),
        detail: None,
        truncated: false,
        pdf: None,
        words: None,
        slides: None,
        sheets: None,
    };

    let Some(format) = format else {
        vm.detail = Some(Refusal::NotADocument.sentence(None));
        return Ok(vm);
    };

    match format {
        DocumentFormat::Pdf => {
            let probe = probe_pdf(path, size_bytes)?;
            if probe.page_count.is_none() && size_bytes > PDF_PROBE_MAX_BYTES {
                vm.detail = Some(format!(
                    "this PDF is {}, so keeper did not scan it for a page count — the pages themselves still render",
                    format_file_size(size_bytes)
                ));
            } else if probe.encrypted {
                vm.detail = Some(
                    "this PDF is encrypted, so it may not render without its password".to_owned(),
                );
            }
            vm.pdf = Some(probe);
        }
        DocumentFormat::Docx | DocumentFormat::Pptx | DocumentFormat::Xlsx => {
            if size_bytes > OOXML_MAX_BYTES {
                vm.detail = Some(Refusal::TooLarge(size_bytes).sentence(Some(format)));
                return Ok(vm);
            }
            match read_ooxml(path, format) {
                Ok(body) => {
                    vm.truncated = body.truncated;
                    if body.truncated {
                        vm.detail = Some(body.note);
                    }
                    vm.words = body.words;
                    vm.slides = body.slides;
                    vm.sheets = body.sheets;
                }
                Err(refusal) => vm.detail = Some(refusal.sentence(Some(format))),
            }
        }
    }

    Ok(vm)
}

/// How much of the head is needed to tell the four formats apart.
const SNIFF_BYTES: u64 = 8;

/// What the bytes are, from the bytes.
///
/// `path` is used only for the ZIP case, where telling DOCX from PPTX from XLSX
/// needs the container's part list rather than its first bytes — all three
/// begin `PK\x03\x04`. Reading the central directory is a seek to the end of
/// the file, not a scan, so this stays cheap for a large container.
fn sniff(path: &Path, head: &[u8]) -> Option<DocumentFormat> {
    if head.starts_with(b"%PDF-") {
        return Some(DocumentFormat::Pdf);
    }
    // Every OOXML container is a ZIP. An empty ZIP (`PK\x05\x06`) holds no
    // main part and is correctly rejected below.
    if !head.starts_with(b"PK") {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(io::BufReader::new(file)).ok()?;
    ooxml_format(&mut archive)
}

/// Which OOXML flavour a container is, by which main part it holds.
fn ooxml_format<R: io::Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Option<DocumentFormat> {
    if archive.by_name("word/document.xml").is_ok() {
        return Some(DocumentFormat::Docx);
    }
    if archive.by_name("ppt/presentation.xml").is_ok() {
        return Some(DocumentFormat::Pptx);
    }
    if archive.by_name("xl/workbook.xml").is_ok() {
        return Some(DocumentFormat::Xlsx);
    }
    None
}

/// Read at most `limit` bytes from the head of a file.
///
/// `Read::take` rather than `fs::read` followed by a slice, the same reasoning
/// [`crate::text_file`] gives: the point is not to hold the whole file for even
/// an instant.
fn read_prefix(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    io::BufReader::new(file).take(limit).read_to_end(&mut buf)?;
    Ok(buf)
}

// ---------------------------------------------------------------------------
// OOXML
// ---------------------------------------------------------------------------

/// One parsed OOXML body plus whether anything was left out.
struct OoxmlBody {
    words: Option<WordsVm>,
    slides: Option<SlidesVm>,
    sheets: Option<SheetsVm>,
    truncated: bool,
    note: String,
}

fn read_ooxml(path: &Path, format: DocumentFormat) -> Result<OoxmlBody, Refusal> {
    let file = std::fs::File::open(path)
        .map_err(|error| Refusal::Malformed(format!("it could not be opened ({error})")))?;
    let mut archive = zip::ZipArchive::new(io::BufReader::new(file))
        .map_err(|error| Refusal::Malformed(format!("its container is damaged ({error})")))?;
    let mut budget = Budget::new(OOXML_MAX_INFLATED_BYTES);

    match format {
        DocumentFormat::Docx => read_docx(&mut archive, &mut budget),
        DocumentFormat::Pptx => read_pptx(&mut archive, &mut budget),
        DocumentFormat::Xlsx => read_xlsx(&mut archive, &mut budget),
        DocumentFormat::Pdf => unreachable!("a PDF is not a ZIP container"),
    }
}

/// Whether a container holds a part, releasing the borrow before returning.
///
/// `by_name` hands back a reader that borrows the archive, so the obvious
/// `archive.by_name(x).is_ok().then(|| read_part(archive, x))` holds the
/// lookup's borrow across the read and does not compile. Confining it to a
/// function is the smallest fix that keeps the call sites readable.
fn has_part<R: io::Read + io::Seek>(archive: &mut zip::ZipArchive<R>, name: &str) -> bool {
    archive.by_name(name).is_ok()
}

/// Read one part, inflating no more than the caps allow.
///
/// Three guards, in increasing cost order:
///
/// 1. The declared uncompressed size, which is free but attacker-controlled and
///    therefore only ever used to refuse *early*, never to trust.
/// 2. `Read::take` at [`OOXML_MAX_PART_BYTES`] plus one, so exceeding the cap is
///    detectable rather than silently producing a truncated part that parses
///    into a plausible-looking half document.
/// 3. The document-wide [`Budget`], because a bomb does not have to be one part.
fn read_part<R: io::Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
    budget: &mut Budget,
) -> Result<String, Refusal> {
    let mut part = archive
        .by_name(name)
        .map_err(|_| Refusal::Malformed(format!("{name} is missing")))?;

    if part.size() > OOXML_MAX_PART_BYTES {
        return Err(Refusal::Bomb(format!(
            "{name} declares {} once decompressed, past the {} keeper will inflate",
            format_file_size(part.size()),
            format_file_size(OOXML_MAX_PART_BYTES)
        )));
    }

    let mut bytes = Vec::new();
    part.by_ref()
        .take(OOXML_MAX_PART_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            Refusal::Malformed(format!("{name} could not be decompressed ({error})"))
        })?;

    if bytes.len() as u64 > OOXML_MAX_PART_BYTES {
        return Err(Refusal::Bomb(format!(
            "{name} inflates past the {} keeper will hold for one part, so it is a decompression bomb rather than a document",
            format_file_size(OOXML_MAX_PART_BYTES)
        )));
    }
    budget.charge(bytes.len() as u64)?;

    String::from_utf8(bytes)
        .map_err(|_| Refusal::Malformed(format!("{name} is not valid UTF-8 text")))
}

/// The local element name, with any namespace prefix removed.
///
/// OOXML parts declare `w:`, `a:` and `p:` prefixes that a writer is free to
/// rename, so matching on the qualified name would work for Word's output and
/// fail for a conforming producer that spells the prefixes differently.
fn local(name: quick_xml::name::QName<'_>) -> Vec<u8> {
    name.local_name().as_ref().to_vec()
}

/// The text of a text event, never expanding an entity it does not recognise.
///
/// Two steps, and the second is the load-bearing one.
/// [`quick_xml::escape::unescape`] resolves the five XML built-ins and *errors*
/// on anything else — there is a `unescape_with` taking a resolver for callers
/// who want DTD entities, and this deliberately does not use it. On that error
/// the raw characters are kept: showing a reader `&lol9;` is correct, because
/// it is what the file says, and is unambiguously better than either expanding
/// it or dropping the paragraph.
fn text_of(event: &quick_xml::events::BytesText<'_>) -> String {
    let Ok(decoded) = event.decode() else {
        return String::from_utf8_lossy(event.as_ref()).into_owned();
    };
    quick_xml::escape::unescape(&decoded).map_or_else(
        |_| decoded.clone().into_owned(),
        std::borrow::Cow::into_owned,
    )
}

/// Whether an OOXML boolean toggle attribute is on.
///
/// `<w:b/>` means bold; `<w:b w:val="0"/>` means *not* bold, and appears
/// whenever a style sets bold and one run turns it off. Treating the element's
/// presence as truth would render those runs bold — the common failure of a
/// naive DOCX reader.
fn toggle_on(event: &quick_xml::events::BytesStart<'_>) -> bool {
    for attribute in event.attributes().flatten() {
        if local(attribute.key) == b"val" {
            let value = attribute.unescape_value().unwrap_or_default();
            return !matches!(value.as_ref(), "0" | "false" | "off");
        }
    }
    true
}

// ---------------------------------------------------------------------------
// DOCX
// ---------------------------------------------------------------------------

fn read_docx<R: io::Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    budget: &mut Budget,
) -> Result<OoxmlBody, Refusal> {
    let xml = read_part(archive, "word/document.xml", budget)?;
    let words = parse_docx(&xml)?;
    let truncated = words.truncated;
    Ok(OoxmlBody {
        note: format!(
            "this document has {} paragraphs; keeper is showing the first {MAX_BLOCKS}",
            words.block_count
        ),
        words: Some(words),
        slides: None,
        sheets: None,
        truncated,
    })
}

/// Walk `word/document.xml` and project its paragraphs.
///
/// Streaming rather than building a tree: the document's own size decides how
/// much a tree would cost, which is exactly the decision an attacker should not
/// get to make. Depth is tracked with flags rather than a stack because the
/// only nesting that matters is shallow and known.
///
/// **Text is collected inside `w:t` and nowhere else.** The whitespace a
/// pretty-printing producer puts between `<w:r>` elements is markup, not
/// content, and treating every text event inside a paragraph as prose pulls it
/// in — turning one run into two and prefixing every paragraph with a newline
/// and the writer's indentation. It also happens to be why `w:instrText` and
/// `w:delText` need no special case: a field code and a tracked deletion are
/// different elements, so gating on `w:t` excludes both by construction rather
/// than by a list that the next OOXML element will not be on.
fn parse_docx(xml: &str) -> Result<WordsVm, Refusal> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(xml);
    let mut blocks: Vec<WordBlockVm> = Vec::new();
    let mut block_count: u32 = 0;

    // The paragraph being built, if any.
    let mut style = WordBlockStyle::Paragraph;
    let mut runs: Vec<WordRunVm> = Vec::new();
    let mut in_paragraph = false;
    // Emphasis of the run being built. `in_run_properties` distinguishes
    // `w:b` inside `w:rPr` (this run is bold) from the same element inside
    // `w:pPr`'s style definitions.
    let mut bold = false;
    let mut italic = false;
    let mut in_run_properties = false;
    let mut in_paragraph_properties = false;
    let mut run_text = String::new();
    // Set only inside `w:t`. See the doc comment: this is what keeps a
    // producer's indentation, a field code and a tracked deletion out of the
    // prose.
    let mut in_text = false;

    loop {
        match reader.read_event() {
            Err(error) => {
                return Err(Refusal::Malformed(format!(
                    "its text is not valid XML ({error})"
                )))
            }
            Ok(Event::Eof) => break,
            Ok(Event::Start(event)) => match local(event.name()).as_slice() {
                b"p" => {
                    in_paragraph = true;
                    style = WordBlockStyle::Paragraph;
                    runs.clear();
                }
                b"pPr" => in_paragraph_properties = true,
                b"rPr" => in_run_properties = true,
                b"b" if in_run_properties => bold = toggle_on(&event),
                b"i" if in_run_properties => italic = toggle_on(&event),
                b"t" => in_text = true,
                _ => {}
            },
            Ok(Event::Empty(event)) => match local(event.name()).as_slice() {
                b"b" if in_run_properties => bold = toggle_on(&event),
                b"i" if in_run_properties => italic = toggle_on(&event),
                b"pStyle" if in_paragraph_properties => {
                    if let Some(named) = attribute_value(&event, b"val") {
                        style = style_for(&named);
                    }
                }
                // A numbered or bulleted paragraph. It wins over a `pStyle` of
                // `ListParagraph` only in the sense that both map here.
                b"numPr" if in_paragraph_properties => style = WordBlockStyle::ListItem,
                // A soft line break inside a run is content.
                b"br" | b"cr" if in_paragraph => run_text.push('\n'),
                b"tab" if in_paragraph => run_text.push('\t'),
                _ => {}
            },
            Ok(Event::Text(event)) if in_text => {
                run_text.push_str(&text_of(&event));
            }
            Ok(Event::End(event)) => match local(event.name()).as_slice() {
                b"p" if in_paragraph => {
                    flush_run(&mut runs, &mut run_text, bold, italic);
                    block_count = block_count.saturating_add(1);
                    if blocks.len() < MAX_BLOCKS {
                        blocks.push(WordBlockVm {
                            style,
                            runs: std::mem::take(&mut runs),
                        });
                    }
                    in_paragraph = false;
                    runs.clear();
                }
                b"r" => {
                    flush_run(&mut runs, &mut run_text, bold, italic);
                    bold = false;
                    italic = false;
                }
                b"rPr" => in_run_properties = false,
                b"pPr" => in_paragraph_properties = false,
                b"t" => in_text = false,
                _ => {}
            },
            Ok(_) => {}
        }
    }

    // An open paragraph at end of input means the part stopped mid-document —
    // a truncated or corrupt container. Returning what was parsed would render
    // a half document that looks whole, which is the one outcome a viewer must
    // never produce: the reader cannot tell, and will believe the file is
    // damaged when it is keeper that gave up early.
    if in_paragraph {
        return Err(Refusal::Malformed(
            "its text ends part-way through a paragraph, so the file is truncated".to_owned(),
        ));
    }

    Ok(WordsVm {
        truncated: block_count as usize > blocks.len(),
        block_count,
        blocks,
    })
}

/// Close the run being accumulated, dropping it when it carries no text.
///
/// A DOCX emits runs holding only properties, bookmarks or proofing marks; each
/// would otherwise become an empty span the renderer has to filter.
fn flush_run(runs: &mut Vec<WordRunVm>, text: &mut String, bold: bool, italic: bool) {
    if text.is_empty() {
        return;
    }
    let mut taken = std::mem::take(text);
    truncate_chars(&mut taken, MAX_BLOCK_CHARS);
    runs.push(WordRunVm {
        text: taken,
        bold,
        italic,
    });
}

/// Word's built-in style names, mapped to the handful of shapes a viewer draws.
///
/// Only the outline levels a reader navigates by are distinguished. `Heading4`
/// and below become paragraphs deliberately: rendering six heading sizes in a
/// preview pane produces a wall of near-identical text, and the reader gains
/// nothing over the three that are visually distinct.
fn style_for(name: &str) -> WordBlockStyle {
    match name {
        "Title" => WordBlockStyle::Title,
        "Heading1" | "heading 1" => WordBlockStyle::Heading1,
        "Heading2" | "heading 2" => WordBlockStyle::Heading2,
        "Heading3" | "heading 3" => WordBlockStyle::Heading3,
        "ListParagraph" => WordBlockStyle::ListItem,
        "Quote" | "IntenseQuote" => WordBlockStyle::Quote,
        _ => WordBlockStyle::Paragraph,
    }
}

/// One attribute's value by local name.
fn attribute_value(event: &quick_xml::events::BytesStart<'_>, want: &[u8]) -> Option<String> {
    event.attributes().flatten().find_map(|attribute| {
        (local(attribute.key) == want)
            .then(|| {
                attribute
                    .unescape_value()
                    .ok()
                    .map(std::borrow::Cow::into_owned)
            })
            .flatten()
    })
}

/// Cut a string to `limit` CHARACTERS, never mid-character.
///
/// `String::truncate` takes a byte index and panics on a character boundary, so
/// a cap applied to bytes would crash on a document whose 256th byte lands
/// inside a multi-byte character — which is to say, on most documents not
/// written in English.
fn truncate_chars(text: &mut String, limit: usize) {
    if let Some((index, _)) = text.char_indices().nth(limit) {
        text.truncate(index);
    }
}

// ---------------------------------------------------------------------------
// PPTX
// ---------------------------------------------------------------------------

fn read_pptx<R: io::Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    budget: &mut Budget,
) -> Result<OoxmlBody, Refusal> {
    let names = slide_part_names(archive);
    if names.is_empty() {
        return Err(Refusal::Malformed("it contains no slides".to_owned()));
    }
    let slide_count = u32::try_from(names.len()).unwrap_or(u32::MAX);
    let truncated = names.len() > MAX_SLIDES;

    let mut slides = Vec::new();
    for (index, name) in names.iter().take(MAX_SLIDES).enumerate() {
        let xml = read_part(archive, name, budget)?;
        let mut slide = parse_slide(&xml)?;
        slide.number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        slides.push(slide);
    }

    Ok(OoxmlBody {
        note: format!(
            "this presentation has {slide_count} slides; keeper is showing the first {MAX_SLIDES}"
        ),
        words: None,
        slides: Some(SlidesVm {
            slides,
            slide_count,
            truncated,
        }),
        sheets: None,
        truncated,
    })
}

/// Every `ppt/slides/slideN.xml`, in presentation order.
///
/// Sorted by the number in the name, NUMERICALLY. A lexicographic sort — which
/// is what the container's own part order and every naive `sort()` give — puts
/// slide 10 before slide 2, so a deck of more than nine slides renders shuffled.
/// That is the whole reason this is a function and not an inline filter.
fn slide_part_names<R: io::Read + io::Seek>(archive: &zip::ZipArchive<R>) -> Vec<String> {
    let mut named: Vec<(u32, String)> = archive
        .file_names()
        .filter(|name| name.starts_with("ppt/slides/slide") && name.ends_with(".xml"))
        .filter_map(|name| slide_number(name).map(|number| (number, name.to_owned())))
        .collect();
    named.sort_unstable();
    named.into_iter().map(|(_, name)| name).collect()
}

/// The `N` in `ppt/slides/slideN.xml`.
fn slide_number(name: &str) -> Option<u32> {
    name.strip_prefix("ppt/slides/slide")?
        .strip_suffix(".xml")?
        .parse()
        .ok()
}

/// Walk one slide and project its text, keeping the title placeholder apart.
///
/// A slide is shapes; a shape holds paragraphs; a paragraph holds runs. The
/// title is the shape whose placeholder type is `title` or `ctrTitle`. Text is
/// accumulated per PARAGRAPH rather than per run, because PowerPoint splits a
/// sentence across runs at every formatting change and a per-run projection
/// turns one bullet into eight fragments.
fn parse_slide(xml: &str) -> Result<SlideVm, Refusal> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(xml);
    let mut title: Option<String> = None;
    let mut lines: Vec<String> = Vec::new();

    let mut shape_is_title = false;
    let mut shape_lines: Vec<String> = Vec::new();
    let mut paragraph = String::new();
    let mut in_shape = false;
    // Set only inside `a:t`, for the reason `parse_docx` spells out: a
    // pretty-printed slide's indentation is markup, and collecting every text
    // event would put it into every bullet.
    let mut in_text = false;

    loop {
        match reader.read_event() {
            Err(error) => {
                return Err(Refusal::Malformed(format!(
                    "a slide is not valid XML ({error})"
                )))
            }
            Ok(Event::Eof) => break,
            // `a:t` only ever opens on a Start: an empty `<a:t/>` carries no
            // text, and setting the flag for one would leave it set — every
            // stretch of markup whitespace after it would become bullet text.
            Ok(Event::Start(event)) if local(event.name()) == b"t" => in_text = true,
            Ok(Event::Start(event) | Event::Empty(event)) => {
                match local(event.name()).as_slice() {
                    b"sp" => {
                        in_shape = true;
                        shape_is_title = false;
                        shape_lines.clear();
                    }
                    // `<p:ph type="title"/>` — the placeholder that makes this
                    // shape the slide's title. A shape with no `type` is a body
                    // placeholder, which is why the default is not "title".
                    b"ph" => {
                        if let Some(kind) = attribute_value(&event, b"type") {
                            shape_is_title = matches!(kind.as_str(), "title" | "ctrTitle");
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(event)) if in_text => paragraph.push_str(&text_of(&event)),
            Ok(Event::End(event)) => match local(event.name()).as_slice() {
                b"t" => in_text = false,
                // `a:p` closes one paragraph of text within a shape.
                b"p" => {
                    let line = paragraph.trim().to_owned();
                    paragraph.clear();
                    if !line.is_empty() && in_shape {
                        shape_lines.push(line);
                    }
                }
                b"sp" => {
                    in_shape = false;
                    if shape_is_title && title.is_none() {
                        title = shape_lines.first().cloned();
                        // Anything else in the title shape is still text on the
                        // slide and must not vanish because it shared a box
                        // with the title.
                        lines.extend(shape_lines.iter().skip(1).cloned());
                    } else {
                        lines.append(&mut shape_lines);
                    }
                    shape_lines.clear();
                }
                // A run's text is joined into the paragraph, not emitted.
                _ => {}
            },
            Ok(_) => {}
        }
    }

    lines.truncate(MAX_SLIDE_LINES);
    for line in &mut lines {
        truncate_chars(line, MAX_CELL_CHARS);
    }
    if let Some(title) = title.as_mut() {
        truncate_chars(title, MAX_CELL_CHARS);
    }

    Ok(SlideVm {
        number: 0,
        title,
        lines,
    })
}

// ---------------------------------------------------------------------------
// XLSX
// ---------------------------------------------------------------------------

fn read_xlsx<R: io::Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    budget: &mut Budget,
) -> Result<OoxmlBody, Refusal> {
    let workbook = read_part(archive, "xl/workbook.xml", budget)?;
    let declared = parse_workbook(&workbook)?;
    if declared.is_empty() {
        return Err(Refusal::Malformed("it contains no sheets".to_owned()));
    }
    // Both optional parts are looked up through `has_part` rather than inline,
    // because `by_name` returns a handle that borrows the archive for as long
    // as it lives — testing for a part and then reading it in one expression
    // holds two borrows at once.
    let relationships = if has_part(archive, "xl/_rels/workbook.xml.rels") {
        parse_relationships(&read_part(archive, "xl/_rels/workbook.xml.rels", budget)?)?
    } else {
        HashMap::new()
    };

    let shared = if has_part(archive, "xl/sharedStrings.xml") {
        parse_shared_strings(&read_part(archive, "xl/sharedStrings.xml", budget)?)?
    } else {
        Vec::new()
    };

    let sheet_count = u32::try_from(declared.len()).unwrap_or(u32::MAX);
    let truncated_sheets = declared.len() > MAX_SHEETS;
    let mut cells_left = MAX_CELLS;
    let mut sheets = Vec::new();

    for (index, declared_sheet) in declared.iter().take(MAX_SHEETS).enumerate() {
        let part = sheet_part_name(&relationships, declared_sheet, index);
        // A workbook may name a sheet whose part is absent — a repaired file
        // does this. That is one broken sheet, not a broken workbook, so it is
        // rendered as an empty sheet with its name rather than failing the lot.
        let Ok(xml) = read_part(archive, &part, budget) else {
            sheets.push(SheetVm {
                name: declared_sheet.name.clone(),
                rows: Vec::new(),
                row_count: 0,
                column_count: 0,
                truncated: false,
            });
            continue;
        };
        let mut sheet = parse_sheet(&xml, &shared, &mut cells_left)?;
        sheet.name.clone_from(&declared_sheet.name);
        sheets.push(sheet);
    }

    let truncated = truncated_sheets || sheets.iter().any(|sheet| sheet.truncated);
    Ok(OoxmlBody {
        note: if truncated_sheets {
            format!(
                "this workbook has {sheet_count} sheets; keeper is showing the first {MAX_SHEETS}"
            )
        } else {
            format!("keeper is showing the first {MAX_ROWS_PER_SHEET} rows of each sheet")
        },
        words: None,
        slides: None,
        sheets: Some(SheetsVm {
            sheets,
            sheet_count,
            truncated: truncated_sheets,
        }),
        truncated,
    })
}

/// A sheet as the workbook declares it, before its part is found.
struct DeclaredSheet {
    name: String,
    relationship_id: Option<String>,
}

/// The `<sheets>` list from `xl/workbook.xml`, in workbook (tab) order.
fn parse_workbook(xml: &str) -> Result<Vec<DeclaredSheet>, Refusal> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(xml);
    let mut sheets = Vec::new();
    loop {
        match reader.read_event() {
            Err(error) => {
                return Err(Refusal::Malformed(format!(
                    "its workbook part is not valid XML ({error})"
                )))
            }
            Ok(Event::Eof) => break,
            Ok(Event::Start(event) | Event::Empty(event)) => {
                if local(event.name()) == b"sheet" {
                    sheets.push(DeclaredSheet {
                        name: attribute_value(&event, b"name")
                            .unwrap_or_else(|| format!("Sheet{}", sheets.len() + 1)),
                        relationship_id: attribute_value(&event, b"id"),
                    });
                }
            }
            Ok(_) => {}
        }
    }
    Ok(sheets)
}

/// `rId` → part path, from `xl/_rels/workbook.xml.rels`.
fn parse_relationships(xml: &str) -> Result<HashMap<String, String>, Refusal> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(xml);
    let mut map = HashMap::new();
    loop {
        match reader.read_event() {
            Err(error) => {
                return Err(Refusal::Malformed(format!(
                    "its relationship part is not valid XML ({error})"
                )))
            }
            Ok(Event::Eof) => break,
            Ok(Event::Start(event) | Event::Empty(event)) => {
                if local(event.name()) == b"Relationship" {
                    if let (Some(id), Some(target)) = (
                        attribute_value(&event, b"Id"),
                        attribute_value(&event, b"Target"),
                    ) {
                        map.insert(id, target);
                    }
                }
            }
            Ok(_) => {}
        }
    }
    Ok(map)
}

/// Where one declared sheet's XML lives.
///
/// The relationship is authoritative — a workbook may map `rId1` to
/// `worksheets/sheet7.xml`, and assuming position would then show the wrong
/// data under the right tab name, which is the worst kind of wrong because it
/// looks right. Position is only the fallback for a container with no
/// relationship part.
fn sheet_part_name(
    relationships: &HashMap<String, String>,
    sheet: &DeclaredSheet,
    index: usize,
) -> String {
    let target = sheet
        .relationship_id
        .as_ref()
        .and_then(|id| relationships.get(id));
    match target {
        // Targets are relative to `xl/`, and may be spelled with a leading
        // slash to mean the package root.
        Some(target) => target
            .strip_prefix('/')
            .map_or_else(|| format!("xl/{target}"), std::borrow::ToOwned::to_owned),
        None => format!("xl/worksheets/sheet{}.xml", index + 1),
    }
}

/// The shared-string table.
///
/// Excel stores every distinct string once and refers to it by index, so a
/// worksheet cell of type `s` holds a number. A reader that skipped this part
/// would render a spreadsheet of integers.
fn parse_shared_strings(xml: &str) -> Result<Vec<String>, Refusal> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(xml);
    let mut strings = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut in_text = false;

    loop {
        match reader.read_event() {
            Err(error) => {
                return Err(Refusal::Malformed(format!(
                    "its shared strings are not valid XML ({error})"
                )))
            }
            Ok(Event::Eof) => break,
            Ok(Event::Start(event)) => match local(event.name()).as_slice() {
                b"si" => {
                    in_string = true;
                    current.clear();
                }
                // A rich-text `si` holds several `<r><t>` runs and its value is
                // their CONCATENATION, so `current` spans the whole `si` and is
                // only flushed at its end. `t` is still what gates collection —
                // the `rPr` formatting elements between the runs are markup.
                b"t" if in_string => in_text = true,
                _ => {}
            },
            Ok(Event::Text(event)) if in_text => current.push_str(&text_of(&event)),
            Ok(Event::End(event)) => match local(event.name()).as_slice() {
                b"t" => in_text = false,
                b"si" if in_string => {
                    in_string = false;
                    truncate_chars(&mut current, MAX_CELL_CHARS);
                    strings.push(std::mem::take(&mut current));
                }
                _ => {}
            },
            Ok(_) => {}
        }
    }
    Ok(strings)
}

/// Walk one worksheet and project its cells.
///
/// `cells_left` is the workbook-wide budget and is decremented across sheets, so
/// the projection is bounded by the workbook rather than by any one sheet.
///
/// Rows are streamed to the end even after the projection stops keeping them,
/// because `row_count` must be the sheet's real height. Skipping the tail would
/// make a 50 000-row sheet report 500 rows, which is 44.11's forbidden count.
fn parse_sheet(xml: &str, shared: &[String], cells_left: &mut usize) -> Result<SheetVm, Refusal> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(xml);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row_count: u32 = 0;
    let mut column_count: usize = 0;
    let mut truncated = false;

    let mut row: Vec<String> = Vec::new();
    let mut in_row = false;
    // The current cell: where it sits, what type it is, and what it holds.
    let mut cell_column: usize = 0;
    let mut cell_type = CellType::Number;
    let mut value = String::new();
    let mut in_value = false;

    loop {
        match reader.read_event() {
            Err(error) => {
                return Err(Refusal::Malformed(format!(
                    "a worksheet is not valid XML ({error})"
                )))
            }
            Ok(Event::Eof) => break,
            Ok(Event::Start(event) | Event::Empty(event)) => {
                match local(event.name()).as_slice() {
                    b"row" => {
                        in_row = true;
                        row.clear();
                    }
                    b"c" => {
                        cell_column = attribute_value(&event, b"r")
                            .as_deref()
                            .and_then(column_index)
                            .unwrap_or(row.len());
                        cell_type = attribute_value(&event, b"t")
                            .as_deref()
                            .map_or(CellType::Number, CellType::parse);
                        value.clear();
                    }
                    // `v` is the stored value; `t` inside an `is` is an inline
                    // string. Both feed the same accumulator.
                    b"v" | b"t" => in_value = true,
                    _ => {}
                }
            }
            Ok(Event::Text(event)) if in_value => value.push_str(&text_of(&event)),
            Ok(Event::End(event)) => match local(event.name()).as_slice() {
                b"v" | b"t" => in_value = false,
                b"c" if in_row => {
                    let text = cell_type.render(&value, shared);
                    // A cell carries its column, so a sparse row — Excel omits
                    // empty cells entirely — must be padded or every value
                    // after the gap shifts left into the wrong column.
                    if cell_column < MAX_COLUMNS {
                        while row.len() < cell_column {
                            row.push(String::new());
                        }
                        row.push(text);
                    } else {
                        truncated = true;
                    }
                }
                b"row" if in_row => {
                    in_row = false;
                    row_count = row_count.saturating_add(1);
                    column_count = column_count.max(row.len());
                    if rows.len() < MAX_ROWS_PER_SHEET {
                        if *cells_left < row.len() {
                            truncated = true;
                        } else {
                            *cells_left -= row.len();
                            rows.push(std::mem::take(&mut row));
                        }
                    }
                    row.clear();
                }
                _ => {}
            },
            Ok(_) => {}
        }
    }

    if row_count as usize > rows.len() {
        truncated = true;
    }

    Ok(SheetVm {
        // Overwritten by the caller with the workbook's own tab name, which is
        // the one a person sees; a worksheet part does not carry it.
        name: String::new(),
        rows,
        row_count,
        column_count: u32::try_from(column_count).unwrap_or(u32::MAX),
        truncated,
    })
}

/// What a worksheet cell's `t` attribute says it holds.
#[derive(Clone, Copy)]
enum CellType {
    /// `t="s"` — an index into the shared-string table.
    SharedString,
    /// `t="inlineStr"` or `t="str"` — the text is right there.
    InlineString,
    /// `t="b"` — `0` or `1`, and rendering it as such is not useful.
    Boolean,
    /// `t="e"` — a formula error like `#DIV/0!`, already stored as its text.
    Error,
    /// No `t`, or `t="n"`.
    Number,
}

impl CellType {
    fn parse(raw: &str) -> Self {
        match raw {
            "s" => Self::SharedString,
            "inlineStr" | "str" => Self::InlineString,
            "b" => Self::Boolean,
            "e" => Self::Error,
            _ => Self::Number,
        }
    }

    /// The text to show for one cell.
    ///
    /// A shared-string index that is out of range renders empty rather than
    /// panicking: the index comes from the file, so it is attacker-controlled,
    /// and `shared[index]` would be a crash on a malformed workbook.
    fn render(self, value: &str, shared: &[String]) -> String {
        let mut text = match self {
            Self::SharedString => value
                .parse::<usize>()
                .ok()
                .and_then(|index| shared.get(index))
                .cloned()
                .unwrap_or_default(),
            Self::Boolean => match value {
                "1" => "TRUE".to_owned(),
                "0" => "FALSE".to_owned(),
                other => other.to_owned(),
            },
            Self::InlineString | Self::Error | Self::Number => value.to_owned(),
        };
        truncate_chars(&mut text, MAX_CELL_CHARS);
        text
    }
}

/// The zero-based column from a cell reference like `AB12`.
///
/// Base-26 with no zero digit: `A` is 1, `Z` is 26, `AA` is 27. Treating it as
/// ordinary base-26 puts `AA` at 26 and shifts every column past `Z`.
fn column_index(reference: &str) -> Option<usize> {
    let mut index = 0_usize;
    let mut seen = false;
    for byte in reference.bytes() {
        if byte.is_ascii_alphabetic() {
            seen = true;
            let digit = usize::from(byte.to_ascii_uppercase() - b'A') + 1;
            index = index.checked_mul(26)?.checked_add(digit)?;
        } else {
            break;
        }
    }
    seen.then(|| index - 1)
}

// ---------------------------------------------------------------------------
// PDF
// ---------------------------------------------------------------------------

/// Read what can be read of a PDF without rendering it.
fn probe_pdf(path: &Path, size_bytes: u64) -> io::Result<PdfProbeVm> {
    // Asked on the NAME, by the protocol's own predicate, because that is the
    // question the protocol will actually answer when the webview asks it. See
    // [`PdfProbeVm::servable`] for why the name and the content can disagree.
    let servable = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(crate::file_asset::is_servable_path);

    if size_bytes > PDF_PROBE_MAX_BYTES {
        // The header is still worth having: it is the first eight bytes.
        let head = read_prefix(path, 16)?;
        return Ok(PdfProbeVm {
            version: pdf_version(&head),
            page_count: None,
            encrypted: false,
            servable,
        });
    }
    let bytes = read_prefix(path, PDF_PROBE_MAX_BYTES)?;
    Ok(PdfProbeVm {
        version: pdf_version(&bytes),
        page_count: pdf_page_count(&bytes),
        encrypted: find(&bytes, b"/Encrypt").is_some(),
        servable,
    })
}

/// The version from a `%PDF-1.7` header.
fn pdf_version(bytes: &[u8]) -> Option<String> {
    let rest = bytes.strip_prefix(b"%PDF-")?;
    let end = rest
        .iter()
        .position(|byte| !(byte.is_ascii_digit() || *byte == b'.'))
        .unwrap_or(rest.len());
    let version = std::str::from_utf8(&rest[..end]).ok()?;
    (!version.is_empty()).then(|| version.to_owned())
}

/// How many pages, or `None`.
///
/// Two strategies, in order, over the body and then over the inflated object
/// streams:
///
/// 1. **The page tree's `/Count`.** A `/Type /Pages` node states how many pages
///    hang beneath it, and the root node's number is the document's. Taking the
///    maximum over every such node finds the root without resolving a single
///    indirect reference. `/Count` also appears on `/Type /Outlines`, which is
///    why the search is anchored to a `/Pages` node's own object rather than
///    run over the whole file.
///
/// 2. **Counting `/Type /Page` objects**, for a document whose page tree this
///    probe could not read.
///
/// Object streams are inflated because a PDF 1.5+ writer packs the page tree
/// into them, and without that step the answer is `None` for most real
/// documents rather than for unusual ones.
///
/// It is a probe. A document that defeats both strategies gets `None` and the
/// viewer omits the count — the pages still render, because the webview does
/// that and does not consult this.
fn pdf_page_count(bytes: &[u8]) -> Option<u32> {
    if let Some(count) = page_count_in(bytes) {
        return Some(count);
    }
    let inflated = inflate_object_streams(bytes);
    if inflated.is_empty() {
        return None;
    }
    page_count_in(&inflated)
}

/// Both strategies over one buffer.
fn page_count_in(bytes: &[u8]) -> Option<u32> {
    let mut best: Option<u32> = None;
    let mut at = 0_usize;
    while let Some(found) = find(&bytes[at..], b"/Pages") {
        let position = at + found;
        at = position + b"/Pages".len();
        if !preceded_by_type(bytes, position) {
            continue;
        }
        if let Some(count) = count_near(bytes, position) {
            best = Some(best.map_or(count, |previous: u32| previous.max(count)));
        }
    }
    if let Some(count) = best.filter(|count| *count > 0) {
        return Some(count);
    }

    // Strategy 2. `/Page` immediately followed by `s` is a `/Pages` node and
    // must not be counted as a leaf.
    let mut pages = 0_u32;
    let mut at = 0_usize;
    while let Some(found) = find(&bytes[at..], b"/Page") {
        let position = at + found;
        at = position + b"/Page".len();
        let next = bytes.get(position + b"/Page".len()).copied();
        if matches!(next, Some(b's')) || !preceded_by_type(bytes, position) {
            continue;
        }
        pages = pages.saturating_add(1);
    }
    (pages > 0).then_some(pages)
}

/// Whether the name at `position` is the value of a `/Type` key.
///
/// Without this, the string `/Pages` appearing anywhere — in a name tree, in a
/// piece of metadata — would be mistaken for a page-tree node.
fn preceded_by_type(bytes: &[u8], position: usize) -> bool {
    let start = position.saturating_sub(32);
    let before = &bytes[start..position];
    let trimmed = before
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(before, |last| &before[..=last]);
    trimmed.ends_with(b"/Type")
}

/// The `/Count` belonging to the object containing `position`.
///
/// Bounded to the enclosing `obj`/`endobj` where those markers are within
/// reach, so a `/Count` from the next object is not attributed to this one.
fn count_near(bytes: &[u8], position: usize) -> Option<u32> {
    const REACH: usize = 4_096;
    let end = find(&bytes[position..], b"endobj")
        .map_or_else(|| bytes.len().min(position + REACH), |at| position + at);
    let start = position.saturating_sub(REACH);
    let window = &bytes[start..end];
    let mut best: Option<u32> = None;
    let mut at = 0_usize;
    while let Some(found) = find(&window[at..], b"/Count") {
        let after = at + found + b"/Count".len();
        at = after;
        if let Some(count) = read_integer(&window[after..]) {
            best = Some(best.map_or(count, |previous: u32| previous.max(count)));
        }
    }
    best
}

/// The integer at the head of `bytes`, after any whitespace.
fn read_integer(bytes: &[u8]) -> Option<u32> {
    let digits: Vec<u8> = bytes
        .iter()
        .skip_while(|byte| byte.is_ascii_whitespace())
        .take_while(|byte| byte.is_ascii_digit())
        .copied()
        .collect();
    if digits.is_empty() {
        return None;
    }
    std::str::from_utf8(&digits).ok()?.parse().ok()
}

/// Inflate every `/Type /ObjStm` stream, up to [`PDF_MAX_INFLATED_BYTES`].
///
/// A best-effort scan, deliberately: a stream whose length is an indirect
/// reference cannot be measured without resolving it, so the extent is found by
/// searching for `endstream`. A stream whose payload happens to contain those
/// nine bytes therefore inflates short, and the result is a smaller buffer to
/// search — never a wrong page count, because a partial inflate yields either a
/// `/Count` that is really in the file or nothing at all.
fn inflate_object_streams(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0_usize;
    while let Some(found) = find(&bytes[at..], b"/ObjStm") {
        let position = at + found;
        // `at` is advanced past this stream at the bottom of the loop, once its
        // extent is known. Every other path out of the body breaks.
        let Some(relative) = find(&bytes[position..], b"stream") else {
            break;
        };
        let mut start = position + relative + b"stream".len();
        // The keyword is followed by CRLF or LF, and the payload begins after
        // it. Consuming the wrong number of bytes corrupts the zlib header.
        if bytes.get(start) == Some(&b'\r') {
            start += 1;
        }
        if bytes.get(start) == Some(&b'\n') {
            start += 1;
        }
        let Some(length) = find(&bytes[start..], b"endstream") else {
            break;
        };
        let payload = &bytes[start..start + length];
        let room = PDF_MAX_INFLATED_BYTES.saturating_sub(out.len() as u64);
        if room == 0 {
            break;
        }
        let mut decoder = flate2::read::ZlibDecoder::new(payload).take(room);
        // A stream this probe cannot inflate is one it skips. It is not an
        // error: an ObjStm may use a filter chain keeper does not implement,
        // and the correct outcome is a page count of `None`, not a failure.
        let _ = decoder.read_to_end(&mut out);
        at = start + length;
    }
    out
}

/// The first occurrence of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests;
