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
//! **The position is fractional on purpose**, and the arithmetic is not this
//! module's. [`crate::notes::order::drop_order`] owns it, because `order` is a
//! property of the note and a board is only one thing that drags notes around —
//! the widget board in an ordinary note (FR-264) drops cards through the same
//! function. This module owns what is genuinely a session's: turning a drop into
//! a [`Plan`] against session-relative paths.
//!
//! **Normally.** Halving a gap forever runs out of `f64`, and `drop_order`
//! answers `None` rather than pretending otherwise; [`compile_move`] then
//! renumbers the target column with whole numbers — every file in it, in one
//! plan. That is a rare, bounded, visible cost, and the alternative is a drop
//! that silently does nothing because the card landed on a tie the title
//! break then resolved the other way.
//!
//! Pure, like the rest of the domain: the shell reads the column's files and
//! executes the plan. Nothing here opens a file, and nothing here mints an id —
//! a task keeper did not author keeps its bytes (FR-121).

use matrix_sdk::ruma::UserId;

use crate::agents::card::{ALLOWED_BY, SCHEDULED_BY};
use crate::notes::frontmatter::{FieldValue, Frontmatter};
use crate::notes::order::{drop_order, renumbered_order, set_order_in};
use crate::sessions::files::{check_rel, FileVerbError};
use crate::sessions::plan::{Plan, PlanStep};
use crate::sessions::shape::TaskStatus;

/// The frontmatter key that decides a card's column.
pub const TASK_STATUS_KEY: &str = "status";

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
                let renumbered = renumbered_order(slot);
                // Only the files whose number actually changes: a plan that
                // rewrites a file to the bytes it already holds is a sync
                // commit nobody made.
                if (file.order - renumbered).abs() > f64::EPSILON {
                    steps.push(PlanStep::guarded(
                        format!("{session}/{}", file.rel),
                        file.text,
                        set_order_in(file.text, renumbered),
                    ));
                }
            }
            renumbered_order(at)
        }
    };

    let moved_text = Frontmatter::set_in(
        text,
        TASK_STATUS_KEY,
        FieldValue::Str(status.as_str().to_owned()),
    );
    steps.push(PlanStep::guarded(
        format!("{session}/{moved}"),
        text,
        set_order_in(&moved_text, order),
    ));

    Ok(Plan {
        verb: "task-move".to_owned(),
        session: session.to_owned(),
        steps,
    })
}

/// Why a person's *Allow* compiled no plan.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AllowError {
    #[error(transparent)]
    Path(#[from] FileVerbError),
    #[error(
        "{rel} carries no schedule an agent wrote, so there is nothing to allow. It runs on its \
         schedule as it is."
    )]
    NothingToAllow { rel: String },
    #[error("{person} is not a Matrix user id, so it cannot be recorded as who allowed this.")]
    NotAPerson { person: String },
    #[error("Sign in as {owner} to allow this schedule.")]
    SignIn { owner: String },
}

/// The plan a person's *Allow* runs on a card whose schedule an agent wrote
/// (Q16): its `scheduled_by:` line becomes `allowed_by: <person>` (R76, the
/// requester of its scheduled runs), and no other byte changes.
///
/// A [`PlanStep::GuardedWrite`] on the exact bytes the shell read (their
/// SHA-256, R120), so a card rewritten between the read and the write — by
/// even one same-length edit — is refused rather than reverted.
///
/// # Errors
/// Whatever [`check_rel`] refuses, [`AllowError::NothingToAllow`] for a card
/// without the mark, [`AllowError::NotAPerson`] for a `person` that is not a
/// Matrix user id.
pub fn compile_allow_schedule(
    session: &str,
    rel: &str,
    text: &str,
    person: &str,
) -> Result<Plan, AllowError> {
    check_rel(rel)?;
    let person = UserId::parse(person).map_err(|_| AllowError::NotAPerson {
        person: person.to_owned(),
    })?;
    let (frontmatter, _) = Frontmatter::parse(text);
    if !frontmatter.keys().any(|key| key == SCHEDULED_BY) {
        return Err(AllowError::NothingToAllow {
            rel: rel.to_owned(),
        });
    }
    // Every `scheduled_by:` goes, and the one `allowed_by:` takes the first
    // one's place: a card with the key twice is never left marked.
    let allowed = Frontmatter::remove_all_in(text, ALLOWED_BY);
    let allowed = Frontmatter::set_after_in(
        &allowed,
        &[SCHEDULED_BY],
        ALLOWED_BY,
        FieldValue::Str(person.to_string()),
    );
    Ok(Plan {
        verb: "task-allow-schedule".to_owned(),
        session: session.to_owned(),
        steps: vec![PlanStep::guarded(
            format!("{session}/{rel}"),
            text,
            Frontmatter::remove_all_in(&allowed, SCHEDULED_BY),
        )],
    })
}

/// Who a person's *Allow* records (R118): of the accounts signed in on this
/// device, the one whose user owns the drive by its `_drive.toml`, else the
/// only one. With neither, it is refused, naming whom to sign in as.
pub fn allowing_person(owner: Option<&str>, signed_in: &[String]) -> Result<String, AllowError> {
    if let Some(owner) = owner.filter(|owner| signed_in.iter().any(|user| user == owner)) {
        return Ok(owner.to_owned());
    }
    match signed_in {
        [only] => Ok(only.clone()),
        _ => Err(AllowError::SignIn {
            owner: owner.unwrap_or("the drive's owner").to_owned(),
        }),
    }
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
    fn a_normal_move_writes_exactly_one_file() {
        let a = card("1", "todo");
        let b = card("2", "todo");
        let moved = card("7", "todo");
        let column = [file("a.md", &a, 1.0), file("b.md", &b, 2.0)];
        let plan = compile_move("active/s", "c.md", &moved, TaskStatus::Done, &column, 1)
            .expect("an ordinary card in an ordinary column");
        assert_eq!(plan.verb, "task-move");
        assert_eq!(plan.steps.len(), 1, "one card moved, one file written");
        let PlanStep::GuardedWrite { path, content, .. } = &plan.steps[0] else {
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
        let PlanStep::GuardedWrite { content, .. } = &plan.steps[0] else {
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
        let PlanStep::GuardedWrite { content, .. } = plan
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
                PlanStep::GuardedWrite { path, .. } => path.as_str(),
                other => panic!("expected only writes, got {other:?}"),
            })
            .collect();
        // `a.md` keeps 1 and is therefore not rewritten; `b.md` moves to slot 3
        // to leave the hole at 2; the moved card is written LAST (AD-111).
        assert_eq!(paths, vec!["active/s/b.md", "active/s/c.md"]);
        let PlanStep::GuardedWrite { content, .. } = &plan.steps[0] else {
            panic!("expected a write");
        };
        assert!(content.contains("order: 3"), "renumbered, whole");
        let PlanStep::GuardedWrite { content, .. } = &plan.steps[1] else {
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
        let PlanStep::GuardedWrite { content, .. } = &plan.steps[0] else {
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

    /// A card carrying all nine agent keys, and the person's two.
    const AGENT_CARD: &str = "---\ntitle: Tidy the inbox\ntags: [task]\nstatus: todo\norder: 1\nassignee: tola-grey\nhost: hesperia\nrequested_by: \"@nixi:h\"\nschedule: \"@daily\"\nworkflow: triage\nscheduled_by: \"@nixi:h\"\nintegrity: untrusted\nrun: blocked\nlast_run: \"2026-10-04T09:00:00+02:00\"\n---\n\nSort what came in.\n";

    /// AC3: a move rewrites `status:` and `order:` and leaves every agent
    /// key's line as it was.
    #[test]
    fn moving_a_card_keeps_its_agent_keys_byte_for_byte() {
        let plan = compile_move("active/s", "inbox.md", AGENT_CARD, TaskStatus::Done, &[], 0)
            .expect("an agent card is a card");
        let PlanStep::GuardedWrite { content, .. } = &plan.steps[0] else {
            panic!("expected a write");
        };
        assert!(content.contains("status: done\n"));
        let kept = |text: &str| -> Vec<String> {
            text.lines()
                .filter(|line| !line.starts_with("status:") && !line.starts_with("order:"))
                .map(str::to_owned)
                .collect()
        };
        assert_eq!(kept(content), kept(AGENT_CARD));
        assert_eq!(AGENT_CARD.lines().count(), content.lines().count());
    }

    /// AC11 with R76: *Allow* turns the `scheduled_by:` line into
    /// `allowed_by: <person>` and changes no other byte; a card without the
    /// mark is refused.
    #[test]
    fn allowing_a_schedule_removes_one_line() {
        let plan = compile_allow_schedule("active/s", "inbox.md", AGENT_CARD, "@tgorka:h")
            .expect("a marked card");
        assert_eq!(plan.steps.len(), 1);
        let PlanStep::GuardedWrite {
            path,
            expect_len,
            content,
            ..
        } = &plan.steps[0]
        else {
            panic!("a guarded write");
        };
        assert_eq!(path, "active/s/inbox.md");
        assert_eq!(*expect_len, AGENT_CARD.len());
        assert_eq!(
            *content,
            AGENT_CARD.replace("scheduled_by: \"@nixi:h\"\n", "allowed_by: \"@tgorka:h\"\n")
        );

        let unmarked = AGENT_CARD.replace("scheduled_by: \"@nixi:h\"\n", "");
        assert_eq!(
            compile_allow_schedule("active/s", "inbox.md", &unmarked, "@tgorka:h"),
            Err(AllowError::NothingToAllow {
                rel: "inbox.md".to_owned()
            })
        );
        assert!(matches!(
            compile_allow_schedule("active/s", "workspace/x.md", AGENT_CARD, "@tgorka:h"),
            Err(AllowError::Path(_))
        ));
        assert!(matches!(
            compile_allow_schedule("active/s", "inbox.md", AGENT_CARD, "tgorka"),
            Err(AllowError::NotAPerson { .. })
        ));
    }

    /// R4-05: a card carrying `scheduled_by:` twice is left with neither,
    /// and one `allowed_by:`.
    #[test]
    fn allowing_a_doubly_marked_card_leaves_no_mark() {
        let doubled = AGENT_CARD.replace(
            "scheduled_by: \"@nixi:h\"\n",
            "scheduled_by: \"@nixi:h\"\nallowed_by: \"@old:h\"\nscheduled_by: \"@nixi:h\"\n",
        );
        let plan = compile_allow_schedule("active/s", "inbox.md", &doubled, "@tgorka:h")
            .expect("a marked card");
        let PlanStep::GuardedWrite { content, .. } = &plan.steps[0] else {
            panic!("a guarded write");
        };
        let (fm, _) = Frontmatter::parse(content);
        assert_eq!(fm.count(SCHEDULED_BY), 0, "{content}");
        assert_eq!(fm.lines_of(ALLOWED_BY), "allowed_by: \"@tgorka:h\"\n");
    }

    /// R118: the drive's owner when signed in, else the only account
    /// signed in, else a sentence naming whom to sign in as.
    #[test]
    fn the_person_allowing_is_the_owner_or_the_only_account() {
        let (owner, marta, x) = ("@tgorka:h", "@marta:h".to_owned(), "@x:h".to_owned());
        assert_eq!(
            allowing_person(Some(owner), &[marta.clone(), owner.to_owned()]),
            Ok(owner.to_owned())
        );
        assert_eq!(
            allowing_person(Some(owner), std::slice::from_ref(&marta)),
            Ok(marta.clone())
        );
        assert_eq!(
            allowing_person(None, std::slice::from_ref(&marta)),
            Ok(marta.clone())
        );
        let refused = allowing_person(Some(owner), &[]).expect_err("nobody signed in");
        assert_eq!(
            refused.to_string(),
            "Sign in as @tgorka:h to allow this schedule."
        );
        assert!(allowing_person(None, &[marta.clone(), x.clone()]).is_err());
        assert!(
            allowing_person(Some(owner), &[marta, x]).is_err(),
            "two others signed in, the owner not: nobody is guessed"
        );
    }
}
