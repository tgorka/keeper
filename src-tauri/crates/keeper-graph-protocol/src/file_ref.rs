//! A graph node that points at a file (story 116.5; README *The knowledge
//! graph*, **Files**): the entry a node names, the one-node rule, and the
//! `kg_file_refs` row each change leaves.
//!
//! A `Resource` names an entry by its `$locator` (`kind` `internal`, text
//! `files:<domain>/<entry_id>`, 116.1's decision 1); a `Collection` names a
//! `dir` entry by its `okf:directory` property holding the same text. No
//! other node, and no other locator text (`keeper:drive/notes` included),
//! names an entry.
//!
//! **One node per entry.** In a final candidate at most one active node
//! names an entry, whatever its kind. A change leaving a second one active
//! is a `conflict` whose precondition is [`PRECONDITION`], naming the other
//! active node ([`judge`]).
//!
//! **The row** (H-10). `kg_file_refs` maps an entry to the node that
//! [`counts`] for it: the active one; else the one archived last; a node
//! merged away counts only when its survivor names the entry too. The row
//! is written in the change's own commit ([`Write`]); no event operation
//! carries it, so the delta digest never covers it.
//!
//! What only the server knows — whether the entry exists in the graph's
//! domain and whether it is a `dir` — is [`admit`]'s, given the domain's
//! manifest.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{is_ulid, locator, Refusal};

/// The table.
pub const TABLE: &str = "kg_file_refs";
/// The prefix of an entry's `internal` locator text.
pub const PREFIX: &str = "files:";
/// The `Collection` property binding it to a `dir` entry.
pub const DIRECTORY: &str = "okf:directory";
/// The property of the domain's root `Collection`, which is never bound.
pub const PROFILE_VERSION: &str = "okf:profile_version";
/// The conflict precondition of the one-node rule.
pub const PRECONDITION: &str = "file_ref";
/// Reserved for generated text and unused: no node carries it (J-1, M-7).
pub const SUMMARY: &str = "okf:summary";
/// Reserved for the provenance of generated text and unused (L-6).
pub const PROVENANCE: [&str; 4] = [
    "okf:model",
    "okf:prompt",
    "okf:generator",
    "okf:source_content_id",
];

/// The most `merged_into_id` hops [`counts`] follows to a survivor.
const MAX_HOPS: usize = crate::candidate::MAX_REDIRECT_HOPS;

/// An entry a locator text names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub domain: String,
    pub entry_id: String,
}

/// The `internal` locator text of `entry_id` in `domain`.
#[must_use]
pub fn text(domain: &str, entry_id: &str) -> String {
    format!("{PREFIX}{domain}/{entry_id}")
}

/// The entry `text` names: `files:<domain>/<entry_id>`, the domain
/// non-empty without `/`, the entry id a ULID. Anything else names none.
#[must_use]
pub fn parse(text: &str) -> Option<Target> {
    let (domain, entry_id) = text.strip_prefix(PREFIX)?.split_once('/')?;
    (!domain.is_empty() && is_ulid(entry_id)).then(|| Target {
        domain: domain.to_owned(),
        entry_id: entry_id.to_owned(),
    })
}

/// The text node image `node` would name an entry by: a `Resource`'s
/// `internal` `$locator`, a `Collection`'s `okf:directory`.
fn entry_text(node: &Map<String, Value>) -> Option<&str> {
    let properties = node.get("properties")?;
    match node.get("kind")?.as_str()? {
        "Resource" => {
            let envelope = properties.get(locator::MEMBER)?;
            if envelope.get("kind")?.as_str()? != "internal" {
                return None;
            }
            envelope.get("locator")?.as_str()
        }
        "Collection" => properties.get(DIRECTORY)?.as_str(),
        _ => None,
    }
}

/// The entry node image `node` names by its own members: a `Resource`'s
/// `internal` `$locator`, a `Collection`'s `okf:directory`.
#[must_use]
pub fn named(node: &Map<String, Value>) -> Option<Target> {
    entry_text(node).and_then(parse)
}

fn active(node: &Map<String, Value>) -> bool {
    node.get("lifecycle").and_then(Value::as_str) == Some("active")
}

fn id(node: &Map<String, Value>) -> &str {
    node.get("id").and_then(Value::as_str).unwrap_or_default()
}

/// Whether `node` counts for `entry_id`: it names the entry and, when
/// merged away, the end of its redirect chain names it too. `node_of`
/// reads final images.
pub fn counts(
    node: &Map<String, Value>,
    entry_id: &str,
    node_of: &dyn Fn(&str) -> Option<Map<String, Value>>,
) -> bool {
    if named(node).is_none_or(|t| t.entry_id != entry_id) {
        return false;
    }
    let names = |n: &Map<String, Value>| named(n).is_some_and(|t| t.entry_id == entry_id);
    let mut current = node.clone();
    for _ in 0..=MAX_HOPS {
        match current.get("merged_into_id").and_then(Value::as_str) {
            None => return names(&current),
            Some(next) => match node_of(next) {
                Some(survivor) => current = survivor,
                None => return false,
            },
        }
    }
    false
}

/// One `kg_file_refs` row, without its `graph` scope column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub entry_id: String,
    pub node_id: String,
    pub node_kind: String,
}

impl Row {
    /// The row read back from a replica or state image; `None` when it is
    /// not one.
    #[must_use]
    pub fn of(image: &Map<String, Value>) -> Option<Self> {
        let field = |k: &str| image.get(k).and_then(Value::as_str).map(str::to_owned);
        Some(Self {
            entry_id: field("entry_id")?,
            node_id: field("node_id")?,
            node_kind: field("node_kind")?,
        })
    }

    fn naming(entry_id: &str, node: &Map<String, Value>) -> Self {
        Self {
            entry_id: entry_id.to_owned(),
            node_id: id(node).to_owned(),
            node_kind: node
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        }
    }
}

/// What a change's commit does to one row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "write", rename_all = "snake_case")]
pub enum Write {
    Upsert(Row),
    Delete { entry_id: String },
}

/// A node the change wrote: its accepted image (none when created) and its
/// final one.
#[derive(Clone, Copy, Debug)]
pub struct Touched<'a> {
    pub before: Option<&'a Map<String, Value>>,
    pub after: &'a Map<String, Value>,
}

/// The entries the written nodes name, before or after, ascending.
#[must_use]
pub fn entries(touched: &[Touched<'_>]) -> Vec<String> {
    let mut ids: Vec<String> = touched
        .iter()
        .flat_map(|t| [t.before.and_then(named), named(t.after)])
        .flatten()
        .map(|t| t.entry_id)
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// The verdict on one entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The row stays as it is (or absent).
    Keep,
    Write(Write),
    /// A second active node: the one already there.
    Conflict {
        node_id: String,
    },
}

/// Judge `entry_id` after a change: `touched` the nodes it wrote, in order
/// of first touch; `row` the entry's current row; `node_of` final images
/// (the change's own, else the accepted state's).
///
/// 1. Each written node left active that counts for the entry, in order:
///    another active node counting for it — another written one, or the
///    row's — is a conflict naming that one (the lowest id when several).
/// 2. The row names the active node counting for the entry; else the last
///    written node this change archived that counted before and still
///    counts; else the row's node while it still counts; else the last
///    written node that counts; else no row.
pub fn judge(
    entry_id: &str,
    touched: &[Touched<'_>],
    row: Option<&Row>,
    node_of: &dyn Fn(&str) -> Option<Map<String, Value>>,
) -> Outcome {
    let row_node = row
        .filter(|r| !touched.iter().any(|t| id(t.after) == r.node_id))
        .and_then(|r| node_of(&r.node_id))
        .filter(|n| counts(n, entry_id, node_of));
    let live: Vec<&Map<String, Value>> = touched
        .iter()
        .map(|t| t.after)
        .chain(row_node.as_ref())
        .filter(|n| active(n) && counts(n, entry_id, node_of))
        .collect();
    for t in touched {
        if !(active(t.after) && counts(t.after, entry_id, node_of)) {
            continue;
        }
        if let Some(other) = live
            .iter()
            .map(|n| id(n))
            .filter(|other| *other != id(t.after))
            .min()
        {
            return Outcome::Conflict {
                node_id: other.to_owned(),
            };
        }
    }
    let archived_here = touched.iter().rev().find(|t| {
        !active(t.after)
            && counts(t.after, entry_id, node_of)
            && t.before
                .is_some_and(|b| active(b) && counts(b, entry_id, node_of))
    });
    let counting_written = touched
        .iter()
        .rev()
        .find(|t| counts(t.after, entry_id, node_of));
    let chosen = live
        .first()
        .copied()
        .or(archived_here.map(|t| t.after))
        .or(row_node.as_ref())
        .or(counting_written.map(|t| t.after));
    match (chosen.map(|n| Row::naming(entry_id, n)), row) {
        (Some(next), Some(current)) if next == *current => Outcome::Keep,
        (Some(next), _) => Outcome::Write(Write::Upsert(next)),
        (None, Some(_)) => Outcome::Write(Write::Delete {
            entry_id: entry_id.to_owned(),
        }),
        (None, None) => Outcome::Keep,
    }
}

/// The kind of a manifest entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Dir,
}

/// Whether the server admits `node` as written into the graph of `domain`,
/// `entry` answering the domain's manifest by entry id: a node naming an
/// entry names one of this domain (another domain's id answers exactly as
/// one that exists nowhere); a `files:` text that [`parse`] rejects (a
/// lower-case id, a trailing segment) is refused rather than read as naming
/// none; a `Collection` binds a `dir` only; the domain's root `Collection`
/// (carrying [`PROFILE_VERSION`]) is never bound. Every refusal is
/// `invalid_change`.
pub fn admit(
    node: &Map<String, Value>,
    domain: &str,
    entry: &dyn Fn(&str) -> Option<EntryKind>,
) -> Result<(), Refusal> {
    let properties = node.get("properties");
    let collection = node.get("kind").and_then(Value::as_str) == Some("Collection");
    if collection
        && properties
            .is_some_and(|p| p.get(PROFILE_VERSION).is_some() && p.get(DIRECTORY).is_some())
    {
        return Err(Refusal::InvalidChange);
    }
    if collection
        && properties
            .and_then(|p| p.get(DIRECTORY))
            .is_some_and(|d| d.as_str().and_then(parse).is_none())
    {
        return Err(Refusal::InvalidChange);
    }
    if !collection && entry_text(node).is_some_and(|t| t.starts_with(PREFIX) && parse(t).is_none())
    {
        return Err(Refusal::InvalidChange);
    }
    let Some(target) = named(node) else {
        return Ok(());
    };
    let kind = (target.domain == domain)
        .then(|| entry(&target.entry_id))
        .flatten();
    match (kind, collection) {
        (None, _) | (Some(EntryKind::File), true) => Err(Refusal::InvalidChange),
        _ => Ok(()),
    }
}
