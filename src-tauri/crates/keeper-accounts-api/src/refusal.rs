//! The stable codes the accounts contract adds, each answered as
//! `{"error": "<code>"}`. A request the admin guard does not admit keeps the
//! guard's own answer, and a domain the credential does not open keeps
//! today's 401 (`sync.auth_required`, or Syncular's own on its routes) — the
//! same answer a name that is no domain gets — so neither is a code here.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Why the server refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// The admin request does not parse as the contract's.
    Malformed,
    /// The request id was applied before with another payload.
    RequestReused,
    /// `expected_revision` is not the map's revision.
    StaleRevision,
    /// The map lists no domain of that name.
    UnknownDomain,
    /// A domain listed or changed that no deployment created.
    NotDeployed,
    /// The map already lists that domain.
    DomainExists,
    /// The map has no person with that actor id.
    UnknownPerson,
    /// The map already has that person.
    PersonExists,
    /// The sign-in subject is bound to another person.
    SubjectTaken,
    /// The general domain cannot be read now; the last applied map keeps
    /// serving, and no change is accepted until it can.
    MapUnreadable,
    /// A map-dependent admission while the barrier is closed: a graph
    /// command or scope, a first enrolment, a pairing redemption.
    MapClosed,
    /// An agent profile whose `drives` names a drive not everyone who reads
    /// its database may read (a commit refusal, so no HTTP status).
    ProfileDriveAudience,
}

impl Refusal {
    /// Every refusal, in the vectors' order.
    pub const ALL: [Self; 12] = [
        Self::Malformed,
        Self::RequestReused,
        Self::StaleRevision,
        Self::UnknownDomain,
        Self::NotDeployed,
        Self::DomainExists,
        Self::UnknownPerson,
        Self::PersonExists,
        Self::SubjectTaken,
        Self::MapUnreadable,
        Self::MapClosed,
        Self::ProfileDriveAudience,
    ];

    /// The stable error code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Malformed => "hq.admin.malformed",
            Self::RequestReused => "hq.admin.request_reused",
            Self::StaleRevision => "hq.admin.stale_revision",
            Self::UnknownDomain => "hq.admin.unknown_domain",
            Self::NotDeployed => "hq.admin.not_deployed",
            Self::DomainExists => "hq.admin.domain_exists",
            Self::UnknownPerson => "hq.admin.unknown_person",
            Self::PersonExists => "hq.admin.person_exists",
            Self::SubjectTaken => "hq.admin.subject_taken",
            Self::MapUnreadable => "hq.admin.map_unreadable",
            Self::MapClosed => "hq.map_closed",
            Self::ProfileDriveAudience => "hq.profile.drive_audience",
        }
    }

    /// The HTTP status it is answered with; `None` for a commit refusal.
    #[must_use]
    pub const fn status(self) -> Option<u16> {
        match self {
            Self::Malformed => Some(400),
            Self::UnknownDomain | Self::UnknownPerson => Some(404),
            Self::RequestReused
            | Self::StaleRevision
            | Self::NotDeployed
            | Self::DomainExists
            | Self::PersonExists
            | Self::SubjectTaken => Some(409),
            Self::MapUnreadable | Self::MapClosed => Some(503),
            Self::ProfileDriveAudience => None,
        }
    }

    /// The refusal a code names, if the contract has it.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.code() == code)
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// A refusal's body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefusalBody {
    pub error: String,
}
