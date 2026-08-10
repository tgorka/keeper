//! How many a list holds, and how many of those it will actually let you reach
//! (Story 44.11, FR-166).
//!
//! Story 44.10 windowed the note list, the recordings archive and the Files
//! tree. A windowed list knows two numbers that look identical on screen — how
//! many rows are mounted right now, and how many rows exist — and after 44.10
//! they differ by two orders of magnitude. A surface that prints the first
//! while looking like the second is worse than a surface with no count at all:
//! it is a wrong answer wearing the shape of a right one, and nothing on screen
//! says which it is. So every count in this app is a count of what EXISTS,
//! taken where the whole set is in hand and never derived from what was
//! rendered or from what a page happened to ship.
//!
//! For two of the three surfaces that is the end of it — the set is the set.
//! The note list has one complication, and it is the reason this module exists
//! rather than a bare `len()` at each call site.
//!
//! ## `keeper.limit` caps SELECTION, not rendering
//!
//! A space's `keeper.limit` has been read out of frontmatter, shipped to the
//! frontend and written back on every save since Story 37.4, and nothing has
//! ever applied it (DW-163). Applying it needs a decision first, because the
//! two readings are not the same feature:
//!
//! - **A rendering cap** would bound how many rows are drawn. 44.10 already
//!   bounds that, by the viewport, for every list — so a second render cap
//!   would only bound *what can be scrolled to*, which is a selection cap
//!   wearing a disguise. And the transport window already has an owner:
//!   `NoteQueryReq.limit`, the page the frontend grows as it scrolls. Two
//!   answers to one question is the second convention the house style forbids.
//! - **A selection cap** is a thing a person can mean and cannot say any other
//!   way: *this space holds the twenty most recent, and the sort decides which
//!   twenty*. It composes with `keeper.sort` — the cap is applied AFTER the
//!   ordering, so the space keeps the twenty the sort put first, not twenty
//!   arbitrary matches.
//!
//! So: selection. A capped space genuinely holds `total` notes, and `total` is
//! what its count shows, because a count of 347 on a space that will only ever
//! hand back 20 names a set nobody can reach.
//!
//! ## …and a cap that bites is never silent
//!
//! "A limit that silently truncates a count is the same defect as a count of
//! the rendered window", and it would be, so it is not silent in either place
//! it could be:
//!
//! - On screen, [`Selection::matched`] travels beside [`Selection::total`], so
//!   the surface says `20 of 347 notes` rather than `20 notes`.
//! - In the log, [`Selection::report`] words the decline at [`REPORT_FLOOR`].
//!   Declining to select notes the query matched is this story's one way of
//!   doing nothing, and a story that can decline to act and says so only at
//!   `debug!` says nothing at all on the owner's machine (DW-162).
//!
//! Pure, like the rest of `keeper::notes`: numbers in, numbers and sentences
//! out, no connection, no clock and no vault.

use std::ops::Range;

/// The level this module's one report is emitted at, and the floor every
/// variant of it must clear.
///
/// The desktop app installs `EnvFilter::new("info")` and no `RUST_LOG` is set
/// anywhere in the bundle, so anything below `INFO` is written to a log nobody
/// can read (DW-162). Pinned as a constant here, beside the decision it
/// describes, so the choice is asserted rather than left to whichever `tracing::`
/// macro a call site reached for — the mistake Story 44.3 made twice.
pub const REPORT_FLOOR: tracing::Level = tracing::Level::INFO;

/// The two numbers a list has to be able to tell apart (Story 44.11).
///
/// Both are counts of notes that EXIST. Neither is ever a count of rows that
/// were rendered, shipped in a page, or measured by a window — those are
/// 44.10's business and no surface may present one of them as this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// How many the query matched, before the space's own cap.
    ///
    /// Equal to [`Selection::total`] whenever no cap is in force, which is
    /// every list outside a space and every space that sets no `keeper.limit`.
    pub matched: u32,
    /// How many the space actually selects — [`Selection::matched`] capped by
    /// `keeper.limit`.
    ///
    /// This is the number a count leads with, because it is the number of notes
    /// a person can reach. It is also what paginates: a caller must never be
    /// able to page past the cap into notes the space declined.
    pub total: u32,
}

impl Selection {
    /// Whether the cap actually declined notes the query matched.
    ///
    /// A space whose `keeper.limit` is 500 and whose query matches 12 has a cap
    /// and is not capped. The distinction is the whole of when a surface says
    /// `12 notes` and when it says `500 of 4 210 notes`, and of when the log
    /// has anything to report.
    #[must_use]
    pub fn capped(self) -> bool {
        self.total < self.matched
    }

    /// What the log must say when this selection declined notes, or `None` when
    /// it declined none.
    ///
    /// The level lives here rather than at the `tracing::` call site so it can
    /// be asserted against [`REPORT_FLOOR`] without a Tauri app — the shape
    /// [`crate::notes::default_spaces::SeedOutcome::report`] established after
    /// the third field report on a run that logged nothing.
    ///
    /// `None` rather than an `INFO` line saying "nothing was dropped": this runs
    /// on every list read, and a log that repeats a non-event on every keystroke
    /// is a log the events get lost in.
    #[must_use]
    pub fn report(self, space: &str) -> Option<(tracing::Level, String)> {
        if !self.capped() {
            return None;
        }
        Some((
            REPORT_FLOOR,
            format!(
                "space \"{space}\" matched {matched} notes and selects {total}: \
                 keeper.limit declined the other {declined}, and no surface will \
                 list them",
                matched = self.matched,
                total = self.total,
                declined = self.matched - self.total,
            ),
        ))
    }
}

/// Apply a space's cap to a matched set.
///
/// `matched` is the size of the whole matched set — every entry the query
/// admitted, counted before any window, page or offset — and saturates rather
/// than wrapping, because a vault larger than `u32::MAX` notes should show a
/// preposterous number rather than a small wrong one.
///
/// `limit` is [`read_limit`]'s answer: `None` for a space that sets no cap,
/// which is the common case and leaves both numbers equal.
#[must_use]
pub fn select(matched: usize, limit: Option<u32>) -> Selection {
    let matched = u32::try_from(matched).unwrap_or(u32::MAX);
    Selection {
        matched,
        total: match limit {
            Some(cap) => matched.min(cap),
            None => matched,
        },
    }
}

/// Which slice of the ordered match set one read carries.
///
/// **The cap is applied before the offset, and that is the whole of this
/// function.** A caller holding a page size and an offset can walk as far as it
/// likes; if the offset were applied to the matched set and the cap to the page,
/// a space capped at twenty would hand back its notes 21–40 on the second read
/// and the cap would be decoration again. Bounded by
/// [`Selection::total`] — what the space SELECTS — so paging past the cap
/// returns nothing rather than the notes the space declined.
///
/// The returned range is over the ordered match set, so `start` is also the
/// offset the caller should echo back: an offset past the end clamps to the end
/// rather than reporting a position that does not exist.
#[must_use]
pub fn page(selection: Selection, offset: u32, size: u32) -> Range<usize> {
    let start = offset.min(selection.total);
    let end = start.saturating_add(size).min(selection.total);
    (start as usize)..(end as usize)
}

/// The cap a space's `keeper.limit` sets, or `None` for a space that sets none.
///
/// **Absent and zero are the same answer, and that is a repair.** Until this
/// story the reader mapped a missing, zero or negative `limit` onto the shell's
/// `MAX_LIMIT` — so "this space sets no cap" and "this space caps at 500" were
/// the same value, and the space editor, which round-trips a value it does not
/// render, wrote `limit: 500` into the frontmatter of every space that was
/// saved once and had never had the key. Now unset is `None`, `None` writes
/// nothing back, and a space grows no key to explain a cap it does not have.
///
/// **Not clamped to the transport window.** `NoteQueryReq.limit` is how many
/// rows one page carries and has its own ceiling; this is how many notes the
/// space contains. Clamping a `limit: 2000` space down to a page size would
/// silently drop 1 500 notes, which is the exact defect this story exists to
/// remove — the frontend pages through 2 000 the same way it pages through
/// 20 000.
///
/// Anything that is not at least one whole note is no cap: a negative, a NaN,
/// an infinity, and `0.5`, which is a typo rather than a request for half a
/// note. A fraction above one truncates toward zero — `42.9` is a cap of 42,
/// because a space cannot hold nine tenths of a note and rounding up would
/// admit one the file did not ask for.
#[must_use]
pub fn read_limit(raw: f64) -> Option<u32> {
    if !raw.is_finite() || raw < 1.0 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let capped = raw.min(f64::from(u32::MAX)) as u32;
    Some(capped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_space_with_no_cap_selects_everything_it_matched() {
        let selection = select(347, None);
        assert_eq!(selection.matched, 347);
        assert_eq!(selection.total, 347);
        assert!(!selection.capped());
        assert_eq!(selection.report("Inbox"), None);
    }

    #[test]
    fn a_cap_larger_than_the_match_is_a_cap_that_did_not_bite() {
        let selection = select(12, Some(500));
        assert_eq!(selection.total, 12);
        assert!(!selection.capped());
        assert_eq!(
            selection.report("Inbox"),
            None,
            "a cap nobody reached is not an event"
        );
    }

    #[test]
    fn a_cap_that_bites_keeps_both_numbers_so_nothing_is_silent() {
        let selection = select(347, Some(20));
        assert_eq!(selection.total, 20, "the space genuinely holds twenty");
        assert_eq!(
            selection.matched, 347,
            "and the surface can still say how many it turned away"
        );
        assert!(selection.capped());
    }

    #[test]
    fn a_cap_exactly_at_the_match_declines_nothing() {
        let selection = select(20, Some(20));
        assert!(!selection.capped());
        assert_eq!(selection.report("Recent"), None);
    }

    #[test]
    fn a_decline_is_reported_where_the_app_can_actually_print_it() {
        // Pinned to the literal level, not merely to `REPORT_FLOOR`. Comparing
        // the report against the constant alone is vacuous the moment somebody
        // lowers the constant to make a `debug!` fit — the round-2 defect
        // DW-162 records, and the reason Story 44.3's equivalent test is
        // written this way.
        assert_eq!(
            REPORT_FLOOR,
            tracing::Level::INFO,
            "the desktop subscriber's default filter is `info`; a floor below \
             it means this whole test asserts nothing"
        );
        let (level, message) = select(347, Some(20))
            .report("Inbox")
            .expect("a cap that dropped 327 notes has something to say");
        assert_eq!(level, tracing::Level::INFO);
        assert!(message.contains("Inbox"), "{message}");
        assert!(message.contains("347"), "{message}");
        assert!(message.contains("20"), "{message}");
        assert!(
            message.contains("327"),
            "the count that matters is how many were declined: {message}"
        );
    }

    #[test]
    fn a_page_is_carved_out_of_what_the_space_selected_and_never_past_it() {
        // Story 44.11 / DW-163. A caller paging with a window of 20 must not be
        // able to walk into the 327 notes the cap declined: the cap is applied
        // before the offset, so the second page of a space capped at 20 is
        // empty rather than notes 21–40.
        let capped = select(347, Some(20));
        assert_eq!(page(capped, 0, 60), 0..20, "the first read gets all twenty");
        assert_eq!(page(capped, 20, 60), 20..20, "and there is no second read");
        assert_eq!(
            page(capped, 300, 60),
            20..20,
            "an offset past the cap clamps"
        );
    }

    #[test]
    fn an_uncapped_page_walks_the_whole_matched_set() {
        let whole = select(347, None);
        assert_eq!(page(whole, 0, 200), 0..200);
        assert_eq!(page(whole, 200, 200), 200..347);
        assert_eq!(page(whole, 347, 200), 347..347);
    }

    #[test]
    fn a_page_size_that_would_overflow_stops_at_the_end() {
        // `offset + size` is where an arithmetic slip becomes a panic in
        // release and a wrong slice in debug.
        assert_eq!(page(select(5, None), 3, u32::MAX), 3..5);
        assert_eq!(page(select(0, None), 0, 60), 0..0);
    }

    #[test]
    fn an_unset_limit_is_no_cap_rather_than_a_cap_of_zero() {
        assert_eq!(read_limit(0.0), None);
        assert_eq!(read_limit(-5.0), None);
        assert_eq!(read_limit(0.5), None);
        assert_eq!(read_limit(f64::NAN), None);
        assert_eq!(read_limit(f64::INFINITY), None);
        assert_eq!(read_limit(f64::NEG_INFINITY), None);
    }

    #[test]
    fn a_limit_is_taken_at_its_word_and_not_clamped_to_a_page() {
        assert_eq!(read_limit(1.0), Some(1));
        assert_eq!(read_limit(20.0), Some(20));
        assert_eq!(read_limit(42.9), Some(42), "a space cannot hold 0.9 notes");
        assert_eq!(
            read_limit(2_000.0),
            Some(2_000),
            "the page size is not the space's business"
        );
        assert_eq!(read_limit(1e30), Some(u32::MAX));
    }

    #[test]
    fn a_match_larger_than_u32_saturates_rather_than_wrapping() {
        // Unreachable from a vault and reachable from a bug; a small wrong
        // number is the one outcome a count must never produce.
        let selection = select(usize::MAX, None);
        assert_eq!(selection.matched, u32::MAX);
        assert_eq!(selection.total, u32::MAX);
    }
}
