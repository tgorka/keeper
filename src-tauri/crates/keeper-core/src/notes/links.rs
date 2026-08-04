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
    /// Byte range of the whole link syntax in the body, `!` included.
    pub span: (usize, usize),
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

    Some(RawLink {
        target,
        alias: alias.filter(|a| !a.is_empty()),
        embed,
        span: (outer, close + 2),
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
    Some(RawLink {
        target,
        alias: (!text.is_empty()).then(|| text.to_owned()),
        embed,
        span: (outer, dest_end + 1),
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
