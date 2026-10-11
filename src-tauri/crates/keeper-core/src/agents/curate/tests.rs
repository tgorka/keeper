use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use matrix_sdk::ruma::OwnedUserId;
use ulid::Ulid;

use super::*;
use crate::agents::consolidate::{self, Night, SessionFacts};
use crate::agents::label::{Integrity, Label, Readers};
use crate::agents::memory::MemoryTarget;
use crate::agents::proposal::{Op, Target};
use crate::agents::skills::{index, metadata_value, SkillFilter};

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-04T04:00:00Z")
        .expect("time")
        .with_timezone(&Utc)
}

fn tgorka() -> OwnedUserId {
    OwnedUserId::try_from("@tgorka:example.org").expect("user")
}

/// A pending proposal of nixi's from a session of `origin`, made
/// `days_ago` days before the sweep: its stem and text.
fn proposal(seq: u128, origin: Origin, days_ago: i64) -> (String, String) {
    proposal_at(seq, origin, now() - Duration::days(days_ago))
}

/// A pending proposal of nixi's from a session of `origin`, made at
/// `created`: its stem and text.
fn proposal_at(seq: u128, origin: Origin, created: DateTime<Utc>) -> (String, String) {
    let id = Ulid::from_parts(
        u64::try_from(created.timestamp_millis()).expect("after 1970"),
        seq,
    );
    let text = Proposal {
        id,
        agent: "nixi".to_owned(),
        target: Target::Memory(MemoryTarget::Memory),
        op: Op::Add,
        matched: None,
        session: "60-sessions/active/s0".to_owned(),
        host: "electra".to_owned(),
        origin,
        label: Label {
            readers: Readers::Only(BTreeSet::from([tgorka()])),
            integrity: Integrity::Owner,
            local_only: false,
        },
        created_at: created,
        body: "tgorka reviews on Fridays".to_owned(),
    }
    .render();
    (id.to_string(), text)
}

fn plan_of(proposals: &[(String, String, String)]) -> SweepPlan {
    sweep(&Sweep {
        skills: &[],
        archived: &BTreeSet::new(),
        listed: &BTreeSet::new(),
        workflows: &[],
        incomplete: None,
        proposals,
        decided_by: "curator@electra",
        now: now(),
    })
}

/// An agent's own skill `name`, last changed `days_ago` days before the
/// sweep.
fn managed_skill(name: &str, days_ago: i64) -> Skill {
    Skill {
        name: name.to_owned(),
        text: format!(
            "---\nname: {name}\ndescription: Does {name}.\nmetadata:\n  keeper_proposal: 01J9ZZ5K8V9Q3W2E1R0T7Y6X5Z\n---\n\nSteps.\n"
        ),
        last_change: Some(now() - Duration::days(days_ago)),
        uncommitted: false,
    }
}

fn skills_plan(skills: &[Skill], incomplete: Option<&str>) -> SweepPlan {
    sweep(&Sweep {
        skills,
        archived: &BTreeSet::new(),
        listed: &BTreeSet::new(),
        workflows: &[],
        incomplete,
        proposals: &[],
        decided_by: "curator@electra",
        now: now(),
    })
}

/// R95U-04: when the names that protect a skill could not all be read,
/// absence from them proves nothing — no skill moves that week, not even
/// to stale; read whole, the same skills move.
#[test]
fn an_incomplete_protection_read_protects_every_skill() {
    let skills = [managed_skill("old", 31), managed_skill("aging", 15)];
    let whole = skills_plan(&skills, None);
    assert_eq!(whole.archives.len(), 1);
    assert_eq!(whole.stale, ["aging"]);

    let partial = skills_plan(
        &skills,
        Some("80-agents/_workflows holds more than 2000 files"),
    );
    assert!(!partial.commits(), "{partial:?}");
}

/// R95U3-01: ownership and pinning are read as the offered index reads
/// adoption (R204). A month-old agent's skill whose `keeper_pinned` is said
/// twice, or whose pin sits in a second `metadata` block, is neither
/// archived nor marked — no occurrence is taken for the answer — and the
/// plan says it stayed; the same skill read whole beside it is archived.
#[test]
fn ambiguous_ownership_metadata_keeps_a_skill() {
    let ambiguous = |name: &str, metadata: &str| Skill {
        name: name.to_owned(),
        text: format!("---\nname: {name}\ndescription: Does {name}.\n{metadata}---\n\nSteps.\n"),
        ..managed_skill(name, 31)
    };
    let skills = [
        ambiguous(
            "pinned-twice",
            "metadata:\n  keeper_proposal: pending\n  keeper_pinned: \"false\"\n  keeper_pinned: \"true\"\n",
        ),
        ambiguous(
            "two-blocks",
            "metadata:\n  keeper_proposal: pending\nmetadata:\n  keeper_pinned: \"true\"\n",
        ),
        managed_skill("old", 31),
    ];
    for skill in &skills[..2] {
        assert_eq!(
            ownership(&skill.text),
            Ownership::Unreadable,
            "{}",
            skill.name
        );
    }
    let plan = skills_plan(&skills, None);
    let archived: Vec<&str> = plan.archives.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(archived, ["_skills/old"]);
    assert!(plan.marks.is_empty(), "{plan:?}");
    assert_eq!(plan.notes.len(), 2, "{plan:?}");
}

/// R95U-09 and R95U-07: a skill whose last change the history read did
/// not reach, or whose folder holds a change not committed yet, stays as
/// it is; one beside it as old moves.
#[test]
fn a_skill_of_unknown_age_or_with_a_change_on_the_disk_stays() {
    let unknown = Skill {
        last_change: None,
        ..managed_skill("unknown", 90)
    };
    let edited = Skill {
        uncommitted: true,
        ..managed_skill("edited", 90)
    };
    let plan = skills_plan(&[unknown, edited, managed_skill("old", 90)], None);
    let archived: Vec<&str> = plan.archives.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(archived, ["_skills/old"]);
    assert!(plan.marks.is_empty());
    assert_eq!(plan.notes.len(), 2, "{plan:?}");
}

/// R95U-10/11, acceptance 8's boundary: a gate proposal expires at exactly
/// 30 days of elapsed time, one second short of it does not, and the same
/// instant written at another UTC offset is the same age.
#[test]
fn a_gate_proposal_expires_at_exactly_thirty_days() {
    let due = now() - Duration::days(GATE_PROPOSAL_DAYS);
    let (short, short_text) = proposal_at(1, Origin::Gate, due + Duration::seconds(1));
    let (exact, exact_text) = proposal_at(2, Origin::Gate, due);
    let (offset, offset_text) = proposal_at(3, Origin::Gate, due);
    let utc = due.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let shifted = due
        .with_timezone(&chrono::FixedOffset::east_opt(2 * 3600).expect("offset"))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    assert!(offset_text.contains(&utc), "{offset_text}");
    let offset_text = offset_text.replace(&utc, &shifted);
    let home = |stem: &str, text: &str| ("nixi".to_owned(), stem.to_owned(), text.to_owned());
    let plan = plan_of(&[
        home(&short, &short_text),
        home(&exact, &exact_text),
        home(&offset, &offset_text),
    ]);
    let expired: Vec<String> = plan
        .expired
        .iter()
        .map(|(_, settled)| settled.id.to_string())
        .collect();
    assert_eq!(expired, [exact, offset]);
}

/// 95.3 acceptance 5: setting `metadata.keeper_stale` on a `SKILL.md` in
/// keeper's canonical shape adds one line inside the `metadata` map and
/// nothing else, agentskills still accepts the file (still waiting for a
/// person), and clearing the mark gives back the original bytes.
#[test]
fn the_stale_mark_changes_one_key() {
    let original = "---\nname: tidy\ndescription: Tidy the inbox.\nmetadata:\n  keeper_proposal: 01J9ZZ5K8V9Q3W2E1R0T7Y6X5Z\nlicense: MIT\n---\n\n# Tidy\n\nSteps.\n";
    let marked = set_metadata(original, STALE_KEY, Some("2026-10-04"));
    let line = "  keeper_stale: 2026-10-04\n";
    let at = original.find("license:").expect("the key after metadata");
    assert_eq!(
        marked,
        format!("{}{line}{}", &original[..at], &original[at..]),
        "one line in the metadata map, every other byte kept"
    );
    assert_eq!(
        metadata_value(&marked, STALE_KEY).as_deref(),
        Some("2026-10-04")
    );
    let checked = index(&[("tidy".to_owned(), marked.clone())], &SkillFilter::All);
    assert!(checked.refused.is_empty(), "{checked:?}");
    assert_eq!(checked.waiting, ["tidy"]);
    assert_eq!(set_metadata(&marked, STALE_KEY, None), original);
}

/// 95.3 acceptance 8, the plan: a `gate` proposal 29 days old is left
/// alone; at 30 days or more it expires, unread, decided by the curator;
/// a proposal of any other origin is never the sweep's, however old. The
/// nights in between settle nothing of it: they skip it unscored.
#[test]
fn gate_proposals_expire_unread() {
    let (young, young_text) = proposal(1, Origin::Gate, 29);
    let (due, due_text) = proposal(2, Origin::Gate, 30);
    let (old, old_text) = proposal(3, Origin::Gate, 45);
    let (mine, mine_text) = proposal(4, Origin::Foreground, 60);
    let home = |stem: &str, text: &str| ("nixi".to_owned(), stem.to_owned(), text.to_owned());
    let plan = plan_of(&[
        home(&young, &young_text),
        home(&due, &due_text),
        home(&old, &old_text),
        home(&mine, &mine_text),
        home("not-a-ulid", "garbage"),
    ]);
    let expired: Vec<String> = plan
        .expired
        .iter()
        .map(|(home, settled)| {
            assert_eq!(home, "nixi");
            assert_eq!(settled.verdict, Verdict::Expired);
            assert_eq!(settled.decided_by, "curator@electra");
            settled.id.to_string()
        })
        .collect();
    assert_eq!(expired, [old.clone(), due.clone()], "in id order");
    assert!(plan.commits());
    assert!(plan.marks.is_empty() && plan.archives.is_empty());

    // Every night before, the consolidator scored none of them, wrote no
    // verdict and read none into MEMORY.md.
    let night = consolidate::plan(&Night {
        agent: "nixi",
        home: "nixi",
        decided_by: "consolidator@electra",
        owner: &tgorka(),
        readers: &BTreeSet::from([tgorka()]),
        local_only: false,
        promote: true,
        user: None,
        memory: Some("---\ntype: memory\n---\n"),
        skills: &BTreeMap::new(),
        proposals: &[(young, young_text), (due, due_text), (old, old_text)],
        under_review: &BTreeSet::new(),
        reviewing: &BTreeSet::new(),
        declined: &BTreeMap::new(),
        sessions: &|_: &str| {
            Some(SessionFacts {
                requested_by: tgorka(),
            })
        },
        now: now() - Duration::days(1),
    });
    assert!(night.settled.is_empty(), "{night:?}");
    assert!(night.writes.is_empty(), "{night:?}");
    assert_eq!(night.skipped.len(), 3);
}
