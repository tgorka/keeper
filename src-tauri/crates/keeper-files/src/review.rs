//! Proposals, decisions, acknowledgements, reversals and history against a
//! domain's files server.
//!
//! Every mutation is planned against the local replica, uploaded when it
//! carries new file bytes, and sent through [`Files::deliver`]; queries page
//! through [`Files::read`], while a proposer's own revisions are also readable
//! directly from the replica's `files_proposals` table.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use keeper_files_api::history::{HistoryFilter, HistoryPage, HistoryParams, LIMIT_MAX};
use keeper_files_api::ids::EMPTY_CONTENT_ID;
use keeper_files_api::refusal::Code;
use keeper_files_api::reversal::Preview;
use keeper_files_api::review::{
    DecideItem, DecideReceipt, ProposalsPage, ProposedDetail, UnreviewedPage, PAGE_MAX,
};
use keeper_files_api::rows::{Kind, Outcome, Proposal};
use keeper_files_api::wire::{Decision, ProposedChange, Reviewers, WireOp};
use keeper_ports::transfer::DownloadReference;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::command::{code_of, refusal, unreachable, Delivered};
use crate::put::{mtime_ms, place};
use crate::queue::PendingOp;
use crate::{Files, FilesError, Target};

/// A change a caller asks to apply directly or submit as a proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Create a new file or replace an existing file's bytes from `file`.
    Put { file: PathBuf, target: Target },
    /// Move or rename `target` to `to`.
    Move { target: Target, to: Target },
    /// Delete `target`.
    Delete { target: Target },
}

/// A command the server accepted and committed to the domain's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed {
    pub request_id: String,
    pub event_id: Option<String>,
    pub seq: Option<String>,
}

/// A proposal revision the server recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposed {
    pub request_id: String,
    pub detail: ProposedDetail,
    pub operations: Vec<WireOp>,
}

/// One file body opened from a proposal revision into the device's cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub entry_id: String,
    pub content_id: String,
    pub size: u64,
    pub file: PathBuf,
    pub fetched: bool,
}

#[derive(Deserialize)]
struct ProposalAnswer {
    proposals: Vec<Proposal>,
}

const PROPOSAL_JSON_COLS: [&str; 4] = ["change", "reviewers", "origin_chain", "source_labels"];
const PROPOSAL_INT_COLS: [&str; 4] = [
    "expires_at_ms",
    "created_at_ms",
    "decided_at_ms",
    "job_ordinal",
];

fn proposal_of(row: &Map<String, Value>) -> Result<Proposal, FilesError> {
    let mut fields = row.clone();
    fields.remove("author_actor");
    fields.remove("author_client");
    for col in PROPOSAL_JSON_COLS {
        if let Some(Value::String(text)) = fields.get(col) {
            let parsed: Value = serde_json::from_str(text)
                .map_err(|error| FilesError::Row(format!("proposal {col}: {error}")))?;
            fields.insert(col.to_owned(), parsed);
        }
    }
    for col in PROPOSAL_INT_COLS {
        if let Some(Value::String(text)) = fields.get(col) {
            let number: i64 = text
                .parse()
                .map_err(|error| FilesError::Row(format!("proposal {col}: {error}")))?;
            fields.insert(col.to_owned(), Value::from(number));
        }
    }
    serde_json::from_value(Value::Object(fields))
        .map_err(|error| FilesError::Row(format!("proposal row: {error}")))
}

impl Files {
    /// Plan `change` against the replica into one [`WireOp`]; a `Put` uploads
    /// its bytes to the store first.
    pub fn operation(&self, change: &Change) -> Result<WireOp, FilesError> {
        match change {
            Change::Put { file, target } => {
                let (_, op) = self.plan(target)?;
                let transfer = self
                    .transfer
                    .as_ref()
                    .ok_or_else(|| FilesError::Offline("no transfer for upload".to_owned()))?;
                let uploaded = match transfer.upload_file(
                    file,
                    Some(keeper_files_api::names::media_type(place(&op).1)),
                    op.upload_target(),
                    keeper_ports::transfer::UploadUse::Proposal,
                    &mut |_| {},
                ) {
                    Ok(uploaded) => uploaded,
                    Err(error) if matches!(&error, keeper_ports::ReplicaError::Remote { code, .. } if code == keeper_ports::transfer::PLACEMENT_PENDING) => {
                        return Err(error.into())
                    }
                    Err(error) if unreachable(&error) => {
                        return Err(FilesError::Offline(error.to_string()));
                    }
                    Err(error) => return Err(error.into()),
                };
                let mtime = mtime_ms(file);
                match op {
                    PendingOp::Create {
                        entry_id,
                        parent_id,
                        name,
                    } => Ok(WireOp::Create {
                        id: entry_id,
                        kind: Kind::File,
                        parent_id: Some(parent_id),
                        name,
                        content_id: Some(uploaded.content_id),
                        size: Some(uploaded.size),
                        mtime_ms: mtime,
                        mode: None,
                        git_mode: None,
                        provenance: None,
                    }),
                    PendingOp::Replace {
                        entry_id,
                        expected_revision,
                        ..
                    } => Ok(WireOp::Replace {
                        id: entry_id,
                        expected_revision,
                        content_id: uploaded.content_id,
                        size: uploaded.size,
                        mtime_ms: mtime,
                        mode: None,
                        git_mode: None,
                        provenance: None,
                    }),
                }
            }
            Change::Move { target, to } => {
                let entry = self.resolve(target)?;
                let Target::Path { drive, path } = to else {
                    return Err(FilesError::NameRefused(
                        "move destination must be a path".to_owned(),
                    ));
                };
                let segments = keeper_paths::plain_segments(path)
                    .map_err(|refusal| FilesError::PathRefused(format!("{refusal:?}")))?;
                let Some(leaf) = segments.last().and_then(|leaf| leaf.to_str()) else {
                    return Err(FilesError::PathRefused(path.clone()));
                };
                keeper_files_api::names::admit_name(leaf)
                    .map_err(|_| FilesError::NameRefused(leaf.to_owned()))?;
                let parent_path = path.rsplit_once('/').map_or("", |(parent, _)| parent);
                let parent = self.resolve_path(drive, parent_path)?;
                if parent.kind != Kind::Dir {
                    return Err(FilesError::NotAFolder(parent.id));
                }
                Ok(WireOp::Move {
                    id: entry.id,
                    expected_revision: entry.revision,
                    parent_id: parent.id,
                    name: leaf.to_owned(),
                    provenance: None,
                })
            }
            Change::Delete { target } => {
                let entry = self.resolve(target)?;
                Ok(WireOp::Delete {
                    id: entry.id,
                    expected_revision: entry.revision,
                    provenance: None,
                })
            }
        }
    }

    /// Apply a `Move` or `Delete` directly through `files.move` / `files.delete`.
    /// `Put` must go through [`Files::put`].
    pub fn change_now(
        &self,
        change: &Change,
        change_set_id: Option<&str>,
    ) -> Result<Committed, FilesError> {
        let descriptor = match change {
            Change::Put { .. } => {
                return Err(FilesError::Storage(
                    "put goes through Files::put".to_owned(),
                ));
            }
            Change::Move { .. } => "files.move",
            Change::Delete { .. } => "files.delete",
        };
        let op = self.operation(change)?;
        let mut payload = json!({ "operation": op });
        if let (Some(change_set_id), Some(map)) = (change_set_id, payload.as_object_mut()) {
            map.insert("change_set_id".to_owned(), json!(change_set_id));
        }
        let request_id = ulid::Ulid::new().to_string();
        self.expect_committed(descriptor, request_id, payload)
    }

    /// Submit `change` as a new proposal through `files.propose`.
    pub fn propose(
        &self,
        change: &Change,
        reviewers: &Reviewers,
        expires_at_ms: Option<i64>,
        change_set_id: Option<&str>,
    ) -> Result<Proposed, FilesError> {
        let op = self.operation(change)?;
        let operations = vec![op];
        let proposal_id = ulid::Ulid::new().to_string();
        let mut payload = json!({
            "proposal_id": proposal_id,
            "base_revision": null,
            "change": { "operations": &operations },
            "reviewers": reviewers,
            "expires_at_ms": expires_at_ms,
        });
        if let (Some(change_set_id), Some(map)) = (change_set_id, payload.as_object_mut()) {
            map.insert("change_set_id".to_owned(), json!(change_set_id));
        }
        let request_id = ulid::Ulid::new().to_string();
        match self.deliver("files.propose", &request_id, payload)? {
            Delivered::Unreached => Err(FilesError::Offline(
                "files.propose could not reach server".to_owned(),
            )),
            Delivered::Unseen => Err(FilesError::Unanswered(request_id)),
            Delivered::Receipt(row) => match row.outcome {
                Outcome::Proposed => {
                    let detail_value = row.detail.ok_or_else(|| {
                        FilesError::Row("files.propose receipt missing detail".to_owned())
                    })?;
                    let detail: ProposedDetail =
                        serde_json::from_value(detail_value).map_err(|error| {
                            FilesError::Row(format!("files.propose detail: {error}"))
                        })?;
                    Ok(Proposed {
                        request_id,
                        detail,
                        operations,
                    })
                }
                Outcome::Conflict => Err(FilesError::Refused {
                    code: Code::Conflict,
                    detail: row.detail,
                }),
                Outcome::Rejected => Err(refusal(row.reason.as_deref(), row.detail)),
                Outcome::Accepted => Err(FilesError::Row(
                    "files.propose receipt outcome accepted".to_owned(),
                )),
            },
        }
    }

    /// This actor's own proposal revisions from the local replica, ordered by id.
    pub fn own_proposals(&self) -> Result<Vec<Proposal>, FilesError> {
        let rows = self.source.replica.query(
            "SELECT * FROM files_proposals WHERE proposer_actor = ? ORDER BY id",
            &[json!(self.source.actor)],
        )?;
        rows.iter().map(proposal_of).collect()
    }

    /// Every page of open, unexpired proposal revisions addressed to this caller.
    pub fn proposals(&self, change_set_id: Option<&str>) -> Result<Vec<Proposal>, FilesError> {
        let mut all = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let mut params = json!({
                "version": 1,
                "limit": PAGE_MAX,
            });
            if let Some(map) = params.as_object_mut() {
                if let Some(cursor) = &after {
                    map.insert("after".to_owned(), json!(cursor));
                }
                if let Some(change_set_id) = change_set_id {
                    map.insert("change_set_id".to_owned(), json!(change_set_id));
                }
            }
            let value = self.read("files.proposals", params)?;
            let page: ProposalsPage = serde_json::from_value(value)
                .map_err(|error| FilesError::Row(format!("files.proposals: {error}")))?;
            all.extend(page.proposals);
            match page.next {
                Some(next) if after.as_deref() != Some(next.as_str()) => after = Some(next),
                _ => break,
            }
        }
        Ok(all)
    }

    /// Every revision of `proposal_id` visible to this caller.
    pub fn proposal(&self, proposal_id: &str) -> Result<Vec<Proposal>, FilesError> {
        let value = self.read(
            "files.proposal",
            json!({
                "version": 1,
                "proposal_id": proposal_id,
            }),
        )?;
        let answer: ProposalAnswer = serde_json::from_value(value)
            .map_err(|error| FilesError::Row(format!("files.proposal: {error}")))?;
        Ok(answer.proposals)
    }

    /// Ensure every file body referenced by `revision`'s create/replace
    /// operations is present in the device's cache.
    pub fn open_revision(&self, revision: &Proposal) -> Result<Vec<Opened>, FilesError> {
        let change: ProposedChange = serde_json::from_value(revision.change.clone())
            .map_err(|error| FilesError::Row(format!("proposal change: {error}")))?;
        let mut opened = Vec::new();
        for op in change.operations {
            let (entry_id, content_id, size) = match op {
                WireOp::Create {
                    id,
                    kind: Kind::File,
                    content_id: Some(content_id),
                    size: Some(size),
                    ..
                }
                | WireOp::Replace {
                    id,
                    content_id,
                    size,
                    ..
                } => (id, content_id, size),
                _ => continue,
            };
            if let Some(file) = self.cache.valid(&content_id, size)? {
                opened.push(Opened {
                    entry_id,
                    content_id,
                    size,
                    file,
                    fetched: false,
                });
                continue;
            }
            if content_id == EMPTY_CONTENT_ID {
                let file = self.cache.store_empty()?;
                opened.push(Opened {
                    entry_id,
                    content_id,
                    size,
                    file,
                    fetched: false,
                });
                continue;
            }
            let transfer = self
                .transfer
                .as_ref()
                .ok_or_else(|| FilesError::Offline(format!("{content_id} is not cached")))?;
            let file = self.cache.path(&content_id)?;
            let reference = DownloadReference::Proposal {
                proposal_id: &revision.proposal_id,
                proposal_revision: &revision.revision,
            };
            match transfer.download_file(&content_id, reference, size, &file, &mut |_| {}) {
                Ok(_) => {}
                Err(error) if unreachable(&error) => {
                    return Err(FilesError::Offline(error.to_string()));
                }
                Err(error) => return Err(error.into()),
            }
            opened.push(Opened {
                entry_id,
                content_id,
                size,
                file,
                fetched: true,
            });
        }
        Ok(opened)
    }

    /// Submit one or more proposal decisions through `files.decide`.
    pub fn decide(&self, decisions: Vec<Decision>) -> Result<DecideReceipt, FilesError> {
        let request_id = ulid::Ulid::new().to_string();
        match self.deliver(
            "files.decide",
            &request_id,
            json!({ "decisions": decisions }),
        )? {
            Delivered::Unreached => Err(FilesError::Offline(
                "files.decide could not reach server".to_owned(),
            )),
            Delivered::Unseen => Err(FilesError::Unanswered(request_id)),
            Delivered::Receipt(row) => {
                if let Some(detail) = row.detail.as_ref() {
                    if let Ok(items) = serde_json::from_value::<Vec<DecideItem>>(detail.clone()) {
                        if !items.is_empty() {
                            return Ok(DecideReceipt {
                                outcome: row.outcome,
                                code: row.reason.as_deref().and_then(code_of),
                                event_id: row.event_id,
                                seq: row.seq,
                                detail: items,
                            });
                        }
                    }
                }
                match row.outcome {
                    Outcome::Conflict => Err(FilesError::Refused {
                        code: Code::Conflict,
                        detail: row.detail,
                    }),
                    Outcome::Rejected => Err(refusal(row.reason.as_deref(), row.detail)),
                    Outcome::Accepted | Outcome::Proposed => Err(FilesError::Row(
                        "files.decide receipt missing items".to_owned(),
                    )),
                }
            }
        }
    }

    /// One page of unreviewed non-human events from `files.unreviewed`.
    pub fn unreviewed(
        &self,
        after: &str,
        limit: u32,
        change_set_id: Option<&str>,
    ) -> Result<UnreviewedPage, FilesError> {
        let mut params = json!({
            "version": 1,
            "after": after,
            "limit": limit,
        });
        if let (Some(change_set_id), Some(map)) = (change_set_id, params.as_object_mut()) {
            map.insert("change_set_id".to_owned(), json!(change_set_id));
        }
        let value = self.read("files.unreviewed", params)?;
        serde_json::from_value(value)
            .map_err(|error| FilesError::Row(format!("files.unreviewed: {error}")))
    }

    /// Acknowledge `event_ids` through `files.acknowledge`, resolving each
    /// event's `delta_digest` from `files.unreviewed` and then `files.history`.
    pub fn acknowledge(&self, event_ids: &[String]) -> Result<Committed, FilesError> {
        let wanted: HashSet<&str> = event_ids.iter().map(String::as_str).collect();
        let mut digests: HashMap<String, String> = HashMap::new();
        if !wanted.is_empty() {
            let mut cursor = "0".to_owned();
            loop {
                let page = self.unreviewed(&cursor, PAGE_MAX, None)?;
                let count = page.events.len();
                for item in page.events {
                    if wanted.contains(item.event.id.as_str()) {
                        digests.insert(item.event.id, item.event.delta_digest);
                    }
                }
                if digests.len() == wanted.len()
                    || count < PAGE_MAX as usize
                    || page.scanned_through == cursor
                {
                    break;
                }
                cursor = page.scanned_through;
            }
            if digests.len() < wanted.len() {
                let mut cursor = "0".to_owned();
                loop {
                    let page = self.history(&HistoryParams {
                        version: 1,
                        after: cursor.clone(),
                        limit: LIMIT_MAX,
                        filter: HistoryFilter::default(),
                    })?;
                    let count = page.events.len();
                    for item in page.events {
                        if wanted.contains(item.event.id.as_str()) {
                            digests.insert(item.event.id, item.event.delta_digest);
                        }
                    }
                    if digests.len() == wanted.len()
                        || count < LIMIT_MAX as usize
                        || page.scanned_through == cursor
                    {
                        break;
                    }
                    cursor = page.scanned_through;
                }
            }
        }
        let targets: Vec<Value> = event_ids
            .iter()
            .map(|id| {
                let digest = digests.get(id).map_or("", String::as_str);
                json!({
                    "target_event_id": id,
                    "target_delta_digest": digest,
                })
            })
            .collect();
        let request_id = ulid::Ulid::new().to_string();
        self.expect_committed(
            "files.acknowledge",
            request_id,
            json!({ "targets": targets }),
        )
    }

    /// Ask the server for the planned inverse of `event_id` without writing.
    pub fn preview_reversal(&self, event_id: &str) -> Result<Preview, FilesError> {
        let value = self.read(
            "files.preview_reversal",
            json!({
                "version": 1,
                "event_id": event_id,
            }),
        )?;
        serde_json::from_value(value)
            .map_err(|error| FilesError::Row(format!("files.preview_reversal: {error}")))
    }

    /// Reverse `event_id` through `files.reverse`.
    pub fn reverse(&self, event_id: &str) -> Result<Committed, FilesError> {
        let request_id = ulid::Ulid::new().to_string();
        self.expect_committed("files.reverse", request_id, json!({ "event_id": event_id }))
    }

    /// One page of `files.history`.
    pub fn history(&self, params: &HistoryParams) -> Result<HistoryPage, FilesError> {
        let params = serde_json::to_value(params)
            .map_err(|error| FilesError::Row(format!("files.history params: {error}")))?;
        let value = self.read("files.history", params)?;
        serde_json::from_value(value)
            .map_err(|error| FilesError::Row(format!("files.history: {error}")))
    }

    /// Every event after `after` matching `filter` whose `change_set_id` is
    /// `change_set_id` (`files.history` has no change-set filter, so its pages
    /// are walked to the head); `scanned_through` is the last page's.
    pub fn change_set_history(
        &self,
        after: &str,
        filter: &HistoryFilter,
        change_set_id: &str,
    ) -> Result<HistoryPage, FilesError> {
        let mut events = Vec::new();
        let mut cursor = after.to_owned();
        loop {
            let page = self.history(&HistoryParams {
                version: 1,
                after: cursor.clone(),
                limit: LIMIT_MAX,
                filter: filter.clone(),
            })?;
            let count = page.events.len();
            events.extend(
                page.events
                    .into_iter()
                    .filter(|item| item.event.change_set_id.as_deref() == Some(change_set_id)),
            );
            if count < LIMIT_MAX as usize || page.scanned_through == cursor {
                return Ok(HistoryPage {
                    events,
                    scanned_through: page.scanned_through,
                });
            }
            cursor = page.scanned_through;
        }
    }

    fn expect_committed(
        &self,
        descriptor: &str,
        request_id: String,
        payload: Value,
    ) -> Result<Committed, FilesError> {
        match self.deliver(descriptor, &request_id, payload)? {
            Delivered::Unreached => Err(FilesError::Offline(format!(
                "{descriptor} could not reach server"
            ))),
            Delivered::Unseen => Err(FilesError::Unanswered(request_id)),
            Delivered::Receipt(row) => match row.outcome {
                Outcome::Accepted => Ok(Committed {
                    request_id,
                    event_id: row.event_id,
                    seq: row.seq,
                }),
                Outcome::Conflict => Err(FilesError::Refused {
                    code: Code::Conflict,
                    detail: row.detail,
                }),
                Outcome::Rejected => Err(refusal(row.reason.as_deref(), row.detail)),
                Outcome::Proposed => Err(FilesError::Row(format!(
                    "{descriptor} receipt outcome proposed"
                ))),
            },
        }
    }
}
