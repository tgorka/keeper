//! An agent's capability in one domain, as the operator's admin path writes
//! it ([`AdminCommand::SetAgentGrant`]) and lists it back
//! ([`AgentGrantRecord`]) (stories 114.3, 114.6).
//!
//! The graph part is the knowledge graph's capability grammar: six classes,
//! the reserved kinds, collection and guidance roots, `domainWideContent`.
//! `classes` is empty only beside a non-empty `files`, every other graph
//! member then empty or absent: a files-only grant, which gives no graph
//! change, so no graph event's grant basis cites it. Two members ride
//! beside the graph part, each non-empty when given:
//!
//! - `rootModes`: the mode each collection root's `content`-class changes
//!   land by — [`GrantMode::Ask`] (a person approves each call, and every
//!   root not named), [`GrantMode::Propose`] or [`GrantMode::Apply`]. A key
//!   is one of `collectionRoots`; guidance, vocabulary and what
//!   `domainWideContent` alone covers stay `ask`. These are an agent
//!   instance's modes: a tool has no mode per root, and its changes inside
//!   its whole grant land `apply`.
//! - `files`: the file part, a list of [`FileItem`]s `{roots, classes,
//!   mode}`. Its shape is frozen here; the words its `classes` and `mode`
//!   may hold are the files module's, so they are carried as given.
//!
//! A capability is stored as given: an optional member absent stays absent,
//! and a parser refuses one given as `null`, a member it does not know, or
//! one the grammar does not admit. A listed capability is always one the
//! grammar accepted, so the list answer reads it with the same type: a
//! server that adds a member changes this contract first.
//!
//! [`AdminCommand::SetAgentGrant`]: crate::admin::AdminCommand::SetAgentGrant

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize};

use crate::admin::{present, Issuer};

/// The organisation's one general domain: no agent holds anything there, so
/// an agent grant naming it does not parse.
pub const GENERAL_DOMAIN: &str = "makidrive";

/// The most roots one root list holds.
pub const MAX_ROOTS: usize = 256;

/// The most UTF-8 bytes one node id holds.
pub const MAX_NODE_ID_BYTES: usize = 128;

/// The graph's reserved kinds: the only ones a grant's `allowedKinds` lists.
/// `Vocabulary` is the `vocabulary` class's alone and never one of them.
pub const RESERVED_KINDS: [&str; 5] = ["Concept", "Collection", "Resource", "Tag", "Guidance"];

/// What an agent may do to a domain's graph. Review, ownership and policy
/// are people's alone: no class names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityClass {
    Read,
    Content,
    Vocabulary,
    GuidanceBody,
    GuidanceScope,
    ReverseOwn,
}

impl CapabilityClass {
    /// Every class, in the contract's order.
    pub const ALL: [Self; 6] = [
        Self::Read,
        Self::Content,
        Self::Vocabulary,
        Self::GuidanceBody,
        Self::GuidanceScope,
        Self::ReverseOwn,
    ];

    /// Whether the class writes; only `read` does not.
    #[must_use]
    pub const fn writes(self) -> bool {
        !matches!(self, Self::Read)
    }
}

/// How a change inside a grant lands: a person approves each call (`ask`),
/// it goes as a proposal a person decides (`propose`), or it is accepted at
/// once and listed for a reviewer (`apply`). The same three words are the
/// `mode` of a graph event's `{basis: "grant", …}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantMode {
    Ask,
    Propose,
    Apply,
}

impl GrantMode {
    /// Every mode, in the contract's order.
    pub const ALL: [Self; 3] = [Self::Ask, Self::Propose, Self::Apply];
}

/// One item of a capability's file part: file `classes` over the directory
/// entries `roots`, landing by `mode`. Every member is required; the
/// vocabulary of `classes` and `mode` is the files module's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileItem {
    pub roots: Vec<String>,
    pub classes: Vec<String>,
    pub mode: String,
}

/// A capability's members as written; [`Capability`] is one the grammar
/// accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityFields {
    pub classes: Vec<CapabilityClass>,
    pub allowed_kinds: Vec<String>,
    pub collection_roots: Vec<String>,
    pub guidance_roots: Vec<String>,
    /// Absent reads as `false`, and stays absent.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub domain_wide_content: Option<bool>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub root_modes: Option<BTreeMap<String, GrantMode>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub files: Option<Vec<FileItem>>,
}

/// An agent's capability in one domain that the grammar accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CapabilityFields", into = "CapabilityFields")]
pub struct Capability(CapabilityFields);

/// Why the grammar refuses a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityError {
    /// `classes` is empty without a non-empty `files`, or lists a class
    /// twice.
    Classes,
    /// `allowedKinds` lists a kind twice, one that is not reserved, or any
    /// kind without a write class.
    AllowedKinds,
    /// A root list holds a value twice, more than [`MAX_ROOTS`], or a
    /// malformed node id.
    Roots,
    /// `collectionRoots` without `content` or `reverse_own`, or `content`
    /// with neither `collectionRoots` nor `domainWideContent`.
    CollectionRoots,
    /// A guidance class without `guidanceRoots`, or roots without one.
    GuidanceRoots,
    /// `domainWideContent` without `content`.
    DomainWideContent,
    /// `rootModes` given empty, or a key that is not one of
    /// `collectionRoots`.
    RootModes,
    /// `files` given empty, or a file item with no root, no class, an empty
    /// or repeated value, or an empty mode.
    Files,
}

impl std::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = match self {
            Self::Classes => "classes",
            Self::AllowedKinds => "allowedKinds",
            Self::Roots => "roots",
            Self::CollectionRoots => "collectionRoots",
            Self::GuidanceRoots => "guidanceRoots",
            Self::DomainWideContent => "domainWideContent",
            Self::RootModes => "rootModes",
            Self::Files => "files",
        };
        write!(f, "capability refused: {what}")
    }
}

impl std::error::Error for CapabilityError {}

impl Capability {
    /// The members as written.
    #[must_use]
    pub fn fields(&self) -> &CapabilityFields {
        &self.0
    }

    /// Whether the capability holds `class`.
    #[must_use]
    pub fn holds(&self, class: CapabilityClass) -> bool {
        self.0.classes.contains(&class)
    }

    /// The mode an agent instance's `content`-class changes under `root`
    /// land by: `ask` unless `rootModes` names another. A tool's grant has
    /// no mode per root: its changes inside the whole grant land `apply`.
    /// Whether the holder is a tool is the credential file's, not the
    /// capability's, so this type cannot tell; the caller that knows the
    /// holder picks the rule.
    #[must_use]
    pub fn mode_of(&self, root: &str) -> GrantMode {
        self.0
            .root_modes
            .as_ref()
            .and_then(|modes| modes.get(root).copied())
            .unwrap_or(GrantMode::Ask)
    }
}

impl TryFrom<CapabilityFields> for Capability {
    type Error = CapabilityError;

    fn try_from(fields: CapabilityFields) -> Result<Self, Self::Error> {
        check(&fields)?;
        Ok(Self(fields))
    }
}

impl From<Capability> for CapabilityFields {
    fn from(capability: Capability) -> Self {
        capability.0
    }
}

fn distinct<T: Ord>(items: &[T]) -> bool {
    items.iter().collect::<BTreeSet<_>>().len() == items.len()
}

fn node_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_NODE_ID_BYTES
        && !id
            .chars()
            .any(|c| c.is_ascii_control() || c == '/' || c == ':')
}

fn roots(ids: &[String]) -> bool {
    ids.len() <= MAX_ROOTS && distinct(ids) && ids.iter().all(|id| node_id(id))
}

fn words(items: &[String]) -> bool {
    !items.is_empty() && distinct(items) && items.iter().all(|item| !item.is_empty())
}

fn check(c: &CapabilityFields) -> Result<(), CapabilityError> {
    use CapabilityClass::{Content, GuidanceBody, GuidanceScope, ReverseOwn};
    use CapabilityError as E;
    let has = |class| c.classes.contains(&class);
    let has_files = c.files.as_ref().is_some_and(|files| !files.is_empty());
    if (c.classes.is_empty() && !has_files) || !distinct(&c.classes) {
        return Err(E::Classes);
    }
    let writes = c.classes.iter().any(|class| class.writes());
    if !distinct(&c.allowed_kinds)
        || c.allowed_kinds
            .iter()
            .any(|kind| !RESERVED_KINDS.contains(&kind.as_str()))
        || (!writes && !c.allowed_kinds.is_empty())
    {
        return Err(E::AllowedKinds);
    }
    if !roots(&c.collection_roots) || !roots(&c.guidance_roots) {
        return Err(E::Roots);
    }
    if !c.collection_roots.is_empty() && !has(Content) && !has(ReverseOwn) {
        return Err(E::CollectionRoots);
    }
    let guidance = has(GuidanceBody) || has(GuidanceScope);
    if guidance == c.guidance_roots.is_empty() {
        return Err(E::GuidanceRoots);
    }
    let domain_wide = c.domain_wide_content == Some(true);
    if domain_wide && !has(Content) {
        return Err(E::DomainWideContent);
    }
    if has(Content) && c.collection_roots.is_empty() && !domain_wide {
        return Err(E::CollectionRoots);
    }
    if let Some(modes) = &c.root_modes {
        if modes.is_empty() || modes.keys().any(|root| !c.collection_roots.contains(root)) {
            return Err(E::RootModes);
        }
    }
    if let Some(files) = &c.files {
        if files.is_empty()
            || files
                .iter()
                .any(|item| !words(&item.roots) || !words(&item.classes) || item.mode.is_empty())
        {
            return Err(E::Files);
        }
    }
    Ok(())
}

/// A domain an agent grant names: any but [`GENERAL_DOMAIN`].
pub(crate) fn agent_domain<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let name = String::deserialize(deserializer)?;
    if name == GENERAL_DOMAIN {
        return Err(serde::de::Error::custom(
            "no agent grant names the general domain",
        ));
    }
    Ok(name)
}

/// One agent grant as the admin path lists it: the capability as the
/// request gave it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentGrantRecord {
    pub domain: String,
    pub agent_id: String,
    pub issuer: Issuer,
    /// The map revision that wrote the grant: a graph event's
    /// `grant_revision` under it.
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<i64>,
    pub capability: Capability,
}

impl AgentGrantRecord {
    /// The grant's id, `<domain>/<agent_id>`: a graph event's `grant_id`
    /// under it. A domain name holds no `/`, so the id splits at its first.
    #[must_use]
    pub fn grant_id(&self) -> String {
        format!("{}/{}", self.domain, self.agent_id)
    }
}
