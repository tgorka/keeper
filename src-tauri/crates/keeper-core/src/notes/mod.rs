//! The pure notes domain (AD-55).
//!
//! Everything here is a *rule* rather than an *effect*: what a frontmatter block
//! means, what filename a title produces, what a tag tree looks like, what a
//! wikilink resolves to, what a template expands to, what a space query matches,
//! what the index is. It takes bytes and returns values. It never opens a file,
//! never spawns a task, and never learns that a profile id means anything to git
//! — the vault IO lives in the `keeper` shell on `keeper-sync`'s watcher
//! (AD-56). That split is the reason this phase is testable at all: every rule
//! below is exercised over `&str` inputs, with no vault, no tokio and no Tauri.

pub mod frontmatter;
pub mod index;
pub mod links;
pub mod naming;
pub mod query;
pub mod search;
pub mod tags;
pub mod templates;
pub mod vm;

/// Everything the notes domain can refuse to do.
#[derive(Debug, thiserror::Error)]
pub enum NotesError {
    #[error("note frontmatter is malformed at line {line}: {reason}")]
    Frontmatter { line: usize, reason: String },
    #[error("space query: {message}")]
    Query { message: String, token_index: usize },
    #[error("template: {0}")]
    Template(String),
    #[error("invalid note name: {0}")]
    Name(String),
    #[error("no such note: {0}")]
    NotFound(String),
    #[error("vault {0} is not indexed")]
    VaultUnknown(String),
}

/// Byte bounds of the line beginning at `at`: `(line_start, line_end,
/// next_line_start)`, where `line_end` excludes the terminator and
/// `next_line_start` includes it. `None` once `at` is past the end.
///
/// Shared by the frontmatter scanner and the tag/link code-region scanner
/// because all three record *spans over the original bytes* and must agree on
/// where a line stops to the byte — on a CRLF document a disagreement of one
/// byte leaves a stray `\r` inside a recorded value, and FR-121 is a byte-level
/// promise.
pub(crate) fn line_bounds(s: &str, at: usize) -> Option<(usize, usize, usize)> {
    if at >= s.len() {
        return None;
    }
    match s[at..].find('\n') {
        Some(nl) => {
            let mut end = at + nl;
            if end > at && s.as_bytes()[end - 1] == b'\r' {
                end -= 1;
            }
            Some((at, end, at + nl + 1))
        }
        None => Some((at, s.len(), s.len())),
    }
}

/// 1-based line number of `offset` in `s`. Only ever called on the diagnostic
/// path, so the linear count costs nothing in the cold-scan budget (NFR-28).
pub(crate) fn line_number(s: &str, offset: usize) -> usize {
    s[..offset.min(s.len())]
        .bytes()
        .filter(|b| *b == b'\n')
        .count()
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_bounds_excludes_the_terminator_and_a_crlf_carriage_return() {
        assert_eq!(line_bounds("a\nbb\n", 0), Some((0, 1, 2)));
        assert_eq!(line_bounds("a\r\nbb", 0), Some((0, 1, 3)));
        assert_eq!(line_bounds("a\r\nbb", 3), Some((3, 5, 5)));
        assert_eq!(line_bounds("a\n", 2), None);
    }

    #[test]
    fn line_number_counts_from_one_and_clamps() {
        let s = "one\ntwo\nthree";
        assert_eq!(line_number(s, 0), 1);
        assert_eq!(line_number(s, 4), 2);
        assert_eq!(line_number(s, 8), 3);
        assert_eq!(line_number(s, 9_999), 3);
    }
}
