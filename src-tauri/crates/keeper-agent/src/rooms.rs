//! Who may bring an agent into a room, and whose words in a room become a
//! turn (F5, AD-380, ruling R30).
//!
//! An invited client sees only the room's stripped state, so a host cannot
//! read a brief or a first message before it joins: the decision to join is
//! made from the room's type and the inviter alone. Once joined, the host —
//! which decrypts — is what keeps a person's free text out of any session
//! that is not their proxy's conversation, because the homeserver sees every
//! encrypted event as `m.room.encrypted` and cannot tell a decision from text.

use std::collections::BTreeSet;

use keeper_core::agents::agentd::TrustEntry;
use keeper_core::agents::delegation::{enveloped_brief, trusted_brief, DelegateContent};
use keeper_core::agents::events::{CONTROL_ROOM_TYPE, SESSION_ROOM_TYPE};
use keeper_core::agents::home::AgentKind;
use keeper_core::agents::label::{check_sink, Label, Readers, Sink, SinkVerdict};
use keeper_core::agents::room::holds_agent_power;
use keeper_core::agents::session::SessionKind;
use matrix_sdk::ruma::events::room::power_levels::RoomPowerLevels;
use matrix_sdk::ruma::events::MessageLikeEventType;
use matrix_sdk::ruma::{OwnedUserId, UserId};
use serde_json::Value;

/// An invite as the stripped state shows it.
#[derive(Debug, Clone)]
pub struct Invite {
    /// The stripped `m.room.create`'s `type`, when it has one.
    pub room_type: Option<String>,
    /// Who sent the invite.
    pub inviter: OwnedUserId,
    /// The agent user invited.
    pub invited: OwnedUserId,
}

/// An agent this host knows: homed in a drive it mounts.
#[derive(Debug, Clone)]
pub struct KnownAgent {
    /// Its id in its home drive.
    pub id: String,
    /// Its home drive's id.
    pub drive: String,
    /// The name it answers to.
    pub name: String,
    pub matrix_user: OwnedUserId,
    pub kind: AgentKind,
    /// A proxy's person.
    pub human: Option<OwnedUserId>,
    /// Whether this host serves it (an `[[agents]]` entry names it).
    pub hosted: bool,
    /// Its home drive's readers.
    pub home_readers: Readers,
    /// Its opening label: its home drive's readers, its `local_only`.
    pub opening: Label,
    /// `[tools].drives`: what a session of its may have in scope.
    pub drives: Vec<String>,
}

impl KnownAgent {
    /// `<drive>/<id>`, how a delegation names it (R67).
    pub fn name_in_drive(&self) -> String {
        format!("{}/{}", self.drive, self.id)
    }
}

/// What a host knows when an invite arrives.
#[derive(Debug, Clone, Default)]
pub struct Known {
    /// Every agent homed in a drive this host mounts.
    pub agents: Vec<KnownAgent>,
    /// `agentd.toml`'s `[[trust]]`.
    pub trust: Vec<TrustEntry>,
}

/// Join, or leave the invite pending: never declined, so a person can still
/// see it and a later pin can accept it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteDecision {
    Join,
    Pending,
}

/// Whether to join the room `invite` names.
///
/// Besides a visited control room (below), only a session room
/// (`dev.keeper.agent.session`) is ever joined, and only when the inviter is:
/// - **(a)** the `human` of the invited proxy, hosted here — a new proxy
///   conversation;
/// - **(b)** the user of an agent homed in a drive this host mounts, when
///   `check_sink(Room)` lets the invited agent's opening label reach that
///   agent's audience — a delegation;
/// - **(c)** the `proxy` of a `[[trust]]` entry that is pinned (it has a
///   `master_key`) and whose person reads the invited agent's home drive,
///   when `check_sink(Room)` lets the opening label reach that person.
///
/// The desktop has no `[[trust]]`, so there only (a) and (b) join (R63).
///
/// A control room (`dev.keeper.agent.control`) is joined only as a
/// visitor: by a steward hosted here, invited by someone who is not an
/// agent this host knows and who reads the steward's home drive by this
/// host's pins — a person bringing the steward into another principal's
/// control room, so a drive both mount rings there (Q15, R29 F19).
pub fn invite_decision(invite: &Invite, known: &Known) -> InviteDecision {
    if invite.room_type.as_deref() == Some(CONTROL_ROOM_TYPE) {
        return visitor_decision(invite, known);
    }
    if invite.room_type.as_deref() != Some(SESSION_ROOM_TYPE) {
        return InviteDecision::Pending;
    }
    let Some(invited) = known
        .agents
        .iter()
        .find(|agent| agent.hosted && agent.matrix_user == invite.invited)
    else {
        return InviteDecision::Pending;
    };
    let reaches = |sink: Sink| check_sink(&invited.opening, &sink) == SinkVerdict::Allow;
    let proxy_person = invited.kind == AgentKind::Proxy
        && invited.human.as_deref() == Some(invite.inviter.as_ref());
    let mounted_agent = known.agents.iter().any(|agent| {
        agent.matrix_user == invite.inviter
            && reaches(Sink::Room {
                humans: BTreeSet::new(),
                agent_audiences: vec![agent.home_readers.clone()],
            })
    });
    let pinned_proxy = known.trust.iter().any(|trust| {
        trust.master_key.is_some()
            && trust.proxy.as_deref() == Some(invite.inviter.as_ref())
            && reads(&invited.home_readers, &trust.user)
            && reaches(Sink::Room {
                humans: BTreeSet::from([trust.user.clone()]),
                agent_audiences: Vec::new(),
            })
    });
    if proxy_person || mounted_agent || pinned_proxy {
        InviteDecision::Join
    } else {
        InviteDecision::Pending
    }
}

/// The control-room arm of [`invite_decision`].
fn visitor_decision(invite: &Invite, known: &Known) -> InviteDecision {
    let steward = known.agents.iter().find(|agent| {
        agent.hosted && agent.kind == AgentKind::Steward && agent.matrix_user == invite.invited
    });
    let an_agent = known
        .agents
        .iter()
        .any(|agent| agent.matrix_user == invite.inviter);
    match steward {
        Some(steward) if !an_agent && reads(&steward.home_readers, &invite.inviter) => {
            InviteDecision::Join
        }
        _ => InviteDecision::Pending,
    }
}

fn reads(readers: &Readers, user: &UserId) -> bool {
    match readers {
        Readers::Anyone => true,
        Readers::Only(set) => set.contains(user),
    }
}

/// What a host reads of the room a brief arrived in, as the room is now.
#[derive(Debug, Clone)]
pub struct BriefRoom {
    /// The `m.room.create` `type`.
    pub room_type: Option<String>,
    /// Who made the room.
    pub creators: Vec<OwnedUserId>,
    /// Its `m.room.power_levels`; `None` when they could not be read.
    pub levels: Option<RoomPowerLevels>,
    /// Who is in it, or invited.
    pub members: BTreeSet<OwnedUserId>,
}

/// A brief's event, decrypted.
#[derive(Debug, Clone, Copy)]
pub struct BriefEvent<'a> {
    pub event_type: &'a str,
    pub sender: &'a UserId,
    pub content: &'a Value,
    /// Whether its sender's own device sealed it (R30).
    pub sealed: bool,
}

/// An event that is not an `m.text` message carrying a delegation its body
/// says, or is an edit of one.
pub const NOT_A_BRIEF: &str = "a brief is an m.text message carrying a delegation, never an edit";
/// A brief its sender's device did not seal.
pub const UNSEALED_BRIEF: &str = "a brief its sender's device did not seal is not taken";
/// A room that is not a delegated session's.
pub const NOT_A_DELEGATED_ROOM: &str = "a brief is taken only in a delegated session's room";
/// A sender who did not make the room, or a brief naming another sender.
pub const NOT_THE_CREATOR: &str =
    "a brief is taken only from the agent that made its room, in its own name";
/// A sender who is no agent this host knows and no pinned proxy.
pub const NOT_AN_AGENT: &str =
    "a brief is taken only from an agent this host knows or a pinned person's proxy";
/// A sender below an agent's power in the room now.
pub const NO_AGENT_POWER: &str = "a brief is taken only from a sender at an agent's power";
/// A brief for another agent.
pub const NOT_ADDRESSED: &str = "a brief is taken only by the agent it is for";
/// A brief whose label does not reach this agent's audience and the room's
/// people.
pub const BEYOND_THE_LABEL: &str =
    "a brief whose label does not reach this agent's audience and the room's people is not taken";

/// The one test a brief passes before anything acts on it (R93): the live
/// intake, the read-back after a restart, and the session that serves it.
///
/// The event is a brief's envelope ([`enveloped_brief`], R114: an
/// `m.room.message` of `m.text`, not an edit, whose body is the brief it
/// carries) its sender's device sealed. The room is a delegated session's —
/// session-typed, and a person may not send a message in clear there, as
/// they may in a proxy's own rooms. Its sender made the room and names
/// itself as `from` ([`trusted_brief`]; a person's devices read a brief by
/// both rules too); on a host it is also an agent this host knows or the
/// `proxy` of a pinned `[[trust]]` person, and holds an agent's power there
/// now — a creator demoted since has none. The brief is addressed to `me`,
/// and its label reaches `me`'s audience and every other person in the
/// room.
pub fn admit_brief(
    room: &BriefRoom,
    event: &BriefEvent<'_>,
    me: &UserId,
    known: &Known,
) -> Result<DelegateContent, &'static str> {
    let carried = enveloped_brief(event.event_type, event.content).ok_or(NOT_A_BRIEF)?;
    if !event.sealed {
        return Err(UNSEALED_BRIEF);
    }
    let levels = room.levels.as_ref();
    let delegated = room.room_type.as_deref() == Some(SESSION_ROOM_TYPE)
        && levels.is_some_and(|levels| {
            levels.for_message(MessageLikeEventType::RoomMessage) > levels.users_default
        });
    if !delegated {
        return Err(NOT_A_DELEGATED_ROOM);
    }
    let sender = event.sender;
    let brief = trusted_brief(carried, sender, &room.creators).ok_or(NOT_THE_CREATOR)?;
    let is = |user: &UserId, other: &UserId| user.as_str() == other.as_str();
    let agent = known
        .agents
        .iter()
        .any(|agent| is(&agent.matrix_user, sender))
        || known.trust.iter().any(|trust| {
            trust.master_key.is_some() && trust.proxy.as_deref().is_some_and(|p| is(p, sender))
        });
    if !agent {
        return Err(NOT_AN_AGENT);
    }
    if !holds_agent_power(levels, sender) {
        return Err(NO_AGENT_POWER);
    }
    if !is(&brief.to, me) {
        return Err(NOT_ADDRESSED);
    }
    let target = known
        .agents
        .iter()
        .find(|agent| is(&agent.matrix_user, me))
        .ok_or(NOT_ADDRESSED)?;
    let people = room
        .members
        .iter()
        .filter(|member| !is(member, sender) && !is(member, me))
        .cloned()
        .collect();
    let sink = Sink::Delegation {
        target_audience: target.home_readers.clone(),
        room_members: people,
    };
    if check_sink(&brief.label, &sink) != SinkVerdict::Allow {
        return Err(BEYOND_THE_LABEL);
    }
    Ok(brief)
}

/// What arrived in a served session's room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    /// An `m.room.message` that is not an edit.
    Text,
    /// An `m.replace` of an earlier event.
    Edit,
    /// A `dev.keeper.agent.approval.decision`.
    Decision {
        /// Whether the sender's device is verified for this host.
        verified: bool,
    },
    /// A `dev.keeper.agent.scope`: the drives in scope and the docked
    /// note's focus, from the proxy's person.
    Scope {
        /// Whether the sender's device is signed by its owner's identity
        /// (R47).
        owner_signed: bool,
    },
    /// A `dev.keeper.agent.conversation.request` in a proxy's `main` DM
    /// (R36).
    ConversationRequest {
        /// As for a scope (R47).
        owner_signed: bool,
    },
    /// A brief: an `m.room.message` carrying `dev.keeper.agent.delegate`
    /// (R53), the first round of a delegation or a later one (R49).
    Brief,
    /// The target agent joined a room this session delegated into: routed
    /// here from that room by the host (R55).
    Joined,
    /// The target agent's reply in a room this session delegated into,
    /// routed here the same way.
    Replied,
    /// Any other `dev.keeper.agent.*` event: a status, a turn reference, a
    /// claim.
    AgentEvent,
}

/// What a host does with an arrival.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// The proxy's person spoke in its conversation: a turn.
    Turn,
    /// A reader decided, from a verified device: the approval path's.
    Decision,
    /// The proxy's person set the scope or the focus in its conversation.
    Scope,
    /// The proxy's person asked, in its DM, for a new conversation.
    NewConversation,
    /// A delegation this session made moved: its target joined or replied.
    Delegation,
    /// Neither: not logged, not a turn. The sentence is the host's own note.
    Ignored(&'static str),
}

/// The session's agent and the people around it.
#[derive(Debug, Clone, Copy)]
pub struct Served<'a> {
    pub agent_kind: AgentKind,
    pub human: Option<&'a UserId>,
    pub session_kind: SessionKind,
    pub agent_user: &'a UserId,
    /// Who asked for the session: a delegated one's delegating agent.
    pub requester: &'a UserId,
    /// The session's readers (its label's).
    pub readers: &'a Readers,
}

/// A person's text in a session that is not a proxy conversation.
pub const OBSERVER_TEXT: &str =
    "a person's free text in a session that is not their proxy's conversation is not a turn";
/// A message from anyone but the proxy's person.
pub const NOT_THE_PERSON: &str = "only the proxy's person starts a turn in its conversation";
/// The agent's own events, echoed by sync.
pub const OWN_EVENT: &str = "the agent's own event";
/// A decision from someone who does not read the session, or from an
/// unverified device.
pub const UNTRUSTED_DECISION: &str =
    "a decision is accepted only from a reader of the session on a verified device";
/// An agent's event or an edit sent by anyone but the agent itself.
pub const FORGED: &str =
    "an agent's event or an edit is the agent's own to send; from anyone else it is ignored";
/// A scope or a request for a conversation outside the proxy's own rooms.
pub const NOT_A_PROXY_ROOM: &str =
    "a scope or a new conversation is the proxy's person's to ask for in the proxy's own rooms";
/// A request for a conversation outside the proxy's `main` DM.
pub const NOT_THE_DM: &str = "a new conversation is asked for in the proxy's DM";
/// A scope or a request for a conversation from a device its owner did not
/// sign (R47).
pub const UNSIGNED_DEVICE: &str =
    "a scope or a new conversation counts only from a device its owner's identity signed";
/// A brief from anyone but the delegating agent, or outside a delegated
/// session.
pub const NOT_THE_REQUESTER: &str =
    "a brief is a turn only in a delegated session, from the agent that delegated it";

/// Whether `sender`'s arrival becomes a turn, a decision, a scope, a new
/// conversation, or nothing (AD-380, R30). Text becomes a turn only in a
/// proxy's `main` or `conversation` session, from its `human`; everywhere
/// else a person is an observer whose text the host never feeds to the model.
///
/// Every session room lets a person send `m.room.encrypted` at power 0, so
/// the homeserver cannot stop a person sending an encrypted status, turn
/// reference or an edit of the agent's anchor: the host, which decrypts,
/// checks the sender of each instead. Only the agent's own user sends those,
/// and a decision counts only from a reader's verified device.
///
/// Two agent events are the person's (AD-382, R36): a scope — the drives
/// in scope and the docked note — in the proxy's `main` or `conversation`
/// session, and a request for a new conversation in its `main` DM. Each
/// counts only from the proxy's `human`, on a device that person's
/// cross-signing identity signed (R47).
pub fn classify(served: &Served<'_>, sender: &UserId, arrival: Arrival) -> Disposition {
    if sender == served.agent_user {
        return Disposition::Ignored(OWN_EVENT);
    }
    let conversation = served.agent_kind == AgentKind::Proxy
        && matches!(
            served.session_kind,
            SessionKind::Main | SessionKind::Conversation
        );
    match arrival {
        Arrival::Edit | Arrival::AgentEvent => Disposition::Ignored(FORGED),
        Arrival::Brief => {
            if served.session_kind == SessionKind::Delegated && sender == served.requester {
                Disposition::Turn
            } else {
                Disposition::Ignored(NOT_THE_REQUESTER)
            }
        }
        // Made only by this host, from the room the session delegated into;
        // the worker checks the sender against the delegation it made.
        Arrival::Joined | Arrival::Replied => Disposition::Delegation,
        Arrival::Scope { owner_signed } | Arrival::ConversationRequest { owner_signed } => {
            let asks_conversation = matches!(arrival, Arrival::ConversationRequest { .. });
            if !conversation {
                Disposition::Ignored(NOT_A_PROXY_ROOM)
            } else if served.human != Some(sender) {
                Disposition::Ignored(NOT_THE_PERSON)
            } else if !owner_signed {
                Disposition::Ignored(UNSIGNED_DEVICE)
            } else if !asks_conversation {
                Disposition::Scope
            } else if served.session_kind == SessionKind::Main {
                Disposition::NewConversation
            } else {
                Disposition::Ignored(NOT_THE_DM)
            }
        }
        Arrival::Decision { verified } => {
            if verified && reads(served.readers, sender) {
                Disposition::Decision
            } else {
                Disposition::Ignored(UNTRUSTED_DECISION)
            }
        }
        Arrival::Text => {
            if !conversation {
                Disposition::Ignored(OBSERVER_TEXT)
            } else if served.human == Some(sender) {
                Disposition::Turn
            } else {
                Disposition::Ignored(NOT_THE_PERSON)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use keeper_core::agents::label::Integrity;

    use super::*;

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user id")
    }

    fn readers(ids: &[&str]) -> Readers {
        Readers::Only(ids.iter().map(|id| user(id)).collect::<BTreeSet<_>>())
    }

    fn agent(
        id: &str,
        kind: AgentKind,
        human: Option<&str>,
        home: &[&str],
        hosted: bool,
    ) -> KnownAgent {
        KnownAgent {
            id: id
                .trim_start_matches('@')
                .split(':')
                .next()
                .unwrap_or_default()
                .to_owned(),
            drive: "tgdrive".to_owned(),
            name: id.to_owned(),
            matrix_user: user(id),
            kind,
            human: human.map(user),
            hosted,
            home_readers: readers(home),
            opening: Label {
                readers: readers(home),
                integrity: Integrity::Owner,
                local_only: false,
            },
            drives: vec!["tgdrive".to_owned()],
        }
    }

    const TGORKA: &str = "@tgorka:example.org";
    const MARTA: &str = "@marta:example.org";
    const LUCYNA: &str = "@lucyna:example.org";

    fn known(trust: Vec<TrustEntry>) -> Known {
        Known {
            agents: vec![
                agent(
                    "@nixi:example.org",
                    AgentKind::Proxy,
                    Some(TGORKA),
                    &[TGORKA],
                    true,
                ),
                agent(
                    "@amelia:example.org",
                    AgentKind::Specialist,
                    None,
                    &[TGORKA],
                    true,
                ),
                // An agent of neuradrive, mounted here, whose readers include tgorka's.
                agent(
                    "@lena:example.org",
                    AgentKind::Steward,
                    None,
                    &[TGORKA, MARTA],
                    false,
                ),
                // tgdrive's own steward: same readers.
                agent(
                    "@tola:example.org",
                    AgentKind::Steward,
                    None,
                    &[TGORKA],
                    false,
                ),
            ],
            trust,
        }
    }

    fn invite(inviter: &str, invited: &str, typed: bool) -> Invite {
        Invite {
            room_type: typed.then(|| SESSION_ROOM_TYPE.to_owned()),
            inviter: user(inviter),
            invited: user(invited),
        }
    }

    fn trust(person: &str, proxy: &str, pinned: bool) -> TrustEntry {
        TrustEntry {
            user: user(person),
            master_key: pinned.then(|| "ed25519:AAAA".to_owned()),
            proxy: Some(user(proxy)),
        }
    }

    #[test]
    fn invite_decision_table() {
        let known = known(vec![
            trust(MARTA, "@mira:example.org", true),
            trust(LUCYNA, "@luna:example.org", false),
        ]);
        let joins = [
            // (a) the proxy's person.
            invite(TGORKA, "@nixi:example.org", true),
            // (b) an agent of a mounted drive whose home readers the invited
            // agent may reach.
            invite("@tola:example.org", "@amelia:example.org", true),
        ];
        for case in &joins {
            assert_eq!(
                invite_decision(case, &known),
                InviteDecision::Join,
                "{case:?}"
            );
        }
        let pending = [
            // An unknown user.
            invite("@stranger:example.org", "@amelia:example.org", true),
            // A person who is not the invited proxy's.
            invite(TGORKA, "@amelia:example.org", true),
            // An agent whose home readers are wider than the invited agent's.
            invite("@lena:example.org", "@amelia:example.org", true),
            // A trusted person's proxy without a master key.
            invite("@luna:example.org", "@amelia:example.org", true),
            // A room not typed as a session.
            invite(TGORKA, "@nixi:example.org", false),
            // An agent this host does not serve.
            invite(TGORKA, "@tola:example.org", true),
        ];
        for case in &pending {
            assert_eq!(
                invite_decision(case, &known),
                InviteDecision::Pending,
                "{case:?}"
            );
        }
        // (c) a pinned person's proxy, when that person reads the invited home.
        let marta_reads = Known {
            agents: vec![agent(
                "@amelia:example.org",
                AgentKind::Specialist,
                None,
                &[TGORKA, MARTA],
                true,
            )],
            trust: vec![trust(MARTA, "@mira:example.org", true)],
        };
        assert_eq!(
            invite_decision(
                &invite("@mira:example.org", "@amelia:example.org", true),
                &marta_reads
            ),
            InviteDecision::Join
        );
        // ... and not when the person does not read it.
        assert_eq!(
            invite_decision(
                &invite("@mira:example.org", "@amelia:example.org", true),
                &known
            ),
            InviteDecision::Pending
        );

        // 92.4 acceptance 9: a control room is joined only as a visitor — a
        // steward hosted here, brought in by a person who reads its home.
        let control = |inviter: &str, invited: &str| Invite {
            room_type: Some(CONTROL_ROOM_TYPE.to_owned()),
            inviter: user(inviter),
            invited: user(invited),
        };
        let visiting = Known {
            agents: vec![
                agent(LUCYNA, AgentKind::Steward, None, &[TGORKA, MARTA], true),
                agent(
                    "@amelia:example.org",
                    AgentKind::Specialist,
                    None,
                    &[TGORKA, MARTA],
                    true,
                ),
                agent(
                    "@nixi:example.org",
                    AgentKind::Proxy,
                    Some(TGORKA),
                    &[TGORKA],
                    false,
                ),
            ],
            trust: Vec::new(),
        };
        assert_eq!(
            invite_decision(&control(TGORKA, LUCYNA), &visiting),
            InviteDecision::Join,
            "a reader of her home brings the steward into his control room"
        );
        for (case, why) in [
            (
                control("@stranger:example.org", LUCYNA),
                "a person who does not read her home",
            ),
            (
                control(TGORKA, "@amelia:example.org"),
                "a specialist is never a visitor",
            ),
            (
                control("@nixi:example.org", LUCYNA),
                "an agent's invite is not a person's",
            ),
            (
                control(TGORKA, "@tola:example.org"),
                "a steward this host does not serve",
            ),
        ] {
            assert_eq!(
                invite_decision(&case, &visiting),
                InviteDecision::Pending,
                "{why}"
            );
        }
        // On a drive anyone reads, an agent still brings no steward in: only
        // a person does.
        let mut public = visiting.clone();
        public.agents[0].home_readers = Readers::Anyone;
        assert_eq!(
            invite_decision(&control("@nixi:example.org", LUCYNA), &public),
            InviteDecision::Pending
        );
        assert_eq!(
            invite_decision(&control("@stranger:example.org", LUCYNA), &public),
            InviteDecision::Join
        );

        // 92.1 acceptance 11: a delegation's room. On Dr Lucyna Novak's host,
        // which mounts neither tgdrive nor Nixi's home, Nixi's invite joins
        // only while tgorka is pinned there with `proxy = "@nixi:…"`.
        let lucyna = || agent(LUCYNA, AgentKind::Steward, None, &[TGORKA, MARTA], true);
        let nixi_invites = invite("@nixi:example.org", LUCYNA, true);
        let pinned_for_nixi = Known {
            agents: vec![lucyna()],
            trust: vec![trust(TGORKA, "@nixi:example.org", true)],
        };
        assert_eq!(
            invite_decision(&nixi_invites, &pinned_for_nixi),
            InviteDecision::Join
        );
        for unpinned in [
            trust(TGORKA, "@nixi:example.org", false),
            TrustEntry {
                proxy: None,
                ..trust(TGORKA, "@nixi:example.org", true)
            },
        ] {
            let known = Known {
                agents: vec![lucyna()],
                trust: vec![unpinned.clone()],
            };
            assert_eq!(
                invite_decision(&nixi_invites, &known),
                InviteDecision::Pending,
                "{unpinned:?}"
            );
        }
        // A session-typed room from someone this host does not know stays
        // pending, however it is shaped.
        assert_eq!(
            invite_decision(
                &invite("@stranger:example.org", LUCYNA, true),
                &pinned_for_nixi
            ),
            InviteDecision::Pending
        );
    }

    fn served<'a>(
        kind: AgentKind,
        human: Option<&'a UserId>,
        session: SessionKind,
        agent: &'a UserId,
        readers: &'a Readers,
    ) -> Served<'a> {
        Served {
            agent_kind: kind,
            human,
            session_kind: session,
            agent_user: agent,
            requester: human.unwrap_or(agent),
            readers,
        }
    }

    /// R53: a brief is a turn only in a delegated session and only from the
    /// agent that delegated it — never from an observer, nor in any other
    /// session.
    #[test]
    fn a_brief_is_a_turn_only_from_the_delegating_agent() {
        let nixi = user("@nixi:example.org");
        let tola = user("@tola:example.org");
        let tgorka = user(TGORKA);
        let room = readers(&[TGORKA]);
        let delegated = Served {
            requester: &nixi,
            ..served(
                AgentKind::Steward,
                None,
                SessionKind::Delegated,
                &tola,
                &room,
            )
        };
        assert_eq!(
            classify(&delegated, &nixi, Arrival::Brief),
            Disposition::Turn
        );
        assert_eq!(
            classify(&delegated, &tgorka, Arrival::Brief),
            Disposition::Ignored(NOT_THE_REQUESTER)
        );
        let main = Served {
            requester: &nixi,
            ..served(AgentKind::Steward, None, SessionKind::Main, &tola, &room)
        };
        assert_eq!(
            classify(&main, &nixi, Arrival::Brief),
            Disposition::Ignored(NOT_THE_REQUESTER)
        );
    }

    #[test]
    fn free_text_in_a_non_proxy_session_is_ignored() {
        let tgorka = user(TGORKA);
        let amelia = user("@amelia:example.org");
        let room = readers(&[TGORKA]);
        let delegated = served(
            AgentKind::Specialist,
            None,
            SessionKind::Delegated,
            &amelia,
            &room,
        );
        assert_eq!(
            classify(&delegated, &tgorka, Arrival::Text),
            Disposition::Ignored(OBSERVER_TEXT)
        );
        // A proxy's session of another kind is not a conversation either.
        let nixi = user("@nixi:example.org");
        let scheduled = served(
            AgentKind::Proxy,
            Some(&tgorka),
            SessionKind::Scheduled,
            &nixi,
            &room,
        );
        assert_eq!(
            classify(&scheduled, &tgorka, Arrival::Text),
            Disposition::Ignored(OBSERVER_TEXT)
        );
        // The same reader's decision from a verified device is accepted
        // there, and from an unverified one is not.
        assert_eq!(
            classify(&delegated, &tgorka, Arrival::Decision { verified: true }),
            Disposition::Decision
        );
        assert_eq!(
            classify(&delegated, &tgorka, Arrival::Decision { verified: false }),
            Disposition::Ignored(UNTRUSTED_DECISION)
        );
        assert_eq!(
            classify(
                &delegated,
                &user(MARTA),
                Arrival::Decision { verified: true }
            ),
            Disposition::Ignored(UNTRUSTED_DECISION)
        );
    }

    /// A person can send any encrypted event at power 0 (R30), so a status,
    /// a turn reference or an edit of the anchor is checked by sender: from
    /// anyone but the agent it is ignored, even from its person in its own
    /// conversation. A scope is the one agent event the person sends there
    /// (91.2): accepted from the proxy's person on a device their identity
    /// signed, in `main` or `conversation`, and ignored everywhere else.
    #[test]
    fn an_agent_event_or_an_edit_from_anyone_but_the_agent_is_ignored() {
        let tgorka = user(TGORKA);
        let mallory = user("@mallory:example.org");
        let nixi = user("@nixi:example.org");
        let room = readers(&[TGORKA]);
        let signed = Arrival::Scope { owner_signed: true };
        for kind in [SessionKind::Main, SessionKind::Delegated] {
            let session = served(AgentKind::Proxy, Some(&tgorka), kind, &nixi, &room);
            for arrival in [Arrival::AgentEvent, Arrival::Edit] {
                assert_eq!(
                    classify(&session, &tgorka, arrival),
                    Disposition::Ignored(FORGED),
                    "{kind:?} {arrival:?}"
                );
                assert_eq!(
                    classify(&session, &mallory, arrival),
                    Disposition::Ignored(FORGED)
                );
                assert_eq!(
                    classify(&session, &nixi, arrival),
                    Disposition::Ignored(OWN_EVENT)
                );
            }
            assert_eq!(
                classify(&session, &nixi, signed),
                Disposition::Ignored(OWN_EVENT)
            );
        }
        // The person's scope in their proxy's own rooms is theirs to set.
        for kind in [SessionKind::Main, SessionKind::Conversation] {
            let session = served(AgentKind::Proxy, Some(&tgorka), kind, &nixi, &room);
            assert_eq!(
                classify(&session, &tgorka, signed),
                Disposition::Scope,
                "{kind:?}"
            );
            assert_eq!(
                classify(&session, &mallory, signed),
                Disposition::Ignored(NOT_THE_PERSON)
            );
            // A device its owner never signed (R47).
            assert_eq!(
                classify(
                    &session,
                    &tgorka,
                    Arrival::Scope {
                        owner_signed: false
                    }
                ),
                Disposition::Ignored(UNSIGNED_DEVICE)
            );
        }
        // Anywhere else — a delegated session, another agent's room — not.
        let delegated = served(
            AgentKind::Proxy,
            Some(&tgorka),
            SessionKind::Delegated,
            &nixi,
            &room,
        );
        assert_eq!(
            classify(&delegated, &tgorka, signed),
            Disposition::Ignored(NOT_A_PROXY_ROOM)
        );
        let tola = user("@tola:example.org");
        let stewards = served(AgentKind::Steward, None, SessionKind::Main, &tola, &room);
        assert_eq!(
            classify(&stewards, &tgorka, signed),
            Disposition::Ignored(NOT_A_PROXY_ROOM)
        );
    }

    /// A new conversation is asked for in the proxy's DM only, by its person,
    /// from a device their identity signed (R36, R47).
    #[test]
    fn a_new_conversation_is_asked_for_in_the_dm_only() {
        let tgorka = user(TGORKA);
        let nixi = user("@nixi:example.org");
        let room = readers(&[TGORKA]);
        let ask = Arrival::ConversationRequest { owner_signed: true };
        let dm = served(
            AgentKind::Proxy,
            Some(&tgorka),
            SessionKind::Main,
            &nixi,
            &room,
        );
        assert_eq!(classify(&dm, &tgorka, ask), Disposition::NewConversation);
        assert_eq!(
            classify(&dm, &user(MARTA), ask),
            Disposition::Ignored(NOT_THE_PERSON)
        );
        assert_eq!(
            classify(
                &dm,
                &tgorka,
                Arrival::ConversationRequest {
                    owner_signed: false
                }
            ),
            Disposition::Ignored(UNSIGNED_DEVICE)
        );
        let conversation = served(
            AgentKind::Proxy,
            Some(&tgorka),
            SessionKind::Conversation,
            &nixi,
            &room,
        );
        assert_eq!(
            classify(&conversation, &tgorka, ask),
            Disposition::Ignored(NOT_THE_DM)
        );
        let delegated = served(
            AgentKind::Proxy,
            Some(&tgorka),
            SessionKind::Delegated,
            &nixi,
            &room,
        );
        assert_eq!(
            classify(&delegated, &tgorka, ask),
            Disposition::Ignored(NOT_A_PROXY_ROOM)
        );
    }

    #[test]
    fn a_message_from_someone_else_is_ignored_and_not_logged() {
        let tgorka = user(TGORKA);
        let nixi = user("@nixi:example.org");
        let room = readers(&[TGORKA, MARTA]);
        for kind in [SessionKind::Main, SessionKind::Conversation] {
            let session = served(AgentKind::Proxy, Some(&tgorka), kind, &nixi, &room);
            assert_eq!(
                classify(&session, &tgorka, Arrival::Text),
                Disposition::Turn
            );
            assert_eq!(
                classify(&session, &user(MARTA), Arrival::Text),
                Disposition::Ignored(NOT_THE_PERSON)
            );
            assert_eq!(
                classify(&session, &nixi, Arrival::Text),
                Disposition::Ignored(OWN_EVENT)
            );
        }
    }

    /// A room's power levels as `events::power_levels` makes them for
    /// `kind`, `creator` at 100 and `agents` at 50, with `changes` applied.
    fn levels_of(
        kind: SessionKind,
        creator: &UserId,
        agents: &[OwnedUserId],
        changes: impl FnOnce(&mut Value),
    ) -> RoomPowerLevels {
        use matrix_sdk::ruma::events::room::power_levels::RoomPowerLevelsEventContent;
        use matrix_sdk::ruma::room_version_rules::AuthorizationRules;
        let mut json = keeper_core::agents::events::power_levels(kind, creator, agents);
        changes(&mut json);
        let content: RoomPowerLevelsEventContent = serde_json::from_value(json).expect("levels");
        RoomPowerLevels::new(
            content.into(),
            &AuthorizationRules::V1,
            Vec::<OwnedUserId>::new(),
        )
    }

    /// R93: the one admission test a brief passes — live, after a restart,
    /// and in the session that serves it. Only a sealed `m.text` from the
    /// room's creator, a known agent still at an agent's power, addressed to
    /// this agent in a delegated room under a label that reaches its audience
    /// and the room's people, is a brief.
    #[test]
    fn a_brief_is_admitted_only_from_the_delegating_agent_in_its_room() {
        use keeper_core::agents::delegation::{
            brief_content, DelegateContent, DelegateFrom, DelegateLimits,
        };
        use keeper_core::agents::events::CONTENT_VERSION;

        let known = known(vec![trust(MARTA, "@mira:example.org", true)]);
        let (tola, amelia, tgorka) = (
            user("@tola:example.org"),
            user("@amelia:example.org"),
            user(TGORKA),
        );
        let delegation = |from: &OwnedUserId, readers_: &[&str]| DelegateContent {
            v: CONTENT_VERSION,
            id: ulid::Ulid::new().to_string(),
            from: DelegateFrom {
                agent: from.clone(),
                drive: "tgdrive".to_owned(),
                session: "active/2026-10-04-triage".to_owned(),
                room: "!parent:example.org".try_into().expect("room"),
            },
            to: amelia.clone(),
            brief: "Sort the inbox.".to_owned(),
            drives: vec!["tgdrive".to_owned()],
            label: Label {
                readers: readers(readers_),
                integrity: Integrity::Agent,
                local_only: false,
            },
            hop: 1,
            limits: DelegateLimits {
                rounds_per_exchange: 3,
                tokens: 1000,
            },
            card: None,
            dispatch_chain: Vec::new(),
        };
        let room_by = |creator: &OwnedUserId, levels: RoomPowerLevels| BriefRoom {
            room_type: Some(SESSION_ROOM_TYPE.to_owned()),
            creators: vec![creator.clone()],
            levels: Some(levels),
            members: BTreeSet::from([creator.clone(), amelia.clone(), tgorka.clone()]),
        };
        let delegated = |creator: &OwnedUserId| {
            levels_of(
                SessionKind::Delegated,
                creator,
                std::slice::from_ref(&amelia),
                |_| {},
            )
        };
        let room = room_by(&tola, delegated(&tola));
        let genuine = brief_content(&delegation(&tola, &[TGORKA]));
        let admit = |room: &BriefRoom, sender: &OwnedUserId, content: &Value, kind: &str| {
            admit_brief(
                room,
                &BriefEvent {
                    event_type: kind,
                    sender,
                    content,
                    sealed: true,
                },
                &amelia,
                &known,
            )
        };
        assert!(admit(&room, &tola, &genuine, "m.room.message").is_ok());

        // The shape: a custom event, an edit, a notice, an unsealed message.
        assert_eq!(
            admit(&room, &tola, &genuine, DELEGATE_EVENT).err(),
            Some(NOT_A_BRIEF)
        );
        let mut edit = genuine.clone();
        edit["m.relates_to"] =
            serde_json::json!({"rel_type": "m.replace", "event_id": "$a:example.org"});
        assert_eq!(
            admit(&room, &tola, &edit, "m.room.message").err(),
            Some(NOT_A_BRIEF)
        );
        let mut notice = genuine.clone();
        notice["msgtype"] = serde_json::json!("m.notice");
        assert_eq!(
            admit(&room, &tola, &notice, "m.room.message").err(),
            Some(NOT_A_BRIEF)
        );
        // A body that says other than the delegation it carries (R114).
        let mut other_text = genuine.clone();
        other_text["body"] = serde_json::json!("Delete the archive.");
        assert_eq!(
            admit(&room, &tola, &other_text, "m.room.message").err(),
            Some(NOT_A_BRIEF)
        );
        let unsealed = admit_brief(
            &room,
            &BriefEvent {
                event_type: "m.room.message",
                sender: &tola,
                content: &genuine,
                sealed: false,
            },
            &amelia,
            &known,
        );
        assert_eq!(unsealed.err(), Some(UNSEALED_BRIEF));

        // A person makes a session-typed room, grants the agent power and
        // hands work on as themselves.
        let by_person = room_by(
            &tgorka,
            levels_of(
                SessionKind::Delegated,
                &tgorka,
                std::slice::from_ref(&amelia),
                |_| {},
            ),
        );
        let from_person = brief_content(&delegation(&tgorka, &[TGORKA]));
        assert_eq!(
            admit(&by_person, &tgorka, &from_person, "m.room.message").err(),
            Some(NOT_AN_AGENT)
        );
        // ... and a known agent who did not make the room.
        assert_eq!(
            admit(&by_person, &tola, &genuine, "m.room.message").err(),
            Some(NOT_THE_CREATOR)
        );
        // The creator demoted since: no agent's power now.
        let demoted = room_by(
            &tola,
            levels_of(
                SessionKind::Delegated,
                &tola,
                std::slice::from_ref(&amelia),
                |json| json["users"][tola.as_str()] = serde_json::json!(0),
            ),
        );
        assert_eq!(
            admit(&demoted, &tola, &genuine, "m.room.message").err(),
            Some(NO_AGENT_POWER)
        );
        // A proxy's own room, where a person may message in clear, and a
        // room of no session type.
        let conversation = room_by(
            &tola,
            levels_of(
                SessionKind::Conversation,
                &tola,
                std::slice::from_ref(&amelia),
                |_| {},
            ),
        );
        assert_eq!(
            admit(&conversation, &tola, &genuine, "m.room.message").err(),
            Some(NOT_A_DELEGATED_ROOM)
        );
        let untyped = BriefRoom {
            room_type: None,
            ..room.clone()
        };
        assert_eq!(
            admit(&untyped, &tola, &genuine, "m.room.message").err(),
            Some(NOT_A_DELEGATED_ROOM)
        );
        // Addressed to someone else, naming another sender, or of a version
        // this build does not read.
        let mut elsewhere = delegation(&tola, &[TGORKA]);
        elsewhere.to = user("@nixi:example.org");
        assert_eq!(
            admit(&room, &tola, &brief_content(&elsewhere), "m.room.message").err(),
            Some(NOT_ADDRESSED)
        );
        let as_lena = brief_content(&delegation(&user("@lena:example.org"), &[TGORKA]));
        assert_eq!(
            admit(&room, &tola, &as_lena, "m.room.message").err(),
            Some(NOT_THE_CREATOR)
        );
        let mut later = genuine.clone();
        later[DELEGATE_EVENT]["v"] = serde_json::json!(99);
        assert_eq!(
            admit(&room, &tola, &later, "m.room.message").err(),
            Some(NOT_A_BRIEF)
        );
        // A label narrower than the audience, or a person in the room it
        // does not reach.
        let narrow = brief_content(&delegation(&tola, &[]));
        assert_eq!(
            admit(&room, &tola, &narrow, "m.room.message").err(),
            Some(BEYOND_THE_LABEL)
        );
        let mut watched = room.clone();
        watched.members.insert(user(MARTA));
        assert_eq!(
            admit(&watched, &tola, &genuine, "m.room.message").err(),
            Some(BEYOND_THE_LABEL)
        );

        // A pinned person's proxy is an agent here, though no drive this
        // host mounts homes it.
        let mira = user("@mira:example.org");
        let by_mira = room_by(&mira, delegated(&mira));
        let from_mira = brief_content(&delegation(&mira, &[TGORKA]));
        assert!(admit(&by_mira, &mira, &from_mira, "m.room.message").is_ok());
    }

    const DELEGATE_EVENT: &str = keeper_core::agents::events::DELEGATE;
}
