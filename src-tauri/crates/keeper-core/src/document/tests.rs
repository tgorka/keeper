//! Tests for [`super`] (Story 45.8).
//!
//! Every fixture is BUILT here rather than checked in as a binary. A committed
//! `.docx` is a blob nobody can read in a diff and nobody can vary; a builder
//! makes the hostile cases — the bomb, the billion-laughs DTD, the fifty
//! thousand row sheet — as easy to write as the ordinary ones, which is the
//! only reason they get written at all.

use std::io::Write as _;
use std::path::PathBuf;

use super::*;

/// A scratch directory that removes itself.
///
/// Named with the process id and a counter so two tests running in parallel —
/// which `cargo nextest` does by default — cannot collide on a path.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "keeper-document-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).expect("write fixture");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Build a ZIP container from `(name, contents)` pairs, deflated.
fn zip_of(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buffer);
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, contents) in parts {
            writer.start_file(*name, options).expect("start part");
            writer.write_all(contents).expect("write part");
        }
        writer.finish().expect("finish zip");
    }
    buffer.into_inner()
}

// ---------------------------------------------------------------------------
// Format sniffing
// ---------------------------------------------------------------------------

/// The format is read from the CONTENT. A `.xlsx` holding a Word document is
/// reported as a Word document, because a viewer that trusted the extension
/// would render an empty spreadsheet over a perfectly good file.
#[test]
fn format_comes_from_the_bytes_not_the_extension() {
    let scratch = Scratch::new("sniff");
    let path = scratch.file("quarterly.xlsx", &docx_of(&[("Heading1", "Actually Word")]));

    let vm = open_document(&path).expect("open");

    assert_eq!(vm.format, Some(DocumentFormat::Docx));
    assert!(vm.words.is_some(), "it should have been read as Word");
    assert!(vm.sheets.is_none(), "and not as a spreadsheet");
}

/// A file that is not any of the four is not an error and not a blank pane: it
/// is a named refusal the viewer can render (AD-91).
#[test]
fn a_file_that_is_not_a_document_is_named_rather_than_thrown() {
    let scratch = Scratch::new("notdoc");
    let path = scratch.file("notes.bin", b"\x00\x01\x02 this is not a document at all");

    let vm = open_document(&path).expect("open");

    assert_eq!(vm.format, None);
    let detail = vm.detail.expect("a sentence");
    assert!(
        detail.contains("could not recognise"),
        "the reason must name what happened, got {detail:?}"
    );
    assert!(vm.pdf.is_none() && vm.words.is_none() && vm.slides.is_none() && vm.sheets.is_none());
}

/// A ZIP that is not an OOXML container has no main part and must not be
/// claimed as a document.
#[test]
fn a_plain_zip_is_not_a_document() {
    let scratch = Scratch::new("plainzip");
    let path = scratch.file("archive.zip", &zip_of(&[("readme.txt", b"hello")]));

    let vm = open_document(&path).expect("open");

    assert_eq!(vm.format, None);
}

/// Every document carries the size formatted by the ONE Rust formatter, so this
/// banner and the Files pane's size column cannot disagree (Story 45.5).
#[test]
fn size_is_formatted_by_the_shared_rust_formatter() {
    let scratch = Scratch::new("size");
    let bytes = docx_of(&[("Normal", "hi")]);
    let path = scratch.file("a.docx", &bytes);

    let vm = open_document(&path).expect("open");

    assert_eq!(vm.size_bytes, bytes.len() as u64);
    assert_eq!(vm.size_label, format_file_size(bytes.len() as u64));
}

// ---------------------------------------------------------------------------
// DOCX
// ---------------------------------------------------------------------------

/// Build a `word/document.xml` from `(style, text)` pairs.
fn docx_of(paragraphs: &[(&str, &str)]) -> Vec<u8> {
    let body: String = paragraphs
        .iter()
        .map(|(style, text)| {
            format!(
                r#"<w:p><w:pPr><w:pStyle w:val="{style}"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#
            )
        })
        .collect();
    docx_with_body(&body)
}

fn docx_with_body(body: &str) -> Vec<u8> {
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
    );
    zip_of(&[("word/document.xml", xml.as_bytes())])
}

/// A Word document renders as its paragraphs, with their outline level — the
/// story's "renders something asserted", asserted on content rather than on a
/// container existing.
#[test]
fn a_word_document_renders_its_paragraphs_and_their_styles() {
    let scratch = Scratch::new("docx");
    let path = scratch.file(
        "report.docx",
        &docx_of(&[
            ("Title", "Quarterly Report"),
            ("Heading1", "Revenue"),
            ("Normal", "Revenue rose."),
            ("ListParagraph", "Europe"),
        ]),
    );

    let vm = open_document(&path).expect("open");

    assert_eq!(vm.format, Some(DocumentFormat::Docx));
    let words = vm.words.expect("a Word body");
    assert_eq!(words.block_count, 4);
    assert!(!words.truncated);
    assert_eq!(
        words
            .blocks
            .iter()
            .map(|block| block.style)
            .collect::<Vec<_>>(),
        vec![
            WordBlockStyle::Title,
            WordBlockStyle::Heading1,
            WordBlockStyle::Paragraph,
            WordBlockStyle::ListItem,
        ]
    );
    assert_eq!(words.blocks[0].runs[0].text, "Quarterly Report");
    assert_eq!(words.blocks[2].runs[0].text, "Revenue rose.");
}

/// Bold and italic survive, and `w:val="0"` turns them OFF.
///
/// The second half is the interesting one: a style that sets bold and a run
/// that cancels it is ordinary Word output, and a reader that treats the
/// element's presence as truth renders that run bold. That is a wrong document,
/// silently.
#[test]
fn run_emphasis_is_read_including_its_off_switch() {
    let scratch = Scratch::new("emph");
    let path = scratch.file(
        "e.docx",
        &docx_with_body(
            r#"<w:p>
                <w:r><w:rPr><w:b/></w:rPr><w:t>bold </w:t></w:r>
                <w:r><w:rPr><w:i/></w:rPr><w:t>italic </w:t></w:r>
                <w:r><w:rPr><w:b w:val="0"/></w:rPr><w:t>plain</w:t></w:r>
             </w:p>"#,
        ),
    );

    let vm = open_document(&path).expect("open");
    let words = vm.words.expect("a Word body");
    let runs = &words.blocks[0].runs;

    assert_eq!(runs.len(), 3, "three runs, got {runs:?}");
    assert!(runs[0].bold && !runs[0].italic);
    assert!(runs[1].italic && !runs[1].bold);
    assert!(
        !runs[2].bold,
        "w:val=\"0\" must turn bold off, not merely declare it"
    );
}

/// Text inside a table is still text and still appears, in document order.
///
/// Table STRUCTURE is deliberately not rendered (see [`WordsVm`]), but silently
/// dropping the words inside one would give a reader a document that looks
/// complete and is not — the failure this test exists to make impossible.
#[test]
fn text_inside_a_table_is_not_dropped() {
    let scratch = Scratch::new("tbl");
    let path = scratch.file(
        "t.docx",
        &docx_with_body(
            r"<w:p><w:r><w:t>before</w:t></w:r></w:p>
              <w:tbl><w:tr><w:tc><w:p><w:r><w:t>in a cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
              <w:p><w:r><w:t>after</w:t></w:r></w:p>",
        ),
    );

    let vm = open_document(&path).expect("open");
    let words = vm.words.expect("a Word body");

    let text: Vec<String> = words
        .blocks
        .iter()
        .map(|block| {
            block
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
        })
        .collect();
    assert_eq!(text, vec!["before", "in a cell", "after"]);
}

/// Field codes and tracked deletions are machinery, not prose.
#[test]
fn field_codes_and_deleted_text_are_not_shown() {
    let scratch = Scratch::new("fld");
    let path = scratch.file(
        "f.docx",
        &docx_with_body(
            r"<w:p>
                <w:r><w:t>see </w:t></w:r>
                <w:r><w:instrText>HYPERLINK http://example.test</w:instrText></w:r>
                <w:r><w:delText>REMOVED</w:delText></w:r>
                <w:r><w:t>page 3</w:t></w:r>
              </w:p>",
        ),
    );

    let vm = open_document(&path).expect("open");
    let words = vm.words.expect("a Word body");
    let text: String = words.blocks[0]
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect();

    assert_eq!(text, "see page 3");
    assert!(!text.contains("REMOVED"), "deleted text must stay deleted");
    assert!(!text.contains("HYPERLINK"), "a field code is not prose");
}

/// A `.docx` whose main part is missing degrades with a NAMED reason.
#[test]
fn a_corrupt_word_document_degrades_with_a_named_reason() {
    let scratch = Scratch::new("badxml");
    // A container with the main part present but unparseable: sniffing succeeds
    // (so it IS claimed as a Word document) and parsing then fails, which is
    // the interesting path — a file that says it is a document and is not.
    let path = scratch.file(
        "broken.docx",
        &zip_of(&[("word/document.xml", b"<w:document><w:body><w:p>" as &[u8])]),
    );

    let vm = open_document(&path).expect("open, not throw");

    assert_eq!(vm.format, Some(DocumentFormat::Docx));
    let detail = vm.detail.expect("a sentence");
    assert!(
        detail.contains("could not read this word document"),
        "the reason must name the format, got {detail:?}"
    );
    assert!(vm.words.is_none(), "no half-parsed body may be shown");
}

/// A document with more paragraphs than the cap keeps a bounded prefix AND
/// reports the real total. A count that quietly meant "loaded so far" would
/// make a 9 000-paragraph document look like a 3 000-paragraph one (44.11).
#[test]
fn a_long_word_document_is_bounded_and_says_so() {
    let scratch = Scratch::new("longdocx");
    let paragraphs: Vec<(&str, &str)> = (0..MAX_BLOCKS + 500).map(|_| ("Normal", "x")).collect();
    let path = scratch.file("long.docx", &docx_of(&paragraphs));

    let vm = open_document(&path).expect("open");
    let words = vm.words.expect("a Word body");

    assert_eq!(words.blocks.len(), MAX_BLOCKS, "the projection is bounded");
    assert_eq!(
        words.block_count,
        u32::try_from(MAX_BLOCKS + 500).expect("a block count fits u32"),
        "the count is the document's, not the projection's"
    );
    assert!(words.truncated);
    assert!(vm.truncated, "the summary flag agrees with the body");
    assert!(vm.detail.is_some(), "and a sentence says so");
}

// ---------------------------------------------------------------------------
// The hostile cases
// ---------------------------------------------------------------------------

/// A decompression bomb whose header is HONEST is refused before a byte is
/// inflated, by the cheap guard.
///
/// The fixture is real: a few hundred bytes of ZIP holding a part that inflates
/// past [`OOXML_MAX_PART_BYTES`]. This is the first of the two bomb guards and
/// the one that costs nothing — but it believes the container, which is why the
/// next test exists.
#[test]
fn a_decompression_bomb_is_refused_before_it_is_inflated() {
    let scratch = Scratch::new("bomb");
    let container = bomb_container();

    assert!(
        container.len() < 200_000,
        "the fixture must actually be a bomb: {} bytes compressed",
        container.len()
    );

    let path = scratch.file("bomb.docx", &container);
    let vm = open_document(&path).expect("open, not throw");

    let detail = vm.detail.expect("a sentence");
    assert!(
        detail.contains("declares") && detail.contains(&format_file_size(OOXML_MAX_PART_BYTES)),
        "the refusal must name what it refused and the limit, got {detail:?}"
    );
    assert!(vm.words.is_none(), "nothing may be rendered from a bomb");
}

/// **The guard that actually holds: a bomb that LIES about its size.**
///
/// A container's declared uncompressed size is written by whoever built it, so
/// the cheap check above can be defeated by writing a small number. This
/// fixture does exactly that — the same bomb, with both the local header and
/// the central directory patched to claim 42 bytes — and proves the second
/// guard, the one that counts bytes as they are produced, stops it anyway.
///
/// Without this test the previous one would pass against a `read_part` that had
/// no real cap at all, which is precisely the false confidence a bomb test is
/// supposed to eliminate.
#[test]
fn a_bomb_that_lies_about_its_size_is_refused_while_inflating() {
    let scratch = Scratch::new("liar");
    let container = forge_declared_size(bomb_container(), 42);
    let path = scratch.file("liar.docx", &container);

    let vm = open_document(&path).expect("open, not throw");

    let detail = vm.detail.expect("a sentence");
    assert!(
        detail.contains("inflates past"),
        "the refusal must come from the guard that counts produced bytes, got {detail:?}"
    );
    assert!(vm.words.is_none());
}

/// A part that inflates past [`OOXML_MAX_PART_BYTES`], stored compressed.
fn bomb_container() -> Vec<u8> {
    let huge = vec![b'A'; (OOXML_MAX_PART_BYTES + 1_000) as usize];
    let mut part = Vec::from(&b"<w:document><w:body><w:p><w:r><w:t>"[..]);
    part.extend_from_slice(&huge);
    part.extend_from_slice(b"</w:t></w:r></w:p></w:body></w:document>");
    zip_of(&[("word/document.xml", part.as_slice())])
}

/// Rewrite every declared uncompressed size in a ZIP to `lie`.
///
/// Patches the field at offset 22 of each local file header (`PK\x03\x04`) and
/// offset 24 of each central directory header (`PK\x01\x02`). `zip` reads sizes
/// from the central directory, so the second is the one that fools
/// `ZipFile::size`; the first is patched too so the container stays internally
/// consistent and the test is not accidentally passing because of a mismatch.
fn forge_declared_size(mut container: Vec<u8>, lie: u32) -> Vec<u8> {
    let bytes = lie.to_le_bytes();
    for index in 0..container.len().saturating_sub(30) {
        let offset = if container[index..].starts_with(b"PK\x03\x04") {
            Some(index + 22)
        } else if container[index..].starts_with(b"PK\x01\x02") {
            Some(index + 24)
        } else {
            None
        };
        if let Some(offset) = offset {
            container[offset..offset + 4].copy_from_slice(&bytes);
        }
    }
    container
}

/// The third guard: many parts, each individually legal, that together exceed
/// what keeper will inflate for one document.
///
/// A bomb does not have to be one big part, and the per-part cap alone would
/// wave this through — every slide here is under it.
#[test]
fn parts_that_are_individually_legal_still_exhaust_the_document_budget() {
    let scratch = Scratch::new("budget");
    // Each slide inflates to just under the per-part cap, so the per-part guard
    // never fires and only the running total can stop this.
    let filler = "B".repeat((OOXML_MAX_PART_BYTES - 1_000) as usize);
    let slides: Vec<String> = (0..4)
        .map(|_| slide_xml(None, &[filler.as_str()]))
        .collect();
    let path = scratch.file("budget.pptx", &pptx_of(&slides));

    let vm = open_document(&path).expect("open, not throw");

    let detail = vm.detail.expect("a sentence");
    assert!(
        detail.contains("expands to more than"),
        "the document-wide budget must be what refused, got {detail:?}"
    );
    assert!(vm.slides.is_none());
}

/// **The property this module's DTD safety rests on.**
///
/// `quick-xml` does not expand unrecognised entities — it errors, and
/// [`super::text_of`] then keeps the raw characters. A billion-laughs `.docx`
/// therefore costs its own size and nothing more.
///
/// This test asserts the LIBRARY's behaviour, not keeper's, on purpose: if a
/// future `quick-xml` starts expanding entities, `document.rs` becomes unsafe
/// without a single line of it changing, and this is the only thing that would
/// notice.
#[test]
fn entity_expansion_is_refused() {
    let scratch = Scratch::new("laughs");
    let dtd = r#"<?xml version="1.0"?>
<!DOCTYPE w:document [
  <!ENTITY lol "lol">
  <!ENTITY lol1 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">
  <!ENTITY lol2 "&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;">
  <!ENTITY lol3 "&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;">
  <!ENTITY lol4 "&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;">
  <!ENTITY lol5 "&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;">
  <!ENTITY lol6 "&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;">
  <!ENTITY lol7 "&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;">
  <!ENTITY lol8 "&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;">
  <!ENTITY lol9 "&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;">
]>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>&lol9;</w:t></w:r></w:p></w:body></w:document>"#;
    let path = scratch.file(
        "laughs.docx",
        &zip_of(&[("word/document.xml", dtd.as_bytes())]),
    );

    let vm = open_document(&path).expect("open, not hang and not die");

    // Whatever it renders, it is small. A billion laughs is 10^9 bytes; the
    // whole projection here must stay within a paragraph's cap.
    let rendered: usize = vm
        .words
        .iter()
        .flat_map(|words| &words.blocks)
        .map(|block| block.runs.iter().map(|run| run.text.len()).sum::<usize>())
        .sum();
    assert!(
        rendered <= MAX_BLOCK_CHARS,
        "entities must not have been expanded: {rendered} bytes rendered"
    );
}

/// A container larger than the cap is refused BEFORE it is opened, and the
/// refusal points at Open With rather than pretending to have tried.
///
/// Tested at both sides of the boundary: a container one byte over is refused,
/// and the ordinary-sized one in every other test here is not.
#[test]
fn a_container_over_the_cap_is_refused_with_its_size() {
    let scratch = Scratch::new("toobig");
    // A valid container, then padded past the cap. The padding lands after the
    // central directory, which `zip` tolerates — so this genuinely exercises
    // the size gate rather than a parse failure.
    let mut container = docx_of(&[("Normal", "small")]);
    container.resize((OOXML_MAX_BYTES + 1) as usize, 0);
    let path = scratch.file("huge.docx", &container);

    let vm = open_document(&path).expect("open");

    assert_eq!(vm.format, Some(DocumentFormat::Docx), "it is still a docx");
    let detail = vm.detail.expect("a sentence");
    assert!(
        detail.contains("Open With"),
        "the refusal must offer the way forward, got {detail:?}"
    );
    assert!(
        detail.contains(&format_file_size(OOXML_MAX_BYTES)),
        "and name the limit, got {detail:?}"
    );
    assert!(vm.words.is_none());
}

/// The other side of the same boundary: a container at the cap is read.
#[test]
fn a_container_at_the_cap_is_read() {
    let scratch = Scratch::new("atcap");
    let mut container = docx_of(&[("Normal", "just inside")]);
    container.resize(OOXML_MAX_BYTES as usize, 0);
    let path = scratch.file("big.docx", &container);

    let vm = open_document(&path).expect("open");

    assert!(
        vm.words.is_some(),
        "at the cap is inside it: {:?}",
        vm.detail
    );
}

// ---------------------------------------------------------------------------
// PPTX
// ---------------------------------------------------------------------------

fn slide_xml(title: Option<&str>, lines: &[&str]) -> String {
    let mut shapes = String::new();
    if let Some(title) = title {
        shapes.push_str(&format!(
            r#"<p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>{title}</a:t></a:r></a:p></p:txBody></p:sp>"#
        ));
    }
    if !lines.is_empty() {
        let body: String = lines
            .iter()
            .map(|line| format!("<a:p><a:r><a:t>{line}</a:t></a:r></a:p>"))
            .collect();
        shapes.push_str(&format!(
            r#"<p:sp><p:nvSpPr><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr><p:txBody>{body}</p:txBody></p:sp>"#
        ));
    }
    format!(
        r#"<?xml version="1.0"?><p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree>{shapes}</p:spTree></p:cSld></p:sld>"#
    )
}

fn pptx_of(slides: &[String]) -> Vec<u8> {
    let mut parts: Vec<(String, Vec<u8>)> = vec![(
        "ppt/presentation.xml".to_owned(),
        br#"<?xml version="1.0"?><p:presentation/>"#.to_vec(),
    )];
    for (index, xml) in slides.iter().enumerate() {
        parts.push((
            format!("ppt/slides/slide{}.xml", index + 1),
            xml.clone().into_bytes(),
        ));
    }
    let borrowed: Vec<(&str, &[u8])> = parts
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    zip_of(&borrowed)
}

/// A presentation renders its slide count and each slide's title and text.
#[test]
fn a_presentation_renders_its_slides() {
    let scratch = Scratch::new("pptx");
    let path = scratch.file(
        "deck.pptx",
        &pptx_of(&[
            slide_xml(Some("Welcome"), &["First point", "Second point"]),
            slide_xml(Some("Numbers"), &["Up and to the right"]),
        ]),
    );

    let vm = open_document(&path).expect("open");

    assert_eq!(vm.format, Some(DocumentFormat::Pptx));
    let slides = vm.slides.expect("a slide body");
    assert_eq!(slides.slide_count, 2);
    assert_eq!(slides.slides[0].number, 1);
    assert_eq!(slides.slides[0].title.as_deref(), Some("Welcome"));
    assert_eq!(
        slides.slides[0].lines,
        vec!["First point".to_owned(), "Second point".to_owned()]
    );
    assert_eq!(slides.slides[1].title.as_deref(), Some("Numbers"));
}

/// Slides are ordered NUMERICALLY, not lexicographically.
///
/// A container lists `slide10.xml` before `slide2.xml`, so a deck of more than
/// nine slides renders shuffled unless the order is computed. This is the
/// defect the test exists for; it is invisible in a nine-slide fixture.
#[test]
fn slides_past_nine_stay_in_order() {
    let scratch = Scratch::new("order");
    let slides: Vec<String> = (1..=12)
        .map(|n| slide_xml(Some(&format!("Slide {n}")), &[]))
        .collect();
    let path = scratch.file("many.pptx", &pptx_of(&slides));

    let vm = open_document(&path).expect("open");
    let slides = vm.slides.expect("a slide body");

    assert_eq!(slides.slide_count, 12);
    let titles: Vec<&str> = slides
        .slides
        .iter()
        .filter_map(|slide| slide.title.as_deref())
        .collect();
    assert_eq!(titles[1], "Slide 2", "slide 2 must not sort after slide 10");
    assert_eq!(titles[9], "Slide 10");
    assert_eq!(titles[11], "Slide 12");
    assert_eq!(
        slides
            .slides
            .iter()
            .map(|slide| slide.number)
            .collect::<Vec<_>>(),
        (1..=12).collect::<Vec<u32>>()
    );
}

/// A deck with no slides is a named refusal rather than an empty outline that
/// looks like a deck of nothing.
#[test]
fn a_presentation_with_no_slides_is_a_named_refusal() {
    let scratch = Scratch::new("noslides");
    let path = scratch.file("empty.pptx", &pptx_of(&[]));

    let vm = open_document(&path).expect("open");

    assert_eq!(vm.format, Some(DocumentFormat::Pptx));
    let detail = vm.detail.expect("a sentence");
    assert!(detail.contains("no slides"), "got {detail:?}");
}

// ---------------------------------------------------------------------------
// XLSX
// ---------------------------------------------------------------------------

/// Build a workbook. `sheets` is `(tab name, rows of cell text)`.
///
/// Strings go through the shared-string table, exactly as Excel writes them, so
/// the fixture exercises the indirection rather than the inline-string path a
/// hand-written test would otherwise take.
fn xlsx_of(sheets: &[(&str, Vec<Vec<&str>>)]) -> Vec<u8> {
    let mut shared: Vec<String> = Vec::new();
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();

    let mut declarations = String::new();
    let mut relationships = String::new();
    for (index, (name, rows)) in sheets.iter().enumerate() {
        let id = index + 1;
        declarations.push_str(&format!(
            r#"<sheet name="{name}" sheetId="{id}" r:id="rId{id}"/>"#
        ));
        relationships.push_str(&format!(
            r#"<Relationship Id="rId{id}" Target="worksheets/sheet{id}.xml"/>"#
        ));

        let mut body = String::new();
        for (row_index, row) in rows.iter().enumerate() {
            let row_number = row_index + 1;
            let mut cells = String::new();
            for (column, text) in row.iter().enumerate() {
                let reference = format!("{}{row_number}", column_letters(column));
                let position = shared.iter().position(|s| s == *text).unwrap_or_else(|| {
                    shared.push((*text).to_owned());
                    shared.len() - 1
                });
                cells.push_str(&format!(
                    r#"<c r="{reference}" t="s"><v>{position}</v></c>"#
                ));
            }
            body.push_str(&format!(r#"<row r="{row_number}">{cells}</row>"#));
        }
        parts.push((
            format!("xl/worksheets/sheet{id}.xml"),
            format!(r#"<?xml version="1.0"?><worksheet><sheetData>{body}</sheetData></worksheet>"#)
                .into_bytes(),
        ));
    }

    let shared_xml: String = shared
        .iter()
        .map(|text| format!("<si><t>{text}</t></si>"))
        .collect();
    parts.push((
        "xl/sharedStrings.xml".to_owned(),
        format!(r#"<?xml version="1.0"?><sst>{shared_xml}</sst>"#).into_bytes(),
    ));
    parts.push((
        "xl/_rels/workbook.xml.rels".to_owned(),
        format!(r#"<?xml version="1.0"?><Relationships>{relationships}</Relationships>"#)
            .into_bytes(),
    ));
    parts.push((
        "xl/workbook.xml".to_owned(),
        format!(r#"<?xml version="1.0"?><workbook><sheets>{declarations}</sheets></workbook>"#)
            .into_bytes(),
    ));

    let borrowed: Vec<(&str, &[u8])> = parts
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    zip_of(&borrowed)
}

/// `0` -> `A`, `25` -> `Z`, `26` -> `AA`.
fn column_letters(mut index: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + u8::try_from(index % 26).expect("an index modulo 26 fits u8"));
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).expect("the generated filler is ASCII")
}

/// A workbook renders its sheet names and their cells — the story's "a sheet
/// name", asserted.
#[test]
fn a_workbook_renders_its_sheet_names_and_cells() {
    let scratch = Scratch::new("xlsx");
    let path = scratch.file(
        "budget.xlsx",
        &xlsx_of(&[
            (
                "Revenue",
                vec![vec!["Region", "Total"], vec!["Europe", "1200"]],
            ),
            ("Notes", vec![vec!["Checked by", "Ada"]]),
        ]),
    );

    let vm = open_document(&path).expect("open");

    assert_eq!(vm.format, Some(DocumentFormat::Xlsx));
    let sheets = vm.sheets.expect("a workbook body");
    assert_eq!(sheets.sheet_count, 2);
    assert_eq!(
        sheets
            .sheets
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Revenue", "Notes"]
    );
    assert_eq!(sheets.sheets[0].row_count, 2);
    assert_eq!(sheets.sheets[0].column_count, 2);
    assert_eq!(
        sheets.sheets[0].rows,
        vec![
            vec!["Region".to_owned(), "Total".to_owned()],
            vec!["Europe".to_owned(), "1200".to_owned()],
        ]
    );
    assert_eq!(sheets.sheets[1].rows[0][1], "Ada");
}

/// Excel omits empty cells entirely, so a sparse row must be padded from its
/// cell references. Without that every value after a gap shifts left and the
/// spreadsheet renders plausible, wrong numbers under the wrong headings.
#[test]
fn a_sparse_row_keeps_its_columns() {
    let scratch = Scratch::new("sparse");
    let sheet = r#"<?xml version="1.0"?><worksheet><sheetData>
        <row r="1"><c r="A1" t="inlineStr"><is><t>left</t></is></c><c r="D1" t="inlineStr"><is><t>far</t></is></c></row>
    </sheetData></worksheet>"#;
    let path = scratch.file(
        "sparse.xlsx",
        &zip_of(&[
            (
                "xl/workbook.xml",
                br#"<workbook><sheets><sheet name="S" sheetId="1"/></sheets></workbook>"#
                    as &[u8],
            ),
            ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
        ]),
    );

    let vm = open_document(&path).expect("open");
    let sheets = vm.sheets.expect("a workbook body");

    assert_eq!(
        sheets.sheets[0].rows[0],
        vec![
            "left".to_owned(),
            String::new(),
            String::new(),
            "far".to_owned()
        ],
        "D1 must land in the fourth column"
    );
}

/// A workbook with more rows than the cap keeps a bounded prefix AND reports
/// the sheet's real height.
///
/// This is the story's "a large document mounts a bounded number of pages,
/// asserted by counting", at the Rust boundary: nothing beyond the cap is even
/// sent to the webview.
#[test]
fn a_huge_sheet_is_bounded_and_reports_its_real_height() {
    let scratch = Scratch::new("hugesheet");
    let rows: Vec<Vec<&str>> = (0..MAX_ROWS_PER_SHEET + 2_000).map(|_| vec!["x"]).collect();
    let path = scratch.file("big.xlsx", &xlsx_of(&[("Data", rows)]));

    let vm = open_document(&path).expect("open");
    let sheets = vm.sheets.expect("a workbook body");
    let sheet = &sheets.sheets[0];

    assert_eq!(
        sheet.rows.len(),
        MAX_ROWS_PER_SHEET,
        "the projection is bounded"
    );
    assert_eq!(
        sheet.row_count,
        u32::try_from(MAX_ROWS_PER_SHEET + 2_000).expect("a row count fits u32"),
        "the count is the sheet's, not the projection's"
    );
    assert!(sheet.truncated, "and the sheet says it was cut");
    assert!(vm.truncated);
}

/// More sheets than the cap: bounded, counted honestly, and said out loud.
#[test]
fn a_workbook_with_too_many_sheets_is_bounded_and_says_so() {
    let scratch = Scratch::new("manysheets");
    let names: Vec<String> = (0..MAX_SHEETS + 4).map(|n| format!("S{n}")).collect();
    let sheets: Vec<(&str, Vec<Vec<&str>>)> = names
        .iter()
        .map(|name| (name.as_str(), vec![vec!["a"]]))
        .collect();
    let path = scratch.file("many.xlsx", &xlsx_of(&sheets));

    let vm = open_document(&path).expect("open");
    let body = vm.sheets.expect("a workbook body");

    assert_eq!(body.sheets.len(), MAX_SHEETS);
    assert_eq!(
        body.sheet_count,
        u32::try_from(MAX_SHEETS + 4).expect("a sheet count fits u32")
    );
    assert!(body.truncated);
    let detail = vm.detail.expect("a sentence");
    assert!(detail.contains("sheets"), "got {detail:?}");
}

/// The relationship, not the position, decides which part a tab shows.
///
/// A workbook that maps `rId1` to `sheet2.xml` is legal, and assuming position
/// would put the wrong data under the right name — wrong in the way that looks
/// right.
#[test]
fn a_sheet_is_found_through_its_relationship_not_its_position() {
    let scratch = Scratch::new("rels");
    let sheet_for = |text: &str| {
        format!(
            r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>{text}</t></is></c></row></sheetData></worksheet>"#
        )
    };
    let first = sheet_for("i am sheet7");
    let path = scratch.file(
        "rel.xlsx",
        &zip_of(&[
            (
                "xl/workbook.xml",
                br#"<workbook xmlns:r="x"><sheets><sheet name="Only" sheetId="1" r:id="rIdX"/></sheets></workbook>"# as &[u8],
            ),
            (
                "xl/_rels/workbook.xml.rels",
                br#"<Relationships><Relationship Id="rIdX" Target="worksheets/sheet7.xml"/></Relationships>"# as &[u8],
            ),
            ("xl/worksheets/sheet7.xml", first.as_bytes()),
        ]),
    );

    let vm = open_document(&path).expect("open");
    let sheets = vm.sheets.expect("a workbook body");

    assert_eq!(sheets.sheets[0].name, "Only");
    assert_eq!(sheets.sheets[0].rows[0][0], "i am sheet7");
}

/// A shared-string index past the end of the table renders empty rather than
/// panicking. The index comes out of the file, so it is attacker-controlled.
#[test]
fn an_out_of_range_shared_string_does_not_panic() {
    let scratch = Scratch::new("oob");
    let path = scratch.file(
        "oob.xlsx",
        &zip_of(&[
            (
                "xl/workbook.xml",
                br#"<workbook><sheets><sheet name="S" sheetId="1"/></sheets></workbook>"# as &[u8],
            ),
            ("xl/sharedStrings.xml", br#"<sst><si><t>only</t></si></sst>"# as &[u8]),
            (
                "xl/worksheets/sheet1.xml",
                br#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>9999</v></c></row></sheetData></worksheet>"# as &[u8],
            ),
        ]),
    );

    let vm = open_document(&path).expect("open, not panic");
    let sheets = vm.sheets.expect("a workbook body");

    assert_eq!(sheets.sheets[0].rows[0][0], "");
}

/// Column references are base-26 with no zero digit.
#[test]
fn column_references_are_parsed_as_bijective_base_26() {
    assert_eq!(column_index("A1"), Some(0));
    assert_eq!(column_index("Z9"), Some(25));
    assert_eq!(column_index("AA1"), Some(26), "AA is 27th, not 26th");
    assert_eq!(column_index("AB1"), Some(27));
    assert_eq!(column_index("BA1"), Some(52));
    assert_eq!(
        column_index("12"),
        None,
        "a reference must start with letters"
    );
}

// ---------------------------------------------------------------------------
// PDF
// ---------------------------------------------------------------------------

/// A minimal, structurally valid PDF with a classic page tree.
fn pdf_of(pages: u32) -> Vec<u8> {
    let mut kids = String::new();
    let mut objects = String::new();
    for page in 0..pages {
        let id = 3 + page;
        kids.push_str(&format!("{id} 0 R "));
        objects.push_str(&format!(
            "{id} 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n"
        ));
    }
    format!(
        "%PDF-1.4\n\
         1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
         2 0 obj\n<< /Type /Pages /Count {pages} /Kids [{kids}] >>\nendobj\n\
         {objects}\
         trailer\n<< /Root 1 0 R >>\n%%EOF\n"
    )
    .into_bytes()
}

/// A PDF reports its version and page count — the story's "a page count",
/// asserted.
#[test]
fn a_pdf_reports_its_version_and_page_count() {
    let scratch = Scratch::new("pdf");
    let path = scratch.file("paper.pdf", &pdf_of(7));

    let vm = open_document(&path).expect("open");

    assert_eq!(vm.format, Some(DocumentFormat::Pdf));
    let probe = vm.pdf.expect("a probe");
    assert_eq!(probe.version.as_deref(), Some("1.4"));
    assert_eq!(probe.page_count, Some(7));
    assert!(!probe.encrypted);
}

/// A 400-page PDF answers with 400 and costs one number, not 400 of anything.
///
/// The frontend's bound is asserted separately; this is the half that proves
/// the count is real rather than the projection's length.
#[test]
fn a_four_hundred_page_pdf_reports_four_hundred() {
    let scratch = Scratch::new("pdf400");
    let path = scratch.file("long.pdf", &pdf_of(400));

    let probe = open_document(&path).expect("open").pdf.expect("a probe");

    assert_eq!(probe.page_count, Some(400));
}

/// A page tree packed into a compressed object stream is still counted.
///
/// Most PDFs written this decade are PDF 1.5+ and put the page tree in an
/// `/ObjStm`. Without inflation the honest answer would be `None` for the
/// common case and a number for the rare one, which is the wrong way round.
///
/// **The payload is padded so that deflate actually compresses it.** The first
/// version of this test deflated a 37-byte dictionary, and flate2 emitted a
/// STORED block for it — compression does not pay on input that short — so the
/// page tree stayed legible as plain ASCII inside the "compressed" stream and
/// the raw scan found it without inflating anything. The test passed, and a
/// mutation that disabled object-stream inflation entirely passed with it. The
/// two assertions before the act are what make the fixture honest, and they
/// belong here rather than in a comment because a future flate2 could change
/// the threshold back.
#[test]
fn a_page_tree_inside_an_object_stream_is_counted() {
    use flate2::write::ZlibEncoder;

    let scratch = Scratch::new("objstm");
    // Redundant filler, so deflate emits a real Huffman-coded block and the
    // dictionary's bytes stop being their own ASCII.
    let mut inner = Vec::from(&b"<< /Type /Pages /Count 23 /Kids [] >>"[..]);
    inner.extend(std::iter::repeat_n(b' ', 4_096));
    let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&inner).expect("deflate");
    let compressed = encoder.finish().expect("finish");

    let mut pdf = Vec::from(
        &b"%PDF-1.5\n1 0 obj\n<< /Type /ObjStm /N 1 /Filter /FlateDecode >>\nstream\n"[..],
    );
    pdf.extend_from_slice(&compressed);
    pdf.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n");
    let path = scratch.file("modern.pdf", &pdf);

    // The fixture must be UNREADABLE without inflating, or the test below
    // passes on the raw scan and proves nothing about object streams. This is
    // the guard whose absence let a mutation that disabled inflation entirely
    // survive the sweep: the assertion at the bottom was true either way.
    assert!(
        find(&pdf, b"/Pages").is_none(),
        "the page tree must not be legible in the compressed bytes"
    );
    assert!(
        page_count_in(&pdf).is_none(),
        "the raw scan must find nothing here, or inflation is not what is being tested"
    );

    let probe = open_document(&path).expect("open").pdf.expect("a probe");

    assert_eq!(
        probe.page_count,
        Some(23),
        "the page tree was inside the object stream"
    );
}

/// `/Count` on an outline dictionary is not a page count.
///
/// A document with 3 pages and a 99-entry table of contents must report 3. The
/// naive "largest /Count in the file" reports 99, and it is exactly the shortcut
/// this probe was tempted by.
///
/// **The outline also NAMES `/Pages`**, and that half is what makes this test
/// bite. `/Pages` appears in real PDFs as an ordinary name — a destination, a
/// key in a name tree, an outline's `/Next` — not only as the value of a
/// `/Type`. Without a `/Pages` sitting inside an object that also has a
/// `/Count`, removing [`super::preceded_by_type`]'s anchor changes nothing and
/// the guard looks untested because it IS untested. With it, an unanchored
/// probe reports 99 pages for a three-page document.
#[test]
fn an_outline_count_is_not_mistaken_for_a_page_count() {
    let scratch = Scratch::new("outline");
    let mut pdf = pdf_of(3);
    pdf.extend_from_slice(b"90 0 obj\n<< /Type /Outlines /Count 99 /Next /Pages >>\nendobj\n");
    let path = scratch.file("toc.pdf", &pdf);

    let probe = open_document(&path).expect("open").pdf.expect("a probe");

    assert_eq!(
        probe.page_count,
        Some(3),
        "99 is the outline, not the pages"
    );
}

/// **A PDF the protocol will not serve is flagged, not silently un-drawable.**
///
/// `sniff` reads the CONTENT and `file_asset::is_servable_path` reads the NAME.
/// Both are right, and they disagree about a renamed download: this is a real
/// PDF, and `keeper-file://` will 404 it because the name says `.xlsx`. Without
/// `servable` the viewer would mount an `<embed>` at that URL, receive nothing,
/// and say nothing — a failed plugin render is invisible to JavaScript, so the
/// reader would get a blank pane over a perfectly good file.
#[test]
fn a_pdf_the_protocol_will_not_serve_says_so() {
    let scratch = Scratch::new("unservable");

    let named_pdf = scratch.file("real.pdf", &pdf_of(2));
    let probe = open_document(&named_pdf)
        .expect("open")
        .pdf
        .expect("a probe");
    assert!(probe.servable, "a file named .pdf is servable");

    // The same bytes, renamed. Still a PDF, still two pages, NOT servable.
    let renamed = scratch.file("quarterly.xlsx", &pdf_of(2));
    let probe = open_document(&renamed).expect("open").pdf.expect("a probe");
    assert_eq!(probe.page_count, Some(2), "it is still a readable PDF");
    assert!(
        !probe.servable,
        "the protocol reads the name, so these pages cannot be drawn"
    );
}

/// A PDF whose page tree this probe cannot read reports `None` — never a guess.
#[test]
fn an_unreadable_page_tree_reports_no_count_rather_than_guessing() {
    let scratch = Scratch::new("nocount");
    let path = scratch.file(
        "opaque.pdf",
        b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<< >>\n%%EOF\n",
    );

    let probe = open_document(&path).expect("open").pdf.expect("a probe");

    assert_eq!(probe.version.as_deref(), Some("1.7"));
    assert_eq!(
        probe.page_count, None,
        "no count is better than a wrong one"
    );
}

/// An encrypted PDF is flagged and explained, because a blank pane in the
/// webview is then expected rather than a bug.
#[test]
fn an_encrypted_pdf_says_so() {
    let scratch = Scratch::new("enc");
    let mut pdf = pdf_of(2);
    pdf.extend_from_slice(b"trailer\n<< /Encrypt 55 0 R >>\n%%EOF\n");
    let path = scratch.file("locked.pdf", &pdf);

    let vm = open_document(&path).expect("open");

    assert!(vm.pdf.expect("a probe").encrypted);
    let detail = vm.detail.expect("a sentence");
    assert!(detail.contains("encrypted"), "got {detail:?}");
}

/// A truncated PDF is still a PDF: the header is honest, the count is `None`,
/// and nothing throws. The webview will render what it can, which is the same
/// thing Preview does with the same file.
#[test]
fn a_corrupt_pdf_degrades_rather_than_throwing() {
    let scratch = Scratch::new("badpdf");
    let mut bytes = pdf_of(5);
    bytes.truncate(20);
    let path = scratch.file("cut.pdf", &bytes);

    let vm = open_document(&path).expect("open, not throw");

    assert_eq!(vm.format, Some(DocumentFormat::Pdf));
    assert_eq!(vm.pdf.expect("a probe").page_count, None);
}

/// Above the probe cap the pages still render and only the COUNT is dropped,
/// with a sentence saying which of the two happened.
#[test]
fn a_pdf_over_the_probe_cap_keeps_rendering_and_drops_only_the_count() {
    let scratch = Scratch::new("bigpdf");
    let mut bytes = pdf_of(3);
    bytes.resize((PDF_PROBE_MAX_BYTES + 1) as usize, b' ');
    let path = scratch.file("scan.pdf", &bytes);

    let vm = open_document(&path).expect("open");
    let probe = vm.pdf.expect("a probe");

    assert_eq!(probe.version.as_deref(), Some("1.4"), "the header is free");
    assert_eq!(probe.page_count, None);
    let detail = vm.detail.expect("a sentence");
    assert!(
        detail.contains("still render"),
        "it must say the pages are unaffected, got {detail:?}"
    );
}

/// The other side of that boundary.
#[test]
fn a_pdf_at_the_probe_cap_is_still_counted() {
    let scratch = Scratch::new("atpdfcap");
    let mut bytes = pdf_of(3);
    bytes.resize(PDF_PROBE_MAX_BYTES as usize, b' ');
    let path = scratch.file("edge.pdf", &bytes);

    let probe = open_document(&path).expect("open").pdf.expect("a probe");

    assert_eq!(probe.page_count, Some(3));
}
