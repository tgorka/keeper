//! The `graph.apply` payload: one change set (contract §4, §9.5).
//!
//! A payload is `{"ops": [<op>, …], "approval_id"?: <string>,
//! "event_time_ms"?: <integer> | null}`: 1 to [`MAX_OPERATIONS`] operations
//! applied in order. Every operation is an object whose `op` names it and
//! whose other members are exactly these (`?` optional):
//!
//! | `op` | members |
//! | --- | --- |
//! | `create_node` | `id`, `kind`, `title`, `description` (string or null), `properties` |
//! | `update_node` | `id`, `expected_revision`, `set`? (`{title?, description?}`), `set_properties`?, `remove_properties`? — at least one, none empty |
//! | `archive_node`, `unarchive_node` | `id`, `expected_revision` |
//! | `create_edge` | `id`, `from_id`, `to_id`, `type`, `properties`, `from_lifecycle_revision`, `to_lifecycle_revision` |
//! | `update_edge` | `id`, `expected_revision`, `set_properties`?, `remove_properties`? (at least one), `from_lifecycle_revision`, `to_lifecycle_revision` |
//! | `archive_edge`, `unarchive_edge` | `id`, `expected_revision`, `from_lifecycle_revision`, `to_lifecycle_revision` |
//! | `add_tag`, `remove_tag` | `node_id`, `tag`, `expected_revision` |
//!
//! Ids are ULIDs the caller mints for the nodes and edges it creates; a tag
//! is named by its text, which keeper's tag policy makes canonical, and a tag
//! or membership gets its id from the server. Every precondition is the
//! pre-change state: `expected_revision` the entity's accepted revision (a
//! tag operation's, its node's), an edge's `*_lifecycle_revision` its
//! endpoint's accepted `lifecycle_revision`; `null` (only where a node is
//! named) means that node is created by an earlier operation of this change.
//! `approval_id` is how an agent names the approval it acts under (the
//! server derives the basis; a person sends none). `event_time_ms` is the
//! source's time, never a concurrency token.
//!
//! What this module refuses needs nothing but the payload; what needs the
//! graph is [`crate::candidate`].

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use crate::symbol::{encode, SymbolKind};
use crate::value::{check_properties, normalize};
use crate::vocabulary::{stored_edge_type, stored_node_kind};
use crate::{is_decimal, is_ulid, Refusal};

/// The descriptor of the command whose payload this is.
pub const DESCRIPTOR: &str = "graph.apply";
/// The most operations one payload may hold (its event holds at most
/// [`crate::delta::MAX_OPERATIONS`] as well).
pub const MAX_OPERATIONS: usize = crate::delta::MAX_OPERATIONS;
/// The most bytes an `approval_id` may have.
pub const MAX_APPROVAL_ID_BYTES: usize = 256;
/// The largest `event_time_ms`: an exact JSON integer.
pub const MAX_EVENT_TIME_MS: u64 = 9_007_199_254_740_991;

/// A node's fields an `update_node` may set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeFields {
    pub title: Option<String>,
    /// `Some(None)` clears the description; `None` leaves it.
    pub description: Option<Option<String>>,
}

/// One operation of a change set. Tags hold the text as sent.
#[derive(Clone, Debug, PartialEq)]
pub enum ChangeOp {
    CreateNode {
        id: String,
        kind: String,
        title: String,
        description: Option<String>,
        properties: Map<String, Value>,
    },
    UpdateNode {
        id: String,
        expected_revision: String,
        set: NodeFields,
        set_properties: Map<String, Value>,
        remove_properties: Vec<String>,
    },
    ArchiveNode {
        id: String,
        expected_revision: String,
    },
    UnarchiveNode {
        id: String,
        expected_revision: String,
    },
    CreateEdge {
        id: String,
        from_id: String,
        to_id: String,
        edge_type: String,
        properties: Map<String, Value>,
        from_lifecycle_revision: Option<String>,
        to_lifecycle_revision: Option<String>,
    },
    UpdateEdge {
        id: String,
        expected_revision: String,
        set_properties: Map<String, Value>,
        remove_properties: Vec<String>,
        from_lifecycle_revision: String,
        to_lifecycle_revision: String,
    },
    ArchiveEdge {
        id: String,
        expected_revision: String,
        from_lifecycle_revision: String,
        to_lifecycle_revision: String,
    },
    UnarchiveEdge {
        id: String,
        expected_revision: String,
        from_lifecycle_revision: String,
        to_lifecycle_revision: String,
    },
    AddTag {
        node_id: String,
        tag: String,
        expected_revision: Option<String>,
    },
    RemoveTag {
        node_id: String,
        tag: String,
        expected_revision: Option<String>,
    },
}

/// A checked change set.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    ops: Vec<ChangeOp>,
    /// The canonical text of each operation's tag (`None` for the others).
    tags: Vec<Option<String>>,
    approval_id: Option<String>,
    event_time_ms: Option<u64>,
}

impl Change {
    /// Check `ops` as a change set, `canonical_tag` being the tag policy
    /// (keeper's `notes::tags::normalise`). The same rules as [`parse`].
    pub fn new(
        ops: Vec<ChangeOp>,
        approval_id: Option<String>,
        event_time_ms: Option<u64>,
        canonical_tag: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, Refusal> {
        if ops.len() > MAX_OPERATIONS {
            return Err(Refusal::ChangeLimit);
        }
        if approval_id.as_deref().is_some_and(|id| !approval_ok(id))
            || event_time_ms.is_some_and(|ms| ms > MAX_EVENT_TIME_MS)
        {
            return Err(Refusal::InvalidChange);
        }
        let mut ops = ops;
        for op in &mut ops {
            normalize_values(op)?;
        }
        let tags = Seen::default().check(&ops, &canonical_tag)?;
        Ok(Self {
            ops,
            tags,
            approval_id,
            event_time_ms,
        })
    }

    /// The operations, in the order they apply.
    #[must_use]
    pub fn ops(&self) -> &[ChangeOp] {
        &self.ops
    }

    /// The canonical tag of operation `index`, if it is a tag operation.
    #[must_use]
    pub fn canonical_tag(&self, index: usize) -> Option<&str> {
        self.tags.get(index).and_then(Option::as_deref)
    }

    #[must_use]
    pub fn approval_id(&self) -> Option<&str> {
        self.approval_id.as_deref()
    }

    #[must_use]
    pub fn event_time_ms(&self) -> Option<u64> {
        self.event_time_ms
    }

    /// The payload this change is sent as. [`parse`] reads it back to an
    /// equal change.
    #[must_use]
    pub fn payload(&self) -> Value {
        let mut out = Map::new();
        out.insert(
            "ops".into(),
            Value::Array(self.ops.iter().map(op_value).collect()),
        );
        if let Some(id) = &self.approval_id {
            out.insert("approval_id".into(), json!(id));
        }
        if let Some(ms) = self.event_time_ms {
            out.insert("event_time_ms".into(), json!(ms));
        }
        Value::Object(out)
    }
}

/// Read a `graph.apply` payload (a request's `payload`, numbers already
/// normalised by [`crate::request::parse`]). The operation count is checked
/// before any operation is read.
pub fn parse(
    payload: &Value,
    canonical_tag: impl Fn(&str) -> Option<String>,
) -> Result<Change, Refusal> {
    let fields = payload.as_object().ok_or(Refusal::InvalidChange)?;
    if fields
        .keys()
        .any(|key| !["ops", "approval_id", "event_time_ms"].contains(&key.as_str()))
    {
        return Err(Refusal::InvalidChange);
    }
    let Some(Value::Array(ops)) = fields.get("ops") else {
        return Err(Refusal::InvalidChange);
    };
    if ops.len() > MAX_OPERATIONS {
        return Err(Refusal::ChangeLimit);
    }
    let approval_id = match fields.get("approval_id") {
        None => None,
        Some(Value::String(id)) => Some(id.clone()),
        Some(_) => return Err(Refusal::InvalidChange),
    };
    let event_time_ms = match fields.get("event_time_ms") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_u64().ok_or(Refusal::InvalidChange)?),
    };
    let ops = ops.iter().map(read_op).collect::<Result<Vec<_>, _>>()?;
    Change::new(ops, approval_id, event_time_ms, canonical_tag)
}

/// Normalise the numbers of every value an operation writes, so the images
/// and rows it produces hold the contract's one number form.
fn normalize_values(op: &mut ChangeOp) -> Result<(), Refusal> {
    let map = match op {
        ChangeOp::CreateNode { properties, .. } | ChangeOp::CreateEdge { properties, .. } => {
            properties
        }
        ChangeOp::UpdateNode { set_properties, .. }
        | ChangeOp::UpdateEdge { set_properties, .. } => set_properties,
        _ => return Ok(()),
    };
    if let Value::Object(normal) = normalize(&Value::Object(std::mem::take(map)))? {
        *map = normal;
    }
    Ok(())
}

fn approval_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_APPROVAL_ID_BYTES && !id.chars().any(char::is_control)
}

/// What a change has said about each entity so far.
#[derive(Default)]
struct Seen {
    entities: BTreeMap<String, Entity>,
    /// (node id, canonical tag) memberships changed.
    memberships: BTreeSet<(String, String)>,
}

struct Entity {
    node: bool,
    created: bool,
    explicit: bool,
    /// A node's precondition as its operations named it so far: `None`
    /// until one names it, `Some(None)` for a node created here.
    revision: Option<Option<String>>,
    /// A node's lifecycle precondition as edges named it so far.
    lifecycle: Option<String>,
}

impl Seen {
    fn check(
        mut self,
        ops: &[ChangeOp],
        canonical_tag: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Vec<Option<String>>, Refusal> {
        if ops.is_empty() {
            return Err(Refusal::InvalidChange);
        }
        ops.iter().map(|op| self.op(op, canonical_tag)).collect()
    }

    fn op(
        &mut self,
        op: &ChangeOp,
        canonical_tag: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Option<String>, Refusal> {
        match op {
            ChangeOp::CreateNode {
                id,
                kind,
                title,
                properties,
                ..
            } => {
                stored_node_kind(kind)?;
                title_ok(title)?;
                check_properties(&Value::Object(properties.clone()))?;
                self.explicit(id, true, true, None)?;
            }
            ChangeOp::UpdateNode {
                id,
                expected_revision,
                set,
                set_properties,
                remove_properties,
            } => {
                revision_ok(expected_revision)?;
                if let Some(title) = &set.title {
                    title_ok(title)?;
                }
                let fields = set.title.is_some() || set.description.is_some();
                if !property_change(set_properties, remove_properties)? && !fields {
                    return Err(Refusal::InvalidChange);
                }
                self.explicit(id, true, false, Some(expected_revision))?;
            }
            ChangeOp::ArchiveNode {
                id,
                expected_revision,
            }
            | ChangeOp::UnarchiveNode {
                id,
                expected_revision,
            } => {
                revision_ok(expected_revision)?;
                self.explicit(id, true, false, Some(expected_revision))?;
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
                stored_edge_type(edge_type)?;
                check_properties(&Value::Object(properties.clone()))?;
                self.endpoint(from_id, from_lifecycle_revision.as_ref())?;
                self.endpoint(to_id, to_lifecycle_revision.as_ref())?;
                self.explicit(id, false, true, None)?;
            }
            ChangeOp::UpdateEdge {
                id,
                expected_revision,
                set_properties,
                remove_properties,
                from_lifecycle_revision,
                to_lifecycle_revision,
            } => {
                [
                    expected_revision,
                    from_lifecycle_revision,
                    to_lifecycle_revision,
                ]
                .into_iter()
                .try_for_each(|r| revision_ok(r))?;
                if !property_change(set_properties, remove_properties)? {
                    return Err(Refusal::InvalidChange);
                }
                self.explicit(id, false, false, None)?;
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
                [
                    expected_revision,
                    from_lifecycle_revision,
                    to_lifecycle_revision,
                ]
                .into_iter()
                .try_for_each(|r| revision_ok(r))?;
                self.explicit(id, false, false, None)?;
            }
            ChangeOp::AddTag {
                node_id,
                tag,
                expected_revision,
            }
            | ChangeOp::RemoveTag {
                node_id,
                tag,
                expected_revision,
            } => {
                let canonical = canonical_tag(tag).ok_or(Refusal::InvalidChange)?;
                if !self
                    .memberships
                    .insert((node_id.clone(), canonical.clone()))
                {
                    return Err(Refusal::InvalidChange);
                }
                self.named(node_id, expected_revision.as_ref(), false)?;
                return Ok(Some(canonical));
            }
        }
        Ok(None)
    }

    /// A create or an explicit field/lifecycle operation of `id`: at most
    /// one per entity.
    fn explicit(
        &mut self,
        id: &str,
        node: bool,
        create: bool,
        expected: Option<&String>,
    ) -> Result<(), Refusal> {
        if !is_ulid(id) {
            return Err(Refusal::InvalidChange);
        }
        match self.entities.get(id) {
            None => {
                self.entities.insert(
                    id.to_owned(),
                    Entity {
                        node,
                        created: create,
                        explicit: true,
                        revision: node.then(|| expected.cloned()),
                        lifecycle: None,
                    },
                );
                Ok(())
            }
            // Only tag operations or edges named it before: this operation
            // must agree with what they named.
            Some(entity) if entity.node && node && !entity.explicit && !create => {
                self.named(id, expected, true)
            }
            Some(_) => Err(Refusal::InvalidChange),
        }
    }

    /// Node `id` named under precondition `revision` by a tag operation (or,
    /// `explicit`, by the explicit operation following earlier mentions):
    /// every operation of the change naming the node names the same, `null`
    /// exactly when this change created it earlier.
    fn named(
        &mut self,
        id: &str,
        revision: Option<&String>,
        explicit: bool,
    ) -> Result<(), Refusal> {
        if !is_ulid(id) {
            return Err(Refusal::InvalidChange);
        }
        if let Some(revision) = revision {
            revision_ok(revision)?;
        }
        match self.entities.get_mut(id) {
            None if revision.is_some() => {
                self.entities.insert(
                    id.to_owned(),
                    Entity {
                        node: true,
                        created: false,
                        explicit,
                        revision: Some(revision.cloned()),
                        lifecycle: None,
                    },
                );
                Ok(())
            }
            Some(entity) if entity.node => {
                let agrees = match (&entity.revision, entity.created) {
                    (_, true) => revision.is_none(),
                    // Only edges named it so far: this names its revision.
                    (None, false) if revision.is_some() => {
                        entity.revision = Some(revision.cloned());
                        true
                    }
                    (Some(named), false) => revision.is_some() && named.as_ref() == revision,
                    (None, false) => false,
                };
                if !agrees {
                    return Err(Refusal::InvalidChange);
                }
                entity.explicit |= explicit;
                Ok(())
            }
            _ => Err(Refusal::InvalidChange),
        }
    }

    /// An edge endpoint: `null` names a node created earlier in this change,
    /// a lifecycle revision a node this change does not create — not later
    /// either: the endpoint is recorded, so a create of it afterwards
    /// refuses, and every edge naming it must name the same lifecycle
    /// revision.
    fn endpoint(&mut self, id: &str, revision: Option<&String>) -> Result<(), Refusal> {
        if !is_ulid(id) {
            return Err(Refusal::InvalidChange);
        }
        let Some(revision) = revision else {
            return match self.entities.get(id) {
                Some(entity) if entity.node && entity.created => Ok(()),
                _ => Err(Refusal::InvalidChange),
            };
        };
        revision_ok(revision)?;
        match self.entities.get_mut(id) {
            None => {
                self.entities.insert(
                    id.to_owned(),
                    Entity {
                        node: true,
                        created: false,
                        explicit: false,
                        revision: None,
                        lifecycle: Some(revision.clone()),
                    },
                );
                Ok(())
            }
            Some(entity) if entity.node && !entity.created => match &entity.lifecycle {
                Some(named) if named != revision => Err(Refusal::InvalidChange),
                _ => {
                    entity.lifecycle = Some(revision.clone());
                    Ok(())
                }
            },
            Some(_) => Err(Refusal::InvalidChange),
        }
    }
}

fn revision_ok(revision: &str) -> Result<(), Refusal> {
    if is_decimal(revision) && revision != "0" {
        Ok(())
    } else {
        Err(Refusal::InvalidChange)
    }
}

fn title_ok(title: &str) -> Result<(), Refusal> {
    if title.is_empty() {
        Err(Refusal::InvalidChange)
    } else {
        Ok(())
    }
}

/// Whether a set/remove pair changes properties. Refuses a key both set
/// and removed, a removal named twice and a key no property may have.
fn property_change(set: &Map<String, Value>, remove: &[String]) -> Result<bool, Refusal> {
    if !set.is_empty() {
        check_properties(&Value::Object(set.clone()))?;
    }
    let mut removed = BTreeSet::new();
    for key in remove {
        if !property_key(key) || set.contains_key(key) || !removed.insert(key) {
            return Err(Refusal::InvalidChange);
        }
    }
    Ok(!set.is_empty() || !remove.is_empty())
}

/// A custom property key: one [`check_properties`] admits.
fn property_key(key: &str) -> bool {
    !key.starts_with('$') && encode(SymbolKind::Property, key).is_ok()
}

const EDGE_MEMBERS: [&str; 4] = [
    "id",
    "expected_revision",
    "from_lifecycle_revision",
    "to_lifecycle_revision",
];

/// Read one operation object, its members exactly the ones its `op` has.
fn read_op(value: &Value) -> Result<ChangeOp, Refusal> {
    let fields = value.as_object().ok_or(Refusal::InvalidChange)?;
    let op = fields
        .get("op")
        .and_then(Value::as_str)
        .ok_or(Refusal::InvalidChange)?;
    let (required, optional): (&[&str], &[&str]) = match op {
        "create_node" => (&["id", "kind", "title", "description", "properties"], &[]),
        "update_node" => (
            &["id", "expected_revision"],
            &["set", "set_properties", "remove_properties"],
        ),
        "archive_node" | "unarchive_node" => (&["id", "expected_revision"], &[]),
        "create_edge" => (
            &[
                "id",
                "from_id",
                "to_id",
                "type",
                "properties",
                "from_lifecycle_revision",
                "to_lifecycle_revision",
            ],
            &[],
        ),
        "update_edge" => (&EDGE_MEMBERS, &["set_properties", "remove_properties"]),
        "archive_edge" | "unarchive_edge" => (&EDGE_MEMBERS, &[]),
        "add_tag" | "remove_tag" => (&["node_id", "tag", "expected_revision"], &[]),
        _ => return Err(Refusal::InvalidChange),
    };
    let r = Reader { fields };
    r.members(required, optional, true)?;
    Ok(match op {
        "create_node" => ChangeOp::CreateNode {
            id: r.text("id")?,
            kind: r.text("kind")?,
            title: r.text("title")?,
            description: r.nullable("description")?,
            properties: r.object("properties")?,
        },
        "update_node" => ChangeOp::UpdateNode {
            id: r.text("id")?,
            expected_revision: r.text("expected_revision")?,
            set: match fields.get("set") {
                None => NodeFields::default(),
                Some(set) => node_fields(set)?,
            },
            set_properties: r.optional_object("set_properties")?,
            remove_properties: r.optional_keys("remove_properties")?,
        },
        "archive_node" => ChangeOp::ArchiveNode {
            id: r.text("id")?,
            expected_revision: r.text("expected_revision")?,
        },
        "unarchive_node" => ChangeOp::UnarchiveNode {
            id: r.text("id")?,
            expected_revision: r.text("expected_revision")?,
        },
        "create_edge" => ChangeOp::CreateEdge {
            id: r.text("id")?,
            from_id: r.text("from_id")?,
            to_id: r.text("to_id")?,
            edge_type: r.text("type")?,
            properties: r.object("properties")?,
            from_lifecycle_revision: r.nullable("from_lifecycle_revision")?,
            to_lifecycle_revision: r.nullable("to_lifecycle_revision")?,
        },
        "update_edge" => ChangeOp::UpdateEdge {
            id: r.text("id")?,
            expected_revision: r.text("expected_revision")?,
            set_properties: r.optional_object("set_properties")?,
            remove_properties: r.optional_keys("remove_properties")?,
            from_lifecycle_revision: r.text("from_lifecycle_revision")?,
            to_lifecycle_revision: r.text("to_lifecycle_revision")?,
        },
        "archive_edge" => ChangeOp::ArchiveEdge {
            id: r.text("id")?,
            expected_revision: r.text("expected_revision")?,
            from_lifecycle_revision: r.text("from_lifecycle_revision")?,
            to_lifecycle_revision: r.text("to_lifecycle_revision")?,
        },
        "unarchive_edge" => ChangeOp::UnarchiveEdge {
            id: r.text("id")?,
            expected_revision: r.text("expected_revision")?,
            from_lifecycle_revision: r.text("from_lifecycle_revision")?,
            to_lifecycle_revision: r.text("to_lifecycle_revision")?,
        },
        "add_tag" => ChangeOp::AddTag {
            node_id: r.text("node_id")?,
            tag: r.text("tag")?,
            expected_revision: r.nullable("expected_revision")?,
        },
        "remove_tag" => ChangeOp::RemoveTag {
            node_id: r.text("node_id")?,
            tag: r.text("tag")?,
            expected_revision: r.nullable("expected_revision")?,
        },
        _ => return Err(Refusal::InvalidChange),
    })
}

/// An `update_node`'s `set`: a non-empty object of `title` and/or
/// `description`.
fn node_fields(set: &Value) -> Result<NodeFields, Refusal> {
    let fields = set.as_object().ok_or(Refusal::InvalidChange)?;
    if fields.is_empty() {
        return Err(Refusal::InvalidChange);
    }
    let r = Reader { fields };
    r.members(&[], &["title", "description"], false)?;
    Ok(NodeFields {
        title: match fields.get("title") {
            None => None,
            Some(_) => Some(r.text("title")?),
        },
        description: match fields.get("description") {
            None => None,
            Some(_) => Some(r.nullable("description")?),
        },
    })
}

struct Reader<'a> {
    fields: &'a Map<String, Value>,
}

impl Reader<'_> {
    /// Every `required` member present, nothing outside `required`,
    /// `optional` and, for an operation object (`op`), its discriminator.
    fn members(&self, required: &[&str], optional: &[&str], op: bool) -> Result<(), Refusal> {
        let known =
            |key: &str| (op && key == "op") || required.contains(&key) || optional.contains(&key);
        if required.iter().all(|key| self.fields.contains_key(*key))
            && self.fields.keys().all(|key| known(key))
        {
            Ok(())
        } else {
            Err(Refusal::InvalidChange)
        }
    }

    fn text(&self, key: &str) -> Result<String, Refusal> {
        match self.fields.get(key) {
            Some(Value::String(text)) => Ok(text.clone()),
            _ => Err(Refusal::InvalidChange),
        }
    }

    fn nullable(&self, key: &str) -> Result<Option<String>, Refusal> {
        match self.fields.get(key) {
            Some(Value::Null) => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.clone())),
            _ => Err(Refusal::InvalidChange),
        }
    }

    fn object(&self, key: &str) -> Result<Map<String, Value>, Refusal> {
        match self.fields.get(key) {
            Some(Value::Object(object)) => Ok(object.clone()),
            _ => Err(Refusal::InvalidChange),
        }
    }

    /// An optional object: absent is empty, present must not be.
    fn optional_object(&self, key: &str) -> Result<Map<String, Value>, Refusal> {
        match self.fields.get(key) {
            None => Ok(Map::new()),
            Some(Value::Object(object)) if !object.is_empty() => Ok(object.clone()),
            Some(_) => Err(Refusal::InvalidChange),
        }
    }

    /// An optional array of strings: absent is empty, present must not be.
    fn optional_keys(&self, key: &str) -> Result<Vec<String>, Refusal> {
        match self.fields.get(key) {
            None => Ok(Vec::new()),
            Some(Value::Array(items)) if !items.is_empty() => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_owned)
                        .ok_or(Refusal::InvalidChange)
                })
                .collect(),
            Some(_) => Err(Refusal::InvalidChange),
        }
    }
}

fn op_value(op: &ChangeOp) -> Value {
    let mut out = Map::new();
    let mut put = |key: &str, value: Value| {
        out.insert(key.to_owned(), value);
    };
    match op {
        ChangeOp::CreateNode {
            id,
            kind,
            title,
            description,
            properties,
        } => {
            put("op", json!("create_node"));
            put("id", json!(id));
            put("kind", json!(kind));
            put("title", json!(title));
            put("description", json!(description));
            put("properties", Value::Object(properties.clone()));
        }
        ChangeOp::UpdateNode {
            id,
            expected_revision,
            set,
            set_properties,
            remove_properties,
        } => {
            put("op", json!("update_node"));
            put("id", json!(id));
            put("expected_revision", json!(expected_revision));
            let mut fields = Map::new();
            if let Some(title) = &set.title {
                fields.insert("title".into(), json!(title));
            }
            if let Some(description) = &set.description {
                fields.insert("description".into(), json!(description));
            }
            if !fields.is_empty() {
                put("set", Value::Object(fields));
            }
            properties_members(&mut put, set_properties, remove_properties);
        }
        ChangeOp::ArchiveNode {
            id,
            expected_revision,
        }
        | ChangeOp::UnarchiveNode {
            id,
            expected_revision,
        } => {
            let name = if matches!(op, ChangeOp::ArchiveNode { .. }) {
                "archive_node"
            } else {
                "unarchive_node"
            };
            put("op", json!(name));
            put("id", json!(id));
            put("expected_revision", json!(expected_revision));
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
            put("op", json!("create_edge"));
            put("id", json!(id));
            put("from_id", json!(from_id));
            put("to_id", json!(to_id));
            put("type", json!(edge_type));
            put("properties", Value::Object(properties.clone()));
            put("from_lifecycle_revision", json!(from_lifecycle_revision));
            put("to_lifecycle_revision", json!(to_lifecycle_revision));
        }
        ChangeOp::UpdateEdge {
            id,
            expected_revision,
            set_properties,
            remove_properties,
            from_lifecycle_revision,
            to_lifecycle_revision,
        } => {
            put("op", json!("update_edge"));
            put("id", json!(id));
            put("expected_revision", json!(expected_revision));
            properties_members(&mut put, set_properties, remove_properties);
            put("from_lifecycle_revision", json!(from_lifecycle_revision));
            put("to_lifecycle_revision", json!(to_lifecycle_revision));
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
            let name = if matches!(op, ChangeOp::ArchiveEdge { .. }) {
                "archive_edge"
            } else {
                "unarchive_edge"
            };
            put("op", json!(name));
            put("id", json!(id));
            put("expected_revision", json!(expected_revision));
            put("from_lifecycle_revision", json!(from_lifecycle_revision));
            put("to_lifecycle_revision", json!(to_lifecycle_revision));
        }
        ChangeOp::AddTag {
            node_id,
            tag,
            expected_revision,
        }
        | ChangeOp::RemoveTag {
            node_id,
            tag,
            expected_revision,
        } => {
            let name = if matches!(op, ChangeOp::AddTag { .. }) {
                "add_tag"
            } else {
                "remove_tag"
            };
            put("op", json!(name));
            put("node_id", json!(node_id));
            put("tag", json!(tag));
            put("expected_revision", json!(expected_revision));
        }
    }
    Value::Object(out)
}

fn properties_members(
    put: &mut impl FnMut(&str, Value),
    set: &Map<String, Value>,
    remove: &[String],
) {
    if !set.is_empty() {
        put("set_properties", Value::Object(set.clone()));
    }
    if !remove.is_empty() {
        put("remove_properties", json!(remove));
    }
}
