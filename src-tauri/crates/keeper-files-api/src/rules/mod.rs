//! The reference rules: [`evaluate`] judges a `files.*` command against a
//! manifest exactly as the server must (`syncular/README.md`, *The drive*,
//! the evaluation order), so every implementation reproduces
//! `syncular/files.vectors.json` through it.
//!
//! Steps: 1 and 2 here; 3–6 [`parse`]; 7 [`authorize`]; 8–9 [`stage`];
//! 10 and the reversal's inverse [`commit`]. Read-only helpers are in
//! [`helpers`].

mod authorize;
mod commit;
mod helpers;
mod history;
mod parse;
mod reversal;
mod stage;

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;

pub use commit::inverse;
pub use helpers::{canonical, check_event, is_inside, list, narrower_or_equal, path_of};
pub use history::history;
pub use reversal::preview_reversal;

use crate::caller::Caller;
use crate::refusal::{Code, Refusal};
use crate::rows::{Entry, Event, Head, Op};
use crate::wire::WireOp;

/// One domain's manifest as the server holds it.
#[derive(Debug, Clone, Default)]
pub struct Manifest {
    pub domain: String,
    /// Every row of the domain, any lifecycle.
    pub entries: BTreeMap<String, Entry>,
    /// `None` before the first event (sequence `"0"`).
    pub head: Option<Head>,
    /// Every accepted event with its operations, by event id.
    pub events: BTreeMap<String, (Event, Vec<Op>)>,
    /// The content ids the domain's file key space holds.
    pub objects: BTreeSet<String>,
    /// `request_id` → (the digest value it is bound to, its result: the
    /// accepted event's id or the refusal).
    pub bindings: BTreeMap<String, (Value, Result<String, Refusal>)>,
}

/// What the server assigns to an accepted request.
#[derive(Debug, Clone, Deserialize)]
pub struct Server {
    pub event_id: String,
    pub recorded_at_ms: i64,
    pub client_id: String,
    #[serde(default)]
    pub review_id: Option<String>,
}

/// An accepted request: its event and its operations.
#[derive(Debug, Clone, PartialEq)]
pub struct Accepted {
    pub event: Event,
    pub ops: Vec<Op>,
}

/// A request that passed steps 3–6.
#[derive(Debug, Clone)]
pub(crate) struct Request {
    pub descriptor: String,
    pub request_id: String,
    pub context_id: Option<String>,
    /// The digest value (`{version, descriptor, context_id, approval_id,
    /// job, payload}`) whose canonical SHA-256 is `request_digest`.
    pub digest: Value,
    pub body: Body,
}

#[derive(Debug, Clone)]
pub(crate) enum Body {
    /// `files.change`, a shorthand (normalised to one operation) or
    /// `files.import` (`import` true).
    Change {
        ops: Vec<WireOp>,
        change_set_id: Option<String>,
        event_time_ms: Option<i64>,
        import: bool,
    },
    /// `files.reverse`.
    Reverse { event_id: String },
}

/// Steps 8–9's result: the change judged against the manifest, not yet
/// written.
#[derive(Debug, Clone)]
pub(crate) struct Staged {
    /// The final row of every entry the change writes (each created or
    /// changed one, and every row a cross-drive move carries), keyed by id,
    /// with `revision`, `created_event_id`, `updated_event_id` and
    /// `updated_seq` still as before (empty strings for a create): step 10
    /// sets them.
    pub rows: BTreeMap<String, Entry>,
    /// One per operation, in order.
    pub steps: Vec<StagedOp>,
}

#[derive(Debug, Clone)]
pub(crate) struct StagedOp {
    pub op: WireOp,
    /// The entity's row before the change; `None` for a create.
    pub before: Option<Entry>,
    /// For a move across drives: the ids of every descendant row it
    /// carries (any lifecycle), ascending. Empty otherwise.
    pub carried: Vec<String>,
}

/// Judges one command. Mutates `m` only when it is accepted, and records the
/// request's binding whatever the result (once step 1 passed).
pub fn evaluate(
    m: &mut Manifest,
    caller: &Caller,
    descriptor: &str,
    params: &Value,
    server: &Server,
) -> Result<Accepted, Refusal> {
    parse::check_version(params)?;
    let digest = parse::digest_value(descriptor, params);
    let Some(request_id) = params
        .get("request_id")
        .and_then(Value::as_str)
        .filter(|id| crate::ids::is_ulid(id))
        .map(str::to_owned)
    else {
        return Err(Refusal::new(Code::InvalidChange));
    };
    if let Some((bound, result)) = m.bindings.get(&request_id) {
        if *bound != digest {
            return Err(Refusal::new(Code::IdempotencyMismatch));
        }
        return match result {
            Ok(event_id) => m
                .events
                .get(event_id)
                .map(|(event, ops)| Accepted {
                    event: event.clone(),
                    ops: ops.clone(),
                })
                .ok_or_else(|| Refusal::new(Code::InvalidChange)),
            Err(refusal) => Err(refusal.clone()),
        };
    }
    let result = run(m, caller, descriptor, params, server);
    let recorded = match &result {
        Ok(accepted) => Ok(accepted.event.id.clone()),
        Err(refusal) => Err(refusal.clone()),
    };
    m.bindings.insert(request_id, (digest, recorded));
    result
}

fn run(
    m: &mut Manifest,
    caller: &Caller,
    descriptor: &str,
    params: &Value,
    server: &Server,
) -> Result<Accepted, Refusal> {
    let request = parse::parse(caller, descriptor, params)?;
    let (ops, reverses, import, event_time_ms) = match &request.body {
        Body::Change {
            ops,
            import,
            event_time_ms,
            ..
        } => (ops.clone(), None, *import, *event_time_ms),
        Body::Reverse { event_id } => (
            commit::inverse(m, event_id)?,
            Some(event_id.as_str()),
            false,
            None,
        ),
    };
    let basis = authorize::authorize(m, caller, &request, &ops, reverses)?;
    let staged = stage::stage(m, caller, &ops, import, event_time_ms, reverses.is_some())?;
    Ok(commit::commit(
        m, caller, &request, basis, staged, reverses, server,
    ))
}
