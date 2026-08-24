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
    /// gives `[("reference", "x")]`.
    ///
    /// The convention is Pandoc's and Obsidian leaves it alone, which is what
    /// makes it usable here: a vault carrying these still opens in an editor
    /// that has never heard of them, and the attributes read as text rather
    /// than breaking the link.
    ///
    /// Empty for the overwhelming majority of links. A pair whose value is not
    /// quoted is dropped rather than guessed at — `{reference=aaa bbb}` has no
    /// reading that is obviously right, and a wrong predicate is worse than an
    /// absent one in a graph somebody queries.
    pub attrs: Vec<(String, String)>,
    /// The CURIE predicates written in the link's attribute block(s), in the
    /// order they were written, with exact duplicates dropped:
    /// `{schema:creator, foaf:knows}` gives `["schema:creator", "foaf:knows"]`.
    ///
    /// A token is a predicate by *shape* and by nothing else — `prefix:local`,
    /// both halves `[A-Za-z][A-Za-z0-9_-]*`. No vocabulary is consulted, and no
    /// prefix is resolved here: the prefix map lives in the note's frontmatter,
    /// and a reader that needed it could not read a link in isolation.
    ///
    /// A token that is neither a CURIE nor a quoted `key="value"` pair is
    /// dropped rather than guessed at. `{a b}` and `{:b}` could be read as
    /// several things and the author only meant one of them; a wrong predicate
    /// is worse than an absent one in a graph somebody queries.
    pub predicates: Vec<String>,
}

/// What the attribute block or blocks after a link said.
#[derive(Debug, Clone, PartialEq)]
pub struct AttrBlocks {
    /// `key="value"` pairs, in order, as written.
    pub attrs: Vec<(String, String)>,
    /// Bare CURIE tokens, in order, exact duplicates dropped.
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
/// A block holds `key="value"` pairs and bare CURIE predicates in any mixture,
/// separated by commas, whitespace, or both. Returns `None` when there is no
/// block, otherwise the contents and the byte just past the last closing brace
/// — which is what keeps `RawLink::span` covering the whole syntax when an
/// author wrote more than one block.
///
/// The first block has to begin immediately after the link — one space is not
/// allowed, because `[a](b) {this}` is a sentence with braces in it and
/// `[a](b){this}` is a link with attributes, and only the author knows which
/// they meant. The same rule holds between blocks.
pub fn read_attrs(body: &str, at: usize) -> Option<AttrBlocks> {
    let mut attrs = Vec::new();
    let mut predicates = Vec::new();
    let mut end = None;
    let mut cursor = at;

    while body.as_bytes().get(cursor) == Some(&b'{') {
        // A block never wraps: an unclosed brace is prose, and stopping here
        // leaves the span where the previous block ended.
        let limit = line_limit(body, cursor);
        let Some(close) = body[cursor..limit].find('}').map(|i| i + cursor) else {
            break;
        };
        for_each_token(&body[cursor + 1..close], |token| {
            if let Some(pair) = read_pair(token) {
                attrs.push(pair);
            } else if is_curie(token) && !predicates.iter().any(|had| had == token) {
                predicates.push(token.to_owned());
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

/// `key="value"`, or `None` if the token is not one. An unquoted value is not
/// one: `{reference=aaa bbb}` has no reading that is obviously right.
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

/// `prefix:local`, the compact form every RDF serialisation understands.
fn is_curie(token: &str) -> bool {
    match token.split_once(':') {
        Some((prefix, local)) => is_name(prefix) && is_name(local),
        None => false,
    }
}

/// A CURIE half: a letter, then letters, digits, `_` or `-`. Deliberately
/// narrower than XML's NCName — the extra characters buy nothing here and each
/// one widens what gets mistaken for a predicate.
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

    /// A token that is not a CURIE is dropped rather than guessed at: a wrong
    /// predicate is worse than an absent one in a graph somebody queries. The
    /// block is still consumed, so the span still covers it and the braces do
    /// not survive into the rendered line.
    #[test]
    fn junk_yields_no_predicates_and_does_not_corrupt_the_span() {
        for block in ["{not a curie}", "{a:}", "{:b}", "{a b}", "{}"] {
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
}
