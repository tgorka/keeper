//! The weekly curator's sweep (AD-402, story 95.3): what a drive's agent
//! skills and gate proposals come to this week.
//!
//! Pure. The host reads the drive at one fetched commit — each skill's
//! `SKILL.md` and its last change from git, the names its agents'
//! `[tools].skills` and its `_workflows/` files give, and every pending
//! proposal of the homes it serves — hands them here, and writes what
//! [`sweep`] answers through keeper-sync's one commit path.
//!
//! - **Managed** = an agent's skill no person adopted: `metadata` holds
//!   `keeper_proposal` and not `keeper_pinned: "true"`. Nothing else is
//!   touched — a person's skill, an adopted one, a pinned one — and
//!   neither is a skill whose `metadata` keeper cannot read as one block
//!   map of distinct keys (R204): whose it is, or whether it is pinned, is
//!   not known, so it stays and the plan says why.
//! - **Named** skills are in use by definition and never move: one an
//!   agent's `[tools].skills` names, or one a `_workflows/` file names as a
//!   whole word. When those names could not all be read, no skill is
//!   archived that week (R208): absence from a partial read proves nothing.
//! - **Last use = last change.** A managed skill is never offered (R28
//!   S-12), so nothing ever views it: its clock is the latest commit that
//!   changed its folder other than the curator's own — a newer applied
//!   patch is a use, a stale mark is not — and Hermes' state machine
//!   decides: stale at 14 days (`metadata.keeper_stale: "<date>"` set),
//!   archived at 30 (the folder moved whole to `_skills/.archive/<name>/`,
//!   never deleted), active again when changed since (the mark cleared). A
//!   skill whose last change is not known, or that holds a change not
//!   committed yet, is left alone.
//! - **A gate's proposals** are skipped by every night (R29 F4); the first
//!   sweep on or after a pending `gate` proposal's 30th day gives it
//!   `verdict = "expired"`, unread.

use std::collections::BTreeSet;

use chrono::{DateTime, Duration, Utc};
use keeper_ported::hermes::curator::{self, Row, State, Windows};

use crate::agents::consolidate::{After, FileChange, Settled, Verdict, ARCHIVE_DIR};
use crate::agents::proposal::{Origin, Proposal};
use crate::agents::skills::{metadata_of, set_metadata, PROPOSAL_KEY};
use crate::notes::frontmatter::{FieldValue, Frontmatter};

/// When the sweep runs: Sundays at 04:00 at the curating host's offset,
/// after the night's 03:00 (R129's rule, weekly).
pub const SCHEDULE: &str = "0 4 * * 0";
/// The `metadata` key a stale skill carries: the date it was marked.
pub const STALE_KEY: &str = "keeper_stale";
/// The `metadata` key that keeps an agent's skill from the curator when
/// it reads `"true"`.
pub const PINNED_KEY: &str = "keeper_pinned";
/// Days a pending `gate` proposal waits before the sweep expires it.
pub const GATE_PROPOSAL_DAYS: i64 = 30;

/// The curator's job: its completion is `claim::completion_key(JOB, drive)`;
/// it runs under the drive's one maintenance claim (R207).
pub const JOB: &str = "curate";

/// `Memory-Origin` of a sweep's commit.
pub fn origin_of_host(host: &str) -> String {
    format!("curator@{host}")
}

/// Whether a `Memory-Origin` value is a curator's: `curator@` and a host's
/// slug, nothing else.
pub fn is_curator_origin(origin: &str) -> bool {
    origin
        .strip_prefix("curator@")
        .is_some_and(|host| crate::agents::log::HostSlug::new(host).is_ok())
}

/// What the curator may make of a `SKILL.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    /// An agent's skill no person adopted or pinned; `stale` when it
    /// carries the stale mark.
    Managed { stale: bool },
    /// A person's, an adopted or a pinned skill: never touched.
    Kept,
    /// Its `metadata` is not one block map of distinct keys, so whether it
    /// is managed is not known: never touched.
    Unreadable,
}

/// What the curator may make of `text`, its `metadata` read as the
/// offered index reads it ([`metadata_of`]).
pub fn ownership(text: &str) -> Ownership {
    let (frontmatter, _) = Frontmatter::parse(text);
    let Some(pairs) = metadata_of(&frontmatter) else {
        return Ownership::Unreadable;
    };
    let value = |key: &str| pairs.iter().find(|(name, _)| name == key).map(|(_, v)| v);
    let pinned = matches!(value(PINNED_KEY), Some(FieldValue::Str(v)) if v == "true");
    if value(PROPOSAL_KEY).is_none() || pinned {
        return Ownership::Kept;
    }
    Ownership::Managed {
        stale: matches!(value(STALE_KEY), Some(FieldValue::Str(_))),
    }
}

/// Whether `text` names the skill `name` as a whole word: no letter, digit,
/// `-` or `_` on either side.
pub fn names_skill(text: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let part = |c: char| c.is_alphanumeric() || c == '-' || c == '_';
    text.match_indices(name).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        !before.is_some_and(part) && !after.is_some_and(part)
    })
}

/// One skill of the drive, as the sweep reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// Its folder name under `_skills/`.
    pub name: String,
    /// Its `SKILL.md` at the commit read.
    pub text: String,
    /// The latest commit that changed its folder, the curator's own left
    /// out; `None` when the history read did not reach one.
    pub last_change: Option<DateTime<Utc>>,
    /// Whether the disk holds a change of its folder not committed yet.
    pub uncommitted: bool,
}

/// Everything one sweep reads, at the drive's fetched commit.
pub struct Sweep<'a> {
    pub skills: &'a [Skill],
    /// Folder names already under `_skills/.archive/`.
    pub archived: &'a BTreeSet<String>,
    /// Every name an agent of the drive's `[tools].skills` lists.
    pub listed: &'a BTreeSet<String>,
    /// The text of every file under `_workflows/`.
    pub workflows: &'a [String],
    /// Why `listed` and `workflows` are not every name the drive gives,
    /// when they are not: then nothing is archived.
    pub incomplete: Option<&'a str>,
    /// Pending proposals of the served homes: `(home folder, stem, text)`.
    pub proposals: &'a [(String, String, String)],
    /// `curator@<host>`.
    pub decided_by: &'a str,
    pub now: DateTime<Utc>,
}

/// One sweep of a drive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepPlan {
    /// `SKILL.md`s whose stale mark is set or cleared (agents-zone-relative).
    pub marks: Vec<FileChange>,
    /// Skill folders moved whole under `_skills/.archive/`.
    pub archives: Vec<FileChange>,
    /// Expired gate proposals, by home folder.
    pub expired: Vec<(String, Settled)>,
    /// Marked stale, and made active again, by name.
    pub stale: Vec<String>,
    pub reactivated: Vec<String>,
    /// What the sweep left alone and why, for the host's log.
    pub notes: Vec<String>,
}

impl SweepPlan {
    /// Whether the sweep commits anything.
    pub fn commits(&self) -> bool {
        !self.marks.is_empty() || !self.archives.is_empty() || !self.expired.is_empty()
    }

    /// The commit's subject: `skills: <drive> — <n> stale, <m> archived, …`.
    pub fn subject(&self, drive: &str) -> String {
        format!(
            "skills: {drive} — {} stale, {} active again, {} archived, {} gate proposals expired",
            self.stale.len(),
            self.reactivated.len(),
            self.archives.len(),
            self.expired.len()
        )
    }
}

/// The sweep of one drive at `sweep.now`.
pub fn sweep(sweep: &Sweep<'_>) -> SweepPlan {
    let mut out = SweepPlan::default();
    let now = sweep.now.timestamp();
    let date = sweep.now.format("%Y-%m-%d").to_string();
    if let Some(why) = sweep.incomplete {
        out.notes.push(format!(
            "every skill stays as it is this week: {why}, so which skills are named is not known"
        ));
    }
    for skill in sweep.skills {
        let stale = match ownership(&skill.text) {
            Ownership::Kept => continue,
            Ownership::Unreadable => {
                out.notes.push(format!(
                    "_skills/{} stays: its metadata is not one block map of distinct keys, so \
                     whether an agent proposed it and no person pinned it is not known",
                    skill.name
                ));
                continue;
            }
            Ownership::Managed { stale } => stale,
        };
        let Some(last_change) = skill.last_change else {
            out.notes.push(format!(
                "_skills/{} stays: its last change is not in the history read",
                skill.name
            ));
            continue;
        };
        if skill.uncommitted {
            out.notes.push(format!(
                "_skills/{} stays: it holds a change not committed yet",
                skill.name
            ));
            continue;
        }
        let named = sweep.incomplete.is_some()
            || sweep.listed.contains(&skill.name)
            || sweep
                .workflows
                .iter()
                .any(|text| names_skill(text, &skill.name));
        let row = Row {
            name: skill.name.clone(),
            state: if stale { State::Stale } else { State::Active },
            pinned: false,
            last_activity_at: Some(last_change.timestamp()),
            created_at: None,
            // Never offered, never used: its clock is its last change.
            use_count: 0,
        };
        let Some(next) = curator::transition(&row, named, now, Windows::default()) else {
            continue;
        };
        let path = format!("_skills/{}/SKILL.md", skill.name);
        match next {
            State::Archived => {
                if sweep.archived.contains(&skill.name) {
                    out.notes.push(format!(
                        "_skills/{ARCHIVE_DIR}/{} exists already, so _skills/{} stays",
                        skill.name, skill.name
                    ));
                    continue;
                }
                out.archives.push(FileChange {
                    path: format!("_skills/{}", skill.name),
                    before: Some(skill.text.clone()),
                    after: After::MovedTo(format!("_skills/{ARCHIVE_DIR}/{}", skill.name)),
                });
            }
            State::Stale => {
                out.stale.push(skill.name.clone());
                out.marks.push(FileChange {
                    path,
                    before: Some(skill.text.clone()),
                    after: After::Text(set_metadata(&skill.text, STALE_KEY, Some(&date))),
                });
            }
            State::Active => {
                out.reactivated.push(skill.name.clone());
                out.marks.push(FileChange {
                    path,
                    before: Some(skill.text.clone()),
                    after: After::Text(set_metadata(&skill.text, STALE_KEY, None)),
                });
            }
        }
    }
    let due = sweep.now - Duration::days(GATE_PROPOSAL_DAYS);
    for (home, stem, text) in sweep.proposals {
        let Ok(proposal) = Proposal::parse(stem, text) else {
            continue;
        };
        if proposal.origin != Origin::Gate || proposal.created_at > due {
            continue;
        }
        out.expired.push((
            home.clone(),
            Settled {
                id: proposal.id,
                verdict: Verdict::Expired,
                reason: format!(
                    "a gate's proposal is never scored; it expired unread after {GATE_PROPOSAL_DAYS} days (R29 F4)"
                ),
                decided_by: sweep.decided_by.to_owned(),
            },
        ));
    }
    out.expired
        .sort_by_key(|(home, settled)| (home.clone(), settled.id));
    out
}

#[cfg(test)]
mod tests;
