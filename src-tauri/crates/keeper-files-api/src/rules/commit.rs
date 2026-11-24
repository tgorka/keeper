//! Step 10, and the inverse operations a reversal judges.

use std::cmp::Reverse;

use serde_json::{json, Map, Value};

use super::helpers::digest_hex;
use super::{Accepted, Body, Manifest, Request, Server, Staged, StagedOp};
use crate::caller::Caller;
use crate::ids;
use crate::names::FileMode;
use crate::refusal::{Code, Refusal};
use crate::rows::{ApprovalBasis, Drive, EntityType, Entry, Event, Head, Lifecycle, Op, Operation};
use crate::wire::WireOp;

/// The inverse of every entry operation of event `event_id`, in reverse
/// ordinal order, each expecting the target operation's `after_revision`.
///
/// Carried cross-drive descendant operations and `review` operations are
/// skipped. Returns `conflict` `{entity_id: event_id, precondition: "absent"}`
/// when the event is absent, or `invalid_change` when the event has no entry
/// operation to invert.
pub fn inverse(m: &Manifest, event_id: &str) -> Result<Vec<WireOp>, Refusal> {
    let Some((_, ops)) = m.events.get(event_id) else {
        return Err(Refusal::with_detail(
            Code::Conflict,
            json!({
                "entity_id": event_id,
                "precondition": "absent",
            }),
        ));
    };

    let mut entry_ops: Vec<&Op> = ops
        .iter()
        .filter(|op| op.entity_type == EntityType::Entry && !is_carried_move(op))
        .collect();
    if entry_ops.is_empty() {
        return Err(Refusal::new(Code::InvalidChange));
    }
    entry_ops.sort_by_key(|op| Reverse(op.ordinal));

    let mut out = Vec::with_capacity(entry_ops.len());
    for op in entry_ops {
        out.push(invert_op(op)?);
    }
    Ok(out)
}

fn is_carried_move(op: &Op) -> bool {
    op.operation == Operation::Move
        && op
            .after_value
            .as_ref()
            .and_then(Value::as_object)
            .is_some_and(|obj| obj.len() == 1 && obj.contains_key("drive_key"))
}

fn invert_op(op: &Op) -> Result<WireOp, Refusal> {
    let expected_revision = op
        .after_revision
        .clone()
        .ok_or_else(|| Refusal::new(Code::InvalidChange))?;
    let id = op.entity_id.clone();

    match op.operation {
        Operation::Create | Operation::Restore => Ok(WireOp::Delete {
            id,
            expected_revision,
            provenance: None,
        }),
        Operation::Delete => Ok(WireOp::Restore {
            id,
            expected_revision,
            provenance: None,
        }),
        Operation::Replace => {
            let content_id = op
                .before_content_id
                .clone()
                .ok_or_else(|| Refusal::new(Code::InvalidChange))?;
            let before = op
                .before_value
                .as_ref()
                .and_then(Value::as_object)
                .ok_or_else(|| Refusal::new(Code::InvalidChange))?;
            let size = before
                .get("size")
                .and_then(Value::as_u64)
                .ok_or_else(|| Refusal::new(Code::InvalidChange))?;
            let mtime_ms = match before.get("mtime_ms") {
                Some(Value::Null) => None,
                Some(v) => Some(
                    v.as_i64()
                        .ok_or_else(|| Refusal::new(Code::InvalidChange))?,
                ),
                None => return Err(Refusal::new(Code::InvalidChange)),
            };
            let mode = match before.get("mode") {
                None => None,
                Some(v) => Some(
                    serde_json::from_value::<FileMode>(v.clone())
                        .map_err(|_| Refusal::new(Code::InvalidChange))?,
                ),
            };
            Ok(WireOp::Replace {
                id,
                expected_revision,
                content_id,
                size,
                mtime_ms,
                mode,
                git_mode: None,
                provenance: None,
            })
        }
        Operation::Move => {
            let before = op
                .before_value
                .as_ref()
                .and_then(Value::as_object)
                .ok_or_else(|| Refusal::new(Code::InvalidChange))?;
            let parent_id = before
                .get("parent_id")
                .and_then(Value::as_str)
                .ok_or_else(|| Refusal::new(Code::InvalidChange))?
                .to_owned();
            let name = before
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| Refusal::new(Code::InvalidChange))?
                .to_owned();
            Ok(WireOp::Move {
                id,
                expected_revision,
                parent_id,
                name,
                provenance: None,
            })
        }
    }
}

/// Step 10: writes the accepted change into `m` and returns its event.
pub(crate) fn commit(
    m: &mut Manifest,
    caller: &Caller,
    request: &Request,
    basis: ApprovalBasis,
    mut staged: Staged,
    reverses: Option<&str>,
    server: &Server,
) -> Accepted {
    let seq = ids::next_decimal(m.head.as_ref().map_or("0", |h| h.seq.as_str()));
    let import = request.descriptor == "files.import";
    let page_key = ids::page_key(&m.domain, server.recorded_at_ms, import);

    for (id, row) in &mut staged.rows {
        if let Some(before) = m.entries.get(id) {
            row.revision = ids::next_decimal(&before.revision);
        } else {
            row.revision = "1".to_owned();
            row.created_event_id = server.event_id.clone();
        }
        row.updated_event_id = server.event_id.clone();
        row.updated_seq = seq.clone();
    }

    let mut ops = Vec::with_capacity(staged.steps.len() + usize::from(reverses.is_some()));
    for step in &staged.steps {
        if let Some(op) = build_step_op(m, &staged, step, &page_key, server, &ops) {
            ops.push(op);
        }
        for carried_id in &step.carried {
            if let Some(op) =
                build_carried_op(m, &staged, step, carried_id, &page_key, server, &ops)
            {
                ops.push(op);
            }
        }
    }

    if let Some(target_event_id) = reverses {
        let ordinal = next_ordinal(&ops);
        let review_id = server
            .review_id
            .clone()
            .unwrap_or_else(|| format!("{}/review", server.event_id));
        ops.push(Op {
            id: format!("{}/{ordinal}", server.event_id),
            domain: m.domain.clone(),
            event_id: server.event_id.clone(),
            ordinal,
            page_key: page_key.clone(),
            entity_type: EntityType::Review,
            entity_id: review_id,
            operation: Operation::Create,
            before_revision: None,
            after_revision: Some("1".to_owned()),
            before_value: None,
            after_value: Some(json!({
                "target_event_id": target_event_id,
                "kind": "reversal",
                "reviewer_actor": caller.actor_id(),
            })),
            before_content_id: None,
            after_content_id: None,
        });
    }

    let delta_digest = compute_delta_digest(&ops);
    let request_digest = digest_hex(&request.digest);
    let (change_set_id, event_time_ms) = match &request.body {
        Body::Change {
            change_set_id,
            event_time_ms,
            ..
        } => (change_set_id.clone(), *event_time_ms),
        Body::Reverse { .. } => (None, None),
    };

    let event = Event {
        id: server.event_id.clone(),
        domain: m.domain.clone(),
        seq: seq.clone(),
        page_key,
        descriptor: request.descriptor.clone(),
        request_id: request.request_id.clone(),
        request_digest,
        actor_id: caller.actor_id().to_owned(),
        client_id: server.client_id.clone(),
        origin_class: caller.origin_class(),
        origin_actor: None,
        origin_chain: None,
        context_id: request.context_id.clone(),
        approval_id: None,
        approval_basis: basis,
        proposal_id: None,
        change_set_id,
        job_id: None,
        job_ordinal: None,
        job_attempt_id: None,
        reverses_event_id: reverses.map(str::to_owned),
        event_time_ms,
        recorded_at_ms: server.recorded_at_ms,
        op_count: next_ordinal(&ops),
        delta_digest: delta_digest.clone(),
    };

    let drives = updated_drives(m.head.as_ref(), &staged);

    for (id, row) in staged.rows {
        m.entries.insert(id, row);
    }
    m.head = Some(Head {
        id: m.domain.clone(),
        domain: m.domain.clone(),
        seq,
        event_id: server.event_id.clone(),
        digest: delta_digest,
        drives,
    });
    m.events
        .insert(server.event_id.clone(), (event.clone(), ops.clone()));

    Accepted { event, ops }
}

fn next_ordinal(ops: &[Op]) -> u32 {
    u32::try_from(ops.len()).unwrap_or(u32::MAX)
}

fn build_step_op(
    m: &Manifest,
    staged: &Staged,
    step: &StagedOp,
    page_key: &str,
    server: &Server,
    ops: &[Op],
) -> Option<Op> {
    let entity_id = step.op.id();
    let after = staged.rows.get(entity_id)?;
    let before = step.before.as_ref();
    let ordinal = next_ordinal(ops);

    let (operation, before_revision, before_value, after_value, before_content_id) = match &step.op
    {
        WireOp::Create { .. } => (
            Operation::Create,
            None,
            None,
            Some(create_after_value(after)),
            None,
        ),
        WireOp::Replace { .. } => {
            let b = before?;
            let (bv, av) = replace_values(b, after);
            (
                Operation::Replace,
                Some(b.revision.clone()),
                Some(bv),
                Some(av),
                b.content_id.clone(),
            )
        }
        WireOp::Move { .. } => {
            let b = before?;
            let (bv, av) = move_values(b, after);
            (
                Operation::Move,
                Some(b.revision.clone()),
                Some(bv),
                Some(av),
                b.content_id.clone(),
            )
        }
        WireOp::Delete { .. } => {
            let b = before?;
            (
                Operation::Delete,
                Some(b.revision.clone()),
                Some(json!({ "lifecycle": b.lifecycle })),
                Some(json!({ "lifecycle": after.lifecycle })),
                b.content_id.clone(),
            )
        }
        WireOp::Restore { .. } => {
            let b = before?;
            (
                Operation::Restore,
                Some(b.revision.clone()),
                Some(json!({ "lifecycle": b.lifecycle })),
                Some(json!({ "lifecycle": after.lifecycle })),
                b.content_id.clone(),
            )
        }
    };

    Some(Op {
        id: format!("{}/{ordinal}", server.event_id),
        domain: m.domain.clone(),
        event_id: server.event_id.clone(),
        ordinal,
        page_key: page_key.to_owned(),
        entity_type: EntityType::Entry,
        entity_id: entity_id.to_owned(),
        operation,
        before_revision,
        after_revision: Some(after.revision.clone()),
        before_value,
        after_value,
        before_content_id,
        after_content_id: after.content_id.clone(),
    })
}

fn build_carried_op(
    m: &Manifest,
    staged: &Staged,
    step: &StagedOp,
    carried_id: &str,
    page_key: &str,
    server: &Server,
    ops: &[Op],
) -> Option<Op> {
    let after = staged.rows.get(carried_id)?;
    let before = m.entries.get(carried_id);
    let ordinal = next_ordinal(ops);
    let before_drive_key = before
        .map(|b| b.drive_key.as_str())
        .or_else(|| step.before.as_ref().map(|b| b.drive_key.as_str()))
        .unwrap_or_default();
    let before_revision = before.map(|b| b.revision.clone());
    let before_content_id = before
        .map(|b| b.content_id.clone())
        .unwrap_or_else(|| after.content_id.clone());

    Some(Op {
        id: format!("{}/{ordinal}", server.event_id),
        domain: m.domain.clone(),
        event_id: server.event_id.clone(),
        ordinal,
        page_key: page_key.to_owned(),
        entity_type: EntityType::Entry,
        entity_id: carried_id.to_owned(),
        operation: Operation::Move,
        before_revision,
        after_revision: Some(after.revision.clone()),
        before_value: Some(json!({ "drive_key": before_drive_key })),
        after_value: Some(json!({ "drive_key": after.drive_key })),
        before_content_id,
        after_content_id: after.content_id.clone(),
    })
}

fn create_after_value(row: &Entry) -> Value {
    json!({
        "kind": row.kind,
        "drive_key": row.drive_key,
        "parent_id": row.parent_id,
        "name": row.name,
        "name_fold": row.name_fold,
        "content_id": row.content_id,
        "size": row.size,
        "media_type": row.media_type,
        "mtime_ms": row.mtime_ms,
        "mode": row.mode,
        "lifecycle": row.lifecycle,
    })
}

fn replace_values(before: &Entry, after: &Entry) -> (Value, Value) {
    let mut bv = Map::new();
    let mut av = Map::new();
    bv.insert("content_id".to_owned(), json!(before.content_id));
    bv.insert("size".to_owned(), json!(before.size));
    bv.insert("mtime_ms".to_owned(), json!(before.mtime_ms));
    av.insert("content_id".to_owned(), json!(after.content_id));
    av.insert("size".to_owned(), json!(after.size));
    av.insert("mtime_ms".to_owned(), json!(after.mtime_ms));
    if before.mode != after.mode {
        bv.insert("mode".to_owned(), json!(before.mode));
        av.insert("mode".to_owned(), json!(after.mode));
    }
    (Value::Object(bv), Value::Object(av))
}

fn move_values(before: &Entry, after: &Entry) -> (Value, Value) {
    let mut bv = Map::new();
    let mut av = Map::new();
    bv.insert("parent_id".to_owned(), json!(before.parent_id));
    bv.insert("name".to_owned(), json!(before.name));
    bv.insert("name_fold".to_owned(), json!(before.name_fold));
    av.insert("parent_id".to_owned(), json!(after.parent_id));
    av.insert("name".to_owned(), json!(after.name));
    av.insert("name_fold".to_owned(), json!(after.name_fold));
    if before.media_type != after.media_type {
        bv.insert("media_type".to_owned(), json!(before.media_type));
        av.insert("media_type".to_owned(), json!(after.media_type));
    }
    if before.drive_key != after.drive_key {
        bv.insert("drive_key".to_owned(), json!(before.drive_key));
        av.insert("drive_key".to_owned(), json!(after.drive_key));
    }
    (Value::Object(bv), Value::Object(av))
}

fn compute_delta_digest(ops: &[Op]) -> String {
    let array = Value::Array(
        ops.iter()
            .map(|op| {
                json!({
                    "ordinal": op.ordinal,
                    "entity_type": op.entity_type,
                    "entity_id": op.entity_id,
                    "operation": op.operation,
                    "before_revision": op.before_revision,
                    "after_revision": op.after_revision,
                    "before_value": op.before_value,
                    "after_value": op.after_value,
                    "before_content_id": op.before_content_id,
                    "after_content_id": op.after_content_id,
                })
            })
            .collect(),
    );
    digest_hex(&array)
}

fn updated_drives(head: Option<&Head>, staged: &Staged) -> Vec<Drive> {
    let mut drives = head.map_or_else(Vec::new, |h| h.drives.clone());
    for row in staged.rows.values() {
        if !row.is_root() {
            continue;
        }
        match row.lifecycle {
            Lifecycle::Deleted => drives.retain(|d| d.root_id != row.id),
            Lifecycle::Live => {
                if !drives.iter().any(|d| d.root_id == row.id) {
                    drives.push(Drive {
                        name: row.name.clone(),
                        root_id: row.id.clone(),
                    });
                }
            }
        }
    }
    drives.sort_by(|a, b| a.name.cmp(&b.name));
    drives
}
