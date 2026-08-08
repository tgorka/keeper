//! Tags: extraction, normalisation and the hierarchical tree (FR-104, FR-143).
//!
//! **This module is the tag vocabulary, for every producer** (Story 42.5). Two
//! things in keeper put tags into the tree — a note's frontmatter and inline
//! `#a/b/c` tags, and a recording session's tag field — and until 42.5 they
//! disagreed: notes came through [`normalise`], recordings were comma-split and
//! trimmed and normalised nowhere, so `Client/Acme ` on a recording and
//! `client/acme` on a note were two tags a person could spend an afternoon
//! failing to reconcile. There is now one rule, stated once in [`normalise`]'s
//! doc comment and applied by both producers through [`normalise_all`]. **If a
//! rule about case, whitespace or slashes is ever restated outside this file,
//! the second vocabulary is back.**
//!
//! A note's tags are the union of its frontmatter `tags` property and the
//! inline `#a/b/c` tags in its body — Obsidian treats both as first-class, so a
//! vault keeper indexes only half of would look broken to the person who wrote
//! it.
//!
//! Inline scanning is where the care goes. A `#` is only a tag when it is
//! actually a tag, and a markdown body is full of hashes that are not: heading
//! markers, colour literals in a code fence, URL fragments, issue references,
//! and escaped literals. Getting this wrong is not cosmetic — a bogus tag lands
//! in the tag tree, in the sidebar and in every space query that reads it.
//!
//! This module also owns the code-region scanner both it and
//! [`crate::notes::links`] need, because "is this offset inside code?" must mean
//! exactly the same thing to a tag as to a wikilink.

use std::collections::{BTreeMap, BTreeSet};

use crate::notes::frontmatter::Frontmatter;
use crate::notes::line_bounds;

/// Normalise one tag into its canonical form, or reject it. **The one rule, for
/// every producer** (Story 42.5, FR-143).
///
/// Stated as rules rather than left to be discovered by reading the callers,
/// because two callers reading it differently is the bug this function exists
/// to close. Applied in this order:
///
/// 1. **Whitespace at the edges is not part of the tag.** The input is trimmed,
///    so `Client/Acme ` and `Client/Acme` are the same tag.
/// 2. **A leading `#` is punctuation, not a segment.** Any run of them is
///    dropped, so the inline `#project` and the frontmatter `project` agree.
/// 3. **Case does not distinguish tags.** Everything is lowercased. Obsidian
///    tags are case-insensitive, and rendering two casings as two tags is the
///    single most common tag-tree complaint.
/// 4. **Interior whitespace folds to `-`, it is not dropped.** `my tag` becomes
///    `my-tag`: `tags: [my tag]` is a real thing people write in frontmatter,
///    and silently discarding it loses data. Whitespace adjacent to a `/` folds
///    to nothing, because the slash is already a boundary — `a / b` is `a/b`.
/// 5. **`/` separates segments and never produces an empty one.** A leading
///    slash, a trailing slash and any doubled run collapse away, so `/a//b/`
///    is `a/b`. There is no empty segment to file anything under.
/// 6. **A tag that normalises to nothing is rejected**, and so is one with no
///    alphanumeric character at all: `#---` is a horizontal rule someone forgot
///    to escape, not a category. `None` means "this is not a tag" and the caller
///    drops it — an empty tag is never stored.
///
/// One-way, by construction: the canonical form is what the index and the tree
/// hold, and nothing here reverses it. What the user typed stays where the user
/// typed it — in the note's frontmatter, and in the recording's `manifest.json`.
pub fn normalise(tag: &str) -> Option<String> {
    let trimmed = tag.trim().trim_start_matches('#');
    let mut out = String::with_capacity(trimmed.len());
    let mut pending_space = false;
    let mut after_slash = false;

    for c in trimmed.chars() {
        if c.is_whitespace() {
            pending_space = true;
            continue;
        }
        if c == '/' {
            // Leading and repeated slashes carry no segment with them.
            if !out.is_empty() && !after_slash {
                out.push('/');
                after_slash = true;
            }
            pending_space = false;
            continue;
        }
        if pending_space && !out.is_empty() && !after_slash {
            out.push('-');
        }
        pending_space = false;
        after_slash = false;
        for lower in c.to_lowercase() {
            out.push(lower);
        }
    }

    while out.ends_with('/') {
        out.pop();
    }
    if out.is_empty() || !out.chars().any(char::is_alphanumeric) {
        return None;
    }
    Some(out)
}

/// Normalise a whole tag field into the canonical list the index stores: each
/// tag through [`normalise`], rejects dropped, duplicates collapsed (Story
/// 42.5).
///
/// **The collapse is the point, not a tidy-up.** `Acme` and `acme` on one
/// recording are one tag and must be counted once, or the sidebar's number
/// stops being the number of things behind it. First-appearance order is kept
/// rather than sorted: the caller's order is the order the user wrote, and a
/// recording's chips have no reason to be alphabetised. (Note tags are sorted
/// by [`note_tags`] instead, because a note's tags come from two places and
/// "frontmatter first, body second" is not an order anyone chose.)
pub fn normalise_all<'a>(tags: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let Some(canonical) = normalise(tag) else {
            continue;
        };
        if !out.contains(&canonical) {
            out.push(canonical);
        }
    }
    out
}

/// Split one comma-separated tag field into the tags the user meant, verbatim
/// (Story 42.5).
///
/// **The one tokenisation.** The recording metadata card is a single text input
/// whose separator is a comma, and until 42.5 the split lived in TypeScript
/// while the trim lived again in Rust. It lives here now, beside the rule that
/// canonicalises what it produces, so there is one answer to "where does one
/// tag end and the next begin".
///
/// The tokens come out **as typed** — trimmed of the whitespace around the
/// comma and with empty ones dropped, and nothing else. That is deliberate:
/// this is what gets written into the session's `manifest.json`, which is the
/// user's own text and must keep saying what they wrote. [`normalise_all`] is
/// what the row and the index apply on top, at their boundary.
pub fn split_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Every inline `#tag` in a body, normalised, deduplicated, in first-appearance
/// order.
///
/// A `#` opens a tag only at the start of the document or after whitespace.
/// That one rule does most of the work: it rejects `https://example.com/#frag`
/// (the `#` follows a `/`), the anchor in `[text](doc#section)`, an escaped
/// `\#`, and `C#` in prose. Fenced blocks and inline code spans are skipped
/// wholesale, and a purely numeric tag is refused because `#1` is an issue
/// reference in every repository on earth.
pub fn inline_tags(body: &str) -> Vec<String> {
    let code = code_spans(body);
    let bytes = body.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut at = 0usize;

    while at < bytes.len() {
        // A multi-byte character never contains an ASCII byte, so this compare
        // can never land mid-character.
        if bytes[at] != b'#' {
            at += 1;
            continue;
        }
        if in_code(&code, at) {
            at += 1;
            continue;
        }
        match body[..at].chars().next_back() {
            None => {}
            Some(c) if c.is_whitespace() => {}
            Some(_) => {
                at += 1;
                continue;
            }
        }

        let start = at + 1;
        let mut end = start;
        for (offset, c) in body[start..].char_indices() {
            if is_tag_char(c) {
                end = start + offset + c.len_utf8();
            } else {
                break;
            }
        }

        let raw = &body[start..end];
        if raw.is_empty() {
            at += 1;
            continue;
        }
        if raw.chars().all(|c| c.is_ascii_digit() || c == '/') {
            at = end;
            continue;
        }
        if let Some(tag) = normalise(raw) {
            if !out.contains(&tag) {
                out.push(tag);
            }
        }
        at = end;
    }

    out
}

/// The characters a tag body may contain. Unicode letters are in, because
/// Obsidian allows them and a Japanese vault is full of them.
fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '/')
}

/// The union of a note's frontmatter and inline tags: normalised, deduplicated
/// and sorted.
///
/// Both `tags` and the singular `tag` are read, because Obsidian accepts both
/// and vaults in the wild contain both.
pub fn note_tags(fm: &Frontmatter, body: &str) -> Vec<String> {
    let mut all: Vec<String> = Vec::new();

    for key in ["tags", "tag"] {
        if let Some(values) = fm.as_list(key) {
            all.extend(values.iter().filter_map(|t| normalise(t.as_str())));
        }
    }
    all.extend(inline_tags(body));

    all.sort();
    all.dedup();
    all
}

/// One node of the hierarchical tag tree (FR-104).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TagNode {
    /// The last path segment — what the sidebar renders.
    pub name: String,
    /// The full slash-separated path, which is what a `tag:` query matches.
    pub path: String,
    /// Notes carrying this tag *or any descendant of it*, each counted once.
    pub count: u32,
    pub children: Vec<TagNode>,
}

/// Build the tag tree from one normalised tag list per note.
///
/// A note tagged `project/keeper` and `project/site` contributes one to
/// `project`, not two: the count answers "how many notes are in here", which is
/// the number the sidebar puts next to a folder-shaped thing.
pub fn tag_tree<'a>(all: impl Iterator<Item = &'a [String]>) -> Vec<TagNode> {
    let mut root = Branch::default();

    for tags in all {
        // Ancestors are deduplicated per note before counting, so two sibling
        // tags do not inflate their shared parent.
        let mut ancestors: BTreeSet<&str> = BTreeSet::new();
        for tag in tags {
            let mut cut = 0usize;
            while let Some(slash) = tag[cut..].find('/') {
                ancestors.insert(&tag[..cut + slash]);
                cut += slash + 1;
            }
            ancestors.insert(tag.as_str());
        }
        for path in ancestors {
            root.bump(path);
        }
    }

    root.into_nodes("")
}

/// The tree under construction. A `BTreeMap` because the output is sorted by
/// name and sorting once, structurally, beats sorting every level afterwards.
#[derive(Default)]
struct Branch {
    count: u32,
    children: BTreeMap<String, Branch>,
}

impl Branch {
    fn bump(&mut self, path: &str) {
        let mut node = self;
        for segment in path.split('/') {
            node = node.children.entry(segment.to_owned()).or_default();
        }
        node.count += 1;
    }

    fn into_nodes(self, prefix: &str) -> Vec<TagNode> {
        self.children
            .into_iter()
            .map(|(name, branch)| {
                let path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                let count = branch.count;
                let children = branch.into_nodes(&path);
                TagNode {
                    name,
                    path,
                    count,
                    children,
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Code regions
// ---------------------------------------------------------------------------

/// Byte ranges of `text` that are code: fenced blocks (fences included) and
/// inline code spans (backticks included). Sorted and non-overlapping.
///
/// Inline spans are matched within a single line. CommonMark allows a code span
/// to wrap, but a stray backtick in prose would then swallow the rest of the
/// document's tags and links — a false negative that is invisible until someone
/// wonders where their tags went.
pub(crate) fn code_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut at = 0usize;

    while let Some((ls, le, next)) = line_bounds(text, at) {
        let line = &text[ls..le];
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let run = fence_run(trimmed);

        match fence {
            Some((open_char, open_len)) => {
                claim(&mut spans, ls, next);
                if let Some((c, n)) = run {
                    if c == open_char && n >= open_len && trimmed[n..].trim().is_empty() {
                        fence = None;
                    }
                }
            }
            None => match run {
                // Four spaces of indent is itself a code block opener in
                // markdown, so a "fence" that deep is literal text.
                Some((c, n)) if indent < 4 => {
                    claim(&mut spans, ls, next);
                    fence = Some((c, n));
                }
                _ => scan_inline_code(line, ls, &mut spans),
            },
        }

        at = next;
    }

    spans
}

/// Whether `at` falls inside one of the (sorted, disjoint) code spans.
pub(crate) fn in_code(spans: &[(usize, usize)], at: usize) -> bool {
    let i = spans.partition_point(|(start, _)| *start <= at);
    i > 0 && spans[i - 1].1 > at
}

/// The opening run of a fence line: three or more backticks or tildes.
fn fence_run(trimmed: &str) -> Option<(u8, usize)> {
    let marker = match trimmed.as_bytes().first().copied() {
        Some(c @ (b'`' | b'~')) => c,
        _ => return None,
    };
    let len = trimmed.bytes().take_while(|b| *b == marker).count();
    (len >= 3).then_some((marker, len))
}

/// Append a span, merging it into the previous one when they touch. Fenced
/// blocks arrive a line at a time and must come out as one range.
fn claim(spans: &mut Vec<(usize, usize)>, start: usize, end: usize) {
    match spans.last_mut() {
        Some(last) if last.1 >= start => last.1 = end,
        _ => spans.push((start, end)),
    }
}

fn scan_inline_code(line: &str, base: usize, spans: &mut Vec<(usize, usize)>) {
    let bytes = line.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let open = bytes[i..].iter().take_while(|b| **b == b'`').count();

        // A span closes on a backtick run of exactly the same length.
        let mut j = i + open;
        let mut closed = None;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let run = bytes[j..].iter().take_while(|b| **b == b'`').count();
                if run == open {
                    closed = Some(j + run);
                    break;
                }
                j += run;
            } else {
                j += 1;
            }
        }

        match closed {
            Some(end) => {
                claim(spans, base + i, base + end);
                i = end;
            }
            // An unmatched run is literal text, not the start of code.
            None => i += open,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(source: &str) -> (Frontmatter, &str) {
        let (fm, body) = Frontmatter::parse(source);
        (fm, &source[body..])
    }

    #[test]
    fn normalise_folds_case_trims_and_collapses_slashes() {
        assert_eq!(normalise("#Project"), Some("project".to_owned()));
        assert_eq!(
            normalise("  Project/Keeper  "),
            Some("project/keeper".to_owned())
        );
        assert_eq!(normalise("a//b///c"), Some("a/b/c".to_owned()));
        assert_eq!(normalise("/leading/"), Some("leading".to_owned()));
        assert_eq!(normalise("my tag"), Some("my-tag".to_owned()));
        assert_eq!(normalise("日本語/ノート"), Some("日本語/ノート".to_owned()));
    }

    #[test]
    fn normalise_rejects_empty_and_all_punctuation_tags() {
        assert_eq!(normalise(""), None);
        assert_eq!(normalise("#"), None);
        assert_eq!(normalise("   "), None);
        assert_eq!(normalise("---"), None);
        assert_eq!(normalise("///"), None);
        assert_eq!(normalise("#___"), None);
    }

    #[test]
    fn inline_tags_finds_hierarchical_tags() {
        assert_eq!(
            inline_tags("A #project/keeper note about #Review and #project/keeper again."),
            vec!["project/keeper".to_owned(), "review".to_owned()]
        );
    }

    #[test]
    fn inline_tags_ignores_a_hash_inside_a_fence() {
        let body = "before #real\n\n```sh\n# not a tag\ngrep '#alsonot' file\n```\n\nafter #tail\n";
        assert_eq!(
            inline_tags(body),
            vec!["real".to_owned(), "tail".to_owned()]
        );
    }

    #[test]
    fn inline_tags_ignores_a_hash_inside_backticks() {
        assert_eq!(
            inline_tags("use `#include <stdio.h>` here"),
            Vec::<String>::new()
        );
        assert_eq!(
            inline_tags("``a ` and #nope`` but #yes"),
            vec!["yes".to_owned()]
        );
    }

    #[test]
    fn inline_tags_ignores_a_url_fragment() {
        assert_eq!(
            inline_tags("see https://example.com/x/#fragment and [doc](notes/a.md#heading)"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn inline_tags_ignores_an_escaped_hash() {
        assert_eq!(inline_tags(r"a literal \#hash sign"), Vec::<String>::new());
        assert_eq!(
            inline_tags(r"escaped \#no but #yes"),
            vec!["yes".to_owned()]
        );
    }

    #[test]
    fn inline_tags_ignores_heading_markers_and_issue_numbers() {
        assert_eq!(inline_tags("# Heading\n## Sub\n"), Vec::<String>::new());
        assert_eq!(inline_tags("fixes #123 and #45/6"), Vec::<String>::new());
        assert_eq!(inline_tags("release #v2"), vec!["v2".to_owned()]);
    }

    #[test]
    fn inline_tags_accepts_a_tag_at_the_very_start_of_the_body() {
        assert_eq!(inline_tags("#inbox first"), vec!["inbox".to_owned()]);
    }

    #[test]
    fn note_tags_unions_frontmatter_and_inline_and_sorts() {
        let source = "---\ntags:\n  - Project/Keeper\n  - review\ntag: Extra\n---\n\nBody with #inbox and #project/keeper.\n";
        let (fm, body) = parsed(source);
        assert_eq!(
            note_tags(&fm, body),
            vec![
                "extra".to_owned(),
                "inbox".to_owned(),
                "project/keeper".to_owned(),
                "review".to_owned(),
            ]
        );
    }

    #[test]
    fn note_tags_survives_a_note_with_no_tags_at_all() {
        let (fm, body) = parsed("just a body\n");
        assert!(note_tags(&fm, body).is_empty());
    }

    #[test]
    fn tag_tree_counts_each_note_once_per_node() {
        let a = vec!["project/keeper".to_owned(), "project/site".to_owned()];
        let b = vec!["project/keeper".to_owned()];
        let c = vec!["review".to_owned()];
        let notes = [a.as_slice(), b.as_slice(), c.as_slice()];

        let tree = tag_tree(notes.iter().copied());
        assert_eq!(tree.len(), 2);

        let project = &tree[0];
        assert_eq!(project.name, "project");
        assert_eq!(project.path, "project");
        // Two notes are in `project`, not three.
        assert_eq!(project.count, 2);
        assert_eq!(project.children.len(), 2);
        assert_eq!(project.children[0].path, "project/keeper");
        assert_eq!(project.children[0].count, 2);
        assert_eq!(project.children[1].path, "project/site");
        assert_eq!(project.children[1].count, 1);

        assert_eq!(tree[1].name, "review");
        assert_eq!(tree[1].count, 1);
    }

    #[test]
    fn tag_tree_keeps_a_parent_alive_while_a_sibling_survives() {
        let only_child = vec!["project/keeper".to_owned()];
        let notes = [only_child.as_slice()];
        let tree = tag_tree(notes.iter().copied());

        // The parent exists as a node even though no note carries it directly.
        assert_eq!(tree[0].path, "project");
        assert_eq!(tree[0].count, 1);
        assert_eq!(tree[0].children[0].path, "project/keeper");

        // Drop the only note carrying it and the whole branch is gone.
        assert!(tag_tree(std::iter::empty::<&[String]>()).is_empty());
    }

    #[test]
    fn code_spans_cover_a_fence_and_stop_at_its_close() {
        let text = "a\n```\nb\n```\nc\n";
        let spans = code_spans(text);
        assert_eq!(spans, vec![(2, 12)]);
        assert!(!in_code(&spans, 0));
        assert!(in_code(&spans, 6));
        assert!(!in_code(&spans, 12));
    }

    #[test]
    fn code_spans_handle_a_tilde_fence_and_a_longer_inner_fence() {
        let text = "~~~\n```\nstill code\n```\n~~~\nout\n";
        let spans = code_spans(text);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0], (0, 27));
    }

    #[test]
    fn every_rule_the_normalise_doc_comment_states_is_the_rule_it_applies() {
        // Story 42.5: the doc comment is the contract two producers read, so
        // each of its six numbered rules gets an assertion. A rule that stops
        // being true here is a rule that lied to whoever implemented against it.
        // 1. Edge whitespace is not part of the tag.
        assert_eq!(normalise("Client/Acme "), Some("client/acme".to_owned()));
        assert_eq!(normalise("\tclient/acme\n"), Some("client/acme".to_owned()));
        // 2. A leading `#` — any run of them — is punctuation.
        assert_eq!(normalise("##client"), Some("client".to_owned()));
        // 3. Case does not distinguish tags.
        assert_eq!(normalise("CLIENT/Acme"), normalise("client/acme"));
        // 4. Interior whitespace folds to `-`; whitespace beside a `/` folds to
        //    nothing, because the slash is already a boundary.
        assert_eq!(normalise("my tag"), Some("my-tag".to_owned()));
        assert_eq!(normalise("a  b"), Some("a-b".to_owned()));
        assert_eq!(normalise("a / b"), Some("a/b".to_owned()));
        // 5. `/` never produces an empty segment.
        assert_eq!(normalise("/a//b/"), Some("a/b".to_owned()));
        // 6. What normalises to nothing is not a tag.
        assert_eq!(normalise("   "), None);
        assert_eq!(normalise("///"), None);
        assert_eq!(normalise("#---"), None);
    }

    #[test]
    fn a_recording_tag_and_a_note_tag_normalise_to_one_string() {
        // The AC1 pair, at the level of the rule. That they then land on ONE
        // TREE NODE is asserted over the tree itself, in `notes::index`.
        assert_eq!(normalise("Client/Acme "), normalise("client/acme"));
    }

    #[test]
    fn promoting_the_rule_moved_no_existing_note_tag() {
        // Story 42.5's blocking condition, frozen as a table. Normalisation
        // became the vocabulary for a SECOND producer; it must not have become a
        // different vocabulary for the first. Every pair here is a tag shape a
        // vault already contains and the node it already resolved to, so a
        // future tidy-up of the rule that would re-file somebody's notes fails
        // here instead of shipping as a silent migration.
        for (written, node) in [
            ("#Project", "project"),
            ("project/keeper", "project/keeper"),
            ("  Project/Keeper  ", "project/keeper"),
            ("a//b///c", "a/b/c"),
            ("/leading/", "leading"),
            ("my tag", "my-tag"),
            ("日本語/ノート", "日本語/ノート"),
            ("Q3-2026", "q3-2026"),
            ("client/acme/renewal", "client/acme/renewal"),
            ("some_tag", "some_tag"),
        ] {
            assert_eq!(
                normalise(written).as_deref(),
                Some(node),
                "`{written}` must still file under `{node}`"
            );
        }
        for rejected in ["", "#", "   ", "---", "///", "#___"] {
            assert_eq!(
                normalise(rejected),
                None,
                "`{rejected}` was never a tag and must still not be one"
            );
        }
    }

    #[test]
    fn normalise_all_collapses_duplicates_and_drops_what_is_not_a_tag() {
        // The matrix's duplicate-after-normalising row: `Acme` and `acme` on one
        // recording are one tag, counted once.
        assert_eq!(
            normalise_all(["Acme", "acme", " ACME "]),
            vec!["acme".to_owned()]
        );
        // The matrix's empty-after-normalising row: dropped, never stored as an
        // empty tag.
        assert_eq!(
            normalise_all(["  ", "///", "client/acme", "#---"]),
            vec!["client/acme".to_owned()]
        );
        assert!(normalise_all(["  ", "///"]).is_empty());
        // First-appearance order, so a recording's chips stay in the order the
        // person typed them.
        assert_eq!(
            normalise_all(["Renewal", "Client/Acme "]),
            vec!["renewal".to_owned(), "client/acme".to_owned()]
        );
    }

    #[test]
    fn split_list_tokenises_the_card_field_and_canonicalises_nothing() {
        // The one tokenisation. It must NOT normalise: what it returns is what
        // `manifest.json` records, and the manifest is the user's own text.
        assert_eq!(
            split_list("Client/Acme , renewal"),
            vec!["Client/Acme".to_owned(), "renewal".to_owned()]
        );
        // Blank tokens are not tags — a trailing comma is a typo, not an entry.
        assert_eq!(
            split_list("standup, ,q3,"),
            vec!["standup".to_owned(), "q3".to_owned()]
        );
        assert!(split_list("   ").is_empty());
        assert!(split_list("").is_empty());
        // A tag with a comma in it cannot exist, and a tag with a space in it
        // survives the split intact for `normalise` to fold.
        assert_eq!(split_list("my tag"), vec!["my tag".to_owned()]);
    }
}
