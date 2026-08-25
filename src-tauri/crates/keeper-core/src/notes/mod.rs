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

pub mod attach;
pub mod counts;
pub mod csv;
pub mod default_spaces;
pub mod embed;
pub mod export;
pub mod frontmatter;
pub mod index;
pub mod links;
pub mod naming;
pub mod okf;
pub mod order;
pub mod query;
pub mod recording_note;
pub mod search;
pub mod seed;
pub mod sort;
pub mod tags;
pub mod template_update;
pub mod templates;
pub mod vm;
/// The two files an Open Knowledge Format bundle reserves, which are never
/// documents.
///
/// OKF's rule is that every `.md` in a bundle carries a non-empty `type:`
/// EXCEPT these two: `index.md` is generated from the documents around it and
/// `log.md` is a hand-kept ledger. Neither has frontmatter, by the format's own
/// definition.
///
/// keeper has to know them because it reads a folder's PATH in one place — a
/// note under `spaces/` is a space — and a generated listing that lands in such
/// a folder then becomes a space with no query, which shows up in the rail as a
/// row that selects nothing and says its query cannot be read. That is not a
/// broken space; it is not a space.
pub const OKF_RESERVED: [&str; 2] = ["index.md", "log.md"];

/// Whether this vault-relative path is one of OKF's reserved files.
pub fn is_okf_reserved(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    OKF_RESERVED.contains(&name)
}

pub mod widget;

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

/// Length in bytes of a leading UTF-8 byte-order mark, or zero.
///
/// Shared rather than duplicated because two span-recording scanners depend on
/// it agreeing with itself: frontmatter must not read the BOM as part of its
/// opening `---` fence, and the CSV scanner must not read it as part of the
/// first cell's text. A disagreement of three bytes between them would make one
/// of the two eat Excel's marker on the first edit.
pub(crate) fn bom_len(source: &str) -> usize {
    if source.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    }
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

#[cfg(test)]
mod okf_reserved_tests {
    use super::is_okf_reserved;

    /// The two files OKF reserves, wherever they sit. A vault that adopts the
    /// format grows one per bundle directory, and every one of them lands in
    /// some folder whose name keeper reads.
    #[test]
    fn the_two_reserved_names_are_recognised_at_any_depth() {
        assert!(is_okf_reserved("index.md"));
        assert!(is_okf_reserved("spaces/index.md"));
        assert!(is_okf_reserved("10-notes/operations/log.md"));
    }

    /// A note is not reserved because its name contains one of them: `index.md`
    /// is a listing, `my-index.md` is somebody's note about indices.
    #[test]
    fn a_name_that_merely_ends_in_one_is_not_reserved() {
        assert!(!is_okf_reserved("spaces/my-index.md"));
        assert!(!is_okf_reserved("spaces/changelog.md"));
        assert!(!is_okf_reserved("spaces/2026-08-09-inbox.md"));
    }
}
