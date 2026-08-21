//! A note's own position in a list (Story 44.5, FR-159, AD-81).
//!
//! Two facts get confused here constantly, so this module is deliberately only
//! one of them: **`order` is a property of the NOTE**, it lives in frontmatter,
//! it travels with the file through a clone or a sync, and Obsidian shows it in
//! the property list. Which key a space *sorts* by is the viewer's lens and lives
//! in Story 44.4's `notes::sort`; that module calls [`cmp_order`] for its `order`
//! case rather than re-deriving what a note's position means.
//!
//! # Why the default is a constant and not a timestamp
//!
//! Every note needs an order or a list is half-ordered — half the rows placed and
//! the rest wherever they landed, which is the failure this story exists to
//! remove. There were two candidates and both have a real cost:
//!
//! * A **timestamp** default (created, or modified) gives every note a distinct
//!   value, so nothing ever ties. It also makes `order` a second copy of a date
//!   that will drift from it: edit `created`, and `order` still says what
//!   `created` used to say. Worse, it makes an unordered list *look* ordered — a
//!   column of thirteen-digit numbers the reader cannot account for, which is
//!   precisely "reads as randomness" with extra digits. And it would mean writing
//!   an `order` into ten thousand notes keeper did not author, which FR-121
//!   forbids.
//! * A **constant** default makes every un-ordered note tie. That is the cost,
//!   and it is paid once, openly, by [`cmp_order`]'s stated tiebreak — rather
//!   than paid silently by whatever iteration order the entries happened to be
//!   in.
//!
//! The constant wins, and the constant is [`DEFAULT_NOTE_ORDER`] = 0, ascending,
//! negatives allowed. CSS `order` is the same design for the same reason, so
//! `order: -1` meaning "before the un-ordered majority" is a convention a reader
//! already has. The default is **implicit**: absent frontmatter means 0, and
//! keeper never stamps `order: 0` into a file to make it so.
//!
//! # Why the value is `f64` and not an integer
//!
//! `order: 1.5` is what a person actually types to slot a note between 1 and 2,
//! and it is what a future drag-to-reorder would write so that moving one note
//! does not rewrite the frontmatter of every note after it. An integer field
//! would read 1.5 and 1.2 as the same 1 — a tie the reader has no way to account
//! for, invented by the type rather than by the vault. Comparison is
//! [`f64::total_cmp`], so the order is total even if a NaN somehow reaches it.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::notes::frontmatter::{FieldValue, Frontmatter};
use crate::notes::index::IndexEntry;
use crate::notes::search;

/// The frontmatter key. Unnamespaced and unremarkable on purpose: it is the
/// user's property, editable in Obsidian's own property editor, not keeper
/// bookkeeping like the reserved `keeper.*` keys.
pub const NOTE_ORDER_KEY: &str = "order";

/// The order of a note that has never been given one.
pub const DEFAULT_NOTE_ORDER: f64 = 0.0;

/// The step between renumbered notes, and the first one's own position.
///
/// One, not zero: [`DEFAULT_NOTE_ORDER`] is what a silent file reads as, so a
/// note renumbered to zero would claim a position it did not choose and then
/// sort against every silent note by title.
pub const ORDER_STEP: f64 = 1.0;

/// Where a note dropped between two neighbours goes — or `None` when nothing
/// fits between them any more.
///
/// `before` is the order of the note it lands *after* and `after` the order of
/// the one it lands *before*; either is `None` at the ends of a list, and both
/// are `None` in an empty one.
///
/// This is the drag-to-reorder arithmetic the module header promises above:
/// dropping between two neighbours normally writes exactly one number, in one
/// file, rather than renumbering everything below it.
///
/// The `None` answer is the honest one rather than a defeat. Two neighbours can
/// end up adjacent in `f64` after enough halving, or equal because two files
/// were written with the same number by hand — and in both cases there is no
/// value that sorts strictly between them. Returning the midpoint anyway would
/// place the note on a tie, and [`cmp_order`]'s tiebreak (folded title) would
/// then decide the position instead of the drop: the note jumps somewhere the
/// operator did not drop it, and pressing again does nothing. A caller that
/// gets `None` renumbers; see `sessions::tasks::compile_move` for the shape.
///
/// Ends of a list are open-ended by a whole [`ORDER_STEP`] rather than by a
/// fraction, so the common "drag to the bottom" case keeps the numbers small
/// and readable in the file — a person reading `order: 4` in frontmatter can
/// tell what it means; `order: 3.0000000000000004` teaches nothing.
#[must_use]
pub fn drop_order(before: Option<f64>, after: Option<f64>) -> Option<f64> {
    match (before, after) {
        (None, None) => Some(ORDER_STEP),
        (Some(a), None) => Some(a + ORDER_STEP),
        (None, Some(b)) => Some(b - ORDER_STEP),
        (Some(a), Some(b)) => {
            let mid = a + (b - a) / 2.0;
            // Strictly between, tested rather than assumed: this is false when
            // the two are equal, when they are adjacent floats, and when either
            // is non-finite — all of which are reachable from a hand-edited file.
            (mid > a && mid < b).then_some(mid)
        }
    }
}

/// The whole number a note renumbered into `slot` takes.
///
/// Separate from [`drop_order`] because it is the *other* half of the same
/// decision: when no fraction fits, positions are handed out again from the top,
/// and both halves have to agree on where a list starts ([`ORDER_STEP`], never
/// zero).
#[must_use]
pub fn renumbered_order(slot: usize) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let position = (slot + 1) as f64;
    position * ORDER_STEP
}

/// Where a note's order came from — which the list has to render, because a
/// number the reader cannot account for is the thing this story removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NoteOrderSource {
    /// The note said so: `order` is present and reads as a number.
    Own,
    /// The note is silent and took [`DEFAULT_NOTE_ORDER`].
    Default,
    /// `order` is present and is not a number. The note still gets
    /// [`DEFAULT_NOTE_ORDER`] so the list stays fully ordered, and the surface
    /// says the value could not be read — a fallback nobody is told about is a
    /// list that quietly disagrees with the file.
    Unreadable,
}

/// One note's position, and whether the note actually said it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteOrder {
    pub value: f64,
    pub source: NoteOrderSource,
}

impl Default for NoteOrder {
    fn default() -> Self {
        Self {
            value: DEFAULT_NOTE_ORDER,
            source: NoteOrderSource::Default,
        }
    }
}

impl NoteOrder {
    /// A note that stated its own position.
    pub fn own(value: f64) -> Self {
        Self {
            value,
            source: NoteOrderSource::Own,
        }
    }

    /// Whether this order is the note's own rather than the default.
    pub fn is_own(&self) -> bool {
        matches!(self.source, NoteOrderSource::Own)
    }
}

/// Read a note's order out of its parsed frontmatter.
///
/// Generous about spelling, exact about meaning. A YAML number is the normal
/// case; a *quoted* number (`order: "3"`) is accepted because a template or a
/// hand-edit produces one and the value is not ambiguous; an empty property
/// (`order:`, which is what Obsidian writes when you clear a field) is an absent
/// value rather than a broken one. Anything else — a list, a map, a word — is
/// [`NoteOrderSource::Unreadable`]: it gets the default so the list is still
/// totally ordered, and it is reported so the surface can say the file and the
/// list disagree.
///
/// A key that is present but whose value the parser could not model at all
/// ([`Frontmatter::unparsed`]) reads as `Unreadable` too, not as absent, which is
/// why this checks [`Frontmatter::keys`] rather than trusting `get` to
/// distinguish "no key" from "no value I understand".
pub fn read_order(fm: &Frontmatter) -> NoteOrder {
    let present = fm.keys().any(|key| key == NOTE_ORDER_KEY);
    if !present {
        return NoteOrder::default();
    }
    let unreadable = NoteOrder {
        value: DEFAULT_NOTE_ORDER,
        source: NoteOrderSource::Unreadable,
    };
    match fm.get(NOTE_ORDER_KEY) {
        Some(FieldValue::Num(n)) if n.is_finite() => NoteOrder::own(*n),
        Some(FieldValue::Str(s)) if s.trim().is_empty() => NoteOrder::default(),
        Some(FieldValue::Str(s)) => match s.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => NoteOrder::own(n),
            _ => unreadable,
        },
        _ => unreadable,
    }
}

/// The total order over notes by their own `order`, ascending.
///
/// Three terms, and every one of them is load-bearing:
///
/// 1. **`order`**, by [`f64::total_cmp`] — the fact the user set.
/// 2. **Folded title**, through [`search::fold_cmp`], so "alphabetical" means the
///    same thing here as it does to search and to a space's `name` sort. This is
///    the tiebreak a reader can account for: with the default constant, most
///    notes tie, and "then alphabetically" is a sentence the list's own contents
///    demonstrate.
/// 3. **Vault-relative path**, which is unique by construction and therefore
///    makes the order *total*.
///
/// Term 3 is not belt-and-braces. The shell holds its entries in a `HashMap`, so
/// the sequence handed to a sort is in hash order and differs between launches;
/// a comparator that returned `Equal` for two distinct notes would let that
/// reshuffle straight through into the list, and the same vault would present a
/// different order every time the app opened. Nothing here may depend on input
/// position — not even as a stable-sort fallback.
pub fn cmp_order(a: &IndexEntry, b: &IndexEntry) -> Ordering {
    a.order
        .value
        .total_cmp(&b.order.value)
        .then_with(|| search::fold_cmp(&a.title, &b.title))
        .then_with(|| a.path.cmp(&b.path))
}

/// [`cmp_order`] with the `order` value reversed and the tiebreaks left
/// ascending.
///
/// Only the primary fact flips. Reversing the tiebreaks with it would mean the
/// alphabet runs backwards inside a tie for `order desc` but forwards for
/// `order asc`, which is a second rule the reader has to learn to explain the
/// same pile of notes — and it is not how the other sort keys behave either.
pub fn cmp_order_desc(a: &IndexEntry, b: &IndexEntry) -> Ordering {
    b.order
        .value
        .total_cmp(&a.order.value)
        .then_with(|| search::fold_cmp(&a.title, &b.title))
        .then_with(|| a.path.cmp(&b.path))
}

/// Write `order` into a note's source, returning the whole new document.
///
/// Splices through [`Frontmatter::set_in`], which is the FR-121 promise: the key
/// changes and every other byte of the file — key order, comments, CRLF endings,
/// the body — is untouched. A note with no frontmatter block gains one.
///
/// A non-finite `order` cannot arrive from the webview (JSON has no `NaN` or
/// `Infinity`), and if one ever did, `FieldValue::Num`'s renderer quotes it
/// rather than emitting a bare `NaN` that would read back as a string — so the
/// document stays parseable either way and this function has no failure mode of
/// its own.
pub fn set_order_in(source: &str, order: f64) -> String {
    Frontmatter::set_in(source, NOTE_ORDER_KEY, FieldValue::Num(order))
}

/// Remove `order`, returning the note to [`DEFAULT_NOTE_ORDER`].
///
/// The inverse of [`set_order_in`], and the honest way to say "unordered":
/// writing `order: 0` would claim the user placed this note where every silent
/// note already is.
pub fn clear_order_in(source: &str) -> String {
    Frontmatter::remove_in(source, NOTE_ORDER_KEY)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn entry(path: &str, title: &str, order: NoteOrder) -> IndexEntry {
        IndexEntry {
            link_attrs: Default::default(),
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
            order,
        }
    }

    fn read(source: &str) -> NoteOrder {
        let (fm, _) = Frontmatter::parse(source);
        read_order(&fm)
    }

    #[test]
    fn an_empty_list_starts_at_one_not_zero() {
        // Zero is what a file with no `order` reads as, so a note placed there
        // would claim a position it did not choose.
        assert_eq!(drop_order(None, None), Some(1.0));
    }

    #[test]
    fn the_ends_of_a_list_stay_whole_numbers() {
        assert_eq!(drop_order(Some(3.0), None), Some(4.0));
        assert_eq!(drop_order(None, Some(3.0)), Some(2.0));
        // Including below zero: a list whose top note is 1 has somewhere to
        // drop above it without renumbering anything.
        assert_eq!(drop_order(None, Some(1.0)), Some(0.0));
    }

    #[test]
    fn a_drop_between_two_notes_is_their_midpoint() {
        assert_eq!(drop_order(Some(1.0), Some(2.0)), Some(1.5));
        assert_eq!(drop_order(Some(1.5), Some(2.0)), Some(1.75));
    }

    #[test]
    fn a_gap_that_cannot_be_halved_says_so() {
        // Two files written with the same number by hand.
        assert_eq!(drop_order(Some(2.0), Some(2.0)), None);
        // Adjacent floats: the midpoint is one of the endpoints, so no value
        // sorts strictly between them.
        let next = f64::from_bits(2.0_f64.to_bits() + 1);
        assert_eq!(drop_order(Some(2.0), Some(next)), None);
        // Inverted neighbours cannot arrive from a sorted list, but a
        // hand-edited file can invert one — and no value is between them.
        assert_eq!(drop_order(Some(3.0), Some(1.0)), None);
        // Non-finite values are unreachable over JSON and reachable in YAML.
        assert_eq!(drop_order(Some(f64::NAN), Some(1.0)), None);
    }

    #[test]
    fn a_renumbered_slot_counts_from_one() {
        assert_eq!(renumbered_order(0), 1.0);
        assert_eq!(renumbered_order(3), 4.0);
        // And agrees with the empty-list case, which is the same statement made
        // by the other half of the pair.
        assert_eq!(Some(renumbered_order(0)), drop_order(None, None));
    }

    #[test]
    fn a_note_that_never_had_an_order_takes_the_default() {
        assert_eq!(read("---\ntitle: A\n---\nbody\n"), NoteOrder::default());
        assert_eq!(read("no frontmatter at all\n"), NoteOrder::default());
        assert_eq!(read("---\n---\n"), NoteOrder::default());
        assert_eq!(NoteOrder::default().value, 0.0);
        assert!(!NoteOrder::default().is_own());
    }

    #[test]
    fn an_order_the_note_states_is_read_including_a_fraction_and_a_negative() {
        assert_eq!(read("---\norder: 3\n---\n"), NoteOrder::own(3.0));
        // The whole reason the value is not an integer: 1.5 and 1.2 must not
        // collapse into one tie.
        assert_eq!(read("---\norder: 1.5\n---\n"), NoteOrder::own(1.5));
        assert_eq!(read("---\norder: 1.2\n---\n"), NoteOrder::own(1.2));
        assert_eq!(read("---\norder: -1\n---\n"), NoteOrder::own(-1.0));
        // A quoted number is a hand-edit or a template, not a mistake.
        assert_eq!(read("---\norder: \"7\"\n---\n"), NoteOrder::own(7.0));
    }

    #[test]
    fn an_order_that_is_not_a_number_falls_back_visibly_rather_than_silently() {
        for source in [
            "---\norder: soon\n---\n",
            "---\norder: [1, 2]\n---\n",
            "---\norder: 3rd\n---\n",
            // `f64::from_str` accepts both of these; a position does not. The
            // frontmatter scanner already keeps them out of `Num`, and the string
            // branch here must not let them back in — a NaN order would be the
            // one value `total_cmp` can order but nobody can explain.
            "---\norder: NaN\n---\n",
            "---\norder: inf\n---\n",
        ] {
            let read = read(source);
            assert_eq!(
                read.source,
                NoteOrderSource::Unreadable,
                "should be unreadable: {source}"
            );
            assert_eq!(read.value, DEFAULT_NOTE_ORDER, "source: {source}");
        }
        // An emptied property is an absent value, which is what Obsidian writes
        // when a field is cleared — not a complaint. An explicit `null` reads the
        // same way, because `Frontmatter` spells one as the other.
        assert_eq!(read("---\norder:\n---\n").source, NoteOrderSource::Default);
        assert_eq!(
            read("---\norder: null\n---\n").source,
            NoteOrderSource::Default
        );
    }

    #[test]
    fn the_order_is_total_and_independent_of_the_sequence_it_arrives_in() {
        // Four notes in one tie class (all defaulted) plus two placed ones, and
        // inside the tie class two notes share a title up to case — so EVERY term
        // of the comparator is load-bearing for this one fixture. Titles, paths
        // and placement all disagree with each other on purpose: a comparator
        // that dropped a term, or leaned on the sequence it was handed, cannot
        // produce this list.
        let fixture = || {
            vec![
                entry("z/alpha.md", "Alpha", NoteOrder::default()),
                entry("a/zulu.md", "Zulu", NoteOrder::default()),
                entry("m/mike.md", "alpha", NoteOrder::default()),
                entry("b/bravo.md", "bravo", NoteOrder::default()),
                entry("y/first.md", "First", NoteOrder::own(-1.0)),
                entry("c/last.md", "Last", NoteOrder::own(10.0)),
            ]
        };
        let expected = [
            // Placed below the default, however its title sorts.
            "y/first.md",
            // The tie class, folded-alphabetically — and the two `alpha`s
            // separated by path, the only term that can separate them.
            "m/mike.md",
            "z/alpha.md",
            "b/bravo.md",
            "a/zulu.md",
            // Placed above the default, even though "Last" precedes "Zulu".
            "c/last.md",
        ];

        // Every permutation of the input must produce the same list. This is the
        // assertion that fails if the comparator ever leans on input position:
        // the shell's entries live in a `HashMap`, so "the sequence it arrives
        // in" is hash order and changes between launches.
        let mut permutation = fixture();
        let mut seen = 0;
        permute(&mut permutation, 0, &mut |candidate| {
            seen += 1;
            let mut sorted = candidate.to_vec();
            sorted.sort_by(cmp_order);
            let paths: Vec<&str> = sorted.iter().map(|e| e.path.as_str()).collect();
            assert_eq!(paths, expected, "a permutation sorted differently");
        });
        assert_eq!(seen, 720, "all 6! orderings should have been exercised");
    }

    /// Call `visit` once per permutation of `items`.
    fn permute(items: &mut [IndexEntry], at: usize, visit: &mut impl FnMut(&[IndexEntry])) {
        if at == items.len() {
            visit(items);
            return;
        }
        for i in at..items.len() {
            items.swap(at, i);
            permute(items, at + 1, visit);
            items.swap(at, i);
        }
    }

    #[test]
    fn a_folded_title_tie_still_resolves_and_only_by_path() {
        // Same order, same title up to case and diacritics: only the path can
        // separate these, and it must.
        let a = entry("notes/one.md", "Ábc", NoteOrder::default());
        let b = entry("notes/two.md", "abc", NoteOrder::default());
        assert_eq!(cmp_order(&a, &b), Ordering::Less);
        assert_eq!(cmp_order(&b, &a), Ordering::Greater);
        assert_eq!(cmp_order(&a, &a), Ordering::Equal);
    }

    #[test]
    fn descending_reverses_the_order_value_and_leaves_the_alphabet_alone() {
        let first = entry("a.md", "Aaa", NoteOrder::own(1.0));
        let second = entry("b.md", "Bbb", NoteOrder::own(2.0));
        assert_eq!(cmp_order_desc(&second, &first), Ordering::Less);

        let tie_a = entry("a.md", "Aaa", NoteOrder::default());
        let tie_b = entry("b.md", "Bbb", NoteOrder::default());
        assert_eq!(cmp_order_desc(&tie_a, &tie_b), Ordering::Less);
        assert_eq!(cmp_order(&tie_a, &tie_b), Ordering::Less);
    }

    #[test]
    fn writing_an_order_changes_that_key_and_no_other_byte() {
        let source = "---\ntitle: A note\r\n# a comment\r\ntags:\r\n  - one\r\norder: 2\r\n---\r\nBody stays.\r\n";
        let written = set_order_in(source, 5.0);
        assert_eq!(
            written,
            "---\ntitle: A note\r\n# a comment\r\ntags:\r\n  - one\r\norder: 5\r\n---\r\nBody stays.\r\n"
        );
        assert_eq!(read(&written), NoteOrder::own(5.0));
        // A fraction survives the round trip as a number, not as text.
        assert_eq!(read(&set_order_in(source, 2.5)), NoteOrder::own(2.5));
    }

    #[test]
    fn a_note_with_no_frontmatter_gains_a_block_and_keeps_its_body() {
        let written = set_order_in("# Title\n\nBody.\n", 1.0);
        assert_eq!(written, "---\norder: 1\n---\n# Title\n\nBody.\n");
        assert_eq!(read(&written), NoteOrder::own(1.0));
    }

    #[test]
    fn clearing_an_order_returns_the_note_to_the_default() {
        let source = "---\ntitle: A\norder: 4\n---\nBody.\n";
        let cleared = clear_order_in(source);
        assert_eq!(cleared, "---\ntitle: A\n---\nBody.\n");
        assert_eq!(read(&cleared), NoteOrder::default());
        // Round trip: setting it back restores the original bytes exactly.
        assert_eq!(set_order_in(&cleared, 4.0), source);
    }
}
