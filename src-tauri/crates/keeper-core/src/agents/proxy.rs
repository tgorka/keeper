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
//!
//! Whose proxy the agent is fails closed (ruling R72, [`KnownProxies`]): the
//! agent is the signed-in person's proxy only when this device's agents
//! zone says its `human` is them ([`AgentProxies`], the Mac), or the
//! person's own account data `dev.keeper.agent.proxies` lists it and no
//! zone here says otherwise (the phone). Only the person's keeper writes
//! that list, from its own zone ([`KnownProxies::mirrored`]). An agent
//! neither names is no one's proxy here: its rooms are not listed, spoken
//! to, asked for a surface or told where the person is.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Mutex, RwLock};

use matrix_sdk::ruma::events::macros::EventContent;
use matrix_sdk::ruma::{EventId, OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;
use ulid::Ulid;

use crate::agents::events::{
    ConversationRequestContent, ScopeContent, SurfaceResultContent, CONTENT_VERSION,
    CONVERSATION_REQUEST, SCOPE, SURFACE_RESULT,
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
    /// Per person: the `dev.keeper.agent.proxies` list this device last
    /// wrote, until the account's sync brings it back.
    written: Mutex<HashMap<OwnedUserId, BTreeSet<OwnedUserId>>>,
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

    /// Whether `list` is owed to `me`'s account data: not the one this
    /// device last wrote there. Recorded as written when it is.
    pub fn owes(&self, me: &UserId, list: &BTreeSet<OwnedUserId>) -> bool {
        let mut written = self
            .written
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if written.get(me) == Some(list) {
            return false;
        }
        written.insert(me.to_owned(), list.clone());
        true
    }

    /// The write of `me`'s list did not land: the next reading owes it again.
    pub fn unwritten(&self, me: &UserId) {
        self.written
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(me);
    }
}

/// `dev.keeper.agent.proxies` (global account data, ruling R72): the agents
/// the person's own keeper found, in an agents zone on one of their
/// devices, to be their proxies — what a device without that zone (the
/// phone) admits a proxy by.
#[derive(Clone, Debug, Serialize, Deserialize, EventContent)]
#[ruma_event(type = "dev.keeper.agent.proxies", kind = GlobalAccountData)]
pub struct ProxyListEventContent {
    pub v: u32,
    pub agents: Vec<OwnedUserId>,
}

impl ProxyListEventContent {
    /// The content for `agents`.
    pub fn of(agents: &BTreeSet<OwnedUserId>) -> ProxyListEventContent {
        ProxyListEventContent {
            v: CONTENT_VERSION,
            agents: agents.iter().cloned().collect(),
        }
    }

    /// The agents it lists; `None` for a version this keeper does not read,
    /// which then admits nothing and is not written over.
    pub fn agents(&self) -> Option<BTreeSet<OwnedUserId>> {
        (self.v == CONTENT_VERSION).then(|| self.agents.iter().cloned().collect())
    }
}

/// What this account knows of whose proxy an agent is (R72): this device's
/// zone facts, and the person's own `dev.keeper.agent.proxies` list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownProxies {
    pub facts: BTreeMap<OwnedUserId, ProxyFacts>,
    pub listed: BTreeSet<OwnedUserId>,
}

impl KnownProxies {
    /// Whether `agent` is `me`'s proxy: the zone here says so (its facts
    /// come along), or the person's list names it and no zone here
    /// contradicts it. Anything else is refused — unknown is not yes.
    pub fn proxy_of(&self, agent: &UserId, me: &UserId) -> Result<Option<&ProxyFacts>, Refusal> {
        match self.facts.get(agent) {
            Some(facts) if facts.human == me => Ok(Some(facts)),
            Some(_) => Err(Refusal::SomeoneElses),
            None if self.listed.contains(agent) => Ok(None),
            None => Err(Refusal::NotKnownYours),
        }
    }

    /// The list `me`'s keeper keeps in their account data: what it says,
    /// with every agent this device's zone says is `me`'s added and every
    /// one it says is someone else's removed. Equal to [`Self::listed`]
    /// when nothing is owed — a device without a zone changes nothing.
    pub fn mirrored(&self, me: &UserId) -> BTreeSet<OwnedUserId> {
        let mut list = self.listed.clone();
        for (agent, facts) in &self.facts {
            if facts.human == me {
                list.insert(agent.clone());
            } else {
                list.remove(agent);
            }
        }
        list
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
    /// The drives the agent's own newest scope names (its host's echo);
    /// `None` before one is read.
    pub scope: Option<Vec<String>>,
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
    #[error("keeper has not been told this agent is your proxy.")]
    NotKnownYours,
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
/// is `me`'s DM with that agent; and `known` says the agent is `me`'s
/// proxy ([`KnownProxies::proxy_of`], R72). A member who holds power in a
/// room can say anything in its status — none of the rest.
pub fn admits<'a>(
    row: &'a ProxyRoomRow,
    me: &UserId,
    known: &'a KnownProxies,
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
    let facts = known.proxy_of(agent, me)?;
    Ok(Admitted { kind, agent, facts })
}

/// The proxy whose conversation a room is, and the drives it declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomProxy {
    pub agent: OwnedUserId,
    /// The drives this device knows the proxy to have; `None` where it
    /// knows none (a phone before the proxy's host echoed a scope), so a
    /// request's drive cannot be checked.
    pub drives: Option<Vec<String>>,
}

/// The proxy whose conversation `row` is, when [`admits`] admits it, with
/// the drives it declares: where this device keeps the proxy's agents zone,
/// its `[tools].drives`; elsewhere (the phone), the drives its own scope
/// names — the ones its host resolves a surface call within — and `None`
/// before its host echoed one. A surface request is acted on only from that
/// agent, and only over those drives where they are known.
pub fn room_proxy(row: &ProxyRoomRow, me: &UserId, known: &KnownProxies) -> Option<RoomProxy> {
    let admitted = admits(row, me, known).ok()?;
    let drives = match admitted.facts {
        Some(facts) => Some(facts.allowed.iter().map(|drive| drive.id.clone()).collect()),
        None => row.scope.clone(),
    };
    Some(RoomProxy {
        agent: admitted.agent.to_owned(),
        drives,
    })
}

/// The agents that are `me`'s own proxies by `known` ([`KnownProxies::proxy_of`]):
/// every one the zone here or the person's list names that is not someone
/// else's. Presence goes only into control rooms one of them made.
pub fn own_proxies(me: &UserId, known: &KnownProxies) -> BTreeSet<OwnedUserId> {
    known
        .facts
        .keys()
        .chain(&known.listed)
        .filter(|agent| known.proxy_of(agent, me).is_ok())
        .cloned()
        .collect()
}

/// Whether a control room made by `creators` is one of `me`'s own
/// proxies' (`own`, [`own_proxies`]): its only creator is one of them. A
/// control room of a principal the person merely belongs to is not.
pub fn is_own_control_room(creators: &[OwnedUserId], own: &BTreeSet<OwnedUserId>) -> bool {
    matches!(creators, [creator] if own.contains(creator))
}

/// What the dock may send into a proxy room: a closed set, each with its
/// fixed event type. A scope (with or without drives, with the focus or
/// none), a request for a conversation, and a surface result for a request
/// this device was handed (91.3) — never a status, a claim, a turn or an
/// approval decision, which are the agent's or the reader's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentOutbound {
    Scope(ScopeContent),
    ConversationRequest(ConversationRequestContent),
    SurfaceResult(SurfaceResultContent),
}

impl AgentOutbound {
    /// The event's type.
    pub fn event_type(&self) -> &'static str {
        match self {
            AgentOutbound::Scope(_) => SCOPE,
            AgentOutbound::ConversationRequest(_) => CONVERSATION_REQUEST,
            AgentOutbound::SurfaceResult(_) => SURFACE_RESULT,
        }
    }

    /// The event's content.
    pub fn content(&self) -> Result<serde_json::Value, serde_json::Error> {
        match self {
            AgentOutbound::Scope(content) => serde_json::to_value(content),
            AgentOutbound::ConversationRequest(content) => serde_json::to_value(content),
            AgentOutbound::SurfaceResult(content) => serde_json::to_value(content),
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
pub fn proxy_rooms(rows: &[ProxyRoomRow], me: &UserId, known: &KnownProxies) -> Vec<ProxyRoomVm> {
    let mut listed: Vec<(bool, u64, ProxyRoomVm)> = rows
        .iter()
        .filter_map(|row| {
            let admitted = admits(row, me, known).ok()?;
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
            scope: None,
        }
    }

    /// The phone: no zone, the person's own list naming `agents`.
    fn listing(agents: &[&OwnedUserId]) -> KnownProxies {
        KnownProxies {
            facts: BTreeMap::new(),
            listed: agents.iter().map(|agent| (*agent).clone()).collect(),
        }
    }

    /// The Mac: a zone naming each agent's human, and no list yet.
    fn zoned(humans: &[(&OwnedUserId, &OwnedUserId)]) -> KnownProxies {
        KnownProxies {
            facts: humans
                .iter()
                .map(|(agent, human)| {
                    (
                        (*agent).clone(),
                        ProxyFacts {
                            human: (*human).clone(),
                            allowed: Vec::new(),
                        },
                    )
                })
                .collect(),
            listed: BTreeSet::new(),
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
        let listed = proxy_rooms(&rows, &me, &listing(&[&nixi]));
        let ids: Vec<&str> = listed.iter().map(|room| room.room_id.as_str()).collect();
        assert_eq!(ids, ["!dm", "!conv", "!older"]);
        assert_eq!(listed[0].kind, SessionKind::Main);
        assert_eq!(listed[0].allowed, None);
        // The DM is named by its room; a conversation by its status's
        // title, which travels encrypted (its room name is generic).
        let names: Vec<&str> = listed.iter().map(|room| room.name.as_str()).collect();
        assert_eq!(names, ["Nixi", "conv", "older"]);

        // Where the zone is on the device, its allowed drives come along.
        let allowed = vec![ScopeDriveVm {
            id: "tgdrive".to_owned(),
            title: "tgdrive".to_owned(),
        }];
        let mut mine = zoned(&[(&nixi, &me)]);
        if let Some(facts) = mine.facts.get_mut(&nixi) {
            facts.allowed = allowed.clone();
        }
        let listed = proxy_rooms(&rows, &me, &mine);
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].allowed.as_ref(), Some(&allowed));
    }

    /// R72: whose proxy an agent is fails closed. Its rooms count as the
    /// person's only where this device's zone says its human is them, or
    /// their own list names it and no zone here says otherwise; an agent
    /// nobody vouched for — on a phone with no list, or a room Marta made
    /// for herself and calls a `conversation` — is nobody's proxy here.
    #[test]
    fn a_proxy_is_mine_only_when_my_zone_or_my_list_says_so() {
        let me = user("@tgorka:example.org");
        let marta = user("@marta:example.org");
        let nixi = user("@nixi:example.org");
        let dm = row("!dm", Some(SessionKind::Main), &nixi, 10);
        // Marta's own room: hers alone, encrypted, her status naming
        // herself a `conversation` agent.
        let hers = row("!hers", Some(SessionKind::Conversation), &marta, 9);
        let rows = [dm.clone(), hers.clone()];

        let nobody = KnownProxies::default();
        assert_eq!(admits(&dm, &me, &nobody), Err(Refusal::NotKnownYours));
        assert_eq!(admits(&hers, &me, &nobody), Err(Refusal::NotKnownYours));
        assert!(proxy_rooms(&rows, &me, &nobody).is_empty());
        assert_eq!(room_proxy(&dm, &me, &nobody), None);
        assert!(own_proxies(&me, &nobody).is_empty());

        // The phone, with the list the person's Mac wrote.
        let phone = listing(&[&nixi]);
        assert_eq!(
            admits(&dm, &me, &phone).map(|ok| (ok.agent.to_owned(), ok.facts)),
            Ok((nixi.clone(), None))
        );
        assert_eq!(admits(&hers, &me, &phone), Err(Refusal::NotKnownYours));
        assert_eq!(own_proxies(&me, &phone), BTreeSet::from([nixi.clone()]));

        // The Mac, whose zone says whose each is, list or none.
        let mac = zoned(&[(&nixi, &me)]);
        assert_eq!(
            admits(&dm, &me, &mac).map(|ok| ok.facts.is_some()),
            Ok(true)
        );
        // A zone here naming another human wins over the list.
        let contradicted = KnownProxies {
            listed: BTreeSet::from([nixi.clone(), marta.clone()]),
            ..zoned(&[(&nixi, &marta), (&marta, &marta)])
        };
        assert_eq!(admits(&dm, &me, &contradicted), Err(Refusal::SomeoneElses));
        assert_eq!(
            admits(&hers, &me, &contradicted),
            Err(Refusal::SomeoneElses)
        );
        assert!(proxy_rooms(&rows, &me, &contradicted).is_empty());
        assert!(own_proxies(&me, &contradicted).is_empty());
    }

    /// The person's keeper keeps their list from its own zone: it adds an
    /// agent its zone says is theirs and removes one it says is someone
    /// else's, keeps the rest, and owes no write when nothing changes — a
    /// phone, with no zone, never changes it. A list of a version it does
    /// not read admits nothing.
    #[test]
    fn my_keeper_keeps_my_list_from_its_zone() {
        let me = user("@tgorka:example.org");
        let marta = user("@marta:example.org");
        let (nixi, tola, lucyna) = (
            user("@nixi:example.org"),
            user("@tola:example.org"),
            user("@lucyna:example.org"),
        );
        let mac = KnownProxies {
            listed: BTreeSet::from([nixi.clone(), lucyna.clone()]),
            ..zoned(&[(&tola, &me), (&lucyna, &marta)])
        };
        let list = mac.mirrored(&me);
        assert_eq!(list, BTreeSet::from([nixi.clone(), tola.clone()]));
        let phone = listing(&[&nixi, &tola]);
        assert_eq!(phone.mirrored(&me), phone.listed);

        let proxies = AgentProxies::default();
        assert!(proxies.owes(&me, &list));
        assert!(!proxies.owes(&me, &list), "written once");
        proxies.unwritten(&me);
        assert!(
            proxies.owes(&me, &list),
            "a write that failed is owed again"
        );
        assert!(proxies.owes(&marta, &list), "per person");

        let content = ProxyListEventContent::of(&list);
        assert_eq!(content.agents(), Some(list));
        let later = ProxyListEventContent {
            v: 2,
            agents: vec![nixi],
        };
        assert_eq!(later.agents(), None);
    }

    /// F1: Marta makes a room typed as a session, sends a status naming
    /// herself `main` and invites tgorka. Her word in the status is all
    /// she controls: the room is not one an agent made alone, not
    /// encrypted, not tgorka's DM — any one of those keeps it out, even
    /// for an agent tgorka's list names.
    #[test]
    fn a_status_alone_does_not_make_a_room_my_proxys() {
        let me = user("@tgorka:example.org");
        let marta = user("@marta:example.org");
        let nixi = user("@nixi:example.org");
        let known = listing(&[&nixi, &marta]);
        let dm = row("!dm", Some(SessionKind::Main), &nixi, 10);
        assert_eq!(
            admits(&dm, &me, &known).map(|ok| ok.kind),
            Ok(SessionKind::Main)
        );

        let clear = ProxyRoomRow {
            encrypted: false,
            ..dm.clone()
        };
        assert_eq!(admits(&clear, &me, &known), Err(Refusal::Unencrypted));
        // Marta made it, and her status names Nixi as its agent.
        let made_by_marta = ProxyRoomRow {
            creators: vec![marta.clone()],
            ..dm.clone()
        };
        assert_eq!(
            admits(&made_by_marta, &me, &known),
            Err(Refusal::NotMadeByItsAgent)
        );
        // Marta made it with Nixi as an additional creator.
        let shared = ProxyRoomRow {
            creators: vec![marta.clone(), nixi.clone()],
            ..dm.clone()
        };
        assert_eq!(
            admits(&shared, &me, &known),
            Err(Refusal::NotMadeByItsAgent)
        );
        // Marta's own room, her status naming herself `main`: hers alone,
        // encrypted even — but not tgorka's DM with her as an agent.
        let hers = ProxyRoomRow {
            agent: Some(marta.clone()),
            creators: vec![marta],
            ..dm.clone()
        };
        assert_eq!(admits(&hers, &me, &known), Err(Refusal::NotYourDm));
        assert!(proxy_rooms(&[clear, made_by_marta, shared, hers], &me, &known).is_empty());
    }

    /// A surface request names a drive: the device acts only over the
    /// drives the room's own proxy declares — its zone's `[tools].drives`
    /// where the zone is here, else its own scope's echo — never over every
    /// folder this device happens to sync. Before the host echoed a scope
    /// the phone knows none, and says so rather than guessing.
    #[test]
    fn a_proxy_declares_its_zones_drives_else_its_own_scope() {
        let me = user("@tgorka:example.org");
        let marta = user("@marta:example.org");
        let nixi = user("@nixi:example.org");
        let fresh = row("!dm", Some(SessionKind::Main), &nixi, 10);
        let dm = ProxyRoomRow {
            scope: Some(drives(&["tgdrive"])),
            ..fresh.clone()
        };

        // The phone: no zone, the scope Nixi's host echoed — or none yet.
        let phone = listing(&[&nixi]);
        assert_eq!(
            room_proxy(&dm, &me, &phone),
            Some(RoomProxy {
                agent: nixi.clone(),
                drives: Some(drives(&["tgdrive"])),
            })
        );
        assert_eq!(
            room_proxy(&fresh, &me, &phone),
            Some(RoomProxy {
                agent: nixi.clone(),
                drives: None,
            })
        );
        // The Mac: the zone's `[tools].drives`, whatever the scope says.
        let zone = |human: &OwnedUserId| {
            let mut known = zoned(&[(&nixi, human)]);
            if let Some(facts) = known.facts.get_mut(&nixi) {
                facts.allowed = vec![
                    ScopeDriveVm {
                        id: "tgdrive".to_owned(),
                        title: "tgdrive".to_owned(),
                    },
                    ScopeDriveVm {
                        id: "neuradrive".to_owned(),
                        title: "Neura".to_owned(),
                    },
                ];
            }
            known
        };
        assert_eq!(
            room_proxy(&dm, &me, &zone(&me)).map(|proxy| proxy.drives),
            Some(Some(drives(&["tgdrive", "neuradrive"])))
        );
        // Someone else's proxy, or a room `admits` refuses: no proxy at all.
        assert_eq!(room_proxy(&dm, &me, &zone(&marta)), None);
        let made_by_marta = ProxyRoomRow {
            creators: vec![marta],
            ..dm
        };
        assert_eq!(room_proxy(&made_by_marta, &me, &phone), None);
    }

    /// A presence says whether the person is at their keyboard: it goes into
    /// their own proxies' control rooms, never a shared principal's.
    #[test]
    fn presence_goes_only_into_my_own_proxys_control_room() {
        let me = user("@tgorka:example.org");
        let marta = user("@marta:example.org");
        let nixi = user("@nixi:example.org");
        let tola = user("@tola:example.org");
        let lucyna = user("@lucyna:example.org");
        // Nixi by the person's list; Tola by the zone; Lucyna is Marta's
        // proxy, whatever the list says.
        let known = KnownProxies {
            listed: BTreeSet::from([nixi.clone(), lucyna.clone()]),
            ..zoned(&[(&tola, &me), (&lucyna, &marta)])
        };
        let own = own_proxies(&me, &known);
        assert_eq!(own, BTreeSet::from([nixi.clone(), tola.clone()]));

        assert!(is_own_control_room(std::slice::from_ref(&nixi), &own));
        assert!(is_own_control_room(std::slice::from_ref(&tola), &own));
        assert!(!is_own_control_room(std::slice::from_ref(&lucyna), &own));
        assert!(!is_own_control_room(&[marta, nixi], &own));
        assert!(!is_own_control_room(&[], &own));
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
