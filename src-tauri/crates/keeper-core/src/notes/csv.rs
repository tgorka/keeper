//! CSV as **spans over the original bytes**, not a grid that gets
//! re-serialised (Story 44.16, FR-172).
//!
//! This is [`crate::notes::frontmatter`]'s promise applied to a second file
//! format, for the same reason and with the same mechanism. A CSV in a vault is
//! somebody's export: it was written by a spreadsheet, a bank, a database
//! dump — and it is under sync, so every byte keeper changes is a byte that
//! shows up in a diff and travels to the user's other machines. A parse-then-
//! reserialise loop through a `Vec<Vec<String>>` cannot keep that quiet. It
//! normalises quoting (`"a"` becomes `a`, or `a` becomes `"a"`), it normalises
//! line endings, it invents or removes the trailing newline, it drops the BOM
//! Excel put there and needs to read the file back, and it repairs a ragged row
//! by padding it. Every one of those is a change the user did not ask for, in a
//! file keeper did not author.
//!
//! So the parser records, for every field, the byte range it occupies in the
//! source, and a write is a splice over those original bytes. The delimiters,
//! the terminators, the BOM and everything outside the one edited field are
//! never re-emitted — they are copied. That makes the byte-identity promise
//! structural rather than a property the encoder has to get right:
//!
//! * **An untouched file is not written at all.** [`set_cell`] compares the new
//!   value against the parsed one and returns the source unchanged when they
//!   are equal, whatever the field's quoting was. Retyping a cell's own
//!   contents, or a widget that saves on blur, cannot reformat the file.
//! * **An edited cell moves its own bytes and no others.** The splice is over
//!   one field's span.
//!
//! **A row keeper cannot make sense of is shown, never dropped or repaired.**
//! A record with more or fewer fields than the first one is a real thing in a
//! real export, and a table that silently loses it is worse than a table that
//! shows it as odd — the same judgement frontmatter makes when it records a key
//! it cannot model rather than discarding it. Ragged rows are counted, not
//! fixed: keeper does not add a field to a row it did not write, so
//! [`set_cell`] on a column that row does not have refuses and says why. An
//! unterminated quote likewise parses to one enormous field, which is what the
//! bytes say, and the reader is told so instead of being left to wonder.
//!
//! **The vocabulary is RFC 4180 and nothing wider.** The delimiter is a comma;
//! `;`, tab and pipe are not sniffed, because a sniffer that guesses wrong
//! rewrites the wrong bytes and a `.tsv` is a different file with a different
//! extension. A record ends at `\n` or `\r\n` — the same definition
//! [`crate::notes::line_bounds`] already uses for every other file in this
//! crate — so a lone `\r` stays inside its field rather than becoming a second
//! opinion about what a line is. Anything the grammar does not cover (junk
//! after a closing quote, a quote in the middle of a bare field) is kept in the
//! value rather than rejected; the byte-preserving write means it survives
//! whether or not the parse understood it.

use std::borrow::Cow;

use crate::notes::vm::{NoteCsvRowVm, NoteCsvVm};
use crate::notes::{bom_len, line_number};

/// The field separator. See the module doc for why this is not configurable.
const DELIMITER: u8 = b',';

/// One field, and the bytes it occupies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvField {
    /// Byte range in the source, quotes included. This is what a write
    /// replaces, and nothing outside it is ever re-emitted.
    span: (usize, usize),
    /// The value with the surrounding quotes removed and `""` collapsed to `"`
    /// — what the table shows and what an edit is compared against.
    value: String,
    /// Whether the source wrote this field quoted. An edit keeps the field's
    /// own quoting rather than imposing the minimal form, because changing a
    /// cell's contents is not permission to change the file's conventions.
    quoted: bool,
}

impl CsvField {
    /// What the table cell shows.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Byte range in the source, quotes included.
    pub fn span(&self) -> (usize, usize) {
        self.span
    }

    /// Whether the source wrote this field quoted.
    pub fn quoted(&self) -> bool {
        self.quoted
    }
}

/// One record. A record is not a line: a quoted field may hold newlines, and
/// then one row spans several of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvRow {
    fields: Vec<CsvField>,
    /// Byte range of the record with its terminator **excluded**, so no write
    /// can move or invent a line ending.
    span: (usize, usize),
    /// 1-based line the record starts on, which is how a notice names it.
    line: usize,
}

impl CsvRow {
    pub fn fields(&self) -> &[CsvField] {
        &self.fields
    }

    /// Byte range of the record, terminator excluded.
    pub fn span(&self) -> (usize, usize) {
        self.span
    }

    /// 1-based line the record starts on.
    pub fn line(&self) -> usize {
        self.line
    }
}

/// A parsed CSV: every record, every field, and where each one's bytes are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Csv {
    rows: Vec<CsvRow>,
    width: usize,
    unterminated_quote: Option<usize>,
}

impl Csv {
    /// Record every field's span and value.
    ///
    /// Never fails. A CSV has no syntax error that justifies showing the user
    /// nothing — every byte belongs to some field, and the odd shapes are
    /// reported through [`Csv::ragged_rows`] and
    /// [`Csv::unterminated_quote`] rather than by refusing.
    pub fn parse(source: &str) -> Csv {
        let mut rows: Vec<CsvRow> = Vec::new();
        let mut unterminated_quote = None;
        // The BOM belongs to the file, not to the first field: including it in
        // field (0, 0) would make Excel's marker part of a cell's text and an
        // edit of that cell would eat it.
        let mut at = bom_len(source);
        while at < source.len() {
            let (row, open_quote, next) = parse_record(source, at);
            if open_quote && unterminated_quote.is_none() {
                unterminated_quote = Some(row.line);
            }
            rows.push(row);
            at = next;
        }
        let width = rows.first().map_or(0, |row| row.fields.len());
        Csv {
            rows,
            width,
            unterminated_quote,
        }
    }

    pub fn rows(&self) -> &[CsvRow] {
        &self.rows
    }

    /// How many fields the first record has — the number of columns the table
    /// draws, and the count a row is ragged against.
    ///
    /// The first record and not the widest, because the first record is the
    /// header the reader is looking at. A file whose header is the ragged one
    /// is a file whose every other row is reported odd, which is the honest
    /// reading: keeper has no way to know which record is the mistake.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Whether this record's field count differs from [`Csv::width`].
    pub fn is_ragged(&self, row: usize) -> bool {
        self.rows
            .get(row)
            .is_some_and(|record| record.fields.len() != self.width)
    }

    /// How many records do not have [`Csv::width`] fields.
    pub fn ragged_rows(&self) -> usize {
        (0..self.rows.len())
            .filter(|row| self.is_ragged(*row))
            .count()
    }

    /// The 1-based line of the first record holding a quote that never closed,
    /// if there is one. That record swallowed the rest of the file into its
    /// last field, which is what the bytes say and worth telling the reader.
    pub fn unterminated_quote(&self) -> Option<usize> {
        self.unterminated_quote
    }

    /// One cell's displayed value, or `None` when the table has no such cell.
    pub fn cell(&self, row: usize, column: usize) -> Option<&str> {
        Some(self.rows.get(row)?.fields.get(column)?.value())
    }
}

/// Everything a cell edit can refuse to do.
///
/// Both variants are finished sentences because they are shown to a person:
/// [`crate::notes::vm`]'s rule is that what the user reads is worded in Rust,
/// and "the CSV write failed" is not something anybody can act on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CsvError {
    #[error("this table has {rows} row(s), so there is no row {row} to edit")]
    NoSuchRow { row: usize, rows: usize },
    #[error(
        "the row at line {line} has {have} field(s), so it has no column {column}; \
         keeper shows a row like this as it is rather than adding fields to a row it did not write"
    )]
    NoSuchColumn {
        line: usize,
        column: usize,
        have: usize,
    },
}

/// Splice one cell's new value into the ORIGINAL source, preserving every other
/// byte. `row` and `column` are 0-based. Returns the whole new file.
///
/// Returns the source **unchanged** when `value` already equals what that cell
/// holds. That is the rule this module exists for: an edit that is not an edit
/// does not get to normalise the field's quoting, and a widget that saves on
/// every blur cannot rewrite a file the user only looked at.
pub fn set_cell(source: &str, row: usize, column: usize, value: &str) -> Result<String, CsvError> {
    let csv = Csv::parse(source);
    let record = csv.rows.get(row).ok_or(CsvError::NoSuchRow {
        row: row + 1,
        rows: csv.rows.len(),
    })?;
    let field = record
        .fields
        .get(column)
        .ok_or_else(|| CsvError::NoSuchColumn {
            line: record.line,
            column: column + 1,
            have: record.fields.len(),
        })?;

    if value == field.value {
        return Ok(source.to_owned());
    }

    let (from, to) = field.span;
    let encoded = encode(value, field.quoted);
    let mut out = String::with_capacity(source.len() - (to - from) + encoded.len());
    out.push_str(&source[..from]);
    out.push_str(&encoded);
    out.push_str(&source[to..]);
    Ok(out)
}

/// The bytes a new cell value is written as.
///
/// Quoted when the value forces it — a delimiter, a quote or a line ending
/// inside an unquoted field would end the field early and shift every column
/// after it — and also when the field was already quoted, so an edit inside
/// `"a","b","c"` does not leave one bare column behind.
fn encode(value: &str, keep_quoted: bool) -> Cow<'_, str> {
    let must_quote = value
        .bytes()
        .any(|byte| matches!(byte, DELIMITER | b'"' | b'\n' | b'\r'));
    if !must_quote && !keep_quoted {
        return Cow::Borrowed(value);
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    let mut rest = value;
    while let Some(quote) = rest.find('"') {
        out.push_str(&rest[..quote]);
        out.push_str("\"\"");
        rest = &rest[quote + 1..];
    }
    out.push_str(rest);
    out.push('"');
    Cow::Owned(out)
}

/// One record starting at `start`. Returns it, whether a quote was left open,
/// and the offset the next record starts at.
fn parse_record(source: &str, start: usize) -> (CsvRow, bool, usize) {
    let bytes = source.as_bytes();
    let mut fields = Vec::new();
    let mut open_quote = false;
    let mut at = start;
    loop {
        let (field, unterminated, next) = parse_field(source, at);
        open_quote |= unterminated;
        fields.push(field);
        at = next;
        if bytes.get(at) == Some(&DELIMITER) {
            at += 1;
            continue;
        }
        break;
    }

    // The terminator is outside the record's span. Nothing that writes a field
    // can reach it, which is how CRLF, LF and a missing final newline all
    // survive without the writer knowing which one it is looking at.
    let next = match bytes.get(at) {
        None => at,
        Some(b'\n') => at + 1,
        // `parse_field` stops before a `\r` only when a `\n` follows it, so the
        // only other terminator that can be here is a two-byte CRLF.
        Some(_) => at + 2,
    };

    (
        CsvRow {
            fields,
            span: (start, at),
            line: line_number(source, start),
        },
        open_quote,
        next,
    )
}

/// One field starting at `start`. Returns it, whether its opening quote never
/// closed, and the offset of the delimiter or terminator that ended it.
fn parse_field(source: &str, start: usize) -> (CsvField, bool, usize) {
    if source.as_bytes().get(start) != Some(&b'"') {
        let end = scan_bare(source, start);
        return (
            CsvField {
                span: (start, end),
                value: source[start..end].to_owned(),
                quoted: false,
            },
            false,
            end,
        );
    }

    let mut value = String::new();
    let mut at = start + 1;
    let mut unterminated = false;
    loop {
        let Some(offset) = source[at..].find('"') else {
            // No closing quote anywhere: the rest of the file is this field.
            // Reported rather than repaired — inserting the quote keeper thinks
            // is missing would be keeper deciding where the user's record ends.
            value.push_str(&source[at..]);
            at = source.len();
            unterminated = true;
            break;
        };
        let quote = at + offset;
        value.push_str(&source[at..quote]);
        if source.as_bytes().get(quote + 1) == Some(&b'"') {
            value.push('"');
            at = quote + 2;
            continue;
        }
        at = quote + 1;
        break;
    }

    // Bytes between the closing quote and the field's real end — `"a"x,b` — are
    // not RFC 4180 and are in somebody's file anyway. They join the value
    // instead of being dropped.
    let end = scan_bare(source, at);
    value.push_str(&source[at..end]);
    (
        CsvField {
            span: (start, end),
            value,
            quoted: true,
        },
        unterminated,
        end,
    )
}

/// Advance to the delimiter, the line terminator or the end of the source.
///
/// Byte-wise, which is safe and returns a char boundary because none of `,`,
/// `\n` or `\r` can occur inside a multi-byte UTF-8 sequence.
fn scan_bare(source: &str, from: usize) -> usize {
    let bytes = source.as_bytes();
    let mut at = from;
    while at < bytes.len() {
        match bytes[at] {
            DELIMITER | b'\n' => return at,
            b'\r' if bytes.get(at + 1) == Some(&b'\n') => return at,
            _ => at += 1,
        }
    }
    at
}

// ---------------------------------------------------------------------------
// What the table surface is given
// ---------------------------------------------------------------------------

/// How large a file keeper will open as a table.
///
/// A vault holds exports, and `keeper-sync` already names the 6 GB `.csv` as
/// the case its LFS threshold exists for. Parsing is linear and cheap, but the
/// cells cross IPC as JSON and land in the DOM, so the ceiling is on the file
/// and it is stated rather than discovered as a hang. Four mebibytes is roughly
/// fifty thousand ordinary rows.
pub const MAX_CSV_BYTES: u64 = 4 * 1024 * 1024;

/// How many records the table ships.
///
/// A cap that says it capped, not a cap that truncates quietly: the notice
/// carries the total, and the file is still whole on disk. Editing is unaffected
/// — [`set_cell`] indexes the file's records, not this window.
pub const MAX_TABLE_ROWS: usize = 500;

/// The sentence a file too large to open as a table gets.
///
/// Composed here, and not in the shell, because the `keeper` crate does not
/// build on Linux and a sentence nobody can run is a sentence nobody checked
/// (AD-55/AD-56).
pub fn too_large_notice(rel_path: &str, bytes: u64) -> String {
    format!(
        "{rel_path} is {} MB, and keeper opens a CSV as a table up to {} MB; \
         the file is untouched and opens in whatever you use for spreadsheets",
        bytes / (1024 * 1024),
        MAX_CSV_BYTES / (1024 * 1024),
    )
}

/// Project a parsed file into the table the note shows.
///
/// `rel_path` and `rev` come from the shell, which is the only side that knows
/// what it opened and what revision it read; everything else — including every
/// sentence the reader sees — is decided here so it can be tested on Linux.
pub fn project(source: &str, rel_path: String, rev: String) -> NoteCsvVm {
    let csv = Csv::parse(source);
    let total = csv.rows().len();

    let mut notices = Vec::new();
    let ragged = csv.ragged_rows();
    if ragged > 0 {
        notices.push(format!(
            "{ragged} of {total} rows do not have {} fields; \
             they are shown with the fields they have, and keeper adds none",
            csv.width(),
        ));
    }
    if let Some(line) = csv.unterminated_quote() {
        notices.push(format!(
            "a quote opens on line {line} and never closes, \
             so everything after it reads as one cell",
        ));
    }
    if total > MAX_TABLE_ROWS {
        notices.push(format!(
            "showing the first {MAX_TABLE_ROWS} of {total} rows; \
             the rest are in the file and untouched",
        ));
    }

    let rows = csv
        .rows()
        .iter()
        .take(MAX_TABLE_ROWS)
        .enumerate()
        .map(|(index, record)| NoteCsvRowVm {
            index: index as u32,
            line: record.line() as u32,
            cells: record
                .fields()
                .iter()
                .map(|field| field.value().to_owned())
                .collect(),
            ragged: record.fields().len() != csv.width(),
        })
        .collect();

    NoteCsvVm {
        rel_path,
        rev,
        columns: csv.width() as u32,
        total_rows: total as u32,
        rows,
        notices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A BOM, CRLF terminators, a quoted field holding both a comma and a CRLF
    /// of its own, a bare field and no final newline — every shape the story
    /// names, in one file, so a fix for one of them cannot quietly break
    /// another.
    const NASTY: &str = "\u{feff}id,note,tail\r\n1,\"Doe, Jane\r\nsecond line\",x\r\n2,plain,y";

    /// Every file the story requires to survive untouched, named so a failure
    /// says which shape broke.
    fn corpus() -> Vec<(&'static str, &'static str)> {
        vec![
            ("empty", ""),
            ("bom only", "\u{feff}"),
            ("plain", "a,b\nc,d\n"),
            ("no trailing newline", "a,b\nc,d"),
            ("crlf", "a,b\r\nc,d\r\n"),
            ("bom", "\u{feff}a,b\nc,d\n"),
            ("quoted commas", "name,note\n\"Doe, Jane\",ok\n"),
            ("embedded newline", "a,b\n\"line one\nline two\",z\n"),
            ("ragged", "a,b,c\n1,2\n3,4,5,6\n"),
            ("empty fields", ",,\n\"\",x,\n"),
            ("quoted everything", "\"a\",\"b\"\r\n\"c\",\"d\"\r\n"),
            ("blank line inside", "a,b\n\nc,d\n"),
            ("junk after a closing quote", "\"a\"x,b\nc,d\n"),
            ("unterminated quote", "a,b\n\"never closes,c\n"),
            ("utf8", "naïve,日本語\n\"café, noir\",ok\n"),
            ("nasty", NASTY),
        ]
    }

    /// The promise the whole module exists for, over every shape at once: a
    /// write that puts a cell's own value back produces the original bytes.
    ///
    /// Byte equality, never `contains`: the failures this guards against —
    /// a dropped BOM, LF for CRLF, an invented or removed final newline,
    /// normalised quoting — all leave a file that still *contains* every cell.
    #[test]
    fn writing_a_cell_its_own_value_back_reproduces_the_file_byte_for_byte() {
        for (name, source) in corpus() {
            let csv = Csv::parse(source);
            for (row, record) in csv.rows().iter().enumerate() {
                for (column, field) in record.fields().iter().enumerate() {
                    let written = set_cell(source, row, column, field.value())
                        .unwrap_or_else(|error| panic!("{name} ({row},{column}): {error}"));
                    assert_eq!(
                        written.as_bytes(),
                        source.as_bytes(),
                        "{name}: writing ({row},{column}) back changed the file"
                    );
                }
            }
        }
    }

    /// The parse itself, so the round-trip above cannot pass over a grid that
    /// says nothing true about the file.
    #[test]
    fn a_quoted_field_holds_its_commas_newlines_and_doubled_quotes() {
        let csv = Csv::parse("a,\"one, two\",\"line\nbreak\",\"say \"\"hi\"\"\"\n");
        assert_eq!(csv.rows().len(), 1);
        let values: Vec<&str> = csv.rows()[0].fields().iter().map(CsvField::value).collect();
        assert_eq!(values, vec!["a", "one, two", "line\nbreak", "say \"hi\""]);
        assert_eq!(csv.width(), 4);
    }

    /// A CRLF file's fields must not keep the carriage return: `line_bounds`
    /// strips it everywhere else in this crate, and a cell reading `d\r` would
    /// put an invisible byte into the table and then into the user's edit.
    #[test]
    fn a_crlf_record_ends_before_the_carriage_return() {
        let csv = Csv::parse("a,b\r\nc,d\r\n");
        assert_eq!(csv.cell(0, 1), Some("b"));
        assert_eq!(csv.cell(1, 1), Some("d"));
        assert_eq!(csv.rows()[0].span(), (0, 3));
    }

    /// A lone `\r` is not a terminator here. Deciding it was would give this
    /// module a second opinion about what a line is, and the rest of the crate
    /// already has the first one.
    #[test]
    fn a_lone_carriage_return_stays_inside_its_field() {
        let csv = Csv::parse("a\rb,c\n");
        assert_eq!(csv.rows().len(), 1);
        assert_eq!(csv.cell(0, 0), Some("a\rb"));
    }

    /// The BOM is the file's, not the first cell's.
    #[test]
    fn the_byte_order_mark_belongs_to_no_field() {
        let csv = Csv::parse("\u{feff}a,b\n");
        assert_eq!(csv.cell(0, 0), Some("a"));
        assert_eq!(csv.rows()[0].span(), (3, 6));
    }

    /// Shown, not dropped and not padded.
    #[test]
    fn a_ragged_row_is_kept_with_the_field_count_it_actually_has() {
        let csv = Csv::parse("a,b,c\n1,2\n3,4,5,6\n7,8,9\n");
        assert_eq!(csv.rows().len(), 4);
        assert_eq!(csv.width(), 3);
        assert_eq!(csv.rows()[1].fields().len(), 2);
        assert_eq!(csv.rows()[2].fields().len(), 4);
        assert_eq!(csv.ragged_rows(), 2);
        assert!(csv.is_ragged(1) && csv.is_ragged(2));
        assert!(!csv.is_ragged(0) && !csv.is_ragged(3));
    }

    /// A trailing terminator ends the file; it does not open a phantom record.
    /// A blank line in the middle is a record, because the bytes say so.
    #[test]
    fn a_final_newline_adds_no_row_but_a_blank_line_between_rows_is_one() {
        assert_eq!(Csv::parse("a,b\nc,d\n").rows().len(), 2);
        assert_eq!(Csv::parse("a,b\nc,d").rows().len(), 2);
        assert_eq!(Csv::parse("a,b\r\n").rows().len(), 1);

        let blank = Csv::parse("a,b\n\nc,d\n");
        assert_eq!(blank.rows().len(), 3);
        assert_eq!(blank.rows()[1].fields().len(), 1);
        assert!(blank.is_ragged(1));
    }

    /// An empty file is a table with nothing in it, not an error and not a row.
    #[test]
    fn an_empty_file_parses_to_no_rows_and_refuses_an_edit_by_name() {
        for source in ["", "\u{feff}"] {
            let csv = Csv::parse(source);
            assert!(csv.rows().is_empty());
            assert_eq!(csv.width(), 0);
            assert_eq!(csv.cell(0, 0), None);
            assert_eq!(
                set_cell(source, 0, 0, "x"),
                Err(CsvError::NoSuchRow { row: 1, rows: 0 })
            );
        }
    }

    #[test]
    fn an_unterminated_quote_is_reported_rather_than_closed() {
        let csv = Csv::parse("a,b\n\"never closes,c\n");
        assert_eq!(csv.unterminated_quote(), Some(2));
        assert_eq!(csv.rows().len(), 2);
        // The record has exactly one field: the open quote swallowed the
        // delimiter that would have started a second one, and the rest of the
        // file with it.
        assert_eq!(csv.rows()[1].fields().len(), 1);
        assert_eq!(csv.cell(1, 0), Some("never closes,c\n"));
        assert_eq!(Csv::parse("a,b\n").unterminated_quote(), None);
    }

    /// The other half of the story: the edit lands, and everything else is the
    /// bytes that were there before. Asserted as whole-file equality *and* as
    /// prefix/suffix identity, so a change anywhere outside the field fails.
    #[test]
    fn an_edited_cell_moves_its_own_bytes_and_no_others() {
        let csv = Csv::parse(NASTY);
        let (from, to) = csv.rows()[1].fields()[1].span();
        let written = set_cell(NASTY, 1, 1, "Roe, Richard").expect("row 1 column 1 exists");

        assert_eq!(
            written,
            "\u{feff}id,note,tail\r\n1,\"Roe, Richard\",x\r\n2,plain,y"
        );
        assert_eq!(&written.as_bytes()[..from], &NASTY.as_bytes()[..from]);
        assert_eq!(
            &written.as_bytes()[written.len() - (NASTY.len() - to)..],
            &NASTY.as_bytes()[to..]
        );
    }

    /// A cell's quoting is the file's convention, not something an edit gets to
    /// vote on — and a value that would break the record is quoted whatever the
    /// field was.
    #[test]
    fn an_edit_keeps_the_fields_quoting_and_adds_quotes_only_when_it_must() {
        // Bare stays bare.
        assert_eq!(
            set_cell("a,b\n", 0, 0, "z").expect("the edit applies"),
            "z,b\n"
        );
        // Quoted stays quoted, even for a value that needs nothing.
        assert_eq!(
            set_cell("\"a\",b\n", 0, 0, "z").expect("the edit applies"),
            "\"z\",b\n"
        );
        // A bare field whose new value carries a delimiter, a quote or a line
        // ending must be quoted or the columns after it shift.
        assert_eq!(
            set_cell("a,b\n", 0, 0, "x,y").expect("the edit applies"),
            "\"x,y\",b\n"
        );
        assert_eq!(
            set_cell("a,b\n", 0, 0, "x\ny").expect("the edit applies"),
            "\"x\ny\",b\n"
        );
        assert_eq!(
            set_cell("a,b\n", 0, 0, "x\"y").expect("the edit applies"),
            "\"x\"\"y\",b\n"
        );
        assert_eq!(
            set_cell("a,b\n", 0, 0, "x\r\ny").expect("the edit applies"),
            "\"x\r\ny\",b\n"
        );
        // Emptying a cell is an edit like any other.
        assert_eq!(
            set_cell("a,b\n", 0, 0, "").expect("the edit applies"),
            ",b\n"
        );
    }

    /// An edit at the very end of a file with no final newline must not grow
    /// one, and an edit at the very start must not displace the BOM.
    #[test]
    fn an_edit_at_either_end_leaves_the_files_edges_alone() {
        assert_eq!(
            set_cell("a,b\nc,d", 1, 1, "Z").expect("the edit applies"),
            "a,b\nc,Z"
        );
        assert_eq!(
            set_cell("\u{feff}a,b\n", 0, 0, "Z").expect("the edit applies"),
            "\u{feff}Z,b\n"
        );
    }

    /// Keeper does not add a field to a row it did not write, and it says so in
    /// a sentence naming the line the reader can go and look at.
    #[test]
    fn editing_a_column_a_ragged_row_does_not_have_is_refused_with_a_reason() {
        let source = "a,b,c\n1,2\n";
        assert_eq!(
            set_cell(source, 1, 2, "x"),
            Err(CsvError::NoSuchColumn {
                line: 2,
                column: 3,
                have: 2
            })
        );
        let message = set_cell(source, 1, 2, "x")
            .expect_err("the edit is refused")
            .to_string();
        assert!(
            message.contains("line 2") && message.contains("no column 3"),
            "the refusal must name the line and the column: {message}"
        );
        // The row's own columns are still editable.
        assert_eq!(
            set_cell(source, 1, 1, "x").expect("the edit applies"),
            "a,b,c\n1,x\n"
        );
    }

    /// A record's line number counts the newlines inside quoted fields above
    /// it, so a notice points at the line the user's editor shows.
    #[test]
    fn a_records_line_number_counts_the_newlines_inside_quoted_fields() {
        let csv = Csv::parse("a,b\n\"two\nlines\",z\nlast,row\n");
        assert_eq!(csv.rows()[0].line(), 1);
        assert_eq!(csv.rows()[1].line(), 2);
        assert_eq!(csv.rows()[2].line(), 4);
    }

    /// Junk between a closing quote and the field's end is not a shape this
    /// grammar has, and it is in real exports. It is kept, and the file still
    /// comes back untouched — which is the point of comparing values rather
    /// than re-encoding on every write.
    #[test]
    fn junk_after_a_closing_quote_is_kept_rather_than_dropped() {
        let source = "\"a\"x,b\n";
        let csv = Csv::parse(source);
        assert_eq!(csv.cell(0, 0), Some("ax"));
        assert_eq!(
            set_cell(source, 0, 0, "ax")
                .expect("the edit applies")
                .as_bytes(),
            source.as_bytes()
        );
        // A real edit normalises that one field, and only that one field.
        assert_eq!(
            set_cell(source, 0, 0, "z").expect("the edit applies"),
            "\"z\",b\n"
        );
    }

    #[test]
    fn a_projected_table_carries_every_row_with_the_cells_it_has() {
        let vm = project(
            "a,b,c\n1,2\n3,4,5\n",
            "attachments/data.csv".to_owned(),
            "rev1".to_owned(),
        );
        assert_eq!(vm.rel_path, "attachments/data.csv");
        assert_eq!(vm.rev, "rev1");
        assert_eq!(vm.columns, 3);
        assert_eq!(vm.total_rows, 3);
        assert_eq!(vm.rows.len(), 3);
        assert_eq!(vm.rows[1].cells, vec!["1".to_owned(), "2".to_owned()]);
        assert!(vm.rows[1].ragged);
        assert_eq!(vm.rows[1].index, 1);
        assert_eq!(vm.rows[1].line, 2);
        assert!(!vm.rows[2].ragged);
    }

    /// A clean file says nothing. A notice for a file with nothing wrong with it
    /// is noise the reader learns to ignore, and then misses the real one.
    #[test]
    fn a_clean_table_has_nothing_to_say() {
        let vm = project("a,b\n1,2\n", "d.csv".to_owned(), "r".to_owned());
        assert!(vm.notices.is_empty(), "notices: {:?}", vm.notices);
    }

    #[test]
    fn a_ragged_file_says_how_many_rows_are_odd_and_does_not_pad_them() {
        let vm = project("a,b,c\n1,2\n3,4,5,6\n", "d.csv".to_owned(), "r".to_owned());
        assert_eq!(vm.notices.len(), 1);
        assert!(
            vm.notices[0].contains("2 of 3 rows") && vm.notices[0].contains("3 fields"),
            "notice: {}",
            vm.notices[0]
        );
        // The cells are the file's, not a padded rectangle.
        assert_eq!(vm.rows[1].cells.len(), 2);
        assert_eq!(vm.rows[2].cells.len(), 4);
    }

    /// The cap is visible in the notice and the total, and the row indices stay
    /// the file's — so an edit in the last shown row still names record 499.
    #[test]
    fn a_long_file_is_capped_out_loud_and_keeps_the_files_row_numbers() {
        let mut source = String::from("a,b\n");
        for line in 0..MAX_TABLE_ROWS + 20 {
            source.push_str(&format!("{line},x\n"));
        }
        let vm = project(&source, "big.csv".to_owned(), "r".to_owned());
        assert_eq!(vm.total_rows as usize, MAX_TABLE_ROWS + 21);
        assert_eq!(vm.rows.len(), MAX_TABLE_ROWS);
        assert_eq!(
            vm.rows[MAX_TABLE_ROWS - 1].index as usize,
            MAX_TABLE_ROWS - 1
        );
        assert!(
            vm.notices
                .iter()
                .any(|notice| notice.contains("first 500 of 521 rows")),
            "notices: {:?}",
            vm.notices
        );
        // A row past the window is still editable by its file index.
        let written = set_cell(&source, MAX_TABLE_ROWS + 10, 1, "y").expect("row exists");
        assert_ne!(written, source);
    }

    #[test]
    fn an_unterminated_quote_reaches_the_reader_as_a_sentence_naming_the_line() {
        let vm = project("a,b\nc,\"open\n", "d.csv".to_owned(), "r".to_owned());
        assert!(
            vm.notices.iter().any(|notice| notice.contains("line 2")),
            "notices: {:?}",
            vm.notices
        );
    }

    #[test]
    fn a_file_too_large_to_table_says_its_size_and_the_ceiling() {
        let notice = too_large_notice("exports/huge.csv", 9 * 1024 * 1024);
        assert!(notice.contains("exports/huge.csv"), "{notice}");
        assert!(
            notice.contains("9 MB") && notice.contains("4 MB"),
            "{notice}"
        );
    }
}
