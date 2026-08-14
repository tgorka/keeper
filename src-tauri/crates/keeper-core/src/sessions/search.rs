//! Content search across one sessions zone (FR-267).
//!
//! A zone is not a vault and can never be one — a subfolder flagged as both is
//! refused at profile validation, because the notes indexer and the sessions
//! indexer would each claim the same markdown — so `notes_search` cannot reach
//! a session file no matter which id it is handed. This module is the other
//! half of the answer to "search everywhere": the same matcher
//! ([`crate::notes::search::find`], with its case and diacritic folding) run
//! over the pool the sessions scan already reads.
//!
//! The matcher is reused rather than reimplemented for AD-20's reason: two
//! implementations of "does this line match" drift, and the one that drifts is
//! always the one with fewer tests. What is decided *here* is everything the
//! matcher does not decide — which files are searched, in what order results
//! come back, and when the scan stops.
//!
//! **Order is the session's, then the file's.** Sessions arrive in the order
//! the board already put them in (newest first), and within a session files
//! arrive in name order, which for a flat pool is also date order. Nothing
//! re-ranks by match count or by "relevance": a list that reorders itself as
//! more results stream in is a list you cannot click, and the operator's own
//! question is *which session was that in* — an order they already know beats a
//! score they cannot see.

use crate::notes::search::find;
use crate::sessions::vm::SessionSearchHitVm;

/// The most hits one scan will produce when the caller asks for no limit.
///
/// A ceiling rather than a default, so a caller cannot ask for more: the hits
/// cross an IPC boundary and land in a list a person scrolls, and nobody reads
/// the two-thousandth line that contains the word "the".
pub const MAX_HITS: usize = 500;

/// Which session a batch of files belongs to, gathered once per session rather
/// than re-passed per file.
///
/// A borrowed triple rather than five positional arguments on
/// [`Scan::push_file`]: the three travel together, they are read from one row,
/// and a call site that transposed `id` and `title` would compile.
#[derive(Debug, Clone, Copy)]
pub struct Session<'a> {
    /// The session's id, as [`crate::sessions::vm::SessionRowVm::id`] spells it.
    pub id: &'a str,
    /// Its title, carried so a hit row needs no second lookup.
    pub title: &'a str,
    /// Profile-relative path of the session folder — `<zone>/<session path>` —
    /// which every hit's `subpath` is composed against (AD-65).
    pub prefix: &'a str,
}

/// A running scan: what is left of the hit budget, and the hits so far.
///
/// Carries the budget rather than checking a count at each call site because
/// "did we stop early" is one fact and it belongs in one place —
/// [`Scan::exhausted`] is what a caller flushes on, and it cannot disagree with
/// the ceiling that produced it.
#[derive(Debug)]
pub struct Scan {
    remaining: usize,
    hits: Vec<SessionSearchHitVm>,
}

impl Scan {
    /// A scan bounded by `limit`, clamped to [`MAX_HITS`]; `0` means the
    /// ceiling, which is how the notes request already spells "no preference".
    pub fn new(limit: usize) -> Self {
        let remaining = if limit == 0 {
            MAX_HITS
        } else {
            limit.min(MAX_HITS)
        };
        Self {
            remaining,
            hits: Vec::new(),
        }
    }

    /// Whether the budget is spent — the caller stops walking when it is.
    pub fn exhausted(&self) -> bool {
        self.remaining == 0
    }

    /// How many hits this scan is still willing to take.
    pub fn remaining(&self) -> usize {
        self.remaining
    }

    /// Match one file's text, appending every hit the budget still allows.
    ///
    /// Takes the session's title as well as its id because the row already has
    /// both and the hit list would otherwise need a second lookup per row to
    /// render — a lookup that would be against a mirror the scan does not own,
    /// and so a lookup that can disagree with the hit it is labelling. The
    /// openable path is composed here for the same reason it is composed for
    /// every other session row: the frontend joins no paths (AD-65).
    pub fn push_file(&mut self, session: Session<'_>, file: &str, text: &str, needle: &str) {
        if self.remaining == 0 {
            return;
        }
        for hit in find(text, needle, self.remaining) {
            self.hits.push(SessionSearchHitVm {
                session_id: session.id.to_owned(),
                session_title: session.title.to_owned(),
                file: file.to_owned(),
                subpath: format!("{}/{file}", session.prefix),
                line: hit.line,
                snippet: hit.snippet,
            });
            self.remaining -= 1;
        }
    }

    /// Take everything found so far, leaving the scan empty and its budget
    /// intact — how a batch is flushed mid-walk without ending the walk.
    pub fn take(&mut self) -> Vec<SessionSearchHitVm> {
        std::mem::take(&mut self.hits)
    }

    /// How many hits are waiting to be flushed.
    pub fn pending(&self) -> usize {
        self.hits.len()
    }
}

/// Whether a needle is worth walking a zone for.
///
/// Empty and whitespace-only queries answer nothing rather than everything: the
/// alternative is that clearing the field paints every line of every session,
/// which is both the slowest thing the scan can do and the least useful.
pub fn searchable(needle: &str) -> bool {
    !needle.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in session, so a test that is not about identity says so.
    fn one() -> Session<'static> {
        Session {
            id: "s1",
            title: "one",
            prefix: "60-sessions/active/2026-08-14-one",
        }
    }

    #[test]
    fn a_hit_names_its_session_as_well_as_its_file() {
        let mut scan = Scan::new(10);
        scan.push_file(
            Session {
                id: "01J5AAAAAAAAAAAAAAAAAAAAAA",
                title: "keeper — rolling work session",
                prefix: "60-sessions/active/2026-08-14-keeper",
            },
            "about.md",
            "state as of opening\nthe migration is done\n",
            "migration",
        );
        let hits = scan.take();
        assert_eq!(hits.len(), 1);
        // `about.md` names nothing on its own — every session has one.
        assert_eq!(hits[0].session_title, "keeper — rolling work session");
        assert_eq!(hits[0].file, "about.md");
        assert_eq!(hits[0].line, 2);
    }

    #[test]
    fn a_hit_carries_the_path_that_opens_it_so_the_frontend_joins_nothing() {
        let mut scan = Scan::new(10);
        scan.push_file(one(), "2026-08-14-1030-plan.md", "the plan\n", "plan");
        let hits = scan.take();
        assert_eq!(
            hits[0].subpath,
            "60-sessions/active/2026-08-14-one/2026-08-14-1030-plan.md"
        );
        // The displayed path stays session-relative: a result list that showed
        // the whole zone path in every row would be a column of shared prefix.
        assert_eq!(hits[0].file, "2026-08-14-1030-plan.md");
    }

    #[test]
    fn the_budget_is_spent_across_files_and_not_per_file() {
        let mut scan = Scan::new(3);
        scan.push_file(one(), "a.md", "x\nx\n", "x");
        assert_eq!(scan.remaining(), 1);
        scan.push_file(one(), "b.md", "x\nx\nx\n", "x");
        // Two from the first file, one from the second, then nothing.
        assert!(scan.exhausted());
        assert_eq!(scan.pending(), 3);
        let files: Vec<&str> = scan.hits.iter().map(|hit| hit.file.as_str()).collect();
        assert_eq!(files, ["a.md", "a.md", "b.md"]);
    }

    #[test]
    fn a_spent_scan_reads_nothing_more() {
        let mut scan = Scan::new(1);
        scan.push_file(one(), "a.md", "x\n", "x");
        assert!(scan.exhausted());
        scan.push_file(one(), "b.md", "x\nx\nx\n", "x");
        assert_eq!(scan.pending(), 1);
    }

    #[test]
    fn a_flush_empties_the_hits_and_leaves_the_budget_alone() {
        let mut scan = Scan::new(10);
        scan.push_file(one(), "a.md", "x\n", "x");
        let before = scan.remaining();
        assert_eq!(scan.take().len(), 1);
        assert_eq!(scan.pending(), 0);
        assert_eq!(scan.remaining(), before);
        // Still usable: a flush is a batch boundary, not an end.
        scan.push_file(one(), "b.md", "x\n", "x");
        assert_eq!(scan.pending(), 1);
    }

    #[test]
    fn a_limit_of_zero_is_the_ceiling_and_a_huge_one_is_clamped_to_it() {
        assert_eq!(Scan::new(0).remaining(), MAX_HITS);
        assert_eq!(Scan::new(usize::MAX).remaining(), MAX_HITS);
        assert_eq!(Scan::new(7).remaining(), 7);
    }

    #[test]
    fn an_empty_query_is_not_searchable_so_clearing_the_field_paints_nothing() {
        assert!(!searchable(""));
        assert!(!searchable("   \n\t"));
        assert!(searchable("migration"));
    }

    #[test]
    fn matching_is_the_notes_matcher_so_case_and_accents_fold_the_same_way() {
        let mut scan = Scan::new(10);
        scan.push_file(one(), "a.md", "Référence to the PLAN\n", "reference");
        // Not a second matcher: whatever `notes::search::find` folds, this folds
        // (AD-20). A zone search that disagreed with a vault search about what
        // "matches" would be two products wearing one keystroke.
        assert_eq!(scan.pending(), 1);
    }
}
