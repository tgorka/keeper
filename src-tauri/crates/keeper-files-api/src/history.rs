//! `files.history`: an entry's, an actor's or an origin's events, paged by
//! sequence (`syncular/README.md`, *The drive*, *History*; story 116.3). The
//! server reads it through indexes, never by scanning the sequence and
//! filtering; [`crate::rules::history`] is the reference answer.

use serde::{Deserialize, Serialize};

use crate::rows::{Event, Op, OriginClass};

/// The most events one page holds.
pub const LIMIT_MAX: u32 = 100;

/// Which events a page holds: every given field must match (an absent field
/// matches every event). `entity_id` matches an event with any operation on
/// that entity, so an entry's history follows it across renames and moves.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryFilter {
    #[serde(default)]
    pub entity_id: Option<String>,
    #[serde(default)]
    pub actor_id: Option<String>,
    #[serde(default)]
    pub origin_class: Option<OriginClass>,
}

/// `files.history`'s params: events with `seq` above `after` (decimal, `"0"`
/// first), at most `limit` (1–[`LIMIT_MAX`]) of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryParams {
    pub version: u32,
    pub after: String,
    pub limit: u32,
    #[serde(default)]
    pub filter: HistoryFilter,
}

/// One event of a page: its header and its operations, ascending by
/// ordinal. Under an `entity_id` filter, only that entity's operations
/// (the header's `op_count` still counts all of them).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryItem {
    pub event: Event,
    pub ops: Vec<Op>,
}

/// One page, ascending by `seq`. Every matching event in
/// `(after, scanned_through]` is in `events`; a full page ends at its last
/// event, a short one at the newer of `after` and the head's `seq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryPage {
    pub events: Vec<HistoryItem>,
    pub scanned_through: String,
}
