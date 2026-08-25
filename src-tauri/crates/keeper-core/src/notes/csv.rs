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
//! **The grammar is RFC 4180; the delimiter is detected.** A European Excel
//! export is semicolon-separated, and keeper drew the owner's file as one
//! column because this module used to insist on a comma. It used to argue that
//! a sniffer which guesses wrong rewrites the wrong bytes — true of a
//! parse-then-reserialise writer, and not true here. This writer splices one
//! field's bytes over the original and re-emits nothing else, so a wrong guess
//! draws a wrong TABLE and cannot corrupt a file: the delimiters it misread are
//! bytes it never touched, and a cell whose span is wrong still writes inside
//! the record the author wrote. The only thing detection has to get right is
//! agreeing with the author about where the fields are, which is a question the
//! file itself answers — see [`detect_delimiter`], which believes a candidate
//! only when it yields the SAME field count, greater than one, across the
//! opening records, and falls back to the comma when two candidates both do or
//! neither does. A record ends at `\n` or `\r\n` — the same definition
//! [`crate::notes::line_bounds`] already uses for every other file in this
//! crate — so a lone `\r` stays inside its field rather than becoming a second
//! opinion about what a line is. Anything the grammar does not cover (junk
//! after a closing quote, a quote in the middle of a bare field) is kept in the
//! value rather than rejected; the byte-preserving write means it survives
//! whether or not the parse understood it.

use std::borrow::Cow;

use crate::notes::vm::{NoteCsvRowVm, NoteCsvVm};
use crate::notes::{bom_len, line_number};

/// The separator assumed when the file does not say which one it is: an empty
/// file, a single column, or two candidates that both read the file cleanly.
/// RFC 4180's own answer, and the one a `.csv` means when nothing suggests
/// otherwise.
const DEFAULT_DELIMITER: u8 = b',';

/// The separators [`detect_delimiter`] will consider, in the order it prefers
/// them. Comma first so a tie resolves to it by the same walk that finds it.
///
/// Four and not more: each extra candidate is another way for an ordinary file
/// to be read as a grid it is not, and these are the four a spreadsheet
/// actually writes. A space is deliberately absent — prose is full of them.
const CANDIDATE_DELIMITERS: [u8; 4] = [b',', b';', b'\t', b'|'];

/// How many opening records a candidate is judged on.
///
/// Enough that a coincidence in the header alone cannot decide the file, few
/// enough that detection stays a constant cost on a file of any size — it runs
/// four times, once per candidate, before the real parse.
const DETECTION_RECORDS: usize = 10;

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

/// A parsed CSV: every record, every field, where each one's bytes are, and the
/// separator those fields were split on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Csv {
    rows: Vec<CsvRow>,
    width: usize,
    unterminated_quote: Option<usize>,
    delimiter: u8,
}

/// Hand-written rather than derived: a derived `Default` would put `0` in
/// `delimiter`, and a NUL is not a separator any parse of this type used. An
/// empty `Csv` is an empty comma-separated file, which is what
/// [`DEFAULT_DELIMITER`] means everywhere else here.
impl Default for Csv {
    fn default() -> Self {
        Csv {
            rows: Vec::new(),
            width: 0,
            unterminated_quote: None,
            delimiter: DEFAULT_DELIMITER,
        }
    }
}

impl Csv {
    /// Record every field's span and value, splitting on the separator the file
    /// itself indicates ([`detect_delimiter`]).
    ///
    /// Never fails. A CSV has no syntax error that justifies showing the user
    /// nothing — every byte belongs to some field, and the odd shapes are
    /// reported through [`Csv::ragged_rows`] and
    /// [`Csv::unterminated_quote`] rather than by refusing.
    pub fn parse(source: &str) -> Csv {
        Csv::parse_with(source, detect_delimiter(source))
    }

    /// [`Csv::parse`] against a separator the caller already knows — the
    /// conversion entry points, which are told which delimiter to speak, and
    /// detection itself, which tries all four.
    pub fn parse_with(source: &str, delimiter: u8) -> Csv {
        let mut rows: Vec<CsvRow> = Vec::new();
        let mut unterminated_quote = None;
        // The BOM belongs to the file, not to the first field: including it in
        // field (0, 0) would make Excel's marker part of a cell's text and an
        // edit of that cell would eat it.
        let mut at = bom_len(source);
        while at < source.len() {
            let (row, open_quote, next) = parse_record(source, at, delimiter);
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
            delimiter,
        }
    }

    /// The separator these fields were split on.
    pub fn delimiter(&self) -> u8 {
        self.delimiter
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
    let encoded = encode(value, field.quoted, csv.delimiter);
    let mut out = String::with_capacity(source.len() - (to - from) + encoded.len());
    out.push_str(&source[..from]);
    out.push_str(&encoded);
    out.push_str(&source[to..]);
    Ok(out)
}

/// The bytes a new cell value is written as, for a file separated by
/// `delimiter`.
///
/// Quoted when the value forces it — the file's own delimiter, a quote or a
/// line ending inside an unquoted field would end the field early and shift
/// every column after it — and also when the field was already quoted, so an
/// edit inside `"a","b","c"` does not leave one bare column behind.
///
/// `delimiter` and not a constant: writing a semicolon file's cell as if the
/// separator were a comma would leave `a;b` bare in a `;`-separated record and
/// split one cell into two the next time it is read.
fn encode(value: &str, keep_quoted: bool, delimiter: u8) -> Cow<'_, str> {
    let must_quote = value
        .bytes()
        .any(|byte| byte == delimiter || matches!(byte, b'"' | b'\n' | b'\r'));
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

/// One record starting at `start`, split on `delimiter`. Returns it, whether a
/// quote was left open, and the offset the next record starts at.
fn parse_record(source: &str, start: usize, delimiter: u8) -> (CsvRow, bool, usize) {
    let bytes = source.as_bytes();
    let mut fields = Vec::new();
    let mut open_quote = false;
    let mut at = start;
    loop {
        let (field, unterminated, next) = parse_field(source, at, delimiter);
        open_quote |= unterminated;
        fields.push(field);
        at = next;
        if bytes.get(at) == Some(&delimiter) {
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
fn parse_field(source: &str, start: usize, delimiter: u8) -> (CsvField, bool, usize) {
    if source.as_bytes().get(start) != Some(&b'"') {
        let end = scan_bare(source, start, delimiter);
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
    let end = scan_bare(source, at, delimiter);
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
/// Byte-wise, which is safe and returns a char boundary because every
/// candidate delimiter is ASCII and neither it, `\n` nor `\r` can occur inside
/// a multi-byte UTF-8 sequence. A non-ASCII `delimiter` would break that, which
/// is why [`CANDIDATE_DELIMITERS`] is the only source of one.
fn scan_bare(source: &str, from: usize, delimiter: u8) -> usize {
    let bytes = source.as_bytes();
    let mut at = from;
    while at < bytes.len() {
        match bytes[at] {
            byte if byte == delimiter => return at,
            b'\n' => return at,
            b'\r' if bytes.get(at + 1) == Some(&b'\n') => return at,
            _ => at += 1,
        }
    }
    at
}

// ---------------------------------------------------------------------------
// Which separator the file is using
// ---------------------------------------------------------------------------

/// The separator this file is written with, one of [`CANDIDATE_DELIMITERS`].
///
/// The owner's real attachment is semicolon-separated — which is what Excel
/// exports anywhere the decimal separator is a comma — and keeper drew it as a
/// single column, because the parser only ever split on `,`. That is the defect
/// this exists to close.
///
/// **The test is agreement, not frequency.** A count of candidate bytes picks
/// the comma out of `a;b;"1,5";c` and gets the file wrong. Instead each
/// candidate is used to actually parse the opening records, and one is believed
/// only when every record it produced has the SAME number of fields and that
/// number is greater than one. A separator that is really a separator lays the
/// file out in a rectangle; a character that merely occurs in the text does
/// not. That is also why the shape `id,"a; b",tail` cannot be misread as
/// semicolon-separated: splitting on `;` cuts through the quoted field and the
/// row below it has one field, so the counts disagree and `;` is not believed.
///
/// **Two believable candidates mean the comma.** `a,b;c` reads as a rectangle
/// either way and only the author knows which; RFC 4180 and the file extension
/// both say comma, so an ambiguous file is read the way it always was rather
/// than by a coin toss that changes with the row count.
///
/// Records with a single empty field — a blank line, and the trailing one a
/// great many exports end with — are passed over rather than counted. A line
/// with nothing on it contains no evidence about any candidate, and letting it
/// break the agreement test would send every file that ends in a blank line
/// back to the comma.
pub fn detect_delimiter(source: &str) -> u8 {
    let mut believed: Option<u8> = None;
    for candidate in CANDIDATE_DELIMITERS {
        // A width of one is what a candidate that is simply absent produces on
        // every line, so it is evidence of nothing rather than a rectangle.
        if !matches!(agreed_width(source, candidate), Some(width) if width > 1) {
            continue;
        }
        if believed.is_some() {
            return DEFAULT_DELIMITER;
        }
        believed = Some(candidate);
    }
    believed.unwrap_or(DEFAULT_DELIMITER)
}

/// The field count every one of the opening records has under `delimiter`, or
/// `None` when they disagree or there is nothing to judge.
fn agreed_width(source: &str, delimiter: u8) -> Option<usize> {
    let mut at = bom_len(source);
    let mut agreed: Option<usize> = None;
    let mut judged = 0usize;
    while at < source.len() && judged < DETECTION_RECORDS {
        let (record, _open_quote, next) = parse_record(source, at, delimiter);
        at = next;
        judged += 1;
        // A blank line: one field, holding nothing. Skipped, never compared.
        if record.fields.len() == 1 && record.fields[0].value.is_empty() {
            continue;
        }
        match agreed {
            None => agreed = Some(record.fields.len()),
            Some(width) if width == record.fields.len() => {}
            Some(_) => return None,
        }
    }
    agreed
}

// ---------------------------------------------------------------------------
// Bytes to a grid and back, for the table/markdown conversions
// ---------------------------------------------------------------------------

/// A CSV's cells as a plain grid, for the conversion that turns an attachment
/// into a markdown table.
///
/// Spans are dropped on purpose. A conversion has no cell to splice — it is
/// producing a different document in a different syntax — so it needs the
/// values and nothing else, and handing it a [`Csv`] would hand it the
/// machinery for a byte-preserving write it must not perform. Ragged records
/// keep the field count they have, exactly as the table shows them: a
/// conversion is not the moment to start padding somebody's export.
pub fn table_rows(source: &str, delimiter: u8) -> Vec<Vec<String>> {
    Csv::parse_with(source, delimiter)
        .rows
        .into_iter()
        .map(|record| record.fields.into_iter().map(|field| field.value).collect())
        .collect()
}

/// A grid written back out as RFC 4180 bytes separated by `delimiter`.
///
/// The one place in this module that composes a whole file rather than splicing
/// one field, and it is only reachable from a conversion — the user asked for a
/// markdown table to become an attachment, so these bytes have no original to
/// preserve. [`set_cell`] remains the only path an EXISTING file is written by,
/// and it still never re-serialises.
///
/// Quoting is minimal: a value is bare unless the delimiter, a quote or a line
/// ending inside it would end the field early. `\n` terminators including a
/// final one, because a text file's last line ends — and an empty grid is an
/// empty file rather than a lone newline.
pub fn csv_bytes(rows: &[Vec<String>], delimiter: u8) -> String {
    let separator = delimiter as char;
    let mut out = String::new();
    for row in rows {
        for (column, value) in row.iter().enumerate() {
            if column > 0 {
                out.push(separator);
            }
            out.push_str(&encode(value, false, delimiter));
        }
        out.push('\n');
    }
    out
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
        // The detected separator, so a table the user edits and a conversion
        // back to bytes speak the file's own dialect rather than re-deciding
        // it from cells that no longer carry it.
        delimiter: (csv.delimiter() as char).to_string(),
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

    // -----------------------------------------------------------------------
    // Which separator the file is using (item 7)
    // -----------------------------------------------------------------------

    /// The owner's file. A European Excel export is semicolon-separated, and
    /// keeper drew it as a single column of `id;name;amount` strings.
    #[test]
    fn a_semicolon_export_parses_into_its_columns_rather_than_one() {
        let source = "id;name;amount\n1;Kowalski;12,50\n2;Nowak;3,00\n";
        assert_eq!(detect_delimiter(source), b';');

        let csv = Csv::parse(source);
        assert_eq!(csv.width(), 3, "three columns, not one");
        assert_eq!(csv.rows().len(), 3);
        assert_eq!(csv.cell(1, 1), Some("Kowalski"));
        // The decimal comma is part of the value, which is the whole reason
        // this file is semicolon-separated in the first place.
        assert_eq!(csv.cell(1, 2), Some("12,50"));
        assert_eq!(csv.ragged_rows(), 0);
    }

    #[test]
    fn a_tab_and_a_pipe_file_are_read_the_same_way() {
        let tabbed = "id\tname\n1\tKowalski\n2\tNowak\n";
        assert_eq!(detect_delimiter(tabbed), b'\t');
        assert_eq!(Csv::parse(tabbed).cell(1, 1), Some("Kowalski"));
        assert_eq!(Csv::parse(tabbed).width(), 2);

        let piped = "id|name\n1|Kowalski\n2|Nowak\n";
        assert_eq!(detect_delimiter(piped), b'|');
        assert_eq!(Csv::parse(piped).cell(1, 1), Some("Kowalski"));
        assert_eq!(Csv::parse(piped).width(), 2);
    }

    /// The regression that matters most: every file that worked before still
    /// reads as comma-separated, including the awkward ones detection could
    /// plausibly get wrong (ragged, unterminated quote, BOM, CRLF, one row).
    #[test]
    fn every_comma_file_is_still_read_as_comma_separated() {
        for (name, source) in corpus() {
            assert_eq!(
                detect_delimiter(source),
                b',',
                "{name}: a comma file must not be detected as anything else"
            );
        }
        // And a trailing blank line — which a great many exports have — does
        // not send a semicolon file back to the comma by breaking the
        // agreement test with a one-field record.
        assert_eq!(detect_delimiter("a;b\nc;d\n\n"), b';');
        assert_eq!(detect_delimiter("a;b\n\nc;d\n"), b';');
    }

    /// A file with no separator in it at all is one column, not a failure and
    /// not a file the detector talks itself into splitting.
    #[test]
    fn a_single_column_file_stays_one_column() {
        let source = "heading\nalpha\nbeta\ngamma\n";
        assert_eq!(detect_delimiter(source), b',');
        let csv = Csv::parse(source);
        assert_eq!(csv.width(), 1);
        assert_eq!(csv.rows().len(), 4);
        assert_eq!(csv.cell(2, 0), Some("beta"));
        assert_eq!(csv.ragged_rows(), 0);
    }

    /// A semicolon INSIDE a quoted field of a comma-separated file. Splitting
    /// on `;` cuts straight through the quotes, so the rows it produces do not
    /// agree on a field count and the candidate is not believed.
    ///
    /// The second assertion is the harder one: with only the header to judge,
    /// both candidates lay the file out in a rectangle, and an ambiguous file
    /// must fall back to the comma rather than pick by frequency.
    #[test]
    fn a_semicolon_inside_a_quoted_comma_field_does_not_win() {
        let source = "id,\"note; with a semicolon\",tail\n1,x,y\n2,p,q\n";
        assert_eq!(detect_delimiter(source), b',');
        let csv = Csv::parse(source);
        assert_eq!(csv.width(), 3);
        assert_eq!(csv.cell(0, 1), Some("note; with a semicolon"));
        assert_eq!(csv.ragged_rows(), 0);

        assert_eq!(
            detect_delimiter("id,\"note; here\",tail\n"),
            b',',
            "one record reads as a rectangle under both, so the comma wins the tie"
        );
        assert_eq!(
            detect_delimiter("a,b;c\nd,e;f\n"),
            b',',
            "two candidates that both read the file cleanly mean the comma, \
             not a coin toss that changes with the row count"
        );
    }

    /// The byte-identity promise, on a file that is not comma-separated. The
    /// splice has to know the file's own delimiter or the spans are wrong.
    #[test]
    fn a_semicolon_file_is_written_back_byte_for_byte_and_spliced_in_place() {
        let source = "\u{feff}id;note;tail\r\n1;\"Doe; Jane\";x\r\n2;plain;y";
        let csv = Csv::parse(source);
        for (row, record) in csv.rows().iter().enumerate() {
            for (column, field) in record.fields().iter().enumerate() {
                assert_eq!(
                    set_cell(source, row, column, field.value())
                        .expect("the cell exists")
                        .as_bytes(),
                    source.as_bytes(),
                    "writing ({row},{column}) back changed the file"
                );
            }
        }
        assert_eq!(
            set_cell(source, 1, 1, "Roe; Richard").expect("the edit applies"),
            "\u{feff}id;note;tail\r\n1;\"Roe; Richard\";x\r\n2;plain;y"
        );
        // A value carrying the FILE's delimiter must be quoted even though it
        // holds no comma at all — bare, it would split one cell into two.
        assert_eq!(
            set_cell("a;b\n", 0, 0, "x;y").expect("the edit applies"),
            "\"x;y\";b\n"
        );
        assert_eq!(
            set_cell("a;b\n", 0, 0, "x,y").expect("the edit applies"),
            "x,y;b\n",
            "a comma is ordinary text in a semicolon file and must not be quoted"
        );
    }

    #[test]
    fn the_projected_table_tells_the_webview_which_separator_it_read() {
        assert_eq!(
            project("a;b\n1;2\n", "d.csv".to_owned(), "r".to_owned()).delimiter,
            ";"
        );
        assert_eq!(
            project("a,b\n1,2\n", "d.csv".to_owned(), "r".to_owned()).delimiter,
            ","
        );
        assert_eq!(
            project("a\tb\n1\t2\n", "d.csv".to_owned(), "r".to_owned()).delimiter,
            "\t"
        );
        assert_eq!(
            project("", "d.csv".to_owned(), "r".to_owned()).delimiter,
            ",",
            "an empty file is a comma-separated file with nothing in it"
        );
    }

    // -----------------------------------------------------------------------
    // Bytes to a grid and back (item 8)
    // -----------------------------------------------------------------------

    #[test]
    fn a_grid_survives_the_round_trip_through_bytes_and_back() {
        let rows = vec![
            vec!["id".to_owned(), "note".to_owned(), "tail".to_owned()],
            // Every shape that forces quoting, plus one that must not be
            // quoted, plus a ragged row a conversion must not pad.
            vec![
                "1".to_owned(),
                "Doe, Jane".to_owned(),
                "say \"hi\"".to_owned(),
            ],
            vec!["2".to_owned(), "line\nbreak".to_owned(), String::new()],
            vec!["3".to_owned(), "plain".to_owned()],
            vec![String::new()],
        ];
        for delimiter in CANDIDATE_DELIMITERS {
            let bytes = csv_bytes(&rows, delimiter);
            assert_eq!(
                table_rows(&bytes, delimiter),
                rows,
                "round trip through {:?} lost or changed a cell",
                delimiter as char
            );
        }

        // Minimal quoting: nothing is quoted that does not have to be.
        assert_eq!(
            csv_bytes(
                &[vec!["a".to_owned(), "b;c".to_owned(), "d,e".to_owned()]],
                b','
            ),
            "a,b;c,\"d,e\"\n"
        );
        assert_eq!(
            csv_bytes(
                &[vec!["a".to_owned(), "b;c".to_owned(), "d,e".to_owned()]],
                b';'
            ),
            "a;\"b;c\";d,e\n"
        );
        // An empty grid is an empty file, not a stray newline.
        assert_eq!(csv_bytes(&[], b','), "");
    }

    /// `table_rows` speaks the delimiter it is told, not the one it would have
    /// guessed — the conversion commands carry the file's own separator so a
    /// one-column `;` file with a comma in it cannot be re-split.
    #[test]
    fn table_rows_splits_on_the_delimiter_it_is_given() {
        let source = "a;b,c\n";
        assert_eq!(
            table_rows(source, b';'),
            vec![vec!["a".to_owned(), "b,c".to_owned()]]
        );
        assert_eq!(
            table_rows(source, b','),
            vec![vec!["a;b".to_owned(), "c".to_owned()]]
        );
        // A ragged export converts as it is. Padding it here would put a cell
        // into somebody's table that their file does not have.
        assert_eq!(
            table_rows("a,b,c\n1,2\n", b','),
            vec![
                vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
                vec!["1".to_owned(), "2".to_owned()],
            ]
        );
    }
}
