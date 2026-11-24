//! `files.preview_reversal`: dry-run planning and evaluation of a reversal.

use serde_json::Value;

use super::{authorize, commit, parse, stage, Body, Manifest, Request};
use crate::caller::Caller;
use crate::ids;
use crate::refusal::{Code, Refusal};
use crate::reversal::{Preview, PreviewParams, PreviewStatus};
use crate::wire::WireOp;

/// Plans the inverse of `params.event_id` and evaluates whether `files.reverse`
/// would be accepted for `caller` against `m`, without mutating `m` or binding
/// a request.
pub fn preview_reversal(m: &Manifest, caller: &Caller, params: &Value) -> Result<Preview, Refusal> {
    parse::check_version(params)?;
    let parsed: PreviewParams =
        serde_json::from_value(params.clone()).map_err(|_| Refusal::new(Code::InvalidChange))?;
    if !ids::is_ulid(&parsed.event_id) {
        return Err(Refusal::new(Code::InvalidChange));
    }

    let plan = match commit::inverse(m, &parsed.event_id) {
        Ok(ops) => ops,
        Err(refusal) => return Ok(refused_preview(parsed.event_id, Vec::new(), refusal)),
    };

    let request = Request {
        descriptor: "files.reverse".to_owned(),
        request_id: parsed.event_id.clone(),
        context_id: None,
        digest: Value::Null,
        body: Body::Reverse {
            event_id: parsed.event_id.clone(),
        },
    };

    let outcome = authorize::authorize(m, caller, &request, &plan, Some(parsed.event_id.as_str()))
        .and_then(|_| stage::stage(m, caller, &plan, false, None, true));

    match outcome {
        Ok(_) => Ok(Preview {
            target_event_id: parsed.event_id,
            status: PreviewStatus::Ready,
            plan,
            code: None,
            detail: None,
        }),
        Err(refusal) => Ok(refused_preview(parsed.event_id, plan, refusal)),
    }
}

fn refused_preview(target_event_id: String, plan: Vec<WireOp>, refusal: Refusal) -> Preview {
    let status = if refusal.code == Code::Conflict {
        PreviewStatus::Conflict
    } else {
        PreviewStatus::Rejected
    };
    Preview {
        target_event_id,
        status,
        plan,
        code: Some(refusal.code),
        detail: refusal.detail,
    }
}
