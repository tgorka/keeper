//! What a space's `sort` means, and how one note answers it (Story 44.4,
//! FR-157, FR-158, AD-81).
//!
//! A space carried a `sort` string since Story 37.4 and nothing ever read it:
//! `space_def` parsed it, the VM shipped it, the editor round-tripped it, and
//! the list ordered every space by the same pinned-then-modified rule. This
//! module is the half that was missing, and it is here rather than in the shell
//! for the reason AD-55/AD-56 states — the `keeper` crate does not build on
//! Linux, and an ordering is exactly the kind of rule that goes subtly wrong and
//! must be provable on the host it is written on.
//!
//! **The space's sort is the whole ordering.** Inside a space, pinned notes no
//! longer float to the top; the plain, space-less list keeps that rule
//! untouched. This is a deliberate behaviour change and AD-81 is the reason: a
//! sort with a hidden first term is not the sort the user chose, and "an
//! ordering the reader cannot account for reads as randomness" is what the epic
//! is trying to end. A user who wants their pins first can say so — that is what
//! `is:pinned` and the Pinned space are for.
//!
//! **A stored sort keeper cannot read falls back visibly.** Frontmatter is a
//! file a person and an agent both edit, so `sort: bananas` will happen. It
//! resolves to the default ordering and produces a sentence — composed here,
//! because [`crate::notes::vm`]'s rule is that what the user reads is worded in
//! Rust — that the rail row and the editor both show. Falling back silently
//! would leave someone staring at a list that ignores what their own file says.
//!
//! **The fallback is whole, never partial.** `modified sideways` does not become
//! "modified, in whatever direction": the entire value falls back and the
//! sentence quotes it. That is Story 43.4's rule for the chip vocabulary applied
//! again — if keeper cannot reproduce the value exactly, it does not honour half
//! of it — and it keeps the rule a reader has to hold one sentence long.

use std::cmp::Ordering;

use crate::notes::index::IndexEntry;
use crate::notes::order;
use crate::notes::query::{resolve_date, stamp_ms, DateField};
use crate::notes::recording_note::SESSION_KEY;
use crate::notes::search::fold_cmp;

/// The frontmatter key a recording stub writes the session's local calendar day
/// into (`crate::notes::recording_note::compose`).
const RECORDED_DATE_KEY: &str = "date";

/// The frontmatter key a recording stub writes the session's start clock into.
const RECORDED_TIME_KEY: &str = "start";

/// How much of an unreadable sort value the fallback sentence repeats back.
///
/// The same judgement `space_icon`'s byte cap makes, for the same reason:
/// frontmatter is agent-writable, so `sort:` can hold a megabyte, and a
/// megabyte has no business inside a sidebar subtitle.
const MAX_ECHOED_SORT: usize = 48;

/// Where a space sits in the rail when its file does not say (FR-157).
///
/// Zero, and every space that existed before Story 44.4 is one — so an
/// un-ordered rail is still the alphabetical rail it has always been, and the
/// four seeded defaults still render Inbox, Journal, Pinned, Recordings in the
/// order the deleted fixed rows did. Negative is how a space floats above that
/// block without renumbering everything below it.
pub const DEFAULT_SPACE_ORDER: f64 = 0.0;

/// A space's stored rail position, read.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredOrder {
    pub order: f64,
    /// The finished sentence when the file held something that is not a
    /// position, or `None` when it did not.
    pub warning: Option<String>,
}

/// Read a space's `keeper.order`.
///
/// `f64` for the reason [`crate::notes::order`] gives for a note's: `1.5` is
/// how a person slots a row between 1 and 2, and an integer would read `1.5`
/// and `1.2` as the same position — a tie invented by the type rather than by
/// the vault.
///
/// This is deliberately **not** [`crate::notes::order::read_order`], even
/// though the two apply the same tolerances. That one reads a note's own
/// top-level `order`, which is the note's position in a list; this reads
/// `keeper.order`, which is a space's position in the rail. A space note has
/// both, they mean different things, and one function over one key could only
/// serve one of them.
#[must_use]
pub fn read_order(raw: &str) -> StoredOrder {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        // `order:` with nothing after it is what an editor writes when a field
        // is cleared. That is an absent position, not a broken one.
        return StoredOrder {
            order: DEFAULT_SPACE_ORDER,
            warning: None,
        };
    }
    match trimmed.parse::<f64>() {
        Ok(order) if order.is_finite() => StoredOrder {
            order,
            warning: None,
        },
        _ => StoredOrder {
            order: DEFAULT_SPACE_ORDER,
            warning: Some(format!(
                "keeper doesn't know the position \"{}\", so this space sits where an \
                 unpositioned one does.",
                clip(trimmed)
            )),
        },
    }
}

/// The rail comparison: position first, then name (FR-157).
///
/// `total_cmp` rather than `partial_cmp`, so the comparator is total whatever
/// arrives — the same reason Story 44.5 gives for a note's order. The name
/// tie-break is what the rail sorted by before this story, so a vault nobody
/// has positioned does not move.
#[must_use]
pub fn rail_order(a: (f64, &str), b: (f64, &str)) -> Ordering {
    a.0.total_cmp(&b.0).then_with(|| a.1.cmp(b.1))
}

/// Which fact a space orders the notes it lists by (FR-158).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    /// The note's own `order` (AD-81), which is the one ordering a user sets
    /// directly rather than derives.
    Order,
    /// Display title, case-insensitively.
    Name,
    /// `date:created`'s answer, through the same chain.
    Created,
    /// `date:modified`'s answer, through the same chain.
    Modified,
    /// The instant the session behind the note started. See [`recorded_ms`] for
    /// what a note with no session does.
    Recorded,
}

/// Which way a [`SortKey`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// A space's ordering: one fact and one direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpaceSort {
    pub key: SortKey,
    pub dir: SortDir,
}

/// What a space with no `sort` in its frontmatter is ordered by, and what an
/// unreadable one falls back to.
///
/// Newest-modified-first, which is what every list in this app showed before a
/// space could name a sort, so a vault that upgrades into Story 44.4 sees no
/// list move until somebody asks it to.
pub const DEFAULT_SORT: SpaceSort = SpaceSort {
    key: SortKey::Modified,
    dir: SortDir::Desc,
};

/// A space's stored `sort`, read.
///
/// Two fields rather than a `Result`, because there is no failure here: an
/// unreadable value still yields an ordering the list can run. What it also
/// yields is the sentence saying so, and a caller that drops it is the silent
/// fallback this story exists to forbid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSort {
    pub sort: SpaceSort,
    /// The finished sentence for the rail row and the editor, or `None` when the
    /// stored text named a sort keeper knows.
    pub warning: Option<String>,
}

impl SortKey {
    /// The word this key is written as in frontmatter.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Order => "order",
            Self::Name => "name",
            Self::Created => "created",
            Self::Modified => "modified",
            Self::Recorded => "recorded",
        }
    }

    /// The direction a bare key means.
    ///
    /// Stated rather than defaulted to one constant, because "sort by name" and
    /// "sort by modified" mean opposite directions to everybody: A first, and
    /// newest first. A single default would make one of the five read backwards
    /// for every user who wrote the short form.
    #[must_use]
    pub const fn natural(self) -> SortDir {
        match self {
            Self::Order | Self::Name => SortDir::Asc,
            Self::Created | Self::Modified | Self::Recorded => SortDir::Desc,
        }
    }

    fn from_word(word: &str) -> Option<Self> {
        match word.to_ascii_lowercase().as_str() {
            "order" => Some(Self::Order),
            "name" => Some(Self::Name),
            "created" => Some(Self::Created),
            "modified" => Some(Self::Modified),
            "recorded" => Some(Self::Recorded),
            _ => None,
        }
    }
}

impl SortDir {
    /// The word this direction is written as in frontmatter.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }

    fn from_word(word: &str) -> Option<Self> {
        match word.to_ascii_lowercase().as_str() {
            "asc" | "ascending" => Some(Self::Asc),
            "desc" | "descending" => Some(Self::Desc),
            _ => None,
        }
    }
}

impl SpaceSort {
    /// The text this ordering is stored as: always `<key> <dir>`, both words.
    ///
    /// The long form even when the direction is the natural one, so the file
    /// says what the list does without the reader having to know the table
    /// above.
    #[must_use]
    pub fn canonical(self) -> String {
        format!("{} {}", self.key.word(), self.dir.word())
    }

    /// This ordering as a phrase a sentence can contain.
    #[must_use]
    pub const fn phrase(self) -> &'static str {
        match (self.key, self.dir) {
            (SortKey::Order, SortDir::Asc) => "order, lowest first",
            (SortKey::Order, SortDir::Desc) => "order, highest first",
            (SortKey::Name, SortDir::Asc) => "name, A to Z",
            (SortKey::Name, SortDir::Desc) => "name, Z to A",
            (SortKey::Created, SortDir::Asc) => "created, oldest first",
            (SortKey::Created, SortDir::Desc) => "created, newest first",
            (SortKey::Modified, SortDir::Asc) => "modified, oldest first",
            (SortKey::Modified, SortDir::Desc) => "modified, newest first",
            (SortKey::Recorded, SortDir::Asc) => "recorded, oldest first",
            (SortKey::Recorded, SortDir::Desc) => "recorded, newest first",
        }
    }
}

/// Read a space's stored `sort` text.
///
/// Accepts `<key>` or `<key> <dir>`, case-insensitively, whitespace-tolerantly,
/// because this value is typed by hand into a file as often as it is written by
/// the editor. Everything else — an unknown key, an unknown direction, a third
/// word, an empty first word — is one failure with one sentence.
#[must_use]
pub fn read(raw: &str) -> StoredSort {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        // No `sort` key at all is not a mistake; it is a space that never named
        // one, and it gets the default without a word said about it.
        return StoredSort {
            sort: DEFAULT_SORT,
            warning: None,
        };
    }
    let mut words = trimmed.split_whitespace();
    let read = match (
        words.next().and_then(SortKey::from_word),
        words.next(),
        words.next(),
    ) {
        (Some(key), None, _) => Some(SpaceSort {
            key,
            dir: key.natural(),
        }),
        (Some(key), Some(dir), None) => SortDir::from_word(dir).map(|dir| SpaceSort { key, dir }),
        _ => None,
    };
    match read {
        Some(sort) => StoredSort {
            sort,
            warning: None,
        },
        None => StoredSort {
            sort: DEFAULT_SORT,
            warning: Some(unreadable_sentence(trimmed)),
        },
    }
}

/// The sentence a space shows when its own file names a sort keeper cannot read.
///
/// It names three things because a reader needs all three: that keeper did not
/// understand, what it did not understand (quoted, so a stray character is
/// visible), and what the list is doing instead. Dropping the third would leave
/// the list looking arbitrary, which is the state this replaces.
fn unreadable_sentence(stored: &str) -> String {
    let echoed = clip(stored);
    format!(
        "keeper doesn't know the sort \"{echoed}\", so this space is sorted by {}.",
        DEFAULT_SORT.phrase()
    )
}

/// Quote a value back inside a sentence, safely.
///
/// Two hazards, both real because this text came out of a file an agent may
/// have written. Length is capped at [`MAX_ECHOED_SORT`] *characters*, counted
/// on character boundaries so a multi-byte value cannot panic the sentence that
/// quotes it. And every run of whitespace collapses to one space, because a
/// frontmatter list flattens with newlines in it and a sentence carrying a
/// newline arrives in the sidebar as two half-sentences.
fn clip(value: &str) -> String {
    let flattened: Vec<&str> = value.split_whitespace().collect();
    let flattened = flattened.join(" ");
    let mut out: String = flattened.chars().take(MAX_ECHOED_SORT).collect();
    if out.chars().count() < flattened.chars().count() {
        out.push('…');
    }
    out
}

/// Compare two notes the way `sort` says to.
///
/// Total, and for four of the five keys the tie-break is `path` **ascending
/// whatever the direction is**. Two facts make that the right shape: the order
/// has to be total or a repaint reshuffles equals under the reader's cursor
/// (`notes_vault`'s entry map iterates in an order that changes between
/// launches, so there is no "input order" to fall back on), and the tie-break is
/// keeper's own doing rather than something the user asked for — reversing it
/// with the direction would make the same two notes swap places for no reason
/// the file mentions.
///
/// `order` is the exception, and deliberately: Story 44.5 owns what a note's
/// order is, including its own tie rule, so this defers to
/// [`order::cmp_order`] whole rather than reproducing three lines of it here.
/// A second comparator for the same field is how two surfaces start disagreeing
/// about which of two notes comes first.
#[must_use]
pub fn compare(sort: SpaceSort, a: &IndexEntry, b: &IndexEntry) -> Ordering {
    if sort.key == SortKey::Order {
        return match sort.dir {
            SortDir::Asc => order::cmp_order(a, b),
            SortDir::Desc => order::cmp_order_desc(a, b),
        };
    }
    let primary = match sort.key {
        // Unreachable: handled above, and kept out of this match so adding a
        // sixth key cannot silently acquire the wrong tie-break.
        SortKey::Order => Ordering::Equal,
        SortKey::Name => title_order(a, b),
        SortKey::Created => {
            resolve_date(DateField::Created, a).cmp(&resolve_date(DateField::Created, b))
        }
        SortKey::Modified => {
            resolve_date(DateField::Modified, a).cmp(&resolve_date(DateField::Modified, b))
        }
        SortKey::Recorded => recorded_or_created(a).cmp(&recorded_or_created(b)),
    };
    match sort.dir {
        SortDir::Asc => primary,
        SortDir::Desc => primary.reverse(),
    }
    .then_with(|| a.path.cmp(&b.path))
}

/// When the session behind this note started, or `None` for a note that is not
/// about a recording or whose stub was written without stamps.
///
/// The test for "is this about a recording" is the presence of a non-blank
/// [`SESSION_KEY`], which is exactly what
/// [`crate::notes::recording_note::is_recording_note`] asks of a note's
/// frontmatter — the same key, read off the index's projection of it, so a
/// third definition of "recording note" does not appear here.
///
/// The instant itself is composed from the two keys the stub actually writes:
/// `date` is the session's **local calendar day** and `start` its clock, and
/// the stub writes them separately because the local day is what the note has
/// to be right about across an offset change. A `start` that is not a clock
/// degrades to the day rather than losing the note's stamp entirely: the day is
/// still true, and a session with an unreadable minute is not a session that
/// never happened.
#[must_use]
pub fn recorded_ms(e: &IndexEntry) -> Option<i64> {
    let session = e.fields.get(SESSION_KEY)?;
    if session.trim().is_empty() {
        return None;
    }
    let date = e.fields.get(RECORDED_DATE_KEY)?.trim();
    if date.is_empty() {
        return None;
    }
    match e
        .fields
        .get(RECORDED_TIME_KEY)
        .map(|time| time.trim())
        .filter(|time| !time.is_empty())
    {
        Some(time) => stamp_ms(&format!("{date}T{time}")).or_else(|| stamp_ms(date)),
        None => stamp_ms(date),
    }
}

/// What `sort: recorded` compares, for every note including the ones with no
/// session.
///
/// **A note with no session timestamp is sorted by its `created` date.** The
/// story asked for a rule rather than an arbitrary end, and this is the rule,
/// for three reasons. It is the chain [`DateField::Touched`] already follows a
/// few lines away in `query.rs` — a missing fact degrades to the nearest true
/// one instead of breaking the ordering — so the vocabulary of this codebase
/// stays one idea rather than two. `recorded` and `created` answer the same
/// question, *when did this thing happen*, so the substitute is on the same
/// scale as the fact it stands in for rather than a sentinel pretending to be
/// one. And it is the option that does **not** partition the list: sorting every
/// stampless note to one end would make a note's position depend on a key the
/// reader cannot see in the row, and would move a note across the whole list the
/// moment somebody added `session:` to it. Interleaved, every note's position is
/// accountable from a date that is on its face.
fn recorded_or_created(e: &IndexEntry) -> i64 {
    recorded_ms(e).unwrap_or_else(|| resolve_date(DateField::Created, e))
}

/// Compare two titles the way this app spells "alphabetical".
///
/// [`fold_cmp`] rather than `to_lowercase`, because it is already what `search`
/// means by two strings being the same word — case, Latin diacritics, `ß`, `Œ`,
/// NFC and NFD alike — and Story 44.5's `order` tie-break folds titles through
/// the very same walk. A vault where `Émile` files under `E` in one surface and
/// after `Z` in another is a vault with two alphabets.
///
/// It also allocates nothing: `fold_cmp` folds both sides lazily and stops at
/// the first differing character, which matters because this runs `n log n`
/// times to paint one list of a ten-thousand-note vault (NFR-28).
fn title_order(a: &IndexEntry, b: &IndexEntry) -> Ordering {
    fold_cmp(&a.title, &b.title)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::notes::order::NoteOrder;
    use crate::notes::query::stamp_ms;

    /// A fixture stamp, read through the parser the sort itself uses so a test
    /// cannot pass against an instant `date:` would never have produced.
    fn at(spec: &str) -> i64 {
        stamp_ms(spec).expect("fixture stamp parses")
    }

    fn entry(path: &str, title: &str) -> IndexEntry {
        IndexEntry {
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

    /// Make `e` a note keeper wrote about a recording, stamped the way
    /// `recording_note::compose` stamps one: the local calendar day in `date`
    /// and the clock in `start`, never one joined value.
    fn recorded(mut e: IndexEntry, day: &str, clock: &str) -> IndexEntry {
        e.fields
            .insert(SESSION_KEY.to_owned(), "01DEVICE-01SESSION".to_owned());
        e.fields.insert("date".to_owned(), day.to_owned());
        if !clock.is_empty() {
            e.fields.insert("start".to_owned(), clock.to_owned());
        }
        e
    }

    /// The four notes every sort below is asserted against.
    ///
    /// One fixture, five sorts, five different answers — which is the only
    /// arrangement that proves anything. A fixture where two sorts agree proves
    /// neither of them: `created desc` and `modified desc` would both pass with
    /// the same three-line comparator, and `recorded` would pass while doing
    /// nothing at all if it happened to land on the created order.
    ///
    /// So the four notes disagree on every axis on purpose:
    ///
    /// | note | order | title   | created | modified | recorded |
    /// |------|-------|---------|---------|----------|----------|
    /// | a.md | 1     | Bravo   | 03-01   | 01-01    | session 06-10 09:00 |
    /// | b.md | 2     | Alpha   | 01-01   | 03-01    | none |
    /// | c.md | 3     | Delta   | 05-01   | 04-01    | session 02-01 08:00 |
    /// | d.md | 4     | Charlie | 04-01   | 05-01    | none |
    ///
    /// `c.md` is the load-bearing row of the `recorded` case: it is a real
    /// February recording and it must sort *below* `d.md`, an ordinary note
    /// created in April. That only comes out right if a note with no session
    /// timestamp is interleaved by its `created` date. Either version of
    /// "stampless to one end" produces a different answer, which is the whole
    /// point of choosing a rule.
    fn fixture() -> Vec<IndexEntry> {
        let mut a = entry("a.md", "Bravo");
        a.order = NoteOrder::own(1.0);
        a.created_ms = at("2026-03-01");
        a.updated_ms = at("2026-01-01");
        let a = recorded(a, "2026-06-10", "09:00");

        let mut b = entry("b.md", "Alpha");
        b.order = NoteOrder::own(2.0);
        b.created_ms = at("2026-01-01");
        b.updated_ms = at("2026-03-01");

        let mut c = entry("c.md", "Delta");
        c.order = NoteOrder::own(3.0);
        c.created_ms = at("2026-05-01");
        c.updated_ms = at("2026-04-01");
        let c = recorded(c, "2026-02-01", "08:00");

        let mut d = entry("d.md", "Charlie");
        d.order = NoteOrder::own(4.0);
        d.created_ms = at("2026-04-01");
        d.updated_ms = at("2026-05-01");

        vec![a, b, c, d]
    }

    /// The fixture ordered by `spec`, as the paths' first letters.
    fn ordered(spec: &str) -> String {
        let stored = read(spec);
        assert_eq!(
            stored.warning, None,
            "`{spec}` is meant to be a sort keeper knows"
        );
        let mut notes = fixture();
        notes.sort_by(|a, b| compare(stored.sort, a, b));
        notes
            .iter()
            .map(|e| e.path.chars().next().expect("a path"))
            .collect()
    }

    #[test]
    fn every_sort_orders_the_same_four_notes_differently() {
        assert_eq!(ordered("order asc"), "abcd");
        assert_eq!(ordered("name asc"), "badc");
        assert_eq!(ordered("created desc"), "cdab");
        assert_eq!(ordered("modified desc"), "dcba");
        assert_eq!(ordered("recorded desc"), "adcb");

        // Said out loud rather than left to the reader to check: if any two of
        // these agreed, both of their assertions above would be worthless.
        let answers = [
            ordered("order asc"),
            ordered("name asc"),
            ordered("created desc"),
            ordered("modified desc"),
            ordered("recorded desc"),
        ];
        let distinct: BTreeMap<&String, ()> = answers.iter().map(|a| (a, ())).collect();
        assert_eq!(
            distinct.len(),
            answers.len(),
            "two sorts produced the same order, so neither is proved: {answers:?}"
        );
    }

    #[test]
    fn a_direction_reverses_the_fact_and_never_the_tie_break() {
        assert_eq!(ordered("name desc"), "cdab");
        assert_eq!(ordered("created asc"), "badc");
        assert_eq!(ordered("modified asc"), "abcd");
        assert_eq!(ordered("recorded asc"), "bcda");

        // Two notes the sort cannot tell apart go by path, ascending, in BOTH
        // directions — the tie-break is keeper's own doing and the file never
        // asked for it, so flipping it with the direction would swap two rows
        // for a reason nothing on screen explains.
        let mut tied = [entry("z.md", "Same"), entry("a.md", "Same")];
        let asc = SpaceSort {
            key: SortKey::Name,
            dir: SortDir::Asc,
        };
        tied.sort_by(|a, b| compare(asc, a, b));
        assert_eq!(tied[0].path, "a.md");
        tied.sort_by(|a, b| {
            compare(
                SpaceSort {
                    key: SortKey::Name,
                    dir: SortDir::Desc,
                },
                a,
                b,
            )
        });
        assert_eq!(tied[0].path, "a.md", "the tie-break did not reverse");
    }

    #[test]
    fn a_pinned_note_does_not_float_inside_a_space() {
        // The plain list floats pins; a space's sort is the whole ordering
        // (AD-81). The oldest-modified note is `a.md`, and pinning it must not
        // move it off the bottom of `modified desc`.
        let mut notes = fixture();
        notes[0].flags.push("pinned".to_owned());
        notes.sort_by(|a, b| compare(read("modified desc").sort, a, b));
        assert_eq!(notes[3].path, "a.md");
    }

    #[test]
    fn a_bare_key_takes_the_direction_its_own_word_means() {
        // A single default direction would read backwards for one half of the
        // vocabulary whichever half it picked.
        assert_eq!(read("order").sort.dir, SortDir::Asc);
        assert_eq!(read("name").sort.dir, SortDir::Asc);
        assert_eq!(read("created").sort.dir, SortDir::Desc);
        assert_eq!(read("modified").sort.dir, SortDir::Desc);
        assert_eq!(read("recorded").sort.dir, SortDir::Desc);
        assert_eq!(ordered("name"), ordered("name asc"));
        assert_eq!(ordered("recorded"), ordered("recorded desc"));
    }

    #[test]
    fn a_space_that_names_no_sort_gets_the_default_and_no_complaint() {
        for absent in ["", "   ", "\t\n"] {
            let stored = read(absent);
            assert_eq!(stored.sort, DEFAULT_SORT);
            assert_eq!(
                stored.warning, None,
                "never naming a sort is not a mistake to report"
            );
        }
    }

    #[test]
    fn frontmatter_spelling_is_forgiven_but_meaning_is_not_guessed() {
        // A hand-edited file is the normal case for this key.
        assert_eq!(read("  Modified   DESC  ").sort, DEFAULT_SORT);
        assert_eq!(read("NAME ascending").sort.dir, SortDir::Asc);
        assert_eq!(read("recorded Descending").sort.key, SortKey::Recorded);
    }

    #[test]
    fn a_sort_keeper_cannot_read_falls_back_out_loud() {
        let stored = read("bananas");
        assert_eq!(stored.sort, DEFAULT_SORT);
        assert_eq!(
            stored.warning.as_deref(),
            Some(
                "keeper doesn't know the sort \"bananas\", so this space is sorted by modified, newest first."
            ),
            "the sentence has to name what was not understood AND what is happening instead"
        );
    }

    #[test]
    fn half_a_sort_is_refused_whole_rather_than_half_honoured() {
        // `modified sideways` could have become "modified, in some direction".
        // It does not: the value falls back entire and the sentence quotes all
        // of it, so the rule a reader holds is one sentence long (Story 43.4's
        // rule for the chip vocabulary, applied again).
        for half in ["modified sideways", "modified desc extra", "desc", "3"] {
            let stored = read(half);
            assert_eq!(stored.sort, DEFAULT_SORT, "`{half}`");
            assert!(
                stored
                    .warning
                    .as_deref()
                    .is_some_and(|said| said.contains(&format!("\"{half}\""))),
                "`{half}` was not quoted back: {:?}",
                stored.warning
            );
        }
    }

    #[test]
    fn an_agent_written_sort_cannot_put_a_megabyte_in_the_sidebar() {
        let huge = "x".repeat(4096);
        let said = read(&huge).warning.expect("a warning");
        assert!(said.contains('…'), "the value was not clipped: {said}");
        assert!(
            said.len() < 200,
            "{} bytes reached the sentence",
            said.len()
        );
    }

    #[test]
    fn the_canonical_spelling_reads_back_as_itself() {
        // What the editor writes must be what `read` accepts, or a save would
        // make a space complain about the value keeper itself just wrote.
        for key in [
            SortKey::Order,
            SortKey::Name,
            SortKey::Created,
            SortKey::Modified,
            SortKey::Recorded,
        ] {
            for dir in [SortDir::Asc, SortDir::Desc] {
                let sort = SpaceSort { key, dir };
                let stored = read(&sort.canonical());
                assert_eq!(stored.sort, sort);
                assert_eq!(stored.warning, None, "{}", sort.canonical());
            }
        }
    }

    #[test]
    fn a_recording_notes_stamp_is_its_own_day_and_clock_joined() {
        let e = recorded(entry("r.md", "Standup"), "2026-02-01", "08:30");
        assert_eq!(recorded_ms(&e), Some(at("2026-02-01T08:30")));

        // No clock: the day is still true, and midnight of it is the honest
        // reading of "that day".
        let day_only = recorded(entry("r.md", "Standup"), "2026-02-01", "");
        assert_eq!(recorded_ms(&day_only), Some(at("2026-02-01")));

        // A clock nobody can read degrades to the day rather than throwing the
        // stamp away: a session with an unreadable minute still happened.
        let mut bad_clock = recorded(entry("r.md", "Standup"), "2026-02-01", "half eight");
        assert_eq!(recorded_ms(&bad_clock), Some(at("2026-02-01")));

        // A day nobody can read leaves nothing to stand on.
        bad_clock
            .fields
            .insert("date".to_owned(), "soon".to_owned());
        assert_eq!(recorded_ms(&bad_clock), None);
    }

    #[test]
    fn only_a_note_with_a_session_has_a_recorded_time() {
        // A journal entry has a `date` and is not a recording. Reading its
        // `date` as a session stamp would file every journal note among the
        // sessions, which is the failure `session:` exists to prevent.
        let mut journal = entry("j.md", "Monday");
        journal
            .fields
            .insert("date".to_owned(), "2026-02-01".to_owned());
        assert_eq!(recorded_ms(&journal), None);

        // A blank `session:` is the one state nothing can resolve, so it is not
        // a recording either — the same test `is_recording_note` makes.
        let mut blank = journal.clone();
        blank
            .fields
            .insert(SESSION_KEY.to_owned(), "   ".to_owned());
        assert_eq!(recorded_ms(&blank), None);

        blank
            .fields
            .insert(SESSION_KEY.to_owned(), "01DEV-01SESS".to_owned());
        assert_eq!(recorded_ms(&blank), Some(at("2026-02-01")));
    }

    #[test]
    fn a_note_with_no_session_sorts_by_when_it_was_created() {
        // The stated rule, asserted on its own rather than only through the
        // four-note fixture: `recorded` for a stampless note is its `created`
        // date, on the same scale and interleaved with the real stamps.
        let mut plain = entry("p.md", "Plain");
        plain.created_ms = at("2026-04-01");
        let session_before = recorded(entry("s.md", "Session"), "2026-03-01", "10:00");
        let session_after = recorded(entry("t.md", "Session"), "2026-05-01", "10:00");

        let mut notes = [session_before, plain, session_after];
        notes.sort_by(|a, b| compare(read("recorded desc").sort, a, b));
        assert_eq!(
            notes.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec!["t.md", "p.md", "s.md"],
            "the stampless note belongs between the two sessions, not at an end"
        );
    }

    #[test]
    fn created_and_modified_are_the_dsls_own_answers() {
        // A space sorted by `created` and a space filtered by `date:created`
        // must agree about a note, so the sort reads the same chain: the
        // author's frontmatter outranks the reconciler's timestamp.
        let mut stated = entry("a.md", "A");
        stated.created_ms = at("2026-01-01");
        stated
            .fields
            .insert("created".to_owned(), "2019-05-04".to_owned());
        let mut resolved = entry("b.md", "B");
        resolved.created_ms = at("2020-01-01");

        let mut notes = [stated, resolved];
        notes.sort_by(|a, b| compare(read("created desc").sort, a, b));
        assert_eq!(
            notes[0].path, "b.md",
            "frontmatter `created: 2019` must outrank `created_ms: 2026`"
        );

        // `modified` falls back to the raw mtime when nothing better exists.
        let mut only_mtime = entry("c.md", "C");
        only_mtime.mtime_ns = i128::from(at("2030-01-01")) * 1_000_000;
        let mut updated = entry("d.md", "D");
        updated.updated_ms = at("2026-01-01");
        let mut notes = [updated, only_mtime];
        notes.sort_by(|a, b| compare(read("modified desc").sort, a, b));
        assert_eq!(notes[0].path, "c.md");
    }

    #[test]
    fn the_order_sort_is_story_44_5s_comparator_and_not_a_second_one() {
        // Delegation asserted through behaviour: a note that never stated an
        // order takes the default and ties with every other silent note, and
        // the tie resolves by folded title — which is `cmp_order`'s rule, not
        // one written here.
        let mut zebra = entry("z.md", "zebra");
        zebra.order = NoteOrder::own(-1.0);
        let apple = entry("a.md", "apple");
        let banana = entry("b.md", "Banana");

        let mut notes = [banana, apple, zebra];
        notes.sort_by(|a, b| compare(read("order asc").sort, a, b));
        assert_eq!(
            notes.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec!["z.md", "a.md", "b.md"],
            "a negative order floats above the un-ordered majority, which then \
             goes alphabetically by folded title"
        );

        // Descending flips the order value and leaves the alphabet alone.
        notes.sort_by(|a, b| compare(read("order desc").sort, a, b));
        assert_eq!(
            notes.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec!["a.md", "b.md", "z.md"]
        );
    }

    #[test]
    fn alphabetical_means_what_search_means_by_it() {
        // One alphabet in this app. `Émile` files under E, not after Z, and a
        // lowercase title does not sort after every uppercase one.
        let mut notes = [
            entry("1.md", "Zoe"),
            entry("2.md", "Émile"),
            entry("3.md", "apple"),
        ];
        notes.sort_by(|a, b| compare(read("name asc").sort, a, b));
        assert_eq!(
            notes.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(),
            vec!["apple", "Émile", "Zoe"]
        );
    }

    #[test]
    fn a_rail_nobody_positioned_is_the_alphabetical_rail_it_always_was() {
        // The four seeded defaults carry no `keeper.order`, and this is the
        // assertion that says installing Story 44.4 does not move them.
        let mut rail = [
            (read_order("").order, "Recordings"),
            (read_order("").order, "Inbox"),
            (read_order("").order, "Pinned"),
            (read_order("").order, "Journal"),
        ];
        rail.sort_by(|a, b| rail_order((a.0, a.1), (b.0, b.1)));
        assert_eq!(
            rail.iter().map(|row| row.1).collect::<Vec<_>>(),
            vec!["Inbox", "Journal", "Pinned", "Recordings"]
        );
    }

    #[test]
    fn a_position_lifts_a_space_out_of_the_alphabet_and_a_negative_one_above_it() {
        let mut rail = [
            (read_order("2").order, "Inbox"),
            (read_order("").order, "Journal"),
            (read_order("-1").order, "Recordings"),
            (read_order("1.5").order, "Pinned"),
        ];
        rail.sort_by(|a, b| rail_order((a.0, a.1), (b.0, b.1)));
        assert_eq!(
            rail.iter().map(|row| row.1).collect::<Vec<_>>(),
            vec!["Recordings", "Journal", "Pinned", "Inbox"],
            "-1 floats above the unpositioned 0, and 1.5 slots between 0 and 2 \
             rather than truncating onto 1"
        );
    }

    #[test]
    fn a_position_keeper_cannot_read_falls_back_out_loud_too() {
        // Same rule as the sort, because whoever guessed at one guessed at both.
        let stored = read_order("first");
        assert_eq!(stored.order, DEFAULT_SPACE_ORDER);
        assert_eq!(
            stored.warning.as_deref(),
            Some(
                "keeper doesn't know the position \"first\", so this space sits where an unpositioned one does."
            )
        );

        // A cleared key is absent, not broken.
        assert_eq!(read_order("   ").warning, None);
        // A number written as text is still a number; nobody hand-editing YAML
        // thinks about scalar types.
        assert_eq!(read_order(" 3 ").order, 3.0);
        // A value that is not finite is not a position.
        assert!(read_order("inf").warning.is_some());
        assert!(read_order("NaN").warning.is_some());
    }

    #[test]
    fn a_quoted_value_never_arrives_in_the_sidebar_as_two_half_sentences() {
        // `keeper.order` holding a list flattens with newlines in it. A
        // sentence carrying one is a sentence the row renders broken.
        let said = read_order("one\ntwo\nthree").warning.expect("a warning");
        assert!(!said.contains('\n'), "{said}");
        assert!(said.contains("\"one two three\""), "{said}");
    }
}
