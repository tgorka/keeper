//! Delegation: an agent hands work to another agent, in a session the target
//! owns (AD-385, story 92.1).
//!
//! The delegating host checks the bounds ([`Limits::check`]) and the label
//! (`label::check_sink`), makes the room with [`room_invites`], and once the
//! target agent has joined sends one `m.room.message` whose body is the brief
//! and whose content carries a [`DelegateContent`] ([`brief_content`],
//! ruling R53). The target's host makes the child session from it:
//! [`child_session`] and its card, [`child_card`].

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ulid::Ulid;

use crate::agents::card;
use crate::agents::events::{CONTENT_VERSION, DELEGATE};
use crate::agents::home;
use crate::agents::label::{Integrity, Label, Readers};
use crate::agents::session::{
    Checkpoints, SessionAgent, SessionKind, SessionLimits, SessionParent, HOP_MAX,
};
use crate::notes::frontmatter::{FieldValue, Frontmatter};

/// The child session's card, at its root.
pub const CARD_FILE: &str = "brief.md";

/// A delegation's bounds: how deep it may go, how many rounds one exchange
/// has, and how many tokens the child may spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub hop_limit: u8,
    pub rounds_per_exchange: u32,
    pub tokens: u64,
}

/// Which bound a delegation reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundReached {
    /// A new session would be deeper than `limit` hops.
    Hop { limit: u8 },
    /// The exchange has had its `limit` rounds.
    Rounds { limit: u32 },
    /// The child spent `spent` of its `limit` tokens.
    Tokens { spent: u64, limit: u64 },
}

impl BoundReached {
    /// The bound's word, as a `delegate` or `run` line's reason says it.
    pub fn word(&self) -> &'static str {
        match self {
            BoundReached::Hop { .. } => "hop",
            BoundReached::Rounds { .. } => "rounds",
            BoundReached::Tokens { .. } => "tokens",
        }
    }

    /// What the model and the room are told.
    pub fn sentence(&self) -> String {
        match self {
            BoundReached::Hop { limit } => format!(
                "A delegation may go {limit} hops deep from a person's request, and this one would go deeper, so nothing was handed on."
            ),
            BoundReached::Rounds { limit } => format!(
                "This exchange has had its {limit} rounds, so nothing more was sent; wait for the reply."
            ),
            BoundReached::Tokens { spent, limit } => format!(
                "This delegation spent {spent} tokens of its {limit}-token budget, so it stopped."
            ),
        }
    }
}

impl Limits {
    /// A delegating agent's own bounds, from its home's `[limits]`.
    pub fn of(limits: &home::Limits) -> Limits {
        let deepest = HOP_MAX as u8;
        Limits {
            hop_limit: u8::try_from(limits.hop_limit).map_or(deepest, |hop| hop.min(deepest)),
            rounds_per_exchange: limits.rounds_per_exchange,
            tokens: limits.tokens_per_delegation,
        }
    }

    /// A child session's bounds, from its `agent.toml`'s `[limits]`.
    pub fn of_session(limits: &SessionLimits) -> Limits {
        Limits {
            hop_limit: HOP_MAX as u8,
            rounds_per_exchange: limits.rounds_per_exchange,
            tokens: limits.tokens,
        }
    }

    /// Whether a session at `hop` that has sent `rounds_in_exchange` rounds
    /// of its exchange and whose child spent `tokens_spent` may go on: a new
    /// delegation is one hop deeper, a new round is one more.
    pub fn check(
        &self,
        hop: u8,
        rounds_in_exchange: u32,
        tokens_spent: u64,
    ) -> Result<(), BoundReached> {
        if u32::from(hop) + 1 > u32::from(self.hop_limit) {
            return Err(BoundReached::Hop {
                limit: self.hop_limit,
            });
        }
        if rounds_in_exchange >= self.rounds_per_exchange {
            return Err(BoundReached::Rounds {
                limit: self.rounds_per_exchange,
            });
        }
        if tokens_spent >= self.tokens {
            return Err(BoundReached::Tokens {
                spent: tokens_spent,
                limit: self.tokens,
            });
        }
        Ok(())
    }
}

/// The delegating session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegateFrom {
    /// The delegating agent's user.
    pub agent: OwnedUserId,
    /// Its home drive.
    pub drive: String,
    /// Its session, zone-relative.
    pub session: String,
    /// Its session's room.
    pub room: OwnedRoomId,
}

/// The child's bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegateLimits {
    pub rounds_per_exchange: u32,
    pub tokens: u64,
}

/// The card the child session opens with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegateCard {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
}

/// `dev.keeper.agent.delegate`, inside the brief's encrypted content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegateContent {
    pub v: u32,
    /// A ULID: the child session's id too (AD-368).
    pub id: String,
    pub from: DelegateFrom,
    /// The target agent's user.
    pub to: OwnedUserId,
    pub brief: String,
    /// The child's drives in scope, its home first.
    pub drives: Vec<String>,
    /// The child's label at opening.
    pub label: Label,
    /// The child's hop.
    pub hop: u8,
    pub limits: DelegateLimits,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<DelegateCard>,
    /// Who the work came from (R76): the person who started it, then each
    /// agent that handed it on, the delegating agent last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dispatch_chain: Vec<OwnedUserId>,
}

/// A session's dispatch chain (R76): the one its `agent.toml` names, or —
/// for a session made before chains, or one a person started — the person
/// who asked, then `agent`, the session's own agent: `[human, proxy]` for a
/// proxy's `main` or `conversation`.
pub fn session_chain(session: &SessionAgent, agent: &UserId) -> Vec<OwnedUserId> {
    if !session.dispatch_chain.is_empty() {
        return session.dispatch_chain.clone();
    }
    let mut chain = vec![session.requested_by.clone()];
    if session.requested_by.as_str() != agent.as_str() {
        chain.push(agent.to_owned());
    }
    chain
}

/// The chain a delegation hands its child: the delegating session's, with
/// the delegating agent last (once, when it already ends the chain).
pub fn child_chain(parent: &[OwnedUserId], delegator: &UserId) -> Vec<OwnedUserId> {
    let mut chain = parent.to_vec();
    if chain.last().map(|last| last.as_str()) != Some(delegator.as_str()) {
        chain.push(delegator.to_owned());
    }
    chain
}

/// The label a delegation opens its child with: the delegating session's,
/// at `untrusted` integrity when the card it hands on carries
/// `integrity: untrusted` (Q17).
pub fn child_label(session: &Label, card_untrusted: bool) -> Label {
    let mut label = session.clone();
    if card_untrusted {
        label.integrity = Integrity::Untrusted;
    }
    label
}

/// The brief's `m.room.message` content: an ordinary text message, so a
/// device shows it as one, carrying the delegation.
pub fn brief_content(content: &DelegateContent) -> Value {
    json!({
        "msgtype": "m.text",
        "body": content.brief,
        DELEGATE: content,
    })
}

/// The delegation a message's content carries, when it carries one this
/// build reads.
pub fn read_brief(content: &Value) -> Option<DelegateContent> {
    let delegate = content.get(DELEGATE)?;
    let read: DelegateContent = serde_json::from_value(delegate.clone()).ok()?;
    (read.v == CONTENT_VERSION && Ulid::from_string(&read.id).is_ok()).then_some(read)
}

/// The delegation a message carries when it travels as a brief (R53, R114):
/// an `m.room.message` of `m.text`, no edit, whose body is the
/// delegation's brief. A notice, an emote, media, an edit, or a body that
/// says other than the delegation hands on carries none — what is shown is
/// what is handed on. The host and the person's devices read a brief's
/// envelope by this one rule.
pub fn enveloped_brief(event_type: &str, content: &Value) -> Option<DelegateContent> {
    if event_type != "m.room.message"
        || content["msgtype"] != "m.text"
        || content["m.relates_to"]["rel_type"] == "m.replace"
    {
        return None;
    }
    read_brief(content).filter(|brief| content["body"] == brief.brief.as_str())
}

/// `brief` when `sender` may hand it on in a room made by `creators`: the
/// delegating agent made the room, at 100, and names itself as `from`
/// (R53). Anyone else's is an ordinary message — the host and the person's
/// devices read a brief by this one rule.
pub fn trusted_brief(
    brief: DelegateContent,
    sender: &UserId,
    creators: &[OwnedUserId],
) -> Option<DelegateContent> {
    (creators.iter().any(|creator| creator == sender) && brief.from.agent == sender)
        .then_some(brief)
}

/// Who a delegation's room invites (AD-372): the target agent, then the
/// label's readers as observers at power 0. The requester made the room and
/// is in it already.
pub fn room_invites(target: &UserId, requester: &UserId, label: &Label) -> Vec<OwnedUserId> {
    let mut invites = vec![target.to_owned()];
    if let Readers::Only(readers) = &label.readers {
        invites.extend(
            readers
                .iter()
                .filter(|user| {
                    user.as_str() != target.as_str() && user.as_str() != requester.as_str()
                })
                .cloned(),
        );
    }
    invites
}

/// The observers among `invites`: everyone but the target.
pub fn observers(invites: &[OwnedUserId], target: &UserId) -> BTreeSet<OwnedUserId> {
    invites
        .iter()
        .filter(|user| user.as_str() != target.as_str())
        .cloned()
        .collect()
}

/// A delegated session's title and its room's name: `<agent-id> <YYYY-MM-DD>`,
/// so no state anywhere carries what the work is about.
pub fn session_title(target: &str, at: DateTime<Utc>) -> String {
    format!("{target} {}", at.format("%Y-%m-%d"))
}

/// The child session's `agent.toml`, for the target agent `target` homed in
/// `drive`, in `room`. `None` when the content's id is not a ULID.
pub fn child_session(
    content: &DelegateContent,
    target: &str,
    drive: &str,
    room: &OwnedRoomId,
    created_at: DateTime<Utc>,
) -> Option<SessionAgent> {
    Some(SessionAgent {
        id: Ulid::from_string(&content.id).ok()?,
        agent: target.to_owned(),
        drive: drive.to_owned(),
        kind: SessionKind::Delegated,
        title: session_title(target, created_at),
        requested_by: content.from.agent.clone(),
        parent: Some(SessionParent {
            drive: content.from.drive.clone(),
            session: content.from.session.clone(),
            room: content.from.room.clone(),
        }),
        room: room.clone(),
        drives: content.drives.clone(),
        label: content.label.clone(),
        needs: None,
        pin: None,
        hop: content.hop,
        dispatch_chain: content.dispatch_chain.clone(),
        limits: Some(SessionLimits {
            rounds_per_exchange: content.limits.rounds_per_exchange,
            tokens: content.limits.tokens,
        }),
        workflow: None,
        checkpoints: None,
        outputs: Vec::new(),
        created_at,
    })
}

/// A workflow's run (R104): the session `child_session` makes of `content`
/// — whose `to` is the starting agent itself — of kind `workflow`, naming
/// its workflow and its checkpoints, fixed for the run (R103).
pub fn workflow_session(
    content: &DelegateContent,
    agent: &str,
    drive: &str,
    room: &OwnedRoomId,
    created_at: DateTime<Utc>,
    workflow: &str,
    checkpoints: Checkpoints,
) -> Option<SessionAgent> {
    let child = child_session(content, agent, drive, room, created_at)?;
    Some(SessionAgent {
        kind: SessionKind::Workflow,
        workflow: Some(workflow.to_owned()),
        checkpoints: Some(checkpoints),
        ..child
    })
}

/// The child session's card: a task for `target`, `run: queued`, the brief
/// as its body, stamped as the delegating agent's write (Q16, Q17).
pub fn child_card(content: &DelegateContent, target: &str) -> String {
    let title = content.card.as_ref().map_or_else(
        || content.brief.lines().next().unwrap_or_default().to_owned(),
        |card| card.title.clone(),
    );
    let text = |value: &str| FieldValue::Str(value.to_owned());
    let mut fields = vec![
        ("tags".to_owned(), FieldValue::List(vec![text("task")])),
        ("title".to_owned(), text(title.trim())),
        ("status".to_owned(), text("todo")),
        (card::ASSIGNEE.to_owned(), text(target)),
        (
            card::REQUESTED_BY.to_owned(),
            text(content.from.agent.as_str()),
        ),
    ];
    if let Some(delegated) = &content.card {
        if let Some(schedule) = &delegated.schedule {
            fields.push((card::SCHEDULE.to_owned(), text(schedule)));
        }
        if let Some(workflow) = &delegated.workflow {
            fields.push((card::WORKFLOW.to_owned(), text(workflow)));
        }
    }
    let written = format!(
        "{}\n{}\n",
        Frontmatter::serialise_new(&fields),
        content.brief.trim_end()
    );
    let stamped =
        card::stamp_agent_write(None, &written, &content.from.agent, content.label.integrity);
    // `run:` is the host's, never the agent's write: put in after the stamp.
    Frontmatter::set_after_in(
        &stamped,
        &[card::REQUESTED_BY],
        card::RUN,
        text(card::Run::Queued.as_str()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str) -> OwnedUserId {
        UserId::parse(id).expect("user")
    }

    fn room(id: &str) -> OwnedRoomId {
        OwnedRoomId::try_from(id).expect("room")
    }

    fn mine() -> Label {
        Label {
            readers: Readers::Only(BTreeSet::from([user("@tgorka:h")])),
            integrity: Integrity::Owner,
            local_only: false,
        }
    }

    fn content(label: Label, card: Option<DelegateCard>) -> DelegateContent {
        DelegateContent {
            v: CONTENT_VERSION,
            id: Ulid::new().to_string(),
            from: DelegateFrom {
                agent: user("@nixi:h"),
                drive: "tgdrive".to_owned(),
                session: "active/2026-10-04-chat".to_owned(),
                room: room("!parent:h"),
            },
            to: user("@tola-grey:h"),
            brief: "Tidy the inbox.\nKeep what matters.".to_owned(),
            drives: vec!["tgdrive".to_owned()],
            label,
            hop: 1,
            limits: DelegateLimits {
                rounds_per_exchange: 3,
                tokens: 200_000,
            },
            card,
            dispatch_chain: vec![user("@tgorka:h"), user("@nixi:h")],
        }
    }

    fn field(text: &str, key: &str) -> Option<String> {
        Frontmatter::parse(text).0.as_string(key).map(str::to_owned)
    }

    /// 92.1 acceptance 1, the hop bound: a session at hop 3 cannot hand on;
    /// with `hop_limit = 1` in the delegating agent's home, the second hop
    /// is refused. Rounds and tokens stop at their bound, not before.
    #[test]
    fn a_fourth_hop_is_refused() {
        let defaults = Limits {
            hop_limit: 3,
            rounds_per_exchange: 3,
            tokens: 1000,
        };
        assert_eq!(defaults.check(2, 0, 0), Ok(()));
        assert_eq!(defaults.check(3, 0, 0), Err(BoundReached::Hop { limit: 3 }));
        let shallow = Limits {
            hop_limit: 1,
            ..defaults
        };
        assert_eq!(shallow.check(0, 0, 0), Ok(()));
        assert_eq!(shallow.check(1, 0, 0), Err(BoundReached::Hop { limit: 1 }));

        // The fourth round of one exchange, and a spent budget.
        assert_eq!(defaults.check(0, 2, 0), Ok(()));
        assert_eq!(
            defaults.check(0, 3, 0),
            Err(BoundReached::Rounds { limit: 3 })
        );
        assert_eq!(defaults.check(0, 0, 999), Ok(()));
        assert_eq!(
            defaults.check(0, 0, 1000),
            Err(BoundReached::Tokens {
                spent: 1000,
                limit: 1000
            })
        );
    }

    /// The brief is a message a device shows, carrying exactly its
    /// delegation; a delegation of another version, or with no ULID, is not
    /// read.
    #[test]
    fn the_brief_carries_its_delegation() {
        let sent = content(mine(), None);
        let message = brief_content(&sent);
        assert_eq!(message["msgtype"], "m.text");
        assert_eq!(message["body"], sent.brief);
        assert_eq!(read_brief(&message), Some(sent.clone()));
        let mut old = message.clone();
        old[DELEGATE]["v"] = json!(2);
        assert_eq!(read_brief(&old), None);
        let mut unnamed = message;
        unnamed[DELEGATE]["id"] = json!("child");
        assert_eq!(read_brief(&unnamed), None);
    }

    /// R53: a brief is trusted only from the room's creator naming itself
    /// as `from` — the host's rule and the person's devices' alike.
    #[test]
    fn a_brief_is_trusted_only_from_the_room_creator_it_names() {
        let sent = content(mine(), None);
        let nixi = user("@nixi:h");
        let tola = user("@tola-grey:h");
        let trusted = |sender: &OwnedUserId, creators: &[OwnedUserId]| {
            trusted_brief(sent.clone(), sender, creators)
        };
        assert_eq!(
            trusted(&nixi, std::slice::from_ref(&nixi)),
            Some(sent.clone())
        );
        // Sent by someone who did not make the room.
        assert_eq!(trusted(&tola, std::slice::from_ref(&nixi)), None);
        assert_eq!(trusted(&nixi, &[]), None);
        // Made by Tola, who sends a brief in Nixi's name.
        assert_eq!(trusted(&tola, std::slice::from_ref(&tola)), None);
    }

    /// R114: a brief travels only as an original `m.text` message whose body
    /// is the brief it hands on — the envelope host and device both read.
    #[test]
    fn a_brief_travels_only_as_original_text_saying_what_it_hands_on() {
        let sent = content(mine(), None);
        let message = brief_content(&sent);
        assert_eq!(
            enveloped_brief("m.room.message", &message),
            Some(sent.clone())
        );
        for msgtype in ["m.notice", "m.emote", "m.image"] {
            let mut other = message.clone();
            other["msgtype"] = json!(msgtype);
            assert_eq!(enveloped_brief("m.room.message", &other), None, "{msgtype}");
        }
        assert_eq!(enveloped_brief("dev.keeper.agent.delegate", &message), None);
        let mut edit = message.clone();
        edit["m.relates_to"] = json!({"rel_type": "m.replace", "event_id": "$e:h"});
        assert_eq!(enveloped_brief("m.room.message", &edit), None);
        // A body that says other than the delegation hands on.
        let mut other_text = message.clone();
        other_text["body"] = json!("Delete the archive.");
        assert_eq!(enveloped_brief("m.room.message", &other_text), None);
        // A reply is still a brief: only an edit replaces what was handed on.
        let mut reply = message;
        reply["m.relates_to"] = json!({"m.in_reply_to": {"event_id": "$e:h"}});
        assert_eq!(enveloped_brief("m.room.message", &reply), Some(sent));
    }

    /// AD-372: the target and the label's readers, never the requester.
    #[test]
    fn the_room_invites_the_target_and_the_readers() {
        let label = Label {
            readers: Readers::Only(BTreeSet::from([user("@tgorka:h"), user("@nixi:h")])),
            ..mine()
        };
        let invites = room_invites(&user("@tola-grey:h"), &user("@nixi:h"), &label);
        assert_eq!(invites, vec![user("@tola-grey:h"), user("@tgorka:h")]);
        assert_eq!(
            observers(&invites, &user("@tola-grey:h")),
            BTreeSet::from([user("@tgorka:h")])
        );
    }

    /// 92.1 acceptance 13 (Q16, Q17): a card with a schedule is marked as
    /// the delegating agent's; a delegation from an `untrusted` session, or
    /// of a card carrying `integrity: untrusted` from an `owner` one, opens
    /// an `untrusted` child whose card carries the mark.
    #[test]
    fn the_child_card_carries_scheduled_by_and_integrity() {
        let scheduled = content(
            mine(),
            Some(DelegateCard {
                title: "Weekly digest".to_owned(),
                schedule: Some("@weekly".to_owned()),
                workflow: None,
            }),
        );
        let card = child_card(&scheduled, "tola-grey");
        assert_eq!(field(&card, card::SCHEDULED_BY).as_deref(), Some("@nixi:h"));
        assert_eq!(field(&card, card::SCHEDULE).as_deref(), Some("@weekly"));
        assert_eq!(field(&card, card::INTEGRITY), None);
        assert_eq!(field(&card, "title").as_deref(), Some("Weekly digest"));
        assert_eq!(field(&card, card::ASSIGNEE).as_deref(), Some("tola-grey"));
        assert_eq!(field(&card, card::REQUESTED_BY).as_deref(), Some("@nixi:h"));
        assert_eq!(field(&card, card::RUN).as_deref(), Some("queued"));
        assert!(card.ends_with("Keep what matters.\n"), "{card}");

        let plain = child_card(&content(mine(), None), "tola-grey");
        assert_eq!(field(&plain, card::SCHEDULED_BY), None);
        assert_eq!(field(&plain, "title").as_deref(), Some("Tidy the inbox."));

        let untrusted_session = Label {
            integrity: Integrity::Untrusted,
            ..mine()
        };
        let from_untrusted = content(child_label(&untrusted_session, false), None);
        let card_of_outside = content(child_label(&mine(), true), None);
        for delegated in [from_untrusted, card_of_outside] {
            let card = child_card(&delegated, "tola-grey");
            assert_eq!(field(&card, card::INTEGRITY).as_deref(), Some("untrusted"));
            let child = child_session(
                &delegated,
                "tola-grey",
                "tgdrive",
                &room("!child:h"),
                Utc::now(),
            )
            .expect("a child");
            assert_eq!(child.label.integrity, Integrity::Untrusted);
        }
        assert_eq!(child_label(&mine(), false), mine());
    }

    /// R76: a proxy's session starts at its person, `[human, proxy]`; each
    /// hand-off appends the delegating agent once; a chain a file names is
    /// the chain; the child session keeps the brief's.
    #[test]
    fn the_dispatch_chain_starts_at_the_person() {
        let sent = content(mine(), None);
        let mut main =
            child_session(&sent, "nixi", "tgdrive", &room("!dm:h"), Utc::now()).expect("a session");
        main.kind = SessionKind::Main;
        main.requested_by = user("@tgorka:h");
        main.dispatch_chain = Vec::new();
        let nixi = user("@nixi:h");
        let first = session_chain(&main, &nixi);
        assert_eq!(first, vec![user("@tgorka:h"), nixi.clone()]);
        assert_eq!(
            child_chain(&first, &nixi),
            first,
            "the proxy is not named twice"
        );
        let tola = user("@tola-grey:h");
        let second = child_chain(&first, &tola);
        assert_eq!(second, vec![user("@tgorka:h"), nixi.clone(), tola.clone()]);
        main.dispatch_chain = second.clone();
        assert_eq!(session_chain(&main, &tola), second);
        let child = child_session(&sent, "tola-grey", "tgdrive", &room("!c:h"), Utc::now())
            .expect("a child");
        assert_eq!(child.dispatch_chain, sent.dispatch_chain);
        assert_eq!(read_brief(&brief_content(&sent)), Some(sent));
    }

    /// The child session names its parent, its requester and its bounds,
    /// and its title names no work.
    #[test]
    fn the_child_session_is_the_targets() {
        let sent = content(mine(), None);
        let at = DateTime::parse_from_rfc3339("2026-10-04T09:00:00Z")
            .expect("time")
            .with_timezone(&Utc);
        let child =
            child_session(&sent, "tola-grey", "tgdrive", &room("!child:h"), at).expect("a child");
        assert_eq!(child.id.to_string(), sent.id);
        assert_eq!(child.kind, SessionKind::Delegated);
        assert_eq!(child.agent, "tola-grey");
        assert_eq!(child.title, "tola-grey 2026-10-04");
        assert_eq!(child.requested_by, user("@nixi:h"));
        let parent = child.parent.expect("a parent");
        assert_eq!(parent.session, "active/2026-10-04-chat");
        assert_eq!(parent.room, room("!parent:h"));
        assert_eq!(child.hop, 1);
        assert_eq!(
            child.limits,
            Some(SessionLimits {
                rounds_per_exchange: 3,
                tokens: 200_000
            })
        );
        assert_eq!(child.label, mine());
    }
}
