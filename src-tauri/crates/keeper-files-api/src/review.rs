//! Proposals, decisions and reviews (story 116.6; `syncular/README.md`,
//! *The drive*, *Proposals, decisions and reviews*): the receipts' details
//! and the reads `files.proposal`, `files.proposals` and `files.unreviewed`.
//! [`crate::rules::propose`], [`crate::rules::decide`],
//! [`crate::rules::acknowledge`] and the reads beside them are the reference
//! answers.

use serde::{Deserialize, Serialize};

use crate::history::HistoryItem;
use crate::refusal::Code;
use crate::rows::{Outcome, Proposal, ProposalStatus};

/// The most revisions or events one read page holds.
pub const PAGE_MAX: u32 = 100;

/// A `proposed` receipt's `detail` for `files.propose`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedDetail {
    pub proposal_id: String,
    pub proposal_revision: String,
    pub proposal_digest: String,
}

/// One decision's result, in the list's order: `accepted` with the
/// approval's event id and sequence; `proposed` with the revision's new
/// `status` (a rejection); or `rejected` with its code (`forbidden`,
/// `proposal_stale`, …), the revision's new `status` when the refusal moved
/// it (`rejected` or `expired`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecideItem {
    pub proposal_id: String,
    pub proposal_revision: String,
    pub outcome: Outcome,
    pub event_id: Option<String>,
    pub seq: Option<String>,
    pub status: Option<ProposalStatus>,
    pub code: Option<Code>,
}

/// `files.decide`'s one receipt. `outcome`: a one-item list answers its
/// item's outcome and code; a longer list `accepted` when any item was
/// approved, else `proposed` when any was rejected by decision, else
/// `rejected` with the first item's code. `event_id` and `seq` are set only
/// for a one-item list whose item was approved; `detail` holds every item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecideReceipt {
    pub outcome: Outcome,
    pub code: Option<Code>,
    pub event_id: Option<String>,
    pub seq: Option<String>,
    pub detail: Vec<DecideItem>,
}

/// `files.proposals`' params: the open, unexpired revisions addressed to the
/// caller (by name, or to the role the caller holds now), ascending by row
/// id after `after`, at most `limit` (1–[`PAGE_MAX`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalsParams {
    pub version: u32,
    #[serde(default)]
    pub after: Option<String>,
    pub limit: u32,
    #[serde(default)]
    pub change_set_id: Option<String>,
}

/// One page of `files.proposals`; `next` is the last row id of a full page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalsPage {
    pub proposals: Vec<Proposal>,
    pub next: Option<String>,
}

/// `files.proposal`'s params: one proposal's revisions, as the caller may
/// read them (its proposer all of them; a reviewer those addressed to them;
/// anyone else none).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalParams {
    pub version: u32,
    pub proposal_id: String,
}

/// `files.unreviewed`'s params: agent-, tool- and job-origin events no
/// review targets and no event reverses, with `seq` above `after`, at most
/// `limit` (1–[`PAGE_MAX`]); `reviewer` or `owner` only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnreviewedParams {
    pub version: u32,
    pub after: String,
    pub limit: u32,
    #[serde(default)]
    pub change_set_id: Option<String>,
}

/// One page of `files.unreviewed`, paged as `files.history`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnreviewedPage {
    pub events: Vec<HistoryItem>,
    pub scanned_through: String,
}
