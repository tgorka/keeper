//! The accounts contract (story 114.2): what a domain is, the four roles a
//! membership may carry, the entry `GET /databases` gives for each domain a
//! credential opens ([`discovery`]), and the requests and answers of the
//! operator's admin path ([`admin`]) with their refusals ([`refusal`]).
//!
//! The map itself — which domains, people and memberships exist — is the
//! server's own data, kept in the general domain's schema and never in a
//! contract table, a scope or a replica (`syncular/README.md`, *Domains and
//! the map*). A person's device learns only its own domains, from
//! [`discovery::DomainEntry`]; nothing here lists the domains that exist.
//!
//! `syncular/accounts.vectors.json` pins every shape here, and
//! `syncular/vocabulary.json`'s `roles` is the one list of [`Role`]s;
//! keeper-hq's TypeScript reads the same files. This crate holds types only:
//! it accepts, stores and sends nothing.

pub mod admin;
pub mod discovery;
pub mod refusal;

use serde::{Deserialize, Serialize};

/// A role a person holds in one domain. The same four name a person's
/// standing in every module of the domain — its graph, its files, its jobs —
/// and only the operator's admin path grants one. The order is the
/// contract's (`vocabulary.json`'s `roles`), not a ranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Reader,
    Editor,
    Reviewer,
    Owner,
}

impl Role {
    /// Every role, in `vocabulary.json`'s order.
    pub const ALL: [Self; 4] = [Self::Reader, Self::Editor, Self::Reviewer, Self::Owner];

    /// The role's name on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reader => "reader",
            Self::Editor => "editor",
            Self::Reviewer => "reviewer",
            Self::Owner => "owner",
        }
    }
}

/// What a domain is for: the organisation's one general domain (`makidrive`),
/// one person's own, or a group's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainKind {
    General,
    Personal,
    Group,
}

/// Whether a domain's files are written through its manifest yet: `shadow`
/// while git is still the truth, `open` once writes go to the manifest. The
/// record is the domain's, changed only on the admin path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilesWrites {
    Shadow,
    Open,
}
