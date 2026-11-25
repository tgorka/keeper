//! A file's annotation (story 116.5, T3): "this entry, this note, these
//! tags" as one ordinary `graph.apply` change on the one node that points at
//! the file by its entry id (the contract README's **Files**).
//!
//! - [`plan`] is pure: given what the graph holds for the entry
//!   ([`Existing`]) it writes the change. No node: `create_node` of a
//!   `Resource` titled as the caller says, whose `internal` `$locator` is
//!   `files:<domain>/<entry_id>`, then the note (`create_content`,
//!   `inline_text`) and the tags (`add_tag`). An archived node: the same
//!   change unarchives it first. An active node: the note and tags only.
//! - The title is the caller's to compose (AC17, L-7, M-1): a person's
//!   first annotation takes the file's name ([`title_of`]); a composer for
//!   an approval-free agent or tool call under the untrusted globs passes
//!   its own fixed title. Nothing here reads a zone.
//! - [`annotate`] is a person's path over a [`GraphClient`]: it asks the
//!   server what the entry is (`files.resolve`) and which node holds it
//!   (`files.resource_of`, then `graph.get_node`), plans, applies, and when
//!   the change is a `conflict` on [`file_ref::PRECONDITION`] — another
//!   client created the node first (AC9) — plans once more onto the node
//!   the conflict names and applies that.
//!
//! One entry per call: no bulk path exists here (P-11, AI-KG1-03).

use serde_json::{json, Map, Value};

use crate::client::{Applied, GraphClient, GraphClientError};
use crate::outcome::GraphOutcome;
use crate::protocol::change::ChangeOp;
use crate::protocol::{file_ref, is_ulid, locator, Refusal};

/// The server's remote query naming the node that holds an entry.
pub const RESOURCE_OF: &str = "files.resource_of";
/// The server's remote query resolving an `internal` locator to its entry.
pub const RESOLVE: &str = "files.resolve";
/// The kind of the node an annotation creates.
pub const RESOURCE: &str = "Resource";

/// [`RESOURCE_OF`]'s params for `entry_id`.
#[must_use]
pub fn resource_of_params(entry_id: &str) -> Value {
    json!({"version": 1, "entry_id": entry_id})
}

/// [`RESOLVE`]'s params for the locator text `locator`.
#[must_use]
pub fn resolve_params(locator: &str) -> Value {
    json!({"version": 1, "locator": locator})
}

/// What the graph holds for an entry, as the plan reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Existing {
    /// No node counts for the entry: the plan creates one.
    None,
    /// The node that holds the entry, its accepted revision, and whether it
    /// is active (else archived, and the plan unarchives it).
    Node {
        id: String,
        revision: String,
        active: bool,
    },
}

/// One annotation: this entry of `domain`, this note, these tags, and the
/// title a node created for it takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Annotation {
    pub domain: String,
    pub entry_id: String,
    pub title: String,
    pub note: Option<String>,
    pub tags: Vec<String>,
}

/// The change an annotation is: its operations and the node they land on.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub node_id: String,
    /// Whether the change creates that node.
    pub created: bool,
    pub ops: Vec<ChangeOp>,
}

/// The `$locator` envelope naming `entry_id` of `domain`.
fn envelope(domain: &str, entry_id: &str) -> Value {
    json!({
        "kind": "internal",
        "management": "managed",
        "locator": file_ref::text(domain, entry_id),
        "selector": null,
        "version": null,
        "format": null,
    })
}

/// Plan `annotation` over `existing`; `mint` gives each id the change
/// creates (the node's first, then the note's). Refused `invalid_change`
/// for an entry id that is no ULID, an empty title, or nothing to write.
pub fn plan(
    annotation: &Annotation,
    existing: &Existing,
    mint: &mut dyn FnMut() -> String,
) -> Result<Plan, Refusal> {
    if !is_ulid(&annotation.entry_id) || (annotation.note.is_none() && annotation.tags.is_empty()) {
        return Err(Refusal::InvalidChange);
    }
    let mut ops = Vec::new();
    let (node_id, created, revision) = match existing {
        Existing::None => {
            if annotation.title.is_empty() {
                return Err(Refusal::InvalidChange);
            }
            let id = mint();
            let mut properties = Map::new();
            properties.insert(
                locator::MEMBER.to_owned(),
                envelope(&annotation.domain, &annotation.entry_id),
            );
            ops.push(ChangeOp::CreateNode {
                id: id.clone(),
                kind: RESOURCE.to_owned(),
                title: annotation.title.clone(),
                description: None,
                properties,
            });
            (id, true, None)
        }
        Existing::Node {
            id,
            revision,
            active,
        } => {
            if !active {
                ops.push(ChangeOp::UnarchiveNode {
                    id: id.clone(),
                    expected_revision: revision.clone(),
                });
            }
            (id.clone(), false, Some(revision.clone()))
        }
    };
    if let Some(note) = &annotation.note {
        ops.push(ChangeOp::CreateContent {
            id: mint(),
            owner_node_id: node_id.clone(),
            body: json!({"inline_text": note}),
        });
    }
    for tag in &annotation.tags {
        ops.push(ChangeOp::AddTag {
            node_id: node_id.clone(),
            tag: tag.clone(),
            expected_revision: revision.clone(),
        });
    }
    Ok(Plan {
        node_id,
        created,
        ops,
    })
}

/// What `existing` the graph holds for `entry_id`, from
/// [`RESOURCE_OF`]'s answer row `found` and, when it names a node, that
/// node's image (`graph.get_node` with archived ones). A node that no longer
/// names the entry, or one merged away (whose survivor the plan never
/// follows, AC15), holds nothing for it: the plan creates a node.
#[must_use]
pub fn existing(
    entry_id: &str,
    found: &Map<String, Value>,
    node: Option<&Map<String, Value>>,
) -> Existing {
    let text =
        |m: &Map<String, Value>, k: &str| m.get(k).and_then(Value::as_str).map(str::to_owned);
    if text(found, "status").as_deref() != Some("found") {
        return Existing::None;
    }
    let Some(image) = node else {
        return Existing::None;
    };
    let names = file_ref::named(image).is_some_and(|t| t.entry_id == entry_id);
    let merged = image.get("merged_into_id").is_some_and(|v| !v.is_null());
    match (
        text(image, "id"),
        text(image, "revision"),
        text(image, "lifecycle"),
    ) {
        (Some(id), Some(revision), Some(lifecycle))
            if names && !merged && text(found, "node_id").as_deref() == Some(id.as_str()) =>
        {
            Existing::Node {
                id,
                revision,
                active: lifecycle == "active",
            }
        }
        _ => Existing::None,
    }
}

/// The node a `conflict` detail says already holds the entry (the
/// contract's [`file_ref::PRECONDITION`] answer: `{entity: {entity_type:
/// "node", id}, precondition, current_revision}`), active at that revision.
/// Any other conflict: `None`.
#[must_use]
pub fn retry(detail: &Map<String, Value>) -> Option<Existing> {
    if detail.get("precondition").and_then(Value::as_str) != Some(file_ref::PRECONDITION) {
        return None;
    }
    let entity = detail.get("entity")?;
    if entity.get("entity_type").and_then(Value::as_str) != Some("node") {
        return None;
    }
    let id = entity.get("id").and_then(Value::as_str)?;
    let revision = detail.get("current_revision").and_then(Value::as_str)?;
    is_ulid(id).then(|| Existing::Node {
        id: id.to_owned(),
        revision: revision.to_owned(),
        active: true,
    })
}

/// The file's name from [`RESOLVE`]'s answer row: the last segment of its
/// drive-relative `path`. `None` unless the entry resolved to a non-empty
/// name.
#[must_use]
pub fn title_of(resolved: &Map<String, Value>) -> Option<String> {
    if resolved.get("status").and_then(Value::as_str) != Some("resolved") {
        return None;
    }
    let path = resolved.get("path").and_then(Value::as_str)?;
    let name = path.rsplit('/').next().unwrap_or(path);
    (!name.is_empty()).then(|| name.to_owned())
}

/// Why an annotation was not sent.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum AnnotateError {
    /// The input or the change is outside the contract.
    #[error("refused: {0:?}")]
    Refused(Refusal),
    /// The server has no such entry for this person (`missing`).
    #[error("no entry {0} in this domain")]
    Missing(String),
    #[error(transparent)]
    Client(#[from] GraphClientError),
}

/// What [`annotate`] did: the node it landed on, whether it created it,
/// whether it planned again after a `file_ref` conflict, and the last
/// change's [`Applied`].
#[derive(Debug, PartialEq)]
pub struct Annotated {
    pub node_id: String,
    pub created: bool,
    pub retried: bool,
    pub applied: Applied,
}

fn single(rows: Vec<Map<String, Value>>) -> Result<Map<String, Value>, AnnotateError> {
    let mut rows = rows.into_iter();
    match (rows.next(), rows.next()) {
        (Some(row), None) => Ok(row),
        _ => Err(GraphClientError::Unreadable(Refusal::UnsupportedVersion).into()),
    }
}

/// A person's annotation of `entry_id` in `client`'s domain: resolve, look
/// up, plan, apply; on a `file_ref` conflict, once more onto the node it
/// names.
pub fn annotate(
    client: &GraphClient,
    entry_id: &str,
    note: Option<String>,
    tags: Vec<String>,
) -> Result<Annotated, AnnotateError> {
    if !is_ulid(entry_id) || (note.is_none() && tags.is_empty()) {
        return Err(AnnotateError::Refused(Refusal::InvalidChange));
    }
    let domain = client.domain().to_owned();
    let resolved = single(client.resolve_file(&file_ref::text(&domain, entry_id))?)?;
    if resolved.get("status").and_then(Value::as_str) != Some("resolved") {
        return Err(AnnotateError::Missing(entry_id.to_owned()));
    }
    // A drive's root has no name: it is no file to annotate.
    let Some(title) = title_of(&resolved) else {
        return Err(AnnotateError::Refused(Refusal::InvalidChange));
    };
    let found = single(client.resource_of(entry_id)?)?;
    let node = match found.get("node_id").and_then(Value::as_str) {
        Some(id) => client.get_node(id, true)?.map(|view| view.image),
        None => None,
    };
    let annotation = Annotation {
        domain,
        entry_id: entry_id.to_owned(),
        title,
        note,
        tags,
    };
    let mut mint = || ulid::Ulid::new().to_string();
    let first = existing(entry_id, &found, node.as_ref());
    let (plan_now, applied) = send(client, &annotation, &first, &mut mint)?;
    // Another client's node for the entry (AC9; or one restored while this
    // change unarchived its own): plan once more onto it.
    let conflict = match &applied.outcome {
        Ok(GraphOutcome::Conflict { detail }) => retry(detail)
            .filter(|again| !matches!(again, Existing::Node { id, .. } if *id == plan_now.node_id)),
        _ => None,
    };
    let Some(again) = conflict else {
        return Ok(Annotated {
            node_id: plan_now.node_id,
            created: plan_now.created,
            retried: false,
            applied,
        });
    };
    let (plan_again, applied) = send(client, &annotation, &again, &mut mint)?;
    Ok(Annotated {
        node_id: plan_again.node_id,
        created: plan_again.created,
        retried: true,
        applied,
    })
}

fn send(
    client: &GraphClient,
    annotation: &Annotation,
    existing: &Existing,
    mint: &mut dyn FnMut() -> String,
) -> Result<(Plan, Applied), AnnotateError> {
    let plan = plan(annotation, existing, mint).map_err(AnnotateError::Refused)?;
    let change = client
        .change(plan.ops.clone(), None)
        .map_err(AnnotateError::Refused)?;
    let applied = client.apply(&change)?;
    Ok((plan, applied))
}
