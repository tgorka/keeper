//! `drive_search`'s shape (story 95.4, AD-403, AD-159): the query, the
//! bounds, a result and the sentences that disclose what was not searched.
//!
//! A query is literal terms, folded as the notes index folds them
//! ([`crate::notes::search_index::build_match`]): a term matches where its
//! characters occur, case and accents aside, and never as a pattern — `a.*b`
//! finds the four characters `a.*b`. Inside the notes vault the index ranks
//! (FTS5 tokens there, so the literal guarantee is the scan's); elsewhere the
//! host scans, bounded. Every hit carries the label of the file it came
//! from; the host joins them into the session's.

use crate::agents::label::{Label, Readers};
use crate::notes::search::{find_spans, fold_str};

/// The tool's name.
pub const DRIVE_SEARCH: &str = "drive_search";
/// How many results a call returns when it names no `k`.
pub const DEFAULT_K: usize = 10;
/// The most results a call returns, whatever its `k`.
pub const MAX_K: usize = 25;
/// The most matching lines one result shows.
pub const LINES_PER_HIT: usize = 3;
/// The most characters of one shown line.
pub const LINE_CHARS: usize = 240;
/// The most bytes of the rendered result.
pub const RESULT_BYTES: usize = 80 * 1024;
/// The scan's bounds per call: files opened, bytes read, time spent.
pub const SCAN_FILES: usize = 2_000;
pub const SCAN_BYTES: u64 = 16 * 1024 * 1024;
pub const SCAN_MILLIS: u64 = 1_500;
/// A file larger than this is never scanned.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// How long the query's embedding may take before the search goes lexical.
pub const EMBED_MILLIS: u64 = 1_000;

/// A query: its text and its folded terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    text: String,
    terms: Vec<String>,
}

impl Query {
    /// The terms of `text`: its whitespace-separated words that hold a
    /// letter or a digit, as `build_match` keeps them. `None` without one.
    pub fn parse(text: &str) -> Option<Query> {
        let terms: Vec<String> = text
            .split_whitespace()
            .filter(|term| fold_str(term).chars().any(char::is_alphanumeric))
            .map(str::to_owned)
            .collect();
        (!terms.is_empty()).then(|| Query {
            text: text.trim().to_owned(),
            terms,
        })
    }

    /// The query as asked.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The query as the notes index takes it.
    pub fn index_text(&self) -> String {
        self.terms.join(" ")
    }

    /// Whether `term` occurs in `text`, folded, literally.
    fn holds(text: &str, term: &str) -> bool {
        !find_spans(text, term, 1).is_empty()
    }

    /// Whether every term occurs in one of `texts`.
    pub fn found_in(&self, texts: &[&str]) -> bool {
        self.terms
            .iter()
            .all(|term| texts.iter().any(|text| Query::holds(text, term)))
    }

    /// Whether any term occurs in `line`.
    pub fn in_line(&self, line: &str) -> bool {
        self.terms.iter().any(|term| Query::holds(line, term))
    }

    /// Up to [`LINES_PER_HIT`] lines of `body` holding a term, 1-based,
    /// each cut at [`LINE_CHARS`].
    pub fn lines(&self, body: &str) -> Vec<(usize, String)> {
        body.lines()
            .enumerate()
            .filter(|(_, line)| self.in_line(line))
            .take(LINES_PER_HIT)
            .map(|(at, line)| (at + 1, clip_line(line)))
            .collect()
    }
}

/// How many results a call asking for `requested` gets.
pub fn k_of(requested: Option<u64>) -> usize {
    match requested {
        None | Some(0) => DEFAULT_K,
        Some(k) => usize::try_from(k).unwrap_or(MAX_K).min(MAX_K),
    }
}

/// `line` cut at [`LINE_CHARS`] characters, with `…` where it was cut.
pub fn clip_line(line: &str) -> String {
    let line = line.trim_end_matches('\r');
    match line.char_indices().nth(LINE_CHARS) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line.to_owned(),
    }
}

/// `n` with its thousands set apart by spaces: `2 000`, `5 312`.
pub fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(digit);
    }
    out
}

/// A size as a person reads it.
pub fn size_words(bytes: u64) -> String {
    const KIB: u64 = 1024;
    match bytes {
        b if b < KIB => format!("{b} bytes"),
        b if b < KIB * KIB => format!("{} KiB", b.div_ceil(KIB)),
        b => format!("{:.1} MiB", b as f64 / (KIB * KIB) as f64),
    }
}

/// A call's bounds as a person reads them: `2 000 files, 16.0 MiB or 1.5 s`.
pub fn bounds_words() -> String {
    format!(
        "{} files, {} or {} s",
        grouped(SCAN_FILES),
        size_words(SCAN_BYTES),
        SCAN_MILLIS as f64 / 1000.0
    )
}

/// Where a hit was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// The notes vault's own index.
    Index,
    /// A bundle's `index.md` listing.
    Listing,
    /// The bounded scan.
    Scan,
}

/// One result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub drive: String,
    /// Drive-relative.
    pub path: String,
    pub title: String,
    /// The OKF `type`, when the file states one.
    pub kind: Option<String>,
    /// Matching lines, 1-based, already cut.
    pub lines: Vec<(usize, String)>,
    /// A file whose content is not on this device: its real size. Its text
    /// is never read.
    pub absent: Option<u64>,
    /// The file's label (AD-390): what reading this hit joins.
    pub label: Label,
    pub found: Found,
}

/// What a search says beside its results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// A drive the session may not read: the call is refused by name.
    OutOfScope { drive: String },
    /// The scan stopped at a bound before it had read every file: of how
    /// many it met, or `None` where a bound stopped the walk before it had
    /// met them all.
    ScanCapped {
        drive: String,
        searched: usize,
        of: Option<usize>,
    },
    /// The call's files, bytes or time ran out while the drive's ranked or
    /// listed documents were being read: the rest were not opened.
    Incomplete { drive: String },
    /// Files the scan could not read as text, or that were too large.
    Skipped { drive: String, files: usize },
    /// The drive has no `.okf/config.yaml`: its notes vault is searched.
    NoOkf { drive: String },
    /// The drive's `.okf/config.yaml` is there and keeper cannot read it or
    /// cannot interpret it: nothing of the drive is searched, since what it
    /// excludes is unknown.
    UnreadableOkf { drive: String, why: String },
    /// No notes index on this host: the vault was scanned.
    NoIndex { drive: String },
    /// The vault's index opened and could not answer: the vault was
    /// scanned.
    IndexUnusable { drive: String, why: String },
    /// The call's files, bytes or time were spent before the vault's index
    /// could be opened: it was not.
    IndexCapped { drive: String },
    /// The vault's index answered, and nothing says it holds every note as
    /// it is now; the scan that reads the vault beside it stopped before it
    /// had read it all, so the vault is covered only in part.
    IndexPartial { drive: String },
    /// Folders that may be searched and could not be read whole: what they
    /// hold is unknown, and was not searched.
    Unlisted { drive: String, folders: usize },
    /// The session's label keeps the query off a remote embeddings model.
    StaysLocal,
    /// The embeddings model did not answer within [`EMBED_MILLIS`].
    EmbedLate,
    /// The embeddings model could not be asked.
    EmbedFailed,
    /// The call's time or bytes ran out while the index ranked by meaning.
    MeaningCapped,
    /// The rendered result reached [`RESULT_BYTES`].
    Cut { shown: usize, of: usize },
}

impl Said {
    /// keeper's sentence.
    pub fn sentence(&self) -> String {
        match self {
            Said::OutOfScope { drive } => {
                format!("{drive} is not a drive this session may search; nothing was searched.")
            }
            Said::ScanCapped {
                drive,
                searched,
                of: Some(of),
            } => format!(
                "searched {} of {} files in {drive}; stopped at the cap",
                grouped(*searched),
                grouped(*of)
            ),
            Said::ScanCapped {
                drive,
                searched,
                of: None,
            } => format!(
                "searched {} files in {drive}; stopped at the cap before it had counted them all",
                grouped(*searched)
            ),
            Said::Incomplete { drive } => format!(
                "{drive}: the search's {} ran out before every ranked or listed document was read; the rest were not opened",
                bounds_words()
            ),
            Said::Skipped { drive, files } => format!(
                "{drive}: {} files were not searched — not text, larger than {}, or unreadable",
                grouped(*files),
                size_words(MAX_FILE_BYTES)
            ),
            Said::NoOkf { drive } => {
                format!("{drive}: this drive has no OKF configuration; searched its notes only")
            }
            Said::UnreadableOkf { drive, why } => format!(
                "{drive}: its OKF configuration could not be read ({why}); nothing in it was searched"
            ),
            Said::NoIndex { drive } => format!("{drive}: lexical: no notes index on this host"),
            Said::IndexUnusable { drive, why } => format!(
                "{drive}: lexical: its notes index could not answer ({why}); its notes were scanned"
            ),
            Said::IndexCapped { drive } => format!(
                "{drive}: the search's {} ran out before its notes index was opened",
                bounds_words()
            ),
            Said::IndexPartial { drive } => format!(
                "{drive}: its notes index may not hold every note as it is now, and the scan of its notes stopped before it had read them all: its notes are covered only in part"
            ),
            Said::Unlisted { drive, folders } => format!(
                "{drive}: {} folders could not be read; what they hold was not searched",
                grouped(*folders)
            ),
            Said::StaysLocal => "lexical: this session stays on local models".to_owned(),
            Said::EmbedLate => format!(
                "lexical: the embeddings model did not answer within {} s",
                EMBED_MILLIS / 1000
            ),
            Said::EmbedFailed => "lexical: the embeddings model could not be asked".to_owned(),
            Said::MeaningCapped => {
                "lexical: the search's time or bytes ran out while it ranked by meaning".to_owned()
            }
            Said::Cut { shown, of } => format!(
                "the result reached {}: {shown} of {of} hits are shown",
                size_words(RESULT_BYTES as u64)
            ),
        }
    }
}

/// `per_drive`'s hits, each drive's in its own order, taken in turns and
/// cut at `k`, so one drive cannot crowd out another; a path found twice
/// keeps its first place and gains the other's lines.
pub fn merge(per_drive: Vec<Vec<Hit>>, k: usize) -> Vec<Hit> {
    let mut queues: Vec<std::collections::VecDeque<Hit>> =
        per_drive.into_iter().map(Into::into).collect();
    let mut out: Vec<Hit> = Vec::new();
    while out.len() < k && queues.iter().any(|queue| !queue.is_empty()) {
        for queue in &mut queues {
            if out.len() >= k {
                break;
            }
            while let Some(hit) = queue.pop_front() {
                match out
                    .iter_mut()
                    .find(|seen| seen.drive == hit.drive && seen.path == hit.path)
                {
                    Some(seen) => {
                        for line in hit.lines {
                            if seen.lines.len() < LINES_PER_HIT && !seen.lines.contains(&line) {
                                seen.lines.push(line);
                            }
                        }
                    }
                    None => {
                        out.push(hit);
                        break;
                    }
                }
            }
        }
    }
    out
}

fn label_words(label: &Label) -> String {
    let readers = match &label.readers {
        Readers::Anyone => "anyone".to_owned(),
        Readers::Only(set) if set.is_empty() => "no one".to_owned(),
        Readers::Only(set) => set
            .iter()
            .map(|user| user.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    };
    let local = if label.local_only {
        "; local models only"
    } else {
        ""
    };
    format!(
        "read by {readers}; {} integrity{local}",
        label.integrity.as_word()
    )
}

fn render_hit(at: usize, hit: &Hit) -> String {
    let mut out = format!("{at}. {}/{} — {}", hit.drive, hit.path, hit.title);
    if let Some(kind) = &hit.kind {
        out.push_str(&format!(" ({kind})"));
    }
    out.push('\n');
    out.push_str(&format!("   label: {}\n", label_words(&hit.label)));
    if let Some(size) = hit.absent {
        out.push_str(&format!("   not on this device ({})\n", size_words(size)));
    }
    for (line, text) in &hit.lines {
        out.push_str(&format!("   {line}: {text}\n"));
    }
    out
}

/// The result the model reads: a head line, keeper's sentences, then each
/// hit — its drive and path, title, OKF type, label and matching lines —
/// at most [`RESULT_BYTES`] of it, saying so when that cut it.
pub fn render(query: &Query, hits: &[Hit], said: &[Said]) -> String {
    let mut head = format!(
        "drive_search \"{}\": {} result{}.\n",
        query.text(),
        hits.len(),
        if hits.len() == 1 { "" } else { "s" }
    );
    let rendered: Vec<String> = hits
        .iter()
        .enumerate()
        .map(|(at, hit)| render_hit(at + 1, hit))
        .collect();
    let sentences: String = said.iter().map(|s| format!("{}\n", s.sentence())).collect();
    let budget = RESULT_BYTES.saturating_sub(head.len() + sentences.len() + 128);
    let mut used = 0;
    let shown = rendered
        .iter()
        .take_while(|text| {
            used += text.len();
            used <= budget
        })
        .count();
    head.push_str(&sentences);
    if shown < rendered.len() {
        head.push_str(
            &Said::Cut {
                shown,
                of: rendered.len(),
            }
            .sentence(),
        );
        head.push('\n');
    }
    head.push('\n');
    for text in &rendered[..shown] {
        head.push_str(text);
    }
    head
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::agents::label::Integrity;

    fn hit(drive: &str, path: &str, lines: Vec<(usize, String)>) -> Hit {
        Hit {
            drive: drive.to_owned(),
            path: path.to_owned(),
            title: path.to_owned(),
            kind: None,
            lines,
            absent: None,
            label: Label {
                readers: Readers::Only(BTreeSet::new()),
                integrity: Integrity::Agent,
                local_only: false,
            },
            found: Found::Scan,
        }
    }

    /// AD-159: `k` is 10 unless asked and never above 25; a line is cut at
    /// 240 characters with `…`; the rendered result never passes 80 KiB,
    /// says how many it shows when that cut it, and carries every fact said
    /// beside the hits.
    #[test]
    fn results_are_bounded_and_the_cap_is_said() {
        assert_eq!(k_of(None), 10);
        assert_eq!(k_of(Some(0)), 10);
        assert_eq!(k_of(Some(7)), 7);
        assert_eq!(k_of(Some(25)), 25);
        assert_eq!(k_of(Some(100)), 25);
        let long = "ą".repeat(300);
        let clipped = clip_line(&long);
        assert_eq!(clipped.chars().count(), 241);
        assert!(clipped.ends_with('…'));
        assert_eq!(clip_line(&"x".repeat(240)), "x".repeat(240));
        let query = Query::parse("needle").expect("a query");
        let fat: Vec<Hit> = (0..200)
            .map(|n| {
                hit(
                    "tgdrive",
                    &format!("{n}.md"),
                    vec![(1, "y".repeat(LINE_CHARS)); 3],
                )
            })
            .collect();
        let capped = Said::ScanCapped {
            drive: "tgdrive".to_owned(),
            searched: 2000,
            of: Some(5312),
        };
        let text = render(&query, &fat, std::slice::from_ref(&capped));
        assert!(text.len() <= RESULT_BYTES, "{}", text.len());
        assert!(text.contains(&capped.sentence()));
        let shown = text.matches("\n   label: ").count();
        assert!(shown < 200);
        let cut = Said::Cut { shown, of: 200 };
        assert!(text.contains(&cut.sentence()), "{shown}");
        let small = render(&query, &fat[..2], &[]);
        assert_eq!(small.matches("\n   label: ").count(), 2);
    }

    /// A query is literal: `a.*b` matches those four characters and not
    /// `a` and `b` apart; terms fold case and accents, and every term must
    /// occur somewhere in the file.
    #[test]
    fn a_query_is_literal_terms() {
        let query = Query::parse("a.*b").expect("a query");
        assert!(query.in_line("see a.*b here"));
        assert!(!query.in_line("a then b"));
        assert!(!query.in_line("axxb"));
        let folded = Query::parse("ŁÓDŹ plan").expect("a query");
        assert!(folded.found_in(&["the lodz", "a PLAN"]));
        assert!(!folded.found_in(&["the lodz"]));
        assert_eq!(
            folded.lines("x\nłódź\ny\nplan\nplan\nplan\n"),
            vec![
                (2, "łódź".to_owned()),
                (4, "plan".to_owned()),
                (5, "plan".to_owned())
            ]
        );
        assert!(Query::parse(" .* ").is_none());
    }

    /// Two drives' hits alternate, so neither crowds out the other at `k`;
    /// a path met twice is one result with the other's lines.
    #[test]
    fn merged_drives_take_turns() {
        let a: Vec<Hit> = (0..5)
            .map(|n| hit("a", &format!("{n}.md"), vec![]))
            .collect();
        let b = vec![
            hit("b", "x.md", vec![(1, "one".to_owned())]),
            hit("b", "x.md", vec![(4, "four".to_owned())]),
            hit("b", "y.md", vec![]),
        ];
        let merged = merge(vec![a, b], 4);
        let names: Vec<String> = merged
            .iter()
            .map(|hit| format!("{}/{}", hit.drive, hit.path))
            .collect();
        assert_eq!(names, ["a/0.md", "b/x.md", "a/1.md", "b/y.md"]);
        assert_eq!(merged[1].lines.len(), 2);
    }
}
