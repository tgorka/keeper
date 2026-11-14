//! The inverse of one retained accepted event (contract §4 *Reversal*,
//! §9.5; CC-X26, X27, X29): server-derived and event-bound.
//!
//! [`plan`] derives the compensating operations from the retained target
//! event alone — its header and its ordered operations with their before-
//! and after-images — and [`crate::candidate::apply_inverse`] judges them
//! against the current graph. No client payload carries an inverse
//! operation, no inverse is a [`crate::change::Change`], and nothing here is
//! reachable from one.
//!
//! **Mapping**, in reverse operation order, each planned operation's
//! before-image being the target operation's after-image (the state the
//! reversal requires now, exactly):
//!
//! | target operation | planned operation | its after-image |
//! | --- | --- | --- |
//! | node or edge `create` | `archive` | the created image, archived |
//! | node or edge `update`, `archive`, `unarchive` | by lifecycle: `update`, `archive` or `unarchive` | the target's before-image |
//! | content `update` | `update` | the target's before-image |
//! | membership `create` | `remove` | none |
//! | membership `remove` | `create` | `{id: null, node_id, tag_id}`, the id minted at acceptance |
//! | tag `create` | none: the tag row is retained, inert | — |
//! | content `create` whose owner the event created | none: retained with its archived owner | — |
//! | a reversal's own `review` | none | — |
//!
//! Restored images keep every id; every revision moves forward: an entity's
//! `revision` to one more than the target's after-image, a node's
//! `lifecycle_revision` likewise when its lifecycle changes and unchanged
//! otherwise. So a merge's absorbed-node `archive` (setting
//! `merged_into_id`) inverts to *restore-merged* (`unarchive`, clearing
//! it), its endpoint-rewriting edge `update`s to *restore-endpoints*, and
//! reversing that reversal *re-merges* through the same rule.
//!
//! **Verification (X29)** is the one field never restored: every planned
//! node after-image carrying `$assertion` is `unverified` with null
//! `verified_at`/`verified_by`, whatever the before-image held.
//!
//! **Not reversible** (`invalid_change`): an acknowledgement event
//! ([`crate::review::ACKNOWLEDGE`], or any acknowledgement review
//! operation), a content `create` whose owner the event did not create, an
//! event naming one entity twice, a reversal event without exactly one
//! reversal review of its `reverses_event_id`, and an event that leaves
//! nothing to invert.
//!
//! [`REVERSE`]'s payload is exactly `{"target_event_id", "target_delta_digest",
//! "expected": [<expected>…]}` plus optional `approval_id` and
//! `event_time_ms`, `expected` being [`expected`]'s list as the preview
//! answered it: at most [`MAX_EXPECTED`] entries `{entity_type, id,
//! revision, lifecycle_revision?}` (`lifecycle_revision` on nodes only),
//! strictly ascending by (entity_type, id). The server re-derives the plan
//! at execution; an entity missing from, extra to or different in
//! `expected` is a conflict ([`compare_expected`]).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::assertion;
use crate::candidate::{apply_inverse, Retained, Stamp, Verdict};
use crate::change::{MAX_APPROVAL_ID_BYTES, MAX_EVENT_TIME_MS};
use crate::delta::MAX_OPERATIONS;
use crate::review::{self, Target, REVERSAL};
use crate::vocabulary::{EntityType, Operation};
use crate::wire::Event;
use crate::{is_decimal, is_ulid, Refusal};

/// The descriptor of a reversal.
pub const REVERSE: &str = "graph.reverse";
/// The most entities a reversal's `expected` may list.
pub const MAX_EXPECTED: usize = MAX_OPERATIONS;

/// One planned inverse operation: an event operation without its ordinal,
/// whose entity id is `None` for a membership the server mints.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedOp {
    pub entity_type: EntityType,
    pub entity_id: Option<String>,
    pub operation: Operation,
    pub before_revision: Option<String>,
    pub after_revision: Option<String>,
    pub before_value: Option<Value>,
    pub after_value: Option<Value>,
}

/// The inverse of one event: the event it reverses, the planned
/// operations, and the rows it leaves retained (tags and contents the event
/// created, which have no lifecycle).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub target_event_id: String,
    pub target_delta_digest: String,
    /// The target's graph position: dependents written after it are later.
    pub target_graph_seq: String,
    pub ops: Vec<PlannedOp>,
    pub retained: Vec<Value>,
}

impl Plan {
    /// The review target this plan reverses.
    pub fn target(&self) -> Result<Target, Refusal> {
        Target::new(&self.target_event_id, &self.target_delta_digest)
    }
}

fn text(image: &Map<String, Value>, key: &str) -> String {
    image
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The decimal after `revision`.
pub(crate) fn next(revision: &str) -> Result<String, Refusal> {
    revision
        .parse::<u128>()
        .ok()
        .filter(|_| is_decimal(revision))
        .and_then(|n| n.checked_add(1))
        .map(|n| n.to_string())
        .filter(|n| n.len() <= 20)
        .ok_or(Refusal::InvalidChange)
}

/// The operation that moves an entity from lifecycle `from` to `to`.
pub(crate) fn transition(from: &str, to: &str) -> Operation {
    match (from, to) {
        ("active", "archived") => Operation::Archive,
        ("archived", "active") => Operation::Unarchive,
        _ => Operation::Update,
    }
}

fn object(value: Option<&Value>) -> Result<&Map<String, Value>, Refusal> {
    value
        .and_then(Value::as_object)
        .ok_or(Refusal::InvalidChange)
}

fn planned(
    entity_type: EntityType,
    operation: Operation,
    before: &Map<String, Value>,
    after: Map<String, Value>,
) -> PlannedOp {
    PlannedOp {
        entity_type,
        entity_id: Some(text(before, "id")),
        operation,
        before_revision: Some(text(before, "revision")),
        after_revision: Some(text(&after, "revision")),
        before_value: Some(Value::Object(before.clone())),
        after_value: Some(Value::Object(after)),
    }
}

/// A created node or edge, archived again.
fn archive_created(
    entity_type: EntityType,
    created: &Map<String, Value>,
) -> Result<PlannedOp, Refusal> {
    if text(created, "lifecycle") != "active" {
        return Err(Refusal::InvalidChange);
    }
    let mut after = created.clone();
    after.insert("lifecycle".into(), json!("archived"));
    after.insert("revision".into(), json!(next(&text(created, "revision"))?));
    if entity_type == EntityType::Node {
        let lifecycle = next(&text(created, "lifecycle_revision"))?;
        after.insert("lifecycle_revision".into(), json!(lifecycle));
        assertion::clear(&mut after);
    }
    Ok(planned(entity_type, Operation::Archive, created, after))
}

/// An updated, archived or unarchived entity, put back to its before-image
/// with its revisions moved forward.
fn restore(
    entity_type: EntityType,
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> Result<PlannedOp, Refusal> {
    let mut image = before.clone();
    image.insert("revision".into(), json!(next(&text(after, "revision"))?));
    let operation = match entity_type {
        EntityType::Content => Operation::Update,
        _ => transition(&text(after, "lifecycle"), &text(before, "lifecycle")),
    };
    if entity_type == EntityType::Node {
        let current = text(after, "lifecycle_revision");
        let lifecycle = if text(after, "lifecycle") == text(before, "lifecycle") {
            current
        } else {
            next(&current)?
        };
        image.insert("lifecycle_revision".into(), json!(lifecycle));
        assertion::clear(&mut image);
    }
    Ok(planned(entity_type, operation, after, image))
}

fn type_key(entity_type: EntityType) -> u8 {
    match entity_type {
        EntityType::Node => 0,
        EntityType::Edge => 1,
        EntityType::Tag => 2,
        EntityType::NodeTag => 3,
        EntityType::Content => 4,
        EntityType::Review => 5,
    }
}

/// The inverse of the retained accepted `event` (the module's rules).
pub fn plan(event: &Event) -> Result<Plan, Refusal> {
    event.check().map_err(|_| Refusal::InvalidChange)?;
    let header = &event.event;
    if header.descriptor == review::ACKNOWLEDGE {
        return Err(Refusal::InvalidChange);
    }
    let reversal = header.descriptor == REVERSE;
    let created: BTreeSet<&str> = event
        .ops
        .iter()
        .filter(|op| op.entity_type == EntityType::Node && op.operation == Operation::Create)
        .map(|op| op.entity_id.as_str())
        .collect();
    let mut seen = BTreeSet::new();
    let mut reviews = 0usize;
    let mut ops = Vec::with_capacity(event.ops.len());
    let mut retained = Vec::new();
    for op in event.ops.iter().rev() {
        if !seen.insert((type_key(op.entity_type), op.entity_id.as_str())) {
            return Err(Refusal::InvalidChange);
        }
        let after = op.after_value.as_ref();
        let before = op.before_value.as_ref();
        match (op.entity_type, op.operation) {
            (EntityType::Review, Operation::Create) => {
                let image = object(after)?;
                let own = reversal
                    && text(image, "kind") == REVERSAL
                    && header.reverses_event_id.as_deref()
                        == Some(text(image, "target_event_id").as_str());
                if !own {
                    return Err(Refusal::InvalidChange);
                }
                reviews += 1;
            }
            (EntityType::Tag, Operation::Create) => {
                retained.push(Value::Object(object(after)?.clone()))
            }
            (EntityType::Content, Operation::Create) => {
                let image = object(after)?;
                if !created.contains(text(image, "owner_node_id").as_str()) {
                    return Err(Refusal::InvalidChange);
                }
                retained.push(Value::Object(image.clone()));
            }
            (EntityType::Content, Operation::Update) => {
                ops.push(restore(
                    EntityType::Content,
                    object(before)?,
                    object(after)?,
                )?);
            }
            (EntityType::NodeTag, Operation::Create) => {
                let image = object(after)?;
                ops.push(PlannedOp {
                    entity_type: EntityType::NodeTag,
                    entity_id: Some(op.entity_id.clone()),
                    operation: Operation::Remove,
                    before_revision: None,
                    after_revision: None,
                    before_value: Some(Value::Object(image.clone())),
                    after_value: None,
                });
            }
            (EntityType::NodeTag, Operation::Remove) => {
                let image = object(before)?;
                ops.push(PlannedOp {
                    entity_type: EntityType::NodeTag,
                    entity_id: None,
                    operation: Operation::Create,
                    before_revision: None,
                    after_revision: None,
                    before_value: None,
                    after_value: Some(json!({
                        "id": null, "node_id": text(image, "node_id"), "tag_id": text(image, "tag_id"),
                    })),
                });
            }
            (EntityType::Node | EntityType::Edge, Operation::Create) => {
                ops.push(archive_created(op.entity_type, object(after)?)?);
            }
            (
                EntityType::Node | EntityType::Edge,
                Operation::Update | Operation::Archive | Operation::Unarchive,
            ) => ops.push(restore(op.entity_type, object(before)?, object(after)?)?),
            _ => return Err(Refusal::InvalidChange),
        }
    }
    if reviews != usize::from(reversal) || ops.is_empty() {
        return Err(Refusal::InvalidChange);
    }
    Ok(Plan {
        target_event_id: header.id.clone(),
        target_delta_digest: header.delta_digest.clone(),
        target_graph_seq: header.graph_seq.clone(),
        ops,
        retained,
    })
}

/// One entity a reversal requires at its current revisions.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    /// `content`, `edge` or `node`.
    pub entity_type: String,
    pub id: String,
    pub revision: String,
    /// A node's, and only a node's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_revision: Option<String>,
}

impl Expected {
    fn ok(&self) -> bool {
        let node = self.entity_type == "node";
        ["content", "edge", "node"].contains(&self.entity_type.as_str())
            && is_ulid(&self.id)
            && is_decimal(&self.revision)
            && self.revision != "0"
            && self.lifecycle_revision.is_some() == node
            && self
                .lifecycle_revision
                .as_deref()
                .is_none_or(|r| is_decimal(r) && r != "0")
    }
}

/// The current revisions `plan` requires: every planned node, edge and
/// content at its planned before-image (memberships carry none; their
/// node's revision covers them), ascending by (entity_type, id).
#[must_use]
pub fn expected(plan: &Plan) -> Vec<Expected> {
    let mut out: Vec<Expected> = plan
        .ops
        .iter()
        .filter_map(|op| {
            let before = op.before_value.as_ref()?.as_object()?;
            let entity_type = match op.entity_type {
                EntityType::Node => "node",
                EntityType::Edge => "edge",
                EntityType::Content => "content",
                _ => return None,
            };
            Some(Expected {
                entity_type: entity_type.to_owned(),
                id: text(before, "id"),
                revision: text(before, "revision"),
                lifecycle_revision: (op.entity_type == EntityType::Node)
                    .then(|| text(before, "lifecycle_revision")),
            })
        })
        .collect();
    out.sort();
    out
}

/// `given` against the plan's `required` list: the first entity, in
/// (entity_type, id) order, missing from, extra to or different in `given`
/// is the conflict detail `{entity, precondition, current_revision}` —
/// `lifecycle_revision` when only that differs, else `revision`, and the
/// plan's revision (null for an entity the plan does not name).
pub fn compare_expected(required: &[Expected], given: &[Expected]) -> Result<(), Value> {
    let detail = |e: &Expected, precondition: &str, current: Option<&str>| {
        json!({
            "entity": {"entity_type": e.entity_type, "id": e.id},
            "precondition": precondition, "current_revision": current,
        })
    };
    let key = |e: &Expected| (e.entity_type.clone(), e.id.clone());
    let (mut r, mut g) = (required.iter().peekable(), given.iter().peekable());
    loop {
        match (r.peek().copied(), g.peek().copied()) {
            (None, None) => return Ok(()),
            (Some(want), Some(got)) if key(want) == key(got) => {
                if want.revision != got.revision {
                    return Err(detail(want, "revision", Some(&want.revision)));
                }
                if want.lifecycle_revision != got.lifecycle_revision {
                    return Err(detail(
                        want,
                        "lifecycle_revision",
                        want.lifecycle_revision.as_deref(),
                    ));
                }
                r.next();
                g.next();
            }
            (Some(want), Some(got)) if key(want) > key(got) => {
                return Err(detail(got, "revision", None));
            }
            (None, Some(got)) => return Err(detail(got, "revision", None)),
            (Some(want), _) => return Err(detail(want, "revision", Some(&want.revision))),
        }
    }
}

/// A checked [`REVERSE`] payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reverse {
    pub target: Target,
    pub expected: Vec<Expected>,
    pub approval_id: Option<String>,
    pub event_time_ms: Option<u64>,
}

impl Reverse {
    /// Check a reversal's parts by the rules of [`parse_reverse`].
    pub fn new(
        target: Target,
        expected: Vec<Expected>,
        approval_id: Option<String>,
        event_time_ms: Option<u64>,
    ) -> Result<Self, Refusal> {
        let approval_ok = approval_id.as_deref().is_none_or(|id| {
            !id.is_empty() && id.len() <= MAX_APPROVAL_ID_BYTES && !id.chars().any(char::is_control)
        });
        if expected.len() > MAX_EXPECTED {
            return Err(Refusal::ChangeLimit);
        }
        if expected.is_empty()
            || !expected.iter().all(Expected::ok)
            || !expected.windows(2).all(|pair| {
                (&pair[0].entity_type, &pair[0].id) < (&pair[1].entity_type, &pair[1].id)
            })
            || !approval_ok
            || event_time_ms.is_some_and(|ms| ms > MAX_EVENT_TIME_MS)
        {
            return Err(Refusal::InvalidChange);
        }
        Ok(Self {
            target,
            expected,
            approval_id,
            event_time_ms,
        })
    }

    /// The reversal of what a preview answered: its target and its
    /// `expected` list.
    pub fn of(
        plan: &Plan,
        approval_id: Option<String>,
        event_time_ms: Option<u64>,
    ) -> Result<Self, Refusal> {
        Self::new(plan.target()?, expected(plan), approval_id, event_time_ms)
    }

    /// The payload [`parse_reverse`] reads back as this reversal.
    #[must_use]
    pub fn payload(&self) -> Value {
        let mut out = Map::new();
        out.insert("target_event_id".into(), json!(self.target.event_id));
        out.insert(
            "target_delta_digest".into(),
            json!(self.target.delta_digest),
        );
        out.insert("expected".into(), json!(self.expected));
        if let Some(id) = &self.approval_id {
            out.insert("approval_id".into(), json!(id));
        }
        if let Some(ms) = self.event_time_ms {
            out.insert("event_time_ms".into(), json!(ms));
        }
        Value::Object(out)
    }
}

/// Read a [`REVERSE`] payload. The `expected` count is checked before any
/// entry is read.
pub fn parse_reverse(payload: &Value) -> Result<Reverse, Refusal> {
    let fields = payload.as_object().ok_or(Refusal::InvalidChange)?;
    let known = [
        "target_event_id",
        "target_delta_digest",
        "expected",
        "approval_id",
        "event_time_ms",
    ];
    if fields.keys().any(|k| !known.contains(&k.as_str())) {
        return Err(Refusal::InvalidChange);
    }
    let text = |key: &str| fields.get(key).and_then(Value::as_str);
    let (Some(event_id), Some(digest)) = (text("target_event_id"), text("target_delta_digest"))
    else {
        return Err(Refusal::InvalidChange);
    };
    let Some(Value::Array(items)) = fields.get("expected") else {
        return Err(Refusal::InvalidChange);
    };
    if items.len() > MAX_EXPECTED {
        return Err(Refusal::ChangeLimit);
    }
    let expected = items
        .iter()
        .map(|item| serde_json::from_value(item.clone()).map_err(|_| Refusal::InvalidChange))
        .collect::<Result<Vec<Expected>, _>>()?;
    let approval_id = match fields.get("approval_id") {
        None => None,
        Some(Value::String(id)) => Some(id.clone()),
        Some(_) => return Err(Refusal::InvalidChange),
    };
    let event_time_ms = match fields.get("event_time_ms") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_u64().ok_or(Refusal::InvalidChange)?),
    };
    Reverse::new(
        Target::new(event_id, digest)?,
        expected,
        approval_id,
        event_time_ms,
    )
}

/// Execute `request` against the retained `event` it names and the current
/// graph: the event must be the one named (else `invalid_change`), its
/// [`plan`] must hold, [`apply_inverse`] must accept it, and `expected`
/// must be exactly the plan's ([`compare_expected`], a conflict). The
/// reverser is `stamp`'s actor.
pub fn reverse(
    state: &dyn Retained,
    event: &Event,
    request: &Reverse,
    stamp: &Stamp<'_>,
    mint: &mut dyn FnMut() -> String,
) -> Verdict {
    if event.event.id != request.target.event_id
        || event.event.delta_digest != request.target.delta_digest
    {
        return Verdict::Refused(Refusal::InvalidChange);
    }
    let plan = match plan(event) {
        Ok(plan) => plan,
        Err(refusal) => return Verdict::Refused(refusal),
    };
    match apply_inverse(state, &plan, stamp, mint) {
        Verdict::Accepted(effects) => match compare_expected(&expected(&plan), &request.expected) {
            Ok(()) => Verdict::Accepted(effects),
            Err(detail) => Verdict::Conflict(detail),
        },
        other => other,
    }
}

/// What a preview says a reversal would do now. Any other `outcome` on the
/// wire is unreadable ([`decode_preview`] refuses `unsupported_version`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewOutcome {
    /// The plan applies now.
    Ready,
    /// The plan applies now, but this caller, an agent, needs a per-call
    /// approval to reverse it (keeper-hq's answer only: a person never
    /// gets it, and [`preview`], which knows no caller, never gives it).
    ApprovalRequired,
    /// `detail` is the conflict, as [`apply_inverse`] reports it.
    Conflict,
    /// `reason` is the refusal code.
    Rejected,
}

/// What a reversal of one retained event would do now: [`crate::wire::PREVIEW_REVERSAL`]'s
/// one row. `plan` and `expected` are absent only when the event is not
/// reversible by shape. The planned operations name a restored membership
/// with a null id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub target_event_id: String,
    pub plan: Option<Plan>,
    pub expected: Vec<Expected>,
    pub outcome: PreviewOutcome,
    pub reason: Option<String>,
    pub detail: Option<Value>,
}

/// The preview of reversing `event` against `state` by `stamp`'s actor: the
/// same [`plan`] and [`apply_inverse`] a reversal runs, nothing written.
#[must_use]
pub fn preview(state: &dyn Retained, event: &Event, stamp: &Stamp<'_>) -> Preview {
    let target_event_id = event.event.id.clone();
    let plan = match plan(event) {
        Ok(plan) => plan,
        Err(refusal) => {
            return Preview {
                target_event_id,
                plan: None,
                expected: Vec::new(),
                outcome: PreviewOutcome::Rejected,
                reason: Some(refusal.code().to_owned()),
                detail: None,
            }
        }
    };
    // Nothing is minted for a preview: one fixed placeholder id.
    let mut mint = || "00000000000000000000000000".to_owned();
    let (outcome, reason, detail) = match apply_inverse(state, &plan, stamp, &mut mint) {
        Verdict::Accepted(_) => (PreviewOutcome::Ready, None, None),
        Verdict::Conflict(detail) => (PreviewOutcome::Conflict, None, Some(detail)),
        Verdict::Refused(refusal) => (
            PreviewOutcome::Rejected,
            Some(refusal.code().to_owned()),
            None,
        ),
    };
    Preview {
        target_event_id,
        expected: expected(&plan),
        plan: Some(plan),
        outcome,
        reason,
        detail,
    }
}

/// Decode [`crate::wire::PREVIEW_REVERSAL`]'s answer for `event_id`: exactly
/// one row naming that event, its `expected` the plan's, and `outcome`,
/// `reason` and `detail` agreeing (a reason exactly when rejected, a detail
/// exactly when a conflict, a plan whenever ready or approval-required, no
/// plan only when rejected). Anything else, an unknown outcome included,
/// refuses `unsupported_version`.
pub fn decode_preview(event_id: &str, rows: Vec<Map<String, Value>>) -> Result<Preview, Refusal> {
    let mut rows = rows.into_iter();
    let (Some(row), None) = (rows.next(), rows.next()) else {
        return Err(Refusal::UnsupportedVersion);
    };
    let preview: Preview =
        serde_json::from_value(Value::Object(row)).map_err(|_| Refusal::UnsupportedVersion)?;
    let shaped = match (preview.outcome, &preview.reason, &preview.detail) {
        (PreviewOutcome::Ready | PreviewOutcome::ApprovalRequired, None, None) => {
            preview.plan.is_some()
        }
        (PreviewOutcome::Conflict, None, Some(Value::Object(_))) => preview.plan.is_some(),
        (PreviewOutcome::Rejected, Some(reason), None) => {
            crate::vocabulary::ERRORS.contains(&reason.as_str())
        }
        _ => false,
    };
    let planned = match &preview.plan {
        Some(plan) => {
            plan.target_event_id == event_id
                && plan.target().is_ok()
                && expected(plan) == preview.expected
        }
        None => preview.expected.is_empty(),
    };
    if !shaped || !planned || preview.target_event_id != event_id {
        return Err(Refusal::UnsupportedVersion);
    }
    Ok(preview)
}
