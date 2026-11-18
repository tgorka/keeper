//! A tag's canonical text: the one tag policy keeper has (Story 42.5,
//! FR-143). Notes, recordings, spaces and the graph client all fold a tag
//! through [`normalise`]; it lives in the contract so that every one of them
//! reaches the same definition and none restates it.

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
