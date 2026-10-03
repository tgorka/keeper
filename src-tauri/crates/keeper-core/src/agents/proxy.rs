//! The person's proxy beside the notes view (AD-380, AD-382; story 91.2).
//!
//! Pure decisions the dock and the proxy's host share: which of a person's
//! rooms are their proxy's conversations ([`admits`], [`proxy_rooms`]),
//! which drives a scope may hold ([`ScopeRequest::check`]), what the dock
//! may send there ([`AgentOutbound`]) and which session a request for a new
//! conversation names ([`conversation_session_id`]). When the docked note's
//! focus goes out is [`super::focus`]'s.
//!
//! A room is a proxy's conversation when its status says `main` or
//! `conversation` (R25, Q3): the phone has no agents zone, so the room's own
//! status is what it reads. A status alone is anyone's word who holds power
//! in the room, so the room must also be one the agent made — its only
//! creator — and encrypted, and a `main` room the person's DM with it.
//! Where the zone is on the device (the Mac), the proxy's `agent.toml` also
//! says whose proxy it is and which drives its `[tools].drives` allows
//! ([`AgentProxies`]).

use std::collections::BTreeMap;
use std::sync::RwLock;

use matrix_sdk::ruma::{EventId, OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;
use ulid::Ulid;

use crate::agents::events::{
    ConversationRequestContent, ScopeContent, CONVERSATION_REQUEST, SCOPE,
};
use crate::agents::room::ScopeDriveVm;
use crate::agents::session::SessionKind;

/// The title of a conversation the person asked for without naming one.
pub const NEW_CONVERSATION_TITLE: &str = "conversation";

/// The drives a person asked to have in scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeRequest(pub Vec<String>);

/// A scope that names a drive the agent may not use.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{} not among the drives this agent may use.", named(.outside))]
pub struct ScopeRefusal {
    /// Every drive asked for outside `[tools].drives`, in the asked order.
    pub outside: Vec<String>,
}

fn named(drives: &[String]) -> String {
    match drives {
        [one] => format!("{one} is"),
        many => format!("{} are", many.join(", ")),
    }
}

impl ScopeRequest {
    /// The scope this request gives an agent whose `[tools].drives` is
    /// `allowed` and whose home drive is `home`: the home always, first, then
    /// the drives asked for in `allowed`'s order. A drive outside `allowed`
    /// refuses the whole request, naming it.
    pub fn check(&self, allowed: &[String], home: &str) -> Result<Vec<String>, ScopeRefusal> {
        let mut outside: Vec<String> = Vec::new();
        for drive in &self.0 {
            if drive != home && !allowed.contains(drive) && !outside.contains(drive) {
                outside.push(drive.clone());
            }
        }
        if !outside.is_empty() {
            return Err(ScopeRefusal { outside });
        }
        let mut scope = vec![home.to_owned()];
        for drive in allowed {
            if drive != home && self.0.contains(drive) {
                scope.push(drive.clone());
            }
        }
        Ok(scope)
    }
}

/// What a device that keeps an agents zone knows of a proxy, from its
/// `agent.toml` and the drives' `_drive.toml`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyFacts {
    /// The proxy's person.
    pub human: OwnedUserId,
    /// `[tools].drives`, the home drive first, each with its title where its
    /// `_drive.toml` is on this device.
    pub allowed: Vec<ScopeDriveVm>,
}

/// The proxies whose zones are on this device, by agent user: replaced on
/// each scan of the desktop's agents host; empty on the phone.
#[derive(Debug, Default)]
pub struct AgentProxies {
    proxies: RwLock<BTreeMap<OwnedUserId, ProxyFacts>>,
}

impl AgentProxies {
    pub fn replace(&self, proxies: BTreeMap<OwnedUserId, ProxyFacts>) {
        *self
            .proxies
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = proxies;
    }

    pub fn snapshot(&self) -> BTreeMap<OwnedUserId, ProxyFacts> {
        self.proxies
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

/// One of the person's agent session rooms, as the account reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyRoomRow {
    pub room_id: String,
    /// The room's name, else its id.
    pub name: String,
    /// The kind its newest readable status says; `None` before one is read.
    pub kind: Option<SessionKind>,
    /// The agent its newest readable status names.
    pub agent: Option<OwnedUserId>,
    /// The title its newest readable status says.
    pub title: Option<String>,
    /// Larger is more recent.
    pub recency: u64,
    /// Who created the room (`m.room.create`'s sender and any additional
    /// creators); empty before the create event is known.
    pub creators: Vec<OwnedUserId>,
    /// Whether the room is encrypted, as the homeserver last said.
    pub encrypted: bool,
    /// Whom the person's `m.direct` names for this room.
    pub direct_to: Vec<OwnedUserId>,
}

/// Why a room is not one of the person's proxy conversations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("keeper has not read this room's status yet.")]
    Unread,
    #[error("This room is not one of your proxy's conversations.")]
    NotAConversation,
    #[error("This room is not encrypted, so keeper sends nothing about your notes there.")]
    Unencrypted,
    #[error("This room was not made by the agent its status names.")]
    NotMadeByItsAgent,
    #[error("This room is not your direct conversation with its agent.")]
    NotYourDm,
    #[error("This agent is someone else's proxy.")]
    SomeoneElses,
}

/// What [`admits`] read a proxy room as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admitted<'a> {
    pub kind: SessionKind,
    pub agent: &'a UserId,
    /// What this device's agents zone knows of the proxy, if it has it.
    pub facts: Option<&'a ProxyFacts>,
}

/// Whether `row` is one of `me`'s proxy conversations: its newest readable
/// status says `main` or `conversation` and names an agent; the room is
/// encrypted (R30: every session room is) and the agent is its only
/// creator (session rooms are made by their agent, at 100); a `main` room
/// is `me`'s DM with that agent; and where `proxies` knows the agent, its
/// `human` is `me`. A member who holds power in a room can say anything in
/// its status — none of the rest.
pub fn admits<'a>(
    row: &'a ProxyRoomRow,
    me: &UserId,
    proxies: &'a BTreeMap<OwnedUserId, ProxyFacts>,
) -> Result<Admitted<'a>, Refusal> {
    let (Some(kind), Some(agent)) = (row.kind, row.agent.as_deref()) else {
        return Err(Refusal::Unread);
    };
    if !matches!(kind, SessionKind::Main | SessionKind::Conversation) {
        return Err(Refusal::NotAConversation);
    }
    if !row.encrypted {
        return Err(Refusal::Unencrypted);
    }
    if row.creators.len() != 1 || row.creators[0] != agent {
        return Err(Refusal::NotMadeByItsAgent);
    }
    if kind == SessionKind::Main && !row.direct_to.iter().any(|user| user == agent) {
        return Err(Refusal::NotYourDm);
    }
    let facts = proxies.get(agent);
    if facts.is_some_and(|facts| facts.human != me) {
        return Err(Refusal::SomeoneElses);
    }
    Ok(Admitted { kind, agent, facts })
}

/// What the dock may send into a proxy room: a closed set, each with its
/// fixed event type. A scope (with or without drives, with the focus or
/// none) and a request for a conversation — never a status, a claim, a turn
/// or an approval decision, which are the agent's or the reader's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentOutbound {
    Scope(ScopeContent),
    ConversationRequest(ConversationRequestContent),
}

impl AgentOutbound {
    /// The event's type.
    pub fn event_type(&self) -> &'static str {
        match self {
            AgentOutbound::Scope(_) => SCOPE,
            AgentOutbound::ConversationRequest(_) => CONVERSATION_REQUEST,
        }
    }

    /// The event's content.
    pub fn content(&self) -> Result<serde_json::Value, serde_json::Error> {
        match self {
            AgentOutbound::Scope(content) => serde_json::to_value(content),
            AgentOutbound::ConversationRequest(content) => serde_json::to_value(content),
        }
    }
}

/// A proxy conversation the dock offers (UX-DR130).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProxyRoomVm {
    pub room_id: String,
    pub name: String,
    /// `main` (the DM, the dock's default) or `conversation`.
    pub kind: SessionKind,
    /// The proxy's Matrix user id.
    pub agent: String,
    /// The drives the scope chip may offer (`[tools].drives`, the home drive
    /// first); `None` where the proxy's agents zone is not on this device.
    pub allowed: Option<Vec<ScopeDriveVm>>,
}

/// The docked note as the notes view knows it: Rust names its drive, its
/// drive-relative path and the heading above `line` (AD-65: the webview
/// joins no paths).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentFocusReq {
    pub vault_id: String,
    pub note_id: String,
    /// The caret's 1-based line in the editor's buffer (the body, without
    /// the frontmatter).
    pub line: u32,
    /// The editor's buffer from its start through the caret's line, as it
    /// is now — saved or not — which Rust reads the heading from; `None`
    /// where no editor holds the note (Preview), which is read from disk.
    #[ts(optional)]
    pub text: Option<String>,
}

/// The rooms among `rows` that are `me`'s proxy conversations ([`admits`]),
/// the `main` DM first and then the most recent first. A room whose status
/// is not read yet is not listed — it is listed once its status arrives,
/// never guessed from the room's shape. A conversation is named by its
/// status's title, which travels encrypted; its room name is generic.
pub fn proxy_rooms(
    rows: &[ProxyRoomRow],
    me: &UserId,
    proxies: &BTreeMap<OwnedUserId, ProxyFacts>,
) -> Vec<ProxyRoomVm> {
    let mut listed: Vec<(bool, u64, ProxyRoomVm)> = rows
        .iter()
        .filter_map(|row| {
            let admitted = admits(row, me, proxies).ok()?;
            let name = match (admitted.kind, &row.title) {
                (SessionKind::Conversation, Some(title)) => title.clone(),
                _ => row.name.clone(),
            };
            Some((
                admitted.kind == SessionKind::Main,
                row.recency,
                ProxyRoomVm {
                    room_id: row.room_id.clone(),
                    name,
                    kind: admitted.kind,
                    agent: admitted.agent.to_string(),
                    allowed: admitted.facts.map(|facts| facts.allowed.clone()),
                },
            ))
        })
        .collect();
    listed.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.1.cmp(&a.1))
            .then_with(|| a.2.room_id.cmp(&b.2.room_id))
    });
    listed.into_iter().map(|(_, _, room)| room).collect()
}

/// The id of the conversation session a request makes: derived from the
/// proxy and the request's event id, so a request served twice — a restart,
/// another copy's replay — names one session (AD-368, R36).
pub fn conversation_session_id(drive: &str, agent: &str, request: &EventId) -> Ulid {
    let digest = Sha256::digest(
        format!("keeper.agents.session\n{drive}\n{agent}\nconversation\n{request}").as_bytes(),
    );
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Ulid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drives(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    /// A session room `nixi` made, encrypted, the DM when `kind` is main.
    fn row(id: &str, kind: Option<SessionKind>, agent: &OwnedUserId, recency: u64) -> ProxyRoomRow {
        ProxyRoomRow {
            room_id: id.to_owned(),
            name: "Nixi".to_owned(),
            kind,
            agent: Some(agent.clone()),
            title: Some(id.trim_start_matches('!').to_owned()),
            recency,
            creators: vec![agent.clone()],
            encrypted: true,
            direct_to: if kind == Some(SessionKind::Main) {
                vec![agent.clone()]
            } else {
                Vec::new()
            },
        }
    }

    #[test]
    fn drives_in_scope_are_the_persons_choice_within_the_agents_allow() {
        let allowed = drives(&["tgdrive", "neuradrive"]);
        assert_eq!(
            ScopeRequest(drives(&["tgdrive", "neuradrive"])).check(&allowed, "tgdrive"),
            Ok(drives(&["tgdrive", "neuradrive"]))
        );
        // Without the home drive, the home is kept, and first.
        assert_eq!(
            ScopeRequest(drives(&["neuradrive"])).check(&allowed, "tgdrive"),
            Ok(drives(&["tgdrive", "neuradrive"]))
        );
        assert_eq!(
            ScopeRequest(Vec::new()).check(&allowed, "tgdrive"),
            Ok(drives(&["tgdrive"]))
        );
        // A drive outside the allow refuses the request, naming it.
        let refused = ScopeRequest(drives(&["tgdrive", "marta-drive"]))
            .check(&allowed, "tgdrive")
            .expect_err("refused");
        assert_eq!(refused.outside, drives(&["marta-drive"]));
        assert_eq!(
            refused.to_string(),
            "marta-drive is not among the drives this agent may use."
        );
        let two = ScopeRequest(drives(&["x", "neuradrive", "y", "x"]))
            .check(&allowed, "tgdrive")
            .expect_err("refused");
        assert_eq!(two.outside, drives(&["x", "y"]));
    }

    #[test]
    fn the_dm_is_the_docks_default() {
        let me = user("@tgorka:example.org");
        let nixi = user("@nixi:example.org");
        let rows = vec![
            row("!conv", Some(SessionKind::Conversation), &nixi, 30),
            row(
                "!tola",
                Some(SessionKind::Delegated),
                &user("@tola:example.org"),
                40,
            ),
            row("!dm", Some(SessionKind::Main), &nixi, 10),
            row("!unread", None, &nixi, 50),
            row("!older", Some(SessionKind::Conversation), &nixi, 20),
        ];
        // An ordinary DM with a person is not an agent session room, so it
        // never reaches the rows; an unread status is never guessed.
        let listed = proxy_rooms(&rows, &me, &BTreeMap::new());
        let ids: Vec<&str> = listed.iter().map(|room| room.room_id.as_str()).collect();
        assert_eq!(ids, ["!dm", "!conv", "!older"]);
        assert_eq!(listed[0].kind, SessionKind::Main);
        assert_eq!(listed[0].allowed, None);
        // The DM is named by its room; a conversation by its status's
        // title, which travels encrypted (its room name is generic).
        let names: Vec<&str> = listed.iter().map(|room| room.name.as_str()).collect();
        assert_eq!(names, ["Nixi", "conv", "older"]);

        // Where the zone is on the device, the proxy must be this person's,
        // and its allowed drives come along.
        let allowed = vec![ScopeDriveVm {
            id: "tgdrive".to_owned(),
            title: "tgdrive".to_owned(),
        }];
        let mine = BTreeMap::from([(
            nixi.clone(),
            ProxyFacts {
                human: me.clone(),
                allowed: allowed.clone(),
            },
        )]);
        let listed = proxy_rooms(&rows, &me, &mine);
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].allowed.as_ref(), Some(&allowed));
        let martas = BTreeMap::from([(
            nixi,
            ProxyFacts {
                human: user("@marta:example.org"),
                allowed,
            },
        )]);
        assert!(proxy_rooms(&rows, &me, &martas).is_empty());
    }

    /// F1: Marta makes a room typed as a session, sends a status naming
    /// herself `main` and invites tgorka. Her word in the status is all
    /// she controls: the room is not one an agent made alone, not
    /// encrypted, not tgorka's DM — any one of those keeps it out.
    #[test]
    fn a_status_alone_does_not_make_a_room_my_proxys() {
        let me = user("@tgorka:example.org");
        let marta = user("@marta:example.org");
        let nixi = user("@nixi:example.org");
        let none = BTreeMap::new();
        let dm = row("!dm", Some(SessionKind::Main), &nixi, 10);
        assert_eq!(
            admits(&dm, &me, &none).map(|ok| ok.kind),
            Ok(SessionKind::Main)
        );

        let clear = ProxyRoomRow {
            encrypted: false,
            ..dm.clone()
        };
        assert_eq!(admits(&clear, &me, &none), Err(Refusal::Unencrypted));
        // Marta made it, and her status names Nixi as its agent.
        let made_by_marta = ProxyRoomRow {
            creators: vec![marta.clone()],
            ..dm.clone()
        };
        assert_eq!(
            admits(&made_by_marta, &me, &none),
            Err(Refusal::NotMadeByItsAgent)
        );
        // Marta made it with Nixi as an additional creator.
        let shared = ProxyRoomRow {
            creators: vec![marta.clone(), nixi.clone()],
            ..dm.clone()
        };
        assert_eq!(admits(&shared, &me, &none), Err(Refusal::NotMadeByItsAgent));
        // Marta's own room, her status naming herself `main`: hers alone,
        // encrypted even — but not tgorka's DM with her as an agent.
        let hers = ProxyRoomRow {
            agent: Some(marta.clone()),
            creators: vec![marta.clone()],
            ..dm.clone()
        };
        assert_eq!(admits(&hers, &me, &none), Err(Refusal::NotYourDm));
        // The same as a `conversation` is admitted only on the phone's
        // word that it is encrypted and hers alone; where the zone is on
        // the device, her agent is not tgorka's proxy.
        let conversation = ProxyRoomRow {
            kind: Some(SessionKind::Conversation),
            direct_to: Vec::new(),
            ..hers.clone()
        };
        assert_eq!(
            admits(
                &ProxyRoomRow {
                    encrypted: false,
                    ..conversation.clone()
                },
                &me,
                &none
            ),
            Err(Refusal::Unencrypted)
        );
        let zone = BTreeMap::from([(
            marta.clone(),
            ProxyFacts {
                human: marta,
                allowed: Vec::new(),
            },
        )]);
        assert_eq!(
            admits(&conversation, &me, &zone),
            Err(Refusal::SomeoneElses)
        );
        assert!(proxy_rooms(&[clear, made_by_marta, shared, hers], &me, &none).is_empty());
    }

    #[test]
    fn a_request_names_one_conversation_session() {
        let first = EventId::parse("$a:example.org").expect("id");
        let second = EventId::parse("$b:example.org").expect("id");
        let id = conversation_session_id("tgdrive", "nixi", &first);
        assert_eq!(id, conversation_session_id("tgdrive", "nixi", &first));
        assert_ne!(id, conversation_session_id("tgdrive", "nixi", &second));
        assert_ne!(id, conversation_session_id("tgdrive", "tola", &first));
    }
}
