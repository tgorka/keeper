//! What one change set does to a graph: the shared acceptance algebra
//! (contract §4, §9.5, §9.6). Pure: the graph is read through [`State`]
//! (the accepted, pre-change state), nothing is written, and ids the server
//! mints (tags, memberships) come from the caller's `mint`.
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
//! `contains` edge created or unarchived; then the event's bounds
//! ([`crate::delta::digest`]). The receipt's `affected` lists nodes, edges
//! and new tags in order of their first touch by the change, which may
//! differ from the order of their event operations.
//!
//! **Revisions.** A created node or edge starts at revision 1 (a node also at
//! lifecycle revision 1, active); a new tag at revision 1. A node touched by
//! this change in any way — fields, tag memberships, lifecycle — moves its
//! revision by exactly one; an archive or unarchive also moves its lifecycle
//! revision by one. An edge operation moves the edge's revision by one and
//! never its endpoints'.
//!
//! **Event operations,** one or more per change operation, in its order:
//! `create_node`, `update_node`, `archive_node`, `unarchive_node` give one
//! node operation of that name; the first tag operation of a node that no
//! explicit operation of this change names gives one node `update` (its
//! revision move) before its own; `add_tag` gives a tag `create` when no tag
//! has the text yet, then a membership `create`; `remove_tag` a membership
//! `remove`; an edge operation one edge operation of its name. A node or
//! edge image is the entity's whole row as it is before the change and after
//! all of it (never an intermediate state): node `{id, kind, title,
//! description, description_source_id, properties, lifecycle,
//! merged_into_id, revision, lifecycle_revision}`, edge `{id, from_id,
//! to_id, type, properties, lifecycle, revision}`, tag `{id, text, label,
//! revision}`, membership `{id, node_id, tag_id}`. A new tag's `label` is
//! null.
//!
//! **Containment.** The active `contains` edges of the final candidate stay
//! acyclic. For each `contains` edge `P → C` created or unarchived, the walk
//! goes up from `P` one level at a time: a level is every active `contains`
//! edge of the final candidate into the current frontier; a level past
//! [`MAX_CONTAINMENT_DEPTH`], or more than [`MAX_CONTAINMENT_EDGES`] edges
//! read by all walks of the change together, refuses `change_limit`; a level
//! holding an edge from `C` (or `P` = `C`) refuses `invalid_change`; the
//! frontier is then the level's new `from` nodes. Reads are bounded as they
//! are made: a level is fetched with a limit of one more edge than the
//! budget still allows ([`State::contains_into`]), never whole.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use crate::change::{Change, ChangeOp};
use crate::delta::{self, Op};
use crate::value::{check_properties, typed_ref, RefType};
use crate::vocabulary::{EntityType, Operation};
use crate::Refusal;

/// The most levels a containment walk may climb.
pub const MAX_CONTAINMENT_DEPTH: usize = 64;
/// The most `contains` edges the containment walks of one change may read.
pub const MAX_CONTAINMENT_EDGES: usize = 10_000;

/// The accepted graph a change is judged against. Every answer is an image
/// in the shapes above.
pub trait State {
    /// The node with `id`, any lifecycle.
    fn node(&self, id: &str) -> Option<Map<String, Value>>;
    /// The edge with `id`, any lifecycle.
    fn edge(&self, id: &str) -> Option<Map<String, Value>>;
    /// The tag whose canonical text is `text`.
    fn tag(&self, text: &str) -> Option<Map<String, Value>>;
    /// The membership of node `node_id` in tag `tag_id`.
    fn membership(&self, node_id: &str, tag_id: &str) -> Option<Map<String, Value>>;
    /// The accepted active `contains` edges whose `to_id` is one of `to_ids`,
    /// ascending by id, **at most `limit`** of them. The walk asks for one
    /// more edge than it may still read (plus any it will replace with this
    /// change's own), so a full answer proves the budget exceeded without
    /// reading the rest.
    fn contains_into(&self, to_ids: &[String], limit: usize) -> Vec<Map<String, Value>>;
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
    /// with `lifecycle_revision`) for each node, edge and new tag, in order
    /// of their first operation.
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
pub fn apply(state: &dyn State, change: &Change, mint: &mut dyn FnMut() -> String) -> Verdict {
    let mut candidate = Candidate::new(state, change);
    match candidate.run(change, mint) {
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
}

struct Candidate<'a> {
    state: &'a dyn State,
    nodes: BTreeMap<String, Entry>,
    edges: BTreeMap<String, Entry>,
    /// New tags by canonical text.
    tags: BTreeMap<String, Image>,
    /// Nodes an explicit operation of the change names.
    explicit: BTreeSet<String>,
    /// Nodes whose tag-only `update` has been emitted.
    emitted: BTreeSet<String>,
    /// Values written, to check their typed references.
    written: Vec<Value>,
    /// Edges whose endpoints must be active (if the edge is), in order.
    endpoint_checks: Vec<String>,
    /// `contains` edges to walk, in order.
    containment: Vec<String>,
    slots: Vec<Slot>,
    /// Nodes, edges and new tags (by text) in order of first touch: the
    /// receipt's `affected` order.
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
    Stop::Conflict(json!({
        "entity": { "entity_type": entity_type, "id": id },
        "precondition": precondition,
        "current_revision": current,
    }))
}

fn revision_le(a: &str, b: &str) -> bool {
    matches!((a.parse::<u128>(), b.parse::<u128>()), (Ok(a), Ok(b)) if a <= b)
}

impl<'a> Candidate<'a> {
    fn new(state: &'a dyn State, change: &Change) -> Self {
        let explicit = change
            .ops()
            .iter()
            .filter_map(|op| match op {
                ChangeOp::CreateNode { id, .. }
                | ChangeOp::UpdateNode { id, .. }
                | ChangeOp::ArchiveNode { id, .. }
                | ChangeOp::UnarchiveNode { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect();
        Self {
            state,
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            tags: BTreeMap::new(),
            explicit,
            emitted: BTreeSet::new(),
            written: Vec::new(),
            endpoint_checks: Vec::new(),
            containment: Vec::new(),
            slots: Vec::new(),
            order: Vec::new(),
        }
    }

    fn run(&mut self, change: &Change, mint: &mut dyn FnMut() -> String) -> Result<Effects, Stop> {
        for (index, op) in change.ops().iter().enumerate() {
            self.op(op, change.canonical_tag(index), mint)?;
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

    /// A create's id must be new to the graph, as a node or an edge.
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
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn op(
        &mut self,
        op: &ChangeOp,
        tag: Option<&str>,
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
                }
                if !set_properties.is_empty() || !remove_properties.is_empty() {
                    edit_properties(&mut entry.after, set_properties, remove_properties)?;
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
                edit_properties(&mut entry.after, set_properties, remove_properties)?;
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
        }
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

    /// Every typed reference in `value` names a node of the final candidate
    /// at a revision it has had; content references refuse until content
    /// exists.
    fn references(&self, value: &Value) -> Result<(), Stop> {
        if let Some(reference) = typed_ref(value)? {
            if reference.entity_type == RefType::Content {
                return Err(Refusal::InvalidChange.into());
            }
            let node = self
                .final_node(&reference.id)
                .ok_or(Refusal::InvalidChange)?;
            if let Some(revision) = &reference.revision {
                if revision == "0" || !revision_le(revision, &text(&node, "revision")) {
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
                    _ => {
                        let tag = self.tags.get(key).ok_or(Refusal::InvalidChange)?;
                        json!({"entity_type": "tag", "id": tag.get("id"), "revision": "1"})
                    }
                })
            })
            .collect()
    }
}

fn object(value: Value) -> Image {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// Set and remove properties of an image; removing a key it lacks refuses,
/// and the result must still be a valid properties object.
fn edit_properties(
    image: &mut Image,
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
    check_properties(&properties)?;
    image.insert("properties".into(), properties);
    Ok(())
}
