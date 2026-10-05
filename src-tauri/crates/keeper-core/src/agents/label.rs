//! Labels: who may read what an agent read, and how far it can be trusted
//! (AD-390, story 89.4).
//!
//! A label is a point in FIDES' product lattice: a set of readers, ordered by
//! inclusion, times an integrity, ordered `Untrusted < Agent < Peer < Owner`,
//! times whether the content may only reach a local model. A session's label
//! is the [`Label::join`] of everything it has read, so it only ever narrows:
//! readers intersect, integrity falls to the lower side, `local_only` sticks.
//!
//! This module labels inputs, answers [`Label::may_reach`], and decides
//! whether content with a label may go to a sink ([`check_sink`], AD-391).

use std::collections::BTreeSet;
use std::fmt;

use globset::{GlobBuilder, GlobSetBuilder};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, UserId};
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::ser::{SerializeSeq, Serializer};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::agents::drive::DriveDecl;
use crate::agents::tier::Tier;
use crate::notes::frontmatter::Frontmatter;
use crate::notes::okf;

/// How far content can be trusted. The derived order is the lattice's:
/// `Untrusted < Agent < Peer < Owner`, and a join takes the minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Integrity {
    /// Fetched from outside, or written into a zone where outside content
    /// lands (an inbox, messages, recordings), whoever wrote it.
    Untrusted,
    /// Written by an agent, or by someone keeper cannot name.
    Agent,
    /// Said by another reader of the session's label.
    Peer,
    /// Said or written by the session's own person.
    Owner,
}

impl Integrity {
    /// The word the log and the chip use.
    pub fn as_word(self) -> &'static str {
        match self {
            Self::Untrusted => "untrusted",
            Self::Agent => "agent",
            Self::Peer => "peer",
            Self::Owner => "owner",
        }
    }
}

/// Who may read. `Anyone` is the top of the order and the identity of a join.
///
/// Serialised as `"*"` or as an array of Matrix ids, sorted (a `BTreeSet`
/// iterates in order), so two hosts write the same bytes for the same label.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Readers {
    /// No restriction.
    Anyone,
    /// Exactly these people. The empty set is the bottom: nobody.
    Only(BTreeSet<OwnedUserId>),
}

impl Readers {
    /// The intersection, with `Anyone` as the identity.
    pub fn meet(&self, other: &Readers) -> Readers {
        match (self, other) {
            (Readers::Anyone, x) | (x, Readers::Anyone) => x.clone(),
            (Readers::Only(a), Readers::Only(b)) => {
                Readers::Only(a.intersection(b).cloned().collect())
            }
        }
    }

    /// Whether every reader of `self` is a reader of `other`.
    pub fn is_within(&self, other: &Readers) -> bool {
        match (self, other) {
            (_, Readers::Anyone) => true,
            (Readers::Anyone, Readers::Only(_)) => false,
            (Readers::Only(a), Readers::Only(b)) => a.is_subset(b),
        }
    }

    /// The union, with `Anyone` absorbing: who reads what either side shows.
    pub fn union(&self, other: &Readers) -> Readers {
        match (self, other) {
            (Readers::Anyone, _) | (_, Readers::Anyone) => Readers::Anyone,
            (Readers::Only(a), Readers::Only(b)) => Readers::Only(a.union(b).cloned().collect()),
        }
    }
}

impl Serialize for Readers {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Readers::Anyone => serializer.serialize_str("*"),
            Readers::Only(set) => {
                let mut seq = serializer.serialize_seq(Some(set.len()))?;
                for user in set {
                    seq.serialize_element(user.as_str())?;
                }
                seq.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Readers {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ReadersVisitor;

        impl<'de> Visitor<'de> for ReadersVisitor {
            type Value = Readers;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("\"*\" or an array of Matrix user ids")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Readers, E> {
                if value == "*" {
                    Ok(Readers::Anyone)
                } else {
                    Err(E::invalid_value(de::Unexpected::Str(value), &self))
                }
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Readers, A::Error> {
                let mut set = BTreeSet::new();
                while let Some(id) = seq.next_element::<String>()? {
                    let user = UserId::parse(id.as_str()).map_err(|_| {
                        de::Error::invalid_value(de::Unexpected::Str(&id), &"a Matrix user id")
                    })?;
                    if !set.insert(user) {
                        return Err(de::Error::custom(format!("reader {id} is listed twice")));
                    }
                }
                Ok(Readers::Only(set))
            }
        }

        deserializer.deserialize_any(ReadersVisitor)
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// A label: readers, integrity, and whether only a local model may see it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Label {
    /// Who may read what carries this label.
    pub readers: Readers,
    /// How far it can be trusted.
    pub integrity: Integrity,
    /// Whether it came from a `local_only` drive (AD-377): written only when
    /// true, absent reads false.
    #[serde(default, skip_serializing_if = "is_false")]
    pub local_only: bool,
}

impl Label {
    /// The top: anyone, the owner's word, any model. A join's identity.
    pub fn top() -> Label {
        Label {
            readers: Readers::Anyone,
            integrity: Integrity::Owner,
            local_only: false,
        }
    }

    /// The least upper bound of what both sides allow: readers intersect,
    /// integrity takes the lower, `local_only` either side's.
    pub fn join(&self, other: &Label) -> Label {
        Label {
            readers: self.readers.meet(&other.readers),
            integrity: self.integrity.min(other.integrity),
            local_only: self.local_only || other.local_only,
        }
    }

    /// Whether content with this label may be shown to `audience`: every
    /// member of the audience must be one of the readers. The primitive
    /// `check_sink` builds on.
    pub fn may_reach(&self, audience: &Readers) -> bool {
        audience.is_within(&self.readers)
    }

    /// Whether content with this label may be sent to a model that runs
    /// (`local`) or does not run on a machine the readers control.
    pub fn may_use_model(&self, local: bool) -> bool {
        !self.local_only || local
    }

    /// A session's label at its opening: the home drive's readers, the
    /// requester's integrity, and the home drive's `local_only`.
    pub fn opening(home: &DriveDecl, requester: Integrity) -> Label {
        Label {
            readers: Readers::Only(home.readers.clone()),
            integrity: requester,
            local_only: home.local_only,
        }
    }

    /// The sentence the session frame tells the model, naming each reader as
    /// `name` does, in the readers' sorted order.
    pub fn sentence(&self, name: &dyn Fn(&UserId) -> String) -> String {
        let mut out = match &self.readers {
            Readers::Anyone => "What you read here may be shown to anyone.".to_owned(),
            Readers::Only(set) if set.is_empty() => {
                "What you read here may be shown to no one.".to_owned()
            }
            Readers::Only(set) => {
                let names: Vec<String> = set.iter().map(|user| name(user)).collect();
                format!(
                    "What you read here may be shown only to: {}.",
                    names.join(", ")
                )
            }
        };
        if self.local_only {
            out.push_str(" It may be sent only to a model that runs locally.");
        }
        out
    }

    /// A `main` session's label at its person's next turn (R28 S-09): the
    /// integrity is the one `said` carries — what the person said is the
    /// turn's own standing — while the readers keep narrowing and
    /// `local_only` sticks. Every other session only narrows ([`Label::join`]).
    pub fn at_persons_turn(&self, said: &Label) -> Label {
        Label {
            readers: self.readers.meet(&said.readers),
            integrity: said.integrity,
            local_only: self.local_only || said.local_only,
        }
    }
}

/// Where content goes (AD-391): every place an agent sends what it read
/// names who will read it there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sink {
    /// A room: its people, and the audience of every agent in it.
    Room {
        humans: BTreeSet<OwnedUserId>,
        agent_audiences: Vec<Readers>,
    },
    /// A delegation: the target agent's audience and the room's other
    /// members.
    Delegation {
        target_audience: Readers,
        room_members: BTreeSet<OwnedUserId>,
    },
    /// A write into a drive, read by its readers.
    DriveWrite { drive_readers: Readers },
    /// A write into an agent's memory, read with its home.
    MemoryWrite { home_readers: Readers },
    /// A model call (R28 S-04): a provider is a processor the person chose,
    /// not an audience, so only `local_only` binds it.
    Model { local: bool },
    /// A configured MCP server or KVM, read by its configured `readers`
    /// (R24(1)): `*` only when the configuration says so.
    External { readers: Readers },
}

/// What a sink may receive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SinkVerdict {
    Allow,
    /// Not sent: the sentence the caller says instead, and the readers the
    /// sink would add (empty when it would reach anyone, or for a model).
    Block {
        reason: String,
        wider: BTreeSet<OwnedUserId>,
    },
}

/// What a model that may not see a `local_only` label is told it was not
/// sent.
pub const LOCAL_ONLY_SINK: &str =
    "This session has read something that may go only to a model on its readers' own machines.";

/// Whether content labelled `label` may go to `sink`: a model by
/// [`Label::may_use_model`], every other sink by [`Label::may_reach`] over
/// the sink's whole audience.
pub fn check_sink(label: &Label, sink: &Sink) -> SinkVerdict {
    let audience = match sink {
        Sink::Model { local } => {
            return if label.may_use_model(*local) {
                SinkVerdict::Allow
            } else {
                SinkVerdict::Block {
                    reason: LOCAL_ONLY_SINK.to_owned(),
                    wider: BTreeSet::new(),
                }
            };
        }
        Sink::Room {
            humans,
            agent_audiences,
        } => agent_audiences
            .iter()
            .fold(Readers::Only(humans.clone()), |all, one| all.union(one)),
        Sink::Delegation {
            target_audience,
            room_members,
        } => target_audience.union(&Readers::Only(room_members.clone())),
        Sink::DriveWrite { drive_readers } => drive_readers.clone(),
        Sink::MemoryWrite { home_readers } => home_readers.clone(),
        Sink::External { readers } => readers.clone(),
    };
    if label.may_reach(&audience) {
        return SinkVerdict::Allow;
    }
    let only = match &label.readers {
        Readers::Only(set) => set
            .iter()
            .map(|user| user.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        Readers::Anyone => String::new(),
    };
    let only = if only.is_empty() {
        "no one".to_owned()
    } else {
        only
    };
    match (&audience, &label.readers) {
        (Readers::Only(audience), Readers::Only(readers)) => {
            let wider: BTreeSet<OwnedUserId> = audience.difference(readers).cloned().collect();
            let names: Vec<&str> = wider.iter().map(|user| user.as_str()).collect();
            SinkVerdict::Block {
                reason: format!(
                    "This would let {} read what only {only} may read.",
                    names.join(", ")
                ),
                wider,
            }
        }
        _ => SinkVerdict::Block {
            reason: format!("This would let anyone read what only {only} may read."),
            wider: BTreeSet::new(),
        },
    }
}

/// What an action that needs a person's approval answers until approvals
/// exist (AD-391, epic 93): a declassification, and a consequential call
/// decided under `untrusted` integrity.
pub const NEEDS_APPROVAL: &str =
    "Letting this through needs an approval, which this keeper cannot take yet.";

/// What the integrity rule says of one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallVerdict {
    Allow,
    /// Not done: a person must let it through ([`NEEDS_APPROVAL`]).
    NeedsApproval,
    /// Not done: the sentence the caller says instead.
    Block {
        reason: String,
    },
}

/// Whom one call sends to, as the host resolved it (R167): the name the
/// call gave, and its audience — a known agent's own (its home drive's
/// readers), a person's `{them}` — or `None` when the host cannot name it:
/// an external address, an agent no mounted drive homes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recipient<'a> {
    pub name: &'a str,
    pub audience: Option<&'a Readers>,
}

/// The integrity rule (AD-391, Q11, R167): under `untrusted` integrity a
/// call to a recipient that is not already a reader of the label — one the
/// host cannot name, or whose audience is wider than the label's readers —
/// is blocked, since the outside content may have chosen it; a known agent
/// whose audience is within the label passes, and its session opens
/// `untrusted` (R94 joins the labels). A consequential call — `tier`, the
/// call's classified tier, at T3 or above (R82) — needs approval. A block
/// is decided first.
pub fn check_call(label: &Label, tool: &str, tier: Tier, recipients: &[Recipient]) -> CallVerdict {
    if label.integrity > Integrity::Untrusted {
        return CallVerdict::Allow;
    }
    if let Some(outside) = recipients.iter().find(|recipient| {
        !recipient
            .audience
            .is_some_and(|audience| label.may_reach(audience))
    }) {
        return CallVerdict::Block {
            reason: format!(
                "This session has read outside content, and {} is not already a reader of it, so {tool} was not called.",
                outside.name.trim()
            ),
        };
    }
    if tier >= Tier::T3 {
        return CallVerdict::NeedsApproval;
    }
    CallVerdict::Allow
}

/// Where a blocked flow would have gone: the concrete drive and
/// path, room, person or agent, so one request names one effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// A file of a drive, drive-relative.
    Drive { drive: String, path: String },
    /// A room, as it is.
    Room { room: OwnedRoomId },
    /// A person: an invite, a new conversation.
    Person { user: OwnedUserId },
    /// An agent homed in `drive`, through `room` when the room exists.
    Agent {
        drive: String,
        agent: OwnedUserId,
        room: Option<OwnedRoomId>,
    },
}

impl Destination {
    /// The audit row's target (R65): the drive (or `""`) and the path,
    /// room or user.
    pub fn target(&self) -> (&str, &str) {
        match self {
            Destination::Drive { drive, path } => (drive, path),
            Destination::Room { room } => ("", room.as_str()),
            Destination::Person { user } => ("", user.as_str()),
            Destination::Agent { drive, agent, .. } => (drive, agent.as_str()),
        }
    }
}

/// A request to let one blocked flow through (AD-391, FR-795): the effect
/// that would happen — by the SHA-256 of its canonical bytes and, for a
/// file, its drive-relative path, never the bytes — where exactly, who may
/// allow it, and the DM of each one's proxy it is asked in. Epic 93 sends
/// it and records the decision; until then it is refused with
/// [`NEEDS_APPROVAL`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclassifyRequest {
    /// The SHA-256 of the blocked effect's canonical bytes, hex: what the
    /// producer would have sent or written, whole (a delegation's whole
    /// brief event, a write's file content, an edit's target and change).
    pub effect_sha256: String,
    /// The file those bytes are, drive-relative, when they are one.
    pub artifact: Option<String>,
    pub destination: Destination,
    pub sink: Sink,
    /// Who may let it through: the label's readers.
    pub approvers: BTreeSet<OwnedUserId>,
    /// Each approver's proxy DM, where the host found one.
    pub route: Vec<(OwnedUserId, Option<OwnedRoomId>)>,
}

/// The [`DeclassifyRequest`] for the effect whose canonical bytes are
/// `effect` (the file `artifact`, when it is one) labelled `label` going to
/// `sink` at `destination`; `proxy_dm` is the host's own lookup of a
/// person's proxy DM.
pub fn declassify_request(
    effect: &[u8],
    artifact: Option<&str>,
    destination: &Destination,
    sink: &Sink,
    label: &Label,
    proxy_dm: &dyn Fn(&UserId) -> Option<OwnedRoomId>,
) -> DeclassifyRequest {
    use sha2::{Digest, Sha256};
    let approvers = match &label.readers {
        Readers::Only(set) => set.clone(),
        Readers::Anyone => BTreeSet::new(),
    };
    let route = approvers
        .iter()
        .map(|approver| (approver.clone(), proxy_dm(approver)))
        .collect();
    DeclassifyRequest {
        effect_sha256: format!("{:x}", Sha256::digest(effect)),
        artifact: artifact.map(str::to_owned),
        destination: destination.clone(),
        sink: sink.clone(),
        approvers,
        route,
    }
}

/// Who last wrote a file, as far as the host could tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Author {
    /// A person, by Matrix id.
    Reader(OwnedUserId),
    /// An agent (OKF's actor, or the commit's `Keeper-Device` trailer).
    Agent,
    /// Nobody keeper can name.
    Unknown,
}

/// What the host learnt about one file it read from a drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadFacts {
    /// The file's drive-relative path, `/`-separated.
    pub path: String,
    /// Who last wrote it.
    pub last_author: Author,
    /// The file's own OKF `human_reviewed:`, when it states one.
    pub okf_human_reviewed: Option<bool>,
    /// Whether the file's OKF `sources` name an `http(s)` URL.
    pub okf_external_source: bool,
    /// Whether the file is a card carrying `integrity: untrusted` — made from
    /// outside content (Q17), read from its own text by
    /// [`crate::agents::card::marked_untrusted`].
    pub card_untrusted: bool,
}

/// The two label-relevant facts a file's OKF frontmatter carries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OkfLabelFacts {
    /// `human_reviewed:` as written, when it is a boolean.
    pub human_reviewed: Option<bool>,
    /// A `sources[]` entry whose `id` or `resource` is an `http(s)` URL.
    pub external_source: bool,
}

/// Read [`OkfLabelFacts`] from a file's text with keeper's own OKF reader.
pub fn okf_label_facts(text: &str) -> OkfLabelFacts {
    let (frontmatter, offset) = Frontmatter::parse(text);
    if offset == 0 {
        return OkfLabelFacts::default();
    }
    let doc = okf::read(&frontmatter);
    let external_source = doc
        .sources
        .iter()
        .any(|source| is_web_url(&source.resource) || source.id.as_deref().is_some_and(is_web_url));
    OkfLabelFacts {
        human_reviewed: frontmatter.as_bool("human_reviewed"),
        external_source,
    }
}

fn is_web_url(text: &str) -> bool {
    let text = text.trim();
    ["http://", "https://"].iter().any(|scheme| {
        text.get(..scheme.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(scheme))
    })
}

/// Whether `path` lies in one of the drive's untrusted zones. A pattern that
/// does not compile counts as matching: a zone keeper cannot read is treated
/// as the lower trust, never the higher.
fn in_untrusted_zone(drive: &DriveDecl, path: &str) -> bool {
    let mut builder = GlobSetBuilder::new();
    for pattern in &drive.untrusted {
        match GlobBuilder::new(pattern)
            .literal_separator(true)
            // Folded, as the write fence folds: keeper ships on a case-insensitive
            // volume, where `00-Inbox/` is `00-inbox/`.
            .case_insensitive(true)
            .build()
        {
            Ok(glob) => {
                builder.add(glob);
            }
            Err(_) => return true,
        }
    }
    match builder.build() {
        Ok(set) => set.is_match(path.trim_start_matches('/')),
        Err(_) => true,
    }
}

/// The label of a file read from `drive`: the drive's readers and
/// `local_only`, and an integrity from who wrote it — `owner` for a reader,
/// `agent` for an agent or anyone keeper cannot name (fail low), `untrusted`
/// for a writer outside the readers, a file in an untrusted zone, one that
/// cites the web or a card made from outside content. An OKF
/// `human_reviewed: false` lowers it to `agent`.
pub fn label_drive_read(drive: &DriveDecl, facts: &ReadFacts) -> Label {
    let integrity = if facts.okf_external_source
        || facts.card_untrusted
        || in_untrusted_zone(drive, &facts.path)
    {
        Integrity::Untrusted
    } else {
        let by_author = match &facts.last_author {
            Author::Reader(user) if drive.readers.contains(user) => Integrity::Owner,
            Author::Reader(_) => Integrity::Untrusted,
            Author::Agent | Author::Unknown => Integrity::Agent,
        };
        match facts.okf_human_reviewed {
            Some(false) => by_author.min(Integrity::Agent),
            _ => by_author,
        }
    };
    Label {
        readers: Readers::Only(drive.readers.clone()),
        integrity,
        local_only: drive.local_only,
    }
}

/// The label of a person's message in a session room: the sender and the
/// room's readers may read it; `owner` from the session's own person, `peer`
/// from another reader, `untrusted` from anyone else.
pub fn label_person_message(
    sender: &UserId,
    session_person: &UserId,
    room_readers: &BTreeSet<OwnedUserId>,
) -> Label {
    let mut readers = room_readers.clone();
    readers.insert(sender.to_owned());
    let integrity = if sender == session_person {
        Integrity::Owner
    } else if room_readers.contains(sender) {
        Integrity::Peer
    } else {
        Integrity::Untrusted
    };
    Label {
        readers: Readers::Only(readers),
        integrity,
        local_only: false,
    }
}

/// An agent's message carries its session's label.
pub fn label_agent_message(session_label: &Label) -> Label {
    session_label.clone()
}

/// Anything from outside — an MCP result, a fetched page, a screen.
pub fn label_outside() -> Label {
    Label {
        readers: Readers::Anyone,
        integrity: Integrity::Untrusted,
        local_only: false,
    }
}

/// Why a session's label changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelCauseKind {
    /// A file read from a drive.
    DriveRead,
    /// A tool's result.
    ToolResult,
    /// A person's message.
    PersonMessage,
    /// Another agent's message.
    AgentMessage,
    /// Content from outside.
    Outside,
    /// A delegation's label.
    Delegation,
}

/// What joined into the label, and where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelCause {
    /// What kind of input it was.
    pub kind: LabelCauseKind,
    /// A drive path, a line id or an event id.
    #[serde(rename = "ref")]
    pub reference: String,
}

/// The body of a `label` log line: the session label after a join.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelBody {
    /// The label's readers.
    pub readers: Readers,
    /// The label's integrity.
    pub integrity: Integrity,
    /// The label's `local_only`, written only when true.
    #[serde(default, skip_serializing_if = "is_false")]
    pub local_only: bool,
    /// What caused the change.
    pub cause: LabelCause,
}

impl LabelBody {
    /// The line body for `label`, caused by `cause`.
    pub fn new(label: &Label, cause: LabelCause) -> LabelBody {
        LabelBody {
            readers: label.readers.clone(),
            integrity: label.integrity,
            local_only: label.local_only,
            cause,
        }
    }

    /// The label this line records.
    pub fn label(&self) -> Label {
        Label {
            readers: self.readers.clone(),
            integrity: self.integrity,
            local_only: self.local_only,
        }
    }
}

/// The label chip: readers by name, integrity as a word, and the sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LabelVm {
    /// The readers' names in the readers' order; empty when `anyone`.
    pub readers: Vec<String>,
    /// Whether anyone may read.
    pub anyone: bool,
    /// `owner`, `peer`, `agent` or `untrusted`.
    pub integrity: String,
    /// Whether only a local model may see it.
    pub local_only: bool,
    /// [`Label::sentence`], verbatim.
    pub sentence: String,
}

impl LabelVm {
    /// The chip for `label`, naming readers as `name` does.
    pub fn compose(label: &Label, name: &dyn Fn(&UserId) -> String) -> LabelVm {
        let (readers, anyone) = match &label.readers {
            Readers::Anyone => (Vec::new(), true),
            Readers::Only(set) => (set.iter().map(|user| name(user)).collect(), false),
        };
        LabelVm {
            readers,
            anyone,
            integrity: label.integrity.as_word().to_owned(),
            local_only: label.local_only,
            sentence: label.sentence(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;

    fn user(id: &str) -> OwnedUserId {
        UserId::parse(id).expect("user id")
    }

    fn only(ids: &[&str]) -> Readers {
        Readers::Only(ids.iter().map(|id| user(id)).collect())
    }

    const UNIVERSE: [&str; 3] = ["@tgorka:h", "@marta:h", "@x:h"];
    const INTEGRITIES: [Integrity; 4] = [
        Integrity::Untrusted,
        Integrity::Agent,
        Integrity::Peer,
        Integrity::Owner,
    ];

    /// `Anyone` and the eight subsets of the universe.
    fn all_readers() -> Vec<Readers> {
        let mut out = vec![Readers::Anyone];
        for mask in 0u8..8 {
            let set = UNIVERSE
                .iter()
                .enumerate()
                .filter(|(bit, _)| mask & (1 << bit) != 0)
                .map(|(_, id)| user(id))
                .collect();
            out.push(Readers::Only(set));
        }
        out
    }

    /// 9 × 4 labels for each `local_only`: 72.
    fn all_labels() -> Vec<Label> {
        let mut out = Vec::new();
        for local_only in [false, true] {
            for readers in all_readers() {
                for integrity in INTEGRITIES {
                    out.push(Label {
                        readers: readers.clone(),
                        integrity,
                        local_only,
                    });
                }
            }
        }
        out
    }

    fn drive(id: &str, readers: &[&str], local_only: bool) -> DriveDecl {
        DriveDecl {
            id: id.to_owned(),
            title: id.to_owned(),
            principal: "tgorka".to_owned(),
            owner: user("@tgorka:h"),
            readers: readers.iter().map(|id| user(id)).collect(),
            local_only,
            untrusted: crate::agents::drive::DEFAULT_UNTRUSTED
                .iter()
                .map(|glob| (*glob).to_owned())
                .collect(),
        }
    }

    fn neuradrive() -> DriveDecl {
        drive("neuradrive", &["@marta:h", "@tgorka:h"], false)
    }

    fn tgdrive() -> DriveDecl {
        drive("tgdrive", &["@tgorka:h"], false)
    }

    #[test]
    fn the_label_lattice_holds_over_every_label() {
        let labels = all_labels();
        assert_eq!(labels.len(), 72);
        let identity = Label::top();
        let bottom = Label {
            readers: Readers::Only(BTreeSet::new()),
            integrity: Integrity::Untrusted,
            local_only: true,
        };
        for a in &labels {
            assert_eq!(a.join(a), *a, "idempotent");
            assert_eq!(a.join(&identity), *a, "Anyone with Owner is the identity");
            assert_eq!(identity.join(a), *a, "Anyone with Owner is the identity");
            assert_eq!(a.join(&bottom), bottom, "nobody with Untrusted absorbs");
            for b in &labels {
                let ab = a.join(b);
                assert_eq!(ab, b.join(a), "commutative");
                for c in &labels {
                    assert_eq!(ab.join(c), a.join(&b.join(c)), "associative");
                }
            }
        }
    }

    #[test]
    fn a_join_never_widens_readers_or_raises_integrity() {
        let labels = all_labels();
        for a in &labels {
            for b in &labels {
                let joined = a.join(b);
                for side in [a, b] {
                    assert!(joined.readers.is_within(&side.readers), "{a:?} ⊔ {b:?}");
                    assert!(joined.integrity <= side.integrity, "{a:?} ⊔ {b:?}");
                    assert!(joined.local_only >= side.local_only, "{a:?} ⊔ {b:?}");
                    // Whatever the join may reach, each side could reach.
                    for audience in all_readers() {
                        if joined.may_reach(&audience) {
                            assert!(side.may_reach(&audience));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn may_reach_is_the_audience_within_the_readers() {
        let neura = Label::opening(&neuradrive(), Integrity::Owner);
        assert!(neura.may_reach(&only(&["@tgorka:h"])));
        assert!(neura.may_reach(&only(&["@marta:h", "@tgorka:h"])));
        assert!(!neura.may_reach(&only(&["@tgorka:h", "@x:h"])));
        assert!(!neura.may_reach(&Readers::Anyone));
        let tg = Label::opening(&tgdrive(), Integrity::Owner);
        assert!(!tg.may_reach(&only(&["@marta:h", "@tgorka:h"])));
        assert!(label_outside().may_reach(&Readers::Anyone));
    }

    #[test]
    fn local_only_travels_with_the_label_and_gates_the_model() {
        let private = drive("diary", &["@tgorka:h"], true);
        let opening = Label::opening(&private, Integrity::Owner);
        assert!(opening.local_only);
        assert!(opening.may_use_model(true));
        assert!(!opening.may_use_model(false));
        let read = label_drive_read(
            &private,
            &ReadFacts {
                path: "notes/a.md".to_owned(),
                last_author: Author::Reader(user("@tgorka:h")),
                okf_human_reviewed: None,
                okf_external_source: false,
                card_untrusted: false,
            },
        );
        assert!(read.local_only);
        let session = Label::opening(&tgdrive(), Integrity::Owner);
        assert!(session.may_use_model(false));
        assert!(!session.join(&read).may_use_model(false), "a join keeps it");
    }

    fn fixture(rel: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/agents/okf")
            .join(rel);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn read_fixture(drive: &DriveDecl, rel: &str, author: Author) -> Label {
        read_text(drive, rel, &fixture(rel), author)
    }

    /// A read of `text` at `rel`, its facts taken from the text as the host
    /// takes them.
    fn read_text(drive: &DriveDecl, rel: &str, text: &str, author: Author) -> Label {
        let okf = okf_label_facts(text);
        label_drive_read(
            drive,
            &ReadFacts {
                path: rel.to_owned(),
                last_author: author,
                okf_human_reviewed: okf.human_reviewed,
                okf_external_source: okf.external_source,
                card_untrusted: crate::agents::card::marked_untrusted(text),
            },
        )
    }

    /// AC12 (Q17): a card carrying `integrity: untrusted`, read by an
    /// `owner`-integrity session, joins it down to `untrusted`; the same card
    /// without the key reads by its author.
    #[test]
    fn a_card_made_from_outside_content_reads_untrusted() {
        let tg = tgdrive();
        let tgorka = || Author::Reader(user("@tgorka:h"));
        let card = "---\ntags: [task]\ntitle: Answer the letter\nstatus: todo\nassignee: tola-grey\nintegrity: untrusted\n---\n\nFrom the inbox.\n";
        let rel = "60-sessions/active/s/answer.md";
        let session = Label::opening(&tg, Integrity::Owner);
        let marked = read_text(&tg, rel, card, tgorka());
        assert_eq!(marked.integrity, Integrity::Untrusted);
        assert_eq!(session.join(&marked).integrity, Integrity::Untrusted);
        let plain = read_text(
            &tg,
            rel,
            &card.replace("integrity: untrusted\n", ""),
            tgorka(),
        );
        assert_eq!(plain.integrity, Integrity::Owner);
    }

    #[test]
    fn labels_follow_who_wrote_a_file() {
        let tg = tgdrive();
        let tgorka = || Author::Reader(user("@tgorka:h"));

        let plan = read_fixture(&tg, "notes/plan.md", tgorka());
        assert_eq!(plan.readers, only(&["@tgorka:h"]));
        assert_eq!(plan.integrity, Integrity::Owner);

        let draft = read_fixture(&tg, "notes/agent-draft.md", tgorka());
        assert_eq!(
            okf_label_facts(&fixture("notes/agent-draft.md")).human_reviewed,
            Some(false)
        );
        assert_eq!(
            draft.integrity,
            Integrity::Agent,
            "human_reviewed: false lowers it"
        );

        let unknown = read_fixture(&tg, "notes/plan.md", Author::Unknown);
        assert_eq!(unknown.integrity, Integrity::Agent, "fail low, never Owner");
        let by_agent = read_fixture(&tg, "notes/plan.md", Author::Agent);
        assert_eq!(by_agent.integrity, Integrity::Agent);
        let stranger = read_fixture(&tg, "notes/plan.md", Author::Reader(user("@x:h")));
        assert_eq!(stranger.integrity, Integrity::Untrusted);
    }

    #[test]
    fn integrity_zones_and_web_sources_are_untrusted_whoever_wrote_them() {
        let tg = tgdrive();
        let tgorka = || Author::Reader(user("@tgorka:h"));
        for rel in ["00-inbox/forwarded.md", "70-comms/thread.md"] {
            let label = read_fixture(&tg, rel, tgorka());
            assert_eq!(label.integrity, Integrity::Untrusted, "{rel}");
            assert_eq!(label.readers, only(&["@tgorka:h"]), "{rel}");
        }
        assert!(okf_label_facts(&fixture("notes/clipping.md")).external_source);
        let clipping = read_fixture(&tg, "notes/clipping.md", tgorka());
        assert_eq!(clipping.integrity, Integrity::Untrusted);

        // Folded, as the write fence folds: `00-Inbox/` is the inbox.
        let shouted = label_drive_read(
            &tg,
            &ReadFacts {
                path: "00-Inbox/forwarded.md".to_owned(),
                last_author: tgorka(),
                okf_human_reviewed: None,
                okf_external_source: false,
                card_untrusted: false,
            },
        );
        assert_eq!(shouted.integrity, Integrity::Untrusted);
        // A present table replaces the default; an empty one means none.
        let mut open = tgdrive();
        open.untrusted.clear();
        let inbox = read_fixture(&open, "00-inbox/forwarded.md", tgorka());
        assert_eq!(inbox.integrity, Integrity::Owner);
        // A pattern that does not compile fails low.
        open.untrusted = vec!["notes/[".to_owned()];
        assert_eq!(
            read_fixture(&open, "notes/plan.md", tgorka()).integrity,
            Integrity::Untrusted
        );
    }

    #[test]
    fn messages_are_labelled_by_who_sent_them() {
        let room: BTreeSet<OwnedUserId> = [user("@marta:h")].into_iter().collect();
        let own = label_person_message(&user("@tgorka:h"), &user("@tgorka:h"), &room);
        assert_eq!(own.readers, only(&["@marta:h", "@tgorka:h"]));
        assert_eq!(own.integrity, Integrity::Owner);
        let peer = label_person_message(&user("@marta:h"), &user("@tgorka:h"), &room);
        assert_eq!(peer.readers, only(&["@marta:h"]));
        assert_eq!(peer.integrity, Integrity::Peer);
        let stranger = label_person_message(&user("@x:h"), &user("@tgorka:h"), &room);
        assert_eq!(stranger.integrity, Integrity::Untrusted);

        let session = Label::opening(&neuradrive(), Integrity::Peer);
        assert_eq!(label_agent_message(&session), session);
        assert_eq!(
            label_outside(),
            Label {
                readers: Readers::Anyone,
                integrity: Integrity::Untrusted,
                local_only: false
            }
        );
    }

    #[test]
    fn serde_shapes_are_stable() {
        let neura = Label::opening(&neuradrive(), Integrity::Owner);
        assert_eq!(
            serde_json::to_string(&neura).expect("json"),
            r#"{"readers":["@marta:h","@tgorka:h"],"integrity":"owner"}"#
        );
        assert_eq!(
            serde_json::to_string(&label_outside()).expect("json"),
            r#"{"readers":"*","integrity":"untrusted"}"#
        );
        let local = Label {
            local_only: true,
            ..label_outside()
        };
        assert_eq!(
            serde_json::to_string(&local).expect("json"),
            r#"{"readers":"*","integrity":"untrusted","local_only":true}"#
        );
        // Unsorted on the way in, sorted on the way out.
        let back: Label =
            serde_json::from_value(json!({"readers":["@tgorka:h","@marta:h"],"integrity":"owner"}))
                .expect("parse");
        assert_eq!(back, neura);
        assert!(
            serde_json::from_value::<Label>(json!({"readers":"*","integrity":"boss"})).is_err()
        );
        assert!(
            serde_json::from_value::<Label>(json!({"readers":"all","integrity":"agent"})).is_err()
        );
        assert!(serde_json::from_value::<Label>(
            json!({"readers":["not-an-id"],"integrity":"agent"})
        )
        .is_err());
        assert!(serde_json::from_value::<Label>(
            json!({"readers":["@a:h","@a:h"],"integrity":"agent"})
        )
        .is_err());

        let body = LabelBody::new(
            &neura,
            LabelCause {
                kind: LabelCauseKind::DriveRead,
                reference: "notes/plan.md".to_owned(),
            },
        );
        assert_eq!(
            serde_json::to_string(&body).expect("json"),
            r#"{"readers":["@marta:h","@tgorka:h"],"integrity":"owner","cause":{"kind":"drive_read","ref":"notes/plan.md"}}"#
        );
        assert_eq!(body.label(), neura);
    }

    fn display_name(user: &UserId) -> String {
        match user.localpart() {
            "marta" => "Marta".to_owned(),
            other => other.to_owned(),
        }
    }

    #[test]
    fn the_prompt_says_who_may_read() {
        let neura = Label::opening(&neuradrive(), Integrity::Owner);
        assert_eq!(
            neura.sentence(&display_name),
            "What you read here may be shown only to: Marta, tgorka."
        );
        assert_eq!(
            label_outside().sentence(&display_name),
            "What you read here may be shown to anyone."
        );
        let nobody = Label {
            readers: Readers::Only(BTreeSet::new()),
            ..label_outside()
        };
        assert_eq!(
            nobody.sentence(&display_name),
            "What you read here may be shown to no one."
        );
        let private = Label::opening(&drive("diary", &["@tgorka:h"], true), Integrity::Owner);
        assert_eq!(
            private.sentence(&display_name),
            "What you read here may be shown only to: tgorka. It may be sent only to a model that \
             runs locally."
        );
        let vm = LabelVm::compose(&neura, &display_name);
        assert_eq!(vm.readers, vec!["Marta".to_owned(), "tgorka".to_owned()]);
        assert_eq!(vm.integrity, "owner");
        assert_eq!(vm.sentence, neura.sentence(&display_name));
    }

    /// 92.1 acceptance 12: a model is bound by `local_only` alone; every
    /// other arm blocks exactly when its whole audience is not within the
    /// label's readers, naming who would be added.
    #[test]
    fn check_sink_decides_every_arm() {
        let tgorka = only(&["@tgorka:h"]);
        let mine = Label {
            readers: tgorka.clone(),
            integrity: Integrity::Owner,
            local_only: false,
        };
        for readers in all_readers() {
            let local_only = Label {
                readers,
                integrity: Integrity::Owner,
                local_only: true,
            };
            assert!(matches!(
                check_sink(&local_only, &Sink::Model { local: false }),
                SinkVerdict::Block { .. }
            ));
            assert_eq!(
                check_sink(&local_only, &Sink::Model { local: true }),
                SinkVerdict::Allow
            );
        }
        assert_eq!(
            check_sink(&mine, &Sink::Model { local: false }),
            SinkVerdict::Allow
        );
        assert!(matches!(
            check_sink(
                &mine,
                &Sink::External {
                    readers: Readers::Anyone
                }
            ),
            SinkVerdict::Block { .. }
        ));
        assert_eq!(
            check_sink(
                &mine,
                &Sink::External {
                    readers: tgorka.clone()
                }
            ),
            SinkVerdict::Allow
        );

        for label in all_labels() {
            for audience in all_readers() {
                let mut sinks = vec![
                    Sink::DriveWrite {
                        drive_readers: audience.clone(),
                    },
                    Sink::MemoryWrite {
                        home_readers: audience.clone(),
                    },
                    Sink::External {
                        readers: audience.clone(),
                    },
                    Sink::Delegation {
                        target_audience: audience.clone(),
                        room_members: BTreeSet::new(),
                    },
                    Sink::Room {
                        humans: BTreeSet::new(),
                        agent_audiences: vec![audience.clone()],
                    },
                ];
                // People are named one by one: a member set is never "anyone".
                if let Readers::Only(people) = &audience {
                    sinks.push(Sink::Delegation {
                        target_audience: Readers::Only(BTreeSet::new()),
                        room_members: people.clone(),
                    });
                    sinks.push(Sink::Room {
                        humans: people.clone(),
                        agent_audiences: Vec::new(),
                    });
                }
                for sink in sinks {
                    let verdict = check_sink(&label, &sink);
                    assert_eq!(
                        verdict == SinkVerdict::Allow,
                        label.may_reach(&audience),
                        "{label:?} {sink:?}"
                    );
                    if let (
                        SinkVerdict::Block { wider, .. },
                        Readers::Only(readers),
                        Readers::Only(audience),
                    ) = (&verdict, &label.readers, &audience)
                    {
                        let expected: BTreeSet<OwnedUserId> =
                            audience.difference(readers).cloned().collect();
                        assert_eq!(wider, &expected, "{label:?} {sink:?}");
                    }
                }
            }
        }

        // Nixi's {tgorka} session handing work to Dr Lucyna Novak, whose
        // audience is {tgorka, Marta}: blocked, naming Marta (92.1 AC4); a
        // room whose extra member is Marta is blocked the same way (AC5).
        let lucyna = only(&["@marta:h", "@tgorka:h"]);
        let SinkVerdict::Block { reason, wider } = check_sink(
            &mine,
            &Sink::Delegation {
                target_audience: lucyna,
                room_members: BTreeSet::from([user("@tgorka:h")]),
            },
        ) else {
            panic!("blocked")
        };
        assert_eq!(wider, BTreeSet::from([user("@marta:h")]));
        assert!(reason.contains("@marta:h"), "{reason}");
        assert!(matches!(
            check_sink(
                &mine,
                &Sink::Room {
                    humans: BTreeSet::from([user("@tgorka:h"), user("@marta:h")]),
                    agent_audiences: vec![tgorka],
                },
            ),
            SinkVerdict::Block { .. }
        ));
    }

    /// 92.6 acceptance 3: a {tgorka, Marta} session that reads a tgdrive
    /// file is {tgorka} from then on, so its next neuradrive write is
    /// blocked naming Marta; the join never widens it back.
    #[test]
    fn a_read_narrows_the_next_write() {
        let shared = only(&["@marta:h", "@tgorka:h"]);
        let session = Label {
            readers: shared.clone(),
            integrity: Integrity::Owner,
            local_only: false,
        };
        let neuradrive = Sink::DriveWrite {
            drive_readers: shared.clone(),
        };
        assert_eq!(check_sink(&session, &neuradrive), SinkVerdict::Allow);
        let tgdrive_read = Label {
            readers: only(&["@tgorka:h"]),
            integrity: Integrity::Owner,
            local_only: false,
        };
        let after = session.join(&tgdrive_read);
        let SinkVerdict::Block { wider, .. } = check_sink(&after, &neuradrive) else {
            panic!("blocked")
        };
        assert_eq!(wider, BTreeSet::from([user("@marta:h")]));
        let reread = after.join(&session);
        assert!(matches!(
            check_sink(&reread, &neuradrive),
            SinkVerdict::Block { .. }
        ));
    }

    /// 92.6 acceptance 4 as R167 restates it: under `untrusted`, a
    /// recipient the host cannot name, or whose audience is wider than the
    /// label, is blocked whatever the tool; a known agent whose audience is
    /// within the label passes; a consequential call needs approval; above
    /// `untrusted` the rule allows everything it is asked.
    #[test]
    fn a_recipient_taken_from_outside_content_is_blocked() {
        let at = |integrity| Label {
            readers: only(&["@tgorka:h"]),
            integrity,
            local_only: false,
        };
        let untrusted = at(Integrity::Untrusted);
        let tola = only(&["@tgorka:h"]);
        let shared = only(&["@marta:h", "@tgorka:h"]);
        let unnamed = Recipient {
            name: "evil/exfil",
            audience: None,
        };
        let known = Recipient {
            name: "tola-grey",
            audience: Some(&tola),
        };
        let wider = Recipient {
            name: "neuradrive/lucyna",
            audience: Some(&shared),
        };
        let t1 = Tier::T1;
        assert!(matches!(
            check_call(&untrusted, "delegate", t1, &[unnamed]),
            CallVerdict::Block { reason } if reason.contains("evil/exfil")
        ));
        assert!(matches!(
            check_call(&untrusted, "delegate", t1, &[wider]),
            CallVerdict::Block { reason } if reason.contains("neuradrive/lucyna")
        ));
        assert_eq!(
            check_call(&untrusted, "delegate", t1, &[known]),
            CallVerdict::Allow
        );
        assert!(matches!(
            check_call(&untrusted, "delegate", t1, &[known, unnamed]),
            CallVerdict::Block { .. }
        ));
        // R82: consequential is the classified tier at T3 or above — a
        // write outside the session raised by `untrusted`.
        assert_eq!(
            check_call(&untrusted, "drive_write", Tier::T3, &[]),
            CallVerdict::NeedsApproval
        );
        assert_eq!(
            check_call(&untrusted, "card_update", Tier::T4, &[]),
            CallVerdict::NeedsApproval
        );
        assert_eq!(
            check_call(&untrusted, "delegate", Tier::T2, &[known]),
            CallVerdict::Allow
        );
        assert!(matches!(
            check_call(&untrusted, "drive_edit", Tier::T3, &[unnamed]),
            CallVerdict::Block { .. }
        ));
        for integrity in [Integrity::Agent, Integrity::Peer, Integrity::Owner] {
            for tool in ["delegate", "drive_write", "drive_edit"] {
                assert_eq!(
                    check_call(&at(integrity), tool, Tier::T3, &[unnamed, wider]),
                    CallVerdict::Allow
                );
            }
        }
    }

    /// 92.6 acceptance 8 (pure): at the person's turn a `main` session's
    /// integrity is that of the person's line, however low the last turn
    /// went, while its readers keep narrowing and `local_only` sticks.
    #[test]
    fn a_main_sessions_integrity_resets_at_the_persons_turn() {
        for before in all_labels() {
            for said in all_labels() {
                let next = before.at_persons_turn(&said);
                assert_eq!(next.integrity, said.integrity, "{before:?} {said:?}");
                assert_eq!(next.readers, before.readers.meet(&said.readers));
                assert!(next.readers.is_within(&before.readers));
                assert_eq!(next.local_only, before.local_only || said.local_only);
            }
        }
        let relayed = Label {
            readers: only(&["@tgorka:h"]),
            integrity: Integrity::Untrusted,
            local_only: false,
        };
        let tgorka = Label {
            readers: only(&["@tgorka:h"]),
            integrity: Integrity::Owner,
            local_only: false,
        };
        assert_eq!(relayed.at_persons_turn(&tgorka).integrity, Integrity::Owner);
        assert_eq!(relayed.join(&tgorka).integrity, Integrity::Untrusted);
    }

    /// 92.6 acceptance 5: a block in a {tgorka} session asks tgorka, in
    /// Nixi's DM, about the exact effect by its SHA-256 and its destination
    /// — the request carries a digest, a path and where, never the content.
    #[test]
    fn a_declassification_goes_to_the_owners_proxy() {
        let label = Label {
            readers: only(&["@tgorka:h"]),
            integrity: Integrity::Owner,
            local_only: false,
        };
        let sink = Sink::Delegation {
            target_audience: only(&["@marta:h", "@tgorka:h"]),
            room_members: BTreeSet::from([user("@tgorka:h")]),
        };
        let lucyna = Destination::Agent {
            drive: "neuradrive".to_owned(),
            agent: user("@lucyna:h"),
            room: None,
        };
        let nixis_dm = OwnedRoomId::try_from("!nixi-dm:h").expect("room");
        let route = |person: &UserId| (person == "@tgorka:h").then(|| nixis_dm.clone());
        let content = b"the plan: sell in March";
        let request = declassify_request(content, None, &lucyna, &sink, &label, &route);
        assert_eq!(request.approvers, BTreeSet::from([user("@tgorka:h")]));
        assert_eq!(
            request.route,
            vec![(user("@tgorka:h"), Some(nixis_dm.clone()))]
        );
        assert_eq!(
            request.effect_sha256,
            "743952fca51d2205792eab844091bc4bb29df579c77a8ec8668a04f9b42007bc"
        );
        assert_eq!(request.sink, sink);
        assert_eq!(request.destination, lucyna);
        assert_eq!(lucyna.target(), ("neuradrive", "@lucyna:h"));
        assert!(!format!("{request:?}").contains("sell in March"));
        let at = Destination::Drive {
            drive: "tgdrive".to_owned(),
            path: "60-sessions/active/s/artifacts/plan.md".to_owned(),
        };
        let file = declassify_request(
            content,
            Some("60-sessions/active/s/artifacts/plan.md"),
            &at,
            &sink,
            &label,
            &route,
        );
        assert_eq!(
            file.artifact.as_deref(),
            Some("60-sessions/active/s/artifacts/plan.md")
        );
        assert_eq!(
            file.destination.target(),
            ("tgdrive", "60-sessions/active/s/artifacts/plan.md")
        );
        assert_eq!(file.effect_sha256, request.effect_sha256);
    }
}
