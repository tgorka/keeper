//! Steps 1, 3–6: the envelope, who may send what, the grant, the payload.

use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::{Body, Request};
use crate::caller::{self, Caller};
use crate::ids;
use crate::names::{self, FileMode};
use crate::refusal::{Code, Refusal};
use crate::rows::Kind;
use crate::wire::{
    self, AcknowledgePayload, ChangePayload, DecidePayload, Envelope, ImportPayload,
    ProposePayload, ReversePayload, ShorthandPayload, WireOp,
};

const fn invalid() -> Refusal {
    Refusal::new(Code::InvalidChange)
}

/// Step 1: `params.version` is 1, else `unsupported_version` (whatever else
/// `params` holds, a non-object included).
pub(crate) fn check_version(params: &Value) -> Result<(), Refusal> {
    if params.get("version").and_then(Value::as_u64) == Some(1) {
        Ok(())
    } else {
        Err(Refusal::new(Code::UnsupportedVersion))
    }
}

/// The digest value `{version, descriptor, context_id, approval_id, job,
/// payload}` (absent members as `null`).
pub(crate) fn digest_value(descriptor: &str, params: &Value) -> Value {
    let field = |key: &str| params.get(key).cloned().unwrap_or(Value::Null);
    json!({
        "version": field("version"),
        "descriptor": descriptor,
        "context_id": field("context_id"),
        "approval_id": field("approval_id"),
        "job": field("job"),
        "payload": field("payload"),
    })
}

/// Steps 3–6.
pub(crate) fn parse(caller: &Caller, descriptor: &str, params: &Value) -> Result<Request, Refusal> {
    // Step 3: envelope shape, reserved fields, context_id rules, ULIDs.
    let env: Envelope = serde_json::from_value(params.clone()).map_err(|_| invalid())?;
    // `job` is a job caller's alone (the server's registration point judged
    // its attempt current before deriving one); a job caller always names it.
    let is_job = matches!(caller, Caller::Job { .. });
    if env.approval_id.is_some() || (env.job.is_some() && !is_job) {
        return Err(Refusal::new(Code::ReservedField));
    }
    if is_job && env.job.is_none() {
        return Err(Refusal::new(Code::Forbidden));
    }
    if env.job.as_ref().is_some_and(|job| {
        !ids::is_ulid(&job.job_id) || !ids::is_ulid(&job.attempt_id) || job.ordinal < 0
    }) {
        return Err(invalid());
    }
    match caller {
        Caller::Agent { .. } => {
            if env.context_id.is_none() {
                return Err(Refusal::new(Code::Forbidden));
            }
        }
        Caller::Person { .. }
        | Caller::Tool { .. }
        | Caller::Job { .. }
        | Caller::Import { .. } => {
            if env.context_id.is_some() {
                return Err(Refusal::new(Code::ReservedField));
            }
        }
    }
    if !ids::is_ulid(&env.request_id)
        || env
            .context_id
            .as_deref()
            .is_some_and(|id| !ids::is_ulid(id))
    {
        return Err(invalid());
    }

    // Step 4: descriptor validity and import caller isolation.
    if !wire::DESCRIPTORS.contains(&descriptor) {
        return Err(invalid());
    }
    let is_import_caller = matches!(caller, Caller::Import { .. });
    let is_import_desc = descriptor == "files.import";
    if is_import_caller != is_import_desc {
        return Err(Refusal::new(Code::Forbidden));
    }

    // Step 5: grant file-part check for rooted callers.
    if let Caller::Agent { files, .. } | Caller::Tool { files, .. } | Caller::Job { files, .. } =
        caller
    {
        let _ = caller::parse_file_part(files)?;
    }

    // Step 6: reserved `sources` key before any serde parse of the payload.
    if has_member(&env.payload, "sources") {
        return Err(Refusal::new(Code::ReservedField));
    }

    let body = parse_body(descriptor, env.payload)?;

    Ok(Request {
        descriptor: descriptor.to_owned(),
        request_id: env.request_id,
        context_id: env.context_id,
        job: env.job,
        digest: digest_value(descriptor, params),
        body,
    })
}

fn parse_body(descriptor: &str, payload: Value) -> Result<Body, Refusal> {
    match descriptor {
        "files.change" => {
            let parsed: ChangePayload = serde_json::from_value(payload).map_err(|_| invalid())?;
            if parsed
                .change_set_id
                .as_deref()
                .is_some_and(|id| !ids::is_ulid(id))
            {
                return Err(invalid());
            }
            validate_ops(&parsed.operations, false)?;
            Ok(Body::Change {
                ops: parsed.operations,
                change_set_id: parsed.change_set_id,
                event_time_ms: parsed.event_time_ms,
                import: false,
            })
        }
        "files.put" | "files.move" | "files.delete" | "files.restore" => {
            let parsed: ShorthandPayload =
                serde_json::from_value(payload).map_err(|_| invalid())?;
            if !shorthand_matches(descriptor, &parsed.operation)
                || parsed
                    .change_set_id
                    .as_deref()
                    .is_some_and(|id| !ids::is_ulid(id))
            {
                return Err(invalid());
            }
            let ops = vec![parsed.operation];
            validate_ops(&ops, false)?;
            Ok(Body::Change {
                ops,
                change_set_id: parsed.change_set_id,
                event_time_ms: None,
                import: false,
            })
        }
        "files.import" => {
            let parsed: ImportPayload = serde_json::from_value(payload).map_err(|_| invalid())?;
            if !ids::is_ulid(&parsed.change_set_id) {
                return Err(invalid());
            }
            validate_ops(&parsed.operations, true)?;
            Ok(Body::Change {
                ops: parsed.operations,
                change_set_id: Some(parsed.change_set_id),
                event_time_ms: Some(parsed.event_time_ms),
                import: true,
            })
        }
        "files.reverse" => {
            let parsed: ReversePayload = serde_json::from_value(payload).map_err(|_| invalid())?;
            if !ids::is_ulid(&parsed.event_id) {
                return Err(invalid());
            }
            Ok(Body::Reverse {
                event_id: parsed.event_id,
            })
        }
        // Shapes only here: `rules::propose` and `rules::decide` judge the
        // rest (bounds, reviewers, revisions) in their own order.
        "files.propose" => {
            let parsed: ProposePayload = serde_json::from_value(payload).map_err(|_| invalid())?;
            validate_ops(&parsed.change.operations, false)?;
            Ok(Body::Propose(parsed))
        }
        "files.decide" => {
            let parsed: DecidePayload = serde_json::from_value(payload).map_err(|_| invalid())?;
            Ok(Body::Decide(parsed))
        }
        "files.acknowledge" => {
            let parsed: AcknowledgePayload =
                serde_json::from_value(payload).map_err(|_| invalid())?;
            Ok(Body::Acknowledge(parsed.targets))
        }
        _ => Err(invalid()),
    }
}

fn shorthand_matches(descriptor: &str, op: &WireOp) -> bool {
    matches!(
        (descriptor, op),
        (
            "files.put",
            WireOp::Create {
                kind: Kind::File,
                ..
            } | WireOp::Replace { .. },
        ) | ("files.move", WireOp::Move { .. })
            | ("files.delete", WireOp::Delete { .. })
            | ("files.restore", WireOp::Restore { .. })
    )
}

fn validate_ops(ops: &[WireOp], import: bool) -> Result<(), Refusal> {
    if ops.is_empty() {
        return Err(invalid());
    }
    let mut seen = BTreeSet::new();
    for op in ops {
        if !seen.insert(op.id()) {
            return Err(invalid());
        }
        validate_op_shape(op, import)?;
    }
    for op in ops {
        match op {
            WireOp::Create { name, .. } | WireOp::Move { name, .. } => {
                names::admit_name(name)?;
            }
            WireOp::Replace { .. } | WireOp::Delete { .. } | WireOp::Restore { .. } => {}
        }
    }
    Ok(())
}

fn validate_op_shape(op: &WireOp, import: bool) -> Result<(), Refusal> {
    if !ids::is_ulid(op.id()) || op.provenance().is_some() != import {
        return Err(invalid());
    }
    match op {
        WireOp::Create {
            kind,
            parent_id,
            content_id,
            size,
            mtime_ms,
            mode,
            git_mode,
            ..
        } => {
            match parent_id {
                Some(pid) => {
                    if !ids::is_ulid(pid) {
                        return Err(invalid());
                    }
                }
                None => {
                    if *kind != Kind::Dir {
                        return Err(invalid());
                    }
                }
            }
            match kind {
                Kind::File => {
                    let Some(cid) = content_id.as_deref() else {
                        return Err(invalid());
                    };
                    if !ids::is_content_id(cid) || size.is_none() {
                        return Err(invalid());
                    }
                    if import {
                        if mode.is_some() || mtime_ms.is_some() {
                            return Err(invalid());
                        }
                        let Some(gm) = git_mode.as_deref() else {
                            return Err(invalid());
                        };
                        let _ = FileMode::from_git(gm)?;
                    } else if git_mode.is_some() {
                        return Err(invalid());
                    }
                }
                Kind::Dir => {
                    if content_id.is_some()
                        || size.is_some()
                        || mtime_ms.is_some()
                        || git_mode.is_some()
                        || (import && mode.is_some())
                        || matches!(mode, Some(FileMode::Executable))
                    {
                        return Err(invalid());
                    }
                }
            }
        }
        WireOp::Replace {
            expected_revision,
            content_id,
            mtime_ms,
            mode,
            git_mode,
            ..
        } => {
            if !is_decimal_digits(expected_revision) || !ids::is_content_id(content_id) {
                return Err(invalid());
            }
            if import {
                if mode.is_some() || mtime_ms.is_some() {
                    return Err(invalid());
                }
                let Some(gm) = git_mode.as_deref() else {
                    return Err(invalid());
                };
                let _ = FileMode::from_git(gm)?;
            } else if git_mode.is_some() {
                return Err(invalid());
            }
        }
        WireOp::Move {
            expected_revision,
            parent_id,
            ..
        } => {
            if !is_decimal_digits(expected_revision) || !ids::is_ulid(parent_id) {
                return Err(invalid());
            }
        }
        WireOp::Delete {
            expected_revision, ..
        }
        | WireOp::Restore {
            expected_revision, ..
        } => {
            if !is_decimal_digits(expected_revision) {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

fn is_decimal_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

fn has_member(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(map) => map.contains_key(key) || map.values().any(|v| has_member(v, key)),
        Value::Array(items) => items.iter().any(|v| has_member(v, key)),
        _ => false,
    }
}
