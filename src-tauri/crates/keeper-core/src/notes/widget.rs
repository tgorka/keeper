//! Three views of a note query, embeddable in any note (FR-264).
//!
//! A session's board, log and reference list turned out to be nothing a session
//! owns. Each one is "run a query, draw the rows this way" — and the operator
//! asked for exactly that in ordinary notes too: *"the trello like task view
//! should be the widget inside the md file I could use in the notes as well -
//! not only in the sessions"*. So the three live here, in `notes`, and the
//! sessions detail is one caller among two.
//!
//! # One query, three renderings
//!
//! There is deliberately **no per-kind row type**. A task, a log entry and a
//! reference are all notes; what differs is which facts the view draws and how
//! it sorts them. So this module answers one [`WidgetRow`] shape carrying every
//! fact the three need, plus the per-kind [`WidgetKind::compare`] that decides
//! their sequence. A second row type per kind would be three projections to keep
//! in step for no fact any of them holds alone.
//!
//! # The marker set is defined once, in Rust
//!
//! [`WidgetKind`] is `ts-rs`-exported, so the TypeScript that scans a document
//! for `> [!board]` builds its pattern from a `readonly WidgetKind[]` and the
//! compiler refuses both a variant added here and forgotten there, and a word
//! invented there that means nothing here. A regex spelling the three words
//! independently is the version of this that goes stale silently.
//!
//! # The argument is a query, and Rust composes it
//!
//! `> [!board] tag:task path:projects/**` filters; `> [!board]` alone takes the
//! kind's default. Nothing splices a string on the TypeScript side (AD-65): the
//! frontend sends the callout's argument verbatim and this module decides what
//! that means, which is also why a widget in a note and a session's own board
//! cannot drift apart in what they select.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::notes::index::IndexEntry;
use crate::notes::order::NoteOrderSource;
use crate::notes::search;

/// The frontmatter key a board reads a card's column from — the same key the
/// session board writes ([`crate::sessions::tasks::TASK_STATUS_KEY`]), because
/// a task dragged in a note and a task dragged in a session are the same file.
pub const WIDGET_STATUS_KEY: &str = "status";

/// The three views a callout can name.
///
/// Closed, like `IS_FLAGS` and [`crate::sessions::shape::TaskStatus`]: an
/// unknown marker is not a widget, and `> [!warning]` must keep rendering as
/// Obsidian's own callout rather than as a broken keeper block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum WidgetKind {
    /// Four columns of `status:`, dragged between (FR-263).
    Board,
    /// Newest first — a running record rather than a list.
    Log,
    /// What this note points at, alphabetically.
    Refs,
}

/// Every kind, in the order a picker offers them. Board first: it is the one
/// the operator asked for by name.
pub const WIDGET_KINDS: [WidgetKind; 3] = [WidgetKind::Board, WidgetKind::Log, WidgetKind::Refs];

impl WidgetKind {
    /// The word inside the callout marker, lowercase.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Board => "board",
            Self::Log => "log",
            Self::Refs => "refs",
        }
    }

    /// The kind a marker names, or `None` for a word that is not one.
    ///
    /// Case-insensitive because `gallery-block.rs`'s callout head already is:
    /// Obsidian writes `[!NOTE]` as readily as `[!note]`, and a widget that
    /// worked only in lowercase would look broken for a reason nobody could see.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        WIDGET_KINDS
            .into_iter()
            .find(|kind| kind.as_str().eq_ignore_ascii_case(word.trim()))
    }

    /// What the kind selects when the callout names no query of its own.
    ///
    /// The three tags the flat session shape already uses (FR-256), so a widget
    /// pasted into a session's `about.md` with no argument shows that session's
    /// tasks — which is the case the operator will hit first.
    #[must_use]
    pub fn default_query(self) -> &'static str {
        match self {
            Self::Board => "tag:task",
            Self::Log => "tag:log",
            Self::Refs => "tag:ref",
        }
    }

    /// The query this widget actually runs.
    ///
    /// Composed here rather than in the webview (AD-65). A blank argument is the
    /// default rather than "select everything": a `> [!board]` that drew every
    /// note in the vault as an unplaced card would be a wall of strays, and the
    /// operator typed three characters, not a query.
    #[must_use]
    pub fn effective_query(self, argument: &str) -> String {
        let trimmed = argument.trim();
        if trimmed.is_empty() {
            self.default_query().to_owned()
        } else {
            trimmed.to_owned()
        }
    }

    /// The sequence this kind draws its rows in.
    ///
    /// Each is the ordering that kind's *own* surface already uses, so a widget
    /// and the session pane it mirrors never disagree:
    ///
    /// * **Board** — `order` ascending, the position a drag writes
    ///   ([`crate::notes::order::cmp_order`]).
    /// * **Log** — path descending, which for `YYYY-MM-DD-HHMM-slug.md` is
    ///   newest-first without parsing a date out of a filename. A log sorted by
    ///   mtime would reshuffle every time an old entry was corrected.
    /// * **Refs** — folded title ascending, the same "alphabetical" search and a
    ///   space's `name` sort mean.
    #[must_use]
    pub fn compare(self, a: &WidgetRow, b: &WidgetRow) -> Ordering {
        match self {
            Self::Board => a
                .order
                .total_cmp(&b.order)
                .then_with(|| search::fold_cmp(&a.title, &b.title))
                .then_with(|| a.path.cmp(&b.path)),
            Self::Log => b.path.cmp(&a.path),
            Self::Refs => search::fold_cmp(&a.title, &b.title).then_with(|| a.path.cmp(&b.path)),
        }
    }
}

/// One selected note, carrying what all three views draw between them.
///
/// `status` is the file's own word rather than a parsed column. A board shows a
/// card whose `status:` is none of the four in its own section, keeping that
/// word — a task nobody can see is worse than a task in the wrong place, and
/// collapsing "blocked" to `null` would throw away the only thing that explains
/// why the card is there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WidgetRow {
    /// The note's id — what opens it, and what a move addresses.
    pub id: String,
    /// Vault-relative path, shown when a title is not enough to tell two apart.
    pub path: String,
    pub title: String,
    /// The list snippet, which is what a log entry and a reference are read by.
    pub snippet: String,
    pub tags: Vec<String>,
    /// Last modification, ms since the epoch.
    #[ts(type = "number")]
    pub updated_ms: i64,
    /// Frontmatter `status`, verbatim and trimmed; `null` when the note is
    /// silent. Only a board draws it.
    #[ts(type = "string | null")]
    pub status: Option<String>,
    /// Position within a board column.
    pub order: f64,
    /// The note stated that position itself, rather than taking the default.
    pub order_is_own: bool,
}

/// Project one index entry into a widget row.
#[must_use]
pub fn row_of(entry: &IndexEntry) -> WidgetRow {
    WidgetRow {
        id: entry.id.clone(),
        path: entry.path.clone(),
        title: entry.title.clone(),
        snippet: entry.snippet.clone(),
        tags: entry.tags.clone(),
        updated_ms: entry.updated_ms,
        status: status_of(entry),
        order: entry.order.value,
        order_is_own: entry.order.source == NoteOrderSource::Own,
    }
}

/// A note's column, as its own frontmatter spells it.
///
/// Blank is `None` rather than `Some("")`: `status:` with nothing after it is a
/// key somebody started and did not finish, and it should read the same as no
/// key at all rather than opening a column named by the empty string.
#[must_use]
pub fn status_of(entry: &IndexEntry) -> Option<String> {
    entry
        .fields
        .get(WIDGET_STATUS_KEY)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

/// Project and sort a selection for one kind — the whole of what a widget draws.
#[must_use]
pub fn rows_of(kind: WidgetKind, entries: &[&IndexEntry]) -> Vec<WidgetRow> {
    let mut rows: Vec<WidgetRow> = entries.iter().map(|entry| row_of(entry)).collect();
    rows.sort_by(|a, b| kind.compare(a, b));
    rows
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::notes::order::NoteOrder;

    fn entry(path: &str, title: &str) -> IndexEntry {
        IndexEntry {
            link_predicates: Default::default(),
            id: format!("id:{path}"),
            path: path.to_owned(),
            title: title.to_owned(),
            size: 0,
            mtime_ns: 0,
            ino: 0,
            created_ms: 0,
            updated_ms: 0,
            tags: Vec::new(),
            fields: BTreeMap::new(),
            links: Vec::new(),
            flags: Vec::new(),
            snippet: String::new(),
            order: NoteOrder::default(),
        }
    }

    fn with_status(path: &str, title: &str, status: &str) -> IndexEntry {
        let mut e = entry(path, title);
        e.fields
            .insert(WIDGET_STATUS_KEY.to_owned(), status.to_owned());
        e
    }

    #[test]
    fn the_three_markers_round_trip_and_a_fourth_is_not_one() {
        for kind in WIDGET_KINDS {
            assert_eq!(WidgetKind::parse(kind.as_str()), Some(kind));
        }
        // Obsidian's own callouts must keep rendering as callouts.
        assert_eq!(WidgetKind::parse("warning"), None);
        assert_eq!(WidgetKind::parse("gallery"), None);
        assert_eq!(WidgetKind::parse(""), None);
    }

    #[test]
    fn a_marker_is_read_however_it_was_capitalised() {
        assert_eq!(WidgetKind::parse("BOARD"), Some(WidgetKind::Board));
        assert_eq!(WidgetKind::parse(" Log "), Some(WidgetKind::Log));
    }

    #[test]
    fn a_widget_with_no_argument_takes_its_kinds_own_tag() {
        assert_eq!(WidgetKind::Board.effective_query("   "), "tag:task");
        assert_eq!(WidgetKind::Log.effective_query(""), "tag:log");
        assert_eq!(WidgetKind::Refs.effective_query(""), "tag:ref");
    }

    #[test]
    fn an_argument_replaces_the_default_rather_than_extending_it() {
        // Replacing, not anding: a widget that silently kept `tag:task` would
        // make `> [!board] tag:bug` select nothing and look empty rather than
        // wrong.
        assert_eq!(
            WidgetKind::Board.effective_query(" tag:bug path:x/** "),
            "tag:bug path:x/**"
        );
    }

    #[test]
    fn a_blank_status_reads_as_no_status_at_all() {
        assert_eq!(status_of(&with_status("a.md", "A", "  ")), None);
        assert_eq!(status_of(&entry("b.md", "B")), None);
        assert_eq!(
            status_of(&with_status("c.md", "C", " todo ")),
            Some("todo".to_owned())
        );
    }

    #[test]
    fn a_status_the_board_has_no_column_for_survives_the_projection() {
        // The card is drawn as a stray *with its own word*, which is the only
        // thing that explains why it is not in a column.
        let row = row_of(&with_status("a.md", "A", "blocked"));
        assert_eq!(row.status, Some("blocked".to_owned()));
    }

    #[test]
    fn a_board_is_ordered_by_the_position_a_drag_writes() {
        let mut second = entry("b.md", "B");
        second.order = NoteOrder::own(2.0);
        let mut first = entry("a.md", "A");
        first.order = NoteOrder::own(1.5);
        let rows = rows_of(WidgetKind::Board, &[&second, &first]);
        assert_eq!(
            rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            vec!["A", "B"]
        );
        assert!(rows[0].order_is_own, "and says the note placed itself");
    }

    #[test]
    fn a_board_breaks_an_order_tie_by_title_not_by_arrival() {
        // Every silent note ties on the default, so the tiebreak is what the
        // reader actually sees most of the time.
        let a = entry("z.md", "Alpha");
        let b = entry("a.md", "Beta");
        let rows = rows_of(WidgetKind::Board, &[&b, &a]);
        assert_eq!(
            rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            vec!["Alpha", "Beta"]
        );
    }

    #[test]
    fn a_log_is_newest_first_by_the_name_the_file_carries() {
        let old = entry("log/2026-08-01-0900-start.md", "Start");
        let new = entry("log/2026-08-12-1730-ship.md", "Ship");
        let mid = entry("log/2026-08-12-0900-review.md", "Review");
        let rows = rows_of(WidgetKind::Log, &[&old, &mid, &new]);
        assert_eq!(
            rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            vec!["Ship", "Review", "Start"]
        );
    }

    #[test]
    fn references_are_alphabetical_the_way_search_means_it() {
        // Folded: accents and case sort where a reader expects, not where their
        // code points fall.
        let a = entry("r/1.md", "Zebra");
        let b = entry("r/2.md", "Ápple");
        let c = entry("r/3.md", "banana");
        let rows = rows_of(WidgetKind::Refs, &[&a, &b, &c]);
        assert_eq!(
            rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            vec!["Ápple", "banana", "Zebra"]
        );
    }

    #[test]
    fn every_ordering_is_total_so_a_repaint_never_reshuffles() {
        // Two notes alike in every sorted fact still order the same way twice,
        // because the shell hands entries over in hash order.
        let a = entry("a.md", "Same");
        let b = entry("b.md", "Same");
        for kind in WIDGET_KINDS {
            let one = rows_of(kind, &[&a, &b]);
            let two = rows_of(kind, &[&b, &a]);
            assert_eq!(
                one.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
                two.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
                "{} reshuffled on input order",
                kind.as_str()
            );
        }
    }
}
