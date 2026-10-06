//! The nightly consolidation's plan (AD-401, story 95.2): what one agent's
//! pending proposals come to tonight.
//!
//! Pure. The host reads the agent's `USER.md`, `MEMORY.md`, skills and
//! pending proposals at the drive's fetched head, hands them here, and
//! writes what [`plan`] answers through keeper-sync's one commit path.
//!
//! In order, for each proposal:
//! - **malformed** — outside the grammar, another agent's, or naming no
//!   session folder of the drive (a forged `session` cannot reach a
//!   trailer) — is `rejected`;
//! - **structural** (OpenClaw's gate, R127): a `gate` proposal is skipped,
//!   no verdict, for the curator to expire (R29 F4); `untrusted` integrity,
//!   a `scheduled` or a `delegated` origin is `rejected` unscored;
//! - **memory**: proposals with one key (target, op, folded text) are one
//!   candidate; OpenClaw's gates measure it (epic 95 Q4) — too old expires,
//!   short of a threshold stays pending, else it promotes;
//! - **who stands behind it**: a promoting candidate is applied by the night
//!   only on a private drive, with `[memory].promote` on (R131), when one of
//!   its proposals was written at `owner` or `peer` integrity (R28 S-13) and
//!   it is not a review pass's replace or remove (R127); anything else takes
//!   the review path for the drive's owner — or, for a shared drive's
//!   `USER.md`, the source sessions' requesters (R26);
//! - **skills** (epic 95 Q5, R132): validated by agentskills; an agent's own
//!   unadopted skill is applied stamped `metadata.keeper_proposal`, a
//!   person's skill or any change on a shared drive waits for a person, and
//!   is written without the key when approved.
//!
//! A file's promoted changes are one Hermes batch against its final budget;
//! a stale pin is `rejected` with Hermes' sentence; a batch over the cap
//! changes nothing and the card lists the entries; a rewrite losing more
//! than a quarter of the file's entries waits for a person (NFR-118). A
//! memory file a person left in a shape keeper would not write back makes
//! the whole night write nothing for that agent, the card quoting why.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, SecondsFormat, Utc};
use keeper_ported::hermes::memory::{
    drift_message, pinned_index, stale_entry_message, Op as HermesOp,
};
use keeper_ported::hermes::threats::{first_threat_message, Scope};
use keeper_ported::openclaw::gates::{self, Exclusion, Gate, Provenance, SessionKind, Signals};
use matrix_sdk::ruma::OwnedUserId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ulid::Ulid;

use crate::agents::approval::{
    ApprovalRecord, CallRef, Checkpoint, FilePin, Parking, Preconditions,
};
use crate::agents::label::{check_sink, Integrity, Label, Readers, Sink, SinkVerdict};
use crate::agents::memory::{MemoryFile, MemoryTarget};
use crate::agents::proposal::{self, Op, Origin, Proposal, Target};
use crate::agents::session::SessionKind as AgentSessionKind;
use crate::agents::skills::{self, SkillFilter, PROPOSAL_KEY};
use crate::agents::tier::{classify, AgentTool, CallFacts, Context};
use crate::notes::frontmatter::{FieldValue, Frontmatter};

/// When the night runs: 03:00 at the consolidating host's offset (R26).
pub const SCHEDULE: &str = "0 3 * * *";
/// Where a settled proposal goes, in its home.
pub const DONE_DIR: &str = "proposals/done";
/// The folder an archived skill moves to, under `_skills/`.
pub const ARCHIVE_DIR: &str = ".archive";
/// What a review session's folder name holds before the agent's id.
pub const REVIEW_SESSION_INFIX: &str = "memory-review";

/// The night's maintenance job: its completion is recorded under
/// [`crate::agents::claim::completion_key`]`(JOB, drive)`, its lease is the
/// drive's one maintenance claim (R129, R207).
pub const JOB: &str = "consolidate";

/// Whether a night is owed: the lease's last window is older than `window`,
/// the latest 03:00 at or before now (R129, R58). However many nights a host
/// missed, one run settles them.
pub fn night_due(last: Option<DateTime<Utc>>, window: DateTime<Utc>) -> bool {
    last.is_none_or(|last| last < window)
}

/// `Memory-Origin` of a night's commit.
pub fn origin_of_host(host: &str) -> String {
    format!("consolidator@{host}")
}

/// The id of `agent`'s review session for the night `window` of `drive`:
/// the same on every host (R66's derived ids).
pub fn review_session_id(drive: &str, agent: &str, window: DateTime<Utc>) -> Ulid {
    let digest = Sha256::digest(format!(
        "{drive}\n{agent}\n{REVIEW_SESSION_INFIX}\n{}",
        window.to_rfc3339()
    ));
    let mut wide = [0u8; 16];
    wide[6..].copy_from_slice(&digest[..10]);
    Ulid::from_parts(
        u64::try_from(window.timestamp_millis()).unwrap_or(0),
        u128::from_be_bytes(wide),
    )
}

/// The folder name of that review session: `<date>-memory-review-<agent>`.
pub fn review_session_name(agent: &str, window: DateTime<Utc>) -> String {
    format!(
        "{}-{REVIEW_SESSION_INFIX}-{agent}",
        window.format("%Y-%m-%d")
    )
}

/// SHA-256, lowercase hex: what a change pins a file's bytes by.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// What became of a proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Promoted,
    Rejected,
    Expired,
}

/// `proposals/done/<ulid>.verdict.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerdictFile {
    pub verdict: Verdict,
    pub reason: String,
    /// The drive's commit the decision was taken over: the head the night
    /// read, or the head an approved change was applied on. The verdict
    /// lands in the commit it would otherwise have to name.
    pub commit: String,
    pub decided_at: String,
    /// `consolidator@<host>`, or the deciding person's Matrix id.
    pub decided_by: String,
}

impl VerdictFile {
    pub fn render(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }

    pub fn parse(text: &str) -> Result<VerdictFile, String> {
        toml::from_str(text).map_err(|error| error.to_string())
    }
}

/// Where proposal `id` of the home `home` (zone-relative) is pending, where
/// it is moved to, and its verdict file.
pub fn proposal_paths(home: &str, id: &Ulid) -> (String, String, String) {
    (
        format!("{home}/{}/{id}.md", proposal::DIR),
        format!("{home}/{DONE_DIR}/{id}.md"),
        format!("{home}/{DONE_DIR}/{id}.verdict.toml"),
    )
}

/// One proposal's settlement in a night's commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    pub id: Ulid,
    pub verdict: Verdict,
    pub reason: String,
    pub decided_by: String,
}

/// What a file becomes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum After {
    /// These bytes.
    Text(String),
    /// The folder at the change's path moves here, whole (a skill's
    /// archive): nothing is deleted.
    MovedTo(String),
}

/// One file (or a skill's folder) a night changes, zone-relative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileChange {
    pub path: String,
    /// Its bytes at the head the night read (`None`: absent); for a folder
    /// move, its `SKILL.md`'s.
    pub before: Option<String>,
    pub after: After,
}

/// A change that waits for a person: the review path (R128).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub change: FileChange,
    pub proposals: Vec<Ulid>,
    /// The sessions it came from, drive-relative.
    pub sessions: BTreeSet<String>,
    /// Who may approve it: never the data's label (R206).
    pub approvers: BTreeSet<OwnedUserId>,
    /// Its proposals' labels joined: what the change's content may reach,
    /// carried to the record and checked again before it applies. An
    /// approval never widens it.
    pub label: Label,
    /// Why it waits, for the card.
    pub why: String,
    /// Proposed entries that do not fit the file's cap, each with its
    /// sessions: listed in the preview, not part of the change, still
    /// pending (acceptance 3).
    pub over_cap: Vec<String>,
}

/// One agent's night.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Proposals moved to `proposals/done/` with a verdict, in id order.
    pub settled: Vec<Settled>,
    /// What the night writes now.
    pub writes: Vec<FileChange>,
    /// The sessions of what it promoted: one `Source-Session` each.
    pub sources: BTreeSet<String>,
    /// What waits for a person.
    pub reviews: Vec<Review>,
    /// What the card says that nobody can approve: a file the night will
    /// not touch, a batch that does not fit.
    pub notes: Vec<String>,
    /// `gate` proposals, left pending without a verdict.
    pub skipped: Vec<Ulid>,
}

impl Plan {
    /// Whether the night commits anything for this agent.
    pub fn commits(&self) -> bool {
        !self.settled.is_empty() || !self.writes.is_empty()
    }

    /// The commit's subject: `memory: <agent> — <n> promoted, <m> rejected`.
    pub fn subject(&self, agent: &str) -> String {
        let count = |verdict| self.settled.iter().filter(|s| s.verdict == verdict).count();
        format!(
            "memory: {agent} — {} promoted, {} rejected",
            count(Verdict::Promoted),
            count(Verdict::Rejected)
        )
    }
}

/// What a proposal's `session` names, as the drive holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFacts {
    /// The person whose work the session is.
    pub requested_by: OwnedUserId,
}

/// Everything one agent's night reads, at the drive's fetched head.
pub struct Night<'a> {
    pub agent: &'a str,
    /// The agent's home, zone-relative.
    pub home: &'a str,
    /// `consolidator@<host>`.
    pub decided_by: &'a str,
    pub owner: &'a OwnedUserId,
    /// The drive's readers: who reads the home, its memory and its review
    /// sessions — every place a proposal's content reaches tonight. More
    /// than one, nothing applies without its person (R26).
    pub readers: &'a BTreeSet<OwnedUserId>,
    /// Whether the drive is `local_only` (AD-377).
    pub local_only: bool,
    /// `[memory].promote` (R131): off, every candidate takes the review path.
    pub promote: bool,
    pub user: Option<&'a str>,
    pub memory: Option<&'a str>,
    /// `_skills/<name>/SKILL.md` by name.
    pub skills: &'a BTreeMap<String, String>,
    /// The pending `proposals/<stem>.md`, by stem.
    pub proposals: &'a [(String, String)],
    /// Proposals an earlier night's review holds: not planned again.
    pub under_review: &'a BTreeSet<Ulid>,
    /// The zone-relative paths an open review changes: until it ends,
    /// nothing else is written over them or waits on them, so one person's
    /// approval is never voided by another change the night made.
    pub reviewing: &'a BTreeSet<String>,
    /// Proposals a person declined on review, and who.
    pub declined: &'a BTreeMap<Ulid, String>,
    /// The session folder a proposal names — where it is now, archived or
    /// moved — when the drive holds one.
    pub sessions: &'a dyn Fn(&str) -> Option<SessionFacts>,
    pub now: DateTime<Utc>,
}

impl Night<'_> {
    /// More than one reader: nothing applies without its person (R26).
    pub fn shared(&self) -> bool {
        self.readers.len() > 1
    }
}

/// Why content labelled `label` may not land in a home of a drive read by
/// `readers` — its memory, its skills, or a review session and its room;
/// `None` when it may (AD-391). Checked before anything is published, and
/// again before an approved change applies: an approval never declassifies.
pub fn sink_refusal(
    label: &Label,
    readers: &BTreeSet<OwnedUserId>,
    local_only: bool,
) -> Option<String> {
    if label.local_only && !local_only {
        return Some(
            "it came from a local-only drive, so it may not be written into one that is not"
                .to_owned(),
        );
    }
    match check_sink(
        label,
        &Sink::MemoryWrite {
            home_readers: Readers::Only(readers.clone()),
        },
    ) {
        SinkVerdict::Allow => None,
        SinkVerdict::Block { reason, .. } => Some(reason),
    }
}

/// Who approves a change to an agent's `USER.md` (`user_file`) or anything
/// else of its home, coming from `sessions` (R26, Q6): on a shared drive a
/// `USER.md` change is its source sessions' requesters' — when they name
/// anyone — and everything else the drive owner's.
pub fn approvers_of(
    shared: bool,
    owner: &OwnedUserId,
    user_file: bool,
    sessions: &BTreeSet<String>,
    facts: &dyn Fn(&str) -> Option<SessionFacts>,
) -> BTreeSet<OwnedUserId> {
    if shared && user_file {
        let requesters: BTreeSet<OwnedUserId> = sessions
            .iter()
            .filter_map(|session| facts(session))
            .map(|facts| facts.requested_by)
            .collect();
        if !requesters.is_empty() {
            return requesters;
        }
    }
    BTreeSet::from([owner.clone()])
}

/// Whitespace collapsed, case folded: two proposals of one fact are one
/// candidate (epic 95 Q4).
fn fold(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A candidate's key: target, op, and the folded text it changes.
fn key_of(proposal: &Proposal) -> (String, &'static str, String) {
    let pin = proposal.matched.as_deref().unwrap_or("");
    let text = match proposal.op {
        Op::Add => fold(&proposal.body),
        Op::Replace => format!("{}\u{0}{}", fold(pin), fold(&proposal.body)),
        _ => fold(pin),
    };
    (proposal.target.as_word(), proposal.op.as_word(), text)
}

/// The structural gate over keeper's origins (OpenClaw's, R127): `Ok(None)`
/// scores `proposal`, `Ok(Some(reason))` rejects it unscored, `Err(())`
/// skips a gate's.
#[allow(clippy::result_unit_err)]
pub fn structural(proposal: &Proposal) -> Result<Option<String>, ()> {
    let session = match proposal.origin {
        Origin::Gate => return Err(()),
        Origin::Foreground | Origin::Review => SessionKind::Interactive,
        Origin::Scheduled => SessionKind::Cron,
        Origin::Delegated => SessionKind::Subagent,
    };
    let provenance = match proposal.label.integrity {
        Integrity::Untrusted => Provenance::Untrusted,
        _ => Provenance::Trusted,
    };
    Ok(
        gates::excluded(provenance, session).map(|exclusion| match exclusion {
            Exclusion::Provenance(_) => "the structural gate: it was proposed at untrusted \
                                         integrity, so it is never scored (AD-401)"
                .to_owned(),
            Exclusion::Session(_) => format!(
                "the structural gate: it was proposed in a {} session, so it is never scored \
                 (AD-401)",
                proposal.origin.as_word()
            ),
        }),
    )
}

/// One candidate: proposals sharing a key, oldest first.
struct Candidate {
    proposals: Vec<Proposal>,
}

impl Candidate {
    fn first(&self) -> &Proposal {
        &self.proposals[0]
    }

    fn ids(&self) -> Vec<Ulid> {
        self.proposals.iter().map(|p| p.id).collect()
    }

    fn sessions(&self) -> BTreeSet<String> {
        self.proposals.iter().map(|p| p.session.clone()).collect()
    }

    fn signals(&self, now: DateTime<Utc>) -> Signals {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let newest = self
            .proposals
            .iter()
            .map(|p| p.created_at)
            .max()
            .unwrap_or(now);
        Signals {
            recall_count: count(self.proposals.len()),
            unique_queries: count(self.sessions().len()),
            days: count(
                self.proposals
                    .iter()
                    .map(|p| p.created_at.date_naive())
                    .collect::<BTreeSet<_>>()
                    .len(),
            ),
            age_days: (now - newest).num_seconds().max(0) as f64 / 86_400.0,
        }
    }

    /// A person, or a person-authored file, said it (R28 S-13).
    fn backed(&self) -> bool {
        self.proposals
            .iter()
            .any(|p| matches!(p.label.integrity, Integrity::Owner | Integrity::Peer))
    }

    /// Its proposals' labels joined: what its content may reach.
    fn label(&self) -> Label {
        self.proposals
            .iter()
            .fold(Label::top(), |label, p| label.join(&p.label))
    }

    /// The entry it proposes, with the sessions it came from: what a
    /// person reads of it when it does not fit.
    fn listed(&self) -> String {
        let first = self.first();
        let what = match first.op {
            Op::Remove => format!("remove “{}”", first.matched.as_deref().unwrap_or("")),
            Op::Replace => format!(
                "replace “{}” with “{}”",
                first.matched.as_deref().unwrap_or(""),
                first.body
            ),
            _ => format!("add “{}”", first.body),
        };
        let sessions: Vec<String> = self.sessions().into_iter().collect();
        format!("{what} (from {})", sessions.join(", "))
    }
}

/// The night of one agent.
pub fn plan(night: &Night<'_>) -> Plan {
    let mut out = Plan::default();
    let settle = |out: &mut Plan, id: Ulid, verdict: Verdict, reason: String| {
        out.settled.push(Settled {
            id,
            verdict,
            reason,
            decided_by: night.decided_by.to_owned(),
        });
    };

    // A file a person left in a shape keeper would not write back stops the
    // whole night for this agent: nothing it changes is worth their words.
    let mut files = BTreeMap::new();
    for (target, text) in [
        (MemoryTarget::User, night.user),
        (MemoryTarget::Memory, night.memory),
    ] {
        match MemoryFile::read(target, text) {
            Ok(file) if file.drifted() => out.notes.push(drift_message(target.file())),
            Ok(file) => {
                files.insert(target, file);
            }
            Err(problem) => out.notes.push(problem.sentence),
        }
    }
    if !out.notes.is_empty() {
        return out;
    }

    let mut scored: Vec<Proposal> = Vec::new();
    let mut stems: Vec<&(String, String)> = night.proposals.iter().collect();
    stems.sort_by(|a, b| a.0.cmp(&b.0));
    for (stem, text) in stems {
        let Ok(id) = Ulid::from_string(stem) else {
            continue;
        };
        if night.under_review.contains(&id) {
            continue;
        }
        if let Some(person) = night.declined.get(&id) {
            out.settled.push(Settled {
                id,
                verdict: Verdict::Rejected,
                reason: "a person declined it on review".to_owned(),
                decided_by: person.clone(),
            });
            continue;
        }
        let proposal = match Proposal::parse(stem, text) {
            Ok(proposal) => proposal,
            Err(sentence) => {
                settle(
                    &mut out,
                    id,
                    Verdict::Rejected,
                    format!("malformed: {sentence}"),
                );
                continue;
            }
        };
        if proposal.agent != night.agent {
            settle(
                &mut out,
                id,
                Verdict::Rejected,
                "malformed: it names another agent than the home it is in".to_owned(),
            );
            continue;
        }
        // A gate's proposal is never settled by the night, whatever else
        // is true of it: the curator expires it (acceptance 1, R29 F4).
        if proposal.origin == Origin::Gate {
            out.skipped.push(id);
            continue;
        }
        if (night.sessions)(&proposal.session).is_none() {
            settle(
                &mut out,
                id,
                Verdict::Rejected,
                "malformed: its session names no session folder of this drive".to_owned(),
            );
            continue;
        }
        match structural(&proposal) {
            Err(()) => out.skipped.push(id),
            Ok(Some(reason)) => settle(&mut out, id, Verdict::Rejected, reason),
            Ok(None) => scored.push(proposal),
        }
    }

    let (memory, skill): (Vec<Proposal>, Vec<Proposal>) = scored
        .into_iter()
        .partition(|p| matches!(p.target, Target::Memory(_)));
    plan_memory(night, &files, memory, &mut out);
    plan_skills(night, skill, &mut out);
    out.settled.sort_by_key(|s| s.id);
    out
}

/// Who approves a change to `target` from `sessions` (R26).
fn approvers(
    night: &Night<'_>,
    target: &Target,
    sessions: &BTreeSet<String>,
) -> BTreeSet<OwnedUserId> {
    approvers_of(
        night.shared(),
        night.owner,
        *target == Target::Memory(MemoryTarget::User),
        sessions,
        night.sessions,
    )
}

/// Why a promoting candidate waits for a person, when it does.
fn waits(night: &Night<'_>, candidate: &Candidate) -> Option<&'static str> {
    if night.shared() {
        Some("this drive has more than one reader, so its person decides (AD-401)")
    } else if !night.promote {
        Some("[memory] promote is off, so every change waits for a person (R131)")
    } else if candidate
        .proposals
        .iter()
        .any(|p| p.origin == Origin::Review && matches!(p.op, Op::Replace | Op::Remove))
    {
        // Any one of them: a review pass's destructive proposal never
        // supplies an automatic promotion, whoever else proposed the same.
        Some("a review pass's replace or remove always waits for a person (R127)")
    } else if !candidate.backed() {
        Some(
            "only the agent said it, never a person, and an agent's own repetition needs a \
             person (R28 S-13)",
        )
    } else {
        None
    }
}

/// What waits for a person per file and approver set: its candidates and
/// why.
type Waiting = BTreeMap<(MemoryTarget, BTreeSet<OwnedUserId>), (Vec<Candidate>, String)>;

fn plan_memory(
    night: &Night<'_>,
    files: &BTreeMap<MemoryTarget, MemoryFile>,
    proposals: Vec<Proposal>,
    out: &mut Plan,
) {
    let mut candidates: Vec<((String, &'static str, String), Candidate)> = Vec::new();
    for proposal in proposals {
        let key = key_of(&proposal);
        match candidates.iter_mut().find(|(k, _)| *k == key) {
            Some((_, candidate)) => candidate.proposals.push(proposal),
            None => candidates.push((
                key,
                Candidate {
                    proposals: vec![proposal],
                },
            )),
        }
    }
    let path_of = |target: MemoryTarget| format!("{}/{}", night.home, target.file());
    let mut now: BTreeMap<MemoryTarget, Vec<Candidate>> = BTreeMap::new();
    let mut later = Waiting::new();
    for (_, candidate) in candidates {
        let Target::Memory(target) = candidate.first().target.clone() else {
            continue;
        };
        match gates::gate(&candidate.signals(night.now)) {
            Gate::Pending => continue,
            Gate::Expired => {
                for id in candidate.ids() {
                    out.settled.push(Settled {
                        id,
                        verdict: Verdict::Expired,
                        reason: format!(
                            "its newest proposal is older than {} days",
                            gates::MAX_AGE_DAYS
                        ),
                        decided_by: night.decided_by.to_owned(),
                    });
                }
                continue;
            }
            Gate::Promote => {}
        }
        if let Some(why) = sink_refusal(&candidate.label(), night.readers, night.local_only) {
            for id in candidate.ids() {
                out.settled.push(Settled {
                    id,
                    verdict: Verdict::Rejected,
                    reason: format!(
                        "{} is read by more people than it may reach: {why}",
                        target.file()
                    ),
                    decided_by: night.decided_by.to_owned(),
                });
            }
            continue;
        }
        // A file an open review changes waits for that review to end.
        if night.reviewing.contains(&path_of(target)) {
            continue;
        }
        match waits(night, &candidate) {
            None => now.entry(target).or_default().push(candidate),
            Some(why) => {
                let approvers = approvers(night, &Target::Memory(target), &candidate.sessions());
                later
                    .entry((target, approvers))
                    .or_insert_with(|| (Vec::new(), why.to_owned()))
                    .0
                    .push(candidate);
            }
        }
    }

    // What the night applies changes the base a person's approval applies to.
    let mut bases: BTreeMap<MemoryTarget, (MemoryFile, Option<String>)> = files
        .iter()
        .map(|(target, file)| (*target, (file.clone(), original(night, *target))))
        .collect();
    // One review per file: a second, over the same base, would be voided
    // by the first one's approval; the rest waits for a later night.
    let mut reviewed: BTreeSet<MemoryTarget> = BTreeSet::new();
    for (target, candidates) in now {
        let Some(file) = files.get(&target) else {
            continue;
        };
        let Some(batched) = batch(night, file, candidates, out) else {
            continue;
        };
        let kept = file
            .entries
            .iter()
            .filter(|e| batched.entries.contains(e))
            .count();
        let after = file.render(&batched.entries);
        let change = FileChange {
            path: path_of(target),
            before: original(night, target),
            after: After::Text(after.clone()),
        };
        let sessions: BTreeSet<String> = batched
            .candidates
            .iter()
            .flat_map(Candidate::sessions)
            .collect();
        let withheld = if !batched.over_cap.is_empty() {
            Some(format!(
                "all of tonight's changes would put {} over its cap ({}); this applies the \
                 part that fits, and the rest is listed and waits",
                target.file(),
                batched.error
            ))
        } else if gates::loses_too_much(file.entries.len(), kept) {
            Some(format!(
                "it would drop {} of the {} entries of {}, more than a quarter (NFR-118)",
                file.entries.len() - kept,
                file.entries.len(),
                target.file()
            ))
        } else {
            None
        };
        if let Some(why) = withheld {
            reviewed.insert(target);
            out.reviews.push(Review {
                why,
                approvers: approvers(night, &Target::Memory(target), &sessions),
                label: joined(&batched.candidates),
                change,
                proposals: batched.candidates.iter().flat_map(Candidate::ids).collect(),
                sessions,
                over_cap: batched.over_cap,
            });
            continue;
        }
        out.sources.extend(sessions);
        for candidate in &batched.candidates {
            let reason = promoted_reason(&candidate.signals(night.now));
            for id in candidate.ids() {
                out.settled.push(Settled {
                    id,
                    verdict: Verdict::Promoted,
                    reason: reason.clone(),
                    decided_by: night.decided_by.to_owned(),
                });
            }
        }
        if change.before.as_deref() != Some(after.as_str()) {
            out.writes.push(change);
        }
        if let Ok(read) = MemoryFile::read(target, Some(&after)) {
            bases.insert(target, (read, Some(after)));
        }
    }

    for ((target, approvers), (candidates, why)) in later {
        if reviewed.contains(&target) {
            continue;
        }
        let Some((file, before)) = bases.get(&target) else {
            continue;
        };
        let Some(batched) = batch(night, file, candidates, out) else {
            continue;
        };
        reviewed.insert(target);
        let why = if batched.over_cap.is_empty() {
            why
        } else {
            format!(
                "{why}; and all of them would put {} over its cap ({}), so this is the part \
                 that fits, and the rest is listed and waits",
                target.file(),
                batched.error
            )
        };
        out.reviews.push(Review {
            change: FileChange {
                path: path_of(target),
                before: before.clone(),
                after: After::Text(file.render(&batched.entries)),
            },
            proposals: batched.candidates.iter().flat_map(Candidate::ids).collect(),
            sessions: batched
                .candidates
                .iter()
                .flat_map(Candidate::sessions)
                .collect(),
            label: joined(&batched.candidates),
            approvers,
            why,
            over_cap: batched.over_cap,
        });
    }
}

/// `candidates`' labels joined.
fn joined(candidates: &[Candidate]) -> Label {
    candidates
        .iter()
        .fold(Label::top(), |label, c| label.join(&c.label()))
}

/// The file's text at the head, as read.
fn original(night: &Night<'_>, target: MemoryTarget) -> Option<String> {
    match target {
        MemoryTarget::User => night.user,
        MemoryTarget::Memory => night.memory,
    }
    .map(str::to_owned)
}

fn promoted_reason(signals: &Signals) -> String {
    format!(
        "promoted: score {:.3}, {} proposals from {} sessions on {} days",
        gates::score(signals),
        signals.recall_count,
        signals.unique_queries,
        signals.days
    )
}

/// One file's batch: the candidates that went in and the entries after;
/// when not all of them fit, what did not, listed, and Hermes' cap error.
struct Batched {
    candidates: Vec<Candidate>,
    entries: Vec<String>,
    over_cap: Vec<String>,
    error: String,
}

/// `candidates` as one Hermes batch over `file` (acceptance 3): a stale
/// pin is rejected first with Hermes' sentence. A batch over the cap is
/// never applied: the candidates that fit, in order, become the change a
/// person may approve and the rest are listed beside it, still pending;
/// when none fits, the card lists what was proposed and the entries now,
/// and its proposals stay pending.
fn batch(
    night: &Night<'_>,
    file: &MemoryFile,
    candidates: Vec<Candidate>,
    out: &mut Plan,
) -> Option<Batched> {
    let mut fresh = Vec::new();
    for candidate in candidates {
        match &candidate.first().matched {
            Some(pin) if pinned_index(&file.entries, pin).is_none() => {
                for id in candidate.ids() {
                    out.settled.push(Settled {
                        id,
                        verdict: Verdict::Rejected,
                        reason: stale_entry_message(pin),
                        decided_by: night.decided_by.to_owned(),
                    });
                }
            }
            _ => fresh.push(candidate),
        }
    }
    if fresh.is_empty() {
        return None;
    }
    let ops: Vec<HermesOp> = fresh
        .iter()
        .filter_map(|candidate| candidate.first().hermes_op())
        .collect();
    let mut store = file.store();
    let error = match store.apply_batch(&ops) {
        Ok(_) => {
            return Some(Batched {
                candidates: fresh,
                entries: store.entries(),
                over_cap: Vec::new(),
                error: String::new(),
            })
        }
        Err(failure) => failure.error,
    };
    let (mut fits, mut fitting, mut over_cap) = (Vec::new(), Vec::new(), Vec::new());
    for candidate in fresh {
        let Some(op) = candidate.first().hermes_op() else {
            continue;
        };
        let mut trial = fitting.clone();
        trial.push(op);
        if file.store().apply_batch(&trial).is_ok() {
            fitting = trial;
            fits.push(candidate);
        } else {
            over_cap.push(candidate.listed());
        }
    }
    let mut store = file.store();
    if fits.is_empty() || store.apply_batch(&fitting).is_err() {
        let mut said = format!("{}: {error}\nProposed tonight:", file.target.file());
        for listed in &over_cap {
            said.push_str(&format!("\n- {listed}"));
        }
        said.push_str("\nIts entries now:");
        for (at, entry) in file.entries.iter().enumerate() {
            said.push_str(&format!("\n{}. {entry}", at + 1));
        }
        out.notes.push(said);
        return None;
    }
    Some(Batched {
        candidates: fits,
        entries: store.entries(),
        over_cap,
        error,
    })
}

/// `text` with `metadata.keeper_proposal` set to `id`, or removed when
/// `id` is `None`: the `metadata` block re-rendered, every other byte kept.
pub fn stamp(text: &str, id: Option<&Ulid>) -> String {
    let id = id.map(Ulid::to_string);
    skills::set_metadata(text, PROPOSAL_KEY, id.as_deref())
}

/// Whether `text` is an agent's skill no person adopted.
fn carries_key(text: &str) -> bool {
    matches!(
        Frontmatter::parse(text).0.get("metadata"),
        Some(FieldValue::Map(pairs)) if pairs.iter().any(|(key, _)| key == PROPOSAL_KEY)
    )
}

fn plan_skills(night: &Night<'_>, proposals: Vec<Proposal>, out: &mut Plan) {
    let mut taken: BTreeSet<String> = night.skills.keys().cloned().collect();
    // One change per skill a night, against the skill as read: a second
    // proposal on it waits, and is checked against what the first made.
    let mut planned: BTreeSet<String> = BTreeSet::new();
    for proposal in proposals {
        let Target::Skill(name) = &proposal.target else {
            continue;
        };
        if planned.contains(name)
            || night.reviewing.contains(&format!("_skills/{name}"))
            || night
                .reviewing
                .contains(&format!("_skills/{name}/SKILL.md"))
        {
            continue;
        }
        let current = night.skills.get(name);
        let reject = |out: &mut Plan, reason: String| {
            out.settled.push(Settled {
                id: proposal.id,
                verdict: Verdict::Rejected,
                reason,
                decided_by: night.decided_by.to_owned(),
            });
        };
        if let Some(why) = sink_refusal(&proposal.label, night.readers, night.local_only) {
            reject(
                out,
                format!("_skills/ is read by more people than it may reach: {why}"),
            );
            continue;
        }
        // Q5: the body as committed tonight is scanned, whatever was
        // scanned when it was staged.
        if proposal.op != Op::Archive {
            if let Some(blocked) = first_threat_message(&proposal.body, Scope::Strict) {
                reject(out, blocked);
                continue;
            }
        }
        let ours = match (proposal.op, current) {
            (Op::Create, _) if taken.contains(name) => {
                reject(out, format!("_skills/{name} exists already"));
                continue;
            }
            (Op::Create, _) => true,
            (_, None) => {
                reject(out, format!("_skills/{name} is not a skill in this drive"));
                continue;
            }
            (_, Some(text)) => {
                if proposal.matched.as_deref() != Some(sha256_hex(text.as_bytes()).as_str()) {
                    reject(
                        out,
                        format!("_skills/{name}/SKILL.md changed since it was proposed"),
                    );
                    continue;
                }
                carries_key(text)
            }
        };
        let applies = ours && !night.shared() && night.promote;
        let (path, after) = match proposal.op {
            Op::Archive => (
                format!("_skills/{name}"),
                After::MovedTo(format!("_skills/{ARCHIVE_DIR}/{name}")),
            ),
            _ => {
                // Applied by the night, it is stamped; approved by a person,
                // that approval is the adoption (Q5).
                let text = stamp(&proposal.body, applies.then_some(&proposal.id));
                let checked = skills::index(&[(name.clone(), text.clone())], &SkillFilter::All);
                if let Some((_, reasons)) = checked.refused.first() {
                    reject(out, format!("agentskills: {}", reasons.join("; ")));
                    continue;
                }
                (format!("_skills/{name}/SKILL.md"), After::Text(text))
            }
        };
        let change = FileChange {
            path,
            before: current.cloned(),
            after,
        };
        taken.insert(name.clone());
        planned.insert(name.clone());
        if applies {
            out.sources.insert(proposal.session.clone());
            out.settled.push(Settled {
                id: proposal.id,
                verdict: Verdict::Promoted,
                reason: format!(
                    "{} of an agent's own skill, stamped and offered once a person adopts it",
                    proposal.op.as_word()
                ),
                decided_by: night.decided_by.to_owned(),
            });
            out.writes.push(change);
            continue;
        }
        let why = if night.shared() {
            "this drive has more than one reader, so its owner adopts the skill by approving it (AD-402)".to_owned()
        } else if !night.promote {
            "[memory] promote is off, so every change waits for a person (R131)".to_owned()
        } else {
            format!("_skills/{name} is a person's skill, so an agent's change to it waits for them (AD-402)")
        };
        out.reviews.push(Review {
            change,
            proposals: vec![proposal.id],
            sessions: BTreeSet::from([proposal.session.clone()]),
            approvers: approvers(night, &proposal.target, &BTreeSet::new()),
            label: proposal.label.clone(),
            why,
            over_cap: Vec::new(),
        });
    }
}

/// An RFC 3339 instant, to the second, as a verdict states it.
pub fn stamp_time(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// The verdict file of `settled`, decided at `at` over `commit`.
pub fn verdict_file(settled: &Settled, commit: &str, at: DateTime<Utc>) -> VerdictFile {
    VerdictFile {
        verdict: settled.verdict,
        reason: settled.reason.clone(),
        commit: commit.to_owned(),
        decided_at: stamp_time(at),
        decided_by: settled.decided_by.clone(),
    }
}

/// The arguments of a `memory_apply` approval (R128): exactly the change a
/// person sees in the preview and approves, applied as it is or not at all.
/// Who may decide it and the label of what it writes are part of them, so
/// the approval's digest binds both (R206).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyArgs {
    pub v: u32,
    pub agent: String,
    /// The agent's home, zone-relative.
    pub home: String,
    pub change: FileChange,
    pub proposals: Vec<String>,
    pub sessions: Vec<String>,
    /// Who may decide it: the drive's owner, or for a shared drive's
    /// `USER.md` the source sessions' requesters ([`approvers_of`]).
    pub approvers: Vec<String>,
    /// The joined label of its proposals: what the change may reach.
    pub label: Label,
    /// The before/after artifact, session-relative, and its SHA-256
    /// (UX-DR138).
    pub preview: String,
    pub preview_sha256: String,
}

/// Why an approval's approvers no longer count.
pub const APPROVERS_MOVED: &str =
    "who may decide it is no longer who the drive's owner and its sessions make it";

impl ApplyArgs {
    pub const VERSION: u32 = 1;

    /// What a person approves for `review` of `agent` (home `home`),
    /// previewed at `preview`, whose bytes are `preview_text`.
    pub fn of(
        agent: &str,
        home: &str,
        review: &Review,
        preview: &str,
        preview_text: &str,
    ) -> ApplyArgs {
        ApplyArgs {
            v: ApplyArgs::VERSION,
            agent: agent.to_owned(),
            home: home.to_owned(),
            change: review.change.clone(),
            proposals: review.proposals.iter().map(Ulid::to_string).collect(),
            sessions: review.sessions.iter().cloned().collect(),
            approvers: review.approvers.iter().map(ToString::to_string).collect(),
            label: review.label.clone(),
            preview: preview.to_owned(),
            preview_sha256: sha256_hex(preview_text.as_bytes()),
        }
    }

    /// The proposals it settles, read back.
    pub fn proposal_ids(&self) -> Vec<Ulid> {
        self.proposals
            .iter()
            .filter_map(|id| Ulid::from_string(id).ok())
            .collect()
    }

    /// The host action that carries it out.
    pub fn tool(&self) -> AgentTool {
        if self.change.path.starts_with("_skills/") {
            AgentTool::SkillApply
        } else {
            AgentTool::MemoryApply
        }
    }

    /// Whether these are arguments the consolidator of `agent`, homed at
    /// `home`, could have written for a record of `tool`: this version, this
    /// agent and home, a change only to its `USER.md`/`MEMORY.md` or to one
    /// skill (an archive moving it whole under `.archive/`), proposals by
    /// id, and a preview of the session's own `artifacts/`.
    pub fn belongs_to(&self, agent: &str, home: &str, tool: &str) -> Result<(), String> {
        let refused = |why: &str| Err(format!("the approval is not this agent's: {why}"));
        if self.v != ApplyArgs::VERSION || self.agent != agent || self.home != home {
            return refused("it names another version, agent or home");
        }
        if self.tool().as_wire() != tool {
            return refused("its action does not change what its arguments change");
        }
        let plain = |name: &str| {
            !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\', '\n'])
        };
        let allowed = match (
            &self.change.after,
            self.change.path.strip_prefix("_skills/"),
        ) {
            (After::Text(_), None) => [MemoryTarget::User, MemoryTarget::Memory]
                .iter()
                .any(|target| self.change.path == format!("{home}/{}", target.file())),
            (After::Text(_), Some(rest)) => rest.strip_suffix("/SKILL.md").is_some_and(plain),
            (After::MovedTo(to), Some(name)) => {
                plain(name) && *to == format!("_skills/{ARCHIVE_DIR}/{name}")
            }
            (After::MovedTo(_), None) => false,
        };
        if !allowed {
            return refused("it changes a file outside the agent's memory and skills");
        }
        if self.proposals.is_empty() || self.proposal_ids().len() != self.proposals.len() {
            return refused("its proposals are not named by id");
        }
        if !self
            .preview
            .strip_prefix("artifacts/")
            .is_some_and(|name| plain(name) && name.ends_with(".md"))
        {
            return refused("its preview is not one of the session's artifacts");
        }
        Ok(())
    }

    /// The approvers these arguments bind, when they are exactly who the
    /// drive's `owner` and `readers` and the source sessions' requesters
    /// (`facts`) make them now, and every one of them may read what it
    /// changes; else why not (R206: checked on adoption and on decision).
    pub fn approvers_now(
        &self,
        owner: &OwnedUserId,
        readers: &BTreeSet<OwnedUserId>,
        facts: &dyn Fn(&str) -> Option<SessionFacts>,
    ) -> Result<BTreeSet<OwnedUserId>, String> {
        let bound: BTreeSet<OwnedUserId> = self
            .approvers
            .iter()
            .map(|user| OwnedUserId::try_from(user.as_str()))
            .collect::<Result<_, _>>()
            .map_err(|_| APPROVERS_MOVED.to_owned())?;
        let user_file = self.change.path == format!("{}/{}", self.home, MemoryTarget::User.file());
        let sessions: BTreeSet<String> = self.sessions.iter().cloned().collect();
        let now = approvers_of(readers.len() > 1, owner, user_file, &sessions, facts);
        if bound.is_empty() || bound != now || !self.label.may_reach(&Readers::Only(bound.clone()))
        {
            return Err(APPROVERS_MOVED.to_owned());
        }
        Ok(bound)
    }
}

/// The T2 record `args` waits on in its review session (R128): the
/// consolidator's action `memory_apply` (`skill_apply` for a skill),
/// classified by the central table in its scheduled session — fixed T2,
/// nothing raises it — bound to `args` (its approvers and label with it),
/// labelled with the data's label, and pinned to the target's SHA-256 as
/// the night read it (R79). It parks no model call: the call it names is
/// the action's own, and its checkpoint is empty.
pub fn review_record(
    id: &Ulid,
    at: DateTime<Utc>,
    session: &str,
    drive: &str,
    host: &str,
    args: &ApplyArgs,
    pin: FilePin,
) -> Result<ApprovalRecord, String> {
    let tool = args.tool();
    let value = serde_json::to_value(args).map_err(|error| error.to_string())?;
    let classification = classify(
        tool,
        &CallFacts::default(),
        &Context {
            delegated: false,
            unattended: true,
            integrity: args.label.integrity,
            via_kvm: false,
            grant: None,
        },
    );
    let id_text = id.to_string();
    let mut record = ApprovalRecord::new(Parking {
        id: &id_text,
        created_at: at,
        session,
        session_kind: AgentSessionKind::Scheduled,
        agent: &args.agent,
        drive,
        host,
        epoch: 0,
        call: CallRef {
            line: id_text.clone(),
            call_id: format!("{}-{id_text}", tool.as_wire()),
        },
        dispatch_chain: args.approvers.clone(),
        checkpoint: Checkpoint {
            chunk: String::new(),
            through: String::new(),
            sha256: String::new(),
        },
        args: &value,
        exec_binding: serde_json::Value::Null,
        classification: &classification,
        label: &args.label,
        preconditions: Preconditions {
            files: vec![pin],
            ..Preconditions::default()
        },
    })
    .map_err(|float| float.to_string())?;
    record.action.preview = Some(serde_json::json!({
        "path": args.preview,
        "sha256": args.preview_sha256,
    }));
    Ok(record)
}

/// Whether `record` is one of the consolidator's actions, which park no
/// model call.
pub fn is_host_action(record: &ApprovalRecord) -> bool {
    [AgentTool::MemoryApply, AgentTool::SkillApply]
        .iter()
        .any(|tool| tool.as_wire() == record.action.tool)
}

/// The before/after artifact of one review (UX-DR138): what changes, why
/// it waits, and both texts whole.
pub fn preview(agent: &str, review: &Review) -> String {
    let fenced = |text: Option<&str>| match text {
        Some(text) => format!("````text\n{}\n````\n", text.trim_end_matches('\n')),
        None => "(absent)\n".to_owned(),
    };
    let after = match &review.change.after {
        After::Text(text) => fenced(Some(text)),
        After::MovedTo(to) => format!("Moved, whole, to `{to}`; nothing is deleted.\n"),
    };
    let sessions: Vec<&str> = review.sessions.iter().map(String::as_str).collect();
    let mut out = format!(
        "# {agent}: `{}`\n\nWhy it waits: {}.\n\nFrom: {}\n\n## Before\n\n{}\n## After\n\n{}",
        review.change.path,
        review.why,
        sessions.join(", "),
        fenced(review.change.before.as_deref()),
        after
    );
    if !review.over_cap.is_empty() {
        out.push_str("\n## Proposed, over the cap\n\nNot part of this change; still pending.\n\n");
        for listed in &review.over_cap {
            out.push_str(&format!("- {listed}\n"));
        }
    }
    out
}

/// The review session's card: what waits, and what nobody can approve.
pub fn review_card(agent: &str, previews: &[String], notes: &[String]) -> String {
    let mut out = format!("# Memory review: {agent}\n\n");
    if !previews.is_empty() {
        out.push_str(
            "Tonight's consolidation found changes only a person may make. Each waits for an \
             approval: approving applies exactly the change its preview shows, declining \
             rejects its proposals.\n\n",
        );
        for preview in previews {
            out.push_str(&format!("- [{preview}]({preview})\n"));
        }
        out.push('\n');
    }
    if !notes.is_empty() {
        out.push_str("Nothing below can be approved; keeper changed nothing because of it.\n\n");
        for note in notes {
            out.push_str(&format!("> {}\n\n", note.replace('\n', "\n> ")));
        }
    }
    out
}

#[cfg(test)]
mod tests;
