//! Answer builders for proposals, decisions, acknowledgements, reversals,
//! direct move/delete and history verbs under `keeper-cli files`.

use std::path::Path;

use keeper_files::{Change, Committed, Files, Proposed, Target};
use keeper_files_api::history::{HistoryFilter, HistoryParams};
use keeper_files_api::review::PAGE_MAX;
use keeper_files_api::rows::{OriginClass, Outcome, Proposal};
use keeper_files_api::wire::{Decision, ReviewerRole, Reviewers, RoleReviewers, Verdict};
use serde_json::{json, Value};

use super::{failure, files_failure, ProposeFlags};

fn reviewers_of(flags: &ProposeFlags) -> Result<Reviewers, Value> {
    if flags.reviewers_role {
        if !flags.reviewer.is_empty() {
            return Err(failure(
                "usage",
                "pass either --reviewer <actor> or --reviewers-role, not both",
            ));
        }
        return Ok(Reviewers::Role(RoleReviewers {
            role: ReviewerRole::Reviewer,
        }));
    }
    if flags.reviewer.is_empty() {
        return Err(failure(
            "usage",
            "proposing requires --reviewer <actor> or --reviewers-role",
        ));
    }
    let mut actors = flags.reviewer.clone();
    actors.sort();
    actors.dedup();
    Ok(Reviewers::Actors(actors))
}

fn proposed_answer(proposed: &Proposed) -> Value {
    json!({
        "outcome": "proposed",
        "request_id": proposed.request_id,
        "proposal_id": proposed.detail.proposal_id,
        "proposal_revision": proposed.detail.proposal_revision,
        "proposal_digest": proposed.detail.proposal_digest,
        "operations": proposed.operations,
    })
}

fn committed_answer(committed: &Committed) -> Value {
    json!({
        "outcome": "accepted",
        "request_id": committed.request_id,
        "event_id": committed.event_id,
        "seq": committed.seq,
    })
}

fn latest_revision(revisions: &[Proposal]) -> Option<&Proposal> {
    revisions
        .iter()
        .max_by_key(|rev| rev.revision.parse::<u64>().unwrap_or(0))
}

fn parse_origin(value: &str) -> Result<OriginClass, Value> {
    match value {
        "human" => Ok(OriginClass::Human),
        "agent" => Ok(OriginClass::Agent),
        "tool" => Ok(OriginClass::Tool),
        "import" => Ok(OriginClass::Import),
        "job" => Ok(OriginClass::Job),
        _ => Err(failure(
            "usage",
            &format!("unknown --origin {value:?}; expected human, agent, tool, import or job"),
        )),
    }
}

pub(crate) fn propose_put(
    files: &Files,
    file: &Path,
    target: &Target,
    propose: &ProposeFlags,
) -> Result<Value, Value> {
    let reviewers = reviewers_of(propose)?;
    let change = Change::Put {
        file: file.to_path_buf(),
        target: target.clone(),
    };
    let proposed = files
        .propose(
            &change,
            &reviewers,
            propose.expires_at_ms,
            propose.change_set.as_deref(),
        )
        .map_err(|e| files_failure(&e))?;
    Ok(proposed_answer(&proposed))
}

pub(crate) fn move_entry(
    files: &Files,
    target: &Target,
    to: &Target,
    propose: &ProposeFlags,
) -> Result<Value, Value> {
    let change = Change::Move {
        target: target.clone(),
        to: to.clone(),
    };
    if propose.propose {
        let reviewers = reviewers_of(propose)?;
        let proposed = files
            .propose(
                &change,
                &reviewers,
                propose.expires_at_ms,
                propose.change_set.as_deref(),
            )
            .map_err(|e| files_failure(&e))?;
        Ok(proposed_answer(&proposed))
    } else {
        let committed = files
            .change_now(&change, propose.change_set.as_deref())
            .map_err(|e| files_failure(&e))?;
        Ok(committed_answer(&committed))
    }
}

pub(crate) fn delete_entry(
    files: &Files,
    target: &Target,
    propose: &ProposeFlags,
) -> Result<Value, Value> {
    let change = Change::Delete {
        target: target.clone(),
    };
    if propose.propose {
        let reviewers = reviewers_of(propose)?;
        let proposed = files
            .propose(
                &change,
                &reviewers,
                propose.expires_at_ms,
                propose.change_set.as_deref(),
            )
            .map_err(|e| files_failure(&e))?;
        Ok(proposed_answer(&proposed))
    } else {
        let committed = files
            .change_now(&change, propose.change_set.as_deref())
            .map_err(|e| files_failure(&e))?;
        Ok(committed_answer(&committed))
    }
}

pub(crate) fn proposals(
    files: &Files,
    change_set: Option<&str>,
    mine: bool,
) -> Result<Value, Value> {
    let list = if mine {
        let mut own = files.own_proposals().map_err(|e| files_failure(&e))?;
        if let Some(cs_id) = change_set {
            own.retain(|p| p.change_set_id.as_deref() == Some(cs_id));
        }
        own
    } else {
        files.proposals(change_set).map_err(|e| files_failure(&e))?
    };
    Ok(json!({ "proposals": list }))
}

pub(crate) fn proposal(files: &Files, id: &str, open: bool) -> Result<Value, Value> {
    let revisions = files.proposal(id).map_err(|e| files_failure(&e))?;
    let opened = if open {
        match latest_revision(&revisions) {
            Some(latest) => files
                .open_revision(latest)
                .map_err(|e| files_failure(&e))?
                .into_iter()
                .map(|item| {
                    json!({
                        "entry_id": item.entry_id,
                        "content_id": item.content_id,
                        "size": item.size,
                        "file": item.file,
                        "fetched": item.fetched,
                    })
                })
                .collect::<Vec<_>>(),
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };
    Ok(json!({
        "proposal_id": id,
        "revisions": revisions,
        "opened": opened,
    }))
}

pub(crate) fn decide(
    files: &Files,
    ids: &[String],
    change_set: Option<&str>,
    approve: bool,
    reject: bool,
) -> Result<Value, Value> {
    let verdict = match (approve, reject) {
        (true, false) => Verdict::Approve,
        (false, true) => Verdict::Reject,
        _ => {
            return Err(failure(
                "usage",
                "pass exactly one of --approve or --reject",
            ))
        }
    };
    let decisions: Vec<Decision> = match (ids.is_empty(), change_set) {
        (false, None) => {
            let mut out = Vec::with_capacity(ids.len());
            for named in ids {
                let (id, revision) = named
                    .split_once('/')
                    .filter(|(id, rev)| !id.is_empty() && !rev.is_empty())
                    .ok_or_else(|| {
                        failure(
                            "usage",
                            &format!(
                                "name the revision you reviewed as <proposal_id>/<revision>, \
                                 not {named}"
                            ),
                        )
                    })?;
                let revisions = files.proposal(id).map_err(|e| files_failure(&e))?;
                let named_revision = revisions
                    .iter()
                    .find(|rev| rev.revision == revision)
                    .ok_or_else(|| {
                        failure("not_found", &format!("no proposal revision {named}"))
                    })?;
                out.push(Decision {
                    proposal_id: named_revision.proposal_id.clone(),
                    proposal_revision: named_revision.revision.clone(),
                    proposal_digest: named_revision.digest.clone(),
                    decision: verdict,
                });
            }
            out
        }
        (true, Some(cs_id)) => {
            let list = files
                .proposals(Some(cs_id))
                .map_err(|e| files_failure(&e))?;
            if list.is_empty() {
                return Err(failure(
                    "not_found",
                    &format!("no open proposals in change set {cs_id}"),
                ));
            }
            list.into_iter()
                .map(|rev| Decision {
                    proposal_id: rev.proposal_id,
                    proposal_revision: rev.revision,
                    proposal_digest: rev.digest,
                    decision: verdict,
                })
                .collect()
        }
        _ => {
            return Err(failure(
                "usage",
                "pass proposal ids or --change-set <id>, not both",
            ))
        }
    };
    let receipt = files.decide(decisions).map_err(|e| files_failure(&e))?;
    if receipt.outcome == Outcome::Rejected {
        let code = receipt
            .code
            .and_then(|c| serde_json::to_value(c).ok())
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "rejected".to_owned());
        return Err(json!({
            "ok": false,
            "outcome": receipt.outcome,
            "code": code,
            "error": format!("decision rejected: {code}"),
            "event_id": receipt.event_id,
            "seq": receipt.seq,
            "items": receipt.detail,
        }));
    }
    Ok(json!({
        "outcome": receipt.outcome,
        "code": receipt.code,
        "event_id": receipt.event_id,
        "seq": receipt.seq,
        "items": receipt.detail,
    }))
}

pub(crate) fn unreviewed(
    files: &Files,
    after: &str,
    limit: u32,
    change_set: Option<&str>,
) -> Result<Value, Value> {
    let page = files
        .unreviewed(after, limit, change_set)
        .map_err(|e| files_failure(&e))?;
    Ok(json!({
        "events": page.events,
        "scanned_through": page.scanned_through,
    }))
}

pub(crate) fn acknowledge(
    files: &Files,
    ids: &[String],
    change_set: Option<&str>,
) -> Result<Value, Value> {
    let targets: Vec<String> = match (ids.is_empty(), change_set) {
        (false, None) => ids.to_vec(),
        (true, Some(cs_id)) => {
            let mut cursor = "0".to_owned();
            let mut collected = Vec::new();
            let page_cap = usize::try_from(PAGE_MAX).unwrap_or(100);
            loop {
                let page = files
                    .unreviewed(&cursor, PAGE_MAX, Some(cs_id))
                    .map_err(|e| files_failure(&e))?;
                let full = page.events.len() >= page_cap;
                for item in page.events {
                    collected.push(item.event.id);
                }
                if !full || page.scanned_through == cursor {
                    break;
                }
                cursor = page.scanned_through;
            }
            collected
        }
        _ => {
            return Err(failure(
                "usage",
                "pass event ids or --change-set <id>, not both",
            ))
        }
    };
    let committed = files.acknowledge(&targets).map_err(|e| files_failure(&e))?;
    Ok(json!({
        "outcome": "accepted",
        "request_id": committed.request_id,
        "event_id": committed.event_id,
        "seq": committed.seq,
        "targets": targets,
    }))
}

pub(crate) fn preview_reversal(files: &Files, event_id: &str) -> Result<Value, Value> {
    let preview = files
        .preview_reversal(event_id)
        .map_err(|e| files_failure(&e))?;
    serde_json::to_value(&preview).map_err(|e| failure("storage", &e.to_string()))
}

pub(crate) fn reverse(files: &Files, event_id: &str) -> Result<Value, Value> {
    let committed = files.reverse(event_id).map_err(|e| files_failure(&e))?;
    Ok(committed_answer(&committed))
}

pub(crate) fn history(
    files: &Files,
    after: &str,
    limit: u32,
    entity: Option<&str>,
    actor: Option<&str>,
    origin: Option<&str>,
    change_set: Option<&str>,
) -> Result<Value, Value> {
    let origin_class = match origin {
        Some(value) => Some(parse_origin(value)?),
        None => None,
    };
    let filter = HistoryFilter {
        entity_id: entity.map(str::to_owned),
        actor_id: actor.map(str::to_owned),
        origin_class,
    };
    if let Some(cs_id) = change_set {
        let page = files
            .change_set_history(after, &filter, cs_id)
            .map_err(|e| files_failure(&e))?;
        return Ok(json!({
            "events": page.events,
            "scanned_through": page.scanned_through,
        }));
    }
    let page = files
        .history(&HistoryParams {
            version: 1,
            after: after.to_owned(),
            limit,
            filter,
        })
        .map_err(|e| files_failure(&e))?;
    Ok(json!({
        "events": page.events,
        "scanned_through": page.scanned_through,
    }))
}
