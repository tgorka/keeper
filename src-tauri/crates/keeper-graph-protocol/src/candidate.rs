//! What one change set does to a graph: the shared acceptance algebra
//! (contract §4, §9.5, §9.6). Pure: the graph is read through [`State`]
//! (the accepted, pre-change state), nothing is written, and ids the server
//! mints (tags, memberships) come from the caller's `mint`; the acceptance
//! time and the authenticated actor a verification is stamped with come
//! from the caller's [`Stamp`].
//!
//! Operations apply in order to a candidate; then the whole final candidate
//! is validated. The verdict is exactly one of:
//!
//! - [`Verdict::Accepted`]: the event's operations (ordinal = their order),
//!   its delta digest, and the affected entities with their new revisions;
//! - [`Verdict::Conflict`]: a precondition named a revision the graph no
//!   longer has (or an entity it does not have, or one a create finds);
//! - [`Verdict::Refused`]: `invalid_change` or `change_limit`.
//!
//! Checks run in this order, the first failure deciding: each operation in
//! turn (its preconditions, then what it does); then every typed reference
//! written; then every edge left active that this change created,
//! unarchived or updated (both endpoints active, and a `guidance_scope`
//! edge starting at a `Guidance` node); then containment for every
//! `contains` edge created, unarchived or rewritten by a merge; then the
//! final candidate ([`Candidate::finish`]: every node written within its
//! kind's reserved-member rules, description provenance, content owners
//! and reference bodies, and the verification rules); then the event's
//! bounds ([`crate::delta::digest`]). The receipt's `affected` lists nodes,
//! edges, new tags and contents in order of their first touch by the
//! change, which may differ from the order of their event operations.
//!
//! **Revisions.** A created node or edge starts at revision 1 (a node also at
//! lifecycle revision 1, active); a new tag or content at revision 1. A node
//! touched by this change in any way — fields, tag memberships, lifecycle,
//! a merge absorbing it, its verification — moves its revision by exactly
//! one; an archive or unarchive (and a merge's archive) also moves its
//! lifecycle revision by one. An edge operation moves the edge's revision by
//! one and never its endpoints'. A content update moves only the content's
//! revision (never its owner's).
//!
//! **Event operations,** one or more per change operation, in its order:
//! `create_node`, `update_node`, `archive_node`, `unarchive_node` give one
//! node operation of that name; `verify_assertion` one node `update`; the
//! first tag operation of a node that no explicit operation of this change
//! names gives one node `update` (its revision move) before its own;
//! `add_tag` gives a tag `create` when no tag has the text yet, then a
//! membership `create`; `remove_tag` a membership `remove`; an edge
//! operation one edge operation of its name; `create_content` and
//! `update_content` one content operation of that name; `merge_nodes` one
//! node `archive` per absorbed node (in its order, `merged_into_id` the
//! target), then one edge `update` per listed edge (in its order, each
//! absorbed endpoint rewritten to the target). A node, edge or content image
//! is the entity's whole row as it is before the change and after all of it
//! (never an intermediate state): node `{id, kind, title, description,
//! description_source_id, properties, lifecycle, merged_into_id, revision,
//! lifecycle_revision}`, edge `{id, from_id, to_id, type, properties,
//! lifecycle, revision}`, tag `{id, text, label, revision}`, membership
//! `{id, node_id, tag_id}`, content `{id, owner_node_id, revision, body}`.
//! A new tag's `label` is null.
//!
//! **Containment.** The active `contains` edges of the final candidate stay
//! acyclic. For each `contains` edge `P → C` created, unarchived or
//! rewritten, the walk goes up from `P` one level at a time: a level is
//! every active `contains` edge of the final candidate into the current
//! frontier; a level past [`MAX_CONTAINMENT_DEPTH`], or more than
//! [`MAX_CONTAINMENT_EDGES`] edges read by all walks of the change together,
//! refuses `change_limit`; a level holding an edge from `C` (or `P` = `C`)
//! refuses `invalid_change`; the frontier is then the level's new `from`
//! nodes. Reads are bounded as they are made: a level is fetched with a
//! limit of one more edge than the budget still allows
//! ([`State::contains_into`]), never whole.
//!
//! **Merge.** The target is active, unmerged and at `target_revision`; each
//! absorbed node active at its `expected_revision`; `edges` exactly the
//! active edges incident to any absorbed node ([`exact_incident_edges`]),
//! each under its `update_edge` preconditions taken from the pre-merge
//! endpoints — any difference is a conflict. Refused (`invalid_change`): a
//! `Guidance` or `Vocabulary` target or absorbed node, one carrying
//! `$assertion`, a rewritten `guidance_scope` edge, a rewrite that would
//! make a self-loop ([`rewrite_endpoints`]), and a resulting redirect chain
//! with a cycle ([`redirect_depth`]); over [`MAX_REDIRECT_HOPS`] hops refuses
//! `change_limit`. Parallel edges keep their ids; archived incident edges,
//! tag memberships, contents and typed references stay with the absorbed
//! node.
//!
//! **Verification** ([`crate::assertion`]). `verify_assertion` is accepted
//! only against the exact checked state: the node at `expected_revision`,
//! active and carrying `$assertion`; `sources` exactly its active outgoing
//! `sources` edges (each at its revision, its typed reference naming
//! `ref_id` at `ref_revision`, which is that entity's current revision); and
//! `contents` exactly the contents it owns (each at its revision). A
//! difference is a conflict; no active `sources` edge, a reference that is
//! absent, null-revisioned or does not resolve, an archived target node, or
//! more than 64 of either refuses. After it, every other write to a node
//! verified in the pre-state clears its verification in the same operation;
//! a write to such a node's `sources` edges or contents refuses unless the
//! same change holds an `update_node` of it; and the verification is the
//! only operation of its change touching the node, its contents or its
//! outgoing `sources` edges. A change may verify several assertions; each
//! ends verified. No operation of a verifying change may write a node or
//! content one of its checked `sources` references (another verification
//! included): that verification would be stale on acceptance, so the
//! change refuses `invalid_change`.
//!
//! **Reversal.** [`apply_inverse`] is the second entry point: it judges a
//! server-derived inverse ([`crate::inverse`]) through the same per-entity
//! preconditions and whole-candidate checks, plus the reversal-only
//! dependency rule and its `dependency` conflict precondition. Nothing a
//! [`Change`] can name reaches it.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use crate::assertion::{self, MAX_CHECKED_CONTENTS, MAX_CHECKED_SOURCES};
use crate::change::{Change, ChangeOp, CheckedContent, CheckedSource, MergeEdge};
use crate::content::{revise, Revision};
use crate::delta::{self, Op};
use crate::value::{check_node_properties, check_properties, typed_ref, RefType};
use crate::vocabulary::{EntityType, Operation};
use crate::Refusal;

/// The most levels a containment walk may climb.
pub const MAX_CONTAINMENT_DEPTH: usize = 64;
/// The most `contains` edges the containment walks of one change may read.
pub const MAX_CONTAINMENT_EDGES: usize = 10_000;
/// The most `merged_into_id` hops a redirect chain may have.
pub const MAX_REDIRECT_HOPS: usize = 8;
/// The most redirected nodes the redirect walks of one merge may read.
pub const MAX_REDIRECT_READS: usize = 10_000;

/// The accepted graph a change is judged against. Every answer is an image
/// in the shapes above; every list is ascending by id and **at most
/// `limit`** long, so a full answer proves a bound exceeded without reading
/// the rest.
pub trait State {
    /// The node with `id`, any lifecycle.
    fn node(&self, id: &str) -> Option<Map<String, Value>>;
    /// The edge with `id`, any lifecycle.
    fn edge(&self, id: &str) -> Option<Map<String, Value>>;
    /// The tag whose canonical text is `text`.
    fn tag(&self, text: &str) -> Option<Map<String, Value>>;
    /// The membership of node `node_id` in tag `tag_id`.
    fn membership(&self, node_id: &str, tag_id: &str) -> Option<Map<String, Value>>;
    /// The accepted active `contains` edges whose `to_id` is one of `to_ids`.
    /// The walk asks for one more edge than it may still read (plus any it
    /// will replace with this change's own).
    fn contains_into(&self, to_ids: &[String], limit: usize) -> Vec<Map<String, Value>>;
    /// The content with `id`.
    fn content(&self, id: &str) -> Option<Map<String, Value>>;
    /// The contents owned by node `owner_id` (`kg_contents_owner`).
    fn contents_of(&self, owner_id: &str, limit: usize) -> Vec<Map<String, Value>>;
    /// The active `sources` edges from node `from_id`
    /// (`kg_edges_from_active`).
    fn sources_from(&self, from_id: &str, limit: usize) -> Vec<Map<String, Value>>;
    /// The active edges with `from_id` or `to_id` one of `node_ids`.
    fn incident_active(&self, node_ids: &[String], limit: usize) -> Vec<Map<String, Value>>;
    /// The nodes whose `merged_into_id` is one of `node_ids`
    /// (`kg_nodes_merged_into_id`).
    fn merged_into(&self, node_ids: &[String], limit: usize) -> Vec<Map<String, Value>>;
}

impl<S: State + ?Sized> assertion::Current for S {
    fn node(&self, id: &str) -> Option<Map<String, Value>> {
        State::node(self, id)
    }
    fn content(&self, id: &str) -> Option<Map<String, Value>> {
        State::content(self, id)
    }
    fn sources_from(&self, id: &str, limit: usize) -> Vec<Map<String, Value>> {
        State::sources_from(self, id, limit)
    }
}

/// What the server stamps a verification with: its acceptance time and the
/// authenticated actor, never a payload's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp<'a> {
    pub time_ms: u64,
    pub actor_id: &'a str,
}

/// What the change does.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Accepted(Effects),
    /// The receipt's detail: `{entity: {entity_type, id}, precondition:
    /// "revision" | "lifecycle_revision", current_revision: <decimal> | null}`.
    Conflict(Value),
    Refused(Refusal),
}

/// An accepted change's event.
#[derive(Clone, Debug, PartialEq)]
pub struct Effects {
    pub ops: Vec<Op>,
    pub delta_digest: String,
    /// The receipt's `affected`: `{entity_type, id, revision}` (a node's
    /// with `lifecycle_revision`) for each node, edge, new tag and content,
    /// in order of their first operation.
    pub affected: Vec<Value>,
}

impl Effects {
    /// The accepted receipt's detail.
    #[must_use]
    pub fn detail(&self) -> Value {
        json!({ "affected": self.affected })
    }
}

/// Judge `change` against `state`.
pub fn apply(
    state: &dyn State,
    change: &Change,
    stamp: &Stamp<'_>,
    mint: &mut dyn FnMut() -> String,
) -> Verdict {
    let mut candidate = Candidate::new(state, change);
    match candidate.run(change, stamp, mint) {
        Ok(effects) => Verdict::Accepted(effects),
        Err(Stop::Conflict(detail)) => Verdict::Conflict(detail),
        Err(Stop::Refused(refusal)) => Verdict::Refused(refusal),
    }
}

enum Stop {
    Conflict(Value),
    Refused(Refusal),
}

impl From<Refusal> for Stop {
    fn from(refusal: Refusal) -> Self {
        Self::Refused(refusal)
    }
}

type Image = Map<String, Value>;

/// One entity in the candidate: its accepted image (none when created
/// here) and its current one.
struct Entry {
    before: Option<Image>,
    after: Image,
    /// Its revision has moved in this change.
    touched: bool,
}

enum Slot {
    Node(String, Operation),
    Edge(String, Operation),
    Tag(String),
    Membership(Operation, Image),
    Content(String, Operation),
    Review(Image),
}

struct Candidate<'a> {
    state: &'a dyn State,
    nodes: BTreeMap<String, Entry>,
    edges: BTreeMap<String, Entry>,
    contents: BTreeMap<String, Entry>,
    /// New tags by canonical text.
    tags: BTreeMap<String, Image>,
    /// Nodes an explicit operation of the change names.
    explicit: BTreeSet<String>,
    /// Nodes an `update_node` of the change names.
    updated: BTreeSet<String>,
    /// Nodes whose tag-only `update` has been emitted.
    emitted: BTreeSet<String>,
    /// The nodes the change's `verify_assertion`s verify.
    verified: BTreeSet<String>,
    /// The entities the verified assertions' checked `sources` reference.
    checked_targets: Vec<(RefType, String)>,
    /// Values written, to check their typed references.
    written: Vec<Value>,
    /// Edges whose endpoints must be active (if the edge is), in order.
    endpoint_checks: Vec<String>,
    /// `contains` edges to walk, in order.
    containment: Vec<String>,
    /// (node, source) of each generated description.
    provenance: Vec<(String, String)>,
    slots: Vec<Slot>,
    /// Nodes, edges, new tags (by text) and contents in order of first
    /// touch: the receipt's `affected` order.
    order: Vec<(&'static str, String)>,
}

fn text(image: &Image, key: &str) -> String {
    image
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn next(revision: &str) -> Result<String, Stop> {
    let next = revision
        .parse::<u128>()
        .ok()
        .and_then(|n| n.checked_add(1))
        .map(|n| n.to_string())
        .filter(|n| n.len() <= 20)
        .ok_or(Refusal::InvalidChange)?;
    Ok(next)
}

fn conflict(entity_type: &str, id: &str, precondition: &str, current: Option<String>) -> Stop {
    Stop::Conflict(conflict_detail(entity_type, id, precondition, current))
}

fn conflict_detail(
    entity_type: &str,
    id: &str,
    precondition: &str,
    current: Option<String>,
) -> Value {
    json!({
        "entity": { "entity_type": entity_type, "id": id },
        "precondition": precondition,
        "current_revision": current,
    })
}

fn revision_le(a: &str, b: &str) -> bool {
    matches!((a.parse::<u128>(), b.parse::<u128>()), (Ok(a), Ok(b)) if a <= b)
}

/// A node kind no merge may absorb or target in R1.
fn unmergeable(node: &Image) -> bool {
    matches!(text(node, "kind").as_str(), "Guidance" | "Vocabulary")
        || assertion::of(node).is_some()
}

/// Check that `listed` is exactly the active edges incident to `node_ids`
/// in `state`: the first difference is the conflict detail (an unlisted
/// edge with its revision, or a listed one that is not an active incident
/// edge with its revision or null). Reads at most `listed.len() + 1` edges.
pub fn exact_incident_edges(
    state: &dyn State,
    node_ids: &[String],
    listed: &[String],
) -> Result<(), Value> {
    let wanted: BTreeSet<&str> = listed.iter().map(String::as_str).collect();
    let found = state.incident_active(node_ids, listed.len().saturating_add(1));
    let mut seen = BTreeSet::new();
    for edge in &found {
        let id = text(edge, "id");
        if !wanted.contains(id.as_str()) {
            return Err(conflict_detail(
                "edge",
                &id,
                "revision",
                Some(text(edge, "revision")),
            ));
        }
        seen.insert(id);
    }
    for id in listed {
        if !seen.contains(id) {
            let current = state.edge(id).map(|e| text(&e, "revision"));
            return Err(conflict_detail("edge", id, "revision", current));
        }
    }
    Ok(())
}

/// The edge image with every endpoint in `absorbed` rewritten to `target`;
/// `None` when the result would be a self-loop.
#[must_use]
pub fn rewrite_endpoints(edge: &Image, absorbed: &BTreeSet<String>, target: &str) -> Option<Image> {
    let mut out = edge.clone();
    for end in ["from_id", "to_id"] {
        if absorbed.contains(&text(edge, end)) {
            out.insert(end.into(), json!(target));
        }
    }
    (text(&out, "from_id") != text(&out, "to_id")).then_some(out)
}

/// The deepest redirect chain ending at any of `roots`: the most
/// `merged_into_id` hops from a node into one of them (0 when none
/// redirects there). Walked a level at a time over
/// [`State::merged_into`], at most [`MAX_REDIRECT_HOPS`] levels and
/// [`MAX_REDIRECT_READS`] nodes (`change_limit` past either); a node met
/// twice is a cycle (`invalid_change`).
pub fn redirect_depth(state: &dyn State, roots: &[String]) -> Result<usize, Refusal> {
    let mut seen: BTreeSet<String> = roots.iter().cloned().collect();
    let mut frontier = roots.to_vec();
    let mut reads = 0usize;
    let mut depth = 0;
    while !frontier.is_empty() {
        let level = state.merged_into(
            &frontier,
            MAX_REDIRECT_READS.saturating_sub(reads).saturating_add(1),
        );
        if level.is_empty() {
            break;
        }
        depth += 1;
        reads += level.len();
        if depth > MAX_REDIRECT_HOPS || reads > MAX_REDIRECT_READS {
            return Err(Refusal::ChangeLimit);
        }
        frontier = Vec::with_capacity(level.len());
        for node in level {
            let id = text(&node, "id");
            if !seen.insert(id.clone()) {
                return Err(Refusal::InvalidChange);
            }
            frontier.push(id);
        }
    }
    Ok(depth)
}

/// The redirect chain from node `id`: the ids its `merged_into_id` links
/// lead through, in order, ending at the first unmerged node. More than
/// [`MAX_REDIRECT_HOPS`] hops refuses `change_limit`, a cycle or a missing
/// node `invalid_change`. Inspection reports it; it never substitutes the
/// survivor for `id`.
pub fn redirect_chain(state: &dyn State, id: &str) -> Result<Vec<String>, Refusal> {
    let mut chain = Vec::new();
    let mut seen = BTreeSet::from([id.to_owned()]);
    let mut node = state.node(id).ok_or(Refusal::InvalidChange)?;
    while let Some(next) = node.get("merged_into_id").and_then(Value::as_str) {
        if chain.len() == MAX_REDIRECT_HOPS {
            return Err(Refusal::ChangeLimit);
        }
        if !seen.insert(next.to_owned()) {
            return Err(Refusal::InvalidChange);
        }
        chain.push(next.to_owned());
        node = state.node(next).ok_or(Refusal::InvalidChange)?;
    }
    Ok(chain)
}

impl<'a> Candidate<'a> {
    fn new(state: &'a dyn State, change: &Change) -> Self {
        let mut candidate = Self::empty(state);
        for op in change.ops() {
            match op {
                ChangeOp::UpdateNode { id, .. } => {
                    candidate.explicit.insert(id.clone());
                    candidate.updated.insert(id.clone());
                }
                ChangeOp::CreateNode { id, .. }
                | ChangeOp::ArchiveNode { id, .. }
                | ChangeOp::UnarchiveNode { id, .. }
                | ChangeOp::VerifyAssertion { id, .. } => {
                    candidate.explicit.insert(id.clone());
                }
                _ => {}
            }
        }
        candidate
    }

    fn empty(state: &'a dyn State) -> Self {
        Self {
            state,
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            contents: BTreeMap::new(),
            tags: BTreeMap::new(),
            explicit: BTreeSet::new(),
            updated: BTreeSet::new(),
            emitted: BTreeSet::new(),
            verified: BTreeSet::new(),
            checked_targets: Vec::new(),
            written: Vec::new(),
            endpoint_checks: Vec::new(),
            containment: Vec::new(),
            provenance: Vec::new(),
            slots: Vec::new(),
            order: Vec::new(),
        }
    }

    fn run(
        &mut self,
        change: &Change,
        stamp: &Stamp<'_>,
        mint: &mut dyn FnMut() -> String,
    ) -> Result<Effects, Stop> {
        for (index, op) in change.ops().iter().enumerate() {
            self.op(op, change.canonical_tag(index), stamp, mint)?;
        }
        for value in std::mem::take(&mut self.written) {
            self.references(&value)?;
        }
        for id in std::mem::take(&mut self.endpoint_checks) {
            self.endpoints_active(&id)?;
        }
        let mut budget = 0;
        for id in std::mem::take(&mut self.containment) {
            self.acyclic(&id, &mut budget)?;
        }
        self.finish()?;
        self.effects()
    }

    /// Load node `id` into the candidate; `None` when the graph has none.
    fn node(&mut self, id: &str) -> Option<&mut Entry> {
        if !self.nodes.contains_key(id) {
            let image = self.state.node(id)?;
            self.nodes.insert(
                id.to_owned(),
                Entry {
                    before: Some(image.clone()),
                    after: image,
                    touched: false,
                },
            );
        }
        self.nodes.get_mut(id)
    }

    fn edge(&mut self, id: &str) -> Option<&mut Entry> {
        if !self.edges.contains_key(id) {
            let image = self.state.edge(id)?;
            self.edges.insert(
                id.to_owned(),
                Entry {
                    before: Some(image.clone()),
                    after: image,
                    touched: false,
                },
            );
        }
        self.edges.get_mut(id)
    }

    /// Record the first touch of an entity, for `affected`.
    fn first(&mut self, kind: &'static str, id: &str) {
        if !self.order.iter().any(|(k, i)| *k == kind && i == id) {
            self.order.push((kind, id.to_owned()));
        }
    }

    /// Node `id` under precondition `expected` (`None`: created earlier in
    /// this change); moves its revision once.
    fn touch_node(&mut self, id: &str, expected: Option<&String>) -> Result<(), Stop> {
        let Some(expected) = expected else {
            self.first("node", id);
            return Ok(());
        };
        let Some(entry) = self.node(id) else {
            return Err(conflict("node", id, "revision", None));
        };
        if !entry.touched {
            let before = entry.before.as_ref().map(|b| text(b, "revision"));
            if before.as_ref() != Some(expected) {
                return Err(conflict("node", id, "revision", before));
            }
            let revision = next(expected)?;
            entry.after.insert("revision".into(), json!(revision));
            entry.touched = true;
        }
        self.first("node", id);
        Ok(())
    }

    /// An endpoint precondition: node `id`'s accepted lifecycle revision.
    fn endpoint(&mut self, id: &str, expected: Option<&String>) -> Result<(), Stop> {
        let Some(expected) = expected else {
            return Ok(());
        };
        let Some(entry) = self.node(id) else {
            return Err(conflict("node", id, "lifecycle_revision", None));
        };
        let current = entry.before.as_ref().map(|b| text(b, "lifecycle_revision"));
        if current.as_ref() != Some(expected) {
            return Err(conflict("node", id, "lifecycle_revision", current));
        }
        Ok(())
    }

    /// An existing edge `id` under its preconditions; moves its revision.
    fn touch_edge(
        &mut self,
        id: &str,
        expected: &str,
        from_lifecycle: &str,
        to_lifecycle: &str,
    ) -> Result<(), Stop> {
        let Some(entry) = self.edge(id) else {
            return Err(conflict("edge", id, "revision", None));
        };
        let before = entry.before.as_ref().map(|b| text(b, "revision"));
        if before.as_deref() != Some(expected) {
            return Err(conflict("edge", id, "revision", before));
        }
        let revision = next(expected)?;
        entry.after.insert("revision".into(), json!(revision));
        entry.touched = true;
        let (from, to) = (text(&entry.after, "from_id"), text(&entry.after, "to_id"));
        self.first("edge", id);
        self.endpoint(&from, Some(&from_lifecycle.to_owned()))?;
        self.endpoint(&to, Some(&to_lifecycle.to_owned()))
    }

    /// A create's id must be new to the graph, as a node, an edge or a
    /// content.
    fn absent(&self, id: &str) -> Result<(), Stop> {
        if let Some(node) = self.state.node(id) {
            return Err(conflict(
                "node",
                id,
                "revision",
                Some(text(&node, "revision")),
            ));
        }
        if let Some(edge) = self.state.edge(id) {
            return Err(conflict(
                "edge",
                id,
                "revision",
                Some(text(&edge, "revision")),
            ));
        }
        if let Some(content) = self.state.content(id) {
            return Err(conflict(
                "content",
                id,
                "revision",
                Some(text(&content, "revision")),
            ));
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn op(
        &mut self,
        op: &ChangeOp,
        tag: Option<&str>,
        stamp: &Stamp<'_>,
        mint: &mut dyn FnMut() -> String,
    ) -> Result<(), Stop> {
        match op {
            ChangeOp::CreateNode {
                id,
                kind,
                title,
                description,
                properties,
            } => {
                self.absent(id)?;
                let image = json!({
                    "id": id, "kind": kind, "title": title, "description": description,
                    "description_source_id": null, "properties": properties,
                    "lifecycle": "active", "merged_into_id": null,
                    "revision": "1", "lifecycle_revision": "1",
                });
                self.written.push(Value::Object(properties.clone()));
                self.nodes.insert(
                    id.clone(),
                    Entry {
                        before: None,
                        after: object(image),
                        touched: true,
                    },
                );
                self.slots.push(Slot::Node(id.clone(), Operation::Create));
                self.first("node", id);
            }
            ChangeOp::UpdateNode {
                id,
                expected_revision,
                set,
                set_properties,
                remove_properties,
            } => {
                self.touch_node(id, Some(expected_revision))?;
                let entry = self.nodes.get_mut(id).ok_or(Refusal::InvalidChange)?;
                if let Some(title) = &set.title {
                    entry.after.insert("title".into(), json!(title));
                }
                if let Some(description) = &set.description {
                    entry.after.insert("description".into(), json!(description));
                    // An authored description has no provenance.
                    if set.description_source.is_none() {
                        entry
                            .after
                            .insert("description_source_id".into(), Value::Null);
                    }
                }
                if let Some(source) = &set.description_source {
                    entry
                        .after
                        .insert("description_source_id".into(), json!(source));
                    if let Some(source) = source {
                        self.provenance.push((id.clone(), source.clone()));
                    }
                }
                if !set_properties.is_empty() || !remove_properties.is_empty() {
                    let entry = self.nodes.get_mut(id).ok_or(Refusal::InvalidChange)?;
                    edit_properties(&mut entry.after, true, set_properties, remove_properties)?;
                    self.written.push(Value::Object(set_properties.clone()));
                }
                self.slots.push(Slot::Node(id.clone(), Operation::Update));
            }
            ChangeOp::ArchiveNode {
                id,
                expected_revision,
            }
            | ChangeOp::UnarchiveNode {
                id,
                expected_revision,
            } => {
                let archive = matches!(op, ChangeOp::ArchiveNode { .. });
                self.touch_node(id, Some(expected_revision))?;
                let entry = self.nodes.get_mut(id).ok_or(Refusal::InvalidChange)?;
                let (from, to) = if archive {
                    ("active", "archived")
                } else {
                    ("archived", "active")
                };
                if text(&entry.after, "lifecycle") != from
                    || (!archive && !entry.after.get("merged_into_id").is_none_or(Value::is_null))
                {
                    return Err(Refusal::InvalidChange.into());
                }
                let lifecycle = next(&text(&entry.after, "lifecycle_revision"))?;
                entry.after.insert("lifecycle".into(), json!(to));
                entry
                    .after
                    .insert("lifecycle_revision".into(), json!(lifecycle));
                let operation = if archive {
                    Operation::Archive
                } else {
                    Operation::Unarchive
                };
                self.slots.push(Slot::Node(id.clone(), operation));
            }
            ChangeOp::CreateEdge {
                id,
                from_id,
                to_id,
                edge_type,
                properties,
                from_lifecycle_revision,
                to_lifecycle_revision,
            } => {
                self.absent(id)?;
                self.endpoint(from_id, from_lifecycle_revision.as_ref())?;
                self.endpoint(to_id, to_lifecycle_revision.as_ref())?;
                let image = json!({
                    "id": id, "from_id": from_id, "to_id": to_id, "type": edge_type,
                    "properties": properties, "lifecycle": "active", "revision": "1",
                });
                self.written.push(Value::Object(properties.clone()));
                self.edges.insert(
                    id.clone(),
                    Entry {
                        before: None,
                        after: object(image),
                        touched: true,
                    },
                );
                self.endpoint_checks.push(id.clone());
                if edge_type == "contains" {
                    self.containment.push(id.clone());
                }
                self.slots.push(Slot::Edge(id.clone(), Operation::Create));
                self.first("edge", id);
            }
            ChangeOp::UpdateEdge {
                id,
                expected_revision,
                set_properties,
                remove_properties,
                from_lifecycle_revision,
                to_lifecycle_revision,
            } => {
                self.touch_edge(
                    id,
                    expected_revision,
                    from_lifecycle_revision,
                    to_lifecycle_revision,
                )?;
                let entry = self.edges.get_mut(id).ok_or(Refusal::InvalidChange)?;
                edit_properties(&mut entry.after, false, set_properties, remove_properties)?;
                self.written.push(Value::Object(set_properties.clone()));
                self.endpoint_checks.push(id.clone());
                self.slots.push(Slot::Edge(id.clone(), Operation::Update));
            }
            ChangeOp::ArchiveEdge {
                id,
                expected_revision,
                from_lifecycle_revision,
                to_lifecycle_revision,
            }
            | ChangeOp::UnarchiveEdge {
                id,
                expected_revision,
                from_lifecycle_revision,
                to_lifecycle_revision,
            } => {
                let archive = matches!(op, ChangeOp::ArchiveEdge { .. });
                self.touch_edge(
                    id,
                    expected_revision,
                    from_lifecycle_revision,
                    to_lifecycle_revision,
                )?;
                let entry = self.edges.get_mut(id).ok_or(Refusal::InvalidChange)?;
                let (from, to) = if archive {
                    ("active", "archived")
                } else {
                    ("archived", "active")
                };
                if text(&entry.after, "lifecycle") != from {
                    return Err(Refusal::InvalidChange.into());
                }
                entry.after.insert("lifecycle".into(), json!(to));
                let contains = text(&entry.after, "type") == "contains";
                if !archive {
                    self.endpoint_checks.push(id.clone());
                    if contains {
                        self.containment.push(id.clone());
                    }
                }
                let operation = if archive {
                    Operation::Archive
                } else {
                    Operation::Unarchive
                };
                self.slots.push(Slot::Edge(id.clone(), operation));
            }
            ChangeOp::AddTag {
                node_id,
                expected_revision,
                ..
            }
            | ChangeOp::RemoveTag {
                node_id,
                expected_revision,
                ..
            } => {
                let add = matches!(op, ChangeOp::AddTag { .. });
                let text_ = tag.ok_or(Refusal::InvalidChange)?.to_owned();
                self.touch_node(node_id, expected_revision.as_ref())?;
                if !self.explicit.contains(node_id) && self.emitted.insert(node_id.clone()) {
                    self.slots
                        .push(Slot::Node(node_id.clone(), Operation::Update));
                }
                if add {
                    self.add_tag(node_id, &text_, mint)?;
                } else {
                    self.remove_tag(node_id, &text_)?;
                }
            }
            ChangeOp::MergeNodes {
                target_id,
                target_revision,
                absorbed,
                edges,
            } => self.merge(target_id, target_revision, absorbed, edges)?,
            ChangeOp::CreateContent {
                id,
                owner_node_id,
                body,
            } => {
                self.absent(id)?;
                let image = json!({
                    "id": id, "owner_node_id": owner_node_id, "revision": "1", "body": body,
                });
                self.contents.insert(
                    id.clone(),
                    Entry {
                        before: None,
                        after: object(image),
                        touched: true,
                    },
                );
                self.slots
                    .push(Slot::Content(id.clone(), Operation::Create));
                self.first("content", id);
            }
            ChangeOp::UpdateContent {
                id,
                owner_node_id,
                expected_revision,
                body,
            } => {
                let Some(before) = self.state.content(id) else {
                    return Err(conflict("content", id, "revision", None));
                };
                let update = object(json!({
                    "id": id, "owner_node_id": owner_node_id,
                    "expected_revision": expected_revision, "body": body,
                }));
                let after = match revise(&before, &update) {
                    Revision::Accepted(op) => match op.after_value {
                        Some(Value::Object(after)) => after,
                        _ => return Err(Refusal::InvalidChange.into()),
                    },
                    Revision::Conflict(detail) => return Err(Stop::Conflict(detail)),
                    Revision::Refused(refusal) => return Err(refusal.into()),
                };
                self.contents.insert(
                    id.clone(),
                    Entry {
                        before: Some(before),
                        after,
                        touched: true,
                    },
                );
                self.slots
                    .push(Slot::Content(id.clone(), Operation::Update));
                self.first("content", id);
            }
            ChangeOp::VerifyAssertion {
                id,
                expected_revision,
                sources,
                contents,
            } => self.verify(id, expected_revision, sources, contents, stamp)?,
        }
        Ok(())
    }

    /// `merge_nodes`: its preconditions, its refusals, then its archives
    /// and rewrites (the module's *Merge*).
    fn merge(
        &mut self,
        target_id: &str,
        target_revision: &str,
        absorbed: &[crate::change::Absorbed],
        edges: &[MergeEdge],
    ) -> Result<(), Stop> {
        let Some(target) = self.node(target_id) else {
            return Err(conflict("node", target_id, "revision", None));
        };
        let target = target.before.clone().ok_or(Refusal::InvalidChange)?;
        if text(&target, "revision") != target_revision {
            return Err(conflict(
                "node",
                target_id,
                "revision",
                Some(text(&target, "revision")),
            ));
        }
        for node in absorbed {
            self.touch_node(&node.id, Some(&node.expected_revision))?;
        }
        let ids: Vec<String> = absorbed.iter().map(|n| n.id.clone()).collect();
        let listed: Vec<String> = edges.iter().map(|e| e.id.clone()).collect();
        exact_incident_edges(self.state, &ids, &listed).map_err(Stop::Conflict)?;
        for edge in edges {
            self.touch_edge(
                &edge.id,
                &edge.expected_revision,
                &edge.from_lifecycle_revision,
                &edge.to_lifecycle_revision,
            )?;
        }
        if text(&target, "lifecycle") != "active"
            || !target.get("merged_into_id").is_none_or(Value::is_null)
            || unmergeable(&target)
        {
            return Err(Refusal::InvalidChange.into());
        }
        let set: BTreeSet<String> = ids.iter().cloned().collect();
        for id in &ids {
            let entry = self.nodes.get_mut(id).ok_or(Refusal::InvalidChange)?;
            let after = &mut entry.after;
            if text(after, "lifecycle") != "active"
                || !after.get("merged_into_id").is_none_or(Value::is_null)
                || unmergeable(after)
            {
                return Err(Refusal::InvalidChange.into());
            }
            let lifecycle = next(&text(after, "lifecycle_revision"))?;
            after.insert("lifecycle".into(), json!("archived"));
            after.insert("lifecycle_revision".into(), json!(lifecycle));
            after.insert("merged_into_id".into(), json!(target_id));
        }
        for id in &listed {
            let entry = self.edges.get_mut(id).ok_or(Refusal::InvalidChange)?;
            if text(&entry.after, "type") == "guidance_scope" {
                return Err(Refusal::InvalidChange.into());
            }
            entry.after =
                rewrite_endpoints(&entry.after, &set, target_id).ok_or(Refusal::InvalidChange)?;
            if text(&entry.after, "type") == "contains" {
                self.containment.push(id.clone());
            }
            self.endpoint_checks.push(id.clone());
        }
        redirect_bound(self.state, &ids)?;
        self.slots
            .extend(ids.into_iter().map(|id| Slot::Node(id, Operation::Archive)));
        self.slots.extend(
            listed
                .into_iter()
                .map(|id| Slot::Edge(id, Operation::Update)),
        );
        Ok(())
    }

    /// `verify_assertion`: the exact checked state against the pre-state,
    /// then the one node `update` that verifies (the module's
    /// *Verification*).
    fn verify(
        &mut self,
        id: &str,
        expected: &str,
        sources: &[CheckedSource],
        contents: &[CheckedContent],
        stamp: &Stamp<'_>,
    ) -> Result<(), Stop> {
        let Some(node) = self.state.node(id) else {
            return Err(conflict("node", id, "revision", None));
        };
        if text(&node, "revision") != expected {
            return Err(conflict(
                "node",
                id,
                "revision",
                Some(text(&node, "revision")),
            ));
        }
        if text(&node, "lifecycle") != "active" || assertion::of(&node).is_none() {
            return Err(Refusal::InvalidChange.into());
        }
        let found = self.state.sources_from(id, MAX_CHECKED_SOURCES + 1);
        if found.len() > MAX_CHECKED_SOURCES {
            return Err(Refusal::ChangeLimit.into());
        }
        if found.is_empty() && sources.is_empty() {
            return Err(Refusal::InvalidChange.into());
        }
        let listed: BTreeMap<&str, &CheckedSource> =
            sources.iter().map(|s| (s.edge_id.as_str(), s)).collect();
        for edge in &found {
            let edge_id = text(edge, "id");
            if !listed.contains_key(edge_id.as_str()) {
                return Err(conflict(
                    "edge",
                    &edge_id,
                    "revision",
                    Some(text(edge, "revision")),
                ));
            }
        }
        for source in sources {
            let Some(edge) = found.iter().find(|e| text(e, "id") == source.edge_id) else {
                let current = self
                    .state
                    .edge(&source.edge_id)
                    .map(|e| text(&e, "revision"));
                return Err(conflict("edge", &source.edge_id, "revision", current));
            };
            let edge_revision = text(edge, "revision");
            if edge_revision != source.edge_revision {
                return Err(conflict(
                    "edge",
                    &source.edge_id,
                    "revision",
                    Some(edge_revision),
                ));
            }
            let reference = assertion::source_ref(edge).ok_or(Refusal::InvalidChange)?;
            let Some(ref_revision) = &reference.revision else {
                return Err(Refusal::InvalidChange.into());
            };
            if reference.id != source.ref_id || *ref_revision != source.ref_revision {
                return Err(conflict(
                    "edge",
                    &source.edge_id,
                    "revision",
                    Some(edge_revision),
                ));
            }
            let current =
                assertion::resolve(self.state, edge, &reference).ok_or(Refusal::InvalidChange)?;
            if current != source.ref_revision {
                let entity = match reference.entity_type {
                    RefType::Node => "node",
                    RefType::Content => "content",
                };
                return Err(conflict(entity, &reference.id, "revision", Some(current)));
            }
            let to = self.state.node(&text(edge, "to_id"));
            if !to.is_some_and(|to| text(&to, "lifecycle") == "active") {
                return Err(Refusal::InvalidChange.into());
            }
            self.checked_targets
                .push((reference.entity_type, reference.id.clone()));
        }
        let owned = self.state.contents_of(id, MAX_CHECKED_CONTENTS + 1);
        if owned.len() > MAX_CHECKED_CONTENTS {
            return Err(Refusal::ChangeLimit.into());
        }
        let listed: BTreeMap<&str, &CheckedContent> =
            contents.iter().map(|c| (c.id.as_str(), c)).collect();
        for content in &owned {
            let content_id = text(content, "id");
            let revision = text(content, "revision");
            match listed.get(content_id.as_str()) {
                Some(checked) if checked.revision == revision => {}
                _ => return Err(conflict("content", &content_id, "revision", Some(revision))),
            }
        }
        for checked in contents {
            if !owned.iter().any(|c| text(c, "id") == checked.id) {
                let current = self
                    .state
                    .content(&checked.id)
                    .map(|c| text(&c, "revision"));
                return Err(conflict("content", &checked.id, "revision", current));
            }
        }
        self.touch_node(id, Some(&expected.to_owned()))?;
        let entry = self.nodes.get_mut(id).ok_or(Refusal::InvalidChange)?;
        assertion::verify(&mut entry.after, stamp.time_ms, stamp.actor_id)?;
        self.verified.insert(id.to_owned());
        self.slots
            .push(Slot::Node(id.to_owned(), Operation::Update));
        Ok(())
    }

    fn add_tag(
        &mut self,
        node_id: &str,
        tag: &str,
        mint: &mut dyn FnMut() -> String,
    ) -> Result<(), Stop> {
        let tag_id = if let Some(image) = self.tags.get(tag) {
            text(image, "id")
        } else if let Some(image) = self.state.tag(tag) {
            let id = text(&image, "id");
            if self.state.membership(node_id, &id).is_some() {
                return Err(Refusal::InvalidChange.into());
            }
            id
        } else {
            let id = mint();
            let image = object(json!({"id": id, "text": tag, "label": null, "revision": "1"}));
            self.tags.insert(tag.to_owned(), image);
            self.slots.push(Slot::Tag(tag.to_owned()));
            self.first("tag", tag);
            id
        };
        let image = object(json!({"id": mint(), "node_id": node_id, "tag_id": tag_id}));
        self.slots.push(Slot::Membership(Operation::Create, image));
        Ok(())
    }

    fn remove_tag(&mut self, node_id: &str, tag: &str) -> Result<(), Stop> {
        // A tag new in this change has no membership to remove.
        if self.tags.contains_key(tag) {
            return Err(Refusal::InvalidChange.into());
        }
        let image = self
            .state
            .tag(tag)
            .and_then(|tag| self.state.membership(node_id, &text(&tag, "id")))
            .ok_or(Refusal::InvalidChange)?;
        self.slots.push(Slot::Membership(Operation::Remove, image));
        Ok(())
    }

    /// The final image of node `id`, wherever it is.
    fn final_node(&self, id: &str) -> Option<Image> {
        match self.nodes.get(id) {
            Some(entry) => Some(entry.after.clone()),
            None => self.state.node(id),
        }
    }

    /// The final image of content `id`, wherever it is.
    fn final_content(&self, id: &str) -> Option<Image> {
        match self.contents.get(id) {
            Some(entry) => Some(entry.after.clone()),
            None => self.state.content(id),
        }
    }

    /// Every typed reference in `value` names a node or content of the
    /// final candidate at a revision it has had.
    fn references(&self, value: &Value) -> Result<(), Stop> {
        if let Some(reference) = typed_ref(value)? {
            let entity = match reference.entity_type {
                RefType::Node => self.final_node(&reference.id),
                RefType::Content => self.final_content(&reference.id),
            }
            .ok_or(Refusal::InvalidChange)?;
            if let Some(revision) = &reference.revision {
                if revision == "0" || !revision_le(revision, &text(&entity, "revision")) {
                    return Err(Refusal::InvalidChange.into());
                }
            }
            return Ok(());
        }
        match value {
            Value::Array(items) => items.iter().try_for_each(|item| self.references(item)),
            Value::Object(members) => members.values().try_for_each(|m| self.references(m)),
            _ => Ok(()),
        }
    }

    /// An edge left active by this change has both endpoints active, and a
    /// `guidance_scope` edge starts at a `Guidance` node (contract §3).
    fn endpoints_active(&self, id: &str) -> Result<(), Stop> {
        let Some(edge) = self.edges.get(id) else {
            return Ok(());
        };
        if text(&edge.after, "lifecycle") != "active" {
            return Ok(());
        }
        for end in ["from_id", "to_id"] {
            let node = self.final_node(&text(&edge.after, end));
            let active = node
                .as_ref()
                .is_some_and(|node| text(node, "lifecycle") == "active");
            let guidance = end != "from_id"
                || text(&edge.after, "type") != "guidance_scope"
                || node.is_some_and(|node| text(&node, "kind") == "Guidance");
            if !active || !guidance {
                return Err(Refusal::InvalidChange.into());
            }
        }
        Ok(())
    }

    /// The active `contains` edges of the final candidate into `frontier`,
    /// reading at most `remaining + 1` of them (beyond those this change
    /// replaces): more than `remaining` means the budget is exceeded.
    fn level(&self, frontier: &[String], remaining: usize) -> Vec<Image> {
        let replaced = self
            .edges
            .values()
            .filter(|entry| {
                entry.before.as_ref().is_some_and(|before| {
                    text(before, "type") == "contains" && text(before, "lifecycle") == "active"
                })
            })
            .count();
        let mut level: BTreeMap<String, Image> = BTreeMap::new();
        for edge in self
            .state
            .contains_into(frontier, remaining.saturating_add(1 + replaced))
        {
            let id = text(&edge, "id");
            if !self.edges.contains_key(&id) {
                level.insert(id, edge);
            }
        }
        for (id, entry) in &self.edges {
            let edge = &entry.after;
            if text(edge, "type") == "contains"
                && text(edge, "lifecycle") == "active"
                && frontier.contains(&text(edge, "to_id"))
            {
                level.insert(id.clone(), edge.clone());
            }
        }
        level.into_values().collect()
    }

    fn acyclic(&self, id: &str, budget: &mut usize) -> Result<(), Stop> {
        let Some(edge) = self.edges.get(id) else {
            return Ok(());
        };
        if text(&edge.after, "lifecycle") != "active" {
            return Ok(());
        }
        let (parent, child) = (text(&edge.after, "from_id"), text(&edge.after, "to_id"));
        if parent == child {
            return Err(Refusal::InvalidChange.into());
        }
        let mut seen = BTreeSet::from([parent.clone()]);
        let mut frontier = vec![parent];
        let mut depth = 0;
        loop {
            let level = self.level(&frontier, MAX_CONTAINMENT_EDGES.saturating_sub(*budget));
            if level.is_empty() {
                return Ok(());
            }
            depth += 1;
            *budget += level.len();
            if depth > MAX_CONTAINMENT_DEPTH || *budget > MAX_CONTAINMENT_EDGES {
                return Err(Refusal::ChangeLimit.into());
            }
            let parents: BTreeSet<String> = level.iter().map(|e| text(e, "from_id")).collect();
            if parents.contains(&child) {
                return Err(Refusal::InvalidChange.into());
            }
            frontier = parents
                .into_iter()
                .filter(|p| seen.insert(p.clone()))
                .collect();
            if frontier.is_empty() {
                return Ok(());
            }
        }
    }

    /// Whether node `id` is `verified` in the pre-state.
    fn verified_before(&self, id: &str) -> bool {
        self.state
            .node(id)
            .is_some_and(|n| assertion::is_verified(&n))
    }

    /// The final candidate's own rules (the module's order): node members
    /// by kind, description provenance, content owners and reference
    /// bodies, then the verification rules, clearing what they clear.
    fn finish(&mut self) -> Result<(), Stop> {
        for entry in self.nodes.values().filter(|e| e.touched) {
            let kind = text(&entry.after, "kind");
            let properties = entry
                .after
                .get("properties")
                .ok_or(Refusal::InvalidChange)?;
            check_node_properties(Some(&kind), properties)?;
        }
        for (node, source) in &self.provenance {
            let from_content = self
                .final_content(source)
                .is_some_and(|c| text(&c, "owner_node_id") == *node);
            let from_assertion = self
                .final_node(source)
                .is_some_and(|n| assertion::of(&n).is_some());
            if !from_content && !from_assertion {
                return Err(Refusal::InvalidChange.into());
            }
        }
        for entry in self.contents.values() {
            if self
                .final_node(&text(&entry.after, "owner_node_id"))
                .is_none()
            {
                return Err(Refusal::InvalidChange.into());
            }
            if let Some(reference) = entry.after.get("body").and_then(|b| b.get("reference")) {
                let resource = reference
                    .get("resource_node_id")
                    .and_then(Value::as_str)
                    .and_then(|id| self.final_node(id));
                if !resource.is_some_and(|r| text(&r, "kind") == "Resource") {
                    return Err(Refusal::InvalidChange.into());
                }
            }
        }
        // A verification stays bound to its checked state: no write of this
        // change may move an entity one of its `sources` references.
        for (entity_type, target) in &self.checked_targets {
            let written = match entity_type {
                RefType::Node => self.nodes.get(target).is_some_and(|e| e.touched),
                RefType::Content => self.contents.contains_key(target),
            };
            if written {
                return Err(Refusal::InvalidChange.into());
            }
        }
        let verified = &self.verified;
        // Writes to a checked state: refused while it is verified unless the
        // same change clears it, and never beside its own verification.
        let guarded = |owner: &str| -> Result<(), Stop> {
            if verified.contains(owner)
                || (self.verified_before(owner) && !self.updated.contains(owner))
            {
                return Err(Refusal::InvalidChange.into());
            }
            Ok(())
        };
        for entry in self.edges.values().filter(|e| e.touched) {
            if text(&entry.after, "type") != "sources" {
                continue;
            }
            guarded(&text(&entry.after, "from_id"))?;
            if let Some(before) = &entry.before {
                guarded(&text(before, "from_id"))?;
            }
        }
        for entry in self.contents.values() {
            guarded(&text(&entry.after, "owner_node_id"))?;
        }
        for (id, entry) in &mut self.nodes {
            if entry.touched && entry.before.is_some() && !self.verified.contains(id) {
                assertion::clear(&mut entry.after);
            }
        }
        Ok(())
    }

    fn effects(&self) -> Result<Effects, Stop> {
        let mut ops = Vec::with_capacity(self.slots.len());
        for slot in &self.slots {
            let (entity_type, operation, before, after) = match slot {
                Slot::Node(id, operation) => {
                    let entry = self.nodes.get(id).ok_or(Refusal::InvalidChange)?;
                    (
                        EntityType::Node,
                        *operation,
                        entry.before.clone(),
                        Some(entry.after.clone()),
                    )
                }
                Slot::Edge(id, operation) => {
                    let entry = self.edges.get(id).ok_or(Refusal::InvalidChange)?;
                    (
                        EntityType::Edge,
                        *operation,
                        entry.before.clone(),
                        Some(entry.after.clone()),
                    )
                }
                Slot::Content(id, operation) => {
                    let entry = self.contents.get(id).ok_or(Refusal::InvalidChange)?;
                    (
                        EntityType::Content,
                        *operation,
                        entry.before.clone(),
                        Some(entry.after.clone()),
                    )
                }
                Slot::Tag(tag) => {
                    let image = self.tags.get(tag).ok_or(Refusal::InvalidChange)?;
                    (
                        EntityType::Tag,
                        Operation::Create,
                        None,
                        Some(image.clone()),
                    )
                }
                Slot::Membership(Operation::Remove, image) => (
                    EntityType::NodeTag,
                    Operation::Remove,
                    Some(image.clone()),
                    None,
                ),
                Slot::Membership(operation, image) => {
                    (EntityType::NodeTag, *operation, None, Some(image.clone()))
                }
                Slot::Review(image) => (
                    EntityType::Review,
                    Operation::Create,
                    None,
                    Some(image.clone()),
                ),
            };
            let revision = |image: &Option<Image>| {
                image
                    .as_ref()
                    .filter(|_| entity_type != EntityType::NodeTag)
                    .map(|image| text(image, "revision"))
            };
            let entity_id = text(
                after
                    .as_ref()
                    .or(before.as_ref())
                    .ok_or(Refusal::InvalidChange)?,
                "id",
            );
            ops.push(Op {
                ordinal: u32::try_from(ops.len()).map_err(|_| Refusal::ChangeLimit)?,
                entity_type,
                entity_id,
                operation,
                before_revision: revision(&before),
                after_revision: revision(&after),
                before_value: before.map(Value::Object),
                after_value: after.map(Value::Object),
            });
        }
        let delta_digest = delta::digest(&ops)?;
        Ok(Effects {
            ops,
            delta_digest,
            affected: self.affected()?,
        })
    }

    /// The receipt's `affected`, in order of first touch.
    fn affected(&self) -> Result<Vec<Value>, Stop> {
        self.order
            .iter()
            .map(|(kind, key)| {
                Ok(match *kind {
                    "node" => {
                        let after = &self.nodes.get(key).ok_or(Refusal::InvalidChange)?.after;
                        json!({"entity_type": "node", "id": key, "revision": after.get("revision"),
                               "lifecycle_revision": after.get("lifecycle_revision")})
                    }
                    "edge" => {
                        let after = &self.edges.get(key).ok_or(Refusal::InvalidChange)?.after;
                        json!({"entity_type": "edge", "id": key, "revision": after.get("revision")})
                    }
                    "content" => {
                        let after = &self.contents.get(key).ok_or(Refusal::InvalidChange)?.after;
                        json!({"entity_type": "content", "id": key, "revision": after.get("revision")})
                    }
                    "review" => json!({"entity_type": "review", "id": key, "revision": "1"}),
                    _ => {
                        let tag = self.tags.get(key).ok_or(Refusal::InvalidChange)?;
                        json!({"entity_type": "tag", "id": tag.get("id"), "revision": "1"})
                    }
                })
            })
            .collect()
    }
}

/// A merge's redirect bound: no chain into an absorbed node may be so deep
/// that, one hop further to the target, it passes [`MAX_REDIRECT_HOPS`].
fn redirect_bound(state: &dyn State, absorbed: &[String]) -> Result<(), Refusal> {
    if redirect_depth(state, absorbed)? + 1 > MAX_REDIRECT_HOPS {
        return Err(Refusal::ChangeLimit);
    }
    Ok(())
}

fn object(value: Value) -> Image {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// Set and remove properties of an image; removing a key it lacks refuses,
/// and the result must still be a valid properties object (a node's may
/// hold reserved members, checked against its kind in
/// [`Candidate::finish`]).
fn edit_properties(
    image: &mut Image,
    node: bool,
    set: &Map<String, Value>,
    remove: &[String],
) -> Result<(), Stop> {
    let mut properties = match image.get("properties") {
        Some(Value::Object(properties)) => properties.clone(),
        _ => Map::new(),
    };
    for key in remove {
        if properties.remove(key).is_none() {
            return Err(Refusal::InvalidChange.into());
        }
    }
    for (key, value) in set {
        properties.insert(key.clone(), value.clone());
    }
    let properties = Value::Object(properties);
    if node {
        check_node_properties(None, &properties)?;
    } else {
        check_properties(&properties)?;
    }
    image.insert("properties".into(), properties);
    Ok(())
}

/// What a reversal reads beyond a change's [`State`]: when a current row
/// was last written, and who references a node. Both answer the current
/// graph only.
pub trait Retained: State {
    /// The graph position of the event that last wrote the node, edge or
    /// content `id` (`None` when there is no such row).
    fn written_at(&self, id: &str) -> Option<u64>;
    /// The nodes, edges and contents whose current row holds a typed
    /// reference to node `node_id` that the domain may see (a `state:`
    /// source of `domain` visibility: never a proposal's, an event's or a
    /// receipt's), ascending by id, at most `limit`.
    fn referrers(&self, node_id: &str, limit: usize) -> Vec<String>;
}

/// The most dependents (incident edges, owned contents and referrers) the
/// dependency checks of one reversal may read.
pub const MAX_DEPENDENCY_READS: usize = 10_000;

/// Judge the reversal `plan` ([`crate::inverse::plan`]) against `state`, by
/// `stamp`'s actor. The same machinery as [`apply`], in this order, the
/// first failure deciding:
///
/// 1. the plan's own shape (`invalid_change`): each operation as
///    [`crate::inverse`] maps it — ids, kinds, types and owners kept,
///    `revision` one more than its before-image's, a node's
///    `lifecycle_revision` one more exactly when its lifecycle changes, the
///    operation the lifecycle transition names, images within their rules —
///    no entity twice, and no node after-image `verified` or with a non-null
///    `verified_at`/`verified_by` (X29: only `verify_assertion` verifies);
/// 2. each operation's precondition, in order: the entity exactly its
///    planned before-image now (else a conflict on its `revision`, or its
///    `lifecycle_revision` when only that moved); a membership to remove
///    present, one to restore absent (else a conflict on its node);
/// 3. for each node the reversal archives, in plan order: a re-merge's
///    target active and unmerged in the final candidate (else a
///    `lifecycle_revision` conflict on the target), its redirect chain
///    within [`MAX_REDIRECT_HOPS`] (`change_limit`; a cycle
///    `invalid_change`) and no active edge left incident to it; for every
///    archived node, no current-state dependent written after the target
///    event ([`Retained`]: an active incident edge, an owned content, a
///    domain-visible referrer). A blocked node is a conflict whose
///    precondition is `dependency`, naming the node (never the dependent)
///    at its current revision. More than [`MAX_DEPENDENCY_READS`] refuses
///    `change_limit`;
/// 4. typed references written, edges left active (endpoints active, a
///    `guidance_scope` edge from a `Guidance` node), containment, node
///    members by kind, content owners and reference bodies — as [`apply`];
/// 5. verification: a `sources` edge (its from-node before or after) or a
///    content of an assertion `verified` now refuses `invalid_change`
///    unless the plan also holds an operation on that assertion;
/// 6. the event: the planned operations, then one `review` creation of
///    kind `reversal` naming the target, within [`crate::delta::digest`]'s
///    bounds. Restored memberships and the review take ids from `mint`, in
///    that order; `affected` ends with the review.
pub fn apply_inverse(
    state: &dyn Retained,
    plan: &crate::inverse::Plan,
    stamp: &Stamp<'_>,
    mint: &mut dyn FnMut() -> String,
) -> Verdict {
    let mut candidate = Candidate::empty(state);
    match candidate.inverse(state, plan, stamp, mint) {
        Ok(effects) => Verdict::Accepted(effects),
        Err(Stop::Conflict(detail)) => Verdict::Conflict(detail),
        Err(Stop::Refused(refusal)) => Verdict::Refused(refusal),
    }
}

/// Whether a node image carries any verification: `verified`, or a
/// non-null `verified_at`/`verified_by`.
fn carries_verification(node: &Image) -> bool {
    assertion::is_verified(node)
        || assertion::of(node).is_some_and(|a| {
            !a.get("verified_at").is_none_or(Value::is_null)
                || !a.get("verified_by").is_none_or(Value::is_null)
        })
}

/// One planned operation's shape (step 1 of [`apply_inverse`]).
fn planned_ok(op: &crate::inverse::PlannedOp) -> Result<(), Refusal> {
    use crate::inverse::{next, transition};
    use crate::wire::image_ok;
    let image = |v: &Option<Value>| v.as_ref().and_then(Value::as_object).cloned();
    let (before, after) = (image(&op.before_value), image(&op.after_value));
    let ok = match (op.entity_type, op.operation) {
        (EntityType::NodeTag, Operation::Remove) => before.as_ref().is_some_and(|b| {
            op.after_value.is_none()
                && op.entity_id.as_deref() == Some(text(b, "id").as_str())
                && image_ok(EntityType::NodeTag, &text(b, "id"), b)
        }),
        (EntityType::NodeTag, Operation::Create) => after.as_ref().is_some_and(|a| {
            op.before_value.is_none()
                && op.entity_id.is_none()
                && a.len() == 3
                && a.get("id").is_some_and(Value::is_null)
                && ["node_id", "tag_id"].iter().all(|k| {
                    a.get(*k)
                        .and_then(Value::as_str)
                        .is_some_and(crate::is_ulid)
                })
        }),
        (EntityType::NodeTag, _) => false,
        (EntityType::Node | EntityType::Edge | EntityType::Content, operation) => {
            let (Some(b), Some(a), Some(id)) = (&before, &after, op.entity_id.as_deref()) else {
                return Err(Refusal::InvalidChange);
            };
            let node = op.entity_type == EntityType::Node;
            let fixed = match op.entity_type {
                EntityType::Node => "kind",
                EntityType::Edge => "type",
                _ => "owner_node_id",
            };
            let lifecycle_ok = match op.entity_type {
                EntityType::Content => operation == Operation::Update,
                _ => operation == transition(&text(b, "lifecycle"), &text(a, "lifecycle")),
            };
            let lifecycle_revision_ok = !node || {
                let current = text(b, "lifecycle_revision");
                let moved = text(b, "lifecycle") != text(a, "lifecycle");
                let want = if moved {
                    next(&current).ok()
                } else {
                    Some(current)
                };
                want.as_deref() == Some(text(a, "lifecycle_revision").as_str())
            };
            image_ok(op.entity_type, id, b)
                && image_ok(op.entity_type, id, a)
                && b.get(fixed) == a.get(fixed)
                && lifecycle_ok
                && lifecycle_revision_ok
                && next(&text(b, "revision")).ok().as_deref() == Some(text(a, "revision").as_str())
                && op.before_revision.as_deref() == Some(text(b, "revision").as_str())
                && op.after_revision.as_deref() == Some(text(a, "revision").as_str())
                && !(node && carries_verification(a))
        }
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(Refusal::InvalidChange)
    }
}

/// The conflict of an entity that is not exactly `expected` now.
fn moved(entity_type: &str, id: &str, current: Option<&Image>, expected: &Image) -> Stop {
    let Some(current) = current else {
        return conflict(entity_type, id, "revision", None);
    };
    if entity_type == "node"
        && text(current, "revision") == text(expected, "revision")
        && text(current, "lifecycle_revision") != text(expected, "lifecycle_revision")
    {
        return conflict(
            entity_type,
            id,
            "lifecycle_revision",
            Some(text(current, "lifecycle_revision")),
        );
    }
    conflict(entity_type, id, "revision", Some(text(current, "revision")))
}

impl Candidate<'_> {
    fn inverse(
        &mut self,
        retained: &dyn Retained,
        plan: &crate::inverse::Plan,
        stamp: &Stamp<'_>,
        mint: &mut dyn FnMut() -> String,
    ) -> Result<Effects, Stop> {
        let target = plan.target()?;
        let after_seq: u64 = plan
            .target_graph_seq
            .parse()
            .ok()
            .filter(|_| crate::is_decimal(&plan.target_graph_seq))
            .ok_or(Refusal::InvalidChange)?;
        let mut entities = BTreeSet::new();
        for op in &plan.ops {
            planned_ok(op)?;
            if let Some(id) = &op.entity_id {
                if !entities.insert(id.clone()) {
                    return Err(Refusal::InvalidChange.into());
                }
            }
        }
        for op in &plan.ops {
            self.inverse_op(op, mint)?;
            if let Some(id) = &op.entity_id {
                if op.entity_type == EntityType::Node {
                    self.updated.insert(id.clone());
                }
            }
        }
        for op in &plan.ops {
            if op.entity_type == EntityType::Node {
                let id = op.entity_id.as_deref().unwrap_or_default();
                self.dependencies(retained, id, after_seq, &entities)?;
            }
        }
        for value in std::mem::take(&mut self.written) {
            self.references(&value)?;
        }
        for id in std::mem::take(&mut self.endpoint_checks) {
            self.endpoints_active(&id)?;
        }
        let mut budget = 0;
        for id in std::mem::take(&mut self.containment) {
            self.acyclic(&id, &mut budget)?;
        }
        self.finish_inverse()?;
        let id = mint();
        let image = crate::review::image(&id, &target, crate::review::REVERSAL, stamp.actor_id)?;
        self.slots.push(Slot::Review(image));
        self.first("review", &id);
        self.effects()
    }

    /// Step 2 of [`apply_inverse`] for one planned operation.
    fn inverse_op(
        &mut self,
        op: &crate::inverse::PlannedOp,
        mint: &mut dyn FnMut() -> String,
    ) -> Result<(), Stop> {
        let image = |v: &Option<Value>| -> Result<Image, Stop> {
            Ok(v.as_ref()
                .and_then(Value::as_object)
                .cloned()
                .ok_or(Refusal::InvalidChange)?)
        };
        let id = op.entity_id.clone().unwrap_or_default();
        match op.entity_type {
            EntityType::Node | EntityType::Edge | EntityType::Content => {
                let (before, after) = (image(&op.before_value)?, image(&op.after_value)?);
                let (name, current) = match op.entity_type {
                    EntityType::Node => ("node", self.state.node(&id)),
                    EntityType::Edge => ("edge", self.state.edge(&id)),
                    _ => ("content", self.state.content(&id)),
                };
                if current.as_ref() != Some(&before) {
                    return Err(moved(name, &id, current.as_ref(), &before));
                }
                let entry = Entry {
                    before: Some(before.clone()),
                    after: after.clone(),
                    touched: true,
                };
                match op.entity_type {
                    EntityType::Node => {
                        self.written
                            .push(after.get("properties").cloned().unwrap_or(Value::Null));
                        self.nodes.insert(id.clone(), entry);
                        self.slots.push(Slot::Node(id.clone(), op.operation));
                        self.first("node", &id);
                    }
                    EntityType::Edge => {
                        self.written
                            .push(after.get("properties").cloned().unwrap_or(Value::Null));
                        let active = text(&after, "lifecycle") == "active";
                        let rewired = text(&before, "lifecycle") != "active"
                            || text(&before, "from_id") != text(&after, "from_id")
                            || text(&before, "to_id") != text(&after, "to_id");
                        if active {
                            self.endpoint_checks.push(id.clone());
                            if rewired && text(&after, "type") == "contains" {
                                self.containment.push(id.clone());
                            }
                        }
                        self.edges.insert(id.clone(), entry);
                        self.slots.push(Slot::Edge(id.clone(), op.operation));
                        self.first("edge", &id);
                    }
                    _ => {
                        self.contents.insert(id.clone(), entry);
                        self.slots.push(Slot::Content(id.clone(), op.operation));
                        self.first("content", &id);
                    }
                }
            }
            EntityType::NodeTag => {
                let membership = match op.operation {
                    Operation::Remove => image(&op.before_value)?,
                    _ => image(&op.after_value)?,
                };
                let node_id = text(&membership, "node_id");
                let current = self
                    .state
                    .membership(&node_id, &text(&membership, "tag_id"));
                let holds = match op.operation {
                    Operation::Remove => current.as_ref() == Some(&membership),
                    _ => current.is_none(),
                };
                if !holds {
                    let revision = self.state.node(&node_id).map(|n| text(&n, "revision"));
                    return Err(conflict("node", &node_id, "revision", revision));
                }
                if op.operation == Operation::Remove {
                    self.slots
                        .push(Slot::Membership(Operation::Remove, membership));
                } else {
                    let mut restored = membership;
                    restored.insert("id".into(), json!(mint()));
                    self.slots
                        .push(Slot::Membership(Operation::Create, restored));
                }
            }
            _ => return Err(Refusal::InvalidChange.into()),
        }
        Ok(())
    }

    /// Step 3 of [`apply_inverse`] for node `id`, when the reversal
    /// archives it.
    fn dependencies(
        &self,
        retained: &dyn Retained,
        id: &str,
        after_seq: u64,
        planned: &BTreeSet<String>,
    ) -> Result<(), Stop> {
        let Some(entry) = self.nodes.get(id) else {
            return Ok(());
        };
        let before = entry.before.as_ref().ok_or(Refusal::InvalidChange)?;
        if text(before, "lifecycle") != "active" || text(&entry.after, "lifecycle") != "archived" {
            return Ok(());
        }
        let blocked = || conflict("node", id, "dependency", Some(text(before, "revision")));
        let remerge = entry
            .after
            .get("merged_into_id")
            .and_then(Value::as_str)
            .filter(|_| before.get("merged_into_id").is_none_or(Value::is_null));
        if let Some(into) = remerge {
            let survivor = self.final_node(into);
            let usable = survivor.as_ref().is_some_and(|s| {
                text(s, "lifecycle") == "active"
                    && s.get("merged_into_id").is_none_or(Value::is_null)
            });
            if into == id || !usable {
                let current = survivor.map(|s| text(&s, "lifecycle_revision"));
                return Err(conflict("node", into, "lifecycle_revision", current));
            }
            redirect_bound(self.state, &[id.to_owned()])?;
        }
        let later = |other: &str| {
            !planned.contains(other) && retained.written_at(other).is_none_or(|at| at > after_seq)
        };
        let mut reads = 0usize;
        let mut read = |count: usize| -> Result<usize, Stop> {
            reads += count;
            if reads > MAX_DEPENDENCY_READS {
                return Err(Refusal::ChangeLimit.into());
            }
            Ok(MAX_DEPENDENCY_READS - reads + 1)
        };
        let ids = [id.to_owned()];
        let found = self.state.incident_active(&ids, read(0)?);
        read(found.len())?;
        let mut edges: BTreeMap<String, Image> =
            found.into_iter().map(|e| (text(&e, "id"), e)).collect();
        for (edge_id, edge) in &self.edges {
            edges.insert(edge_id.clone(), edge.after.clone());
        }
        for (edge_id, edge) in &edges {
            let incident = text(edge, "from_id") == id || text(edge, "to_id") == id;
            if incident
                && text(edge, "lifecycle") == "active"
                && (remerge.is_some() || later(edge_id))
            {
                return Err(blocked());
            }
        }
        let owned = retained.contents_of(id, read(0)?);
        read(owned.len())?;
        if owned.iter().any(|c| later(&text(c, "id"))) {
            return Err(blocked());
        }
        let referrers = retained.referrers(id, read(0)?);
        read(referrers.len())?;
        if referrers.iter().any(|r| later(r)) {
            return Err(blocked());
        }
        Ok(())
    }

    /// Steps 4 (node members, contents) and 5 of [`apply_inverse`].
    fn finish_inverse(&self) -> Result<(), Stop> {
        for entry in self.nodes.values() {
            let kind = text(&entry.after, "kind");
            let properties = entry
                .after
                .get("properties")
                .ok_or(Refusal::InvalidChange)?;
            check_node_properties(Some(&kind), properties)?;
        }
        for entry in self.contents.values() {
            if self
                .final_node(&text(&entry.after, "owner_node_id"))
                .is_none()
            {
                return Err(Refusal::InvalidChange.into());
            }
            if let Some(reference) = entry.after.get("body").and_then(|b| b.get("reference")) {
                let resource = reference
                    .get("resource_node_id")
                    .and_then(Value::as_str)
                    .and_then(|id| self.final_node(id));
                if !resource.is_some_and(|r| text(&r, "kind") == "Resource") {
                    return Err(Refusal::InvalidChange.into());
                }
            }
        }
        let guarded = |owner: &str| -> Result<(), Stop> {
            if self.verified_before(owner) && !self.updated.contains(owner) {
                return Err(Refusal::InvalidChange.into());
            }
            Ok(())
        };
        for entry in self.edges.values() {
            if text(&entry.after, "type") != "sources" {
                continue;
            }
            guarded(&text(&entry.after, "from_id"))?;
            if let Some(before) = &entry.before {
                guarded(&text(before, "from_id"))?;
            }
        }
        for entry in self.contents.values() {
            guarded(&text(&entry.after, "owner_node_id"))?;
        }
        Ok(())
    }
}
