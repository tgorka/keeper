//! The operator's admin path: the only way the map changes (DF/AD-6). Its
//! one client is the operator's own tool, and its one guard is the
//! operator's token (DF/OQ-14); no device credential, agent instance,
//! service client or import token changes the map, an owner's device
//! included.
//!
//! A change is one [`AdminRequest`]: a [`AdminCommand`], the request's id and
//! the map revision the operator last saw. The server binds the id to the
//! digest of the whole request the first time it applies it, answers the same
//! id and payload again from that record with no second effect, and refuses
//! the same id with another payload ([`Refusal::RequestReused`]) and a request
//! whose `expected_revision` a later change moved past
//! ([`Refusal::StaleRevision`]), so a replay cannot roll a revocation back.
//! A request answered [`AdminAnswer::ConfirmationRequired`] or refused applied
//! nothing and binds nothing. Every applied request is one event of the map's
//! history, attributed to the operator, in the request's own transaction.
//!
//! A request's fields are closed: a parser refuses one it does not know. An
//! answer's are open: a reader ignores one it does not know.
//!
//! [`Refusal::RequestReused`]: crate::refusal::Refusal::RequestReused
//! [`Refusal::StaleRevision`]: crate::refusal::Refusal::StaleRevision

use serde::{Deserialize, Serialize};

use crate::{DomainKind, FilesWrites, Role};

/// One change to the map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminRequest {
    /// Chosen by the sender, unique per intended change; a retry after a lost
    /// answer reuses it with the same payload.
    pub request_id: String,
    /// The map's revision the operator last saw ([`MapView::revision`]).
    pub expected_revision: u64,
    pub command: AdminCommand,
    /// The operator's confirmation of exactly the entries a
    /// [`AdminAnswer::ConfirmationRequired`] listed for this command. The
    /// server applies the change only when the entries it finds now are these;
    /// otherwise it answers with the new list and changes nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm: Option<Vec<Unopenable>>,
}

/// What a request changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum AdminCommand {
    /// A new person: an actor of the map, a member of nothing yet.
    AddPerson {
        actor_id: String,
        display_name: String,
        /// The person's own domain, once it is deployed and listed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        personal_domain: Option<String>,
        /// The agents that serve this person.
        #[serde(default)]
        serving_agents: Vec<String>,
    },
    /// A domain a deployment created is listed in the map, and served from
    /// then on. A group is a domain of kind `group`; its people are its
    /// memberships. A deployed domain the map does not list serves nobody.
    ListDomain {
        name: String,
        kind: DomainKind,
        display_name: String,
        /// The person a `personal` domain belongs to.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        personal_of: Option<String>,
    },
    /// A person becomes a member of a domain, or a member's role changes.
    /// No role is a membership without one: the person's devices open the
    /// domain, and no module grants them anything by role.
    SetMembership {
        domain: String,
        actor_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<Role>,
    },
    /// A person leaves a domain: their credentials there are revoked and their
    /// sockets on it closed.
    RemoveMembership {
        domain: String,
        actor_id: String,
    },
    /// The sign-in subject that is this person, replacing any earlier one.
    BindSubject {
        actor_id: String,
        oidc_subject: String,
    },
    /// The person signs in as nobody until a subject is bound again.
    UnbindSubject {
        actor_id: String,
    },
    /// The git repository a domain replaces, its remote; told to the domain's
    /// members on their discovery entry.
    SetReplacedRepository {
        domain: String,
        repository: String,
    },
    ClearReplacedRepository {
        domain: String,
    },
    /// Re-applies the map after a failed apply or an old restore. Every
    /// `(person, domain)` that differs from the last applied map must be sent
    /// again or listed here as accepted; until each is, the answer is
    /// [`AdminAnswer::Unreconciled`] and the barrier stays closed.
    Reconcile {
        #[serde(default)]
        accept: Vec<PersonDomain>,
    },
}

/// One person in one domain.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonDomain {
    pub actor_id: String,
    pub domain: String,
}

/// An entry of `domain` whose sources name `source_domain`, which `readers`
/// — readers of `domain` after the change — could not open.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unopenable {
    pub domain: String,
    pub entry_id: String,
    pub source_domain: String,
    pub readers: Vec<String>,
}

/// The server's answer to an [`AdminRequest`] it did not refuse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AdminAnswer {
    /// Committed and applied: every affected domain serves the new map.
    Applied { event_id: String, revision: u64 },
    /// Committed but not applied: the event stands, the barrier is closed
    /// in every domain, and the operator reconciles.
    CommittedNotApplied {
        event_id: String,
        revision: u64,
        failure: String,
    },
    /// The change would leave these entries' sources unopenable for some
    /// reader; nothing changed. Send the request again with `confirm`.
    ConfirmationRequired { entries: Vec<Unopenable> },
    /// A reconcile that cannot open the barrier yet: these pairs differ from
    /// the last applied map and were neither sent again nor accepted.
    Unreconciled { differences: Vec<PersonDomain> },
}

/// Whether map-dependent admissions are open, and why not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Barrier {
    Open,
    /// No map has been applied since the server started.
    Starting,
    /// A change is being applied.
    Reconciling,
    /// A change was committed and not applied.
    Failed,
    /// The general domain was restored from a copy older than the last
    /// applied map.
    StaleRestore,
}

/// Who wrote a membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Issuer {
    /// The operator, on the admin path.
    Operator,
    /// The one-time seed from the operator's file.
    Seed,
}

/// The whole map, as the admin path lists it to the operator. Never told to
/// a person's device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapView {
    /// The `seq` of the map's last event; 0 for a map with none.
    pub revision: u64,
    pub barrier: Barrier,
    pub domains: Vec<DomainRecord>,
    pub people: Vec<PersonRecord>,
    pub memberships: Vec<MembershipRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainRecord {
    pub name: String,
    pub kind: DomainKind,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub personal_of: Option<String>,
    /// The deployment that created it has been seen.
    pub provisioned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_writes: Option<FilesWrites>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonRecord {
    pub actor_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub personal_domain: Option<String>,
    #[serde(default)]
    pub serving_agents: Vec<String>,
    /// Server-only: never synced, never told to a device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oidc_subject: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipRecord {
    pub domain: String,
    pub actor_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    pub issuer: Issuer,
    pub revision: u64,
    pub granted_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<i64>,
}
