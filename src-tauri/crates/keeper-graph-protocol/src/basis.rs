//! An accepted event's `approval_basis`: the authority its change landed
//! under (stories 111.2, 114.6). The server derives it from the credential,
//! the context and the request, never from a payload.
//!
//! - `{"basis": "direct"}` — a person's own write.
//! - `{"basis": "approval", approval_id, grant_id, origin}` — an agent's
//!   write under a person's approval of that call; `grant_id` the standing
//!   grant that approved it, or null.
//! - `{"basis": "proposal", proposal_id, proposal_revision, proposal_digest,
//!   proposer_actor, proposer_client, proposer_context}` — an approved
//!   proposal ([`crate::proposal::approval_basis`]).
//! - `{"basis": "grant", grant_id, grant_revision, mode}` — a change
//!   accepted with no approval of the call because the actor's graph grant
//!   lets it land: a tool identity's write inside its grant, or an agent
//!   instance's under a root its grant gives `apply`. `grant_id` is the
//!   grant's id in the map, `<domain>/<actor_id>` (a domain name holds no
//!   `/`), `grant_revision` the map revision that wrote the grant, and
//!   `mode` the mode that applied — one of `ask`, `propose`, `apply`, the
//!   words of the accounts contract's `grant_modes`.
//!
//! Every member of a form is always present (`null` where a value may be
//! absent) and a form is closed: a member it does not have does not parse.
//! The event header keeps the column as JSON
//! ([`crate::wire::EventHeader::approval_basis`]), so a client that does not
//! know a form still reads the event.

use serde::{Deserialize, Deserializer, Serialize};

/// The largest integer a JavaScript reader holds exactly, 2^53−1: the
/// bound of a map revision.
pub const MAX_EXACT_INTEGER: u64 = (1 << 53) - 1;

/// How a change inside a grant lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// An event's `approval_basis`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "basis", rename_all = "snake_case", deny_unknown_fields)]
pub enum ApprovalBasis {
    /// A struct variant, so a member it does not have is refused as on the
    /// other forms (a unit variant would ignore it).
    Direct {},
    Approval {
        #[serde(deserialize_with = "required")]
        approval_id: Option<String>,
        #[serde(deserialize_with = "required")]
        grant_id: Option<String>,
        origin: String,
    },
    Proposal {
        proposal_id: String,
        proposal_revision: String,
        proposal_digest: String,
        proposer_actor: String,
        proposer_client: String,
        #[serde(deserialize_with = "required")]
        proposer_context: Option<String>,
    },
    Grant {
        #[serde(deserialize_with = "grant_id_text")]
        grant_id: String,
        #[serde(deserialize_with = "revision")]
        grant_revision: u64,
        mode: GrantMode,
    },
}

/// The id of `actor_id`'s graph grant in `domain`: `<domain>/<actor_id>`.
#[must_use]
pub fn grant_id(domain: &str, actor_id: &str) -> String {
    format!("{domain}/{actor_id}")
}

/// A domain name: a lowercase letter, then 2 to 55 lowercase letters or
/// digits.
fn domain_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (3..=56).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// A nullable member that is always present: `null` is `None`, absence
/// does not parse.
fn required<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

fn grant_id_text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let id = String::deserialize(deserializer)?;
    match id.split_once('/') {
        Some((domain, actor)) if domain_name(domain) && !actor.is_empty() => Ok(id),
        _ => Err(serde::de::Error::custom(
            "grant_id is not <domain>/<actor_id>",
        )),
    }
}

/// A map revision that wrote something: 1 to 2^53−1.
fn revision<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let value = u64::deserialize(deserializer)?;
    if value == 0 || value > MAX_EXACT_INTEGER {
        return Err(serde::de::Error::custom("revision outside 1..=2^53-1"));
    }
    Ok(value)
}
