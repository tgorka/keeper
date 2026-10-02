//! Core memory: `USER.md` and `MEMORY.md`, their caps and the frozen snapshot (AD-364, story 89.3).
//!
//! Both files are markdown whose body is a list of entries separated by a
//! line holding only `§` (Hermes' format). Optional frontmatter is ignored and
//! not counted. A session reads both once, when it opens, and uses that
//! snapshot to its end: [`snapshot`] is that read, and its digest is what the
//! session's `open` line records.
//!
//! This story has no writer (95.1 has). A file a person already left over its
//! cap, or holding a duplicate entry or an invisible format character, is
//! left out of the snapshot whole and named in a problem that lists its
//! entries (choice C3): keeper never truncates or cleans memory itself.

use sha2::{Digest, Sha256};

use crate::notes::frontmatter::Frontmatter;

/// `USER.md`'s cap, in Unicode scalar values.
pub const USER_CAP: usize = 1375;
/// `MEMORY.md`'s cap, in Unicode scalar values.
pub const MEMORY_CAP: usize = 2200;
/// What separates two entries, in the canonical text.
pub const SEPARATOR: &str = "\n§\n";

/// The entries of a body: the text between lines holding only `§`, each
/// trimmed, empty ones dropped. A `§` inside a line is text.
pub fn entries(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut at = 0usize;
    for line in body.split_inclusive('\n') {
        if line.trim() == "§" {
            out.push(&body[start..at]);
            start = at + line.len();
        }
        at += line.len();
    }
    out.push(&body[start..]);
    out.into_iter()
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect()
}

/// The size a cap is measured against: Unicode scalar values of the body's
/// entries joined canonically (`\n§\n`), so trailing blank lines and the
/// spelling of a separator line cost nothing, and bytes never count.
pub fn count(body: &str) -> usize {
    let entries = entries(body);
    let separators = entries.len().saturating_sub(1) * SEPARATOR.chars().count();
    entries.iter().map(|e| e.chars().count()).sum::<usize>() + separators
}

/// Bidirectional controls, zero-width characters and other invisible format
/// characters: what lets an entry say something a person reading the file
/// does not see (§9.3).
fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

/// A memory file left out of the snapshot, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryProblem {
    /// `USER.md` or `MEMORY.md`.
    pub file: &'static str,
    /// The sentence the person reads.
    pub sentence: String,
    /// The file's entries as they are, so the person can choose what to cut.
    pub entries: Vec<String>,
}

/// Core memory as one session sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySnapshot {
    pub user: Vec<String>,
    pub memory: Vec<String>,
    pub problems: Vec<MemoryProblem>,
    /// SHA-256, lowercase hex, of [`MemorySnapshot::canonical`].
    pub sha256: String,
}

impl MemorySnapshot {
    /// The canonical text the digest is of: every entry, `USER.md`'s first,
    /// joined by `\n§\n`.
    pub fn canonical(&self) -> String {
        let all: Vec<&str> = self
            .user
            .iter()
            .chain(self.memory.iter())
            .map(String::as_str)
            .collect();
        all.join(SEPARATOR)
    }
}

/// Read both files once. `None` is a file that does not exist, which is an
/// empty memory, not a problem.
pub fn snapshot(user: Option<&str>, memory: Option<&str>) -> MemorySnapshot {
    let mut problems = Vec::new();
    let user = read_file("USER.md", USER_CAP, user, &mut problems);
    let memory = read_file("MEMORY.md", MEMORY_CAP, memory, &mut problems);
    let mut snapshot = MemorySnapshot {
        user,
        memory,
        problems,
        sha256: String::new(),
    };
    snapshot.sha256 = hex::encode(Sha256::digest(snapshot.canonical().as_bytes()));
    snapshot
}

fn read_file(
    file: &'static str,
    cap: usize,
    text: Option<&str>,
    problems: &mut Vec<MemoryProblem>,
) -> Vec<String> {
    let Some(text) = text else {
        return Vec::new();
    };
    let (_, body_offset) = Frontmatter::parse(text);
    let body = &text[body_offset..];
    let found: Vec<String> = entries(body).into_iter().map(str::to_owned).collect();
    match problem_with(file, cap, body, &found) {
        None => found,
        Some(sentence) => {
            problems.push(MemoryProblem {
                file,
                sentence,
                entries: found,
            });
            Vec::new()
        }
    }
}

fn problem_with(file: &str, cap: usize, body: &str, found: &[String]) -> Option<String> {
    let size = count(body);
    if size > cap {
        return Some(format!(
            "{file} is {size} characters; the cap is {cap}. Shorten it; keeper does not cut it for you."
        ));
    }
    for (at, entry) in found.iter().enumerate() {
        if let Some(c) = entry.chars().find(|&c| is_invisible_format(c)) {
            return Some(format!(
                "{file} entry {} holds U+{:04X}, an invisible format character. Remove it; keeper \
                 does not strip it for you.",
                at + 1,
                u32::from(c)
            ));
        }
        if let Some(first) = found[..at].iter().position(|earlier| earlier == entry) {
            return Some(format!(
                "{file} entry {} repeats entry {}. Remove one; keeper does not merge them for you.",
                at + 1,
                first + 1
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(path: &str) -> String {
        let root = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/agents/zone-ok/nixi/"
        );
        std::fs::read_to_string(format!("{root}{path}")).expect("fixture is readable")
    }

    fn body(text: &str) -> &str {
        &text[Frontmatter::parse(text).1..]
    }

    #[test]
    fn the_fixture_memory_sits_exactly_on_its_caps() {
        let user = fixture("USER.md");
        let memory = fixture("MEMORY.md");
        assert_eq!(count(body(&user)), USER_CAP);
        assert_eq!(count(body(&memory)), MEMORY_CAP);
        let snapshot = snapshot(Some(&user), Some(&memory));
        assert_eq!(snapshot.problems, Vec::new());
        assert_eq!(snapshot.user.len(), 14);
        assert_eq!(snapshot.memory.len(), 22);
    }

    #[test]
    fn one_scalar_over_the_cap_leaves_the_file_out_and_lists_its_entries() {
        let user = fixture("USER.md");
        let over = user.replacen("tgorka is Tomasz", "tgorka is Tomasz ", 1);
        let snapshot = snapshot(Some(&over), Some(&fixture("MEMORY.md")));
        assert!(
            snapshot.user.is_empty(),
            "the over-cap file is left out whole"
        );
        assert_eq!(snapshot.memory.len(), 22, "the other file is unaffected");
        assert_eq!(snapshot.problems.len(), 1);
        let problem = &snapshot.problems[0];
        assert_eq!(problem.file, "USER.md");
        assert_eq!(
            problem.sentence,
            "USER.md is 1376 characters; the cap is 1375. Shorten it; keeper does not cut it for you."
        );
        assert_eq!(problem.entries.len(), 14);
        assert!(problem.entries[0].starts_with("tgorka is Tomasz  Gorka"));

        let memory_over = format!("{}x", fixture("MEMORY.md").trim_end());
        let snapshot = super::snapshot(None, Some(&memory_over));
        assert_eq!(snapshot.problems[0].file, "MEMORY.md");
        assert!(snapshot.problems[0]
            .sentence
            .starts_with("MEMORY.md is 2201 characters"));
    }

    #[test]
    fn the_cap_counts_scalars_not_bytes_and_not_frontmatter() {
        let wide = "ł".repeat(USER_CAP);
        assert_eq!(wide.len(), 2750);
        let snapshot = snapshot(Some(&wide), None);
        assert_eq!(snapshot.problems, Vec::new());
        assert_eq!(snapshot.user, std::slice::from_ref(&wide));

        let fronted = format!(
            "---\ntype: memory\nnote: {}\n---\n{wide}\n",
            "y".repeat(500)
        );
        assert_eq!(super::snapshot(Some(&fronted), None).problems, Vec::new());

        let over = format!("{wide}ł");
        assert_eq!(super::snapshot(Some(&over), None).problems.len(), 1);
    }

    #[test]
    fn a_section_sign_inside_a_line_is_text() {
        assert_eq!(
            entries("Article § 3 applies.\n§\nsecond\n  §  \nthird § too\n"),
            ["Article § 3 applies.", "second", "third § too"]
        );
        assert_eq!(count("a\n§\nb"), 5);
        assert_eq!(
            count("a\n\n§\n\nb\n\n"),
            5,
            "separator spelling and trailing lines are free"
        );
    }

    #[test]
    fn a_duplicate_or_an_invisible_character_is_refused_naming_the_entry() {
        let snapshot = snapshot(Some("one\n§\ntwo\n§\none\n"), None);
        assert!(snapshot.user.is_empty());
        assert_eq!(
            snapshot.problems[0].sentence,
            "USER.md entry 3 repeats entry 1. Remove one; keeper does not merge them for you."
        );
        for c in ['\u{202E}', '\u{200B}', '\u{2066}', '\u{FEFF}'] {
            let text = format!("fine\n§\nhid{c}den\n");
            let snapshot = super::snapshot(None, Some(&text));
            assert!(snapshot.memory.is_empty());
            assert_eq!(
                snapshot.problems[0].sentence,
                format!(
                    "MEMORY.md entry 2 holds U+{:04X}, an invisible format character. Remove it; \
                     keeper does not strip it for you.",
                    u32::from(c)
                )
            );
        }
    }

    #[test]
    fn the_digest_is_of_the_canonical_text() {
        let user = fixture("USER.md");
        let memory = fixture("MEMORY.md");
        let first = snapshot(Some(&user), Some(&memory));
        let again = snapshot(Some(&user), Some(&memory));
        assert_eq!(first.sha256, again.sha256);
        assert_eq!(
            first.sha256,
            hex::encode(Sha256::digest(first.canonical().as_bytes()))
        );
        assert!(
            first.canonical().starts_with("tgorka is Tomasz Gorka"),
            "USER.md first"
        );
        assert!(first
            .canonical()
            .contains("\n§\ntgdrive is tgorka's private drive"));

        let changed = memory.replacen("Never paste a token", "Never paste a secret", 1);
        let other = snapshot(Some(&user), Some(&changed));
        assert_ne!(first.sha256, other.sha256);

        let reflowed = user.replace("\n§\n", "\n\n§\n\n");
        assert_eq!(
            snapshot(Some(&reflowed), Some(&memory)).sha256,
            first.sha256
        );
    }
}
