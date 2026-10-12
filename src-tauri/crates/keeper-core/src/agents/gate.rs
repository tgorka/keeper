//! A gate: the door for one outside system (AD-415, story 99.1).
//!
//! Pure. One `[[gate]]` of a `kind = "gate"` home is one gate session and
//! one room, made once by `keeper-agentd agents gate` (99.1 #5): the
//! session's id is derived from (drive, agent, system), so every run and
//! every host names the same one; its `agent.toml` is written from the
//! people-written table — `delegates` included, the only agents a request
//! can ever reach (AD-416) — and never rewritten. The room gives the outside
//! system's account no power: it posts at 0 and writes no state, invites,
//! kicks, bans or redacts nobody (99.1 #8; codemap Q3).

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use matrix_sdk::ruma::events::room::power_levels::{RoomPowerLevels, RoomPowerLevelsEventContent};
use matrix_sdk::ruma::{OwnedUserId, RoomId, UserId};
use serde_json::{json, Value};
use ulid::Ulid;

use super::agentd::TrustEntry;
use super::events;
use super::home::{AgentConfig, GateConfig};
use super::label::{Label, Readers};
use super::seed::derived_session_id;
use super::session::{GateSession, SessionAgent, SessionKind};

/// The id of the gate session serving `system`, derived as
/// `main_session_id` is. A system is `[a-z0-9-]{1,32}` and so holds no
/// newline: the name never meets `main` or a steward's duty (R66).
pub fn gate_session_id(drive: &str, agent: &str, system: &str) -> Ulid {
    derived_session_id(drive, agent, &format!("gate\n{system}"))
}

/// The gate session `gate` of `config` opens in `room` (codemap Q2): asked
/// for by the home drive's pinned `owner`, who heads its chain, so what only
/// a chain's head may decide is theirs; its label is the gate's audience at
/// `untrusted` for life (AD-416); the home drive is its only drive.
pub fn gate_session(
    config: &AgentConfig,
    gate: &GateConfig,
    room: &RoomId,
    owner: &UserId,
    now: DateTime<Utc>,
) -> SessionAgent {
    SessionAgent {
        id: gate_session_id(&config.drive, &config.id, &gate.system),
        agent: config.id.clone(),
        drive: config.drive.clone(),
        kind: SessionKind::Gate,
        title: gate.system.clone(),
        requested_by: owner.to_owned(),
        parent: None,
        reply: None,
        room: room.to_owned(),
        drives: vec![config.drive.clone()],
        label: Label::gate_opening(&gate.audience, config.local_only),
        needs: None,
        pin: None,
        hop: 0,
        dispatch_chain: vec![owner.to_owned(), config.matrix_user.clone()],
        limits: None,
        workflow: None,
        checkpoints: None,
        outputs: Vec::new(),
        created_at: now,
        gate: Some(GateSession {
            system: gate.system.clone(),
            peer: gate.peer.clone(),
            delegates: gate
                .delegates
                .iter()
                .map(|id| format!("{}/{id}", config.drive))
                .collect(),
            encrypted: gate.encrypted,
        }),
    }
}

/// A gate room's name: the agent's, then the system it is the door for.
pub fn room_name(agent_name: &str, system: &str) -> String {
    format!("{agent_name} · {system}")
}

/// A gate room's power levels (codemap Q3 (a)): the gate 100, everyone
/// else — the peer and the people watching — at 0. Every session room's
/// rows hold: `m.room.encrypted` and a person's decisions at 0, state,
/// invite, kick, ban and redact at 50, which only the gate reaches. A clear
/// room (Q9) also opens `m.room.message` at 0, so the peer can post there.
pub fn room_levels(creator: &UserId, encrypted: bool) -> Value {
    let mut levels = events::power_levels(SessionKind::Gate, creator, &[]);
    if !encrypted {
        levels["events"]["m.room.message"] = json!(0);
    }
    levels
}

/// The people who decide a gate's tickets (codemap Q2 (a)): the home
/// drive's pinned readers whose master key is pinned in `[[trust]]`.
pub fn approvers(readers: &BTreeSet<OwnedUserId>, trust: &[TrustEntry]) -> Vec<OwnedUserId> {
    readers
        .iter()
        .filter(|reader| {
            trust
                .iter()
                .any(|entry| entry.user == **reader && entry.master_key.is_some())
        })
        .cloned()
        .collect()
}

/// Why a gate's room cannot be made as its `[[gate]]` and `agentd.toml`
/// say: the sentence `agents gate` prints.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InviteRefusal {
    /// The peer is a person or a proxy `[[trust]]` names: their text would
    /// be outside content where their decisions count.
    #[error("{peer} is named in agentd.toml's [[trust]], so it is a person or their proxy and never a gate's peer: give the {system} system its own account.")]
    PeerTrusted { peer: OwnedUserId, system: String },
    /// No reader of the drive has a pinned key (codemap Q2 (a)): nobody
    /// would decide the gate's tickets or read what the system writes.
    #[error("No reader of {drive} has a master key pinned in agentd.toml's [[trust]], so nobody would decide the {system} gate's tickets or see its room: pin a reader's key, then run this again.")]
    NoApprover { drive: String, system: String },
    /// An approver is outside a listed audience: every member of the room
    /// reads what is posted there.
    #[error("{approver} decides this gate's tickets (a reader of the drive with a [[trust]] key), so they watch its room, and the [[gate]] {system}'s audience does not list them: add them, or write [\"*\"].")]
    ApproverOutside {
        approver: OwnedUserId,
        system: String,
    },
}

/// Who a gate's room is made inviting: its peer, then its `approvers` as
/// observers. Refused when the peer is a person or a proxy `[[trust]]`
/// names, when there is no approver (`drive` is the home drive, named in
/// the sentence), or when an approver is outside the gate's audience.
pub fn room_invitees(
    gate: &GateConfig,
    drive: &str,
    approvers: &[OwnedUserId],
    trust: &[TrustEntry],
) -> Result<Vec<OwnedUserId>, InviteRefusal> {
    if trust
        .iter()
        .any(|entry| entry.user == gate.peer || entry.proxy.as_ref() == Some(&gate.peer))
    {
        return Err(InviteRefusal::PeerTrusted {
            peer: gate.peer.clone(),
            system: gate.system.clone(),
        });
    }
    if approvers.is_empty() {
        return Err(InviteRefusal::NoApprover {
            drive: drive.to_owned(),
            system: gate.system.clone(),
        });
    }
    if let Readers::Only(audience) = &gate.audience {
        if let Some(outside) = approvers.iter().find(|person| !audience.contains(*person)) {
            return Err(InviteRefusal::ApproverOutside {
                approver: outside.clone(),
                system: gate.system.clone(),
            });
        }
    }
    // The peer is never a reader of the drive (`[[gate]].peer`'s rule), so
    // never an approver too.
    let mut invite = vec![gate.peer.clone()];
    invite.extend_from_slice(approvers);
    Ok(invite)
}

/// Why a room that is a gate's room by its status or its name is not
/// adopted as it (R366 as amended): only a room this copy made, at the
/// levels [`room_levels`] makes for its encryption, encrypted as the
/// `[[gate]]` says, is the gate's — anything else would bind the gate
/// session to a room where the gate may not hold the room alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdoptRefusal {
    /// Its `m.room.create` is not this copy's alone.
    NotMine,
    /// Its power levels are not [`room_levels`]'s, or could not be read.
    OtherLevels,
    /// The room is `room_encrypted`, and the `[[gate]]` says otherwise.
    OtherEncryption { room_encrypted: bool },
}

impl AdoptRefusal {
    /// The sentence `agents gate` prints for `room`, found as `gate`'s.
    pub fn sentence(&self, room: &RoomId, me: &UserId, gate: &GateConfig) -> String {
        let system = &gate.system;
        match self {
            Self::NotMine => format!(
                "{room} says it is the {system} gate's room, but {me} did not make it, so it is not adopted: have {me} leave it, then run this again."
            ),
            Self::OtherLevels => format!(
                "{room} is the {system} gate's room, but its power levels are not the ones a gate room is made with, so it is not adopted: restore them, or have {me} leave it, then run this again."
            ),
            Self::OtherEncryption { room_encrypted } => format!(
                "{room} is the {system} gate's room and is {}, but the [[gate]] says encrypted = {}, so it is not adopted: write encrypted = {room_encrypted} back, or have {me} leave the room, then run this again.",
                if *room_encrypted { "encrypted" } else { "clear" },
                gate.encrypted
            ),
        }
    }
}

/// Whether a room the copy `me` is in, found as `gate`'s room, may be
/// adopted as it: made by `me` alone (`creators`, the room's
/// `m.room.create`), at [`room_levels`]`(me, room_encrypted)`, and
/// `room_encrypted` as `gate.encrypted` says.
pub fn adoption(
    me: &UserId,
    creators: &[OwnedUserId],
    levels: Option<&RoomPowerLevels>,
    room_encrypted: bool,
    gate: &GateConfig,
) -> Result<(), AdoptRefusal> {
    if !matches!(creators, [only] if only == me) {
        return Err(AdoptRefusal::NotMine);
    }
    if !levels.is_some_and(|levels| levels_are_gate_rooms(levels, me, room_encrypted)) {
        return Err(AdoptRefusal::OtherLevels);
    }
    if room_encrypted != gate.encrypted {
        return Err(AdoptRefusal::OtherEncryption { room_encrypted });
    }
    Ok(())
}

/// Whether `levels` are, row for row, the ones [`room_levels`] makes a
/// gate room with for `creator` and `encrypted`.
fn levels_are_gate_rooms(levels: &RoomPowerLevels, creator: &UserId, encrypted: bool) -> bool {
    serde_json::from_value::<RoomPowerLevelsEventContent>(room_levels(creator, encrypted))
        .is_ok_and(|made| {
            levels.users == made.users
                && levels.users_default == made.users_default
                && levels.events == made.events
                && levels.events_default == made.events_default
                && levels.state_default == made.state_default
                && levels.ban == made.ban
                && levels.kick == made.kick
                && levels.invite == made.invite
                && levels.redact == made.redact
        })
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use matrix_sdk::ruma::OwnedRoomId;

    use super::*;
    use crate::agents::drive::{self, DriveDecl};
    use crate::agents::home::parse_agent_toml;
    use crate::agents::label::Integrity;
    use crate::agents::seed::{main_session_id, steward_session_id};
    use crate::agents::session::{compose_session_agent_toml, parse_session_agent_toml};

    const OWNER: &str = "@tgorka:example.org";
    const MARTA: &str = "@marta:example.org";
    const PEER: &str = "@github:example.org";

    fn user(id: &str) -> OwnedUserId {
        UserId::parse(id).expect("user")
    }

    fn tgdrive() -> DriveDecl {
        drive::parse(&format!(
            "version = 1\nid = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"{OWNER}\"\nreaders = [\"{OWNER}\", \"{MARTA}\"]\n"
        ))
        .expect("tgdrive parses")
    }

    fn forge(audience: &str) -> AgentConfig {
        parse_agent_toml(
            &format!(
                "version = 1\nid = \"forge\"\nname = \"Forge\"\nkind = \"gate\"\n\
                 matrix_user = \"@forge:example.org\"\n\
                 [[gate]]\nsystem = \"github\"\npeer = \"{PEER}\"\naudience = {audience}\n\
                 delegates = [\"tola-grey\", \"amelia\"]\nmodes = [\"sync\", \"async\"]\n\
                 [model]\nbot = \"bot:ollama:http://localhost:11434#qwen3\"\n"
            ),
            "forge",
            &tgdrive(),
        )
        .expect("the gate parses")
    }

    fn room() -> OwnedRoomId {
        RoomId::parse("!gate:example.org").expect("room")
    }

    #[test]
    fn a_gate_session_id_is_one_per_system_and_meets_no_other() {
        let github = gate_session_id("tgdrive", "forge", "github");
        assert_eq!(github, gate_session_id("tgdrive", "forge", "github"));
        for other in [
            gate_session_id("tgdrive", "forge", "gitlab"),
            gate_session_id("tgdrive", "nixi", "github"),
            gate_session_id("neuradrive", "forge", "github"),
            main_session_id("tgdrive", "forge"),
            steward_session_id("tgdrive", "forge", "github"),
        ] {
            assert_ne!(github, other);
        }
    }

    /// Codemap Q2, 99.2 #1: the owner asks and heads the chain; the label
    /// is the gate's audience at `untrusted`, never the drive's readers; the
    /// delegates are written as `<drive>/<id>`; the file reads back whole.
    #[test]
    fn a_gate_session_opens_untrusted_for_its_audience() {
        let config = forge(&format!("[\"{PEER}\", \"{OWNER}\"]"));
        let now = Utc
            .with_ymd_and_hms(2026, 10, 12, 9, 0, 0)
            .single()
            .expect("time");
        let session = gate_session(&config, &config.gates[0], &room(), &user(OWNER), now);
        assert_eq!(session.id, gate_session_id("tgdrive", "forge", "github"));
        assert_eq!(session.kind, SessionKind::Gate);
        assert_eq!(session.title, "github");
        assert_eq!(session.requested_by, user(OWNER));
        assert_eq!(
            session.dispatch_chain,
            [user(OWNER), user("@forge:example.org")]
        );
        assert_eq!(
            session.label,
            Label {
                readers: Readers::Only([user(PEER), user(OWNER)].into_iter().collect()),
                integrity: Integrity::Untrusted,
                local_only: false,
            }
        );
        assert_eq!(session.drives, ["tgdrive"]);
        assert_eq!(
            session.gate,
            Some(GateSession {
                system: "github".to_owned(),
                peer: user(PEER),
                delegates: vec!["tgdrive/tola-grey".to_owned(), "tgdrive/amelia".to_owned()],
                encrypted: true,
            })
        );
        let read = parse_session_agent_toml(&compose_session_agent_toml(&session))
            .expect("the gate session's file reads");
        assert_eq!(read, session);

        let anyone = forge("[\"*\"]");
        let session = gate_session(&anyone, &anyone.gates[0], &room(), &user(OWNER), now);
        assert_eq!(session.label.readers, Readers::Anyone);
        assert_eq!(session.label.integrity, Integrity::Untrusted);
    }

    /// 99.1 #8, codemap Q3: the peer sits at 0 like a person — it can post
    /// (encrypted, or in clear where the room is clear) and reaches no state,
    /// invite, kick, ban or redaction, which only the gate's 100 does.
    #[test]
    fn a_gate_rooms_levels_give_the_peer_nothing() {
        let gate = user("@forge:example.org");
        for encrypted in [true, false] {
            let levels = room_levels(&gate, encrypted);
            assert_eq!(levels["users"], json!({ "@forge:example.org": 100 }));
            assert_eq!(levels["users_default"], 0);
            for power in [
                "state_default",
                "events_default",
                "invite",
                "kick",
                "ban",
                "redact",
            ] {
                assert_eq!(levels[power], 50, "{power}");
            }
            assert_eq!(levels["events"]["m.room.encrypted"], 0);
            assert_eq!(levels["events"][events::APPROVAL_DECISION], 0);
            assert!(levels["events"].get(events::CLAIM).is_none());
            assert!(levels["events"].get("m.room.power_levels").is_none());
            assert_eq!(
                levels["events"].get("m.room.message").is_some(),
                !encrypted,
                "the peer posts in clear only in a clear room"
            );
        }
        assert_eq!(room_levels(&gate, false)["events"]["m.room.message"], 0);
    }

    fn trusted(user_id: &str, key: Option<&str>, proxy: Option<&str>) -> TrustEntry {
        TrustEntry {
            user: user(user_id),
            master_key: key.map(str::to_owned),
            proxy: proxy.map(user),
        }
    }

    /// Codemap Q2: the approvers are the pinned readers with a pinned key;
    /// the room invites the peer, then them, and every one of them must be
    /// in a listed audience; a peer `[[trust]]` names is refused, and so is
    /// a gate nobody would decide for (R99R1-02).
    #[test]
    fn a_gate_room_invites_its_peer_and_its_approvers() {
        let readers: BTreeSet<OwnedUserId> = [user(OWNER), user(MARTA)].into_iter().collect();
        let trust = [
            trusted(OWNER, Some("ed25519:AAAA"), Some("@nixi:example.org")),
            trusted(MARTA, None, None),
            trusted("@stranger:example.org", Some("ed25519:BBBB"), None),
        ];
        let approvers = approvers(&readers, &trust);
        assert_eq!(
            approvers,
            [user(OWNER)],
            "a reader with a key, and only a reader"
        );

        let listed = forge(&format!("[\"{PEER}\", \"{OWNER}\"]"));
        assert_eq!(
            room_invitees(&listed.gates[0], "tgdrive", &approvers, &trust),
            Ok(vec![user(PEER), user(OWNER)])
        );
        let anyone = forge("[\"*\"]");
        assert_eq!(
            room_invitees(&anyone.gates[0], "tgdrive", &approvers, &trust),
            Ok(vec![user(PEER), user(OWNER)])
        );
        let without_owner = forge(&format!("[\"{PEER}\", \"{MARTA}\"]"));
        assert_eq!(
            room_invitees(&without_owner.gates[0], "tgdrive", &approvers, &trust),
            Err(InviteRefusal::ApproverOutside {
                approver: user(OWNER),
                system: "github".to_owned(),
            })
        );
        let unpinned = [trusted(OWNER, None, None), trusted(MARTA, None, None)];
        for audience in [anyone.clone(), listed.clone()] {
            assert_eq!(
                room_invitees(&audience.gates[0], "tgdrive", &[], &unpinned),
                Err(InviteRefusal::NoApprover {
                    drive: "tgdrive".to_owned(),
                    system: "github".to_owned(),
                }),
                "no reader with a pinned key"
            );
        }

        for (who, proxy) in [(PEER, None), ("@someone:example.org", Some(PEER))] {
            let trust = [
                trusted(OWNER, Some("ed25519:AAAA"), None),
                trusted(who, Some("ed25519:CCCC"), proxy),
            ];
            assert_eq!(
                room_invitees(&anyone.gates[0], "tgdrive", &approvers, &trust),
                Err(InviteRefusal::PeerTrusted {
                    peer: user(PEER),
                    system: "github".to_owned(),
                }),
                "{who}"
            );
        }
    }

    fn levels_of(content: Value) -> RoomPowerLevels {
        use matrix_sdk::ruma::room_version_rules::AuthorizationRules;
        let content: RoomPowerLevelsEventContent = serde_json::from_value(content).expect("levels");
        RoomPowerLevels::new(
            content.into(),
            &AuthorizationRules::V1,
            Vec::<OwnedUserId>::new(),
        )
    }

    /// R366 as amended (R99R1-01): a room found as a gate's, by its status
    /// or its name, is adopted only when this copy alone made it, at the
    /// levels a gate room is made with for its encryption, and encrypted as
    /// the `[[gate]]` says.
    #[test]
    fn a_gate_room_is_adopted_only_when_it_is_the_gates_own() {
        let me = user("@forge:example.org");
        let config = forge("[\"*\"]");
        let encrypted = &config.gates[0];
        assert!(encrypted.encrypted);
        let clear = GateConfig {
            encrypted: false,
            ..encrypted.clone()
        };
        let mine = [me.clone()];
        let made = levels_of(room_levels(&me, true));
        let made_clear = levels_of(room_levels(&me, false));
        assert_eq!(adoption(&me, &mine, Some(&made), true, encrypted), Ok(()));
        assert_eq!(
            adoption(&me, &mine, Some(&made_clear), false, &clear),
            Ok(())
        );

        for creators in [
            vec![user(PEER)],
            vec![user(OWNER), me.clone()],
            vec![me.clone(), user(PEER)],
            Vec::new(),
        ] {
            assert_eq!(
                adoption(&me, &creators, Some(&made), true, encrypted),
                Err(AdoptRefusal::NotMine),
                "{creators:?}"
            );
        }

        let mut raised = room_levels(&me, true);
        raised["users"][PEER] = json!(50);
        let mut opened = room_levels(&me, true);
        opened["invite"] = json!(0);
        for (what, levels) in [
            ("the peer raised", Some(levels_of(raised))),
            ("invites opened", Some(levels_of(opened))),
            ("a clear room's rows", Some(made_clear.clone())),
            ("levels unread", None),
        ] {
            assert_eq!(
                adoption(&me, &mine, levels.as_ref(), true, encrypted),
                Err(AdoptRefusal::OtherLevels),
                "{what}"
            );
        }

        assert_eq!(
            adoption(&me, &mine, Some(&made_clear), false, encrypted),
            Err(AdoptRefusal::OtherEncryption {
                room_encrypted: false
            })
        );
        assert_eq!(
            adoption(&me, &mine, Some(&made), true, &clear),
            Err(AdoptRefusal::OtherEncryption {
                room_encrypted: true
            })
        );
    }
}
