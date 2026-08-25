//! Link extraction (FR-108, FR-109, FR-110).
//!
//! Four shapes, all of which a real Obsidian vault contains: `[[target]]`,
//! `[[target|alias]]`, `![[embed]]` and the markdown `[text](rel/path)`. What
//! comes out is *raw* — the text the author wrote, not a resolved note. Turning
//! a target into a note is the index's job (filename stem, then alias, then
//! path), and it needs the whole vault to do it; this module needs only the
//! body.
//!
//! Code regions are skipped through the same scanner tags uses, so a wikilink
//! inside a fenced block is documentation about wikilinks rather than a link.
//! External URLs are skipped too: `[docs](https://example.com)` is not an edge
//! in the vault graph, and putting it in the backlink panel would be noise.

use percent_encoding::percent_decode_str;

use crate::notes::tags::{code_spans, in_code};

/// One link exactly as the author wrote it.
#[derive(Debug, Clone, PartialEq)]
pub struct RawLink {
    /// The note title, alias or vault-relative path being pointed at. Any
    /// `#heading` or `#^block` anchor has been removed — the anchor selects a
    /// place *within* the target and does not change which note it is.
    pub target: String,
    /// `[[target|alias]]`, or the link text of a markdown link.
    pub alias: Option<String>,
    /// `![[…]]` or `![…](…)`: render the target inline rather than link to it.
    pub embed: bool,
    /// Byte range of the whole link syntax in the body, `!` included — and the
    /// attribute block too, when there is one, because a renderer that hides
    /// the syntax has to hide all of it.
    pub span: (usize, usize),
    /// The attribute block written after the link, as written: `{reference="x"}`
    /// gives `[("reference", "x")]`, and `{ :type="Metric" }` gives
    /// `[(":type", "Metric")]` — the key keeps its colon, because that colon is
    /// the only thing that separates a semantic pair from a presentational
    /// `class="…"`, and `attrs` is the raw record. What a key *means* is the
    /// projection's decision, not this parser's.
    ///
    /// The convention is kramdown's inline attribute list, which Pandoc and
    /// Obsidian both leave alone — which is what makes it usable here: a vault
    /// carrying these still opens in an editor that has never heard of them,
    /// and the attributes read as text rather than breaking the link.
    ///
    /// Empty for the overwhelming majority of links. A pair whose value is not
    /// quoted is dropped rather than guessed at — `{reference=aaa bbb}` has no
    /// reading that is obviously right, and a wrong predicate is worse than an
    /// absent one in a graph somebody queries.
    pub attrs: Vec<(String, String)>,
    /// Every predicate name the link's attribute block(s) announced, in true
    /// written order, with exact duplicates dropped: `{schema:creator}` gives
    /// `["schema:creator"]`, and `{ :depends_on }`, `{ depends_on }` and
    /// `{ :depends_on="soon" }` all give `["depends_on"]`.
    ///
    /// Three spellings, one meaning, because kramdown and the Semantic Markdown
    /// Spec V0 both allow all three and one vault will contain all three:
    /// `prefix:local` is a CURIE; `:local` is the same name in the document's
    /// *default* vocabulary, and the empty prefix's colon is stripped so that
    /// spelling and the bare one are one string rather than two; a bare `local`
    /// — no `=`, no leading `.` or `#` — is V0's rule that an attribute which
    /// is neither a class, an id nor a pair is a property name.
    ///
    /// No vocabulary is consulted and no prefix is resolved here, the empty one
    /// least of all: the prefix map lives in the note's frontmatter and the
    /// empty prefix's base in the drive's registry, and a reader that needed
    /// either could not read a link in isolation.
    ///
    /// A colon is a syntactic marker, so a colon-keyed pair announces itself as
    /// a predicate by its own shape, and recording its key here is recording
    /// rather than deciding — and it is the only way written order survives,
    /// order being a fact about the document: `attrs` and `predicates` are two
    /// vectors, so `{ cites, :type="x" }` can only be known to read
    /// `["cites", "type"]` by the tokeniser that walked the block. Deciding
    /// which *bare* keys are load-bearing — `rel` is an edge, `class` is not —
    /// is vocabulary policy and lives in `notes::index` alone, so a bare-key
    /// pair contributes nothing here.
    ///
    /// Names only, never objects. `{ :type="Metric" }` gives `["type"]` and
    /// nothing else; the literal `Metric` is already in `attrs` under `":type"`,
    /// and carrying it here as well would give two consumers two places to
    /// disagree about one token.
    ///
    /// A token with no single obvious reading is dropped, never repaired: `{a:}`
    /// and `{a:b:c}` have an empty or an ambiguous half, and a wrong edge in a
    /// graph somebody queries is worse than an absent one. `{:b}` used to be on
    /// that list for being readable "several ways", which was wrong — kramdown
    /// and V0 agree it is the single name `b`, and dropping it lost the
    /// commonest spelling in the vaults this exists to read.
    pub predicates: Vec<String>,
}

/// What the attribute block or blocks after a link said.
#[derive(Debug, Clone, PartialEq)]
pub struct AttrBlocks {
    /// `key="value"` pairs, in order, with each key exactly as written —
    /// `":type"` keeps its colon.
    pub attrs: Vec<(String, String)>,
    /// Predicate names, in written order, exact duplicates dropped: the bare and
    /// colon-marked tokens, and the keys of colon-keyed pairs.
    pub predicates: Vec<String>,
    /// The byte just past the closing brace of the *last* block consumed.
    pub end: usize,
}

impl AttrBlocks {
    /// What a link with no block at all has: nothing, ending where the link
    /// itself ended.
    fn none_at(end: usize) -> Self {
        Self {
            attrs: Vec::new(),
            predicates: Vec::new(),
            end,
        }
    }
}

/// Read the attribute block starting at `at`, and every block written directly
/// against it, merging them in order: `{dcterms:source}{schema:status}` reads
/// the same as `{dcterms:source, schema:status}`.
///
/// A block holds `key="value"` pairs and bare predicate tokens in any mixture,
/// separated by commas, whitespace, or both. Returns `None` when there is no
/// block, otherwise the contents and the byte just past the last closing brace
/// — which is what keeps `RawLink::span` covering the whole syntax when an
/// author wrote more than one block.
///
/// The first block has to begin immediately after the link, or immediately
/// after the emphasis markers that close against it — `**[x](y)**{ :p }`, see
/// `emphasis_close`. One space is not allowed, because `[a](b) {this}` is a
/// sentence with braces in it and `[a](b){this}` is a link with attributes, and
/// only the author knows which they meant. The same rule holds between blocks.
///
/// Nothing here looks at what precedes `at` beyond that emphasis run, so the
/// same reader serves the tail of a fenced block's info string (```` ```json
/// { :type="Metric" } ````): pass the offset of the `{`.
pub fn read_attrs(body: &str, at: usize) -> Option<AttrBlocks> {
    let mut attrs = Vec::new();
    let mut predicates = Vec::new();
    let mut end = None;
    let mut cursor = emphasis_close(body, at).unwrap_or(at);

    while body.as_bytes().get(cursor) == Some(&b'{') {
        // A block never wraps: an unclosed brace is prose, and stopping here
        // leaves the span where the previous block ended.
        let limit = line_limit(body, cursor);
        let Some(close) = body[cursor..limit].find('}').map(|i| i + cursor) else {
            break;
        };
        for_each_token(&body[cursor + 1..close], |token| {
            if let Some((key, value)) = read_pair(token) {
                // A colon-marked key is a predicate carrying a literal object.
                // Only the name is recorded: `attrs` below keeps the object
                // under the key as written, and a bare key (`rel`, `class`) has
                // no colon and so contributes no predicate here — what `rel`
                // means is the projection's decision.
                if let Some(name) = pair_predicate(&key) {
                    push_unique(&mut predicates, name);
                }
                attrs.push((key, value));
            } else if let Some(name) = predicate_name(token) {
                push_unique(&mut predicates, name);
            }
        });
        cursor = close + 1;
        end = Some(cursor);
    }

    end.map(|end| AttrBlocks {
        attrs,
        predicates,
        end,
    })
}

/// Where the attribute block begins when the author closed emphasis between the
/// link and the block: `**[x](y)**{ :p }`, which is how the owner's documents
/// mark up a link they also want to shout about. `at` is the byte just past the
/// link, so the markers inside the link text (`[**x**](y)`) are never in view.
///
/// `None` unless this really is a closing run with a block against it: a lone
/// `**` ending a sentence after a link keeps its own bytes rather than
/// disappearing into the link's span, and `**[x](y)** {p}` stays prose by the
/// same one-space rule as everything else here.
fn emphasis_close(body: &str, at: usize) -> Option<usize> {
    let marker = match body.as_bytes().get(at) {
        Some(b'*') => b'*',
        Some(b'_') => b'_',
        _ => return None,
    };
    let run = body[at..].bytes().take_while(|b| *b == marker).count();
    // `***x***` is the longest run markdown gives a meaning to; anything longer
    // is a rule or a typo, and swallowing it into the link's span would delete
    // characters the author can see.
    if run > 3 {
        return None;
    }
    // A short-circuit rather than a rule — with no block against the run the
    // answer is `None` either way — which keeps the backwards scan below off
    // every ordinary emphasised link.
    if body.as_bytes().get(at + run) != Some(&b'{') {
        return None;
    }
    // The run has to close something. Without this, prose like
    // `see [note](n.md)**{ :p }` would have its stray asterisks hidden by a
    // renderer that trusts the span. Full delimiter matching is CommonMark's
    // whole emphasis algorithm; the opener being somewhere earlier on the line
    // is the cheap half of it, and it is the half that rejects the typo.
    let line = line_start(body, at);
    body[line..at]
        .contains(&body[at..at + run])
        .then_some(at + run)
}

/// Split a block's contents into tokens on commas and whitespace, except
/// inside quotes: `{rel="see also", schema:creator}` is two tokens, not three.
fn for_each_token(inner: &str, mut visit: impl FnMut(&str)) {
    let mut quote: Option<char> = None;
    let mut start = 0usize;
    for (i, c) in inner.char_indices() {
        match quote {
            Some(open) => {
                if c == open {
                    quote = None;
                }
            }
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == ',' || c.is_whitespace() => {
                if start < i {
                    visit(&inner[start..i]);
                }
                start = i + c.len_utf8();
            }
            None => {}
        }
    }
    if start < inner.len() {
        visit(&inner[start..]);
    }
}

/// `key="value"`, or `None` if the token is not one. The key is returned
/// exactly as written, colon and all: `:type` stays `":type"`, because that
/// colon is what tells a consumer a semantic pair from a `class="…"`, and this
/// function's job is to record rather than to interpret.
///
/// An unquoted value is not a pair: `{reference=aaa bbb}` has no reading that
/// is obviously right.
fn read_pair(token: &str) -> Option<(String, String)> {
    let (key, value) = token.split_once('=')?;
    let key = key.trim();
    let value = value.trim();
    let unquoted = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })?;
    if key.is_empty() || unquoted.is_empty() {
        return None;
    }
    Some((key.to_owned(), unquoted.to_owned()))
}

/// The predicate a bare token names, or `None` when it names nothing certain.
///
/// - `prefix:local` — a CURIE, kept as written.
/// - `:local` — the empty prefix, which is the document's default vocabulary.
///   The colon is stripped so this and the bare form are one string: keeper
///   displays a name and never invents a base, and resolving the empty prefix
///   needs the note's own `prefixes:` or the drive's registry, neither of which
///   a link can see from here.
/// - `local` — Semantic Markdown V0: an attribute that is not a class, not an
///   id and not a pair is a property name.
///
/// Everything else is dropped rather than repaired. `a:b:c` and `a:` have an
/// ambiguous or empty half; `.class`, `#id` and the lone `:` of kramdown's
/// `{: .foo}` marker fall out of `is_name`'s letter-first rule without needing
/// a case of their own, which is why there is not one.
pub fn predicate_name(token: &str) -> Option<&str> {
    match token.split_once(':') {
        Some(("", local)) => is_name(local).then_some(local),
        Some((prefix, local)) => (is_name(prefix) && is_name(local)).then_some(token),
        None => is_name(token).then_some(token),
    }
}

/// The predicate a `key="value"` pair announces, or `None` when its key is
/// presentational. The colon is the whole test, and it is a test of syntax
/// rather than of vocabulary: `:type` and `dc:title` are predicates carrying a
/// literal object, while `class`, `id`, `width` — and the legacy
/// `rel`/`reference`, whose *value* is the predicate name and which are edges
/// only because a vocabulary says so — are not. Which bare keys are
/// load-bearing is `notes::index`'s decision, which is why this is private.
fn pair_predicate(key: &str) -> Option<&str> {
    key.contains(':').then(|| predicate_name(key)).flatten()
}

/// Append unless it is already there. Written order wins, so a copy-paste that
/// left `{ :a, a }` behind draws one edge rather than two.
fn push_unique(into: &mut Vec<String>, name: &str) {
    if !into.iter().any(|had| had == name) {
        into.push(name.to_owned());
    }
}

/// A predicate name half: a letter, then letters, digits, `_` or `-`.
/// Deliberately narrower than XML's NCName — the extra characters buy nothing
/// here and each one widens what gets mistaken for a predicate.
fn is_name(part: &str) -> bool {
    let mut chars = part.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// Every link in a body, in document order.
pub fn extract(body: &str) -> Vec<RawLink> {
    let code = code_spans(body);
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut at = 0usize;

    while at < bytes.len() {
        let (start, embed) = match bytes[at] {
            b'!' if at + 1 < bytes.len() && bytes[at + 1] == b'[' => (at + 1, true),
            b'[' => (at, false),
            _ => {
                at += 1;
                continue;
            }
        };

        if in_code(&code, at) || is_escaped(body, at) {
            at += 1;
            continue;
        }

        let parsed = if body[start..].starts_with("[[") {
            wikilink(body, start, at, embed)
        } else {
            markdown_link(body, start, at, embed)
        };

        match parsed {
            Some(link) => {
                at = link.span.1;
                out.push(link);
            }
            None => at += 1,
        }
    }

    out
}

/// Whether the character at `at` is preceded by an odd number of backslashes,
/// which is what markdown means by escaped.
fn is_escaped(body: &str, at: usize) -> bool {
    body[..at].bytes().rev().take_while(|b| *b == b'\\').count() % 2 == 1
}

/// `[[target]]`, `[[target|alias]]`. `start` is the first `[`; `outer` is where
/// the whole link begins, which for an embed is the `!` one byte earlier.
fn wikilink(body: &str, start: usize, outer: usize, embed: bool) -> Option<RawLink> {
    let inner_start = start + 2;
    // A wikilink never wraps: bounding the search to the line stops one stray
    // `[[` from consuming half the document.
    let limit = line_limit(body, inner_start);
    let close = body[inner_start..limit].find("]]")? + inner_start;
    let inner = &body[inner_start..close];

    let (target_raw, alias) = match inner.split_once('|') {
        Some((t, a)) => (t, Some(a.trim().to_owned())),
        None => (inner, None),
    };
    let target = strip_anchor(target_raw).trim().to_owned();
    if target.is_empty() {
        // `[[#heading]]` points inside this very note; there is no edge to draw.
        return None;
    }

    // A wikilink can carry them too. Obsidian writes none, but a vault that
    // adopts the convention should not have to remember which of its two link
    // syntaxes it applies to, and `[[belief]]{skos:related}` is as legitimate
    // an edge label as the markdown form.
    let blocks = read_attrs(body, close + 2).unwrap_or_else(|| AttrBlocks::none_at(close + 2));
    Some(RawLink {
        target,
        alias: alias.filter(|a| !a.is_empty()),
        embed,
        span: (outer, blocks.end),
        attrs: blocks.attrs,
        predicates: blocks.predicates,
    })
}

/// `[text](target)`, `![alt](target)`.
fn markdown_link(body: &str, start: usize, outer: usize, embed: bool) -> Option<RawLink> {
    let limit = line_limit(body, start);
    let text_end = body[start + 1..limit].find(']')? + start + 1;
    if body.as_bytes().get(text_end + 1) != Some(&b'(') {
        return None;
    }

    let dest_start = text_end + 2;
    let dest_end = closing_paren(body, dest_start, line_limit(body, dest_start))?;
    let mut dest = body[dest_start..dest_end].trim();

    // `(path "Title")` — the optional title is not part of the destination. The
    // quote must follow whitespace, or `it's notes.md` would lose its tail.
    if let Some(quote) = dest.find(['"', '\'']) {
        if quote > 0 && dest[..quote].ends_with(char::is_whitespace) {
            dest = dest[..quote].trim_end();
        }
    }
    // `(<path with spaces>)`
    dest = dest.trim_start_matches('<').trim_end_matches('>');

    if dest.is_empty() || is_external(dest) {
        return None;
    }

    let target = strip_anchor(dest);
    if target.is_empty() {
        return None;
    }
    // Obsidian percent-encodes spaces and non-ASCII in markdown-style links, so
    // the raw target has to be decoded before it can match a path on disk.
    let target = percent_decode_str(target).decode_utf8_lossy().into_owned();

    let text = body[start + 1..text_end].trim();
    // Attributes come after the destination, with no space between: see
    // `read_attrs`. The span grows to cover them — every block of them — because
    // a renderer that hides the link syntax has to hide the attributes too or
    // they are left on screen as loose braces.
    let blocks =
        read_attrs(body, dest_end + 1).unwrap_or_else(|| AttrBlocks::none_at(dest_end + 1));
    Some(RawLink {
        target,
        alias: (!text.is_empty()).then(|| text.to_owned()),
        embed,
        span: (outer, blocks.end),
        attrs: blocks.attrs,
        predicates: blocks.predicates,
    })
}

/// End of the line containing `at`, which is as far as a link may reach.
fn line_limit(body: &str, at: usize) -> usize {
    body[at.min(body.len())..]
        .find('\n')
        .map_or(body.len(), |i| at + i)
}

/// Start of the line containing `at`, which is as far back as an emphasis
/// opener may be looked for.
fn line_start(body: &str, at: usize) -> usize {
    body[..at.min(body.len())].rfind('\n').map_or(0, |i| i + 1)
}

/// Match the `)` that closes a destination, tolerating one level of balanced
/// parentheses — Wikipedia paths are full of them.
fn closing_paren(body: &str, from: usize, limit: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, c) in body[from..limit].char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(from + offset),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Drop a `#heading` or `#^block` anchor from a target.
fn strip_anchor(target: &str) -> &str {
    match target.find('#') {
        Some(i) => &target[..i],
        None => target,
    }
}

/// Whether a markdown destination points outside the vault. Anything with a
/// URL scheme does, and so does a bare in-page anchor.
fn is_external(dest: &str) -> bool {
    if dest.starts_with('#') {
        return true;
    }
    match dest.find(':') {
        Some(i) if i > 0 => dest[..i]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(body: &str) -> Vec<String> {
        extract(body).into_iter().map(|l| l.target).collect()
    }

    #[test]
    fn extracts_the_four_link_shapes() {
        let body = "See [[Vault as a lens]], [[Weekly review|the review]], ![[diagram.png]] and [notes](sub/other.md).";
        let links = extract(body);

        assert_eq!(links.len(), 4);
        assert_eq!(links[0].target, "Vault as a lens");
        assert_eq!(links[0].alias, None);
        assert!(!links[0].embed);

        assert_eq!(links[1].target, "Weekly review");
        assert_eq!(links[1].alias.as_deref(), Some("the review"));

        assert_eq!(links[2].target, "diagram.png");
        assert!(links[2].embed);

        assert_eq!(links[3].target, "sub/other.md");
        assert_eq!(links[3].alias.as_deref(), Some("notes"));
        assert!(!links[3].embed);
    }

    #[test]
    fn spans_cover_the_whole_syntax_including_the_embed_bang() {
        let body = "before ![[diagram.png]] after";
        let links = extract(body);
        let (s, e) = links[0].span;
        assert_eq!(&body[s..e], "![[diagram.png]]");
    }

    #[test]
    fn anchors_select_a_place_not_a_different_note() {
        assert_eq!(
            targets("[[Weekly review#Agenda]] and [[Notes#^abc123]] and [x](a/b.md#top)"),
            vec!["Weekly review", "Notes", "a/b.md"]
        );
        // A link into this same note is not an edge.
        assert!(extract("[[#Agenda]] and [top](#top)").is_empty());
    }

    #[test]
    fn skips_links_inside_code() {
        let body = "real [[One]]\n\n```md\n[[NotALink]]\n![[NorThis]]\n```\n\n`[[NorThis either]]` but [[Two]]\n";
        assert_eq!(targets(body), vec!["One", "Two"]);
    }

    #[test]
    fn skips_external_urls_but_keeps_relative_paths() {
        let body = "[a](https://example.com) [b](mailto:x@y.z) [c](attachments/img.png) [d](../sibling.md)";
        assert_eq!(targets(body), vec!["attachments/img.png", "../sibling.md"]);
    }

    #[test]
    fn decodes_percent_encoding_in_markdown_targets() {
        assert_eq!(
            targets("[x](attachments/my%20file%20(1).png)"),
            vec!["attachments/my file (1).png"]
        );
    }

    #[test]
    fn tolerates_a_title_and_angle_brackets_in_a_destination() {
        assert_eq!(
            targets("[x](sub/other.md \"A title\") [y](<sub/with space.md>)"),
            vec!["sub/other.md", "sub/with space.md"]
        );
    }

    #[test]
    fn an_escaped_bracket_is_not_a_link() {
        assert_eq!(targets(r"\[[Not a link]] but [[Real]]"), vec!["Real"]);
    }

    #[test]
    fn an_unterminated_wikilink_does_not_swallow_the_document() {
        let body = "[[open\nnext line [[Closed]]\n";
        assert_eq!(targets(body), vec!["Closed"]);
    }

    #[test]
    fn a_markdown_link_with_no_destination_is_not_a_link() {
        assert!(extract("[just brackets] and [empty]()").is_empty());
    }

    #[test]
    fn an_image_embed_is_marked_as_one() {
        let links = extract("![alt text](attachments/pic.png)");
        assert_eq!(links.len(), 1);
        assert!(links[0].embed);
        assert_eq!(links[0].alias.as_deref(), Some("alt text"));
    }

    #[test]
    fn adjacent_links_are_all_found() {
        assert_eq!(targets("[[A]][[B]][[C]]"), vec!["A", "B", "C"]);
    }
}

#[cfg(test)]
mod attribute_tests {
    use super::*;

    fn only(body: &str) -> RawLink {
        let found = extract(body);
        assert_eq!(found.len(), 1, "expected exactly one link in {body:?}");
        found.into_iter().next().expect("one link")
    }

    /// The shape that was asked for, on the syntax it was asked on.
    #[test]
    fn a_markdown_link_carries_its_predicate() {
        let link = only("see [Belief](notes/belief.md){reference=\"supports\"} for the argument");
        assert_eq!(link.target, "notes/belief.md");
        assert_eq!(
            link.attrs,
            vec![("reference".to_owned(), "supports".to_owned())]
        );
    }

    /// A wikilink too: a vault that adopts the convention should not have to
    /// remember which of its two link syntaxes it applies to.
    #[test]
    fn a_wikilink_carries_one_too() {
        let link = only("[[belief|Belief]]{reference=\"supports\"}");
        assert_eq!(
            link.attrs,
            vec![("reference".to_owned(), "supports".to_owned())]
        );
    }

    /// The span has to cover the braces. A renderer that hides the link syntax
    /// and stops at the destination leaves `{reference="supports"}` on screen.
    #[test]
    fn the_span_covers_the_attributes() {
        let body = "[Belief](notes/belief.md){reference=\"supports\"}";
        let link = only(body);
        assert_eq!(&body[link.span.0..link.span.1], body);
    }

    /// One space and it is prose. `[a](b) {draft}` is a sentence with braces in
    /// it; only the author knows, and the author says so by not typing a space.
    #[test]
    fn a_space_before_the_brace_makes_it_prose() {
        let body = "[Belief](notes/belief.md) {reference=\"supports\"}";
        let link = only(body);
        assert!(link.attrs.is_empty());
        // And the span stops at the link, so the braces stay on screen as the
        // prose they are.
        assert_eq!(&body[link.span.0..link.span.1], "[Belief](notes/belief.md)");
    }

    /// An unquoted value is dropped rather than guessed at: `{reference=a b}`
    /// has no reading that is obviously right, and a wrong predicate is worse
    /// than an absent one in a graph somebody queries.
    #[test]
    fn an_unquoted_value_is_not_a_predicate() {
        assert!(only("[Belief](notes/belief.md){reference=supports}")
            .attrs
            .is_empty());
    }

    /// Several, because the convention allows it and a reader who writes two
    /// should not silently lose one.
    #[test]
    fn several_pairs_all_come_through() {
        let link = only("[Belief](notes/belief.md){reference=\"supports\" strength=\"weak\"}");
        assert_eq!(
            link.attrs,
            vec![
                ("reference".to_owned(), "supports".to_owned()),
                ("strength".to_owned(), "weak".to_owned()),
            ]
        );
    }

    /// The ordinary link is the overwhelming majority and must not change.
    #[test]
    fn a_plain_link_has_no_attributes_and_the_same_span_as_before() {
        let body = "[Belief](notes/belief.md)";
        let link = only(body);
        assert!(link.attrs.is_empty());
        assert_eq!(link.span, (0, body.len()));
    }
}

#[cfg(test)]
mod predicate_tests {
    use super::*;

    fn only(body: &str) -> RawLink {
        let found = extract(body);
        assert_eq!(found.len(), 1, "expected exactly one link in {body:?}");
        found.into_iter().next().expect("one link")
    }

    fn predicates(body: &str) -> Vec<String> {
        only(body).predicates
    }

    /// The plainest form: one CURIE, no quotes, no equals sign.
    #[test]
    fn one_curie_is_read_as_a_predicate() {
        let link = only("[Belief](notes/belief.md){schema:creator}");
        assert_eq!(link.predicates, vec!["schema:creator".to_owned()]);
        // It is a predicate, not a pair; `attrs` must not have grown a member.
        assert!(link.attrs.is_empty());
    }

    /// Commas are what a writer reaches for, so commas have to work.
    #[test]
    fn comma_separated_predicates_all_come_through() {
        assert_eq!(
            predicates("[Belief](notes/belief.md){schema:creator, foaf:knows}"),
            vec!["schema:creator".to_owned(), "foaf:knows".to_owned()]
        );
    }

    /// Pandoc's own separator is whitespace, and the same vault will contain
    /// both spellings the week after the convention is announced.
    #[test]
    fn space_separated_predicates_all_come_through() {
        assert_eq!(
            predicates("[Belief](notes/belief.md){schema:creator foaf:knows}"),
            vec!["schema:creator".to_owned(), "foaf:knows".to_owned()]
        );
    }

    /// Two blocks written against each other are one list. The span has to
    /// cover both, or a renderer that hides the syntax leaves the second block
    /// on screen as loose braces.
    #[test]
    fn adjacent_blocks_merge_in_order_and_the_span_covers_them_all() {
        let body = "[Belief](notes/belief.md){dcterms:source}{schema:status}";
        let link = only(body);
        assert_eq!(
            link.predicates,
            vec!["dcterms:source".to_owned(), "schema:status".to_owned()]
        );
        assert_eq!(&body[link.span.0..link.span.1], body);
    }

    /// A gap between two blocks ends the run: `{a} {b}` is a block and then a
    /// sentence, by the same rule that makes `[a](b) {c}` prose.
    #[test]
    fn a_gap_between_blocks_ends_the_run() {
        let body = "[Belief](notes/belief.md){dcterms:source} {schema:status}";
        let link = only(body);
        assert_eq!(link.predicates, vec!["dcterms:source".to_owned()]);
        assert_eq!(
            &body[link.span.0..link.span.1],
            "[Belief](notes/belief.md){dcterms:source}"
        );
    }

    /// The two kinds of token share a block. `rel="cites"` is what vaults are
    /// already writing, and adding a predicate beside it must not disturb it.
    #[test]
    fn a_predicate_and_a_quoted_pair_share_one_block() {
        let link = only("[Belief](notes/belief.md){schema:creator, rel=\"cites\"}");
        assert_eq!(link.predicates, vec!["schema:creator".to_owned()]);
        assert_eq!(link.attrs, vec![("rel".to_owned(), "cites".to_owned())]);
    }

    /// Splitting on commas must not split inside a quoted value, or a
    /// two-word `rel` would be lost the moment a predicate joined it.
    #[test]
    fn a_quoted_value_may_contain_the_separators() {
        let link = only("[Belief](notes/belief.md){rel=\"see also\", schema:creator}");
        assert_eq!(link.attrs, vec![("rel".to_owned(), "see also".to_owned())]);
        assert_eq!(link.predicates, vec!["schema:creator".to_owned()]);
    }

    /// `[[belief]]{skos:related}` is as legitimate an edge label as the
    /// markdown form, and a vault should not have to remember which of its two
    /// link syntaxes carries predicates.
    #[test]
    fn a_wikilink_carries_predicates_too() {
        let body = "[[belief|Belief]]{skos:related, prov:wasDerivedFrom}";
        let link = only(body);
        assert_eq!(
            link.predicates,
            vec!["skos:related".to_owned(), "prov:wasDerivedFrom".to_owned()]
        );
        assert_eq!(&body[link.span.0..link.span.1], body);
    }

    /// An external destination is not an edge in the vault graph, so `extract`
    /// drops it — but the block after it is still a block, and a reader that
    /// went looking for one (an RDF export walking outward citations) must get
    /// the predicates and the end of the syntax. The reader is destination
    /// blind on purpose.
    #[test]
    fn an_external_destination_still_carries_its_predicates() {
        let body = "[x](https://example.com){schema:codeRepository}";
        assert!(
            extract(body).is_empty(),
            "external links are not vault edges"
        );

        let at = body.find('{').expect("a block");
        let blocks = read_attrs(body, at).expect("a block is read there");
        assert_eq!(blocks.predicates, vec!["schema:codeRepository".to_owned()]);
        assert_eq!(blocks.end, body.len());
    }

    /// Duplicates are the shape a copy-paste leaves behind. One edge, once.
    #[test]
    fn exact_duplicates_are_dropped_and_order_is_kept() {
        assert_eq!(
            predicates(
                "[Belief](notes/belief.md){schema:creator, foaf:knows}{schema:creator, skos:related}"
            ),
            vec![
                "schema:creator".to_owned(),
                "foaf:knows".to_owned(),
                "skos:related".to_owned(),
            ]
        );
    }

    /// A token with no single obvious reading is dropped rather than repaired: a
    /// wrong edge in a graph somebody queries is worse than an absent one. The
    /// block is still consumed, so the span still covers it and the braces do
    /// not survive into the rendered line.
    ///
    /// `{:b}` and `{a b}` used to be on this list, which was wrong: kramdown and
    /// Semantic Markdown V0 both read them as names, and dropping them lost the
    /// two commonest spellings in the vaults this exists to read.
    #[test]
    fn junk_yields_no_predicates_and_does_not_corrupt_the_span() {
        for block in [
            // An empty half either side of the colon, and one colon too many.
            "{a:}",
            "{:}",
            "{a:b:c}",
            // Presentational: kramdown's class, id and marker.
            "{.cls}",
            "{#id}",
            // Not a name at all: a digit first, punctuation inside, and nothing.
            "{1st}",
            "{a.b}",
            "{}",
            // A pair with an unquoted value is no pair and no name.
            "{reference=supports}",
        ] {
            let body = format!("[Belief](notes/belief.md){block}");
            let link = only(&body);
            assert!(
                link.predicates.is_empty(),
                "{block} should yield no predicates, got {:?}",
                link.predicates
            );
            assert!(link.attrs.is_empty(), "{block} should yield no attributes");
            assert_eq!(&body[link.span.0..link.span.1], body, "span for {block}");
        }
    }

    /// One space and it is prose, exactly as for `attrs`. The author says which
    /// they meant by typing the space or not typing it.
    #[test]
    fn a_space_before_the_brace_leaves_the_predicates_unread() {
        let body = "[Belief](notes/belief.md) {schema:creator}";
        let link = only(body);
        assert!(link.predicates.is_empty());
        assert_eq!(&body[link.span.0..link.span.1], "[Belief](notes/belief.md)");
    }

    /// The overwhelming majority of links carry nothing, and they must cost
    /// nothing and look the same as they did before this field existed.
    #[test]
    fn a_plain_link_has_no_predicates() {
        let body = "[Belief](notes/belief.md)";
        let link = only(body);
        assert!(link.predicates.is_empty());
        assert!(link.attrs.is_empty());
        assert_eq!(link.span, (0, body.len()));
    }

    /// An unclosed brace is prose: the run stops before it and the span stays
    /// where the link ended, so nothing downstream reads past the line.
    #[test]
    fn an_unclosed_block_is_not_a_block() {
        let body = "[Belief](notes/belief.md){schema:creator\nnext line";
        let link = only(body);
        assert!(link.predicates.is_empty());
        assert_eq!(&body[link.span.0..link.span.1], "[Belief](notes/belief.md)");
    }

    /// The owner's commonest spelling. The empty prefix is the document's
    /// default vocabulary and the colon is stripped, so this and the bare form
    /// are one string rather than two names that mean the same thing.
    #[test]
    fn an_empty_prefix_is_read_and_loses_its_colon() {
        assert_eq!(
            predicates("[Auth](notes/auth.md){ :depends_on }"),
            vec!["depends_on".to_owned()]
        );
    }

    /// Semantic Markdown V0: an attribute that is not a class, not an id and not
    /// a pair is a property name.
    #[test]
    fn a_bare_word_is_a_property_name() {
        assert_eq!(
            predicates("[Auth](notes/auth.md){ depends_on }"),
            vec!["depends_on".to_owned()]
        );
    }

    /// The owner's metric block, written the way they write it: no commas, and a
    /// value that is a URL. Both keys announce predicates; both literals stay in
    /// `attrs` under the key as written, colon included.
    #[test]
    fn colon_keyed_pairs_announce_names_and_keep_their_objects() {
        let link = only(
            "[Metric](notes/metric.md){ :type=\"Metric\" :owned_by=\"https://company.internal\" }",
        );
        assert_eq!(
            link.predicates,
            vec!["type".to_owned(), "owned_by".to_owned()]
        );
        assert_eq!(
            link.attrs,
            vec![
                (":type".to_owned(), "Metric".to_owned()),
                (
                    ":owned_by".to_owned(),
                    "https://company.internal".to_owned()
                ),
            ]
        );
    }

    /// Order is a fact about the document, and `attrs` and `predicates` being two
    /// vectors means this reader is the only layer that can still see it: a
    /// consumer handed the two lists cannot tell that `cites` came first.
    #[test]
    fn a_mixed_block_keeps_its_written_order() {
        let link = only("[Belief](notes/belief.md){ cites, :type=\"x\" }");
        assert_eq!(link.predicates, vec!["cites".to_owned(), "type".to_owned()]);
        assert_eq!(link.attrs, vec![(":type".to_owned(), "x".to_owned())]);
    }

    /// A bare key is presentational until a vocabulary says otherwise, and that
    /// decision is not this file's. The pair is still recorded, because `attrs`
    /// is the raw record: this is how `rel="cites"` keeps working.
    #[test]
    fn a_bare_key_pair_is_recorded_and_announces_nothing() {
        for (block, key, value) in [
            ("{ rel=\"cites\" }", "rel", "cites"),
            ("{ class=\"wide\" }", "class", "wide"),
        ] {
            let link = only(&format!("[Belief](notes/belief.md){block}"));
            assert_eq!(
                link.attrs,
                vec![(key.to_owned(), value.to_owned())],
                "attrs for {block}"
            );
            assert!(
                link.predicates.is_empty(),
                "{block} should announce nothing here, got {:?}",
                link.predicates
            );
        }
    }

    /// A quoted value is opaque: the space must not split the token, and the
    /// colon inside it must not be read as a prefix boundary.
    #[test]
    fn a_quoted_value_may_contain_a_space_and_a_colon() {
        let link = only("[Belief](notes/belief.md){ :source=\"see also: the 1998 memo\" }");
        assert_eq!(
            link.attrs,
            vec![(":source".to_owned(), "see also: the 1998 memo".to_owned())]
        );
        assert_eq!(link.predicates, vec!["source".to_owned()]);
    }

    /// kramdown's `{: .cls #id}` is styling. The marker colon, the class and the
    /// id are all presentational, and none of them is an edge — but the block is
    /// still consumed, so a renderer hiding the span does not leave braces.
    #[test]
    fn a_kramdown_style_block_announces_nothing() {
        let body = "[Belief](notes/belief.md){: .cls #id }";
        let link = only(body);
        assert!(link.predicates.is_empty(), "got {:?}", link.predicates);
        assert!(link.attrs.is_empty());
        assert_eq!(&body[link.span.0..link.span.1], body);
    }

    /// Two blocks, one in each spelling, are one list — a vault will contain both
    /// the week after the convention is announced.
    #[test]
    fn adjacent_blocks_mix_the_spellings() {
        let body = "[Belief](notes/belief.md){ :depends_on }{ owned_by }";
        let link = only(body);
        assert_eq!(
            link.predicates,
            vec!["depends_on".to_owned(), "owned_by".to_owned()]
        );
        assert_eq!(&body[link.span.0..link.span.1], body);
    }

    /// The three spellings are one name, so they are one edge. Anything else
    /// draws the same arrow three times in a graph somebody queries.
    #[test]
    fn the_spellings_of_one_name_collapse_to_one_edge() {
        assert_eq!(
            predicates(
                "[Belief](notes/belief.md){ :depends_on, depends_on }{ :depends_on=\"soon\" }"
            ),
            vec!["depends_on".to_owned()]
        );
    }

    /// `**[x](y)**{ :p }` — the author emphasised the link and hung the block off
    /// the closing markers, which is how the owner's documents mark up a link
    /// they also want to shout about. The span is one range, so covering the
    /// block means covering the markers between it and the link.
    #[test]
    fn a_block_after_closing_emphasis_belongs_to_the_link() {
        let body = "The pipeline relies on **[JWT Auth](notes/jwt.md)**{ :depends_on }.";
        let link = only(body);
        assert_eq!(link.predicates, vec!["depends_on".to_owned()]);
        assert_eq!(
            &body[link.span.0..link.span.1],
            "[JWT Auth](notes/jwt.md)**{ :depends_on }"
        );
    }

    /// Every marker kramdown emphasises with, since a vault writes all of them.
    #[test]
    fn every_emphasis_marker_closes_the_same_way() {
        for (open, close) in [("*", "*"), ("**", "**"), ("_", "_"), ("__", "__")] {
            let body = format!("{open}[Belief](notes/belief.md){close}{{ :depends_on }}");
            let link = only(&body);
            assert_eq!(
                link.predicates,
                vec!["depends_on".to_owned()],
                "predicates for {body}"
            );
            assert_eq!(
                &body[link.span.0..link.span.1],
                &body[open.len()..],
                "span for {body}"
            );
        }
    }

    /// The owner's line, verbatim. Its destination is external, so `extract`
    /// keeps it out of the vault graph — but the block is still a block, and a
    /// reader that went looking for one (an RDF export walking outward
    /// citations) must get the predicate and the end of the syntax through the
    /// markers.
    #[test]
    fn the_owners_emphasised_external_link_is_read_through_its_markers() {
        let body = "The checkout pipeline relies heavily on the **[JWT Auth Service](https://github.com)**{ :depends_on }.";
        assert!(
            extract(body).is_empty(),
            "external links are not vault edges"
        );

        let after = body.find(")**").expect("the closing markers") + 1;
        let blocks = read_attrs(body, after).expect("a block after the markers");
        assert_eq!(blocks.predicates, vec!["depends_on".to_owned()]);
        assert_eq!(&body[after..blocks.end], "**{ :depends_on }");
    }

    /// A marker run that closes nothing is not a closer: in
    /// `see [note](n.md)**{ :p }` the asterisks are a typo, and hiding them
    /// inside the link's span would delete characters the author can see.
    #[test]
    fn a_marker_run_that_closes_nothing_is_not_a_closer() {
        let body = "see [Belief](notes/belief.md)**{ :depends_on }";
        let link = only(body);
        assert!(link.predicates.is_empty(), "got {:?}", link.predicates);
        assert_eq!(&body[link.span.0..link.span.1], "[Belief](notes/belief.md)");
    }

    /// Emphasis *inside* the link text closes nothing after the link, so it can
    /// never be mistaken for the run that holds a block.
    #[test]
    fn emphasis_inside_the_link_text_is_not_a_closer() {
        let body = "[**Belief**](notes/belief.md){ :depends_on }";
        let link = only(body);
        assert_eq!(link.predicates, vec!["depends_on".to_owned()]);
        assert_eq!(&body[link.span.0..link.span.1], body);
    }

    /// One space after the markers and it is prose, by the same rule as
    /// everywhere else here: the author says which they meant by typing it.
    #[test]
    fn a_space_after_the_closing_markers_makes_it_prose() {
        let body = "**[Belief](notes/belief.md)** { :depends_on }";
        let link = only(body);
        assert!(link.predicates.is_empty());
        assert_eq!(&body[link.span.0..link.span.1], "[Belief](notes/belief.md)");
    }

    /// Four markers are a typo or a rule, not emphasis, so the run is left alone
    /// rather than swallowed — the block goes unread with it, which is the
    /// conservative half of the trade.
    #[test]
    fn a_run_longer_than_emphasis_is_not_a_closer() {
        let body = "****[Belief](notes/belief.md)****{ :depends_on }";
        let link = only(body);
        assert!(link.predicates.is_empty(), "got {:?}", link.predicates);
        assert_eq!(&body[link.span.0..link.span.1], "[Belief](notes/belief.md)");
    }
}
