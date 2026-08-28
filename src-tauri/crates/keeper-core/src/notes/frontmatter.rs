//! First-party frontmatter: a **span-recording scanner**, not a document model
//! (AD-55).
//!
//! FR-121 promises that keeper does not mangle files it did not author, and the
//! operative form of that promise is byte-level: *a write that changes key `K`
//! leaves every byte outside `K`'s span identical*. Comment lines, key order,
//! quoting style, indentation, block-versus-flow list form, trailing inline
//! comments, CRLF line endings — all of it survives. No parse-then-reserialise
//! loop through a `Value` model can keep that promise, which is why this file
//! exists instead of a YAML dependency.
//!
//! So the parser records, for every top-level key, the byte range of its value
//! and the byte range of its whole lines. A write is a splice over the original
//! source; nothing outside the targeted key is ever re-emitted.
//!
//! The grammar is the Obsidian property subset and nothing more: a leading `---`
//! fence, `key: scalar`, block lists (`- item`), flow lists (`[a, b]`),
//! single/double-quoted strings, plain strings taken literally, and exactly one
//! level of nesting (which the reserved `keeper:` namespace needs). Anchors,
//! aliases, merge keys, tags (`!!str`) and block scalars (`|`, `>`) are **not**
//! understood — but a key carrying one is still *recorded*, with no value, so a
//! later write replaces the whole construct instead of appending a duplicate
//! key. The document is then flagged through [`Frontmatter::unparsed`].
//!
//! That last part is deliberate. Losing a user's key is worse than not
//! understanding it, and refusing to index a note because its metadata is odd
//! would violate the spirit of NFR-30: the note is the user's, the index is
//! ours.
//!
//! There is no implicit typing beyond a closed keyword set. `country: NO` is the
//! string `NO`, not `false` — the Norway problem is a real bug in somebody's
//! real metadata, and this parser refuses to reproduce it.

use std::fmt;
use std::fmt::Write as _;

use crate::notes::{bom_len, line_bounds, line_number};

/// A frontmatter value in the Obsidian property subset.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Str(String),
    Num(f64),
    Bool(bool),
    List(Vec<FieldValue>),
    Map(Vec<(String, FieldValue)>),
}

impl FieldValue {
    /// The form the index stores in `IndexEntry.fields`, so the `field:` query
    /// predicate has something flat to compare.
    ///
    /// Identical to [`Display`](fmt::Display) for scalars, but a list joins on
    /// `\n` rather than `, `. A frontmatter scalar is single-line by
    /// construction and so can never contain a newline, whereas it very much can
    /// contain a comma — `authors: ["Doe, Jane"]` must not read back as two
    /// authors.
    pub fn index_string(&self) -> String {
        match self {
            Self::Str(s) => s.clone(),
            Self::Num(n) => num_text(*n),
            Self::Bool(b) => bool_text(*b).to_owned(),
            Self::List(items) => items
                .iter()
                .map(Self::index_string)
                .collect::<Vec<_>>()
                .join("\n"),
            Self::Map(pairs) => pairs
                .iter()
                .map(|(k, v)| format!("{k}: {}", v.index_string()))
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

impl fmt::Display for FieldValue {
    /// The human-facing rendering, for a properties row or a list column. Use
    /// [`FieldValue::index_string`] when something is going to parse it back.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Str(s) => f.write_str(s),
            Self::Num(n) => f.write_str(&num_text(*n)),
            Self::Bool(b) => f.write_str(bool_text(*b)),
            Self::List(items) => {
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{item}")?;
                }
                Ok(())
            }
            Self::Map(pairs) => {
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                Ok(())
            }
        }
    }
}

/// What the parser met and could not model, and where.
///
/// Not an error: the note is still indexed, still searchable and still
/// editable. The properties panel shows the raw block with this complaint
/// attached, and the index carries the `unparsed` flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unparsed {
    pub reason: String,
    /// 1-based line number in the document.
    pub line: usize,
}

/// How a key's value is laid out in the source, which decides how a write to
/// that key splices back in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    /// `key: value` — the value text sits on the key's line.
    Inline,
    /// `key:` with nothing after it. The span covers the single space after the
    /// colon, if there is one, so a write can restore `key: value` exactly.
    Absent,
    /// A block list or a nested map: the value occupies its own lines.
    Block,
}

#[derive(Debug, Clone)]
struct Entry {
    key: String,
    /// `None` for a key whose value uses a construct outside the subset. The key
    /// is still recorded so it is neither lost nor duplicated by a later write.
    value: Option<FieldValue>,
    style: Style,
    /// Byte range of the value text within the source document.
    value_span: (usize, usize),
    /// Byte range of the entry's whole lines, terminator included.
    line_span: (usize, usize),
}

/// Parsed frontmatter that REMEMBERS its source bytes, so a write that touches
/// one key leaves every other byte identical (the FR-121 guarantee).
#[derive(Debug, Clone, Default)]
pub struct Frontmatter {
    /// `source[..body_offset]` — the whole `---` block, verbatim.
    block: Box<str>,
    /// Byte range between the fences, exclusive of both.
    inner: (usize, usize),
    entries: Vec<Entry>,
    unparsed: Option<Unparsed>,
}

impl Frontmatter {
    /// `(frontmatter, body_offset)`. No leading `---` block => empty
    /// frontmatter, offset 0.
    ///
    /// A `---` first line only opens frontmatter when a closing fence follows;
    /// an unterminated one is a thematic break and the whole file is body. A
    /// `---` that merely *appears* early — inside an opening code fence, say —
    /// is never a frontmatter fence, because the block must start at byte zero.
    pub fn parse(source: &str) -> (Frontmatter, usize) {
        let Some(fence) = Fence::find(source) else {
            return (Frontmatter::default(), 0);
        };
        let (entries, unparsed) = scan(source, fence.inner_start, fence.inner_end);
        let fm = Frontmatter {
            block: source[..fence.body_offset].into(),
            inner: (fence.inner_start, fence.inner_end),
            entries,
            unparsed,
        };
        (fm, fence.body_offset)
    }

    /// The value of `key`, or `None` when the key is absent *or* carries a
    /// construct outside the subset (see [`Frontmatter::unparsed`]).
    ///
    /// Duplicate keys are a YAML error; the first occurrence wins here, and
    /// writes target the same one.
    pub fn get(&self, key: &str) -> Option<&FieldValue> {
        self.entry(key).and_then(|e| e.value.as_ref())
    }

    /// Every top-level key, in source order. A key may appear here while
    /// [`Frontmatter::get`] returns `None` for it: the key exists, its value is
    /// something this parser does not model.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.key.as_str())
    }

    /// Splice a key into the ORIGINAL source, preserving every other byte.
    /// Returns the whole new document.
    ///
    /// With no frontmatter block present, one is created in front of the body —
    /// and the body keeps its first line, unshifted and unblanked.
    pub fn set_in(source: &str, key: &str, value: FieldValue) -> String {
        let (fm, _) = Self::parse(source);

        if fm.block.is_empty() {
            let bom = bom_len(source);
            let mut out = String::with_capacity(source.len() + 32);
            out.push_str(&source[..bom]);
            out.push_str(&Self::serialise_new(&[(key.to_owned(), value)]));
            out.push_str(&source[bom..]);
            return out;
        }

        let newline = newline_of(&fm.block);

        let Some(entry) = fm.entry(key) else {
            // A new key goes immediately before the closing fence, so the
            // existing key order — which Obsidian shows verbatim — is undisturbed.
            let rendered = render_entry(key, &value, true, newline);
            return splice(source, (fm.inner.1, fm.inner.1), &rendered);
        };

        if let Some(inline) = render_inline(&value) {
            match entry.style {
                Style::Inline => return splice(source, entry.value_span, &inline),
                // The span covers the space after the colon (or nothing at all),
                // so re-emitting it keeps `key: value` well-formed either way.
                Style::Absent => return splice(source, entry.value_span, &format!(" {inline}")),
                // Fall through: an existing block list stays a block list.
                Style::Block => {}
            }
        }

        let rendered = render_entry(key, &value, true, newline);
        splice(source, entry.line_span, &rendered)
    }

    /// Delete `key` and its lines. Unknown key, or no block at all, returns the
    /// source unchanged.
    pub fn remove_in(source: &str, key: &str) -> String {
        let (fm, body_offset) = Self::parse(source);
        let Some(entry) = fm.entry(key) else {
            return source.to_owned();
        };

        // If only whitespace would be left between the fences, take the block
        // with it: `---\n---\n` is noise Obsidian renders as an empty property
        // list. Comments count as content and keep the block alive.
        let head = &source[fm.inner.0..entry.line_span.0];
        let tail = &source[entry.line_span.1.min(fm.inner.1)..fm.inner.1];
        if head.trim().is_empty() && tail.trim().is_empty() {
            let bom = bom_len(source);
            return format!("{}{}", &source[..bom], &source[body_offset..]);
        }

        splice(source, entry.line_span, "")
    }

    /// Render a fresh block for a note keeper authors.
    pub fn serialise_new(pairs: &[(String, FieldValue)]) -> String {
        let mut out = String::from("---\n");
        for (key, value) in pairs {
            out.push_str(&render_entry(key, value, true, "\n"));
        }
        out.push_str("---\n");
        out
    }

    pub fn as_string(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            FieldValue::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            FieldValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// A list field as strings. A bare scalar reads as a one-element list —
    /// Obsidian accepts `tags: project` as well as `tags: [project]`, and so
    /// must anything reading a real vault.
    pub fn as_list(&self, key: &str) -> Option<Vec<String>> {
        match self.get(key)? {
            FieldValue::List(items) => Some(items.iter().map(FieldValue::index_string).collect()),
            FieldValue::Map(_) => None,
            FieldValue::Str(s) if s.trim().is_empty() => Some(Vec::new()),
            scalar => Some(vec![scalar.index_string()]),
        }
    }

    /// What the parser could not model, if anything.
    pub fn unparsed(&self) -> Option<&Unparsed> {
        self.unparsed.as_ref()
    }

    /// The `---` block exactly as it appears in the source, fences included.
    /// The properties panel shows this when the block did not fully parse.
    pub fn raw_block(&self) -> &str {
        &self.block
    }

    /// Whether the document had a frontmatter block at all. A block with no
    /// keys in it still counts.
    pub fn has_block(&self) -> bool {
        !self.block.is_empty()
    }

    fn entry(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.key == key)
    }
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

struct Fence {
    inner_start: usize,
    inner_end: usize,
    body_offset: usize,
}

impl Fence {
    fn find(source: &str) -> Option<Fence> {
        let start = bom_len(source);
        let (_, first_end, mut at) = line_bounds(source, start)?;
        if source[start..first_end].trim_end() != "---" {
            return None;
        }

        let inner_start = at;
        while let Some((ls, le, next)) = line_bounds(source, at) {
            let line = source[ls..le].trim_end();
            if line == "---" || line == "..." {
                return Some(Fence {
                    inner_start,
                    inner_end: ls,
                    body_offset: next,
                });
            }
            at = next;
        }
        // Unterminated: a thematic break, not frontmatter.
        None
    }
}

/// Which multi-line shape follows a `key:` with no inline value.
enum Shape {
    Absent,
    Seq(Vec<FieldValue>),
    Map(Vec<(String, FieldValue)>),
    /// Present, but outside the subset. Swallowed whole so a write replaces it.
    Opaque,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Unknown,
    Seq,
    Map,
}

fn scan(source: &str, inner_start: usize, inner_end: usize) -> (Vec<Entry>, Option<Unparsed>) {
    let mut entries: Vec<Entry> = Vec::new();
    let mut unparsed: Option<Unparsed> = None;
    let mut at = inner_start;

    while at < inner_end {
        let Some((ls, le, next)) = line_bounds(source, at) else {
            break;
        };
        let line = &source[ls..le];
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            at = next;
            continue;
        }
        if indent > 0 {
            complain(&mut unparsed, source, ls, "unexpected indented line");
            at = next;
            continue;
        }
        if let Some(reason) = rejected_construct(trimmed) {
            complain(&mut unparsed, source, ls, reason);
            at = next;
            continue;
        }
        let Some((key, colon)) = split_key(trimmed) else {
            complain(&mut unparsed, source, ls, "expected `key: value`");
            at = next;
            continue;
        };

        // The canonical single space after the colon belongs to the value's
        // span, so writing into an empty value produces `key: value` and not
        // `key:value`. Any further padding does not.
        let after_colon = ls + colon + 1;
        let space_end = if after_colon < le && source.as_bytes()[after_colon] == b' ' {
            after_colon + 1
        } else {
            after_colon
        };
        let mut text_start = space_end;
        while text_start < le && matches!(source.as_bytes()[text_start], b' ' | b'\t') {
            text_start += 1;
        }

        let region = &source[text_start..le];
        let token = region[..scalar_extent(region)].trim_end();

        if token.is_empty() {
            let (shape, end) = lookahead(source, next, inner_end);
            match shape {
                Shape::Absent => {
                    entries.push(Entry {
                        key: key.to_owned(),
                        value: Some(FieldValue::Str(String::new())),
                        style: Style::Absent,
                        value_span: (after_colon, space_end),
                        line_span: (ls, next),
                    });
                    at = next;
                }
                Shape::Seq(items) => {
                    entries.push(Entry {
                        key: key.to_owned(),
                        value: Some(FieldValue::List(items)),
                        style: Style::Block,
                        value_span: (next, end),
                        line_span: (ls, end),
                    });
                    at = end;
                }
                Shape::Map(pairs) => {
                    entries.push(Entry {
                        key: key.to_owned(),
                        value: Some(FieldValue::Map(pairs)),
                        style: Style::Block,
                        value_span: (next, end),
                        line_span: (ls, end),
                    });
                    at = end;
                }
                Shape::Opaque => {
                    complain(
                        &mut unparsed,
                        source,
                        ls,
                        "value is outside the property subset",
                    );
                    entries.push(Entry {
                        key: key.to_owned(),
                        value: None,
                        style: Style::Block,
                        value_span: (next, end),
                        line_span: (ls, end),
                    });
                    at = end;
                }
            }
            continue;
        }

        if token.starts_with('|') || token.starts_with('>') {
            // A block scalar's payload is the indented run beneath it. Swallow
            // it into the key's span so a later write replaces the whole thing
            // rather than leaving orphaned lines behind a new `key: value`.
            complain(
                &mut unparsed,
                source,
                ls,
                "block scalars (`|`, `>`) are outside the property subset",
            );
            let end = swallow_indented(source, next, inner_end);
            entries.push(Entry {
                key: key.to_owned(),
                value: None,
                style: Style::Block,
                value_span: (text_start, end),
                line_span: (ls, end),
            });
            at = end;
            continue;
        }

        let value = value_of(token);
        if value.is_none() {
            complain(
                &mut unparsed,
                source,
                ls,
                "value is outside the property subset",
            );
        }
        entries.push(Entry {
            key: key.to_owned(),
            value,
            style: Style::Inline,
            value_span: (text_start, text_start + token.len()),
            line_span: (ls, next),
        });
        at = next;
    }

    (entries, unparsed)
}

/// Consume the lines that make up a block list or a one-level nested map.
/// Returns the shape and the byte offset just past the last line it claimed.
fn lookahead(source: &str, from: usize, inner_end: usize) -> (Shape, usize) {
    let mut kind = Kind::Unknown;
    let mut opaque = false;
    let mut items: Vec<FieldValue> = Vec::new();
    let mut pairs: Vec<(String, FieldValue)> = Vec::new();
    let mut map_indent = 0usize;
    let mut end = from;
    let mut probe = from;

    while probe < inner_end {
        let Some((ls, le, next)) = line_bounds(source, probe) else {
            break;
        };
        let line = &source[ls..le];
        let trimmed = line.trim_start();

        // Blank and comment lines belong to whatever surrounds them, so they
        // never *extend* the claimed span — a trailing comment after the last
        // list item stays outside the key.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            probe = next;
            continue;
        }

        let indent = line.len() - trimmed.len();

        if trimmed == "-" || trimmed.starts_with("- ") {
            if kind == Kind::Map {
                break;
            }
            kind = Kind::Seq;
            match value_of(value_token(&trimmed[1..])) {
                Some(v) => items.push(v),
                None => opaque = true,
            }
        } else if indent > 0 {
            match kind {
                // An indented line under a sequence is a nested structure we do
                // not model; claim it so it cannot be orphaned.
                Kind::Seq => opaque = true,
                Kind::Unknown => {
                    kind = Kind::Map;
                    map_indent = indent;
                    push_pair(trimmed, &mut pairs, &mut opaque);
                }
                Kind::Map => {
                    if indent == map_indent {
                        push_pair(trimmed, &mut pairs, &mut opaque);
                    } else {
                        // A second nesting level. One is all the subset allows.
                        opaque = true;
                    }
                }
            }
        } else {
            // Unindented and not a list item: this is the next top-level key.
            break;
        }

        end = next;
        probe = next;
    }

    if opaque {
        return (Shape::Opaque, end);
    }
    match kind {
        Kind::Unknown => (Shape::Absent, from),
        Kind::Seq => (Shape::Seq(items), end),
        Kind::Map => (Shape::Map(pairs), end),
    }
}

fn push_pair(trimmed: &str, pairs: &mut Vec<(String, FieldValue)>, opaque: &mut bool) {
    let Some((key, colon)) = split_key(trimmed) else {
        *opaque = true;
        return;
    };
    match value_of(value_token(&trimmed[colon + 1..])) {
        Some(value) => pairs.push((key.to_owned(), value)),
        None => *opaque = true,
    }
}

/// Claim the indented run beneath a construct we could not model.
fn swallow_indented(source: &str, from: usize, inner_end: usize) -> usize {
    let mut end = from;
    let mut probe = from;
    while probe < inner_end {
        let Some((ls, le, next)) = line_bounds(source, probe) else {
            break;
        };
        let line = &source[ls..le];
        if line.trim().is_empty() {
            probe = next;
            continue;
        }
        if !line.starts_with(' ') && !line.starts_with('\t') {
            break;
        }
        end = next;
        probe = next;
    }
    end
}

/// YAML constructs the subset rejects outright, with the complaint to record.
fn rejected_construct(trimmed: &str) -> Option<&'static str> {
    if trimmed == "-" || trimmed.starts_with("- ") {
        return Some("a top-level sequence is not a property block");
    }
    if trimmed.starts_with("<<:") {
        return Some("merge keys are outside the property subset");
    }
    match trimmed.as_bytes().first().copied() {
        Some(b'&') => Some("anchors are outside the property subset"),
        Some(b'*') => Some("aliases are outside the property subset"),
        Some(b'!') => Some("type tags are outside the property subset"),
        Some(b'%') => Some("directives are outside the property subset"),
        Some(b'?') => Some("explicit keys are outside the property subset"),
        _ => None,
    }
}

/// Split `key: …`, returning the key and the byte offset of its colon.
///
/// A `:` only separates a key from a value when a space or the line end follows
/// it. `url:https://example.com` is a plain scalar in YAML, not a mapping, and
/// treating it as one would invent a key called `url` out of a body line.
fn split_key(trimmed: &str) -> Option<(&str, usize)> {
    let colon = trimmed.find(':')?;
    let bytes = trimmed.as_bytes();
    if colon + 1 < bytes.len() && !matches!(bytes[colon + 1], b' ' | b'\t') {
        return None;
    }
    let key = unquote_key(trimmed[..colon].trim_end());
    if key.is_empty() || key.contains('#') {
        return None;
    }
    Some((key, colon))
}

fn unquote_key(key: &str) -> &str {
    for quote in ['"', '\''] {
        if key.len() >= 2 && key.starts_with(quote) && key.ends_with(quote) {
            return &key[1..key.len() - 1];
        }
    }
    key
}

/// The value token in the text after a colon: padding and any trailing `#`
/// comment removed.
fn value_token(after_colon: &str) -> &str {
    let text = after_colon.trim_start();
    text[..scalar_extent(text)].trim_end()
}

/// Turn one value token into a [`FieldValue`], or `None` when it uses something
/// outside the subset.
fn value_of(token: &str) -> Option<FieldValue> {
    if token.is_empty() {
        return Some(FieldValue::Str(String::new()));
    }
    match token.as_bytes().first().copied() {
        Some(b'[') => parse_flow_list(token).map(FieldValue::List),
        Some(b'{' | b'|' | b'>' | b'&' | b'*' | b'!') => None,
        _ => Some(parse_scalar(token)),
    }
}

fn parse_scalar(text: &str) -> FieldValue {
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        return FieldValue::Str(unescape_double(&text[1..text.len() - 1]));
    }
    if text.len() >= 2 && text.starts_with('\'') && text.ends_with('\'') {
        return FieldValue::Str(text[1..text.len() - 1].replace("''", "'"));
    }
    match text {
        "true" | "True" | "TRUE" => FieldValue::Bool(true),
        "false" | "False" | "FALSE" => FieldValue::Bool(false),
        // `FieldValue` has no null, and widening it for a value nobody queries
        // would push an `Option` through every consumer. An explicit null reads
        // as the empty string, which is what an empty property is anyway.
        "null" | "Null" | "NULL" | "~" | "" => FieldValue::Str(String::new()),
        _ => match numeric(text) {
            Some(n) => FieldValue::Num(n),
            None => FieldValue::Str(text.to_owned()),
        },
    }
}

/// Numbers, and only numbers. The character allow-list keeps `inf`, `NaN` and
/// `1_000` out — `f64::from_str` accepts the first two, and a note that says
/// `budget: infinity` means the word.
fn numeric(text: &str) -> Option<f64> {
    let first = text.chars().next()?;
    if !first.is_ascii_digit() && first != '-' && first != '+' {
        return None;
    }
    if !text
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'))
    {
        return None;
    }
    let n: f64 = text.parse().ok()?;
    n.is_finite().then_some(n)
}

fn unescape_double(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            // An escape we do not know keeps both of its bytes rather than
            // silently losing the backslash.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn parse_flow_list(text: &str) -> Option<Vec<FieldValue>> {
    let inner = text.strip_prefix('[')?.strip_suffix(']')?;
    let mut items = Vec::new();
    for raw in split_flow(inner)? {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }
        items.push(parse_scalar(item));
    }
    Some(items)
}

/// Split a flow sequence's interior on top-level commas. `None` when it nests,
/// which the subset does not model.
fn split_flow(inner: &str) -> Option<Vec<&str>> {
    let bytes = inner.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut quote: Option<u8> = None;
    let mut i = 0usize;

    while i < bytes.len() {
        let c = bytes[i];
        match quote {
            Some(q) => {
                if c == b'\\' && q == b'"' {
                    i += 2;
                    continue;
                }
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                b'"' | b'\'' => quote = Some(c),
                b'[' | b'{' => return None,
                b',' => {
                    out.push(&inner[start..i]);
                    start = i + 1;
                }
                _ => {}
            },
        }
        i += 1;
    }
    out.push(&inner[start..]);
    Some(out)
}

/// Byte length of the value token at the start of `text`. Every stop byte is
/// ASCII, so the returned length is always a character boundary.
fn scalar_extent(text: &str) -> usize {
    match text.as_bytes().first().copied() {
        None => 0,
        Some(b'#') => 0,
        Some(b'"') => quoted_extent(text, b'"'),
        Some(b'\'') => quoted_extent(text, b'\''),
        Some(b'[') => bracket_extent(text, b'[', b']'),
        Some(b'{') => bracket_extent(text, b'{', b'}'),
        _ => plain_extent(text),
    }
}

/// A plain scalar ends at the first `#` that opens a comment — which, in YAML,
/// is a `#` preceded by whitespace. `status: open  # was blocked` therefore has
/// the value `open`, and the comment lives outside the key's span, which is
/// exactly what FR-121 needs.
fn plain_extent(text: &str) -> usize {
    let bytes = text.as_bytes();
    for (i, pair) in bytes.windows(2).enumerate() {
        if pair[1] == b'#' && matches!(pair[0], b' ' | b'\t') {
            return i + 1;
        }
    }
    bytes.len()
}

fn quoted_extent(text: &str, quote: u8) -> usize {
    let bytes = text.as_bytes();
    let mut i = 1usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && quote == b'"' {
            i += 2;
            continue;
        }
        if bytes[i] == quote {
            // `''` inside a single-quoted scalar is an escaped quote.
            if quote == b'\'' && i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    // Unterminated: take the rest of the line rather than dropping the value.
    bytes.len()
}

fn bracket_extent(text: &str, open: u8, close: u8) -> usize {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut i = 0usize;

    while i < bytes.len() {
        let c = bytes[i];
        match quote {
            Some(q) => {
                if c == b'\\' && q == b'"' {
                    i += 2;
                    continue;
                }
                if c == q {
                    quote = None;
                }
            }
            None => {
                if c == b'"' || c == b'\'' {
                    quote = Some(c);
                } else if c == open {
                    depth += 1;
                } else if c == close {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return i + 1;
                    }
                }
            }
        }
        i += 1;
    }
    bytes.len()
}

fn complain(slot: &mut Option<Unparsed>, source: &str, at: usize, reason: &str) {
    // First complaint wins: the properties panel shows one located problem, and
    // a cascade of follow-on noise would bury it.
    if slot.is_none() {
        *slot = Some(Unparsed {
            reason: reason.to_owned(),
            line: line_number(source, at),
        });
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn splice(source: &str, span: (usize, usize), text: &str) -> String {
    let mut out = String::with_capacity(source.len() + text.len());
    out.push_str(&source[..span.0]);
    out.push_str(text);
    out.push_str(&source[span.1..]);
    out
}

/// Match the document's own line ending. A vault edited on Windows is full of
/// CRLF, and emitting a lone `\n` into it would show up as a whole-file diff on
/// the next commit.
fn newline_of(block: &str) -> &'static str {
    if block.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// A whole `key: value` entry with its terminator. `prefer_block` keeps an
/// existing block list in block form; a non-empty map is always block, because
/// that is the shape Obsidian's property editor understands.
fn render_entry(key: &str, value: &FieldValue, prefer_block: bool, newline: &str) -> String {
    let key = render_key(key);
    match value {
        FieldValue::List(items) if prefer_block && !items.is_empty() => {
            let mut out = format!("{key}:{newline}");
            for item in items {
                let _ = write!(out, "  - {}{newline}", render_flow(item));
            }
            out
        }
        FieldValue::Map(pairs) if !pairs.is_empty() => {
            let mut out = format!("{key}:{newline}");
            for (k, v) in pairs {
                let _ = write!(out, "  {}: {}{newline}", render_key(k), render_flow(v));
            }
            out
        }
        _ => format!("{key}: {}{newline}", render_flow(value)),
    }
}

/// The single-line form of a value, or `None` when block form is wanted.
fn render_inline(value: &FieldValue) -> Option<String> {
    match value {
        FieldValue::Map(pairs) if !pairs.is_empty() => None,
        other => Some(render_flow(other)),
    }
}

fn render_flow(value: &FieldValue) -> String {
    match value {
        FieldValue::Str(s) => render_str(s),
        FieldValue::Num(n) => render_num(*n),
        FieldValue::Bool(b) => bool_text(*b).to_owned(),
        FieldValue::List(items) => {
            let inner: Vec<String> = items.iter().map(render_flow).collect();
            format!("[{}]", inner.join(", "))
        }
        FieldValue::Map(pairs) => {
            let inner: Vec<String> = pairs
                .iter()
                .map(|(k, v)| format!("{}: {}", render_key(k), render_flow(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

fn render_key(key: &str) -> String {
    if key.is_empty() || key.trim() != key || key.contains([':', '#', '\n', '\r']) {
        quote_double(key)
    } else {
        key.to_owned()
    }
}

fn render_str(s: &str) -> String {
    if needs_quotes(s) {
        quote_double(s)
    } else {
        s.to_owned()
    }
}

/// Quote whenever the bare form would read back as something else — a different
/// type, a truncated string, or a syntax error inside a flow list.
fn needs_quotes(s: &str) -> bool {
    if s.is_empty() || s.trim() != s {
        return true;
    }
    if s.contains(['\n', '\r', '\t', '"', ',', ']', '}']) {
        return true;
    }
    if s.contains(": ") || s.ends_with(':') || s.contains(" #") {
        return true;
    }
    if let Some(first) = s.chars().next() {
        if "-?&*!|>%@`'#[{".contains(first) {
            return true;
        }
    }
    match parse_scalar(s) {
        FieldValue::Str(v) => v != s,
        _ => true,
    }
}

fn quote_double(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn bool_text(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

/// A number as a human wrote it: `3`, not `3.0`.
fn num_text(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{n:.0}")
    } else {
        format!("{n}")
    }
}

fn render_num(n: f64) -> String {
    if n.is_finite() {
        num_text(n)
    } else {
        // Not a YAML number. Quoting keeps the document parseable instead of
        // emitting a bare `NaN` that reads back as a string anyway.
        format!("\"{n}\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A block shaped like one Obsidian actually writes: a comment, six keys
    /// keeper does not own, a quoted string, a trailing inline comment, a block
    /// list with mixed quoting, a flow list, and the nested `keeper:` namespace.
    const REAL_WORLD: &str = "\
---
# properties Obsidian wrote; do not reorder
title: \"Weekly review\"
status: open      # was: blocked
priority: 3
project: keeper
reviewer: mary
pinned: false
aliases:
  - Review
  - 'Weekly Review'
cssclasses: [wide, dense]
keeper:
  space: false
  capture: true
custom_field: some value
---

# Weekly review

Body text that must not move.
";

    #[test]
    fn parses_the_real_world_block() {
        let (fm, body) = Frontmatter::parse(REAL_WORLD);
        assert_eq!(
            &REAL_WORLD[body..],
            "\n# Weekly review\n\nBody text that must not move.\n"
        );
        assert_eq!(fm.unparsed(), None);

        assert_eq!(
            fm.keys().collect::<Vec<_>>(),
            vec![
                "title",
                "status",
                "priority",
                "project",
                "reviewer",
                "pinned",
                "aliases",
                "cssclasses",
                "keeper",
                "custom_field",
            ]
        );

        assert_eq!(fm.as_string("title"), Some("Weekly review"));
        // The trailing comment is not part of the value.
        assert_eq!(fm.as_string("status"), Some("open"));
        assert_eq!(fm.get("priority"), Some(&FieldValue::Num(3.0)));
        assert_eq!(fm.as_bool("pinned"), Some(false));
        assert_eq!(
            fm.as_list("aliases"),
            Some(vec!["Review".to_owned(), "Weekly Review".to_owned()])
        );
        assert_eq!(
            fm.as_list("cssclasses"),
            Some(vec!["wide".to_owned(), "dense".to_owned()])
        );
        assert_eq!(
            fm.get("keeper"),
            Some(&FieldValue::Map(vec![
                ("space".to_owned(), FieldValue::Bool(false)),
                ("capture".to_owned(), FieldValue::Bool(true)),
            ]))
        );
        assert_eq!(fm.as_string("custom_field"), Some("some value"));
    }

    #[test]
    fn writing_one_key_leaves_every_other_byte_identical() {
        let out = Frontmatter::set_in(REAL_WORLD, "pinned", FieldValue::Bool(true));
        assert_eq!(out, REAL_WORLD.replace("pinned: false", "pinned: true"));
    }

    #[test]
    fn writing_an_absent_key_disturbs_nothing_but_adds_a_line() {
        let out = Frontmatter::set_in(REAL_WORLD, "id", FieldValue::Str("01J8ZQ".to_owned()));
        assert_eq!(
            out,
            REAL_WORLD.replace(
                "custom_field: some value\n---",
                "custom_field: some value\nid: 01J8ZQ\n---"
            )
        );
    }

    #[test]
    fn writing_a_key_with_a_trailing_comment_keeps_the_comment() {
        let out = Frontmatter::set_in(REAL_WORLD, "status", FieldValue::Str("done".to_owned()));
        assert!(out.contains("status: done      # was: blocked"), "{out}");
        assert_eq!(out, REAL_WORLD.replace("status: open ", "status: done "));
    }

    #[test]
    fn writing_a_block_list_keeps_it_a_block_list() {
        let value = FieldValue::List(vec![
            FieldValue::Str("Review".to_owned()),
            FieldValue::Str("Retro".to_owned()),
        ]);
        let out = Frontmatter::set_in(REAL_WORLD, "aliases", value);
        assert_eq!(
            out,
            REAL_WORLD.replace(
                "aliases:\n  - Review\n  - 'Weekly Review'\n",
                "aliases:\n  - Review\n  - Retro\n"
            )
        );
    }

    #[test]
    fn writing_a_nested_map_keeps_the_namespace_nested() {
        let value = FieldValue::Map(vec![
            ("space".to_owned(), FieldValue::Bool(true)),
            ("capture".to_owned(), FieldValue::Bool(false)),
        ]);
        let out = Frontmatter::set_in(REAL_WORLD, "keeper", value);
        assert_eq!(
            out,
            REAL_WORLD.replace(
                "keeper:\n  space: false\n  capture: true\n",
                "keeper:\n  space: true\n  capture: false\n"
            )
        );
    }

    #[test]
    fn round_trips_a_crlf_document_with_crlf() {
        let source = REAL_WORLD.replace('\n', "\r\n");
        let out = Frontmatter::set_in(&source, "id", FieldValue::Str("01J8ZQ".to_owned()));
        assert!(out.contains("\r\nid: 01J8ZQ\r\n---\r\n"), "{out}");
        assert!(!out.contains("id: 01J8ZQ\n---\r\n"));
    }

    #[test]
    fn a_document_with_no_frontmatter_gains_a_block_and_keeps_its_first_line() {
        let source = "# Just a heading\n\nand a body.\n";
        let out = Frontmatter::set_in(source, "pinned", FieldValue::Bool(true));
        assert_eq!(
            out,
            "---\npinned: true\n---\n# Just a heading\n\nand a body.\n"
        );
        // The body is byte-identical and starts exactly where the block ends.
        let (_, body) = Frontmatter::parse(&out);
        assert_eq!(&out[body..], source);
    }

    #[test]
    fn a_dashed_line_inside_an_opening_code_fence_is_not_frontmatter() {
        let source = "```\n---\nfoo: bar\n---\n```\n\nreal body\n";
        let (fm, body) = Frontmatter::parse(source);
        assert!(!fm.has_block());
        assert_eq!(body, 0);
        assert_eq!(fm.get("foo"), None);

        let out = Frontmatter::set_in(source, "pinned", FieldValue::Bool(true));
        assert_eq!(out, format!("---\npinned: true\n---\n{source}"));
        // The fenced block survives untouched.
        assert!(out.ends_with(source));
    }

    #[test]
    fn an_unterminated_opening_fence_is_a_thematic_break_not_frontmatter() {
        let source = "---\nnot really frontmatter\n";
        let (fm, body) = Frontmatter::parse(source);
        assert!(!fm.has_block());
        assert_eq!(body, 0);
    }

    #[test]
    fn an_empty_block_parses_to_nothing_but_still_counts_as_a_block() {
        let (fm, body) = Frontmatter::parse("---\n---\nbody\n");
        assert!(fm.has_block());
        assert_eq!(body, 8);
        assert_eq!(fm.keys().count(), 0);
    }

    #[test]
    fn an_unmodelled_value_keeps_its_key_and_is_reported() {
        let source = "---\ntitle: ok\nnotes: |\n  line one\n  line two\nafter: yes\n---\nbody\n";
        let (fm, _) = Frontmatter::parse(source);

        // The key survives even though its value does not.
        assert_eq!(
            fm.keys().collect::<Vec<_>>(),
            vec!["title", "notes", "after"]
        );
        assert_eq!(fm.get("notes"), None);
        assert_eq!(fm.as_string("title"), Some("ok"));
        // `yes` is a string, not a bool. The Norway problem stays fixed.
        assert_eq!(fm.as_string("after"), Some("yes"));

        let complaint = fm.unparsed().map(|u| u.line);
        assert_eq!(complaint, Some(3));

        // And a write replaces the whole construct rather than duplicating the key.
        let out = Frontmatter::set_in(source, "notes", FieldValue::Str("short".to_owned()));
        assert_eq!(out, "---\ntitle: ok\nnotes: short\nafter: yes\n---\nbody\n");
        assert_eq!(out.matches("notes:").count(), 1);
    }

    #[test]
    fn an_anchor_is_reported_and_its_line_is_preserved() {
        let source = "---\n&anchor\ntitle: ok\n---\nbody\n";
        let (fm, _) = Frontmatter::parse(source);
        assert_eq!(fm.as_string("title"), Some("ok"));
        assert!(fm
            .unparsed()
            .is_some_and(|u| u.reason.contains("anchors") && u.line == 2));

        // Untouched keys keep every byte, the odd line included.
        let out = Frontmatter::set_in(source, "title", FieldValue::Str("new".to_owned()));
        assert_eq!(out, "---\n&anchor\ntitle: new\n---\nbody\n");
    }

    #[test]
    fn a_url_value_is_not_mistaken_for_a_nested_key() {
        let (fm, _) = Frontmatter::parse("---\nsource: https://example.com/x\n---\n");
        assert_eq!(fm.as_string("source"), Some("https://example.com/x"));
    }

    #[test]
    fn an_iso_date_stays_a_string_and_a_version_number_does_not_lie() {
        let (fm, _) =
            Frontmatter::parse("---\ncreated: 2026-08-02T09:00:00+02:00\nn: 1_000\n---\n");
        assert_eq!(fm.as_string("created"), Some("2026-08-02T09:00:00+02:00"));
        assert_eq!(fm.as_string("n"), Some("1_000"));
    }

    #[test]
    fn a_bare_scalar_reads_as_a_one_element_list() {
        let (fm, _) = Frontmatter::parse("---\ntags: project\nempty:\n---\n");
        assert_eq!(fm.as_list("tags"), Some(vec!["project".to_owned()]));
        assert_eq!(fm.as_list("empty"), Some(Vec::new()));
    }

    #[test]
    fn writing_into_an_empty_value_produces_a_well_formed_line() {
        assert_eq!(
            Frontmatter::set_in("---\npinned:\n---\n", "pinned", FieldValue::Bool(true)),
            "---\npinned: true\n---\n"
        );
        assert_eq!(
            Frontmatter::set_in(
                "---\npinned:  # unset\n---\n",
                "pinned",
                FieldValue::Bool(true)
            ),
            "---\npinned: true # unset\n---\n"
        );
    }

    #[test]
    fn removing_a_key_takes_its_lines_and_nothing_else() {
        let out = Frontmatter::remove_in(REAL_WORLD, "aliases");
        assert_eq!(
            out,
            REAL_WORLD.replace("aliases:\n  - Review\n  - 'Weekly Review'\n", "")
        );
        assert_eq!(Frontmatter::remove_in(REAL_WORLD, "nope"), REAL_WORLD);
    }

    #[test]
    fn removing_the_last_key_removes_the_empty_block() {
        assert_eq!(
            Frontmatter::remove_in("---\npinned: true\n---\nbody\n", "pinned"),
            "body\n"
        );
        // …but a comment is content, and content keeps the block.
        assert_eq!(
            Frontmatter::remove_in("---\n# why\npinned: true\n---\nbody\n", "pinned"),
            "---\n# why\n---\nbody\n"
        );
    }

    #[test]
    fn serialise_new_writes_the_shape_obsidian_expects() {
        let out = Frontmatter::serialise_new(&[
            ("id".to_owned(), FieldValue::Str("01J8ZQ".to_owned())),
            (
                "tags".to_owned(),
                FieldValue::List(vec![
                    FieldValue::Str("project/keeper".to_owned()),
                    FieldValue::Str("review".to_owned()),
                ]),
            ),
            ("pinned".to_owned(), FieldValue::Bool(false)),
        ]);
        assert_eq!(
            out,
            "---\nid: 01J8ZQ\ntags:\n  - project/keeper\n  - review\npinned: false\n---\n"
        );
        // And it reads back as what went in.
        let (fm, _) = Frontmatter::parse(&out);
        assert_eq!(fm.as_string("id"), Some("01J8ZQ"));
        assert_eq!(fm.as_bool("pinned"), Some(false));
    }

    #[test]
    fn ambiguous_strings_are_quoted_so_they_read_back_as_strings() {
        let cases = [
            ("true", "\"true\""),
            ("3", "\"3\""),
            ("null", "\"null\""),
            ("", "\"\""),
            ("- leading dash", "\"- leading dash\""),
            ("Doe, Jane", "\"Doe, Jane\""),
            ("Notes: a story", "\"Notes: a story\""),
            ("plain words", "plain words"),
            ("2026-08-02", "2026-08-02"),
        ];
        for (input, expected) in cases {
            let out =
                Frontmatter::serialise_new(&[("k".to_owned(), FieldValue::Str(input.to_owned()))]);
            assert_eq!(out, format!("---\nk: {expected}\n---\n"), "input {input:?}");

            let (fm, _) = Frontmatter::parse(&out);
            assert_eq!(fm.as_string("k"), Some(input), "input {input:?}");
        }
    }

    #[test]
    fn field_values_stringify_differently_for_humans_and_for_the_index() {
        let list = FieldValue::List(vec![
            FieldValue::Str("Doe, Jane".to_owned()),
            FieldValue::Num(3.0),
        ]);
        assert_eq!(list.to_string(), "Doe, Jane, 3");
        assert_eq!(list.index_string(), "Doe, Jane\n3");
        assert_eq!(FieldValue::Num(2.5).index_string(), "2.5");
        assert_eq!(FieldValue::Bool(true).index_string(), "true");
    }

    #[test]
    fn a_byte_order_mark_does_not_hide_the_block() {
        let source = "\u{feff}---\npinned: true\n---\nbody\n";
        let (fm, body) = Frontmatter::parse(source);
        assert_eq!(fm.as_bool("pinned"), Some(true));
        assert_eq!(&source[body..], "body\n");
    }
}

#[cfg(test)]
mod authored_block_tests {
    use super::*;

    /// The exact block keeper writes for a note it authors, observed on the
    /// agent-desktop run of 2026-08-03. The timestamps matter: an unquoted
    /// RFC 3339 value carries four colons and a `+`, and a scanner that splits a
    /// line on every colon rather than the first would lose `id` — which the
    /// index keys on, so the note becomes unopenable by the very id its own
    /// frontmatter carries.
    #[test]
    fn a_keeper_authored_block_round_trips_its_own_keys() {
        let source = "---\n\
                      id: 01KZ2MG27SXP9C6MGX40KT\n\
                      created: 2026-08-03T01:41:00.281700134+00:00\n\
                      updated: 2026-08-03T01:41:00.281711405+00:00\n\
                      ---\n\
                      \n\
                      Vault as a lens\n";
        let (fm, body) = Frontmatter::parse(source);

        assert_eq!(fm.as_string("id"), Some("01KZ2MG27SXP9C6MGX40KT"));
        assert_eq!(
            fm.as_string("created"),
            Some("2026-08-03T01:41:00.281700134+00:00"),
            "an RFC 3339 value keeps every colon after the first"
        );
        assert_eq!(&source[body..], "\nVault as a lens\n");
    }
}
