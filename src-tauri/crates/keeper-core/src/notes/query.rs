//! The space query language (FR-105).
//!
//! A space is an ordinary markdown note whose frontmatter carries one line of
//! query text, which gives the language two hard constraints: an agent writes it
//! as often as a human does, and a person who has used Gmail or GitHub search
//! must be able to guess it. That is why the grammar is juxtaposition-for-AND,
//! `|` for OR, `-` for NOT and `key:value` for everything else, and why it is a
//! hand-written recursive descent over a `Vec<Token>` with no generator, no
//! grammar file and no dependency: small enough to audit is a feature of a
//! language that lives in a synced file an agent edits.
//!
//! Three rules here are contract rather than implementation detail.
//!
//! **A broken query matches nothing.** Never everything. A space is a surface
//! people run bulk actions from, and "your filter silently stopped filtering" is
//! the failure that turns a typo into data loss. Every parse failure is a located
//! [`QueryError`] the space row renders as a warning chip and the editor
//! underlines.
//!
//! **Index-only predicates run before `text:`.** Inside one conjunction the terms
//! are reordered at *parse* time so everything answerable from the
//! [`IndexEntry`] alone runs before any term needing a body read. `text:` is the
//! only predicate that touches a file, and by the time it runs the candidate set
//! is usually two orders of magnitude smaller — which is what keeps NFR-28's
//! 100 ms list paint true for a space containing a text term.
//!
//! **The parser is bounded.** Nesting is capped at [`MAX_DEPTH`] and token count
//! at [`MAX_TOKENS`]. A hand-written query never approaches either; an
//! agent-written one cannot use them to make parsing expensive.
//!
//! Presentation — `sort`, `lens`, `limit` — is deliberately *not* in this
//! grammar. It is a separate frontmatter key, because the moment a boolean
//! expression grows an `order by` it grows a parser, and this one has to stay
//! hand-auditable.

use std::collections::BTreeSet;
use std::fmt;

use globset::GlobBuilder;

use crate::notes::index::{
    is_tag_descendant, link_key, tag_covers, IndexEntry, IndexSnapshot, NoteTagTerm, FIELD_DEVICE,
    FIELD_LIST_SEPARATOR, FIELD_ORIGIN, FIELD_TOUCHED,
};
use crate::notes::search;
use crate::notes::vm::{NoteSpaceTagVm, NoteSpaceTermsVm};

/// Maximum nesting of parenthesised groups.
pub const MAX_DEPTH: usize = 8;

/// Maximum number of tokens in one query.
pub const MAX_TOKENS: usize = 256;

/// The closed `is:` flag set. Closed on purpose: an unknown flag is a located
/// parse error rather than a predicate that is silently false forever, because a
/// space whose filter quietly stopped filtering is worse than one that refuses.
const IS_FLAGS: [&str; 11] = [
    "pinned",
    "archived",
    "unread",
    "conflict",
    "journal",
    "template",
    "space",
    "capture",
    "recording",
    "orphan",
    "untagged",
];

/// The one `is:` flag derived from the entry rather than read out of `flags`.
const IS_UNTAGGED: &str = "untagged";

// Every closed set the grammar accepts, spelled once so an error message can
// never drift from what the parser will actually take. A located error that
// lists the wrong alternatives is worse than one that lists none: it sends the
// author to try something that also fails.

/// The predicate keys, for the unknown-key error.
const KEYS: &str = "tag, path, field, date, origin, is, text, link or backlink";

/// The three `date:` fields, for the unknown-field error.
const DATE_FIELDS: &str = "created, modified or touched";

/// The `datespec` forms, for the malformed-date error.
const DATE_SPECS: &str = "YYYY-MM-DD, YYYY-MM-DDTHH:MM, today, yesterday or -<n>[dwmy]";

/// The `origin:` values, for the unknown-value error.
const ORIGINS: &str = "local, agent, remote or device:<label>";

const DAY_MS: i64 = 86_400_000;
const HOUR_MS: i64 = 3_600_000;
const MINUTE_MS: i64 = 60_000;

/// Largest `n` accepted in a `-<n><unit>` offset. Not a usability limit — nobody
/// filters on the last hundred thousand years — but an arithmetic one: it keeps
/// every date computation far inside `i64` without a checked add on the hot path.
const MAX_RELATIVE: i64 = 100_000;

/// A parsed, ready-to-evaluate space query.
///
/// Opaque by design: the AST shape is free to change, the DSL text is the
/// contract. `needs_body` is computed at parse time so a caller can decide
/// whether it will ever open a file *before* iterating ten thousand entries.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    root: Node,
    needs_body: bool,
}

/// A parse failure, located precisely enough to underline.
///
/// `token_index` indexes the token stream (the editor highlights that token) and
/// equals the token count when the failure is at end-of-input. `byte_span` is a
/// **byte** range into the original query string, so a caller rendering it over a
/// text input must index by bytes or convert first.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryError {
    /// Human-readable explanation, already phrased for display.
    pub message: String,
    /// Index of the offending token.
    pub token_index: usize,
    /// Byte range of the offending token within the query string.
    pub byte_span: (usize, usize),
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// One node of the boolean tree.
#[derive(Debug, Clone, PartialEq)]
enum Node {
    /// Any child matches.
    Or(Vec<Node>),
    /// Every child matches. Children are ordered index-only-first at parse time.
    And(Vec<Node>),
    /// The child does not match.
    Not(Box<Node>),
    /// A leaf predicate.
    Pred(Pred),
}

/// A leaf predicate, fully resolved at parse time so evaluation is pure walking.
#[derive(Debug, Clone, PartialEq)]
enum Pred {
    /// Segment-prefix tag match; `strict` is the `/*` descendants-only form.
    Tag { path: String, strict: bool },
    /// Vault-relative glob over the note's path. Boxed because a compiled
    /// `GlobMatcher` carries a whole regex program and would otherwise set the
    /// size of every node in the tree.
    Path(Box<PathGlob>),
    /// Frontmatter field: presence when `op` is `None`, otherwise a typed compare.
    Field {
        key: String,
        op: Option<CmpOp>,
        value: String,
    },
    /// A timestamp comparison against the index's resolved dates.
    Date {
        field: DateField,
        op: CmpOp,
        spec: DateSpec,
    },
    /// Provenance of the last commit touching the note.
    Origin(OriginSpec),
    /// One of the closed [`IS_FLAGS`].
    Is(String),
    /// Folded substring over title + body. The only predicate that reads a file.
    Text(String),
    /// This note links to the target.
    Link {
        /// The folded literal target, used while the query is unbound.
        key: String,
        /// Every key the resolved target answers to, filled by [`bind_index`].
        bound: Option<BTreeSet<String>>,
    },
    /// The target links to this note.
    Backlink {
        /// The folded literal target; kept so a bound query can be rebound.
        key: String,
        /// The ids the target links to, filled by [`bind_index`]. `None` — an
        /// unbound query — matches nothing, because inbound links are a
        /// whole-index fact a single [`IndexEntry`] cannot know.
        bound: Option<BTreeSet<String>>,
    },
}

/// A compiled path glob plus the pattern it came from.
///
/// Equality is on the pattern: `globset::GlobMatcher` is not `PartialEq`, and two
/// matchers compiled from the same text are the same predicate by definition.
#[derive(Debug, Clone)]
struct PathGlob {
    pattern: String,
    matcher: globset::GlobMatcher,
}

impl PartialEq for PathGlob {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern
    }
}

/// The six comparison operators `field:` and `date:` share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Which of the index's three timestamps a `date:` predicate reads.
///
/// Public because a space's `sort` names the same two facts the DSL does
/// (Story 44.4): `sort: created` and `date:created` have to resolve through
/// one chain, or a space could list notes in an order its own query
/// contradicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateField {
    Created,
    Modified,
    Touched,
}

/// A point in time named by a query, resolved against `now_ms` at evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DateSpec {
    /// An absolute instant and the width of the unit that named it: a bare
    /// `YYYY-MM-DD` is a whole day, `YYYY-MM-DDTHH:MM` a whole minute. The width
    /// is what makes `date:modified=2026-08-02` mean "that day" rather than "that
    /// exact millisecond".
    At {
        ms: i64,
        width_ms: i64,
    },
    Today,
    Yesterday,
    /// `-<n><unit>` before today's local midnight.
    Ago {
        n: i64,
        unit: Unit,
    },
}

/// The unit of a relative offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Days,
    Weeks,
    Months,
    Years,
}

/// What `origin:` accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
enum OriginSpec {
    Local,
    Agent,
    Remote,
    /// A named device, compared case-insensitively against `Keeper-Device`.
    Device(String),
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

/// One lexical token.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    LParen,
    RParen,
    Pipe,
    /// A word. `colon` is the byte offset within `text` of the first `:` that
    /// stood **outside** quotes; `None` makes the word a bareword, which is sugar
    /// for `text:`. Tracking it during lexing rather than splitting afterwards is
    /// what lets `text:"12:30"` be a text search and `"12:30"` a bareword,
    /// instead of both tripping over a colon the user quoted deliberately.
    Word {
        text: String,
        colon: Option<usize>,
    },
}

/// A token plus its byte range in the source, which is the whole of the located
/// error story.
#[derive(Debug, Clone, PartialEq)]
struct Token {
    tok: Tok,
    span: (usize, usize),
}

/// Accumulator for the word currently being lexed.
#[derive(Default)]
struct WordAcc {
    text: String,
    colon: Option<usize>,
    start: usize,
    end: usize,
    open: bool,
}

impl WordAcc {
    /// Mark `len` bytes at `at` as consumed by the current word, opening one if
    /// none is. Used for characters that belong to the token's span but not to
    /// its text — the quote marks.
    fn mark(&mut self, at: usize, len: usize) {
        if !self.open {
            self.open = true;
            self.start = at;
        }
        self.end = at + len;
    }

    fn push(&mut self, at: usize, c: char) {
        self.mark(at, c.len_utf8());
        self.text.push(c);
    }

    /// Record the position of the first unquoted `:`.
    fn mark_colon(&mut self) {
        if self.colon.is_none() {
            self.colon = Some(self.text.len());
        }
    }

    fn flush(&mut self, out: &mut Vec<Token>) {
        if !self.open {
            return;
        }
        out.push(Token {
            tok: Tok::Word {
                text: std::mem::take(&mut self.text),
                colon: self.colon.take(),
            },
            span: (self.start, self.end),
        });
        self.open = false;
    }
}

/// Split one line of query text into tokens.
///
/// Whitespace separates words except inside double quotes; `(`, `)` and `|` are
/// standalone; `\"` is a literal quote. An unterminated quote is closed at
/// end-of-input rather than rejected — the live validator runs on every
/// keystroke, and refusing `text:"foo` while the user is still reaching for the
/// closing quote would underline every partial query in red.
fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut acc = WordAcc::default();
    let mut quoted = false;
    let mut chars = input.char_indices().peekable();

    while let Some((at, c)) = chars.next() {
        if c == '\\' {
            if let Some(&(quote_at, '"')) = chars.peek() {
                chars.next();
                acc.mark(at, 1);
                acc.text.push('"');
                acc.end = quote_at + 1;
                continue;
            }
        }
        if c == '"' {
            quoted = !quoted;
            acc.mark(at, 1);
            continue;
        }
        if !quoted {
            if c.is_whitespace() {
                acc.flush(&mut tokens);
                continue;
            }
            if let Some(tok) = standalone(c) {
                acc.flush(&mut tokens);
                tokens.push(Token {
                    tok,
                    span: (at, at + c.len_utf8()),
                });
                continue;
            }
            if c == ':' {
                acc.mark_colon();
            }
        }
        acc.push(at, c);
    }
    acc.flush(&mut tokens);
    tokens
}

/// The three characters that are always their own token.
fn standalone(c: char) -> Option<Tok> {
    match c {
        '(' => Some(Tok::LParen),
        ')' => Some(Tok::RParen),
        '|' => Some(Tok::Pipe),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse one line of query text.
///
/// An empty query is an error, not a match-everything: `conjunction = term,
/// { term }` needs at least one term, and a space defined by nothing should say
/// so rather than quietly select the whole vault.
pub fn parse(input: &str) -> Result<Query, QueryError> {
    let tokens = tokenize(input);
    let eof = (input.len(), input.len());
    if tokens.len() > MAX_TOKENS {
        return Err(QueryError {
            message: format!(
                "query has {} tokens; the limit is {MAX_TOKENS}",
                tokens.len()
            ),
            token_index: MAX_TOKENS,
            byte_span: tokens.get(MAX_TOKENS).map_or(eof, |t| t.span),
        });
    }
    if tokens.is_empty() {
        return Err(QueryError {
            message: "empty query".to_owned(),
            token_index: 0,
            byte_span: eof,
        });
    }

    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
        input_len: input.len(),
    };
    let root = parser.disjunction(0)?;
    if parser.pos < tokens.len() {
        // The only token that can stop `disjunction` early is a `)` with no `(`.
        return Err(parser.err(parser.pos, "unbalanced parenthesis: no `(` to close"));
    }
    let needs_body = node_needs_body(&root);
    Ok(Query { root, needs_body })
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    input_len: usize,
}

impl Parser<'_> {
    fn err(&self, at: usize, message: impl Into<String>) -> QueryError {
        let eof = (self.input_len, self.input_len);
        QueryError {
            message: message.into(),
            token_index: at,
            byte_span: self.tokens.get(at).map_or(eof, |t| t.span),
        }
    }

    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos).map(|t| &t.tok)
    }

    /// `disjunction = conjunction , { "|" , conjunction }` — lowest precedence.
    fn disjunction(&mut self, depth: usize) -> Result<Node, QueryError> {
        let first = self.conjunction(depth)?;
        if !matches!(self.peek(), Some(Tok::Pipe)) {
            return Ok(first);
        }
        let mut parts = vec![first];
        while matches!(self.peek(), Some(Tok::Pipe)) {
            self.pos += 1;
            parts.push(self.conjunction(depth)?);
        }
        Ok(Node::Or(parts))
    }

    /// `conjunction = term , { term }` — AND by juxtaposition, and the place the
    /// index-before-body ordering contract is enforced.
    fn conjunction(&mut self, depth: usize) -> Result<Node, QueryError> {
        let first = self.term(depth)?;
        let mut rest = Vec::new();
        while matches!(self.peek(), Some(Tok::Word { .. } | Tok::LParen)) {
            rest.push(self.term(depth)?);
        }
        if rest.is_empty() {
            return Ok(first);
        }
        let mut parts = Vec::with_capacity(rest.len() + 1);
        parts.push(first);
        parts.append(&mut rest);
        // Stable, so terms of equal cost keep the order the author wrote.
        parts.sort_by_key(node_needs_body);
        Ok(Node::And(parts))
    }

    /// `term = [ "-" ] , ( group | predicate | bareword )`.
    fn term(&mut self, depth: usize) -> Result<Node, QueryError> {
        let at = self.pos;
        let Some(tok) = self.tokens.get(at).map(|t| t.tok.clone()) else {
            return Err(self.err(at, "expected a term"));
        };
        match tok {
            Tok::Pipe => Err(self.err(at, "expected a term before `|`")),
            Tok::RParen => Err(self.err(at, "unbalanced parenthesis: no `(` to close")),
            Tok::LParen => {
                self.pos += 1;
                // `depth` counts the groups already entered, so entering another
                // one at the cap is the rejection point.
                if depth >= MAX_DEPTH {
                    let message = format!("query nests deeper than {MAX_DEPTH} groups");
                    return Err(self.err(at, message));
                }
                let inner = self.disjunction(depth + 1)?;
                if !matches!(self.peek(), Some(Tok::RParen)) {
                    return Err(self.err(self.pos, "unbalanced parenthesis: expected `)`"));
                }
                self.pos += 1;
                Ok(inner)
            }
            Tok::Word { text, colon } => {
                // A lone `-` negates whatever term follows it, including a group.
                if text == "-" {
                    self.pos += 1;
                    let inner = self.term(depth)?;
                    return Ok(Node::Not(Box::new(inner)));
                }
                self.pos += 1;
                let (negated, body, colon) = match text.strip_prefix('-') {
                    Some(rest) => (true, rest, colon.map(|c| c.saturating_sub(1))),
                    None => (false, text.as_str(), colon),
                };
                let pred = Node::Pred(self.predicate(body, colon, at)?);
                Ok(if negated {
                    Node::Not(Box::new(pred))
                } else {
                    pred
                })
            }
        }
    }

    /// Turn one word into a leaf. A word with no unquoted `:` is a bareword and
    /// therefore sugar for `text:`.
    fn predicate(&self, word: &str, colon: Option<usize>, at: usize) -> Result<Pred, QueryError> {
        let Some(split) = colon else {
            return Ok(Pred::Text(word.to_owned()));
        };
        let key = &word[..split];
        let value = &word[split + 1..];
        match key {
            "tag" => Ok(tag_pred(value)),
            "path" => self.path_pred(value, at),
            "field" => self.field_pred(value, at),
            "date" => self.date_pred(value, at),
            "origin" => self.origin_pred(value, at),
            "is" => self.is_pred(value, at),
            "text" => Ok(Pred::Text(value.to_owned())),
            "link" => Ok(Pred::Link {
                key: link_key(value),
                bound: None,
            }),
            "backlink" => Ok(Pred::Backlink {
                key: link_key(value),
                bound: None,
            }),
            other => Err(self.err(at, format!("unknown search key `{other}`; expected {KEYS}"))),
        }
    }

    fn path_pred(&self, value: &str, at: usize) -> Result<Pred, QueryError> {
        // `literal_separator` is what makes `spaces/*` mean "directly inside
        // spaces" and `journal/**` mean "anywhere under journal" — the split the
        // documented examples rely on.
        //
        // Case sensitivity follows the filesystem the user is looking at. This is
        // the only `cfg!` in `keeper-core`, and it is a value rather than a code
        // path: AD-26 keeps platform *ports* in the shell, and a glob's case rule
        // is not a port, it is a property of the disk the paths came off.
        let glob = GlobBuilder::new(value)
            .literal_separator(true)
            .case_insensitive(cfg!(target_os = "macos") || cfg!(target_os = "windows"))
            .build()
            .map_err(|e| self.err(at, format!("path: invalid glob `{value}`: {e}")))?;
        Ok(Pred::Path(Box::new(PathGlob {
            pattern: value.to_owned(),
            matcher: glob.compile_matcher(),
        })))
    }

    fn field_pred(&self, value: &str, at: usize) -> Result<Pred, QueryError> {
        match split_cmp(value) {
            Some((key, _, _)) if key.trim().is_empty() => {
                Err(self.err(at, "field: expected a key before the comparison"))
            }
            Some((key, op, wanted)) => Ok(Pred::Field {
                key: key.trim().to_owned(),
                op: Some(op),
                value: wanted.trim().to_owned(),
            }),
            None if value.trim().is_empty() => Err(self.err(at, "field: expected a key")),
            None => Ok(Pred::Field {
                key: value.trim().to_owned(),
                op: None,
                value: String::new(),
            }),
        }
    }

    fn date_pred(&self, value: &str, at: usize) -> Result<Pred, QueryError> {
        let Some((raw_field, op, raw_spec)) = split_cmp(value) else {
            return Err(self.err(
                at,
                "date: expected a comparison, e.g. `date:modified>=-14d`",
            ));
        };
        let field = match raw_field.trim().to_ascii_lowercase().as_str() {
            "created" => DateField::Created,
            "modified" => DateField::Modified,
            "touched" => DateField::Touched,
            other => {
                let message = format!("date: unknown field `{other}`; expected {DATE_FIELDS}");
                return Err(self.err(at, message));
            }
        };
        let Some(spec) = parse_datespec(raw_spec) else {
            let spec_text = raw_spec.trim();
            let message = format!("date: malformed date `{spec_text}`; expected {DATE_SPECS}");
            return Err(self.err(at, message));
        };
        Ok(Pred::Date { field, op, spec })
    }

    fn origin_pred(&self, value: &str, at: usize) -> Result<Pred, QueryError> {
        let folded = value.trim().to_ascii_lowercase();
        let spec = match folded.as_str() {
            "local" => OriginSpec::Local,
            "agent" => OriginSpec::Agent,
            "remote" => OriginSpec::Remote,
            other => match other.strip_prefix("device:") {
                Some(label) if !label.is_empty() => OriginSpec::Device(label.to_owned()),
                _ => {
                    let message = format!("origin: unknown value `{other}`; expected {ORIGINS}");
                    return Err(self.err(at, message));
                }
            },
        };
        Ok(Pred::Origin(spec))
    }

    fn is_pred(&self, value: &str, at: usize) -> Result<Pred, QueryError> {
        let folded = value.trim().to_ascii_lowercase();
        if IS_FLAGS.contains(&folded.as_str()) {
            return Ok(Pred::Is(folded));
        }
        let hint = IS_FLAGS.join(", ");
        let message = format!("is: unknown flag `{folded}`; expected one of {hint}");
        Err(self.err(at, message))
    }
}

/// `tag:` with its optional `/*` descendants-only suffix.
///
/// The value is normalised exactly as an indexed tag is, and by the one function
/// that defines what a tag is (Story 42.5) — so `tag:#Project` finds `project`,
/// and `tag:Client/Acme ` finds the node a recording and a note now share.
///
/// Normalisation never rejects here: a term that is not a tag becomes the empty
/// path, which no indexed tag equals and none descends from, so the predicate
/// simply matches nothing. That is deliberately not the same class of mistake as
/// an unknown key — `tag:---` is a search that finds nothing, not a query error.
/// The empty path is used rather than a hand-folded copy of the term, because a
/// second lowercase-and-trim here would be a second definition of a tag.
fn tag_pred(value: &str) -> Pred {
    let (base, strict) = match value.strip_suffix("/*") {
        Some(base) => (base, true),
        None => (value, false),
    };
    let path = crate::notes::tags::normalise(base).unwrap_or_default();
    Pred::Tag { path, strict }
}

// ---------------------------------------------------------------------------
// Decomposition
// ---------------------------------------------------------------------------

/// Read a stored query back into the vocabulary a space editor's controls speak
/// (FR-149, UX-DR55), or say which of its terms they cannot hold.
///
/// The inverse direction of the DSL, and it lives here for the reason the
/// forward direction does: there is one grammar, one tokenizer and one
/// definition of what a tag is, and a space editor that re-derived any of them
/// in TypeScript would be a second parser drifting against this one from the
/// day it was written (AD-20, AD-58).
///
/// **The chips are all-or-nothing.** A query is either entirely expressible as
/// chips or it is not editable through them at all — see
/// [`NoteSpaceTermsVm`]'s own note. The failure this refuses is the one the
/// story names as the worst available outcome: an editor that shows three of a
/// query's four terms, saves what it can see, and quietly deletes
/// `date:modified>=-14d` on the way. keeper does not rewrite a term it could
/// not read.
///
/// The chip vocabulary is a **flat conjunction** of: `tag:x` / `-tag:x` (at most
/// one term per tag, since a chip has one slot per tag), `is:<flag>`,
/// `origin:<value>` and one free-text term. Everything else the grammar
/// parses — `|`, groups, `path:`, `field:`, `date:`, `link:`, `backlink:`,
/// `tag:x/*`, and negation of anything that is not a `tag:` — is reported
/// verbatim rather than approximated.
///
/// A query that does not parse is an `Err`, not an empty chip set: the space row
/// already renders that failure (`NoteSpaceVm::error`), and an editor that
/// silently offered "no terms" for a broken query would be one Save away from
/// replacing a typo with a space that selects the whole vault.
pub fn decompose(input: &str) -> Result<NoteSpaceTermsVm, QueryError> {
    parse(input)?;
    let tokens = tokenize(input);

    // Structure first. A `|` or a group is not a term, so there is no honest way
    // to name the offending *part* of it — the whole query goes back verbatim.
    let mut words = Vec::with_capacity(tokens.len());
    for token in &tokens {
        let Tok::Word { text, colon } = &token.tok else {
            return Ok(NoteSpaceTermsVm::Unrepresentable {
                terms: vec![input.trim().to_owned()],
            });
        };
        words.push((text.as_str(), *colon, token.span));
    }

    let mut tags: Vec<NoteSpaceTagVm> = Vec::new();
    let mut flags: Vec<String> = Vec::new();
    let mut origin: Option<String> = None;
    let mut text: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();

    let mut at = 0;
    while at < words.len() {
        let (word, colon, span) = words[at];
        at += 1;
        // A lone `-` negates the term after it. The pair is one term, so it is
        // reported as one — splitting it would name `-` on its own, which reads
        // like a typo rather than like the negation it is.
        if word == "-" {
            let end = words.get(at).map_or(span.1, |next| next.2 .1);
            at += 1;
            rest.push(input[span.0..end].trim().to_owned());
            continue;
        }
        let source = input[span.0..span.1].to_owned();
        let (negated, body, colon) = match word.strip_prefix('-') {
            Some(after) => (true, after, colon.map(|c| c.saturating_sub(1))),
            None => (false, word, colon),
        };
        let Some(split) = colon else {
            // A bareword is sugar for `text:`, and the bar holds one text term.
            if negated || text.is_some() {
                rest.push(source);
            } else {
                text = Some(body.to_owned());
            }
            continue;
        };
        let value = &body[split + 1..];
        match &body[..split] {
            "tag" => match chip_tag(value) {
                // One slot per tag is the whole of 43.3's guarantee, so a query
                // naming a tag twice cannot be shown as chips without one of the
                // two disappearing.
                Some(tag) if !tags.iter().any(|held| held.tag == tag) => {
                    tags.push(NoteSpaceTagVm {
                        tag,
                        term: if negated {
                            NoteTagTerm::Exclude
                        } else {
                            NoteTagTerm::Include
                        },
                    })
                }
                _ => rest.push(source),
            },
            "is" if !negated && !flags.iter().any(|held| same_flag(held, value)) => {
                flags.push(value.to_owned());
            }
            "origin" if !negated && origin.is_none() => origin = Some(value.to_owned()),
            "text" if !negated && text.is_none() => text = Some(value.to_owned()),
            _ => rest.push(source),
        }
    }

    if rest.is_empty() {
        Ok(NoteSpaceTermsVm::Chips {
            tags,
            flags,
            origin,
            text,
        })
    } else {
        Ok(NoteSpaceTermsVm::Unrepresentable { terms: rest })
    }
}

/// The tag a chip would carry for a `tag:` value, or `None` when no chip can.
///
/// Two refusals, both because a chip names a node: `tag:x/*` is the subtree
/// *without* its own node, which no chip state spells, and a value that is not a
/// tag at all normalises to the empty path — the DSL lets that match nothing
/// (see [`tag_pred`]), but a chip labelled with the empty string is a control
/// with nothing written on it.
fn chip_tag(value: &str) -> Option<String> {
    if value.ends_with("/*") {
        return None;
    }
    crate::notes::tags::normalise(value)
}

/// Whether two `is:` values name the same flag. The parser folds case before it
/// matches [`IS_FLAGS`], so `is:Pinned is:pinned` is one flag written twice and
/// the chip row must not show it as two.
fn same_flag(held: &str, other: &str) -> bool {
    held.trim().eq_ignore_ascii_case(other.trim())
}

/// Split `key OP value` at the leftmost operator, preferring the two-character
/// forms so `>=` never lexes as `>` followed by a stray `=`.
fn split_cmp(value: &str) -> Option<(&str, CmpOp, &str)> {
    for (i, byte) in value.bytes().enumerate() {
        // A multi-byte character's continuation bytes are all >= 0x80, so they
        // can never match an operator and `i` is always a char boundary here.
        let (op, len) = match value.get(i..i + 2) {
            Some("!=") => (CmpOp::Ne, 2),
            Some("<=") => (CmpOp::Le, 2),
            Some(">=") => (CmpOp::Ge, 2),
            _ => match byte {
                b'=' => (CmpOp::Eq, 1),
                b'<' => (CmpOp::Lt, 1),
                b'>' => (CmpOp::Gt, 1),
                _ => continue,
            },
        };
        return Some((&value[..i], op, &value[i + len..]));
    }
    None
}

/// Parse a `datespec`: `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM`, `today`, `yesterday`, or
/// a relative `-<n>[dwmy]`.
fn parse_datespec(raw: &str) -> Option<DateSpec> {
    let spec = raw.trim();
    let folded = spec.to_ascii_lowercase();
    match folded.as_str() {
        "today" => return Some(DateSpec::Today),
        "yesterday" => return Some(DateSpec::Yesterday),
        _ => {}
    }
    if let Some(rest) = folded.strip_prefix('-') {
        let (unit_at, unit_char) = rest.char_indices().next_back()?;
        let unit = match unit_char {
            'd' => Unit::Days,
            'w' => Unit::Weeks,
            'm' => Unit::Months,
            'y' => Unit::Years,
            _ => return None,
        };
        let n: i64 = rest[..unit_at].parse().ok()?;
        if !(0..=MAX_RELATIVE).contains(&n) {
            return None;
        }
        return Some(DateSpec::Ago { n, unit });
    }
    let (ms, width_ms) = parse_absolute(spec)?;
    Some(DateSpec::At { ms, width_ms })
}

/// Parse an absolute `YYYY-MM-DD[THH:MM…]` into `(start_ms, width_ms)`.
///
/// Anything after the minute is ignored rather than rejected, so a frontmatter
/// value written as full RFC3339 (`2026-08-02T10:30:00Z`) is usable by `date:`
/// without the user having to know which precision keeper happened to store.
fn parse_absolute(spec: &str) -> Option<(i64, i64)> {
    let (date, time) = match spec.split_once(['T', 't']) {
        Some((date, time)) => (date, Some(time)),
        None => (spec, None),
    };
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=9999).contains(&year) || !(1..=12).contains(&month) {
        return None;
    }
    if !(1..=days_in_month(year, month)).contains(&day) {
        return None;
    }
    let midnight = days_from_civil(year, month, day) * DAY_MS;
    let Some(time) = time else {
        return Some((midnight, DAY_MS));
    };
    let (hh, rest) = time.split_once(':')?;
    let hour: i64 = hh.parse().ok()?;
    let minute_digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let minute: i64 = minute_digits.parse().ok()?;
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) {
        return None;
    }
    Some((midnight + hour * HOUR_MS + minute * MINUTE_MS, MINUTE_MS))
}

/// Days from 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`, the standard branch-free formulation).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The inverse of [`days_from_civil`], as `(year, month, day)`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (if month <= 2 { y + 1 } else { y }, month, day)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Local midnight of the day `now_ms` falls in.
///
/// `now_ms` is the caller's **local** wall clock expressed as ms since the epoch.
/// `keeper-core` has no timezone database and no clock, so the shell shifts once
/// and every relative spec resolves against local midnight here — which is the
/// point of the rule: a space must not change meaning mid-afternoon.
fn midnight_ms(now_ms: i64) -> i64 {
    now_ms.div_euclid(DAY_MS) * DAY_MS
}

/// Resolve a [`DateSpec`] to the half-open window `[start, start + width)`.
fn window(spec: DateSpec, now_ms: i64) -> (i64, i64) {
    match spec {
        DateSpec::At { ms, width_ms } => (ms, width_ms),
        DateSpec::Today => (midnight_ms(now_ms), DAY_MS),
        DateSpec::Yesterday => (midnight_ms(now_ms) - DAY_MS, DAY_MS),
        DateSpec::Ago { n, unit } => {
            let midnight = midnight_ms(now_ms);
            let start = match unit {
                Unit::Days => midnight - n * DAY_MS,
                Unit::Weeks => midnight - n * 7 * DAY_MS,
                Unit::Months => months_before(midnight, n),
                Unit::Years => months_before(midnight, n * 12),
            };
            (start, DAY_MS)
        }
    }
}

/// `months` calendar months before the midnight at `from_ms`, clamping the day of
/// month so "one month before the 31st" lands on the last day of a short month
/// rather than skidding into the next one.
fn months_before(from_ms: i64, months: i64) -> i64 {
    let (year, month, day) = civil_from_days(from_ms.div_euclid(DAY_MS));
    let total = year * 12 + (month - 1) - months;
    let new_year = total.div_euclid(12);
    let new_month = total.rem_euclid(12) + 1;
    let new_day = day.min(days_in_month(new_year, new_month));
    days_from_civil(new_year, new_month, new_day) * DAY_MS
}

// ---------------------------------------------------------------------------
// Binding and evaluation
// ---------------------------------------------------------------------------

/// Whether the query contains a term that has to read a note's body.
pub fn needs_body(q: &Query) -> bool {
    q.needs_body
}

fn node_needs_body(node: &Node) -> bool {
    match node {
        Node::Or(kids) | Node::And(kids) => kids.iter().any(node_needs_body),
        Node::Not(inner) => node_needs_body(inner),
        // An empty needle matches everything without looking, so it costs no read.
        Node::Pred(Pred::Text(needle)) => !needle.is_empty(),
        Node::Pred(_) => false,
    }
}

/// Resolve the `link:` and `backlink:` targets in a query against an index.
///
/// [`eval`] sees one [`IndexEntry`] at a time, but "who links to me" is a
/// whole-index fact, so `backlink:` cannot be answered from an entry alone: an
/// **unbound** `backlink:` matches nothing, the same degradation a broken query
/// already takes. Binding also upgrades `link:` from a literal string match to a
/// real resolution, so `link:notes/vault.md` matches a body that wrote
/// `[[Vault as a Lens]]`.
///
/// Cheap and idempotent: call it once per snapshot revision before running a
/// space, or not at all for a query that uses neither predicate.
pub fn bind_index(query: &mut Query, index: &IndexSnapshot) {
    bind_node(&mut query.root, index);
}

fn bind_node(node: &mut Node, index: &IndexSnapshot) {
    match node {
        Node::Or(kids) | Node::And(kids) => {
            for kid in kids {
                bind_node(kid, index);
            }
        }
        Node::Not(inner) => bind_node(inner, index),
        Node::Pred(Pred::Link { key, bound }) => {
            *bound = index.resolve_link(key).map(IndexEntry::link_keys);
        }
        Node::Pred(Pred::Backlink { key, bound }) => {
            *bound = index.resolve_link(key).map(|target| {
                target
                    .links
                    .iter()
                    .filter_map(|link| index.resolve_link(link))
                    .map(|hit| hit.id.clone())
                    .collect()
            });
        }
        Node::Pred(_) => {}
    }
}

/// Evaluate a query against one entry.
///
/// `body` is called at most once per `text:` term that is actually reached, and
/// never at all when [`needs_body`] is false — callers memoise a single file read
/// behind it. `now_ms` is local wall clock in ms since the epoch (see
/// [`midnight_ms`]).
pub fn eval(q: &Query, e: &IndexEntry, body: &mut dyn FnMut() -> String, now_ms: i64) -> bool {
    eval_node(&q.root, e, body, now_ms)
}

fn eval_node(node: &Node, e: &IndexEntry, body: &mut dyn FnMut() -> String, now_ms: i64) -> bool {
    match node {
        Node::Or(kids) => {
            for kid in kids {
                if eval_node(kid, e, body, now_ms) {
                    return true;
                }
            }
            false
        }
        Node::And(kids) => {
            for kid in kids {
                if !eval_node(kid, e, body, now_ms) {
                    return false;
                }
            }
            true
        }
        Node::Not(inner) => !eval_node(inner, e, body, now_ms),
        Node::Pred(pred) => eval_pred(pred, e, body, now_ms),
    }
}

fn eval_pred(pred: &Pred, e: &IndexEntry, body: &mut dyn FnMut() -> String, now_ms: i64) -> bool {
    match pred {
        Pred::Tag { path, strict } => e.tags.iter().any(|tag| {
            if *strict {
                is_tag_descendant(tag, path)
            } else {
                tag_covers(tag, path)
            }
        }),
        Pred::Path(glob) => glob.matcher.is_match(e.path.as_str()),
        Pred::Field { key, op, value } => eval_field(e, key, *op, value),
        Pred::Date { field, op, spec } => {
            let (start, width) = window(*spec, now_ms);
            compare_window(resolve_date(*field, e), *op, start, width)
        }
        Pred::Origin(spec) => eval_origin(e, spec),
        Pred::Is(flag) => {
            if flag.as_str() == IS_UNTAGGED {
                e.tags.is_empty()
            } else {
                e.has_flag(flag)
            }
        }
        Pred::Text(needle) => eval_text(needle, e, body),
        Pred::Link { key, bound } => e.links.iter().any(|link| {
            let folded = link_key(link);
            match bound {
                // Bound: match any key the resolved target answers to.
                Some(keys) => keys.contains(&folded),
                // Unbound: the literal target is all we have.
                None => &folded == key,
            }
        }),
        Pred::Backlink { bound, .. } => bound.as_ref().is_some_and(|ids| ids.contains(&e.id)),
    }
}

// The `tag:` segment rule lives in `index::is_tag_descendant`, beside the tree
// that rolls counts up it and the chip predicate that deselects subtrees by it.

fn eval_text(needle: &str, e: &IndexEntry, body: &mut dyn FnMut() -> String) -> bool {
    if needle.is_empty() {
        return true;
    }
    // Title first: a hit there spares the body read entirely. The folding comes
    // from `search::find` so a space and the search surface can never disagree
    // about what "café" matches.
    if !search::find(&e.title, needle, 1).is_empty() {
        return true;
    }
    !search::find(&body(), needle, 1).is_empty()
}

fn eval_origin(e: &IndexEntry, spec: &OriginSpec) -> bool {
    let raw = e.fields.get(FIELD_ORIGIN).map_or("", |v| v.trim());
    // A note nobody has committed yet is this device's work.
    let origin = if raw.is_empty() { "local" } else { raw };
    match spec {
        OriginSpec::Local => origin.eq_ignore_ascii_case("local"),
        OriginSpec::Agent => origin.eq_ignore_ascii_case("agent"),
        OriginSpec::Remote => origin.eq_ignore_ascii_case("remote"),
        OriginSpec::Device(label) => e
            .fields
            .get(FIELD_DEVICE)
            .is_some_and(|device| device.trim().eq_ignore_ascii_case(label)),
    }
}

/// `date:`'s documented fallback chain.
///
/// The chain is the whole reason `date:` exists separately from `field:`:
/// `created` prefers the author's frontmatter and falls back to whatever the
/// reconciler resolved from the first commit or the file's birth time;
/// `modified` prefers frontmatter `updated`, then the resolved timestamp, then
/// the raw mtime. `touched` is a per-device fact the frozen [`IndexEntry`] has no
/// slot for, so it reads [`FIELD_TOUCHED`] when something supplies one and
/// otherwise degrades to `modified` — a missing local fact must never break a
/// shared space.
///
/// Public for the same reason [`DateField`] is: a space sorted by `created`
/// and a space filtered by `date:created` must be answering the same question
/// about the same note (Story 44.4).
pub fn resolve_date(field: DateField, e: &IndexEntry) -> i64 {
    match field {
        DateField::Created => field_ms(e, "created").unwrap_or(e.created_ms),
        DateField::Modified => field_ms(e, "updated").unwrap_or_else(|| {
            if e.updated_ms == 0 {
                i64::try_from(e.mtime_ns / 1_000_000).unwrap_or(i64::MAX)
            } else {
                e.updated_ms
            }
        }),
        DateField::Touched => {
            field_ms(e, FIELD_TOUCHED).unwrap_or_else(|| resolve_date(DateField::Modified, e))
        }
    }
}

/// Read a stored timestamp — `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM`, or full RFC 3339
/// — as epoch milliseconds, or `None` when the text is not a date.
///
/// The one reading of a frontmatter stamp. A space's `recorded` sort composes
/// the recording stub's own `date` and `start` keys and has to land on the
/// instant `date:` would have compared against, so it comes through here rather
/// than through a second parser (Story 44.4).
pub fn stamp_ms(spec: &str) -> Option<i64> {
    parse_absolute(spec.trim()).map(|(ms, _)| ms)
}

/// Read one field as a timestamp, or `None` when it is absent or not a date.
fn field_ms(e: &IndexEntry, key: &str) -> Option<i64> {
    stamp_ms(e.fields.get(key)?)
}

/// Apply an operator to a timestamp against the half-open window a datespec names.
fn compare_window(ts: i64, op: CmpOp, start: i64, width: i64) -> bool {
    let end = start.saturating_add(width);
    match op {
        CmpOp::Eq => ts >= start && ts < end,
        CmpOp::Ne => ts < start || ts >= end,
        CmpOp::Lt => ts < start,
        CmpOp::Le => ts < end,
        CmpOp::Gt => ts >= end,
        CmpOp::Ge => ts >= start,
    }
}

fn eval_field(e: &IndexEntry, key: &str, op: Option<CmpOp>, wanted: &str) -> bool {
    let Some(stored) = lookup_field(e, key) else {
        // A note without the field has no value to compare, so *every* operator
        // is false here — `-field:x=y` is how you ask for "not y, or absent".
        return false;
    };
    let Some(op) = op else {
        return !stored.trim().is_empty();
    };
    match op {
        // `=` against a list field means contains, so the stored value is read as
        // the list the index flattened it from, and `!=` is exactly its negation.
        CmpOp::Eq => field_contains(stored, wanted),
        CmpOp::Ne => !field_contains(stored, wanted),
        other => compare_values(stored.trim(), other, wanted),
    }
}

/// Whether any item of a (possibly list-valued) field equals `wanted`.
fn field_contains(stored: &str, wanted: &str) -> bool {
    field_items(stored)
        .iter()
        .any(|item| compare_values(item, CmpOp::Eq, wanted))
}

/// Frontmatter keys belong to the user, so look up exactly first and only then
/// fold case — an exact hit must never be shadowed by a differently-cased sibling.
fn lookup_field<'a>(e: &'a IndexEntry, key: &str) -> Option<&'a str> {
    if let Some(hit) = e.fields.get(key) {
        return Some(hit.as_str());
    }
    e.fields
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
}

/// The items of a possibly-list-valued field. A scalar yields itself.
///
/// [`FIELD_LIST_SEPARATOR`] is the contract, because a frontmatter scalar is
/// single-line by construction and so can never contain a newline. The comma
/// fallback exists only for a value that has no newline at all: it costs one
/// extra candidate and it means a vault indexed by an older flattener still
/// answers `field:authors=Grace`. It deliberately keeps the whole value as a
/// candidate too, so "Doe, Jane" still matches itself rather than only its halves.
fn field_items(stored: &str) -> Vec<&str> {
    let lines: Vec<&str> = stored
        .split(FIELD_LIST_SEPARATOR)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect();
    if lines.len() > 1 {
        return lines;
    }
    let mut commas: Vec<&str> = stored
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect();
    if commas.len() > 1 {
        commas.push(stored.trim());
        return commas;
    }
    lines
}

/// The typed value comparison behind `field:`.
///
/// The index stores every field as a string, so the *type* is inferred from both
/// sides and a comparison between incompatible types is **false, never an
/// error**. That is the rule that stops an agent writing `priority: high` into a
/// numeric field from breaking the space that queries it.
fn compare_values(stored: &str, op: CmpOp, wanted: &str) -> bool {
    let ord = match (classify(stored), classify(wanted)) {
        (Typed::Num(a), Typed::Num(b)) => a.partial_cmp(&b),
        (Typed::Bool(a), Typed::Bool(b)) => Some(a.cmp(&b)),
        (Typed::Date(a), Typed::Date(b)) => Some(a.cmp(&b)),
        (Typed::Str(a), Typed::Str(b)) => Some(a.to_lowercase().cmp(&b.to_lowercase())),
        _ => None,
    };
    let Some(ord) = ord else {
        // Incompatible types: false, never an error.
        return false;
    };
    match op {
        CmpOp::Eq => ord.is_eq(),
        CmpOp::Ne => ord.is_ne(),
        CmpOp::Lt => ord.is_lt(),
        CmpOp::Le => ord.is_le(),
        CmpOp::Gt => ord.is_gt(),
        CmpOp::Ge => ord.is_ge(),
    }
}

/// The inferred type of a stringified field value.
#[derive(Debug)]
enum Typed<'a> {
    Num(f64),
    Bool(bool),
    Date(i64),
    Str(&'a str),
}

fn classify(raw: &str) -> Typed<'_> {
    let text = raw.trim();
    // `inf` and `NaN` parse as f64 but nobody writes them in frontmatter meaning
    // a number; leave them as strings.
    if let Some(n) = text.parse::<f64>().ok().filter(|n| n.is_finite()) {
        return Typed::Num(n);
    }
    // Only `true`/`false` are booleans. `yes`/`no` stay strings on purpose — the
    // Norway problem: a `country: NO` field must not become `false`.
    if text.eq_ignore_ascii_case("true") {
        return Typed::Bool(true);
    }
    if text.eq_ignore_ascii_case("false") {
        return Typed::Bool(false);
    }
    if let Some((ms, _)) = parse_absolute(text) {
        return Typed::Date(ms);
    }
    Typed::Str(text)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::notes::index::{IndexBuilder, NoteDelta};

    /// 2026-08-02T12:00, local wall clock as ms since the epoch (day 20 667).
    const NOW: i64 = 20_667 * DAY_MS + 12 * HOUR_MS;

    fn entry(path: &str) -> IndexEntry {
        IndexEntry {
            id: format!("id-{path}"),
            path: path.to_owned(),
            title: path.to_owned(),
            size: 0,
            mtime_ns: 0,
            ino: 0,
            created_ms: 0,
            updated_ms: 0,
            tags: Vec::new(),
            fields: BTreeMap::new(),
            links: Vec::new(),
            flags: Vec::new(),
            snippet: String::new(),
            order: crate::notes::order::NoteOrder::default(),
        }
    }

    /// Evaluate against an empty body.
    fn hit(query: &str, e: &IndexEntry) -> bool {
        let q = parse(query).unwrap_or_else(|err| panic!("parse `{query}`: {}", err.message));
        let mut body = String::new;
        eval(&q, e, &mut body, NOW)
    }

    fn hit_with_body(query: &str, e: &IndexEntry, text: &str) -> bool {
        let q = parse(query).unwrap_or_else(|err| panic!("parse `{query}`: {}", err.message));
        let mut body = || text.to_owned();
        eval(&q, e, &mut body, NOW)
    }

    fn error(query: &str) -> QueryError {
        match parse(query) {
            Ok(_) => panic!("`{query}` should not parse"),
            Err(err) => err,
        }
    }

    #[test]
    fn now_is_the_date_the_tests_claim_it_is() {
        // Every date assertion below reads as prose only if this holds.
        assert_eq!(civil_from_days(midnight_ms(NOW) / DAY_MS), (2026, 8, 2));
    }

    #[test]
    fn tag_matches_by_segment_prefix_never_by_lexical_prefix() {
        let mut e = entry("a.md");
        e.tags = vec!["project/keeper".to_owned()];
        assert!(hit("tag:project", &e), "a parent covers its descendants");
        assert!(hit("tag:project/keeper", &e), "an exact tag matches itself");
        assert!(!hit("tag:projects", &e), "`projects` is not `project`");
        assert!(!hit("tag:proj", &e), "a partial segment is not a prefix");

        let mut exact = entry("b.md");
        exact.tags = vec!["project".to_owned()];
        assert!(hit("tag:project", &exact));
        assert!(
            !hit("tag:project/*", &exact),
            "`/*` is strict descendants only"
        );
        assert!(hit("tag:project/*", &e), "the child matches `/*`");
        // Which is what makes the documented exact-match idiom work.
        assert!(hit("tag:project -tag:project/*", &exact));
        assert!(!hit("tag:project -tag:project/*", &e));
    }

    #[test]
    fn a_tag_term_is_read_as_the_one_vocabulary_and_a_non_tag_finds_nothing() {
        // Story 42.5: the `tag:` term goes through the one normalisation, so a
        // person typing what a recording's card showed them finds the node the
        // sidebar calls `client/acme`.
        let mut e = entry("a.md");
        e.tags = vec!["client/acme".to_owned()];
        for typed in [
            "tag:Client/Acme",
            "tag:#client/acme",
            "tag:client//acme/",
            "tag:CLIENT/ACME",
        ] {
            assert!(hit(typed, &e), "`{typed}` is the tag this note carries");
        }
        // A term that is not a tag matches nothing rather than matching
        // everything — the empty path is not a wildcard.
        assert!(!hit("tag:---", &e));
        assert!(!hit("tag:///", &e));
        assert!(!hit("tag:---/*", &e));
    }

    #[test]
    fn path_globs_respect_the_separator() {
        let deep = entry("journal/2026/2026-08-02.md");
        let shallow = entry("readme.md");
        let space = entry("spaces/active.md");
        assert!(hit("path:journal/**", &deep));
        assert!(!hit("path:journal/*", &deep), "`*` does not cross `/`");
        assert!(hit("path:*.md", &shallow));
        assert!(!hit("path:*.md", &deep), "`*.md` is top level only");
        assert!(hit("path:**/*.md", &deep));
        assert!(hit("path:spaces/*", &space));
    }

    #[test]
    fn field_presence_and_typed_comparison_including_the_mismatch_rule() {
        let mut e = entry("a.md");
        e.fields.insert("status".to_owned(), "open".to_owned());
        e.fields.insert("priority".to_owned(), "3".to_owned());
        e.fields.insert("blank".to_owned(), "  ".to_owned());
        e.fields.insert("done".to_owned(), "false".to_owned());

        assert!(hit("field:status", &e), "present and non-empty");
        assert!(!hit("field:blank", &e), "whitespace is not presence");
        assert!(!hit("field:missing", &e));

        assert!(hit("field:status=open", &e));
        assert!(hit("field:status=OPEN", &e), "strings fold case");
        assert!(hit("field:status!=closed", &e));
        assert!(hit("field:priority>=3", &e));
        assert!(hit("field:priority<10", &e), "numbers compare numerically");
        assert!(!hit("field:priority>3", &e));
        assert!(hit("field:done=false", &e));
        assert!(!hit("field:done=true", &e));

        // The rule that stops an agent from breaking a space: an incompatible
        // comparison is false, and never an error.
        let mut prose = entry("b.md");
        prose
            .fields
            .insert("priority".to_owned(), "high".to_owned());
        assert!(parse("field:priority>=3").is_ok(), "it still parses");
        assert!(!hit("field:priority>=3", &prose));
        assert!(!hit("field:priority<3", &prose));
        assert!(
            !hit("field:priority=3", &prose),
            "even equality is false across types"
        );
        // …and a missing field is false for every operator, including `!=`.
        assert!(!hit("field:nothing!=x", &e));
        assert!(hit("-field:nothing!=x", &e), "negation is how you ask");
    }

    #[test]
    fn equality_against_a_list_field_means_contains() {
        let mut e = entry("a.md");
        // The index flattens a list with `FIELD_LIST_SEPARATOR`.
        e.fields
            .insert("authors".to_owned(), "Ada\nGrace\nDoe, Jane".to_owned());
        assert!(hit("field:authors=Grace", &e));
        assert!(hit("field:authors=\"Doe, Jane\"", &e), "commas are data");
        assert!(!hit("field:authors=Alan", &e));
        assert!(hit("field:authors!=Alan", &e), "`!=` negates contains");
        assert!(!hit("field:authors!=Grace", &e));
    }

    #[test]
    fn date_resolves_the_fallback_chain_in_that_order() {
        // `modified` prefers frontmatter `updated`, then the resolved timestamp,
        // then the raw mtime — the chain is the reason `date:` is not `field:`.
        let mut only_mtime = entry("a.md");
        only_mtime.mtime_ns = i128::from(NOW - 3 * DAY_MS) * 1_000_000;
        assert!(hit("date:modified>=-7d", &only_mtime));
        assert!(!hit("date:modified>=-1d", &only_mtime));

        let mut with_updated_ms = only_mtime.clone();
        with_updated_ms.updated_ms = NOW;
        assert!(
            hit("date:modified>=-1d", &with_updated_ms),
            "updated_ms outranks mtime"
        );

        let mut with_frontmatter = with_updated_ms.clone();
        with_frontmatter
            .fields
            .insert("updated".to_owned(), "2020-01-01".to_owned());
        assert!(
            !hit("date:modified>=-1d", &with_frontmatter),
            "frontmatter outranks updated_ms"
        );
        assert!(hit("date:modified<2021-01-01", &with_frontmatter));

        // `created` prefers frontmatter `created`, else `created_ms`.
        let mut created = entry("b.md");
        created.created_ms = NOW;
        assert!(hit("date:created=today", &created));
        created
            .fields
            .insert("created".to_owned(), "2019-05-04".to_owned());
        assert!(!hit("date:created=today", &created));
        assert!(hit("date:created=2019-05-04", &created));

        // `touched` has no slot on IndexEntry, so it reads the reserved field and
        // otherwise degrades to `modified` rather than failing.
        let mut touched = entry("c.md");
        touched.updated_ms = NOW;
        assert!(hit("date:touched=today", &touched), "degrades to modified");
        touched
            .fields
            .insert(FIELD_TOUCHED.to_owned(), "2001-09-11".to_owned());
        assert!(!hit("date:touched=today", &touched));
        assert!(hit("date:touched=2001-09-11", &touched));
    }

    #[test]
    fn relative_and_keyword_datespecs_resolve_against_local_midnight() {
        let mut e = entry("a.md");
        e.updated_ms = NOW;
        assert!(hit("date:modified=today", &e));
        assert!(!hit("date:modified=yesterday", &e));
        assert!(hit("date:modified>=-0d", &e));

        let mut old = entry("b.md");
        old.updated_ms = NOW - 20 * DAY_MS;
        assert!(hit("date:modified>=-30d", &old));
        assert!(!hit("date:modified>=-14d", &old));
        assert!(hit("date:modified>=-3w", &old));
        assert!(hit("date:modified>=-1m", &old));
        assert!(hit("date:modified>=-1y", &old));
        assert!(hit("date:modified<today", &old));

        // A bare day names a whole day, not an instant.
        let mut same_day = entry("c.md");
        same_day.updated_ms = midnight_ms(NOW) + 23 * HOUR_MS;
        assert!(hit("date:modified=2026-08-02", &same_day));
        assert!(hit("date:modified<=2026-08-02", &same_day));
        assert!(!hit("date:modified>2026-08-02", &same_day));
        // A minute-precision spec narrows the window to that minute.
        assert!(hit("date:modified>=2026-08-02T22:00", &same_day));
        assert!(!hit("date:modified=2026-08-02T22:00", &same_day));
    }

    #[test]
    fn origin_defaults_to_local_and_names_devices() {
        let uncommitted = entry("a.md");
        assert!(hit("origin:local", &uncommitted), "no commit yet is local");
        assert!(!hit("origin:agent", &uncommitted));

        let mut bot = entry("b.md");
        bot.fields
            .insert(FIELD_ORIGIN.to_owned(), "agent".to_owned());
        bot.fields
            .insert(FIELD_DEVICE.to_owned(), "hesperia".to_owned());
        assert!(hit("origin:agent", &bot));
        assert!(!hit("origin:local", &bot));
        assert!(hit("origin:device:HESPERIA", &bot), "labels fold case");
        assert!(!hit("origin:device:other", &bot));
    }

    #[test]
    fn is_reads_index_flags_and_derives_untagged() {
        let mut pinned = entry("a.md");
        pinned.flags = vec!["pinned".to_owned(), "journal".to_owned()];
        pinned.tags = vec!["x".to_owned()];
        assert!(hit("is:pinned", &pinned));
        assert!(hit("is:journal", &pinned));
        assert!(!hit("is:archived", &pinned));
        assert!(!hit("is:untagged", &pinned));
        assert!(hit("is:untagged", &entry("b.md")));
    }

    /// The Recordings lens is a saved `is:` scope, so a space must be able to
    /// say the same thing the sidebar row says — and `is:recording` was a parse
    /// error before the flag joined the closed set.
    #[test]
    fn is_recording_is_a_flag_a_space_can_name() {
        let mut note = entry("recordings/standup.md");
        note.flags = vec!["recording".to_owned()];
        assert!(hit("is:recording", &note));
        assert!(!hit("is:recording", &entry("groceries.md")));
    }

    #[test]
    fn text_and_barewords_search_title_then_body() {
        let mut e = entry("notes/meeting.md");
        e.title = "Weekly sync".to_owned();
        assert!(hit_with_body("weekly", &e, ""), "a bareword is text:");
        assert!(hit_with_body("text:WEEKLY", &e, ""), "folded");
        assert!(hit_with_body("text:cadence", &e, "on cadence"), "body too");
        assert!(!hit_with_body("text:absent", &e, "on cadence"));
        assert!(
            hit_with_body("text:\"two words\"", &e, "has two words in it"),
            "quotes hold a phrase together"
        );
    }

    #[test]
    fn a_query_with_no_text_term_never_touches_the_body() {
        let mut e = entry("a.md");
        e.tags = vec!["x".to_owned()];
        let q = parse("tag:x is:untagged | path:*.md").expect("parses");
        assert!(!needs_body(&q));
        let mut calls = 0usize;
        let mut body = || {
            calls += 1;
            String::new()
        };
        assert!(eval(&q, &e, &mut body, NOW));
        assert_eq!(calls, 0, "no body read for an index-only query");
    }

    #[test]
    fn index_only_terms_run_before_text_inside_a_conjunction() {
        // The reordering contract, observed the only way that matters: the body
        // closure is never called for an entry the cheap term already rejected,
        // whichever order the author wrote the terms in.
        let e = entry("a.md");
        for query in ["text:anything tag:absent", "tag:absent text:anything"] {
            let q = parse(query).expect("parses");
            assert!(needs_body(&q), "`{query}` has a text term");
            let mut calls = 0usize;
            let mut body = || {
                calls += 1;
                "anything".to_owned()
            };
            assert!(!eval(&q, &e, &mut body, NOW));
            assert_eq!(calls, 0, "`{query}` read a body it did not need");
        }
        // And when the cheap term passes, the body is read exactly once.
        let mut tagged = entry("b.md");
        tagged.tags = vec!["present".to_owned()];
        let q = parse("text:anything tag:present").expect("parses");
        let mut calls = 0usize;
        let mut body = || {
            calls += 1;
            "anything".to_owned()
        };
        assert!(eval(&q, &tagged, &mut body, NOW));
        assert_eq!(calls, 1);
    }

    #[test]
    fn juxtaposition_binds_tighter_than_pipe_and_minus_takes_one_term() {
        // `a b | c` is `(a AND b) OR c`.
        let q = parse("tag:a tag:b | tag:c").expect("parses");
        match &q.root {
            Node::Or(parts) => {
                assert_eq!(parts.len(), 2, "two disjuncts");
                assert!(matches!(&parts[0], Node::And(kids) if kids.len() == 2));
                assert!(matches!(&parts[1], Node::Pred(Pred::Tag { .. })));
            }
            other => panic!("expected a disjunction, got {other:?}"),
        }

        let mut only_c = entry("a.md");
        only_c.tags = vec!["c".to_owned()];
        assert!(hit("tag:a tag:b | tag:c", &only_c));
        let mut only_a = entry("b.md");
        only_a.tags = vec!["a".to_owned()];
        assert!(!hit("tag:a tag:b | tag:c", &only_a));

        // `-` negates exactly the following term, not the rest of the line.
        assert!(hit("-tag:b tag:a", &only_a));
        assert!(!hit("-tag:a tag:a", &only_a));
        // A standalone `-` negates a whole group.
        assert!(!hit("- (tag:a)", &only_a));
        assert!(hit("- (tag:b)", &only_a));
    }

    #[test]
    fn parentheses_regroup() {
        let mut e = entry("a.md");
        e.tags = vec!["a".to_owned(), "c".to_owned()];
        assert!(hit("tag:a (tag:b | tag:c)", &e));
        e.tags = vec!["a".to_owned()];
        assert!(!hit("tag:a (tag:b | tag:c)", &e));
    }

    #[test]
    fn a_broken_query_is_a_located_error_and_therefore_matches_nothing() {
        let unbalanced = error("tag:a (tag:b");
        assert!(
            unbalanced.message.contains("parenthesis"),
            "{}",
            unbalanced.message
        );
        assert_eq!(unbalanced.token_index, 3, "points past the last token");
        assert_eq!(unbalanced.byte_span, (12, 12), "an end-of-input span");

        let stray = error("tag:a )");
        assert!(stray.message.contains("parenthesis"), "{}", stray.message);
        assert_eq!(stray.token_index, 1);
        assert_eq!(stray.byte_span, (6, 7));

        let unknown_key = error("tag:a colour:red");
        assert!(
            unknown_key.message.contains("unknown search key `colour`"),
            "{}",
            unknown_key.message
        );
        assert_eq!(unknown_key.token_index, 1);
        assert_eq!(unknown_key.byte_span, (6, 16));

        let unknown_flag = error("is:sparkly");
        assert!(
            unknown_flag.message.contains("unknown flag `sparkly`"),
            "{}",
            unknown_flag.message
        );
        assert_eq!(unknown_flag.byte_span, (0, 10));

        // The whole point: none of these produced a Query, so nothing matches.
        for broken in ["tag:a (tag:b", "tag:a )", "colour:red", "is:sparkly", "   "] {
            assert!(parse(broken).is_err(), "`{broken}` must not parse");
        }
    }

    #[test]
    fn other_malformed_values_are_located_errors_too() {
        assert!(error("date:whenever>=today")
            .message
            .contains("unknown field"));
        assert!(error("date:modified>=soon")
            .message
            .contains("malformed date"));
        assert!(error("date:modified>=2026-02-30")
            .message
            .contains("malformed date"));
        assert!(error("date:modified").message.contains("comparison"));
        assert!(error("origin:somewhere").message.contains("unknown value"));
        assert!(error("field:").message.contains("expected a key"));
        assert!(error("field:=x").message.contains("before the comparison"));
        assert!(error("path:[unclosed").message.contains("invalid glob"));
    }

    #[test]
    fn depth_and_token_caps_reject_rather_than_recurse() {
        let at_cap = format!("{}tag:a{}", "(".repeat(MAX_DEPTH), ")".repeat(MAX_DEPTH));
        assert!(parse(&at_cap).is_ok(), "exactly at the cap is fine");

        let too_deep = format!(
            "{}tag:a{}",
            "(".repeat(MAX_DEPTH + 1),
            ")".repeat(MAX_DEPTH + 1)
        );
        let err = error(&too_deep);
        assert!(err.message.contains("nests deeper"), "{}", err.message);
        assert_eq!(err.token_index, MAX_DEPTH, "points at the offending `(`");

        let too_many = vec!["tag:a"; MAX_TOKENS + 1].join(" ");
        let err = error(&too_many);
        assert!(err.message.contains("the limit is"), "{}", err.message);
        assert_eq!(err.token_index, MAX_TOKENS);
        assert!(
            parse(&vec!["tag:a"; MAX_TOKENS].join(" ")).is_ok(),
            "exactly at the cap is fine"
        );
    }

    #[test]
    fn quoting_holds_colons_and_spaces_together() {
        let mut e = entry("a.md");
        e.title = "at 12:30 sharp".to_owned();
        // A quoted token with a colon is a bareword, not an unknown key.
        assert!(hit_with_body("\"12:30\"", &e, ""));
        assert!(hit_with_body("text:\"12:30\"", &e, ""));
        // An escaped quote is a literal quote.
        let mut quotey = entry("b.md");
        quotey.title = "he said \"hi\"".to_owned();
        assert!(hit_with_body("text:\\\"hi\\\"", &quotey, ""));
    }

    #[test]
    fn link_matches_outbound_and_backlink_needs_binding() {
        let mut source = entry("notes/source.md");
        source.title = "Source".to_owned();
        source.links = vec!["Vault as a Lens".to_owned()];
        let mut target = entry("notes/vault.md");
        target.title = "Vault as a Lens".to_owned();

        // Unbound, `link:` still matches a literal target…
        assert!(hit("link:\"Vault as a Lens\"", &source));
        // …but `backlink:` cannot know inbound links from one entry, so it
        // matches nothing rather than guessing.
        assert!(!hit("backlink:Source", &target));

        let builder = IndexBuilder::from_entries(vec![source.clone(), target.clone()]);
        let index = builder.snapshot();
        let mut bound = parse("backlink:Source").expect("parses");
        bind_index(&mut bound, &index);
        let mut body = String::new;
        assert!(eval(&bound, &target, &mut body, NOW), "now it resolves");
        assert!(!eval(&bound, &source, &mut body, NOW));

        // Binding also upgrades `link:` from a literal to a real resolution: the
        // body wrote a title, the query names a path.
        let mut by_path = parse("link:notes/vault.md").expect("parses");
        assert!(
            !eval(&by_path, &source, &mut body, NOW),
            "unbound is literal"
        );
        bind_index(&mut by_path, &index);
        assert!(eval(&by_path, &source, &mut body, NOW));
    }

    #[test]
    fn a_rebound_query_follows_the_index_rather_than_going_stale() {
        let mut source = entry("notes/source.md");
        source.links = vec!["notes/target.md".to_owned()];
        let target = entry("notes/target.md");
        let mut builder = IndexBuilder::from_entries(vec![source.clone(), target.clone()]);

        let mut q = parse("backlink:notes/source.md").expect("parses");
        bind_index(&mut q, &builder.snapshot());
        let mut body = String::new;
        assert!(eval(&q, &target, &mut body, NOW));

        // The source drops the link; a rebind must forget it.
        let mut unlinked = source.clone();
        unlinked.links.clear();
        builder.apply(NoteDelta::Upsert(Box::new(unlinked)));
        bind_index(&mut q, &builder.snapshot());
        assert!(!eval(&q, &target, &mut body, NOW));
    }

    #[test]
    fn the_worked_example_from_the_architecture_parses_and_selects() {
        let query = "tag:project/keeper -tag:archive (field:status=open | field:status=review) \
                     date:modified>=-14d";
        let q = parse(query).expect("the documented example parses");
        assert!(!needs_body(&q));

        let mut wanted = entry("notes/a.md");
        wanted.tags = vec!["project/keeper".to_owned()];
        wanted
            .fields
            .insert("status".to_owned(), "review".to_owned());
        wanted.updated_ms = NOW - DAY_MS;
        assert!(hit(query, &wanted));

        let mut archived = wanted.clone();
        archived.tags.push("archive".to_owned());
        assert!(!hit(query, &archived));

        let mut stale = wanted.clone();
        stale.updated_ms = NOW - 40 * DAY_MS;
        assert!(!hit(query, &stale));

        let mut closed = wanted.clone();
        closed
            .fields
            .insert("status".to_owned(), "closed".to_owned());
        assert!(!hit(query, &closed));
    }

    #[test]
    fn civil_date_arithmetic_round_trips_and_clamps() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        for days in [-10_000, -1, 0, 1, 19_000, 30_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "round trip at {days}");
        }
        // One month before the 31st is the last day of a short month, not a skid
        // into the following one.
        let march31 = days_from_civil(2026, 3, 31) * DAY_MS;
        assert_eq!(
            civil_from_days(months_before(march31, 1).div_euclid(DAY_MS)),
            (2026, 2, 28)
        );
        assert_eq!(
            civil_from_days(months_before(march31, 12).div_euclid(DAY_MS)),
            (2025, 3, 31)
        );
    }

    // -----------------------------------------------------------------------
    // Decomposition (Story 43.4)
    // -----------------------------------------------------------------------

    /// A decomposed query, flattened for assertion: tag terms, `is:` flags,
    /// `origin:` and the free-text needle.
    type Chips = (
        Vec<(String, NoteTagTerm)>,
        Vec<String>,
        Option<String>,
        Option<String>,
    );

    /// The chip set a query decomposes into, or a panic naming what stopped it.
    fn chips(query: &str) -> Chips {
        match decompose(query).unwrap_or_else(|err| panic!("parse `{query}`: {}", err.message)) {
            NoteSpaceTermsVm::Chips {
                tags,
                flags,
                origin,
                text,
            } => (
                tags.into_iter().map(|t| (t.tag, t.term)).collect(),
                flags,
                origin,
                text,
            ),
            NoteSpaceTermsVm::Unrepresentable { terms } => {
                panic!("`{query}` should be chips, not {terms:?}")
            }
        }
    }

    /// The terms a query refuses to give up, or a panic when it gave them all up.
    fn refused(query: &str) -> Vec<String> {
        match decompose(query).unwrap_or_else(|err| panic!("parse `{query}`: {}", err.message)) {
            NoteSpaceTermsVm::Unrepresentable { terms } => terms,
            NoteSpaceTermsVm::Chips { .. } => panic!("`{query}` should not be chips"),
        }
    }

    #[test]
    fn a_query_the_chips_can_hold_comes_back_as_chips_in_the_order_it_was_written() {
        let (tags, flags, origin, text) =
            chips("tag:client/acme -tag:draft is:pinned origin:agent text:\"quarterly review\"");
        assert_eq!(
            tags,
            vec![
                ("client/acme".to_owned(), NoteTagTerm::Include),
                ("draft".to_owned(), NoteTagTerm::Exclude),
            ]
        );
        assert_eq!(flags, vec!["pinned".to_owned()]);
        assert_eq!(origin.as_deref(), Some("agent"));
        assert_eq!(text.as_deref(), Some("quarterly review"));
    }

    #[test]
    fn a_tag_is_read_back_through_the_one_vocabulary() {
        let (tags, ..) = chips("tag:#Client/Acme");
        assert_eq!(tags, vec![("client/acme".to_owned(), NoteTagTerm::Include)]);
    }

    /// Everything the bar itself writes has to survive the trip back, or editing
    /// a space keeper saved would be the first thing to lose a term.
    #[test]
    fn every_query_the_filter_bar_writes_decomposes_into_chips() {
        for query in [
            "tag:a",
            "-tag:a",
            "tag:a -tag:b",
            "is:pinned",
            "tag:a is:pinned is:journal",
            "origin:agent",
            "text:\"two words\"",
            "tag:a -tag:b is:pinned origin:agent text:\"two words\"",
        ] {
            let _ = chips(query);
        }
    }

    #[test]
    fn a_grouped_or_disjoint_query_is_refused_whole_because_a_group_is_not_a_term() {
        assert_eq!(refused("tag:a | tag:b"), vec!["tag:a | tag:b".to_owned()]);
        assert_eq!(
            refused("tag:a (tag:b | tag:c)"),
            vec!["tag:a (tag:b | tag:c)".to_owned()]
        );
        assert_eq!(refused("-(tag:a tag:b)"), vec!["-(tag:a tag:b)".to_owned()]);
    }

    #[test]
    fn a_term_outside_the_chip_vocabulary_is_named_verbatim() {
        assert_eq!(
            refused("tag:a date:modified>=-14d"),
            vec!["date:modified>=-14d".to_owned()]
        );
        assert_eq!(
            refused("path:journal/**"),
            vec!["path:journal/**".to_owned()]
        );
        assert_eq!(
            refused("field:priority=high"),
            vec!["field:priority=high".to_owned()]
        );
        assert_eq!(
            refused("link:notes/a.md"),
            vec!["link:notes/a.md".to_owned()]
        );
        assert_eq!(
            refused("backlink:notes/a.md"),
            vec!["backlink:notes/a.md".to_owned()]
        );
    }

    /// Every refusal in one query, which is the shape the byte-identity
    /// guarantee has to hold for.
    #[test]
    fn a_query_of_nothing_but_refusals_names_every_one_of_them() {
        assert_eq!(
            refused(
                "path:a/** field:x=1 date:created<today link:b.md backlink:c.md tag:d/* -is:pinned"
            ),
            vec![
                "path:a/**".to_owned(),
                "field:x=1".to_owned(),
                "date:created<today".to_owned(),
                "link:b.md".to_owned(),
                "backlink:c.md".to_owned(),
                "tag:d/*".to_owned(),
                "-is:pinned".to_owned(),
            ]
        );
    }

    /// The exact query `space-editor.test.tsx` round-trips for byte identity.
    /// The two halves of that guarantee live in two languages, so they name the
    /// same string: if this ever decomposed into chips, the editor would start
    /// re-emitting it and the TypeScript test would be asserting against a case
    /// that no longer exists.
    #[test]
    fn the_editors_worked_lossy_example_is_refused_whole() {
        let query = concat!(
            "tag:client/acme (tag:urgent | tag:blocked) path:journal/** ",
            "field:priority=high date:modified>=-14d -(tag:done tag:archive) tag:client/*"
        );
        assert_eq!(refused(query), vec![query.to_owned()]);
    }

    /// The flat companion of the case above, also shared with
    /// `space-editor.test.tsx`: no grouping, so each refused term is named on
    /// its own rather than the query being refused whole.
    #[test]
    fn the_editors_worked_flat_lossy_example_names_its_two_refused_terms() {
        assert_eq!(
            refused("tag:client/acme path:journal/** date:modified>=-14d"),
            vec![
                "path:journal/**".to_owned(),
                "date:modified>=-14d".to_owned()
            ]
        );
    }

    /// `tag:x/*` is the subtree without its own node. No chip state spells that,
    /// and pretending it is `tag:x` would widen the space by one node's notes.
    #[test]
    fn the_descendants_only_tag_form_is_refused_rather_than_flattened() {
        assert_eq!(refused("tag:client/*"), vec!["tag:client/*".to_owned()]);
    }

    /// The DSL lets `tag:---` match nothing. A chip carrying it would be a
    /// control with nothing written on it, and saving the bar would drop it.
    #[test]
    fn a_tag_term_that_names_no_tag_is_refused() {
        assert_eq!(refused("tag:---"), vec!["tag:---".to_owned()]);
    }

    /// One slot per tag is the whole of Story 43.3's guarantee, so a query that
    /// names one tag twice cannot become chips without one of them vanishing.
    #[test]
    fn a_tag_named_twice_is_refused_because_the_bar_has_one_slot_per_tag() {
        assert_eq!(refused("tag:a tag:a"), vec!["tag:a".to_owned()]);
        assert_eq!(refused("tag:a -tag:a"), vec!["-tag:a".to_owned()]);
        // Two spellings of one tag collide after normalisation, exactly as they
        // do in the chip bar.
        assert_eq!(
            refused("tag:Draft -tag:draft"),
            vec!["-tag:draft".to_owned()]
        );
    }

    #[test]
    fn a_second_free_text_term_is_refused_because_the_bar_has_one_search_field() {
        assert_eq!(refused("text:one text:two"), vec!["text:two".to_owned()]);
        assert_eq!(refused("one two"), vec!["two".to_owned()]);
    }

    #[test]
    fn a_flag_named_twice_under_two_spellings_is_refused_once() {
        assert_eq!(refused("is:pinned is:Pinned"), vec!["is:Pinned".to_owned()]);
    }

    /// A negated anything-but-a-tag has no control. Reported as one term with
    /// its `-`, because `-` alone reads like a typo rather than like negation.
    #[test]
    fn negation_of_a_term_that_is_not_a_tag_is_refused_with_its_sign() {
        assert_eq!(refused("-is:pinned"), vec!["-is:pinned".to_owned()]);
        assert_eq!(refused("-origin:agent"), vec!["-origin:agent".to_owned()]);
        assert_eq!(refused("-text:draft"), vec!["-text:draft".to_owned()]);
        assert_eq!(refused("- tag:a"), vec!["- tag:a".to_owned()]);
    }

    /// A broken space already says so on its row. Handing its editor an empty
    /// chip set would put it one Save away from selecting the whole vault.
    #[test]
    fn a_query_that_does_not_parse_decomposes_into_an_error_not_an_empty_chip_set() {
        assert!(decompose("nope:x").is_err());
        assert!(decompose("(tag:a").is_err());
        assert!(decompose("").is_err());
        assert!(decompose("   ").is_err());
    }
}
