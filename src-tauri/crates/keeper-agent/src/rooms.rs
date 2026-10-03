//! Who may bring an agent into a room, and whose words in a room become a
//! turn (F5, AD-380, ruling R30).
//!
//! An invited client sees only the room's stripped state, so a host cannot
//! read a brief or a first message before it joins: the decision to join is
//! made from the room's type and the inviter alone. Once joined, the host —
//! which decrypts — is what keeps a person's free text out of any session
//! that is not their proxy's conversation, because the homeserver sees every
//! encrypted event as `m.room.encrypted` and cannot tell a decision from text.

use keeper_core::agents::agentd::TrustEntry;
use keeper_core::agents::events::SESSION_ROOM_TYPE;
use keeper_core::agents::home::AgentKind;
use keeper_core::agents::label::{Label, Readers};
use keeper_core::agents::session::SessionKind;
use matrix_sdk::ruma::{OwnedUserId, UserId};

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
/// Only a session room (`dev.keeper.agent.session`) is ever joined, and only
/// when the inviter is:
/// - **(a)** the `human` of the invited proxy, hosted here — a new proxy
///   conversation;
/// - **(b)** the user of an agent homed in a drive this host mounts, when the
///   invited agent's opening label may reach that agent's home readers;
/// - **(c)** the `proxy` of a `[[trust]]` entry that is pinned (it has a
///   `master_key`) and whose person reads the invited agent's home drive.
pub fn invite_decision(invite: &Invite, known: &Known) -> InviteDecision {
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
    let proxy_person = invited.kind == AgentKind::Proxy
        && invited.human.as_deref() == Some(invite.inviter.as_ref());
    let mounted_agent = known.agents.iter().any(|agent| {
        agent.matrix_user == invite.inviter && invited.opening.may_reach(&agent.home_readers)
    });
    let pinned_proxy = known.trust.iter().any(|trust| {
        trust.master_key.is_some()
            && trust.proxy.as_deref() == Some(invite.inviter.as_ref())
            && reads(&invited.home_readers, &trust.user)
    });
    if proxy_person || mounted_agent || pinned_proxy {
        InviteDecision::Join
    } else {
        InviteDecision::Pending
    }
}

fn reads(readers: &Readers, user: &UserId) -> bool {
    match readers {
        Readers::Anyone => true,
        Readers::Only(set) => set.contains(user),
    }
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
    /// Any other `dev.keeper.agent.*` event: a status, a scope, a turn
    /// reference, a claim.
    AgentEvent,
}

/// What a host does with an arrival.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// The proxy's person spoke in its conversation: a turn.
    Turn,
    /// A reader decided, from a verified device: the approval path's.
    Decision,
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

/// Whether `sender`'s arrival becomes a turn, a decision, or nothing (AD-380,
/// R30). Text becomes a turn only in a proxy's `main` or `conversation`
/// session, from its `human`; everywhere else a person is an observer whose
/// text the host never feeds to the model.
///
/// Every session room lets a person send `m.room.encrypted` at power 0, so
/// the homeserver cannot stop a person sending an encrypted status, scope,
/// turn reference or an edit of the agent's anchor: the host, which
/// decrypts, checks the sender of each instead. Only the agent's own user
/// sends those, and a decision counts only from a reader's verified device.
pub fn classify(served: &Served<'_>, sender: &UserId, arrival: Arrival) -> Disposition {
    if sender == served.agent_user {
        return Disposition::Ignored(OWN_EVENT);
    }
    match arrival {
        Arrival::Edit | Arrival::AgentEvent => Disposition::Ignored(FORGED),
        Arrival::Decision { verified } => {
            if verified && reads(served.readers, sender) {
                Disposition::Decision
            } else {
                Disposition::Ignored(UNTRUSTED_DECISION)
            }
        }
        Arrival::Text => {
            let conversation = served.agent_kind == AgentKind::Proxy
                && matches!(
                    served.session_kind,
                    SessionKind::Main | SessionKind::Conversation
                );
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
            readers,
        }
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
    /// a scope, a turn reference or an edit of the anchor is checked by
    /// sender: from anyone but the agent it is ignored, even from its person
    /// in its own conversation.
    #[test]
    fn an_agent_event_or_an_edit_from_anyone_but_the_agent_is_ignored() {
        let tgorka = user(TGORKA);
        let nixi = user("@nixi:example.org");
        let room = readers(&[TGORKA]);
        for kind in [SessionKind::Main, SessionKind::Delegated] {
            let session = served(AgentKind::Proxy, Some(&tgorka), kind, &nixi, &room);
            for arrival in [Arrival::AgentEvent, Arrival::Edit] {
                assert_eq!(
                    classify(&session, &tgorka, arrival),
                    Disposition::Ignored(FORGED),
                    "{kind:?} {arrival:?}"
                );
                assert_eq!(
                    classify(&session, &user("@mallory:example.org"), arrival),
                    Disposition::Ignored(FORGED)
                );
                assert_eq!(
                    classify(&session, &nixi, arrival),
                    Disposition::Ignored(OWN_EVENT)
                );
            }
        }
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
}
