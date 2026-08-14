//! Moving a card on the board (FR-263): which column, and where in it.
//!
//! A card is a `task`-tagged markdown file, and the board is a *view* of the
//! pool rather than a structure beside it (AD-110). So moving a card is one
//! write to one file: `status:` says the column, `order:` says the position,
//! and both are ordinary frontmatter that Obsidian shows and an agent can set.
//! Nothing outside the moved file has to be told a card moved — which is the
//! whole reason the board can be a widget in an arbitrary note later, and the
//! reason two agents editing two different tasks never conflict.
//!
//! **The position is fractional on purpose.** [`crate::notes::order`]'s own
//! header already said this is "what a future drag-to-reorder would write so
//! that moving one note does not rewrite the frontmatter of every note after
//! it". This is that future: dropping a card between two others normally writes
//! exactly one number, in one file.
//!
//! **Normally.** Halving a gap forever runs out of `f64`, and this module says
//! so rather than pretending otherwise: [`drop_order`] answers `None` when the
//! midpoint is not strictly between its neighbours, and [`compile_move`] then
//! renumbers the target column with whole numbers — every file in it, in one
//! plan. That is a rare, bounded, visible cost, and the alternative is a drop
//! that silently does nothing because the card landed on a tie the title
//! break then resolved the other way.
//!
//! Pure, like the rest of the domain: the shell reads the column's files and
//! executes the plan. Nothing here opens a file, and nothing here mints an id —
//! a task keeper did not author keeps its bytes (FR-121).

use crate::notes::frontmatter::{FieldValue, Frontmatter};
use crate::notes::order::set_order_in;
use crate::sessions::files::{check_rel, FileVerbError};
use crate::sessions::plan::{Plan, PlanStep};
use crate::sessions::shape::TaskStatus;

/// The frontmatter key that decides a card's column.
pub const TASK_STATUS_KEY: &str = "status";

/// The step between renumbered cards, and the first card's own position.
///
/// One, not zero: `order: 0` is what a file with no `order` key reads as
/// ([`crate::notes::order::DEFAULT_NOTE_ORDER`]), so a card renumbered to zero
/// would claim a position it did not choose and sort against silent files by
/// title. The board's own spaces use `1..5` for the same reason.
const STEP: f64 = 1.0;

/// One member of a column, as the shell read it.
///
/// `text` is the file's whole current content, because a renumber rewrites it
/// through the splice writer and the splice writer needs the bytes it is
/// preserving. `order` is what [`crate::notes::order::read_order`] answered —
/// including the default for a file that never stated one, which is exactly the
/// case a renumber exists to repair.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskFile<'a> {
    /// Session-relative path.
    pub rel: &'a str,
    pub text: &'a str,
    pub order: f64,
}

/// Where a card dropped between two neighbours goes — or `None` when nothing
/// fits between them any more.
///
/// `before` is the order of the card it lands *after* and `after` the order of
/// the card it lands *before*; either is `None` at the ends of a column, and
/// both are `None` in an empty one.
///
/// The `None` answer is the honest one rather than a defeat. Two neighbours can
/// end up adjacent in `f64` after enough halving, or equal because two files
/// were written with the same number by hand — and in both cases there is no
/// value that sorts strictly between them. Returning the midpoint anyway would
/// place the card on a tie, and the tie break (folded title) would then decide
/// the position instead of the drop: the card jumps somewhere the operator did
/// not drop it, and pressing again does nothing.
///
/// Ends of a column are open-ended by a whole [`STEP`] rather than by a fraction,
/// so the common "drag to the bottom" case keeps the numbers small and readable
/// in the file — a person reading `order: 4` in frontmatter can tell what it
/// means; `order: 3.0000000000000004` teaches nothing.
#[must_use]
pub fn drop_order(before: Option<f64>, after: Option<f64>) -> Option<f64> {
    match (before, after) {
        (None, None) => Some(STEP),
        (Some(a), None) => Some(a + STEP),
        (None, Some(b)) => Some(b - STEP),
        (Some(a), Some(b)) => {
            let mid = a + (b - a) / 2.0;
            // Strictly between, tested rather than assumed: this is false when
            // the two are equal, when they are adjacent floats, and when either
            // is non-finite — all of which are reachable from a hand-edited file.
            (mid > a && mid < b).then_some(mid)
        }
    }
}

/// The plan that moves one card: status, position, and a renumber if forced.
///
/// `session` is the session's zone-relative folder and `moved` is
/// session-relative; the join happens here so no caller composes a zone path
/// (AD-65).
///
/// `column` is the target column's current members **in rendered order and
/// without the moved card**, and `index` is where in that list the card lands
/// (`0` = top, `column.len()` = bottom). Passing the column rather than two
/// neighbour orders is what lets this module answer the exhausted case at all:
/// a renumber needs every member, and a caller that had already reduced the
/// column to two numbers could not produce one without a second round trip.
///
/// The moved file is written last. Its write is the one that makes the move
/// visible — the card is not in the column until its `status` says so — and
/// AD-111 puts the step everything else is preparation for at the end, so a
/// crash halfway leaves a renumbered column and a card that has not moved,
/// rather than a card in a column whose numbering never happened.
///
/// Both keys go through [`Frontmatter::set_in`], so each write changes one key
/// and leaves every other byte — key order, comments, CRLF endings, the body —
/// exactly as it was (FR-121).
///
/// # Errors
/// Whatever [`check_rel`] refuses: a path that leaves the session, an extension
/// keeper does not author, or scratch. A card is markdown in the pool, so a
/// `.png` under `workspace/` is not one however it was dragged.
pub fn compile_move(
    session: &str,
    moved: &str,
    text: &str,
    status: TaskStatus,
    column: &[TaskFile<'_>],
    index: usize,
) -> Result<Plan, FileVerbError> {
    check_rel(moved)?;
    let at = index.min(column.len());
    let before = at
        .checked_sub(1)
        .and_then(|i| column.get(i))
        .map(|f| f.order);
    let after = column.get(at).map(|f| f.order);

    let mut steps = Vec::new();
    let order = match drop_order(before, after) {
        Some(order) => order,
        None => {
            // The gap collapsed. Renumber the column whole — one file at a
            // time, whole numbers, keeping the order the operator is looking
            // at — and place the moved card in the hole this leaves at `at`.
            for (position, file) in column.iter().enumerate() {
                let slot = if position < at {
                    position
                } else {
                    position + 1
                };
                #[allow(clippy::cast_precision_loss)]
                let renumbered = (slot + 1) as f64 * STEP;
                // Only the files whose number actually changes: a plan that
                // rewrites a file to the bytes it already holds is a sync
                // commit nobody made.
                if (file.order - renumbered).abs() > f64::EPSILON {
                    steps.push(PlanStep::WriteFile {
                        path: format!("{session}/{}", file.rel),
                        content: set_order_in(file.text, renumbered),
                    });
                }
            }
            #[allow(clippy::cast_precision_loss)]
            let placed = (at + 1) as f64 * STEP;
            placed
        }
    };

    let moved_text = Frontmatter::set_in(
        text,
        TASK_STATUS_KEY,
        FieldValue::Str(status.as_str().to_owned()),
    );
    steps.push(PlanStep::WriteFile {
        path: format!("{session}/{moved}"),
        content: set_order_in(&moved_text, order),
    });

    Ok(Plan {
        verb: "task-move".to_owned(),
        session: session.to_owned(),
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file<'a>(rel: &'a str, text: &'a str, order: f64) -> TaskFile<'a> {
        TaskFile { rel, text, order }
    }

    fn card(order: &str, status: &str) -> String {
        format!("---\nid: 01J5AAAAAAAAAAAAAAAAAAAAAA\ntitle: A task\ntags: [task]\nstatus: {status}\norder: {order}\n---\n\n# A task\n\nBody.\n")
    }

    #[test]
    fn an_empty_column_starts_at_one_not_zero() {
        // Zero is what a file with no `order` reads as, so a card placed there
        // would claim a position it did not choose.
        assert_eq!(drop_order(None, None), Some(1.0));
    }

    #[test]
    fn the_ends_of_a_column_stay_whole_numbers() {
        assert_eq!(drop_order(Some(3.0), None), Some(4.0));
        assert_eq!(drop_order(None, Some(3.0)), Some(2.0));
        // Including below zero: a column whose top card is 1 has somewhere to
        // drop above it without renumbering anything.
        assert_eq!(drop_order(None, Some(1.0)), Some(0.0));
    }

    #[test]
    fn a_drop_between_two_cards_is_their_midpoint() {
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
        // Inverted neighbours cannot arrive from a sorted column, but a
        // hand-edited file can invert one — and no value is between them.
        assert_eq!(drop_order(Some(3.0), Some(1.0)), None);
        // Non-finite values are unreachable over JSON and reachable in YAML.
        assert_eq!(drop_order(Some(f64::NAN), Some(1.0)), None);
    }

    #[test]
    fn a_normal_move_writes_exactly_one_file() {
        let a = card("1", "todo");
        let b = card("2", "todo");
        let moved = card("7", "todo");
        let column = [file("a.md", &a, 1.0), file("b.md", &b, 2.0)];
        let plan = compile_move("active/s", "c.md", &moved, TaskStatus::Done, &column, 1)
            .expect("an ordinary card in an ordinary column");
        assert_eq!(plan.verb, "task-move");
        assert_eq!(plan.steps.len(), 1, "one card moved, one file written");
        let PlanStep::WriteFile { path, content } = &plan.steps[0] else {
            panic!("expected a write");
        };
        assert_eq!(path, "active/s/c.md");
        assert!(content.contains("status: done"), "the column is the status");
        assert!(
            content.contains("order: 1.5"),
            "and the position is the gap"
        );
        // FR-121: one key each, everything else byte-identical.
        assert!(content.contains("id: 01J5AAAAAAAAAAAAAAAAAAAAAA"));
        assert!(content.contains("# A task\n\nBody.\n"));
        assert!(content.contains("tags: [task]"), "the list is not reflowed");
    }

    #[test]
    fn dropping_into_an_empty_column_writes_the_first_number() {
        let moved = card("3", "todo");
        let plan = compile_move("active/s", "c.md", &moved, TaskStatus::Deferred, &[], 0)
            .expect("an empty column takes any card");
        let PlanStep::WriteFile { content, .. } = &plan.steps[0] else {
            panic!("expected a write");
        };
        assert!(content.contains("status: deferred"));
        assert!(content.contains("order: 1"));
    }

    #[test]
    fn an_index_past_the_end_lands_at_the_end() {
        let a = card("1", "todo");
        let column = [file("a.md", &a, 1.0)];
        let plan = compile_move("active/s", "c.md", &a, TaskStatus::Todo, &column, 99)
            .expect("an index past the end is clamped, not refused");
        let PlanStep::WriteFile { content, .. } = plan
            .steps
            .last()
            .expect("a move always writes the moved card")
        else {
            panic!("expected a write");
        };
        assert!(content.contains("order: 2"));
    }

    #[test]
    fn an_exhausted_gap_renumbers_the_column_and_writes_the_card_last() {
        let a = card("1", "todo");
        let b = card("1", "todo"); // the same number, by hand
        let moved = card("9", "done");
        let column = [file("a.md", &a, 1.0), file("b.md", &b, 1.0)];
        let plan = compile_move("active/s", "c.md", &moved, TaskStatus::Todo, &column, 1)
            .expect("an exhausted gap renumbers rather than refuses");
        let paths: Vec<&str> = plan
            .steps
            .iter()
            .map(|step| match step {
                PlanStep::WriteFile { path, .. } => path.as_str(),
                other => panic!("expected only writes, got {other:?}"),
            })
            .collect();
        // `a.md` keeps 1 and is therefore not rewritten; `b.md` moves to slot 3
        // to leave the hole at 2; the moved card is written LAST (AD-111).
        assert_eq!(paths, vec!["active/s/b.md", "active/s/c.md"]);
        let PlanStep::WriteFile { content, .. } = &plan.steps[0] else {
            panic!("expected a write");
        };
        assert!(content.contains("order: 3"), "renumbered, whole");
        let PlanStep::WriteFile { content, .. } = &plan.steps[1] else {
            panic!("expected a write");
        };
        assert!(content.contains("order: 2"), "into the hole");
        assert!(content.contains("status: todo"));
    }

    #[test]
    fn a_renumber_skips_files_that_already_hold_their_number() {
        let a = card("1", "todo");
        let b = card("2", "todo");
        let moved = card("5", "todo");
        // The collapse is at the front, so every later card already sits where
        // the renumber would put it.
        let column = [file("a.md", &a, 1.0), file("b.md", &b, 2.0)];
        let plan = compile_move("active/s", "c.md", &moved, TaskStatus::Todo, &column, 2)
            .expect("a drop at the end of a column");
        assert_eq!(plan.steps.len(), 1, "nothing to renumber at the end");
    }

    #[test]
    fn a_card_gains_the_keys_it_never_had() {
        // The case a hand-written or agent-written task is in: no `order`, and
        // a `status` the reader defaulted rather than read.
        let bare = "---\ntitle: Bare\ntags: [task]\n---\n\nBody.\n";
        let plan = compile_move("active/s", "c.md", bare, TaskStatus::Done, &[], 0)
            .expect("a card missing both keys is still a card");
        let PlanStep::WriteFile { content, .. } = &plan.steps[0] else {
            panic!("expected a write");
        };
        assert!(content.contains("status: done"));
        assert!(content.contains("order: 1"));
        assert!(content.contains("title: Bare"), "and keeps what it had");
        // Still no `id`: keeper does not stamp a file it did not author.
        assert!(!content.contains("id:"));
    }

    #[test]
    fn a_path_that_is_not_a_card_is_refused_before_anything_is_planned() {
        // The fence, one scope in from where it is enforced (AD-113).
        assert!(compile_move("active/s", "workspace/x.md", "", TaskStatus::Todo, &[], 0).is_err());
        assert!(compile_move("active/s", "../x.md", "", TaskStatus::Todo, &[], 0).is_err());
        assert!(compile_move("active/s", "shot.png", "", TaskStatus::Todo, &[], 0).is_err());
    }
}
