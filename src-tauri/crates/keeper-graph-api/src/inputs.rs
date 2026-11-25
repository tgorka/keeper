//! The graph operations' inputs. Every one names the `(server, domain)`
//! pair it addresses at `/target`.

use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// The domain an operation addresses: the agents server's address and the
/// database.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub server: String,
    pub domain: String,
}

/// A payload operation's input: the contract's payload object, with
/// `target` beside its own keys (the payload's keys are read by the
/// protocol's hand-written rules, `change::parse` and `proposal::parse`).
#[derive(Clone, Debug, PartialEq)]
pub struct Targeted {
    pub target: Target,
    pub payload: Map<String, Value>,
}

impl Serialize for Targeted {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = self.payload.clone();
        let target = serde_json::to_value(&self.target).map_err(serde::ser::Error::custom)?;
        map.insert("target".into(), target);
        map.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Targeted {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut payload = Map::deserialize(deserializer)?;
        let target = payload
            .remove("target")
            .ok_or_else(|| D::Error::missing_field("target"))?;
        let target = Target::deserialize(target).map_err(D::Error::custom)?;
        Ok(Self { target, payload })
    }
}

/// `graph.annotate` (116.5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Annotate {
    pub target: Target,
    pub entry_id: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// `graph.pending`, `graph.resume`, `graph.status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TargetOnly {
    pub target: Target,
}

/// `graph.discard`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Discard {
    pub target: Target,
    pub request_id: String,
}

fn sixty() -> u64 {
    60
}

/// `graph.sync`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sync {
    pub target: Target,
    /// How long to wait for the domain's graph, in seconds.
    #[serde(default = "sixty")]
    pub wait_secs: u64,
}

fn empty_params() -> String {
    "{}".into()
}

/// `graph.query`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub target: Target,
    /// One `KG-Cypher/1` query.
    pub source: String,
    /// The query's parameters, as JSON text.
    #[serde(default = "empty_params")]
    pub params: String,
    #[serde(default)]
    pub allow_stale: bool,
}

/// What `graph.inspect` looks up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Entity {
    Node,
    Edge,
    Content,
}

/// `graph.inspect`. `entity` is read as text so that another word keeps the
/// command line's own answer (`query_unsupported`); its schema is the enum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Inspect {
    pub target: Target,
    #[schemars(with = "Entity")]
    pub entity: String,
    pub id: String,
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default)]
    pub allow_stale: bool,
}

fn zero() -> String {
    "0".into()
}

fn hundred() -> u32 {
    100
}

/// `graph.history`, `graph.unreviewed`: one page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub target: Target,
    #[serde(default = "zero")]
    pub after: String,
    #[serde(default = "hundred")]
    pub limit: u32,
}

/// `graph.proposal`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposalRead {
    pub target: Target,
    pub proposal_id: String,
}

/// `graph.acknowledge`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Acknowledge {
    pub target: Target,
    pub event_id: String,
    pub delta_digest: String,
}

/// `graph.preview_reversal`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventId {
    pub target: Target,
    pub event_id: String,
}

/// `graph.reverse`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Reverse {
    pub target: Target,
    pub event_id: String,
    /// The per-call approval an agent's reversal needs.
    #[serde(default)]
    pub approval_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_targeted_input_is_the_payload_beside_its_target() {
        let wire = json!({"ops": [], "target": {"server": "s", "domain": "d"}});
        let input: Targeted = serde_json::from_value(wire.clone()).expect("targeted");
        assert_eq!(input.target.domain, "d");
        assert_eq!(Value::Object(input.payload.clone()), json!({"ops": []}));
        assert_eq!(serde_json::to_value(&input).expect("json"), wire);
        let missing = serde_json::from_value::<Targeted>(json!({"ops": []}));
        assert!(missing
            .expect_err("no target")
            .to_string()
            .contains("target"));
        let extra = serde_json::from_value::<Targeted>(
            json!({"target": {"server": "s", "domain": "d", "x": 1}}),
        );
        assert!(extra.is_err());
    }
}
