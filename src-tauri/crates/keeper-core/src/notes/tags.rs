//! Tags: extraction, normalisation and the hierarchical tree (FR-104).
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

/// Normalise one tag into its canonical form, or reject it.
///
/// Case-folded (Obsidian tags are case-insensitive and rendering two casings as
/// two tags is the single most common tag-tree complaint), trimmed, a leading
/// `#` removed, repeated and edge slashes collapsed, and internal whitespace
/// folded to `-` rather than dropped — `tags: [my tag]` is a real thing people
/// write in frontmatter, and silently discarding it loses data.
///
/// Rejects the empty tag and the all-punctuation tag: `#---` is a horizontal
/// rule someone forgot to escape, not a category.
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
}
