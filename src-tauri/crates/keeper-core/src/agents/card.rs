//! An agent's card: a session task file with the agent keys of AD-386
//! (stories 92.1, 92.2).
//!
//! A card is an ordinary task (`tags: [task]`, one of the four `status:`
//! columns) whose frontmatter may also carry nine agent keys. `run:` is the
//! agent's run, never a column; the host writes it. `scheduled_by:` marks a
//! schedule an agent wrote, which runs only after a person's tick (Q16), and
//! `integrity: untrusted` marks a card made from outside content (Q17). An
//! agent's write never removes either mark: [`stamp_agent_write`] is the one
//! function every door an agent writes a card through passes.

use matrix_sdk::ruma::UserId;

use crate::agents::label::Integrity;
use crate::notes::frontmatter::{FieldValue, Frontmatter};

/// The agent's run on the card.
pub const RUN: &str = "run";
/// The agent the card is for: an agent id of the card's drive.
pub const ASSIGNEE: &str = "assignee";
/// A host slug the card's run is pinned to.
pub const HOST: &str = "host";
/// Who asked: a Matrix user id.
pub const REQUESTED_BY: &str = "requested_by";
/// When it runs, kept raw: keeper-sync's parser answers whether it reads.
pub const SCHEDULE: &str = "schedule";
/// The window the last run ran in, RFC 3339.
pub const LAST_RUN: &str = "last_run";
/// The workflow it runs.
pub const WORKFLOW: &str = "workflow";
/// The agent whose write set the schedule or the workflow.
pub const SCHEDULED_BY: &str = "scheduled_by";
/// `untrusted`: the card was made from outside content.
pub const INTEGRITY: &str = "integrity";

/// The nine agent keys, in the order a card keeps them.
pub const KEYS: [&str; 9] = [
    RUN,
    ASSIGNEE,
    HOST,
    REQUESTED_BY,
    SCHEDULE,
    LAST_RUN,
    WORKFLOW,
    SCHEDULED_BY,
    INTEGRITY,
];

/// The six values of `run:` (`waiting` by ruling R25).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Run {
    Queued,
    Running,
    Waiting,
    Blocked,
    Review,
    Failed,
}

impl Run {
    pub const ALL: [Run; 6] = [
        Run::Queued,
        Run::Running,
        Run::Waiting,
        Run::Blocked,
        Run::Review,
        Run::Failed,
    ];

    /// The word the card holds.
    pub fn as_str(self) -> &'static str {
        match self {
            Run::Queued => "queued",
            Run::Running => "running",
            Run::Waiting => "waiting",
            Run::Blocked => "blocked",
            Run::Review => "review",
            Run::Failed => "failed",
        }
    }

    /// The value a card's `run:` holds, trimmed and lower-cased as a
    /// `status:` is; `None` for anything else, a column's name included.
    pub fn parse(word: &str) -> Option<Run> {
        let word = word.trim().to_ascii_lowercase();
        Run::ALL.into_iter().find(|run| run.as_str() == word)
    }
}

/// `text` with `run:` set to `run`; `None` when it already says so, so the
/// host writes only on a transition.
pub fn set_run(text: &str, run: Run) -> Option<String> {
    let (frontmatter, _) = Frontmatter::parse(text);
    if frontmatter.as_string(RUN).and_then(Run::parse) == Some(run) {
        return None;
    }
    Some(Frontmatter::set_in(
        text,
        RUN,
        FieldValue::Str(run.as_str().to_owned()),
    ))
}

/// What an agent's write of a card stores, given the card's bytes before it
/// (`None` for a new card), the bytes the agent wrote, the agent's user and
/// the integrity of the session it wrote from:
///
/// - a write that sets or changes `schedule:` or `workflow:` stores
///   `scheduled_by: <agent>`, whatever the agent wrote there;
/// - any other write keeps the `scheduled_by:` the card had — dropped,
///   replaced or newly written, it is put back as it was (or taken out);
/// - a write from a session at `untrusted` integrity stores
///   `integrity: untrusted`; any other keeps the card's `integrity:` as it
///   was, so only a person removes or changes either mark.
///
/// Every other byte is the agent's.
pub fn stamp_agent_write(
    old: Option<&str>,
    new: &str,
    agent: &UserId,
    integrity: Integrity,
) -> String {
    let before = old.map(|text| Frontmatter::parse(text).0);
    let (after, _) = Frontmatter::parse(new);
    let was = |key: &str| before.as_ref().and_then(|fm| fm.get(key)).cloned();
    let sets = |key: &str| {
        after
            .get(key)
            .is_some_and(|value| Some(value) != was(key).as_ref())
    };
    // The mark the card had, back in place of whatever the write says.
    let keep = |out: String, key: &str| match (was(key), after.get(key)) {
        (Some(mark), now) if now != Some(&mark) => Frontmatter::set_in(&out, key, mark),
        (None, Some(_)) => Frontmatter::remove_in(&out, key),
        _ => out,
    };
    let out = if sets(SCHEDULE) || sets(WORKFLOW) {
        Frontmatter::set_in(new, SCHEDULED_BY, FieldValue::Str(agent.to_string()))
    } else {
        keep(new.to_owned(), SCHEDULED_BY)
    };
    let untrusted = FieldValue::Str(Integrity::Untrusted.as_word().to_owned());
    // An agent may mark its own card untrusted; only a person lifts it.
    if after.get(INTEGRITY) == Some(&untrusted) {
        out
    } else if integrity == Integrity::Untrusted {
        Frontmatter::set_in(&out, INTEGRITY, untrusted)
    } else {
        keep(out, INTEGRITY)
    }
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::OwnedUserId;

    use super::*;

    fn nixi() -> OwnedUserId {
        UserId::parse("@nixi:h").expect("user")
    }

    fn field(text: &str, key: &str) -> Option<String> {
        Frontmatter::parse(text).0.as_string(key).map(str::to_owned)
    }

    const CARD: &str = "---\ntags: [task]\ntitle: Tidy the inbox\nstatus: todo\nassignee: tola-grey\n---\n\nSort what came in.\n";

    #[test]
    fn run_reads_its_six_words_and_nothing_else() {
        for run in Run::ALL {
            assert_eq!(Run::parse(run.as_str()), Some(run));
        }
        assert_eq!(Run::parse(" Running "), Some(Run::Running));
        assert_eq!(Run::parse("todo"), None, "a column is not a run");
        assert_eq!(Run::parse("blocked!"), None);
    }

    #[test]
    fn set_run_writes_only_on_a_transition() {
        let queued = set_run(CARD, Run::Queued).expect("a transition");
        assert_eq!(field(&queued, RUN).as_deref(), Some("queued"));
        assert_eq!(set_run(&queued, Run::Queued), None);
        let review = set_run(&queued, Run::Review).expect("a transition");
        assert_eq!(field(&review, RUN).as_deref(), Some("review"));
        assert_eq!(
            review.replace("run: review\n", ""),
            queued.replace("run: queued\n", ""),
            "one key changed"
        );
    }

    /// Q16, Q17: an agent's schedule is marked and stays marked; a session
    /// at `untrusted` integrity marks the card; nothing else is touched.
    #[test]
    fn an_agents_card_write_is_stamped_and_never_unstamped() {
        let scheduled = CARD.replace("assignee", "schedule: \"@daily\"\nassignee");
        let stamped = stamp_agent_write(None, &scheduled, &nixi(), Integrity::Agent);
        assert_eq!(field(&stamped, SCHEDULED_BY).as_deref(), Some("@nixi:h"));
        assert_eq!(field(&stamped, INTEGRITY), None);

        // A rewrite that drops the mark gets it back; one that leaves the
        // schedule as it was adds nothing more.
        let dropped = stamp_agent_write(Some(&stamped), &scheduled, &nixi(), Integrity::Agent);
        assert_eq!(field(&dropped, SCHEDULED_BY).as_deref(), Some("@nixi:h"));
        let plain = stamp_agent_write(Some(CARD), CARD, &nixi(), Integrity::Owner);
        assert_eq!(plain, CARD);

        // A forged mark is overwritten by the writer's own.
        let forged = scheduled.replace("assignee", "scheduled_by: \"@tgorka:h\"\nassignee");
        let honest = stamp_agent_write(Some(CARD), &forged, &nixi(), Integrity::Agent);
        assert_eq!(field(&honest, SCHEDULED_BY).as_deref(), Some("@nixi:h"));

        let untrusted = stamp_agent_write(None, CARD, &nixi(), Integrity::Untrusted);
        assert_eq!(field(&untrusted, INTEGRITY).as_deref(), Some("untrusted"));
        let kept = stamp_agent_write(Some(&untrusted), CARD, &nixi(), Integrity::Owner);
        assert_eq!(field(&kept, INTEGRITY).as_deref(), Some("untrusted"));
        assert!(kept.ends_with("\nSort what came in.\n"));
    }

    /// An ordinary rewrite — the schedule as it was — that replaces either
    /// mark's value, or writes one the card never had, leaves the card's
    /// marks exactly as they were.
    #[test]
    fn an_agents_rewrite_cannot_replace_a_mark() {
        let scheduled = CARD.replace("assignee", "schedule: \"@daily\"\nassignee");
        let stamped = stamp_agent_write(None, &scheduled, &nixi(), Integrity::Untrusted);
        let rewrite = stamped
            .replace("scheduled_by: \"@nixi:h\"", "scheduled_by: \"@tgorka:h\"")
            .replace("integrity: untrusted", "integrity: owner")
            .replace("Sort what came in.", "Sorted.");
        assert_eq!(field(&rewrite, SCHEDULED_BY).as_deref(), Some("@tgorka:h"));
        assert_eq!(field(&rewrite, INTEGRITY).as_deref(), Some("owner"));
        let kept = stamp_agent_write(Some(&stamped), &rewrite, &nixi(), Integrity::Owner);
        assert_eq!(field(&kept, SCHEDULED_BY).as_deref(), Some("@nixi:h"));
        assert_eq!(field(&kept, INTEGRITY).as_deref(), Some("untrusted"));
        assert!(kept.ends_with("\nSorted.\n"), "{kept}");

        // A mark the card never had is not the agent's to add.
        let forged = CARD.replace(
            "assignee",
            "scheduled_by: \"@tgorka:h\"\nintegrity: owner\nassignee",
        );
        let plain = stamp_agent_write(Some(CARD), &forged, &nixi(), Integrity::Owner);
        assert_eq!(field(&plain, SCHEDULED_BY), None);
        assert_eq!(field(&plain, INTEGRITY), None);
    }
}
